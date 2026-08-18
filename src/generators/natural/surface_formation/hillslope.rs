use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, SurfaceWaterField, SurfaceWaterKind, ELEVATION_MAX_M,
    ELEVATION_MIN_M, FORMATION_HILLSLOPE_CRITICAL_SLOPE, FORMATION_HILLSLOPE_DENOMINATOR_MIN,
    FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR, FORMATION_HILLSLOPE_ERODIBILITY_BASE,
    FORMATION_HILLSLOPE_ERODIBILITY_RANGE, FORMATION_HILLSLOPE_FRACTURE_BASE,
    FORMATION_HILLSLOPE_FRACTURE_RANGE, FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX,
    FORMATION_HILLSLOPE_PRECIPITATION_REFERENCE_MM, FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION,
    FORMATION_HILLSLOPE_WEATHERING_BASE, FORMATION_HILLSLOPE_WEATHERING_RANGE,
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
}

#[derive(Debug, Clone, Copy)]
struct EdgeTransfer {
    donor: usize,
    receiver: usize,
    requested_volume_m3: f64,
}

/// Reusable dense buffers for the paired edge solve.
#[derive(Debug, Default)]
pub struct HillslopeWorkspace {
    transfers: Vec<EdgeTransfer>,
    outgoing_requested_m3: Vec<f64>,
    incoming_requested_m3: Vec<f64>,
    outgoing_limit_m3: Vec<f64>,
    incoming_limit_m3: Vec<f64>,
    erosion_volume_m3: Vec<f64>,
    deposition_volume_m3: Vec<f64>,
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
            &mut self.outgoing_requested_m3,
            &mut self.incoming_requested_m3,
            &mut self.outgoing_limit_m3,
            &mut self.incoming_limit_m3,
            &mut self.erosion_volume_m3,
            &mut self.deposition_volume_m3,
        ] {
            if values.len() != cell_count {
                values.resize(cell_count, 0.0);
                allocated = true;
            }
            values.fill(0.0);
        }
        self.outgoing_limit_m3.fill(f64::INFINITY);
        self.incoming_limit_m3.fill(f64::INFINITY);
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
    transported_volume_m3: f64,
    removed_volume_m3: f64,
    deposited_volume_m3: f64,
    retained_volume_relative_error: f64,
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

    pub const fn transported_volume_m3(&self) -> f64 {
        self.transported_volume_m3
    }

    pub const fn removed_volume_m3(&self) -> f64 {
        self.removed_volume_m3
    }

    pub const fn deposited_volume_m3(&self) -> f64 {
        self.deposited_volume_m3
    }

    pub const fn retained_volume_relative_error(&self) -> f64 {
        self.retained_volume_relative_error
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
            let pair_limit_m3 = FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION * relief_m
                / (1.0 / donor_area_m2 + 1.0 / receiver_area_m2);
            let requested_volume_m3 = requested_volume_m3.min(pair_limit_m3);
            workspace.transfers.push(EdgeTransfer {
                donor,
                receiver,
                requested_volume_m3,
            });
            workspace.outgoing_requested_m3[donor] += requested_volume_m3;
            workspace.incoming_requested_m3[receiver] += requested_volume_m3;
            workspace.outgoing_limit_m3[donor] = workspace.outgoing_limit_m3[donor]
                .min(FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION * relief_m * donor_area_m2);
            workspace.incoming_limit_m3[receiver] = workspace.incoming_limit_m3[receiver]
                .min(FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION * relief_m * receiver_area_m2);
        }

        let mut transported_volume_m3 = 0.0_f64;
        for (position, transfer) in workspace.transfers.iter().enumerate() {
            poll_cancelled(cancellation, position)?;
            let outgoing_scale = limit_scale(
                workspace.outgoing_limit_m3[transfer.donor],
                workspace.outgoing_requested_m3[transfer.donor],
            );
            let incoming_scale = limit_scale(
                workspace.incoming_limit_m3[transfer.receiver],
                workspace.incoming_requested_m3[transfer.receiver],
            );
            let retained_volume_m3 =
                transfer.requested_volume_m3 * outgoing_scale.min(incoming_scale);
            workspace.erosion_volume_m3[transfer.donor] += retained_volume_m3;
            workspace.deposition_volume_m3[transfer.receiver] += retained_volume_m3;
            transported_volume_m3 += retained_volume_m3;
        }

        let mut hillslope_erosion_m = Vec::with_capacity(cell_count);
        let mut hillslope_deposition_m = Vec::with_capacity(cell_count);
        let mut elevation_m = Vec::with_capacity(cell_count);
        let mut retained_removed_volume_m3 = 0.0_f64;
        let mut retained_deposited_volume_m3 = 0.0_f64;
        for index in 0..cell_count {
            poll_cancelled(cancellation, index)?;
            let area_m2 = surface.cells()[index].area.get();
            let erosion_m = (workspace.erosion_volume_m3[index] / area_m2) as f32;
            let deposition_m = (workspace.deposition_volume_m3[index] / area_m2) as f32;
            retained_removed_volume_m3 += f64::from(erosion_m) * area_m2;
            retained_deposited_volume_m3 += f64::from(deposition_m) * area_m2;
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
        let retained_scale = retained_removed_volume_m3
            .abs()
            .max(retained_deposited_volume_m3.abs())
            .max(1.0);
        let retained_volume_relative_error =
            (retained_removed_volume_m3 - retained_deposited_volume_m3).abs() / retained_scale;
        check_cancelled(cancellation)?;
        Ok(HillslopeTransportStep {
            elevation_m,
            hillslope_erosion_m,
            hillslope_deposition_m,
            transported_volume_m3,
            removed_volume_m3: transported_volume_m3,
            deposited_volume_m3: transported_volume_m3,
            retained_volume_relative_error,
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
        ] {
            if !value.is_finite() || !(0.0..=maximum).contains(&value) {
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
