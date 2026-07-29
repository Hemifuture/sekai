use std::collections::BTreeMap;

use thiserror::Error;

use crate::world::fields::{
    ExtensionFieldSet, FieldData, FieldDomain, FieldId, FieldRegistry, FieldSchema, FieldValueType,
    StableIdKind,
};

/// One renderer-neutral field value borrowed by index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldValue {
    /// A finite scalar value.
    Scalar(f32),
    /// A declared category key.
    Category(u32),
    /// A boolean value.
    Boolean(bool),
    /// A finite two-component vector.
    Vector2([f32; 2]),
    /// A stable identifier and its target kind.
    StableId {
        /// The referenced object kind.
        target: StableIdKind,
        /// The raw stable identifier.
        value: u32,
    },
}

/// A renderer-neutral reference to one controlled field payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldPayloadRef<'a> {
    /// Borrowed finite scalar values.
    ScalarF32(&'a [f32]),
    /// Borrowed unsigned category keys.
    CategoryU32(&'a [u32]),
    /// Borrowed bit-packed boolean values.
    Boolean(&'a Vec<bool>),
    /// Borrowed finite two-component vectors.
    Vector2F32(&'a [[f32; 2]]),
    /// Borrowed stable identifiers with their exact target kind.
    StableIdU32 {
        /// The kind referenced by every raw identifier.
        target: StableIdKind,
        /// Raw stable identifiers in domain order.
        values: &'a [u32],
    },
}

impl FieldPayloadRef<'_> {
    fn len(self) -> usize {
        match self {
            Self::ScalarF32(values) => values.len(),
            Self::CategoryU32(values) => values.len(),
            Self::Boolean(values) => values.len(),
            Self::Vector2F32(values) => values.len(),
            Self::StableIdU32 { values, .. } => values.len(),
        }
    }

    fn is_empty(self) -> bool {
        self.len() == 0
    }
}

impl<'a> From<&'a FieldData> for FieldPayloadRef<'a> {
    fn from(data: &'a FieldData) -> Self {
        match data {
            FieldData::ScalarF32(values) => Self::ScalarF32(values),
            FieldData::CategoryU32(values) => Self::CategoryU32(values),
            FieldData::Boolean(values) => Self::Boolean(values),
            FieldData::Vector2F32(values) => Self::Vector2F32(values),
            FieldData::StableIdU32 { target, values } => Self::StableIdU32 {
                target: *target,
                values,
            },
        }
    }
}

/// Field representations supported by the V1 cell-fill renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellFillKind {
    /// Continuous scalar fill.
    Scalar,
    /// Discrete category fill.
    Category,
}

/// Errors returned while constructing or using borrowed field views.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum FieldViewError {
    /// A validated schema was paired with an incompatible payload.
    #[error("field {field:?} payload type does not match {expected:?}")]
    TypeMismatch {
        /// The field whose payload representation was incompatible.
        field: FieldId,
        /// The payload representation required by the schema.
        expected: FieldValueType,
    },
    /// A borrowed payload was supplied for an unregistered field.
    #[error("borrowed payload field {field:?} is not registered")]
    UnknownPayload {
        /// The unregistered payload identifier.
        field: FieldId,
    },
    /// A borrowed payload identifier appeared more than once.
    #[error("borrowed payload field {field:?} was supplied more than once")]
    DuplicatePayload {
        /// The duplicated payload identifier.
        field: FieldId,
    },
    /// The field cannot drive the V1 cell-fill renderer.
    #[error(
        "field {field:?} with domain {domain:?} and type {value_type:?} cannot fill cells in display V1"
    )]
    UnsupportedCellFill {
        /// The unsupported field.
        field: FieldId,
        /// The field's domain.
        domain: FieldDomain,
        /// The field's payload representation.
        value_type: FieldValueType,
    },
}

/// A borrowed, validated pairing of one field schema and payload.
#[derive(Debug, Clone, Copy)]
pub struct FieldView<'a> {
    schema: &'a FieldSchema,
    payload: FieldPayloadRef<'a>,
}

impl<'a> FieldView<'a> {
    /// Pairs a schema with a payload after checking their exact representations.
    pub fn new(schema: &'a FieldSchema, data: &'a FieldData) -> Result<Self, FieldViewError> {
        Self::from_payload(schema, data.into())
    }

    /// Pairs a schema with a borrowed core or extension payload.
    pub fn from_payload(
        schema: &'a FieldSchema,
        payload: FieldPayloadRef<'a>,
    ) -> Result<Self, FieldViewError> {
        if !payload_matches(schema.value_type, payload) {
            return Err(FieldViewError::TypeMismatch {
                field: schema.id.clone(),
                expected: schema.value_type,
            });
        }
        Ok(Self { schema, payload })
    }

    /// Returns the borrowed schema.
    pub const fn schema(&self) -> &'a FieldSchema {
        self.schema
    }

    /// Returns the number of values in the payload.
    pub fn len(&self) -> usize {
        self.payload.len()
    }

    /// Returns whether the payload contains no values.
    pub fn is_empty(&self) -> bool {
        self.payload.is_empty()
    }

    /// Reads one value without copying the payload collection.
    pub fn value(&self, index: usize) -> Option<FieldValue> {
        match self.payload {
            FieldPayloadRef::ScalarF32(values) => {
                values.get(index).copied().map(FieldValue::Scalar)
            }
            FieldPayloadRef::CategoryU32(values) => {
                values.get(index).copied().map(FieldValue::Category)
            }
            FieldPayloadRef::Boolean(values) => values.get(index).copied().map(FieldValue::Boolean),
            FieldPayloadRef::Vector2F32(values) => {
                values.get(index).copied().map(FieldValue::Vector2)
            }
            FieldPayloadRef::StableIdU32 { target, values } => values
                .get(index)
                .copied()
                .map(|value| FieldValue::StableId { target, value }),
        }
    }

    /// Borrows scalar values when this is a scalar field.
    pub fn scalar_values(&self) -> Option<&'a [f32]> {
        match self.payload {
            FieldPayloadRef::ScalarF32(values) => Some(values),
            _ => None,
        }
    }

    /// Borrows category keys when this is a category field.
    pub fn category_values(&self) -> Option<&'a [u32]> {
        match self.payload {
            FieldPayloadRef::CategoryU32(values) => Some(values),
            _ => None,
        }
    }

    /// Borrows vector values when this is a vector field.
    pub fn vector_values(&self) -> Option<&'a [[f32; 2]]> {
        match self.payload {
            FieldPayloadRef::Vector2F32(values) => Some(values),
            _ => None,
        }
    }

    /// Borrows stable identifiers and returns their exact target kind.
    pub fn stable_id_values(&self) -> Option<(StableIdKind, &'a [u32])> {
        match self.payload {
            FieldPayloadRef::StableIdU32 { target, values } => Some((target, values)),
            _ => None,
        }
    }

    /// Returns the V1 cell-fill representation or an explicit unsupported error.
    pub fn cell_fill_kind(&self) -> Result<CellFillKind, FieldViewError> {
        match (self.schema.domain, self.schema.value_type) {
            (FieldDomain::Cells, FieldValueType::ScalarF32) => Ok(CellFillKind::Scalar),
            (FieldDomain::Cells, FieldValueType::CategoryU32) => Ok(CellFillKind::Category),
            (domain, value_type) => Err(FieldViewError::UnsupportedCellFill {
                field: self.schema.id.clone(),
                domain,
                value_type,
            }),
        }
    }
}

/// One stable catalog entry, including schemas whose payload is absent.
#[derive(Debug, Clone)]
pub struct FieldCatalogEntry<'a> {
    schema: &'a FieldSchema,
    view: Option<FieldView<'a>>,
}

impl<'a> FieldCatalogEntry<'a> {
    /// Returns the registered field schema.
    pub const fn schema(&self) -> &'a FieldSchema {
        self.schema
    }

    /// Returns the field view when the current payload contains this field.
    pub const fn view(&self) -> Option<&FieldView<'a>> {
        self.view.as_ref()
    }
}

/// A stable, borrowed catalog of all registered extension fields.
#[derive(Debug, Clone)]
pub struct FieldCatalog<'a> {
    entries: Vec<FieldCatalogEntry<'a>>,
}

impl<'a> FieldCatalog<'a> {
    /// Builds a catalog in registry order while preserving missing payloads.
    pub fn from_extension_fields(
        registry: &'a FieldRegistry,
        fields: &'a ExtensionFieldSet,
    ) -> Result<Self, FieldViewError> {
        let entries = registry
            .iter()
            .map(|(id, schema)| {
                fields
                    .get(id)
                    .map(|data| FieldView::new(schema, data))
                    .transpose()
                    .map(|view| FieldCatalogEntry { schema, view })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { entries })
    }

    /// Builds a catalog from borrowed core payloads without copying their arrays.
    pub fn from_payloads(
        registry: &'a FieldRegistry,
        payloads: impl IntoIterator<Item = (FieldId, FieldPayloadRef<'a>)>,
    ) -> Result<Self, FieldViewError> {
        let mut payloads_by_id = BTreeMap::new();
        for (field, payload) in payloads {
            if registry.get(&field).is_none() {
                return Err(FieldViewError::UnknownPayload { field });
            }
            if payloads_by_id.insert(field.clone(), payload).is_some() {
                return Err(FieldViewError::DuplicatePayload { field });
            }
        }

        let entries = registry
            .iter()
            .map(|(id, schema)| {
                payloads_by_id
                    .get(id)
                    .copied()
                    .map(|payload| FieldView::from_payload(schema, payload))
                    .transpose()
                    .map(|view| FieldCatalogEntry { schema, view })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { entries })
    }

    /// Returns entries in stable field-identifier order.
    pub fn entries(&self) -> &[FieldCatalogEntry<'a>] {
        &self.entries
    }

    /// Finds an entry by its exact stable field identifier.
    pub fn get(&self, id: &FieldId) -> Option<&FieldCatalogEntry<'a>> {
        self.entries
            .binary_search_by(|entry| entry.schema.id.cmp(id))
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Returns the first available field supported by the V1 cell-fill renderer.
    pub fn first_renderable(&self) -> Option<&FieldCatalogEntry<'a>> {
        self.entries.iter().find(|entry| {
            entry
                .view()
                .is_some_and(|view| view.cell_fill_kind().is_ok())
        })
    }
}

fn payload_matches(expected: FieldValueType, payload: FieldPayloadRef<'_>) -> bool {
    match (expected, payload) {
        (FieldValueType::ScalarF32, FieldPayloadRef::ScalarF32(_))
        | (FieldValueType::CategoryU32, FieldPayloadRef::CategoryU32(_))
        | (FieldValueType::Boolean, FieldPayloadRef::Boolean(_))
        | (FieldValueType::Vector2F32, FieldPayloadRef::Vector2F32(_)) => true,
        (FieldValueType::StableIdU32(expected), FieldPayloadRef::StableIdU32 { target, .. }) => {
            expected == target
        }
        _ => false,
    }
}
