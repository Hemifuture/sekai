use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    CapabilityContribution, HydroErosionModel, ResolvedRulePackRef, ResolvedRulePackSet, RulePackId,
};
use crate::world::natural::{HydroErosionSpec, HydroErosionSpecError};

/// The supported serialized schema for a current-slice hydro-erosion rule audit.
pub const HYDRO_EROSION_RULE_RESOLUTION_SCHEMA_V1: u16 = 1;

/// Errors returned while resolving or revalidating a hydro-erosion rule audit.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HydroErosionRuleResolutionError {
    /// The base hydro-erosion specification is invalid.
    #[error("invalid base hydro-erosion specification: {0}")]
    InvalidBaseSpec(HydroErosionSpecError),
    /// A capability-resolved pack set contains no hydro-erosion model.
    #[error("resolved rule-pack set contains no hydro-erosion model")]
    MissingHydroErosionModel,
    /// A supposedly capability-resolved pack set contains several hydro-erosion models.
    #[error("resolved rule-pack set contains several hydro-erosion model contributions")]
    MultipleHydroErosionModels,
    /// The resolved specification failed the natural-domain contract.
    #[error("invalid resolved hydro-erosion specification: {0}")]
    InvalidResolvedSpec(HydroErosionSpecError),
    /// A serialized audit uses an unsupported schema.
    #[error(
        "unsupported hydro-erosion rule-resolution schema {found}; supported schema is {HYDRO_EROSION_RULE_RESOLUTION_SCHEMA_V1}"
    )]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
    },
    /// A serialized audit repeats a participating pack ID.
    #[error("resolved hydro-erosion rule-pack audit repeats pack {pack_id:?}")]
    DuplicateResolvedPack {
        /// The repeated pack ID.
        pack_id: RulePackId,
    },
}

/// A full deterministic audit of one current-slice hydro-erosion world-law resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HydroErosionRuleResolution {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: HydroErosionModel,
    spec: HydroErosionSpec,
}

#[derive(Deserialize)]
struct HydroErosionRuleResolutionWire {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: HydroErosionModel,
    spec: HydroErosionSpec,
}

impl HydroErosionRuleResolution {
    /// Returns the serialized audit schema.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns participating pack identities in resolved dependency order.
    pub fn resolved_packs(&self) -> &[ResolvedRulePackRef] {
        &self.resolved_packs
    }

    /// Returns the selected trusted hydro-erosion model.
    pub const fn model(&self) -> HydroErosionModel {
        self.model
    }

    /// Returns the final validated hydro-erosion specification.
    pub const fn spec(&self) -> &HydroErosionSpec {
        &self.spec
    }

    /// Revalidates all serialized audit invariants.
    pub fn validate(&self) -> Result<(), HydroErosionRuleResolutionError> {
        if self.schema_version != HYDRO_EROSION_RULE_RESOLUTION_SCHEMA_V1 {
            return Err(HydroErosionRuleResolutionError::UnsupportedSchema {
                found: self.schema_version,
            });
        }
        self.spec
            .validate()
            .map_err(HydroErosionRuleResolutionError::InvalidResolvedSpec)?;

        let mut pack_ids = BTreeSet::new();
        for pack in &self.resolved_packs {
            if !pack_ids.insert(pack.pack_id().clone()) {
                return Err(HydroErosionRuleResolutionError::DuplicateResolvedPack {
                    pack_id: pack.pack_id().clone(),
                });
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for HydroErosionRuleResolution {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = HydroErosionRuleResolutionWire::deserialize(deserializer)?;
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

/// Stateless resolver for the closed V1 hydro-erosion world-law capability.
#[derive(Debug, Clone, Copy, Default)]
pub struct HydroErosionRuleResolver;

impl HydroErosionRuleResolver {
    /// Resolves capability-validated packs against one base hydro-erosion specification.
    pub fn resolve(
        base: &HydroErosionSpec,
        packs: &ResolvedRulePackSet<'_>,
    ) -> Result<HydroErosionRuleResolution, HydroErosionRuleResolutionError> {
        base.validate()
            .map_err(HydroErosionRuleResolutionError::InvalidBaseSpec)?;

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
                    CapabilityContribution::HydroErosionModel(candidate) => {
                        if model.replace(*candidate).is_some() {
                            return Err(
                                HydroErosionRuleResolutionError::MultipleHydroErosionModels,
                            );
                        }
                    }
                    CapabilityContribution::TectonicModel(_)
                    | CapabilityContribution::GeologicModel(_)
                    | CapabilityContribution::ClimateModel(_)
                    | CapabilityContribution::TectonicConstraint(_) => {}
                }
            }
        }
        let model = model.ok_or(HydroErosionRuleResolutionError::MissingHydroErosionModel)?;
        let resolution = HydroErosionRuleResolution {
            schema_version: HYDRO_EROSION_RULE_RESOLUTION_SCHEMA_V1,
            resolved_packs,
            model,
            spec: base.clone(),
        };
        resolution.validate()?;
        Ok(resolution)
    }
}
