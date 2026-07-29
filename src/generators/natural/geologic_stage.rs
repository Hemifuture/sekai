//! Engine adapters for deterministic current-slice geologic generation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{MantleGenerationError, MantleGenerator, ResolvedGeologicInputArtifact};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SpatialArtifact;
use crate::rules::GeologicModel;
use crate::world::natural::{GeologicSpecError, MantleSnapshot, MantleValidationError};

const INVALID_SPEC_CODE: &str = "natural.invalid-geologic-spec";
const BUILD_FAILED_CODE: &str = "natural.mantle-build-failed";
const INVALID_MANTLE_CODE: &str = "natural.invalid-mantle";

/// Engine transport wrapper for a complete validated mantle snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MantleArtifact {
    snapshot: MantleSnapshot,
}

impl MantleArtifact {
    /// Wraps a complete validated mantle snapshot for engine transport.
    pub const fn new(snapshot: MantleSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the wrapped mantle snapshot.
    pub const fn snapshot(&self) -> &MantleSnapshot {
        &self.snapshot
    }
}

impl Artifact for MantleArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.mantle");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_MANTLE_CODE, error.to_string()))
    }
}

/// Restricted typed dependencies supplied to [`MantleStage`].
pub struct MantleStageInputs {
    resolved_input: Arc<ResolvedGeologicInputArtifact>,
    spatial: Arc<SpatialArtifact>,
}

impl StageInputs for MantleStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[ResolvedGeologicInputArtifact::KEY, SpatialArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolved_input: artifacts.get::<ResolvedGeologicInputArtifact>()?,
            spatial: artifacts.get::<SpatialArtifact>()?,
        })
    }
}

/// Deterministic stage that builds independent present-day mantle forcing.
#[derive(Debug, Clone, Copy, Default)]
pub struct MantleStage;

impl Stage for MantleStage {
    type Inputs = MantleStageInputs;
    type Output = MantleArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.mantle")
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
        let resolved_input = inputs.resolved_input.input();
        resolved_input.spec().validate().map_err(invalid_spec)?;
        let snapshot = match resolved_input.model() {
            GeologicModel::CurrentSliceV1 => {
                MantleGenerator::generate(inputs.spatial.snapshot(), resolved_input.spec(), rng)
            }
        }
        .map_err(generation_failure)?;
        snapshot
            .validate_against(inputs.spatial.snapshot())
            .map_err(invalid_mantle)?;
        Ok(MantleArtifact::new(snapshot))
    }
}

fn invalid_spec(error: GeologicSpecError) -> StageError {
    StageError::new(
        INVALID_SPEC_CODE,
        format!("invalid geologic specification: {error}"),
    )
}

fn generation_failure(error: MantleGenerationError) -> StageError {
    match error {
        MantleGenerationError::InvalidSpec(error) => invalid_spec(error),
        MantleGenerationError::HotspotCountExceedsCells { .. } => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
        MantleGenerationError::InvalidSnapshot(error) => invalid_mantle(error),
    }
}

fn invalid_mantle(error: MantleValidationError) -> StageError {
    StageError::new(
        INVALID_MANTLE_CODE,
        format!("generated mantle snapshot failed validation: {error}"),
    )
}
