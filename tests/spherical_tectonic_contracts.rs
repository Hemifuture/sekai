use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BoundaryRecord, CrustKind, CrustKindField, PlateIdField, SphericalPlate,
    SphericalPlateRotation, SphericalTectonicSnapshot, SphericalTectonicValidationError,
    MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR, MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR,
    OCEANIC_CRUST_MIN_THICKNESS_KM, TECTONIC_SNAPSHOT_SCHEMA_V2,
};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, UnitVector3, SPATIAL_SCHEMA_V1};
use sekai::world::{CellId, Meters, PlateId, SphericalSpaceSpec};

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn unit(x: f64, y: f64, z: f64) -> UnitVector3 {
    UnitVector3::new(x, y, z).unwrap()
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

#[test]
fn euler_rotation_derives_tangent_velocity_from_one_shared_angular_vector() {
    let radius = meters(6_371_000.0);
    let pole = unit(0.0, 0.0, 1.0);
    let rotation = SphericalPlateRotation::new(pole, 10_000).unwrap();

    assert_eq!(rotation.pole(), pole);
    assert_eq!(rotation.angular_rate_prad_per_year(), 10_000);
    assert_eq!(rotation.angular_rate_rad_per_year(), 1.0e-8);
    assert_eq!(
        rotation.angular_velocity_vector_rad_per_year(),
        [0.0, 0.0, 1.0e-8]
    );

    let equator = unit(1.0, 0.0, 0.0);
    let equatorial_velocity = rotation.velocity_mm_per_year(radius, equator).unwrap();
    assert_eq!(equatorial_velocity[0], 0.0);
    assert!((equatorial_velocity[1] - 63.71).abs() <= 1.0e-12);
    assert_eq!(equatorial_velocity[2], 0.0);
    assert!(dot(equatorial_velocity, equator.components()).abs() <= 1.0e-12);

    let arbitrary = unit(0.3, -0.4, 0.5);
    let arbitrary_velocity = rotation.velocity_mm_per_year(radius, arbitrary).unwrap();
    assert!(dot(arbitrary_velocity, arbitrary.components()).abs() <= 1.0e-9);
    assert_eq!(
        rotation.velocity_mm_per_year(radius, pole).unwrap(),
        [0.0; 3]
    );
}

#[test]
fn angular_rate_is_fixed_point_and_radius_validation_enforces_the_speed_cap() {
    let pole = unit(0.0, 0.0, 1.0);
    let fastest_small_world =
        SphericalPlateRotation::new(pole, MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR).unwrap();
    assert_eq!(
        fastest_small_world
            .maximum_speed_mm_per_year(meters(1.0))
            .unwrap(),
        MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR
    );
    fastest_small_world
        .validate_for_radius(meters(1.0))
        .unwrap();
    assert!(matches!(
        fastest_small_world.validate_for_radius(meters(2.0)),
        Err(SphericalTectonicValidationError::PlateSpeedOutOfRange { .. })
    ));

    let fastest_large_world = SphericalPlateRotation::new(pole, 1_200).unwrap();
    fastest_large_world
        .validate_for_radius(meters(100_000_000.0))
        .unwrap();
    assert!(
        (fastest_large_world
            .maximum_speed_mm_per_year(meters(100_000_000.0))
            .unwrap()
            - MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR)
            .abs()
            <= 1.0e-12
    );

    assert!(matches!(
        SphericalPlateRotation::new(pole, 0),
        Err(SphericalTectonicValidationError::AngularRateOutOfRange { .. })
    ));
    assert!(matches!(
        SphericalPlateRotation::new(pole, MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR + 1),
        Err(SphericalTectonicValidationError::AngularRateOutOfRange { .. })
    ));
}

#[test]
fn euler_rotation_wire_is_canonical_strict_and_revalidated() {
    let rotation = SphericalPlateRotation::new(unit(0.0, 0.0, 1.0), 10_000).unwrap();
    let value = serde_json::to_value(rotation).unwrap();

    assert_eq!(value["pole"], serde_json::json!([0.0, 0.0, 1.0]));
    assert_eq!(value["angular_rate_prad_per_year"], 10_000);
    assert_eq!(
        serde_json::from_value::<SphericalPlateRotation>(value.clone()).unwrap(),
        rotation
    );

    let mut zero = value.clone();
    zero["angular_rate_prad_per_year"] = serde_json::json!(0);
    assert!(serde_json::from_value::<SphericalPlateRotation>(zero).is_err());

    let mut non_unit = value.clone();
    non_unit["pole"] = serde_json::json!([0.0, 0.0, 2.0]);
    assert!(serde_json::from_value::<SphericalPlateRotation>(non_unit).is_err());

    let mut unknown = value;
    unknown["projection"] = serde_json::json!("equirectangular");
    assert!(serde_json::from_value::<SphericalPlateRotation>(unknown).is_err());
}

fn spherical_surface(radius: f64) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: meters(radius),
        target_cell_count: 42,
    })
    .unwrap()
}

fn one_plate_snapshot(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
) -> SphericalTectonicSnapshot {
    let rotation = SphericalPlateRotation::new(unit(0.0, 0.0, 1.0), 10_000).unwrap();
    SphericalTectonicSnapshot::new(
        TECTONIC_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        vec![SphericalPlate::new(
            PlateId::from_raw(0),
            CellId::from_raw(0),
            rotation,
        )],
        PlateIdField::from_ids(vec![PlateId::from_raw(0); surface.cells().len()]),
        CrustKindField::from_kinds(vec![CrustKind::Oceanic; surface.cells().len()]),
        vec![OCEANIC_CRUST_MIN_THICKNESS_KM; surface.cells().len()],
        vec![BoundaryRecord::none(); surface.edges().len()],
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn spherical_snapshot_round_trips_with_exact_surface_identity() {
    let surface = spherical_surface(6_371_000.0);
    let snapshot = one_plate_snapshot(&surface);

    snapshot.validate().unwrap();
    snapshot.validate_against(&surface).unwrap();
    assert_eq!(snapshot.schema_version(), TECTONIC_SNAPSHOT_SCHEMA_V2);
    assert_eq!(snapshot.surface_ref(), SurfaceRef::for_spherical(&surface));
    assert_eq!(snapshot.plates().len(), 1);
    assert_eq!(snapshot.cell_plates().len(), surface.cells().len());
    assert_eq!(snapshot.crust_kinds().len(), surface.cells().len());
    assert_eq!(snapshot.boundaries().len(), surface.edges().len());
    assert!(snapshot.boundary_segments().is_empty());

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["schema_version"], TECTONIC_SNAPSHOT_SCHEMA_V2);
    assert_eq!(encoded["surface_ref"]["geometry_kind"], "spherical_v1");
    assert!(encoded.get("cell_count").is_none());
    assert!(encoded.get("edge_count").is_none());
    let decoded: SphericalTectonicSnapshot = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);

    let mut unknown = encoded;
    unknown["projection"] = serde_json::json!("mercator");
    assert!(serde_json::from_value::<SphericalTectonicSnapshot>(unknown).is_err());
}

#[test]
fn spherical_snapshot_rejects_non_spherical_identity_and_dense_length_errors() {
    let surface = spherical_surface(6_371_000.0);
    let planar_identity = SurfaceRef::new(
        SurfaceGeometryKind::PlanarV1,
        SPATIAL_SCHEMA_V1,
        surface.cells().len() as u32,
        surface.edges().len() as u32,
        [7; 32],
    )
    .unwrap();
    let rotation = SphericalPlateRotation::new(unit(0.0, 0.0, 1.0), 10_000).unwrap();
    let build = |surface_ref, cell_plates: Vec<PlateId>| {
        SphericalTectonicSnapshot::new(
            TECTONIC_SNAPSHOT_SCHEMA_V2,
            surface_ref,
            vec![SphericalPlate::new(
                PlateId::from_raw(0),
                CellId::from_raw(0),
                rotation,
            )],
            PlateIdField::from_ids(cell_plates),
            CrustKindField::from_kinds(vec![CrustKind::Oceanic; surface.cells().len()]),
            vec![OCEANIC_CRUST_MIN_THICKNESS_KM; surface.cells().len()],
            vec![BoundaryRecord::none(); surface.edges().len()],
            Vec::new(),
        )
    };

    assert!(matches!(
        build(
            planar_identity,
            vec![PlateId::from_raw(0); surface.cells().len()]
        ),
        Err(SphericalTectonicValidationError::InvalidSurfaceKind { .. })
    ));
    assert!(matches!(
        build(
            SurfaceRef::for_spherical(&surface),
            vec![PlateId::from_raw(0); surface.cells().len() - 1]
        ),
        Err(SphericalTectonicValidationError::FieldLengthMismatch { .. })
    ));
}

#[test]
fn equal_cardinality_different_surface_and_excess_speed_are_rejected() {
    let surface = spherical_surface(6_371_000.0);
    let different_radius = spherical_surface(6_000_000.0);
    assert_eq!(surface.cells().len(), different_radius.cells().len());
    assert_eq!(surface.edges().len(), different_radius.edges().len());
    let snapshot = one_plate_snapshot(&surface);
    assert!(matches!(
        snapshot.validate_against(&different_radius),
        Err(SphericalTectonicValidationError::SurfaceMismatch { .. })
    ));

    let excessive = SphericalTectonicSnapshot::new(
        TECTONIC_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::for_spherical(&surface),
        vec![SphericalPlate::new(
            PlateId::from_raw(0),
            CellId::from_raw(0),
            SphericalPlateRotation::new(
                unit(0.0, 0.0, 1.0),
                MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR,
            )
            .unwrap(),
        )],
        PlateIdField::from_ids(vec![PlateId::from_raw(0); surface.cells().len()]),
        CrustKindField::from_kinds(vec![CrustKind::Oceanic; surface.cells().len()]),
        vec![OCEANIC_CRUST_MIN_THICKNESS_KM; surface.cells().len()],
        vec![BoundaryRecord::none(); surface.edges().len()],
        Vec::new(),
    )
    .unwrap();
    assert!(matches!(
        excessive.validate_against(&surface),
        Err(SphericalTectonicValidationError::PlateSpeedOutOfRange { .. })
    ));
}
