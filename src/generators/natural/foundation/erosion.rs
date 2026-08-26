use thiserror::Error;

use super::super::erosion::{
    generate_erosion_core, validate_erosion_semantic_inputs, FluvialErosionError,
    FluvialErosionGenerator,
};
use super::super::topology::NaturalTopologyIndex;
use crate::world::natural::{
    HydroErosionSpec, HydroErosionSpecError, SphericalGeologicSnapshot,
    SphericalGeologicValidationError, SphericalHydrologySnapshot,
    SphericalHydrologyValidationError, SphericalReliefSnapshot, SphericalReliefValidationError,
    SphericalSurfaceProcessSnapshot, SphericalSurfaceProcessValidationError,
    SURFACE_PROCESS_SCHEMA_V2,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRef, SurfaceRefError,
};

impl FluvialErosionGenerator {
    /// Applies one bounded current-state fluvial formation update on a closed sphere.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
        geology: &SphericalGeologicSnapshot,
        hydrology: &SphericalHydrologySnapshot,
        spec: &HydroErosionSpec,
    ) -> Result<SphericalSurfaceProcessSnapshot, SphericalFluvialErosionError> {
        spec.validate()?;
        surface.validate()?;
        relief.validate_against_validated_surface(surface)?;
        geology.validate()?;
        hydrology.validate_against_validated_surface(surface)?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        validate_upstream_surface("geology", geology.surface_ref(), view.surface_ref())?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        generate_spherical_from_validated_inputs(
            surface, &view, &topology, relief, geology, hydrology, spec,
        )
    }
}

pub(in crate::generators::natural) fn generate_spherical_from_validated_inputs(
    surface_snapshot: &SphericalSurfaceSnapshot,
    surface: &SphericalNaturalSurface<'_>,
    topology: &NaturalTopologyIndex,
    relief: &SphericalReliefSnapshot,
    geology: &SphericalGeologicSnapshot,
    hydrology: &SphericalHydrologySnapshot,
    spec: &HydroErosionSpec,
) -> Result<SphericalSurfaceProcessSnapshot, SphericalFluvialErosionError> {
    validate_erosion_semantic_inputs(
        surface.cell_count(),
        relief.land_ocean(),
        geology.erosion_resistance(),
        hydrology.semantic_payload(),
    )?;
    let output = generate_erosion_core(
        surface,
        topology,
        relief.elevation_m(),
        relief.land_ocean(),
        geology.erosion_resistance(),
        hydrology.semantic_payload(),
        spec,
    )?;
    let snapshot = SphericalSurfaceProcessSnapshot::new(
        SURFACE_PROCESS_SCHEMA_V2,
        surface.surface_ref(),
        output.erosion_depth_m,
        output.deposition_thickness_m,
        output.surface_elevation_m,
        output.sediment_throughput_m3,
        output.sediment_ocean_delivery_m3,
        output.sediment_endorheic_storage_m3,
    )?;
    snapshot.validate_against_validated_surface(surface_snapshot, relief)?;
    Ok(snapshot)
}

fn validate_upstream_surface(
    input: &'static str,
    found: SurfaceRef,
    expected: SurfaceRef,
) -> Result<(), SphericalFluvialErosionError> {
    if found != expected {
        return Err(SphericalFluvialErosionError::UpstreamSurfaceMismatch {
            input,
            found,
            expected,
        });
    }
    Ok(())
}

/// Errors returned while applying bounded fluvial formation on a closed sphere.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalFluvialErosionError {
    /// The shared controls are invalid.
    #[error("invalid hydro-erosion specification: {0}")]
    InvalidSpec(#[from] HydroErosionSpecError),
    /// The authoritative sphere is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The exact surface identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// Constructional relief is invalid or incompatible.
    #[error("invalid spherical relief input: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
    /// Geologic substrate is invalid.
    #[error("invalid spherical geology input: {0}")]
    InvalidGeology(#[from] SphericalGeologicValidationError),
    /// First-pass hydrology is invalid or incompatible.
    #[error("invalid spherical hydrology input: {0}")]
    InvalidHydrology(#[from] SphericalHydrologyValidationError),
    /// A self-validating upstream belongs to another exact surface.
    #[error("{input} surface {found:?} does not match authoritative surface {expected:?}")]
    UpstreamSurfaceMismatch {
        input: &'static str,
        found: SurfaceRef,
        expected: SurfaceRef,
    },
    /// The shared erosion or sediment-routing core failed.
    #[error("spherical fluvial formation failed: {0}")]
    Formation(#[from] FluvialErosionError),
    /// The generated surface-process V2 snapshot is invalid.
    #[error("invalid generated spherical surface process: {0}")]
    InvalidSnapshot(#[from] SphericalSurfaceProcessValidationError),
}
