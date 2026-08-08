//! Renderer-neutral field selection state and prepared data for spherical presentation.

use std::sync::Arc;

#[cfg(test)]
use std::cell::Cell;

use thiserror::Error;

use super::palette::prepare_scalar_or_category_field;
use super::{
    built_in_palette, DiagnosticScope, DisplayPrepareError, DisplayRangeMode, DisplayRevision,
    DisplayRevisionClock, FieldCatalog, FieldView, GlobeCamera, LinearRgba, MapCamera,
    OwnedViewDiagnostic, PaletteId, PreparedCellField, PreparedDiagnosticMask, PreparedFieldKind,
    PreparedGlobeMesh, PreparedProjectedMap, ResolvedDisplayRange, SphericalPresentationSource,
    SphericalProjection, SphericalProjectionError, SphericalProjectionKind, SphericalViewMode,
    ViewDiagnosticSeverity,
};
use crate::world::fields::{FieldDomain, FieldId, FieldPaletteHint, FieldValueType};
use crate::world::spatial::{canonical_east_north_basis, SurfaceRef, UnitVector3};
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
    pub(crate) diagnostic_validation_values_scanned: usize,
    pub(crate) diagnostic_fingerprint_values_scanned: usize,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectedSurfaceEntity {
    /// One authoritative surface cell.
    Cell(CellId),
    /// One authoritative surface edge.
    Edge(EdgeId),
}

/// The density of prepared vector glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum VectorGlyphLod {
    /// Sparse glyph subset.
    Low,
    /// Balanced glyph subset.
    #[default]
    Medium,
    /// Dense glyph subset.
    High,
}

/// A discrete, cacheable vector-glyph density selected from stable zoom thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlyphLodKey {
    /// One source-keyed cell in sixteen, plus the selected cell.
    Low,
    /// One source-keyed cell in eight, plus the selected cell.
    #[default]
    Medium,
    /// One source-keyed cell in four, plus the selected cell.
    High,
}

impl GlyphLodKey {
    /// Returns the exact stable sampling denominator for this density.
    pub const fn denominator(self) -> u64 {
        match self {
            Self::Low => 16,
            Self::Medium => 8,
            Self::High => 4,
        }
    }

    /// Tests one already-computed stable score without introducing runtime randomness.
    pub const fn includes_score(self, score: u64) -> bool {
        score % self.denominator() == 0
    }

    /// Resolves explicit density plus zoom through the fixed `2x` and `4x` thresholds.
    ///
    /// Zoom can only add glyphs. It never replaces an already-visible stable identity.
    pub fn for_zoom(base: VectorGlyphLod, zoom: f64) -> Self {
        let zoom = if zoom.is_finite() { zoom.max(0.0) } else { 0.0 };
        match base {
            VectorGlyphLod::High => Self::High,
            VectorGlyphLod::Medium if zoom >= 2.0 => Self::High,
            VectorGlyphLod::Medium => Self::Medium,
            VectorGlyphLod::Low if zoom >= 4.0 => Self::High,
            VectorGlyphLod::Low if zoom >= 2.0 => Self::Medium,
            VectorGlyphLod::Low => Self::Low,
        }
    }
}

impl From<VectorGlyphLod> for GlyphLodKey {
    fn from(value: VectorGlyphLod) -> Self {
        match value {
            VectorGlyphLod::Low => Self::Low,
            VectorGlyphLod::Medium => Self::Medium,
            VectorGlyphLod::High => Self::High,
        }
    }
}

/// Fixed-size display-only vector animation state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VectorAnimationUniform {
    phase: f32,
}

impl VectorAnimationUniform {
    /// Creates a phase normalized modulo one. Non-finite inputs start at zero.
    pub fn new(phase: f32) -> Self {
        Self {
            phase: if phase.is_finite() {
                phase.rem_euclid(1.0)
            } else {
                0.0
            },
        }
    }

    /// Returns the moving-highlight phase in `[0, 1)`.
    pub const fn phase(self) -> f32 {
        self.phase
    }

    /// Advances only the display highlight, clamping speed to the supported readability range.
    pub fn advance(&mut self, frame_delta_seconds: f32, display_speed: f32, paused: bool) {
        if paused || !frame_delta_seconds.is_finite() || frame_delta_seconds <= 0.0 {
            return;
        }
        let speed = if display_speed.is_finite() {
            display_speed.clamp(0.0, 4.0)
        } else {
            0.0
        };
        self.phase = (self.phase + frame_delta_seconds * speed).rem_euclid(1.0);
    }

    /// Labels this phase as display-only rather than physical time.
    pub const fn display_semantics(self) -> &'static str {
        "display-only"
    }
}

impl Default for VectorAnimationUniform {
    fn default() -> Self {
        Self::new(0.0)
    }
}

/// Errors from spherical field-layer display controls.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum FieldLayerError {
    /// The vector animation display speed is non-finite or outside its supported range.
    #[error("vector display speed must be finite and within 0.0..=4.0, got {0}")]
    InvalidVectorDisplaySpeed(f32),
    /// The view zoom used to resolve vector-glyph density is non-finite or non-positive.
    #[error("vector view zoom must be finite and greater than zero, got {0}")]
    InvalidVectorViewZoom(f64),
    /// Source-bound glyph inputs do not all describe the same authoritative world.
    #[error("{resource} has a different spherical presentation source")]
    VectorGlyphSourceMismatch { resource: &'static str },
    /// Vector glyph data does not match the authoritative cell allocation.
    #[error("vector glyph field cardinality {actual} does not match {expected} cells")]
    VectorGlyphCardinalityMismatch { expected: usize, actual: usize },
    /// A selected vector-glyph cell lies outside the authoritative allocation.
    #[error("selected vector glyph cell {cell:?} lies outside {cell_count} cells")]
    SelectedVectorCellOutOfRange { cell: CellId, cell_count: usize },
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

    /// Returns the number of authoritative cell vectors retained for inspection.
    pub fn len(&self) -> usize {
        self.components.len()
    }

    /// Returns whether this field contains no authoritative cell vectors.
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }
}

/// One projected-map vector arrow with authoritative identity and display encodings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapVectorGlyph {
    cell: CellId,
    origin: [f32; 2],
    direction: [f32; 2],
    components: [f32; 2],
    magnitude: f32,
    color_position: f32,
    length: f32,
    cell_spacing: f32,
}

impl MapVectorGlyph {
    /// Returns the authoritative cell represented by this display arrow.
    pub const fn cell(self) -> CellId {
        self.cell
    }

    /// Returns the projection-local arrow origin.
    pub const fn origin(self) -> [f32; 2] {
        self.origin
    }

    /// Returns the normalized projected direction.
    pub const fn direction(self) -> [f32; 2] {
        self.direction
    }

    /// Returns the authoritative canonical east/north components.
    pub const fn components(self) -> [f32; 2] {
        self.components
    }

    /// Returns the authoritative vector magnitude.
    pub const fn magnitude(self) -> f32 {
        self.magnitude
    }

    /// Returns the shared normalized magnitude used for palette sampling.
    pub const fn color_position(self) -> f32 {
        self.color_position
    }

    /// Returns the projection-local display length.
    pub const fn length(self) -> f32 {
        self.length
    }

    /// Returns display length as a fraction of the active LOD cell spacing.
    pub fn length_fraction(self) -> f32 {
        self.length / self.cell_spacing
    }
}

/// One unit-globe tangent vector arrow with authoritative identity and display encodings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobeVectorGlyph {
    cell: CellId,
    radial: UnitVector3,
    direction: [f32; 3],
    components: [f32; 2],
    magnitude: f32,
    color_position: f32,
    length: f32,
    cell_spacing: f32,
}

impl GlobeVectorGlyph {
    /// Returns the authoritative cell represented by this display arrow.
    pub const fn cell(self) -> CellId {
        self.cell
    }

    /// Returns the exact unit radial used as the arrow origin.
    pub const fn radial(self) -> UnitVector3 {
        self.radial
    }

    /// Returns the normalized tangent direction reconstructed from east/north components.
    pub const fn direction(self) -> [f32; 3] {
        self.direction
    }

    /// Returns the authoritative canonical east/north components.
    pub const fn components(self) -> [f32; 2] {
        self.components
    }

    /// Returns the authoritative vector magnitude.
    pub const fn magnitude(self) -> f32 {
        self.magnitude
    }

    /// Returns the shared normalized magnitude used for palette sampling.
    pub const fn color_position(self) -> f32 {
        self.color_position
    }

    /// Returns the angular display length on the unit sphere.
    pub const fn length(self) -> f32 {
        self.length
    }

    /// Returns display length as a fraction of the active LOD cell spacing.
    pub fn length_fraction(self) -> f32 {
        self.length / self.cell_spacing
    }
}

/// Source-bound deterministic map/globe glyph instances for one vector field and LOD key.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedVectorGlyphs {
    source: SphericalPresentationSource,
    lod_key: GlyphLodKey,
    sampled_cells: Arc<[CellId]>,
    map: Arc<[MapVectorGlyph]>,
    globe: Arc<[GlobeVectorGlyph]>,
    diagnostics: Arc<[OwnedViewDiagnostic]>,
}

impl PreparedVectorGlyphs {
    /// Builds nested stable glyph identities and mode-specific directions once.
    pub fn build(
        source: &SphericalPresentationSource,
        map: &PreparedProjectedMap,
        globe: &PreparedGlobeMesh,
        field: &PreparedVectorField,
        selected_cell: Option<CellId>,
        lod_key: GlyphLodKey,
    ) -> Result<Self, FieldLayerError> {
        if map.source() != source {
            return Err(FieldLayerError::VectorGlyphSourceMismatch {
                resource: "projected map",
            });
        }
        if globe.source() != source {
            return Err(FieldLayerError::VectorGlyphSourceMismatch {
                resource: "unit globe",
            });
        }
        let cell_count = map.cell_count();
        if globe.cell_count() != cell_count || field.len() != cell_count {
            return Err(FieldLayerError::VectorGlyphCardinalityMismatch {
                expected: cell_count,
                actual: field.len(),
            });
        }
        if let Some(cell) = selected_cell {
            if cell.raw() as usize >= cell_count {
                return Err(FieldLayerError::SelectedVectorCellOutOfRange { cell, cell_count });
            }
        }
        let map_area = (map.bounds().max_x() - map.bounds().min_x())
            * (map.bounds().max_y() - map.bounds().min_y());
        let density_scale = lod_key.denominator() as f64 / cell_count as f64;
        let map_spacing = (map_area * density_scale).sqrt() as f32;
        let globe_spacing = (std::f64::consts::TAU * 2.0 * density_scale).sqrt() as f32;
        let mut sampled_cells = Vec::new();
        let mut map_glyphs = Vec::new();
        let mut globe_glyphs = Vec::new();
        let mut diagnostics = Vec::new();

        for (index, (&components, &magnitude)) in field
            .components()
            .iter()
            .zip(field.magnitudes())
            .enumerate()
        {
            let cell = CellId::from_raw(index as u32);
            if selected_cell != Some(cell)
                && !lod_key.includes_score(vector_glyph_score(source, cell))
            {
                continue;
            }
            sampled_cells.push(cell);
            if components == [0.0, 0.0] {
                continue;
            }
            let radial = globe.cell_centroids()[index];
            let color_position = normalize_magnitude(field.display_range(), magnitude);
            let mut pair = prepare_vector_glyph_pair(
                cell,
                radial,
                components,
                magnitude,
                color_position,
                map_spacing,
                globe_spacing,
                map.projection(),
            );
            if let Some(mut diagnostic) = pair.diagnostic.take() {
                diagnostic.field_id = Some(field.field_id().clone());
                diagnostics.push(diagnostic);
            }
            if let Some(glyph) = pair.map {
                map_glyphs.push(glyph);
            }
            if let Some(glyph) = pair.globe {
                globe_glyphs.push(glyph);
            }
        }

        Ok(Self {
            source: source.clone(),
            lod_key,
            sampled_cells: Arc::from(sampled_cells),
            map: Arc::from(map_glyphs),
            globe: Arc::from(globe_glyphs),
            diagnostics: Arc::from(diagnostics),
        })
    }

    /// Returns the source identity from which every instance was derived.
    pub const fn source(&self) -> &SphericalPresentationSource {
        &self.source
    }

    /// Returns the discrete cache key used for this nested subset.
    pub const fn lod_key(&self) -> GlyphLodKey {
        self.lod_key
    }

    /// Returns sampled identities, including selected and zero-vector cells.
    pub fn sampled_cells(&self) -> &[CellId] {
        &self.sampled_cells
    }

    /// Returns usable projected-map direction glyphs.
    pub fn map(&self) -> &[MapVectorGlyph] {
        &self.map
    }

    /// Returns usable unit-globe tangent glyphs.
    pub fn globe(&self) -> &[GlobeVectorGlyph] {
        &self.globe
    }

    /// Returns display-only diagnostics for map Jacobian omissions.
    pub fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.diagnostics
    }
}

pub(crate) fn prepare_map_vector_glyphs(
    source: &SphericalPresentationSource,
    map: &PreparedProjectedMap,
    globe: &PreparedGlobeMesh,
    field: &PreparedVectorField,
    selected_cell: Option<CellId>,
    lod_key: GlyphLodKey,
) -> Result<(Vec<MapVectorGlyph>, Vec<OwnedViewDiagnostic>), FieldLayerError> {
    validate_map_vector_inputs(source, map, globe, field, selected_cell)?;
    let cell_count = map.cell_count();
    let map_area = (map.bounds().max_x() - map.bounds().min_x())
        * (map.bounds().max_y() - map.bounds().min_y());
    let map_spacing = (map_area * lod_key.denominator() as f64 / cell_count as f64).sqrt() as f32;
    let mut glyphs = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, (&components, &magnitude)) in field
        .components()
        .iter()
        .zip(field.magnitudes())
        .enumerate()
    {
        let cell = CellId::from_raw(index as u32);
        if !vector_cell_is_sampled(source, cell, selected_cell, lod_key) || components == [0.0, 0.0]
        {
            continue;
        }
        let color_position = normalize_magnitude(field.display_range(), magnitude);
        let (glyph, diagnostic) = prepare_map_vector_glyph(
            cell,
            globe.cell_centroids()[index],
            components,
            magnitude,
            color_position,
            map_spacing,
            map.projection(),
        );
        if let Some(glyph) = glyph {
            glyphs.push(glyph);
        }
        if let Some(mut diagnostic) = diagnostic {
            diagnostic.field_id = Some(field.field_id().clone());
            diagnostics.push(diagnostic);
        }
    }
    Ok((glyphs, diagnostics))
}

pub(crate) fn prepare_globe_vector_glyphs(
    source: &SphericalPresentationSource,
    globe: &PreparedGlobeMesh,
    field: &PreparedVectorField,
    selected_cell: Option<CellId>,
    lod_key: GlyphLodKey,
) -> Result<Vec<GlobeVectorGlyph>, FieldLayerError> {
    validate_globe_vector_inputs(source, globe, field, selected_cell)?;
    let cell_count = globe.cell_count();
    let globe_spacing = (std::f64::consts::TAU * 2.0 * lod_key.denominator() as f64
        / cell_count as f64)
        .sqrt() as f32;
    let mut glyphs = Vec::new();
    for (index, (&components, &magnitude)) in field
        .components()
        .iter()
        .zip(field.magnitudes())
        .enumerate()
    {
        let cell = CellId::from_raw(index as u32);
        if !vector_cell_is_sampled(source, cell, selected_cell, lod_key) || components == [0.0, 0.0]
        {
            continue;
        }
        let color_position = normalize_magnitude(field.display_range(), magnitude);
        if let Some(glyph) = prepare_globe_vector_glyph(
            cell,
            globe.cell_centroids()[index],
            components,
            magnitude,
            color_position,
            globe_spacing,
        ) {
            glyphs.push(glyph);
        }
    }
    Ok(glyphs)
}

fn validate_map_vector_inputs(
    source: &SphericalPresentationSource,
    map: &PreparedProjectedMap,
    globe: &PreparedGlobeMesh,
    field: &PreparedVectorField,
    selected_cell: Option<CellId>,
) -> Result<(), FieldLayerError> {
    if map.source() != source {
        return Err(FieldLayerError::VectorGlyphSourceMismatch {
            resource: "projected map",
        });
    }
    validate_globe_vector_inputs(source, globe, field, selected_cell)?;
    if map.cell_count() != globe.cell_count() {
        return Err(FieldLayerError::VectorGlyphCardinalityMismatch {
            expected: map.cell_count(),
            actual: globe.cell_count(),
        });
    }
    Ok(())
}

fn validate_globe_vector_inputs(
    source: &SphericalPresentationSource,
    globe: &PreparedGlobeMesh,
    field: &PreparedVectorField,
    selected_cell: Option<CellId>,
) -> Result<(), FieldLayerError> {
    if globe.source() != source {
        return Err(FieldLayerError::VectorGlyphSourceMismatch {
            resource: "unit globe",
        });
    }
    let cell_count = globe.cell_count();
    if field.len() != cell_count {
        return Err(FieldLayerError::VectorGlyphCardinalityMismatch {
            expected: cell_count,
            actual: field.len(),
        });
    }
    if let Some(cell) = selected_cell {
        if cell.raw() as usize >= cell_count {
            return Err(FieldLayerError::SelectedVectorCellOutOfRange { cell, cell_count });
        }
    }
    Ok(())
}

fn vector_cell_is_sampled(
    source: &SphericalPresentationSource,
    cell: CellId,
    selected_cell: Option<CellId>,
    lod_key: GlyphLodKey,
) -> bool {
    selected_cell == Some(cell) || lod_key.includes_score(vector_glyph_score(source, cell))
}

struct PreparedVectorGlyphPair {
    map: Option<MapVectorGlyph>,
    globe: Option<GlobeVectorGlyph>,
    diagnostic: Option<OwnedViewDiagnostic>,
}

#[allow(clippy::too_many_arguments)]
fn prepare_vector_glyph_pair(
    cell: CellId,
    radial: UnitVector3,
    components: [f32; 2],
    magnitude: f32,
    color_position: f32,
    map_spacing: f32,
    globe_spacing: f32,
    projection: SphericalProjection,
) -> PreparedVectorGlyphPair {
    let globe = prepare_globe_vector_glyph(
        cell,
        radial,
        components,
        magnitude,
        color_position,
        globe_spacing,
    );
    let (map, diagnostic) = prepare_map_vector_glyph(
        cell,
        radial,
        components,
        magnitude,
        color_position,
        map_spacing,
        projection,
    );
    PreparedVectorGlyphPair {
        map,
        globe,
        diagnostic,
    }
}

fn prepare_globe_vector_glyph(
    cell: CellId,
    radial: UnitVector3,
    components: [f32; 2],
    magnitude: f32,
    color_position: f32,
    globe_spacing: f32,
) -> Option<GlobeVectorGlyph> {
    let (east, north) = canonical_east_north_basis(radial);
    let tangent = [
        east[0] * f64::from(components[0]) + north[0] * f64::from(components[1]),
        east[1] * f64::from(components[0]) + north[1] * f64::from(components[1]),
        east[2] * f64::from(components[0]) + north[2] * f64::from(components[1]),
    ];
    let tangent_length = tangent
        .into_iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    let length_fraction = 0.35 + 0.65 * color_position;
    (tangent_length.is_finite() && tangent_length > 0.0).then(|| GlobeVectorGlyph {
        cell,
        radial,
        direction: tangent.map(|value| (value / tangent_length) as f32),
        components,
        magnitude,
        color_position,
        length: globe_spacing * length_fraction,
        cell_spacing: globe_spacing,
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_map_vector_glyph(
    cell: CellId,
    radial: UnitVector3,
    components: [f32; 2],
    magnitude: f32,
    color_position: f32,
    map_spacing: f32,
    projection: SphericalProjection,
) -> (Option<MapVectorGlyph>, Option<OwnedViewDiagnostic>) {
    let length_fraction = 0.35 + 0.65 * color_position;
    let mapped =
        projection.map_local_vector(radial, [f64::from(components[0]), f64::from(components[1])]);
    match mapped {
        Ok(Some(direction)) => match projection.forward(radial) {
            Ok(origin) => (
                Some(MapVectorGlyph {
                    cell,
                    origin: [origin.x() as f32, origin.y() as f32],
                    direction: [direction.x() as f32, direction.y() as f32],
                    components,
                    magnitude,
                    color_position,
                    length: map_spacing * length_fraction,
                    cell_spacing: map_spacing,
                }),
                None,
            ),
            Err(_) => (None, None),
        },
        Err(SphericalProjectionError::ProjectionJacobianDegenerate) => (
            None,
            Some(OwnedViewDiagnostic {
                severity: ViewDiagnosticSeverity::Warning,
                code: "display.vector_projection_jacobian_degenerate".into(),
                field_id: None,
                cell_id: Some(cell),
                message:
                    "map vector glyph omitted because the local projection Jacobian is degenerate"
                        .into(),
            }),
        ),
        Ok(None) | Err(_) => (None, None),
    }
}

fn normalize_magnitude(range: ResolvedDisplayRange, magnitude: f32) -> f32 {
    let (min, max) = range.bounds();
    if max == min {
        0.5
    } else {
        ((magnitude - min) / (max - min)).clamp(0.0, 1.0)
    }
}

fn vector_glyph_score(source: &SphericalPresentationSource, cell: CellId) -> u64 {
    let mut hasher = blake3::Hasher::new_keyed(source.build_result_hash().as_bytes());
    hasher.update(b"sekai.spherical-vector-glyph-lod.v1\0");
    hasher.update(&source.root_seed().raw().to_le_bytes());
    let surface_ref: SurfaceRef = source.surface_ref();
    hasher.update(&surface_ref.fingerprint());
    hasher.update(&source.graph_contract_version().to_le_bytes());
    hasher.update(&cell.raw().to_le_bytes());
    let bytes = hasher.finalize();
    u64::from_le_bytes(
        bytes.as_bytes()[..8]
            .try_into()
            .expect("BLAKE3 prefix is eight bytes"),
    )
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
    /// Source-bound vector glyph instances selected by cell and discrete LOD.
    pub vector_glyphs: DisplayRevision,
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
    diagnostics_fingerprint: blake3::Hash,
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedLayerState {
    fill_field: Option<FieldId>,
    overlay_field: Option<FieldId>,
    range_mode: DisplayRangeMode,
    palette_override: Option<PaletteId>,
    diagnostic_scope: DiagnosticScope,
    selected_cell: Option<CellId>,
    glyph_lod_key: GlyphLodKey,
}

impl From<&SphericalFieldDisplayState> for PreparedLayerState {
    fn from(state: &SphericalFieldDisplayState) -> Self {
        Self {
            fill_field: state.fill_field.clone(),
            overlay_field: state.overlay_field.clone(),
            range_mode: state.range_mode,
            palette_override: state.palette_override,
            diagnostic_scope: state.diagnostic_scope,
            selected_cell: match state.selected_entity {
                Some(SelectedSurfaceEntity::Cell(cell)) => Some(cell),
                Some(SelectedSurfaceEntity::Edge(_)) | None => None,
            },
            glyph_lod_key: GlyphLodKey::for_zoom(state.vector_lod, state.vector_view_zoom),
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

    /// Returns the selected cell forced into every vector-glyph LOD set.
    pub const fn selected_vector_cell(&self) -> Option<CellId> {
        self.prepared_state.selected_cell
    }

    /// Returns the discrete key controlling cached vector-glyph identities.
    pub const fn glyph_lod_key(&self) -> GlyphLodKey {
        self.prepared_state.glyph_lod_key
    }

    /// Returns whether every layer-bearing state input already matches this packet.
    ///
    /// Raw camera zoom and display-only vector animation are intentionally excluded. Their
    /// effects are represented by the effective glyph key and fixed frame uniform respectively.
    pub(crate) fn matches_camera_only_state(&self, state: &SphericalFieldDisplayState) -> bool {
        self.prepared_state.fill_field.as_ref() == state.fill_field.as_ref()
            && self.prepared_state.overlay_field.as_ref() == state.overlay_field.as_ref()
            && self.prepared_state.range_mode == state.range_mode
            && self.prepared_state.palette_override == state.palette_override
            && self.prepared_state.diagnostic_scope == state.diagnostic_scope
            && self.prepared_state.selected_cell
                == match state.selected_entity {
                    Some(SelectedSurfaceEntity::Cell(cell)) => Some(cell),
                    Some(SelectedSurfaceEntity::Edge(_)) | None => None,
                }
            && self.prepared_state.glyph_lod_key
                == GlyphLodKey::for_zoom(state.vector_lod, state.vector_view_zoom)
            && self.diagnostics_enabled == state.diagnostics_enabled
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
    vector_view_zoom: f64,
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
            vector_view_zoom: 1.0,
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

    /// Selects the minimum vector-glyph density; view zoom may raise it at fixed thresholds.
    pub fn set_vector_lod(&mut self, lod: VectorGlyphLod) {
        self.vector_lod = lod;
    }

    /// Returns the user-selected minimum vector-glyph density.
    pub const fn vector_lod(&self) -> VectorGlyphLod {
        self.vector_lod
    }

    /// Sets the current view zoom used to resolve the effective discrete glyph density.
    pub fn set_vector_view_zoom(&mut self, zoom: f64) -> Result<(), FieldLayerError> {
        if !zoom.is_finite() || zoom <= 0.0 {
            return Err(FieldLayerError::InvalidVectorViewZoom(zoom));
        }
        self.vector_view_zoom = zoom;
        Ok(())
    }

    /// Returns the current view zoom used for vector-glyph LOD thresholds.
    pub const fn vector_view_zoom(&self) -> f64 {
        self.vector_view_zoom
    }

    /// Synchronizes vector-glyph density from the currently active map or globe camera.
    pub fn sync_vector_view_zoom_from_cameras(
        &mut self,
        mode: SphericalViewMode,
        projection: SphericalProjectionKind,
        map_camera: MapCamera,
        globe_camera: GlobeCamera,
    ) {
        self.vector_view_zoom = match mode {
            SphericalViewMode::Map => map_camera.zoom(projection),
            SphericalViewMode::Globe => globe_camera.orthographic_scale(),
        };
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
        diagnostics_fingerprint: diagnostics_fingerprint(diagnostics),
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
    if current.source() != &source {
        return prepare_spherical_field_layers(
            source,
            catalog,
            cell_count,
            edge_count,
            diagnostics,
            preferred_fill,
            preferred_range,
            state,
            clock,
        );
    }
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
    let next_diagnostics_fingerprint = diagnostics_fingerprint(diagnostics);
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
    let diagnostics = if diagnostics_need_preparation(
        &current.prepared_state,
        &next_state,
        current.diagnostics_fingerprint,
        next_diagnostics_fingerprint,
    ) {
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
    if vector_glyphs_need_rebuild(catalog, &current.prepared_state, &next_state) {
        revisions.vector_glyphs = candidate_clock.issue()?;
    }
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
        diagnostics_fingerprint: next_diagnostics_fingerprint,
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
    #[cfg(test)]
    record_preparation(|counts| {
        counts.diagnostic_validation_values_scanned += diagnostics.len();
    });
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

fn diagnostics_need_preparation(
    current: &PreparedLayerState,
    next: &PreparedLayerState,
    current_fingerprint: blake3::Hash,
    next_fingerprint: blake3::Hash,
) -> bool {
    current.diagnostic_scope != next.diagnostic_scope
        || current.fill_field != next.fill_field
        || current_fingerprint != next_fingerprint
}

fn diagnostics_fingerprint(diagnostics: &[OwnedViewDiagnostic]) -> blake3::Hash {
    #[cfg(test)]
    record_preparation(|counts| {
        counts.diagnostic_fingerprint_values_scanned += diagnostics.len();
    });
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.spherical-field-diagnostics.v1\0");
    hasher.update(&(diagnostics.len() as u64).to_le_bytes());
    for diagnostic in diagnostics {
        hasher.update(&[match diagnostic.severity {
            super::ViewDiagnosticSeverity::Info => 1,
            super::ViewDiagnosticSeverity::Warning => 2,
            super::ViewDiagnosticSeverity::Error => 3,
        }]);
        hash_diagnostic_text(&mut hasher, &diagnostic.code);
        match &diagnostic.field_id {
            Some(field) => {
                hasher.update(&[1]);
                hash_diagnostic_text(&mut hasher, field.namespace());
                hash_diagnostic_text(&mut hasher, field.name());
                hasher.update(&field.version().to_le_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
        match diagnostic.cell_id {
            Some(cell) => {
                hasher.update(&[1]);
                hasher.update(&cell.raw().to_le_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
        hash_diagnostic_text(&mut hasher, &diagnostic.message);
    }
    hasher.finalize()
}

fn hash_diagnostic_text(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
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

fn vector_glyphs_need_rebuild(
    catalog: &FieldCatalog<'_>,
    current: &PreparedLayerState,
    next: &PreparedLayerState,
) -> bool {
    if current.selected_cell == next.selected_cell && current.glyph_lod_key == next.glyph_lod_key {
        return false;
    }
    next.overlay_field
        .as_ref()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .is_some_and(|field| {
            classify_spherical_channel(field.schema().domain, field.schema().value_type)
                == Some(SphericalFieldChannel::VectorOverlay)
        })
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
        vector_glyphs: clock.issue()?,
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

#[cfg(test)]
mod vector_glyph_tests {
    use std::collections::BTreeSet;
    use std::f64::consts::FRAC_PI_4;
    use std::sync::Arc;

    use super::{GlyphLodKey, PreparedVectorField, PreparedVectorGlyphs, ResolvedDisplayRange};
    use crate::engine::BuildResultHash;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::view::{
        PreparedGlobeMesh, PreparedProjectedMap, SphericalMeshBudgets, SphericalPresentationSource,
        SphericalProjection, SphericalProjectionKind,
    };
    use crate::world::fields::FieldId;
    use crate::world::fields::{
        DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain,
        FieldPaletteHint, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
        MissingValuePolicy, ValueRange,
    };
    use crate::world::spatial::{canonical_east_north_basis, SurfaceRef, UnitVector3};
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    fn glyph_fixture(seed: u64, selected: CellId, lod: GlyphLodKey) -> PreparedVectorGlyphs {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let source = SphericalPresentationSource::new(
            RootSeed::new(seed),
            SurfaceRef::for_spherical(&surface),
            BuildResultHash::new([seed as u8; 32]),
            1,
        );
        let projection =
            SphericalProjection::new(SphericalProjectionKind::Equirectangular, FRAC_PI_4).unwrap();
        let map = Arc::new(
            PreparedProjectedMap::build(
                source.clone(),
                &surface,
                projection,
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap(),
        );
        let globe = Arc::new(
            PreparedGlobeMesh::build(source.clone(), &surface, SphericalMeshBudgets::DEFAULT)
                .unwrap(),
        );
        let mut components = (0..surface.cells().len())
            .map(|index| [index as f32 + 1.0, 2.0 - index as f32 * 0.01])
            .collect::<Vec<_>>();
        components[0] = [0.0, 0.0];
        let magnitudes = components
            .iter()
            .map(|value| value[0].hypot(value[1]))
            .collect();
        let field = PreparedVectorField {
            field_id: FieldId::new("test.spherical", "vectors", 1).unwrap(),
            components,
            magnitudes,
            display_range: ResolvedDisplayRange::new(0.0, 50.0).unwrap(),
        };
        PreparedVectorGlyphs::build(&source, &map, &globe, &field, Some(selected), lod).unwrap()
    }

    fn glyph_bytes(glyphs: &PreparedVectorGlyphs) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(glyphs.sampled_cells().len() as u64).to_le_bytes());
        for cell in glyphs.sampled_cells() {
            bytes.extend_from_slice(&cell.raw().to_le_bytes());
        }
        bytes.extend_from_slice(&(glyphs.map().len() as u64).to_le_bytes());
        for glyph in glyphs.map() {
            bytes.extend_from_slice(&glyph.cell().raw().to_le_bytes());
            for value in glyph
                .origin()
                .into_iter()
                .chain(glyph.direction())
                .chain(glyph.components())
                .chain([
                    glyph.magnitude(),
                    glyph.color_position(),
                    glyph.length(),
                    glyph.length_fraction(),
                ])
            {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        bytes.extend_from_slice(&(glyphs.globe().len() as u64).to_le_bytes());
        for glyph in glyphs.globe() {
            bytes.extend_from_slice(&glyph.cell().raw().to_le_bytes());
            for value in glyph.radial().components() {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            for value in glyph
                .direction()
                .into_iter()
                .chain(glyph.components())
                .chain([
                    glyph.magnitude(),
                    glyph.color_position(),
                    glyph.length(),
                    glyph.length_fraction(),
                ])
            {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn source_keyed_lod_sets_are_deterministic_nested_unique_and_include_selection() {
        for seed in [3, 17] {
            for selected in [CellId::from_raw(0), CellId::from_raw(19)] {
                let low = glyph_fixture(seed, selected, GlyphLodKey::Low);
                let repeated = glyph_fixture(seed, selected, GlyphLodKey::Low);
                let medium = glyph_fixture(seed, selected, GlyphLodKey::Medium);
                let high = glyph_fixture(seed, selected, GlyphLodKey::High);

                assert_eq!(glyph_bytes(&low), glyph_bytes(&repeated));
                assert!(low.sampled_cells().contains(&selected));
                assert!(medium.sampled_cells().contains(&selected));
                assert!(high.sampled_cells().contains(&selected));
                let low_set = low.sampled_cells().iter().copied().collect::<BTreeSet<_>>();
                let medium_set = medium
                    .sampled_cells()
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>();
                let high_set = high
                    .sampled_cells()
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>();
                assert_eq!(low_set.len(), low.sampled_cells().len());
                assert_eq!(medium_set.len(), medium.sampled_cells().len());
                assert_eq!(high_set.len(), high.sampled_cells().len());
                assert!(low_set.is_subset(&medium_set));
                assert!(medium_set.is_subset(&high_set));
            }
        }
    }

    #[test]
    fn globe_direction_uses_canonical_components_and_zero_has_no_direction_glyph() {
        let glyphs = glyph_fixture(23, CellId::from_raw(0), GlyphLodKey::High);
        assert!(glyphs.sampled_cells().contains(&CellId::from_raw(0)));
        assert!(glyphs
            .globe()
            .iter()
            .all(|glyph| glyph.cell() != CellId::from_raw(0)));
        assert!(glyphs
            .map()
            .iter()
            .all(|glyph| glyph.cell() != CellId::from_raw(0)));

        let glyph = glyphs.globe().first().unwrap();
        let (east, north) = canonical_east_north_basis(glyph.radial());
        let [x, y] = glyph.components();
        let expected = [
            east[0] * f64::from(x) + north[0] * f64::from(y),
            east[1] * f64::from(x) + north[1] * f64::from(y),
            east[2] * f64::from(x) + north[2] * f64::from(y),
        ];
        let expected_length = expected
            .into_iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        for (found, expected) in glyph.direction().into_iter().zip(expected) {
            assert!((f64::from(found) - expected / expected_length).abs() < 2.0e-6);
        }
        let expected_fraction = 0.35 + 0.65 * glyph.color_position();
        assert!((glyph.length_fraction() - expected_fraction).abs() < 1.0e-6);
        let map = glyphs
            .map()
            .iter()
            .find(|map| map.cell() == glyph.cell())
            .expect("ordinary fixture cell has a usable map Jacobian");
        assert_eq!(map.components(), glyph.components());
        assert_eq!(map.magnitude(), glyph.magnitude());
        assert_eq!(map.color_position(), glyph.color_position());
        assert!((map.length_fraction() - expected_fraction).abs() < 1.0e-6);
    }

    #[test]
    fn degenerate_map_jacobian_omits_only_map_glyph_and_emits_display_diagnostic() {
        let radial = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
        let projection =
            SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
        let prepared = super::prepare_vector_glyph_pair(
            CellId::from_raw(7),
            radial,
            [3.0, 4.0],
            5.0,
            0.5,
            0.25,
            0.5,
            projection,
        );

        assert!(prepared.map.is_none());
        assert!(prepared.globe.is_some());
        let diagnostic = prepared.diagnostic.unwrap();
        assert_eq!(diagnostic.cell_id, Some(CellId::from_raw(7)));
        assert_eq!(
            diagnostic.code,
            "display.vector_projection_jacobian_degenerate"
        );
    }

    #[test]
    fn vector_glyph_preparation_does_not_mutate_unit_globe_geometry() {
        let glyphs = glyph_fixture(31, CellId::from_raw(4), GlyphLodKey::High);
        for glyph in glyphs.globe() {
            let radius = glyph
                .radial()
                .components()
                .into_iter()
                .map(|value| value * value)
                .sum::<f64>()
                .sqrt();
            assert!((radius - 1.0).abs() < 1.0e-12);
            let dot = glyph
                .radial()
                .components()
                .into_iter()
                .zip(glyph.direction())
                .map(|(radial, direction)| radial * f64::from(direction))
                .sum::<f64>();
            assert!(dot.abs() < 2.0e-6);
        }
    }

    #[test]
    fn selected_lod_and_same_cardinality_world_changes_have_exact_revision_scope() {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let source = SphericalPresentationSource::new(
            RootSeed::new(91),
            SurfaceRef::for_spherical(&surface),
            BuildResultHash::new([91; 32]),
            1,
        );
        let cell_count = surface.cells().len();
        let edge_count = surface.edges().len();
        let fill_id = FieldId::new("test.spherical", "fill", 1).unwrap();
        let vector_id = FieldId::new("test.spherical", "vector", 1).unwrap();
        let mut registry = FieldRegistryBuilder::new();
        registry
            .register(FieldSchema {
                id: fill_id.clone(),
                domain: FieldDomain::Cells,
                value_type: FieldValueType::ScalarF32,
                unit: FieldUnit::Unitless,
                valid_range: Some(ValueRange::new(0.0, 1.0).unwrap()),
                missing: MissingValuePolicy::Forbidden,
                dependencies: Vec::new(),
                category_labels: Default::default(),
                display: FieldDisplayMetadata::new(
                    "field.test.spherical.fill",
                    FieldPaletteHint::Sequential,
                    4,
                )
                .unwrap(),
            })
            .unwrap();
        registry
            .register(FieldSchema {
                id: vector_id.clone(),
                domain: FieldDomain::Cells,
                value_type: FieldValueType::Vector2F32,
                unit: FieldUnit::Unitless,
                valid_range: None,
                missing: MissingValuePolicy::Forbidden,
                dependencies: Vec::new(),
                category_labels: Default::default(),
                display: FieldDisplayMetadata::new(
                    "field.test.spherical.vector",
                    FieldPaletteHint::Vector,
                    4,
                )
                .unwrap(),
            })
            .unwrap();
        let registry = registry.build().unwrap();
        let mut fields = ExtensionFieldSet::new();
        let sizes = DomainSizes::new(cell_count, edge_count);
        fields
            .insert(
                &registry,
                fill_id.clone(),
                FieldData::ScalarF32(vec![0.5; cell_count]),
                &sizes,
            )
            .unwrap();
        fields
            .insert(
                &registry,
                vector_id.clone(),
                FieldData::Vector2F32(vec![[1.0, 0.0]; cell_count]),
                &sizes,
            )
            .unwrap();
        let catalog = crate::view::FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
        let mut state = super::SphericalFieldDisplayState::default();
        state.select_overlay(Some(vector_id));
        let mut clock = crate::view::DisplayRevisionClock::default();
        let initial = super::prepare_spherical_field_layers(
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        super::reset_field_layer_preparation_counts();
        state.select_entity(Some(super::SelectedSurfaceEntity::Cell(CellId::from_raw(
            7,
        ))));
        let selected = super::update_spherical_field_layers(
            &initial,
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_ne!(
            initial.revisions().vector_glyphs,
            selected.revisions().vector_glyphs
        );
        assert_eq!(initial.revisions().overlay, selected.revisions().overlay);
        let (
            Some(super::PreparedSphericalOverlay::Vector(initial_field)),
            Some(super::PreparedSphericalOverlay::Vector(selected_field)),
        ) = (initial.overlay(), selected.overlay())
        else {
            panic!("fixture retains its vector overlay");
        };
        assert!(Arc::ptr_eq(initial_field, selected_field));
        assert_eq!(super::field_layer_preparation_counts().overlay, 0);

        state.set_vector_paused(true);
        state.set_vector_display_speed(3.0).unwrap();
        let paused = super::update_spherical_field_layers(
            &selected,
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(selected.revisions(), paused.revisions());

        state.set_vector_lod(super::VectorGlyphLod::Low);
        state.set_vector_view_zoom(1.0).unwrap();
        let low = super::update_spherical_field_layers(
            &paused,
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_ne!(
            paused.revisions().vector_glyphs,
            low.revisions().vector_glyphs
        );
        assert_eq!(low.glyph_lod_key(), GlyphLodKey::Low);
        assert_eq!(paused.revisions().overlay, low.revisions().overlay);

        state.set_vector_view_zoom(1.99).unwrap();
        let low_in_band = super::update_spherical_field_layers(
            &low,
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(low.revisions(), low_in_band.revisions());
        assert_eq!(low_in_band.glyph_lod_key(), GlyphLodKey::Low);

        state.set_vector_view_zoom(2.0).unwrap();
        let medium = super::update_spherical_field_layers(
            &low_in_band,
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_ne!(
            low_in_band.revisions().vector_glyphs,
            medium.revisions().vector_glyphs
        );
        assert_eq!(medium.glyph_lod_key(), GlyphLodKey::Medium);
        assert_eq!(low_in_band.revisions().overlay, medium.revisions().overlay);

        state.set_vector_view_zoom(2.5).unwrap();
        let medium_in_band = super::update_spherical_field_layers(
            &medium,
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(medium.revisions(), medium_in_band.revisions());

        state.set_vector_view_zoom(4.0).unwrap();
        let high = super::update_spherical_field_layers(
            &medium_in_band,
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_ne!(
            medium_in_band.revisions().vector_glyphs,
            high.revisions().vector_glyphs
        );
        assert_eq!(high.glyph_lod_key(), GlyphLodKey::High);
        assert_eq!(medium_in_band.revisions().overlay, high.revisions().overlay);

        let repeated_high = super::update_spherical_field_layers(
            &high,
            source.clone(),
            &catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id.clone()),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();
        assert_eq!(high.revisions(), repeated_high.revisions());

        let mut changed_fields = ExtensionFieldSet::new();
        changed_fields
            .insert(
                &registry,
                fill_id.clone(),
                FieldData::ScalarF32(vec![0.75; cell_count]),
                &DomainSizes::new(cell_count, edge_count),
            )
            .unwrap();
        let vector_id = state.overlay_field().unwrap().clone();
        changed_fields
            .insert(
                &registry,
                vector_id,
                FieldData::Vector2F32(vec![[0.0, 2.0]; cell_count]),
                &DomainSizes::new(cell_count, edge_count),
            )
            .unwrap();
        let changed_catalog =
            crate::view::FieldCatalog::from_extension_fields(&registry, &changed_fields).unwrap();
        let changed_source = SphericalPresentationSource::new(
            RootSeed::new(92),
            SurfaceRef::for_spherical(&surface),
            BuildResultHash::new([92; 32]),
            1,
        );
        let world_changed = super::update_spherical_field_layers(
            &repeated_high,
            changed_source.clone(),
            &changed_catalog,
            cell_count,
            edge_count,
            &[],
            Some(fill_id),
            |_| Some(crate::view::DisplayRangeMode::Data),
            &mut state,
            &mut clock,
        )
        .unwrap();

        assert_eq!(world_changed.source(), &changed_source);
        assert_eq!(f32::from_bits(world_changed.fill().raw_values()[0]), 0.75);
        let Some(super::PreparedSphericalOverlay::Vector(world_vector)) = world_changed.overlay()
        else {
            panic!("changed world retains its vector overlay");
        };
        assert_eq!(world_vector.components()[0], [0.0, 2.0]);
        assert!(!Arc::ptr_eq(
            repeated_high.fill_arc(),
            world_changed.fill_arc()
        ));
        assert!(!Arc::ptr_eq(
            repeated_high.diagnostics_arc(),
            world_changed.diagnostics_arc()
        ));
        assert!(!Arc::ptr_eq(
            repeated_high.fill_palette_arc(),
            world_changed.fill_palette_arc()
        ));
        assert!(
            repeated_high.revisions().fill != world_changed.revisions().fill
                && repeated_high.revisions().overlay != world_changed.revisions().overlay
                && repeated_high.revisions().diagnostics != world_changed.revisions().diagnostics
                && repeated_high.revisions().fill_palette != world_changed.revisions().fill_palette
                && repeated_high.revisions().overlay_palette
                    != world_changed.revisions().overlay_palette
                && repeated_high.revisions().vector_glyphs
                    != world_changed.revisions().vector_glyphs
        );
    }
}
