//! Renderer-neutral field selection state for spherical presentation.

use thiserror::Error;

use super::{DiagnosticScope, DisplayRangeMode, PaletteId};
use crate::world::fields::{FieldDomain, FieldId, FieldValueType};
use crate::world::{CellId, EdgeId};

/// The presentation channel compatible with a spherical field schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SphericalFieldChannel {
    /// A scalar or categorical cell fill.
    CellFill,
    /// A scalar or categorical edge annotation.
    EdgeOverlay,
    /// A two-dimensional vector glyph at each cell.
    VectorOverlay,
}

/// A stable spherical entity selected by either presentation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedSurfaceEntity {
    /// One authoritative surface cell.
    Cell(CellId),
    /// One authoritative surface edge.
    Edge(EdgeId),
}

/// The density of prepared vector glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VectorGlyphLod {
    /// Sparse glyph subset.
    Low,
    /// Balanced glyph subset.
    #[default]
    Medium,
    /// Dense glyph subset.
    High,
}

/// Errors from spherical field-layer display controls.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum FieldLayerError {
    /// The vector animation display speed is non-finite or outside its supported range.
    #[error("vector display speed must be finite and within 0.0..=4.0, got {0}")]
    InvalidVectorDisplaySpeed(f32),
}

/// UI-independent selection and preferences for spherical field presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct SphericalFieldDisplayState {
    fill_field: Option<FieldId>,
    overlay_field: Option<FieldId>,
    range_mode: DisplayRangeMode,
    palette_override: Option<PaletteId>,
    diagnostics_enabled: bool,
    diagnostic_scope: DiagnosticScope,
    selected_entity: Option<SelectedSurfaceEntity>,
    vector_lod: VectorGlyphLod,
    vector_paused: bool,
    vector_display_speed: f32,
}

impl Default for SphericalFieldDisplayState {
    fn default() -> Self {
        Self {
            fill_field: None,
            overlay_field: None,
            range_mode: DisplayRangeMode::Data,
            palette_override: None,
            diagnostics_enabled: true,
            diagnostic_scope: DiagnosticScope::SelectedField,
            selected_entity: None,
            vector_lod: VectorGlyphLod::default(),
            vector_paused: false,
            vector_display_speed: 1.0,
        }
    }
}

impl SphericalFieldDisplayState {
    /// Selects the field used for the single cell-fill channel.
    pub fn select_fill(&mut self, field: FieldId) {
        self.fill_field = Some(field);
    }

    /// Returns the field selected for the cell-fill channel.
    pub fn fill_field(&self) -> Option<&FieldId> {
        self.fill_field.as_ref()
    }

    /// Selects or clears the single overlay field.
    pub fn select_overlay(&mut self, field: Option<FieldId>) {
        self.overlay_field = field;
    }

    /// Returns the field selected for the overlay channel.
    pub fn overlay_field(&self) -> Option<&FieldId> {
        self.overlay_field.as_ref()
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

    /// Enables or disables diagnostic overlays.
    pub fn set_diagnostics_enabled(&mut self, enabled: bool) {
        self.diagnostics_enabled = enabled;
    }

    /// Returns whether diagnostic overlays are enabled.
    pub const fn diagnostics_enabled(&self) -> bool {
        self.diagnostics_enabled
    }

    /// Selects which field diagnostics participate in overlays.
    pub fn set_diagnostic_scope(&mut self, scope: DiagnosticScope) {
        self.diagnostic_scope = scope;
    }

    /// Returns the active diagnostic scope.
    pub const fn diagnostic_scope(&self) -> DiagnosticScope {
        self.diagnostic_scope
    }

    /// Selects or clears one stable surface entity.
    pub fn select_entity(&mut self, entity: Option<SelectedSurfaceEntity>) {
        self.selected_entity = entity;
    }

    /// Returns the selected stable surface entity.
    pub const fn selected_entity(&self) -> Option<SelectedSurfaceEntity> {
        self.selected_entity
    }

    /// Selects the vector-glyph level of detail.
    pub fn set_vector_lod(&mut self, lod: VectorGlyphLod) {
        self.vector_lod = lod;
    }

    /// Returns the vector-glyph level of detail.
    pub const fn vector_lod(&self) -> VectorGlyphLod {
        self.vector_lod
    }

    /// Pauses or resumes display-only vector animation.
    pub fn set_vector_paused(&mut self, paused: bool) {
        self.vector_paused = paused;
    }

    /// Returns whether display-only vector animation is paused.
    pub const fn vector_paused(&self) -> bool {
        self.vector_paused
    }

    /// Sets the display-only vector animation speed.
    pub fn set_vector_display_speed(&mut self, speed: f32) -> Result<(), FieldLayerError> {
        if !speed.is_finite() || !(0.0..=4.0).contains(&speed) {
            return Err(FieldLayerError::InvalidVectorDisplaySpeed(speed));
        }
        self.vector_display_speed = speed;
        Ok(())
    }

    /// Returns the display-only vector animation speed.
    pub const fn vector_display_speed(&self) -> f32 {
        self.vector_display_speed
    }
}

/// Classifies an exact field schema domain/type pair for spherical presentation.
pub fn classify_spherical_channel(
    domain: FieldDomain,
    value_type: FieldValueType,
) -> Option<SphericalFieldChannel> {
    match (domain, value_type) {
        (FieldDomain::Cells, FieldValueType::ScalarF32 | FieldValueType::CategoryU32) => {
            Some(SphericalFieldChannel::CellFill)
        }
        (FieldDomain::Edges, FieldValueType::ScalarF32 | FieldValueType::CategoryU32) => {
            Some(SphericalFieldChannel::EdgeOverlay)
        }
        (FieldDomain::Cells, FieldValueType::Vector2F32) => {
            Some(SphericalFieldChannel::VectorOverlay)
        }
        _ => None,
    }
}
