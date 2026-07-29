//! Deterministic, data-only rule-pack and author-input contracts.

mod builtin;
mod capability;
mod climate;
mod constraints;
mod geology;
mod ids;
mod manifest;
mod registry;
mod resolution;
mod tectonics;

pub use builtin::{
    core_capability_registry, default_rule_pack_set, earthlike_rule_pack, BuiltinRuleError,
    EARTHLIKE_RULE_PACK_ID,
};
pub use capability::{
    climate_model_capability_id, geologic_model_capability_id, tectonic_controls_capability_id,
    tectonic_model_capability_id, CapabilityCardinality, CapabilityContribution,
    CapabilityDescriptor, CapabilityRegistry, CapabilityRegistryBuilder, CapabilityRegistryError,
    ClimateModel, GeologicModel, RulePackKind, TectonicModel,
};
pub use climate::{
    ClimateRuleResolution, ClimateRuleResolutionError, ClimateRuleResolver,
    CLIMATE_RULE_RESOLUTION_SCHEMA_V1,
};
pub use constraints::{
    ActivitySet, AuthorConstraint, AuthorConstraints, ConstraintError, ConstraintSource,
    ConstraintStrength, ConstraintWeight, InclusiveU16Range, RuleTectonicConstraint,
    TectonicConstraintClause, TectonicControl, AUTHOR_CONSTRAINTS_SCHEMA_V1,
    MAX_AUTHOR_CONSTRAINTS, MAX_CONSTRAINT_WEIGHT, MAX_CONTINENTAL_CRUST_PERMILLE,
    MIN_CONSTRAINT_WEIGHT, MIN_CONTINENTAL_CRUST_PERMILLE,
};
pub use geology::{
    GeologicRuleResolution, GeologicRuleResolutionError, GeologicRuleResolver,
    GEOLOGIC_RULE_RESOLUTION_SCHEMA_V1,
};
pub use ids::{
    CapabilityId, CoreSchemaRange, RuleContentHash, RuleIdentityError, RuleItemId, RulePackId,
    RuleVersion, RuleVersionRequirement,
};
pub use manifest::{
    RulePack, RulePackDependency, RulePackError, RulePackManifest,
    MAX_RULE_PACK_CAPABILITY_REQUIREMENTS, MAX_RULE_PACK_CONTRIBUTIONS, MAX_RULE_PACK_DEPENDENCIES,
    RULE_PACK_SCHEMA_V1,
};
pub use registry::{
    ResolvedRulePackSet, RulePackSet, RulePackSetError, MAX_RULE_PACKS, MAX_RULE_SET_CONTRIBUTIONS,
};
pub use resolution::ResolvedRulePackRef;
pub use tectonics::{
    ConstraintAdoption, ConstraintAdoptionOutcome, TectonicRuleResolution,
    TectonicRuleResolutionError, TectonicRuleResolver, TECTONIC_RULE_RESOLUTION_SCHEMA_V1,
};
