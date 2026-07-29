//! Engine transport and stage adapters for pure hydro-erosion rule resolution.

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
    core_capability_registry, HydroErosionModel, HydroErosionRuleResolution,
    HydroErosionRuleResolutionError, HydroErosionRuleResolver,
};
use crate::world::natural::{HydroErosionSpec, HydroErosionSpecError};
use crate::world::WORLD_SPEC_SCHEMA_V1;

const INVALID_BASE_SPEC_CODE: &str = "rules.invalid-base-hydro-erosion-spec";
const INVALID_RESOLUTION_CODE: &str = "rules.invalid-hydro-erosion-resolution";
const CAPABILITY_CONTRACT_CODE: &str = "rules.capability-contract";
const INVALID_RESOLVED_INPUT_CODE: &str = "natural.invalid-resolved-hydro-erosion-input";

/// Engine transport for an externally supplied hydro-erosion specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HydroErosionSpecArtifact {
    spec: HydroErosionSpec,
}

impl HydroErosionSpecArtifact {
    /// Wraps a hydro-erosion specification for validated engine transport.
    pub const fn new(spec: HydroErosionSpec) -> Self {
        Self { spec }
    }

    /// Returns the wrapped hydro-erosion specification.
    pub const fn spec(&self) -> &HydroErosionSpec {
        &self.spec
    }
}

impl Artifact for HydroErosionSpecArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.hydro-erosion-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.spec.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_BASE_SPEC_CODE, error.to_string())
        })
    }
}

/// Engine transport for the complete read-only hydro-erosion rule audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HydroErosionRuleResolutionArtifact {
    resolution: HydroErosionRuleResolution,
}

impl HydroErosionRuleResolutionArtifact {
    /// Wraps one validated hydro-erosion rule-resolution audit.
    pub const fn new(resolution: HydroErosionRuleResolution) -> Self {
        Self { resolution }
    }

    /// Returns the complete pure resolution audit.
    pub const fn resolution(&self) -> &HydroErosionRuleResolution {
        &self.resolution
    }
}

impl Artifact for HydroErosionRuleResolutionArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("rules.hydro-erosion-resolution");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.resolution.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RESOLUTION_CODE, error.to_string())
        })
    }
}

/// The minimal audit-free input consumed by hydro-erosion generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedHydroErosionInput {
    model: HydroErosionModel,
    spec: HydroErosionSpec,
}

#[derive(Deserialize)]
struct ResolvedHydroErosionInputWire {
    model: HydroErosionModel,
    spec: HydroErosionSpec,
}

impl ResolvedHydroErosionInput {
    /// Creates a minimal input after validating the resolved specification.
    pub fn new(
        model: HydroErosionModel,
        spec: HydroErosionSpec,
    ) -> Result<Self, HydroErosionSpecError> {
        spec.validate()?;
        Ok(Self { model, spec })
    }

    /// Returns the trusted compiled model selection.
    pub const fn model(&self) -> HydroErosionModel {
        self.model
    }

    /// Returns the exact resolved generation specification.
    pub const fn spec(&self) -> &HydroErosionSpec {
        &self.spec
    }

    /// Revalidates the minimal generation contract.
    pub fn validate(&self) -> Result<(), HydroErosionSpecError> {
        self.spec.validate()
    }
}

impl<'de> Deserialize<'de> for ResolvedHydroErosionInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ResolvedHydroErosionInputWire::deserialize(deserializer)?;
        Self::new(wire.model, wire.spec).map_err(D::Error::custom)
    }
}

/// Engine transport for the audit-free hydro-erosion generation input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedHydroErosionInputArtifact {
    input: ResolvedHydroErosionInput,
}

impl ResolvedHydroErosionInputArtifact {
    /// Wraps one validated minimal generation input.
    pub const fn new(input: ResolvedHydroErosionInput) -> Self {
        Self { input }
    }

    /// Returns the minimal generation input.
    pub const fn input(&self) -> &ResolvedHydroErosionInput {
        &self.input
    }
}

impl Artifact for ResolvedHydroErosionInputArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("natural.resolved-hydro-erosion-input");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        self.input.validate().map_err(|error| {
            ArtifactValidationError::new(INVALID_RESOLVED_INPUT_CODE, error.to_string())
        })
    }
}

/// Typed external inputs visible to [`RuleHydroErosionResolutionStage`].
pub struct RuleHydroErosionResolutionStageInputs {
    base_spec: Arc<HydroErosionSpecArtifact>,
    pack_set: Arc<RulePackSetArtifact>,
}

impl StageInputs for RuleHydroErosionResolutionStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[HydroErosionSpecArtifact::KEY, RulePackSetArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            base_spec: artifacts.get::<HydroErosionSpecArtifact>()?,
            pack_set: artifacts.get::<RulePackSetArtifact>()?,
        })
    }
}

/// Resolves the hydro-erosion world-law capability into one full audit.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuleHydroErosionResolutionStage;

impl Stage for RuleHydroErosionResolutionStage {
    type Inputs = RuleHydroErosionResolutionStageInputs;
    type Output = HydroErosionRuleResolutionArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.resolve-hydro-erosion-rules")
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
        let resolution = HydroErosionRuleResolver::resolve(inputs.base_spec.spec(), &packs)
            .map_err(hydro_erosion_resolution_failure)?;
        Ok(HydroErosionRuleResolutionArtifact::new(resolution))
    }
}

/// Typed audit dependency visible to [`ResolvedHydroErosionInputStage`].
pub struct ResolvedHydroErosionInputStageInputs {
    resolution: Arc<HydroErosionRuleResolutionArtifact>,
}

impl StageInputs for ResolvedHydroErosionInputStageInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[HydroErosionRuleResolutionArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            resolution: artifacts.get::<HydroErosionRuleResolutionArtifact>()?,
        })
    }
}

/// Projects a full hydro-erosion rule audit into only model and specification.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResolvedHydroErosionInputStage;

impl Stage for ResolvedHydroErosionInputStage {
    type Inputs = ResolvedHydroErosionInputStageInputs;
    type Output = ResolvedHydroErosionInputArtifact;

    fn id(&self) -> StageId {
        StageId::new("natural.project-hydro-erosion-input")
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
        let input =
            ResolvedHydroErosionInput::new(resolution.model(), resolution.spec().clone())
                .map_err(|error| StageError::new(INVALID_RESOLVED_INPUT_CODE, error.to_string()))?;
        Ok(ResolvedHydroErosionInputArtifact::new(input))
    }
}

fn hydro_erosion_resolution_failure(error: HydroErosionRuleResolutionError) -> StageError {
    let code = match error {
        HydroErosionRuleResolutionError::InvalidBaseSpec(_) => INVALID_BASE_SPEC_CODE,
        HydroErosionRuleResolutionError::MissingHydroErosionModel
        | HydroErosionRuleResolutionError::MultipleHydroErosionModels => CAPABILITY_CONTRACT_CODE,
        HydroErosionRuleResolutionError::InvalidResolvedSpec(_)
        | HydroErosionRuleResolutionError::UnsupportedSchema { .. }
        | HydroErosionRuleResolutionError::DuplicateResolvedPack { .. } => INVALID_RESOLUTION_CODE,
    };
    StageError::new(code, error.to_string())
}
