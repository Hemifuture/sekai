//! Typed engine publication for the P4 climate work domain and global circulation.

use std::sync::Arc;

use serde::Serialize;
use thiserror::Error;

use super::quality::{
    evaluate_global_circulation_quality_cancellable, validate_global_circulation_quality_report,
    QualityBuildError,
};
use super::{
    climate_work_domain::{
        validate_climate_work_domain_reconstruction,
        validate_climate_work_domain_reconstruction_cancellable,
    },
    ClimateWorkDomainBuildError, ClimateWorkDomainBuilder, EvolvedTectonicStage,
    GeologicSubstrateStage, GlobalCirculationGenerationError, GlobalCirculationGenerator,
    GlobalClimateForcing, GlobalClimateForcingBuilder, GlobalClimateForcingError,
    NaturalQualityProfileArtifact, PrimaryReliefArtifact, PrimaryReliefStage, ReliefSpecArtifact,
    ResolvedClimateInputArtifact, ResolvedGeologicInputArtifact, ResolvedTectonicInputArtifact,
    ResolvedWorldFormationArtifact, SELECTED_PRODUCTION_INTEGRATOR,
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
///
/// The portable [`ClimateWorkDomainSnapshot`] remains strict serde data, but
/// this trusted product envelope is Serialize-only. A decoded snapshot must be
/// rebound to its authoritative surface through [`Self::rehydrate`] so forged
/// overlap support or tangent transforms cannot acquire an Artifact identity.
///
/// ```compile_fail
/// use sekai::generators::natural::ClimateWorkDomainArtifact;
///
/// let _: ClimateWorkDomainArtifact = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateWorkDomainArtifact {
    snapshot: ClimateWorkDomainSnapshot,
}

impl ClimateWorkDomainArtifact {
    pub(crate) const fn new(snapshot: ClimateWorkDomainSnapshot) -> Self {
        Self { snapshot }
    }

    pub fn build(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        profile: crate::world::natural::NaturalQualityProfile,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<Self, ClimateWorkDomainBuildError> {
        ClimateWorkDomainBuilder::build(surface, profile, cancellation).map(Self::new)
    }

    pub fn rehydrate(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        snapshot: ClimateWorkDomainSnapshot,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<Self, ClimateWorkDomainBuildError> {
        snapshot
            .validate_against_cancellable(surface, &|| cancellation.is_cancelled())
            .map_err(|error| {
                if error == crate::world::natural::ClimateWorkDomainValidationError::Cancelled {
                    ClimateWorkDomainBuildError::Cancelled
                } else {
                    ClimateWorkDomainBuildError::Validation(error)
                }
            })?;
        Ok(Self::new(snapshot))
    }

    pub const fn snapshot(&self) -> &ClimateWorkDomainSnapshot {
        &self.snapshot
    }

    fn validate_cancellable(
        &self,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<(), ArtifactValidationError> {
        let cancelled = || cancellation.is_cancelled();
        self.snapshot
            .validate_cancellable(&cancelled)
            .map_err(|error| {
                ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string())
            })?;
        validate_climate_work_domain_reconstruction_cancellable(&self.snapshot, &cancelled)
            .map_err(|error| ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string()))
    }
}

impl Artifact for ClimateWorkDomainArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.climate-work-domain");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string())
        })?;
        validate_climate_work_domain_reconstruction(&self.snapshot)
            .map_err(|error| ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string()))
    }

    fn validate_cancellable(
        &self,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<(), ArtifactValidationError> {
        ClimateWorkDomainArtifact::validate_cancellable(self, cancellation)
    }
}

/// Atomic publication of C2 fields and their exact per-world quality evidence.
///
/// The raw `(snapshot, report)` constructor is intentionally crate-private and
/// this product does not implement [`Deserialize`]. Callers must use
/// [`GlobalCirculationArtifact::generate`], which runs the locked C2 generator
/// and evaluator over authoritative inputs. That keeps structurally valid but
/// fabricated solve, budget, remap, or quality reports from becoming a product
/// artifact.
///
/// ```compile_fail
/// use sekai::generators::natural::GlobalCirculationArtifact;
/// use sekai::world::natural::{GlobalCirculationSnapshot, NaturalQualityReport};
///
/// fn forge(snapshot: GlobalCirculationSnapshot, report: NaturalQualityReport) {
///     let _ = GlobalCirculationArtifact::new(snapshot, report);
/// }
/// ```
///
/// ```compile_fail
/// use sekai::generators::natural::GlobalCirculationArtifact;
///
/// let _: GlobalCirculationArtifact = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCirculationArtifact {
    snapshot: GlobalCirculationSnapshot,
    quality_report: NaturalQualityReport,
}

impl GlobalCirculationArtifact {
    pub(crate) const fn new(
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

    /// Runs the selected generator and locked evaluator, then publishes their
    /// inseparable result. This is the only public construction path.
    pub fn generate(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &GlobalClimateForcing,
        relief: &crate::world::natural::PrimaryReliefSnapshot,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<Self, GlobalCirculationProductError> {
        domain
            .validate_against_cancellable(surface, &|| cancellation.is_cancelled())
            .map_err(|error| {
                if error == crate::world::natural::ClimateWorkDomainValidationError::Cancelled {
                    GlobalCirculationProductError::Cancelled
                } else {
                    GlobalCirculationProductError::InvalidDomain(error.to_string())
                }
            })?;
        forcing.validate_relief_identity_cancellable(relief, cancellation)?;
        let snapshot = GlobalCirculationGenerator::generate_from_validated(
            surface,
            domain,
            forcing,
            ClimateModelProfile::C2LayeredV1,
            cancellation,
        )?;
        Self::evaluate_cancellable(surface, relief, forcing, snapshot, cancellation)
            .map_err(GlobalCirculationProductError::from)
    }

    pub(crate) fn evaluate_cancellable(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        relief: &crate::world::natural::PrimaryReliefSnapshot,
        forcing: &GlobalClimateForcing,
        snapshot: GlobalCirculationSnapshot,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<Self, QualityBuildError> {
        let quality_report = evaluate_global_circulation_quality_cancellable(
            surface,
            relief,
            forcing,
            &snapshot,
            cancellation,
        )?;
        let artifact = Self::new(snapshot, quality_report);
        artifact
            .validate_cancellable(cancellation)
            .map_err(|error| {
                if cancellation.is_cancelled() {
                    QualityBuildError::Cancelled
                } else {
                    QualityBuildError::InvalidInput {
                        input: "global_circulation_artifact",
                        reason: error.to_string(),
                    }
                }
            })?;
        Ok(artifact)
    }

    fn validate_cancellable(
        &self,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<(), ArtifactValidationError> {
        self.snapshot
            .validate_cancellable(&|| cancellation.is_cancelled())
            .map_err(|error| {
                ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string())
            })?;
        if self.snapshot.integrator() != SELECTED_PRODUCTION_INTEGRATOR {
            return Err(ArtifactValidationError::new(
                INVALID_ARTIFACT_CODE,
                "only the locked split-explicit integrator may own a product artifact",
            ));
        }
        validate_global_circulation_quality_report(
            &self.quality_report,
            self.snapshot.surface_ref(),
            self.snapshot.checkpoint().fingerprint(),
        )
        .map_err(|error| ArtifactValidationError::new(INVALID_QUALITY_CODE, error))?;
        Ok(())
    }
}

/// Failures from the only public, generator-owned P4 product factory.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GlobalCirculationProductError {
    #[error("global circulation product generation was cancelled")]
    Cancelled,
    #[error("invalid climate work domain: {0}")]
    InvalidDomain(String),
    #[error(transparent)]
    Forcing(GlobalClimateForcingError),
    #[error(transparent)]
    Generation(GlobalCirculationGenerationError),
    #[error(transparent)]
    Quality(QualityBuildError),
}

impl From<GlobalClimateForcingError> for GlobalCirculationProductError {
    fn from(error: GlobalClimateForcingError) -> Self {
        if error == GlobalClimateForcingError::Cancelled {
            Self::Cancelled
        } else {
            Self::Forcing(error)
        }
    }
}

impl From<GlobalCirculationGenerationError> for GlobalCirculationProductError {
    fn from(error: GlobalCirculationGenerationError) -> Self {
        if matches!(
            &error,
            GlobalCirculationGenerationError::Cancelled
                | GlobalCirculationGenerationError::Forcing(GlobalClimateForcingError::Cancelled)
        ) {
            Self::Cancelled
        } else {
            Self::Generation(error)
        }
    }
}

impl From<QualityBuildError> for GlobalCirculationProductError {
    fn from(error: QualityBuildError) -> Self {
        if error == QualityBuildError::Cancelled {
            Self::Cancelled
        } else {
            Self::Quality(error)
        }
    }
}

impl Artifact for GlobalCirculationArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.global-circulation");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string())
        })?;
        if self.snapshot.integrator() != SELECTED_PRODUCTION_INTEGRATOR {
            return Err(ArtifactValidationError::new(
                INVALID_ARTIFACT_CODE,
                "only the locked split-explicit integrator may own a product artifact",
            ));
        }
        validate_global_circulation_quality_report(
            &self.quality_report,
            self.snapshot.surface_ref(),
            self.snapshot.checkpoint().fingerprint(),
        )
        .map_err(|error| ArtifactValidationError::new(INVALID_QUALITY_CODE, error))?;
        Ok(())
    }

    fn validate_cancellable(
        &self,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<(), ArtifactValidationError> {
        GlobalCirculationArtifact::validate_cancellable(self, cancellation)
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
        let cancellation = rng.cancellation_signal();
        let snapshot = ClimateWorkDomainBuilder::build(
            inputs.surface.snapshot(),
            inputs.profile.profile(),
            &cancellation,
        )
        .map_err(domain_failure)?;
        let artifact = ClimateWorkDomainArtifact::new(snapshot);
        artifact
            .validate_cancellable(&cancellation)
            .map_err(|error| {
                if cancellation.is_cancelled() {
                    cancelled()
                } else {
                    StageError::new(error.code(), error.message())
                }
            })?;
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
        rng.check_cancelled().map_err(|_| cancelled())?;
        let cancellation = rng.cancellation_signal();
        let surface = inputs.surface.snapshot();
        inputs
            .resolved_climate
            .input()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        let surface_ref = crate::world::spatial::SurfaceRef::from_validated_spherical(surface)
            .map_err(|error| invalid_input(error.to_string()))?;
        if inputs.relief.snapshot().surface_ref() != surface_ref {
            return Err(invalid_input(
                "primary relief does not match the authoritative surface".to_owned(),
            ));
        }
        match inputs.resolved_climate.input().model() {
            ClimateModel::SeasonalEnergyMoistureV1 => {}
        }
        let forcing = GlobalClimateForcingBuilder::build(
            surface,
            inputs.relief.snapshot(),
            inputs.resolved_climate.input().spec(),
            inputs.domain.snapshot(),
            &cancellation,
        )
        .map_err(forcing_failure)?;
        let snapshot = GlobalCirculationGenerator::generate_from_validated(
            surface,
            inputs.domain.snapshot(),
            &forcing,
            ClimateModelProfile::C2LayeredV1,
            &cancellation,
        )
        .map_err(circulation_failure)?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        let artifact = GlobalCirculationArtifact::evaluate_cancellable(
            surface,
            inputs.relief.snapshot(),
            &forcing,
            snapshot,
            &cancellation,
        )
        .map_err(quality_failure)?;
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
        | ClimateWorkDomainBuildError::ReconstructionMismatch
        | ClimateWorkDomainBuildError::CanonicalMapMismatch => {
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
        | GlobalClimateForcingError::PayloadIdentityMismatch { .. }
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
        QualityBuildError::Cancelled => cancelled(),
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
