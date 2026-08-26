use std::sync::Arc;

use crate::engine::{BuildReport, DiagnosticSeverity};
use crate::view::{
    prepare_spherical_field_layers, update_spherical_field_layers, DisplayPrepareError,
    DisplayRangeMode, DisplayRevisionClock, FieldCatalog, FieldViewError, GlobeCamera, MapCamera,
    OwnedViewDiagnostic, PreparedFieldLayers, SphericalFieldDisplayState,
    SphericalPresentationSource, SphericalProjectionKind, SphericalViewMode,
    ViewDiagnosticSeverity,
};
use crate::world::fields::FieldId;

/// Renderer-independent field-data boundary shared by formal world documents.
pub(super) trait FieldDocument {
    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError>;
    fn diagnostics(&self) -> &[OwnedViewDiagnostic];
    fn preferred_field(&self) -> Option<FieldId>;
    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode>;
}

/// Field document metadata needed to build geometry-free spherical field layers.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) trait SphericalFieldLayerDocument: FieldDocument {
    fn presentation_source(&self) -> SphericalPresentationSource;
    fn spherical_cell_count(&self) -> usize;
    fn spherical_edge_count(&self) -> usize;
}

/// Prepares the shared spherical field packet directly from its owning document.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn prepare_spherical_document_layers<D: SphericalFieldLayerDocument + ?Sized>(
    document: &D,
    mode: SphericalViewMode,
    projection: SphericalProjectionKind,
    map_camera: MapCamera,
    globe_camera: GlobeCamera,
    state: &mut SphericalFieldDisplayState,
    clock: &mut DisplayRevisionClock,
) -> Result<PreparedFieldLayers, DisplayPrepareError> {
    let mut candidate_state = state.clone();
    candidate_state.sync_vector_view_zoom_from_cameras(mode, projection, map_camera, globe_camera);
    let catalog = document.catalog()?;
    let layers = prepare_spherical_field_layers(
        document.presentation_source(),
        &catalog,
        document.spherical_cell_count(),
        document.spherical_edge_count(),
        document.diagnostics(),
        document.preferred_field(),
        |field| document.preferred_range(field),
        &mut candidate_state,
        clock,
    )?;
    *state = candidate_state;
    Ok(layers)
}

/// Reconciles shared spherical field layers after consuming the active camera's real zoom.
#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
pub(super) fn update_spherical_document_layers<D: SphericalFieldLayerDocument + ?Sized>(
    document: &D,
    current: &PreparedFieldLayers,
    mode: SphericalViewMode,
    projection: SphericalProjectionKind,
    map_camera: MapCamera,
    globe_camera: GlobeCamera,
    state: &mut SphericalFieldDisplayState,
    clock: &mut DisplayRevisionClock,
) -> Result<PreparedFieldLayers, DisplayPrepareError> {
    let mut candidate_state = state.clone();
    candidate_state.sync_vector_view_zoom_from_cameras(mode, projection, map_camera, globe_camera);
    let catalog = document.catalog()?;
    let layers = update_spherical_field_layers(
        current,
        document.presentation_source(),
        &catalog,
        document.spherical_cell_count(),
        document.spherical_edge_count(),
        document.diagnostics(),
        document.preferred_field(),
        |field| document.preferred_range(field),
        &mut candidate_state,
        clock,
    )?;
    *state = candidate_state;
    Ok(layers)
}

/// Reconciles a camera-only event without scanning document data inside one LOD band.
///
/// The raw active-camera zoom is always published. Catalog construction and full layer
/// reconciliation are deferred while source identity, layer-bearing state, and effective glyph
/// density remain unchanged, so callers retain the exact outer packet identity on the ordinary
/// camera fast path. Source or pending non-camera state changes fall through to the general path.
#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
pub(super) fn reconcile_spherical_document_camera<D: SphericalFieldLayerDocument + ?Sized>(
    document: &D,
    current: &Arc<PreparedFieldLayers>,
    mode: SphericalViewMode,
    projection: SphericalProjectionKind,
    map_camera: MapCamera,
    globe_camera: GlobeCamera,
    state: &mut SphericalFieldDisplayState,
    clock: &mut DisplayRevisionClock,
) -> Result<Arc<PreparedFieldLayers>, DisplayPrepareError> {
    let mut candidate_state = state.clone();
    candidate_state.sync_vector_view_zoom_from_cameras(mode, projection, map_camera, globe_camera);
    if current.source() == &document.presentation_source()
        && current.matches_camera_only_state(&candidate_state)
    {
        *state = candidate_state;
        return Ok(Arc::clone(current));
    }

    update_spherical_document_layers(
        document,
        current,
        mode,
        projection,
        map_camera,
        globe_camera,
        state,
        clock,
    )
    .map(Arc::new)
}

/// Copies engine diagnostics into renderer-independent, document-owned values.
pub(super) fn owned_view_diagnostics(report: &BuildReport) -> Vec<OwnedViewDiagnostic> {
    report
        .diagnostics()
        .iter()
        .map(|diagnostic| OwnedViewDiagnostic {
            severity: match diagnostic.severity() {
                DiagnosticSeverity::Info => ViewDiagnosticSeverity::Info,
                DiagnosticSeverity::Warning => ViewDiagnosticSeverity::Warning,
                DiagnosticSeverity::Error => ViewDiagnosticSeverity::Error,
            },
            code: diagnostic.code().to_owned(),
            field_id: diagnostic.context().field_id.clone(),
            cell_id: diagnostic.context().cell_id,
            message: diagnostic.message().to_owned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::FieldDocument;
    use crate::view::{DisplayRangeMode, FieldCatalog, FieldPayloadRef, FieldViewError};
    use crate::world::fields::{FieldId, FieldRegistry};
    use crate::world::natural::{natural_field_registry, surface_elevation_m_field_id};

    struct DataOnlyDocument {
        registry: FieldRegistry,
        surface_elevation_m: Vec<f32>,
    }

    impl DataOnlyDocument {
        fn new() -> Self {
            Self {
                registry: natural_field_registry(12).unwrap(),
                surface_elevation_m: vec![-100.0, 250.0],
            }
        }
    }

    impl FieldDocument for DataOnlyDocument {
        fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
            FieldCatalog::from_payloads(
                &self.registry,
                [(
                    surface_elevation_m_field_id(),
                    FieldPayloadRef::ScalarF32(&self.surface_elevation_m),
                )],
            )
        }

        fn diagnostics(&self) -> &[crate::view::OwnedViewDiagnostic] {
            &[]
        }

        fn preferred_field(&self) -> Option<FieldId> {
            Some(surface_elevation_m_field_id())
        }

        fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode> {
            (field == &surface_elevation_m_field_id()).then_some(DisplayRangeMode::Data)
        }
    }

    #[test]
    fn data_only_documents_expose_fields_without_a_presentation_mesh() {
        let document = DataOnlyDocument::new();
        let catalog = document.catalog().unwrap();
        let elevation = catalog
            .get(&surface_elevation_m_field_id())
            .unwrap()
            .view()
            .unwrap()
            .scalar_values()
            .unwrap();

        assert_eq!(elevation, [-100.0, 250.0]);
        assert!(document.diagnostics().is_empty());
        assert_eq!(
            document.preferred_field(),
            Some(surface_elevation_m_field_id())
        );
        assert_eq!(
            document.preferred_range(&surface_elevation_m_field_id()),
            Some(DisplayRangeMode::Data)
        );
    }
}
