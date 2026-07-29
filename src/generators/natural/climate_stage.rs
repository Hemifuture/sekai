//! Engine adapter for deterministic preliminary-climate generation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    ClimateGenerationError, ClimateGenerator, ReliefArtifact, ResolvedClimateInputArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SpatialArtifact;
use crate::rules::ClimateModel;
use crate::world::natural::{ClimateValidationError, PreliminaryClimateSnapshot};

const INVALID_INPUT_CODE: &str = "natural.invalid-preliminary-climate-input";
const BUILD_FAILED_CODE: &str = "natural.climate-build-failed";
const INVALID_SNAPSHOT_CODE: &str = "natural.invalid-preliminary-climate";

/// Engine transport for a complete validated preliminary-climate snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreliminaryClimateArtifact {
    snapshot: PreliminaryClimateSnapshot,
}

impl PreliminaryClimateArtifact {
    /// Wraps a complete validated snapshot for engine transport.
    pub const fn new(snapshot: PreliminaryClimateSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the authoritative preliminary-climate snapshot.
    pub const fn snapshot(&self) -> &PreliminaryClimateSnapshot {
        &self.snapshot
    }
}

impl Artifact for PreliminaryClimateArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.preliminary-climate");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SNAPSHOT_CODE, error.to_string()))
    }
}

/// Exact typed dependencies visible to [`PreliminaryClimateStage`].
pub struct PreliminaryClimateStageInputs {
    resolved_input: Arc<ResolvedClimateInputArtifact>,
    relief: Arc<ReliefArtifact>,
    spatial: Arc<SpatialArtifact>,
}

impl StageInputs for PreliminaryClimateStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedClimateInputArtifact::KEY,
            ReliefArtifact::KEY,
            SpatialArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolved_input: artifacts.get::<ResolvedClimateInputArtifact>()?,
            relief: artifacts.get::<ReliefArtifact>()?,
            spatial: artifacts.get::<SpatialArtifact>()?,
        })
    }
}

/// Publishes bounded monthly climate forcing for the current natural slice.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreliminaryClimateStage;

impl Stage for PreliminaryClimateStage {
    type Inputs = PreliminaryClimateStageInputs;
    type Output = PreliminaryClimateArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.preliminary-climate")
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
        _rng: &mut StageRng,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        let resolved = inputs.resolved_input.input();
        let spatial = inputs.spatial.snapshot();
        let relief = inputs.relief.snapshot();

        resolved
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        relief
            .validate_against(spatial)
            .map_err(|error| invalid_input(error.to_string()))?;
        let snapshot = match resolved.model() {
            ClimateModel::SeasonalEnergyMoistureV1 => {
                ClimateGenerator::generate(spatial, relief, resolved.spec())
            }
        }
        .map_err(generation_failure)?;
        snapshot
            .validate_against(spatial, relief)
            .map_err(invalid_snapshot)?;
        Ok(PreliminaryClimateArtifact::new(snapshot))
    }
}

fn generation_failure(error: ClimateGenerationError) -> StageError {
    match error {
        ClimateGenerationError::InvalidSpec(error) => invalid_input(error.to_string()),
        ClimateGenerationError::InvalidRelief(error) => invalid_input(error.to_string()),
        ClimateGenerationError::InvalidSnapshot(error) => invalid_snapshot(error),
        ClimateGenerationError::EmptySpatialSnapshot
        | ClimateGenerationError::CellCountOverflow { .. } => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn invalid_snapshot(error: ClimateValidationError) -> StageError {
    StageError::new(
        INVALID_SNAPSHOT_CODE,
        format!("generated preliminary climate failed validation: {error}"),
    )
}
