use sekai::view::{
    classify_spherical_channel, DiagnosticScope, DisplayRangeMode, FieldLayerError, GlyphLodKey,
    PreparedOverlayKind, SelectedSurfaceEntity, SphericalFieldChannel, SphericalFieldDisplayState,
    VectorAnimationUniform, VectorGlyphLod,
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
fn spherical_packets_distinguish_edge_and_vector_overlay_kinds() {
    assert_eq!(
        PreparedOverlayKind::EdgeScalar,
        PreparedOverlayKind::EdgeScalar
    );
    assert_ne!(
        PreparedOverlayKind::EdgeCategory,
        PreparedOverlayKind::CellVector
    );
}

#[test]
fn spherical_state_preserves_fill_overlay_and_stable_entity_independently() {
    let mut state = SphericalFieldDisplayState::default();
    state.select_fill(surface_elevation_m_field_id());
    state.select_overlay(Some(plate_velocity_field_id()));
    state.select_entity(Some(SelectedSurfaceEntity::Cell(CellId::from_raw(7))));
    state.set_vector_lod(VectorGlyphLod::Medium);
    state.set_vector_view_zoom(1.75).unwrap();
    state.set_vector_paused(false);
    state.set_vector_display_speed(1.5).unwrap();

    assert_eq!(state.fill_field(), Some(&surface_elevation_m_field_id()));
    assert_eq!(state.overlay_field(), Some(&plate_velocity_field_id()));
    assert_eq!(
        state.selected_entity(),
        Some(SelectedSurfaceEntity::Cell(CellId::from_raw(7)))
    );
    assert_eq!(state.vector_lod(), VectorGlyphLod::Medium);
    assert_eq!(state.vector_view_zoom(), 1.75);
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
    assert_eq!(state.vector_view_zoom(), 1.0);
    assert!(!state.vector_paused());
    assert_eq!(state.vector_display_speed(), 1.0);
}

#[test]
fn vector_view_zoom_rejects_non_positive_and_non_finite_values_atomically() {
    let mut state = SphericalFieldDisplayState::default();

    for zoom in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        assert!(matches!(
            state.set_vector_view_zoom(zoom),
            Err(FieldLayerError::InvalidVectorViewZoom(_))
        ));
        assert_eq!(state.vector_view_zoom(), 1.0);
    }
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

#[test]
fn glyph_lod_keys_keep_the_exact_nested_sampling_denominators() {
    assert_eq!(GlyphLodKey::Low.denominator(), 16);
    assert_eq!(GlyphLodKey::Medium.denominator(), 8);
    assert_eq!(GlyphLodKey::High.denominator(), 4);

    for score in 0_u64..256 {
        let low = GlyphLodKey::Low.includes_score(score);
        let medium = GlyphLodKey::Medium.includes_score(score);
        let high = GlyphLodKey::High.includes_score(score);
        assert!(!low || medium, "low score {score} must remain in medium");
        assert!(!medium || high, "medium score {score} must remain in high");
    }
}

#[test]
fn zoom_lod_changes_only_at_predefined_discrete_thresholds() {
    assert_eq!(
        GlyphLodKey::for_zoom(VectorGlyphLod::Low, 1.0),
        GlyphLodKey::Low
    );
    assert_eq!(
        GlyphLodKey::for_zoom(VectorGlyphLod::Low, 1.99),
        GlyphLodKey::Low
    );
    assert_eq!(
        GlyphLodKey::for_zoom(VectorGlyphLod::Low, 2.0),
        GlyphLodKey::Medium
    );
    assert_eq!(
        GlyphLodKey::for_zoom(VectorGlyphLod::Low, 4.0),
        GlyphLodKey::High
    );
    assert_eq!(
        GlyphLodKey::for_zoom(VectorGlyphLod::Medium, 1.99),
        GlyphLodKey::Medium
    );
    assert_eq!(
        GlyphLodKey::for_zoom(VectorGlyphLod::Medium, 2.0),
        GlyphLodKey::High
    );
    assert_eq!(
        GlyphLodKey::for_zoom(VectorGlyphLod::High, 0.5),
        GlyphLodKey::High
    );
}

#[test]
fn vector_animation_phase_is_bounded_display_state_not_physical_time() {
    let mut animation = VectorAnimationUniform::new(0.9);
    animation.advance(0.25, 2.0, false);
    assert!((animation.phase() - 0.4).abs() < 1.0e-6);

    let paused = animation;
    animation.advance(1000.0, 4.0, true);
    assert_eq!(animation, paused);

    animation.advance(1.0, 40.0, false);
    assert!((animation.phase() - 0.4).abs() < 1.0e-6);
    assert_eq!(animation.display_semantics(), "display-only");
}
