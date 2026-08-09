use std::process::Command;
use std::time::{Duration, Instant};

use eframe::egui_wgpu::wgpu;
use sekai::app::{
    build_spherical_presentation_candidate, default_spherical_space_spec,
    PublishedSphericalPresentation, SphericalRendererPreparer, PRODUCT_DEFAULT_WORLD_SEED,
};
use sekai::engine::MemoryStageCache;
use sekai::gpu::spherical::{SphericalFieldRenderer, SphericalUploadCounters};
use sekai::view::{
    prepare_spherical_field_layers, DisplayRevisionClock, GlyphLodKey, PreparedGlobeMesh,
    PreparedProjectedMap, PreparedSphericalOverlay, PreparedVectorGlyphs, SphericalEntityLocator,
    SphericalFieldDisplayState, SphericalMeshBudgets, SphericalProjection, SphericalProjectionKind,
    VectorGlyphLod,
};
use sekai::world::natural::{
    preliminary_prevailing_wind_m_s_field_id, surface_elevation_m_field_id, GeologicSpec,
    TectonicSpec, WorldFormationSpec,
};
use sekai::world::Meters;

const PRODUCT_CELL_COUNT: u32 = 20_000;
const MAX_PRESENTATION_BYTES: usize = 128 * 1024 * 1024;
const MAX_PREPARATION_TIME: Duration = Duration::from_secs(1);

#[test]
#[ignore = "Release-only 20k CPU/GPU performance and resident-memory acceptance gate"]
fn release_20k_presentation_derivatives_fit_time_memory_and_static_upload_budgets() {
    if std::hint::black_box(cfg!(debug_assertions)) {
        panic!("run this acceptance gate with cargo test --release");
    }

    let space = default_spherical_space_spec();
    assert_eq!(space.radius, Meters::new(6_371_000.0).unwrap());
    assert_eq!(space.target_cell_count, PRODUCT_CELL_COUNT);
    let mut requested_state = SphericalFieldDisplayState::default();
    requested_state.select_fill(surface_elevation_m_field_id());
    requested_state.select_overlay(Some(preliminary_prevailing_wind_m_s_field_id()));
    requested_state.set_vector_lod(VectorGlyphLod::Medium);
    let mut cache = MemoryStageCache::new();
    let candidate = build_spherical_presentation_candidate(
        PRODUCT_DEFAULT_WORLD_SEED,
        &space,
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &GeologicSpec::default(),
        &mut cache,
        &requested_state,
        &DisplayRevisionClock::default(),
    )
    .expect("20k formal spherical product candidate builds");
    let document = candidate.document();
    let surface = document.surface();
    let actual_cell_count = surface.cells().len();
    assert_eq!(candidate.source().root_seed(), PRODUCT_DEFAULT_WORLD_SEED);
    assert_eq!(actual_cell_count, 20_252);

    let source = candidate.source().clone();
    let projection = SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
    let (map_time, map) = timed(|| {
        PreparedProjectedMap::build(
            source.clone(),
            surface,
            projection,
            SphericalMeshBudgets::DEFAULT,
        )
        .unwrap()
    });
    let (globe_time, globe) = timed(|| {
        PreparedGlobeMesh::build(source.clone(), surface, SphericalMeshBudgets::DEFAULT).unwrap()
    });
    let (locator_time, locator) =
        timed(|| SphericalEntityLocator::new(source.clone(), surface).unwrap());

    let catalog = document.catalog().unwrap();
    let mut layer_state = requested_state.clone();
    let mut clock = DisplayRevisionClock::default();
    let (layers_time, layers) = timed(|| {
        prepare_spherical_field_layers(
            source.clone(),
            &catalog,
            surface.cells().len(),
            surface.edges().len(),
            document.diagnostics(),
            document.preferred_field(),
            |field| document.preferred_range(field),
            &mut layer_state,
            &mut clock,
        )
        .unwrap()
    });
    let vector = match layers.overlay().expect("medium wind overlay is retained") {
        PreparedSphericalOverlay::Vector(vector) => vector,
        PreparedSphericalOverlay::Edge(_) => panic!("medium wind must be a cell vector overlay"),
    };
    assert_eq!(layers.fill().field_id(), &surface_elevation_m_field_id());
    assert_eq!(
        vector.field_id(),
        &preliminary_prevailing_wind_m_s_field_id()
    );
    assert_eq!(layers.glyph_lod_key(), GlyphLodKey::Medium);
    let (glyph_time, glyphs) = timed(|| {
        PreparedVectorGlyphs::build(&source, &map, &globe, vector, None, GlyphLodKey::Medium)
            .unwrap()
    });

    let components = [
        ("equal_earth_map", map_time, map.resident_bytes().unwrap()),
        ("unit_globe", globe_time, globe.resident_bytes().unwrap()),
        (
            "entity_locator",
            locator_time,
            locator.resident_bytes().unwrap(),
        ),
        (
            "field_layers",
            layers_time,
            layers.resident_bytes().unwrap(),
        ),
        (
            "medium_wind_glyphs",
            glyph_time,
            glyphs.resident_bytes().unwrap(),
        ),
    ];
    let total_time = components
        .iter()
        .try_fold(Duration::ZERO, |total, (_, elapsed, _)| {
            total.checked_add(*elapsed)
        })
        .expect("presentation duration sum does not overflow");
    let total_bytes = components
        .iter()
        .try_fold(0_usize, |total, (_, _, bytes)| total.checked_add(*bytes))
        .expect("presentation resident byte sum does not overflow");

    let (adapter_info, device, queue) = request_device();
    let mut renderer = SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let published = {
        let mut gpu = SphericalRendererPreparer::new(&mut renderer, &device, &queue);
        PublishedSphericalPresentation::try_new(candidate, &mut gpu).unwrap()
    };
    renderer
        .prepare_map_frame(
            &queue,
            published.gpu_packet(),
            Default::default(),
            [1280, 720],
            Default::default(),
        )
        .unwrap();
    let first_frame = renderer.upload_counters();
    renderer
        .prepare_map_frame(
            &queue,
            published.gpu_packet(),
            Default::default(),
            [1280, 720],
            Default::default(),
        )
        .unwrap();
    let second_frame = renderer.upload_counters();
    assert_eq!(large_uploads(second_frame), large_uploads(first_frame));
    assert_eq!(second_frame.uniforms, first_frame.uniforms + 1);

    println!(
        "reference machine: os={} arch={} cpu={} rustc={} gpu={:?} backend={:?} requested_cells={} actual_cells={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown".to_owned()),
        rustc_version(),
        adapter_info.name,
        adapter_info.backend,
        PRODUCT_CELL_COUNT,
        actual_cell_count,
    );
    for (name, elapsed, bytes) in components {
        println!("{name}: elapsed={elapsed:?} resident_bytes={bytes}");
    }
    println!(
        "presentation_total: elapsed={total_time:?} resident_bytes={total_bytes} initial_gpu_uploaded_bytes={} static_second_frame_large_upload_delta=0",
        first_frame.uploaded_bytes,
    );

    assert!(
        total_bytes <= MAX_PRESENTATION_BYTES,
        "presentation derivatives use {total_bytes} bytes, budget is {MAX_PRESENTATION_BYTES}"
    );
    assert!(
        total_time <= MAX_PREPARATION_TIME,
        "presentation derivatives take {total_time:?}, budget is {MAX_PREPARATION_TIME:?}"
    );
}

fn timed<T>(operation: impl FnOnce() -> T) -> (Duration, T) {
    let started = Instant::now();
    let output = operation();
    (started.elapsed(), output)
}

fn large_uploads(counters: SphericalUploadCounters) -> [u64; 7] {
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

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn request_device() -> (wgpu::AdapterInfo, wgpu::Device, wgpu::Queue) {
    pollster::block_on(async {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
        {
            Some(adapter) => adapter,
            None => instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: true,
                    compatible_surface: None,
                })
                .await
                .expect("20k acceptance requires a hardware or fallback GPU adapter"),
        };
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Spherical 20k Performance Acceptance Device"),
                    required_limits: wgpu::Limits::downlevel_defaults(),
                    ..Default::default()
                },
                None,
            )
            .await
            .expect("20k acceptance requires a compatible GPU device");
        (info, device, queue)
    })
}
