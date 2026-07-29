use std::collections::{BTreeMap, BTreeSet};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    CoreSchemaRange, RulePack, RulePackError, RulePackId, RuleVersion, RuleVersionRequirement,
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

        Ok(ResolvedRulePackSet {
            packs: ordered_indices
                .into_iter()
                .map(|index| &self.packs[index])
                .collect(),
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
}
