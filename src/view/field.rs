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
    data: &'a FieldData,
}

impl<'a> FieldView<'a> {
    /// Pairs a schema with a payload after checking their exact representations.
    pub fn new(schema: &'a FieldSchema, data: &'a FieldData) -> Result<Self, FieldViewError> {
        if !data_matches(schema.value_type, data) {
            return Err(FieldViewError::TypeMismatch {
                field: schema.id.clone(),
                expected: schema.value_type,
            });
        }
        Ok(Self { schema, data })
    }

    /// Returns the borrowed schema.
    pub const fn schema(&self) -> &'a FieldSchema {
        self.schema
    }

    /// Returns the number of values in the payload.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns whether the payload contains no values.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Reads one value without copying the payload collection.
    pub fn value(&self, index: usize) -> Option<FieldValue> {
        match self.data {
            FieldData::ScalarF32(values) => values.get(index).copied().map(FieldValue::Scalar),
            FieldData::CategoryU32(values) => values.get(index).copied().map(FieldValue::Category),
            FieldData::Boolean(values) => values.get(index).copied().map(FieldValue::Boolean),
            FieldData::Vector2F32(values) => values.get(index).copied().map(FieldValue::Vector2),
            FieldData::StableIdU32 { target, values } => {
                values
                    .get(index)
                    .copied()
                    .map(|value| FieldValue::StableId {
                        target: *target,
                        value,
                    })
            }
        }
    }

    /// Borrows scalar values when this is a scalar field.
    pub fn scalar_values(&self) -> Option<&'a [f32]> {
        match self.data {
            FieldData::ScalarF32(values) => Some(values),
            _ => None,
        }
    }

    /// Borrows category keys when this is a category field.
    pub fn category_values(&self) -> Option<&'a [u32]> {
        match self.data {
            FieldData::CategoryU32(values) => Some(values),
            _ => None,
        }
    }

    /// Borrows vector values when this is a vector field.
    pub fn vector_values(&self) -> Option<&'a [[f32; 2]]> {
        match self.data {
            FieldData::Vector2F32(values) => Some(values),
            _ => None,
        }
    }

    /// Borrows stable identifiers and returns their exact target kind.
    pub fn stable_id_values(&self) -> Option<(StableIdKind, &'a [u32])> {
        match self.data {
            FieldData::StableIdU32 { target, values } => Some((*target, values)),
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

fn data_matches(expected: FieldValueType, data: &FieldData) -> bool {
    match (expected, data) {
        (FieldValueType::ScalarF32, FieldData::ScalarF32(_))
        | (FieldValueType::CategoryU32, FieldData::CategoryU32(_))
        | (FieldValueType::Boolean, FieldData::Boolean(_))
        | (FieldValueType::Vector2F32, FieldData::Vector2F32(_)) => true,
        (FieldValueType::StableIdU32(expected), FieldData::StableIdU32 { target, .. }) => {
            expected == *target
        }
        _ => false,
    }
}
