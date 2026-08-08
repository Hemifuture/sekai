//! Renderer-neutral field selection state and prepared data for spherical presentation.

use std::sync::Arc;

#[cfg(test)]
use std::cell::Cell;

use thiserror::Error;

use super::palette::prepare_scalar_or_category_field;
use super::{
    built_in_palette, DiagnosticScope, DisplayPrepareError, DisplayRangeMode, DisplayRevision,
    DisplayRevisionClock, FieldCatalog, FieldView, LinearRgba, OwnedViewDiagnostic, PaletteId,
    PreparedCellField, PreparedDiagnosticMask, PreparedFieldKind, ResolvedDisplayRange,
    SphericalPresentationSource,
};
use crate::world::fields::{FieldDomain, FieldId, FieldPaletteHint, FieldValueType};
use crate::world::{CellId, EdgeId};

#[cfg(test)]
thread_local! {
    static PREPARATION_COUNTS: Cell<FieldLayerPreparationCounts> = Cell::new(FieldLayerPreparationCounts::default());
}

/// Test-only counts for expensive prepared payload construction.
#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FieldLayerPreparationCounts {
    pub(crate) fill: usize,
    pub(crate) overlay: usize,
    pub(crate) diagnostics: usize,
}

#[cfg(test)]
pub(crate) fn reset_field_layer_preparation_counts() {
    PREPARATION_COUNTS.with(|counts| counts.set(FieldLayerPreparationCounts::default()));
}

#[cfg(test)]
pub(crate) fn field_layer_preparation_counts() -> FieldLayerPreparationCounts {
    PREPARATION_COUNTS.with(Cell::get)
}

#[cfg(test)]
fn record_preparation(update: impl FnOnce(&mut FieldLayerPreparationCounts)) {
    PREPARATION_COUNTS.with(|counts| {
        let mut next = counts.get();
        update(&mut next);
        counts.set(next);
    });
}

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

/// The renderer-neutral kind of an optional spherical overlay packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedOverlayKind {
    /// An edge scalar field with a display range.
    EdgeScalar,
    /// An edge category field with compact category keys.
    EdgeCategory,
    /// A per-cell two-dimensional vector field.
    CellVector,
}

/// A prepared edge scalar or category field, kept distinct from cell fills.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEdgeField {
    field_id: FieldId,
    kind: PreparedFieldKind,
    raw_values: Vec<u32>,
    display_range: Option<ResolvedDisplayRange>,
    category_keys: Vec<u32>,
}

impl PreparedEdgeField {
    /// Returns the stable source field identifier.
    pub fn field_id(&self) -> &FieldId {
        &self.field_id
    }

    /// Returns the packed field representation.
    pub const fn kind(&self) -> PreparedFieldKind {
        self.kind
    }

    /// Returns packed values in stable edge order.
    pub fn raw_values(&self) -> &[u32] {
        &self.raw_values
    }

    /// Returns the number of prepared edge values.
    pub fn len(&self) -> usize {
        self.raw_values.len()
    }

    /// Returns whether no edge values were prepared.
    pub fn is_empty(&self) -> bool {
        self.raw_values.is_empty()
    }

    /// Returns the active scalar display range, if this is scalar data.
    pub const fn display_range(&self) -> Option<ResolvedDisplayRange> {
        self.display_range
    }

    /// Returns sorted raw category keys indexed by the packed values.
    pub fn category_keys(&self) -> &[u32] {
        &self.category_keys
    }
}

/// A prepared per-cell local east/north vector field.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedVectorField {
    field_id: FieldId,
    components: Vec<[f32; 2]>,
    magnitudes: Vec<f32>,
    display_range: ResolvedDisplayRange,
}

impl PreparedVectorField {
    /// Returns the stable source field identifier.
    pub fn field_id(&self) -> &FieldId {
        &self.field_id
    }

    /// Returns local east/north components in stable cell order.
    pub fn components(&self) -> &[[f32; 2]] {
        &self.components
    }

    /// Returns the precomputed vector magnitudes in stable cell order.
    pub fn magnitudes(&self) -> &[f32] {
        &self.magnitudes
    }

    /// Returns the active magnitude display range.
    pub const fn display_range(&self) -> ResolvedDisplayRange {
        self.display_range
    }
}

/// The optional non-fill payload for one spherical presentation packet.
#[derive(Debug, Clone)]
pub enum PreparedSphericalOverlay {
    /// One prepared edge scalar or category field.
    Edge(Arc<PreparedEdgeField>),
    /// One prepared per-cell vector field.
    Vector(Arc<PreparedVectorField>),
}

impl PreparedSphericalOverlay {
    fn kind(&self) -> PreparedOverlayKind {
        match self {
            Self::Edge(field) => match field.kind() {
                PreparedFieldKind::Scalar => PreparedOverlayKind::EdgeScalar,
                PreparedFieldKind::Category => PreparedOverlayKind::EdgeCategory,
            },
            Self::Vector(_) => PreparedOverlayKind::CellVector,
        }
    }
}

/// Independent prepared-data revisions for a spherical field-layer packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldLayerRevisions {
    /// Cell fill data and its active scalar range.
    pub fill: DisplayRevision,
    /// Optional edge or vector overlay data and its active scalar range.
    pub overlay: DisplayRevision,
    /// Per-cell diagnostics.
    pub diagnostics: DisplayRevision,
    /// Fill palette entries.
    pub fill_palette: DisplayRevision,
    /// Optional overlay palette entries.
    pub overlay_palette: DisplayRevision,
}

/// One complete geometry-free packet shared by spherical map and globe presenters.
#[derive(Debug, Clone)]
pub struct PreparedFieldLayers {
    source: SphericalPresentationSource,
    fill: Arc<PreparedCellField>,
    overlay: Option<PreparedSphericalOverlay>,
    diagnostics: Arc<PreparedDiagnosticMask>,
    fill_palette: Arc<[LinearRgba]>,
    overlay_palette: Option<Arc<[LinearRgba]>>,
    revisions: FieldLayerRevisions,
    diagnostics_enabled: bool,
    prepared_state: PreparedLayerState,
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedLayerState {
    fill_field: Option<FieldId>,
    overlay_field: Option<FieldId>,
    range_mode: DisplayRangeMode,
    palette_override: Option<PaletteId>,
    diagnostic_scope: DiagnosticScope,
}

impl From<&SphericalFieldDisplayState> for PreparedLayerState {
    fn from(state: &SphericalFieldDisplayState) -> Self {
        Self {
            fill_field: state.fill_field.clone(),
            overlay_field: state.overlay_field.clone(),
            range_mode: state.range_mode,
            palette_override: state.palette_override,
            diagnostic_scope: state.diagnostic_scope,
        }
    }
}

impl PreparedFieldLayers {
    /// Returns the immutable build identity from which all packet data was derived.
    pub const fn source(&self) -> &SphericalPresentationSource {
        &self.source
    }

    /// Returns the selected cell fill.
    pub fn fill(&self) -> &PreparedCellField {
        &self.fill
    }

    /// Returns the shared selected cell fill allocation.
    pub fn fill_arc(&self) -> &Arc<PreparedCellField> {
        &self.fill
    }

    /// Returns the selected edge or vector overlay.
    pub fn overlay(&self) -> Option<&PreparedSphericalOverlay> {
        self.overlay.as_ref()
    }

    /// Returns the semantic kind of the selected overlay.
    pub fn overlay_kind(&self) -> Option<PreparedOverlayKind> {
        self.overlay.as_ref().map(PreparedSphericalOverlay::kind)
    }

    /// Returns the prepared diagnostic mask.
    pub fn diagnostics(&self) -> &PreparedDiagnosticMask {
        &self.diagnostics
    }

    /// Returns the shared diagnostic allocation.
    pub fn diagnostics_arc(&self) -> &Arc<PreparedDiagnosticMask> {
        &self.diagnostics
    }

    /// Returns the fill palette.
    pub fn fill_palette(&self) -> &[LinearRgba] {
        &self.fill_palette
    }

    /// Returns the shared fill palette allocation.
    pub fn fill_palette_arc(&self) -> &Arc<[LinearRgba]> {
        &self.fill_palette
    }

    /// Returns the optional overlay palette.
    pub fn overlay_palette(&self) -> Option<&[LinearRgba]> {
        self.overlay_palette.as_deref()
    }

    /// Returns the shared optional overlay palette allocation.
    pub fn overlay_palette_arc(&self) -> Option<&Arc<[LinearRgba]>> {
        self.overlay_palette.as_ref()
    }

    /// Returns the packet's independent data revisions.
    pub const fn revisions(&self) -> FieldLayerRevisions {
        self.revisions
    }

    /// Returns whether diagnostics are currently drawn.
    pub const fn diagnostics_enabled(&self) -> bool {
        self.diagnostics_enabled
    }
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

/// Prepares an edge scalar or category field without assigning it cell semantics.
pub fn prepare_edge_field(
    field: &FieldView<'_>,
    expected_edges: usize,
    range_mode: DisplayRangeMode,
) -> Result<PreparedEdgeField, DisplayPrepareError> {
    if classify_spherical_channel(field.schema().domain, field.schema().value_type)
        != Some(SphericalFieldChannel::EdgeOverlay)
    {
        return Err(DisplayPrepareError::UnsupportedSphericalChannel {
            field: field.schema().id.clone(),
        });
    }
    if field.len() != expected_edges {
        return Err(DisplayPrepareError::FieldCardinalityMismatch {
            field: field.schema().id.clone(),
            domain: FieldDomain::Edges,
            expected: expected_edges,
            actual: field.len(),
        });
    }
    let packed = prepare_scalar_or_category_field(field, FieldDomain::Edges, range_mode)?;
    Ok(PreparedEdgeField {
        field_id: packed.field_id,
        kind: packed.kind,
        raw_values: packed.raw_values,
        display_range: packed.display_range,
        category_keys: packed.category_keys,
    })
}

/// Prepares a cell vector field and resolves one magnitude range for display.
pub fn prepare_vector_field(
    field: &FieldView<'_>,
    expected_cells: usize,
    range_mode: DisplayRangeMode,
) -> Result<PreparedVectorField, DisplayPrepareError> {
    if field.len() != expected_cells {
        return Err(DisplayPrepareError::FieldCardinalityMismatch {
            field: field.schema().id.clone(),
            domain: FieldDomain::Cells,
            expected: expected_cells,
            actual: field.len(),
        });
    }
    if classify_spherical_channel(field.schema().domain, field.schema().value_type)
        != Some(SphericalFieldChannel::VectorOverlay)
    {
        return Err(DisplayPrepareError::UnsupportedSphericalChannel {
            field: field.schema().id.clone(),
        });
    }
    let components =
        field
            .vector_values()
            .ok_or_else(|| DisplayPrepareError::UnsupportedSphericalChannel {
                field: field.schema().id.clone(),
            })?;
    let mut magnitudes = Vec::with_capacity(components.len());
    for (index, &[east, north]) in components.iter().enumerate() {
        let magnitude = east.hypot(north);
        if !east.is_finite() || !north.is_finite() || !magnitude.is_finite() {
            return Err(DisplayPrepareError::NonFiniteVector {
                field: field.schema().id.clone(),
                index,
            });
        }
        magnitudes.push(magnitude);
    }
    let display_range = match range_mode {
        DisplayRangeMode::Schema => field
            .schema()
            .valid_range
            .map(ResolvedDisplayRange::from)
            .ok_or_else(|| DisplayPrepareError::MissingSchemaRange {
                field: field.schema().id.clone(),
            })?,
        DisplayRangeMode::Data => magnitude_range(&magnitudes).ok_or_else(|| {
            DisplayPrepareError::NoFiniteScalarValues {
                field: field.schema().id.clone(),
            }
        })?,
        DisplayRangeMode::Manual(range) => range.into(),
    };
    Ok(PreparedVectorField {
        field_id: field.schema().id.clone(),
        components: components.to_vec(),
        magnitudes,
        display_range,
    })
}

/// Reconciles state and prepares the single shared fill, overlay, diagnostics, and palettes.
pub fn prepare_spherical_field_layers<F>(
    source: SphericalPresentationSource,
    catalog: &FieldCatalog<'_>,
    cell_count: usize,
    edge_count: usize,
    diagnostics: &[OwnedViewDiagnostic],
    preferred_fill: Option<FieldId>,
    preferred_range: F,
    state: &mut SphericalFieldDisplayState,
    clock: &mut DisplayRevisionClock,
) -> Result<PreparedFieldLayers, DisplayPrepareError>
where
    F: Fn(&FieldId) -> Option<DisplayRangeMode>,
{
    let mut candidate_state = state.clone();
    let mut candidate_clock = clock.clone();
    reconcile_spherical_state(
        catalog,
        cell_count,
        edge_count,
        preferred_fill,
        &preferred_range,
        &mut candidate_state,
    );
    let parts = prepare_layer_parts(
        catalog,
        cell_count,
        edge_count,
        diagnostics,
        &candidate_state,
    )?;
    let packet = PreparedFieldLayers {
        source,
        fill: parts.fill,
        overlay: parts.overlay,
        diagnostics: parts.diagnostics,
        fill_palette: parts.fill_palette,
        overlay_palette: parts.overlay_palette,
        revisions: issue_all_revisions(&mut candidate_clock)?,
        diagnostics_enabled: candidate_state.diagnostics_enabled(),
        prepared_state: PreparedLayerState::from(&candidate_state),
    };
    *state = candidate_state;
    *clock = candidate_clock;
    Ok(packet)
}

/// Reconciles one changed spherical state and reuses every unchanged shared allocation.
pub fn update_spherical_field_layers<F>(
    current: &PreparedFieldLayers,
    source: SphericalPresentationSource,
    catalog: &FieldCatalog<'_>,
    cell_count: usize,
    edge_count: usize,
    diagnostics: &[OwnedViewDiagnostic],
    preferred_fill: Option<FieldId>,
    preferred_range: F,
    state: &mut SphericalFieldDisplayState,
    clock: &mut DisplayRevisionClock,
) -> Result<PreparedFieldLayers, DisplayPrepareError>
where
    F: Fn(&FieldId) -> Option<DisplayRangeMode>,
{
    let mut candidate_state = state.clone();
    let mut candidate_clock = clock.clone();
    reconcile_spherical_state(
        catalog,
        cell_count,
        edge_count,
        preferred_fill,
        &preferred_range,
        &mut candidate_state,
    );
    validate_diagnostics(diagnostics, cell_count)?;
    let next_state = PreparedLayerState::from(&candidate_state);
    let mut revisions = current.revisions;
    let fill = if fill_needs_preparation(&current.prepared_state, &next_state) {
        reuse_or_replace(
            &current.fill,
            prepare_fill(catalog, cell_count, &candidate_state)?,
            &mut revisions.fill,
            &mut candidate_clock,
        )?
    } else {
        current.fill.clone()
    };
    let overlay = if overlay_needs_preparation(&current.prepared_state, &next_state) {
        reuse_overlay_or_replace(
            &current.overlay,
            prepare_overlay_for_state(catalog, cell_count, edge_count, &candidate_state)?,
            &mut revisions.overlay,
            &mut candidate_clock,
        )?
    } else {
        current.overlay.clone()
    };
    let diagnostics = if diagnostics_need_preparation(&current.prepared_state, &next_state) {
        reuse_or_replace(
            &current.diagnostics,
            prepare_diagnostics(diagnostics, cell_count, &candidate_state)?,
            &mut revisions.diagnostics,
            &mut candidate_clock,
        )?
    } else {
        current.diagnostics.clone()
    };
    let fill_palette = if fill_palette_needs_preparation(&current.prepared_state, &next_state) {
        reuse_or_replace(
            &current.fill_palette,
            prepare_fill_palette(catalog, &candidate_state)?,
            &mut revisions.fill_palette,
            &mut candidate_clock,
        )?
    } else {
        current.fill_palette.clone()
    };
    let overlay_palette = if overlay_palette_needs_preparation(&current.prepared_state, &next_state)
    {
        reuse_optional_or_replace(
            &current.overlay_palette,
            prepare_overlay_palette(catalog, &candidate_state)?,
            &mut revisions.overlay_palette,
            &mut candidate_clock,
        )?
    } else {
        current.overlay_palette.clone()
    };
    let packet = PreparedFieldLayers {
        source,
        fill,
        overlay,
        diagnostics,
        fill_palette,
        overlay_palette,
        revisions,
        diagnostics_enabled: candidate_state.diagnostics_enabled(),
        prepared_state: next_state,
    };
    *state = candidate_state;
    *clock = candidate_clock;
    Ok(packet)
}

struct PreparedLayerParts {
    fill: Arc<PreparedCellField>,
    overlay: Option<PreparedSphericalOverlay>,
    diagnostics: Arc<PreparedDiagnosticMask>,
    fill_palette: Arc<[LinearRgba]>,
    overlay_palette: Option<Arc<[LinearRgba]>>,
}

fn prepare_layer_parts(
    catalog: &FieldCatalog<'_>,
    cell_count: usize,
    edge_count: usize,
    diagnostics: &[OwnedViewDiagnostic],
    state: &SphericalFieldDisplayState,
) -> Result<PreparedLayerParts, DisplayPrepareError> {
    Ok(PreparedLayerParts {
        fill: prepare_fill(catalog, cell_count, state)?,
        overlay: prepare_overlay_for_state(catalog, cell_count, edge_count, state)?,
        diagnostics: prepare_diagnostics(diagnostics, cell_count, state)?,
        fill_palette: prepare_fill_palette(catalog, state)?,
        overlay_palette: prepare_overlay_palette(catalog, state)?,
    })
}

fn prepare_fill(
    catalog: &FieldCatalog<'_>,
    cell_count: usize,
    state: &SphericalFieldDisplayState,
) -> Result<Arc<PreparedCellField>, DisplayPrepareError> {
    #[cfg(test)]
    record_preparation(|counts| counts.fill += 1);
    let fill_entry = state
        .fill_field()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .ok_or(DisplayPrepareError::NoRenderableField)?;
    let fill = Arc::new(super::prepare_cell_field(
        fill_entry,
        cell_count,
        state.range_mode(),
    )?);
    Ok(fill)
}

fn prepare_overlay_for_state(
    catalog: &FieldCatalog<'_>,
    cell_count: usize,
    edge_count: usize,
    state: &SphericalFieldDisplayState,
) -> Result<Option<PreparedSphericalOverlay>, DisplayPrepareError> {
    #[cfg(test)]
    if state.overlay_field().is_some() {
        record_preparation(|counts| counts.overlay += 1);
    }
    state
        .overlay_field()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .map(|field| prepare_overlay(field, cell_count, edge_count, state.range_mode()))
        .transpose()
}

fn prepare_diagnostics(
    diagnostics: &[OwnedViewDiagnostic],
    cell_count: usize,
    state: &SphericalFieldDisplayState,
) -> Result<Arc<PreparedDiagnosticMask>, DisplayPrepareError> {
    #[cfg(test)]
    record_preparation(|counts| counts.diagnostics += 1);
    Ok(Arc::new(PreparedDiagnosticMask::build(
        cell_count,
        diagnostics.iter().map(OwnedViewDiagnostic::as_ref),
        state.fill_field(),
        state.diagnostic_scope(),
    )?))
}

fn validate_diagnostics(
    diagnostics: &[OwnedViewDiagnostic],
    cell_count: usize,
) -> Result<(), DisplayPrepareError> {
    for diagnostic in diagnostics {
        let Some(cell) = diagnostic.cell_id else {
            continue;
        };
        let index = usize::try_from(cell.raw())
            .map_err(|_| DisplayPrepareError::DiagnosticCellOutOfRange { cell, cell_count })?;
        if index >= cell_count {
            return Err(DisplayPrepareError::DiagnosticCellOutOfRange { cell, cell_count });
        }
    }
    Ok(())
}

fn prepare_fill_palette(
    catalog: &FieldCatalog<'_>,
    state: &SphericalFieldDisplayState,
) -> Result<Arc<[LinearRgba]>, DisplayPrepareError> {
    let fill = state
        .fill_field()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .ok_or(DisplayPrepareError::NoRenderableField)?;
    Ok(Arc::from(built_in_palette(fill_palette_for(fill, state)?)))
}

fn prepare_overlay_palette(
    catalog: &FieldCatalog<'_>,
    state: &SphericalFieldDisplayState,
) -> Result<Option<Arc<[LinearRgba]>>, DisplayPrepareError> {
    state
        .overlay_field()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .map(|overlay| Ok(Arc::from(built_in_palette(overlay_palette_for(overlay)?))))
        .transpose()
}

fn prepare_overlay(
    field: &FieldView<'_>,
    cell_count: usize,
    edge_count: usize,
    range_mode: DisplayRangeMode,
) -> Result<PreparedSphericalOverlay, DisplayPrepareError> {
    match classify_spherical_channel(field.schema().domain, field.schema().value_type) {
        Some(SphericalFieldChannel::EdgeOverlay) => Ok(PreparedSphericalOverlay::Edge(Arc::new(
            prepare_edge_field(field, edge_count, range_mode)?,
        ))),
        Some(SphericalFieldChannel::VectorOverlay) => Ok(PreparedSphericalOverlay::Vector(
            Arc::new(prepare_vector_field(field, cell_count, range_mode)?),
        )),
        _ => Err(DisplayPrepareError::UnsupportedSphericalChannel {
            field: field.schema().id.clone(),
        }),
    }
}

fn reconcile_spherical_state<F>(
    catalog: &FieldCatalog<'_>,
    cell_count: usize,
    edge_count: usize,
    preferred_fill: Option<FieldId>,
    preferred_range: &F,
    state: &mut SphericalFieldDisplayState,
) where
    F: Fn(&FieldId) -> Option<DisplayRangeMode>,
{
    let selected_fill = state
        .fill_field()
        .and_then(|id| catalog.get(id))
        .filter(|entry| {
            entry.view().is_some_and(|view| {
                classify_spherical_channel(view.schema().domain, view.schema().value_type)
                    == Some(SphericalFieldChannel::CellFill)
                    && view.len() == cell_count
            })
        });
    let fill = selected_fill
        .or_else(|| {
            preferred_fill
                .as_ref()
                .and_then(|id| catalog.get(id))
                .filter(|entry| {
                    entry.view().is_some_and(|view| {
                        classify_spherical_channel(view.schema().domain, view.schema().value_type)
                            == Some(SphericalFieldChannel::CellFill)
                            && view.len() == cell_count
                    })
                })
        })
        .or_else(|| {
            catalog.entries().iter().find(|entry| {
                entry.view().is_some_and(|view| {
                    classify_spherical_channel(view.schema().domain, view.schema().value_type)
                        == Some(SphericalFieldChannel::CellFill)
                        && view.len() == cell_count
                })
            })
        });
    let fill_changed = state.fill_field() != fill.map(|entry| &entry.schema().id);
    state.fill_field = fill.map(|entry| entry.schema().id.clone());
    if fill_changed {
        state.range_mode = state
            .fill_field()
            .and_then(preferred_range)
            .unwrap_or(DisplayRangeMode::Data);
    }

    let overlay = state
        .overlay_field()
        .and_then(|id| catalog.get(id))
        .filter(|entry| {
            entry.view().is_some_and(|view| {
                match classify_spherical_channel(view.schema().domain, view.schema().value_type) {
                    Some(SphericalFieldChannel::EdgeOverlay) => view.len() == edge_count,
                    Some(SphericalFieldChannel::VectorOverlay) => view.len() == cell_count,
                    _ => false,
                }
            })
        });
    state.overlay_field = overlay.map(|entry| entry.schema().id.clone());

    if state.selected_entity.is_some_and(|entity| match entity {
        SelectedSurfaceEntity::Cell(cell) => cell.raw() as usize >= cell_count,
        SelectedSurfaceEntity::Edge(edge) => edge.raw() as usize >= edge_count,
    }) {
        state.selected_entity = None;
    }
    if let (Some(palette), Some(entry)) = (
        state.palette_override,
        state.fill_field().and_then(|id| catalog.get(id)),
    ) {
        if !palette_matches_hint(palette, entry.schema().display.palette()) {
            state.palette_override = None;
        }
    }
}

fn fill_palette_for(
    field: &FieldView<'_>,
    state: &SphericalFieldDisplayState,
) -> Result<PaletteId, DisplayPrepareError> {
    let schema_palette = match field.schema().display.palette() {
        FieldPaletteHint::Sequential | FieldPaletteHint::Vector => PaletteId::Sequential,
        FieldPaletteHint::Diverging => PaletteId::Diverging,
        FieldPaletteHint::Categorical => PaletteId::Categorical,
        FieldPaletteHint::Boolean => {
            return Err(DisplayPrepareError::UnsupportedSphericalChannel {
                field: field.schema().id.clone(),
            });
        }
    };
    Ok(state.palette_override().unwrap_or(schema_palette))
}

fn overlay_palette_for(field: &FieldView<'_>) -> Result<PaletteId, DisplayPrepareError> {
    match field.schema().display.palette() {
        FieldPaletteHint::Sequential | FieldPaletteHint::Vector => Ok(PaletteId::Sequential),
        FieldPaletteHint::Diverging => Ok(PaletteId::Diverging),
        FieldPaletteHint::Categorical => Ok(PaletteId::Categorical),
        FieldPaletteHint::Boolean => Err(DisplayPrepareError::UnsupportedSphericalChannel {
            field: field.schema().id.clone(),
        }),
    }
}

fn fill_needs_preparation(current: &PreparedLayerState, next: &PreparedLayerState) -> bool {
    current.fill_field != next.fill_field || current.range_mode != next.range_mode
}

fn overlay_needs_preparation(current: &PreparedLayerState, next: &PreparedLayerState) -> bool {
    current.overlay_field != next.overlay_field || current.range_mode != next.range_mode
}

fn diagnostics_need_preparation(current: &PreparedLayerState, next: &PreparedLayerState) -> bool {
    current.diagnostic_scope != next.diagnostic_scope || current.fill_field != next.fill_field
}

fn fill_palette_needs_preparation(current: &PreparedLayerState, next: &PreparedLayerState) -> bool {
    current.fill_field != next.fill_field || current.palette_override != next.palette_override
}

fn overlay_palette_needs_preparation(
    current: &PreparedLayerState,
    next: &PreparedLayerState,
) -> bool {
    current.overlay_field != next.overlay_field
}

fn palette_matches_hint(palette: PaletteId, hint: FieldPaletteHint) -> bool {
    matches!(
        (palette, hint),
        (
            PaletteId::Sequential,
            FieldPaletteHint::Sequential | FieldPaletteHint::Vector
        ) | (PaletteId::Diverging, FieldPaletteHint::Diverging)
            | (PaletteId::Categorical, FieldPaletteHint::Categorical)
    )
}

fn magnitude_range(values: &[f32]) -> Option<ResolvedDisplayRange> {
    let first = *values.first()?;
    let (min, max) = values
        .iter()
        .copied()
        .fold((first, first), |(min, max), value| {
            (min.min(value), max.max(value))
        });
    ResolvedDisplayRange::new(min, max).ok()
}

fn issue_all_revisions(
    clock: &mut DisplayRevisionClock,
) -> Result<FieldLayerRevisions, DisplayPrepareError> {
    Ok(FieldLayerRevisions {
        fill: clock.issue()?,
        overlay: clock.issue()?,
        diagnostics: clock.issue()?,
        fill_palette: clock.issue()?,
        overlay_palette: clock.issue()?,
    })
}

fn reuse_or_replace<T: PartialEq + ?Sized>(
    current: &Arc<T>,
    next: Arc<T>,
    revision: &mut DisplayRevision,
    clock: &mut DisplayRevisionClock,
) -> Result<Arc<T>, DisplayPrepareError> {
    if current.as_ref() == next.as_ref() {
        Ok(current.clone())
    } else {
        *revision = clock.issue()?;
        Ok(next)
    }
}

fn reuse_optional_or_replace<T: PartialEq + ?Sized>(
    current: &Option<Arc<T>>,
    next: Option<Arc<T>>,
    revision: &mut DisplayRevision,
    clock: &mut DisplayRevisionClock,
) -> Result<Option<Arc<T>>, DisplayPrepareError> {
    match (current, next) {
        (Some(current), Some(next)) => reuse_or_replace(current, next, revision, clock).map(Some),
        (None, None) => Ok(None),
        (_, next) => {
            *revision = clock.issue()?;
            Ok(next)
        }
    }
}

fn reuse_overlay_or_replace(
    current: &Option<PreparedSphericalOverlay>,
    next: Option<PreparedSphericalOverlay>,
    revision: &mut DisplayRevision,
    clock: &mut DisplayRevisionClock,
) -> Result<Option<PreparedSphericalOverlay>, DisplayPrepareError> {
    match (current, next) {
        (
            Some(PreparedSphericalOverlay::Edge(current)),
            Some(PreparedSphericalOverlay::Edge(next)),
        ) => reuse_or_replace(current, next, revision, clock)
            .map(PreparedSphericalOverlay::Edge)
            .map(Some),
        (
            Some(PreparedSphericalOverlay::Vector(current)),
            Some(PreparedSphericalOverlay::Vector(next)),
        ) => reuse_or_replace(current, next, revision, clock)
            .map(PreparedSphericalOverlay::Vector)
            .map(Some),
        (None, None) => Ok(None),
        (_, next) => {
            *revision = clock.issue()?;
            Ok(next)
        }
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
