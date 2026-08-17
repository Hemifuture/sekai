//! Typed engine publication for the P4 climate work domain and global circulation.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::quality::{
    evaluate_global_circulation_quality, validate_global_circulation_quality_report,
    QualityBuildError,
};
use super::{
    ClimateWorkDomainBuildError, ClimateWorkDomainBuilder, EvolvedTectonicStage,
    GeologicSubstrateStage, GlobalCirculationGenerationError, GlobalCirculationGenerator,
    GlobalClimateForcingBuilder, GlobalClimateForcingError, NaturalQualityProfileArtifact,
    PrimaryReliefArtifact, PrimaryReliefStage, ReliefSpecArtifact, ResolvedClimateInputArtifact,
    ResolvedGeologicInputArtifact, ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    GraphError, Stage, StageError, StageGraph, StageGraphBuilder, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::rules::ClimateModel;
use crate::world::natural::{
    ClimateModelProfile, ClimateWorkDomainSnapshot, GlobalCirculationSnapshot, NaturalQualityReport,
};

const INVALID_INPUT_CODE: &str = "global-circulation.invalid-input";
const DOMAIN_BUILD_FAILED_CODE: &str = "global-circulation.work-domain-build-failed";
const FORCING_BUILD_FAILED_CODE: &str = "global-circulation.forcing-build-failed";
const CIRCULATION_BUILD_FAILED_CODE: &str = "global-circulation.build-failed";
const INVALID_ARTIFACT_CODE: &str = "global-circulation.invalid-artifact";
const INVALID_QUALITY_CODE: &str = "global-circulation.invalid-quality";
const CANCELLED_CODE: &str = "engine.cancelled";

/// Atomic publication of the reconstructable cubed-sphere climate work domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateWorkDomainArtifact {
    snapshot: ClimateWorkDomainSnapshot,
}

impl ClimateWorkDomainArtifact {
    pub const fn new(snapshot: ClimateWorkDomainSnapshot) -> Self {
        Self { snapshot }
    }

    pub const fn snapshot(&self) -> &ClimateWorkDomainSnapshot {
        &self.snapshot
    }
}

impl Artifact for ClimateWorkDomainArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.climate-work-domain");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string()))
    }
}

/// Atomic publication of C2 fields and their exact per-world quality evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCirculationArtifact {
    snapshot: GlobalCirculationSnapshot,
    quality_report: NaturalQualityReport,
}

impl GlobalCirculationArtifact {
    pub const fn new(
        snapshot: GlobalCirculationSnapshot,
        quality_report: NaturalQualityReport,
    ) -> Self {
        Self {
            snapshot,
            quality_report,
        }
    }

    pub const fn snapshot(&self) -> &GlobalCirculationSnapshot {
        &self.snapshot
    }

    pub const fn quality_report(&self) -> &NaturalQualityReport {
        &self.quality_report
    }
}

impl Artifact for GlobalCirculationArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.global-circulation");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string())
        })?;
        validate_global_circulation_quality_report(
            &self.quality_report,
            self.snapshot.surface_ref(),
        )
        .map_err(|error| ArtifactValidationError::new(INVALID_QUALITY_CODE, error))?;
        Ok(())
    }
}

pub struct ClimateWorkDomainStageInputs {
    profile: Arc<NaturalQualityProfileArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for ClimateWorkDomainStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            NaturalQualityProfileArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            profile: artifacts.get::<NaturalQualityProfileArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ClimateWorkDomainStage;

impl Stage for ClimateWorkDomainStage {
    type Inputs = ClimateWorkDomainStageInputs;
    type Output = ClimateWorkDomainArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.climate-work-domain")
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
        let cancellation = rng.cancellation_signal();
        let snapshot = ClimateWorkDomainBuilder::build(
            inputs.surface.snapshot(),
            inputs.profile.profile(),
            &cancellation,
        )
        .map_err(domain_failure)?;
        let artifact = ClimateWorkDomainArtifact::new(snapshot);
        artifact
            .validate()
            .map_err(|error| StageError::new(error.code(), error.message()))?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        Ok(artifact)
    }
}

pub struct GlobalCirculationStageInputs {
    resolved_climate: Arc<ResolvedClimateInputArtifact>,
    domain: Arc<ClimateWorkDomainArtifact>,
    relief: Arc<PrimaryReliefArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for GlobalCirculationStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedClimateInputArtifact::KEY,
            ClimateWorkDomainArtifact::KEY,
            PrimaryReliefArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolved_climate: artifacts.get::<ResolvedClimateInputArtifact>()?,
            domain: artifacts.get::<ClimateWorkDomainArtifact>()?,
            relief: artifacts.get::<PrimaryReliefArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalCirculationStage;

impl Stage for GlobalCirculationStage {
    type Inputs = GlobalCirculationStageInputs;
    type Output = GlobalCirculationArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.global-circulation")
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
        let surface = inputs.surface.snapshot();
        inputs
            .resolved_climate
            .input()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .domain
            .snapshot()
            .validate_against(surface)
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .relief
            .snapshot()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        if inputs.relief.snapshot().surface_ref()
            != crate::world::spatial::SurfaceRef::for_spherical(surface)
        {
            return Err(invalid_input(
                "primary relief does not match the authoritative surface".to_owned(),
            ));
        }
        match inputs.resolved_climate.input().model() {
            ClimateModel::SeasonalEnergyMoistureV1 => {}
        }
        let cancellation = rng.cancellation_signal();
        let forcing = GlobalClimateForcingBuilder::build(
            surface,
            inputs.relief.snapshot(),
            inputs.resolved_climate.input().spec(),
            inputs.domain.snapshot(),
            &cancellation,
        )
        .map_err(forcing_failure)?;
        let snapshot = GlobalCirculationGenerator::generate(
            surface,
            inputs.domain.snapshot(),
            &forcing,
            ClimateModelProfile::C2LayeredV1,
            &cancellation,
        )
        .map_err(circulation_failure)?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        let quality_report =
            evaluate_global_circulation_quality(surface, inputs.relief.snapshot(), &snapshot)
                .map_err(quality_failure)?;
        let artifact = GlobalCirculationArtifact::new(snapshot, quality_report);
        artifact
            .validate()
            .map_err(|error| StageError::new(error.code(), error.message()))?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        Ok(artifact)
    }
}

/// Builds the isolated P4 graph without changing any earlier product graph.
pub fn global_circulation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<NaturalQualityProfileArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ReliefSpecArtifact>()
        .external::<ResolvedClimateInputArtifact>()
        .external::<SphericalSurfaceArtifact>()
        .stage(EvolvedTectonicStage)
        .stage(GeologicSubstrateStage)
        .stage(PrimaryReliefStage)
        .stage(ClimateWorkDomainStage)
        .stage(GlobalCirculationStage)
        .build()
}

fn domain_failure(error: ClimateWorkDomainBuildError) -> StageError {
    match error {
        ClimateWorkDomainBuildError::Cancelled => cancelled(),
        ClimateWorkDomainBuildError::InvalidSource { .. }
        | ClimateWorkDomainBuildError::Validation(_) => invalid_input(error.to_string()),
        ClimateWorkDomainBuildError::CubedSphere(_)
        | ClimateWorkDomainBuildError::ConservativeRemap { .. }
        | ClimateWorkDomainBuildError::ReconstructionMismatch => {
            StageError::new(DOMAIN_BUILD_FAILED_CODE, error.to_string())
        }
    }
}

fn forcing_failure(error: GlobalClimateForcingError) -> StageError {
    match error {
        GlobalClimateForcingError::Cancelled => cancelled(),
        GlobalClimateForcingError::InvalidInput { .. }
        | GlobalClimateForcingError::Relief(_)
        | GlobalClimateForcingError::WorkDomain(_)
        | GlobalClimateForcingError::SourceMismatch
        | GlobalClimateForcingError::GridMismatch
        | GlobalClimateForcingError::FieldLengthMismatch { .. }
        | GlobalClimateForcingError::ValueOutOfRange { .. } => invalid_input(error.to_string()),
        GlobalClimateForcingError::CubedSphere(_)
        | GlobalClimateForcingError::Remap(_)
        | GlobalClimateForcingError::Operator(_)
        | GlobalClimateForcingError::InvalidForcing { .. }
        | GlobalClimateForcingError::FingerprintMismatch => {
            StageError::new(FORCING_BUILD_FAILED_CODE, error.to_string())
        }
    }
}

fn circulation_failure(error: GlobalCirculationGenerationError) -> StageError {
    match error {
        GlobalCirculationGenerationError::Cancelled
        | GlobalCirculationGenerationError::Forcing(GlobalClimateForcingError::Cancelled) => {
            cancelled()
        }
        GlobalCirculationGenerationError::InvalidLayout { .. }
        | GlobalCirculationGenerationError::InvalidForcing { .. }
        | GlobalCirculationGenerationError::WorkDomain(_)
        | GlobalCirculationGenerationError::Forcing(_) => invalid_input(error.to_string()),
        _ => StageError::new(CIRCULATION_BUILD_FAILED_CODE, error.to_string()),
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
    StageError::new(
        CANCELLED_CODE,
        "P4 global circulation generation was cancelled",
    )
}
