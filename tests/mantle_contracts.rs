use sekai::world::natural::{
    Hotspot, MantleSnapshot, MantleValidationError, HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2,
    MANTLE_SNAPSHOT_SCHEMA_V1, MAX_HOTSPOT_COUNT,
};
use sekai::world::spatial::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
use sekai::world::{
    BoundaryCondition, CellId, EdgeId, HotspotId, Meters, SquareMeters, WorldPoint, WorldRect,
};

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(meters(x), meters(y))
}

fn cell(id: u32, site: (f64, f64), polygon: &[(f64, f64)], neighbors: &[u32]) -> SpatialCell {
    SpatialCell {
        id: CellId::from_raw(id),
        site: point(site.0, site.1),
        centroid: point(site.0, site.1),
        area: SquareMeters::new(1_000_000_000_000.0).unwrap(),
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
        cells: cells.map(|cell| cell.map(CellId::from_raw)),
    }
}

fn four_cell_spatial_fixture() -> SpatialSnapshot {
    let m = 1_000_000.0;
    let cells = vec![
        cell(
            0,
            (0.5 * m, 0.5 * m),
            &[(0.0, 0.0), (m, 0.0), (m, m), (0.0, m)],
            &[1, 2],
        ),
        cell(
            1,
            (1.5 * m, 0.5 * m),
            &[(m, 0.0), (2.0 * m, 0.0), (2.0 * m, m), (m, m)],
            &[0, 3],
        ),
        cell(
            2,
            (0.5 * m, 1.5 * m),
            &[(0.0, m), (m, m), (m, 2.0 * m), (0.0, 2.0 * m)],
            &[0, 3],
        ),
        cell(
            3,
            (1.5 * m, 1.5 * m),
            &[(m, m), (2.0 * m, m), (2.0 * m, 2.0 * m), (m, 2.0 * m)],
            &[1, 2],
        ),
    ];
    let edges = vec![
        edge(0, (0.0, 0.0), (m, 0.0), [Some(0), None]),
        edge(1, (0.0, m), (0.0, 0.0), [Some(0), None]),
        edge(2, (m, 0.0), (2.0 * m, 0.0), [Some(1), None]),
        edge(3, (2.0 * m, 0.0), (2.0 * m, m), [Some(1), None]),
        edge(4, (0.0, 2.0 * m), (0.0, m), [Some(2), None]),
        edge(5, (m, 2.0 * m), (0.0, 2.0 * m), [Some(2), None]),
        edge(6, (2.0 * m, m), (2.0 * m, 2.0 * m), [Some(3), None]),
        edge(7, (2.0 * m, 2.0 * m), (m, 2.0 * m), [Some(3), None]),
        edge(8, (m, 0.0), (m, m), [Some(0), Some(1)]),
        edge(9, (m, m), (0.0, m), [Some(0), Some(2)]),
        edge(10, (m, m), (2.0 * m, m), [Some(1), Some(3)]),
        edge(11, (m, m), (m, 2.0 * m), [Some(2), Some(3)]),
    ];
    SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        WorldRect::new(point(0.0, 0.0), point(2.0 * m, 2.0 * m)).unwrap(),
        BoundaryCondition::Closed,
        cells,
        edges,
    )
    .unwrap()
}

fn hotspot(id: u32, source: u32) -> Hotspot {
    Hotspot::new(
        HotspotId::from_raw(id),
        CellId::from_raw(source),
        800,
        meters(250_000.0),
    )
    .unwrap()
}

fn valid_snapshot() -> MantleSnapshot {
    MantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V1,
        4,
        vec![hotspot(0, 1)],
        vec![50.0, 190.0, 90.0, 55.0],
        vec![0.0, 1.0, 0.3, 0.0],
    )
    .unwrap()
}

#[test]
fn mantle_snapshot_accepts_valid_current_slice_fields() {
    let spatial = four_cell_spatial_fixture();
    let snapshot = valid_snapshot();

    snapshot.validate().unwrap();
    snapshot.validate_against(&spatial).unwrap();
    assert_eq!(snapshot.schema_version(), MANTLE_SNAPSHOT_SCHEMA_V1);
    assert_eq!(snapshot.cell_count(), 4);
    assert_eq!(snapshot.hotspots()[0].id(), HotspotId::from_raw(0));
    assert_eq!(snapshot.hotspots()[0].source_cell(), CellId::from_raw(1));
    assert_eq!(snapshot.hotspots()[0].strength_permille(), 800);
    assert_eq!(snapshot.hotspots()[0].support_radius_m().get(), 250_000.0);
    assert_eq!(snapshot.heat_flow_mw_m2(), &[50.0, 190.0, 90.0, 55.0]);
    assert_eq!(snapshot.volcanic_influence(), &[0.0, 1.0, 0.3, 0.0]);
}

#[test]
fn hotspot_constructor_rejects_invalid_strength_and_radius() {
    for strength in [0, 1_001] {
        assert!(matches!(
            Hotspot::new(
                HotspotId::from_raw(0),
                CellId::from_raw(0),
                strength,
                meters(1.0)
            ),
            Err(MantleValidationError::HotspotStrengthOutOfRange { .. })
        ));
    }
    for radius in [0.0, -1.0] {
        assert!(matches!(
            Hotspot::new(
                HotspotId::from_raw(0),
                CellId::from_raw(0),
                1,
                meters(radius)
            ),
            Err(MantleValidationError::InvalidSupportRadius { .. })
        ));
    }
    assert!(Meters::new(f64::NAN).is_err());
}

#[test]
fn snapshot_rejects_schema_identity_source_and_length_errors() {
    assert!(matches!(
        MantleSnapshot::new(2, 4, vec![hotspot(0, 0)], vec![50.0; 4], vec![0.0; 4]),
        Err(MantleValidationError::UnsupportedSchema { .. })
    ));
    assert!(matches!(
        MantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V1,
            4,
            vec![hotspot(1, 0)],
            vec![50.0; 4],
            vec![0.0; 4]
        ),
        Err(MantleValidationError::NonContiguousHotspotId { .. })
    ));
    assert!(matches!(
        MantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V1,
            4,
            vec![hotspot(0, 1), hotspot(1, 1)],
            vec![50.0; 4],
            vec![0.0; 4]
        ),
        Err(MantleValidationError::DuplicateHotspotSourceCell { .. })
    ));
    assert!(matches!(
        MantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V1,
            4,
            vec![hotspot(0, 4)],
            vec![50.0; 4],
            vec![0.0; 4]
        ),
        Err(MantleValidationError::HotspotSourceCellOutOfRange { .. })
    ));
    assert!(matches!(
        MantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V1,
            4,
            vec![hotspot(0, 0)],
            vec![50.0; 3],
            vec![0.0; 4]
        ),
        Err(MantleValidationError::FieldLengthMismatch { .. })
    ));

    let too_many = (0..=u32::from(MAX_HOTSPOT_COUNT))
        .map(|id| hotspot(id, id))
        .collect();
    assert!(matches!(
        MantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V1,
            u32::from(MAX_HOTSPOT_COUNT) + 1,
            too_many,
            vec![50.0; usize::from(MAX_HOTSPOT_COUNT) + 1],
            vec![0.0; usize::from(MAX_HOTSPOT_COUNT) + 1]
        ),
        Err(MantleValidationError::TooManyHotspots { .. })
    ));
}

#[test]
fn dense_fields_reject_non_finite_and_out_of_range_values() {
    for value in [
        f32::NAN,
        f32::INFINITY,
        HEAT_FLOW_MIN_MW_M2 - 1.0,
        HEAT_FLOW_MAX_MW_M2 + 1.0,
    ] {
        let mut heat = vec![50.0; 4];
        heat[2] = value;
        assert!(matches!(
            MantleSnapshot::new(
                MANTLE_SNAPSHOT_SCHEMA_V1,
                4,
                vec![hotspot(0, 0)],
                heat,
                vec![0.0; 4]
            ),
            Err(MantleValidationError::HeatFlowOutOfRange { .. })
        ));
    }

    for value in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
        let mut influence = vec![0.0; 4];
        influence[2] = value;
        assert!(matches!(
            MantleSnapshot::new(
                MANTLE_SNAPSHOT_SCHEMA_V1,
                4,
                vec![hotspot(0, 0)],
                vec![50.0; 4],
                influence
            ),
            Err(MantleValidationError::VolcanicInfluenceOutOfRange { .. })
        ));
    }
}

#[test]
fn topology_validation_rejects_count_and_radius_mismatch() {
    let spatial = four_cell_spatial_fixture();
    let wrong_count = MantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V1,
        3,
        vec![hotspot(0, 0)],
        vec![50.0; 3],
        vec![0.0; 3],
    )
    .unwrap();
    assert!(matches!(
        wrong_count.validate_against(&spatial),
        Err(MantleValidationError::SpatialCellCountMismatch { .. })
    ));

    let over_diagonal = MantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V1,
        4,
        vec![Hotspot::new(
            HotspotId::from_raw(0),
            CellId::from_raw(0),
            800,
            meters(3_000_000.0),
        )
        .unwrap()],
        vec![50.0; 4],
        vec![0.0; 4],
    )
    .unwrap();
    assert!(matches!(
        over_diagonal.validate_against(&spatial),
        Err(MantleValidationError::SupportRadiusExceedsWorldDiagonal { .. })
    ));
}

#[test]
fn constructor_canonicalizes_hotspots_and_deserialization_revalidates() {
    let snapshot = MantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V1,
        4,
        vec![hotspot(1, 3), hotspot(0, 1)],
        vec![50.0; 4],
        vec![0.0; 4],
    )
    .unwrap();
    assert_eq!(snapshot.hotspots()[0].id(), HotspotId::from_raw(0));
    assert_eq!(snapshot.hotspots()[1].id(), HotspotId::from_raw(1));

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: MantleSnapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, snapshot);

    let mut malformed = serde_json::to_value(&snapshot).unwrap();
    malformed["hotspots"][1]["id"] = serde_json::json!(3);
    assert!(serde_json::from_value::<MantleSnapshot>(malformed).is_err());
}
