//! Typed engine adapter for the atomic spherical hydro-erosion output.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::super::{
    HydroErosionGenerator, ResolvedHydroErosionInputArtifact, SphericalGeologicArtifact,
    SphericalHydroErosionGenerationError, SphericalPreliminaryClimateArtifact,
    SphericalReliefArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::rules::HydroErosionModel;
use crate::world::natural::SphericalHydroErosionSnapshot;

const INVALID_INPUT_CODE: &str = "spherical-natural.invalid-hydro-erosion-input";
const BUILD_FAILED_CODE: &str = "spherical-natural.hydro-erosion-build-failed";
const INVALID_HYDRO_EROSION_CODE: &str = "spherical-natural.invalid-hydro-erosion";

/// Engine transport for the single atomic spherical surface-and-hydrology snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalHydroErosionArtifact {
    snapshot: SphericalHydroErosionSnapshot,
}

impl SphericalHydroErosionArtifact {
    /// Wraps one complete atomic spherical hydro-erosion snapshot.
    pub const fn new(snapshot: SphericalHydroErosionSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the immutable surface-bound snapshot.
    pub const fn snapshot(&self) -> &SphericalHydroErosionSnapshot {
        &self.snapshot
    }
}

impl Artifact for SphericalHydroErosionArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spherical-hydro-erosion");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_HYDRO_EROSION_CODE, error.to_string())
        })
    }
}

/// The exact typed inputs visible to [`SphericalHydroErosionStage`].
pub struct SphericalHydroErosionStageInputs {
    resolved_input: Arc<ResolvedHydroErosionInputArtifact>,
    geology: Arc<SphericalGeologicArtifact>,
    climate: Arc<SphericalPreliminaryClimateArtifact>,
    relief: Arc<SphericalReliefArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for SphericalHydroErosionStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedHydroErosionInputArtifact::KEY,
            SphericalGeologicArtifact::KEY,
            SphericalPreliminaryClimateArtifact::KEY,
            SphericalReliefArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolved_input: artifacts.get::<ResolvedHydroErosionInputArtifact>()?,
            geology: artifacts.get::<SphericalGeologicArtifact>()?,
            climate: artifacts.get::<SphericalPreliminaryClimateArtifact>()?,
            relief: artifacts.get::<SphericalReliefArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

/// Publishes the fixed one-index, two-hydrology-pass spherical result atomically.
#[derive(Debug, Clone, Copy, Default)]
pub struct SphericalHydroErosionStage;

impl Stage for SphericalHydroErosionStage {
    type Inputs = SphericalHydroErosionStageInputs;
    type Output = SphericalHydroErosionArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.spherical-hydro-erosion")
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
            HydroErosionModel::PriorityFloodStreamPowerV1 => {
                HydroErosionGenerator::generate_spherical(
                    inputs.surface.snapshot(),
                    inputs.relief.snapshot(),
                    inputs.geology.snapshot(),
                    inputs.climate.snapshot(),
                    resolved.spec(),
                )
            }
        }
        .map_err(generation_failure)?;
        snapshot
            .validate_against(
                inputs.surface.snapshot(),
                inputs.relief.snapshot(),
                inputs.geology.snapshot(),
                inputs.climate.snapshot(),
            )
            .map_err(|error| invalid_snapshot(error.to_string()))?;
        Ok(SphericalHydroErosionArtifact::new(snapshot))
    }
}

fn generation_failure(error: SphericalHydroErosionGenerationError) -> StageError {
    match error {
        SphericalHydroErosionGenerationError::InvalidSpec(_)
        | SphericalHydroErosionGenerationError::InvalidSurface(_)
        | SphericalHydroErosionGenerationError::InvalidSurfaceIdentity(_)
        | SphericalHydroErosionGenerationError::InvalidRelief(_)
        | SphericalHydroErosionGenerationError::InvalidGeology(_)
        | SphericalHydroErosionGenerationError::InvalidClimate(_)
        | SphericalHydroErosionGenerationError::UpstreamSurfaceMismatch { .. } => {
            invalid_input(error.to_string())
        }
        SphericalHydroErosionGenerationError::InitialHydrology(_)
        | SphericalHydroErosionGenerationError::Erosion(_)
        | SphericalHydroErosionGenerationError::FinalHydrology(_) => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
        SphericalHydroErosionGenerationError::Composite(_) => invalid_snapshot(error.to_string()),
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn invalid_snapshot(message: String) -> StageError {
    StageError::new(INVALID_HYDRO_EROSION_CODE, message)
}
