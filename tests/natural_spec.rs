use sekai::{
    engine::Artifact,
    generators::natural::ReliefSpecArtifact,
    world::{
        natural::{
            NaturalSpecError, ReliefSpec, ReliefSpecError, TectonicActivity, TectonicSpec,
            MAX_CONTINENTAL_CRUST_FRACTION, MAX_PLATE_COUNT, MAX_TARGET_LAND_FRACTION,
            MAX_WATER_INVENTORY_RATIO, MIN_CONTINENTAL_CRUST_FRACTION, MIN_PLATE_COUNT,
            MIN_TARGET_LAND_FRACTION, MIN_WATER_INVENTORY_RATIO, RELIEF_SPEC_SCHEMA_V2,
            TECTONIC_SPEC_SCHEMA_V1,
        },
        BoundarySegmentId, PlateId,
    },
};

#[test]
fn natural_ids_preserve_their_raw_and_serialized_values() {
    let plate = PlateId::from_raw(7);
    let boundary = BoundarySegmentId::from_raw(11);

    assert_eq!(plate.raw(), 7);
    assert_eq!(boundary.raw(), 11);
    assert_eq!(serde_json::to_string(&plate).unwrap(), "7");
    assert_eq!(serde_json::to_string(&boundary).unwrap(), "11");
    assert_eq!(serde_json::from_str::<PlateId>("7").unwrap(), plate);
    assert_eq!(
        serde_json::from_str::<BoundarySegmentId>("11").unwrap(),
        boundary
    );
}

#[test]
fn default_tectonic_spec_is_the_v1_earthlike_baseline() {
    let spec = TectonicSpec::default();

    assert_eq!(spec.schema_version, TECTONIC_SPEC_SCHEMA_V1);
    assert_eq!(spec.plate_count, 12);
    assert_eq!(spec.continental_crust_fraction, 0.38);
    assert_eq!(spec.activity, TectonicActivity::Moderate);
    assert!(spec.validate().is_ok());
}

#[test]
fn accepts_inclusive_tectonic_safety_boundaries() {
    for plate_count in [MIN_PLATE_COUNT, MAX_PLATE_COUNT] {
        let spec = TectonicSpec {
            plate_count,
            ..TectonicSpec::default()
        };
        assert!(spec.validate().is_ok());
    }

    for continental_crust_fraction in [
        MIN_CONTINENTAL_CRUST_FRACTION,
        MAX_CONTINENTAL_CRUST_FRACTION,
    ] {
        let spec = TectonicSpec {
            continental_crust_fraction,
            ..TectonicSpec::default()
        };
        assert!(spec.validate().is_ok());
    }
}

#[test]
fn rejects_unsupported_schema() {
    let spec = TectonicSpec {
        schema_version: TECTONIC_SPEC_SCHEMA_V1 + 1,
        ..TectonicSpec::default()
    };

    assert_eq!(
        spec.validate(),
        Err(NaturalSpecError::UnsupportedSchema {
            found: TECTONIC_SPEC_SCHEMA_V1 + 1,
            supported: TECTONIC_SPEC_SCHEMA_V1,
        })
    );
}

#[test]
fn rejects_plate_counts_outside_the_safety_range() {
    for plate_count in [MIN_PLATE_COUNT - 1, MAX_PLATE_COUNT + 1] {
        let spec = TectonicSpec {
            plate_count,
            ..TectonicSpec::default()
        };
        assert_eq!(
            spec.validate(),
            Err(NaturalSpecError::PlateCountOutOfRange {
                found: plate_count,
                min: MIN_PLATE_COUNT,
                max: MAX_PLATE_COUNT,
            })
        );
    }
}

#[test]
fn rejects_non_finite_and_out_of_range_continental_fraction() {
    for continental_crust_fraction in [
        f32::NAN,
        f32::INFINITY,
        MIN_CONTINENTAL_CRUST_FRACTION - 0.01,
        MAX_CONTINENTAL_CRUST_FRACTION + 0.01,
    ] {
        let spec = TectonicSpec {
            continental_crust_fraction,
            ..TectonicSpec::default()
        };

        assert!(matches!(
            spec.validate(),
            Err(NaturalSpecError::ContinentalCrustFractionOutOfRange { .. })
        ));
    }
}

#[test]
fn tectonic_spec_has_a_deterministic_json_round_trip() {
    let spec = TectonicSpec::default();
    let encoded = serde_json::to_string(&spec).unwrap();
    let decoded: TectonicSpec = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, spec);
    assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded);
}

#[test]
fn default_relief_spec_is_a_valid_explicit_land_area_target() {
    let spec = ReliefSpec::default();

    assert_eq!(spec.schema_version, RELIEF_SPEC_SCHEMA_V2);
    assert_eq!(spec.target_land_fraction, 0.38);
    assert_eq!(spec.validate(), Ok(()));
    assert!(ReliefSpecArtifact::new(spec).validate().is_ok());
}

#[test]
fn default_relief_spec_serializes_the_physical_water_inventory_driver() {
    let encoded = serde_json::to_value(ReliefSpec::default()).unwrap();

    assert_eq!(encoded["schema_version"], serde_json::json!(2));
    assert_eq!(
        encoded["sea_level_policy"],
        serde_json::json!("WaterInventory")
    );
    assert_eq!(encoded["water_inventory_ratio"], serde_json::json!(1.0));
}

#[test]
fn relief_spec_deserialization_enforces_the_water_inventory_ratio_range() {
    let valid = serde_json::json!({
        "schema_version": 2,
        "target_land_fraction": 0.38,
        "sea_level_policy": "TargetLandFraction",
        "water_inventory_ratio": 0.05,
    });
    assert!(serde_json::from_value::<ReliefSpec>(valid.clone()).is_ok());

    for ratio in [
        serde_json::json!(0.049),
        serde_json::json!(5.001),
        serde_json::json!("NaN"),
    ] {
        let mut invalid = valid.clone();
        invalid["water_inventory_ratio"] = ratio;
        assert!(serde_json::from_value::<ReliefSpec>(invalid).is_err());
    }
}

#[test]
fn relief_spec_accepts_only_finite_inclusive_land_area_bounds() {
    for target_land_fraction in [MIN_TARGET_LAND_FRACTION, MAX_TARGET_LAND_FRACTION] {
        assert_eq!(
            ReliefSpec {
                target_land_fraction,
                ..ReliefSpec::default()
            }
            .validate(),
            Ok(())
        );
    }

    for target_land_fraction in [
        f32::NAN,
        f32::INFINITY,
        MIN_TARGET_LAND_FRACTION - 0.01,
        MAX_TARGET_LAND_FRACTION + 0.01,
    ] {
        assert!(matches!(
            ReliefSpec {
                target_land_fraction,
                ..ReliefSpec::default()
            }
            .validate(),
            Err(ReliefSpecError::TargetLandFractionOutOfRange { .. })
        ));
    }

    assert_eq!(
        ReliefSpec {
            schema_version: RELIEF_SPEC_SCHEMA_V2 + 1,
            ..ReliefSpec::default()
        }
        .validate(),
        Err(ReliefSpecError::UnsupportedSchema {
            found: RELIEF_SPEC_SCHEMA_V2 + 1,
            supported: RELIEF_SPEC_SCHEMA_V2,
        })
    );
}

#[test]
fn relief_spec_accepts_only_finite_inclusive_water_inventory_bounds() {
    for water_inventory_ratio in [MIN_WATER_INVENTORY_RATIO, MAX_WATER_INVENTORY_RATIO] {
        assert_eq!(
            ReliefSpec {
                water_inventory_ratio,
                ..ReliefSpec::default()
            }
            .validate(),
            Ok(())
        );
    }

    for water_inventory_ratio in [
        f32::NAN,
        f32::INFINITY,
        MIN_WATER_INVENTORY_RATIO - 0.01,
        MAX_WATER_INVENTORY_RATIO + 0.01,
    ] {
        assert!(matches!(
            ReliefSpec {
                water_inventory_ratio,
                ..ReliefSpec::default()
            }
            .validate(),
            Err(ReliefSpecError::WaterInventoryRatioOutOfRange { .. })
        ));
    }
}

#[test]
fn relief_spec_deserialization_validates_before_returning_a_value() {
    let spec = ReliefSpec {
        target_land_fraction: 0.55,
        ..ReliefSpec::default()
    };
    let encoded = serde_json::to_string(&spec).unwrap();
    assert_eq!(serde_json::from_str::<ReliefSpec>(&encoded).unwrap(), spec);

    let invalid = format!(
        r#"{{"schema_version":{},"target_land_fraction":0.38,"sea_level_policy":"WaterInventory","water_inventory_ratio":1.0}}"#,
        RELIEF_SPEC_SCHEMA_V2 + 1
    );
    assert!(serde_json::from_str::<ReliefSpec>(&invalid).is_err());
}
