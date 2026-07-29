//! Engine transport and stage adapters for pure preliminary-climate rule resolution.

use std::sync::Arc;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use super::rule_input::{invalid_builtin_definition, pack_resolution_failure};
use super::RulePackSetArtifact;
use crate::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    Stage, StageError, StageId, StageInputs, StageRng,
};
use crate::rules::{
    core_capability_registry, ClimateModel, ClimateRuleResolution, ClimateRuleResolutionError,
    ClimateRuleResolver,
};
use crate::world::natural::{ClimateSpec, ClimateSpecError};
use crate::world::WORLD_SPEC_SCHEMA_V1;

const INVALID_BASE_SPEC_CODE: &str = "rules.invalid-base-climate-spec";
const INVALID_RESOLUTION_CODE: &str = "rules.invalid-climate-resolution";
const CAPABILITY_CONTRACT_CODE: &str = "rules.capability-contract";
const INVALID_RESOLVED_INPUT_CODE: &str = "natural.invalid-resolved-climate-input";

/// Engine transport for an externally supplied preliminary-climate specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClimateSpecArtifact {
    spec: ClimateSpec,
}

impl ClimateSpecArtifact {
    /// Wraps a climate specification for validated engine transport.
    pub const fn new(spec: ClimateSpec) -> Self {
        Self { spec }
    }

    /// Returns the wrapped climate specification.
    pub const fn spec(&self) -> &ClimateSpec {
        &self.spec
    }
}

impl Artifact for ClimateSpecArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.climate-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.spec.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_BASE_SPEC_CODE, error.to_string())
        })
    }
}

/// Engine transport for the complete read-only preliminary-climate rule audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClimateRuleResolutionArtifact {
    resolution: ClimateRuleResolution,
}

impl ClimateRuleResolutionArtifact {
    /// Wraps one validated climate rule-resolution audit.
    pub const fn new(resolution: ClimateRuleResolution) -> Self {
        Self { resolution }
    }

    /// Returns the complete pure resolution audit.
    pub const fn resolution(&self) -> &ClimateRuleResolution {
        &self.resolution
    }
}

impl Artifact for ClimateRuleResolutionArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("rules.climate-resolution");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.resolution.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RESOLUTION_CODE, error.to_string())
        })
    }
}

/// The minimal audit-free input consumed by preliminary-climate generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedClimateInput {
    model: ClimateModel,
    spec: ClimateSpec,
}

#[derive(Deserialize)]
struct ResolvedClimateInputWire {
    model: ClimateModel,
    spec: ClimateSpec,
}

impl ResolvedClimateInput {
    /// Creates a minimal input after validating the resolved climate specification.
    pub fn new(model: ClimateModel, spec: ClimateSpec) -> Result<Self, ClimateSpecError> {
        spec.validate()?;
        Ok(Self { model, spec })
    }

    /// Returns the trusted compiled model selection.
    pub const fn model(&self) -> ClimateModel {
        self.model
    }

    /// Returns the exact resolved generation specification.
    pub const fn spec(&self) -> &ClimateSpec {
        &self.spec
    }

    /// Revalidates the minimal generation contract.
    pub fn validate(&self) -> Result<(), ClimateSpecError> {
        self.spec.validate()
    }
}

impl<'de> Deserialize<'de> for ResolvedClimateInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ResolvedClimateInputWire::deserialize(deserializer)?;
        Self::new(wire.model, wire.spec).map_err(D::Error::custom)
    }
}

/// Engine transport for the audit-free preliminary-climate generation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedClimateInputArtifact {
    input: ResolvedClimateInput,
}

impl ResolvedClimateInputArtifact {
    /// Wraps one validated minimal generation input.
    pub const fn new(input: ResolvedClimateInput) -> Self {
        Self { input }
    }

    /// Returns the minimal generation input.
    pub const fn input(&self) -> &ResolvedClimateInput {
        &self.input
    }
}

impl Artifact for ResolvedClimateInputArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.resolved-climate-input");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.input.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RESOLVED_INPUT_CODE, error.to_string())
        })
    }
}

/// Typed external inputs visible to [`RuleClimateResolutionStage`].
pub struct RuleClimateResolutionStageInputs {
    base_spec: Arc<ClimateSpecArtifact>,
    pack_set: Arc<RulePackSetArtifact>,
}

impl StageInputs for RuleClimateResolutionStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[ClimateSpecArtifact::KEY, RulePackSetArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            base_spec: artifacts.get::<ClimateSpecArtifact>()?,
            pack_set: artifacts.get::<RulePackSetArtifact>()?,
        })
    }
}

/// Resolves the preliminary-climate world-law capability into one full audit.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleClimateResolutionStage;

impl Stage for RuleClimateResolutionStage {
    type Inputs = RuleClimateResolutionStageInputs;
    type Output = ClimateRuleResolutionArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.resolve-climate-rules")
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
        let resolution = ClimateRuleResolver::resolve(inputs.base_spec.spec(), &packs)
            .map_err(climate_resolution_failure)?;
        Ok(ClimateRuleResolutionArtifact::new(resolution))
    }
}

/// Typed audit dependency visible to [`ResolvedClimateInputStage`].
pub struct ResolvedClimateInputStageInputs {
    resolution: Arc<ClimateRuleResolutionArtifact>,
}

impl StageInputs for ResolvedClimateInputStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[ClimateRuleResolutionArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolution: artifacts.get::<ClimateRuleResolutionArtifact>()?,
        })
    }
}

/// Projects a full climate rule audit into only the model and specification.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolvedClimateInputStage;

impl Stage for ResolvedClimateInputStage {
    type Inputs = ResolvedClimateInputStageInputs;
    type Output = ResolvedClimateInputArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.project-climate-input")
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
        let resolution = inputs.resolution.resolution();
        let input = ResolvedClimateInput::new(resolution.model(), resolution.spec().clone())
            .map_err(|error| StageError::new(INVALID_RESOLVED_INPUT_CODE, error.to_string()))?;
        Ok(ResolvedClimateInputArtifact::new(input))
    }
}

fn climate_resolution_failure(error: ClimateRuleResolutionError) -> StageError {
    let code = match error {
        ClimateRuleResolutionError::InvalidBaseSpec(_) => INVALID_BASE_SPEC_CODE,
        ClimateRuleResolutionError::MissingClimateModel
        | ClimateRuleResolutionError::MultipleClimateModels => CAPABILITY_CONTRACT_CODE,
        ClimateRuleResolutionError::InvalidResolvedSpec(_)
        | ClimateRuleResolutionError::UnsupportedSchema { .. }
        | ClimateRuleResolutionError::DuplicateResolvedPack { .. } => INVALID_RESOLUTION_CODE,
    };
    StageError::new(code, error.to_string())
}
