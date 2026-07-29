use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    AuthorConstraints, CapabilityContribution, ConstraintError, ConstraintSource,
    ConstraintStrength, ResolvedRulePackSet, RuleContentHash, RulePackId, RuleVersion,
    TectonicConstraintClause, TectonicControl, TectonicModel, MAX_CONTINENTAL_CRUST_PERMILLE,
    MIN_CONTINENTAL_CRUST_PERMILLE,
};
use crate::world::natural::{
    NaturalSpecError, TectonicActivity, TectonicSpec, MAX_PLATE_COUNT, MIN_PLATE_COUNT,
};

/// The supported serialized schema for a tectonic rule-resolution audit.
pub const TECTONIC_RULE_RESOLUTION_SCHEMA_V1: u16 = 1;

/// Whether a resolved constraint was honored by the final control value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConstraintAdoptionOutcome {
    /// The final value lies inside the constraint's allowed range or set.
    Satisfied,
    /// A lower-priority preference was not selected.
    Compromised,
}

/// One stable audit reference to a rule pack that participated in resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRulePackRef {
    pack_id: RulePackId,
    version: RuleVersion,
    content_hash: RuleContentHash,
}

impl ResolvedRulePackRef {
    /// Returns the participating pack ID.
    pub const fn pack_id(&self) -> &RulePackId {
        &self.pack_id
    }

    /// Returns the exact participating pack version.
    pub const fn version(&self) -> RuleVersion {
        self.version
    }

    /// Returns the exact participating semantic content hash.
    pub const fn content_hash(&self) -> RuleContentHash {
        self.content_hash
    }
}

/// One deterministic record of how a typed constraint affected the result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstraintAdoption {
    source: ConstraintSource,
    strength: ConstraintStrength,
    target: TectonicControl,
    outcome: ConstraintAdoptionOutcome,
}

impl ConstraintAdoption {
    /// Returns the exact rule or author source.
    pub const fn source(&self) -> &ConstraintSource {
        &self.source
    }

    /// Returns the declared constraint strength.
    pub const fn strength(&self) -> ConstraintStrength {
        self.strength
    }

    /// Returns the independently solved target.
    pub const fn target(&self) -> TectonicControl {
        self.target
    }

    /// Returns whether the final value satisfied the constraint.
    pub const fn outcome(&self) -> ConstraintAdoptionOutcome {
        self.outcome
    }
}

/// Errors returned while resolving or revalidating a tectonic rule audit.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TectonicRuleResolutionError {
    /// The base preference spec is invalid.
    #[error("invalid base tectonic specification: {0}")]
    InvalidBaseSpec(NaturalSpecError),
    /// The author-constraint input is invalid.
    #[error("invalid author constraints: {0}")]
    InvalidAuthorConstraints(ConstraintError),
    /// A capability-resolved pack set contains no model contribution.
    #[error("resolved rule-pack set contains no tectonic model")]
    MissingTectonicModel,
    /// A supposedly capability-resolved pack set contains several models.
    #[error("resolved rule-pack set contains several tectonic model contributions")]
    MultipleTectonicModels,
    /// Hard constraints leave no candidate for one target.
    #[error("hard constraints conflict on {target:?}; sources: {sources:?}")]
    HardConstraintConflict {
        /// The infeasible independently solved control.
        target: TectonicControl,
        /// Every hard source on that target, sorted and deduplicated.
        sources: Vec<ConstraintSource>,
    },
    /// Checked preference score accumulation overflowed.
    #[error("preference score overflow while resolving {target:?}")]
    ScoreOverflow {
        /// The target whose score could not be represented.
        target: TectonicControl,
    },
    /// A resolved specification failed the natural-domain contract.
    #[error("invalid resolved tectonic specification: {0}")]
    InvalidResolvedSpec(NaturalSpecError),
    /// A serialized audit uses an unsupported schema.
    #[error(
        "unsupported tectonic rule-resolution schema {found}; supported schema is {TECTONIC_RULE_RESOLUTION_SCHEMA_V1}"
    )]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
    },
    /// A serialized audit repeats a participating pack ID.
    #[error("resolved rule-pack audit repeats pack {pack_id:?}")]
    DuplicateResolvedPack {
        /// The repeated pack ID.
        pack_id: RulePackId,
    },
    /// Serialized adoption records are not in canonical order.
    #[error("constraint adoption records are not in strict canonical order")]
    NonCanonicalAdoptionOrder,
    /// A hard constraint was serialized as compromised.
    #[error("hard constraint {constraint_source:?} on {target:?} cannot be compromised")]
    HardConstraintCompromised {
        /// The invalid hard source.
        constraint_source: ConstraintSource,
        /// The invalid target.
        target: TectonicControl,
    },
    /// A rule-sourced adoption names a pack absent from the audit.
    #[error("constraint adoption refers to absent rule pack {pack_id:?}")]
    UnknownAdoptionRulePack {
        /// The absent pack ID.
        pack_id: RulePackId,
    },
}

/// A full, deterministic audit of one tectonic rule resolution.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TectonicRuleResolution {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: TectonicModel,
    spec: TectonicSpec,
    adoptions: Vec<ConstraintAdoption>,
}

#[derive(Deserialize)]
struct TectonicRuleResolutionWire {
    schema_version: u16,
    resolved_packs: Vec<ResolvedRulePackRef>,
    model: TectonicModel,
    spec: TectonicSpec,
    adoptions: Vec<ConstraintAdoption>,
}

impl TectonicRuleResolution {
    /// Returns the serialized audit schema.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns participating pack identities in resolved dependency order.
    pub fn resolved_packs(&self) -> &[ResolvedRulePackRef] {
        &self.resolved_packs
    }

    /// Returns the selected trusted tectonic model.
    pub const fn model(&self) -> TectonicModel {
        self.model
    }

    /// Returns the final validated tectonic specification.
    pub const fn spec(&self) -> &TectonicSpec {
        &self.spec
    }

    /// Returns stable constraint-adoption records.
    pub fn adoptions(&self) -> &[ConstraintAdoption] {
        &self.adoptions
    }

    /// Revalidates all serialized audit invariants.
    pub fn validate(&self) -> Result<(), TectonicRuleResolutionError> {
        if self.schema_version != TECTONIC_RULE_RESOLUTION_SCHEMA_V1 {
            return Err(TectonicRuleResolutionError::UnsupportedSchema {
                found: self.schema_version,
            });
        }
        self.spec
            .validate()
            .map_err(TectonicRuleResolutionError::InvalidResolvedSpec)?;

        let mut pack_ids = BTreeSet::new();
        for pack in &self.resolved_packs {
            if !pack_ids.insert(pack.pack_id.clone()) {
                return Err(TectonicRuleResolutionError::DuplicateResolvedPack {
                    pack_id: pack.pack_id.clone(),
                });
            }
        }

        if !self
            .adoptions
            .windows(2)
            .all(|pair| adoption_key(&pair[0]) < adoption_key(&pair[1]))
        {
            return Err(TectonicRuleResolutionError::NonCanonicalAdoptionOrder);
        }
        for adoption in &self.adoptions {
            if adoption.strength == ConstraintStrength::Hard
                && adoption.outcome == ConstraintAdoptionOutcome::Compromised
            {
                return Err(TectonicRuleResolutionError::HardConstraintCompromised {
                    constraint_source: adoption.source.clone(),
                    target: adoption.target,
                });
            }
            if let ConstraintSource::RulePack { pack_id, .. } = &adoption.source {
                if !pack_ids.contains(pack_id) {
                    return Err(TectonicRuleResolutionError::UnknownAdoptionRulePack {
                        pack_id: pack_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for TectonicRuleResolution {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = TectonicRuleResolutionWire::deserialize(deserializer)?;
        let resolution = Self {
            schema_version: wire.schema_version,
            resolved_packs: wire.resolved_packs,
            model: wire.model,
            spec: wire.spec,
            adoptions: wire.adoptions,
        };
        resolution.validate().map_err(D::Error::custom)?;
        Ok(resolution)
    }
}

/// Stateless resolver for the closed V1 tectonic rule capabilities.
#[derive(Debug, Clone, Copy, Default)]
pub struct TectonicRuleResolver;

impl TectonicRuleResolver {
    /// Resolves capability-validated packs and authors against a base preference spec.
    pub fn resolve(
        base: &TectonicSpec,
        packs: &ResolvedRulePackSet<'_>,
        authors: &AuthorConstraints,
    ) -> Result<TectonicRuleResolution, TectonicRuleResolutionError> {
        base.validate()
            .map_err(TectonicRuleResolutionError::InvalidBaseSpec)?;
        authors
            .validate()
            .map_err(TectonicRuleResolutionError::InvalidAuthorConstraints)?;

        let mut model = None;
        let mut constraints = Vec::new();
        let mut resolved_packs = Vec::with_capacity(packs.len());
        for pack in packs.packs() {
            let manifest = pack.manifest();
            resolved_packs.push(ResolvedRulePackRef {
                pack_id: manifest.id().clone(),
                version: manifest.version(),
                content_hash: manifest.content_hash(),
            });
            for contribution in pack.contributions() {
                match contribution {
                    CapabilityContribution::TectonicModel(candidate) => {
                        if model.replace(*candidate).is_some() {
                            return Err(TectonicRuleResolutionError::MultipleTectonicModels);
                        }
                    }
                    CapabilityContribution::GeologicModel(_) => {}
                    CapabilityContribution::TectonicConstraint(constraint) => {
                        constraints.push(SourcedConstraint {
                            source: ConstraintSource::RulePack {
                                pack_id: manifest.id().clone(),
                                item_id: constraint.item_id().clone(),
                            },
                            strength: constraint.strength(),
                            clause: constraint.clause().clone(),
                        });
                    }
                }
            }
        }
        let model = model.ok_or(TectonicRuleResolutionError::MissingTectonicModel)?;
        constraints.extend(
            authors
                .constraints()
                .iter()
                .map(|constraint| SourcedConstraint {
                    source: ConstraintSource::Author(constraint.id()),
                    strength: constraint.strength(),
                    clause: constraint.clause().clone(),
                }),
        );
        constraints.sort_by(|left, right| {
            (&left.source, left.clause.target()).cmp(&(&right.source, right.clause.target()))
        });

        let mut spec = base.clone();
        for target in [
            TectonicControl::PlateCount,
            TectonicControl::ContinentalCrustFraction,
            TectonicControl::Activity,
        ] {
            let target_constraints: Vec<_> = constraints
                .iter()
                .filter(|constraint| constraint.clause.target() == target)
                .collect();
            if target_constraints.is_empty() {
                continue;
            }
            let candidate = solve_target(base, target, &target_constraints)?;
            match candidate {
                Candidate::Numeric(value) if target == TectonicControl::PlateCount => {
                    spec.plate_count = value;
                }
                Candidate::Numeric(value)
                    if target == TectonicControl::ContinentalCrustFraction =>
                {
                    spec.continental_crust_fraction = f32::from(value) / 1000.0;
                }
                Candidate::Activity(activity) if target == TectonicControl::Activity => {
                    spec.activity = activity;
                }
                _ => unreachable!("candidate kind must match its independently solved target"),
            }
        }
        spec.validate()
            .map_err(TectonicRuleResolutionError::InvalidResolvedSpec)?;

        let mut adoptions: Vec<_> = constraints
            .iter()
            .map(|constraint| ConstraintAdoption {
                source: constraint.source.clone(),
                strength: constraint.strength,
                target: constraint.clause.target(),
                outcome: if clause_satisfied_by_spec(&constraint.clause, &spec) {
                    ConstraintAdoptionOutcome::Satisfied
                } else {
                    ConstraintAdoptionOutcome::Compromised
                },
            })
            .collect();
        adoptions.sort_by(|left, right| adoption_key(left).cmp(&adoption_key(right)));

        let resolution = TectonicRuleResolution {
            schema_version: TECTONIC_RULE_RESOLUTION_SCHEMA_V1,
            resolved_packs,
            model,
            spec,
            adoptions,
        };
        resolution.validate()?;
        Ok(resolution)
    }
}

#[derive(Debug, Clone)]
struct SourcedConstraint {
    source: ConstraintSource,
    strength: ConstraintStrength,
    clause: TectonicConstraintClause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Candidate {
    Numeric(u16),
    Activity(TectonicActivity),
}

impl Candidate {
    fn stable_value(self) -> u16 {
        match self {
            Self::Numeric(value) => value,
            Self::Activity(activity) => activity_value(activity),
        }
    }
}

fn solve_target(
    base: &TectonicSpec,
    target: TectonicControl,
    constraints: &[&SourcedConstraint],
) -> Result<Candidate, TectonicRuleResolutionError> {
    let mut candidates: Vec<_> = match target {
        TectonicControl::PlateCount => (MIN_PLATE_COUNT..=MAX_PLATE_COUNT)
            .map(Candidate::Numeric)
            .collect(),
        TectonicControl::ContinentalCrustFraction => (MIN_CONTINENTAL_CRUST_PERMILLE
            ..=MAX_CONTINENTAL_CRUST_PERMILLE)
            .map(Candidate::Numeric)
            .collect(),
        TectonicControl::Activity => [
            TectonicActivity::Quiet,
            TectonicActivity::Moderate,
            TectonicActivity::Active,
        ]
        .into_iter()
        .map(Candidate::Activity)
        .collect(),
    };
    candidates.retain(|&candidate| {
        constraints.iter().all(|constraint| {
            constraint.strength != ConstraintStrength::Hard
                || clause_distance(&constraint.clause, candidate) == 0
        })
    });
    if candidates.is_empty() {
        let sources = constraints
            .iter()
            .filter(|constraint| constraint.strength == ConstraintStrength::Hard)
            .map(|constraint| constraint.source.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        return Err(TectonicRuleResolutionError::HardConstraintConflict { target, sources });
    }
    let mut best = None;
    for candidate in candidates {
        let mut soft_penalty = 0u64;
        let mut hint_penalty = 0u64;
        for constraint in constraints {
            let (score, weight) = match constraint.strength {
                ConstraintStrength::Hard => continue,
                ConstraintStrength::Soft(weight) => (&mut soft_penalty, weight),
                ConstraintStrength::Hint(weight) => (&mut hint_penalty, weight),
            };
            let weighted_distance = clause_distance(&constraint.clause, candidate)
                .checked_mul(u64::from(weight.get()))
                .ok_or(TectonicRuleResolutionError::ScoreOverflow { target })?;
            *score = score
                .checked_add(weighted_distance)
                .ok_or(TectonicRuleResolutionError::ScoreOverflow { target })?;
        }
        let score = (
            soft_penalty,
            hint_penalty,
            base_distance(base, target, candidate),
            candidate.stable_value(),
        );
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score < *best_score)
        {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, candidate)| candidate).ok_or(
        TectonicRuleResolutionError::HardConstraintConflict {
            target,
            sources: Vec::new(),
        },
    )
}

fn clause_distance(clause: &TectonicConstraintClause, candidate: Candidate) -> u64 {
    match (clause, candidate) {
        (TectonicConstraintClause::PlateCount(range), Candidate::Numeric(value))
        | (TectonicConstraintClause::ContinentalCrustPermille(range), Candidate::Numeric(value)) => {
            range_distance(value, range.minimum(), range.maximum())
        }
        (TectonicConstraintClause::Activity(allowed), Candidate::Activity(activity)) => allowed
            .values()
            .iter()
            .map(|&allowed_activity| {
                u64::from(activity_value(activity).abs_diff(activity_value(allowed_activity)))
            })
            .min()
            .expect("validated activity constraints are non-empty"),
        _ => unreachable!("constraint clause and candidate target must match"),
    }
}

fn range_distance(value: u16, minimum: u16, maximum: u16) -> u64 {
    if value < minimum {
        u64::from(minimum - value)
    } else if value > maximum {
        u64::from(value - maximum)
    } else {
        0
    }
}

fn base_distance(base: &TectonicSpec, target: TectonicControl, candidate: Candidate) -> u64 {
    match (target, candidate) {
        (TectonicControl::PlateCount, Candidate::Numeric(value)) => {
            u64::from(value.abs_diff(base.plate_count))
        }
        (TectonicControl::ContinentalCrustFraction, Candidate::Numeric(value)) => {
            u64::from(value.abs_diff(base_fraction_permille(base)))
        }
        (TectonicControl::Activity, Candidate::Activity(activity)) => {
            u64::from(activity_value(activity).abs_diff(activity_value(base.activity)))
        }
        _ => unreachable!("candidate kind must match base-distance target"),
    }
}

fn base_fraction_permille(base: &TectonicSpec) -> u16 {
    (f64::from(base.continental_crust_fraction) * 1000.0).round() as u16
}

const fn activity_value(activity: TectonicActivity) -> u16 {
    match activity {
        TectonicActivity::Quiet => 0,
        TectonicActivity::Moderate => 1,
        TectonicActivity::Active => 2,
    }
}

fn clause_satisfied_by_spec(clause: &TectonicConstraintClause, spec: &TectonicSpec) -> bool {
    match clause {
        TectonicConstraintClause::PlateCount(range) => range.contains(spec.plate_count),
        TectonicConstraintClause::ContinentalCrustPermille(range) => {
            range.contains(base_fraction_permille(spec))
        }
        TectonicConstraintClause::Activity(allowed) => allowed.contains(spec.activity),
    }
}

fn adoption_key(adoption: &ConstraintAdoption) -> (&ConstraintSource, TectonicControl) {
    (&adoption.source, adoption.target)
}
