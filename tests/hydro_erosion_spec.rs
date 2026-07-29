use sekai::world::natural::{
    HydroErosionSpec, HydroErosionSpecError, HYDRO_EROSION_SPEC_SCHEMA_V1,
    MAX_EROSION_STRENGTH_PERMILLE, MAX_LAKE_DEPTH_CM, MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S,
    MIN_LAKE_DEPTH_CM, MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S,
};

#[test]
fn default_spec_is_the_v1_earthlike_baseline() {
    let spec = HydroErosionSpec::default();

    assert_eq!(spec.schema_version, HYDRO_EROSION_SPEC_SCHEMA_V1);
    assert_eq!(spec.river_discharge_threshold_deci_m3_s, 2_500);
    assert_eq!(spec.erosion_strength_permille, 1_000);
    assert_eq!(spec.minimum_lake_depth_cm, 100);
    spec.validate().unwrap();

    assert_eq!(spec.river_discharge_threshold_m3_s(), 250.0);
    assert_eq!(spec.erosion_strength(), 1.0);
    assert_eq!(spec.minimum_lake_depth_m(), 1.0);
}

#[test]
fn accepts_all_inclusive_safety_boundaries() {
    HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S,
        erosion_strength_permille: 0,
        minimum_lake_depth_cm: MIN_LAKE_DEPTH_CM,
        ..HydroErosionSpec::default()
    }
    .validate()
    .unwrap();

    HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S,
        erosion_strength_permille: MAX_EROSION_STRENGTH_PERMILLE,
        minimum_lake_depth_cm: MAX_LAKE_DEPTH_CM,
        ..HydroErosionSpec::default()
    }
    .validate()
    .unwrap();
}

#[test]
fn rejects_each_invalid_dimension_precisely() {
    assert!(matches!(
        HydroErosionSpec {
            schema_version: HYDRO_EROSION_SPEC_SCHEMA_V1 + 1,
            ..HydroErosionSpec::default()
        }
        .validate(),
        Err(HydroErosionSpecError::UnsupportedSchema { .. })
    ));

    for river_discharge_threshold_deci_m3_s in [
        MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S - 1,
        MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S + 1,
    ] {
        assert!(matches!(
            HydroErosionSpec {
                river_discharge_threshold_deci_m3_s,
                ..HydroErosionSpec::default()
            }
            .validate(),
            Err(HydroErosionSpecError::RiverDischargeThresholdOutOfRange { .. })
        ));
    }

    assert!(matches!(
        HydroErosionSpec {
            erosion_strength_permille: MAX_EROSION_STRENGTH_PERMILLE + 1,
            ..HydroErosionSpec::default()
        }
        .validate(),
        Err(HydroErosionSpecError::ErosionStrengthOutOfRange { .. })
    ));

    for minimum_lake_depth_cm in [MIN_LAKE_DEPTH_CM - 1, MAX_LAKE_DEPTH_CM + 1] {
        assert!(matches!(
            HydroErosionSpec {
                minimum_lake_depth_cm,
                ..HydroErosionSpec::default()
            }
            .validate(),
            Err(HydroErosionSpecError::MinimumLakeDepthOutOfRange { .. })
        ));
    }
}

#[test]
fn serialized_spec_is_canonical_and_revalidated() {
    let spec = HydroErosionSpec::default();
    let encoded = serde_json::to_string(&spec).unwrap();
    let decoded: HydroErosionSpec = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, spec);
    assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded);

    let mut invalid = serde_json::to_value(spec).unwrap();
    invalid["minimum_lake_depth_cm"] = serde_json::json!(0);
    assert!(serde_json::from_value::<HydroErosionSpec>(invalid).is_err());
}
