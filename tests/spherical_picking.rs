use sekai::view::{
    intersect_unit_sphere, ProjectionPoint, SphericalProjection, SphericalProjectionError,
    SphericalProjectionKind, UnitRay,
};
use sekai::world::spatial::UnitVector3;

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
