//! Engine transport and stage adapters for pure geologic rule resolution.

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
    core_capability_registry, GeologicModel, GeologicRuleResolution, GeologicRuleResolutionError,
    GeologicRuleResolver,
};
use crate::world::natural::{GeologicSpec, GeologicSpecError};
use crate::world::WORLD_SPEC_SCHEMA_V1;

const INVALID_BASE_SPEC_CODE: &str = "rules.invalid-base-geologic-spec";
const INVALID_RESOLUTION_CODE: &str = "rules.invalid-geologic-resolution";
const CAPABILITY_CONTRACT_CODE: &str = "rules.capability-contract";
const INVALID_RESOLVED_INPUT_CODE: &str = "natural.invalid-resolved-geologic-input";

/// Engine transport for an externally supplied geologic specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeologicSpecArtifact {
    spec: GeologicSpec,
}

impl GeologicSpecArtifact {
    /// Wraps a geologic specification for validated engine transport.
    pub const fn new(spec: GeologicSpec) -> Self {
        Self { spec }
    }

    /// Returns the wrapped geologic specification.
    pub const fn spec(&self) -> &GeologicSpec {
        &self.spec
    }
}

impl Artifact for GeologicSpecArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.geologic-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.spec.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_BASE_SPEC_CODE, error.to_string())
        })
    }
}

/// Engine transport for the complete read-only geologic rule audit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeologicRuleResolutionArtifact {
    resolution: GeologicRuleResolution,
}

impl GeologicRuleResolutionArtifact {
    /// Wraps one validated geologic rule-resolution audit.
    pub const fn new(resolution: GeologicRuleResolution) -> Self {
        Self { resolution }
    }

    /// Returns the complete pure resolution audit.
    pub const fn resolution(&self) -> &GeologicRuleResolution {
        &self.resolution
    }
}

impl Artifact for GeologicRuleResolutionArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("rules.geologic-resolution");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.resolution.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RESOLUTION_CODE, error.to_string())
        })
    }
}

/// The minimal audit-free input consumed by mantle and geologic generation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedGeologicInput {
    model: GeologicModel,
    spec: GeologicSpec,
}

#[derive(Deserialize)]
struct ResolvedGeologicInputWire {
    model: GeologicModel,
    spec: GeologicSpec,
}

impl ResolvedGeologicInput {
    /// Creates a minimal input after validating the resolved geologic specification.
    pub fn new(model: GeologicModel, spec: GeologicSpec) -> Result<Self, GeologicSpecError> {
        spec.validate()?;
        Ok(Self { model, spec })
    }

    /// Returns the trusted compiled model selection.
    pub const fn model(&self) -> GeologicModel {
        self.model
    }

    /// Returns the exact resolved generation specification.
    pub const fn spec(&self) -> &GeologicSpec {
        &self.spec
    }

    /// Revalidates the minimal generation contract.
    pub fn validate(&self) -> Result<(), GeologicSpecError> {
        self.spec.validate()
    }
}

impl<'de> Deserialize<'de> for ResolvedGeologicInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ResolvedGeologicInputWire::deserialize(deserializer)?;
        Self::new(wire.model, wire.spec).map_err(D::Error::custom)
    }
}

/// Engine transport for the audit-free geologic generation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedGeologicInputArtifact {
    input: ResolvedGeologicInput,
}

impl ResolvedGeologicInputArtifact {
    /// Wraps one validated minimal generation input.
    pub const fn new(input: ResolvedGeologicInput) -> Self {
        Self { input }
    }

    /// Returns the minimal generation input.
    pub const fn input(&self) -> &ResolvedGeologicInput {
        &self.input
    }
}

impl Artifact for ResolvedGeologicInputArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.resolved-geologic-input");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.input.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RESOLVED_INPUT_CODE, error.to_string())
        })
    }
}

/// Typed external inputs visible to [`RuleGeologicResolutionStage`].
pub struct RuleGeologicResolutionStageInputs {
    base_spec: Arc<GeologicSpecArtifact>,
    pack_set: Arc<RulePackSetArtifact>,
}

impl StageInputs for RuleGeologicResolutionStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[GeologicSpecArtifact::KEY, RulePackSetArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            base_spec: artifacts.get::<GeologicSpecArtifact>()?,
            pack_set: artifacts.get::<RulePackSetArtifact>()?,
        })
    }
}

/// Resolves the geologic world-law capability into one full audit.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleGeologicResolutionStage;

impl Stage for RuleGeologicResolutionStage {
    type Inputs = RuleGeologicResolutionStageInputs;
    type Output = GeologicRuleResolutionArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.resolve-geologic-rules")
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
        let resolution = GeologicRuleResolver::resolve(inputs.base_spec.spec(), &packs)
            .map_err(geologic_resolution_failure)?;
        Ok(GeologicRuleResolutionArtifact::new(resolution))
    }
}

/// Typed audit dependency visible to [`ResolvedGeologicInputStage`].
pub struct ResolvedGeologicInputStageInputs {
    resolution: Arc<GeologicRuleResolutionArtifact>,
}

impl StageInputs for ResolvedGeologicInputStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[GeologicRuleResolutionArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolution: artifacts.get::<GeologicRuleResolutionArtifact>()?,
        })
    }
}

/// Projects a full geologic rule audit into only the model and specification.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolvedGeologicInputStage;

impl Stage for ResolvedGeologicInputStage {
    type Inputs = ResolvedGeologicInputStageInputs;
    type Output = ResolvedGeologicInputArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.project-geologic-input")
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
        let input = ResolvedGeologicInput::new(resolution.model(), resolution.spec().clone())
            .map_err(|error| StageError::new(INVALID_RESOLVED_INPUT_CODE, error.to_string()))?;
        Ok(ResolvedGeologicInputArtifact::new(input))
    }
}

fn geologic_resolution_failure(error: GeologicRuleResolutionError) -> StageError {
    let code = match error {
        GeologicRuleResolutionError::InvalidBaseSpec(_) => INVALID_BASE_SPEC_CODE,
        GeologicRuleResolutionError::MissingGeologicModel
        | GeologicRuleResolutionError::MultipleGeologicModels => CAPABILITY_CONTRACT_CODE,
        GeologicRuleResolutionError::InvalidResolvedSpec(_)
        | GeologicRuleResolutionError::UnsupportedSchema { .. }
        | GeologicRuleResolutionError::DuplicateResolvedPack { .. } => INVALID_RESOLUTION_CODE,
    };
    StageError::new(code, error.to_string())
}
