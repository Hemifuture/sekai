use std::cmp::Reverse;
use std::collections::BinaryHeap;

use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::world::natural::{
    FormationSedimentFields, SedimentBudgetReport, SedimentSourceKindField,
    SurfaceFormationValidationError, SurfaceWaterField, SurfaceWaterKind,
    CLIMATOLOGICAL_YEAR_SECONDS, CRUST_DENSITY_MAX_KG_M3, CRUST_DENSITY_MIN_KG_M3, ELEVATION_MAX_M,
    ELEVATION_MIN_M, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3, FORMATION_FLOODPLAIN_ACCOMMODATION_M,
    FORMATION_MARINE_CAPACITY_EXPOSURE_RANGE, FORMATION_SEDIMENT_CAPACITY_KG_M3,
    FORMATION_SEDIMENT_SLOPE_SCALE, FORMATION_SHELF_BREAK_DEPTH_M,
    SEDIMENT_BUDGET_RELATIVE_ERROR_MAX, SEDIMENT_PROVENANCE_SOURCE_COUNT,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SphericalSurfaceValidationError};
use crate::world::CellId;

const CANCELLATION_POLL_MASK: usize = 255;

/// Borrowed retained process fields consumed by one sediment-routing pass.
#[derive(Debug, Clone, Copy)]
pub struct SedimentInputs<'a> {
    pub elevation_m: &'a [f32],
    pub sea_level_m: f32,
    pub surface_water: &'a SurfaceWaterField,
    pub flow_receiver: &'a [Option<CellId>],
    pub drainage_surface_elevation_m: &'a [f32],
    pub lake_depth_m: &'a [f32],
    pub mean_annual_discharge_m3_s: &'a [f32],
    pub fluvial_erosion_m: &'a [f32],
    pub hillslope_removed_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub hillslope_deposited_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub coastal_removed_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub coastal_ocean_injection_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub marine_exposure: &'a [f32],
    pub substrate_density_kg_m3: &'a [f32],
    pub sediment_sources: &'a SedimentSourceKindField,
    pub previous_sediment_thickness_m: &'a [f32],
    pub previous_provenance_fraction: &'a [[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
}

#[derive(Debug, Clone, Copy, Default)]
struct SedimentPacket {
    routed: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    coastal: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
}

impl SedimentPacket {
    fn total(self) -> f64 {
        self.routed.iter().chain(&self.coastal).sum()
    }

    fn add(&mut self, other: Self) -> Result<(), SedimentGenerationError> {
        for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
            self.routed[source] = checked_sum(self.routed[source], other.routed[source])?;
            self.coastal[source] = checked_sum(self.coastal[source], other.coastal[source])?;
        }
        Ok(())
    }

    fn take_fraction(&mut self, fraction: f64) -> Self {
        if fraction >= 1.0 {
            return std::mem::take(self);
        }
        let mut taken = Self::default();
        for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
            taken.routed[source] = self.routed[source] * fraction;
            taken.coastal[source] = self.coastal[source] * fraction;
            self.routed[source] -= taken.routed[source];
            self.coastal[source] -= taken.coastal[source];
        }
        taken
    }
}

/// Retained fields, elevation components, and exact current-step mass ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct SedimentTransportStep {
    fields: FormationSedimentFields,
    budget_report: SedimentBudgetReport,
    routed_sediment_deposition_m: Vec<f32>,
    coastal_deposition_m: Vec<f32>,
    removed_mass_kg: Vec<f64>,
    deposited_mass_kg: Vec<f64>,
}

impl SedimentTransportStep {
    pub const fn fields(&self) -> &FormationSedimentFields {
        &self.fields
    }

    pub const fn budget_report(&self) -> &SedimentBudgetReport {
        &self.budget_report
    }

    pub fn routed_sediment_deposition_m(&self) -> &[f32] {
        &self.routed_sediment_deposition_m
    }

    pub fn coastal_deposition_m(&self) -> &[f32] {
        &self.coastal_deposition_m
    }

    pub fn removed_mass_kg(&self) -> &[f64] {
        &self.removed_mass_kg
    }

    pub fn deposited_mass_kg(&self) -> &[f64] {
        &self.deposited_mass_kg
    }
}

/// One stable upstream-to-downstream, capacity-limited provenance pass.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProvenanceSedimentRouter;

impl ProvenanceSedimentRouter {
    pub fn route(
        surface: &SphericalSurfaceSnapshot,
        inputs: SedimentInputs<'_>,
        step_years: f64,
        cancellation: &BuildCancellation,
    ) -> Result<SedimentTransportStep, SedimentGenerationError> {
        check_cancelled(cancellation)?;
        surface
            .validate_cancellable(&|| cancellation.is_cancelled())
            .map_err(|error| map_surface_error(error, cancellation))?;
        validate_inputs(surface, inputs, step_years, cancellation)?;
        validate_paired_source_ledgers(inputs, cancellation)?;
        let order = upstream_to_downstream_order(surface, inputs.flow_receiver, cancellation)?;
        let count = surface.cells().len();
        let mut packets = vec![SedimentPacket::default(); count];
        let mut deposited_packets = vec![SedimentPacket::default(); count];
        let mut land_lake_by_source = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
        let mut shelf_by_source = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
        let mut deep_by_source = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
        let mut produced_by_source = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
        let mut removed_mass_kg = vec![0.0_f64; count];
        let mut deposited_mass_kg = vec![0.0_f64; count];
        let mut throughput_kg = vec![0.0_f64; count];
        let mut shelf_delivery_kg = vec![0.0_f64; count];
        let mut deep_ocean_delivery_kg = vec![0.0_f64; count];
        let mut endorheic_storage_kg = vec![0.0_f64; count];
        let mut delta_potential = vec![0.0_f32; count];

        for index in 0..count {
            poll_cancelled(cancellation, index)?;
            packets[index].coastal = inputs.coastal_ocean_injection_by_source_kg[index];
            deposited_packets[index].routed = inputs.hillslope_deposited_by_source_kg[index];
            let source = inputs
                .sediment_sources
                .get(index)
                .expect("validated source field covers every cell")
                .raw() as usize;
            let fluvial_mass = f64::from(inputs.fluvial_erosion_m[index])
                * surface.cells()[index].area.get()
                * f64::from(inputs.substrate_density_kg_m3[index]);
            packets[index].routed[source] =
                checked_sum(packets[index].routed[source], fluvial_mass)?;
            for source_index in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
                produced_by_source[source_index] = checked_sum(
                    produced_by_source[source_index],
                    fluvial_mass * f64::from((source_index == source) as u8)
                        + inputs.hillslope_removed_by_source_kg[index][source_index]
                        + inputs.coastal_removed_by_source_kg[index][source_index],
                )?;
                let direct_deposit = inputs.hillslope_deposited_by_source_kg[index][source_index];
                land_lake_by_source[source_index] =
                    checked_sum(land_lake_by_source[source_index], direct_deposit)?;
                deposited_mass_kg[index] = checked_sum(deposited_mass_kg[index], direct_deposit)?;
                removed_mass_kg[index] = checked_sum(
                    removed_mass_kg[index],
                    fluvial_mass * f64::from((source_index == source) as u8)
                        + inputs.hillslope_removed_by_source_kg[index][source_index]
                        + inputs.coastal_removed_by_source_kg[index][source_index],
                )?;
            }
        }

        let step_seconds = step_years * CLIMATOLOGICAL_YEAR_SECONDS;
        for (position, cell) in order.iter().copied().enumerate() {
            poll_cancelled(cancellation, position)?;
            let index = cell.raw() as usize;
            let available = packets[index].total();
            throughput_kg[index] = available;
            if available == 0.0 {
                continue;
            }
            if inputs.surface_water.get(index) == Some(SurfaceWaterKind::Ocean) {
                let exposure = f64::from(inputs.marine_exposure[index]);
                let water_depth_m =
                    (f64::from(inputs.sea_level_m) - f64::from(inputs.elevation_m[index])).max(0.0);
                let accommodation_kg = water_depth_m.min(FORMATION_SHELF_BREAK_DEPTH_M)
                    * surface.cells()[index].area.get()
                    * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3;
                let shelf_mass = (available * (1.0 - exposure)).min(accommodation_kg);
                let shelf_packet = packets[index].take_fraction(shelf_mass / available);
                let deep_packet = std::mem::take(&mut packets[index]);
                record_deposit(
                    &mut deposited_packets[index],
                    shelf_packet,
                    &mut shelf_by_source,
                    &mut deposited_mass_kg[index],
                )?;
                for (source, bucket) in deep_by_source.iter_mut().enumerate() {
                    *bucket = checked_sum(*bucket, deep_packet.routed[source])?;
                    *bucket = checked_sum(*bucket, deep_packet.coastal[source])?;
                }
                shelf_delivery_kg[index] = shelf_packet.total();
                deep_ocean_delivery_kg[index] = deep_packet.total();
                let marine_capacity = FORMATION_SEDIMENT_CAPACITY_KG_M3
                    * f64::from(inputs.mean_annual_discharge_m3_s[index])
                    * step_seconds
                    * (1.0 + FORMATION_MARINE_CAPACITY_EXPOSURE_RANGE * exposure);
                let denominator = available + marine_capacity;
                delta_potential[index] = if denominator > 0.0 {
                    (available / denominator * (1.0 - exposure)) as f32
                } else {
                    0.0
                };
                continue;
            }

            let Some(receiver) = inputs.flow_receiver[index] else {
                let deposit = std::mem::take(&mut packets[index]);
                endorheic_storage_kg[index] = deposit.total();
                record_deposit(
                    &mut deposited_packets[index],
                    deposit,
                    &mut land_lake_by_source,
                    &mut deposited_mass_kg[index],
                )?;
                continue;
            };
            let receiver_index = receiver.raw() as usize;
            let length_m = receiver_length_m(surface, cell, receiver)?;
            let slope = ((f64::from(inputs.drainage_surface_elevation_m[index])
                - f64::from(inputs.drainage_surface_elevation_m[receiver_index]))
                / length_m)
                .max(0.0);
            let capacity = FORMATION_SEDIMENT_CAPACITY_KG_M3
                * f64::from(inputs.mean_annual_discharge_m3_s[index])
                * step_seconds
                * (slope / (slope + FORMATION_SEDIMENT_SLOPE_SCALE)).sqrt();
            let accommodation_depth_m = match inputs.surface_water.get(index) {
                Some(SurfaceWaterKind::Lake) => f64::from(inputs.lake_depth_m[index]),
                _ => (f64::from(inputs.drainage_surface_elevation_m[index])
                    - f64::from(inputs.drainage_surface_elevation_m[receiver_index]))
                .clamp(0.0, FORMATION_FLOODPLAIN_ACCOMMODATION_M),
            };
            let accommodation_kg = accommodation_depth_m
                * surface.cells()[index].area.get()
                * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3;
            let deposit_mass = (available - capacity).max(0.0).min(accommodation_kg);
            let deposit = packets[index].take_fraction(deposit_mass / available);
            record_deposit(
                &mut deposited_packets[index],
                deposit,
                &mut land_lake_by_source,
                &mut deposited_mass_kg[index],
            )?;
            let outgoing = std::mem::take(&mut packets[index]);
            packets[receiver_index].add(outgoing)?;
        }

        let mut final_in_transit_by_source = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
        for (index, packet) in packets.iter().copied().enumerate() {
            poll_cancelled(cancellation, index)?;
            for (source, bucket) in final_in_transit_by_source.iter_mut().enumerate() {
                *bucket = checked_sum(*bucket, packet.routed[source] + packet.coastal[source])?;
            }
        }

        let mut sediment_thickness_m = Vec::with_capacity(count);
        let mut provenance_fraction = Vec::with_capacity(count);
        let mut routed_sediment_deposition_m = Vec::with_capacity(count);
        let mut coastal_deposition_m = Vec::with_capacity(count);
        for (index, deposited_packet) in deposited_packets.iter().enumerate() {
            poll_cancelled(cancellation, index)?;
            let area_m2 = surface.cells()[index].area.get();
            let previous_mass = f64::from(inputs.previous_sediment_thickness_m[index])
                * area_m2
                * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3;
            let mut combined = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
            for (source, combined_mass) in combined.iter_mut().enumerate() {
                *combined_mass = previous_mass
                    * f64::from(inputs.previous_provenance_fraction[index][source])
                    + deposited_packet.routed[source]
                    + deposited_packet.coastal[source];
            }
            let total_mass = combined.iter().sum::<f64>();
            let mut thickness =
                (total_mass / (area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3)) as f32;
            if total_mass > 0.0 && thickness == 0.0 {
                thickness = f32::from_bits(1);
            }
            sediment_thickness_m.push(thickness);
            provenance_fraction.push(provenance_fractions(combined, total_mass));
            routed_sediment_deposition_m.push(
                (deposited_packet.routed.iter().sum::<f64>()
                    / (area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3)) as f32,
            );
            coastal_deposition_m.push(
                (deposited_packet.coastal.iter().sum::<f64>()
                    / (area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3)) as f32,
            );
        }

        let fields = FormationSedimentFields::new(
            sediment_thickness_m,
            provenance_fraction,
            throughput_kg,
            shelf_delivery_kg,
            deep_ocean_delivery_kg,
            endorheic_storage_kg,
            delta_potential,
        )?;
        let accounted_by_source = std::array::from_fn(|source| {
            land_lake_by_source[source]
                + shelf_by_source[source]
                + deep_by_source[source]
                + final_in_transit_by_source[source]
        });
        let budget_report = SedimentBudgetReport::new(
            produced_by_source.iter().sum(),
            land_lake_by_source.iter().sum(),
            shelf_by_source.iter().sum(),
            deep_by_source.iter().sum(),
            final_in_transit_by_source.iter().sum(),
            produced_by_source,
            accounted_by_source,
        )?;
        check_cancelled(cancellation)?;
        Ok(SedimentTransportStep {
            fields,
            budget_report,
            routed_sediment_deposition_m,
            coastal_deposition_m,
            removed_mass_kg,
            deposited_mass_kg,
        })
    }
}

fn record_deposit(
    destination: &mut SedimentPacket,
    deposit: SedimentPacket,
    bucket_by_source: &mut [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    deposited_mass_kg: &mut f64,
) -> Result<(), SedimentGenerationError> {
    destination.add(deposit)?;
    for (source, bucket) in bucket_by_source.iter_mut().enumerate() {
        let source_mass = deposit.routed[source] + deposit.coastal[source];
        *bucket = checked_sum(*bucket, source_mass)?;
        *deposited_mass_kg = checked_sum(*deposited_mass_kg, source_mass)?;
    }
    Ok(())
}

fn provenance_fractions(
    mass: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    total: f64,
) -> [f32; SEDIMENT_PROVENANCE_SOURCE_COUNT] {
    if total == 0.0 {
        return [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT];
    }
    let mut fractions = [0.0_f32; SEDIMENT_PROVENANCE_SOURCE_COUNT];
    let mut retained_sum = 0.0_f64;
    for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT - 1 {
        fractions[source] = (mass[source] / total) as f32;
        retained_sum += f64::from(fractions[source]);
    }
    fractions[SEDIMENT_PROVENANCE_SOURCE_COUNT - 1] = (1.0 - retained_sum).clamp(0.0, 1.0) as f32;
    fractions
}

fn validate_paired_source_ledgers(
    inputs: SedimentInputs<'_>,
    cancellation: &BuildCancellation,
) -> Result<(), SedimentGenerationError> {
    for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
        let mut hillslope_removed = 0.0_f64;
        let mut hillslope_deposited = 0.0_f64;
        let mut coastal_removed = 0.0_f64;
        let mut coastal_injected = 0.0_f64;
        for index in 0..inputs.elevation_m.len() {
            poll_cancelled(cancellation, index)?;
            hillslope_removed = checked_sum(
                hillslope_removed,
                inputs.hillslope_removed_by_source_kg[index][source],
            )?;
            hillslope_deposited = checked_sum(
                hillslope_deposited,
                inputs.hillslope_deposited_by_source_kg[index][source],
            )?;
            coastal_removed = checked_sum(
                coastal_removed,
                inputs.coastal_removed_by_source_kg[index][source],
            )?;
            coastal_injected = checked_sum(
                coastal_injected,
                inputs.coastal_ocean_injection_by_source_kg[index][source],
            )?;
        }
        validate_source_pair("hillslope", source, hillslope_removed, hillslope_deposited)?;
        validate_source_pair("coastal", source, coastal_removed, coastal_injected)?;
    }
    Ok(())
}

fn validate_source_pair(
    process: &'static str,
    source: usize,
    removed: f64,
    accounted: f64,
) -> Result<(), SedimentGenerationError> {
    let relative_error = (removed - accounted).abs() / removed.abs().max(accounted.abs()).max(1.0);
    if relative_error > SEDIMENT_BUDGET_RELATIVE_ERROR_MAX {
        return Err(SedimentGenerationError::SourceLedgerMismatch {
            process,
            source_index: source,
            removed,
            accounted,
            relative_error,
        });
    }
    Ok(())
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    inputs: SedimentInputs<'_>,
    step_years: f64,
    cancellation: &BuildCancellation,
) -> Result<(), SedimentGenerationError> {
    if !step_years.is_finite() || step_years <= 0.0 {
        return Err(SedimentGenerationError::InvalidStepYears { found: step_years });
    }
    if !inputs.sea_level_m.is_finite()
        || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&inputs.sea_level_m)
    {
        return Err(SedimentGenerationError::InvalidSeaLevel {
            found: inputs.sea_level_m,
        });
    }
    let count = surface.cells().len();
    for (field, found) in [
        ("elevation_m", inputs.elevation_m.len()),
        ("surface_water", inputs.surface_water.len()),
        ("flow_receiver", inputs.flow_receiver.len()),
        (
            "drainage_surface_elevation_m",
            inputs.drainage_surface_elevation_m.len(),
        ),
        ("lake_depth_m", inputs.lake_depth_m.len()),
        (
            "mean_annual_discharge_m3_s",
            inputs.mean_annual_discharge_m3_s.len(),
        ),
        ("fluvial_erosion_m", inputs.fluvial_erosion_m.len()),
        (
            "hillslope_removed_by_source_kg",
            inputs.hillslope_removed_by_source_kg.len(),
        ),
        (
            "hillslope_deposited_by_source_kg",
            inputs.hillslope_deposited_by_source_kg.len(),
        ),
        (
            "coastal_removed_by_source_kg",
            inputs.coastal_removed_by_source_kg.len(),
        ),
        (
            "coastal_ocean_injection_by_source_kg",
            inputs.coastal_ocean_injection_by_source_kg.len(),
        ),
        ("marine_exposure", inputs.marine_exposure.len()),
        (
            "substrate_density_kg_m3",
            inputs.substrate_density_kg_m3.len(),
        ),
        ("sediment_sources", inputs.sediment_sources.len()),
        (
            "previous_sediment_thickness_m",
            inputs.previous_sediment_thickness_m.len(),
        ),
        (
            "previous_provenance_fraction",
            inputs.previous_provenance_fraction.len(),
        ),
    ] {
        if found != count {
            return Err(SedimentGenerationError::CellCountMismatch {
                field,
                expected: count,
                found,
            });
        }
    }
    for index in 0..count {
        poll_cancelled(cancellation, index)?;
        let cell = CellId::from_raw(index as u32);
        for (field, value, minimum, maximum) in [
            (
                "elevation_m",
                inputs.elevation_m[index],
                ELEVATION_MIN_M,
                ELEVATION_MAX_M,
            ),
            (
                "drainage_surface_elevation_m",
                inputs.drainage_surface_elevation_m[index],
                ELEVATION_MIN_M,
                ELEVATION_MAX_M,
            ),
            (
                "lake_depth_m",
                inputs.lake_depth_m[index],
                0.0,
                ELEVATION_MAX_M - ELEVATION_MIN_M,
            ),
            (
                "mean_annual_discharge_m3_s",
                inputs.mean_annual_discharge_m3_s[index],
                0.0,
                f32::MAX,
            ),
            (
                "fluvial_erosion_m",
                inputs.fluvial_erosion_m[index],
                0.0,
                ELEVATION_MAX_M - ELEVATION_MIN_M,
            ),
            ("marine_exposure", inputs.marine_exposure[index], 0.0, 1.0),
            (
                "substrate_density_kg_m3",
                inputs.substrate_density_kg_m3[index],
                CRUST_DENSITY_MIN_KG_M3,
                CRUST_DENSITY_MAX_KG_M3,
            ),
            (
                "previous_sediment_thickness_m",
                inputs.previous_sediment_thickness_m[index],
                0.0,
                ELEVATION_MAX_M - ELEVATION_MIN_M,
            ),
        ] {
            if !value.is_finite() || !(minimum..=maximum).contains(&value) {
                return Err(SedimentGenerationError::InvalidCellValue {
                    field,
                    cell,
                    found: f64::from(value),
                });
            }
        }
        let expected_fraction_sum = if inputs.previous_sediment_thickness_m[index] == 0.0 {
            0.0
        } else {
            1.0
        };
        let mut fraction_sum = 0.0_f64;
        for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
            let fraction = inputs.previous_provenance_fraction[index][source];
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err(SedimentGenerationError::InvalidCellValue {
                    field: "previous_provenance_fraction",
                    cell,
                    found: f64::from(fraction),
                });
            }
            fraction_sum += f64::from(fraction);
            for (field, value) in [
                (
                    "hillslope_removed_by_source_kg",
                    inputs.hillslope_removed_by_source_kg[index][source],
                ),
                (
                    "hillslope_deposited_by_source_kg",
                    inputs.hillslope_deposited_by_source_kg[index][source],
                ),
                (
                    "coastal_removed_by_source_kg",
                    inputs.coastal_removed_by_source_kg[index][source],
                ),
                (
                    "coastal_ocean_injection_by_source_kg",
                    inputs.coastal_ocean_injection_by_source_kg[index][source],
                ),
            ] {
                if !value.is_finite() || value < 0.0 {
                    return Err(SedimentGenerationError::InvalidCellValue {
                        field,
                        cell,
                        found: value,
                    });
                }
            }
        }
        if (fraction_sum - expected_fraction_sum).abs() > 1.0e-6 {
            return Err(SedimentGenerationError::InvalidProvenanceSum {
                cell,
                found: fraction_sum,
                expected: expected_fraction_sum,
            });
        }
        if let Some(receiver) = inputs.flow_receiver[index] {
            if receiver.raw() as usize >= count || receiver == cell {
                return Err(SedimentGenerationError::ReceiverNotAdjacent { cell, receiver });
            }
            receiver_length_m(surface, cell, receiver)?;
        }
    }
    Ok(())
}

fn upstream_to_downstream_order(
    surface: &SphericalSurfaceSnapshot,
    receivers: &[Option<CellId>],
    cancellation: &BuildCancellation,
) -> Result<Vec<CellId>, SedimentGenerationError> {
    let mut indegree = vec![0_usize; receivers.len()];
    for (index, receiver) in receivers.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        if let Some(receiver) = receiver {
            indegree[receiver.raw() as usize] += 1;
        }
    }
    let mut ready = BinaryHeap::new();
    for (index, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            ready.push(Reverse(index as u32));
        }
    }
    let mut order = Vec::with_capacity(receivers.len());
    while let Some(Reverse(raw)) = ready.pop() {
        poll_cancelled(cancellation, order.len())?;
        let cell = CellId::from_raw(raw);
        order.push(cell);
        if let Some(receiver) = receivers[raw as usize] {
            receiver_length_m(surface, cell, receiver)?;
            let degree = &mut indegree[receiver.raw() as usize];
            *degree -= 1;
            if *degree == 0 {
                ready.push(Reverse(receiver.raw()));
            }
        }
    }
    if order.len() != receivers.len() {
        return Err(SedimentGenerationError::ReceiverCycle);
    }
    Ok(order)
}

fn receiver_length_m(
    surface: &SphericalSurfaceSnapshot,
    cell: CellId,
    receiver: CellId,
) -> Result<f64, SedimentGenerationError> {
    surface
        .cell_edges(cell)
        .and_then(|edges| {
            edges.iter().find_map(|&edge| {
                (surface.opposite_cell(cell, edge) == Some(receiver))
                    .then(|| {
                        surface
                            .edge(edge)
                            .map(|record| record.center_distance.get())
                    })
                    .flatten()
            })
        })
        .ok_or(SedimentGenerationError::ReceiverNotAdjacent { cell, receiver })
}

fn checked_sum(left: f64, right: f64) -> Result<f64, SedimentGenerationError> {
    let result = left + right;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(SedimentGenerationError::NumericalOverflow)
    }
}

fn poll_cancelled(
    cancellation: &BuildCancellation,
    index: usize,
) -> Result<(), SedimentGenerationError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), SedimentGenerationError> {
    if cancellation.is_cancelled() {
        Err(SedimentGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_surface_error(
    error: SphericalSurfaceValidationError,
    cancellation: &BuildCancellation,
) -> SedimentGenerationError {
    if cancellation.is_cancelled() {
        SedimentGenerationError::Cancelled
    } else {
        SedimentGenerationError::InvalidSurface(error)
    }
}

#[derive(Debug, Error)]
pub enum SedimentGenerationError {
    #[error("sediment routing cancelled")]
    Cancelled,
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("sediment field {field} has length {found}; expected {expected}")]
    CellCountMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("sediment field {field} has invalid value {found} at {cell:?}")]
    InvalidCellValue {
        field: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error("sediment provenance at {cell:?} sums to {found}, expected {expected}")]
    InvalidProvenanceSum {
        cell: CellId,
        found: f64,
        expected: f64,
    },
    #[error("sediment sea level is invalid: {found}")]
    InvalidSeaLevel { found: f32 },
    #[error("sediment step duration must be finite and positive, got {found}")]
    InvalidStepYears { found: f64 },
    #[error("sediment receiver {receiver:?} is not adjacent to {cell:?}")]
    ReceiverNotAdjacent { cell: CellId, receiver: CellId },
    #[error("sediment receiver graph contains a cycle")]
    ReceiverCycle,
    #[error(
        "{process} source {source_index} removed {removed} kg but accounted {accounted} kg ({relative_error} relative)"
    )]
    SourceLedgerMismatch {
        process: &'static str,
        source_index: usize,
        removed: f64,
        accounted: f64,
        relative_error: f64,
    },
    #[error("sediment arithmetic overflowed a finite f64 ledger")]
    NumericalOverflow,
    #[error("invalid retained sediment product: {0}")]
    InvalidRetainedFields(#[from] SurfaceFormationValidationError),
}
