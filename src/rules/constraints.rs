use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{RuleItemId, RulePackId};
use crate::world::natural::{
    TectonicActivity, MAX_CONTINENTAL_CRUST_FRACTION, MAX_PLATE_COUNT,
    MIN_CONTINENTAL_CRUST_FRACTION, MIN_PLATE_COUNT,
};
use crate::world::AuthorObjectId;

/// The supported serialized schema for author constraints.
pub const AUTHOR_CONSTRAINTS_SCHEMA_V1: u16 = 1;
/// The largest author-constraint collection accepted by V1.
pub const MAX_AUTHOR_CONSTRAINTS: usize = 4096;
/// The smallest supported constraint weight.
pub const MIN_CONSTRAINT_WEIGHT: u16 = 1;
/// The largest supported constraint weight.
pub const MAX_CONSTRAINT_WEIGHT: u16 = 1000;
/// The smallest supported continental-crust share in integer permille.
pub const MIN_CONTINENTAL_CRUST_PERMILLE: u16 = (MIN_CONTINENTAL_CRUST_FRACTION * 1000.0) as u16;
/// The largest supported continental-crust share in integer permille.
pub const MAX_CONTINENTAL_CRUST_PERMILLE: u16 = (MAX_CONTINENTAL_CRUST_FRACTION * 1000.0) as u16;

/// Errors returned while constructing typed rule and author constraints.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConstraintError {
    /// A soft or hint weight lies outside the V1 bound.
    #[error(
        "constraint weight {found} is outside {MIN_CONSTRAINT_WEIGHT}..={MAX_CONSTRAINT_WEIGHT}"
    )]
    WeightOutOfRange {
        /// The rejected weight.
        found: u16,
    },
    /// An inclusive integer range is reversed.
    #[error("constraint range minimum {minimum} exceeds maximum {maximum}")]
    ReversedRange {
        /// The rejected minimum.
        minimum: u16,
        /// The rejected maximum.
        maximum: u16,
    },
    /// A plate-count clause exceeds the natural specification bound.
    #[error(
        "plate-count range {minimum}..={maximum} is outside {MIN_PLATE_COUNT}..={MAX_PLATE_COUNT}"
    )]
    PlateCountOutOfRange {
        /// The rejected minimum plate count.
        minimum: u16,
        /// The rejected maximum plate count.
        maximum: u16,
    },
    /// A continental-crust clause exceeds the natural specification bound.
    #[error(
        "continental-crust range {minimum}..={maximum} permille is outside {MIN_CONTINENTAL_CRUST_PERMILLE}..={MAX_CONTINENTAL_CRUST_PERMILLE}"
    )]
    ContinentalCrustPermilleOutOfRange {
        /// The rejected minimum permille value.
        minimum: u16,
        /// The rejected maximum permille value.
        maximum: u16,
    },
    /// An activity clause contains no allowed values.
    #[error("tectonic activity set must not be empty")]
    EmptyActivitySet,
    /// An activity clause repeats one value.
    #[error("tectonic activity set contains duplicate value {activity:?}")]
    DuplicateActivity {
        /// The repeated activity.
        activity: TectonicActivity,
    },
    /// The serialized author-constraint schema is unsupported.
    #[error(
        "unsupported author-constraint schema {found}; supported schema is {AUTHOR_CONSTRAINTS_SCHEMA_V1}"
    )]
    UnsupportedAuthorSchema {
        /// The rejected schema version.
        found: u16,
    },
    /// An author-constraint collection exceeds its allocation budget.
    #[error("author constraint count {found} exceeds V1 limit {MAX_AUTHOR_CONSTRAINTS}")]
    TooManyAuthorConstraints {
        /// The rejected collection length.
        found: usize,
    },
    /// An authored object ID occurs more than once.
    #[error("duplicate author constraint ID {id:?}")]
    DuplicateAuthorConstraint {
        /// The repeated authored object ID.
        id: AuthorObjectId,
    },
    /// A stored author-constraint collection is not in canonical ID order.
    #[error("author constraints must be stored in strictly increasing ID order")]
    NonCanonicalAuthorOrder,
}

/// A validated positive integer constraint weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ConstraintWeight(u16);

impl ConstraintWeight {
    /// Creates a weight inside the V1 bound.
    pub const fn new(value: u16) -> Result<Self, ConstraintError> {
        if value < MIN_CONSTRAINT_WEIGHT || value > MAX_CONSTRAINT_WEIGHT {
            return Err(ConstraintError::WeightOutOfRange { found: value });
        }
        Ok(Self(value))
    }

    /// Returns the validated integer weight.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ConstraintWeight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u16::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Whether a constraint is mandatory, a scored preference, or a style hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConstraintStrength {
    /// The final value must satisfy the clause.
    Hard,
    /// The clause contributes to the primary preference score.
    Soft(ConstraintWeight),
    /// The clause contributes only after all soft scores tie.
    Hint(ConstraintWeight),
}

impl ConstraintStrength {
    /// Creates a validated soft constraint.
    pub const fn soft(weight: u16) -> Result<Self, ConstraintError> {
        match ConstraintWeight::new(weight) {
            Ok(weight) => Ok(Self::Soft(weight)),
            Err(error) => Err(error),
        }
    }

    /// Creates a validated style hint.
    pub const fn hint(weight: u16) -> Result<Self, ConstraintError> {
        match ConstraintWeight::new(weight) {
            Ok(weight) => Ok(Self::Hint(weight)),
            Err(error) => Err(error),
        }
    }

    /// Returns a soft or hint weight, and `None` for hard constraints.
    pub const fn weight(self) -> Option<u16> {
        match self {
            Self::Hard => None,
            Self::Soft(weight) | Self::Hint(weight) => Some(weight.get()),
        }
    }
}

/// A validated inclusive range of unsigned integer control values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct InclusiveU16Range {
    minimum: u16,
    maximum: u16,
}

#[derive(Deserialize)]
struct InclusiveU16RangeWire {
    minimum: u16,
    maximum: u16,
}

impl InclusiveU16Range {
    /// Creates an ordered inclusive range.
    pub const fn new(minimum: u16, maximum: u16) -> Result<Self, ConstraintError> {
        if minimum > maximum {
            return Err(ConstraintError::ReversedRange { minimum, maximum });
        }
        Ok(Self { minimum, maximum })
    }

    /// Returns the inclusive minimum.
    pub const fn minimum(self) -> u16 {
        self.minimum
    }

    /// Returns the inclusive maximum.
    pub const fn maximum(self) -> u16 {
        self.maximum
    }

    /// Returns whether a value lies inside the range.
    pub const fn contains(self, value: u16) -> bool {
        value >= self.minimum && value <= self.maximum
    }
}

impl<'de> Deserialize<'de> for InclusiveU16Range {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = InclusiveU16RangeWire::deserialize(deserializer)?;
        Self::new(wire.minimum, wire.maximum).map_err(D::Error::custom)
    }
}

/// A non-empty, sorted set of allowed broad tectonic activity values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ActivitySet(Vec<TectonicActivity>);

impl ActivitySet {
    /// Creates a sorted activity set and rejects repeated values.
    pub fn new(
        activities: impl IntoIterator<Item = TectonicActivity>,
    ) -> Result<Self, ConstraintError> {
        let mut activities: Vec<_> = activities.into_iter().collect();
        activities.sort_unstable();
        if activities.is_empty() {
            return Err(ConstraintError::EmptyActivitySet);
        }
        if let Some(activity) = activities
            .windows(2)
            .find_map(|pair| (pair[0] == pair[1]).then_some(pair[0]))
        {
            return Err(ConstraintError::DuplicateActivity { activity });
        }
        Ok(Self(activities))
    }

    /// Returns allowed values in stable order.
    pub fn values(&self) -> &[TectonicActivity] {
        &self.0
    }

    /// Returns whether one activity is allowed.
    pub fn contains(&self, activity: TectonicActivity) -> bool {
        self.0.binary_search(&activity).is_ok()
    }
}

impl<'de> Deserialize<'de> for ActivitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<TectonicActivity>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// One independently solved tectonic control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TectonicControl {
    /// Number of generated plates.
    PlateCount,
    /// Share of cells assigned continental crust.
    ContinentalCrustFraction,
    /// Broad present-day plate-motion activity.
    Activity,
}

/// A closed, validated clause over one tectonic control.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum TectonicConstraintClause {
    /// An allowed inclusive plate-count range.
    PlateCount(InclusiveU16Range),
    /// An allowed inclusive continental-crust range in permille.
    ContinentalCrustPermille(InclusiveU16Range),
    /// An allowed set of broad activity values.
    Activity(ActivitySet),
}

#[derive(Deserialize)]
enum TectonicConstraintClauseWire {
    PlateCount(InclusiveU16Range),
    ContinentalCrustPermille(InclusiveU16Range),
    Activity(ActivitySet),
}

impl TectonicConstraintClause {
    /// Creates a plate-count clause inside the natural-spec safety bound.
    pub fn plate_count(minimum: u16, maximum: u16) -> Result<Self, ConstraintError> {
        Self::PlateCount(InclusiveU16Range::new(minimum, maximum)?).validated()
    }

    /// Creates a continental-crust fraction clause in integer permille.
    pub fn continental_crust_permille(minimum: u16, maximum: u16) -> Result<Self, ConstraintError> {
        Self::ContinentalCrustPermille(InclusiveU16Range::new(minimum, maximum)?).validated()
    }

    /// Creates a non-empty tectonic-activity clause.
    pub fn activity(
        activities: impl IntoIterator<Item = TectonicActivity>,
    ) -> Result<Self, ConstraintError> {
        Self::Activity(ActivitySet::new(activities)?).validated()
    }

    /// Returns the control targeted by this clause.
    pub const fn target(&self) -> TectonicControl {
        match self {
            Self::PlateCount(_) => TectonicControl::PlateCount,
            Self::ContinentalCrustPermille(_) => TectonicControl::ContinentalCrustFraction,
            Self::Activity(_) => TectonicControl::Activity,
        }
    }

    /// Revalidates this clause.
    pub fn validate(&self) -> Result<(), ConstraintError> {
        match self {
            Self::PlateCount(range)
                if range.minimum() < MIN_PLATE_COUNT || range.maximum() > MAX_PLATE_COUNT =>
            {
                Err(ConstraintError::PlateCountOutOfRange {
                    minimum: range.minimum(),
                    maximum: range.maximum(),
                })
            }
            Self::ContinentalCrustPermille(range)
                if range.minimum() < MIN_CONTINENTAL_CRUST_PERMILLE
                    || range.maximum() > MAX_CONTINENTAL_CRUST_PERMILLE =>
            {
                Err(ConstraintError::ContinentalCrustPermilleOutOfRange {
                    minimum: range.minimum(),
                    maximum: range.maximum(),
                })
            }
            Self::Activity(activities) if activities.values().is_empty() => {
                Err(ConstraintError::EmptyActivitySet)
            }
            _ => Ok(()),
        }
    }

    fn validated(self) -> Result<Self, ConstraintError> {
        self.validate()?;
        Ok(self)
    }
}

impl<'de> Deserialize<'de> for TectonicConstraintClause {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let clause = match TectonicConstraintClauseWire::deserialize(deserializer)? {
            TectonicConstraintClauseWire::PlateCount(range) => Self::PlateCount(range),
            TectonicConstraintClauseWire::ContinentalCrustPermille(range) => {
                Self::ContinentalCrustPermille(range)
            }
            TectonicConstraintClauseWire::Activity(activities) => Self::Activity(activities),
        };
        clause.validated().map_err(D::Error::custom)
    }
}

/// A stable source for one resolved rule or author constraint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConstraintSource {
    /// A local item inside a validated rule pack.
    RulePack {
        /// The contributing rule pack.
        pack_id: RulePackId,
        /// The contributing local rule item.
        item_id: RuleItemId,
    },
    /// An authored project object.
    Author(AuthorObjectId),
}

/// One typed tectonic constraint contributed by a rule pack.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct RuleTectonicConstraint {
    item_id: RuleItemId,
    strength: ConstraintStrength,
    clause: TectonicConstraintClause,
}

#[derive(Deserialize)]
struct RuleTectonicConstraintWire {
    item_id: RuleItemId,
    strength: ConstraintStrength,
    clause: TectonicConstraintClause,
}

impl RuleTectonicConstraint {
    /// Creates a validated rule-pack tectonic constraint.
    pub fn new(
        item_id: RuleItemId,
        strength: ConstraintStrength,
        clause: TectonicConstraintClause,
    ) -> Result<Self, ConstraintError> {
        clause.validate()?;
        Ok(Self {
            item_id,
            strength,
            clause,
        })
    }

    /// Returns the local rule item identifier.
    pub fn item_id(&self) -> &RuleItemId {
        &self.item_id
    }

    /// Returns the constraint strength.
    pub const fn strength(&self) -> ConstraintStrength {
        self.strength
    }

    /// Returns the typed clause.
    pub const fn clause(&self) -> &TectonicConstraintClause {
        &self.clause
    }

    /// Revalidates the constraint.
    pub fn validate(&self) -> Result<(), ConstraintError> {
        self.clause.validate()
    }
}

impl<'de> Deserialize<'de> for RuleTectonicConstraint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RuleTectonicConstraintWire::deserialize(deserializer)?;
        Self::new(wire.item_id, wire.strength, wire.clause).map_err(D::Error::custom)
    }
}

/// One typed tectonic constraint authored in a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorConstraint {
    id: AuthorObjectId,
    strength: ConstraintStrength,
    clause: TectonicConstraintClause,
}

#[derive(Deserialize)]
struct AuthorConstraintWire {
    id: AuthorObjectId,
    strength: ConstraintStrength,
    clause: TectonicConstraintClause,
}

impl AuthorConstraint {
    /// Creates a validated authored tectonic constraint.
    pub fn new(
        id: AuthorObjectId,
        strength: ConstraintStrength,
        clause: TectonicConstraintClause,
    ) -> Result<Self, ConstraintError> {
        clause.validate()?;
        Ok(Self {
            id,
            strength,
            clause,
        })
    }

    /// Returns the stable authored object ID.
    pub const fn id(&self) -> AuthorObjectId {
        self.id
    }

    /// Returns the constraint strength.
    pub const fn strength(&self) -> ConstraintStrength {
        self.strength
    }

    /// Returns the typed clause.
    pub const fn clause(&self) -> &TectonicConstraintClause {
        &self.clause
    }

    /// Revalidates the constraint.
    pub fn validate(&self) -> Result<(), ConstraintError> {
        self.clause.validate()
    }
}

impl<'de> Deserialize<'de> for AuthorConstraint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = AuthorConstraintWire::deserialize(deserializer)?;
        Self::new(wire.id, wire.strength, wire.clause).map_err(D::Error::custom)
    }
}

/// A bounded, canonical collection of authored tectonic constraints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorConstraints {
    schema_version: u16,
    constraints: Vec<AuthorConstraint>,
}

#[derive(Deserialize)]
struct AuthorConstraintsWire {
    schema_version: u16,
    constraints: Vec<AuthorConstraint>,
}

impl AuthorConstraints {
    /// Creates a validated collection in canonical authored-ID order.
    pub fn new(
        schema_version: u16,
        mut constraints: Vec<AuthorConstraint>,
    ) -> Result<Self, ConstraintError> {
        if schema_version != AUTHOR_CONSTRAINTS_SCHEMA_V1 {
            return Err(ConstraintError::UnsupportedAuthorSchema {
                found: schema_version,
            });
        }
        if constraints.len() > MAX_AUTHOR_CONSTRAINTS {
            return Err(ConstraintError::TooManyAuthorConstraints {
                found: constraints.len(),
            });
        }
        constraints.sort_by_key(AuthorConstraint::id);
        if let Some(id) = constraints
            .windows(2)
            .find_map(|pair| (pair[0].id() == pair[1].id()).then_some(pair[0].id()))
        {
            return Err(ConstraintError::DuplicateAuthorConstraint { id });
        }
        let collection = Self {
            schema_version,
            constraints,
        };
        collection.validate()?;
        Ok(collection)
    }

    /// Returns the serialized schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns constraints in stable authored-ID order.
    pub fn constraints(&self) -> &[AuthorConstraint] {
        &self.constraints
    }

    /// Revalidates the schema, budget, individual clauses, and canonical order.
    pub fn validate(&self) -> Result<(), ConstraintError> {
        if self.schema_version != AUTHOR_CONSTRAINTS_SCHEMA_V1 {
            return Err(ConstraintError::UnsupportedAuthorSchema {
                found: self.schema_version,
            });
        }
        if self.constraints.len() > MAX_AUTHOR_CONSTRAINTS {
            return Err(ConstraintError::TooManyAuthorConstraints {
                found: self.constraints.len(),
            });
        }
        for constraint in &self.constraints {
            constraint.validate()?;
        }
        if !self
            .constraints
            .windows(2)
            .all(|pair| pair[0].id() < pair[1].id())
        {
            return Err(ConstraintError::NonCanonicalAuthorOrder);
        }
        Ok(())
    }
}

impl Default for AuthorConstraints {
    fn default() -> Self {
        Self {
            schema_version: AUTHOR_CONSTRAINTS_SCHEMA_V1,
            constraints: Vec::new(),
        }
    }
}

impl<'de> Deserialize<'de> for AuthorConstraints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = AuthorConstraintsWire::deserialize(deserializer)?;
        Self::new(wire.schema_version, wire.constraints).map_err(D::Error::custom)
    }
}
