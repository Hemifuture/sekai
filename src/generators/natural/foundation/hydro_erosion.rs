use thiserror::Error;

use super::super::hydro_erosion::HydroErosionGenerator;
use super::super::topology::NaturalTopologyIndex;
use super::erosion::{
    generate_spherical_from_validated_inputs as generate_erosion, SphericalFluvialErosionError,
};
use super::hydrology::{
    generate_spherical_from_validated_inputs as generate_hydrology,
    SphericalHydrologyGenerationError,
};
use crate::world::natural::{
    HydroErosionSpec, HydroErosionSpecError, SphericalClimateValidationError,
    SphericalGeologicSnapshot, SphericalGeologicValidationError, SphericalHydroErosionSnapshot,
    SphericalHydroErosionValidationError, SphericalPreliminaryClimateSnapshot,
    SphericalReliefSnapshot, SphericalReliefValidationError, HYDRO_EROSION_SNAPSHOT_SCHEMA_V2,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRef, SurfaceRefError,
};

impl HydroErosionGenerator {
    /// Runs initial hydrology, bounded fluvial formation, and final hydrology on one closed sphere.
    ///
    /// The authoritative topology is indexed once and reused by all three solves. Initial
    /// hydrology is an implementation detail; only the mutually consistent current surface and
    /// recomputed final hydrology are returned.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
        geology: &SphericalGeologicSnapshot,
        climate: &SphericalPreliminaryClimateSnapshot,
        spec: &HydroErosionSpec,
    ) -> Result<SphericalHydroErosionSnapshot, SphericalHydroErosionGenerationError> {
        spec.validate()?;
        surface.validate()?;
        relief.validate_against_validated_surface(surface)?;
        geology.validate()?;
        climate.validate_against_validated_surface(surface, relief)?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        validate_upstream_surface("geology", geology.surface_ref(), view.surface_ref())?;
        let topology = NaturalTopologyIndex::from_surface(&view);

        let initial_hydrology = generate_hydrology(
            surface,
            &view,
            &topology,
            relief.elevation_m(),
            relief.sea_level_m(),
            geology.relative_permeability(),
            climate.monthly_precipitation_mm().values(),
            spec,
        )
        .map_err(SphericalHydroErosionGenerationError::InitialHydrology)?;
        let current_surface = generate_erosion(
            surface,
            &view,
            &topology,
            relief,
            geology,
            &initial_hydrology,
            spec,
        )?;
        let final_hydrology = generate_hydrology(
            surface,
            &view,
            &topology,
            current_surface.surface_elevation_m(),
            relief.sea_level_m(),
            geology.relative_permeability(),
            climate.monthly_precipitation_mm().values(),
            spec,
        )
        .map_err(SphericalHydroErosionGenerationError::FinalHydrology)?;

        let snapshot = SphericalHydroErosionSnapshot::new(
            HYDRO_EROSION_SNAPSHOT_SCHEMA_V2,
            current_surface,
            final_hydrology,
        )?;
        snapshot.validate_relations_against_validated_inputs(
            view.surface_ref(),
            relief,
            geology,
            climate,
        )?;
        Ok(snapshot)
    }
}

fn validate_upstream_surface(
    input: &'static str,
    found: SurfaceRef,
    expected: SurfaceRef,
) -> Result<(), SphericalHydroErosionGenerationError> {
    if found != expected {
        return Err(
            SphericalHydroErosionGenerationError::UpstreamSurfaceMismatch {
                input,
                found,
                expected,
            },
        );
    }
    Ok(())
}

/// Errors returned by the fixed one-index, two-hydrology-pass spherical solve.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalHydroErosionGenerationError {
    /// The shared hydro-erosion controls are invalid.
    #[error("invalid hydro-erosion specification: {0}")]
    InvalidSpec(#[from] HydroErosionSpecError),
    /// The authoritative sphere is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The exact authoritative identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// Constructional relief is invalid or references another surface.
    #[error("invalid spherical relief input: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
    /// Geologic substrate is invalid.
    #[error("invalid spherical geology input: {0}")]
    InvalidGeology(#[from] SphericalGeologicValidationError),
    /// Preliminary climate is invalid or references another surface/relief.
    #[error("invalid spherical climate input: {0}")]
    InvalidClimate(#[from] SphericalClimateValidationError),
    /// A self-validating upstream belongs to another exact surface.
    #[error("{input} surface {found:?} does not match authoritative surface {expected:?}")]
    UpstreamSurfaceMismatch {
        input: &'static str,
        found: SurfaceRef,
        expected: SurfaceRef,
    },
    /// Constructional-surface hydrology failed.
    #[error("initial spherical hydrology failed: {0}")]
    InitialHydrology(SphericalHydrologyGenerationError),
    /// Bounded erosion, routing, or deposition failed.
    #[error("spherical fluvial formation failed: {0}")]
    Erosion(#[from] SphericalFluvialErosionError),
    /// Current-surface hydrology failed.
    #[error("final spherical hydrology failed: {0}")]
    FinalHydrology(SphericalHydrologyGenerationError),
    /// The atomic output violated an identity or semantic relation.
    #[error("invalid atomic spherical hydro-erosion output: {0}")]
    Composite(#[from] SphericalHydroErosionValidationError),
}
