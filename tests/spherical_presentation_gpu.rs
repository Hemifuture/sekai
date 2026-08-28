use sekai::gpu::spherical::{
    SphericalFieldRenderer, SphericalPaintCallback, SphericalRenderError, SphericalRenderMode,
    SphericalUploadCounters,
};
use std::sync::{mpsc, Arc};

use eframe::egui_wgpu::wgpu;
use sekai::app::{
    build_spherical_presentation_candidate_for_view, PublishedSphericalPresentation,
    SphericalPresentationCandidate, SphericalRendererPreparer,
};
use sekai::engine::MemoryStageCache;
use sekai::view::{
    sample_palette, DisplayRevisionClock, GlobeCamera, MapCamera, PreparedFieldKind,
    PreparedOverlayKind, PreparedSphericalOverlay, PreparedVectorGlyphs,
    SphericalFieldDisplayState, SphericalPresentationViewState, SphericalProjection,
    SphericalProjectionKind, SphericalViewMode, VectorAnimationUniform,
};
use sekai::world::fields::{FieldDomain, FieldValueType};
use sekai::world::natural::{
    boundary_kind_field_id, boundary_strength_field_id, land_ocean_field_id,
    preliminary_prevailing_wind_m_s_field_id, surface_elevation_m_field_id, GeologicSpec,
    ReliefSpec, TectonicSpec, WorldFormationSpec,
};
use sekai::world::spatial::{canonical_east_north_basis, UnitVector3};
use sekai::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

const GOLDEN_WIDTH: u32 = 192;
const GOLDEN_HEIGHT: u32 = 96;
const GOLDEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const AUDITED_ADAPTER_NAME: &str = "NVIDIA GeForce RTX 4080 SUPER";
const AUDITED_GL_ADAPTER_NAME: &str = "NVIDIA GeForce RTX 4080 SUPER/PCIe/SSE2";

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
    assert_eq!(
        SphericalRenderError::RendererAlreadyInitialized.to_string(),
        "spherical renderer is already initialized by a publication"
    );
    assert_eq!(
        SphericalRenderError::RendererCurrentPacketMismatch.to_string(),
        "spherical renderer does not contain the publication's expected current packet"
    );
}

#[test]
fn spherical_paint_callback_implements_the_egui_wgpu_callback_contract() {
    fn assert_callback<T: eframe::egui_wgpu::CallbackTrait>() {}
    assert_callback::<SphericalPaintCallback>();
}

#[test]
fn exact_golden_policy_is_keyed_only_to_the_audited_adapter_and_backend() {
    assert!(exact_goldens_are_audited(
        AUDITED_ADAPTER_NAME,
        wgpu::Backend::Vulkan
    ));
    assert!(!exact_goldens_are_audited(
        AUDITED_ADAPTER_NAME,
        wgpu::Backend::Gl
    ));
    assert!(exact_goldens_are_audited(
        AUDITED_GL_ADAPTER_NAME,
        wgpu::Backend::Gl
    ));
    assert!(!exact_goldens_are_audited(
        "another Vulkan adapter",
        wgpu::Backend::Vulkan
    ));
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
    let (scalar, mut scalar_renderer) = publish_for_render(&device, &queue, scalar);
    let map_scalar = render(
        &device,
        &queue,
        &mut scalar_renderer,
        &scalar,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_scalar = render(
        &device,
        &queue,
        &mut scalar_renderer,
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
    let front_camera = GlobeCamera::default();
    let back_camera = GlobeCamera::from_orientation_xyzw([0.0, 1.0, 0.0, 0.0], 1.0).unwrap();
    let north = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
    assert!(front_camera.is_front_facing(north));
    assert!(!back_camera.is_front_facing(north));
    assert_front_back_semantic_ids(&category, front_camera, back_camera);
    let (category, mut category_renderer) = publish_for_render(&device, &queue, category);
    let map_category = render(
        &device,
        &queue,
        &mut category_renderer,
        &category,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_category = render(
        &device,
        &queue,
        &mut category_renderer,
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
    let (edge_scalar, mut edge_scalar_renderer) = publish_for_render(&device, &queue, edge_scalar);
    let map_edge_scalar = render(
        &device,
        &queue,
        &mut edge_scalar_renderer,
        &edge_scalar,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_edge_scalar = render(
        &device,
        &queue,
        &mut edge_scalar_renderer,
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
    let (edge_category, mut edge_category_renderer) =
        publish_for_render(&device, &queue, edge_category);
    let map_edge_category = render(
        &device,
        &queue,
        &mut edge_category_renderer,
        &edge_category,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );
    let globe_edge_category = render(
        &device,
        &queue,
        &mut edge_category_renderer,
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
    let vector_field = match vector.layers().overlay().unwrap() {
        PreparedSphericalOverlay::Vector(field) => field,
        PreparedSphericalOverlay::Edge(_) => unreachable!(),
    };
    let glyphs = PreparedVectorGlyphs::build(
        vector.source(),
        vector.map(),
        vector.globe(),
        vector_field,
        vector.layers().selected_vector_cell(),
        vector.layers().glyph_lod_key(),
    )
    .unwrap();
    assert_vector_glyph_semantics(&vector, &glyphs);
    let (vector, mut vector_renderer) = publish_for_render(&device, &queue, vector);
    let (map_vector_paused, map_vector_animated, globe_vector_paused, globe_vector_animated) =
        render_vector_phases(&device, &queue, &mut vector_renderer, &vector);
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
    let (seam, mut seam_renderer) = publish_for_render(&device, &queue, seam);
    let map_seam = render(
        &device,
        &queue,
        &mut seam_renderer,
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
    let (poles, mut poles_renderer) = publish_for_render(&device, &queue, poles);
    let map_poles = render(
        &device,
        &queue,
        &mut poles_renderer,
        &poles,
        SphericalRenderMode::Map,
        GlobeCamera::default(),
        VectorAnimationUniform::new(0.0),
    );

    let globe_front = render(
        &device,
        &queue,
        &mut category_renderer,
        &category,
        SphericalRenderMode::Globe,
        front_camera,
        VectorAnimationUniform::new(0.0),
    );
    let globe_back = render(
        &device,
        &queue,
        &mut category_renderer,
        &category,
        SphericalRenderMode::Globe,
        back_camera,
        VectorAnimationUniform::new(0.0),
    );
    assert_ne!(globe_front, globe_back);

    assert_ne!(map_scalar, map_category);
    assert_ne!(globe_scalar, globe_category);
    assert_ne!(map_scalar, map_edge_scalar);
    assert_ne!(globe_scalar, globe_edge_scalar);
    assert_ne!(map_edge_scalar, map_edge_category);
    assert_ne!(globe_edge_scalar, globe_edge_category);

    let mismatches: Vec<_> = [
        (
            "map_scalar_fill",
            &map_scalar,
            "96c8ffb312dc1fee1be91e2829611fe937ba5ed2c0d3e53d99a5bf517f84e528",
        ),
        (
            "globe_scalar_fill",
            &globe_scalar,
            "9495e1e1894b5f2842045fec74070ac01479e787d69e631807b4a6b74003213b",
        ),
        (
            "map_category_fill",
            &map_category,
            "ad3d5d42d9a57ef8f517e7b0ff539e57a834288cacc8f206c397e49d74a27c59",
        ),
        (
            "globe_category_fill",
            &globe_category,
            "038228655af50d4a12bbfc92ccd651a0060733539f21b762f1d1c28232eaddb1",
        ),
        (
            "map_edge_scalar",
            &map_edge_scalar,
            "cc78f8332c5dff7eafacd1cd37cde2ab588855c6c9a0aa9349bb89271fcbcefb",
        ),
        (
            "globe_edge_scalar",
            &globe_edge_scalar,
            "ca76951d2ed6f34b39f7888c31743fad4719b8808e41e485bbecc3b195ecf362",
        ),
        (
            "map_edge_category",
            &map_edge_category,
            "759055ebb4f04d9c3606dc0e21d796f8358bec047e56b29bb5fa7b34bc1d0ddb",
        ),
        (
            "globe_edge_category",
            &globe_edge_category,
            "0bb2408e8d9ba3b4715d8e29d973483af6914ef17adc2e6d8b3375d4e1b4782f",
        ),
        (
            "map_vector_paused",
            &map_vector_paused,
            "29fd87a2dc90573878cce1a153ab1e063503522fab089b7cc17d8262c2ba5834",
        ),
        (
            "map_vector_animated",
            &map_vector_animated,
            "099bfd49a66b7cfa61f68b4062629aa8690f5e97e22fcdf260f036cec91fd76d",
        ),
        (
            "globe_vector_paused",
            &globe_vector_paused,
            "29f949974196078a46469406821018e1314f80752a3f6cda2b8b845daa4b9dab",
        ),
        (
            "globe_vector_animated",
            &globe_vector_animated,
            "df8662becfd41718d20da22bf859de5fab0580ebbd63cb5edfe9f0670bd78ce5",
        ),
        (
            "map_seam_fragments",
            &map_seam,
            "f505ab127f62dd7cc2240a7c0fd988d936d0446e22f5737d70a1964adf7acfb1",
        ),
        (
            "map_poles",
            &map_poles,
            "a3df8e83afbec6ddaf132190befac43bd02ed3b28fba094ca00abe7bafc30270",
        ),
        (
            "globe_front_visibility",
            &globe_front,
            "038228655af50d4a12bbfc92ccd651a0060733539f21b762f1d1c28232eaddb1",
        ),
        (
            "globe_back_visibility",
            &globe_back,
            "4d90b88ff6a401ef01261ae7af508c517662c67ccf1fd55a05383ac94b4f542e",
        ),
    ]
    .into_iter()
    .filter_map(|(name, pixels, expected_hash)| {
        golden_mismatch(&adapter_info, name, pixels, expected_hash)
    })
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
        &ReliefSpec::default(),
        &GeologicSpec::default(),
        cache,
        view,
        &state,
        &DisplayRevisionClock::default(),
    )
    .unwrap()
}

fn publish_for_render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    candidate: SphericalPresentationCandidate,
) -> (PublishedSphericalPresentation, SphericalFieldRenderer) {
    let mut renderer = SphericalFieldRenderer::new(device, GOLDEN_FORMAT);
    let published = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, device, queue);
        PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap()
    };
    (published, renderer)
}

fn assert_fill_semantics(
    candidate: &SphericalPresentationCandidate,
    expected: sekai::world::fields::FieldId,
) {
    let prepared = candidate.layers().fill();
    assert_eq!(prepared.field_id(), &expected);
    assert_eq!(candidate.layers().source(), candidate.source());
    let surface = candidate.document().surface();
    assert_eq!(prepared.len(), surface.cells().len());
    for (index, cell) in surface.cells().iter().enumerate() {
        assert_eq!(cell.id.raw() as usize, index);
    }
    let catalog = candidate.document().catalog().unwrap();
    let field = catalog.get(&expected).unwrap().view().unwrap();
    assert_eq!(field.schema().id, expected);
    assert_eq!(field.schema().domain, FieldDomain::Cells);
    assert_eq!(field.len(), surface.cells().len());
    match prepared.kind() {
        PreparedFieldKind::Scalar => {
            assert_eq!(field.schema().value_type, FieldValueType::ScalarF32);
            let authoritative = field.scalar_values().unwrap();
            assert!(prepared.category_keys().is_empty());
            for (raw, &value) in prepared.raw_values().iter().zip(authoritative) {
                assert_eq!(*raw, value.to_bits());
            }
        }
        PreparedFieldKind::Category => {
            assert_eq!(field.schema().value_type, FieldValueType::CategoryU32);
            let authoritative = field.category_values().unwrap();
            let keys = field
                .schema()
                .category_labels
                .keys()
                .copied()
                .collect::<Vec<_>>();
            assert_eq!(prepared.category_keys(), keys);
            for (raw, &value) in prepared.raw_values().iter().zip(authoritative) {
                assert_eq!(*raw as usize, keys.binary_search(&value).unwrap());
            }
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
    assert_eq!(candidate.layers().source(), candidate.source());
    match candidate.layers().overlay().unwrap() {
        PreparedSphericalOverlay::Edge(prepared) => {
            assert_eq!(prepared.field_id(), &expected);
            let surface = candidate.document().surface();
            assert_eq!(authoritative.schema().domain, FieldDomain::Edges);
            assert_eq!(prepared.len(), surface.edges().len());
            assert_eq!(authoritative.len(), surface.edges().len());
            for (index, edge) in surface.edges().iter().enumerate() {
                assert_eq!(edge.id.raw() as usize, index);
            }
            match prepared.kind() {
                PreparedFieldKind::Scalar => {
                    assert_eq!(authoritative.schema().value_type, FieldValueType::ScalarF32);
                    assert!(prepared.category_keys().is_empty());
                    for (raw, &value) in prepared
                        .raw_values()
                        .iter()
                        .zip(authoritative.scalar_values().unwrap())
                    {
                        assert_eq!(*raw, value.to_bits());
                    }
                }
                PreparedFieldKind::Category => {
                    assert_eq!(
                        authoritative.schema().value_type,
                        FieldValueType::CategoryU32
                    );
                    let values = authoritative.category_values().unwrap();
                    let keys = authoritative
                        .schema()
                        .category_labels
                        .keys()
                        .copied()
                        .collect::<Vec<_>>();
                    assert_eq!(prepared.category_keys(), keys);
                    for (raw, &value) in prepared.raw_values().iter().zip(values) {
                        assert_eq!(*raw as usize, keys.binary_search(&value).unwrap());
                    }
                }
            }
            assert_edge_instance_semantics(candidate, prepared);
        }
        PreparedSphericalOverlay::Vector(prepared) => {
            assert_eq!(prepared.field_id(), &expected);
            let surface = candidate.document().surface();
            assert_eq!(authoritative.schema().domain, FieldDomain::Cells);
            assert_eq!(
                authoritative.schema().value_type,
                FieldValueType::Vector2F32
            );
            assert_eq!(prepared.len(), surface.cells().len());
            assert_eq!(authoritative.len(), surface.cells().len());
            for (index, (&components, &magnitude)) in prepared
                .components()
                .iter()
                .zip(prepared.magnitudes())
                .enumerate()
            {
                let expected_components = authoritative.vector_values().unwrap()[index];
                assert_eq!(
                    components.map(f32::to_bits),
                    expected_components.map(f32::to_bits)
                );
                assert_close(
                    magnitude,
                    expected_components[0].hypot(expected_components[1]),
                    2.0e-6,
                );
            }
        }
    }
}

fn assert_edge_instance_semantics(
    candidate: &SphericalPresentationCandidate,
    field: &sekai::view::PreparedEdgeField,
) {
    let edge_count = candidate.document().surface().edges().len();
    let mut represented = vec![false; edge_count];
    let mut visible_fragments = 0_usize;
    for segment in candidate.map().edge_segments() {
        let edge = segment.edge().raw() as usize;
        assert!(edge < edge_count);
        represented[edge] = true;
        let start = segment.start();
        let end = segment.end();
        assert!(start.x().is_finite() && start.y().is_finite());
        assert!(end.x().is_finite() && end.y().is_finite());
        assert_ne!(
            [start.x().to_bits(), start.y().to_bits()],
            [end.x().to_bits(), end.y().to_bits()]
        );
        let visible = match field.kind() {
            PreparedFieldKind::Scalar => f32::from_bits(field.raw_values()[edge]) != 0.0,
            PreparedFieldKind::Category => {
                field.category_keys()[field.raw_values()[edge] as usize] != 0
            }
        };
        visible_fragments += usize::from(visible);
    }
    assert!(represented.into_iter().all(|represented| represented));
    assert!(visible_fragments > 0);
}

fn assert_vector_glyph_semantics(
    candidate: &SphericalPresentationCandidate,
    glyphs: &PreparedVectorGlyphs,
) {
    const EXPECTED_SAMPLED_IDS: &[u32] = &[
        0, 6, 18, 21, 28, 33, 52, 54, 71, 76, 100, 114, 119, 120, 127, 142, 149,
    ];
    assert_eq!(glyphs.source(), candidate.source());
    assert_eq!(glyphs.lod_key(), candidate.layers().glyph_lod_key());
    assert_eq!(
        glyphs
            .sampled_cells()
            .iter()
            .map(|cell| cell.raw())
            .collect::<Vec<_>>(),
        EXPECTED_SAMPLED_IDS
    );
    assert!(glyphs.diagnostics().is_empty());
    let map_ids = glyphs
        .map()
        .iter()
        .map(|glyph| glyph.cell())
        .collect::<Vec<_>>();
    let globe_ids = glyphs
        .globe()
        .iter()
        .map(|glyph| glyph.cell())
        .collect::<Vec<_>>();
    assert_eq!(map_ids, globe_ids);
    let field = match candidate.layers().overlay().unwrap() {
        PreparedSphericalOverlay::Vector(field) => field,
        PreparedSphericalOverlay::Edge(_) => unreachable!(),
    };
    let expected_rendered_ids = glyphs
        .sampled_cells()
        .iter()
        .copied()
        .filter(|cell| field.components()[cell.raw() as usize] != [0.0, 0.0])
        .collect::<Vec<_>>();
    assert_eq!(
        map_ids, expected_rendered_ids,
        "sampled zero vectors are intentionally omitted from rendered instances"
    );

    let palette = candidate.layers().overlay_palette().unwrap();
    let (display_min, display_max) = field.display_range().bounds();
    for (map, globe) in glyphs.map().iter().zip(glyphs.globe()) {
        assert_eq!(map.cell(), globe.cell());
        let index = map.cell().raw() as usize;
        let authoritative = candidate
            .document()
            .catalog()
            .unwrap()
            .get(&preliminary_prevailing_wind_m_s_field_id())
            .unwrap()
            .view()
            .unwrap()
            .vector_values()
            .unwrap()[index];
        let magnitude = authoritative[0].hypot(authoritative[1]);
        let color_position = if display_max == display_min {
            0.5
        } else {
            ((magnitude - display_min) / (display_max - display_min)).clamp(0.0, 1.0)
        };
        assert_eq!(
            map.components().map(f32::to_bits),
            authoritative.map(f32::to_bits)
        );
        assert_eq!(
            globe.components().map(f32::to_bits),
            authoritative.map(f32::to_bits)
        );
        assert_close(map.magnitude(), magnitude, 2.0e-6);
        assert_close(globe.magnitude(), magnitude, 2.0e-6);
        assert_close(map.color_position(), color_position, 2.0e-6);
        assert_close(globe.color_position(), color_position, 2.0e-6);
        assert_close(map.length_fraction(), 0.35 + 0.65 * color_position, 2.0e-6);
        assert_close(
            globe.length_fraction(),
            0.35 + 0.65 * color_position,
            2.0e-6,
        );
        assert!(sample_palette(palette, color_position)
            .components()
            .into_iter()
            .all(f32::is_finite));

        let radial = candidate.document().surface().cells()[index].centroid;
        assert_eq!(globe.radial(), radial);
        let (east, north) = canonical_east_north_basis(radial);
        let tangent = [
            east[0] * f64::from(authoritative[0]) + north[0] * f64::from(authoritative[1]),
            east[1] * f64::from(authoritative[0]) + north[1] * f64::from(authoritative[1]),
            east[2] * f64::from(authoritative[0]) + north[2] * f64::from(authoritative[1]),
        ];
        let tangent_length = tangent
            .into_iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        let expected_globe = tangent.map(|value| (value / tangent_length) as f32);
        assert_direction3(globe.direction(), expected_globe, 3.0e-6);
        let expected_map = finite_difference_map_direction(
            candidate.map().projection(),
            radial,
            authoritative.map(f64::from),
        );
        assert_direction2(map.cell(), map.direction(), expected_map, 2.0e-4);
        let origin = candidate.map().projection().forward(radial).unwrap();
        assert_close(map.origin()[0], origin.x() as f32, 2.0e-6);
        assert_close(map.origin()[1], origin.y() as f32, 2.0e-6);
    }
}

fn finite_difference_map_direction(
    projection: SphericalProjection,
    radial: UnitVector3,
    components: [f64; 2],
) -> [f32; 2] {
    let (east, north) = canonical_east_north_basis(radial);
    let east_difference = finite_projection_difference(projection, radial, east);
    let north_difference = finite_projection_difference(projection, radial, north);
    let delta = [
        components[0] * east_difference[0] + components[1] * north_difference[0],
        components[0] * east_difference[1] + components[1] * north_difference[1],
    ];
    let length = delta[0].hypot(delta[1]);
    [(delta[0] / length) as f32, (delta[1] / length) as f32]
}

fn finite_projection_difference(
    projection: SphericalProjection,
    radial: UnitVector3,
    tangent: [f64; 3],
) -> [f64; 2] {
    const STEP: f64 = 1.0e-7;
    let radial_components = radial.components();
    let positive = UnitVector3::new(
        radial_components[0] * STEP.cos() + tangent[0] * STEP.sin(),
        radial_components[1] * STEP.cos() + tangent[1] * STEP.sin(),
        radial_components[2] * STEP.cos() + tangent[2] * STEP.sin(),
    )
    .unwrap();
    let negative = UnitVector3::new(
        radial_components[0] * STEP.cos() - tangent[0] * STEP.sin(),
        radial_components[1] * STEP.cos() - tangent[1] * STEP.sin(),
        radial_components[2] * STEP.cos() - tangent[2] * STEP.sin(),
    )
    .unwrap();
    let positive = projection.forward(positive).unwrap();
    let negative = projection.forward(negative).unwrap();
    let period = match projection.kind() {
        SphericalProjectionKind::Equirectangular => 2.0,
        SphericalProjectionKind::EqualEarth => {
            const A1: f64 = 1.340_264;
            const A2: f64 = -0.081_106;
            const A3: f64 = 0.000_893;
            const A4: f64 = 0.003_796;
            let latitude = radial.components()[2].asin();
            let m = 3.0_f64.sqrt() / 2.0;
            let theta = (m * latitude.sin()).asin();
            let theta2 = theta * theta;
            let theta6 = theta2 * theta2 * theta2;
            let derivative = A1 + 3.0 * A2 * theta2 + theta6 * (7.0 * A3 + 9.0 * A4 * theta2);
            2.0 * std::f64::consts::PI * theta.cos() / (m * derivative)
        }
    };
    let raw_delta_x = positive.x() - negative.x();
    let delta_x = raw_delta_x - (raw_delta_x / period).round() * period;
    [
        delta_x / (2.0 * STEP),
        (positive.y() - negative.y()) / (2.0 * STEP),
    ]
}

fn assert_direction2(cell: CellId, actual: [f32; 2], expected: [f32; 2], tolerance: f32) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= tolerance),
        "cell {cell:?}: expected {expected:?}, got {actual:?}, tolerance {tolerance}"
    );
    assert_close(actual[0].hypot(actual[1]), 1.0, tolerance);
}

fn assert_direction3(actual: [f32; 3], expected: [f32; 3], tolerance: f32) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert_close(actual, expected, tolerance);
    }
    assert_close(
        actual[0]
            .mul_add(
                actual[0],
                actual[1].mul_add(actual[1], actual[2] * actual[2]),
            )
            .sqrt(),
        1.0,
        tolerance,
    );
}

fn assert_close(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual}, tolerance {tolerance}"
    );
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
    let edge_count = candidate.document().surface().edges().len();
    let mut fragment_counts = vec![0_usize; edge_count];
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
    for segment in map.edge_segments() {
        let edge = segment.edge().raw() as usize;
        assert!(edge < edge_count);
        fragment_counts[edge] += 1;
    }
    assert!(fragment_counts.iter().all(|count| *count >= 1));
    assert!(
        fragment_counts.iter().any(|count| *count > 1),
        "the seam fixture must split at least one authoritative edge without changing its ID"
    );
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
    let surface = candidate.document().surface();
    let north = surface
        .cells()
        .iter()
        .max_by(|left, right| {
            left.centroid.components()[2].total_cmp(&right.centroid.components()[2])
        })
        .unwrap()
        .id;
    let south = surface
        .cells()
        .iter()
        .min_by(|left, right| {
            left.centroid.components()[2].total_cmp(&right.centroid.components()[2])
        })
        .unwrap()
        .id;
    let mut represented = vec![false; surface.cells().len()];
    for vertex in candidate.map().vertices() {
        let cell = vertex.cell().raw() as usize;
        assert!(cell < represented.len());
        represented[cell] = true;
    }
    assert!(represented[north.raw() as usize]);
    assert!(represented[south.raw() as usize]);
}

fn assert_front_back_semantic_ids(
    candidate: &SphericalPresentationCandidate,
    front: GlobeCamera,
    back: GlobeCamera,
) {
    let surface = candidate.document().surface();
    let mut front_ids = Vec::new();
    let mut back_ids = Vec::new();
    let mut horizon_ids = Vec::new();
    for (index, cell) in surface.cells().iter().enumerate() {
        assert_eq!(cell.id.raw() as usize, index);
        let is_front = front.is_front_facing(cell.centroid);
        let is_back = back.is_front_facing(cell.centroid);
        if is_front == is_back {
            assert!(is_front, "a horizon cell is included by both >= 0 clips");
            assert!(cell.centroid.components()[2].abs() <= 2.0e-15);
            horizon_ids.push(cell.id);
        }
        if is_front {
            front_ids.push(cell.id);
        }
        if is_back {
            back_ids.push(cell.id);
        }
    }
    assert!(!front_ids.is_empty());
    assert!(!back_ids.is_empty());
    assert_eq!(
        front_ids.len() + back_ids.len() - horizon_ids.len(),
        surface.cells().len()
    );
}

fn exact_goldens_are_audited(adapter_name: &str, backend: wgpu::Backend) -> bool {
    matches!(
        (adapter_name, backend),
        (AUDITED_ADAPTER_NAME, wgpu::Backend::Vulkan)
            | (AUDITED_GL_ADAPTER_NAME, wgpu::Backend::Gl)
    )
}

fn golden_mismatch(
    adapter: &wgpu::AdapterInfo,
    name: &str,
    pixels: &[u8],
    expected_hash: &str,
) -> Option<String> {
    assert_eq!(pixels.len(), (GOLDEN_WIDTH * GOLDEN_HEIGHT * 4) as usize);
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] != 0));
    let hash = blake3::hash(pixels).to_hex().to_string();
    let audited = exact_goldens_are_audited(&adapter.name, adapter.backend);
    println!(
        "golden {name}: {}x{} {:?} blake3={hash} policy={}",
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
        GOLDEN_FORMAT,
        if audited {
            "audited-exact"
        } else {
            "semantic-only-unaudited"
        }
    );
    if !audited {
        return None;
    }
    (hash != expected_hash).then(|| format!("{name}: expected {expected_hash}, actual {hash}"))
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut SphericalFieldRenderer,
    published: &PublishedSphericalPresentation,
    mode: SphericalRenderMode,
    globe_camera: GlobeCamera,
    animation: VectorAnimationUniform,
) -> Vec<u8> {
    let packet = published.gpu_packet();
    match mode {
        SphericalRenderMode::Map => renderer
            .prepare_map_frame(
                queue,
                packet,
                published.view_state().map_camera(),
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
    readback(device, queue, renderer, mode)
}

fn render_vector_phases(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut SphericalFieldRenderer,
    published: &PublishedSphericalPresentation,
) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let layers = Arc::clone(published.layers_arc());
    let immutable_uploads = renderer.upload_counters();

    renderer
        .prepare_map_frame(
            queue,
            published.gpu_packet(),
            published.view_state().map_camera(),
            [GOLDEN_WIDTH, GOLDEN_HEIGHT],
            VectorAnimationUniform::new(0.0),
        )
        .unwrap();
    let map_paused = readback(device, queue, renderer, SphericalRenderMode::Map);
    renderer
        .prepare_map_frame(
            queue,
            published.gpu_packet(),
            published.view_state().map_camera(),
            [GOLDEN_WIDTH, GOLDEN_HEIGHT],
            VectorAnimationUniform::new(0.375),
        )
        .unwrap();
    let map_animated = readback(device, queue, renderer, SphericalRenderMode::Map);
    renderer
        .prepare_globe_frame(
            queue,
            published.gpu_packet(),
            GlobeCamera::default(),
            [GOLDEN_WIDTH, GOLDEN_HEIGHT],
            VectorAnimationUniform::new(0.0),
        )
        .unwrap();
    let globe_paused = readback(device, queue, renderer, SphericalRenderMode::Globe);
    renderer
        .prepare_globe_frame(
            queue,
            published.gpu_packet(),
            GlobeCamera::default(),
            [GOLDEN_WIDTH, GOLDEN_HEIGHT],
            VectorAnimationUniform::new(0.375),
        )
        .unwrap();
    let globe_animated = readback(device, queue, renderer, SphericalRenderMode::Globe);

    let after_phase_only_frames = renderer.upload_counters();
    assert_eq!(
        after_phase_only_frames.map_geometry,
        immutable_uploads.map_geometry
    );
    assert_eq!(
        after_phase_only_frames.globe_geometry,
        immutable_uploads.globe_geometry
    );
    assert_eq!(
        after_phase_only_frames.fill_field,
        immutable_uploads.fill_field
    );
    assert_eq!(
        after_phase_only_frames.diagnostics,
        immutable_uploads.diagnostics
    );
    assert_eq!(after_phase_only_frames.palettes, immutable_uploads.palettes);
    assert_eq!(
        after_phase_only_frames.map_overlay_instances,
        immutable_uploads.map_overlay_instances
    );
    assert_eq!(
        after_phase_only_frames.globe_overlay_instances,
        immutable_uploads.globe_overlay_instances
    );
    assert_eq!(
        after_phase_only_frames.uniforms,
        immutable_uploads.uniforms + 4
    );
    assert!(after_phase_only_frames.uploaded_bytes > immutable_uploads.uploaded_bytes);
    assert!(Arc::ptr_eq(&layers, published.layers_arc()));
    (map_paused, map_animated, globe_paused, globe_animated)
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
