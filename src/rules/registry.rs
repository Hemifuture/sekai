use std::collections::{BTreeMap, BTreeSet};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    CapabilityCardinality, CapabilityId, CapabilityRegistry, CoreSchemaRange, RulePack,
    RulePackError, RulePackId, RulePackKind, RuleVersion, RuleVersionRequirement,
};

/// The maximum number of rule packs in one V1 resolved set.
pub const MAX_RULE_PACKS: usize = 64;
/// The maximum total typed contributions across one V1 rule-pack set.
pub const MAX_RULE_SET_CONTRIBUTIONS: usize = 4096;

/// Errors returned while constructing or dependency-ordering a rule-pack set.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RulePackSetError {
    /// A rule-pack set exceeds its pack-count budget.
    #[error("rule-pack count {found} exceeds V1 limit {MAX_RULE_PACKS}")]
    TooManyPacks {
        /// The rejected pack count.
        found: usize,
    },
    /// A rule-pack set exceeds its aggregate contribution budget.
    #[error("rule-pack contribution count {found} exceeds V1 limit {MAX_RULE_SET_CONTRIBUTIONS}")]
    TooManyContributions {
        /// The rejected contribution count.
        found: usize,
    },
    /// A contained rule pack failed its own validation.
    #[error("rule pack {pack_id:?} is invalid: {source}")]
    InvalidPack {
        /// The stable ID of the invalid pack.
        pack_id: RulePackId,
        /// The pack-local validation error.
        source: RulePackError,
    },
    /// A pack ID occurs more than once in the set.
    #[error("duplicate rule pack {pack_id:?}")]
    DuplicatePack {
        /// The repeated pack ID.
        pack_id: RulePackId,
    },
    /// A private stored pack vector is not in strict pack-ID order.
    #[error("rule packs are not stored in strict canonical ID order")]
    NonCanonicalPackOrder,
    /// A pack is not compatible with the active core world schema.
    #[error("rule pack {pack_id:?} supports core schema {supported:?}, not active schema {found}")]
    IncompatibleCoreSchema {
        /// The incompatible pack.
        pack_id: RulePackId,
        /// The pack's declared supported schema range.
        supported: CoreSchemaRange,
        /// The active core schema version.
        found: u16,
    },
    /// A declared dependency is absent from the set.
    #[error("rule pack {pack_id:?} requires missing dependency {dependency_id:?}")]
    MissingDependency {
        /// The consuming pack.
        pack_id: RulePackId,
        /// The absent dependency.
        dependency_id: RulePackId,
    },
    /// A present dependency does not satisfy the declared version requirement.
    #[error("rule pack {pack_id:?} requires {dependency_id:?} at {required:?}, found {found:?}")]
    IncompatibleDependencyVersion {
        /// The consuming pack.
        pack_id: RulePackId,
        /// The incompatible dependency.
        dependency_id: RulePackId,
        /// The declared compatibility requirement.
        required: RuleVersionRequirement,
        /// The dependency's actual version.
        found: RuleVersion,
    },
    /// A pack directly depends on itself.
    #[error("rule pack {pack_id:?} depends on itself")]
    SelfDependency {
        /// The self-dependent pack.
        pack_id: RulePackId,
    },
    /// The dependency graph contains a cycle.
    #[error("rule-pack dependency cycle contains {pack_id:?}")]
    DependencyCycle {
        /// The lexicographically smallest pack ID that is actually in a cycle.
        pack_id: RulePackId,
    },
    /// A pack provides a capability absent from the compiled registry.
    #[error("rule pack {pack_id:?} provides unknown capability {capability_id:?}")]
    UnknownProvidedCapability {
        /// The pack declaring the provider.
        pack_id: RulePackId,
        /// The unregistered capability.
        capability_id: CapabilityId,
    },
    /// A pack consumes a capability absent from the compiled registry.
    #[error("rule pack {pack_id:?} consumes unknown capability {capability_id:?}")]
    UnknownConsumedCapability {
        /// The consuming pack.
        pack_id: RulePackId,
        /// The unregistered capability.
        capability_id: CapabilityId,
    },
    /// A pack's permission class is too weak for a provided capability.
    #[error(
        "rule pack {pack_id:?} with permission {found:?} cannot provide {capability_id:?}, which requires {required:?}"
    )]
    InsufficientCapabilityPermission {
        /// The rejected provider.
        pack_id: RulePackId,
        /// The protected capability.
        capability_id: CapabilityId,
        /// The provider's permission class.
        found: RulePackKind,
        /// The minimum required permission.
        required: RulePackKind,
    },
    /// A registered capability consumed by a pack has no provider.
    #[error("rule pack {pack_id:?} consumes unprovided capability {capability_id:?}")]
    MissingConsumedCapability {
        /// The consuming pack.
        pack_id: RulePackId,
        /// The capability with no provider.
        capability_id: CapabilityId,
    },
    /// A required unique capability has no provider.
    #[error("required capability {capability_id:?} has no provider")]
    MissingRequiredCapability {
        /// The missing capability.
        capability_id: CapabilityId,
    },
    /// A unique capability has more than one provider.
    #[error("unique capability {capability_id:?} has multiple providers {provider_ids:?}")]
    MultipleCapabilityProviders {
        /// The conflicted unique capability.
        capability_id: CapabilityId,
        /// All provider IDs in stable pack-ID order.
        provider_ids: Vec<RulePackId>,
    },
}

/// A validated, canonical collection of data-only rule packs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RulePackSet {
    packs: Vec<RulePack>,
}

#[derive(Deserialize)]
struct RulePackSetWire {
    packs: Vec<RulePack>,
}

impl RulePackSet {
    /// Creates a validated set in stable pack-ID order.
    pub fn new(mut packs: Vec<RulePack>) -> Result<Self, RulePackSetError> {
        if packs.len() > MAX_RULE_PACKS {
            return Err(RulePackSetError::TooManyPacks { found: packs.len() });
        }

        let mut contribution_count = 0usize;
        for pack in &packs {
            pack.validate()
                .map_err(|source| RulePackSetError::InvalidPack {
                    pack_id: pack.manifest().id().clone(),
                    source,
                })?;
            contribution_count = contribution_count.saturating_add(pack.contributions().len());
            if contribution_count > MAX_RULE_SET_CONTRIBUTIONS {
                return Err(RulePackSetError::TooManyContributions {
                    found: contribution_count,
                });
            }
        }

        packs.sort_by(|left, right| left.manifest().id().cmp(right.manifest().id()));
        if let Some(pack_id) = packs.windows(2).find_map(|pair| {
            (pair[0].manifest().id() == pair[1].manifest().id())
                .then(|| pair[0].manifest().id().clone())
        }) {
            return Err(RulePackSetError::DuplicatePack { pack_id });
        }

        Ok(Self { packs })
    }

    /// Returns packs in stable pack-ID order.
    pub fn packs(&self) -> &[RulePack] {
        &self.packs
    }

    /// Returns the number of packs.
    pub fn len(&self) -> usize {
        self.packs.len()
    }

    /// Returns whether the set contains no packs.
    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }

    /// Returns the validated aggregate contribution count.
    pub fn contribution_count(&self) -> usize {
        self.packs
            .iter()
            .map(|pack| pack.contributions().len())
            .sum()
    }

    /// Revalidates pack-local contracts, collection budgets, and canonical order.
    pub fn validate(&self) -> Result<(), RulePackSetError> {
        if self.packs.len() > MAX_RULE_PACKS {
            return Err(RulePackSetError::TooManyPacks {
                found: self.packs.len(),
            });
        }
        let mut contribution_count = 0usize;
        for pack in &self.packs {
            pack.validate()
                .map_err(|source| RulePackSetError::InvalidPack {
                    pack_id: pack.manifest().id().clone(),
                    source,
                })?;
            contribution_count = contribution_count.saturating_add(pack.contributions().len());
            if contribution_count > MAX_RULE_SET_CONTRIBUTIONS {
                return Err(RulePackSetError::TooManyContributions {
                    found: contribution_count,
                });
            }
        }
        for pair in self.packs.windows(2) {
            match pair[0].manifest().id().cmp(pair[1].manifest().id()) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => {
                    return Err(RulePackSetError::DuplicatePack {
                        pack_id: pair[0].manifest().id().clone(),
                    });
                }
                std::cmp::Ordering::Greater => {
                    return Err(RulePackSetError::NonCanonicalPackOrder);
                }
            }
        }
        Ok(())
    }

    /// Validates dependencies and returns a deterministic topological order.
    pub fn resolve_dependencies(
        &self,
        active_core_schema: u16,
    ) -> Result<ResolvedRulePackSet<'_>, RulePackSetError> {
        let pack_indices: BTreeMap<_, _> = self
            .packs
            .iter()
            .enumerate()
            .map(|(index, pack)| (pack.manifest().id().clone(), index))
            .collect();
        let pack_count = self.packs.len();
        let mut incoming_counts = vec![0usize; pack_count];
        let mut consumers = vec![Vec::new(); pack_count];
        let mut dependency_reachability = vec![vec![false; pack_count]; pack_count];

        for (pack_index, pack) in self.packs.iter().enumerate() {
            let manifest = pack.manifest();
            if !manifest.core_schema().contains(active_core_schema) {
                return Err(RulePackSetError::IncompatibleCoreSchema {
                    pack_id: manifest.id().clone(),
                    supported: manifest.core_schema(),
                    found: active_core_schema,
                });
            }

            for dependency in manifest.dependencies() {
                if dependency.pack_id() == manifest.id() {
                    return Err(RulePackSetError::SelfDependency {
                        pack_id: manifest.id().clone(),
                    });
                }
                let Some(&dependency_index) = pack_indices.get(dependency.pack_id()) else {
                    return Err(RulePackSetError::MissingDependency {
                        pack_id: manifest.id().clone(),
                        dependency_id: dependency.pack_id().clone(),
                    });
                };
                let dependency_version = self.packs[dependency_index].manifest().version();
                if !dependency.version_requirement().matches(dependency_version) {
                    return Err(RulePackSetError::IncompatibleDependencyVersion {
                        pack_id: manifest.id().clone(),
                        dependency_id: dependency.pack_id().clone(),
                        required: dependency.version_requirement(),
                        found: dependency_version,
                    });
                }

                incoming_counts[pack_index] += 1;
                consumers[dependency_index].push(pack_index);
                dependency_reachability[pack_index][dependency_index] = true;
            }
        }

        for pack_consumers in &mut consumers {
            pack_consumers.sort_unstable();
        }

        let mut ready: BTreeSet<_> = incoming_counts
            .iter()
            .enumerate()
            .filter_map(|(index, &count)| (count == 0).then_some(index))
            .collect();
        let mut ordered_indices = Vec::with_capacity(pack_count);
        while let Some(index) = ready.iter().next().copied() {
            ready.remove(&index);
            ordered_indices.push(index);
            for &consumer in &consumers[index] {
                incoming_counts[consumer] -= 1;
                if incoming_counts[consumer] == 0 {
                    ready.insert(consumer);
                }
            }
        }

        if ordered_indices.len() != pack_count {
            // The remaining Kahn nodes also include acyclic consumers blocked by a
            // cycle. A bounded transitive closure distinguishes actual cycle
            // members and keeps the diagnostic stable without recursive DFS.
            for intermediate in 0..pack_count {
                let intermediate_reachability = dependency_reachability[intermediate].clone();
                for source_reachability in &mut dependency_reachability {
                    if source_reachability[intermediate] {
                        for (target, &intermediate_reaches_target) in source_reachability
                            .iter_mut()
                            .zip(&intermediate_reachability)
                        {
                            *target |= intermediate_reaches_target;
                        }
                    }
                }
            }
            let cycle_member = (0..pack_count)
                .find(|&index| dependency_reachability[index][index])
                .expect("an incomplete Kahn order must contain a dependency cycle");
            return Err(RulePackSetError::DependencyCycle {
                pack_id: self.packs[cycle_member].manifest().id().clone(),
            });
        }

        let ordered_packs = ordered_indices
            .into_iter()
            .map(|index| &self.packs[index])
            .collect();
        Ok(ResolvedRulePackSet {
            packs: ordered_packs,
            providers: index_providers(&self.packs),
        })
    }

    /// Resolves dependencies and enforces the compiled capability contracts.
    pub fn resolve(
        &self,
        registry: &CapabilityRegistry,
        active_core_schema: u16,
    ) -> Result<ResolvedRulePackSet<'_>, RulePackSetError> {
        let dependency_order = self.resolve_dependencies(active_core_schema)?;
        let providers = index_providers(&self.packs);

        for pack in &self.packs {
            let manifest = pack.manifest();
            for capability_id in manifest.provides() {
                let Some(descriptor) = registry.get(capability_id) else {
                    return Err(RulePackSetError::UnknownProvidedCapability {
                        pack_id: manifest.id().clone(),
                        capability_id: capability_id.clone(),
                    });
                };
                if !descriptor.allows_pack_kind(manifest.kind()) {
                    return Err(RulePackSetError::InsufficientCapabilityPermission {
                        pack_id: manifest.id().clone(),
                        capability_id: capability_id.clone(),
                        found: manifest.kind(),
                        required: descriptor.minimum_pack_kind(),
                    });
                }
            }
            for capability_id in manifest.consumes() {
                if registry.get(capability_id).is_none() {
                    return Err(RulePackSetError::UnknownConsumedCapability {
                        pack_id: manifest.id().clone(),
                        capability_id: capability_id.clone(),
                    });
                }
            }
        }

        for pack in &self.packs {
            for capability_id in pack.manifest().consumes() {
                if providers
                    .get(capability_id)
                    .is_none_or(|capability_providers| capability_providers.is_empty())
                {
                    return Err(RulePackSetError::MissingConsumedCapability {
                        pack_id: pack.manifest().id().clone(),
                        capability_id: capability_id.clone(),
                    });
                }
            }
        }

        for descriptor in registry.iter() {
            let capability_providers = providers
                .get(descriptor.id())
                .map(Vec::as_slice)
                .unwrap_or_default();
            match descriptor.cardinality() {
                CapabilityCardinality::UniqueRequired if capability_providers.is_empty() => {
                    return Err(RulePackSetError::MissingRequiredCapability {
                        capability_id: descriptor.id().clone(),
                    });
                }
                CapabilityCardinality::UniqueRequired | CapabilityCardinality::UniqueOptional
                    if capability_providers.len() > 1 =>
                {
                    return Err(RulePackSetError::MultipleCapabilityProviders {
                        capability_id: descriptor.id().clone(),
                        provider_ids: capability_providers
                            .iter()
                            .map(|pack| pack.manifest().id().clone())
                            .collect(),
                    });
                }
                CapabilityCardinality::UniqueRequired
                | CapabilityCardinality::UniqueOptional
                | CapabilityCardinality::Merge => {}
            }
        }

        Ok(ResolvedRulePackSet {
            packs: dependency_order.packs,
            providers,
        })
    }
}

impl<'de> Deserialize<'de> for RulePackSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RulePackSetWire::deserialize(deserializer)?;
        Self::new(wire.packs).map_err(D::Error::custom)
    }
}

/// A validated rule-pack set in deterministic dependency order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRulePackSet<'a> {
    packs: Vec<&'a RulePack>,
    providers: BTreeMap<CapabilityId, Vec<&'a RulePack>>,
}

impl<'a> ResolvedRulePackSet<'a> {
    /// Iterates through packs with every dependency before its consumers.
    pub fn packs(&self) -> impl ExactSizeIterator<Item = &'a RulePack> + '_ {
        self.packs.iter().copied()
    }

    /// Returns the number of resolved packs.
    pub fn len(&self) -> usize {
        self.packs.len()
    }

    /// Returns whether the resolved set contains no packs.
    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }

    /// Returns providers in stable pack-ID order for one exact capability.
    pub fn providers(&self, capability_id: &CapabilityId) -> &[&'a RulePack] {
        self.providers
            .get(capability_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

fn index_providers(packs: &[RulePack]) -> BTreeMap<CapabilityId, Vec<&RulePack>> {
    let mut providers = BTreeMap::<_, Vec<_>>::new();
    for pack in packs {
        for capability_id in pack.manifest().provides() {
            providers
                .entry(capability_id.clone())
                .or_default()
                .push(pack);
        }
    }
    providers
}
