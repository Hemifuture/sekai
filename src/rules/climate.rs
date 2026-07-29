use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    CapabilityContribution, ClimateModel, ResolvedRulePackRef, ResolvedRulePackSet, RulePackId,
};
use crate::world::natural::{ClimateSpec, ClimateSpecError};

/// The supported serialized schema for a preliminary-climate rule-resolution audit.
pub const CLIMATE_RULE_RESOLUTION_SCHEMA_V1: u16 = 1;

/// Errors returned while resolving or revalidating a climate rule audit.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClimateRuleResolutionError {
    /// The base climate specification is invalid.
    #[error("invalid base climate specification: {0}")]
    InvalidBaseSpec(ClimateSpecError),
    /// A capability-resolved pack set contains no climate model contribution.
    #[error("resolved rule-pack set contains no climate model")]
    MissingClimateModel,
    /// A supposedly capability-resolved pack set contains several climate models.
    #[error("resolved rule-pack set contains several climate model contributions")]
    MultipleClimateModels,
    /// The resolved specification failed the natural-domain contract.
    #[error("invalid resolved climate specification: {0}")]
    InvalidResolvedSpec(ClimateSpecError),
    /// A serialized audit uses an unsupported schema.
    #[error(
        "unsupported climate rule-resolution schema {found}; supported schema is {CLIMATE_RULE_RESOLUTION_SCHEMA_V1}"
    )]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
    },
    /// A serialized audit repeats a participating pack ID.
    #[error("resolved climate rule-pack audit repeats pack {pack_id:?}")]
    DuplicateResolvedPack {
        /// The repeated pack ID.
        pack_id: RulePackId,
    },
}

/// A full deterministic audit of one preliminary-climate world-law resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClimateRuleResolution {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: ClimateModel,
    spec: ClimateSpec,
}

#[derive(Deserialize)]
struct ClimateRuleResolutionWire {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: ClimateModel,
    spec: ClimateSpec,
}

impl ClimateRuleResolution {
    /// Returns the serialized audit schema.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns participating pack identities in resolved dependency order.
    pub fn resolved_packs(&self) -> &[ResolvedRulePackRef] {
        &self.resolved_packs
    }

    /// Returns the selected trusted preliminary-climate model.
    pub const fn model(&self) -> ClimateModel {
        self.model
    }

    /// Returns the final validated climate specification.
    pub const fn spec(&self) -> &ClimateSpec {
        &self.spec
    }

    /// Revalidates all serialized audit invariants.
    pub fn validate(&self) -> Result<(), ClimateRuleResolutionError> {
        if self.schema_version != CLIMATE_RULE_RESOLUTION_SCHEMA_V1 {
            return Err(ClimateRuleResolutionError::UnsupportedSchema {
                found: self.schema_version,
            });
        }
        self.spec
            .validate()
            .map_err(ClimateRuleResolutionError::InvalidResolvedSpec)?;

        let mut pack_ids = BTreeSet::new();
        for pack in &self.resolved_packs {
            if !pack_ids.insert(pack.pack_id().clone()) {
                return Err(ClimateRuleResolutionError::DuplicateResolvedPack {
                    pack_id: pack.pack_id().clone(),
                });
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ClimateRuleResolution {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateRuleResolutionWire::deserialize(deserializer)?;
        let resolution = Self {
            schema_version: wire.schema_version,
            resolved_packs: wire.resolved_packs,
            model: wire.model,
            spec: wire.spec,
        };
        resolution.validate().map_err(D::Error::custom)?;
        Ok(resolution)
    }
}

/// Stateless resolver for the closed V1 preliminary-climate world-law capability.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClimateRuleResolver;

impl ClimateRuleResolver {
    /// Resolves capability-validated packs against one base climate specification.
    pub fn resolve(
        base: &ClimateSpec,
        packs: &ResolvedRulePackSet<'_>,
    ) -> Result<ClimateRuleResolution, ClimateRuleResolutionError> {
        base.validate()
            .map_err(ClimateRuleResolutionError::InvalidBaseSpec)?;

        let mut model = None;
        let mut resolved_packs = Vec::with_capacity(packs.len());
        for pack in packs.packs() {
            let manifest = pack.manifest();
            resolved_packs.push(ResolvedRulePackRef::new(
                manifest.id().clone(),
                manifest.version(),
                manifest.content_hash(),
            ));
            for contribution in pack.contributions() {
                match contribution {
                    CapabilityContribution::ClimateModel(candidate) => {
                        if model.replace(*candidate).is_some() {
                            return Err(ClimateRuleResolutionError::MultipleClimateModels);
                        }
                    }
                    CapabilityContribution::TectonicModel(_)
                    | CapabilityContribution::GeologicModel(_)
                    | CapabilityContribution::HydroErosionModel(_)
                    | CapabilityContribution::TectonicConstraint(_) => {}
                }
            }
        }
        let model = model.ok_or(ClimateRuleResolutionError::MissingClimateModel)?;
        let resolution = ClimateRuleResolution {
            schema_version: CLIMATE_RULE_RESOLUTION_SCHEMA_V1,
            resolved_packs,
            model,
            spec: base.clone(),
        };
        resolution.validate()?;
        Ok(resolution)
    }
}
