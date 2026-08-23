//! Typed engine publication for causal P3 substrate and physical primary relief.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::quality::{
    evaluate_primary_relief_quality, validate_primary_relief_quality_report, QualityBuildError,
};
use super::{
    EvolvedTectonicArtifact, EvolvedTectonicStage, GeologicSubstrateGenerationError,
    GeologicSubstrateGenerator, NaturalQualityProfileArtifact, PrimaryReliefGenerationError,
    PrimaryReliefGenerator, ReliefSpecArtifact, ResolvedGeologicInputArtifact,
    ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    GraphError, Stage, StageError, StageGraph, StageGraphBuilder, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::rules::GeologicModel;
use crate::world::natural::{
    GeologicSubstrateSnapshot, NaturalQualityReport, PrimaryReliefSnapshot,
};

const INVALID_INPUT_CODE: &str = "primary-relief.invalid-input";
const SUBSTRATE_BUILD_FAILED_CODE: &str = "primary-relief.substrate-build-failed";
const RELIEF_BUILD_FAILED_CODE: &str = "primary-relief.build-failed";
const INVALID_ARTIFACT_CODE: &str = "primary-relief.invalid-artifact";
const INVALID_QUALITY_CODE: &str = "primary-relief.invalid-quality";
const CANCELLED_CODE: &str = "engine.cancelled";

/// Atomic publication of the complete V5-derived substrate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeologicSubstrateArtifact {
    snapshot: GeologicSubstrateSnapshot,
}

impl GeologicSubstrateArtifact {
    pub const fn new(snapshot: GeologicSubstrateSnapshot) -> Self {
        Self { snapshot }
    }

    pub const fn snapshot(&self) -> &GeologicSubstrateSnapshot {
        &self.snapshot
    }
}

impl Artifact for GeologicSubstrateArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.geologic-substrate");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string()))
    }
}

/// Atomic publication of physical P3 relief and its per-world quality evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrimaryReliefArtifact {
    snapshot: PrimaryReliefSnapshot,
    quality_report: NaturalQualityReport,
}

impl PrimaryReliefArtifact {
    pub const fn new(
        snapshot: PrimaryReliefSnapshot,
        quality_report: NaturalQualityReport,
    ) -> Self {
        Self {
            snapshot,
            quality_report,
        }
    }

    pub const fn snapshot(&self) -> &PrimaryReliefSnapshot {
        &self.snapshot
    }

    pub const fn quality_report(&self) -> &NaturalQualityReport {
        &self.quality_report
    }
}

impl Artifact for PrimaryReliefArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.primary-relief");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string())
        })?;
        validate_primary_relief_quality_report(&self.quality_report, self.snapshot.surface_ref())
            .map_err(|error| ArtifactValidationError::new(INVALID_QUALITY_CODE, error))?;
        Ok(())
    }
}

/// Exact typed dependencies consumed while generating substrate.
pub struct GeologicSubstrateStageInputs {
    evolved: Arc<EvolvedTectonicArtifact>,
    geologic: Arc<ResolvedGeologicInputArtifact>,
    formation: Arc<ResolvedWorldFormationArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for GeologicSubstrateStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            EvolvedTectonicArtifact::KEY,
            ResolvedGeologicInputArtifact::KEY,
            ResolvedWorldFormationArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            evolved: artifacts.get::<EvolvedTectonicArtifact>()?,
            geologic: artifacts.get::<ResolvedGeologicInputArtifact>()?,
            formation: artifacts.get::<ResolvedWorldFormationArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

/// Deterministic substrate stage isolated at version 1.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeologicSubstrateStage;

impl Stage for GeologicSubstrateStage {
    type Inputs = GeologicSubstrateStageInputs;
    type Output = GeologicSubstrateArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.geologic-substrate")
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
        rng.check_cancelled().map_err(|_| cancelled())?;
        inputs
            .surface
            .snapshot()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .evolved
            .snapshot()
            .validate_against(inputs.surface.snapshot())
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .geologic
            .input()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .formation
            .formation()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        match inputs.geologic.input().model() {
            GeologicModel::CurrentSliceV1 => {}
        }
        let snapshot = GeologicSubstrateGenerator::generate(
            inputs.surface.snapshot(),
            inputs.evolved.snapshot(),
            inputs.geologic.input().spec(),
            inputs.formation.formation(),
            rng,
        )
        .map_err(substrate_failure)?;
        let artifact = GeologicSubstrateArtifact::new(snapshot);
        artifact
            .validate()
            .map_err(|error| StageError::new(error.code(), error.message()))?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        Ok(artifact)
    }
}

/// Exact typed dependencies consumed while generating primary relief.
pub struct PrimaryReliefStageInputs {
    evolved: Arc<EvolvedTectonicArtifact>,
    substrate: Arc<GeologicSubstrateArtifact>,
    relief_spec: Arc<ReliefSpecArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for PrimaryReliefStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            EvolvedTectonicArtifact::KEY,
            GeologicSubstrateArtifact::KEY,
            ReliefSpecArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            evolved: artifacts.get::<EvolvedTectonicArtifact>()?,
            substrate: artifacts.get::<GeologicSubstrateArtifact>()?,
            relief_spec: artifacts.get::<ReliefSpecArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

/// Deterministic physical primary-relief stage publishing fractional water geometry.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrimaryReliefStage;

impl Stage for PrimaryReliefStage {
    type Inputs = PrimaryReliefStageInputs;
    type Output = PrimaryReliefArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.primary-relief")
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
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        rng.check_cancelled().map_err(|_| cancelled())?;
        inputs
            .surface
            .snapshot()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .evolved
            .snapshot()
            .validate_against(inputs.surface.snapshot())
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .substrate
            .snapshot()
            .validate_against(inputs.surface.snapshot(), inputs.evolved.snapshot())
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .relief_spec
            .spec()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        let snapshot = PrimaryReliefGenerator::generate(
            inputs.surface.snapshot(),
            inputs.evolved.snapshot(),
            inputs.substrate.snapshot(),
            inputs.relief_spec.spec(),
            rng,
            diagnostics,
        )
        .map_err(relief_failure)?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        let quality_report = evaluate_primary_relief_quality(
            inputs.surface.snapshot(),
            inputs.evolved.snapshot(),
            inputs.substrate.snapshot(),
            &snapshot,
        )
        .map_err(quality_failure)?;
        let artifact = PrimaryReliefArtifact::new(snapshot, quality_report);
        artifact
            .validate()
            .map_err(|error| StageError::new(error.code(), error.message()))?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        Ok(artifact)
    }
}

/// Builds the isolated P2-P3 graph while leaving the frozen product graph intact.
pub fn primary_relief_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<NaturalQualityProfileArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ReliefSpecArtifact>()
        .external::<SphericalSurfaceArtifact>()
        .stage(EvolvedTectonicStage)
        .stage(GeologicSubstrateStage)
        .stage(PrimaryReliefStage)
        .build()
}

fn substrate_failure(error: GeologicSubstrateGenerationError) -> StageError {
    match error {
        GeologicSubstrateGenerationError::Cancelled => cancelled(),
        GeologicSubstrateGenerationError::InvalidSpec(_)
        | GeologicSubstrateGenerationError::InvalidFormation(_)
        | GeologicSubstrateGenerationError::InvalidEvolved(_) => invalid_input(error.to_string()),
        GeologicSubstrateGenerationError::Mantle(_)
        | GeologicSubstrateGenerationError::InvalidSnapshot(_) => {
            StageError::new(SUBSTRATE_BUILD_FAILED_CODE, error.to_string())
        }
    }
}

fn relief_failure(error: PrimaryReliefGenerationError) -> StageError {
    match error {
        PrimaryReliefGenerationError::Cancelled => cancelled(),
        PrimaryReliefGenerationError::InvalidSurface(_)
        | PrimaryReliefGenerationError::InvalidSurfaceIdentity(_)
        | PrimaryReliefGenerationError::InvalidEvolved(_)
        | PrimaryReliefGenerationError::InvalidSubstrate(_)
        | PrimaryReliefGenerationError::InvalidSpec(_) => invalid_input(error.to_string()),
        PrimaryReliefGenerationError::InvalidReliefField(_)
        | PrimaryReliefGenerationError::InvalidCompatibility(_)
        | PrimaryReliefGenerationError::InvalidWaterSolve(_)
        | PrimaryReliefGenerationError::InvalidLandFractionSelection(_)
        | PrimaryReliefGenerationError::InvalidSnapshot(_) => {
            StageError::new(RELIEF_BUILD_FAILED_CODE, error.to_string())
        }
    }
}

fn quality_failure(error: QualityBuildError) -> StageError {
    match error {
        QualityBuildError::InvalidInput { .. } | QualityBuildError::SurfaceMismatch { .. } => {
            invalid_input(error.to_string())
        }
        error => StageError::new(INVALID_QUALITY_CODE, error.to_string()),
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn cancelled() -> StageError {
    StageError::new(CANCELLED_CODE, "P3 generation was cancelled")
}
