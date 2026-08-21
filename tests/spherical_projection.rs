use std::f64::consts::PI;

use sekai::view::{
    ProjectionPoint, SphericalProjection, SphericalProjectionError, SphericalProjectionKind,
};
use sekai::world::spatial::{central_angle, UnitVector3};

const EPS: f64 = 2.0e-12;

fn direction(longitude_degrees: f64, latitude_degrees: f64) -> UnitVector3 {
    let longitude = longitude_degrees.to_radians();
    let latitude = latitude_degrees.to_radians();
    UnitVector3::new(
        latitude.cos() * longitude.cos(),
        latitude.cos() * longitude.sin(),
        latitude.sin(),
    )
    .unwrap()
}

// Independent fixture calculated from Savric et al., "The Equal Earth map
// projection," International Journal of Geographical Information Science (2018),
// doi:10.1080/13658816.2018.1504949, equations 1-3; it does not call the module.
#[test]
#[allow(clippy::excessive_precision)] // Published reference literals are intentionally unrounded.
fn equal_earth_matches_published_spherical_reference_values() {
    let projection = SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
    let equator = projection.forward(direction(0.0, 0.0)).unwrap();
    assert!((equator.x() - 0.0).abs() < EPS);
    assert!((equator.y() - 0.0).abs() < EPS);

    let sample = projection.forward(direction(45.0, 30.0)).unwrap();
    assert!((sample.x() - 0.6329254189568163).abs() < 2.0e-12);
    assert!((sample.y() - 0.5929351198480170).abs() < 2.0e-12);
}

#[test]
fn projections_round_trip_global_grid_seams_poles_and_central_meridians() {
    for kind in [
        SphericalProjectionKind::EqualEarth,
        SphericalProjectionKind::Equirectangular,
    ] {
        for central_meridian in [-170.0_f64, -30.0, 0.0, 75.0, 179.0] {
            let projection = SphericalProjection::new(kind, central_meridian.to_radians()).unwrap();
            for longitude in -180_i32..=180 {
                if longitude % 5 != 0 {
                    continue;
                }
                for latitude in -90_i32..=90 {
                    if latitude % 5 != 0 {
                        continue;
                    }
                    let original = direction(longitude as f64, latitude as f64);
                    let point = projection.forward(original).unwrap();
                    let restored = projection.inverse(point).unwrap_or_else(|error| {
                        panic!(
                            "{kind:?}, central meridian {central_meridian}, longitude {longitude}, latitude {latitude}, point {point:?}: {error:?}"
                        )
                    });
                    if latitude.unsigned_abs() == 90 {
                        assert!((restored.components()[2].abs() - 1.0).abs() < 2.0e-10);
                    } else {
                        assert!(central_angle(original, restored) < 2.0e-10);
                    }
                }
            }

            for seam_offset in [-1.0e-10, 1.0e-10] {
                let original = direction(central_meridian + 180.0 + seam_offset, 37.0);
                let restored = projection
                    .inverse(projection.forward(original).unwrap())
                    .unwrap();
                assert!(central_angle(original, restored) < 2.0e-10);
            }
        }
    }
}

#[test]
fn equirectangular_uses_exact_normalized_coordinates() {
    let projection = SphericalProjection::new(
        SphericalProjectionKind::Equirectangular,
        30.0_f64.to_radians(),
    )
    .unwrap();
    let point = projection.forward(direction(120.0, 45.0)).unwrap();
    assert!((point.x() - 0.5).abs() < EPS);
    assert!((point.y() - 0.5).abs() < EPS);
    assert_eq!(projection.bounds().min_x(), -1.0);
    assert_eq!(projection.bounds().max_y(), 1.0);
}

#[test]
fn invalid_projection_coordinates_have_explicit_errors() {
    let projection = SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
    assert_eq!(
        projection.inverse(ProjectionPoint::new(f64::NAN, 0.0)),
        Err(SphericalProjectionError::NonFiniteInput)
    );
    assert_eq!(
        projection.inverse(ProjectionPoint::new(0.0, 2.0)),
        Err(SphericalProjectionError::OutsideProjectionOutline)
    );
    assert_eq!(
        SphericalProjection::new(SphericalProjectionKind::EqualEarth, f64::INFINITY),
        Err(SphericalProjectionError::NonFiniteInput)
    );
}

#[test]
fn local_vector_mapping_rejects_zero_and_normalizes_finite_results() {
    let projection = SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
    let radial = direction(20.0, 25.0);
    assert_eq!(
        projection.map_local_vector(radial, [0.0, 0.0]).unwrap(),
        None
    );

    let mapped = projection
        .map_local_vector(radial, [3.0, -2.0])
        .unwrap()
        .unwrap();
    assert!(mapped.x().is_finite() && mapped.y().is_finite());
    assert!((mapped.x().hypot(mapped.y()) - 1.0).abs() < 1.0e-12);

    let pole = direction(0.0, 90.0);
    assert_eq!(
        projection.map_local_vector(pole, [1.0, 0.0]),
        Err(SphericalProjectionError::ProjectionJacobianDegenerate)
    );
}

#[test]
fn local_east_north_and_seam_sides_have_explicit_direction_oracles() {
    for kind in [
        SphericalProjectionKind::EqualEarth,
        SphericalProjectionKind::Equirectangular,
    ] {
        let projection = SphericalProjection::new(kind, 33.0_f64.to_radians()).unwrap();
        let radial = direction(33.0, 28.0);
        let east = projection
            .map_local_vector(radial, [1.0, 0.0])
            .unwrap()
            .unwrap();
        let north = projection
            .map_local_vector(radial, [0.0, 1.0])
            .unwrap()
            .unwrap();
        assert!(east.x() > 1.0 - 1.0e-10, "{kind:?} east={east:?}");
        assert!(east.y().abs() < 1.0e-8, "{kind:?} east={east:?}");
        assert!(north.x().abs() < 1.0e-8, "{kind:?} north={north:?}");
        assert!(north.y() > 1.0 - 1.0e-8, "{kind:?} north={north:?}");

        let west_side = direction(33.0 - 180.0 + 1.0e-6, 28.0);
        let east_side = direction(33.0 + 180.0 - 1.0e-6, 28.0);
        let west_east = projection
            .map_local_vector(west_side, [1.0, 0.0])
            .unwrap()
            .unwrap();
        let east_east = projection
            .map_local_vector(east_side, [1.0, 0.0])
            .unwrap()
            .unwrap();
        assert!(west_east.x() > 0.0 && east_east.x() > 0.0);
        assert!((west_east.x() - east_east.x()).abs() < 1.0e-8);
        assert!((west_east.y() - east_east.y()).abs() < 1.0e-8);

        let west_north = projection
            .map_local_vector(west_side, [0.0, 1.0])
            .unwrap()
            .unwrap();
        let east_north = projection
            .map_local_vector(east_side, [0.0, 1.0])
            .unwrap()
            .unwrap();
        assert!((west_north.x() + east_north.x()).abs() < 1.0e-7);
        assert!((west_north.y() - east_north.y()).abs() < 1.0e-7);
        assert!(west_north.y() > 0.0 && east_north.y() > 0.0);
    }
}

#[test]
fn both_projection_poles_preserve_north_and_south_sign() {
    for kind in [
        SphericalProjectionKind::EqualEarth,
        SphericalProjectionKind::Equirectangular,
    ] {
        let projection = SphericalProjection::new(kind, 0.0).unwrap();
        let north_point = projection.forward(direction(0.0, 90.0)).unwrap();
        let south_point = projection.forward(direction(0.0, -90.0)).unwrap();
        let north = projection.inverse(north_point).unwrap();
        let south = projection.inverse(south_point).unwrap();
        assert!(north.components()[2] > 1.0 - EPS);
        assert!(south.components()[2] < -1.0 + EPS);
    }
}

#[test]
fn outline_excludes_equirectangular_points_beyond_normalized_edges() {
    let projection =
        SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.0).unwrap();
    assert!(projection.outline_contains(ProjectionPoint::new(-1.0, -1.0)));
    assert!(projection.outline_contains(ProjectionPoint::new(1.0, 1.0)));
    assert!(!projection.outline_contains(ProjectionPoint::new(1.0 + f64::EPSILON, 0.0)));
    assert_eq!(
        projection.inverse(ProjectionPoint::new(0.0, 1.0 + f64::EPSILON)),
        Err(SphericalProjectionError::OutsideProjectionOutline)
    );
}

#[test]
fn equirectangular_inverse_preserves_cardinal_latitudes() {
    let projection =
        SphericalProjection::new(SphericalProjectionKind::Equirectangular, -PI).unwrap();
    let north = projection.inverse(ProjectionPoint::new(0.0, 1.0)).unwrap();
    assert!(north.components()[2] > 1.0 - EPS);
    let equator = projection.inverse(ProjectionPoint::new(-1.0, 0.0)).unwrap();
    assert!(equator.components()[2].abs() < EPS);
    assert!(central_angle(equator, direction(0.0, 0.0)) < EPS);
    let south = projection.inverse(ProjectionPoint::new(0.0, -1.0)).unwrap();
    assert!(south.components()[2] < -1.0 + EPS);
}

// Independent Equal Earth outline calculation from Savric et al. (2018),
// equations 1-3; this fixture deliberately does not call projection code.
fn equal_earth_outline_at_latitude(latitude_degrees: f64) -> (f64, f64) {
    const A1: f64 = 1.340_264;
    const A2: f64 = -0.081_106;
    const A3: f64 = 0.000_893;
    const A4: f64 = 0.003_796;
    let latitude = latitude_degrees.to_radians();
    let m = 3.0_f64.sqrt() / 2.0;
    let theta = (m * latitude.sin()).asin();
    let theta2 = theta * theta;
    let theta6 = theta2 * theta2 * theta2;
    let derivative = A1 + 3.0 * A2 * theta2 + theta6 * (7.0 * A3 + 9.0 * A4 * theta2);
    let half_width = PI * theta.cos() / (m * derivative);
    let y = theta * (A1 + A2 * theta2 + theta6 * (A3 + A4 * theta2));
    (half_width, y)
}

#[test]
fn equal_earth_strictly_rejects_points_beyond_the_curved_outline() {
    let projection = SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
    let (half_width, y) = equal_earth_outline_at_latitude(30.0);
    let interior = ProjectionPoint::new(half_width * 0.4, y);
    assert!(projection.outline_contains(interior));
    assert!(central_angle(projection.inverse(interior).unwrap(), direction(72.0, 30.0)) < 2.0e-12);

    let boundary = ProjectionPoint::new(half_width, y);
    assert!(projection.outline_contains(boundary));
    assert!(projection.inverse(boundary).is_ok());

    let outside = ProjectionPoint::new(half_width + 5.0e-13, y);
    assert!(!projection.outline_contains(outside));
    assert_eq!(
        projection.inverse(outside),
        Err(SphericalProjectionError::OutsideProjectionOutline)
    );

    let (_, pole_y) = equal_earth_outline_at_latitude(90.0);
    let pole = projection
        .inverse(ProjectionPoint::new(0.0, pole_y))
        .unwrap();
    assert!(pole.components()[2] > 1.0 - EPS);
}
