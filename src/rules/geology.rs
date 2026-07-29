use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    CapabilityContribution, GeologicModel, ResolvedRulePackRef, ResolvedRulePackSet, RulePackId,
};
use crate::world::natural::{GeologicSpec, GeologicSpecError};

/// The supported serialized schema for a geologic rule-resolution audit.
pub const GEOLOGIC_RULE_RESOLUTION_SCHEMA_V1: u16 = 1;

/// Errors returned while resolving or revalidating a geologic rule audit.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GeologicRuleResolutionError {
    /// The base geologic specification is invalid.
    #[error("invalid base geologic specification: {0}")]
    InvalidBaseSpec(GeologicSpecError),
    /// A capability-resolved pack set contains no geologic model contribution.
    #[error("resolved rule-pack set contains no geologic model")]
    MissingGeologicModel,
    /// A supposedly capability-resolved pack set contains several geologic models.
    #[error("resolved rule-pack set contains several geologic model contributions")]
    MultipleGeologicModels,
    /// The resolved specification failed the natural-domain contract.
    #[error("invalid resolved geologic specification: {0}")]
    InvalidResolvedSpec(GeologicSpecError),
    /// A serialized audit uses an unsupported schema.
    #[error(
        "unsupported geologic rule-resolution schema {found}; supported schema is {GEOLOGIC_RULE_RESOLUTION_SCHEMA_V1}"
    )]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
    },
    /// A serialized audit repeats a participating pack ID.
    #[error("resolved geologic rule-pack audit repeats pack {pack_id:?}")]
    DuplicateResolvedPack {
        /// The repeated pack ID.
        pack_id: RulePackId,
    },
}

/// A full deterministic audit of one geologic world-law resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeologicRuleResolution {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: GeologicModel,
    spec: GeologicSpec,
}

#[derive(Deserialize)]
struct GeologicRuleResolutionWire {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: GeologicModel,
    spec: GeologicSpec,
}

impl GeologicRuleResolution {
    /// Returns the serialized audit schema.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns participating pack identities in resolved dependency order.
    pub fn resolved_packs(&self) -> &[ResolvedRulePackRef] {
        &self.resolved_packs
    }

    /// Returns the selected trusted geologic model.
    pub const fn model(&self) -> GeologicModel {
        self.model
    }

    /// Returns the final validated geologic specification.
    pub const fn spec(&self) -> &GeologicSpec {
        &self.spec
    }

    /// Revalidates all serialized audit invariants.
    pub fn validate(&self) -> Result<(), GeologicRuleResolutionError> {
        if self.schema_version != GEOLOGIC_RULE_RESOLUTION_SCHEMA_V1 {
            return Err(GeologicRuleResolutionError::UnsupportedSchema {
                found: self.schema_version,
            });
        }
        self.spec
            .validate()
            .map_err(GeologicRuleResolutionError::InvalidResolvedSpec)?;

        let mut pack_ids = BTreeSet::new();
        for pack in &self.resolved_packs {
            if !pack_ids.insert(pack.pack_id().clone()) {
                return Err(GeologicRuleResolutionError::DuplicateResolvedPack {
                    pack_id: pack.pack_id().clone(),
                });
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for GeologicRuleResolution {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GeologicRuleResolutionWire::deserialize(deserializer)?;
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

/// Stateless resolver for the closed V1 geologic world-law capability.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeologicRuleResolver;

impl GeologicRuleResolver {
    /// Resolves capability-validated packs against one base geologic specification.
    pub fn resolve(
        base: &GeologicSpec,
        packs: &ResolvedRulePackSet<'_>,
    ) -> Result<GeologicRuleResolution, GeologicRuleResolutionError> {
        base.validate()
            .map_err(GeologicRuleResolutionError::InvalidBaseSpec)?;

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
                    CapabilityContribution::GeologicModel(candidate) => {
                        if model.replace(*candidate).is_some() {
                            return Err(GeologicRuleResolutionError::MultipleGeologicModels);
                        }
                    }
                    CapabilityContribution::TectonicModel(_)
                    | CapabilityContribution::ClimateModel(_)
                    | CapabilityContribution::HydroErosionModel(_)
                    | CapabilityContribution::TectonicConstraint(_) => {}
                }
            }
        }
        let model = model.ok_or(GeologicRuleResolutionError::MissingGeologicModel)?;
        let resolution = GeologicRuleResolution {
            schema_version: GEOLOGIC_RULE_RESOLUTION_SCHEMA_V1,
            resolved_packs,
            model,
            spec: base.clone(),
        };
        resolution.validate()?;
        Ok(resolution)
    }
}
