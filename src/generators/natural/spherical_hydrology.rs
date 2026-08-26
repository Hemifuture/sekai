use thiserror::Error;

use super::hydrology::{
    generate_formation_hydrology_core, generate_hydrology_core, DrainageOutletPolicy,
    HydrologyGenerationError, HydrologyGenerator,
};
use super::topology::NaturalTopologyIndex;
use crate::engine::BuildCancellation;
use crate::world::natural::{
    ElevationField, HydroErosionSpec, HydroErosionSpecError, LandOceanField,
    SphericalClimateValidationError, SphericalGeologicSnapshot, SphericalGeologicValidationError,
    SphericalHydrologySnapshot, SphericalHydrologyValidationError,
    SphericalPreliminaryClimateSnapshot, SphericalReliefSnapshot, SphericalReliefValidationError,
    HYDROLOGY_SCHEMA_V2,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRef, SurfaceRefError,
};

impl HydrologyGenerator {
    /// Generates constructional-surface hydrology directly on a closed sphere.
    ///
    /// The atomic hydro-erosion generator uses the same core again on the final
    /// current surface; only that second pass is published by the product path.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
        geology: &SphericalGeologicSnapshot,
        climate: &SphericalPreliminaryClimateSnapshot,
        spec: &HydroErosionSpec,
    ) -> Result<SphericalHydrologySnapshot, SphericalHydrologyGenerationError> {
        spec.validate()?;
        surface.validate()?;
        relief.validate_against_validated_surface(surface)?;
        geology.validate()?;
        climate.validate_against_validated_surface(surface, relief)?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        validate_upstream_surface("geology", geology.surface_ref(), view.surface_ref())?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        generate_spherical_from_validated_inputs(
            surface,
            &view,
            &topology,
            relief.elevation_m(),
            relief.sea_level_m(),
            geology.relative_permeability(),
            climate.monthly_precipitation_mm().values(),
            spec,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_spherical_from_validated_inputs(
    surface_snapshot: &SphericalSurfaceSnapshot,
    surface: &SphericalNaturalSurface<'_>,
    topology: &NaturalTopologyIndex,
    surface_elevation_m: &ElevationField,
    sea_level_m: f32,
    relative_permeability: &[f32],
    monthly_precipitation_mm: &[[f32; crate::world::natural::CLIMATE_MONTH_COUNT]],
    spec: &HydroErosionSpec,
) -> Result<SphericalHydrologySnapshot, SphericalHydrologyGenerationError> {
    let hydrology = generate_hydrology_core(
        surface,
        topology,
        surface_elevation_m,
        sea_level_m,
        relative_permeability,
        monthly_precipitation_mm,
        spec,
        DrainageOutletPolicy::ClosedLocalMinima,
    )?;
    wrap_spherical_hydrology(surface_snapshot, surface, topology, hydrology)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_formation_spherical_from_validated_inputs(
    surface_snapshot: &SphericalSurfaceSnapshot,
    surface: &SphericalNaturalSurface<'_>,
    topology: &NaturalTopologyIndex,
    surface_elevation_m: &[f64],
    land_ocean: &LandOceanField,
    relative_permeability: &[f32],
    monthly_precipitation_mm_day: &[[f32; crate::world::natural::CLIMATE_MONTH_COUNT]],
    spec: &HydroErosionSpec,
    cancellation: &BuildCancellation,
) -> Result<SphericalHydrologySnapshot, SphericalHydrologyGenerationError> {
    let hydrology = generate_formation_hydrology_core(
        surface,
        topology,
        surface_elevation_m,
        land_ocean,
        relative_permeability,
        monthly_precipitation_mm_day,
        spec,
        cancellation,
    )?;
    wrap_spherical_hydrology(surface_snapshot, surface, topology, hydrology)
}

fn wrap_spherical_hydrology(
    surface_snapshot: &SphericalSurfaceSnapshot,
    surface: &SphericalNaturalSurface<'_>,
    topology: &NaturalTopologyIndex,
    hydrology: crate::world::natural::HydrologySnapshot,
) -> Result<SphericalHydrologySnapshot, SphericalHydrologyGenerationError> {
    let river_segment_length_m = hydrology
        .river_segments()
        .iter()
        .map(|segment| {
            let edge = topology
                .edge_between(segment.from(), segment.to())
                .expect("generated river receivers are authoritative neighbor edges");
            surface
                .edge(edge)
                .and_then(|edge| edge.center_distance())
                .expect("closed spherical neighbor edges have a center distance")
                .get()
        })
        .collect();
    let snapshot = SphericalHydrologySnapshot::new(
        HYDROLOGY_SCHEMA_V2,
        surface.surface_ref(),
        hydrology,
        river_segment_length_m,
    )?;
    snapshot.validate_against_validated_surface(surface_snapshot)?;
    Ok(snapshot)
}

fn validate_upstream_surface(
    input: &'static str,
    found: SurfaceRef,
    expected: SurfaceRef,
) -> Result<(), SphericalHydrologyGenerationError> {
    if found != expected {
        return Err(SphericalHydrologyGenerationError::UpstreamSurfaceMismatch {
            input,
            found,
            expected,
        });
    }
    Ok(())
}

/// Errors returned while generating hydrology on an authoritative closed sphere.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalHydrologyGenerationError {
    /// The shared hydro-erosion specification is invalid.
    #[error("invalid hydro-erosion specification: {0}")]
    InvalidSpec(#[from] HydroErosionSpecError),
    /// The authoritative surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The authoritative surface identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The constructional relief is invalid or references another surface.
    #[error("invalid spherical relief input: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
    /// The geologic substrate is invalid.
    #[error("invalid spherical geology input: {0}")]
    InvalidGeology(#[from] SphericalGeologicValidationError),
    /// The preliminary climate is invalid or references another surface/relief.
    #[error("invalid spherical climate input: {0}")]
    InvalidClimate(#[from] SphericalClimateValidationError),
    /// A self-validating upstream snapshot belongs to another exact surface.
    #[error("{input} surface {found:?} does not match authoritative surface {expected:?}")]
    UpstreamSurfaceMismatch {
        input: &'static str,
        found: SurfaceRef,
        expected: SurfaceRef,
    },
    /// The shared deterministic hydrology core failed.
    #[error("spherical hydrology solve failed: {0}")]
    Solve(#[from] HydrologyGenerationError),
    /// The generated V2 snapshot violated its surface-bound contract.
    #[error("invalid generated spherical hydrology: {0}")]
    InvalidSnapshot(#[from] SphericalHydrologyValidationError),
}
