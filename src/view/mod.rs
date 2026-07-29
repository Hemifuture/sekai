//! Renderer-neutral, read-only world presentation contracts.

mod field;

pub use field::{
    CellFillKind, FieldCatalog, FieldCatalogEntry, FieldValue, FieldView, FieldViewError,
};
