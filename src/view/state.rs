use crate::world::fields::{FieldId, FieldPaletteHint};
use crate::world::CellId;

use super::{DiagnosticScope, DisplayRangeMode, FieldCatalog, FieldValue, FieldView, PaletteId};

/// UI-independent field display preferences and selection.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDisplayState {
    selected_field: Option<FieldId>,
    inspected_field: Option<FieldId>,
    range_mode: DisplayRangeMode,
    palette_override: Option<PaletteId>,
    diagnostics_enabled: bool,
    diagnostic_scope: DiagnosticScope,
    selected_cell: Option<CellId>,
    selection_dirty: bool,
}

impl Default for FieldDisplayState {
    fn default() -> Self {
        Self {
            selected_field: None,
            inspected_field: None,
            range_mode: DisplayRangeMode::Data,
            palette_override: None,
            diagnostics_enabled: true,
            diagnostic_scope: DiagnosticScope::SelectedField,
            selected_cell: None,
            selection_dirty: true,
        }
    }
}

impl FieldDisplayState {
    /// Selects a field and marks schema-derived preferences for reconciliation.
    pub fn select_field(&mut self, field: FieldId) {
        if self.selected_field.as_ref() != Some(&field) {
            self.selected_field = Some(field.clone());
            self.selection_dirty = true;
        }
        self.inspected_field = Some(field);
    }

    /// Returns the selected field identifier.
    pub fn selected_field(&self) -> Option<&FieldId> {
        self.selected_field.as_ref()
    }

    /// Selects any registered field for metadata and value inspection.
    pub fn inspect_field(&mut self, field: FieldId) {
        self.inspected_field = Some(field);
    }

    /// Returns the field selected for inspection, including unsupported fills.
    pub fn inspected_field(&self) -> Option<&FieldId> {
        self.inspected_field.as_ref()
    }

    /// Sets the active scalar range mode.
    pub fn set_range_mode(&mut self, mode: DisplayRangeMode) {
        self.range_mode = mode;
    }

    /// Returns the active scalar range mode.
    pub const fn range_mode(&self) -> DisplayRangeMode {
        self.range_mode
    }

    /// Sets an optional compatible palette override.
    pub fn set_palette_override(&mut self, palette: Option<PaletteId>) {
        self.palette_override = palette;
    }

    /// Returns the active palette override.
    pub const fn palette_override(&self) -> Option<PaletteId> {
        self.palette_override
    }

    /// Enables or disables diagnostic cell overlays.
    pub fn set_diagnostics_enabled(&mut self, enabled: bool) {
        self.diagnostics_enabled = enabled;
    }

    /// Returns whether diagnostic cell overlays are enabled.
    pub const fn diagnostics_enabled(&self) -> bool {
        self.diagnostics_enabled
    }

    /// Selects which field diagnostics participate in cell overlays.
    pub fn set_diagnostic_scope(&mut self, scope: DiagnosticScope) {
        self.diagnostic_scope = scope;
    }

    /// Returns the active diagnostic scope.
    pub const fn diagnostic_scope(&self) -> DiagnosticScope {
        self.diagnostic_scope
    }

    /// Selects or clears one inspected cell.
    pub fn select_cell(&mut self, cell: Option<CellId>) {
        self.selected_cell = cell;
    }

    /// Returns the selected cell.
    pub const fn selected_cell(&self) -> Option<CellId> {
        self.selected_cell
    }

    /// Reconciles selections against one current catalog and cell cardinality.
    pub fn reconcile(&mut self, catalog: &FieldCatalog<'_>, cell_count: usize) {
        let requested = self
            .selected_field
            .as_ref()
            .and_then(|id| catalog.get(id))
            .filter(|entry| {
                entry
                    .view()
                    .is_some_and(|view| view.cell_fill_kind().is_ok())
            })
            .or_else(|| catalog.first_renderable());
        let chosen = requested.map(|entry| entry.schema());
        let changed =
            self.selection_dirty || self.selected_field.as_ref() != chosen.map(|schema| &schema.id);

        self.selected_field = chosen.map(|schema| schema.id.clone());
        if changed {
            self.range_mode = if chosen.is_some_and(|schema| schema.valid_range.is_some()) {
                DisplayRangeMode::Schema
            } else {
                DisplayRangeMode::Data
            };
            self.selection_dirty = false;
        }

        if let (Some(palette), Some(schema)) = (self.palette_override, chosen) {
            if !palette_matches_hint(palette, schema.display.palette()) {
                self.palette_override = None;
            }
        } else if chosen.is_none() {
            self.palette_override = None;
        }

        self.inspected_field = self
            .inspected_field
            .as_ref()
            .and_then(|id| catalog.get(id))
            .map(|entry| entry.schema().id.clone())
            .or_else(|| self.selected_field.clone())
            .or_else(|| {
                catalog
                    .entries()
                    .first()
                    .map(|entry| entry.schema().id.clone())
            });

        if self
            .selected_cell
            .is_some_and(|cell| cell.raw() as usize >= cell_count)
        {
            self.selected_cell = None;
        }
    }
}

/// One field value formatted through schema precision, units, and labels.
#[derive(Debug, Clone, PartialEq)]
pub struct FormattedFieldValue {
    /// The unformatted typed value.
    pub raw: FieldValue,
    /// Deterministic human-readable value text.
    pub text: String,
    /// Human-readable unit symbol, empty for unitless values.
    pub unit: String,
    /// Optional localization key for a category.
    pub category_label_key: Option<String>,
}

/// Formats one indexed field value without mutating or copying its payload.
pub fn format_field_value(field: &FieldView<'_>, index: usize) -> Option<FormattedFieldValue> {
    let raw = field.value(index)?;
    let precision = usize::from(field.schema().display.decimal_places());
    let (text, category_label_key) = match raw {
        FieldValue::Scalar(value) => (format!("{value:.precision$}"), None),
        FieldValue::Category(key) => (
            key.to_string(),
            field.schema().category_labels.get(&key).cloned(),
        ),
        FieldValue::Boolean(value) => (value.to_string(), None),
        FieldValue::Vector2([x, y]) => (format!("[{x:.precision$}, {y:.precision$}]"), None),
        FieldValue::StableId { value, .. } => (value.to_string(), None),
    };
    Some(FormattedFieldValue {
        raw,
        text,
        unit: field.schema().unit.symbol().to_owned(),
        category_label_key,
    })
}

fn palette_matches_hint(palette: PaletteId, hint: FieldPaletteHint) -> bool {
    matches!(
        (palette, hint),
        (PaletteId::Sequential, FieldPaletteHint::Sequential)
            | (PaletteId::Diverging, FieldPaletteHint::Diverging)
            | (PaletteId::Hypsometric, FieldPaletteHint::Hypsometric)
            | (PaletteId::Categorical, FieldPaletteHint::Categorical)
            | (PaletteId::LandOcean, FieldPaletteHint::LandOcean)
    )
}
