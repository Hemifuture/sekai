//! Typed engine adapter for surface-bound spherical preliminary climate.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::super::{
    ClimateGenerator, ResolvedClimateInputArtifact, SphericalClimateGenerationError,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::natural::SphericalReliefArtifact;
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::rules::ClimateModel;
use crate::world::natural::SphericalPreliminaryClimateSnapshot;

const INVALID_INPUT_CODE: &str = "spherical-natural.invalid-preliminary-climate-input";
const INVALID_CLIMATE_CODE: &str = "spherical-natural.invalid-preliminary-climate";

/// Engine transport for one complete spherical preliminary-climate snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalPreliminaryClimateArtifact {
    snapshot: SphericalPreliminaryClimateSnapshot,
}

impl SphericalPreliminaryClimateArtifact {
    /// Wraps a locally valid spherical preliminary-climate snapshot.
    pub const fn new(snapshot: SphericalPreliminaryClimateSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the immutable surface-bound snapshot.
    pub const fn snapshot(&self) -> &SphericalPreliminaryClimateSnapshot {
        &self.snapshot
    }
}

impl Artifact for SphericalPreliminaryClimateArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spherical-preliminary-climate");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_CLIMATE_CODE, error.to_string()))
    }
}

/// The exact typed inputs visible to [`SphericalPreliminaryClimateStage`].
pub struct SphericalPreliminaryClimateStageInputs {
    resolved_input: Arc<ResolvedClimateInputArtifact>,
    relief: Arc<SphericalReliefArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for SphericalPreliminaryClimateStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedClimateInputArtifact::KEY,
            SphericalReliefArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolved_input: artifacts.get::<ResolvedClimateInputArtifact>()?,
            relief: artifacts.get::<SphericalReliefArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

/// Publishes the frozen spherical preliminary-climate solution.
#[derive(Debug, Clone, Copy, Default)]
pub struct SphericalPreliminaryClimateStage;

impl Stage for SphericalPreliminaryClimateStage {
    type Inputs = SphericalPreliminaryClimateStageInputs;
    type Output = SphericalPreliminaryClimateArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.spherical-preliminary-climate")
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
        let surface = inputs.surface.snapshot();
        let relief = inputs.relief.snapshot();
        resolved
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        surface
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        relief
            .validate_against_validated_surface(surface)
            .map_err(|error| invalid_input(error.to_string()))?;

        let snapshot = match resolved.model() {
            ClimateModel::SeasonalEnergyMoistureV1 => {
                ClimateGenerator::generate_spherical(surface, relief, resolved.spec())
            }
        }
        .map_err(generation_failure)?;
        snapshot
            .validate_against(surface, relief)
            .map_err(|error| invalid_climate(error.to_string()))?;
        Ok(SphericalPreliminaryClimateArtifact::new(snapshot))
    }
}

fn generation_failure(error: SphericalClimateGenerationError) -> StageError {
    match error {
        SphericalClimateGenerationError::InvalidSpec(_)
        | SphericalClimateGenerationError::InvalidSurface(_)
        | SphericalClimateGenerationError::InvalidRelief(_)
        | SphericalClimateGenerationError::InvalidSurfaceRef(_) => invalid_input(error.to_string()),
        SphericalClimateGenerationError::InvalidSnapshot(_) => invalid_climate(error.to_string()),
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn invalid_climate(message: String) -> StageError {
    StageError::new(INVALID_CLIMATE_CODE, message)
}
