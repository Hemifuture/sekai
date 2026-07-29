//! Engine adapters for deterministic current-slice tectonic generation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{TectonicGenerationError, TectonicGenerator};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SpatialArtifact;
use crate::world::natural::{
    NaturalSpecError, TectonicSnapshot, TectonicSpec, TectonicValidationError,
};

const INVALID_SPEC_CODE: &str = "natural.invalid-tectonic-spec";
const BUILD_FAILED_CODE: &str = "natural.tectonic-build-failed";
const INVALID_SNAPSHOT_CODE: &str = "natural.invalid-tectonic-snapshot";

/// Engine transport wrapper for an externally supplied tectonic specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TectonicSpecArtifact {
    spec: TectonicSpec,
}

impl TectonicSpecArtifact {
    /// Wraps a tectonic specification for validated engine transport.
    pub const fn new(spec: TectonicSpec) -> Self {
        Self { spec }
    }

    /// Returns the wrapped tectonic specification.
    pub const fn spec(&self) -> &TectonicSpec {
        &self.spec
    }
}

impl Artifact for TectonicSpecArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.tectonic-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.spec
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SPEC_CODE, error.to_string()))
    }
}

/// Engine transport wrapper for a complete validated tectonic snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TectonicArtifact {
    snapshot: TectonicSnapshot,
}

impl TectonicArtifact {
    /// Wraps a complete tectonic snapshot for engine transport.
    pub const fn new(snapshot: TectonicSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the wrapped tectonic snapshot.
    pub const fn snapshot(&self) -> &TectonicSnapshot {
        &self.snapshot
    }
}

impl Artifact for TectonicArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.tectonics");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SNAPSHOT_CODE, error.to_string()))
    }
}

/// Restricted typed dependencies supplied to [`TectonicStage`].
pub struct TectonicStageInputs {
    spec: Arc<TectonicSpecArtifact>,
    spatial: Arc<SpatialArtifact>,
}

impl StageInputs for TectonicStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[TectonicSpecArtifact::KEY, SpatialArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            spec: artifacts.get::<TectonicSpecArtifact>()?,
            spatial: artifacts.get::<SpatialArtifact>()?,
        })
    }
}

/// Deterministic stage that builds plates, crust, motion, and current boundary events.
#[derive(Debug, Clone, Copy, Default)]
pub struct TectonicStage;

impl Stage for TectonicStage {
    type Inputs = TectonicStageInputs;
    type Output = TectonicArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.tectonics")
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
        inputs.spec.spec().validate().map_err(invalid_spec)?;
        let snapshot =
            TectonicGenerator::generate(inputs.spatial.snapshot(), inputs.spec.spec(), rng)
                .map_err(generation_failure)?;
        snapshot
            .validate_against(inputs.spatial.snapshot())
            .map_err(invalid_snapshot)?;
        Ok(TectonicArtifact::new(snapshot))
    }
}

fn invalid_spec(error: NaturalSpecError) -> StageError {
    StageError::new(
        INVALID_SPEC_CODE,
        format!("invalid tectonic specification: {error}"),
    )
}

fn generation_failure(error: TectonicGenerationError) -> StageError {
    match error {
        TectonicGenerationError::InvalidSpec(error) => invalid_spec(error),
        TectonicGenerationError::PlateCountExceedsCells { .. } => StageError::new(
            INVALID_SPEC_CODE,
            format!("tectonic specification is incompatible with spatial input: {error}"),
        ),
        TectonicGenerationError::InvalidSnapshot(error) => invalid_snapshot(error),
        TectonicGenerationError::UnsatisfiedRelativeMotion { .. } => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
    }
}

fn invalid_snapshot(error: TectonicValidationError) -> StageError {
    StageError::new(
        INVALID_SNAPSHOT_CODE,
        format!("generated tectonic snapshot failed validation: {error}"),
    )
}
