//! Renderer-neutral, read-only world presentation contracts.

mod diagnostics;
mod field;
mod palette;
mod state;

pub use diagnostics::{
    CellDiagnosticRef, DiagnosticScope, OwnedViewDiagnostic, PreparedDiagnosticMask,
    ViewDiagnosticSeverity,
};
pub use field::{
    CellFillKind, FieldCatalog, FieldCatalogEntry, FieldValue, FieldView, FieldViewError,
};
pub use palette::{
    built_in_palette, category_color, prepare_cell_field, resolve_display_range, sample_palette,
    scalar_color, DisplayPrepareError, DisplayRangeMode, LinearRgba, PaletteId, PreparedCellField,
    PreparedFieldKind, ResolvedDisplayRange, DIAGNOSTIC_ERROR_COLOR, DIAGNOSTIC_INFO_COLOR,
    DIAGNOSTIC_WARNING_COLOR,
};
pub use state::{format_field_value, FieldDisplayState, FormattedFieldValue};
