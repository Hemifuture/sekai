//! Typed atomic engine publication for the P5 coupled surface formation.

use std::sync::Arc;

use serde::Serialize;
use thiserror::Error;

use super::quality::{
    evaluate_surface_formation_quality_cancellable, validate_surface_formation_quality_report,
    QualityBuildError,
};
use super::{
    ClimateWorkDomainArtifact, ClimateWorkDomainStage, EvolvedTectonicArtifact,
    EvolvedTectonicStage, GeologicSubstrateArtifact, GeologicSubstrateStage,
    GlobalCirculationArtifact, GlobalCirculationStage, NaturalQualityProfileArtifact,
    PrimaryReliefArtifact, PrimaryReliefStage, ReliefSpecArtifact, ResolvedClimateInputArtifact,
    ResolvedGeologicInputArtifact, ResolvedHydroErosionInputArtifact,
    ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact, SurfaceFormationGenerationError,
    SurfaceFormationGenerator, SurfaceFormationInputs,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts,
    BuildCancellation, Diagnostic, GraphError, Stage, StageError, StageGraph, StageGraphBuilder,
    StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::rules::HydroErosionModel;
use crate::world::natural::{NaturalQualityReport, NaturalSurfaceFormationSnapshot};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

const INVALID_INPUT_CODE: &str = "surface-formation.invalid-input";
const FORMATION_BUILD_FAILED_CODE: &str = "surface-formation.build-failed";
const INVALID_ARTIFACT_CODE: &str = "surface-formation.invalid-artifact";
const INVALID_QUALITY_CODE: &str = "surface-formation.invalid-quality";
const CANCELLED_CODE: &str = "engine.cancelled";

/// Atomic publication of the converged P5 formation state and its verdict.
///
/// The portable [`NaturalSurfaceFormationSnapshot`] stays strict serde data,
/// but this trusted product envelope is Serialize-only: a decoded snapshot can
/// never acquire an Artifact identity without running the locked generator and
/// evaluator over authoritative inputs.
///
/// ```compile_fail
/// use sekai::generators::natural::NaturalSurfaceFormationArtifact;
///
/// let _: NaturalSurfaceFormationArtifact = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NaturalSurfaceFormationArtifact {
    snapshot: NaturalSurfaceFormationSnapshot,
    quality_report: NaturalQualityReport,
}

impl NaturalSurfaceFormationArtifact {
    pub(crate) const fn new(
        snapshot: NaturalSurfaceFormationSnapshot,
        quality_report: NaturalQualityReport,
    ) -> Self {
        Self {
            snapshot,
            quality_report,
        }
    }

    pub const fn snapshot(&self) -> &NaturalSurfaceFormationSnapshot {
        &self.snapshot
    }

    pub const fn quality_report(&self) -> &NaturalQualityReport {
        &self.quality_report
    }

    /// Runs the locked solve and evaluator, then publishes their inseparable
    /// result. This is the only public construction path.
    pub fn generate(
        inputs: SurfaceFormationInputs<'_>,
        cancellation: &BuildCancellation,
    ) -> Result<Self, SurfaceFormationProductError> {
        let surface = inputs.surface;
        let relief = inputs.relief;
        let snapshot = SurfaceFormationGenerator::generate(inputs, cancellation)?;
        Self::evaluate_cancellable(surface, relief, snapshot, cancellation)
            .map_err(SurfaceFormationProductError::from)
    }

    pub(crate) fn evaluate_cancellable(
        surface: &SphericalSurfaceSnapshot,
        relief: &crate::world::natural::PrimaryReliefSnapshot,
        snapshot: NaturalSurfaceFormationSnapshot,
        cancellation: &BuildCancellation,
    ) -> Result<Self, QualityBuildError> {
        let quality_report = evaluate_surface_formation_quality_cancellable(
            surface,
            relief,
            &snapshot,
            cancellation,
        )?;
        let artifact = Self::new(snapshot, quality_report);
        artifact.validate_product(cancellation).map_err(|error| {
            if cancellation.is_cancelled() {
                QualityBuildError::Cancelled
            } else {
                QualityBuildError::InvalidInput {
                    input: "natural_surface_formation_artifact",
                    reason: error.to_string(),
                }
            }
        })?;
        Ok(artifact)
    }

    fn validate_product(
        &self,
        cancellation: &BuildCancellation,
    ) -> Result<(), ArtifactValidationError> {
        let _ = cancellation;
        self.validate_inseparable_pair()
    }

    fn validate_inseparable_pair(&self) -> Result<(), ArtifactValidationError> {
        self.snapshot.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_ARTIFACT_CODE, error.to_string())
        })?;
        validate_surface_formation_quality_report(
            &self.quality_report,
            self.snapshot.surface_ref(),
            self.snapshot.checkpoint().fingerprint(),
            self.snapshot.checkpoint().quality_profile(),
        )
        .map_err(|error| ArtifactValidationError::new(INVALID_QUALITY_CODE, error))
    }
}

impl Artifact for NaturalSurfaceFormationArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.natural-surface-formation");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.validate_inseparable_pair()
    }

    fn validate_cancellable(
        &self,
        cancellation: &BuildCancellation,
    ) -> Result<(), ArtifactValidationError> {
        NaturalSurfaceFormationArtifact::validate_product(self, cancellation)
    }
}

/// Failures from the only public, generator-owned P5 product factory.
#[derive(Debug, Error)]
pub enum SurfaceFormationProductError {
    #[error("surface formation product generation was cancelled")]
    Cancelled,
    #[error(transparent)]
    Generation(SurfaceFormationGenerationError),
    #[error(transparent)]
    Quality(QualityBuildError),
}

impl From<SurfaceFormationGenerationError> for SurfaceFormationProductError {
    fn from(error: SurfaceFormationGenerationError) -> Self {
        if matches!(error, SurfaceFormationGenerationError::Cancelled) {
            Self::Cancelled
        } else {
            Self::Generation(error)
        }
    }
}

impl From<QualityBuildError> for SurfaceFormationProductError {
    fn from(error: QualityBuildError) -> Self {
        if error == QualityBuildError::Cancelled {
            Self::Cancelled
        } else {
            Self::Quality(error)
        }
    }
}

pub struct SurfaceFormationStageInputs {
    profile: Arc<NaturalQualityProfileArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
    tectonics: Arc<EvolvedTectonicArtifact>,
    substrate: Arc<GeologicSubstrateArtifact>,
    relief: Arc<PrimaryReliefArtifact>,
    domain: Arc<ClimateWorkDomainArtifact>,
    climate: Arc<GlobalCirculationArtifact>,
    resolved_climate: Arc<ResolvedClimateInputArtifact>,
    resolved_hydro_erosion: Arc<ResolvedHydroErosionInputArtifact>,
}

impl StageInputs for SurfaceFormationStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            NaturalQualityProfileArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
            EvolvedTectonicArtifact::KEY,
            GeologicSubstrateArtifact::KEY,
            PrimaryReliefArtifact::KEY,
            ClimateWorkDomainArtifact::KEY,
            GlobalCirculationArtifact::KEY,
            ResolvedClimateInputArtifact::KEY,
            ResolvedHydroErosionInputArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            profile: artifacts.get::<NaturalQualityProfileArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
            tectonics: artifacts.get::<EvolvedTectonicArtifact>()?,
            substrate: artifacts.get::<GeologicSubstrateArtifact>()?,
            relief: artifacts.get::<PrimaryReliefArtifact>()?,
            domain: artifacts.get::<ClimateWorkDomainArtifact>()?,
            climate: artifacts.get::<GlobalCirculationArtifact>()?,
            resolved_climate: artifacts.get::<ResolvedClimateInputArtifact>()?,
            resolved_hydro_erosion: artifacts.get::<ResolvedHydroErosionInputArtifact>()?,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceFormationStage;

impl Stage for SurfaceFormationStage {
    type Inputs = SurfaceFormationStageInputs;
    type Output = NaturalSurfaceFormationArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.surface-formation")
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
        rng.check_cancelled().map_err(|_| cancelled())?;
        let cancellation = rng.cancellation_signal();
        let surface = inputs.surface.snapshot();
        let surface_ref = SurfaceRef::from_validated_spherical(surface)
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .resolved_climate
            .input()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        inputs
            .resolved_hydro_erosion
            .input()
            .validate()
            .map_err(|error| invalid_input(error.to_string()))?;
        match inputs.resolved_hydro_erosion.input().model() {
            HydroErosionModel::PriorityFloodStreamPowerV1 => {}
        }
        for (role, found) in [
            (
                "evolved_tectonics",
                inputs.tectonics.snapshot().surface_ref(),
            ),
            (
                "geologic_substrate",
                inputs.substrate.snapshot().surface_ref(),
            ),
            ("primary_relief", inputs.relief.snapshot().surface_ref()),
            (
                "global_circulation",
                inputs.climate.snapshot().surface_ref(),
            ),
        ] {
            if found != surface_ref {
                return Err(invalid_input(format!(
                    "{role} does not match the authoritative surface"
                )));
            }
        }

        let artifact = NaturalSurfaceFormationArtifact::generate(
            SurfaceFormationInputs {
                surface,
                quality_profile: inputs.profile.profile(),
                tectonics: inputs.tectonics.snapshot(),
                substrate: inputs.substrate.snapshot(),
                relief: inputs.relief.snapshot(),
                domain: inputs.domain.snapshot(),
                climate_spec: inputs.resolved_climate.input().spec(),
                initial_climate: inputs.climate.snapshot(),
                formation_spec: inputs.resolved_hydro_erosion.input().spec(),
            },
            &cancellation,
        )
        .map_err(product_failure)?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        Ok(artifact)
    }
}

/// Builds the isolated P5 graph on top of the unchanged P4 product graph.
pub fn surface_formation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<NaturalQualityProfileArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ReliefSpecArtifact>()
        .external::<ResolvedClimateInputArtifact>()
        .external::<ResolvedHydroErosionInputArtifact>()
        .external::<SphericalSurfaceArtifact>()
        .stage(EvolvedTectonicStage)
        .stage(GeologicSubstrateStage)
        .stage(PrimaryReliefStage)
        .stage(ClimateWorkDomainStage)
        .stage(GlobalCirculationStage)
        .stage(SurfaceFormationStage)
        .build()
}

fn product_failure(error: SurfaceFormationProductError) -> StageError {
    match error {
        SurfaceFormationProductError::Cancelled => cancelled(),
        SurfaceFormationProductError::Generation(
            error @ (SurfaceFormationGenerationError::UpstreamSurfaceMismatch { .. }
            | SurfaceFormationGenerationError::QualityProfileMismatch { .. }
            | SurfaceFormationGenerationError::InvalidSurface(_)
            | SurfaceFormationGenerationError::InvalidSpec(_)
            | SurfaceFormationGenerationError::InvalidUpstream(_)),
        ) => invalid_input(error.to_string()),
        SurfaceFormationProductError::Generation(error) => {
            StageError::new(FORMATION_BUILD_FAILED_CODE, error.to_string())
        }
        SurfaceFormationProductError::Quality(
            error @ (QualityBuildError::InvalidInput { .. }
            | QualityBuildError::SurfaceMismatch { .. }),
        ) => invalid_input(error.to_string()),
        SurfaceFormationProductError::Quality(error) => {
            StageError::new(INVALID_QUALITY_CODE, error.to_string())
        }
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn cancelled() -> StageError {
    StageError::new(
        CANCELLED_CODE,
        "P5 surface formation generation was cancelled",
    )
}
