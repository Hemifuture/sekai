use thiserror::Error;

use super::{
    geologic_model_capability_id, tectonic_controls_capability_id, tectonic_model_capability_id,
    CapabilityCardinality, CapabilityContribution, CapabilityDescriptor, CapabilityRegistry,
    CapabilityRegistryBuilder, CapabilityRegistryError, CoreSchemaRange, GeologicModel,
    RuleIdentityError, RulePack, RulePackError, RulePackId, RulePackKind, RulePackSet,
    RulePackSetError, RuleVersion, TectonicModel,
};
use crate::world::WORLD_SPEC_SCHEMA_V1;

/// The exact stable ID of the built-in current-slice earthlike world law.
pub const EARTHLIKE_RULE_PACK_ID: &str = "sekai.builtin.earthlike";

/// Construction errors for compiled-in, data-only rule definitions.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuiltinRuleError {
    /// A compiled stable ID or version contract is invalid.
    #[error(transparent)]
    Identity(#[from] RuleIdentityError),
    /// The compiled capability registry contains a conflict.
    #[error(transparent)]
    CapabilityRegistry(#[from] CapabilityRegistryError),
    /// A compiled rule pack violates the public pack contract.
    #[error(transparent)]
    RulePack(#[from] RulePackError),
    /// The compiled default pack set violates the public set contract.
    #[error(transparent)]
    RulePackSet(#[from] RulePackSetError),
}

/// Builds the exact V1 registry of compiled natural capabilities.
pub fn core_capability_registry() -> Result<CapabilityRegistry, BuiltinRuleError> {
    let mut builder = CapabilityRegistryBuilder::new();
    builder.register(CapabilityDescriptor::new(
        geologic_model_capability_id(),
        CapabilityCardinality::UniqueRequired,
        RulePackKind::WorldLaw,
        false,
    ))?;
    builder.register(CapabilityDescriptor::new(
        tectonic_model_capability_id(),
        CapabilityCardinality::UniqueRequired,
        RulePackKind::WorldLaw,
        false,
    ))?;
    builder.register(CapabilityDescriptor::new(
        tectonic_controls_capability_id(),
        CapabilityCardinality::Merge,
        RulePackKind::Ordinary,
        true,
    ))?;
    Ok(builder.build())
}

/// Builds the V1 earthlike world law that selects the existing current-slice model.
pub fn earthlike_rule_pack() -> Result<RulePack, BuiltinRuleError> {
    Ok(RulePack::new(
        RulePackId::new(EARTHLIKE_RULE_PACK_ID)?,
        RuleVersion::new(1, 0, 0)?,
        RulePackKind::WorldLaw,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1)?,
        Vec::new(),
        Vec::new(),
        vec![
            CapabilityContribution::TectonicModel(TectonicModel::CurrentSliceV1),
            CapabilityContribution::GeologicModel(GeologicModel::CurrentSliceV1),
        ],
    )?)
}

/// Builds the default V1 rule set containing only the earthlike world law.
pub fn default_rule_pack_set() -> Result<RulePackSet, BuiltinRuleError> {
    Ok(RulePackSet::new(vec![earthlike_rule_pack()?])?)
}
