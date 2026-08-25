use std::cmp::Reverse;
use std::collections::BinaryHeap;

use thiserror::Error;

use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, EvolvedTectonicSnapshot, EvolvedTectonicValidationError,
    GeologicSubstrateSnapshot, GeologicSubstrateValidationError, SphericalHydrologySnapshot,
    SphericalHydrologyValidationError, SurfaceWaterField, SurfaceWaterKind, ELEVATION_MAX_M,
    ELEVATION_MIN_M, FORMATION_STREAM_POWER_AREA_EXPONENT, FORMATION_STREAM_POWER_ERODIBILITY_BASE,
    FORMATION_STREAM_POWER_ERODIBILITY_RANGE,
    FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR,
    FORMATION_STREAM_POWER_RUNOFF_FACTOR_MAX, FORMATION_STREAM_POWER_RUNOFF_FACTOR_MIN,
    FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM, FORMATION_STREAM_POWER_SLOPE_THRESHOLD,
    MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SphericalSurfaceValidationError};
use crate::world::CellId;

const SQUARE_METERS_PER_SQUARE_KILOMETER: f64 = 1_000_000.0;
const MILLIMETERS_PER_METER: f64 = 1_000.0;
const CANCELLATION_POLL_MASK: usize = 255;

/// Borrowed dense fields consumed by the standalone implicit stream-power kernel.
#[derive(Debug, Clone, Copy)]
pub struct StreamPowerInputs<'a> {
    pub elevation_m: &'a [f32],
    pub flow_receiver: &'a [Option<CellId>],
    pub surface_water: &'a SurfaceWaterField,
    pub drainage_area_km2: &'a [f32],
    pub annual_local_runoff_mm: &'a [f32],
    pub uplift_rate_mm_per_year: &'a [f32],
    pub subsidence_rate_mm_per_year: &'a [f32],
    pub substrate_erodibility: &'a [f32],
}

/// Private output of one tectonic-plus-fluvial continuation update.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamPowerStep {
    elevation_m: Vec<f32>,
    tectonic_displacement_m: Vec<f32>,
    fluvial_erosion_m: Vec<f32>,
}

impl StreamPowerStep {
    pub fn elevation_m(&self) -> &[f32] {
        &self.elevation_m
    }

    pub fn tectonic_displacement_m(&self) -> &[f32] {
        &self.tectonic_displacement_m
    }

    pub fn fluvial_erosion_m(&self) -> &[f32] {
        &self.fluvial_erosion_m
    }
}

/// Braun-Willett downstream-stack solver specialized to the locked `n = 1` law.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImplicitStreamPowerSolver;

impl ImplicitStreamPowerSolver {
    /// Advances already extracted fields on the authoritative spherical graph.
    pub fn advance(
        surface: &SphericalSurfaceSnapshot,
        inputs: StreamPowerInputs<'_>,
        step_years: f64,
        cancellation: &BuildCancellation,
    ) -> Result<StreamPowerStep, StreamPowerGenerationError> {
        check_cancelled(cancellation)?;
        surface.validate()?;
        Self::advance_from_validated_surface(surface, inputs, step_years, cancellation)
    }

    /// Same solve for a caller that already validated the authoritative surface
    /// in this build, such as the compound P5 compositor.
    pub(super) fn advance_from_validated_surface(
        surface: &SphericalSurfaceSnapshot,
        inputs: StreamPowerInputs<'_>,
        step_years: f64,
        cancellation: &BuildCancellation,
    ) -> Result<StreamPowerStep, StreamPowerGenerationError> {
        check_cancelled(cancellation)?;
        validate_inputs(surface, inputs, step_years, cancellation)?;
        let order = upstream_to_downstream_order(inputs.flow_receiver, cancellation)?;
        let count = surface.cells().len();
        let mut tectonic_displacement_m = vec![0.0_f32; count];
        let mut fluvial_erosion_m = vec![0.0_f32; count];
        let mut elevation_m = Vec::with_capacity(count);

        // Rock columns move with the plate whether their top is exposed or
        // submerged, so the current forcing carries no surface-water mask. Only
        // the subaerial incision pass below reads `SurfaceWaterKind`.
        for (index, retained_displacement_slot) in tectonic_displacement_m.iter_mut().enumerate() {
            poll_cancelled(cancellation, index)?;
            let initial = inputs.elevation_m[index];
            let net_rate_m_per_year = (f64::from(inputs.uplift_rate_mm_per_year[index])
                - f64::from(inputs.subsidence_rate_mm_per_year[index]))
                / MILLIMETERS_PER_METER;
            let forced = f64::from(initial) + net_rate_m_per_year * step_years;
            let retained_displacement = (forced - f64::from(initial)) as f32;
            *retained_displacement_slot = retained_displacement;
            elevation_m.push(formation_elevation_from_components(
                initial,
                retained_displacement,
            ));
        }

        for (position, &cell) in order.iter().rev().enumerate() {
            poll_cancelled(cancellation, position)?;
            let index = cell.raw() as usize;
            if inputs.surface_water.get(index) != Some(SurfaceWaterKind::DryLand) {
                continue;
            }
            let Some(receiver) = inputs.flow_receiver[index] else {
                continue;
            };
            let receiver_index = receiver.raw() as usize;
            let receiver_height = f64::from(elevation_m[receiver_index]);
            let forced_height = f64::from(elevation_m[index]);
            let length_m = receiver_length_m(surface, cell, receiver)?;
            let annual_runoff_mm = f64::from(inputs.annual_local_runoff_mm[index]);
            let drainage_area_m2 =
                f64::from(inputs.drainage_area_km2[index]) * SQUARE_METERS_PER_SQUARE_KILOMETER;
            if annual_runoff_mm == 0.0 || drainage_area_m2 == 0.0 {
                continue;
            }
            let runoff_factor = (annual_runoff_mm / FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM)
                .clamp(
                    FORMATION_STREAM_POWER_RUNOFF_FACTOR_MIN,
                    FORMATION_STREAM_POWER_RUNOFF_FACTOR_MAX,
                )
                .sqrt();
            let erodibility_per_year = FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR
                * (FORMATION_STREAM_POWER_ERODIBILITY_BASE
                    + FORMATION_STREAM_POWER_ERODIBILITY_RANGE
                        * f64::from(inputs.substrate_erodibility[index]))
                * runoff_factor;
            let candidate = implicit_stream_power_n1_height(
                forced_height,
                receiver_height,
                length_m,
                drainage_area_m2,
                erodibility_per_year,
                step_years,
            )?;
            if candidate >= forced_height {
                continue;
            }
            let retained_erosion = (forced_height - candidate) as f32;
            let mut retained_height = formation_elevation_from_components(
                inputs.elevation_m[index],
                tectonic_displacement_m[index] - retained_erosion,
            );
            let receiver_height_f32 = elevation_m[receiver_index];
            let mut adjusted_erosion = retained_erosion;
            while retained_height < receiver_height_f32 && adjusted_erosion > 0.0 {
                adjusted_erosion = next_down_nonnegative(adjusted_erosion);
                retained_height = formation_elevation_from_components(
                    inputs.elevation_m[index],
                    tectonic_displacement_m[index] - adjusted_erosion,
                );
            }
            fluvial_erosion_m[index] = adjusted_erosion;
            elevation_m[index] = retained_height;
        }
        for (index, &elevation) in elevation_m.iter().enumerate() {
            if !elevation.is_finite() || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&elevation) {
                return Err(StreamPowerGenerationError::ElevationOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: f64::from(elevation),
                });
            }
        }
        check_cancelled(cancellation)?;
        Ok(StreamPowerStep {
            elevation_m,
            tectonic_displacement_m,
            fluvial_erosion_m,
        })
    }

    /// Typed adapter used by the coupled P5 generator.
    pub fn advance_from_snapshots(
        surface: &SphericalSurfaceSnapshot,
        initial_elevation_m: &[f32],
        hydrology: &SphericalHydrologySnapshot,
        tectonics: &EvolvedTectonicSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        step_years: f64,
        cancellation: &BuildCancellation,
    ) -> Result<StreamPowerStep, StreamPowerGenerationError> {
        check_cancelled(cancellation)?;
        hydrology.validate_against(surface)?;
        check_cancelled(cancellation)?;
        tectonics.validate_against(surface)?;
        check_cancelled(cancellation)?;
        substrate.validate_against_surface(surface)?;
        Self::advance_from_validated_snapshots(
            surface,
            initial_elevation_m,
            hydrology,
            tectonics,
            substrate,
            step_years,
            cancellation,
        )
    }

    /// Same solve for a caller that already validated the surface, hydrology,
    /// tectonic, and substrate products in this build.
    pub(super) fn advance_from_validated_snapshots(
        surface: &SphericalSurfaceSnapshot,
        initial_elevation_m: &[f32],
        hydrology: &SphericalHydrologySnapshot,
        tectonics: &EvolvedTectonicSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        step_years: f64,
        cancellation: &BuildCancellation,
    ) -> Result<StreamPowerStep, StreamPowerGenerationError> {
        Self::advance_from_validated_surface(
            surface,
            StreamPowerInputs {
                elevation_m: initial_elevation_m,
                flow_receiver: hydrology.flow_receiver(),
                surface_water: hydrology.surface_water(),
                drainage_area_km2: hydrology.drainage_area_km2(),
                annual_local_runoff_mm: hydrology.annual_local_runoff_mm(),
                uplift_rate_mm_per_year: tectonics.forcing().uplift_rate_mm_per_year(),
                subsidence_rate_mm_per_year: tectonics.forcing().subsidence_rate_mm_per_year(),
                substrate_erodibility: substrate.erodibility(),
            },
            step_years,
            cancellation,
        )
    }
}

/// Closed-form `n = 1` backward-Euler update for one active receiver reach.
pub fn implicit_stream_power_n1_height(
    forced_height_m: f64,
    receiver_height_m: f64,
    receiver_length_m: f64,
    drainage_area_m2: f64,
    erodibility_per_year: f64,
    step_years: f64,
) -> Result<f64, StreamPowerGenerationError> {
    for (field, value) in [
        ("forced_height_m", forced_height_m),
        ("receiver_height_m", receiver_height_m),
        ("receiver_length_m", receiver_length_m),
        ("drainage_area_m2", drainage_area_m2),
        ("erodibility_per_year", erodibility_per_year),
        ("step_years", step_years),
    ] {
        if !value.is_finite() {
            return Err(StreamPowerGenerationError::InvalidScalarInput {
                field,
                found: value,
            });
        }
    }
    for (field, value) in [
        ("receiver_length_m", receiver_length_m),
        ("step_years", step_years),
    ] {
        if value <= 0.0 {
            return Err(StreamPowerGenerationError::InvalidScalarInput {
                field,
                found: value,
            });
        }
    }
    for (field, value) in [
        ("drainage_area_m2", drainage_area_m2),
        ("erodibility_per_year", erodibility_per_year),
    ] {
        if value < 0.0 {
            return Err(StreamPowerGenerationError::InvalidScalarInput {
                field,
                found: value,
            });
        }
    }
    let threshold_height_m =
        receiver_height_m + receiver_length_m * FORMATION_STREAM_POWER_SLOPE_THRESHOLD;
    if forced_height_m <= threshold_height_m
        || drainage_area_m2 == 0.0
        || erodibility_per_year == 0.0
    {
        return Ok(forced_height_m);
    }
    let area_factor = drainage_area_m2.powf(FORMATION_STREAM_POWER_AREA_EXPONENT);
    let c = step_years * erodibility_per_year * area_factor / receiver_length_m;
    let height = (forced_height_m + c * threshold_height_m) / (1.0 + c);
    if !height.is_finite() || height < receiver_height_m || height > forced_height_m {
        return Err(StreamPowerGenerationError::InvalidScalarResult { found: height });
    }
    Ok(height)
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    inputs: StreamPowerInputs<'_>,
    step_years: f64,
    cancellation: &BuildCancellation,
) -> Result<(), StreamPowerGenerationError> {
    if !step_years.is_finite() || step_years <= 0.0 {
        return Err(StreamPowerGenerationError::InvalidStepYears { found: step_years });
    }
    let count = surface.cells().len();
    for (field, found) in [
        ("elevation_m", inputs.elevation_m.len()),
        ("flow_receiver", inputs.flow_receiver.len()),
        ("surface_water", inputs.surface_water.len()),
        ("drainage_area_km2", inputs.drainage_area_km2.len()),
        (
            "annual_local_runoff_mm",
            inputs.annual_local_runoff_mm.len(),
        ),
        (
            "uplift_rate_mm_per_year",
            inputs.uplift_rate_mm_per_year.len(),
        ),
        (
            "subsidence_rate_mm_per_year",
            inputs.subsidence_rate_mm_per_year.len(),
        ),
        ("substrate_erodibility", inputs.substrate_erodibility.len()),
    ] {
        if found != count {
            return Err(StreamPowerGenerationError::CellCountMismatch {
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
            return Err(StreamPowerGenerationError::InvalidCellValue {
                field: "elevation_m",
                cell,
                found: f64::from(elevation),
            });
        }
        for (field, value) in [
            ("drainage_area_km2", inputs.drainage_area_km2[index]),
            (
                "annual_local_runoff_mm",
                inputs.annual_local_runoff_mm[index],
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(StreamPowerGenerationError::InvalidCellValue {
                    field,
                    cell,
                    found: f64::from(value),
                });
            }
        }
        for (field, value, maximum) in [
            (
                "uplift_rate_mm_per_year",
                inputs.uplift_rate_mm_per_year[index],
                MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR,
            ),
            (
                "subsidence_rate_mm_per_year",
                inputs.subsidence_rate_mm_per_year[index],
                MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR,
            ),
            (
                "substrate_erodibility",
                inputs.substrate_erodibility[index],
                1.0,
            ),
        ] {
            if !value.is_finite() || !(0.0..=maximum).contains(&value) {
                return Err(StreamPowerGenerationError::InvalidCellValue {
                    field,
                    cell,
                    found: f64::from(value),
                });
            }
        }
        if let Some(receiver) = inputs.flow_receiver[index] {
            if receiver.raw() as usize >= count {
                return Err(StreamPowerGenerationError::ReceiverOutOfRange {
                    cell,
                    receiver,
                    cell_count: count,
                });
            }
            if receiver == cell || receiver_length_m(surface, cell, receiver).is_err() {
                return Err(StreamPowerGenerationError::ReceiverNotAdjacent { cell, receiver });
            }
        }
    }
    Ok(())
}

fn upstream_to_downstream_order(
    receiver: &[Option<CellId>],
    cancellation: &BuildCancellation,
) -> Result<Vec<CellId>, StreamPowerGenerationError> {
    let mut indegree = vec![0_usize; receiver.len()];
    for (index, downstream) in receiver.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        if let Some(downstream) = downstream {
            indegree[downstream.raw() as usize] += 1;
        }
    }
    let mut ready = BinaryHeap::new();
    for (index, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            ready.push(Reverse(index as u32));
        }
    }
    let mut order = Vec::with_capacity(receiver.len());
    while let Some(Reverse(raw)) = ready.pop() {
        poll_cancelled(cancellation, order.len())?;
        order.push(CellId::from_raw(raw));
        if let Some(downstream) = receiver[raw as usize] {
            let downstream_indegree = &mut indegree[downstream.raw() as usize];
            *downstream_indegree -= 1;
            if *downstream_indegree == 0 {
                ready.push(Reverse(downstream.raw()));
            }
        }
    }
    if order.len() != receiver.len() {
        return Err(StreamPowerGenerationError::ReceiverCycle);
    }
    Ok(order)
}

fn receiver_length_m(
    surface: &SphericalSurfaceSnapshot,
    cell: CellId,
    receiver: CellId,
) -> Result<f64, StreamPowerGenerationError> {
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
        .ok_or(StreamPowerGenerationError::ReceiverNotAdjacent { cell, receiver })
}

fn next_down_nonnegative(value: f32) -> f32 {
    if value <= 0.0 {
        0.0
    } else {
        f32::from_bits(value.to_bits() - 1)
    }
}

fn poll_cancelled(
    cancellation: &BuildCancellation,
    index: usize,
) -> Result<(), StreamPowerGenerationError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), StreamPowerGenerationError> {
    if cancellation.is_cancelled() {
        Err(StreamPowerGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

/// Failures returned by the P5 implicit stream-power kernel.
#[derive(Debug, Error)]
pub enum StreamPowerGenerationError {
    #[error("stream-power generation cancelled")]
    Cancelled,
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("invalid spherical hydrology: {0}")]
    InvalidHydrology(#[from] SphericalHydrologyValidationError),
    #[error("invalid evolved tectonics: {0}")]
    InvalidTectonics(#[from] EvolvedTectonicValidationError),
    #[error("invalid geologic substrate: {0}")]
    InvalidSubstrate(#[from] GeologicSubstrateValidationError),
    #[error("stream-power field {field} has length {found}; expected {expected}")]
    CellCountMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("stream-power field {field} has invalid value {found} at {cell:?}")]
    InvalidCellValue {
        field: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error("stream-power step duration must be finite and positive, got {found}")]
    InvalidStepYears { found: f64 },
    #[error("receiver {receiver:?} from {cell:?} exceeds cell count {cell_count}")]
    ReceiverOutOfRange {
        cell: CellId,
        receiver: CellId,
        cell_count: usize,
    },
    #[error("receiver {receiver:?} is not an authoritative neighbor of {cell:?}")]
    ReceiverNotAdjacent { cell: CellId, receiver: CellId },
    #[error("stream-power receiver graph contains a cycle")]
    ReceiverCycle,
    #[error("invalid scalar stream-power input {field}={found}")]
    InvalidScalarInput { field: &'static str, found: f64 },
    #[error("implicit scalar stream-power update produced invalid height {found}")]
    InvalidScalarResult { found: f64 },
    #[error("stream-power elevation at {cell:?} is outside the supported range: {found}")]
    ElevationOutOfRange { cell: CellId, found: f64 },
}
