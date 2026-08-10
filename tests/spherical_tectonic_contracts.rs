use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BoundaryRecord, CrustKind, CrustKindField, PlateIdField, SphericalCrustState,
    SphericalOrogenyKind, SphericalPlate, SphericalPlateRotation, SphericalTectonicSnapshot,
    SphericalTectonicValidationError, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, ELEVATION_MAX_M, MAX_CRUST_AGE_MYR, MAX_PLATE_COUNT,
    MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR, MAX_SPHERICAL_PLATE_SPEED_MM_PER_YEAR,
    NO_OROGENY_AGE_SENTINEL_MYR, OCEANIC_CRUST_MIN_THICKNESS_KM, TECTONIC_SNAPSHOT_SCHEMA_V3,
};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, UnitVector3, SPATIAL_SCHEMA_V1};
use sekai::world::{
    CellId, Meters, PlateId, SphericalSpaceSpec, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT,
};

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

#[derive(Clone)]
struct CrustInputs {
    kinds: Vec<CrustKind>,
    thickness_km: Vec<f32>,
    age_myr: Vec<f32>,
    tectonic_elevation_m: Vec<f32>,
    lineation_east: Vec<f32>,
    lineation_north: Vec<f32>,
    orogeny_kind: Vec<SphericalOrogenyKind>,
    orogeny_age_myr: Vec<f32>,
}

impl CrustInputs {
    fn oceanic(cell_count: usize) -> Self {
        Self {
            kinds: vec![CrustKind::Oceanic; cell_count],
            thickness_km: vec![OCEANIC_CRUST_MIN_THICKNESS_KM; cell_count],
            age_myr: vec![0.0; cell_count],
            tectonic_elevation_m: vec![0.0; cell_count],
            lineation_east: vec![0.0; cell_count],
            lineation_north: vec![0.0; cell_count],
            orogeny_kind: vec![SphericalOrogenyKind::None; cell_count],
            orogeny_age_myr: vec![NO_OROGENY_AGE_SENTINEL_MYR; cell_count],
        }
    }

    fn build(self) -> Result<SphericalCrustState, SphericalTectonicValidationError> {
        SphericalCrustState::new(
            CrustKindField::from_kinds(self.kinds),
            self.thickness_km,
            self.age_myr,
            self.tectonic_elevation_m,
            self.lineation_east,
            self.lineation_north,
            self.orogeny_kind,
            self.orogeny_age_myr,
        )
    }
}

fn current_crust_state(cell_count: usize) -> SphericalCrustState {
    let mut inputs = CrustInputs::oceanic(cell_count);
    inputs.kinds[0] = CrustKind::Continental;
    inputs.thickness_km[0] = CONTINENTAL_CRUST_MIN_THICKNESS_KM;
    inputs.age_myr[0] = CONTINENTAL_CRUST_AGE_SENTINEL_MYR;
    inputs.tectonic_elevation_m[0] = 1_250.0;
    inputs.lineation_east[0] = 0.6;
    inputs.lineation_north[0] = 0.8;
    inputs.orogeny_kind[0] = SphericalOrogenyKind::Himalayan;
    inputs.orogeny_age_myr[0] = 18.0;
    inputs.build().unwrap()
}

fn one_plate_snapshot(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
) -> SphericalTectonicSnapshot {
    let rotation = SphericalPlateRotation::new(unit(0.0, 0.0, 1.0), 10_000).unwrap();
    SphericalTectonicSnapshot::new(
        TECTONIC_SNAPSHOT_SCHEMA_V3,
        SurfaceRef::for_spherical(surface),
        vec![SphericalPlate::new(
            PlateId::from_raw(0),
            CellId::from_raw(0),
            rotation,
        )],
        PlateIdField::from_ids(vec![PlateId::from_raw(0); surface.cells().len()]),
        current_crust_state(surface.cells().len()),
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
    assert_eq!(snapshot.schema_version(), TECTONIC_SNAPSHOT_SCHEMA_V3);
    assert_eq!(snapshot.surface_ref(), SurfaceRef::for_spherical(&surface));
    assert_eq!(snapshot.plates().len(), 1);
    assert_eq!(snapshot.cell_plates().len(), surface.cells().len());
    assert_eq!(snapshot.crust_kinds().len(), surface.cells().len());
    assert_eq!(snapshot.crust_state().len(), surface.cells().len());
    assert_eq!(
        snapshot.crust_age_myr()[0],
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR
    );
    assert_eq!(snapshot.tectonic_elevation_m()[0], 1_250.0);
    assert_eq!(snapshot.lineation_east()[0], 0.6);
    assert_eq!(snapshot.lineation_north()[0], 0.8);
    assert_eq!(snapshot.orogeny_kind()[0], SphericalOrogenyKind::Himalayan);
    assert_eq!(snapshot.orogeny_age_myr()[0], 18.0);
    assert_eq!(snapshot.boundaries().len(), surface.edges().len());
    assert!(snapshot.boundary_segments().is_empty());

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["schema_version"], TECTONIC_SNAPSHOT_SCHEMA_V3);
    assert_eq!(encoded["surface_ref"]["geometry_kind"], "spherical_v1");
    assert!(encoded.get("crust_kinds").is_none());
    assert_eq!(encoded["crust"]["orogeny_kind"][0], "Himalayan");
    assert!(encoded.get("cell_count").is_none());
    assert!(encoded.get("edge_count").is_none());
    let decoded: SphericalTectonicSnapshot = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);

    let mut v2 = encoded.clone();
    v2["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<SphericalTectonicSnapshot>(v2).is_err());

    let mut unknown_crust = encoded.clone();
    unknown_crust["crust"]["history"] = serde_json::json!([]);
    assert!(serde_json::from_value::<SphericalTectonicSnapshot>(unknown_crust).is_err());

    let mut unknown = encoded;
    unknown["projection"] = serde_json::json!("mercator");
    assert!(serde_json::from_value::<SphericalTectonicSnapshot>(unknown).is_err());
}

#[test]
fn spherical_snapshot_wire_bounds_tables_and_rejects_nested_unknown_fields() {
    let surface = spherical_surface(6_371_000.0);
    let snapshot = one_plate_snapshot(&surface);
    let encoded = serde_json::to_value(snapshot).unwrap();

    let mut unknown_boundary = encoded.clone();
    unknown_boundary["boundaries"][0]["projection"] = serde_json::json!("mercator");
    assert!(serde_json::from_value::<SphericalTectonicSnapshot>(unknown_boundary).is_err());

    let mut too_many_crust_values = encoded.clone();
    too_many_crust_values["crust"]["age_myr"] =
        serde_json::json!(vec![0.0_f32; MAX_SPHERICAL_CELL_COUNT as usize + 1]);
    let error = serde_json::from_value::<SphericalTectonicSnapshot>(too_many_crust_values)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("at most") || error.contains("invalid length"),
        "{error}"
    );

    let mut too_many_plates = encoded;
    let plate = too_many_plates["plates"][0].clone();
    too_many_plates["plates"] =
        serde_json::Value::Array(vec![plate; usize::from(MAX_PLATE_COUNT) + 1]);
    let error = serde_json::from_value::<SphericalTectonicSnapshot>(too_many_plates)
        .unwrap_err()
        .to_string();
    assert!(error.contains("at most 64 elements"), "{error}");

    let mut legacy_boundary = serde_json::to_value(BoundaryRecord::none()).unwrap();
    legacy_boundary["legacy_extension"] = serde_json::json!(true);
    assert_eq!(
        serde_json::from_value::<BoundaryRecord>(legacy_boundary).unwrap(),
        BoundaryRecord::none()
    );
}

#[test]
fn spherical_snapshot_rejects_impossible_surface_allocations() {
    let surface = spherical_surface(6_371_000.0);
    let encoded = serde_json::to_value(one_plate_snapshot(&surface)).unwrap();
    for (field, found) in [
        ("cell_count", MAX_SPHERICAL_CELL_COUNT + 1),
        ("edge_count", MAX_SPHERICAL_EDGE_COUNT + 1),
    ] {
        let mut oversized = encoded.clone();
        oversized["surface_ref"][field] = serde_json::json!(found);
        let error = serde_json::from_value::<SphericalTectonicSnapshot>(oversized)
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds spherical limit"), "{error}");
    }
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
    let build = |surface_ref, cell_plates: Vec<PlateId>, crust| {
        SphericalTectonicSnapshot::new(
            TECTONIC_SNAPSHOT_SCHEMA_V3,
            surface_ref,
            vec![SphericalPlate::new(
                PlateId::from_raw(0),
                CellId::from_raw(0),
                rotation,
            )],
            PlateIdField::from_ids(cell_plates),
            crust,
            vec![BoundaryRecord::none(); surface.edges().len()],
            Vec::new(),
        )
    };

    assert!(matches!(
        build(
            planar_identity,
            vec![PlateId::from_raw(0); surface.cells().len()],
            current_crust_state(surface.cells().len()),
        ),
        Err(SphericalTectonicValidationError::InvalidSurfaceKind { .. })
    ));
    assert!(matches!(
        build(
            SurfaceRef::for_spherical(&surface),
            vec![PlateId::from_raw(0); surface.cells().len() - 1],
            current_crust_state(surface.cells().len()),
        ),
        Err(SphericalTectonicValidationError::FieldLengthMismatch { .. })
    ));
    assert!(matches!(
        build(
            SurfaceRef::for_spherical(&surface),
            vec![PlateId::from_raw(0); surface.cells().len()],
            current_crust_state(surface.cells().len() - 1),
        ),
        Err(SphericalTectonicValidationError::FieldLengthMismatch { field: "crust", .. })
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
        TECTONIC_SNAPSHOT_SCHEMA_V3,
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
        current_crust_state(surface.cells().len()),
        vec![BoundaryRecord::none(); surface.edges().len()],
        Vec::new(),
    )
    .unwrap();
    assert!(matches!(
        excessive.validate_against(&surface),
        Err(SphericalTectonicValidationError::PlateSpeedOutOfRange { .. })
    ));
}

#[test]
fn current_crust_rejects_invalid_dense_state_before_snapshot_publication() {
    let base = CrustInputs::oceanic(4);

    for (field, mutate) in [
        ("thickness_km", 0_usize),
        ("age_myr", 1),
        ("tectonic_elevation_m", 2),
        ("lineation_east", 3),
        ("lineation_north", 4),
        ("orogeny_kind", 5),
        ("orogeny_age_myr", 6),
    ] {
        let mut inputs = base.clone();
        match mutate {
            0 => {
                inputs.thickness_km.pop();
            }
            1 => {
                inputs.age_myr.pop();
            }
            2 => {
                inputs.tectonic_elevation_m.pop();
            }
            3 => {
                inputs.lineation_east.pop();
            }
            4 => {
                inputs.lineation_north.pop();
            }
            5 => {
                inputs.orogeny_kind.pop();
            }
            6 => {
                inputs.orogeny_age_myr.pop();
            }
            _ => unreachable!(),
        }
        assert!(matches!(
            inputs.build(),
            Err(SphericalTectonicValidationError::FieldLengthMismatch {
                field: found,
                expected: 4,
                found: 3,
            }) if found == field
        ));
    }

    let mut oceanic_too_old = base.clone();
    oceanic_too_old.age_myr[0] = MAX_CRUST_AGE_MYR + 0.25;
    assert!(oceanic_too_old.build().is_err());

    let mut continental_without_sentinel = base.clone();
    continental_without_sentinel.kinds[0] = CrustKind::Continental;
    continental_without_sentinel.thickness_km[0] = CONTINENTAL_CRUST_MIN_THICKNESS_KM;
    continental_without_sentinel.age_myr[0] = 0.0;
    assert!(continental_without_sentinel.build().is_err());

    let mut non_unit_lineation = base.clone();
    non_unit_lineation.lineation_east[0] = 0.5;
    assert!(non_unit_lineation.build().is_err());

    let mut none_with_age = base.clone();
    none_with_age.orogeny_age_myr[0] = 0.0;
    assert!(none_with_age.build().is_err());

    let mut non_finite_elevation = base.clone();
    non_finite_elevation.tectonic_elevation_m[0] = f32::NAN;
    assert!(non_finite_elevation.build().is_err());

    let mut excessive_elevation = base;
    excessive_elevation.tectonic_elevation_m[0] = ELEVATION_MAX_M + 1.0;
    assert!(excessive_elevation.build().is_err());
}
