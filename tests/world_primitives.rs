use sekai::world::{
    AuthorObjectId, CellId, DrainageBasinId, LakeId, Meters, RiverSegmentId, RootSeed,
    SquareMeters, WorldPoint, WorldRect,
};
use serde::de::value::{Error as ValueError, F64Deserializer};
use serde::Deserialize;

#[test]
fn typed_ids_round_trip_raw_values() {
    assert_eq!(CellId::from_raw(7).raw(), 7);
    assert_eq!(DrainageBasinId::from_raw(8).raw(), 8);
    assert_eq!(LakeId::from_raw(9).raw(), 9);
    assert_eq!(RiverSegmentId::from_raw(10).raw(), 10);
    assert_eq!(AuthorObjectId::from_raw(99).raw(), 99);
    assert_eq!(RootSeed::new(42).raw(), 42);

    for encoded in ["8", "9", "10"] {
        let decoded = match encoded {
            "8" => serde_json::from_str::<DrainageBasinId>(encoded)
                .unwrap()
                .raw(),
            "9" => serde_json::from_str::<LakeId>(encoded).unwrap().raw(),
            _ => serde_json::from_str::<RiverSegmentId>(encoded)
                .unwrap()
                .raw(),
        };
        assert_eq!(decoded.to_string(), encoded);
    }
}

#[test]
fn units_reject_non_finite_values() {
    assert!(Meters::new(f64::NAN).is_err());
    assert!(Meters::new(f64::INFINITY).is_err());
    assert!(SquareMeters::new(-1.0).is_err());
}

#[test]
fn units_reject_invalid_deserialized_values() {
    assert!(Meters::deserialize(F64Deserializer::<ValueError>::new(f64::NAN)).is_err());
    assert!(SquareMeters::deserialize(F64Deserializer::<ValueError>::new(f64::INFINITY)).is_err());
    assert!(serde_json::from_str::<SquareMeters>("-1.0").is_err());
}

#[test]
fn rectangle_validates_ordered_corners() {
    let min = WorldPoint::new(Meters::new(0.0).unwrap(), Meters::new(0.0).unwrap());
    let max = WorldPoint::new(Meters::new(100.0).unwrap(), Meters::new(50.0).unwrap());
    let rect = WorldRect::new(min, max).unwrap();

    assert_eq!(rect.width().get(), 100.0);
    assert_eq!(rect.height().get(), 50.0);
    assert!(rect.contains(WorldPoint::new(
        Meters::new(25.0).unwrap(),
        Meters::new(10.0).unwrap(),
    )));
    assert!(rect.contains(min));
    assert!(rect.contains(max));
}

#[test]
fn rectangle_rejects_zero_and_reversed_dimensions() {
    let origin = WorldPoint::new(Meters::new(0.0).unwrap(), Meters::new(0.0).unwrap());
    let right = WorldPoint::new(Meters::new(1.0).unwrap(), Meters::new(0.0).unwrap());
    let above = WorldPoint::new(Meters::new(0.0).unwrap(), Meters::new(1.0).unwrap());
    let diagonal = WorldPoint::new(Meters::new(1.0).unwrap(), Meters::new(1.0).unwrap());

    assert!(WorldRect::new(origin, above).is_err());
    assert!(WorldRect::new(origin, right).is_err());
    assert!(WorldRect::new(diagonal, origin).is_err());
}

#[test]
fn rectangles_reject_invalid_deserialized_dimensions() {
    assert!(serde_json::from_str::<WorldRect>(
        r#"{"min":{"x":0.0,"y":0.0},"max":{"x":0.0,"y":1.0}}"#
    )
    .is_err());
    assert!(serde_json::from_str::<WorldRect>(
        r#"{"min":{"x":0.0,"y":0.0},"max":{"x":1.0,"y":0.0}}"#
    )
    .is_err());
    assert!(serde_json::from_str::<WorldRect>(
        r#"{"min":{"x":1.0,"y":1.0},"max":{"x":0.0,"y":0.0}}"#
    )
    .is_err());
}
