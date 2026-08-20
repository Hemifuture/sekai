use sekai::view::{
    intersect_unit_sphere, GlobeCamera, MapCamera, ProjectionPoint, SphericalProjection,
    SphericalProjectionError, SphericalProjectionKind, SphericalViewMode, UnitRay,
};
use sekai::world::spatial::UnitVector3;

fn assert_components_close(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "expected {expected}, got {actual}"
        );
    }
}

#[test]
fn ray_sphere_returns_nearest_positive_hit_and_rejects_misses() {
    let hit =
        intersect_unit_sphere(UnitRay::new([0.0, 0.0, 3.0], [0.0, 0.0, -1.0]).unwrap()).unwrap();
    assert_eq!(hit.direction().components(), [0.0, 0.0, 1.0]);
    assert_eq!(hit.distance(), 2.0);
    assert!(
        intersect_unit_sphere(UnitRay::new([0.0, 0.0, 3.0], [1.0, 0.0, 0.0]).unwrap()).is_none()
    );
}

#[test]
fn ray_sphere_preserves_roundoff_tangents_without_accepting_nearby_true_misses() {
    let tangent = UnitRay::new(
        [1.0002, 0.0, 0.0],
        [-0.019_997_000_574_884_644, 0.999_800_039_992_001_6, 0.0],
    )
    .unwrap();
    assert!(intersect_unit_sphere(tangent).is_some());

    let nearby_miss = UnitRay::new([1.0002, 0.0, 0.0], [-0.019, 1.0, 0.0]).unwrap();
    assert!(intersect_unit_sphere(nearby_miss).is_none());
}

#[test]
fn exact_tangent_reports_the_hand_derived_distance_and_direction() {
    let hit =
        intersect_unit_sphere(UnitRay::new([1.0, -2.0, 0.0], [0.0, 1.0, 0.0]).unwrap()).unwrap();

    assert_eq!(hit.distance(), 2.0);
    assert_eq!(hit.direction().components(), [1.0, 0.0, 0.0]);
}

#[test]
fn ray_construction_normalizes_directions_and_rejects_invalid_components() {
    let ray = UnitRay::new([0.0, 0.0, 3.0], [0.0, 0.0, -4.0]).unwrap();
    assert_eq!(ray.direction().components(), [0.0, 0.0, -1.0]);
    assert!(UnitRay::new([0.0, 0.0, 3.0], [0.0, 0.0, 0.0]).is_err());
    assert!(UnitRay::new([f64::NAN, 0.0, 3.0], [0.0, 0.0, -1.0]).is_err());
}

#[test]
fn projections_reject_outside_map_points_without_clamping() {
    for kind in [
        SphericalProjectionKind::EqualEarth,
        SphericalProjectionKind::Equirectangular,
    ] {
        let projection = SphericalProjection::new(kind, 0.0).unwrap();
        assert_eq!(
            projection.inverse(ProjectionPoint::new(10.0, 10.0)),
            Err(SphericalProjectionError::OutsideProjectionOutline)
        );
    }
}

#[test]
fn ray_hit_and_projection_inverse_preserve_the_same_unit_direction() {
    let expected = UnitVector3::new(0.2, -0.7, 0.6).unwrap();
    let [x, y, z] = expected.components();
    let ray = UnitRay::new([3.0 * x, 3.0 * y, 3.0 * z], [-x, -y, -z]).unwrap();
    let hit = intersect_unit_sphere(ray).unwrap();

    for kind in [
        SphericalProjectionKind::EqualEarth,
        SphericalProjectionKind::Equirectangular,
    ] {
        let projection = SphericalProjection::new(kind, 0.25).unwrap();
        let inverse = projection
            .inverse(projection.forward(expected).unwrap())
            .unwrap();
        assert!(hit.direction().dot(inverse) > 1.0 - 2.0e-12);
    }
}

#[test]
fn globe_camera_reset_drag_visibility_and_center_ray_share_one_view_convention() {
    let canonical_front = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
    let canonical_back = UnitVector3::new(0.0, 0.0, -1.0).unwrap();
    let mut first = GlobeCamera::default();

    assert_eq!(first.orientation_xyzw(), [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(first.orthographic_scale(), 1.0);
    assert!(first.is_front_facing(canonical_front));
    assert!(!first.is_front_facing(canonical_back));
    let reset_hit =
        intersect_unit_sphere(first.screen_to_ray([50.0, 50.0], [100.0, 100.0]).unwrap()).unwrap();
    assert_eq!(reset_hit.direction(), canonical_front);

    assert!(first.trackball_drag([50.0, 50.0], [75.0, 50.0], [100.0, 100.0]));
    let mut second = GlobeCamera::default();
    assert!(second.trackball_drag([50.0, 50.0], [75.0, 50.0], [100.0, 100.0]));
    assert_eq!(first.orientation_xyzw(), second.orientation_xyzw());
    let orientation_norm = first
        .orientation_xyzw()
        .into_iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt();
    assert!((orientation_norm - 1.0).abs() <= 8.0 * f64::EPSILON);

    let rotated_hit =
        intersect_unit_sphere(first.screen_to_ray([50.0, 50.0], [100.0, 100.0]).unwrap()).unwrap();
    assert_components_close(
        rotated_hit.direction().components(),
        [-0.5, 0.0, 3.0_f64.sqrt() * 0.5],
        2.0e-15,
    );
    assert!(first.is_front_facing(rotated_hit.direction()));
    assert!(!first.is_front_facing(UnitVector3::new(1.0, 0.0, 0.0).unwrap()));
    let rotated_back = UnitVector3::new(
        -rotated_hit.direction().components()[0],
        -rotated_hit.direction().components()[1],
        -rotated_hit.direction().components()[2],
    )
    .unwrap();
    assert!(!first.is_front_facing(rotated_back));

    first.reset();
    assert_eq!(first, GlobeCamera::default());
}

#[test]
fn globe_camera_bounds_zoom_rejects_non_finite_input_and_misses_outside_disc() {
    let mut camera = GlobeCamera::default();
    assert!(camera.zoom_by(1.0e6));
    assert_eq!(camera.orthographic_scale(), GlobeCamera::MAX_SCALE);
    assert!(camera.zoom_by(1.0e-12));
    assert_eq!(camera.orthographic_scale(), GlobeCamera::MIN_SCALE);
    assert!(camera.set_orthographic_scale(GlobeCamera::MAX_SCALE * 2.0));
    assert_eq!(camera.orthographic_scale(), GlobeCamera::MAX_SCALE);
    assert!(camera.set_orthographic_scale(GlobeCamera::MIN_SCALE));
    assert!(camera.screen_to_ray([80.0, 50.0], [100.0, 100.0]).is_none());
    assert!(camera
        .screen_to_ray([f64::NAN, 50.0], [100.0, 100.0])
        .is_none());
    assert!(camera.screen_to_ray([50.0, 50.0], [0.0, 100.0]).is_none());

    let unchanged = camera;
    assert!(!camera.zoom_by(f64::NAN));
    assert!(!camera.set_orthographic_scale(f64::INFINITY));
    assert!(!camera.trackball_drag([50.0, 50.0], [f64::NAN, 50.0], [100.0, 100.0]));
    assert_eq!(camera, unchanged);
}

#[test]
fn map_projection_and_globe_cameras_remain_independent_across_view_switches() {
    let mut map = MapCamera::default();
    assert!(map.pan_by(SphericalProjectionKind::EqualEarth, [0.25, -0.5]));
    assert!(map.zoom_by(SphericalProjectionKind::EqualEarth, 2.0));
    assert!(map.pan_by(SphericalProjectionKind::Equirectangular, [-1.0, 0.75]));
    assert!(map.zoom_by(SphericalProjectionKind::Equirectangular, 0.5));

    let mut globe = GlobeCamera::default();
    assert!(globe.trackball_drag([50.0, 50.0], [60.0, 70.0], [100.0, 100.0]));
    assert!(globe.zoom_by(3.0));
    let saved_map = map;
    let saved_globe = globe;

    let mut mode = SphericalViewMode::default();
    assert_eq!(mode, SphericalViewMode::Map);
    mode = SphericalViewMode::Globe;
    assert_eq!(mode, SphericalViewMode::Globe);
    mode = SphericalViewMode::Map;
    assert_eq!(mode, SphericalViewMode::Map);
    assert_eq!(map, saved_map);
    assert_eq!(globe, saved_globe);
    assert_eq!(map.pan(SphericalProjectionKind::EqualEarth), [0.25, -0.5]);
    assert_eq!(map.zoom(SphericalProjectionKind::EqualEarth), 2.0);
    assert_eq!(
        map.pan(SphericalProjectionKind::Equirectangular),
        [-1.0, 0.75]
    );
    assert_eq!(map.zoom(SphericalProjectionKind::Equirectangular), 0.5);

    assert!(!map.pan_by(SphericalProjectionKind::EqualEarth, [f64::NAN, 0.0]));
    assert!(!map.zoom_by(SphericalProjectionKind::EqualEarth, f64::INFINITY));
    assert_eq!(map, saved_map);

    map.reset(SphericalProjectionKind::EqualEarth);
    assert_eq!(map.pan(SphericalProjectionKind::EqualEarth), [0.0, 0.0]);
    assert_eq!(map.zoom(SphericalProjectionKind::EqualEarth), 1.0);
    assert_eq!(
        map.pan(SphericalProjectionKind::Equirectangular),
        saved_map.pan(SphericalProjectionKind::Equirectangular)
    );
    assert_eq!(
        map.zoom(SphericalProjectionKind::Equirectangular),
        saved_map.zoom(SphericalProjectionKind::Equirectangular)
    );
}
