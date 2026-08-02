use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{WorldFormationGenerationError, WorldFormationGenerator};
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::world::natural::{ResolvedWorldFormation, WorldFormationSpec, WorldFormationSpecError};

const INVALID_SPEC_CODE: &str = "natural.invalid-world-formation-spec";
const INVALID_RESOLUTION_CODE: &str = "natural.invalid-resolved-world-formation";

/// Engine transport for an externally supplied world-formation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldFormationSpecArtifact {
    spec: WorldFormationSpec,
}

impl WorldFormationSpecArtifact {
    /// Wraps a requested formation specification.
    pub const fn new(spec: WorldFormationSpec) -> Self {
        Self { spec }
    }

    /// Returns the requested formation specification.
    pub const fn spec(&self) -> &WorldFormationSpec {
        &self.spec
    }
}

impl Artifact for WorldFormationSpecArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.world-formation-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.spec
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_SPEC_CODE, error.to_string()))
    }
}

/// Engine transport for one concrete, auditable formation selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedWorldFormationArtifact {
    formation: ResolvedWorldFormation,
}

impl ResolvedWorldFormationArtifact {
    /// Wraps a validated resolved formation selection.
    pub const fn new(formation: ResolvedWorldFormation) -> Self {
        Self { formation }
    }

    /// Returns the concrete formation selection and its request provenance.
    pub const fn formation(&self) -> &ResolvedWorldFormation {
        &self.formation
    }
}

impl Artifact for ResolvedWorldFormationArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.resolved-world-formation");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.formation.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RESOLUTION_CODE, error.to_string())
        })
    }
}

/// Restricted input view for [`WorldFormationStage`].
pub struct WorldFormationStageInputs {
    spec: Arc<WorldFormationSpecArtifact>,
}

impl StageInputs for WorldFormationStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[WorldFormationSpecArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            spec: artifacts.get::<WorldFormationSpecArtifact>()?,
        })
    }
}

/// Resolves one author-facing formation request into a concrete preset.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorldFormationStage;

impl Stage for WorldFormationStage {
    type Inputs = WorldFormationStageInputs;
    type Output = ResolvedWorldFormationArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.resolve-world-formation")
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
        let formation = WorldFormationGenerator::resolve(inputs.spec.spec(), rng)
            .map_err(resolution_failure)?;
        Ok(ResolvedWorldFormationArtifact::new(formation))
    }
}

fn resolution_failure(error: WorldFormationGenerationError) -> StageError {
    match error {
        WorldFormationGenerationError::InvalidSpec(error) => invalid_spec(error),
        WorldFormationGenerationError::InvalidFormation(error) => StageError::new(
            INVALID_RESOLUTION_CODE,
            format!("resolved world formation failed validation: {error}"),
        ),
    }
}

fn invalid_spec(error: WorldFormationSpecError) -> StageError {
    StageError::new(
        INVALID_SPEC_CODE,
        format!("invalid world-formation specification: {error}"),
    )
}
