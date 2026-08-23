use thiserror::Error;

use super::super::surface_water_geometry::solve_physical_sea_level_cancellable;
use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, LandOceanField, WaterVolumeSolveError, ELEVATION_MAX_M,
    ELEVATION_MIN_M, FORMATION_AIRY_MANTLE_DENSITY_KG_M3,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SphericalSurfaceValidationError};
use crate::world::CellId;

const CANCELLATION_POLL_MASK: usize = 255;

/// Retained local loading/unloading response.
#[derive(Debug, Clone, PartialEq)]
pub struct IsostaticAdjustmentStep {
    isostatic_response_m: Vec<f32>,
    elevation_m: Vec<f32>,
}

impl IsostaticAdjustmentStep {
    pub fn isostatic_response_m(&self) -> &[f32] {
        &self.isostatic_response_m
    }

    pub fn elevation_m(&self) -> &[f32] {
        &self.elevation_m
    }
}

/// Local Airy response to the exact retained removal/deposition mass ledger.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalAiryIsostasy;

impl LocalAiryIsostasy {
    pub fn apply(
        surface: &SphericalSurfaceSnapshot,
        elevation_m: &[f32],
        removed_mass_kg: &[f64],
        deposited_mass_kg: &[f64],
        cancellation: &BuildCancellation,
    ) -> Result<IsostaticAdjustmentStep, IsostasyGenerationError> {
        check_cancelled(cancellation)?;
        surface
            .validate_cancellable(&|| cancellation.is_cancelled())
            .map_err(|error| map_surface_error(error, cancellation))?;
        Self::apply_from_validated_surface(
            surface,
            elevation_m,
            removed_mass_kg,
            deposited_mass_kg,
            cancellation,
        )
    }

    /// Same local response for a caller that already validated the surface.
    pub(super) fn apply_from_validated_surface(
        surface: &SphericalSurfaceSnapshot,
        elevation_m: &[f32],
        removed_mass_kg: &[f64],
        deposited_mass_kg: &[f64],
        cancellation: &BuildCancellation,
    ) -> Result<IsostaticAdjustmentStep, IsostasyGenerationError> {
        check_cancelled(cancellation)?;
        let count = surface.cells().len();
        for (field, found) in [
            ("elevation_m", elevation_m.len()),
            ("removed_mass_kg", removed_mass_kg.len()),
            ("deposited_mass_kg", deposited_mass_kg.len()),
        ] {
            if found != count {
                return Err(IsostasyGenerationError::CellCountMismatch {
                    field,
                    expected: count,
                    found,
                });
            }
        }
        let mut response = Vec::with_capacity(count);
        let mut result = Vec::with_capacity(count);
        for index in 0..count {
            poll_cancelled(cancellation, index)?;
            let cell = CellId::from_raw(index as u32);
            let base = elevation_m[index];
            if !base.is_finite() || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&base) {
                return Err(IsostasyGenerationError::InvalidCellValue {
                    field: "elevation_m",
                    cell,
                    found: f64::from(base),
                });
            }
            for (field, value) in [
                ("removed_mass_kg", removed_mass_kg[index]),
                ("deposited_mass_kg", deposited_mass_kg[index]),
            ] {
                if !value.is_finite() || value < 0.0 {
                    return Err(IsostasyGenerationError::InvalidCellValue {
                        field,
                        cell,
                        found: value,
                    });
                }
            }
            let area_m2 = surface.cells()[index].area.get();
            let adjustment = (removed_mass_kg[index] - deposited_mass_kg[index])
                / (FORMATION_AIRY_MANTLE_DENSITY_KG_M3 * area_m2);
            // The owning process yields at the publishable safety bound (P5
            // design §5): the retained response is reduced so a column already
            // at the bound does not leave the range; the mass ledger is not
            // an elevation and stays exact.
            let retained =
                (adjustment as f32).clamp(ELEVATION_MIN_M - base, ELEVATION_MAX_M - base);
            let final_elevation = formation_elevation_from_components(
                base, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, retained,
            );
            if !final_elevation.is_finite()
                || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&final_elevation)
            {
                return Err(IsostasyGenerationError::ElevationOutOfRange {
                    cell,
                    found: f64::from(final_elevation),
                });
            }
            response.push(retained);
            result.push(final_elevation);
        }
        check_cancelled(cancellation)?;
        Ok(IsostaticAdjustmentStep {
            isostatic_response_m: response,
            elevation_m: result,
        })
    }
}

/// Fixed-water-volume terrain classification returned after each P5 candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct FormationWaterState {
    sea_level_m: f32,
    realized_water_volume_m3: f64,
    relative_error: f64,
    land_ocean: LandOceanField,
}

impl FormationWaterState {
    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }

    pub const fn realized_water_volume_m3(&self) -> f64 {
        self.realized_water_volume_m3
    }

    pub const fn relative_error(&self) -> f64 {
        self.relative_error
    }

    pub const fn land_ocean(&self) -> &LandOceanField {
        &self.land_ocean
    }
}

/// Exact piecewise-linear water solve; it never targets an authored land share.
#[derive(Debug, Clone, Copy, Default)]
pub struct FormationSeaLevelSolver;

impl FormationSeaLevelSolver {
    pub fn solve(
        surface: &SphericalSurfaceSnapshot,
        elevation_m: &[f32],
        water_inventory_m3: f64,
        cancellation: &BuildCancellation,
    ) -> Result<FormationWaterState, IsostasyGenerationError> {
        check_cancelled(cancellation)?;
        surface
            .validate_cancellable(&|| cancellation.is_cancelled())
            .map_err(|error| map_surface_error(error, cancellation))?;
        Self::solve_from_validated_surface(surface, elevation_m, water_inventory_m3, cancellation)
    }

    /// Same physical solve for a caller that already validated the surface.
    pub(super) fn solve_from_validated_surface(
        surface: &SphericalSurfaceSnapshot,
        elevation_m: &[f32],
        water_inventory_m3: f64,
        cancellation: &BuildCancellation,
    ) -> Result<FormationWaterState, IsostasyGenerationError> {
        check_cancelled(cancellation)?;
        if elevation_m.len() != surface.cells().len() {
            return Err(IsostasyGenerationError::CellCountMismatch {
                field: "elevation_m",
                expected: surface.cells().len(),
                found: elevation_m.len(),
            });
        }
        let solution = solve_physical_sea_level_cancellable(
            surface,
            elevation_m,
            water_inventory_m3,
            cancellation,
        )
        .map_err(|error| match error {
            WaterVolumeSolveError::Cancelled => IsostasyGenerationError::Cancelled,
            other => IsostasyGenerationError::WaterSolve(other),
        })?;
        check_cancelled(cancellation)?;
        Ok(FormationWaterState {
            sea_level_m: solution.sea_level_m(),
            realized_water_volume_m3: solution.realized_water_volume_m3(),
            relative_error: solution.relative_error(),
            land_ocean: solution.geometry().land_ocean().clone(),
        })
    }
}

fn poll_cancelled(
    cancellation: &BuildCancellation,
    index: usize,
) -> Result<(), IsostasyGenerationError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), IsostasyGenerationError> {
    if cancellation.is_cancelled() {
        Err(IsostasyGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_surface_error(
    error: SphericalSurfaceValidationError,
    cancellation: &BuildCancellation,
) -> IsostasyGenerationError {
    if cancellation.is_cancelled() {
        IsostasyGenerationError::Cancelled
    } else {
        IsostasyGenerationError::InvalidSurface(error)
    }
}

#[derive(Debug, Error)]
pub enum IsostasyGenerationError {
    #[error("surface-formation isostasy/sea-level solve cancelled")]
    Cancelled,
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("isostasy field {field} has length {found}; expected {expected}")]
    CellCountMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("isostasy field {field} has invalid value {found} at {cell:?}")]
    InvalidCellValue {
        field: &'static str,
        cell: CellId,
        found: f64,
    },
    #[error("isostatic elevation at {cell:?} is outside the supported range: {found}")]
    ElevationOutOfRange { cell: CellId, found: f64 },
    #[error("fixed-water sea-level solve failed: {0}")]
    WaterSolve(#[from] WaterVolumeSolveError),
}
