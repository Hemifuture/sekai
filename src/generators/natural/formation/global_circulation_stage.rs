//! Typed engine publication for the P4 climate work domain.

use std::sync::Arc;

use serde::Serialize;

use super::reduced_climate::ReducedClimateGeometry;
use super::{
    ClimateWorkDomainBuildError, ClimateWorkDomainBuilder, FormationGeometry,
    NaturalQualityProfileArtifact,
};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    PreparedArtifact, Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::generators::spatial::SphericalSurfaceArtifact;
use crate::world::natural::ClimateWorkDomainSnapshot;

const INVALID_INPUT_CODE: &str = "global-circulation.invalid-input";
const DOMAIN_BUILD_FAILED_CODE: &str = "global-circulation.work-domain-build-failed";
const CANCELLED_CODE: &str = "engine.cancelled";

/// Atomic publication of the reconstructable cubed-sphere climate work domain.
///
/// The portable [`ClimateWorkDomainSnapshot`] remains strict serde data, but
/// this trusted product envelope is Serialize-only. A decoded snapshot must be
/// rebound to its authoritative surface through [`Self::rehydrate`] so forged
/// overlap support or tangent transforms cannot acquire an Artifact identity.
///
/// The artifact also carries the reduced climate's seed-independent geometry of the domain
/// (A10 design section 10 T2). It is a pure function of the snapshot, so it is neither
/// serialized nor compared, and it is prewarmed wherever the domain is.
///
/// ```compile_fail
/// use sekai::generators::natural::ClimateWorkDomainArtifact;
///
/// let _: ClimateWorkDomainArtifact = serde_json::from_str("{}").unwrap();
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateWorkDomainArtifact {
    snapshot: ClimateWorkDomainSnapshot,
    #[serde(skip)]
    reduced_geometry: Arc<ReducedClimateGeometry>,
}

impl PartialEq for ClimateWorkDomainArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.snapshot == other.snapshot
    }
}

impl ClimateWorkDomainArtifact {
    pub(crate) fn new(
        snapshot: ClimateWorkDomainSnapshot,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<Self, ClimateWorkDomainBuildError> {
        let reduced_geometry = Arc::new(ReducedClimateGeometry::new(&snapshot, &|| {
            cancellation.is_cancelled()
        })?);
        Ok(Self {
            snapshot,
            reduced_geometry,
        })
    }

    /// Builds a work-domain artifact for one authoritative surface and profile.
    pub fn build(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        profile: crate::world::natural::NaturalQualityProfile,
        cancellation: &crate::engine::BuildCancellation,
    ) -> Result<Self, ClimateWorkDomainBuildError> {
        Self::new(
            ClimateWorkDomainBuilder::build(surface, profile, cancellation)?,
            cancellation,
        )
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
        Self::new(snapshot, cancellation)
    }

    /// Borrows the reconstructable work-domain snapshot.
    pub const fn snapshot(&self) -> &ClimateWorkDomainSnapshot {
        &self.snapshot
    }

    /// Borrows the reduced climate's seed-independent geometry of this domain.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "read by the reduced climate stage from A10 T8")
    )]
    pub(in crate::generators::natural) fn reduced_climate_geometry(
        &self,
    ) -> &ReducedClimateGeometry {
        &self.reduced_geometry
    }
}

impl Artifact for ClimateWorkDomainArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("world.climate-work-domain");

    /// Accepts the wrapper as is: a [`ClimateWorkDomainSnapshot`] is valid by construction
    /// (its constructors and deserialization run the full check and the grid reconstruction),
    /// and the wrapper only exists through the domain builder or [`Self::rehydrate`].
    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Ok(())
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
///
/// A stage made [`Self::with_geometry`] publishes the geometry's prepared domain whenever its
/// inputs are that geometry's profile and authoritative surface, instead of rebuilding it.
#[derive(Debug, Clone, Default)]
pub struct ClimateWorkDomainStage {
    geometry: Option<Arc<FormationGeometry>>,
}

impl ClimateWorkDomainStage {
    /// Creates the stage around prewarmed seed-independent geometry.
    pub const fn with_geometry(geometry: Arc<FormationGeometry>) -> Self {
        Self {
            geometry: Some(geometry),
        }
    }
}

impl Stage for ClimateWorkDomainStage {
    type Inputs = ClimateWorkDomainStageInputs;
    type Output = ClimateWorkDomainArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.climate-work-domain")
    }

    fn version(&self) -> u32 {
        2
    }

    fn namespace(&self) -> &'static str {
        "sekai.core"
    }

    fn prepared_output(&self, inputs: &Self::Inputs) -> Option<&PreparedArtifact<Self::Output>> {
        self.geometry
            .as_deref()?
            .climate_domain_for(inputs.profile.profile(), inputs.surface.snapshot())
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
        let artifact =
            ClimateWorkDomainArtifact::new(snapshot, &cancellation).map_err(domain_failure)?;
        rng.check_cancelled().map_err(|_| cancelled())?;
        Ok(artifact)
    }
}

fn domain_failure(error: ClimateWorkDomainBuildError) -> StageError {
    match error {
        ClimateWorkDomainBuildError::Cancelled => cancelled(),
        ClimateWorkDomainBuildError::Validation(_) => invalid_input(error.to_string()),
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
