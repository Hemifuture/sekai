use sekai::gpu::spherical::{
    SphericalFieldRenderer, SphericalGpuPacket, SphericalPaintCallback, SphericalRenderError,
    SphericalRenderMode, SphericalUploadCounters,
};
use std::sync::{mpsc, Arc};

use eframe::egui_wgpu::wgpu;
use sekai::app::{build_spherical_presentation_candidate_for_view, SphericalPresentationCandidate};
use sekai::engine::MemoryStageCache;
use sekai::view::{
    prepare_spherical_field_layers, DiagnosticScope, DisplayRevisionClock, GlobeCamera, MapCamera,
    OwnedViewDiagnostic, PreparedFieldKind, PreparedFieldLayers, PreparedOverlayKind,
    PreparedSphericalOverlay, SphericalFieldDisplayState, SphericalPresentationViewState,
    SphericalProjection, SphericalProjectionKind, SphericalViewMode, VectorAnimationUniform,
    ViewDiagnosticSeverity,
};
use sekai::world::natural::{
    boundary_kind_field_id, boundary_strength_field_id, land_ocean_field_id,
    preliminary_prevailing_wind_m_s_field_id, surface_elevation_m_field_id, GeologicSpec,
    TectonicSpec, WorldFormationSpec,
};
use sekai::world::spatial::UnitVector3;
use sekai::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

const GOLDEN_WIDTH: u32 = 192;
const GOLDEN_HEIGHT: u32 = 96;
const GOLDEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[test]
fn spherical_gpu_public_counters_start_empty_and_modes_are_distinct() {
    assert_ne!(SphericalRenderMode::Map, SphericalRenderMode::Globe);
    assert_eq!(SphericalRenderMode::default(), SphericalRenderMode::Map);

    let counters = SphericalUploadCounters::default();
    assert_eq!(counters.map_geometry, 0);
    assert_eq!(counters.globe_geometry, 0);
    assert_eq!(counters.fill_field, 0);
    assert_eq!(counters.diagnostics, 0);
    assert_eq!(counters.palettes, 0);
    assert_eq!(counters.map_overlay_instances, 0);
    assert_eq!(counters.globe_overlay_instances, 0);
    assert_eq!(counters.uniforms, 0);
    assert_eq!(counters.uploaded_bytes, 0);
}

#[test]
fn spherical_gpu_rejections_have_stable_typed_contracts() {
    assert_eq!(
        SphericalRenderError::CardinalityMismatch {
            resource: "fill field",
            expected: 12,
            actual: 11,
        }
        .to_string(),
        "fill field cardinality 11 does not match spherical geometry cardinality 12"
    );
    assert_eq!(
        SphericalRenderError::BufferLimitExceeded {
            resource: "map vertices",
            required: 257,
            max: 256,
        }
        .to_string(),
        "map vertices require 257 GPU buffer bytes, limit is 256"
    );
    assert_eq!(
        SphericalRenderError::IntegerOverflow {
            context: "map index count",
        }
        .to_string(),
        "integer overflow while computing map index count"
    );
}

#[test]
fn spherical_paint_callback_implements_the_egui_wgpu_callback_contract() {
    fn assert_callback<T: eframe::egui_wgpu::CallbackTrait>() {}
    assert_callback::<SphericalPaintCallback>();
}

#[test]
fn complete_spherical_offscreen_rgba8_goldens_keep_cpu_semantic_oracles() {
    let (adapter_info, device, queue) = request_test_device();
    println!(
        "spherical golden adapter: {:?} {:?} {:?}; {}x{} {:?}",
        adapter_info.name,
        adapter_info.backend,
        adapter_info.device_type,
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
        GOLDEN_FORMAT,
    );
    let mut cache = MemoryStageCache::new();
    let default_view = SphericalPresentationViewState::default();

    let scalar = candidate(
        0x60_01,
        state(surface_elevation_m_field_id(), None, false),
        default_view,
        &mut cache,
    );
    assert_fill_semantics(&scalar, surface_elevation_m_field_id());
    assert_undeformed_globe(&scalar);
    let map_scalar = render(
        &device,
        &queue,
        &scalar,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_scalar = render(
        &device,
        &queue,
        &scalar,
        SphericalRenderMode::Globe,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );

    let category = candidate(
        0x60_01,
        state(land_ocean_field_id(), None, false),
        default_view,
        &mut cache,
    );
    assert_fill_semantics(&category, land_ocean_field_id());
    let map_category = render(
        &device,
        &queue,
        &category,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_category = render(
        &device,
        &queue,
        &category,
        SphericalRenderMode::Globe,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );

    let edge_scalar = candidate(
        0x60_01,
        state(
            surface_elevation_m_field_id(),
            Some(boundary_strength_field_id()),
            false,
        ),
        default_view,
        &mut cache,
    );
    assert_overlay_semantics(
        &edge_scalar,
        boundary_strength_field_id(),
        PreparedOverlayKind::EdgeScalar,
    );
    let map_edge_scalar = render(
        &device,
        &queue,
        &edge_scalar,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_edge_scalar = render(
        &device,
        &queue,
        &edge_scalar,
        SphericalRenderMode::Globe,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );

    let edge_category = candidate(
        0x60_01,
        state(
            surface_elevation_m_field_id(),
            Some(boundary_kind_field_id()),
            false,
        ),
        default_view,
        &mut cache,
    );
    assert_overlay_semantics(
        &edge_category,
        boundary_kind_field_id(),
        PreparedOverlayKind::EdgeCategory,
    );
    let map_edge_category = render(
        &device,
        &queue,
        &edge_category,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_edge_category = render(
        &device,
        &queue,
        &edge_category,
        SphericalRenderMode::Globe,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );

    let vector = candidate(
        0x60_01,
        state(
            surface_elevation_m_field_id(),
            Some(preliminary_prevailing_wind_m_s_field_id()),
            false,
        ),
        default_view,
        &mut cache,
    );
    assert_overlay_semantics(
        &vector,
        preliminary_prevailing_wind_m_s_field_id(),
        PreparedOverlayKind::CellVector,
    );
    let map_vector_paused = render(
        &device,
        &queue,
        &vector,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let map_vector_animated = render(
        &device,
        &queue,
        &vector,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.375),
    );
    let globe_vector_paused = render(
        &device,
        &queue,
        &vector,
        SphericalRenderMode::Globe,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_vector_animated = render(
        &device,
        &queue,
        &vector,
        SphericalRenderMode::Globe,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.375),
    );
    assert_ne!(map_vector_paused, map_vector_animated);
    assert_ne!(globe_vector_paused, globe_vector_animated);

    let seam_projection =
        SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.73).unwrap();
    let seam_view = SphericalPresentationViewState::new(
        SphericalViewMode::Map,
        seam_projection,
        MapCamera::default(),
        GlobeCamera::default(),
    );
    let seam = candidate(
        0x60_01,
        state(surface_elevation_m_field_id(), None, false),
        seam_view,
        &mut cache,
    );
    assert_seam_geometry(&seam);
    let map_seam = render(
        &device,
        &queue,
        &seam,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );

    let pole_projection =
        SphericalProjection::new(SphericalProjectionKind::Equirectangular, -0.41).unwrap();
    let pole_view = SphericalPresentationViewState::new(
        SphericalViewMode::Map,
        pole_projection,
        MapCamera::default(),
        GlobeCamera::default(),
    );
    let poles = candidate(
        0x60_01,
        state(surface_elevation_m_field_id(), None, false),
        pole_view,
        &mut cache,
    );
    assert_pole_semantics(&poles);
    let map_poles = render(
        &device,
        &queue,
        &poles,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );

    let front_camera = GlobeCamera::default();
    let back_camera = GlobeCamera::from_orientation_xyzw([0.0, 1.0, 0.0, 0.0], 1.0).unwrap();
    let north = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
    assert!(front_camera.is_front_facing(north));
    assert!(!back_camera.is_front_facing(north));
    let globe_front = render(
        &device,
        &queue,
        &category,
        SphericalRenderMode::Globe,
        front_camera,
        VectorAnimationUniform::new(0.0),
    );
    let globe_back = render(
        &device,
        &queue,
        &category,
        SphericalRenderMode::Globe,
        back_camera,
        VectorAnimationUniform::new(0.0),
    );
    assert_ne!(globe_front, globe_back);

    let (diagnostic_packet, diagnostic_layers, diagnostic) = diagnostic_packet(&scalar);
    assert_diagnostic_semantics(&diagnostic_layers, std::slice::from_ref(&diagnostic));
    let map_diagnostics = render_packet(
        &device,
        &queue,
        &diagnostic_packet,
        scalar.view_state().map_camera(),
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_diagnostics = render_packet(
        &device,
        &queue,
        &diagnostic_packet,
        scalar.view_state().map_camera(),
        SphericalRenderMode::Globe,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );

    assert_ne!(map_scalar, map_category);
    assert_ne!(globe_scalar, globe_category);
    assert_ne!(map_scalar, map_edge_scalar);
    assert_ne!(globe_scalar, globe_edge_scalar);
    assert_ne!(map_edge_scalar, map_edge_category);
    assert_ne!(globe_edge_scalar, globe_edge_category);
    assert_ne!(map_scalar, map_diagnostics);
    assert_ne!(globe_scalar, globe_diagnostics);

    let mismatches: Vec<_> = [
        (
            "map_scalar_fill",
            &map_scalar,
            "4667a5c864059ab62930c13605afd450bcda223de278189901bb3e3f961ea7f6",
        ),
        (
            "globe_scalar_fill",
            &globe_scalar,
            "e5b6c940e404da9e14d3bb361d95ddddc769f456a3cccc9401489629ef0afd8e",
        ),
        (
            "map_category_fill",
            &map_category,
            "a54bc080f98a1286447c4c9f6ac0fc2a6787d7ef39397e182178bd1652be3176",
        ),
        (
            "globe_category_fill",
            &globe_category,
            "b98ab1546e04231e33e6c1a8ab3642276c19b38a2b01e69640c0d389770a2dfa",
        ),
        (
            "map_edge_scalar",
            &map_edge_scalar,
            "7e1339526012b1b482aec63b338e2125ddf322a3a26a1e5d2a4a87e2f247fa66",
        ),
        (
            "globe_edge_scalar",
            &globe_edge_scalar,
            "1917b2f73a66fae3055df45e7645b7cb60a6d8be2228d3aa0899121325fcad65",
        ),
        (
            "map_edge_category",
            &map_edge_category,
            "fb98595b43bce82fe8f8cc03a6d6689a1e8be3bcc58b553797f31c661b2e3006",
        ),
        (
            "globe_edge_category",
            &globe_edge_category,
            "34a9d6b98423f73322b93cd542cf220a577dbb6838964e623b0424646546f682",
        ),
        (
            "map_vector_paused",
            &map_vector_paused,
            "854a0c5de26a53f327b99468135228ab62fbd9d13adb681734a7ce4824d519ea",
        ),
        (
            "map_vector_animated",
            &map_vector_animated,
            "2f771a691b94e21376bc6c0afb0b18192445135b5bd6dfaf3a0133970ec553c0",
        ),
        (
            "globe_vector_paused",
            &globe_vector_paused,
            "7bfa53f3a3994b8b09f944b2811a34b1df175b061d1aeaf2cafd2469d3472945",
        ),
        (
            "globe_vector_animated",
            &globe_vector_animated,
            "bfa2a2187339088821a9de44765714bbb5b7c086cd86b8008e27bdde61ee6e13",
        ),
        (
            "map_seam_fragments",
            &map_seam,
            "c3fd61ab59518de6aa58d7ac2d8932c4d0fb9041829d45620aaf793c0f09cb7b",
        ),
        (
            "map_poles",
            &map_poles,
            "a05e4d4b9999272069f2f59b663dbb5207e43d21212ebec96b560f1338fdb367",
        ),
        (
            "globe_front_visibility",
            &globe_front,
            "b98ab1546e04231e33e6c1a8ab3642276c19b38a2b01e69640c0d389770a2dfa",
        ),
        (
            "globe_back_visibility",
            &globe_back,
            "517ed29b500712fe7ff712e6eaa65d839c1e5a158da55e07597b2681532fe34c",
        ),
        (
            "map_diagnostics",
            &map_diagnostics,
            "a150adb60c1432bb1876bd80148ef7c9f3b2fad4e6db6d06cef2aef9df77df39",
        ),
        (
            "globe_diagnostics",
            &globe_diagnostics,
            "119bd962b7a3b682dcf277d46e7e094344c384da7f4012d1c0f72e872917d500",
        ),
    ]
    .into_iter()
    .filter_map(|(name, pixels, expected_hash)| golden_mismatch(name, pixels, expected_hash))
    .collect();
    assert!(
        mismatches.is_empty(),
        "RGBA8 golden changes:\n{}",
        mismatches.join("\n")
    );
}

fn state(
    fill: sekai::world::fields::FieldId,
    overlay: Option<sekai::world::fields::FieldId>,
    diagnostics: bool,
) -> SphericalFieldDisplayState {
    let mut state = SphericalFieldDisplayState::default();
    state.select_fill(fill);
    state.select_overlay(overlay);
    state.set_diagnostics_enabled(diagnostics);
    state
}

fn candidate(
    seed: u64,
    state: SphericalFieldDisplayState,
    view: SphericalPresentationViewState,
    cache: &mut MemoryStageCache,
) -> SphericalPresentationCandidate {
    build_spherical_presentation_candidate_for_view(
        RootSeed::new(seed),
        &SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        },
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &GeologicSpec::default(),
        cache,
        view,
        &state,
        &DisplayRevisionClock::default(),
    )
    .unwrap()
}

fn assert_fill_semantics(
    candidate: &SphericalPresentationCandidate,
    expected: sekai::world::fields::FieldId,
) {
    let prepared = candidate.layers().fill();
    assert_eq!(prepared.field_id(), &expected);
    let catalog = candidate.document().catalog().unwrap();
    let field = catalog.get(&expected).unwrap().view().unwrap();
    for index in [0, prepared.len() / 2, prepared.len() - 1] {
        match prepared.kind() {
            PreparedFieldKind::Scalar => assert_eq!(
                f32::from_bits(prepared.raw_values()[index]),
                field.scalar_values().unwrap()[index]
            ),
            PreparedFieldKind::Category => assert_eq!(
                prepared.category_keys()[prepared.raw_values()[index] as usize],
                field.category_values().unwrap()[index]
            ),
        }
    }
}

fn assert_overlay_semantics(
    candidate: &SphericalPresentationCandidate,
    expected: sekai::world::fields::FieldId,
    kind: PreparedOverlayKind,
) {
    assert_eq!(candidate.layers().overlay_kind(), Some(kind));
    let catalog = candidate.document().catalog().unwrap();
    let authoritative = catalog.get(&expected).unwrap().view().unwrap();
    match candidate.layers().overlay().unwrap() {
        PreparedSphericalOverlay::Edge(prepared) => {
            assert_eq!(prepared.field_id(), &expected);
            for index in [0, prepared.len() / 2, prepared.len() - 1] {
                match prepared.kind() {
                    PreparedFieldKind::Scalar => assert_eq!(
                        f32::from_bits(prepared.raw_values()[index]),
                        authoritative.scalar_values().unwrap()[index]
                    ),
                    PreparedFieldKind::Category => assert_eq!(
                        prepared.category_keys()[prepared.raw_values()[index] as usize],
                        authoritative.category_values().unwrap()[index]
                    ),
                }
            }
        }
        PreparedSphericalOverlay::Vector(prepared) => {
            assert_eq!(prepared.field_id(), &expected);
            for index in [0, prepared.len() / 2, prepared.len() - 1] {
                assert_eq!(
                    prepared.components()[index],
                    authoritative.vector_values().unwrap()[index]
                );
                assert!(prepared.magnitudes()[index].is_finite());
            }
        }
    }
}

fn assert_undeformed_globe(candidate: &SphericalPresentationCandidate) {
    for vertex in candidate.globe().vertices() {
        let [x, y, z] = vertex.position();
        let radius = x.mul_add(x, y.mul_add(y, z * z)).sqrt();
        assert!((radius - 1.0).abs() <= 2.0e-6);
    }
}

fn assert_seam_geometry(candidate: &SphericalPresentationCandidate) {
    let map = candidate.map();
    let half_width = (map.bounds().max_x() - map.bounds().min_x()) * 0.5;
    assert!(map.vertices().iter().any(|vertex| {
        let x = vertex.position().x();
        (x - map.bounds().min_x()).abs() < 1.0e-9 || (x - map.bounds().max_x()).abs() < 1.0e-9
    }));
    for triangle in map.indices().chunks_exact(3) {
        let xs = [triangle[0], triangle[1], triangle[2]]
            .map(|index| map.vertices()[index as usize].position().x());
        let span = xs.into_iter().fold(f64::NEG_INFINITY, f64::max)
            - xs.into_iter().fold(f64::INFINITY, f64::min);
        assert!(span <= half_width + 2.0e-12);
    }
}

fn assert_pole_semantics(candidate: &SphericalPresentationCandidate) {
    let projection = candidate.map().projection();
    for sign in [-1.0, 1.0] {
        let pole = UnitVector3::new(0.0, 0.0, sign).unwrap();
        let restored = projection
            .inverse(projection.forward(pole).unwrap())
            .unwrap();
        assert!(restored.components()[2] * sign > 1.0 - 2.0e-12);
    }
}

fn diagnostic_packet(
    candidate: &SphericalPresentationCandidate,
) -> (
    SphericalGpuPacket,
    Arc<PreparedFieldLayers>,
    OwnedViewDiagnostic,
) {
    let diagnostic = OwnedViewDiagnostic {
        severity: ViewDiagnosticSeverity::Warning,
        code: "acceptance.synthetic-cell-warning".into(),
        field_id: Some(surface_elevation_m_field_id()),
        cell_id: Some(CellId::from_raw(0)),
        message: "source-consistent diagnostic golden fixture".into(),
    };
    let mut state = state(surface_elevation_m_field_id(), None, true);
    state.set_diagnostic_scope(DiagnosticScope::AllFields);
    let mut clock = DisplayRevisionClock::default();
    let catalog = candidate.document().catalog().unwrap();
    let layers = Arc::new(
        prepare_spherical_field_layers(
            candidate.source().clone(),
            &catalog,
            candidate.document().surface().cells().len(),
            candidate.document().surface().edges().len(),
            std::slice::from_ref(&diagnostic),
            candidate.document().preferred_field(),
            |field| candidate.document().preferred_range(field),
            &mut state,
            &mut clock,
        )
        .unwrap(),
    );
    let packet = candidate
        .gpu_packet()
        .try_with_layers(Arc::clone(&layers))
        .unwrap();
    (packet, layers, diagnostic)
}

fn assert_diagnostic_semantics(layers: &PreparedFieldLayers, diagnostics: &[OwnedViewDiagnostic]) {
    let mut expected = vec![0_u32; layers.diagnostics().len()];
    for diagnostic in diagnostics {
        let Some(cell) = diagnostic.cell_id else {
            continue;
        };
        let severity = match diagnostic.severity {
            ViewDiagnosticSeverity::Info => 1,
            ViewDiagnosticSeverity::Warning => 2,
            ViewDiagnosticSeverity::Error => 3,
        };
        expected[cell.raw() as usize] = expected[cell.raw() as usize].max(severity);
    }
    assert!(expected.iter().any(|severity| *severity != 0));
    assert_eq!(layers.diagnostics().cells(), expected);
}

fn golden_mismatch(name: &str, pixels: &[u8], expected_hash: &str) -> Option<String> {
    assert_eq!(pixels.len(), (GOLDEN_WIDTH * GOLDEN_HEIGHT * 4) as usize);
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] != 0));
    let hash = blake3::hash(pixels).to_hex().to_string();
    println!(
        "golden {name}: {}x{} {:?} blake3={hash}",
        GOLDEN_WIDTH, GOLDEN_HEIGHT, GOLDEN_FORMAT
    );
    (hash != expected_hash).then(|| format!("{name}: expected {expected_hash}, actual {hash}"))
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    candidate: &SphericalPresentationCandidate,
    mode: SphericalRenderMode,
    globe_camera: GlobeCamera,
    animation: VectorAnimationUniform,
) -> Vec<u8> {
    render_packet(
        device,
        queue,
        candidate.gpu_packet(),
        candidate.view_state().map_camera(),
        mode,
        globe_camera,
        animation,
    )
}

fn render_packet(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    packet: &SphericalGpuPacket,
    map_camera: MapCamera,
    mode: SphericalRenderMode,
    globe_camera: GlobeCamera,
    animation: VectorAnimationUniform,
) -> Vec<u8> {
    let mut renderer = SphericalFieldRenderer::new(device, GOLDEN_FORMAT);
    renderer.prepare_packet(device, queue, packet).unwrap();
    match mode {
        SphericalRenderMode::Map => renderer
            .prepare_map_frame(
                queue,
                packet,
                map_camera,
                [GOLDEN_WIDTH, GOLDEN_HEIGHT],
                animation,
            )
            .unwrap(),
        SphericalRenderMode::Globe => renderer
            .prepare_globe_frame(
                queue,
                packet,
                globe_camera,
                [GOLDEN_WIDTH, GOLDEN_HEIGHT],
                animation,
            )
            .unwrap(),
    };
    readback(device, queue, &renderer, mode)
}

fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &SphericalFieldRenderer,
    mode: SphericalRenderMode,
) -> Vec<u8> {
    let extent = wgpu::Extent3d {
        width: GOLDEN_WIDTH,
        height: GOLDEN_HEIGHT,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Spherical Presentation Acceptance Golden"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: GOLDEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let unpadded_bytes_per_row = GOLDEN_WIDTH * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Spherical Presentation Acceptance Readback"),
        size: u64::from(padded_bytes_per_row) * u64::from(GOLDEN_HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Spherical Presentation Acceptance Encoder"),
    });
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Spherical Presentation Acceptance Pass"),
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
        });
        renderer.paint(mode, &mut pass.forget_lifetime());
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(GOLDEN_HEIGHT),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    let (sender, receiver) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        sender.send(result).expect("golden mapping receiver lives");
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .expect("golden mapping callback runs")
        .expect("golden readback maps");
    let mapped = slice.get_mapped_range();
    let mut rgba8 = vec![0; (unpadded_bytes_per_row * GOLDEN_HEIGHT) as usize];
    for row in 0..GOLDEN_HEIGHT as usize {
        let source_start = row * padded_bytes_per_row as usize;
        let target_start = row * unpadded_bytes_per_row as usize;
        rgba8[target_start..target_start + unpadded_bytes_per_row as usize]
            .copy_from_slice(&mapped[source_start..source_start + unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    readback.unmap();
    rgba8
}

fn request_test_device() -> (wgpu::AdapterInfo, wgpu::Device, wgpu::Queue) {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: true,
                compatible_surface: None,
            })
            .await
        {
            Some(adapter) => adapter,
            None => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                })
                .await
                .expect("spherical goldens require a fallback or hardware adapter"),
        };
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Spherical Presentation Golden Device"),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    ..Default::default()
                },
                None,
            )
            .await
            .expect("spherical goldens require a compatible GPU device");
        (info, device, queue)
    })
}
