//! Typed engine adapters for surface-bound spherical tectonic generation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, HydroErosionSpecArtifact,
    ReliefGenerator, ReliefSpecArtifact, ResolvedClimateInputStage, ResolvedGeologicInputStage,
    ResolvedHydroErosionInputStage, ResolvedTectonicInputArtifact, ResolvedTectonicInputStage,
    ResolvedWorldFormationArtifact, RuleClimateResolutionStage, RuleGeologicResolutionStage,
    RuleHydroErosionResolutionStage, RulePackSetArtifact, RuleTectonicResolutionStage,
    SphericalGeologicStage, SphericalHydroErosionStage, SphericalMantleArtifact,
    SphericalMantleStage, SphericalNaturalQualityStage, SphericalPreliminaryClimateStage,
    SphericalReliefGenerationError, SphericalTectonicGenerationError, TectonicGenerator,
    TectonicSpecArtifact, WorldFormationSpecArtifact, WorldFormationStage,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    GraphError, Stage, StageError, StageGraph, StageGraphBuilder, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::{
    SphericalSpaceArtifact, SphericalSurfaceArtifact, SphericalSurfaceStage,
};
use crate::rules::TectonicModel;
use crate::world::natural::{SphericalReliefSnapshot, SphericalTectonicSnapshot};

const INVALID_INPUT_CODE: &str = "spherical-natural.invalid-tectonic-input";
const BUILD_FAILED_CODE: &str = "spherical-natural.tectonic-build-failed";
const INVALID_TECTONICS_CODE: &str = "spherical-natural.invalid-tectonics";
const INVALID_RELIEF_INPUT_CODE: &str = "spherical-natural.invalid-relief-input";
const RELIEF_BUILD_FAILED_CODE: &str = "spherical-natural.relief-build-failed";
const INVALID_RELIEF_CODE: &str = "spherical-natural.invalid-relief";

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
        4
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

/// Engine transport for one complete spherical relief snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalReliefArtifact {
    snapshot: SphericalReliefSnapshot,
}

impl SphericalReliefArtifact {
    /// Wraps a locally valid spherical relief snapshot.
    pub const fn new(snapshot: SphericalReliefSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the immutable surface-bound snapshot.
    pub const fn snapshot(&self) -> &SphericalReliefSnapshot {
        &self.snapshot
    }
}

impl Artifact for SphericalReliefArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.spherical-relief");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_RELIEF_CODE, error.to_string()))
    }
}

/// The exact typed inputs visible to [`SphericalReliefStage`].
pub struct SphericalReliefStageInputs {
    mantle: Arc<SphericalMantleArtifact>,
    relief_spec: Arc<ReliefSpecArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
    tectonic: Arc<SphericalTectonicArtifact>,
}

impl StageInputs for SphericalReliefStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            SphericalMantleArtifact::KEY,
            ReliefSpecArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
            SphericalTectonicArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            mantle: artifacts.get::<SphericalMantleArtifact>()?,
            relief_spec: artifacts.get::<ReliefSpecArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
            tectonic: artifacts.get::<SphericalTectonicArtifact>()?,
        })
    }
}

/// Deterministic adapter for the frozen spherical relief scientific stream.
#[derive(Debug, Clone, Copy, Default)]
pub struct SphericalReliefStage;

impl Stage for SphericalReliefStage {
    type Inputs = SphericalReliefStageInputs;
    type Output = SphericalReliefArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.spherical-relief")
    }

    fn version(&self) -> u32 {
        3
    }

    fn namespace(&self) -> &'static str {
        "sekai.core"
    }

    fn run(
        &self,
        inputs: Self::Inputs,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        inputs
            .surface
            .snapshot()
            .validate()
            .map_err(|error| invalid_relief_input(error.to_string()))?;
        inputs
            .tectonic
            .snapshot()
            .validate_against(inputs.surface.snapshot())
            .map_err(|error| invalid_relief_input(error.to_string()))?;
        inputs
            .mantle
            .snapshot()
            .validate_against(inputs.surface.snapshot())
            .map_err(|error| invalid_relief_input(error.to_string()))?;
        inputs
            .relief_spec
            .spec()
            .validate()
            .map_err(|error| invalid_relief_input(error.to_string()))?;

        let snapshot = ReliefGenerator::generate_spherical(
            inputs.surface.snapshot(),
            inputs.tectonic.snapshot(),
            inputs.mantle.snapshot(),
            inputs.relief_spec.spec(),
            rng,
            diagnostics,
        )
        .map_err(relief_generation_failure)?;
        snapshot
            .validate_against(
                inputs.surface.snapshot(),
                inputs.tectonic.snapshot(),
                inputs.mantle.snapshot(),
            )
            .map_err(|error| invalid_relief(error.to_string()))?;
        Ok(SphericalReliefArtifact::new(snapshot))
    }
}

/// Builds the authoritative spherical natural-foundation stage graph.
///
/// The spherical surface is the sole geometry source for generated worlds;
/// planar artifacts remain confined to the legacy compatibility graph.
///
/// # Errors
///
/// Returns a graph error if these fixed declarations cease to satisfy the
/// engine dependency contract.
pub fn spherical_natural_foundation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<SphericalSpaceArtifact>()
        .external::<TectonicSpecArtifact>()
        .external::<GeologicSpecArtifact>()
        .external::<ClimateSpecArtifact>()
        .external::<HydroErosionSpecArtifact>()
        .external::<ReliefSpecArtifact>()
        .external::<WorldFormationSpecArtifact>()
        .external::<RulePackSetArtifact>()
        .external::<AuthorConstraintsArtifact>()
        .stage(SphericalSurfaceStage)
        .stage(RuleTectonicResolutionStage)
        .stage(RuleGeologicResolutionStage)
        .stage(RuleClimateResolutionStage)
        .stage(RuleHydroErosionResolutionStage)
        .stage(ResolvedTectonicInputStage)
        .stage(ResolvedGeologicInputStage)
        .stage(ResolvedClimateInputStage)
        .stage(ResolvedHydroErosionInputStage)
        .stage(WorldFormationStage)
        .stage(SphericalTectonicStage)
        .stage(SphericalMantleStage)
        .stage(SphericalReliefStage)
        .stage(SphericalGeologicStage)
        .stage(SphericalPreliminaryClimateStage)
        .stage(SphericalHydroErosionStage)
        .stage(SphericalNaturalQualityStage)
        .build()
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
        SphericalTectonicGenerationError::Morphology { .. } => {
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

fn invalid_relief_input(message: String) -> StageError {
    StageError::new(INVALID_RELIEF_INPUT_CODE, message)
}

fn relief_generation_failure(error: SphericalReliefGenerationError) -> StageError {
    match error {
        SphericalReliefGenerationError::InvalidSurface(_)
        | SphericalReliefGenerationError::InvalidSurfaceIdentity(_)
        | SphericalReliefGenerationError::InvalidTectonics(_)
        | SphericalReliefGenerationError::InvalidMantle(_)
        | SphericalReliefGenerationError::InvalidSpec(_)
        | SphericalReliefGenerationError::InvalidHeightmap { .. } => {
            invalid_relief_input(error.to_string())
        }
        SphericalReliefGenerationError::InvalidLandFraction { .. }
        | SphericalReliefGenerationError::InvalidLandFractionProjection { .. }
        | SphericalReliefGenerationError::InvalidReliefField(_) => {
            StageError::new(RELIEF_BUILD_FAILED_CODE, error.to_string())
        }
        SphericalReliefGenerationError::InvalidSnapshot(_) => invalid_relief(error.to_string()),
    }
}

fn invalid_relief(message: String) -> StageError {
    StageError::new(INVALID_RELIEF_CODE, message)
}
