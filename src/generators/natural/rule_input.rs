//! Engine transport and stage adapters for pure rule-resolution contracts.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::stage::TectonicSpecArtifact;
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::rules::{
    core_capability_registry, AuthorConstraints, BuiltinRuleError, RulePackSet, RulePackSetError,
    TectonicRuleResolution, TectonicRuleResolutionError, TectonicRuleResolver,
};
use crate::world::WORLD_SPEC_SCHEMA_V1;

const INVALID_PACK_SET_CODE: &str = "rules.invalid-pack-set";
const INVALID_AUTHOR_CONSTRAINTS_CODE: &str = "rules.invalid-author-constraints";
const INVALID_RESOLUTION_CODE: &str = "rules.invalid-tectonic-resolution";
const BUILTIN_DEFINITION_CODE: &str = "rules.invalid-builtin-definition";
const PACK_DEPENDENCY_CODE: &str = "rules.pack-dependency-resolution";
const CAPABILITY_CONTRACT_CODE: &str = "rules.capability-contract";
const HARD_CONSTRAINT_CONFLICT_CODE: &str = "rules.hard-constraint-conflict";
const INVALID_BASE_SPEC_CODE: &str = "rules.invalid-base-tectonic-spec";
const RESOLUTION_SCORE_CODE: &str = "rules.tectonic-score";

/// Engine transport for an externally supplied, validated rule-pack set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulePackSetArtifact {
    pack_set: RulePackSet,
}

impl RulePackSetArtifact {
    /// Wraps an already-validated canonical rule-pack set.
    pub const fn new(pack_set: RulePackSet) -> Self {
        Self { pack_set }
    }

    /// Returns the pure rule-pack contract.
    pub const fn pack_set(&self) -> &RulePackSet {
        &self.pack_set
    }
}

impl Artifact for RulePackSetArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("rules.pack-set");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.pack_set
            .validate()
            .map_err(|error| ArtifactValidationError::new(INVALID_PACK_SET_CODE, error.to_string()))
    }
}

/// Engine transport for externally supplied, typed author constraints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorConstraintsArtifact {
    constraints: AuthorConstraints,
}

impl AuthorConstraintsArtifact {
    /// Wraps an already-validated author-constraint collection.
    pub const fn new(constraints: AuthorConstraints) -> Self {
        Self { constraints }
    }

    /// Returns the pure author-constraint contract.
    pub const fn constraints(&self) -> &AuthorConstraints {
        &self.constraints
    }
}

impl Artifact for AuthorConstraintsArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("rules.author-constraints");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.constraints.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_AUTHOR_CONSTRAINTS_CODE, error.to_string())
        })
    }
}

/// Engine transport for the complete, read-only tectonic rule audit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TectonicRuleResolutionArtifact {
    resolution: TectonicRuleResolution,
}

impl TectonicRuleResolutionArtifact {
    /// Wraps one validated rule-resolution audit.
    pub const fn new(resolution: TectonicRuleResolution) -> Self {
        Self { resolution }
    }

    /// Returns the complete pure resolution audit.
    pub const fn resolution(&self) -> &TectonicRuleResolution {
        &self.resolution
    }
}

impl Artifact for TectonicRuleResolutionArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.tectonic-rule-resolution");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.resolution.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RESOLUTION_CODE, error.to_string())
        })
    }
}

/// Typed external inputs visible to [`RuleTectonicResolutionStage`].
pub struct RuleTectonicResolutionStageInputs {
    base_spec: Arc<TectonicSpecArtifact>,
    pack_set: Arc<RulePackSetArtifact>,
    author_constraints: Arc<AuthorConstraintsArtifact>,
}

impl StageInputs for RuleTectonicResolutionStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[
            TectonicSpecArtifact::KEY,
            RulePackSetArtifact::KEY,
            AuthorConstraintsArtifact::KEY,
        ]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            base_spec: artifacts.get::<TectonicSpecArtifact>()?,
            pack_set: artifacts.get::<RulePackSetArtifact>()?,
            author_constraints: artifacts.get::<AuthorConstraintsArtifact>()?,
        })
    }
}

/// Resolves rule capabilities and author constraints into one full audit.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleTectonicResolutionStage;

impl Stage for RuleTectonicResolutionStage {
    type Inputs = RuleTectonicResolutionStageInputs;
    type Output = TectonicRuleResolutionArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.resolve-tectonic-rules")
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
        let registry = core_capability_registry().map_err(invalid_builtin_definition)?;
        let packs = inputs
            .pack_set
            .pack_set()
            .resolve(&registry, WORLD_SPEC_SCHEMA_V1)
            .map_err(pack_resolution_failure)?;
        let resolution = TectonicRuleResolver::resolve(
            inputs.base_spec.spec(),
            &packs,
            inputs.author_constraints.constraints(),
        )
        .map_err(tectonic_resolution_failure)?;
        Ok(TectonicRuleResolutionArtifact::new(resolution))
    }
}

fn invalid_builtin_definition(error: BuiltinRuleError) -> StageError {
    StageError::new(BUILTIN_DEFINITION_CODE, error.to_string())
}

fn pack_resolution_failure(error: RulePackSetError) -> StageError {
    let code = match error {
        RulePackSetError::IncompatibleCoreSchema { .. }
        | RulePackSetError::MissingDependency { .. }
        | RulePackSetError::IncompatibleDependencyVersion { .. }
        | RulePackSetError::SelfDependency { .. }
        | RulePackSetError::DependencyCycle { .. } => PACK_DEPENDENCY_CODE,
        RulePackSetError::UnknownProvidedCapability { .. }
        | RulePackSetError::UnknownConsumedCapability { .. }
        | RulePackSetError::InsufficientCapabilityPermission { .. }
        | RulePackSetError::MissingConsumedCapability { .. }
        | RulePackSetError::MissingRequiredCapability { .. }
        | RulePackSetError::MultipleCapabilityProviders { .. } => CAPABILITY_CONTRACT_CODE,
        RulePackSetError::TooManyPacks { .. }
        | RulePackSetError::TooManyContributions { .. }
        | RulePackSetError::InvalidPack { .. }
        | RulePackSetError::DuplicatePack { .. }
        | RulePackSetError::NonCanonicalPackOrder => INVALID_PACK_SET_CODE,
    };
    StageError::new(code, error.to_string())
}

fn tectonic_resolution_failure(error: TectonicRuleResolutionError) -> StageError {
    let code = match error {
        TectonicRuleResolutionError::HardConstraintConflict { .. } => HARD_CONSTRAINT_CONFLICT_CODE,
        TectonicRuleResolutionError::InvalidBaseSpec(_) => INVALID_BASE_SPEC_CODE,
        TectonicRuleResolutionError::InvalidAuthorConstraints(_) => INVALID_AUTHOR_CONSTRAINTS_CODE,
        TectonicRuleResolutionError::MissingTectonicModel
        | TectonicRuleResolutionError::MultipleTectonicModels => CAPABILITY_CONTRACT_CODE,
        TectonicRuleResolutionError::ScoreOverflow { .. } => RESOLUTION_SCORE_CODE,
        TectonicRuleResolutionError::InvalidResolvedSpec(_)
        | TectonicRuleResolutionError::UnsupportedSchema { .. }
        | TectonicRuleResolutionError::DuplicateResolvedPack { .. }
        | TectonicRuleResolutionError::NonCanonicalAdoptionOrder
        | TectonicRuleResolutionError::HardConstraintCompromised { .. }
        | TectonicRuleResolutionError::UnknownAdoptionRulePack { .. } => INVALID_RESOLUTION_CODE,
    };
    StageError::new(code, error.to_string())
}
