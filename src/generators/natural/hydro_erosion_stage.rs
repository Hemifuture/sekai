//! Engine adapter for the atomic current-slice hydro-erosion output.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    GeologicArtifact, HydroErosionGenerationError, HydroErosionGenerator,
    PreliminaryClimateArtifact, ReliefArtifact, ResolvedHydroErosionInputArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SpatialArtifact;
use crate::rules::HydroErosionModel;
use crate::world::natural::{HydroErosionSnapshot, HydroErosionValidationError};

const INVALID_INPUT_CODE: &str = "natural.invalid-hydro-erosion-input";
const BUILD_FAILED_CODE: &str = "natural.hydro-erosion-build-failed";
const INVALID_SNAPSHOT_CODE: &str = "natural.invalid-hydro-erosion";

/// Engine transport for the single atomic surface-and-hydrology snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HydroErosionArtifact {
    snapshot: HydroErosionSnapshot,
}

impl HydroErosionArtifact {
    /// Wraps a complete atomic hydro-erosion snapshot.
    pub const fn new(snapshot: HydroErosionSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the authoritative current-slice hydro-erosion snapshot.
    pub const fn snapshot(&self) -> &HydroErosionSnapshot {
        &self.snapshot
    }
}

impl Artifact for HydroErosionArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.hydro-erosion");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SNAPSHOT_CODE, error.to_string()))
    }
}

/// Exact typed dependencies visible to [`HydroErosionStage`].
pub struct HydroErosionStageInputs {
    resolved_input: Arc<ResolvedHydroErosionInputArtifact>,
    spatial: Arc<SpatialArtifact>,
    relief: Arc<ReliefArtifact>,
    geology: Arc<GeologicArtifact>,
    climate: Arc<PreliminaryClimateArtifact>,
}

impl StageInputs for HydroErosionStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedHydroErosionInputArtifact::KEY,
            GeologicArtifact::KEY,
            PreliminaryClimateArtifact::KEY,
            ReliefArtifact::KEY,
            SpatialArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolved_input: artifacts.get::<ResolvedHydroErosionInputArtifact>()?,
            spatial: artifacts.get::<SpatialArtifact>()?,
            relief: artifacts.get::<ReliefArtifact>()?,
            geology: artifacts.get::<GeologicArtifact>()?,
            climate: artifacts.get::<PreliminaryClimateArtifact>()?,
        })
    }
}

/// Publishes the fixed two-pass current surface and formal hydrology atomically.
#[derive(Debug, Clone, Copy, Default)]
pub struct HydroErosionStage;

impl Stage for HydroErosionStage {
    type Inputs = HydroErosionStageInputs;
    type Output = HydroErosionArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.hydro-erosion")
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
        resolved
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        let snapshot = match resolved.model() {
            HydroErosionModel::PriorityFloodStreamPowerV1 => HydroErosionGenerator::generate(
                inputs.spatial.snapshot(),
                inputs.relief.snapshot(),
                inputs.geology.snapshot(),
                inputs.climate.snapshot(),
                resolved.spec(),
            ),
        }
        .map_err(generation_failure)?;
        snapshot
            .validate_against(
                inputs.spatial.snapshot(),
                inputs.relief.snapshot(),
                inputs.geology.snapshot(),
                inputs.climate.snapshot(),
            )
            .map_err(invalid_snapshot)?;
        Ok(HydroErosionArtifact::new(snapshot))
    }
}

fn generation_failure(error: HydroErosionGenerationError) -> StageError {
    match error {
        HydroErosionGenerationError::Composite(error) => invalid_snapshot(error),
        HydroErosionGenerationError::Spatial(_)
        | HydroErosionGenerationError::Relief(_)
        | HydroErosionGenerationError::Geology(_)
        | HydroErosionGenerationError::Climate(_)
        | HydroErosionGenerationError::Spec(_)
        | HydroErosionGenerationError::CellCountMismatch { .. } => invalid_input(error.to_string()),
        HydroErosionGenerationError::InitialHydrology(_)
        | HydroErosionGenerationError::Erosion(_)
        | HydroErosionGenerationError::FinalHydrology(_) => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn invalid_snapshot(error: HydroErosionValidationError) -> StageError {
    StageError::new(
        INVALID_SNAPSHOT_CODE,
        format!("generated hydro-erosion snapshot failed validation: {error}"),
    )
}
