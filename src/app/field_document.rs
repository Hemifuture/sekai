use std::sync::Arc;

use crate::ui::field::FieldControlAction;
use crate::view::{
    built_in_palette, prepare_cell_field, resolve_display_range, DisplayPrepareError,
    DisplayRangeMode, DisplayRevisionClock, DisplayRevisions, FieldCatalog, FieldDisplayState,
    FieldView, FieldViewError, LinearRgba, OwnedViewDiagnostic, PaletteId, PreparedCellField,
    PreparedCellMesh, PreparedDiagnosticMask, PreparedFieldDisplay,
};
use crate::world::fields::{FieldId, FieldPaletteHint};

/// Private application boundary shared by legacy and formal world documents.
pub(super) trait AppFieldDocument {
    fn mesh(&self) -> &Arc<PreparedCellMesh>;
    fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError>;
    fn diagnostics(&self) -> &[OwnedViewDiagnostic];
    fn preferred_field(&self) -> Option<FieldId>;
    fn preferred_range(&self, field: &FieldId) -> Option<DisplayRangeMode>;
}

struct PreparedDisplayParts {
    field: Arc<PreparedCellField>,
    diagnostics: Arc<PreparedDiagnosticMask>,
    palette: Arc<[LinearRgba]>,
}

/// Prepares one complete candidate without mutating the currently published document.
pub(super) fn prepare_new_document_display(
    document: &dyn AppFieldDocument,
    current_state: &FieldDisplayState,
    clock: &mut DisplayRevisionClock,
) -> Result<(FieldDisplayState, Arc<PreparedFieldDisplay>), DisplayPrepareError> {
    let catalog = document.catalog()?;
    let mut state = current_state.clone();
    let retained_selection = state
        .selected_field()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .is_some_and(|view| view.cell_fill_kind().is_ok());
    if !retained_selection {
        if let Some(preferred) = document.preferred_field() {
            state.select_field(preferred);
        }
    }
    state.reconcile(&catalog, document.mesh().cell_count());
    if !retained_selection {
        if let Some(mode) = state
            .selected_field()
            .and_then(|field| document.preferred_range(field))
        {
            state.set_range_mode(mode);
        }
    }

    let parts = prepare_display_parts(document, &catalog, &state)?;
    let revisions = issue_all_revisions(clock)?;
    let packet = Arc::new(PreparedFieldDisplay::new(
        document.mesh().clone(),
        parts.field,
        parts.diagnostics,
        parts.palette,
        revisions,
        state.diagnostics_enabled(),
    )?);
    Ok((state, packet))
}

pub(super) fn prepare_control_action(
    document: &dyn AppFieldDocument,
    current: &PreparedFieldDisplay,
    state: &mut FieldDisplayState,
    clock: &mut DisplayRevisionClock,
    action: FieldControlAction,
) -> Result<Arc<PreparedFieldDisplay>, DisplayPrepareError> {
    let catalog = document.catalog()?;
    match action {
        FieldControlAction::InspectField(_) => {
            unreachable!("inspection actions are handled without rebuilding the packet")
        }
        FieldControlAction::SelectField(field) => {
            state.select_field(field);
            state.reconcile(&catalog, document.mesh().cell_count());
            if let Some(mode) = state
                .selected_field()
                .and_then(|field| document.preferred_range(field))
            {
                state.set_range_mode(mode);
            }
            let parts = prepare_display_parts(document, &catalog, state)?;
            rebuild_changed_packet(current, parts, state.diagnostics_enabled(), clock)
        }
        FieldControlAction::SetRangeMode(mode) => {
            state.set_range_mode(mode);
            let Some(view) = selected_field_view(&catalog, state) else {
                return Err(DisplayPrepareError::NoRenderableField);
            };
            if view.scalar_values().is_none() {
                return Ok(Arc::new(current.clone()));
            }
            let range = resolve_display_range(view, state.range_mode())?;
            Ok(Arc::new(current.with_display_range(range)))
        }
        FieldControlAction::SetPaletteOverride(palette) => {
            state.set_palette_override(palette);
            state.reconcile(&catalog, document.mesh().cell_count());
            let palette = prepare_palette(&catalog, state)?;
            let mut revisions = current.revisions();
            let palette = if current.palette() == palette.as_ref() {
                current.palette_arc().clone()
            } else {
                revisions.palette = clock.issue()?;
                palette
            };
            Ok(Arc::new(PreparedFieldDisplay::new(
                current.mesh_arc().clone(),
                current.field_arc().clone(),
                current.diagnostics_arc().clone(),
                palette,
                revisions,
                current.diagnostics_enabled(),
            )?))
        }
        FieldControlAction::SetDiagnosticsEnabled(enabled) => {
            state.set_diagnostics_enabled(enabled);
            Ok(Arc::new(current.with_diagnostics_enabled(enabled)))
        }
        FieldControlAction::SetDiagnosticScope(scope) => {
            state.set_diagnostic_scope(scope);
            let diagnostics = prepare_diagnostics(document, state)?;
            let mut revisions = current.revisions();
            let diagnostics = if current.diagnostics() == diagnostics.as_ref() {
                current.diagnostics_arc().clone()
            } else {
                revisions.diagnostics = clock.issue()?;
                diagnostics
            };
            Ok(Arc::new(PreparedFieldDisplay::new(
                current.mesh_arc().clone(),
                current.field_arc().clone(),
                diagnostics,
                current.palette_arc().clone(),
                revisions,
                current.diagnostics_enabled(),
            )?))
        }
    }
}

fn prepare_display_parts(
    document: &dyn AppFieldDocument,
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
) -> Result<PreparedDisplayParts, DisplayPrepareError> {
    let view = selected_field_view(catalog, state).ok_or(DisplayPrepareError::NoRenderableField)?;
    let field = Arc::new(prepare_cell_field(
        view,
        document.mesh().cell_count(),
        state.range_mode(),
    )?);
    let diagnostics = prepare_diagnostics(document, state)?;
    let palette = prepare_palette(catalog, state)?;
    Ok(PreparedDisplayParts {
        field,
        diagnostics,
        palette,
    })
}

fn selected_field_view<'catalog, 'data>(
    catalog: &'catalog FieldCatalog<'data>,
    state: &FieldDisplayState,
) -> Option<&'catalog FieldView<'data>> {
    state
        .selected_field()
        .and_then(|field| catalog.get(field))
        .and_then(|entry| entry.view())
        .filter(|view| view.cell_fill_kind().is_ok())
}

fn prepare_diagnostics(
    document: &dyn AppFieldDocument,
    state: &FieldDisplayState,
) -> Result<Arc<PreparedDiagnosticMask>, DisplayPrepareError> {
    Ok(Arc::new(PreparedDiagnosticMask::build(
        document.mesh().cell_count(),
        document
            .diagnostics()
            .iter()
            .map(OwnedViewDiagnostic::as_ref),
        state.selected_field(),
        state.diagnostic_scope(),
    )?))
}

fn prepare_palette(
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
) -> Result<Arc<[LinearRgba]>, DisplayPrepareError> {
    let schema = state
        .selected_field()
        .and_then(|field| catalog.get(field))
        .map(|entry| entry.schema())
        .ok_or(DisplayPrepareError::NoRenderableField)?;
    let schema_palette = match schema.display.palette() {
        FieldPaletteHint::Sequential => PaletteId::Sequential,
        FieldPaletteHint::Diverging => PaletteId::Diverging,
        FieldPaletteHint::Categorical => PaletteId::Categorical,
        FieldPaletteHint::Boolean | FieldPaletteHint::Vector => {
            return Err(DisplayPrepareError::UnsupportedCellFill {
                field: schema.id.clone(),
            });
        }
    };
    let palette = state.palette_override().unwrap_or(schema_palette);
    Ok(Arc::from(built_in_palette(palette)))
}

fn rebuild_changed_packet(
    current: &PreparedFieldDisplay,
    parts: PreparedDisplayParts,
    diagnostics_enabled: bool,
    clock: &mut DisplayRevisionClock,
) -> Result<Arc<PreparedFieldDisplay>, DisplayPrepareError> {
    let mut revisions = current.revisions();
    let field = if current.field() == parts.field.as_ref() {
        current.field_arc().clone()
    } else {
        revisions.field = clock.issue()?;
        parts.field
    };
    let diagnostics = if current.diagnostics() == parts.diagnostics.as_ref() {
        current.diagnostics_arc().clone()
    } else {
        revisions.diagnostics = clock.issue()?;
        parts.diagnostics
    };
    let palette = if current.palette() == parts.palette.as_ref() {
        current.palette_arc().clone()
    } else {
        revisions.palette = clock.issue()?;
        parts.palette
    };
    Ok(Arc::new(PreparedFieldDisplay::new(
        current.mesh_arc().clone(),
        field,
        diagnostics,
        palette,
        revisions,
        diagnostics_enabled,
    )?))
}

fn issue_all_revisions(
    clock: &mut DisplayRevisionClock,
) -> Result<DisplayRevisions, DisplayPrepareError> {
    Ok(DisplayRevisions::new(
        clock.issue()?,
        clock.issue()?,
        clock.issue()?,
        clock.issue()?,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{prepare_control_action, prepare_new_document_display, AppFieldDocument};
    use crate::app::natural_display::LegacyPlanarNaturalFieldDocument;
    use crate::ui::field::FieldControlAction;
    use crate::view::{
        DisplayRangeMode, DisplayRevisionClock, FieldCatalog, FieldDisplayState, FieldViewError,
        OwnedViewDiagnostic, PreparedCellMesh,
    };
    use crate::world::fields::FieldId;
    use crate::world::natural::{fluvial_erosion_depth_m_field_id, plate_id_field_id};

    struct InvalidDocument {
        mesh: Arc<PreparedCellMesh>,
    }

    impl AppFieldDocument for InvalidDocument {
        fn mesh(&self) -> &Arc<PreparedCellMesh> {
            &self.mesh
        }

        fn catalog(&self) -> Result<FieldCatalog<'_>, FieldViewError> {
            Err(FieldViewError::UnknownPayload {
                field: FieldId::new("test.app", "invalid", 1).unwrap(),
            })
        }

        fn diagnostics(&self) -> &[OwnedViewDiagnostic] {
            &[]
        }

        fn preferred_field(&self) -> Option<FieldId> {
            None
        }

        fn preferred_range(&self, _field: &FieldId) -> Option<crate::view::DisplayRangeMode> {
            None
        }
    }

    #[test]
    fn switching_fields_reuses_mesh_and_untouched_buffers() {
        let document = LegacyPlanarNaturalFieldDocument::test_fixture();
        let mut clock = DisplayRevisionClock::default();
        let (mut state, initial) =
            prepare_new_document_display(&document, &FieldDisplayState::default(), &mut clock)
                .unwrap();

        let switched = prepare_control_action(
            &document,
            &initial,
            &mut state,
            &mut clock,
            FieldControlAction::SelectField(plate_id_field_id()),
        )
        .unwrap();

        assert!(Arc::ptr_eq(initial.mesh_arc(), switched.mesh_arc()));
        assert!(Arc::ptr_eq(
            initial.diagnostics_arc(),
            switched.diagnostics_arc()
        ));
        assert!(!Arc::ptr_eq(initial.field_arc(), switched.field_arc()));
        assert!(!Arc::ptr_eq(initial.palette_arc(), switched.palette_arc()));
    }

    #[test]
    fn switching_to_a_process_field_uses_its_document_preferred_range() {
        let document = LegacyPlanarNaturalFieldDocument::test_fixture();
        let mut clock = DisplayRevisionClock::default();
        let (mut state, initial) =
            prepare_new_document_display(&document, &FieldDisplayState::default(), &mut clock)
                .unwrap();

        let switched = prepare_control_action(
            &document,
            &initial,
            &mut state,
            &mut clock,
            FieldControlAction::SelectField(fluvial_erosion_depth_m_field_id()),
        )
        .unwrap();

        assert_eq!(state.range_mode(), DisplayRangeMode::Data);
        let values = document
            .hydro_erosion
            .snapshot()
            .surface()
            .erosion_depth_m();
        let expected_max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(
            switched.field().display_range().unwrap().bounds(),
            (0.0, expected_max)
        );
    }

    #[test]
    fn failed_candidate_leaves_current_state_packet_and_clock_unchanged() {
        let current_document = LegacyPlanarNaturalFieldDocument::test_fixture();
        let mut clock = DisplayRevisionClock::default();
        let (current_state, current_packet) = prepare_new_document_display(
            &current_document,
            &FieldDisplayState::default(),
            &mut clock,
        )
        .unwrap();
        let invalid = InvalidDocument {
            mesh: current_document.mesh().clone(),
        };
        let state_before = current_state.clone();
        let packet_before = current_packet.clone();
        let mut candidate_clock = clock.clone();
        let mut expected_clock = clock.clone();

        assert!(
            prepare_new_document_display(&invalid, &current_state, &mut candidate_clock).is_err()
        );
        assert_eq!(current_state, state_before);
        assert!(Arc::ptr_eq(&current_packet, &packet_before));
        assert_eq!(
            candidate_clock.issue().unwrap().get(),
            expected_clock.issue().unwrap().get()
        );
    }
}
