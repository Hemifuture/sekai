//! Renderer-neutral, read-only world presentation contracts.

mod diagnostics;
mod field;
mod field_layers;
mod mesh;
mod palette;
mod prepared;
mod reference;
mod resident;
mod spherical_camera;
mod spherical_mesh;
mod spherical_picking;
mod spherical_projection;
mod spherical_source;
mod state;

pub use diagnostics::{
    CellDiagnosticRef, DiagnosticScope, OwnedViewDiagnostic, PreparedDiagnosticMask,
    ViewDiagnosticSeverity,
};
pub use field::{
    CellFillKind, FieldCatalog, FieldCatalogEntry, FieldPayloadRef, FieldValue, FieldView,
    FieldViewError,
};
pub use field_layers::{
    classify_spherical_channel, prepare_edge_field, prepare_spherical_field_layers,
    prepare_vector_field, update_spherical_field_layers, FieldLayerError, FieldLayerRevisions,
    GlobeVectorGlyph, GlyphLodKey, MapVectorGlyph, PreparedEdgeField, PreparedFieldLayers,
    PreparedOverlayKind, PreparedSphericalOverlay, PreparedVectorField, PreparedVectorGlyphs,
    SelectedSurfaceEntity, SphericalFieldChannel, SphericalFieldDisplayState,
    SphericalLayerVisibility, VectorAnimationUniform, VectorGlyphLod,
};
#[cfg(test)]
pub(crate) use field_layers::{
    field_layer_preparation_counts, reset_field_layer_preparation_counts,
    FieldLayerPreparationCounts,
};
pub(crate) use field_layers::{prepare_globe_vector_glyphs, prepare_map_vector_glyphs};
pub use mesh::{
    CellGeometrySource, DisplayVertex, MeshCompleteness, PreparedCellMesh, MAX_DISPLAY_CELLS,
    MAX_DISPLAY_INDICES, MAX_DISPLAY_VERTICES,
};
pub use palette::{
    built_in_palette, category_color, prepare_cell_field, resolve_display_range, sample_palette,
    scalar_color, DisplayPrepareError, DisplayRangeMode, LinearRgba, PaletteId, PreparedCellField,
    PreparedFieldKind, ResolvedDisplayRange, DIAGNOSTIC_ERROR_COLOR, DIAGNOSTIC_INFO_COLOR,
    DIAGNOSTIC_WARNING_COLOR,
};
pub use prepared::{
    DisplayRevision, DisplayRevisionClock, DisplayRevisions, DisplayStatusError,
    FieldDisplayResourceState, PreparedFieldDisplay,
};
pub use reference::{rasterize_reference, ReferenceImage};
pub use resident::ResidentBytesError;
pub use spherical_camera::{
    GlobeCamera, MapCamera, SphericalPresentationViewState, SphericalViewMode,
};
pub use spherical_mesh::{
    GlobeVertex, PreparedGlobeMesh, PreparedProjectedMap, ProjectedEdgeSegment, ProjectedMapVertex,
    SphericalMeshBudgets, SphericalMeshError,
};
pub use spherical_picking::{
    intersect_unit_sphere, RayError, RaySphereHit, SphericalEntityLocator, SphericalPickingError,
    UnitRay,
};
pub use spherical_projection::{
    ProjectedDirection, ProjectionBounds, ProjectionPoint, SphericalProjection,
    SphericalProjectionError, SphericalProjectionKind,
};
pub use spherical_source::SphericalPresentationSource;
pub use state::{format_field_value, FieldDisplayState, FormattedFieldValue};
