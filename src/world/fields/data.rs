use std::collections::BTreeMap;

use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    EntityKind, FieldDomain, FieldId, FieldRegistry, FieldValueType, StableIdKind, ValueRange,
};

/// Errors returned while validating or inserting extension-field payloads.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum FieldDataError {
    /// No schema was registered for the supplied field identifier.
    #[error("field schema {0:?} is not registered")]
    UnknownSchema(
        /// The unregistered field identifier.
        FieldId,
    ),
    /// The field already has a payload.
    #[error("field {0:?} already has a payload")]
    DuplicateField(
        /// The duplicate field identifier.
        FieldId,
    ),
    /// The payload representation does not match its schema.
    #[error("payload type does not match the schema for field {0:?}")]
    TypeMismatch(
        /// The field whose payload representation did not match.
        FieldId,
    ),
    /// A stable-ID payload references a different target kind than its schema.
    #[error("stable-ID target kind does not match the schema for field {field:?}")]
    StableIdTargetMismatch {
        /// The field whose target kind did not match.
        field: FieldId,
    },
    /// A required entity-domain cardinality was not supplied.
    #[error("no domain size was supplied for {0:?} entities")]
    MissingEntityCount(
        /// The entity kind without a supplied cardinality.
        EntityKind,
    ),
    /// The payload length does not equal its domain cardinality.
    #[error("field {field:?} has payload length {actual}, expected {expected}")]
    LengthMismatch {
        /// The field whose payload length did not match.
        field: FieldId,
        /// The required domain cardinality.
        expected: usize,
        /// The supplied payload length.
        actual: usize,
    },
    /// A floating-point payload contains a non-finite component.
    #[error("field {0:?} contains a non-finite floating-point value")]
    NonFinite(
        /// The field containing the non-finite value.
        FieldId,
    ),
    /// A scalar value lies outside the schema's inclusive valid range.
    #[error("field {field:?} contains scalar {value} outside its valid range")]
    ScalarOutOfRange {
        /// The field containing the rejected scalar.
        field: FieldId,
        /// The rejected scalar value.
        value: f32,
    },
    /// A category value was not declared by the schema.
    #[error("field {field:?} contains undeclared category {value}")]
    UnknownCategory {
        /// The field containing the rejected category.
        field: FieldId,
        /// The rejected category value.
        value: u32,
    },
    /// A stable identifier lies outside the supplied target cardinality.
    #[error("field {field:?} contains stable ID {value} outside target count {target_count}")]
    StableIdOutOfRange {
        /// The field containing the rejected reference.
        field: FieldId,
        /// The rejected raw stable identifier.
        value: u32,
        /// The number of valid reference targets.
        target_count: usize,
    },
}

/// A typed payload for one extension field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldData {
    /// One 32-bit floating-point scalar per domain object.
    ScalarF32(
        /// The scalar values in domain order.
        Vec<f32>,
    ),
    /// One unsigned category key per domain object.
    CategoryU32(
        /// The category keys in domain order.
        Vec<u32>,
    ),
    /// One boolean per domain object.
    Boolean(
        /// The boolean values in domain order.
        Vec<bool>,
    ),
    /// One two-component 32-bit floating-point vector per domain object.
    Vector2F32(
        /// The vector values in domain order.
        Vec<[f32; 2]>,
    ),
    /// One stable identifier per domain object.
    StableIdU32 {
        /// The kind of object referenced by every raw identifier.
        target: StableIdKind,
        /// The raw stable identifiers.
        values: Vec<u32>,
    },
}

impl FieldData {
    fn len(&self) -> usize {
        match self {
            Self::ScalarF32(values) => values.len(),
            Self::CategoryU32(values) => values.len(),
            Self::Boolean(values) => values.len(),
            Self::Vector2F32(values) => values.len(),
            Self::StableIdU32 { values, .. } => values.len(),
        }
    }
}

/// Explicit cardinalities for spatial and entity field domains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainSizes {
    cells: usize,
    edges: usize,
    entities: BTreeMap<EntityKind, usize>,
}

impl DomainSizes {
    /// Creates cardinalities for cell and edge domains.
    pub const fn new(cells: usize, edges: usize) -> Self {
        Self {
            cells,
            edges,
            entities: BTreeMap::new(),
        }
    }

    /// Adds or replaces the cardinality for one exact entity kind.
    pub fn with_entities(mut self, kind: EntityKind, count: usize) -> Self {
        self.entities.insert(kind, count);
        self
    }

    fn domain_count(&self, domain: FieldDomain) -> Result<usize, FieldDataError> {
        match domain {
            FieldDomain::Global => Ok(1),
            FieldDomain::Cells => Ok(self.cells),
            FieldDomain::Edges => Ok(self.edges),
            FieldDomain::Entities(kind) => self
                .entities
                .get(&kind)
                .copied()
                .ok_or(FieldDataError::MissingEntityCount(kind)),
        }
    }

    fn stable_id_count(&self, target: StableIdKind) -> Result<usize, FieldDataError> {
        match target {
            StableIdKind::Cell => Ok(self.cells),
            StableIdKind::Edge => Ok(self.edges),
            StableIdKind::Species => self.entity_count(EntityKind::Species),
            StableIdKind::Culture => self.entity_count(EntityKind::Culture),
            StableIdKind::Settlement => self.entity_count(EntityKind::Settlement),
            StableIdKind::Polity => self.entity_count(EntityKind::Polity),
        }
    }

    fn entity_count(&self, kind: EntityKind) -> Result<usize, FieldDataError> {
        self.entities
            .get(&kind)
            .copied()
            .ok_or(FieldDataError::MissingEntityCount(kind))
    }
}

/// A validated, deterministic collection of extension-field payloads.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExtensionFieldSet {
    fields: BTreeMap<FieldId, FieldData>,
}

impl ExtensionFieldSet {
    /// Creates an empty extension-field set.
    pub const fn new() -> Self {
        Self {
            fields: BTreeMap::new(),
        }
    }

    /// Validates and inserts one payload against an immutable registry and domain sizes.
    pub fn insert(
        &mut self,
        registry: &FieldRegistry,
        id: FieldId,
        data: FieldData,
        sizes: &DomainSizes,
    ) -> Result<(), FieldDataError> {
        let schema = registry
            .get(&id)
            .ok_or_else(|| FieldDataError::UnknownSchema(id.clone()))?;

        if !payload_type_matches(schema.value_type, &data) {
            return Err(FieldDataError::TypeMismatch(id));
        }

        if let (
            FieldValueType::StableIdU32(expected_target),
            FieldData::StableIdU32 { target, values },
        ) = (schema.value_type, &data)
        {
            if expected_target != *target {
                return Err(FieldDataError::StableIdTargetMismatch { field: id });
            }
            let target_count = sizes.stable_id_count(*target)?;
            for value in values {
                if usize::try_from(*value).map_or(true, |value| value >= target_count) {
                    return Err(FieldDataError::StableIdOutOfRange {
                        field: id,
                        value: *value,
                        target_count,
                    });
                }
            }
        }

        let expected = sizes.domain_count(schema.domain)?;
        let actual = data.len();
        if actual != expected {
            return Err(FieldDataError::LengthMismatch {
                field: id,
                expected,
                actual,
            });
        }

        validate_values(&id, &data, schema.valid_range, &schema.category_labels)?;

        if self.fields.contains_key(&id) {
            return Err(FieldDataError::DuplicateField(id));
        }
        self.fields.insert(id, data);
        Ok(())
    }

    /// Returns the validated payload for a field identifier.
    pub fn get(&self, id: &FieldId) -> Option<&FieldData> {
        self.fields.get(id)
    }

    /// Iterates through payloads in stable field-identifier order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&FieldId, &FieldData)> {
        self.fields.iter()
    }

    /// Returns the number of stored field payloads.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Returns whether the set contains no field payloads.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

#[derive(Serialize)]
struct FieldEntry<'a> {
    id: &'a FieldId,
    data: &'a FieldData,
}

impl Serialize for ExtensionFieldSet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.fields.len()))?;
        for (id, data) in &self.fields {
            sequence.serialize_element(&FieldEntry { id, data })?;
        }
        sequence.end()
    }
}

fn payload_type_matches(expected: FieldValueType, data: &FieldData) -> bool {
    matches!(
        (expected, data),
        (FieldValueType::ScalarF32, FieldData::ScalarF32(_))
            | (FieldValueType::CategoryU32, FieldData::CategoryU32(_))
            | (FieldValueType::Boolean, FieldData::Boolean(_))
            | (FieldValueType::Vector2F32, FieldData::Vector2F32(_))
            | (
                FieldValueType::StableIdU32(_),
                FieldData::StableIdU32 { .. }
            )
    )
}

fn validate_values(
    id: &FieldId,
    data: &FieldData,
    range: Option<ValueRange>,
    category_labels: &BTreeMap<u32, String>,
) -> Result<(), FieldDataError> {
    match data {
        FieldData::ScalarF32(values) => {
            for value in values {
                if !value.is_finite() {
                    return Err(FieldDataError::NonFinite(id.clone()));
                }
                if range.is_some_and(|range| !range.contains(*value)) {
                    return Err(FieldDataError::ScalarOutOfRange {
                        field: id.clone(),
                        value: *value,
                    });
                }
            }
        }
        FieldData::CategoryU32(values) => {
            for value in values {
                if !category_labels.contains_key(value) {
                    return Err(FieldDataError::UnknownCategory {
                        field: id.clone(),
                        value: *value,
                    });
                }
            }
        }
        FieldData::Vector2F32(values) => {
            if values
                .iter()
                .flatten()
                .any(|component| !component.is_finite())
            {
                return Err(FieldDataError::NonFinite(id.clone()));
            }
        }
        FieldData::Boolean(_) | FieldData::StableIdU32 { .. } => {}
    }
    Ok(())
}
