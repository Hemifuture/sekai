use sekai::world::natural::{
    GeologicSpec, GeologicSpecError, MantleActivity, GEOLOGIC_SPEC_SCHEMA_V1, MAX_HOTSPOT_COUNT,
};
use sekai::world::HotspotId;

#[test]
fn hotspot_ids_round_trip_raw_values() {
    let id = HotspotId::from_raw(7);

    assert_eq!(id.raw(), 7);
    assert_eq!(
        serde_json::from_str::<HotspotId>(&serde_json::to_string(&id).unwrap()).unwrap(),
        id
    );
}

#[test]
fn default_geologic_spec_is_earthlike_and_valid() {
    let spec = GeologicSpec::default();

    assert_eq!(spec.schema_version, GEOLOGIC_SPEC_SCHEMA_V1);
    assert_eq!(spec.hotspot_count, 4);
    assert_eq!(spec.mantle_activity, MantleActivity::Moderate);
    spec.validate().unwrap();
}

#[test]
fn hotspot_count_accepts_inclusive_boundaries() {
    for count in [0, MAX_HOTSPOT_COUNT] {
        GeologicSpec {
            hotspot_count: count,
            ..GeologicSpec::default()
        }
        .validate()
        .unwrap();
    }
}

#[test]
fn invalid_schema_and_hotspot_count_are_rejected() {
    assert!(matches!(
        GeologicSpec {
            schema_version: 2,
            ..GeologicSpec::default()
        }
        .validate(),
        Err(GeologicSpecError::UnsupportedSchema { .. })
    ));
    assert!(matches!(
        GeologicSpec {
            hotspot_count: MAX_HOTSPOT_COUNT + 1,
            ..GeologicSpec::default()
        }
        .validate(),
        Err(GeologicSpecError::HotspotCountOutOfRange { .. })
    ));
}

#[test]
fn deserialization_cannot_bypass_validation() {
    let mut value = serde_json::to_value(GeologicSpec::default()).unwrap();
    value["schema_version"] = serde_json::json!(2);

    assert!(serde_json::from_value::<GeologicSpec>(value).is_err());
}
