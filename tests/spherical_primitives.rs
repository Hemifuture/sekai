use sekai::world::spatial::{central_angle, UnitVector3};
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
fn spherical_request_resolves_to_an_exact_geodesic_budget() {
    let spec = SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 20_000,
    };
    spec.validate().unwrap();
    assert_eq!(spec.resolved_frequency(), 45);
    assert_eq!(spec.resolved_cell_count(), 20_252);
}
