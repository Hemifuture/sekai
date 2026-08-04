//! Typed engine adapters for surface-bound spherical tectonic generation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
    SphericalTectonicGenerationError, TectonicGenerator,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::rules::TectonicModel;
use crate::world::natural::SphericalTectonicSnapshot;

const INVALID_INPUT_CODE: &str = "spherical-natural.invalid-tectonic-input";
const BUILD_FAILED_CODE: &str = "spherical-natural.tectonic-build-failed";
const INVALID_TECTONICS_CODE: &str = "spherical-natural.invalid-tectonics";

/// Engine transport for one complete spherical tectonic snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalTectonicArtifact {
    snapshot: SphericalTectonicSnapshot,
}

impl SphericalTectonicArtifact {
    /// Wraps a locally valid spherical tectonic snapshot.
    pub const fn new(snapshot: SphericalTectonicSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the immutable surface-bound snapshot.
    pub const fn snapshot(&self) -> &SphericalTectonicSnapshot {
        &self.snapshot
    }
}

impl Artifact for SphericalTectonicArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spherical-tectonics");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_TECTONICS_CODE, error.to_string())
        })
    }
}

/// The exact typed inputs visible to [`SphericalTectonicStage`].
pub struct SphericalTectonicStageInputs {
    formation: Arc<ResolvedWorldFormationArtifact>,
    resolved_input: Arc<ResolvedTectonicInputArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for SphericalTectonicStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedTectonicInputArtifact::KEY,
            ResolvedWorldFormationArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            formation: artifacts.get::<ResolvedWorldFormationArtifact>()?,
            resolved_input: artifacts.get::<ResolvedTectonicInputArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

/// Deterministic adapter for the frozen spherical tectonic scientific stream.
#[derive(Debug, Clone, Copy, Default)]
pub struct SphericalTectonicStage;

impl Stage for SphericalTectonicStage {
    type Inputs = SphericalTectonicStageInputs;
    type Output = SphericalTectonicArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.spherical-tectonics")
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
            TectonicModel::CurrentSliceV1 => TectonicGenerator::generate_spherical(
                inputs.surface.snapshot(),
                inputs.resolved_input.input().spec(),
                inputs.formation.formation(),
                rng,
            ),
        }
        .map_err(generation_failure)?;
        snapshot
            .validate_against(inputs.surface.snapshot())
            .map_err(|error| invalid_tectonics(error.to_string()))?;
        Ok(SphericalTectonicArtifact::new(snapshot))
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn generation_failure(error: SphericalTectonicGenerationError) -> StageError {
    match error {
        SphericalTectonicGenerationError::InvalidSpec(_)
        | SphericalTectonicGenerationError::InvalidFormation(_)
        | SphericalTectonicGenerationError::InvalidSurface(_)
        | SphericalTectonicGenerationError::InvalidSurfaceIdentity(_)
        | SphericalTectonicGenerationError::PlateCountExceedsCells { .. } => {
            invalid_input(error.to_string())
        }
        SphericalTectonicGenerationError::InsufficientCrustFormationArea { .. }
        | SphericalTectonicGenerationError::UnsatisfiedRelativeMotion { .. } => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
        SphericalTectonicGenerationError::InvalidSnapshot(_) => {
            invalid_tectonics(error.to_string())
        }
    }
}

fn invalid_tectonics(message: String) -> StageError {
    StageError::new(INVALID_TECTONICS_CODE, message)
}
