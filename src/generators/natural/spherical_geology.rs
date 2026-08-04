use thiserror::Error;

use super::geology::{boundary_influences, synthesize_geologic_fields, GeologicGenerator};
use super::random::LabeledSubstreams;
use super::topology::NaturalTopologyIndex;
use crate::engine::StageRng;
use crate::world::natural::{
    BedrockKindField, GeologicSpec, GeologicSpecError, SphericalGeologicSnapshot,
    SphericalGeologicValidationError, SphericalMantleSnapshot, SphericalMantleValidationError,
    SphericalReliefSnapshot, SphericalReliefValidationError, SphericalTectonicSnapshot,
    SphericalTectonicValidationError, GEOLOGIC_SNAPSHOT_SCHEMA_V2,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError,
};

impl GeologicGenerator {
    /// Generates present-day geologic material fields on a closed spherical surface.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        tectonic: &SphericalTectonicSnapshot,
        mantle: &SphericalMantleSnapshot,
        relief: &SphericalReliefSnapshot,
        spec: &GeologicSpec,
        rng: &mut StageRng,
    ) -> Result<SphericalGeologicSnapshot, SphericalGeologicGenerationError> {
        spec.validate()?;
        surface.validate()?;
        tectonic.validate_against_validated_surface(surface)?;
        mantle.validate_against_validated_surface(surface)?;
        relief.validate_against_validated_surface(surface)?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        let boundary =
            boundary_influences(&topology, tectonic.cell_plates(), tectonic.boundaries());
        let streams = LabeledSubstreams::capture(rng);
        let fields = synthesize_geologic_fields(
            &topology,
            &boundary,
            tectonic.crust_kinds(),
            mantle.volcanic_influence(),
            mantle.heat_flow_mw_m2(),
            relief.tectonic_offset_m().values(),
            relief.elevation_m().values(),
            &streams,
        );
        let snapshot = SphericalGeologicSnapshot::new(
            GEOLOGIC_SNAPSHOT_SCHEMA_V2,
            view.surface_ref(),
            BedrockKindField::from_kinds(fields.bedrock),
            fields.fracture,
            fields.resistance,
            fields.permeability,
            fields.metallic,
            fields.geothermal,
            fields.sedimentary,
        )?;
        snapshot.validate_against_validated_surface(surface, tectonic)?;
        Ok(snapshot)
    }
}

/// Errors returned while generating surface-bound spherical geology.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalGeologicGenerationError {
    /// The supplied geologic specification is invalid.
    #[error("invalid geologic specification: {0}")]
    InvalidSpec(#[from] GeologicSpecError),
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The surface identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The tectonic upstream is incompatible with the surface.
    #[error("invalid spherical tectonic input: {0}")]
    InvalidTectonics(#[from] SphericalTectonicValidationError),
    /// The mantle upstream is incompatible with the surface.
    #[error("invalid spherical mantle input: {0}")]
    InvalidMantle(#[from] SphericalMantleValidationError),
    /// The relief upstream is incompatible with the surface.
    #[error("invalid spherical relief input: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
    /// The completed surface-bound snapshot violated its V2 contract.
    #[error("invalid generated spherical geology snapshot: {0}")]
    InvalidSnapshot(#[from] SphericalGeologicValidationError),
}
