use crate::world::fields::FieldId;
use crate::world::CellId;

use super::DisplayPrepareError;

/// Renderer-neutral diagnostic severity ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ViewDiagnosticSeverity {
    /// Informational context.
    Info,
    /// A suspicious but displayable result.
    Warning,
    /// An invalid result requiring attention.
    Error,
}

impl ViewDiagnosticSeverity {
    const fn mask_value(self) -> u32 {
        match self {
            Self::Info => 1,
            Self::Warning => 2,
            Self::Error => 3,
        }
    }
}

/// A borrowed diagnostic suitable for presentation without engine coupling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellDiagnosticRef<'a> {
    /// Severity used for ordering and cell overlays.
    pub severity: ViewDiagnosticSeverity,
    /// Stable machine-readable diagnostic code.
    pub code: &'a str,
    /// Optional field associated with the diagnostic.
    pub field_id: Option<&'a FieldId>,
    /// Optional cell associated with the diagnostic.
    pub cell_id: Option<CellId>,
    /// Human-readable diagnostic message.
    pub message: &'a str,
}

/// An owned diagnostic retained by an application adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedViewDiagnostic {
    /// Severity used for ordering and cell overlays.
    pub severity: ViewDiagnosticSeverity,
    /// Stable machine-readable diagnostic code.
    pub code: String,
    /// Optional field associated with the diagnostic.
    pub field_id: Option<FieldId>,
    /// Optional cell associated with the diagnostic.
    pub cell_id: Option<CellId>,
    /// Human-readable diagnostic message.
    pub message: String,
}

impl OwnedViewDiagnostic {
    /// Borrows this owned diagnostic without allocating.
    pub fn as_ref(&self) -> CellDiagnosticRef<'_> {
        CellDiagnosticRef {
            severity: self.severity,
            code: &self.code,
            field_id: self.field_id.as_ref(),
            cell_id: self.cell_id,
            message: &self.message,
        }
    }
}

/// Which field-associated diagnostics participate in the cell overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DiagnosticScope {
    /// Global diagnostics and diagnostics for the selected field.
    SelectedField,
    /// Diagnostics for every field.
    AllFields,
}

/// Highest diagnostic severity for every stable cell index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDiagnosticMask {
    cells: Vec<u32>,
}

impl PreparedDiagnosticMask {
    /// Builds a complete cell mask, validating every referenced cell first.
    pub fn build<'a>(
        cell_count: usize,
        diagnostics: impl IntoIterator<Item = CellDiagnosticRef<'a>>,
        selected_field: Option<&FieldId>,
        scope: DiagnosticScope,
    ) -> Result<Self, DisplayPrepareError> {
        let diagnostics: Vec<_> = diagnostics.into_iter().collect();
        for diagnostic in &diagnostics {
            if let Some(cell) = diagnostic.cell_id {
                let index = usize::try_from(cell.raw()).map_err(|_| {
                    DisplayPrepareError::DiagnosticCellOutOfRange { cell, cell_count }
                })?;
                if index >= cell_count {
                    return Err(DisplayPrepareError::DiagnosticCellOutOfRange { cell, cell_count });
                }
            }
        }

        let mut cells = vec![0; cell_count];
        for diagnostic in diagnostics {
            let included = match scope {
                DiagnosticScope::AllFields => true,
                DiagnosticScope::SelectedField => diagnostic
                    .field_id
                    .is_none_or(|field| Some(field) == selected_field),
            };
            if !included {
                continue;
            }
            let Some(cell) = diagnostic.cell_id else {
                continue;
            };
            let index = cell.raw() as usize;
            cells[index] = cells[index].max(diagnostic.severity.mask_value());
        }
        Ok(Self { cells })
    }

    /// Creates an all-clear mask for a known cell count.
    pub fn empty(cell_count: usize) -> Self {
        Self {
            cells: vec![0; cell_count],
        }
    }

    /// Returns numeric severities in stable cell order.
    pub fn cells(&self) -> &[u32] {
        &self.cells
    }

    /// Returns the number of cells represented by the mask.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Returns whether the mask represents no cells.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Returns owned heap bytes using the diagnostic vector capacity.
    pub fn resident_bytes(&self) -> Result<usize, super::ResidentBytesError> {
        super::resident::capacity_bytes::<u32>(self.cells.capacity(), "prepared diagnostic mask")
    }
}
