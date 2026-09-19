use std::cmp::Reverse;
use std::collections::BinaryHeap;

use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::world::natural::{
    FormationSedimentFields, LandOceanKind, SedimentBudgetReport, SurfaceFormationValidationError,
    CLIMATOLOGICAL_YEAR_SECONDS, ELEVATION_MAX_M, ELEVATION_MIN_M,
    FORMATION_AIRY_MANTLE_DENSITY_KG_M3, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    FORMATION_SEDIMENT_DEPOSITION_COEFFICIENT, FORMATION_SHELF_BREAK_DEPTH_M,
    SEDIMENT_BUDGET_RELATIVE_ERROR_MAX, SEDIMENT_PROVENANCE_SOURCE_COUNT,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SphericalSurfaceValidationError};
use crate::world::CellId;

use super::state::{FormationStateError, SedimentStockState};

const CANCELLATION_POLL_MASK: usize = 255;
const MILLIMETERS_PER_METER: f64 = 1_000.0;

pub(super) fn split_mass_by_weights(
    mass_kg: f64,
    weights: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
) -> [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT] {
    if mass_kg == 0.0 {
        return [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT];
    }
    let weight_sum = weights.iter().sum::<f64>();
    if mass_kg == weight_sum {
        return weights;
    }
    let remainder_source = weights
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|(index, _)| index)
        .expect("the fixed provenance inventory is non-empty");
    let mut result = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
    let mut assigned = 0.0_f64;
    for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
        if source == remainder_source {
            continue;
        }
        result[source] = mass_kg * weights[source] / weight_sum;
        assigned += result[source];
    }
    result[remainder_source] = mass_kg - assigned;
    result
}

/// Borrowed retained process fields consumed by one sediment-routing pass.
#[derive(Debug, Clone, Copy)]
pub struct SedimentInputs<'a> {
    pub elevation_m: &'a [f64],
    pub sea_level_m: f64,
    pub flow_receiver: &'a [Option<CellId>],
    pub mean_annual_discharge_m3_s: &'a [f32],
    /// Per-cell Davy-Lague effective settling velocity `d* v_s`, in meters per
    /// year. Build it with
    /// [`davy_lague_effective_settling_velocity_m_per_year`] so the runoff
    /// scaling stays in one place.
    pub effective_settling_velocity_m_per_year: &'a [f64],
    pub fluvial_removed_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub hillslope_removed_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub hillslope_deposited_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub coastal_removed_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub coastal_ocean_injection_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    pub marine_exposure: &'a [f64],
    pub retained_sediment_mass_by_source_kg: &'a [[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
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
    routed_sediment_deposition_m: Vec<f64>,
    coastal_deposition_m: Vec<f64>,
    removed_mass_kg: Vec<f64>,
    deposited_mass_kg: Vec<f64>,
    deposited_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
}

impl SedimentTransportStep {
    pub const fn fields(&self) -> &FormationSedimentFields {
        &self.fields
    }

    pub const fn budget_report(&self) -> &SedimentBudgetReport {
        &self.budget_report
    }

    pub fn routed_sediment_deposition_m(&self) -> &[f64] {
        &self.routed_sediment_deposition_m
    }

    pub fn coastal_deposition_m(&self) -> &[f64] {
        &self.coastal_deposition_m
    }

    pub fn removed_mass_kg(&self) -> &[f64] {
        &self.removed_mass_kg
    }

    pub fn deposited_mass_kg(&self) -> &[f64] {
        &self.deposited_mass_kg
    }

    pub fn deposited_by_source_kg(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.deposited_by_source_kg
    }
}

/// One stable upstream-to-downstream Davy-Lague provenance pass.
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
        Self::route_from_validated_surface(surface, inputs, step_years, cancellation)
    }

    /// Same conservative pass for a caller that already validated the surface.
    pub(super) fn route_from_validated_surface(
        surface: &SphericalSurfaceSnapshot,
        inputs: SedimentInputs<'_>,
        step_years: f64,
        cancellation: &BuildCancellation,
    ) -> Result<SedimentTransportStep, SedimentGenerationError> {
        check_cancelled(cancellation)?;
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
            for source_index in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
                let fluvial_mass = inputs.fluvial_removed_by_source_kg[index][source_index];
                packets[index].routed[source_index] =
                    checked_sum(packets[index].routed[source_index], fluvial_mass)?;
                produced_by_source[source_index] = checked_sum(
                    produced_by_source[source_index],
                    fluvial_mass
                        + inputs.hillslope_removed_by_source_kg[index][source_index]
                        + inputs.coastal_removed_by_source_kg[index][source_index],
                )?;
                let direct_deposit = inputs.hillslope_deposited_by_source_kg[index][source_index];
                land_lake_by_source[source_index] =
                    checked_sum(land_lake_by_source[source_index], direct_deposit)?;
                deposited_mass_kg[index] = checked_sum(deposited_mass_kg[index], direct_deposit)?;
                removed_mass_kg[index] = checked_sum(
                    removed_mass_kg[index],
                    fluvial_mass
                        + inputs.hillslope_removed_by_source_kg[index][source_index]
                        + inputs.coastal_removed_by_source_kg[index][source_index],
                )?;
            }
        }

        for (position, cell) in order.iter().copied().enumerate() {
            poll_cancelled(cancellation, position)?;
            let index = cell.raw() as usize;
            let available = packets[index].total();
            if available == 0.0 {
                continue;
            }
            if LandOceanKind::classify_exact(inputs.elevation_m[index], inputs.sea_level_m)
                == LandOceanKind::Ocean
            {
                throughput_kg[index] = available;
                let exposure = inputs.marine_exposure[index];
                let water_depth_m = (inputs.sea_level_m - inputs.elevation_m[index]).max(0.0);
                let accommodation_kg = load_compensated_accommodation_mass_kg(
                    water_depth_m.min(FORMATION_SHELF_BREAK_DEPTH_M),
                    surface.cells()[index].area.get(),
                );
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
                delta_potential[index] = (shelf_mass / available) as f32;
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
            let deposit_fraction = davy_lague_deposition_fraction(
                f64::from(inputs.mean_annual_discharge_m3_s[index]) * CLIMATOLOGICAL_YEAR_SECONDS,
                inputs.effective_settling_velocity_m_per_year[index],
                surface.cells()[index].area.get(),
            )?;
            let deposit = packets[index].take_fraction(deposit_fraction);
            record_deposit(
                &mut deposited_packets[index],
                deposit,
                &mut land_lake_by_source,
                &mut deposited_mass_kg[index],
            )?;
            let outgoing = std::mem::take(&mut packets[index]);
            throughput_kg[index] = outgoing.total();
            packets[receiver_index].add(outgoing)?;
        }

        let mut final_in_transit_by_source = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
        for (index, packet) in packets.iter().copied().enumerate() {
            poll_cancelled(cancellation, index)?;
            for (source, bucket) in final_in_transit_by_source.iter_mut().enumerate() {
                *bucket = checked_sum(*bucket, packet.routed[source] + packet.coastal[source])?;
            }
        }

        let mut next_stock_mass_by_source_kg = Vec::with_capacity(count);
        let mut deposited_by_source_kg = Vec::with_capacity(count);
        let mut routed_sediment_deposition_m = Vec::with_capacity(count);
        let mut coastal_deposition_m = Vec::with_capacity(count);
        for (index, deposited_packet) in deposited_packets.iter().enumerate() {
            poll_cancelled(cancellation, index)?;
            let area_m2 = surface.cells()[index].area.get();
            let mut deposited = [0.0_f64; SEDIMENT_PROVENANCE_SOURCE_COUNT];
            let mut combined = inputs.retained_sediment_mass_by_source_kg[index];
            for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
                deposited[source] = checked_sum(
                    deposited_packet.routed[source],
                    deposited_packet.coastal[source],
                )?;
                combined[source] = checked_sum(combined[source], deposited[source])?;
            }
            next_stock_mass_by_source_kg.push(combined);
            deposited_by_source_kg.push(deposited);
            routed_sediment_deposition_m.push(
                deposited_packet.routed.iter().sum::<f64>()
                    / (area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3),
            );
            coastal_deposition_m.push(
                deposited_packet.coastal.iter().sum::<f64>()
                    / (area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3),
            );
        }

        let cell_area_m2 = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .collect::<Vec<_>>();
        let bulk_density_kg_m3 = vec![FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3; count];
        let projected_stock =
            SedimentStockState::from_mass_by_source_kg(next_stock_mass_by_source_kg)
                .map_err(map_stock_error)?
                .to_wire_fields(&cell_area_m2, &bulk_density_kg_m3)
                .map_err(map_stock_error)?;
        let fields = FormationSedimentFields::new(
            projected_stock.sediment_thickness_m().to_vec(),
            projected_stock.provenance_fraction().to_vec(),
            annualize_mass(throughput_kg, step_years),
            annualize_mass(shelf_delivery_kg, step_years),
            annualize_mass(deep_ocean_delivery_kg, step_years),
            annualize_mass(endorheic_storage_kg, step_years),
            delta_potential,
        )?;
        let accounted_by_source = std::array::from_fn(|source| {
            land_lake_by_source[source]
                + shelf_by_source[source]
                + deep_by_source[source]
                + final_in_transit_by_source[source]
        });
        let budget_report = SedimentBudgetReport::new(
            produced_by_source.iter().sum::<f64>() / step_years,
            land_lake_by_source.iter().sum::<f64>() / step_years,
            shelf_by_source.iter().sum::<f64>() / step_years,
            deep_by_source.iter().sum::<f64>() / step_years,
            final_in_transit_by_source.iter().sum::<f64>() / step_years,
            produced_by_source.map(|mass| mass / step_years),
            accounted_by_source.map(|mass| mass / step_years),
        )?;
        check_cancelled(cancellation)?;
        Ok(SedimentTransportStep {
            fields,
            budget_report,
            routed_sediment_deposition_m,
            coastal_deposition_m,
            removed_mass_kg,
            deposited_mass_kg,
            deposited_by_source_kg,
        })
    }
}

/// Returns the Davy-Lague effective settling velocity of one cell.
///
/// Yuan et al. (2019) express `V_eff` as the dimensionless
/// [`FORMATION_SEDIMENT_DEPOSITION_COEFFICIENT`] times the runoff rate, which
/// makes the deposited share of a cell a pure catchment-area ratio and the
/// along-reach total independent of the grid the reach is sampled on. Keeping
/// that mapping here means the coefficient has exactly one consumer.
pub fn davy_lague_effective_settling_velocity_m_per_year(annual_local_runoff_mm: f32) -> f64 {
    FORMATION_SEDIMENT_DEPOSITION_COEFFICIENT * f64::from(annual_local_runoff_mm.max(0.0))
        / MILLIMETERS_PER_METER
}

/// Analytic Davy-Lague deposition fraction for one finite-volume cell.
///
/// This is algebraically `V_eff A / (Q + V_eff A)`. Scaling both terms before
/// division avoids overflowing a valid finite ratio.
fn davy_lague_deposition_fraction(
    discharge_m3_per_year: f64,
    effective_settling_velocity_m_per_year: f64,
    area_m2: f64,
) -> Result<f64, SedimentGenerationError> {
    let settling_volume_m3_per_year = effective_settling_velocity_m_per_year * area_m2;
    if !settling_volume_m3_per_year.is_finite() {
        return Err(SedimentGenerationError::NumericalOverflow);
    }
    if settling_volume_m3_per_year == 0.0 {
        return Ok(0.0);
    }
    if discharge_m3_per_year == 0.0 {
        return Ok(1.0);
    }
    let scale = discharge_m3_per_year.max(settling_volume_m3_per_year);
    let scaled_discharge = discharge_m3_per_year / scale;
    let scaled_settling = settling_volume_m3_per_year / scale;
    Ok(scaled_settling / (scaled_discharge + scaled_settling))
}

fn load_compensated_accommodation_mass_kg(depth_m: f64, area_m2: f64) -> f64 {
    let retained_surface_fraction =
        1.0 - FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3 / FORMATION_AIRY_MANTLE_DENSITY_KG_M3;
    depth_m * area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3 / retained_surface_fraction
}

fn annualize_mass(values: Vec<f64>, step_years: f64) -> Vec<f64> {
    values.into_iter().map(|value| value / step_years).collect()
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

fn map_stock_error(error: FormationStateError) -> SedimentGenerationError {
    SedimentGenerationError::InvalidStockProjection {
        reason: error.to_string(),
    }
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
        || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&inputs.sea_level_m)
    {
        return Err(SedimentGenerationError::InvalidSeaLevel {
            found: inputs.sea_level_m,
        });
    }
    let count = surface.cells().len();
    for (field, found) in [
        ("elevation_m", inputs.elevation_m.len()),
        ("flow_receiver", inputs.flow_receiver.len()),
        (
            "mean_annual_discharge_m3_s",
            inputs.mean_annual_discharge_m3_s.len(),
        ),
        (
            "effective_settling_velocity_m_per_year",
            inputs.effective_settling_velocity_m_per_year.len(),
        ),
        (
            "fluvial_removed_by_source_kg",
            inputs.fluvial_removed_by_source_kg.len(),
        ),
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
            "retained_sediment_mass_by_source_kg",
            inputs.retained_sediment_mass_by_source_kg.len(),
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
        let elevation_m = inputs.elevation_m[index];
        if !elevation_m.is_finite()
            || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M)).contains(&elevation_m)
        {
            return Err(SedimentGenerationError::InvalidCellValue {
                field: "elevation_m",
                cell,
                found: elevation_m,
            });
        }
        let marine_exposure = inputs.marine_exposure[index];
        if !marine_exposure.is_finite() || !(0.0..=1.0).contains(&marine_exposure) {
            return Err(SedimentGenerationError::InvalidCellValue {
                field: "marine_exposure",
                cell,
                found: marine_exposure,
            });
        }
        let discharge = inputs.mean_annual_discharge_m3_s[index];
        if !discharge.is_finite() || discharge < 0.0 {
            return Err(SedimentGenerationError::InvalidCellValue {
                field: "mean_annual_discharge_m3_s",
                cell,
                found: f64::from(discharge),
            });
        }
        let settling_velocity = inputs.effective_settling_velocity_m_per_year[index];
        if !settling_velocity.is_finite() || settling_velocity < 0.0 {
            return Err(SedimentGenerationError::InvalidCellValue {
                field: "effective_settling_velocity_m_per_year",
                cell,
                found: settling_velocity,
            });
        }
        let mut retained_stock_kg = 0.0_f64;
        for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
            let retained_mass_kg = inputs.retained_sediment_mass_by_source_kg[index][source];
            if !retained_mass_kg.is_finite() || retained_mass_kg < 0.0 {
                return Err(SedimentGenerationError::InvalidCellValue {
                    field: "retained_sediment_mass_by_source_kg",
                    cell,
                    found: retained_mass_kg,
                });
            }
            retained_stock_kg = checked_sum(retained_stock_kg, retained_mass_kg)?;
            for (field, value) in [
                (
                    "fluvial_removed_by_source_kg",
                    inputs.fluvial_removed_by_source_kg[index][source],
                ),
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
    #[error("sediment sea level is invalid: {found}")]
    InvalidSeaLevel { found: f64 },
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
    #[error("invalid exact sediment-stock projection: {reason}")]
    InvalidStockProjection { reason: String },
    #[error("invalid retained sediment product: {0}")]
    InvalidRetainedFields(#[from] SurfaceFormationValidationError),
}
