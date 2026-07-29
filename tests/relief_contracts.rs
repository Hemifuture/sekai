use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    ElevationField, LandOceanField, LandOceanKind, ReliefSnapshot, ReliefValidationError,
    CRUST_BASE_ELEVATION_MAX_M, CRUST_BASE_ELEVATION_MIN_M, ELEVATION_MAX_M, ELEVATION_MIN_M,
    REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M, RELIEF_SCHEMA_V1, TECTONIC_OFFSET_MAX_M,
    TECTONIC_OFFSET_MIN_M,
};
use sekai::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec};

fn field(values: &[f32]) -> ElevationField {
    ElevationField::from_values(values.to_vec()).unwrap()
}

fn valid_snapshot() -> ReliefSnapshot {
    ReliefSnapshot::new(
        RELIEF_SCHEMA_V1,
        4,
        0.0,
        field(&[-4_000.0, 100.0, 500.0, -1_000.0]),
        field(&[0.0, 100.0, -50.0, 0.0]),
        field(&[0.0, 0.0, 50.0, 0.0]),
        field(&[-4_000.0, 200.0, 500.0, -1_000.0]),
        LandOceanField::from_kinds(vec![
            LandOceanKind::Ocean,
            LandOceanKind::Land,
            LandOceanKind::Land,
            LandOceanKind::Ocean,
        ]),
    )
    .unwrap()
}

#[test]
fn elevation_fields_reject_non_finite_values() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(matches!(
            ElevationField::from_values(vec![0.0, value]),
            Err(ReliefValidationError::NonFiniteFieldValue { .. })
        ));
    }
}

#[test]
fn relief_snapshot_requires_exact_dense_lengths() {
    let snapshot = valid_snapshot();
    let mut wire = serde_json::to_value(snapshot).unwrap();
    wire["regional_offset_m"].as_array_mut().unwrap().pop();
    let invalid: ReliefSnapshot = serde_json::from_value(wire).unwrap();

    assert!(matches!(
        invalid.validate(),
        Err(ReliefValidationError::FieldLengthMismatch { .. })
    ));
}

#[test]
fn final_and_component_ranges_are_bounded() {
    for (field_name, value) in [
        ("crust_base_elevation_m", CRUST_BASE_ELEVATION_MIN_M - 1.0),
        ("crust_base_elevation_m", CRUST_BASE_ELEVATION_MAX_M + 1.0),
        ("tectonic_offset_m", TECTONIC_OFFSET_MIN_M - 1.0),
        ("tectonic_offset_m", TECTONIC_OFFSET_MAX_M + 1.0),
        ("regional_offset_m", REGIONAL_OFFSET_MIN_M - 1.0),
        ("regional_offset_m", REGIONAL_OFFSET_MAX_M + 1.0),
        ("elevation_m", ELEVATION_MIN_M - 1.0),
        ("elevation_m", ELEVATION_MAX_M + 1.0),
    ] {
        let mut wire = serde_json::to_value(valid_snapshot()).unwrap();
        wire[field_name][0] = serde_json::json!(value);
        let invalid: ReliefSnapshot = serde_json::from_value(wire).unwrap();
        assert!(
            matches!(
                invalid.validate(),
                Err(ReliefValidationError::FieldValueOutOfRange { .. })
            ),
            "{field_name} accepted {value}"
        );
    }
}

#[test]
fn final_elevation_must_equal_the_three_components() {
    let mut wire = serde_json::to_value(valid_snapshot()).unwrap();
    wire["elevation_m"][1] = serde_json::json!(201.0);
    let invalid: ReliefSnapshot = serde_json::from_value(wire).unwrap();

    assert!(matches!(
        invalid.validate(),
        Err(ReliefValidationError::ComponentIdentityMismatch { .. })
    ));
}

#[test]
fn centimeter_quantization_defines_the_shoreline_consistently() {
    let snapshot = ReliefSnapshot::new(
        RELIEF_SCHEMA_V1,
        4,
        0.0,
        field(&[-0.006, -0.004, 0.0, 0.004]),
        field(&[0.0; 4]),
        field(&[0.0; 4]),
        field(&[-0.006, -0.004, 0.0, 0.004]),
        LandOceanField::from_kinds(vec![
            LandOceanKind::Ocean,
            LandOceanKind::Land,
            LandOceanKind::Land,
            LandOceanKind::Land,
        ]),
    )
    .unwrap();

    assert_eq!(
        snapshot.land_ocean_kind(CellId::from_raw(0)),
        Some(LandOceanKind::Ocean)
    );
    assert_eq!(
        snapshot.land_ocean_kind(CellId::from_raw(1)),
        Some(LandOceanKind::Land)
    );
}

#[test]
fn sea_level_must_be_finite_and_categories_must_match_it() {
    let mut wire = serde_json::to_value(valid_snapshot()).unwrap();
    wire["sea_level_m"] = serde_json::json!(1.0e100);
    let invalid: ReliefSnapshot = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        invalid.validate(),
        Err(ReliefValidationError::NonFiniteSeaLevel { .. })
    ));

    let mut wire = serde_json::to_value(valid_snapshot()).unwrap();
    wire["land_ocean_kind"][0] = serde_json::json!(1);
    let invalid: ReliefSnapshot = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        invalid.validate(),
        Err(ReliefValidationError::LandOceanMismatch { .. })
    ));
}

#[test]
fn land_ocean_supports_typed_and_raw_borrowed_access() {
    let snapshot = valid_snapshot();

    assert_eq!(
        snapshot.land_ocean().raw_values(),
        &[0_u32, 1_u32, 1_u32, 0_u32]
    );
    assert_eq!(
        LandOceanKind::try_from_raw(0).unwrap(),
        LandOceanKind::Ocean
    );
    assert_eq!(LandOceanKind::Land.raw(), 1);
    assert!(LandOceanKind::try_from_raw(2).is_err());
    assert_eq!(
        snapshot.elevation_m().values(),
        &[-4_000.0, 200.0, 500.0, -1_000.0]
    );
}

#[test]
fn relief_snapshot_round_trips_and_revalidates_deserialized_data() {
    let snapshot = valid_snapshot();
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: ReliefSnapshot = serde_json::from_slice(&encoded).unwrap();

    decoded.validate().unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);

    let mut wire = serde_json::to_value(decoded).unwrap();
    wire["land_ocean_kind"][2] = serde_json::json!(9);
    let invalid: ReliefSnapshot = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        invalid.validate(),
        Err(ReliefValidationError::InvalidLandOceanKind { .. })
    ));
}

#[test]
fn topology_aware_validation_rejects_cell_count_mismatch() {
    let spatial = PlanarVoronoiBuilder::build(
        &PlanarSpaceSpec {
            width: Meters::new(1_000.0).unwrap(),
            height: Meters::new(500.0).unwrap(),
            target_cell_count: 16,
            boundary: BoundaryCondition::Closed,
        },
        &mut ChaCha8Rng::seed_from_u64(7),
    )
    .unwrap();

    assert!(matches!(
        valid_snapshot().validate_against(&spatial),
        Err(ReliefValidationError::SpatialCellCountMismatch { .. })
    ));
}
