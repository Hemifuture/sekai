use sekai::world::spatial::{central_angle, spherical_triangle_area_unit, UnitVector3};
use sekai::world::{Meters, SphericalSpaceSpec, SurfaceVertexId};

#[test]
fn unit_vectors_are_canonical_and_validated_on_deserialization() {
    let point = UnitVector3::new(3.0, 0.0, 4.0).unwrap();
    assert!((point.norm() - 1.0).abs() <= 1.0e-15);
    assert!(UnitVector3::new(0.0, 0.0, 0.0).is_err());
    assert!(serde_json::from_str::<UnitVector3>(r#"[0.0,0.0,0.0]"#).is_err());
    assert_eq!(SurfaceVertexId::from_raw(7).raw(), 7);
}

#[test]
fn unit_vectors_use_direct_normalization_when_intermediates_are_safe() {
    let point = UnitVector3::new(3.0, 0.0, 4.0).unwrap();

    assert_eq!(point.components(), [0.600_000_000_000_000_1, 0.0, 0.8]);
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
fn near_pi_triangle_area_is_stable_under_rotation() {
    let triangle = [
        UnitVector3::new(1.0, 0.0, 0.0).unwrap(),
        UnitVector3::new(-1.0, 1.0e-4, 0.0).unwrap(),
        UnitVector3::new(0.0, 0.0, 1.0).unwrap(),
    ];
    let rotation = [
        [
            0.376_360_800_722_535_64,
            0.519_346_084_346_028_5,
            -0.767_223_691_210_027_5,
        ],
        [
            0.500_184_662_395_402_3,
            -0.810_957_159_127_695,
            -0.303_584_896_798_138,
        ],
        [
            -0.779_851_172_457_853_7,
            -0.269_496_068_123_426_47,
            -0.564_981_431_625_964_3,
        ],
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

    assert!(area > std::f64::consts::PI - 1.0e-3);
    assert!((area - rotated_area).abs() <= 16.0 * f64::EPSILON);
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
