//! Typed engine publication for the P4 climate work domain.

use std::sync::Arc;

use serde::Serialize;

use super::climate_work_domain::{
    validate_climate_work_domain_reconstruction,
    validate_climate_work_domain_reconstruction_cancellable,
};
use super::{ClimateWorkDomainBuildError, ClimateWorkDomainBuilder, NaturalQualityProfileArtifact};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::world::natural::ClimateWorkDomainSnapshot;

const INVALID_INPUT_CODE: &str = "global-circulation.invalid-input";
const DOMAIN_BUILD_FAILED_CODE: &str = "global-circulation.work-domain-build-failed";
const INVALID_ARTIFACT_CODE: &str = "global-circulation.invalid-artifact";
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

    /// Builds a work-domain artifact for one authoritative surface and profile.
    pub fn build(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        profile: crate::world::natural::NaturalQualityProfile,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<Self, ClimateWorkDomainBuildError> {
        ClimateWorkDomainBuilder::build(surface, profile, cancellation).map(Self::new)
    }

    /// Revalidates a portable work-domain snapshot against its source surface.
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

    /// Borrows the reconstructable work-domain snapshot.
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

/// Dependencies for climate work-domain construction.
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

/// Builds the reusable P4 climate work domain.
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

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn cancelled() -> StageError {
    StageError::new(CANCELLED_CODE, "P4 climate work-domain build was cancelled")
}
