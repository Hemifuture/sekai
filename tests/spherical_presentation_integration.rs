use std::sync::Arc;

use sekai::app::{
    build_spherical_external_artifacts, build_spherical_presentation_candidate,
    PublishedSphericalPresentation, SphericalGlobePresenter, SphericalMapPresenter,
    SphericalPresentationError, SphericalRendererPreparer,
};
use sekai::engine::{ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, HydroErosionSpecArtifact,
    RulePackSetArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SphericalSpaceArtifact};
use sekai::view::{
    DisplayRevisionClock, GlobeCamera, MapCamera, SphericalFieldDisplayState, SphericalMeshBudgets,
    SphericalProjection, SphericalProjectionKind, SphericalViewMode,
};
use sekai::world::natural::{
    preliminary_prevailing_wind_m_s_field_id, GeologicSpec, TectonicSpec, WorldFormationSpec,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

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
    assert_eq!(external.len(), 8);
    assert!(external.hash::<SphericalSpaceArtifact>().is_ok());
    assert!(external.hash::<TectonicSpecArtifact>().is_ok());
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
