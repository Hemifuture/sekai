//! Typed engine boundary for conservative V5 tectonic publication.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::quality::{
    evaluate_evolved_tectonic_quality, validate_evolved_tectonic_quality_report, QualityBuildError,
};
use super::{
    EvolvedTectonicGenerationError, EvolvedTectonicGenerator, ResolvedTectonicInputArtifact,
    ResolvedWorldFormationArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    GraphError, Stage, StageError, StageGraph, StageGraphBuilder, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::{
    ProfileSurfaceBuildError, ProfileSurfaceBuilder, SphericalSurfaceArtifact,
};
use crate::rules::TectonicModel;
use crate::world::natural::{EvolvedTectonicSnapshot, NaturalQualityProfile, NaturalQualityReport};

const INVALID_PROFILE_CODE: &str = "evolved-tectonics.invalid-profile";
const INVALID_INPUT_CODE: &str = "evolved-tectonics.invalid-input";
const BUILD_FAILED_CODE: &str = "evolved-tectonics.build-failed";
const INVALID_QUALITY_CODE: &str = "evolved-tectonics.invalid-quality";
const INVALID_ARTIFACT_CODE: &str = "evolved-tectonics.invalid-artifact";
const CANCELLED_CODE: &str = "engine.cancelled";

/// Strict external selection of one coordinated natural-world quality level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NaturalQualityProfileArtifact {
    profile: NaturalQualityProfile,
}

impl NaturalQualityProfileArtifact {
    /// Wraps one semantic natural-world quality profile.
    pub const fn new(profile: NaturalQualityProfile) -> Self {
        Self { profile }
    }

    /// Returns the selected profile.
    pub const fn profile(&self) -> NaturalQualityProfile {
        self.profile
    }
}

impl Artifact for NaturalQualityProfileArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.quality-profile");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        let target = self.profile.authoritative_target_cell_count();
        if target == 0 || self.profile.tectonic_control_target_cell_count() == 0 {
            return Err(ArtifactValidationError::new(
                INVALID_PROFILE_CODE,
                "natural quality profile resolves to an empty work grid",
            ));
        }
        Ok(())
    }
}

/// Atomic V5 engine publication: scientific state plus its versioned evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvolvedTectonicArtifact {
    snapshot: EvolvedTectonicSnapshot,
    quality_report: NaturalQualityReport,
}

impl EvolvedTectonicArtifact {
    /// Combines a V5 snapshot with quality evidence for the same authority.
    pub const fn new(
        snapshot: EvolvedTectonicSnapshot,
        quality_report: NaturalQualityReport,
    ) -> Self {
        Self {
            snapshot,
            quality_report,
        }
    }

    /// Returns the immutable evolved tectonic state.
    pub const fn snapshot(&self) -> &EvolvedTectonicSnapshot {
        &self.snapshot
    }

    /// Returns the immutable versioned P2 quality report.
    pub const fn quality_report(&self) -> &NaturalQualityReport {
        &self.quality_report
    }
}

impl Artifact for EvolvedTectonicArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.evolved-tectonics");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string())
        })?;
        validate_evolved_tectonic_quality_report(&self.quality_report, self.snapshot.surface_ref())
            .map_err(|error| ArtifactValidationError::new(INVALID_QUALITY_CODE, error))?;
        Ok(())
    }
}

/// The exact four-artifact input boundary visible to V5 tectonics.
pub struct EvolvedTectonicStageInputs {
    profile: Arc<NaturalQualityProfileArtifact>,
    resolved_input: Arc<ResolvedTectonicInputArtifact>,
    formation: Arc<ResolvedWorldFormationArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for EvolvedTectonicStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            NaturalQualityProfileArtifact::KEY,
            ResolvedTectonicInputArtifact::KEY,
            ResolvedWorldFormationArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            profile: artifacts.get::<NaturalQualityProfileArtifact>()?,
            resolved_input: artifacts.get::<ResolvedTectonicInputArtifact>()?,
            formation: artifacts.get::<ResolvedWorldFormationArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

/// Deterministic conservative tectonic stage isolated at version 5.
#[derive(Debug, Clone, Copy, Default)]
pub struct EvolvedTectonicStage;

impl Stage for EvolvedTectonicStage {
    type Inputs = EvolvedTectonicStageInputs;
    type Output = EvolvedTectonicArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.evolved-tectonics")
    }

    fn version(&self) -> u32 {
        5
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
            .resolved_input
            .input()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .formation
            .formation()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        match inputs.resolved_input.input().model() {
            TectonicModel::CurrentSliceV1 => {}
        }

        let cancellation = rng.cancellation_signal();
        let bundle = ProfileSurfaceBuilder::complete(
            inputs.profile.profile(),
            inputs.surface.snapshot(),
            &cancellation,
        )
        .map_err(profile_failure)?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        let snapshot = EvolvedTectonicGenerator::generate(
            &bundle,
            inputs.resolved_input.input().spec(),
            inputs.formation.formation(),
            rng,
        )
        .map_err(generation_failure)?;
        snapshot
            .validate_against(inputs.surface.snapshot())
            .map_err(|error| invalid_input(error.to_string()))?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        let quality_report =
            evaluate_evolved_tectonic_quality(inputs.surface.snapshot(), &snapshot)
                .map_err(quality_failure)?;
        let artifact = EvolvedTectonicArtifact::new(snapshot, quality_report);
        artifact
            .validate()
            .map_err(|error| StageError::new(error.code(), error.message()))?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        Ok(artifact)
    }
}

/// Builds the isolated V5 tectonic graph while leaving the frozen V4 graph intact.
pub fn evolved_tectonic_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<NaturalQualityProfileArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<SphericalSurfaceArtifact>()
        .stage(EvolvedTectonicStage)
        .build()
}

fn profile_failure(error: ProfileSurfaceBuildError) -> StageError {
    match error {
        ProfileSurfaceBuildError::Cancelled => cancelled(),
        ProfileSurfaceBuildError::Profile(_)
        | ProfileSurfaceBuildError::InvalidAuthoritativeSurface(_)
        | ProfileSurfaceBuildError::ResolvedCellCountMismatch {
            role: "authoritative",
            ..
        }
        | ProfileSurfaceBuildError::RadiusMismatch { .. }
        | ProfileSurfaceBuildError::InvalidSurfaceIdentity {
            role: "authoritative",
            ..
        } => invalid_input(error.to_string()),
        error => StageError::new(BUILD_FAILED_CODE, error.to_string()),
    }
}

fn generation_failure(error: EvolvedTectonicGenerationError) -> StageError {
    match error {
        EvolvedTectonicGenerationError::Cancelled => cancelled(),
        EvolvedTectonicGenerationError::InvalidSpec(_)
        | EvolvedTectonicGenerationError::InvalidFormation(_)
        | EvolvedTectonicGenerationError::InvalidBundle(_) => invalid_input(error.to_string()),
        EvolvedTectonicGenerationError::Generation(_) => {
            StageError::new(BUILD_FAILED_CODE, error.to_string())
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
    StageError::new(CANCELLED_CODE, "evolved tectonic generation was cancelled")
}
