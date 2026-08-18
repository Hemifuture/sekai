use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, SedimentSourceKindField, SurfaceWaterField,
    SurfaceWaterKind, CRUST_DENSITY_MAX_KG_M3, CRUST_DENSITY_MIN_KG_M3, ELEVATION_MAX_M,
    ELEVATION_MIN_M, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3, FORMATION_HILLSLOPE_CRITICAL_SLOPE,
    FORMATION_HILLSLOPE_DENOMINATOR_MIN, FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR,
    FORMATION_HILLSLOPE_ERODIBILITY_BASE, FORMATION_HILLSLOPE_ERODIBILITY_RANGE,
    FORMATION_HILLSLOPE_FRACTURE_BASE, FORMATION_HILLSLOPE_FRACTURE_RANGE,
    FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX, FORMATION_HILLSLOPE_PRECIPITATION_REFERENCE_MM,
    FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION, FORMATION_HILLSLOPE_WEATHERING_BASE,
    FORMATION_HILLSLOPE_WEATHERING_RANGE, SEDIMENT_PROVENANCE_SOURCE_COUNT,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SphericalSurfaceValidationError};
use crate::world::CellId;

const CANCELLATION_POLL_MASK: usize = 255;

/// Borrowed physical fields consumed by one nonlinear hillslope step.
#[derive(Debug, Clone, Copy)]
pub struct HillslopeInputs<'a> {
    pub elevation_m: &'a [f32],
    pub surface_water: &'a SurfaceWaterField,
    pub substrate_erodibility: &'a [f32],
    pub fracture_intensity: &'a [f32],
    pub annual_precipitation_mm: &'a [f32],
    pub substrate_density_kg_m3: &'a [f32],
    pub sediment_sources: &'a SedimentSourceKindField,
}

#[derive(Debug, Clone, Copy)]
struct EdgeTransfer {
    donor: usize,
    receiver: usize,
    source_index: usize,
    requested_mass_kg: f64,
}

/// Reusable dense buffers for the paired edge solve.
#[derive(Debug, Default)]
pub struct HillslopeWorkspace {
    transfers: Vec<EdgeTransfer>,
    outgoing_requested_kg: Vec<f64>,
    incoming_requested_kg: Vec<f64>,
    outgoing_limit_kg: Vec<f64>,
    incoming_limit_kg: Vec<f64>,
    erosion_mass_kg: Vec<f64>,
    deposition_mass_kg: Vec<f64>,
    erosion_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    deposition_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    allocation_epoch: u64,
}

impl HillslopeWorkspace {
    fn prepare(&mut self, cell_count: usize, edge_count: usize) {
        let mut allocated = false;
        if self.transfers.capacity() < edge_count {
            self.transfers
                .reserve_exact(edge_count - self.transfers.capacity());
            allocated = true;
        }
        self.transfers.clear();
        for values in [
            &mut self.outgoing_requested_kg,
            &mut self.incoming_requested_kg,
            &mut self.outgoing_limit_kg,
            &mut self.incoming_limit_kg,
            &mut self.erosion_mass_kg,
            &mut self.deposition_mass_kg,
        ] {
            if values.len() != cell_count {
                values.resize(cell_count, 0.0);
                allocated = true;
            }
            values.fill(0.0);
        }
        if self.erosion_by_source_kg.len() != cell_count {
            self.erosion_by_source_kg
                .resize(cell_count, [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]);
            allocated = true;
        }
        if self.deposition_by_source_kg.len() != cell_count {
            self.deposition_by_source_kg
                .resize(cell_count, [0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]);
            allocated = true;
        }
        self.erosion_by_source_kg
            .fill([0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]);
        self.deposition_by_source_kg
            .fill([0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]);
        self.outgoing_limit_kg.fill(f64::INFINITY);
        self.incoming_limit_kg.fill(f64::INFINITY);
        if allocated {
            self.allocation_epoch = self.allocation_epoch.saturating_add(1);
        }
    }

    /// Monotonic testable marker incremented only when dense capacity grows.
    pub const fn allocation_epoch(&self) -> u64 {
        self.allocation_epoch
    }
}

/// Retained output and paired-volume evidence for one hillslope step.
#[derive(Debug, Clone, PartialEq)]
pub struct HillslopeTransportStep {
    elevation_m: Vec<f32>,
    hillslope_erosion_m: Vec<f32>,
    hillslope_deposition_m: Vec<f32>,
    removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    deposited_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    transported_mass_kg: f64,
    removed_volume_m3: f64,
    deposited_volume_m3: f64,
    removed_mass_kg: f64,
    deposited_mass_kg: f64,
    retained_mass_relative_error: f64,
}

impl HillslopeTransportStep {
    pub fn elevation_m(&self) -> &[f32] {
        &self.elevation_m
    }

    pub fn hillslope_erosion_m(&self) -> &[f32] {
        &self.hillslope_erosion_m
    }

    pub fn hillslope_deposition_m(&self) -> &[f32] {
        &self.hillslope_deposition_m
    }

    pub fn deposited_by_source_kg(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.deposited_by_source_kg
    }

    pub fn removed_by_source_kg(&self) -> &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.removed_by_source_kg
    }

    pub const fn transported_mass_kg(&self) -> f64 {
        self.transported_mass_kg
    }

    pub const fn removed_volume_m3(&self) -> f64 {
        self.removed_volume_m3
    }

    pub const fn deposited_volume_m3(&self) -> f64 {
        self.deposited_volume_m3
    }

    pub const fn removed_mass_kg(&self) -> f64 {
        self.removed_mass_kg
    }

    pub const fn deposited_mass_kg(&self) -> f64 {
        self.deposited_mass_kg
    }

    pub const fn retained_mass_relative_error(&self) -> f64 {
        self.retained_mass_relative_error
    }
}

/// Paired finite-volume Roering-style coarse hillslope transport.
#[derive(Debug, Clone, Copy, Default)]
pub struct NonlinearHillslopeTransport;

impl NonlinearHillslopeTransport {
    pub fn advance(
        surface: &SphericalSurfaceSnapshot,
        inputs: HillslopeInputs<'_>,
        step_years: f64,
        workspace: &mut HillslopeWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<HillslopeTransportStep, HillslopeGenerationError> {
        check_cancelled(cancellation)?;
        surface.validate()?;
        validate_inputs(surface, inputs, step_years, cancellation)?;
        let cell_count = surface.cells().len();
        workspace.prepare(cell_count, surface.edges().len());

        for (position, edge) in surface.edges().iter().enumerate() {
            poll_cancelled(cancellation, position)?;
            let first = edge.cells[0].raw() as usize;
            let second = edge.cells[1].raw() as usize;
            if inputs.surface_water.get(first) != Some(SurfaceWaterKind::DryLand)
                || inputs.surface_water.get(second) != Some(SurfaceWaterKind::DryLand)
            {
                continue;
            }
            let first_height = f64::from(inputs.elevation_m[first]);
            let second_height = f64::from(inputs.elevation_m[second]);
            let (donor, receiver, relief_m) = if first_height > second_height {
                (first, second, first_height - second_height)
            } else if second_height > first_height {
                (second, first, second_height - first_height)
            } else {
                continue;
            };
            let distance_m = edge.center_distance.get();
            let slope = relief_m / distance_m;
            let normalized_slope = slope / FORMATION_HILLSLOPE_CRITICAL_SLOPE;
            let denominator = (1.0 - normalized_slope * normalized_slope)
                .max(FORMATION_HILLSLOPE_DENOMINATOR_MIN);
            let effective_diffusivity = FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR
                * 0.5
                * (hillslope_factor(inputs, donor) + hillslope_factor(inputs, receiver));
            let requested_volume_m3 =
                effective_diffusivity * edge.length.get() * slope / denominator * step_years;
            if requested_volume_m3 <= 0.0 {
                continue;
            }
            let donor_area_m2 = surface.cells()[donor].area.get();
            let receiver_area_m2 = surface.cells()[receiver].area.get();
            let donor_density_kg_m3 = f64::from(inputs.substrate_density_kg_m3[donor]);
            let pair_limit_kg = FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION * relief_m
                / (1.0 / (donor_density_kg_m3 * donor_area_m2)
                    + 1.0 / (FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3 * receiver_area_m2));
            let requested_mass_kg = (requested_volume_m3 * donor_density_kg_m3).min(pair_limit_kg);
            let source_index = inputs
                .sediment_sources
                .get(donor)
                .expect("validated source field covers every cell")
                .raw() as usize;
            workspace.transfers.push(EdgeTransfer {
                donor,
                receiver,
                source_index,
                requested_mass_kg,
            });
            workspace.outgoing_requested_kg[donor] += requested_mass_kg;
            workspace.incoming_requested_kg[receiver] += requested_mass_kg;
            workspace.outgoing_limit_kg[donor] = workspace.outgoing_limit_kg[donor].min(
                FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION
                    * relief_m
                    * donor_area_m2
                    * donor_density_kg_m3,
            );
            workspace.incoming_limit_kg[receiver] = workspace.incoming_limit_kg[receiver].min(
                FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION
                    * relief_m
                    * receiver_area_m2
                    * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
            );
        }

        let mut transported_mass_kg = 0.0_f64;
        let mut removed_volume_m3 = 0.0_f64;
        for (position, transfer) in workspace.transfers.iter().enumerate() {
            poll_cancelled(cancellation, position)?;
            let outgoing_scale = limit_scale(
                workspace.outgoing_limit_kg[transfer.donor],
                workspace.outgoing_requested_kg[transfer.donor],
            );
            let incoming_scale = limit_scale(
                workspace.incoming_limit_kg[transfer.receiver],
                workspace.incoming_requested_kg[transfer.receiver],
            );
            let retained_mass_kg = transfer.requested_mass_kg * outgoing_scale.min(incoming_scale);
            workspace.erosion_mass_kg[transfer.donor] += retained_mass_kg;
            workspace.deposition_mass_kg[transfer.receiver] += retained_mass_kg;
            workspace.erosion_by_source_kg[transfer.donor][transfer.source_index] +=
                retained_mass_kg;
            workspace.deposition_by_source_kg[transfer.receiver][transfer.source_index] +=
                retained_mass_kg;
            transported_mass_kg += retained_mass_kg;
            removed_volume_m3 +=
                retained_mass_kg / f64::from(inputs.substrate_density_kg_m3[transfer.donor]);
        }

        let mut hillslope_erosion_m = Vec::with_capacity(cell_count);
        let mut hillslope_deposition_m = Vec::with_capacity(cell_count);
        let mut elevation_m = Vec::with_capacity(cell_count);
        let mut retained_removed_mass_kg = 0.0_f64;
        let mut retained_deposited_mass_kg = 0.0_f64;
        for index in 0..cell_count {
            poll_cancelled(cancellation, index)?;
            let area_m2 = surface.cells()[index].area.get();
            let erosion_m = (workspace.erosion_mass_kg[index]
                / (f64::from(inputs.substrate_density_kg_m3[index]) * area_m2))
                as f32;
            let deposition_m = (workspace.deposition_mass_kg[index]
                / (FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3 * area_m2))
                as f32;
            retained_removed_mass_kg +=
                f64::from(erosion_m) * area_m2 * f64::from(inputs.substrate_density_kg_m3[index]);
            retained_deposited_mass_kg +=
                f64::from(deposition_m) * area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3;
            hillslope_erosion_m.push(erosion_m);
            hillslope_deposition_m.push(deposition_m);
            elevation_m.push(formation_elevation_from_components(
                inputs.elevation_m[index],
                0.0,
                0.0,
                erosion_m,
                deposition_m,
                0.0,
                0.0,
                0.0,
                0.0,
            ));
        }
        validate_no_inversion(surface, inputs, &elevation_m, cancellation)?;
        let retained_scale = retained_removed_mass_kg
            .abs()
            .max(retained_deposited_mass_kg.abs())
            .max(1.0);
        let retained_mass_relative_error =
            (retained_removed_mass_kg - retained_deposited_mass_kg).abs() / retained_scale;
        check_cancelled(cancellation)?;
        Ok(HillslopeTransportStep {
            elevation_m,
            hillslope_erosion_m,
            hillslope_deposition_m,
            removed_by_source_kg: workspace.erosion_by_source_kg.clone(),
            deposited_by_source_kg: workspace.deposition_by_source_kg.clone(),
            transported_mass_kg,
            removed_volume_m3,
            deposited_volume_m3: transported_mass_kg / FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
            removed_mass_kg: transported_mass_kg,
            deposited_mass_kg: transported_mass_kg,
            retained_mass_relative_error,
        })
    }
}

fn hillslope_factor(inputs: HillslopeInputs<'_>, index: usize) -> f64 {
    let erodibility = FORMATION_HILLSLOPE_ERODIBILITY_BASE
        + FORMATION_HILLSLOPE_ERODIBILITY_RANGE * f64::from(inputs.substrate_erodibility[index]);
    let fracture = FORMATION_HILLSLOPE_FRACTURE_BASE
        + FORMATION_HILLSLOPE_FRACTURE_RANGE * f64::from(inputs.fracture_intensity[index]);
    let wetness = (f64::from(inputs.annual_precipitation_mm[index])
        / FORMATION_HILLSLOPE_PRECIPITATION_REFERENCE_MM)
        .clamp(0.0, FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX)
        .sqrt()
        / FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX.sqrt();
    let weathering =
        FORMATION_HILLSLOPE_WEATHERING_BASE + FORMATION_HILLSLOPE_WEATHERING_RANGE * wetness;
    erodibility * fracture * weathering
}

fn limit_scale(limit: f64, requested: f64) -> f64 {
    if requested == 0.0 || !limit.is_finite() {
        1.0
    } else {
        (limit / requested).clamp(0.0, 1.0)
    }
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    inputs: HillslopeInputs<'_>,
    step_years: f64,
    cancellation: &BuildCancellation,
) -> Result<(), HillslopeGenerationError> {
    if !step_years.is_finite() || step_years <= 0.0 {
        return Err(HillslopeGenerationError::InvalidStepYears { found: step_years });
    }
    let count = surface.cells().len();
    for (field, found) in [
        ("elevation_m", inputs.elevation_m.len()),
        ("surface_water", inputs.surface_water.len()),
        ("substrate_erodibility", inputs.substrate_erodibility.len()),
        ("fracture_intensity", inputs.fracture_intensity.len()),
        (
            "annual_precipitation_mm",
            inputs.annual_precipitation_mm.len(),
        ),
        (
            "substrate_density_kg_m3",
            inputs.substrate_density_kg_m3.len(),
        ),
        ("sediment_sources", inputs.sediment_sources.len()),
    ] {
        if found != count {
            return Err(HillslopeGenerationError::CellCountMismatch {
                field,
                expected: count,
                found,
            });
        }
    }
    for index in 0..count {
        poll_cancelled(cancellation, index)?;
        let cell = CellId::from_raw(index as u32);
        let elevation = inputs.elevation_m[index];
        if !elevation.is_finite() || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&elevation) {
            return Err(HillslopeGenerationError::InvalidCellValue {
                field: "elevation_m",
                cell,
                found: f64::from(elevation),
            });
        }
        for (field, value, maximum) in [
            (
                "substrate_erodibility",
                inputs.substrate_erodibility[index],
                1.0,
            ),
            ("fracture_intensity", inputs.fracture_intensity[index], 1.0),
            (
                "annual_precipitation_mm",
                inputs.annual_precipitation_mm[index],
                f32::MAX,
            ),
            (
                "substrate_density_kg_m3",
                inputs.substrate_density_kg_m3[index],
                CRUST_DENSITY_MAX_KG_M3,
            ),
        ] {
            let minimum = if field == "substrate_density_kg_m3" {
                CRUST_DENSITY_MIN_KG_M3
            } else {
                0.0
            };
            if !value.is_finite() || !(minimum..=maximum).contains(&value) {
                return Err(HillslopeGenerationError::InvalidCellValue {
                    field,
                    cell,
                    found: f64::from(value),
                });
            }
        }
    }
    Ok(())
}

fn validate_no_inversion(
    surface: &SphericalSurfaceSnapshot,
    inputs: HillslopeInputs<'_>,
    result_elevation_m: &[f32],
    cancellation: &BuildCancellation,
) -> Result<(), HillslopeGenerationError> {
    for (position, edge) in surface.edges().iter().enumerate() {
        poll_cancelled(cancellation, position)?;
        let first = edge.cells[0].raw() as usize;
        let second = edge.cells[1].raw() as usize;
        if inputs.surface_water.get(first) != Some(SurfaceWaterKind::DryLand)
            || inputs.surface_water.get(second) != Some(SurfaceWaterKind::DryLand)
        {
            continue;
        }
        let original_order = inputs.elevation_m[first].total_cmp(&inputs.elevation_m[second]);
        let retained_order = result_elevation_m[first].total_cmp(&result_elevation_m[second]);
        if (original_order.is_gt() && retained_order.is_lt())
            || (original_order.is_lt() && retained_order.is_gt())
        {
            return Err(HillslopeGenerationError::SlopeInversion {
                first: edge.cells[0],
                second: edge.cells[1],
            });
        }
    }
    Ok(())
}

fn poll_cancelled(
    cancellation: &BuildCancellation,
    index: usize,
) -> Result<(), HillslopeGenerationError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), HillslopeGenerationError> {
    if cancellation.is_cancelled() {
        Err(HillslopeGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

/// Failures returned by paired P5 hillslope transport.
#[derive(Debug, Error)]
pub enum HillslopeGenerationError {
    #[error("hillslope transport cancelled")]
    Cancelled,
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("hillslope field {field} has length {found}; expected {expected}")]
    CellCountMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("hillslope field {field} has invalid value {found} at {cell:?}")]
    InvalidCellValue {
        field: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error("hillslope step duration must be finite and positive, got {found}")]
    InvalidStepYears { found: f64 },
    #[error("hillslope transfer inverted edge {first:?}-{second:?}")]
    SlopeInversion { first: CellId, second: CellId },
}
