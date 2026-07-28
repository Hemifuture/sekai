use sekai::world::spatial::{
    SpatialCell, SpatialEdge, SpatialSnapshot, SpatialValidationError, Topology, SPATIAL_SCHEMA_V1,
};
use sekai::world::{
    BoundaryCondition, CellId, EdgeId, Meters, SquareMeters, WorldPoint, WorldRect,
};

type SpatialParts = (
    WorldRect,
    BoundaryCondition,
    Vec<SpatialCell>,
    Vec<SpatialEdge>,
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
    let dx = end.x().get() - start.x().get();
    let dy = end.y().get() - start.y().get();
    SpatialEdge {
        id: EdgeId::from_raw(id),
        start,
        end,
        length: meters(dx.hypot(dy)),
        cells: cells.map(|cell| cell.map(CellId::from_raw)),
    }
}

fn four_cell_parts() -> SpatialParts {
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
    (bounds, BoundaryCondition::Closed, cells, edges)
}

fn four_cell_fixture() -> SpatialSnapshot {
    let (bounds, boundary, cells, edges) = four_cell_parts();
    SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges).unwrap()
}

fn edge_mut(edges: &mut [SpatialEdge], id: u32) -> &mut SpatialEdge {
    edges
        .iter_mut()
        .find(|edge| edge.id == EdgeId::from_raw(id))
        .unwrap()
}

#[test]
fn validates_a_closed_four_cell_partition() {
    let snapshot = four_cell_fixture();
    snapshot.validate().unwrap();

    assert_eq!(snapshot.cell_count(), 4);
    assert_eq!(
        snapshot.neighbors(CellId::from_raw(0)).unwrap(),
        &[CellId::from_raw(1), CellId::from_raw(2)]
    );
    assert!((snapshot.total_cell_area().get() - 4.0).abs() < 1.0e-9);
}

#[test]
fn rejects_asymmetric_neighbors() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[1].neighbors.clear();
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::AsymmetricNeighbors { .. })
    ));
}

#[test]
fn constructor_sorts_only_cell_and_edge_records_by_id() {
    let (bounds, boundary, mut cells, mut edges) = four_cell_parts();
    cells.reverse();
    edges.reverse();

    let snapshot = SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges).unwrap();

    assert_eq!(snapshot.cell(CellId::from_raw(0)).unwrap().id.raw(), 0);
    assert_eq!(snapshot.edges()[0].id.raw(), 0);
}

#[test]
fn rejects_an_unsupported_schema() {
    let (bounds, boundary, cells, edges) = four_cell_parts();
    assert!(matches!(
        SpatialSnapshot::new(2, bounds, boundary, cells, edges),
        Err(SpatialValidationError::UnsupportedSchema { .. })
    ));
}

#[test]
fn rejects_duplicate_and_gapped_cell_ids() {
    for replacement in [CellId::from_raw(0), CellId::from_raw(9)] {
        let (bounds, boundary, mut cells, edges) = four_cell_parts();
        cells[1].id = replacement;
        assert!(matches!(
            SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
            Err(SpatialValidationError::NonContiguousCellId { .. })
        ));
    }
}

#[test]
fn rejects_duplicate_and_gapped_edge_ids() {
    for replacement in [EdgeId::from_raw(0), EdgeId::from_raw(99)] {
        let (bounds, boundary, cells, mut edges) = four_cell_parts();
        edges[1].id = replacement;
        assert!(matches!(
            SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
            Err(SpatialValidationError::NonContiguousEdgeId { .. })
        ));
    }
}

#[test]
fn rejects_polygons_with_fewer_than_three_vertices() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].polygon.truncate(2);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::PolygonTooSmall { .. })
    ));
}

#[test]
fn rejects_clockwise_or_zero_area_polygons() {
    for polygon in [
        vec![
            point(0.0, 1.0),
            point(1.0, 1.0),
            point(1.0, 0.0),
            point(0.0, 0.0),
        ],
        vec![point(0.0, 0.0), point(1.0, 0.0), point(2.0, 0.0)],
    ] {
        let (bounds, boundary, mut cells, edges) = four_cell_parts();
        cells[0].polygon = polygon;
        assert!(matches!(
            SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
            Err(SpatialValidationError::NonPositivePolygonArea { .. })
        ));
    }
}

#[test]
fn rejects_stored_area_that_differs_from_polygon_area() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].area = square_meters(1.1);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::AreaMismatch { .. })
    ));
}

#[test]
fn rejects_stored_centroid_that_differs_from_polygon_centroid() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].centroid = point(0.6, 0.5);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::CentroidMismatch { .. })
    ));
}

#[test]
fn rejects_sites_outside_the_scaled_bounds_tolerance() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].site = point(-3.0e-9, 0.5);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::SiteOutOfBounds { .. })
    ));
}

#[test]
fn rejects_polygon_vertices_outside_the_scaled_bounds_tolerance() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].polygon[0] = point(-3.0e-9, 0.0);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::PolygonVertexOutOfBounds { .. })
    ));
}

#[test]
fn rejects_unsorted_neighbors_without_normalizing_them() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].neighbors.swap(0, 1);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::UnsortedNeighbors { .. })
    ));
}

#[test]
fn rejects_duplicate_neighbors_independently_of_sorting() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].neighbors = vec![CellId::from_raw(1), CellId::from_raw(1)];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::DuplicateNeighbor { .. })
    ));
}

#[test]
fn rejects_out_of_range_and_self_neighbors() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].neighbors = vec![CellId::from_raw(1), CellId::from_raw(8)];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::InvalidNeighbor { .. })
    ));

    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[0].neighbors = vec![CellId::from_raw(0), CellId::from_raw(1)];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::SelfNeighbor { .. })
    ));
}

#[test]
fn rejects_edges_without_an_owner() {
    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    edge_mut(&mut edges, 0).cells = [None, None];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::InvalidEdgeOwnership { .. })
    ));
}

#[test]
fn rejects_duplicate_or_out_of_range_edge_owners() {
    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    edge_mut(&mut edges, 8).cells = [Some(CellId::from_raw(0)), Some(CellId::from_raw(0))];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::DuplicateEdgeOwner { .. })
    ));

    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    edge_mut(&mut edges, 8).cells = [Some(CellId::from_raw(0)), Some(CellId::from_raw(8))];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::InvalidEdgeOwner { .. })
    ));
}

#[test]
fn rejects_internal_edges_between_non_neighbors() {
    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    edge_mut(&mut edges, 8).cells = [Some(CellId::from_raw(0)), Some(CellId::from_raw(3))];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::InternalEdgeWithoutNeighbors { .. })
    ));
}

#[test]
fn rejects_missing_and_duplicate_internal_edges_for_neighbor_pairs() {
    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    edge_mut(&mut edges, 8).cells = [Some(CellId::from_raw(0)), None];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::NeighborEdgeCount { count: 0, .. })
    ));

    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    edge_mut(&mut edges, 11).cells = [Some(CellId::from_raw(0)), Some(CellId::from_raw(1))];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::NeighborEdgeCount { count: 2, .. })
    ));
}

#[test]
fn rejects_edges_that_do_not_match_each_owner_polygon() {
    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    *edge_mut(&mut edges, 8) = edge(8, (0.0, 0.0), (0.0, 1.0), [Some(0), Some(1)]);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::EdgeNotOnCellPolygon { .. })
    ));
}

#[test]
fn rejects_boundary_edges_that_are_not_on_the_world_rectangle() {
    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    *edge_mut(&mut edges, 0) = edge(0, (0.0, 1.0), (1.0, 1.0), [Some(0), None]);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::BoundaryEdgeOffBounds { .. })
    ));
}

#[test]
fn boundary_tolerance_applies_to_both_edge_endpoints() {
    let (bounds, boundary, mut cells, mut edges) = four_cell_parts();
    cells[0].polygon[0] = point(1.5e-9, 0.0);
    cells[0].polygon[3] = point(1.5e-9, 1.0);
    *edge_mut(&mut edges, 1) = edge(1, (1.5e-9, 1.0), (3.0e-9, 0.0), [Some(0), None]);

    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::BoundaryEdgeOffBounds { .. })
    ));
}

#[test]
fn rejects_stored_edge_lengths_that_differ_from_endpoints() {
    let (bounds, boundary, cells, mut edges) = four_cell_parts();
    edge_mut(&mut edges, 0).length = meters(1.1);
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::EdgeLengthMismatch { .. })
    ));
}

#[test]
fn rejects_cell_area_sums_that_do_not_cover_the_rectangle() {
    let (bounds, boundary, mut cells, edges) = four_cell_parts();
    cells[3].site = point(1.25, 1.5);
    cells[3].centroid = point(1.25, 1.5);
    cells[3].area = square_meters(0.5);
    cells[3].polygon = vec![
        point(1.0, 1.0),
        point(1.5, 1.0),
        point(1.5, 2.0),
        point(1.0, 2.0),
    ];
    assert!(matches!(
        SpatialSnapshot::new(SPATIAL_SCHEMA_V1, bounds, boundary, cells, edges),
        Err(SpatialValidationError::TotalAreaMismatch { .. })
    ));
}

#[test]
fn topology_queries_are_read_only_and_use_site_distance() {
    let snapshot = four_cell_fixture();

    assert_eq!(snapshot.bounds().width().get(), 2.0);
    assert_eq!(snapshot.edges().len(), 12);
    assert_eq!(
        snapshot
            .distance_between_sites(CellId::from_raw(0), CellId::from_raw(3))
            .unwrap()
            .get(),
        2.0_f64.sqrt()
    );
    assert!(snapshot.cell(CellId::from_raw(9)).is_none());
    assert!(snapshot.neighbors(CellId::from_raw(9)).is_none());
    assert!(snapshot
        .distance_between_sites(CellId::from_raw(0), CellId::from_raw(9))
        .is_none());
}

#[test]
fn validate_rechecks_deserialized_record_order() {
    let snapshot = four_cell_fixture();
    let mut wire = serde_json::to_value(snapshot).unwrap();
    wire["cells"].as_array_mut().unwrap().swap(0, 1);
    let deserialized: SpatialSnapshot = serde_json::from_value(wire).unwrap();

    assert!(matches!(
        deserialized.validate(),
        Err(SpatialValidationError::NonContiguousCellId { .. })
    ));
}

#[test]
fn canonical_record_order_makes_serialization_deterministic() {
    let (bounds, boundary, cells, edges) = four_cell_parts();
    let forward = SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        bounds,
        boundary,
        cells.clone(),
        edges.clone(),
    )
    .unwrap();
    let reverse = SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        bounds,
        boundary,
        cells.into_iter().rev().collect(),
        edges.into_iter().rev().collect(),
    )
    .unwrap();

    assert_eq!(
        serde_json::to_string(&forward).unwrap(),
        serde_json::to_string(&reverse).unwrap()
    );
}
