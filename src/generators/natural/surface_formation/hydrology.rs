use thiserror::Error;

use super::super::spherical_hydrology::{
    generate_formation_spherical_from_validated_inputs, SphericalHydrologyGenerationError,
};
use super::super::topology::NaturalTopologyIndex;
use crate::engine::BuildCancellation;
use crate::world::natural::{
    ElevationField, FormationTerrainFields, GeologicSubstrateSnapshot,
    GeologicSubstrateValidationError, GlobalCirculationSnapshot, GlobalCirculationValidationError,
    HydroErosionSpec, HydroErosionSpecError, SphericalHydrologySnapshot,
    SurfaceFormationValidationError,
};
use crate::world::spatial::{
    SphericalNaturalSurface, SphericalSurfaceSnapshot, SphericalSurfaceValidationError,
    SurfaceRefError,
};

/// Deterministic P5 hydrology driven by the retained formation terrain and P4 rates.
#[derive(Debug, Clone, Copy, Default)]
pub struct FormationHydrologyGenerator;

impl FormationHydrologyGenerator {
    /// Builds the final surface-bound drainage product without publishing an
    /// intermediate Priority-Flood pass.
    pub fn generate(
        surface: &SphericalSurfaceSnapshot,
        terrain: &FormationTerrainFields,
        substrate: &GeologicSubstrateSnapshot,
        climate: &GlobalCirculationSnapshot,
        spec: &HydroErosionSpec,
        cancellation: &BuildCancellation,
    ) -> Result<SphericalHydrologySnapshot, FormationHydrologyGenerationError> {
        check_cancelled(cancellation)?;
        spec.validate()?;
        surface.validate()?;
        check_cancelled(cancellation)?;
        terrain.validate()?;
        check_cancelled(cancellation)?;
        substrate.validate_against_surface(surface)?;
        check_cancelled(cancellation)?;
        climate
            .validate_against_cancellable(surface, &|| cancellation.is_cancelled())
            .map_err(|error| {
                if cancellation.is_cancelled() {
                    FormationHydrologyGenerationError::Cancelled
                } else {
                    FormationHydrologyGenerationError::InvalidClimate(error)
                }
            })?;
        Self::generate_from_validated(surface, terrain, substrate, climate, spec, cancellation)
    }

    /// Same drainage solve for a caller that already validated the surface,
    /// terrain, substrate, climate, and specification in this build.
    pub(super) fn generate_from_validated(
        surface: &SphericalSurfaceSnapshot,
        terrain: &FormationTerrainFields,
        substrate: &GeologicSubstrateSnapshot,
        climate: &GlobalCirculationSnapshot,
        spec: &HydroErosionSpec,
        cancellation: &BuildCancellation,
    ) -> Result<SphericalHydrologySnapshot, FormationHydrologyGenerationError> {
        check_cancelled(cancellation)?;
        let expected = surface.cells().len();
        if terrain.final_elevation_m().len() != expected {
            return Err(FormationHydrologyGenerationError::CellCountMismatch {
                input: "formation_terrain",
                expected,
                found: terrain.final_elevation_m().len(),
            });
        }
        check_cancelled(cancellation)?;

        let elevation = ElevationField::from_values(terrain.final_elevation_m().to_vec())
            .map_err(crate::generators::natural::HydrologyGenerationError::from)
            .map_err(SphericalHydrologyGenerationError::from)
            .map_err(FormationHydrologyGenerationError::Solve)?;
        let surface_view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&surface_view);
        generate_formation_spherical_from_validated_inputs(
            surface,
            &surface_view,
            &topology,
            &elevation,
            terrain.sea_level_m(),
            substrate.relative_permeability(),
            climate.fields().monthly_precipitation_mm_day().values(),
            spec,
            cancellation,
        )
        .map_err(|error| match error {
            SphericalHydrologyGenerationError::Solve(
                crate::generators::natural::HydrologyGenerationError::Cancelled,
            ) => FormationHydrologyGenerationError::Cancelled,
            other => FormationHydrologyGenerationError::Solve(other),
        })
    }
}

fn check_cancelled(
    cancellation: &BuildCancellation,
) -> Result<(), FormationHydrologyGenerationError> {
    if cancellation.is_cancelled() {
        Err(FormationHydrologyGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

/// Failures from the typed P5 hydrology boundary.
#[derive(Debug, Error)]
pub enum FormationHydrologyGenerationError {
    /// Cooperative cancellation interrupted validation or dense solve work.
    #[error("surface-formation hydrology cancelled")]
    Cancelled,
    /// The shared hydrology specification is invalid.
    #[error("invalid hydro-erosion specification: {0}")]
    InvalidSpec(#[from] HydroErosionSpecError),
    /// The authoritative sphere is invalid.
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The retained formation terrain is invalid.
    #[error("invalid formation terrain: {0}")]
    InvalidTerrain(#[from] SurfaceFormationValidationError),
    /// The P3 substrate is invalid or belongs to another surface.
    #[error("invalid geologic substrate: {0}")]
    InvalidSubstrate(#[from] GeologicSubstrateValidationError),
    /// The final P4 climate is invalid or belongs to another surface.
    #[error("invalid global circulation: {0}")]
    InvalidClimate(GlobalCirculationValidationError),
    /// One typed dense input does not cover the authoritative sphere.
    #[error("input {input} has length {found}; expected {expected}")]
    CellCountMismatch {
        input: &'static str,
        expected: usize,
        found: usize,
    },
    /// The validated surface identity could not be exposed to the graph core.
    #[error("invalid authoritative surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The shared spherical Priority-Flood solve failed.
    #[error("formation hydrology solve failed: {0}")]
    Solve(SphericalHydrologyGenerationError),
}
