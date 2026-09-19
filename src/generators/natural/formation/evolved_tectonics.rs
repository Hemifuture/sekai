//! Conservative V5 tectonic generation on a complete P1 profile surface bundle.

use thiserror::Error;

use super::super::random::LabeledSubstreams;
use crate::engine::StageRng;
use crate::generators::natural::foundation::tectonics::{
    generate_evolved_spherical, generate_evolved_spherical_from_streams,
};
use crate::generators::spatial::ProfileSurfaceBundle;
use crate::world::natural::{
    EvolvedTectonicSnapshot, NaturalSpecError, ResolvedWorldFormation, TectonicSpec,
    WorldFormationSpecError,
};
use crate::world::spatial::SurfaceRef;

/// Generates the conservative, cause-separated V5 tectonic product.
#[derive(Debug, Clone, Copy, Default)]
pub struct EvolvedTectonicGenerator;

impl EvolvedTectonicGenerator {
    /// Evolves only on the retained P1 control surface and publishes through
    /// its exact conservative overlap map.
    pub fn generate(
        bundle: &ProfileSurfaceBundle,
        spec: &TectonicSpec,
        formation: &ResolvedWorldFormation,
        rng: &mut StageRng,
    ) -> Result<EvolvedTectonicSnapshot, EvolvedTectonicGenerationError> {
        validate_inputs(bundle, spec, formation)?;
        if rng.is_cancelled() {
            return Err(EvolvedTectonicGenerationError::Cancelled);
        }
        match generate_evolved_spherical(bundle, spec, formation, rng) {
            Ok(snapshot) => Ok(snapshot),
            Err(_) if rng.is_cancelled() => Err(EvolvedTectonicGenerationError::Cancelled),
            Err(error) => Err(EvolvedTectonicGenerationError::Generation(
                error.to_string(),
            )),
        }
    }

    /// Generates P2 from a coordinator-owned labeled random root.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::generators::natural) fn generate_from_streams(
        bundle: &ProfileSurfaceBundle,
        spec: &TectonicSpec,
        formation: &ResolvedWorldFormation,
        streams: &LabeledSubstreams,
    ) -> Result<EvolvedTectonicSnapshot, EvolvedTectonicGenerationError> {
        validate_inputs(bundle, spec, formation)?;
        streams
            .check_cancelled()
            .map_err(|_| EvolvedTectonicGenerationError::Cancelled)?;
        match generate_evolved_spherical_from_streams(bundle, spec, formation, streams) {
            Ok(snapshot) => Ok(snapshot),
            Err(_) if streams.check_cancelled().is_err() => {
                Err(EvolvedTectonicGenerationError::Cancelled)
            }
            Err(error) => Err(EvolvedTectonicGenerationError::Generation(
                error.to_string(),
            )),
        }
    }
}

fn validate_inputs(
    bundle: &ProfileSurfaceBundle,
    spec: &TectonicSpec,
    formation: &ResolvedWorldFormation,
) -> Result<(), EvolvedTectonicGenerationError> {
    spec.validate()?;
    formation.validate()?;
    bundle
        .resolution_plan()
        .validate()
        .map_err(|error| EvolvedTectonicGenerationError::InvalidBundle(error.to_string()))?;
    bundle
        .authoritative_surface()
        .validate()
        .map_err(|error| EvolvedTectonicGenerationError::InvalidBundle(error.to_string()))?;
    bundle
        .tectonic_control_surface()
        .validate()
        .map_err(|error| EvolvedTectonicGenerationError::InvalidBundle(error.to_string()))?;
    bundle
        .control_to_authoritative_map()
        .validate()
        .map_err(|error| EvolvedTectonicGenerationError::InvalidBundle(error.to_string()))?;
    let control_ref = SurfaceRef::try_for_spherical(bundle.tectonic_control_surface())
        .map_err(|error| EvolvedTectonicGenerationError::InvalidBundle(error.to_string()))?;
    let authoritative_ref = SurfaceRef::try_for_spherical(bundle.authoritative_surface())
        .map_err(|error| EvolvedTectonicGenerationError::InvalidBundle(error.to_string()))?;
    if bundle.control_to_authoritative_map().source_ref() != control_ref
        || bundle.control_to_authoritative_map().target_ref() != authoritative_ref
    {
        return Err(EvolvedTectonicGenerationError::InvalidBundle(
            "the conservative map is not bound to the supplied control and authoritative surfaces"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Failures returned before a complete V5 artifact can escape publication.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EvolvedTectonicGenerationError {
    /// Cooperative cancellation prevented any partial snapshot publication.
    #[error("evolved tectonic generation was cancelled")]
    Cancelled,
    /// The tectonic specification is invalid.
    #[error("invalid evolved tectonic specification: {0}")]
    InvalidSpec(#[from] NaturalSpecError),
    /// The resolved world-formation selection is invalid.
    #[error("invalid evolved tectonic formation: {0}")]
    InvalidFormation(#[from] WorldFormationSpecError),
    /// The supplied P1 bundle is internally inconsistent.
    #[error("invalid profile-surface bundle: {0}")]
    InvalidBundle(String),
    /// Evolution, remapping, or final validation failed atomically.
    #[error("evolved tectonic generation failed: {0}")]
    Generation(String),
}
