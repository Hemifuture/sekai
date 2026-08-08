use sekai::gpu::spherical::{
    SphericalPaintCallback, SphericalRenderError, SphericalRenderMode, SphericalUploadCounters,
};

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
