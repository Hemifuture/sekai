use std::f64::consts::PI;

use rand::RngCore;
use thiserror::Error;

use super::mantle::{generate_mantle_fields, resolve_mantle_profile, MantleGenerator};
use super::random::{LabeledSubstreams, HOTSPOT_SEEDS_LABEL};
use super::topology::{farthest_point_seeds, NaturalTopologyIndex};
use crate::engine::StageRng;
use crate::world::natural::{
    GeologicSpec, GeologicSpecError, MantleActivity, MantleFormationBias, MantleValidationError,
    SphericalMantleSnapshot, SphericalMantleValidationError, MANTLE_SNAPSHOT_SCHEMA_V2,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError,
};

impl MantleGenerator {
    /// Generates globally distributed mantle forcing on a closed spherical surface.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        spec: &GeologicSpec,
        formation_bias: MantleFormationBias,
        rng: &mut StageRng,
    ) -> Result<SphericalMantleSnapshot, SphericalMantleGenerationError> {
        validate_inputs(surface, spec, formation_bias)?;
        let streams = LabeledSubstreams::capture(rng);
        Self::generate_spherical_from_streams(surface, spec, formation_bias, &streams)
    }

    pub(in crate::generators::natural) fn generate_spherical_from_streams(
        surface: &SphericalSurfaceSnapshot,
        spec: &GeologicSpec,
        formation_bias: MantleFormationBias,
        streams: &LabeledSubstreams,
    ) -> Result<SphericalMantleSnapshot, SphericalMantleGenerationError> {
        let (hotspot_count, mantle_activity) = validate_inputs(surface, spec, formation_bias)?;
        let view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        let mut seed_rng = streams.stream(HOTSPOT_SEEDS_LABEL);
        let sources =
            farthest_point_seeds(&topology, usize::from(hotspot_count), seed_rng.next_u64());
        let fields = generate_mantle_fields(
            &topology,
            sources,
            mantle_activity,
            PI * surface.radius().get(),
            streams,
        )?;
        let snapshot = SphericalMantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V2,
            view.surface_ref(),
            fields.hotspots,
            fields.heat_flow_mw_m2,
            fields.volcanic_influence,
        )?;
        snapshot.validate_against_validated_surface(surface)?;
        Ok(snapshot)
    }
}

fn validate_inputs(
    surface: &SphericalSurfaceSnapshot,
    spec: &GeologicSpec,
    formation_bias: MantleFormationBias,
) -> Result<(u16, MantleActivity), SphericalMantleGenerationError> {
    spec.validate()?;
    surface.validate()?;
    let (hotspot_count, mantle_activity) = resolve_mantle_profile(spec, formation_bias);
    if usize::from(hotspot_count) > surface.cells().len() {
        return Err(SphericalMantleGenerationError::HotspotCountExceedsCells {
            hotspots: hotspot_count,
            cells: surface.cells().len(),
        });
    }
    Ok((hotspot_count, mantle_activity))
}

/// Errors returned while generating surface-bound spherical mantle forcing.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalMantleGenerationError {
    /// The supplied geologic specification is invalid.
    #[error("invalid geologic specification: {0}")]
    InvalidSpec(#[from] GeologicSpecError),
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The authoritative spherical surface identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The requested hotspot cardinality exceeds the surface allocation.
    #[error("hotspot count {hotspots} exceeds spherical surface cell count {cells}")]
    HotspotCountExceedsCells {
        /// The requested hotspot count.
        hotspots: u16,
        /// The available surface cell count.
        cells: usize,
    },
    /// A generated hotspot violated the reused primitive contract.
    #[error("generated hotspot is invalid: {0}")]
    InvalidGeneratedHotspot(#[from] MantleValidationError),
    /// Generated spherical mantle forcing failed its immutable domain contract.
    #[error("generated spherical mantle snapshot is invalid: {0}")]
    InvalidSnapshot(#[from] SphericalMantleValidationError),
}
