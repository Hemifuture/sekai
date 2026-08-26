//! Atomic engine publication for one final P2/P3/P4/P5 formation bundle.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::super::quality::{
    evaluate_evolved_tectonic_quality,
    evaluate_global_circulation_quality_for_formation_cancellable, evaluate_primary_relief_quality,
    evaluate_surface_formation_quality_cancellable, validate_evolved_tectonic_quality_report,
    validate_global_circulation_quality_report, validate_primary_relief_quality_report,
    validate_surface_formation_quality_report, QualityBuildError,
};
use super::super::{
    ClimateWorkDomainArtifact, ClimateWorkDomainStage, ReliefSpecArtifact,
    ResolvedClimateInputArtifact, ResolvedGeologicInputArtifact, ResolvedHydroErosionInputArtifact,
    ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact, SurfaceFormationGenerationError,
};
use super::causal::{
    CausalFormationGenerationError, CausalFormationOutput, CausalNaturalFormationGenerator,
    CausalNaturalFormationInputs,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts,
    BuildCancellation, Diagnostic, GraphError, Stage, StageError, StageGraph, StageGraphBuilder,
    StageId, StageInputs, StageRng,
};
use crate::generators::spatial::{
    ProfileSurfaceBuildError, ProfileSurfaceBuilder, SphericalSurfaceArtifact,
};
use crate::world::natural::{
    NaturalFormationBundle, NaturalFormationBundleParts, NaturalFormationBundleValidationError,
    NaturalQualityProfile, NATURAL_FORMATION_BUNDLE_SCHEMA_V1,
};
use crate::world::spatial::SurfaceRef;

const INVALID_INPUT_CODE: &str = "causal-formation.invalid-input";
const INVALID_PROFILE_CODE: &str = "causal-formation.invalid-profile";
const NUMERICAL_STABILITY_CODE: &str = "causal-formation.numerical-stability";
const SOLID_BUDGET_CODE: &str = "causal-formation.solid-budget";
const SEDIMENT_BUDGET_CODE: &str = "causal-formation.sediment-budget";
const WATER_BUDGET_CODE: &str = "causal-formation.water-budget";
const CLIMATE_NOT_CONVERGED_CODE: &str = "causal-formation.climate-not-converged";
const ENDPOINT_FORCING_MISMATCH_CODE: &str = "causal-formation.endpoint-forcing-mismatch";
const ELEVATION_OUT_OF_RANGE_CODE: &str = "causal-formation.elevation-out-of-range";
const RESOURCE_LIMIT_CODE: &str = "causal-formation.resource-limit";
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
        if self.profile.authoritative_target_cell_count() == 0
            || self.profile.tectonic_control_target_cell_count() == 0
        {
            return Err(ArtifactValidationError::new(
                INVALID_PROFILE_CODE,
                "natural quality profile resolves to an empty work grid",
            ));
        }
        Ok(())
    }
}

/// Serialize-only engine envelope for one validated formation current state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NaturalFormationBundleArtifact {
    bundle: NaturalFormationBundle,
}

impl NaturalFormationBundleArtifact {
    fn generate(
        inputs: CausalNaturalFormationInputs<'_>,
        rng: &mut StageRng,
        cancellation: &BuildCancellation,
    ) -> Result<Self, NaturalFormationBundleGenerationError> {
        let output = CausalNaturalFormationGenerator::generate_working(inputs, rng, cancellation)?;
        Self::from_output(inputs, output, cancellation)
    }

    fn from_output(
        inputs: CausalNaturalFormationInputs<'_>,
        output: CausalFormationOutput,
        cancellation: &BuildCancellation,
    ) -> Result<Self, NaturalFormationBundleGenerationError> {
        let surface = inputs.profile_bundle.authoritative_surface();
        output
            .final_climate_forcing
            .validate_formation_terrain_identity_cancellable(
                output.surface.terrain_fields(),
                cancellation,
            )?;
        if output.final_climate.checkpoint().forcing_fingerprint()
            != output.final_climate_forcing.fingerprint()
        {
            return Err(NaturalFormationBundleGenerationError::EndpointForcingMismatch);
        }

        let tectonic_quality =
            evaluate_evolved_tectonic_quality(surface, &output.evolved_tectonics)?;
        let primary_relief_quality = evaluate_primary_relief_quality(
            surface,
            &output.evolved_tectonics,
            &output.geologic_substrate,
            &output.primary_relief,
        )?;
        let climate_quality = evaluate_global_circulation_quality_for_formation_cancellable(
            surface,
            output.surface.terrain_fields(),
            &output.final_climate_forcing,
            &output.final_climate,
            cancellation,
        )?;
        let surface_quality = evaluate_surface_formation_quality_cancellable(
            surface,
            &output.primary_relief,
            &output.surface,
            cancellation,
        )?;
        let bundle = NaturalFormationBundle::new(NaturalFormationBundleParts {
            schema_version: NATURAL_FORMATION_BUNDLE_SCHEMA_V1,
            surface_ref: SurfaceRef::for_spherical(surface),
            timeline: inputs.formation.timeline(),
            tectonics: output.evolved_tectonics,
            substrate: output.geologic_substrate,
            primary_relief: output.primary_relief,
            climate: output.final_climate,
            surface_formation: output.surface,
            tectonic_quality,
            primary_relief_quality,
            climate_quality,
            surface_quality,
        })?;
        let artifact = Self { bundle };
        artifact
            .validate_product()
            .map_err(|error| NaturalFormationBundleGenerationError::Product(error.to_string()))?;
        Ok(artifact)
    }

    /// Returns the single validated current-state bundle.
    pub const fn bundle(&self) -> &NaturalFormationBundle {
        &self.bundle
    }

    fn validate_product(&self) -> Result<(), ArtifactValidationError> {
        self.bundle
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_INPUT_CODE, error.to_string()))?;
        validate_evolved_tectonic_quality_report(
            self.bundle.tectonic_quality(),
            self.bundle.surface_ref(),
        )
        .map_err(|error| ArtifactValidationError::new(SOLID_BUDGET_CODE, error))?;
        validate_primary_relief_quality_report(
            self.bundle.primary_relief_quality(),
            self.bundle.surface_ref(),
        )
        .map_err(|error| ArtifactValidationError::new(INVALID_INPUT_CODE, error))?;
        validate_global_circulation_quality_report(
            self.bundle.climate_quality(),
            self.bundle.surface_ref(),
            self.bundle.climate().checkpoint().fingerprint(),
        )
        .map_err(|error| ArtifactValidationError::new(CLIMATE_NOT_CONVERGED_CODE, error))?;
        validate_surface_formation_quality_report(
            self.bundle.surface_quality(),
            self.bundle.surface_ref(),
            self.bundle.surface_formation().checkpoint().fingerprint(),
            self.bundle
                .surface_formation()
                .checkpoint()
                .quality_profile(),
        )
        .map_err(|error| ArtifactValidationError::new(SEDIMENT_BUDGET_CODE, error))
    }
}

impl Artifact for NaturalFormationBundleArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.natural-formation-bundle");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.validate_product()
    }
}

/// Dependencies for the sole formation publication stage.
pub struct CausalNaturalFormationStageInputs {
    profile: Arc<NaturalQualityProfileArtifact>,
    resolved_tectonic: Arc<ResolvedTectonicInputArtifact>,
    formation: Arc<ResolvedWorldFormationArtifact>,
    resolved_geologic: Arc<ResolvedGeologicInputArtifact>,
    relief_spec: Arc<ReliefSpecArtifact>,
    domain: Arc<ClimateWorkDomainArtifact>,
    resolved_climate: Arc<ResolvedClimateInputArtifact>,
    resolved_hydro_erosion: Arc<ResolvedHydroErosionInputArtifact>,
    surface: Arc<SphericalSurfaceArtifact>,
}

impl StageInputs for CausalNaturalFormationStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            NaturalQualityProfileArtifact::KEY,
            ResolvedTectonicInputArtifact::KEY,
            ResolvedWorldFormationArtifact::KEY,
            ResolvedGeologicInputArtifact::KEY,
            ReliefSpecArtifact::KEY,
            ClimateWorkDomainArtifact::KEY,
            ResolvedClimateInputArtifact::KEY,
            ResolvedHydroErosionInputArtifact::KEY,
            SphericalSurfaceArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            profile: artifacts.get::<NaturalQualityProfileArtifact>()?,
            resolved_tectonic: artifacts.get::<ResolvedTectonicInputArtifact>()?,
            formation: artifacts.get::<ResolvedWorldFormationArtifact>()?,
            resolved_geologic: artifacts.get::<ResolvedGeologicInputArtifact>()?,
            relief_spec: artifacts.get::<ReliefSpecArtifact>()?,
            domain: artifacts.get::<ClimateWorkDomainArtifact>()?,
            resolved_climate: artifacts.get::<ResolvedClimateInputArtifact>()?,
            resolved_hydro_erosion: artifacts.get::<ResolvedHydroErosionInputArtifact>()?,
            surface: artifacts.get::<SphericalSurfaceArtifact>()?,
        })
    }
}

/// Runs the complete causal formation build and publishes exactly one artifact.
#[derive(Debug, Clone, Copy, Default)]
pub struct CausalNaturalFormationStage;

impl Stage for CausalNaturalFormationStage {
    type Inputs = CausalNaturalFormationStageInputs;
    type Output = NaturalFormationBundleArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.causal-formation")
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
        let profile_bundle = ProfileSurfaceBuilder::complete(
            inputs.profile.profile(),
            inputs.surface.snapshot(),
            &cancellation,
        )
        .map_err(profile_failure)?;
        let artifact = NaturalFormationBundleArtifact::generate(
            CausalNaturalFormationInputs {
                profile_bundle: &profile_bundle,
                quality_profile: inputs.profile.profile(),
                tectonic_spec: inputs.resolved_tectonic.input().spec(),
                formation: inputs.formation.formation(),
                geologic_spec: inputs.resolved_geologic.input().spec(),
                relief_spec: inputs.relief_spec.spec(),
                climate_domain: inputs.domain.snapshot(),
                climate_spec: inputs.resolved_climate.input().spec(),
                surface_spec: inputs.resolved_hydro_erosion.input().spec(),
            },
            rng,
            &cancellation,
        )
        .map_err(product_failure)?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        Ok(artifact)
    }
}

/// Builds the sole production graph for final natural formation.
pub fn causal_natural_formation_graph() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<NaturalQualityProfileArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ReliefSpecArtifact>()
        .external::<ResolvedClimateInputArtifact>()
        .external::<ResolvedHydroErosionInputArtifact>()
        .external::<SphericalSurfaceArtifact>()
        .stage(ClimateWorkDomainStage)
        .stage(CausalNaturalFormationStage)
        .build()
}

#[derive(Debug, Error)]
enum NaturalFormationBundleGenerationError {
    #[error(transparent)]
    Causal(#[from] CausalFormationGenerationError),
    #[error(transparent)]
    Quality(#[from] QualityBuildError),
    #[error(transparent)]
    Bundle(#[from] NaturalFormationBundleValidationError),
    #[error(transparent)]
    Forcing(#[from] super::GlobalClimateForcingError),
    #[error("endpoint climate does not match final terrain forcing")]
    EndpointForcingMismatch,
    #[error("generated natural formation artifact is invalid: {0}")]
    Product(String),
}

fn profile_failure(error: ProfileSurfaceBuildError) -> StageError {
    match error {
        ProfileSurfaceBuildError::Cancelled => cancelled(),
        _ => StageError::new(INVALID_INPUT_CODE, error.to_string()),
    }
}

fn product_failure(error: NaturalFormationBundleGenerationError) -> StageError {
    match error {
        NaturalFormationBundleGenerationError::Causal(
            CausalFormationGenerationError::Cancelled,
        )
        | NaturalFormationBundleGenerationError::Quality(QualityBuildError::Cancelled)
        | NaturalFormationBundleGenerationError::Forcing(
            super::GlobalClimateForcingError::Cancelled,
        ) => cancelled(),
        NaturalFormationBundleGenerationError::EndpointForcingMismatch
        | NaturalFormationBundleGenerationError::Causal(
            CausalFormationGenerationError::EndpointForcingIdentityMismatch,
        ) => StageError::new(ENDPOINT_FORCING_MISMATCH_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Causal(
            CausalFormationGenerationError::SurfaceFormation(
                SurfaceFormationGenerationError::ElevationOutOfRange { .. }
                | SurfaceFormationGenerationError::ElevationDomainExhausted { .. },
            ),
        ) => StageError::new(ELEVATION_OUT_OF_RANGE_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Causal(
            CausalFormationGenerationError::SurfaceFormation(
                SurfaceFormationGenerationError::AllocationOverflow,
            ),
        ) => StageError::new(RESOURCE_LIMIT_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Causal(
            CausalFormationGenerationError::SurfaceFormation(
                SurfaceFormationGenerationError::WaterGeometry(_)
                | SurfaceFormationGenerationError::Hydrology(_),
            ),
        ) => StageError::new(WATER_BUDGET_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Causal(
            CausalFormationGenerationError::SurfaceFormation(
                SurfaceFormationGenerationError::Sediment(_)
                | SurfaceFormationGenerationError::Coast(_),
            ),
        ) => StageError::new(SEDIMENT_BUDGET_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Causal(CausalFormationGenerationError::Climate(
            _,
        )) => StageError::new(CLIMATE_NOT_CONVERGED_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Causal(
            CausalFormationGenerationError::EvolvedTectonics(_)
            | CausalFormationGenerationError::GeologicSubstrate(_),
        ) => StageError::new(SOLID_BUDGET_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Causal(
            CausalFormationGenerationError::SurfaceFormation(_)
            | CausalFormationGenerationError::FormationState(_),
        ) => StageError::new(NUMERICAL_STABILITY_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Bundle(
            NaturalFormationBundleValidationError::IdentityMismatch { .. },
        ) => StageError::new(ENDPOINT_FORCING_MISMATCH_CODE, error.to_string()),
        NaturalFormationBundleGenerationError::Quality(_)
        | NaturalFormationBundleGenerationError::Forcing(_)
        | NaturalFormationBundleGenerationError::Bundle(_)
        | NaturalFormationBundleGenerationError::Product(_)
        | NaturalFormationBundleGenerationError::Causal(_) => {
            StageError::new(INVALID_INPUT_CODE, error.to_string())
        }
    }
}

fn cancelled() -> StageError {
    StageError::new(CANCELLED_CODE, "causal natural formation was cancelled")
}
