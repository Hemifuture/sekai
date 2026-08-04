//! Typed engine adapters for surface-bound spherical mantle and geology data.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    MantleGenerator, ResolvedGeologicInputArtifact, ResolvedWorldFormationArtifact,
    SphericalMantleGenerationError,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::rules::GeologicModel;
use crate::world::natural::SphericalMantleSnapshot;

const INVALID_INPUT_CODE: &str = "spherical-natural.invalid-mantle-input";
const BUILD_FAILED_CODE: &str = "spherical-natural.mantle-build-failed";
const INVALID_MANTLE_CODE: &str = "spherical-natural.invalid-mantle";

/// Engine transport for one complete spherical mantle snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalMantleArtifact {
    snapshot: SphericalMantleSnapshot,
}

impl SphericalMantleArtifact {
    /// Wraps a locally valid spherical mantle snapshot.
    pub const fn new(snapshot: SphericalMantleSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the immutable surface-bound snapshot.
    pub const fn snapshot(&self) -> &SphericalMantleSnapshot {
        &self.snapshot
    }
}

impl Artifact for SphericalMantleArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spherical-mantle");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_MANTLE_CODE, error.to_string()))
    }
}

/// The exact typed inputs visible to [`SphericalMantleStage`].
pub struct SphericalMantleStageInputs {
    formation: Arc<ResolvedWorldFormationArtifact>,
    resolved_input: Arc<ResolvedGeologicInputArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for SphericalMantleStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedGeologicInputArtifact::KEY,
            ResolvedWorldFormationArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            formation: artifacts.get::<ResolvedWorldFormationArtifact>()?,
            resolved_input: artifacts.get::<ResolvedGeologicInputArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

/// Deterministic adapter for the frozen spherical mantle scientific stream.
#[derive(Debug, Clone, Copy, Default)]
pub struct SphericalMantleStage;

impl Stage for SphericalMantleStage {
    type Inputs = SphericalMantleStageInputs;
    type Output = SphericalMantleArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.spherical-mantle")
    }

    fn version(&self) -> u32 {
        1
    }

    fn namespace(&self) -> &'static str {
        "sekai.core"
    }

    fn run(
        &self,
        inputs: Self::Inputs,
        rng: &mut StageRng,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        inputs
            .resolved_input
            .input()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .formation
            .formation()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .surface
            .snapshot()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;

        let snapshot = match inputs.resolved_input.input().model() {
            GeologicModel::CurrentSliceV1 => MantleGenerator::generate_spherical(
                inputs.surface.snapshot(),
                inputs.resolved_input.input().spec(),
                inputs.formation.formation().mantle_bias(),
                rng,
            ),
        }
        .map_err(generation_failure)?;
        snapshot
            .validate_against(inputs.surface.snapshot())
            .map_err(|error| invalid_mantle(error.to_string()))?;
        Ok(SphericalMantleArtifact::new(snapshot))
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn generation_failure(error: SphericalMantleGenerationError) -> StageError {
    match error {
        SphericalMantleGenerationError::InvalidSpec(_)
        | SphericalMantleGenerationError::InvalidSurface(_)
        | SphericalMantleGenerationError::InvalidSurfaceIdentity(_)
        | SphericalMantleGenerationError::HotspotCountExceedsCells { .. } => {
            invalid_input(error.to_string())
        }
        SphericalMantleGenerationError::InvalidGeneratedHotspot(_) => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
        SphericalMantleGenerationError::InvalidSnapshot(_) => invalid_mantle(error.to_string()),
    }
}

fn invalid_mantle(message: String) -> StageError {
    StageError::new(INVALID_MANTLE_CODE, message)
}
