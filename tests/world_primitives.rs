use sekai::world::{AuthorObjectId, CellId, Meters, RootSeed, SquareMeters, WorldPoint, WorldRect};

#[test]
fn typed_ids_round_trip_raw_values() {
    assert_eq!(CellId::from_raw(7).raw(), 7);
    assert_eq!(AuthorObjectId::from_raw(99).raw(), 99);
    assert_eq!(RootSeed::new(42).raw(), 42);
}

#[test]
fn units_reject_non_finite_values() {
    assert!(Meters::new(f64::NAN).is_err());
    assert!(Meters::new(f64::INFINITY).is_err());
    assert!(SquareMeters::new(-1.0).is_err());
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
}
