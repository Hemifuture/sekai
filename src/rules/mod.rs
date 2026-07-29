//! Deterministic, data-only rule-pack and author-input contracts.

mod constraints;
mod ids;

pub use constraints::{
    ActivitySet, AuthorConstraint, AuthorConstraints, ConstraintError, ConstraintSource,
    ConstraintStrength, ConstraintWeight, InclusiveU16Range, RuleTectonicConstraint,
    TectonicConstraintClause, TectonicControl, AUTHOR_CONSTRAINTS_SCHEMA_V1,
    MAX_AUTHOR_CONSTRAINTS, MAX_CONSTRAINT_WEIGHT, MAX_CONTINENTAL_CRUST_PERMILLE,
    MIN_CONSTRAINT_WEIGHT, MIN_CONTINENTAL_CRUST_PERMILLE,
};
pub use ids::{
    CapabilityId, CoreSchemaRange, RuleContentHash, RuleIdentityError, RuleItemId, RulePackId,
    RuleVersion, RuleVersionRequirement,
};
