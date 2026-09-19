//! Typed engine adapter for the final spherical natural-quality report.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::super::quality::{
    evaluate_spherical_foundation_quality_from_validated,
    validate_spherical_quality_input_identities,
};
use super::super::{
    ReliefSpecArtifact, ResolvedWorldFormationArtifact, SphericalHydroErosionArtifact,
    SphericalReliefArtifact, SphericalTectonicArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::world::natural::NaturalQualityReport;

const INVALID_INPUT_CODE: &str = "spherical-natural.invalid-quality-input";
const INVALID_REPORT_CODE: &str = "spherical-natural.invalid-quality-report";

/// Engine transport for the final surface-bound natural-quality evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NaturalQualityArtifact {
    report: NaturalQualityReport,
}

impl NaturalQualityArtifact {
    /// Wraps one validated report for engine transport.
    pub const fn new(report: NaturalQualityReport) -> Self {
        Self { report }
    }

    /// Returns the immutable report.
    pub const fn report(&self) -> &NaturalQualityReport {
        &self.report
    }
}

impl Artifact for NaturalQualityArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.natural-quality");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.report
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_REPORT_CODE, error.to_string()))
    }
}

/// The exact scientific artifacts visible to [`SphericalNaturalQualityStage`].
pub struct SphericalNaturalQualityStageInputs {
    formation: Arc<ResolvedWorldFormationArtifact>,
    hydro_erosion: Arc<SphericalHydroErosionArtifact>,
    relief: Arc<SphericalReliefArtifact>,
    relief_spec: Arc<ReliefSpecArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
    tectonic: Arc<SphericalTectonicArtifact>,
}

impl StageInputs for SphericalNaturalQualityStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            ResolvedWorldFormationArtifact::KEY,
            SphericalHydroErosionArtifact::KEY,
            SphericalReliefArtifact::KEY,
            ReliefSpecArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
            SphericalTectonicArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            formation: artifacts.get::<ResolvedWorldFormationArtifact>()?,
            hydro_erosion: artifacts.get::<SphericalHydroErosionArtifact>()?,
            relief: artifacts.get::<SphericalReliefArtifact>()?,
            relief_spec: artifacts.get::<ReliefSpecArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
            tectonic: artifacts.get::<SphericalTectonicArtifact>()?,
        })
    }
}

/// Publishes the deterministic quality report after the complete spherical foundation.
#[derive(Debug, Clone, Copy, Default)]
pub struct SphericalNaturalQualityStage;

impl Stage for SphericalNaturalQualityStage {
    type Inputs = SphericalNaturalQualityStageInputs;
    type Output = NaturalQualityArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.spherical-quality")
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
        validate_spherical_quality_input_identities(
            inputs.surface.snapshot(),
            inputs.formation.formation(),
            inputs.relief_spec.spec(),
            inputs.tectonic.snapshot(),
            inputs.relief.snapshot(),
            inputs.hydro_erosion.snapshot(),
        )
        .map_err(|error| invalid_input(error.to_string()))?;
        let report = evaluate_spherical_foundation_quality_from_validated(
            inputs.surface.snapshot(),
            inputs.formation.formation(),
            inputs.relief_spec.spec(),
            inputs.tectonic.snapshot(),
            inputs.relief.snapshot(),
            inputs.hydro_erosion.snapshot(),
        )
        .map_err(|error| invalid_report(error.to_string()))?;
        report
            .validate()
            .map_err(|error| invalid_report(error.to_string()))?;
        Ok(NaturalQualityArtifact::new(report))
    }
}

fn invalid_input(message: String) -> StageError {
    StageError::new(INVALID_INPUT_CODE, message)
}

fn invalid_report(message: String) -> StageError {
    StageError::new(INVALID_REPORT_CODE, message)
}
