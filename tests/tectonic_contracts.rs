use sekai::world::natural::{
    BoundaryKind, BoundaryRecord, BoundarySegment, CrustKind, CrustKindField, Plate, PlateIdField,
    PlateVelocity, TectonicSnapshot, TectonicValidationError, MAX_PLATE_VELOCITY_MM_PER_YEAR,
    TECTONIC_SNAPSHOT_SCHEMA_V1,
};
use sekai::world::spatial::{
    SpatialCell, SpatialEdge, SpatialSnapshot, Topology, SPATIAL_SCHEMA_V1,
};
use sekai::world::{
    BoundaryCondition, BoundarySegmentId, CellId, EdgeId, Meters, PlateId, SquareMeters,
    WorldPoint, WorldRect,
};

type TectonicParts = (
    Vec<Plate>,
    PlateIdField,
    CrustKindField,
    Vec<f32>,
    Vec<BoundaryRecord>,
    Vec<BoundarySegment>,
);

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn square_meters(value: f64) -> SquareMeters {
    SquareMeters::new(value).unwrap()
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(meters(x), meters(y))
}

fn cell(id: u32, site: (f64, f64), polygon: &[(f64, f64)], neighbors: &[u32]) -> SpatialCell {
    SpatialCell {
        id: CellId::from_raw(id),
        site: point(site.0, site.1),
        centroid: point(site.0, site.1),
        area: square_meters(1.0),
        polygon: polygon.iter().map(|&(x, y)| point(x, y)).collect(),
        neighbors: neighbors.iter().copied().map(CellId::from_raw).collect(),
    }
}

fn edge(id: u32, start: (f64, f64), end: (f64, f64), cells: [Option<u32>; 2]) -> SpatialEdge {
    let start = point(start.0, start.1);
    let end = point(end.0, end.1);
    SpatialEdge {
        id: EdgeId::from_raw(id),
        start,
        end,
        length: meters((end.x().get() - start.x().get()).hypot(end.y().get() - start.y().get())),
        cells: cells.map(|owner| owner.map(CellId::from_raw)),
    }
}

fn spatial_fixture() -> SpatialSnapshot {
    let bounds = WorldRect::new(point(0.0, 0.0), point(2.0, 2.0)).unwrap();
    let cells = vec![
        cell(
            0,
            (0.5, 0.5),
            &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            &[1, 2],
        ),
        cell(
            1,
            (1.5, 0.5),
            &[(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0)],
            &[0, 3],
        ),
        cell(
            2,
            (0.5, 1.5),
            &[(0.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)],
            &[0, 3],
        ),
        cell(
            3,
            (1.5, 1.5),
            &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)],
            &[1, 2],
        ),
    ];
    let edges = vec![
        edge(0, (0.0, 0.0), (1.0, 0.0), [Some(0), None]),
        edge(1, (0.0, 1.0), (0.0, 0.0), [Some(0), None]),
        edge(2, (1.0, 0.0), (2.0, 0.0), [Some(1), None]),
        edge(3, (2.0, 0.0), (2.0, 1.0), [Some(1), None]),
        edge(4, (0.0, 2.0), (0.0, 1.0), [Some(2), None]),
        edge(5, (1.0, 2.0), (0.0, 2.0), [Some(2), None]),
        edge(6, (2.0, 1.0), (2.0, 2.0), [Some(3), None]),
        edge(7, (2.0, 2.0), (1.0, 2.0), [Some(3), None]),
        edge(8, (1.0, 0.0), (1.0, 1.0), [Some(0), Some(1)]),
        edge(9, (1.0, 1.0), (0.0, 1.0), [Some(0), Some(2)]),
        edge(10, (1.0, 1.0), (2.0, 1.0), [Some(1), Some(3)]),
        edge(11, (1.0, 1.0), (1.0, 2.0), [Some(2), Some(3)]),
    ];
    SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        bounds,
        BoundaryCondition::Closed,
        cells,
        edges,
    )
    .unwrap()
}

fn valid_parts() -> TectonicParts {
    let plates = vec![
        Plate {
            id: PlateId::from_raw(0),
            seed_cell: CellId::from_raw(0),
            velocity: PlateVelocity::new(15, 0).unwrap(),
        },
        Plate {
            id: PlateId::from_raw(1),
            seed_cell: CellId::from_raw(1),
            velocity: PlateVelocity::new(-15, 0).unwrap(),
        },
    ];
    let cell_plates = PlateIdField::from_ids(vec![
        PlateId::from_raw(0),
        PlateId::from_raw(1),
        PlateId::from_raw(0),
        PlateId::from_raw(1),
    ]);
    let crust_kinds = CrustKindField::from_kinds(vec![
        CrustKind::Continental,
        CrustKind::Oceanic,
        CrustKind::Continental,
        CrustKind::Oceanic,
    ]);
    let crust_thickness_km = vec![35.0, 7.0, 36.0, 8.0];
    let mut boundaries = vec![BoundaryRecord::none(); 12];
    boundaries[8] = BoundaryRecord::new(
        BoundaryKind::Transform,
        0.5,
        Some(BoundarySegmentId::from_raw(0)),
        None,
    );
    boundaries[11] = BoundaryRecord::new(
        BoundaryKind::Transform,
        0.7,
        Some(BoundarySegmentId::from_raw(0)),
        None,
    );
    let segments = vec![BoundarySegment {
        id: BoundarySegmentId::from_raw(0),
        plates: [PlateId::from_raw(0), PlateId::from_raw(1)],
        kind: BoundaryKind::Transform,
        member_edges: vec![EdgeId::from_raw(8), EdgeId::from_raw(11)],
        mean_strength: 0.6,
        subducting_plate: None,
        direction: [0.0, 1.0],
    }];
    (
        plates,
        cell_plates,
        crust_kinds,
        crust_thickness_km,
        boundaries,
        segments,
    )
}

fn tectonic_from(parts: TectonicParts) -> Result<TectonicSnapshot, TectonicValidationError> {
    let (plates, cell_plates, crust_kinds, thickness, boundaries, segments) = parts;
    TectonicSnapshot::new(
        TECTONIC_SNAPSHOT_SCHEMA_V1,
        4,
        12,
        plates,
        cell_plates,
        crust_kinds,
        thickness,
        boundaries,
        segments,
    )
}

fn valid_tectonic_fixture() -> TectonicSnapshot {
    tectonic_from(valid_parts()).unwrap()
}

#[test]
fn plate_velocity_is_fixed_point_and_bounded() {
    let velocity = PlateVelocity::new(
        -MAX_PLATE_VELOCITY_MM_PER_YEAR,
        MAX_PLATE_VELOCITY_MM_PER_YEAR,
    )
    .unwrap();
    assert_eq!(velocity.components_mm_per_year(), [-120, 120]);

    assert!(matches!(
        PlateVelocity::new(MAX_PLATE_VELOCITY_MM_PER_YEAR + 1, 0),
        Err(TectonicValidationError::PlateVelocityOutOfRange { .. })
    ));
}

#[test]
fn dense_category_fields_preserve_raw_storage_and_typed_access() {
    let snapshot = valid_tectonic_fixture();
    let plate_ptr = snapshot.cell_plates().raw_values().as_ptr();
    let crust_ptr = snapshot.crust_kinds().raw_values().as_ptr();

    assert_eq!(snapshot.cell_plates().raw_values().as_ptr(), plate_ptr);
    assert_eq!(snapshot.crust_kinds().raw_values().as_ptr(), crust_ptr);
    assert_eq!(
        snapshot.plate_for_cell(CellId::from_raw(2)),
        Some(PlateId::from_raw(0))
    );
    assert_eq!(
        snapshot.crust_kind(CellId::from_raw(1)),
        Some(CrustKind::Oceanic)
    );
    assert_eq!(CrustKind::try_from_raw(0).unwrap(), CrustKind::Oceanic);
    assert_eq!(CrustKind::Continental.raw(), 1);
    assert!(CrustKind::try_from_raw(2).is_err());
}

#[test]
fn valid_snapshot_round_trips_and_validates_against_space() {
    let spatial = spatial_fixture();
    let snapshot = valid_tectonic_fixture();

    snapshot.validate().unwrap();
    snapshot.validate_against(&spatial).unwrap();
    assert_eq!(snapshot.plates().len(), 2);
    assert_eq!(snapshot.boundaries().len(), spatial.edges().len());
    assert_eq!(snapshot.boundary_segments()[0].member_edges.len(), 2);

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: TectonicSnapshot = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    decoded.validate_against(&spatial).unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn rejects_non_contiguous_plates_and_invalid_cell_references() {
    let snapshot = valid_tectonic_fixture();
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    wire["plates"][1]["id"] = serde_json::json!(0);
    let invalid: TectonicSnapshot = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        invalid.validate(),
        Err(TectonicValidationError::NonContiguousPlateId { .. })
    ));

    let mut wire = serde_json::to_value(&snapshot).unwrap();
    wire["cell_plates"][0] = serde_json::json!(99);
    let invalid: TectonicSnapshot = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        invalid.validate(),
        Err(TectonicValidationError::InvalidCellPlate { .. })
    ));
}

#[test]
fn rejects_invalid_crust_codes_and_type_specific_thickness() {
    let snapshot = valid_tectonic_fixture();
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    wire["crust_kinds"][0] = serde_json::json!(9);
    let invalid: TectonicSnapshot = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        invalid.validate(),
        Err(TectonicValidationError::InvalidCrustKind { .. })
    ));

    let mut parts = valid_parts();
    parts.3[0] = 7.0;
    assert!(matches!(
        tectonic_from(parts),
        Err(TectonicValidationError::CrustThicknessOutOfRange { .. })
    ));
}

#[test]
fn rejects_edge_field_length_and_non_finite_or_unbounded_strength() {
    let mut parts = valid_parts();
    parts.4.pop();
    assert!(matches!(
        tectonic_from(parts),
        Err(TectonicValidationError::FieldLengthMismatch { .. })
    ));

    for strength in [f32::INFINITY, -0.1, 1.1] {
        let mut parts = valid_parts();
        parts.4[8].strength = strength;
        let result = tectonic_from(parts);
        assert!(
            matches!(
                result,
                Err(TectonicValidationError::BoundaryStrengthOutOfRange { .. })
            ),
            "strength {strength:?} returned {result:?}"
        );
    }
}

#[test]
fn rejects_invalid_segment_ids_membership_and_partitioning() {
    let snapshot = valid_tectonic_fixture();
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    wire["boundary_segments"][0]["id"] = serde_json::json!(2);
    let invalid: TectonicSnapshot = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        invalid.validate(),
        Err(TectonicValidationError::NonContiguousBoundarySegmentId { .. })
    ));

    for member_edges in [
        vec![],
        vec![EdgeId::from_raw(11), EdgeId::from_raw(8)],
        vec![EdgeId::from_raw(8), EdgeId::from_raw(8)],
    ] {
        let mut parts = valid_parts();
        parts.5[0].member_edges = member_edges;
        assert!(tectonic_from(parts).is_err());
    }

    let mut parts = valid_parts();
    parts.5[0].member_edges.pop();
    parts.5[0].mean_strength = 0.5;
    let result = tectonic_from(parts);
    assert!(
        matches!(
            result,
            Err(TectonicValidationError::BoundarySegmentMismatch { .. })
        ),
        "missing membership returned {result:?}"
    );
}

#[test]
fn topology_validation_rejects_same_plate_events_and_missing_cross_plate_events() {
    let spatial = spatial_fixture();

    let mut parts = valid_parts();
    parts.4[9] = BoundaryRecord::new(
        BoundaryKind::Weak,
        0.1,
        Some(BoundarySegmentId::from_raw(1)),
        None,
    );
    parts.5.push(BoundarySegment {
        id: BoundarySegmentId::from_raw(1),
        plates: [PlateId::from_raw(0), PlateId::from_raw(1)],
        kind: BoundaryKind::Weak,
        member_edges: vec![EdgeId::from_raw(9)],
        mean_strength: 0.1,
        subducting_plate: None,
        direction: [1.0, 0.0],
    });
    assert!(matches!(
        tectonic_from(parts).unwrap().validate_against(&spatial),
        Err(TectonicValidationError::BoundaryTopologyMismatch { .. })
    ));

    let mut parts = valid_parts();
    parts.4[8] = BoundaryRecord::none();
    parts.5[0].member_edges.remove(0);
    parts.5[0].mean_strength = 0.7;
    let result = tectonic_from(parts);
    assert!(
        matches!(
            result.as_ref().unwrap().validate_against(&spatial),
            Err(TectonicValidationError::BoundaryTopologyMismatch { .. })
        ),
        "missing cross-plate event returned {result:?}"
    );
}

#[test]
fn topology_validation_rejects_seed_ownership_and_disconnected_plates() {
    let spatial = spatial_fixture();

    let mut parts = valid_parts();
    parts.0[0].seed_cell = CellId::from_raw(1);
    assert!(matches!(
        tectonic_from(parts).unwrap().validate_against(&spatial),
        Err(TectonicValidationError::PlateSeedOwnership { .. })
    ));

    let mut parts = valid_parts();
    parts.1 = PlateIdField::from_ids(vec![
        PlateId::from_raw(0),
        PlateId::from_raw(1),
        PlateId::from_raw(1),
        PlateId::from_raw(0),
    ]);
    parts.4 = vec![BoundaryRecord::none(); 12];
    for edge_id in [8_usize, 9, 10, 11] {
        parts.4[edge_id] = BoundaryRecord::new(
            BoundaryKind::Transform,
            0.5,
            Some(BoundarySegmentId::from_raw(0)),
            None,
        );
    }
    parts.5[0].member_edges = vec![
        EdgeId::from_raw(8),
        EdgeId::from_raw(9),
        EdgeId::from_raw(10),
        EdgeId::from_raw(11),
    ];
    parts.5[0].mean_strength = 0.5;
    assert!(matches!(
        tectonic_from(parts).unwrap().validate_against(&spatial),
        Err(TectonicValidationError::DisconnectedPlate { .. })
    ));
}
