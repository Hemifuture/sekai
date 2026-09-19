use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::engine::diagnostics::is_valid_identifier;
use crate::world::fields::FieldId;
use crate::world::{AuthorObjectId, CellId};

const MAX_FACTORS_PER_ENTITY: usize = 16;

/// A typed source that contributed to a reported factor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SourceRef {
    /// A registered extension field.
    Field(FieldId),
    /// A generation stage.
    Stage(String),
    /// A registered rule pack.
    RulePack(String),
    /// An authored constraint object.
    AuthorConstraint(AuthorObjectId),
}

/// A typed entity whose major factors are recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EntityRef {
    /// A spatial cell.
    Cell(CellId),
    /// An authored object.
    AuthorObject(AuthorObjectId),
}

/// Errors returned while constructing or combining factor contributions.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ProvenanceError {
    /// A factor code violates the V1 identifier grammar.
    #[error("factor code must use the supported lowercase ASCII identifier grammar")]
    InvalidCode,
    /// A reason identifier violates the V1 identifier grammar.
    #[error("reason identifier must use the supported lowercase ASCII identifier grammar")]
    InvalidReasonId,
    /// A factor weight is NaN or infinite.
    #[error("factor weight must be finite, got {0}")]
    NonFiniteWeight(
        /// The rejected factor weight.
        f32,
    ),
    /// Merging two finite factor weights produced a non-finite result.
    #[error("merged factor weight must remain finite")]
    NonFiniteMergedWeight,
}

/// One weighted explanation for an entity-level result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FactorContribution {
    code: String,
    source: SourceRef,
    weight: f32,
    reason_id: String,
}

#[derive(Deserialize)]
struct FactorContributionWire {
    code: String,
    source: SourceRef,
    weight: f32,
    reason_id: String,
}

impl FactorContribution {
    /// Creates a finite factor contribution with stable code and reason identifiers.
    pub fn new(
        code: impl Into<String>,
        source: SourceRef,
        weight: f32,
        reason_id: impl Into<String>,
    ) -> Result<Self, ProvenanceError> {
        let code = code.into();
        if !is_valid_identifier(&code) {
            return Err(ProvenanceError::InvalidCode);
        }
        let reason_id = reason_id.into();
        if !is_valid_identifier(&reason_id) {
            return Err(ProvenanceError::InvalidReasonId);
        }
        if !weight.is_finite() {
            return Err(ProvenanceError::NonFiniteWeight(weight));
        }
        Ok(Self {
            code,
            source,
            weight,
            reason_id,
        })
    }

    /// Returns the stable factor code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the contributing source.
    pub fn source(&self) -> &SourceRef {
        &self.source
    }

    /// Returns the signed finite contribution weight.
    pub const fn weight(&self) -> f32 {
        self.weight
    }

    /// Returns the stable reason identifier used to merge equivalent factors.
    pub fn reason_id(&self) -> &str {
        &self.reason_id
    }
}

impl<'de> Deserialize<'de> for FactorContribution {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FactorContributionWire::deserialize(deserializer)?;
        Self::new(wire.code, wire.source, wire.weight, wire.reason_id).map_err(D::Error::custom)
    }
}

/// Bounded, deterministic field and entity provenance summaries.
#[derive(Debug, Clone, Default)]
pub struct ProvenanceIndex {
    field_dependencies: BTreeMap<FieldId, Vec<FieldId>>,
    factors: BTreeMap<EntityRef, Vec<FactorContribution>>,
}

impl ProvenanceIndex {
    /// Creates an empty provenance index.
    pub const fn new() -> Self {
        Self {
            field_dependencies: BTreeMap::new(),
            factors: BTreeMap::new(),
        }
    }

    /// Adds a field dependency while retaining sorted, unique entries.
    pub fn add_field_dependency(&mut self, field: FieldId, dependency: FieldId) {
        let dependencies = self.field_dependencies.entry(field).or_default();
        match dependencies.binary_search(&dependency) {
            Ok(_) => {}
            Err(index) => dependencies.insert(index, dependency),
        }
    }

    /// Returns the sorted, unique dependencies of a field, or an empty slice when absent.
    pub fn field_dependencies(&self, field: &FieldId) -> &[FieldId] {
        self.field_dependencies
            .get(field)
            .map_or(&[], Vec::as_slice)
    }

    /// Replaces an entity's factors after atomically merging, ordering, and bounding them.
    pub fn replace_factors(
        &mut self,
        entity: EntityRef,
        contributions: impl IntoIterator<Item = FactorContribution>,
    ) -> Result<(), ProvenanceError> {
        let mut grouped = BTreeMap::<(String, SourceRef), Vec<FactorContribution>>::new();
        for factor in contributions {
            let key = (factor.reason_id.clone(), factor.source.clone());
            grouped.entry(key).or_default().push(factor);
        }

        let mut factors = Vec::with_capacity(grouped.len());
        for (_, mut group) in grouped {
            group.sort_by(|left, right| {
                left.code
                    .cmp(&right.code)
                    .then_with(|| left.weight.total_cmp(&right.weight))
            });
            let mut factors_to_merge = group.into_iter();
            let mut merged = factors_to_merge
                .next()
                .expect("each grouped provenance key has at least one factor");
            for factor in factors_to_merge {
                let merged_weight = merged.weight + factor.weight;
                if !merged_weight.is_finite() {
                    return Err(ProvenanceError::NonFiniteMergedWeight);
                }
                merged.weight = merged_weight;
            }
            factors.push(merged);
        }
        factors.sort_by(compare_factors);
        factors.truncate(MAX_FACTORS_PER_ENTITY);
        self.factors.insert(entity, factors);
        Ok(())
    }

    /// Returns the bounded, deterministically ordered factors for an entity.
    pub fn factors(&self, entity: &EntityRef) -> &[FactorContribution] {
        self.factors.get(entity).map_or(&[], Vec::as_slice)
    }
}

fn compare_factors(left: &FactorContribution, right: &FactorContribution) -> Ordering {
    right
        .weight
        .abs()
        .total_cmp(&left.weight.abs())
        .then_with(|| left.reason_id.cmp(&right.reason_id))
        .then_with(|| left.source.cmp(&right.source))
}
