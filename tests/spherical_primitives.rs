use sekai::world::spatial::{central_angle, spherical_triangle_area_unit, UnitVector3};
use sekai::world::{Meters, SphericalSpaceSpec, SurfaceVertexId};

#[test]
fn unit_vectors_are_canonical_and_validated_on_deserialization() {
    let point = UnitVector3::new(3.0, 0.0, 4.0).unwrap();
    assert!((point.norm() - 1.0).abs() <= 1.0e-15);
    assert_eq!(point.components(), [0.6, 0.0, 0.8]);
    assert!(UnitVector3::new(0.0, 0.0, 0.0).is_err());
    assert!(serde_json::from_str::<UnitVector3>(r#"[0.0,0.0,0.0]"#).is_err());
    assert_eq!(SurfaceVertexId::from_raw(7).raw(), 7);
}

#[test]
fn unit_vectors_normalize_large_finite_components() {
    let point = UnitVector3::new(f64::MAX, f64::MAX, f64::MAX).unwrap();
    assert!((point.norm() - 1.0).abs() <= 1.0e-15);
}

#[test]
fn unit_vectors_normalize_tiny_finite_components() {
    let point = UnitVector3::new(2.0e-162, 2.0e-162, 2.0e-162).unwrap();
    assert!((point.norm() - 1.0).abs() <= 1.0e-15);
}

#[test]
fn central_angle_preserves_near_coincident_vector_separation() {
    let east = UnitVector3::new(1.0, 0.0, 0.0).unwrap();
    let nearly_east = UnitVector3::new(1.0, 1.0e-10, 0.0).unwrap();

    assert!((central_angle(east, nearly_east) - 1.0e-10).abs() <= 1.0e-20);
}

#[test]
fn central_angle_preserves_rotated_near_antipodal_separation() {
    let direction = UnitVector3::new(
        -0.701_242_938_149_854_7,
        -0.637_501_052_372_439_5,
        -0.319_140_642_850_438_8,
    )
    .unwrap();
    let tangent = UnitVector3::new(
        0.643_528_959_863_372_9,
        -0.373_376_060_639_039_7,
        -0.668_177_218_377_608_1,
    )
    .unwrap();
    let direction = direction.components();
    let tangent = tangent.components();
    let complement = 1.0e-10;
    let nearly_antipodal = UnitVector3::new(
        -direction[0] + complement * tangent[0],
        -direction[1] + complement * tangent[1],
        -direction[2] + complement * tangent[2],
    )
    .unwrap();
    let direction = UnitVector3::new(direction[0], direction[1], direction[2]).unwrap();
    let expected = std::f64::consts::PI - complement.atan();

    assert!((central_angle(direction, nearly_antipodal) - expected).abs() <= f64::EPSILON);
}

#[test]
fn near_pi_triangle_area_matches_analytic_oracle_under_rotation() {
    let complement: f64 = 1.0e-4;
    let triangle = [
        UnitVector3::new(1.0, 0.0, 0.0).unwrap(),
        UnitVector3::new(-1.0, complement, 0.0).unwrap(),
        UnitVector3::new(0.0, 0.0, 1.0).unwrap(),
    ];
    let rotation = [
        [0.655_433_041_099_091_8, 0.755_253_287_735_708, 0.0],
        [-0.755_253_287_735_708, 0.655_433_041_099_091_8, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let rotated = triangle.map(|vector| {
        let vector = vector.components();
        UnitVector3::new(
            rotation[0][0] * vector[0] + rotation[0][1] * vector[1] + rotation[0][2] * vector[2],
            rotation[1][0] * vector[0] + rotation[1][1] * vector[1] + rotation[1][2] * vector[2],
            rotation[2][0] * vector[0] + rotation[2][1] * vector[1] + rotation[2][2] * vector[2],
        )
        .unwrap()
    });
    let area = spherical_triangle_area_unit(triangle[0], triangle[1], triangle[2]);
    let rotated_area = spherical_triangle_area_unit(rotated[0], rotated[1], rotated[2]);
    let expected = std::f64::consts::PI - complement.atan();
    let tolerance = 8.0 * f64::EPSILON;

    assert!((area - expected).abs() <= tolerance);
    assert!((rotated_area - expected).abs() <= tolerance);
    assert!((area - rotated_area).abs() <= tolerance);
}

#[test]
fn spherical_request_resolves_to_an_exact_geodesic_budget() {
    let spec = SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 20_000,
    };
    spec.validate().unwrap();
    assert_eq!(spec.resolved_frequency(), 45);
    assert_eq!(spec.resolved_cell_count(), 20_252);
}
