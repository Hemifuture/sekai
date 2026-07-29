use std::sync::Arc;

use thiserror::Error;

use super::{
    DisplayPrepareError, LinearRgba, PreparedCellField, PreparedCellMesh, PreparedDiagnosticMask,
    PreparedFieldKind, ResolvedDisplayRange,
};

/// A non-zero, process-local revision used only for display invalidation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayRevision(u64);

impl DisplayRevision {
    /// Creates a non-zero display revision.
    pub fn new(value: u64) -> Result<Self, DisplayPrepareError> {
        if value == 0 {
            return Err(DisplayPrepareError::ZeroRevision);
        }
        Ok(Self(value))
    }

    /// Returns the raw non-zero revision value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Monotonically issues non-zero process-local display revisions.
#[derive(Debug, Clone)]
pub struct DisplayRevisionClock {
    next: u64,
}

impl Default for DisplayRevisionClock {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl DisplayRevisionClock {
    /// Issues the next revision without wrapping.
    pub fn issue(&mut self) -> Result<DisplayRevision, DisplayPrepareError> {
        let revision = DisplayRevision::new(self.next)?;
        let next = self
            .next
            .checked_add(1)
            .ok_or(DisplayPrepareError::RevisionOverflow)?;
        self.next = next;
        Ok(revision)
    }
}

/// Independent buffer revisions for one complete display packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRevisions {
    /// Static normalized geometry and indices.
    pub mesh: DisplayRevision,
    /// Raw scalar bits or compact category indices.
    pub field: DisplayRevision,
    /// Per-cell diagnostic severities.
    pub diagnostics: DisplayRevision,
    /// Base and diagnostic palette entries.
    pub palette: DisplayRevision,
}

impl DisplayRevisions {
    /// Groups four already validated non-zero revisions.
    pub const fn new(
        mesh: DisplayRevision,
        field: DisplayRevision,
        diagnostics: DisplayRevision,
        palette: DisplayRevision,
    ) -> Self {
        Self {
            mesh,
            field,
            diagnostics,
            palette,
        }
    }
}

/// One immutable, atomically validated packet consumed by the GPU display path.
#[derive(Debug, Clone)]
pub struct PreparedFieldDisplay {
    mesh: Arc<PreparedCellMesh>,
    field: Arc<PreparedCellField>,
    diagnostics: Arc<PreparedDiagnosticMask>,
    palette: Arc<[LinearRgba]>,
    revisions: DisplayRevisions,
    display_range: Option<ResolvedDisplayRange>,
    diagnostics_enabled: bool,
}

impl PreparedFieldDisplay {
    /// Validates cardinality, palette, and field-kind invariants before publication.
    pub fn new(
        mesh: Arc<PreparedCellMesh>,
        field: Arc<PreparedCellField>,
        diagnostics: Arc<PreparedDiagnosticMask>,
        palette: Arc<[LinearRgba]>,
        revisions: DisplayRevisions,
        diagnostics_enabled: bool,
    ) -> Result<Self, DisplayPrepareError> {
        if field.len() != mesh.cell_count() {
            return Err(DisplayPrepareError::CellCountMismatch {
                expected: mesh.cell_count(),
                actual: field.len(),
            });
        }
        if diagnostics.len() != mesh.cell_count() {
            return Err(DisplayPrepareError::CellCountMismatch {
                expected: mesh.cell_count(),
                actual: diagnostics.len(),
            });
        }
        if palette.is_empty() || palette.iter().any(|color| !color.is_valid()) {
            return Err(DisplayPrepareError::InvalidPalette);
        }

        let display_range = match field.kind() {
            PreparedFieldKind::Scalar => Some(field.display_range().ok_or_else(|| {
                DisplayPrepareError::MissingDisplayRange {
                    field: field.field_id().clone(),
                }
            })?),
            PreparedFieldKind::Category => None,
        };
        Ok(Self {
            mesh,
            field,
            diagnostics,
            palette,
            revisions,
            display_range,
            diagnostics_enabled,
        })
    }

    /// Returns the prepared mesh.
    pub fn mesh(&self) -> &PreparedCellMesh {
        &self.mesh
    }

    /// Returns the shared prepared mesh allocation.
    pub fn mesh_arc(&self) -> &Arc<PreparedCellMesh> {
        &self.mesh
    }

    /// Returns the prepared raw field values.
    pub fn field(&self) -> &PreparedCellField {
        &self.field
    }

    /// Returns the shared prepared field allocation.
    pub fn field_arc(&self) -> &Arc<PreparedCellField> {
        &self.field
    }

    /// Returns the prepared diagnostic mask.
    pub fn diagnostics(&self) -> &PreparedDiagnosticMask {
        &self.diagnostics
    }

    /// Returns the shared prepared diagnostic allocation.
    pub fn diagnostics_arc(&self) -> &Arc<PreparedDiagnosticMask> {
        &self.diagnostics
    }

    /// Returns the validated base palette.
    pub fn palette(&self) -> &[LinearRgba] {
        &self.palette
    }

    /// Returns the shared validated palette allocation.
    pub fn palette_arc(&self) -> &Arc<[LinearRgba]> {
        &self.palette
    }

    /// Returns independent buffer revisions.
    pub const fn revisions(&self) -> DisplayRevisions {
        self.revisions
    }

    /// Returns the active scalar display range.
    pub const fn display_range(&self) -> Option<ResolvedDisplayRange> {
        self.display_range
    }

    /// Returns whether diagnostic overlays are enabled.
    pub const fn diagnostics_enabled(&self) -> bool {
        self.diagnostics_enabled
    }

    /// Returns a shallow packet clone with a scalar uniform-range change.
    pub fn with_display_range(&self, range: ResolvedDisplayRange) -> Self {
        let mut next = self.clone();
        if self.field.kind() == PreparedFieldKind::Scalar {
            next.display_range = Some(range);
        }
        next
    }

    /// Returns a shallow packet clone with only its diagnostic uniform flag changed.
    pub fn with_diagnostics_enabled(&self, enabled: bool) -> Self {
        let mut next = self.clone();
        next.diagnostics_enabled = enabled;
        next
    }
}

/// A structured preparation or renderer-status error retained beside the last packet.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DisplayStatusError {
    /// Renderer-neutral preparation failed.
    #[error(transparent)]
    Prepare(#[from] DisplayPrepareError),
    /// Runtime rendering failed after a packet was prepared.
    #[error("{code}: {message}")]
    Runtime {
        /// Lowercase machine-readable status code.
        code: String,
        /// Human-readable status detail.
        message: String,
    },
}

/// The last complete display packet plus a non-destructive status error.
#[derive(Debug, Clone, Default)]
pub struct FieldDisplayResourceState {
    current: Option<Arc<PreparedFieldDisplay>>,
    error: Option<DisplayStatusError>,
}

impl FieldDisplayResourceState {
    /// Creates state containing one complete initial packet.
    pub fn new(current: Arc<PreparedFieldDisplay>) -> Self {
        Self {
            current: Some(current),
            error: None,
        }
    }

    /// Creates an explicitly empty display state.
    pub const fn empty() -> Self {
        Self {
            current: None,
            error: None,
        }
    }

    /// Borrows the last complete packet.
    pub fn current(&self) -> Option<&Arc<PreparedFieldDisplay>> {
        self.current.as_ref()
    }

    /// Clones the last complete packet handle for lock-independent use.
    pub fn current_cloned(&self) -> Option<Arc<PreparedFieldDisplay>> {
        self.current.clone()
    }

    /// Borrows the current status error.
    pub const fn error(&self) -> Option<&DisplayStatusError> {
        self.error.as_ref()
    }

    /// Atomically publishes a complete packet and clears obsolete status.
    pub fn replace(&mut self, packet: Arc<PreparedFieldDisplay>) {
        self.current = Some(packet);
        self.error = None;
    }

    /// Retains the last complete packet and records a preparation failure.
    pub fn reject_prepare(&mut self, error: DisplayPrepareError) {
        self.error = Some(DisplayStatusError::Prepare(error));
    }

    /// Retains the last complete packet and records a validated runtime failure.
    pub fn reject_runtime(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), DisplayPrepareError> {
        let code = code.into();
        if !valid_status_code(&code) {
            return Err(DisplayPrepareError::InvalidStatusCode);
        }
        self.error = Some(DisplayStatusError::Runtime {
            code,
            message: message.into(),
        });
        Ok(())
    }
}

fn valid_status_code(code: &str) -> bool {
    let bytes = code.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
        && bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}
