use sekai::engine::BuildCancellation;
use sekai::generators::spatial::{ProfileSurfaceBuildError, ProfileSurfaceBuilder};
use sekai::world::natural::{NaturalQualityProfile, QualityMetricStatus};
use sekai::world::spatial::audited_float_platform;
use sekai::world::spatial::SurfaceRef;
use sekai::world::Meters;

const RADIUS_M: f64 = 6_371_000.0;
const DRAFT_AUTHORITATIVE_FINGERPRINT: [u8; 32] = [
    0x0d, 0x09, 0xdf, 0x7a, 0xa1, 0x31, 0xd1, 0x20, 0x49, 0x02, 0x02, 0x74, 0x1b, 0x0f, 0xd3, 0x18,
    0x49, 0x19, 0xea, 0x96, 0x81, 0xf1, 0x65, 0x37, 0xa1, 0x4f, 0x81, 0xf0, 0xe5, 0x80, 0x6f, 0x2e,
];

#[test]
fn draft_bundle_is_exact_identity_bound_quality_checked_and_repeatable() {
    let first = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let second = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();

    let plan = first.resolution_plan();
    assert_eq!(plan.authoritative_target_cell_count(), 20_000);
    assert_eq!(plan.authoritative_resolved_cell_count(), 20_252);
    assert_eq!(plan.tectonic_control_target_cell_count(), 4_842);
    assert_eq!(plan.tectonic_control_resolved_cell_count(), 4_842);
    assert_eq!(plan.climate_face_resolution(), 24);
    assert_eq!(first.authoritative_surface().cells().len(), 20_252);
    assert_eq!(first.tectonic_control_surface().cells().len(), 4_842);
    if audited_float_platform() {
        assert_eq!(
            first.authoritative_surface().fingerprint(),
            DRAFT_AUTHORITATIVE_FINGERPRINT
        );
    } else {
        eprintln!("exact identity checks skipped: unaudited float platform");
    }

    let authoritative_ref = SurfaceRef::for_spherical(first.authoritative_surface());
    let control_ref = SurfaceRef::for_spherical(first.tectonic_control_surface());
    let map = first.control_to_authoritative_map();
    assert_eq!(map.source_ref(), control_ref);
    assert_eq!(map.target_ref(), authoritative_ref);
    assert_eq!(first.quality_report().surface_ref(), authoritative_ref);

    let metric_ids = first
        .quality_report()
        .metrics()
        .iter()
        .map(|metric| {
            format!(
                "{}.{}.v{}",
                metric.id().namespace(),
                metric.id().name(),
                metric.id().version()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        metric_ids,
        [
            "remap.category-ambiguity-area-fraction.v1",
            "remap.constant-scalar-max-error.v1",
            "remap.extensive-relative-error.v1",
            "remap.solid-body-direction-agreement.v1",
            "remap.source-margin-max-relative-error.v1",
            "remap.target-margin-max-relative-error.v1",
            "spatial.closed-sphere-area-relative-error.v1",
            "spatial.shared-edge-flux-cancellation-max.v1",
        ]
    );
    assert!(first
        .quality_report()
        .metrics()
        .iter()
        .all(|metric| metric.status() == QualityMetricStatus::Pass));

    assert_eq!(
        serde_json::to_vec(first.resolution_plan()).unwrap(),
        serde_json::to_vec(second.resolution_plan()).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(first.authoritative_surface()).unwrap(),
        serde_json::to_vec(second.authoritative_surface()).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(first.tectonic_control_surface()).unwrap(),
        serde_json::to_vec(second.tectonic_control_surface()).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(first.control_to_authoritative_map()).unwrap(),
        serde_json::to_vec(second.control_to_authoritative_map()).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(first.quality_report()).unwrap(),
        serde_json::to_vec(second.quality_report()).unwrap()
    );
}

#[test]
fn cancelled_profile_build_returns_no_bundle() {
    let cancellation = BuildCancellation::new();
    cancellation.cancel();

    let result = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &cancellation,
    );

    assert!(matches!(result, Err(ProfileSurfaceBuildError::Cancelled)));
}
