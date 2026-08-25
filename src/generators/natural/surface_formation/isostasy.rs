use thiserror::Error;

use super::super::surface_water_geometry::solve_physical_sea_level_cancellable;
use crate::engine::BuildCancellation;
use crate::world::natural::{
    formation_elevation_from_components, WaterVolumeSolution, WaterVolumeSolveError,
    ELEVATION_MAX_M, ELEVATION_MIN_M, FORMATION_AIRY_MANTLE_DENSITY_KG_M3,
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
        if elevation_m.len() != count {
            return Err(IsostasyGenerationError::CellCountMismatch {
                field: "elevation_m",
                expected: count,
                found: elevation_m.len(),
            });
        }
        let exact_response = Self::response_from_validated_surface(
            surface,
            removed_mass_kg,
            deposited_mass_kg,
            cancellation,
        )?;
        let mut response = Vec::with_capacity(count);
        let mut result = Vec::with_capacity(count);
        for (index, &adjustment) in exact_response.iter().enumerate() {
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
            let unquantized_elevation = f64::from(base) + adjustment;
            if !unquantized_elevation.is_finite()
                || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M))
                    .contains(&unquantized_elevation)
            {
                return Err(IsostasyGenerationError::ElevationOutOfRange {
                    cell,
                    found: unquantized_elevation,
                });
            }
            let retained = adjustment as f32;
            let final_elevation = formation_elevation_from_components(base, retained);
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

    /// Computes the exact local response without binding it to a quantized
    /// working elevation. The coupled solver owns the complete component state
    /// and validates the composed candidate there.
    pub(super) fn response_from_validated_surface(
        surface: &SphericalSurfaceSnapshot,
        removed_mass_kg: &[f64],
        deposited_mass_kg: &[f64],
        cancellation: &BuildCancellation,
    ) -> Result<Vec<f64>, IsostasyGenerationError> {
        check_cancelled(cancellation)?;
        let count = surface.cells().len();
        for (field, found) in [
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
        for index in 0..count {
            poll_cancelled(cancellation, index)?;
            let cell = CellId::from_raw(index as u32);
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
            response.push(adjustment);
        }
        check_cancelled(cancellation)?;
        Ok(response)
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
    ) -> Result<WaterVolumeSolution, IsostasyGenerationError> {
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
    ) -> Result<WaterVolumeSolution, IsostasyGenerationError> {
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
        Ok(solution)
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
