use sekai::view::{
    classify_spherical_channel, DiagnosticScope, DisplayRangeMode, FieldLayerError,
    SelectedSurfaceEntity, SphericalFieldChannel, SphericalFieldDisplayState, VectorGlyphLod,
};
use sekai::world::fields::{FieldDomain, FieldValueType};
use sekai::world::natural::{
    boundary_kind_field_id, plate_velocity_field_id, surface_elevation_m_field_id,
};
use sekai::world::{CellId, EdgeId};

#[test]
fn exact_supported_domain_type_pairs_map_to_display_channels() {
    assert_eq!(
        classify_spherical_channel(FieldDomain::Cells, FieldValueType::ScalarF32),
        Some(SphericalFieldChannel::CellFill)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Cells, FieldValueType::CategoryU32),
        Some(SphericalFieldChannel::CellFill)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Edges, FieldValueType::ScalarF32),
        Some(SphericalFieldChannel::EdgeOverlay)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Edges, FieldValueType::CategoryU32),
        Some(SphericalFieldChannel::EdgeOverlay)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Cells, FieldValueType::Vector2F32),
        Some(SphericalFieldChannel::VectorOverlay)
    );
    assert_eq!(
        classify_spherical_channel(FieldDomain::Edges, FieldValueType::Vector2F32),
        None
    );
}

#[test]
fn spherical_state_preserves_fill_overlay_and_stable_entity_independently() {
    let mut state = SphericalFieldDisplayState::default();
    state.select_fill(surface_elevation_m_field_id());
    state.select_overlay(Some(plate_velocity_field_id()));
    state.select_entity(Some(SelectedSurfaceEntity::Cell(CellId::from_raw(7))));
    state.set_vector_lod(VectorGlyphLod::Medium);
    state.set_vector_paused(false);
    state.set_vector_display_speed(1.5).unwrap();

    assert_eq!(state.fill_field(), Some(&surface_elevation_m_field_id()));
    assert_eq!(state.overlay_field(), Some(&plate_velocity_field_id()));
    assert_eq!(
        state.selected_entity(),
        Some(SelectedSurfaceEntity::Cell(CellId::from_raw(7)))
    );
    assert_eq!(state.vector_lod(), VectorGlyphLod::Medium);
    assert!(!state.vector_paused());
    assert_eq!(state.vector_display_speed(), 1.5);

    state.select_overlay(Some(boundary_kind_field_id()));
    state.select_entity(Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(3))));
    assert_eq!(state.overlay_field(), Some(&boundary_kind_field_id()));
    assert_eq!(
        state.selected_entity(),
        Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(3)))
    );
}

#[test]
fn spherical_state_defaults_to_schema_driven_display_preferences() {
    let state = SphericalFieldDisplayState::default();

    assert_eq!(state.fill_field(), None);
    assert_eq!(state.overlay_field(), None);
    assert_eq!(state.range_mode(), DisplayRangeMode::Data);
    assert_eq!(state.palette_override(), None);
    assert!(state.diagnostics_enabled());
    assert_eq!(state.diagnostic_scope(), DiagnosticScope::SelectedField);
    assert_eq!(state.selected_entity(), None);
    assert_eq!(state.vector_lod(), VectorGlyphLod::Medium);
    assert!(!state.vector_paused());
    assert_eq!(state.vector_display_speed(), 1.0);
}

#[test]
fn vector_display_speed_rejects_non_finite_and_out_of_range_values() {
    let mut state = SphericalFieldDisplayState::default();

    for speed in [f32::NAN, f32::INFINITY, -0.1, 4.1] {
        assert!(matches!(
            state.set_vector_display_speed(speed),
            Err(FieldLayerError::InvalidVectorDisplaySpeed(_))
        ));
        assert_eq!(state.vector_display_speed(), 1.0);
    }

    state.set_vector_display_speed(0.0).unwrap();
    assert_eq!(state.vector_display_speed(), 0.0);
    state.set_vector_display_speed(4.0).unwrap();
    assert_eq!(state.vector_display_speed(), 4.0);
}
