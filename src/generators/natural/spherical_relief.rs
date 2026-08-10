use thiserror::Error;

use super::random::{LabeledSubstreams, RELIEF_HOTSPOT_MORPHOLOGY_LABEL};
use super::relief::{reconcile_final_safety, ReliefGenerator, SEA_LEVEL_M};
use super::spherical_island_relief::synthesize_spherical_hotspot_offset;
use crate::engine::{Diagnostic, StageRng};
use crate::world::natural::{
    ElevationField, LandOceanField, ReliefValidationError, SphericalMantleSnapshot,
    SphericalMantleValidationError, SphericalReliefSnapshot, SphericalReliefValidationError,
    SphericalTectonicSnapshot, SphericalTectonicValidationError, RELIEF_SCHEMA_V4,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError,
};

mod directed_noise;
mod tectonic_heightmap;

use tectonic_heightmap::{build_tectonic_heightmap, TectonicHeightmapError};

impl ReliefGenerator {
    /// Generates explainable V4 relief directly on a closed spherical surface.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        tectonic: &SphericalTectonicSnapshot,
        mantle: &SphericalMantleSnapshot,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<SphericalReliefSnapshot, SphericalReliefGenerationError> {
        surface.validate()?;
        tectonic.validate_against_validated_surface(surface)?;
        mantle.validate_against_validated_surface(surface)?;

        let view = SphericalNaturalSurface::from_validated(surface)?;
        let streams = LabeledSubstreams::capture(rng);
        let components = build_tectonic_heightmap(surface, tectonic, &streams)?;
        let mut crust_base = components.crust_base_m;
        let mut tectonic_offset = components.tectonic_offset_m;

        use rand::RngCore as _;
        let mut hotspot_rng = streams.stream(RELIEF_HOTSPOT_MORPHOLOGY_LABEL);
        let mut volcanic_offset =
            synthesize_spherical_hotspot_offset(surface, tectonic, mantle, hotspot_rng.next_u32());
        let mut regional_offset = components.directed_detail_m;
        let elevation = reconcile_final_safety(
            &mut crust_base,
            &mut tectonic_offset,
            &mut volcanic_offset,
            &mut regional_offset,
            diagnostics,
        );

        let crust_base = ElevationField::from_values(crust_base)?;
        let tectonic_offset = ElevationField::from_values(tectonic_offset)?;
        let volcanic_offset = ElevationField::from_values(volcanic_offset)?;
        let regional_offset = ElevationField::from_values(regional_offset)?;
        let elevation = ElevationField::from_values(elevation)?;
        let land_ocean = LandOceanField::classify(&elevation, SEA_LEVEL_M);
        let snapshot = SphericalReliefSnapshot::new(
            RELIEF_SCHEMA_V4,
            view.surface_ref(),
            SEA_LEVEL_M,
            crust_base,
            tectonic_offset,
            volcanic_offset,
            regional_offset,
            elevation,
            land_ocean,
        )?;
        snapshot.validate_against_validated_surface(surface)?;
        Ok(snapshot)
    }
}

/// Errors returned while generating surface-bound spherical relief.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalReliefGenerationError {
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The surface identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The supplied tectonic snapshot is incompatible with the surface.
    #[error("invalid spherical tectonic input: {0}")]
    InvalidTectonics(#[from] SphericalTectonicValidationError),
    /// The supplied mantle snapshot is incompatible with the surface.
    #[error("invalid spherical mantle input: {0}")]
    InvalidMantle(#[from] SphericalMantleValidationError),
    /// Current crust attributes could not be converted into bounded height components.
    #[error("invalid tectonic heightmap input: {message}")]
    InvalidHeightmap { message: String },
    /// A generated dense field violated the shared relief semantics.
    #[error("invalid generated relief field: {0}")]
    InvalidReliefField(#[from] ReliefValidationError),
    /// The completed surface-bound snapshot violated its V4 contract.
    #[error("invalid generated spherical relief snapshot: {0}")]
    InvalidSnapshot(#[from] SphericalReliefValidationError),
}

impl From<TectonicHeightmapError> for SphericalReliefGenerationError {
    fn from(error: TectonicHeightmapError) -> Self {
        Self::InvalidHeightmap {
            message: error.to_string(),
        }
    }
}
