use std::sync::{mpsc, Arc};

use sekai::app::{
    build_spherical_external_artifacts, build_spherical_presentation_candidate, AppRuntimeGraph,
    PersistedWorldOrigin, PublishedSphericalPresentation, SphericalGlobePresenter,
    SphericalMapPresenter, SphericalPresentationError, SphericalRendererPreparer,
};
use sekai::engine::{ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, HydroErosionSpecArtifact,
    ReliefSpecArtifact, RulePackSetArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SphericalSpaceArtifact};
use sekai::view::{
    DisplayRevisionClock, GlobeCamera, MapCamera, SelectedSurfaceEntity,
    SphericalFieldDisplayState, SphericalLayerVisibility, SphericalMeshBudgets,
    SphericalProjection, SphericalProjectionKind, SphericalViewMode, VectorGlyphLod,
};
use sekai::world::fields::FieldId;
use sekai::world::natural::{
    boundary_kind_field_id, boundary_strength_field_id, land_ocean_field_id,
    plate_velocity_field_id, preliminary_prevailing_wind_m_s_field_id,
    surface_elevation_m_field_id, GeologicSpec, ReliefSpec, TectonicSpec, WorldFormationSpec,
};
use sekai::world::{CellId, EdgeId, Meters, RootSeed, SphericalSpaceSpec};
use sekai::{
    ui::spherical::{
        apply_spherical_canvas_action, build_spherical_control_catalog,
        build_spherical_inspector_model, interact_spherical_canvas, legacy_compatibility_ui,
        queue_spherical_canvas_callback, show_spherical_controls, SphericalCanvasAction,
        SphericalCanvasInvalidation, SphericalCanvasState, SphericalOverlayControlKind,
        SphericalUiError, GLYPH_DENSITY_LABELS, VECTOR_DISPLAY_SPEED_LABEL,
    },
    TemplateApp,
};

const EXPECTED_STAGE_IDS: [&str; 16] = [
    "natural.resolve-climate-rules",
    "natural.project-climate-input",
    "natural.resolve-geologic-rules",
    "natural.project-geologic-input",
    "natural.resolve-hydro-erosion-rules",
    "natural.project-hydro-erosion-input",
    "natural.resolve-tectonic-rules",
    "natural.project-tectonic-input",
    "natural.resolve-world-formation",
    "spatial.spherical-voronoi",
    "natural.spherical-mantle",
    "natural.spherical-tectonics",
    "natural.spherical-relief",
    "natural.spherical-geology",
    "natural.spherical-preliminary-climate",
    "natural.spherical-hydro-erosion",
];

fn space() -> SphericalSpaceSpec {
    SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 162,
    }
}

fn external() -> ExternalArtifacts {
    build_spherical_external_artifacts(
        &space(),
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
    )
    .unwrap()
}

fn candidate(
    seed: u64,
    cache: &mut MemoryStageCache,
) -> sekai::app::SphericalPresentationCandidate {
    build_spherical_presentation_candidate(
        RootSeed::new(seed),
        &space(),
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
        cache,
        &SphericalFieldDisplayState::default(),
        &DisplayRevisionClock::default(),
    )
    .unwrap()
}

fn request_test_device() -> (
    eframe::egui_wgpu::wgpu::Device,
    eframe::egui_wgpu::wgpu::Queue,
) {
    use eframe::egui_wgpu::wgpu;

    pollster::block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: true,
                compatible_surface: None,
            })
            .await;
        let adapter = match adapter {
            Some(adapter) => adapter,
            None => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
                .expect("Task 9 integration requires a fallback or hardware GPU adapter"),
        };
        adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Spherical Presentation Integration Device"),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    ..Default::default()
                },
                None,
            )
            .await
            .expect("Task 9 integration requires a compatible GPU device")
    })
}

fn assert_exact_spherical_external_set(external: &ExternalArtifacts) {
    assert_eq!(external.len(), 9);
    assert!(external.hash::<SphericalSpaceArtifact>().is_ok());
    assert!(external.hash::<TectonicSpecArtifact>().is_ok());
    assert!(external.hash::<ReliefSpecArtifact>().is_ok());
    assert!(external.hash::<GeologicSpecArtifact>().is_ok());
    assert!(external.hash::<ClimateSpecArtifact>().is_ok());
    assert!(external.hash::<HydroErosionSpecArtifact>().is_ok());
    assert!(external.hash::<WorldFormationSpecArtifact>().is_ok());
    assert!(external.hash::<RulePackSetArtifact>().is_ok());
    assert!(external.hash::<AuthorConstraintsArtifact>().is_ok());
    assert!(external.hash::<PlanarSpaceArtifact>().is_err());
}

#[test]
fn exact_external_set_and_formal_graph_are_spherical_only() {
    assert_exact_spherical_external_set(&external());

    let mut cache = MemoryStageCache::new();
    let candidate = candidate(41, &mut cache);
    assert_eq!(candidate.report().stage_ids(), EXPECTED_STAGE_IDS);
    assert!(!candidate
        .report()
        .stage_ids()
        .iter()
        .any(|stage_id| matches!(
            *stage_id,
            "spatial.planar-voronoi"
                | "natural.tectonics"
                | "natural.mantle"
                | "natural.relief"
                | "natural.geology"
                | "natural.preliminary-climate"
                | "natural.hydro-erosion"
        )));
}

#[test]
fn candidate_is_source_bound_and_shares_one_layers_allocation() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(43, &mut cache);
    let source = candidate.source();

    assert_eq!(candidate.document().presentation_source(), *source);
    assert_eq!(candidate.locator().source(), source);
    assert_eq!(candidate.map().source(), source);
    assert_eq!(candidate.globe().source(), source);
    assert_eq!(candidate.layers().source(), source);
    assert_eq!(candidate.gpu_packet().source(), source);
    assert_eq!(
        candidate.map().projection().kind(),
        SphericalProjectionKind::EqualEarth
    );
    assert!(Arc::ptr_eq(
        candidate.map_presenter().layers_arc(),
        candidate.globe_presenter().layers_arc()
    ));
    assert!(Arc::ptr_eq(
        candidate.layers_arc(),
        candidate.gpu_packet().layers_arc()
    ));
    assert!(candidate.globe().vertices().iter().all(|vertex| {
        let [x, y, z] = vertex.position();
        ((x * x + y * y + z * z) - 1.0).abs() <= 1.0e-5
    }));
}

#[test]
fn equal_cardinality_never_allows_cross_source_presenter_composition() {
    let mut cache = MemoryStageCache::new();
    let first = candidate(47, &mut cache);
    let second = candidate(53, &mut cache);
    assert_eq!(first.map().cell_count(), second.map().cell_count());

    assert!(matches!(
        SphericalMapPresenter::try_new(
            Arc::clone(second.map_arc()),
            Arc::clone(first.layers_arc())
        ),
        Err(SphericalPresentationError::SourceMismatch {
            resource: "projected map"
        })
    ));
    assert!(matches!(
        SphericalGlobePresenter::try_new(
            Arc::clone(second.globe_arc()),
            Arc::clone(first.layers_arc())
        ),
        Err(SphericalPresentationError::SourceMismatch {
            resource: "unit globe"
        })
    ));
}

#[test]
fn whole_world_and_smaller_candidates_publish_atomically() {
    let mut cache = MemoryStageCache::new();
    let first = candidate(59, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
    let mut published = PublishedSphericalPresentation::try_new(first, &mut gpu).unwrap();

    let document_before = Arc::clone(published.document_arc());
    let locator_before = Arc::clone(published.locator_arc());
    let map_before = Arc::clone(published.map_arc());
    let globe_before = Arc::clone(published.globe_arc());
    let layers_before = Arc::clone(published.layers_arc());
    let gpu_before = Arc::clone(published.gpu_packet_arc());
    let state_before = published.state().clone();
    let report_before = published.report().clone();
    let revisions_before = published.revisions();
    let mut expected_clock = published.clock().clone();
    let expected_next_revision = expected_clock.issue().unwrap();

    let projection =
        SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.75).unwrap();
    let tiny_budget = SphericalMeshBudgets::new(1, 1, 1, 1);
    assert!(published
        .prepare_projection_candidate(projection, tiny_budget)
        .is_err());

    assert!(Arc::ptr_eq(&document_before, published.document_arc()));
    assert!(Arc::ptr_eq(&locator_before, published.locator_arc()));
    assert!(Arc::ptr_eq(&map_before, published.map_arc()));
    assert!(Arc::ptr_eq(&globe_before, published.globe_arc()));
    assert!(Arc::ptr_eq(&layers_before, published.layers_arc()));
    assert!(Arc::ptr_eq(&gpu_before, published.gpu_packet_arc()));
    assert_eq!(published.state(), &state_before);
    assert_eq!(published.report(), &report_before);
    assert_eq!(published.revisions(), revisions_before);
    let mut actual_clock = published.clock().clone();
    assert_eq!(actual_clock.issue().unwrap(), expected_next_revision);

    let projection_candidate = published
        .prepare_projection_candidate(projection, SphericalMeshBudgets::DEFAULT)
        .unwrap();
    published
        .try_replace_projection_candidate(projection_candidate, &mut gpu)
        .unwrap();
    assert!(!Arc::ptr_eq(&map_before, published.map_arc()));
    assert!(Arc::ptr_eq(&globe_before, published.globe_arc()));
    assert!(Arc::ptr_eq(&layers_before, published.layers_arc()));
    assert_eq!(published.state(), &state_before);
    assert_eq!(published.report(), &report_before);
    assert_ne!(published.revisions().0, revisions_before.0);
    assert_eq!(published.revisions().1, revisions_before.1);
    assert_eq!(published.revisions().2, revisions_before.2);

    let map_after_projection = Arc::clone(published.map_arc());
    let packet_after_projection = Arc::clone(published.gpu_packet_arc());
    let revisions_after_projection = published.revisions();
    let mut requested_state = published.state().clone();
    requested_state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
    let field_candidate = published
        .prepare_field_candidate(
            requested_state,
            SphericalViewMode::Map,
            MapCamera::default(),
            GlobeCamera::default(),
        )
        .unwrap();
    published
        .try_replace_field_candidate(field_candidate, &mut gpu)
        .unwrap();
    assert!(Arc::ptr_eq(&map_after_projection, published.map_arc()));
    assert!(Arc::ptr_eq(&globe_before, published.globe_arc()));
    assert!(!Arc::ptr_eq(&layers_before, published.layers_arc()));
    assert!(!Arc::ptr_eq(
        &packet_after_projection,
        published.gpu_packet_arc()
    ));
    assert_eq!(published.revisions().0, revisions_after_projection.0);
    assert_eq!(published.revisions().1, revisions_after_projection.1);
    assert_ne!(published.revisions().2, revisions_after_projection.2);
    assert_eq!(
        published.state().overlay_field(),
        Some(&preliminary_prevailing_wind_m_s_field_id())
    );

    let replacement_state = published.state().clone();
    let second = published
        .prepare_replacement_candidate(
            RootSeed::new(61),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &ReliefSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &replacement_state,
        )
        .unwrap();
    published.try_replace(second, &mut gpu).unwrap();
    assert!(!Arc::ptr_eq(&document_before, published.document_arc()));
    assert!(!Arc::ptr_eq(&locator_before, published.locator_arc()));
    assert!(!Arc::ptr_eq(&map_before, published.map_arc()));
    assert!(!Arc::ptr_eq(&globe_before, published.globe_arc()));
    assert!(!Arc::ptr_eq(&layers_before, published.layers_arc()));
    assert!(!Arc::ptr_eq(&gpu_before, published.gpu_packet_arc()));
    assert_ne!(published.source().root_seed(), RootSeed::new(59));
    assert_eq!(published.source().root_seed(), RootSeed::new(61));
}

#[test]
fn public_frame_paths_cannot_restore_a_retained_packet_after_source_replacement() {
    let mut cache = MemoryStageCache::new();
    let initial = candidate(401, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut published = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(initial, &mut gpu).unwrap()
    };
    let retained = Arc::clone(published.gpu_packet_arc());
    let retained_source = retained.source().clone();
    let replacement = published
        .prepare_replacement_candidate(
            RootSeed::new(409),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &ReliefSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            published.state(),
        )
        .unwrap();
    {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        published.try_replace(replacement, &mut gpu).unwrap();
    }
    assert_ne!(published.source(), &retained_source);
    let after_replacement = renderer.upload_counters();

    assert_eq!(
        renderer.prepare_map_frame(
            &queue,
            &retained,
            MapCamera::default(),
            [192, 96],
            Default::default(),
        ),
        Err(sekai::gpu::spherical::SphericalRenderError::FramePacketNotInstalled)
    );
    assert_eq!(
        renderer.prepare_globe_frame(
            &queue,
            &retained,
            GlobeCamera::default(),
            [192, 96],
            Default::default(),
        ),
        Err(sekai::gpu::spherical::SphericalRenderError::FramePacketNotInstalled)
    );
    assert_eq!(renderer.upload_counters(), after_replacement);

    renderer
        .prepare_map_frame(
            &queue,
            published.gpu_packet(),
            MapCamera::default(),
            [192, 96],
            Default::default(),
        )
        .unwrap();
    let after_current_frame = renderer.upload_counters();
    assert_eq!(after_current_frame.uniforms, after_replacement.uniforms + 1);
    assert_eq!(
        renderer.prepare_map_frame(
            &queue,
            &retained,
            MapCamera::default(),
            [192, 96],
            Default::default(),
        ),
        Err(sekai::gpu::spherical::SphericalRenderError::FramePacketNotInstalled)
    );
    assert_eq!(renderer.upload_counters(), after_current_frame);
}

#[test]
fn a_renderer_cannot_be_initialized_by_two_live_publications() {
    let mut cache = MemoryStageCache::new();
    let first = candidate(421, &mut cache);
    let second = candidate(423, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let published = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(first, &mut gpu).unwrap()
    };
    renderer
        .prepare_map_frame(
            &queue,
            published.gpu_packet(),
            MapCamera::default(),
            [192, 96],
            Default::default(),
        )
        .unwrap();
    let packet_before = Arc::clone(published.gpu_packet_arc());
    let source_before = published.source().clone();
    let revisions_before = published.revisions();
    let state_before = published.state().clone();
    let counters_before = renderer.upload_counters();
    let pixels_before = read_prepared_map_pixels(&device, &queue, &renderer, [192, 96]);

    let result = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(second, &mut gpu)
    };
    assert!(matches!(
        result,
        Err(SphericalPresentationError::Gpu(
            sekai::gpu::spherical::SphericalRenderError::RendererAlreadyInitialized
        ))
    ));
    assert!(Arc::ptr_eq(&packet_before, published.gpu_packet_arc()));
    assert_eq!(published.source(), &source_before);
    assert_eq!(published.revisions(), revisions_before);
    assert_eq!(published.state(), &state_before);
    assert_eq!(renderer.installed_source(), Some(&source_before));
    assert_eq!(renderer.upload_counters(), counters_before);
    assert_eq!(
        read_prepared_map_pixels(&device, &queue, &renderer, [192, 96]),
        pixels_before
    );
}

#[derive(Clone, Copy, Debug)]
enum WrongRendererReplacement {
    Whole,
    Projection,
    Field,
}

#[test]
fn every_replacement_path_rejects_a_renderer_owned_by_another_publication() {
    for replacement in [
        WrongRendererReplacement::Whole,
        WrongRendererReplacement::Projection,
        WrongRendererReplacement::Field,
    ] {
        assert_wrong_renderer_replacement_is_inert(replacement);
    }
}

fn assert_wrong_renderer_replacement_is_inert(replacement: WrongRendererReplacement) {
    let mut first_cache = MemoryStageCache::new();
    let mut second_cache = MemoryStageCache::new();
    let first = candidate(431, &mut first_cache);
    let second = candidate(433, &mut second_cache);
    assert_ne!(first.source(), second.source());
    let (device, queue) = request_test_device();
    let format = eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut first_renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(&device, format);
    let mut second_renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(&device, format);
    let mut first_published = {
        let mut gpu = SphericalRendererPreparer::new(&mut first_renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(first, &mut gpu).unwrap()
    };
    let second_published = {
        let mut gpu = SphericalRendererPreparer::new(&mut second_renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(second, &mut gpu).unwrap()
    };
    first_renderer
        .prepare_map_frame(
            &queue,
            first_published.gpu_packet(),
            MapCamera::default(),
            [192, 96],
            Default::default(),
        )
        .unwrap();
    second_renderer
        .prepare_map_frame(
            &queue,
            second_published.gpu_packet(),
            MapCamera::default(),
            [192, 96],
            Default::default(),
        )
        .unwrap();
    let first_packet = Arc::clone(first_published.gpu_packet_arc());
    let second_packet = Arc::clone(second_published.gpu_packet_arc());
    let first_source = first_published.source().clone();
    let second_source = second_published.source().clone();
    let first_revisions = first_published.revisions();
    let second_revisions = second_published.revisions();
    let first_state = first_published.state().clone();
    let second_state = second_published.state().clone();
    let first_counters = first_renderer.upload_counters();
    let second_counters = second_renderer.upload_counters();
    let first_pixels = read_prepared_map_pixels(&device, &queue, &first_renderer, [192, 96]);
    let second_pixels = read_prepared_map_pixels(&device, &queue, &second_renderer, [192, 96]);

    let result = match replacement {
        WrongRendererReplacement::Whole => {
            let next = first_published
                .prepare_replacement_candidate(
                    RootSeed::new(439),
                    &space(),
                    &WorldFormationSpec::default(),
                    &TectonicSpec::default(),
                    &ReliefSpec::default(),
                    &GeologicSpec::default(),
                    &mut first_cache,
                    first_published.state(),
                )
                .unwrap();
            let mut gpu = SphericalRendererPreparer::new(&mut second_renderer, &device, &queue);
            first_published.try_replace(next, &mut gpu)
        }
        WrongRendererReplacement::Projection => {
            let projection =
                SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.31).unwrap();
            let next = first_published
                .prepare_projection_candidate(projection, SphericalMeshBudgets::DEFAULT)
                .unwrap();
            let mut gpu = SphericalRendererPreparer::new(&mut second_renderer, &device, &queue);
            first_published.try_replace_projection_candidate(next, &mut gpu)
        }
        WrongRendererReplacement::Field => {
            let mut requested = first_published.state().clone();
            requested.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
            let next = first_published
                .prepare_field_candidate(
                    requested,
                    SphericalViewMode::Map,
                    MapCamera::default(),
                    GlobeCamera::default(),
                )
                .unwrap();
            let mut gpu = SphericalRendererPreparer::new(&mut second_renderer, &device, &queue);
            first_published.try_replace_field_candidate(next, &mut gpu)
        }
    };
    assert!(
        matches!(
            result,
            Err(SphericalPresentationError::Gpu(
                sekai::gpu::spherical::SphericalRenderError::RendererCurrentPacketMismatch
            ))
        ),
        "{replacement:?} replacement seized another publication's renderer"
    );
    assert!(Arc::ptr_eq(&first_packet, first_published.gpu_packet_arc()));
    assert!(Arc::ptr_eq(
        &second_packet,
        second_published.gpu_packet_arc()
    ));
    assert_eq!(first_published.source(), &first_source);
    assert_eq!(second_published.source(), &second_source);
    assert_eq!(first_published.revisions(), first_revisions);
    assert_eq!(second_published.revisions(), second_revisions);
    assert_eq!(first_published.state(), &first_state);
    assert_eq!(second_published.state(), &second_state);
    assert_eq!(first_renderer.installed_source(), Some(&first_source));
    assert_eq!(second_renderer.installed_source(), Some(&second_source));
    assert_eq!(first_renderer.upload_counters(), first_counters);
    assert_eq!(second_renderer.upload_counters(), second_counters);
    assert_eq!(
        read_prepared_map_pixels(&device, &queue, &first_renderer, [192, 96]),
        first_pixels
    );
    assert_eq!(
        read_prepared_map_pixels(&device, &queue, &second_renderer, [192, 96]),
        second_pixels
    );
}

#[test]
fn replacement_candidate_cannot_fork_a_second_initial_publication() {
    let mut cache = MemoryStageCache::new();
    let initial = candidate(313, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let predecessor = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(initial, &mut gpu).unwrap()
    };
    let packet_before = Arc::clone(predecessor.gpu_packet_arc());
    let document_before = Arc::clone(predecessor.document_arc());
    let layers_before = Arc::clone(predecessor.layers_arc());
    let source_before = predecessor.source().clone();
    let revisions_before = predecessor.revisions();
    let state_before = predecessor.state().clone();
    let mut clock_before = predecessor.clock().clone();
    let expected_next = clock_before.issue().unwrap();
    let uploads_before = renderer.upload_counters();
    let replacement = predecessor
        .prepare_replacement_candidate(
            RootSeed::new(317),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &ReliefSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            predecessor.state(),
        )
        .unwrap();

    let result = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(replacement, &mut gpu)
    };
    match result {
        Err(error) => assert_eq!(
            error.to_string(),
            "initial spherical publication requires a standalone candidate"
        ),
        Ok(_) => panic!("replacement lineage forked a second initial publication"),
    }

    assert_eq!(renderer.upload_counters(), uploads_before);
    assert!(Arc::ptr_eq(&packet_before, predecessor.gpu_packet_arc()));
    assert!(Arc::ptr_eq(&document_before, predecessor.document_arc()));
    assert!(Arc::ptr_eq(&layers_before, predecessor.layers_arc()));
    assert_eq!(predecessor.source(), &source_before);
    assert_eq!(predecessor.revisions(), revisions_before);
    assert_eq!(predecessor.state(), &state_before);
    let mut clock_after = predecessor.clock().clone();
    assert_eq!(clock_after.issue().unwrap(), expected_next);
}

#[test]
fn standalone_and_replacement_builds_reconcile_edges_to_the_final_overlay_channel() {
    let invalid_overlay = FieldId::new("test.spherical", "missing-overlay", 1).unwrap();
    let edge = Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(0)));
    for (case, overlay, expected_entity) in [
        ("none", None, None),
        (
            "vector",
            Some(preliminary_prevailing_wind_m_s_field_id()),
            None,
        ),
        ("invalid", Some(invalid_overlay.clone()), None),
        ("edge", Some(boundary_strength_field_id()), edge),
    ] {
        let mut cache = MemoryStageCache::new();
        let mut requested = SphericalFieldDisplayState::default();
        requested.select_overlay(overlay.clone());
        requested.select_entity(edge);
        let standalone = build_spherical_presentation_candidate(
            RootSeed::new(331),
            &space(),
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &ReliefSpec::default(),
            &GeologicSpec::default(),
            &mut cache,
            &requested,
            &DisplayRevisionClock::default(),
        )
        .unwrap();
        assert_eq!(
            standalone.state().selected_entity(),
            expected_entity,
            "standalone {case}"
        );

        let (device, queue) = request_test_device();
        let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
            &device,
            eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        let mut published = PublishedSphericalPresentation::try_new(standalone, &mut gpu).unwrap();
        assert!(Arc::ptr_eq(
            published.gpu_packet().layers_arc(),
            published.layers_arc()
        ));
        let map_model =
            build_spherical_inspector_model(&published, published.state(), SphericalViewMode::Map)
                .unwrap();
        let globe_model = build_spherical_inspector_model(
            &published,
            published.state(),
            SphericalViewMode::Globe,
        )
        .unwrap();
        assert_eq!(map_model, globe_model, "standalone {case}");
        assert_eq!(map_model.entity(), expected_entity, "standalone {case}");

        let mut replacement_state = published.state().clone();
        replacement_state.select_overlay(overlay);
        replacement_state.select_entity(edge);
        let replacement = published
            .prepare_replacement_candidate(
                RootSeed::new(337),
                &space(),
                &WorldFormationSpec::default(),
                &TectonicSpec::default(),
                &ReliefSpec::default(),
                &GeologicSpec::default(),
                &mut cache,
                &replacement_state,
            )
            .unwrap();
        assert_eq!(
            replacement.state().selected_entity(),
            expected_entity,
            "replacement {case}"
        );
        published.try_replace(replacement, &mut gpu).unwrap();
        assert_eq!(
            published.state().selected_entity(),
            expected_entity,
            "published {case}"
        );
        assert_eq!(published.source().root_seed(), RootSeed::new(337));
        assert!(Arc::ptr_eq(
            published.gpu_packet().layers_arc(),
            published.layers_arc()
        ));
        assert_eq!(
            published.gpu_packet().layers().revisions(),
            published.revisions().2
        );
        assert_eq!(
            build_spherical_inspector_model(&published, published.state(), SphericalViewMode::Map,)
                .unwrap()
                .entity(),
            expected_entity,
            "replacement inspector {case}"
        );
    }
}

#[test]
fn persisted_origin_defaults_new_apps_to_spherical_and_missing_tags_to_legacy() {
    let app = TemplateApp::default();
    let mut encoded = serde_json::to_value(&app).unwrap();

    assert_eq!(app.world_origin(), PersistedWorldOrigin::SphericalV1);
    assert_eq!(
        app.runtime_graph(),
        AppRuntimeGraph::SphericalNaturalFoundation
    );
    assert_eq!(encoded["world_origin"], "SphericalV1");
    assert_eq!(encoded["spherical_space_spec"]["radius"], 6_371_000.0);
    assert_eq!(encoded["spherical_space_spec"]["target_cell_count"], 20_000);
    assert!(encoded.get("spherical_mode").is_none());
    assert!(encoded.get("geometry_mode").is_none());
    assert!(encoded.get("world_mode").is_none());
    assert!(encoded.get("spherical_presentation").is_none());
    assert!(encoded.get("spherical_renderer").is_none());
    assert!(encoded.get("stage_cache").is_none());

    encoded.as_object_mut().unwrap().remove("world_origin");
    encoded
        .as_object_mut()
        .unwrap()
        .remove("spherical_space_spec");
    let restored: TemplateApp = serde_json::from_value(encoded).unwrap();
    assert_eq!(
        restored.world_origin(),
        PersistedWorldOrigin::LegacyPlanarV1
    );
    assert_eq!(
        restored.runtime_graph(),
        AppRuntimeGraph::LegacyPlanarFoundation
    );
    assert!(restored.legacy_compatibility_notice().is_some());
    assert!(restored.offers_regenerate_as_spherical());
}

#[test]
fn spherical_canvas_state_round_trips_and_rejects_invalid_runtime_numbers() {
    let mut state = SphericalCanvasState::default();
    state
        .apply(SphericalCanvasAction::PanMap {
            delta: [0.25, -0.125],
        })
        .unwrap();
    state
        .apply(SphericalCanvasAction::ZoomMap { factor: 1.5 })
        .unwrap();
    state
        .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe))
        .unwrap();
    state
        .apply(SphericalCanvasAction::TrackballGlobe {
            start: [320.0, 240.0],
            end: [420.0, 200.0],
            canvas_size: [640.0, 480.0],
        })
        .unwrap();
    state
        .apply(SphericalCanvasAction::ZoomGlobe { factor: 1.25 })
        .unwrap();
    state
        .apply(SphericalCanvasAction::SetVectorDisplaySpeed(2.5))
        .unwrap();

    let encoded = serde_json::to_value(&state).unwrap();
    let restored: SphericalCanvasState = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(restored, state);

    let empty: SphericalCanvasState = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(empty, SphericalCanvasState::default());

    for (path, invalid) in [
        ("/map_cameras/equal_earth_zoom", serde_json::json!(-1.0)),
        ("/globe_camera/scale", serde_json::json!(0.0)),
        ("/field_state/vector_display_speed", serde_json::json!(8.0)),
        (
            "/globe_camera/orientation_xyzw",
            serde_json::json!([0.0, 0.0, 0.0, 0.0]),
        ),
    ] {
        let mut broken = encoded.clone();
        *broken.pointer_mut(path).expect("wire path must exist") = invalid;
        assert!(serde_json::from_value::<SphericalCanvasState>(broken).is_err());
    }
}

#[test]
fn layer_visibility_defaults_migrates_round_trips_and_invalidates_only_uniforms() {
    let mut state = SphericalCanvasState::default();
    assert_eq!(
        state.field_state().layer_visibility(),
        SphericalLayerVisibility {
            fill: true,
            overlay: true,
        }
    );

    assert_eq!(
        state
            .apply(SphericalCanvasAction::SetFillVisible(false))
            .unwrap(),
        SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM
    );
    assert_eq!(
        state
            .apply(SphericalCanvasAction::SetFillVisible(false))
            .unwrap(),
        SphericalCanvasInvalidation::NONE
    );
    assert_eq!(
        state
            .apply(SphericalCanvasAction::SetOverlayVisible(false))
            .unwrap(),
        SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM
    );
    assert_eq!(
        state
            .apply(SphericalCanvasAction::SetOverlayVisible(false))
            .unwrap(),
        SphericalCanvasInvalidation::NONE
    );

    let encoded = serde_json::to_value(&state).unwrap();
    assert_eq!(encoded["field_state"]["fill_visible"], false);
    assert_eq!(encoded["field_state"]["overlay_visible"], false);
    let restored: SphericalCanvasState = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(restored, state);

    let mut old_wire = encoded;
    old_wire["field_state"]
        .as_object_mut()
        .unwrap()
        .remove("fill_visible");
    old_wire["field_state"]
        .as_object_mut()
        .unwrap()
        .remove("overlay_visible");
    let migrated: SphericalCanvasState = serde_json::from_value(old_wire).unwrap();
    assert_eq!(
        migrated.field_state().layer_visibility(),
        SphericalLayerVisibility {
            fill: true,
            overlay: true,
        }
    );
}

#[test]
fn layer_visibility_preserves_the_exact_published_packet_and_immutable_uploads() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(403, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut published = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap()
    };
    let mut encoded = serde_json::to_value(SphericalCanvasState::default()).unwrap();
    let field_wire = encoded["field_state"].as_object_mut().unwrap();
    field_wire.insert(
        "fill_field".into(),
        serde_json::to_value(published.state().fill_field()).unwrap(),
    );
    field_wire.insert(
        "overlay_field".into(),
        serde_json::to_value(published.state().overlay_field()).unwrap(),
    );
    field_wire.insert(
        "range_mode".into(),
        serde_json::to_value(published.state().range_mode()).unwrap(),
    );
    let mut state: SphericalCanvasState = serde_json::from_value(encoded).unwrap();
    assert_eq!(state.field_state(), published.state());

    let packet = Arc::clone(published.gpu_packet_arc());
    let layers = Arc::clone(published.layers_arc());
    let source = published.source().clone();
    let revisions = published.revisions();
    let counters = renderer.upload_counters();

    for action in [
        SphericalCanvasAction::SetFillVisible(false),
        SphericalCanvasAction::SetOverlayVisible(false),
    ] {
        assert_eq!(
            apply_spherical_canvas_action(
                &mut published,
                &mut renderer,
                &device,
                &queue,
                &mut state,
                action,
            )
            .unwrap(),
            SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM
        );
    }

    assert_eq!(
        state.field_state().layer_visibility(),
        SphericalLayerVisibility {
            fill: false,
            overlay: false,
        }
    );
    assert_eq!(published.state(), state.field_state());
    assert!(Arc::ptr_eq(&packet, published.gpu_packet_arc()));
    assert!(Arc::ptr_eq(&layers, published.layers_arc()));
    assert_eq!(published.source(), &source);
    assert_eq!(published.revisions(), revisions);
    assert_eq!(renderer.upload_counters(), counters);
}

#[test]
fn map_camera_rejects_gpu_unrenderable_and_out_of_product_bounds_atomically() {
    let projection = SphericalProjectionKind::EqualEarth;
    let mut camera = MapCamera::default();
    assert!(camera.pan_by(projection, [4.0, -4.0]));
    let pan_boundary = camera;
    assert!(!camera.pan_by(projection, [1.0e-12, 0.0]));
    assert_eq!(camera, pan_boundary);
    assert!(!camera.pan_by(projection, [1.0e300, 0.0]));
    assert_eq!(camera, pan_boundary);

    camera.reset(projection);
    assert!(camera.zoom_by(projection, 0.125));
    assert_eq!(camera.zoom(projection), 0.125);
    assert!(camera.zoom_by(projection, 512.0));
    assert_eq!(camera.zoom(projection), 64.0);
    let zoom_boundary = camera;
    assert!(!camera.zoom_by(projection, 1.000_001));
    assert_eq!(camera, zoom_boundary);
    assert!(!camera.zoom_by(projection, 1.0e300));
    assert_eq!(camera, zoom_boundary);

    let mut canvas = SphericalCanvasState::default();
    for action in [
        SphericalCanvasAction::PanMap {
            delta: [1.0e300, 0.0],
        },
        SphericalCanvasAction::ZoomMap { factor: 1.0e300 },
    ] {
        let before = canvas.clone();
        assert!(canvas.apply(action).is_err());
        assert_eq!(canvas, before);
    }

    let encoded = serde_json::to_value(SphericalCanvasState::default()).unwrap();
    for (path, value) in [
        (
            "/map_cameras/equal_earth_pan",
            serde_json::json!([1.0e300, 0.0]),
        ),
        ("/map_cameras/equal_earth_zoom", serde_json::json!(1.0e300)),
    ] {
        let mut corrupt = encoded.clone();
        *corrupt.pointer_mut(path).unwrap() = value;
        assert!(serde_json::from_value::<SphericalCanvasState>(corrupt).is_err());
    }

    let mut lod = MapCamera::default();
    for factor in [1.99, 2.0 / 1.99, 2.0] {
        assert!(lod.zoom_by(projection, factor));
    }
    assert!((lod.zoom(projection) - 4.0).abs() < 1.0e-12);
}

#[test]
fn declarative_canvas_actions_preserve_shared_state_and_invalidate_exactly() {
    let mut state = SphericalCanvasState::default();
    state
        .apply(SphericalCanvasAction::SelectFill(
            surface_elevation_m_field_id(),
        ))
        .unwrap();
    state
        .apply(SphericalCanvasAction::SelectOverlay(Some(
            preliminary_prevailing_wind_m_s_field_id(),
        )))
        .unwrap();
    state
        .apply(SphericalCanvasAction::SetDiagnosticsEnabled(false))
        .unwrap();
    state
        .apply(SphericalCanvasAction::SelectEntity(Some(
            SelectedSurfaceEntity::Cell(CellId::from_raw(7)),
        )))
        .unwrap();

    let field_before = state.field_state().clone();
    let equal_earth_before = state.map_camera();
    let globe_before = state.globe_camera();
    assert_eq!(
        state
            .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe,))
            .unwrap(),
        SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM
    );
    assert_eq!(state.field_state(), &field_before);
    assert_eq!(state.map_camera(), equal_earth_before);
    assert_eq!(state.globe_camera(), globe_before);

    state
        .apply(SphericalCanvasAction::SetProjectionKind(
            SphericalProjectionKind::Equirectangular,
        ))
        .unwrap();
    state
        .apply(SphericalCanvasAction::PanMap { delta: [-0.4, 0.2] })
        .unwrap();
    state
        .apply(SphericalCanvasAction::ZoomMap { factor: 1.75 })
        .unwrap();
    let equirectangular_camera = state.map_camera();

    assert_eq!(
        state
            .apply(SphericalCanvasAction::SetProjectionKind(
                SphericalProjectionKind::EqualEarth,
            ))
            .unwrap(),
        SphericalCanvasInvalidation::MAP_GEOMETRY
    );
    assert_eq!(
        state.map_camera().pan(SphericalProjectionKind::EqualEarth),
        equal_earth_before.pan(SphericalProjectionKind::EqualEarth)
    );
    assert_eq!(
        state.map_camera().zoom(SphericalProjectionKind::EqualEarth),
        equal_earth_before.zoom(SphericalProjectionKind::EqualEarth)
    );
    assert_eq!(
        state
            .map_camera()
            .pan(SphericalProjectionKind::Equirectangular),
        equirectangular_camera.pan(SphericalProjectionKind::Equirectangular)
    );
    assert_eq!(
        state
            .map_camera()
            .zoom(SphericalProjectionKind::Equirectangular),
        equirectangular_camera.zoom(SphericalProjectionKind::Equirectangular)
    );
    assert_eq!(state.globe_camera(), globe_before);
    assert_eq!(state.field_state(), &field_before);

    assert_eq!(
        state
            .apply(SphericalCanvasAction::SetCentralMeridianRadians(0.75))
            .unwrap(),
        SphericalCanvasInvalidation::MAP_GEOMETRY
    );
    assert_eq!(state.field_state(), &field_before);
    assert_eq!(state.globe_camera(), globe_before);

    assert_eq!(
        state
            .apply(SphericalCanvasAction::PanMap {
                delta: [0.05, -0.025],
            })
            .unwrap(),
        SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM
    );
    assert_eq!(
        state
            .apply(SphericalCanvasAction::ZoomGlobe { factor: 1.1 })
            .unwrap(),
        SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM
    );

    let layers_before_phase = state.field_state().clone();
    let phase_before = state.vector_animation().phase();
    assert_eq!(
        state
            .apply(SphericalCanvasAction::AdvanceVectorPhase {
                frame_delta_seconds: 0.25,
            })
            .unwrap(),
        SphericalCanvasInvalidation::PHASE_UNIFORM
    );
    assert_ne!(state.vector_animation().phase(), phase_before);
    assert_eq!(state.field_state(), &layers_before_phase);

    assert_eq!(
        state
            .apply(SphericalCanvasAction::RegenerateAsSpherical)
            .unwrap(),
        SphericalCanvasInvalidation::WORLD_REGENERATION
    );
}

#[test]
fn switching_active_camera_family_reconciles_vector_lod_when_zoom_band_changes() {
    let mut state = SphericalCanvasState::default();
    state
        .apply(SphericalCanvasAction::SetVectorLod(VectorGlyphLod::Low))
        .unwrap();
    state
        .apply(SphericalCanvasAction::SetProjectionKind(
            SphericalProjectionKind::Equirectangular,
        ))
        .unwrap();
    state
        .apply(SphericalCanvasAction::ZoomMap { factor: 3.0 })
        .unwrap();
    state
        .apply(SphericalCanvasAction::SetProjectionKind(
            SphericalProjectionKind::EqualEarth,
        ))
        .unwrap();

    let projection_switch = state
        .apply(SphericalCanvasAction::SetProjectionKind(
            SphericalProjectionKind::Equirectangular,
        ))
        .unwrap();
    assert!(projection_switch.map_geometry());
    assert!(projection_switch.field_layers());
    assert_eq!(state.field_state().vector_view_zoom(), 3.0);

    state
        .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe))
        .unwrap();
    state
        .apply(SphericalCanvasAction::ZoomGlobe { factor: 0.25 })
        .unwrap();
    state
        .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Map))
        .unwrap();
    let view_switch = state
        .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe))
        .unwrap();
    assert!(view_switch.field_layers());
    assert_eq!(
        state.field_state().vector_view_zoom(),
        GlobeCamera::MIN_SCALE
    );
}

#[test]
fn selection_invalidation_tracks_both_old_and_new_cell_bound_state() {
    let mut state = SphericalCanvasState::default();
    let cell_a = Some(SelectedSurfaceEntity::Cell(CellId::from_raw(1)));
    let cell_b = Some(SelectedSurfaceEntity::Cell(CellId::from_raw(2)));
    let edge_a = Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(1)));
    let edge_b = Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(2)));

    for (selection, expected) in [
        (cell_a, SphericalCanvasInvalidation::FIELD_LAYERS),
        (cell_b, SphericalCanvasInvalidation::FIELD_LAYERS),
        (edge_a, SphericalCanvasInvalidation::FIELD_LAYERS),
        (None, SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM),
        (
            edge_a,
            SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM,
        ),
        (
            edge_b,
            SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM,
        ),
        (edge_b, SphericalCanvasInvalidation::NONE),
        (cell_a, SphericalCanvasInvalidation::FIELD_LAYERS),
        (None, SphericalCanvasInvalidation::FIELD_LAYERS),
    ] {
        assert_eq!(
            state
                .apply(SphericalCanvasAction::SelectEntity(selection))
                .unwrap(),
            expected,
            "unexpected invalidation for transition to {selection:?}",
        );
    }
}

#[test]
fn edge_selection_is_catalog_aware_validated_and_atomically_cleared_on_channel_change() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(61, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut published = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap()
    };
    let mut state = SphericalCanvasState::default();
    apply_spherical_canvas_action(
        &mut published,
        &mut renderer,
        &device,
        &queue,
        &mut state,
        SphericalCanvasAction::SelectOverlay(Some(boundary_strength_field_id())),
    )
    .unwrap();
    let edge = SelectedSurfaceEntity::Edge(EdgeId::from_raw(1));
    apply_spherical_canvas_action(
        &mut published,
        &mut renderer,
        &device,
        &queue,
        &mut state,
        SphericalCanvasAction::SelectEntity(Some(edge)),
    )
    .unwrap();
    assert_eq!(published.state().selected_entity(), Some(edge));

    let state_before_invalid = state.clone();
    let layers_before_invalid = Arc::clone(published.layers_arc());
    let packet_before_invalid = Arc::clone(published.gpu_packet_arc());
    let revisions_before_invalid = published.layers().revisions();
    let counters_before_invalid = renderer.upload_counters();
    assert!(matches!(
        apply_spherical_canvas_action(
            &mut published,
            &mut renderer,
            &device,
            &queue,
            &mut state,
            SphericalCanvasAction::SelectEntity(Some(SelectedSurfaceEntity::Edge(
                EdgeId::from_raw(u32::MAX),
            ))),
        ),
        Err(SphericalUiError::EntityValueMissing)
    ));
    assert_eq!(state, state_before_invalid);
    assert!(Arc::ptr_eq(&layers_before_invalid, published.layers_arc()));
    assert!(Arc::ptr_eq(
        &packet_before_invalid,
        published.gpu_packet_arc()
    ));
    assert_eq!(published.layers().revisions(), revisions_before_invalid);
    assert_eq!(renderer.upload_counters(), counters_before_invalid);

    apply_spherical_canvas_action(
        &mut published,
        &mut renderer,
        &device,
        &queue,
        &mut state,
        SphericalCanvasAction::SelectOverlay(None),
    )
    .unwrap();
    assert_eq!(state.field_state().selected_entity(), None);
    assert_eq!(published.state().selected_entity(), None);
    build_spherical_inspector_model(&published, published.state(), SphericalViewMode::Map).unwrap();

    let state_without_overlay = state.clone();
    let layers_without_overlay = Arc::clone(published.layers_arc());
    let counters_without_overlay = renderer.upload_counters();
    assert!(matches!(
        apply_spherical_canvas_action(
            &mut published,
            &mut renderer,
            &device,
            &queue,
            &mut state,
            SphericalCanvasAction::SelectEntity(Some(edge)),
        ),
        Err(SphericalUiError::UnsupportedInspectorEntity)
    ));
    assert_eq!(state, state_without_overlay);
    assert!(Arc::ptr_eq(&layers_without_overlay, published.layers_arc()));
    assert_eq!(renderer.upload_counters(), counters_without_overlay);

    for overlay in [
        boundary_kind_field_id(),
        preliminary_prevailing_wind_m_s_field_id(),
    ] {
        apply_spherical_canvas_action(
            &mut published,
            &mut renderer,
            &device,
            &queue,
            &mut state,
            SphericalCanvasAction::SelectOverlay(Some(overlay)),
        )
        .unwrap();
        if published.layers().overlay_kind() != Some(sekai::view::PreparedOverlayKind::CellVector) {
            apply_spherical_canvas_action(
                &mut published,
                &mut renderer,
                &device,
                &queue,
                &mut state,
                SphericalCanvasAction::SelectEntity(Some(edge)),
            )
            .unwrap();
        }
    }
    assert_eq!(state.field_state().selected_entity(), None);
    assert_eq!(published.state().selected_entity(), None);
    build_spherical_inspector_model(&published, published.state(), SphericalViewMode::Globe)
        .unwrap();
}

#[test]
fn spherical_controls_expose_exact_channels_and_non_physical_vector_labels() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(67, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
    let published = PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap();
    let controls = build_spherical_control_catalog(&published).unwrap();

    assert_eq!(controls.fill_fields().len(), 32);
    assert_eq!(controls.overlay_fields().len(), 5);
    assert_eq!(
        controls
            .overlay_fields()
            .iter()
            .filter(|option| option.kind() == SphericalOverlayControlKind::None)
            .count(),
        1
    );
    assert_eq!(
        controls
            .overlay_fields()
            .iter()
            .filter(|option| option.kind() == SphericalOverlayControlKind::Edge)
            .count(),
        2
    );
    assert_eq!(
        controls
            .overlay_fields()
            .iter()
            .filter(|option| option.kind() == SphericalOverlayControlKind::Vector)
            .count(),
        2
    );
    assert!(controls.contains_overlay(&boundary_kind_field_id()));
    assert!(controls.contains_overlay(&boundary_strength_field_id()));
    assert!(controls.contains_overlay(&plate_velocity_field_id()));
    assert!(controls.contains_overlay(&preliminary_prevailing_wind_m_s_field_id()));
    assert_eq!(VECTOR_DISPLAY_SPEED_LABEL, "显示速度（非物理时间）");
    assert_eq!(GLYPH_DENSITY_LABELS, ["Low", "Medium", "High"]);
    assert_eq!(VectorGlyphLod::default(), VectorGlyphLod::Medium);
}

#[test]
fn spherical_layer_visibility_controls_are_formal_and_emit_exact_actions() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(419, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
    let published = PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap();
    let context = egui::Context::default();
    let mut state = SphericalCanvasState::default();

    let (layout_actions, layout) = run_spherical_controls_frame(
        &context,
        spherical_raw_input(Vec::new()),
        &published,
        &state,
    );
    assert!(layout_actions.is_empty());
    for label in ["显示图层", "显示填色", "显示叠加", "显示诊断"] {
        assert!(
            layout
                .shapes
                .iter()
                .any(|shape| find_text_center(&shape.shape, label).is_some()),
            "missing formal layer control {label}"
        );
    }

    let overlay_center = layout
        .shapes
        .iter()
        .find_map(|shape| find_text_center(&shape.shape, "显示叠加"))
        .unwrap();
    let disabled = click_spherical_control(&context, overlay_center, &published, &state);
    assert!(
        disabled.is_empty(),
        "no selected overlay keeps the checkbox disabled"
    );
    assert!(state.field_state().overlay_visible());

    let fill_center = layout
        .shapes
        .iter()
        .find_map(|shape| find_text_center(&shape.shape, "显示填色"))
        .unwrap();
    let fill = click_spherical_control(&context, fill_center, &published, &state);
    assert_eq!(fill, vec![SphericalCanvasAction::SetFillVisible(false)]);
    state.apply(fill.into_iter().next().unwrap()).unwrap();

    state
        .apply(SphericalCanvasAction::SelectOverlay(Some(
            preliminary_prevailing_wind_m_s_field_id(),
        )))
        .unwrap();
    let (_, overlay_layout) = run_spherical_controls_frame(
        &context,
        spherical_raw_input(Vec::new()),
        &published,
        &state,
    );
    let overlay_center = overlay_layout
        .shapes
        .iter()
        .find_map(|shape| find_text_center(&shape.shape, "显示叠加"))
        .unwrap();
    let overlay = click_spherical_control(&context, overlay_center, &published, &state);
    assert_eq!(
        overlay,
        vec![SphericalCanvasAction::SetOverlayVisible(false)]
    );
    state.apply(overlay.into_iter().next().unwrap()).unwrap();

    let (_, diagnostic_layout) = run_spherical_controls_frame(
        &context,
        spherical_raw_input(Vec::new()),
        &published,
        &state,
    );
    let diagnostic_center = diagnostic_layout
        .shapes
        .iter()
        .find_map(|shape| find_text_center(&shape.shape, "显示诊断"))
        .unwrap();
    let diagnostics = click_spherical_control(&context, diagnostic_center, &published, &state);
    assert_eq!(
        diagnostics,
        vec![SphericalCanvasAction::SetDiagnosticsEnabled(false)]
    );
}

#[test]
fn inspector_uses_authoritative_catalog_and_surface_identically_in_map_and_globe() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(71, &mut cache);
    let mut requested = candidate.state().clone();
    requested.select_fill(surface_elevation_m_field_id());
    requested.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));

    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
    let mut published = PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap();
    let field_candidate = published
        .prepare_field_candidate(
            requested,
            SphericalViewMode::Map,
            MapCamera::default(),
            GlobeCamera::default(),
        )
        .unwrap();
    published
        .try_replace_field_candidate(field_candidate, &mut gpu)
        .unwrap();

    let mut state = published.state().clone();
    state.select_entity(Some(SelectedSurfaceEntity::Cell(CellId::from_raw(0))));
    let map_cell =
        build_spherical_inspector_model(&published, &state, SphericalViewMode::Map).unwrap();
    let globe_cell =
        build_spherical_inspector_model(&published, &state, SphericalViewMode::Globe).unwrap();
    assert_eq!(map_cell, globe_cell);
    assert!(map_cell.has_row("填色值"));
    assert!(map_cell.has_row("东向分量"));
    assert!(map_cell.has_row("北向分量"));
    assert!(map_cell.has_row("模长"));
    assert!(map_cell.has_row("方向角"));
    assert!(map_cell.has_row("填色单位"));
    assert!(map_cell.has_row("填色字段来源"));
    assert!(map_cell.has_row("向量单位"));
    assert!(map_cell.has_row("向量字段来源"));
    assert!(!map_cell.has_row("单位"));
    assert!(!map_cell.has_row("字段来源"));
    let row_value = |label| {
        map_cell
            .rows()
            .iter()
            .find(|row| row.label() == label)
            .unwrap()
            .value()
    };
    let field_id_text = |field: &sekai::world::fields::FieldId| {
        format!("{}.{}@{}", field.namespace(), field.name(), field.version())
    };
    assert_eq!(row_value("填色单位"), "m");
    assert_eq!(
        row_value("填色字段来源"),
        field_id_text(&surface_elevation_m_field_id())
    );
    assert_eq!(row_value("向量单位"), "m/s");
    assert_eq!(
        row_value("向量字段来源"),
        field_id_text(&preliminary_prevailing_wind_m_s_field_id())
    );
    assert!(map_cell.diagnostics().iter().all(|diagnostic| diagnostic
        .cell()
        .is_none_or(|cell| cell == CellId::from_raw(0))));

    state.select_overlay(Some(boundary_strength_field_id()));
    state.select_entity(Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(0))));
    let map_edge =
        build_spherical_inspector_model(&published, &state, SphericalViewMode::Map).unwrap();
    let globe_edge =
        build_spherical_inspector_model(&published, &state, SphericalViewMode::Globe).unwrap();
    assert_eq!(map_edge, globe_edge);
    assert!(map_edge.has_row("边值"));
    assert!(map_edge.has_row("Owners"));
    assert!(map_edge.has_row("单位"));
    assert!(map_edge
        .diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.cell().is_none()));
}

#[test]
fn unselected_inspector_describes_current_fields_ranges_and_legends_without_mutation() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(72, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
    let mut published = PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap();
    let mut requested = published.state().clone();
    requested.select_fill(surface_elevation_m_field_id());
    requested.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
    requested.select_entity(None);
    let field_candidate = published
        .prepare_field_candidate(
            requested,
            SphericalViewMode::Map,
            MapCamera::default(),
            GlobeCamera::default(),
        )
        .unwrap();
    published
        .try_replace_field_candidate(field_candidate, &mut gpu)
        .unwrap();

    let state_before = published.state().clone();
    let layers_before = Arc::clone(published.layers_arc());
    let map =
        build_spherical_inspector_model(&published, &state_before, SphericalViewMode::Map).unwrap();
    let globe =
        build_spherical_inspector_model(&published, &state_before, SphericalViewMode::Globe)
            .unwrap();

    assert_eq!(map, globe);
    assert_eq!(map.entity(), None);
    for label in [
        "填色说明",
        "填色单位",
        "填色范围",
        "填色图例",
        "填色类别图例",
        "叠加说明",
        "叠加单位",
        "叠加范围",
        "叠加图例",
        "叠加类别图例",
    ] {
        assert!(
            map.has_row(label),
            "missing unselected inspector row {label}"
        );
    }
    let row_value = |label| {
        map.rows()
            .iter()
            .find(|row| row.label() == label)
            .unwrap()
            .value()
    };
    assert!(row_value("填色说明").contains("当前地表高程"));
    assert_eq!(row_value("填色单位"), "m");
    assert!(row_value("填色范围").contains('…'));
    assert!(row_value("填色图例").contains("Diverging"));
    assert_eq!(row_value("填色类别图例"), "不适用");
    assert!(row_value("叠加说明").contains("初步盛行风"));
    assert_eq!(row_value("叠加单位"), "m/s");
    assert!(row_value("叠加图例").contains("Vector"));
    assert_eq!(row_value("叠加类别图例"), "不适用");
    assert_eq!(published.state(), &state_before);
    assert!(Arc::ptr_eq(&layers_before, published.layers_arc()));

    let mut category_state = state_before.clone();
    category_state.select_fill(land_ocean_field_id());
    let category_candidate = published
        .prepare_field_candidate(
            category_state,
            SphericalViewMode::Map,
            MapCamera::default(),
            GlobeCamera::default(),
        )
        .unwrap();
    published
        .try_replace_field_candidate(category_candidate, &mut gpu)
        .unwrap();
    let category =
        build_spherical_inspector_model(&published, published.state(), SphericalViewMode::Map)
            .unwrap();
    let category_legend = category
        .rows()
        .iter()
        .find(|row| row.label() == "填色类别图例")
        .unwrap()
        .value();
    assert!(category_legend.contains("海洋"));
    assert!(category_legend.contains("陆地"));
}

#[test]
fn active_spherical_canvas_emits_exactly_one_callback_per_frame() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(73, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
    let published = PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap();
    let context = egui::Context::default();
    let mut state = SphericalCanvasState::default();

    for mode in [SphericalViewMode::Map, SphericalViewMode::Globe] {
        state
            .apply(SphericalCanvasAction::SetViewMode(mode))
            .unwrap();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..egui::RawInput::default()
        };
        let output = context.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let canvas = interact_spherical_canvas(ui, &published, &mut state);
                let rect = canvas.response().rect;
                assert!(canvas.into_actions().is_empty());
                queue_spherical_canvas_callback(ui, &published, &state, rect);
            });
        });
        let callbacks = output
            .shapes
            .iter()
            .filter(|shape| matches!(shape.shape, egui::epaint::Shape::Callback(_)))
            .count();
        assert_eq!(callbacks, 1, "{mode:?} must emit one active callback");
    }
}

#[test]
fn public_canvas_packet_action_paints_the_published_packet_in_the_same_frame() {
    let mut cache = MemoryStageCache::new();
    let initial = candidate(74, &mut cache);
    let (device, queue) = request_test_device();
    let format = eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut spherical_renderer =
        sekai::gpu::spherical::SphericalFieldRenderer::new(&device, format);
    let mut gpu = SphericalRendererPreparer::new(&mut spherical_renderer, &device, &queue);
    let mut published = PublishedSphericalPresentation::try_new(initial, &mut gpu).unwrap();
    let mut egui_renderer = eframe::egui_wgpu::Renderer::new(&device, format, None, 1, false);
    egui_renderer
        .callback_resources
        .insert::<sekai::gpu::spherical::SphericalFieldRenderer>(spherical_renderer);
    let context = egui::Context::default();
    let mut state = SphericalCanvasState::default();

    for action in [
        SphericalCanvasAction::SelectEntity(Some(SelectedSurfaceEntity::Cell(CellId::from_raw(0)))),
        SphericalCanvasAction::ZoomMap { factor: 4.0 },
    ] {
        let output = context.run(spherical_raw_input(Vec::new()), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let canvas = interact_spherical_canvas(ui, &published, &mut state);
                let rect = canvas.response().rect;
                assert!(canvas.into_actions().is_empty());
                let renderer = egui_renderer
                    .callback_resources
                    .get_mut::<sekai::gpu::spherical::SphericalFieldRenderer>()
                    .unwrap();
                apply_spherical_canvas_action(
                    &mut published,
                    renderer,
                    &device,
                    &queue,
                    &mut state,
                    action.clone(),
                )
                .unwrap_or_else(|error| panic!("{action:?}: {error:?}"));
                queue_spherical_canvas_callback(ui, &published, &state, rect);
            });
        });
        let jobs = context.tessellate(output.shapes, 1.0);
        let before = egui_renderer
            .callback_resources
            .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();
        render_queued_canvas_jobs(&device, &queue, &mut egui_renderer, &jobs);
        let after = egui_renderer
            .callback_resources
            .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();

        assert_eq!(after.uniforms, before.uniforms + 1, "{action:?}");
        assert_eq!(
            immutable_upload_counts(after),
            immutable_upload_counts(before),
            "{action:?}"
        );
        assert!(Arc::ptr_eq(
            published.gpu_packet().layers_arc(),
            published.layers_arc()
        ));
    }
    assert_eq!(
        published.state().selected_entity(),
        Some(SelectedSurfaceEntity::Cell(CellId::from_raw(0)))
    );
    assert_eq!(state.map_camera().zoom(state.projection().kind()), 4.0);
}

#[test]
fn public_canvas_uniform_actions_paint_current_pan_and_view_in_the_same_frame() {
    for action in [
        SphericalCanvasAction::PanMap {
            delta: [0.25, -0.125],
        },
        SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe),
        SphericalCanvasAction::SetFillVisible(false),
    ] {
        let mut cache = MemoryStageCache::new();
        let initial = candidate(745, &mut cache);
        let (device, queue) = request_test_device();
        let format = eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut spherical_renderer =
            sekai::gpu::spherical::SphericalFieldRenderer::new(&device, format);
        let mut gpu = SphericalRendererPreparer::new(&mut spherical_renderer, &device, &queue);
        let mut published = PublishedSphericalPresentation::try_new(initial, &mut gpu).unwrap();
        let mut egui_renderer = eframe::egui_wgpu::Renderer::new(&device, format, None, 1, false);
        egui_renderer
            .callback_resources
            .insert::<sekai::gpu::spherical::SphericalFieldRenderer>(spherical_renderer);
        let context = egui::Context::default();
        let mut state = SphericalCanvasState::default();
        let current_fill = published.state().fill_field().unwrap().clone();
        apply_spherical_canvas_action(
            &mut published,
            egui_renderer
                .callback_resources
                .get_mut::<sekai::gpu::spherical::SphericalFieldRenderer>()
                .unwrap(),
            &device,
            &queue,
            &mut state,
            SphericalCanvasAction::SelectFill(current_fill),
        )
        .unwrap();

        let baseline_output = context.run(spherical_raw_input(Vec::new()), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let canvas = interact_spherical_canvas(ui, &published, &mut state);
                let rect = canvas.response().rect;
                assert!(canvas.into_actions().is_empty());
                queue_spherical_canvas_callback(ui, &published, &state, rect);
            });
        });
        let baseline_jobs = context.tessellate(baseline_output.shapes, 1.0);
        let baseline =
            render_queued_canvas_jobs_to_bytes(&device, &queue, &mut egui_renderer, &baseline_jobs);

        let action_output = context.run(spherical_raw_input(Vec::new()), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let canvas = interact_spherical_canvas(ui, &published, &mut state);
                let rect = canvas.response().rect;
                assert!(canvas.into_actions().is_empty());
                let renderer = egui_renderer
                    .callback_resources
                    .get_mut::<sekai::gpu::spherical::SphericalFieldRenderer>()
                    .unwrap();
                apply_spherical_canvas_action(
                    &mut published,
                    renderer,
                    &device,
                    &queue,
                    &mut state,
                    action.clone(),
                )
                .unwrap_or_else(|error| panic!("{action:?}: {error:?}"));
                queue_spherical_canvas_callback(ui, &published, &state, rect);
            });
        });
        assert_eq!(
            action_output
                .shapes
                .iter()
                .filter(|shape| matches!(shape.shape, egui::epaint::Shape::Callback(_)))
                .count(),
            1
        );
        let jobs = context.tessellate(action_output.shapes, 1.0);
        let before = egui_renderer
            .callback_resources
            .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();
        let rendered =
            render_queued_canvas_jobs_to_bytes(&device, &queue, &mut egui_renderer, &jobs);
        let after = egui_renderer
            .callback_resources
            .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();

        assert_ne!(
            rendered, baseline,
            "{action:?} painted its predecessor uniform"
        );
        assert_eq!(after.uniforms, before.uniforms + 1, "{action:?}");
        assert_eq!(
            immutable_upload_counts(after),
            immutable_upload_counts(before),
            "{action:?}"
        );
        match action {
            SphericalCanvasAction::PanMap { delta } => {
                assert_eq!(state.map_camera().pan(state.projection().kind()), delta);
            }
            SphericalCanvasAction::SetViewMode(mode) => assert_eq!(state.view_mode(), mode),
            SphericalCanvasAction::SetFillVisible(visible) => {
                assert_eq!(state.field_state().fill_visible(), visible);
                assert_eq!(published.state().fill_visible(), visible);
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum PublicCanvasInputCase {
    Cell,
    Lod,
    Pan,
    View,
}

#[test]
fn public_canvas_output_actions_publish_before_same_frame_callback() {
    for case in [
        PublicCanvasInputCase::Cell,
        PublicCanvasInputCase::Lod,
        PublicCanvasInputCase::Pan,
        PublicCanvasInputCase::View,
    ] {
        let mut cache = MemoryStageCache::new();
        let initial = candidate(746, &mut cache);
        let (device, queue) = request_test_device();
        let format = eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut spherical_renderer =
            sekai::gpu::spherical::SphericalFieldRenderer::new(&device, format);
        let mut gpu = SphericalRendererPreparer::new(&mut spherical_renderer, &device, &queue);
        let mut published = PublishedSphericalPresentation::try_new(initial, &mut gpu).unwrap();
        let mut egui_renderer = eframe::egui_wgpu::Renderer::new(&device, format, None, 1, false);
        egui_renderer
            .callback_resources
            .insert::<sekai::gpu::spherical::SphericalFieldRenderer>(spherical_renderer);
        let context = egui::Context::default();
        let mut state = SphericalCanvasState::default();
        let current_fill = published.state().fill_field().unwrap().clone();
        apply_spherical_canvas_action(
            &mut published,
            egui_renderer
                .callback_resources
                .get_mut::<sekai::gpu::spherical::SphericalFieldRenderer>()
                .unwrap(),
            &device,
            &queue,
            &mut state,
            SphericalCanvasAction::SelectFill(current_fill),
        )
        .unwrap();

        let (canvas_rect, globe_button) =
            discover_public_canvas_layout(&context, &published, &mut state);
        let (baseline_actions, baseline_jobs) = run_public_canvas_action_frame(
            &context,
            spherical_raw_input(Vec::new()),
            &mut published,
            &mut state,
            &mut egui_renderer,
            &device,
            &queue,
        );
        assert!(baseline_actions.is_empty());
        let baseline =
            render_queued_canvas_jobs_to_bytes(&device, &queue, &mut egui_renderer, &baseline_jobs);

        let center = canvas_rect.center();
        let action_input = match case {
            PublicCanvasInputCase::Cell => {
                run_canvas_interaction_only(
                    &context,
                    spherical_raw_input(vec![
                        egui::Event::PointerMoved(center),
                        egui::Event::PointerButton {
                            pos: center,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]),
                    &published,
                    &mut state,
                );
                spherical_raw_input(vec![egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }])
            }
            PublicCanvasInputCase::Lod => spherical_raw_input(vec![
                egui::Event::PointerMoved(center),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 800.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]),
            PublicCanvasInputCase::Pan => {
                let start = center + egui::vec2(-40.0, 20.0);
                run_canvas_interaction_only(
                    &context,
                    spherical_raw_input(vec![
                        egui::Event::PointerMoved(start),
                        egui::Event::PointerButton {
                            pos: start,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]),
                    &published,
                    &mut state,
                );
                spherical_raw_input(vec![egui::Event::PointerMoved(
                    start + egui::vec2(90.0, -45.0),
                )])
            }
            PublicCanvasInputCase::View => {
                run_canvas_interaction_only(
                    &context,
                    spherical_raw_input(vec![
                        egui::Event::PointerMoved(globe_button),
                        egui::Event::PointerButton {
                            pos: globe_button,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]),
                    &published,
                    &mut state,
                );
                spherical_raw_input(vec![egui::Event::PointerButton {
                    pos: globe_button,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }])
            }
        };

        let (actions, jobs) = run_public_canvas_action_frame(
            &context,
            action_input,
            &mut published,
            &mut state,
            &mut egui_renderer,
            &device,
            &queue,
        );
        assert!(
            actions.iter().any(|action| matches!(
                (case, action),
                (
                    PublicCanvasInputCase::Cell,
                    SphericalCanvasAction::SelectEntity(Some(SelectedSurfaceEntity::Cell(_))),
                ) | (
                    PublicCanvasInputCase::Lod,
                    SphericalCanvasAction::ZoomMap { .. }
                ) | (
                    PublicCanvasInputCase::Pan,
                    SphericalCanvasAction::PanMap { .. }
                ) | (
                    PublicCanvasInputCase::View,
                    SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe),
                )
            )),
            "{case:?} did not emit its public canvas action: {actions:?}"
        );
        let before = egui_renderer
            .callback_resources
            .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();
        let rendered =
            render_queued_canvas_jobs_to_bytes(&device, &queue, &mut egui_renderer, &jobs);
        let after = egui_renderer
            .callback_resources
            .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters();
        assert_eq!(after.uniforms, before.uniforms + 1, "{case:?}");
        assert_eq!(
            immutable_upload_counts(after),
            immutable_upload_counts(before),
            "callback re-uploaded immutable data for {case:?}"
        );
        if matches!(
            case,
            PublicCanvasInputCase::Pan | PublicCanvasInputCase::View
        ) {
            assert_ne!(rendered, baseline, "{case:?} painted its old uniform");
        }
        assert!(Arc::ptr_eq(
            published.gpu_packet().layers_arc(),
            published.layers_arc()
        ));
        assert_eq!(published.state(), state.field_state());
    }
}

#[derive(Clone, Copy, Debug)]
enum PacketChange {
    Field,
    Projection,
    Whole,
}

#[test]
fn queued_stale_callbacks_cannot_rollback_field_projection_or_whole_publications() {
    for change in [
        PacketChange::Field,
        PacketChange::Projection,
        PacketChange::Whole,
    ] {
        assert_queued_stale_callback_is_inert(change);
    }
}

fn assert_queued_stale_callback_is_inert(change: PacketChange) {
    let mut cache = MemoryStageCache::new();
    let initial = candidate(75, &mut cache);
    let (device, queue) = request_test_device();
    let format = eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut spherical_renderer =
        sekai::gpu::spherical::SphericalFieldRenderer::new(&device, format);
    let mut gpu = SphericalRendererPreparer::new(&mut spherical_renderer, &device, &queue);
    let mut published = PublishedSphericalPresentation::try_new(initial, &mut gpu).unwrap();
    let mut egui_renderer = eframe::egui_wgpu::Renderer::new(&device, format, None, 1, false);
    egui_renderer
        .callback_resources
        .insert::<sekai::gpu::spherical::SphericalFieldRenderer>(spherical_renderer);
    let context = egui::Context::default();
    let mut state = SphericalCanvasState::default();
    let stale_jobs = queued_canvas_jobs(&context, &published, &mut state);

    match change {
        PacketChange::Field => {
            let renderer = egui_renderer
                .callback_resources
                .get_mut::<sekai::gpu::spherical::SphericalFieldRenderer>()
                .unwrap();
            apply_spherical_canvas_action(
                &mut published,
                renderer,
                &device,
                &queue,
                &mut state,
                SphericalCanvasAction::SelectFill(land_ocean_field_id()),
            )
            .unwrap();
        }
        PacketChange::Projection => {
            let renderer = egui_renderer
                .callback_resources
                .get_mut::<sekai::gpu::spherical::SphericalFieldRenderer>()
                .unwrap();
            apply_spherical_canvas_action(
                &mut published,
                renderer,
                &device,
                &queue,
                &mut state,
                SphericalCanvasAction::SetCentralMeridianRadians(0.75),
            )
            .unwrap();
        }
        PacketChange::Whole => {
            let replacement = published
                .prepare_replacement_candidate(
                    RootSeed::new(76),
                    &space(),
                    &WorldFormationSpec::default(),
                    &TectonicSpec::default(),
                    &ReliefSpec::default(),
                    &GeologicSpec::default(),
                    &mut cache,
                    published.state(),
                )
                .unwrap();
            let renderer = egui_renderer
                .callback_resources
                .get_mut::<sekai::gpu::spherical::SphericalFieldRenderer>()
                .unwrap();
            let mut gpu = SphericalRendererPreparer::new(renderer, &device, &queue);
            published.try_replace(replacement, &mut gpu).unwrap();
        }
    }

    let expected_source = published.source().clone();
    let immutable_before = immutable_upload_counts(
        egui_renderer
            .callback_resources
            .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
            .unwrap()
            .upload_counters(),
    );
    render_queued_canvas_jobs(&device, &queue, &mut egui_renderer, &stale_jobs);
    let renderer = egui_renderer
        .callback_resources
        .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
        .unwrap();
    assert_eq!(
        renderer.installed_source(),
        Some(&expected_source),
        "{change:?}"
    );
    assert_eq!(
        immutable_upload_counts(renderer.upload_counters()),
        immutable_before,
        "{change:?} stale prepare performed an immutable rollback upload"
    );

    let current_jobs = queued_canvas_jobs(&context, &published, &mut state);
    render_queued_canvas_jobs(&device, &queue, &mut egui_renderer, &current_jobs);
    let renderer = egui_renderer
        .callback_resources
        .get::<sekai::gpu::spherical::SphericalFieldRenderer>()
        .unwrap();
    assert_eq!(
        renderer.installed_source(),
        Some(&expected_source),
        "{change:?}"
    );
    assert_eq!(
        immutable_upload_counts(renderer.upload_counters()),
        immutable_before,
        "{change:?} current prepare must reuse its installed immutable packet"
    );
}

fn immutable_upload_counts(counters: sekai::gpu::spherical::SphericalUploadCounters) -> [u64; 7] {
    [
        counters.map_geometry,
        counters.globe_geometry,
        counters.fill_field,
        counters.diagnostics,
        counters.palettes,
        counters.map_overlay_instances,
        counters.globe_overlay_instances,
    ]
}

fn read_prepared_map_pixels(
    device: &eframe::egui_wgpu::wgpu::Device,
    queue: &eframe::egui_wgpu::wgpu::Queue,
    renderer: &sekai::gpu::spherical::SphericalFieldRenderer,
    viewport: [u32; 2],
) -> Vec<u8> {
    use eframe::egui_wgpu::wgpu;

    let extent = wgpu::Extent3d {
        width: viewport[0],
        height: viewport[1],
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Spherical Publication Ownership Readback Target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let unpadded_bytes_per_row = viewport[0] * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Spherical Publication Ownership Readback Buffer"),
        size: u64::from(padded_bytes_per_row) * u64::from(viewport[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Spherical Publication Ownership Readback Encoder"),
    });
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Spherical Publication Ownership Readback Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        renderer.paint(sekai::gpu::spherical::SphericalRenderMode::Map, &mut pass);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(viewport[1]),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender
            .send(result)
            .expect("ownership readback receiver lives");
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .expect("ownership readback callback runs")
        .expect("ownership readback maps");
    let mapped = slice.get_mapped_range();
    let mut rgba8 = vec![0; (unpadded_bytes_per_row * viewport[1]) as usize];
    for row in 0..viewport[1] as usize {
        let source_start = row * padded_bytes_per_row as usize;
        let target_start = row * unpadded_bytes_per_row as usize;
        rgba8[target_start..target_start + unpadded_bytes_per_row as usize]
            .copy_from_slice(&mapped[source_start..source_start + unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    readback.unmap();
    rgba8
}

fn queued_canvas_jobs(
    context: &egui::Context,
    presentation: &PublishedSphericalPresentation,
    state: &mut SphericalCanvasState,
) -> Vec<egui::ClippedPrimitive> {
    let output = context.run(spherical_raw_input(Vec::new()), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            let canvas = interact_spherical_canvas(ui, presentation, state);
            let rect = canvas.response().rect;
            assert!(canvas.into_actions().is_empty());
            queue_spherical_canvas_callback(ui, presentation, state, rect);
        });
    });
    context.tessellate(output.shapes, 1.0)
}

fn render_queued_canvas_jobs(
    device: &eframe::egui_wgpu::wgpu::Device,
    queue: &eframe::egui_wgpu::wgpu::Queue,
    renderer: &mut eframe::egui_wgpu::Renderer,
    jobs: &[egui::ClippedPrimitive],
) {
    let _ = render_queued_canvas_jobs_to_bytes(device, queue, renderer, jobs);
}

fn render_queued_canvas_jobs_to_bytes(
    device: &eframe::egui_wgpu::wgpu::Device,
    queue: &eframe::egui_wgpu::wgpu::Queue,
    renderer: &mut eframe::egui_wgpu::Renderer,
    jobs: &[egui::ClippedPrimitive],
) -> Vec<u8> {
    use eframe::egui_wgpu::wgpu;

    const WIDTH: u32 = 800;
    const HEIGHT: u32 = 600;
    let descriptor = eframe::egui_wgpu::ScreenDescriptor {
        size_in_pixels: [WIDTH, HEIGHT],
        pixels_per_point: 1.0,
    };
    let extent = wgpu::Extent3d {
        width: WIDTH,
        height: HEIGHT,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Task 10 callback lifecycle target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let unpadded_bytes_per_row = WIDTH * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Task 10 callback lifecycle readback"),
        size: u64::from(padded_bytes_per_row) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Task 10 callback lifecycle encoder"),
    });
    renderer.update_buffers(device, queue, &mut encoder, jobs, &descriptor);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Task 10 callback lifecycle pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, jobs, &descriptor);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);

    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).expect("mapping receiver remains alive");
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .expect("mapping callback runs")
        .expect("callback target maps");
    let mapped = slice.get_mapped_range();
    let mut rgba8 = vec![0; unpadded_bytes_per_row as usize * HEIGHT as usize];
    for row in 0..HEIGHT as usize {
        let source_start = row * padded_bytes_per_row as usize;
        let target_start = row * unpadded_bytes_per_row as usize;
        rgba8[target_start..target_start + unpadded_bytes_per_row as usize]
            .copy_from_slice(&mapped[source_start..source_start + unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    readback.unmap();
    rgba8
}

#[test]
fn multi_frame_canvas_drag_applies_only_each_frames_pointer_delta_in_map_and_globe() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(77, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
    let published = PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap();

    for mode in [SphericalViewMode::Map, SphericalViewMode::Globe] {
        let context = egui::Context::default();
        let mut state = SphericalCanvasState::default();
        state
            .apply(SphericalCanvasAction::SetViewMode(mode))
            .unwrap();
        let (rect, initial_actions) = run_spherical_canvas_frame(
            &context,
            spherical_raw_input(Vec::new()),
            &published,
            &mut state,
        );
        assert!(initial_actions.is_empty());
        let start = rect.center() + egui::vec2(-60.0, 20.0);
        let middle = start + egui::vec2(30.0, -10.0);
        let end = start + egui::vec2(80.0, -35.0);

        for events in [
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerMoved(middle)],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let (_, actions) = run_spherical_canvas_frame(
                &context,
                spherical_raw_input(events),
                &published,
                &mut state,
            );
            for action in actions {
                state.apply(action).unwrap();
            }
        }

        let canvas_size = [f64::from(rect.width()), f64::from(rect.height())];
        let local_start = [
            f64::from(start.x - rect.min.x),
            f64::from(start.y - rect.min.y),
        ];
        let local_end = [f64::from(end.x - rect.min.x), f64::from(end.y - rect.min.y)];
        let local_middle = [
            f64::from(middle.x - rect.min.x),
            f64::from(middle.y - rect.min.y),
        ];
        match mode {
            SphericalViewMode::Map => {
                let actual = state.map_camera().pan(state.projection().kind());
                let expected = [
                    (local_end[0] - local_start[0]) / canvas_size[0],
                    -(local_end[1] - local_start[1]) / canvas_size[1],
                ];
                assert!(
                    (actual[0] - expected[0]).abs() < 1.0e-8,
                    "map x: actual={actual:?}, expected={expected:?}"
                );
                assert!(
                    (actual[1] - expected[1]).abs() < 1.0e-8,
                    "map y: actual={actual:?}, expected={expected:?}"
                );
            }
            SphericalViewMode::Globe => {
                let mut expected = SphericalCanvasState::default();
                expected
                    .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe))
                    .unwrap();
                expected
                    .apply(SphericalCanvasAction::TrackballGlobe {
                        start: local_start,
                        end: local_middle,
                        canvas_size,
                    })
                    .unwrap();
                expected
                    .apply(SphericalCanvasAction::TrackballGlobe {
                        start: local_middle,
                        end: local_end,
                        canvas_size,
                    })
                    .unwrap();
                for (actual, expected) in state
                    .globe_camera()
                    .orientation_xyzw()
                    .into_iter()
                    .zip(expected.globe_camera().orientation_xyzw())
                {
                    assert!((actual - expected).abs() < 1.0e-8);
                }
            }
        }
    }
}

#[test]
fn map_and_globe_picking_share_locator_and_edge_hits_are_incident_and_pixel_bounded() {
    use sekai::world::spatial::UnitVector3;

    let mut cache = MemoryStageCache::new();
    let candidate = candidate(79, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
    let published = PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap();
    let canvas_size = [800.0, 600.0];
    let north = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
    let expected_cell = published.locator().locate_cell(north).unwrap();
    let mut state = SphericalCanvasState::default();

    let map_point = state.projection().forward(north).unwrap();
    let map_screen = map_screen_position(
        state.projection(),
        state.map_camera(),
        map_point,
        canvas_size,
    );
    assert_eq!(
        state.pick_screen(&published, map_screen, canvas_size, 1.0),
        Some(SelectedSurfaceEntity::Cell(expected_cell))
    );

    state
        .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe))
        .unwrap();
    assert_eq!(
        state.pick_screen(&published, [400.0, 300.0], canvas_size, 1.0),
        Some(SelectedSurfaceEntity::Cell(expected_cell))
    );

    state
        .apply(SphericalCanvasAction::SetViewMode(SphericalViewMode::Map))
        .unwrap();
    state
        .apply(SphericalCanvasAction::SelectOverlay(Some(
            boundary_strength_field_id(),
        )))
        .unwrap();
    let segment = published
        .map()
        .edge_segments()
        .iter()
        .find(|segment| {
            let midpoint = sekai::view::ProjectionPoint::new(
                (segment.start().x() + segment.end().x()) * 0.5,
                (segment.start().y() + segment.end().y()) * 0.5,
            );
            state.projection().inverse(midpoint).is_ok()
        })
        .unwrap();
    let segment_midpoint = sekai::view::ProjectionPoint::new(
        (segment.start().x() + segment.end().x()) * 0.5,
        (segment.start().y() + segment.end().y()) * 0.5,
    );
    let edge_screen = map_screen_position(
        state.projection(),
        state.map_camera(),
        segment_midpoint,
        canvas_size,
    );
    assert_eq!(
        state.pick_screen(&published, edge_screen, canvas_size, 1.0),
        Some(SelectedSurfaceEntity::Edge(segment.edge()))
    );
    let tiny_canvas = [1.0, 1.0];
    let tiny_center = [0.5, 0.5];
    assert!(matches!(
        state.pick_screen(&published, tiny_center, tiny_canvas, 1.0),
        Some(SelectedSurfaceEntity::Edge(_))
    ));
    assert!(!published.map().edge_segments().is_empty());
}

#[test]
fn real_publication_actions_preserve_exact_arcs_and_skip_camera_and_phase_uploads() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(83, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut published = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap()
    };
    let mut state = SphericalCanvasState::default();

    apply_spherical_canvas_action(
        &mut published,
        &mut renderer,
        &device,
        &queue,
        &mut state,
        SphericalCanvasAction::SelectOverlay(Some(preliminary_prevailing_wind_m_s_field_id())),
    )
    .unwrap();
    let map_before = Arc::clone(published.map_arc());
    let globe_before = Arc::clone(published.globe_arc());
    let layers_before = Arc::clone(published.layers_arc());
    let packet_before = Arc::clone(published.gpu_packet_arc());
    let counters_before = renderer.upload_counters();

    for action in [
        SphericalCanvasAction::SetViewMode(SphericalViewMode::Globe),
        SphericalCanvasAction::ZoomGlobe { factor: 1.1 },
        SphericalCanvasAction::AdvanceVectorPhase {
            frame_delta_seconds: 0.25,
        },
    ] {
        apply_spherical_canvas_action(
            &mut published,
            &mut renderer,
            &device,
            &queue,
            &mut state,
            action,
        )
        .unwrap();
    }
    assert!(Arc::ptr_eq(&map_before, published.map_arc()));
    assert!(Arc::ptr_eq(&globe_before, published.globe_arc()));
    assert!(Arc::ptr_eq(&layers_before, published.layers_arc()));
    assert!(Arc::ptr_eq(&packet_before, published.gpu_packet_arc()));
    assert_eq!(renderer.upload_counters(), counters_before);

    apply_spherical_canvas_action(
        &mut published,
        &mut renderer,
        &device,
        &queue,
        &mut state,
        SphericalCanvasAction::SetCentralMeridianRadians(0.5),
    )
    .unwrap();
    assert!(!Arc::ptr_eq(&map_before, published.map_arc()));
    assert!(Arc::ptr_eq(&globe_before, published.globe_arc()));
    assert!(Arc::ptr_eq(&layers_before, published.layers_arc()));
    assert_eq!(
        renderer.upload_counters().map_geometry,
        counters_before.map_geometry + 1
    );
    assert_eq!(
        renderer.upload_counters().globe_geometry,
        counters_before.globe_geometry
    );
    assert_eq!(
        renderer.upload_counters().fill_field,
        counters_before.fill_field
    );
}

#[test]
fn publication_selection_transitions_remove_old_cell_layers_and_keep_edge_none_o1() {
    let mut cache = MemoryStageCache::new();
    let candidate = candidate(89, &mut cache);
    let (device, queue) = request_test_device();
    let mut renderer = sekai::gpu::spherical::SphericalFieldRenderer::new(
        &device,
        eframe::egui_wgpu::wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut published = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap()
    };
    let mut state = SphericalCanvasState::default();
    apply_spherical_canvas_action(
        &mut published,
        &mut renderer,
        &device,
        &queue,
        &mut state,
        SphericalCanvasAction::SelectOverlay(Some(preliminary_prevailing_wind_m_s_field_id())),
    )
    .unwrap();
    let map = Arc::clone(published.map_arc());
    let globe = Arc::clone(published.globe_arc());

    let mut previous_layers = Arc::clone(published.layers_arc());
    let mut previous_revisions = published.layers().revisions();
    let mut previous_counters = renderer.upload_counters();
    for selected in [CellId::from_raw(1), CellId::from_raw(2)] {
        assert_eq!(
            apply_spherical_canvas_action(
                &mut published,
                &mut renderer,
                &device,
                &queue,
                &mut state,
                SphericalCanvasAction::SelectEntity(Some(SelectedSurfaceEntity::Cell(selected))),
            )
            .unwrap(),
            SphericalCanvasInvalidation::FIELD_LAYERS,
        );
        assert_eq!(
            published.state().selected_entity(),
            Some(SelectedSurfaceEntity::Cell(selected))
        );
        assert_eq!(published.layers().selected_vector_cell(), Some(selected));
        assert!(Arc::ptr_eq(&map, published.map_arc()));
        assert!(Arc::ptr_eq(&globe, published.globe_arc()));
        assert!(!Arc::ptr_eq(&previous_layers, published.layers_arc()));
        let revisions = published.layers().revisions();
        assert_eq!(revisions.fill, previous_revisions.fill);
        assert_eq!(revisions.overlay, previous_revisions.overlay);
        assert_eq!(revisions.diagnostics, previous_revisions.diagnostics);
        assert_eq!(revisions.fill_palette, previous_revisions.fill_palette);
        assert_eq!(
            revisions.overlay_palette,
            previous_revisions.overlay_palette
        );
        assert!(revisions.vector_glyphs > previous_revisions.vector_glyphs);
        let counters = renderer.upload_counters();
        assert_eq!(counters.map_geometry, previous_counters.map_geometry);
        assert_eq!(counters.globe_geometry, previous_counters.globe_geometry);
        assert_eq!(counters.fill_field, previous_counters.fill_field);
        assert_eq!(counters.diagnostics, previous_counters.diagnostics);
        assert_eq!(
            counters.map_overlay_instances,
            previous_counters.map_overlay_instances + 1
        );
        assert_eq!(
            counters.globe_overlay_instances,
            previous_counters.globe_overlay_instances + 1
        );
        previous_layers = Arc::clone(published.layers_arc());
        previous_revisions = revisions;
        previous_counters = counters;
    }

    assert_eq!(
        apply_spherical_canvas_action(
            &mut published,
            &mut renderer,
            &device,
            &queue,
            &mut state,
            SphericalCanvasAction::SelectOverlay(Some(boundary_strength_field_id())),
        )
        .unwrap(),
        SphericalCanvasInvalidation::FIELD_LAYERS,
    );
    previous_layers = Arc::clone(published.layers_arc());
    previous_revisions = published.layers().revisions();
    previous_counters = renderer.upload_counters();

    let edge = SelectedSurfaceEntity::Edge(EdgeId::from_raw(1));
    assert_eq!(
        apply_spherical_canvas_action(
            &mut published,
            &mut renderer,
            &device,
            &queue,
            &mut state,
            SphericalCanvasAction::SelectEntity(Some(edge)),
        )
        .unwrap(),
        SphericalCanvasInvalidation::FIELD_LAYERS,
    );
    assert_eq!(published.state().selected_entity(), Some(edge));
    assert_eq!(published.layers().selected_vector_cell(), None);
    let edge_layers = Arc::clone(published.layers_arc());
    let edge_packet = Arc::clone(published.gpu_packet_arc());
    let edge_revisions = published.layers().revisions();
    assert_eq!(edge_revisions.diagnostics, previous_revisions.diagnostics);
    let edge_counters = renderer.upload_counters();
    assert!(!Arc::ptr_eq(&previous_layers, published.layers_arc()));
    assert_eq!(edge_revisions, previous_revisions);
    assert_eq!(edge_counters, previous_counters);

    for selection in [
        None,
        Some(SelectedSurfaceEntity::Edge(EdgeId::from_raw(2))),
        None,
    ] {
        assert_eq!(
            apply_spherical_canvas_action(
                &mut published,
                &mut renderer,
                &device,
                &queue,
                &mut state,
                SphericalCanvasAction::SelectEntity(selection),
            )
            .unwrap(),
            SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM,
        );
        assert_eq!(published.state().selected_entity(), selection);
        assert_eq!(published.layers().selected_vector_cell(), None);
        assert!(Arc::ptr_eq(&edge_layers, published.layers_arc()));
        assert!(Arc::ptr_eq(&edge_packet, published.gpu_packet_arc()));
        assert_eq!(published.layers().revisions(), edge_revisions);
        assert_eq!(renderer.upload_counters(), edge_counters);
    }
    let unselected =
        build_spherical_inspector_model(&published, published.state(), SphericalViewMode::Map)
            .unwrap();
    assert!(unselected
        .diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.cell().is_none()));
}

#[test]
fn legacy_compatibility_ui_exposes_only_notice_and_explicit_one_way_action() {
    let legacy: TemplateApp =
        serde_json::from_value(serde_json::json!({ "world_seed": 7 })).unwrap();
    let model = legacy_compatibility_ui(&legacy).unwrap();
    assert!(model.notice().contains("旧平面世界"));
    assert_eq!(model.action_label(), "用当前作者参数重新生成球面世界");
    assert!(!model.notice().contains("模式切换"));
    assert!(legacy_compatibility_ui(&TemplateApp::default()).is_none());
}

fn map_screen_position(
    projection: SphericalProjection,
    camera: MapCamera,
    point: sekai::view::ProjectionPoint,
    canvas_size: [f64; 2],
) -> [f64; 2] {
    let bounds = projection.bounds();
    let bounds_width = bounds.max_x() - bounds.min_x();
    let bounds_height = bounds.max_y() - bounds.min_y();
    let aspect = canvas_size[0] / canvas_size[1];
    let map_aspect = bounds_width / bounds_height;
    let (fit_x, fit_y) = if aspect >= map_aspect {
        (2.0 / (bounds_height * aspect), 2.0 / bounds_height)
    } else {
        (2.0 / bounds_width, 2.0 * aspect / bounds_width)
    };
    let zoom = camera.zoom(projection.kind());
    let pan = camera.pan(projection.kind());
    let center_x = (bounds.min_x() + bounds.max_x()) * 0.5;
    let center_y = (bounds.min_y() + bounds.max_y()) * 0.5;
    let ndc_x = (point.x() - center_x) * fit_x * zoom + pan[0] * 2.0;
    let ndc_y = (point.y() - center_y) * fit_y * zoom + pan[1] * 2.0;
    [
        (ndc_x + 1.0) * 0.5 * canvas_size[0],
        (1.0 - ndc_y) * 0.5 * canvas_size[1],
    ]
}

fn spherical_raw_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        events,
        ..egui::RawInput::default()
    }
}

fn run_spherical_controls_frame(
    context: &egui::Context,
    input: egui::RawInput,
    presentation: &PublishedSphericalPresentation,
    state: &SphericalCanvasState,
) -> (Vec<SphericalCanvasAction>, egui::FullOutput) {
    let mut actions = Vec::new();
    let output = context.run(input, |context| {
        egui::SidePanel::left("spherical-layer-controls-test").show(context, |ui| {
            actions = show_spherical_controls(ui, presentation, state).unwrap();
        });
    });
    (actions, output)
}

fn click_spherical_control(
    context: &egui::Context,
    position: egui::Pos2,
    presentation: &PublishedSphericalPresentation,
    state: &SphericalCanvasState,
) -> Vec<SphericalCanvasAction> {
    let (pressed, _) = run_spherical_controls_frame(
        context,
        spherical_raw_input(vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]),
        presentation,
        state,
    );
    assert!(pressed.is_empty());
    run_spherical_controls_frame(
        context,
        spherical_raw_input(vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]),
        presentation,
        state,
    )
    .0
}

fn discover_public_canvas_layout(
    context: &egui::Context,
    presentation: &PublishedSphericalPresentation,
    state: &mut SphericalCanvasState,
) -> (egui::Rect, egui::Pos2) {
    let mut canvas_rect = None;
    let output = context.run(spherical_raw_input(Vec::new()), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            canvas_rect = Some(
                interact_spherical_canvas(ui, presentation, state)
                    .response()
                    .rect,
            );
        });
    });
    let globe_button = output
        .shapes
        .iter()
        .find_map(|shape| find_text_center(&shape.shape, "三维球体"))
        .expect("the public globe tab is visible");
    (canvas_rect.unwrap(), globe_button)
}

fn find_text_center(shape: &egui::epaint::Shape, text: &str) -> Option<egui::Pos2> {
    match shape {
        egui::epaint::Shape::Text(shape) if shape.galley.text() == text => {
            Some(shape.visual_bounding_rect().center())
        }
        egui::epaint::Shape::Vec(shapes) => shapes
            .iter()
            .find_map(|shape| find_text_center(shape, text)),
        _ => None,
    }
}

fn run_canvas_interaction_only(
    context: &egui::Context,
    input: egui::RawInput,
    presentation: &PublishedSphericalPresentation,
    state: &mut SphericalCanvasState,
) {
    let _ = context.run(input, |context| {
        egui::CentralPanel::default().show(context, |ui| {
            assert!(
                interact_spherical_canvas(ui, presentation, state)
                    .into_actions()
                    .is_empty(),
                "pointer-down priming frame must not publish an action"
            );
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn run_public_canvas_action_frame(
    context: &egui::Context,
    input: egui::RawInput,
    presentation: &mut PublishedSphericalPresentation,
    state: &mut SphericalCanvasState,
    renderer: &mut eframe::egui_wgpu::Renderer,
    device: &eframe::egui_wgpu::wgpu::Device,
    queue: &eframe::egui_wgpu::wgpu::Queue,
) -> (Vec<SphericalCanvasAction>, Vec<egui::ClippedPrimitive>) {
    let mut frame_actions = Vec::new();
    let output = context.run(input, |context| {
        egui::CentralPanel::default().show(context, |ui| {
            let canvas = interact_spherical_canvas(ui, presentation, state);
            let rect = canvas.response().rect;
            frame_actions = canvas.into_actions();
            for action in frame_actions.iter().cloned() {
                apply_spherical_canvas_action(
                    presentation,
                    renderer
                        .callback_resources
                        .get_mut::<sekai::gpu::spherical::SphericalFieldRenderer>()
                        .unwrap(),
                    device,
                    queue,
                    state,
                    action,
                )
                .unwrap();
            }
            queue_spherical_canvas_callback(ui, presentation, state, rect);
        });
    });
    (frame_actions, context.tessellate(output.shapes, 1.0))
}

fn run_spherical_canvas_frame(
    context: &egui::Context,
    input: egui::RawInput,
    presentation: &PublishedSphericalPresentation,
    state: &mut SphericalCanvasState,
) -> (egui::Rect, Vec<SphericalCanvasAction>) {
    let mut canvas_output = None;
    let _ = context.run(input, |context| {
        egui::CentralPanel::default().show(context, |ui| {
            canvas_output = Some(interact_spherical_canvas(ui, presentation, state));
        });
    });
    let canvas_output = canvas_output.unwrap();
    let rect = canvas_output.response().rect;
    (rect, canvas_output.into_actions())
}
