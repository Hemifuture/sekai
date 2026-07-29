use sekai::world::natural::{
    ClimateSpec, ClimateSpecError, CLIMATE_SPEC_SCHEMA_V1, MAX_AXIAL_TILT_CENTIDEG,
    MAX_LATITUDE_CENTIDEG, MAX_MOISTURE_SCALE_PERMILLE, MAX_TEMPERATURE_OFFSET_DECI_C,
    MIN_LATITUDE_CENTIDEG, MIN_LATITUDE_SPAN_CENTIDEG, MIN_MOISTURE_SCALE_PERMILLE,
    MIN_TEMPERATURE_OFFSET_DECI_C,
};

#[test]
fn default_climate_spec_is_the_v1_earthlike_baseline() {
    let spec = ClimateSpec::default();

    assert_eq!(spec.schema_version, CLIMATE_SPEC_SCHEMA_V1);
    assert_eq!(spec.south_latitude_centideg, -7_000);
    assert_eq!(spec.north_latitude_centideg, 7_000);
    assert_eq!(spec.axial_tilt_centideg, 2_340);
    assert_eq!(spec.temperature_offset_deci_c, 0);
    assert_eq!(spec.moisture_scale_permille, 1_000);
    spec.validate().unwrap();

    assert!((spec.south_latitude_degrees() + 70.0).abs() < f32::EPSILON);
    assert!((spec.north_latitude_degrees() - 70.0).abs() < f32::EPSILON);
    assert!((spec.axial_tilt_degrees() - 23.4).abs() < 0.000_1);
    assert_eq!(spec.temperature_offset_c(), 0.0);
    assert_eq!(spec.moisture_scale(), 1.0);
}

#[test]
fn accepts_all_inclusive_safety_boundaries() {
    ClimateSpec {
        south_latitude_centideg: MIN_LATITUDE_CENTIDEG,
        north_latitude_centideg: MAX_LATITUDE_CENTIDEG,
        axial_tilt_centideg: MAX_AXIAL_TILT_CENTIDEG,
        temperature_offset_deci_c: MIN_TEMPERATURE_OFFSET_DECI_C,
        moisture_scale_permille: MIN_MOISTURE_SCALE_PERMILLE,
        ..ClimateSpec::default()
    }
    .validate()
    .unwrap();

    ClimateSpec {
        south_latitude_centideg: 0,
        north_latitude_centideg: MIN_LATITUDE_SPAN_CENTIDEG,
        axial_tilt_centideg: 0,
        temperature_offset_deci_c: MAX_TEMPERATURE_OFFSET_DECI_C,
        moisture_scale_permille: MAX_MOISTURE_SCALE_PERMILLE,
        ..ClimateSpec::default()
    }
    .validate()
    .unwrap();
}

#[test]
fn rejects_each_invalid_climate_dimension_precisely() {
    assert!(matches!(
        ClimateSpec {
            schema_version: CLIMATE_SPEC_SCHEMA_V1 + 1,
            ..ClimateSpec::default()
        }
        .validate(),
        Err(ClimateSpecError::UnsupportedSchema { .. })
    ));
    assert!(matches!(
        ClimateSpec {
            south_latitude_centideg: MIN_LATITUDE_CENTIDEG - 1,
            ..ClimateSpec::default()
        }
        .validate(),
        Err(ClimateSpecError::SouthLatitudeOutOfRange { .. })
    ));
    assert!(matches!(
        ClimateSpec {
            north_latitude_centideg: MAX_LATITUDE_CENTIDEG + 1,
            ..ClimateSpec::default()
        }
        .validate(),
        Err(ClimateSpecError::NorthLatitudeOutOfRange { .. })
    ));
    for (south, north) in [(1_000, 1_000), (1_000, 1_999), (2_000, 1_000)] {
        assert!(matches!(
            ClimateSpec {
                south_latitude_centideg: south,
                north_latitude_centideg: north,
                ..ClimateSpec::default()
            }
            .validate(),
            Err(ClimateSpecError::LatitudeSpanOutOfRange { .. })
        ));
    }
    assert!(matches!(
        ClimateSpec {
            axial_tilt_centideg: MAX_AXIAL_TILT_CENTIDEG + 1,
            ..ClimateSpec::default()
        }
        .validate(),
        Err(ClimateSpecError::AxialTiltOutOfRange { .. })
    ));
    for temperature_offset_deci_c in [
        MIN_TEMPERATURE_OFFSET_DECI_C - 1,
        MAX_TEMPERATURE_OFFSET_DECI_C + 1,
    ] {
        assert!(matches!(
            ClimateSpec {
                temperature_offset_deci_c,
                ..ClimateSpec::default()
            }
            .validate(),
            Err(ClimateSpecError::TemperatureOffsetOutOfRange { .. })
        ));
    }
    for moisture_scale_permille in [
        MIN_MOISTURE_SCALE_PERMILLE - 1,
        MAX_MOISTURE_SCALE_PERMILLE + 1,
    ] {
        assert!(matches!(
            ClimateSpec {
                moisture_scale_permille,
                ..ClimateSpec::default()
            }
            .validate(),
            Err(ClimateSpecError::MoistureScaleOutOfRange { .. })
        ));
    }
}

#[test]
fn serialized_climate_spec_is_canonical_and_revalidated() {
    let spec = ClimateSpec::default();
    let encoded = serde_json::to_string(&spec).unwrap();
    let decoded: ClimateSpec = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, spec);
    assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded);

    let mut invalid = serde_json::to_value(spec).unwrap();
    invalid["moisture_scale_permille"] =
        serde_json::json!(u32::from(MAX_MOISTURE_SCALE_PERMILLE) + 1);
    assert!(serde_json::from_value::<ClimateSpec>(invalid).is_err());
}
