//! Stable extension-field schemas, registries, and validated payloads.

mod data;
mod schema;

pub use data::{DomainSizes, ExtensionFieldSet, FieldData, FieldDataError};
pub use schema::{
    EntityKind, FieldDisplayMetadata, FieldDomain, FieldId, FieldPaletteHint, FieldRegistry,
    FieldRegistryBuilder, FieldSchema, FieldSchemaError, FieldUnit, FieldValueType,
    MissingValuePolicy, StableIdKind, ValueRange,
};
