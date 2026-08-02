//! Engine adapters for deterministic current-slice tectonic generation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, GeologicStage,
    HydroErosionSpecArtifact, HydroErosionStage, MantleArtifact, MantleStage,
    PreliminaryClimateStage, ResolvedClimateInputStage, ResolvedGeologicInputStage,
    ResolvedHydroErosionInputStage, ResolvedTectonicInputArtifact, ResolvedTectonicInputStage,
    ResolvedWorldFormationArtifact, RuleClimateResolutionStage, RuleGeologicResolutionStage,
    RuleHydroErosionResolutionStage, RulePackSetArtifact, RuleTectonicResolutionStage,
    WorldFormationSpecArtifact, WorldFormationStage,
};
use super::{ReliefGenerationError, ReliefGenerator, TectonicGenerationError, TectonicGenerator};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    GraphError, Stage, StageError, StageGraph, StageGraphBuilder, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact, SpatialStage};
use crate::rules::TectonicModel;
use crate::world::natural::{
    MantleValidationError, NaturalSpecError, ReliefSnapshot, ReliefValidationError,
    TectonicSnapshot, TectonicSpec, TectonicValidationError,
};

const INVALID_SPEC_CODE: &str = "natural.invalid-tectonic-spec";
const BUILD_FAILED_CODE: &str = "natural.tectonic-build-failed";
const INVALID_SNAPSHOT_CODE: &str = "natural.invalid-tectonic-snapshot";
const INVALID_TECTONICS_CODE: &str = "natural.invalid-tectonics";
const INVALID_MANTLE_CODE: &str = "natural.invalid-mantle";
const RELIEF_FAILED_CODE: &str = "natural.relief-failed";
const INVALID_RELIEF_CODE: &str = "natural.invalid-relief";

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
    formation: Arc<ResolvedWorldFormationArtifact>,
    resolved_input: Arc<ResolvedTectonicInputArtifact>,
    spatial: Arc<SpatialArtifact>,
}

impl StageInputs for TectonicStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedTectonicInputArtifact::KEY,
            ResolvedWorldFormationArtifact::KEY,
            SpatialArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            formation: artifacts.get::<ResolvedWorldFormationArtifact>()?,
            resolved_input: artifacts.get::<ResolvedTectonicInputArtifact>()?,
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
        2
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
            TectonicModel::CurrentSliceV1 => TectonicGenerator::generate(
                inputs.spatial.snapshot(),
                resolved_input.spec(),
                inputs.formation.formation(),
                rng,
            ),
        }
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
        TectonicGenerationError::InvalidFormation(error) => StageError::new(
            INVALID_SPEC_CODE,
            format!("resolved world formation is invalid: {error}"),
        ),
        TectonicGenerationError::PlateCountExceedsCells { .. } => StageError::new(
            INVALID_SPEC_CODE,
            format!("tectonic specification is incompatible with spatial input: {error}"),
        ),
        TectonicGenerationError::InsufficientCrustFormationArea { .. } => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
        }
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

/// Engine transport wrapper for a complete validated relief snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReliefArtifact {
    snapshot: ReliefSnapshot,
}

impl ReliefArtifact {
    /// Wraps a complete relief snapshot for engine transport.
    pub const fn new(snapshot: ReliefSnapshot) -> Self {
        Self { snapshot }
    }

    /// Returns the wrapped relief snapshot.
    pub const fn snapshot(&self) -> &ReliefSnapshot {
        &self.snapshot
    }
}

impl Artifact for ReliefArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.relief");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_RELIEF_CODE, error.to_string()))
    }
}

/// Restricted typed dependencies supplied to [`ReliefStage`].
pub struct ReliefStageInputs {
    mantle: Arc<MantleArtifact>,
    spatial: Arc<SpatialArtifact>,
    tectonic: Arc<TectonicArtifact>,
}

impl StageInputs for ReliefStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            MantleArtifact::KEY,
            SpatialArtifact::KEY,
            TectonicArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            mantle: artifacts.get::<MantleArtifact>()?,
            spatial: artifacts.get::<SpatialArtifact>()?,
            tectonic: artifacts.get::<TectonicArtifact>()?,
        })
    }
}

/// Deterministic stage that synthesizes explainable current-slice relief.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReliefStage;

impl Stage for ReliefStage {
    type Inputs = ReliefStageInputs;
    type Output = ReliefArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.relief")
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
            .tectonic
            .snapshot()
            .validate_against(inputs.spatial.snapshot())
            .map_err(invalid_tectonics)?;
        inputs
            .mantle
            .snapshot()
            .validate_against(inputs.spatial.snapshot())
            .map_err(invalid_mantle)?;
        let snapshot = ReliefGenerator::generate(
            inputs.spatial.snapshot(),
            inputs.tectonic.snapshot(),
            inputs.mantle.snapshot(),
            rng,
            diagnostics,
        )
        .map_err(relief_failure)?;
        snapshot
            .validate_against(inputs.spatial.snapshot())
            .map_err(invalid_relief)?;
        Ok(ReliefArtifact::new(snapshot))
    }
}

/// Builds the complete current-slice natural foundation stage graph.
///
/// # Errors
///
/// Returns a graph error if the fixed stage declarations cease to satisfy the
/// engine dependency contract.
pub fn natural_foundation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<PlanarSpaceArtifact>()
        .external::<TectonicSpecArtifact>()
        .external::<GeologicSpecArtifact>()
        .external::<ClimateSpecArtifact>()
        .external::<HydroErosionSpecArtifact>()
        .external::<WorldFormationSpecArtifact>()
        .external::<RulePackSetArtifact>()
        .external::<AuthorConstraintsArtifact>()
        .stage(SpatialStage)
        .stage(RuleTectonicResolutionStage)
        .stage(RuleGeologicResolutionStage)
        .stage(RuleClimateResolutionStage)
        .stage(RuleHydroErosionResolutionStage)
        .stage(ResolvedTectonicInputStage)
        .stage(ResolvedGeologicInputStage)
        .stage(ResolvedClimateInputStage)
        .stage(ResolvedHydroErosionInputStage)
        .stage(WorldFormationStage)
        .stage(TectonicStage)
        .stage(MantleStage)
        .stage(ReliefStage)
        .stage(GeologicStage)
        .stage(PreliminaryClimateStage)
        .stage(HydroErosionStage)
        .build()
}

fn invalid_tectonics(error: TectonicValidationError) -> StageError {
    StageError::new(
        INVALID_TECTONICS_CODE,
        format!("tectonic input failed spatial validation: {error}"),
    )
}

fn invalid_mantle(error: MantleValidationError) -> StageError {
    StageError::new(
        INVALID_MANTLE_CODE,
        format!("mantle input failed spatial validation: {error}"),
    )
}

fn relief_failure(error: ReliefGenerationError) -> StageError {
    match error {
        ReliefGenerationError::InvalidTectonics(error) => invalid_tectonics(error),
        ReliefGenerationError::InvalidMantle(error) => invalid_mantle(error),
        ReliefGenerationError::ExposedBoundaryCell { .. } => {
            StageError::new(RELIEF_FAILED_CODE, error.to_string())
        }
        ReliefGenerationError::InvalidRelief(error) => StageError::new(
            RELIEF_FAILED_CODE,
            format!("relief synthesis produced invalid fields: {error}"),
        ),
    }
}

fn invalid_relief(error: ReliefValidationError) -> StageError {
    StageError::new(
        INVALID_RELIEF_CODE,
        format!("generated relief snapshot failed validation: {error}"),
    )
}
