use sekai::view::{
    CellGeometrySource, DisplayPrepareError, MeshCompleteness, PreparedCellMesh, MAX_DISPLAY_CELLS,
};
use sekai::world::spatial::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
use sekai::world::{
    BoundaryCondition, CellId, EdgeId, Meters, SquareMeters, WorldPoint, WorldRect,
};

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn square_meters(value: f64) -> SquareMeters {
    SquareMeters::new(value).unwrap()
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(meters(x), meters(y))
}

fn cell(
    id: u32,
    origin: (f64, f64),
    site: (f64, f64),
    polygon: &[(f64, f64)],
    neighbors: &[u32],
) -> SpatialCell {
    SpatialCell {
        id: CellId::from_raw(id),
        site: point(origin.0 + site.0, origin.1 + site.1),
        centroid: point(origin.0 + site.0, origin.1 + site.1),
        area: square_meters(1.0),
        polygon: polygon
            .iter()
            .map(|&(x, y)| point(origin.0 + x, origin.1 + y))
            .collect(),
        neighbors: neighbors.iter().copied().map(CellId::from_raw).collect(),
    }
}

fn edge(
    id: u32,
    origin: (f64, f64),
    start: (f64, f64),
    end: (f64, f64),
    cells: [Option<u32>; 2],
) -> SpatialEdge {
    let start = point(origin.0 + start.0, origin.1 + start.1);
    let end = point(origin.0 + end.0, origin.1 + end.1);
    let dx = end.x().get() - start.x().get();
    let dy = end.y().get() - start.y().get();
    SpatialEdge {
        id: EdgeId::from_raw(id),
        start,
        end,
        length: meters(dx.hypot(dy)),
        cells: cells.map(|owner| owner.map(CellId::from_raw)),
    }
}

fn four_cell_fixture_with_bounds(origin_x: f64, origin_y: f64) -> SpatialSnapshot {
    let origin = (origin_x, origin_y);
    let bounds = WorldRect::new(
        point(origin_x, origin_y),
        point(origin_x + 2.0, origin_y + 2.0),
    )
    .unwrap();
    let cells = vec![
        cell(
            0,
            origin,
            (0.5, 0.5),
            &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            &[1, 2],
        ),
        cell(
            1,
            origin,
            (1.5, 0.5),
            &[(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0)],
            &[0, 3],
        ),
        cell(
            2,
            origin,
            (0.5, 1.5),
            &[(0.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)],
            &[0, 3],
        ),
        cell(
            3,
            origin,
            (1.5, 1.5),
            &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)],
            &[1, 2],
        ),
    ];
    let edges = vec![
        edge(0, origin, (0.0, 0.0), (1.0, 0.0), [Some(0), None]),
        edge(1, origin, (0.0, 1.0), (0.0, 0.0), [Some(0), None]),
        edge(2, origin, (1.0, 0.0), (2.0, 0.0), [Some(1), None]),
        edge(3, origin, (2.0, 0.0), (2.0, 1.0), [Some(1), None]),
        edge(4, origin, (0.0, 2.0), (0.0, 1.0), [Some(2), None]),
        edge(5, origin, (1.0, 2.0), (0.0, 2.0), [Some(2), None]),
        edge(6, origin, (2.0, 1.0), (2.0, 2.0), [Some(3), None]),
        edge(7, origin, (2.0, 2.0), (1.0, 2.0), [Some(3), None]),
        edge(8, origin, (1.0, 0.0), (1.0, 1.0), [Some(0), Some(1)]),
        edge(9, origin, (1.0, 1.0), (0.0, 1.0), [Some(0), Some(2)]),
        edge(10, origin, (1.0, 1.0), (2.0, 1.0), [Some(1), Some(3)]),
        edge(11, origin, (1.0, 1.0), (1.0, 2.0), [Some(2), Some(3)]),
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

fn four_cell_fixture() -> SpatialSnapshot {
    four_cell_fixture_with_bounds(0.0, 0.0)
}

#[derive(Debug)]
struct TestGeometry {
    bounds: WorldRect,
    declared_cell_count: usize,
    polygons: Vec<Option<Vec<WorldPoint>>>,
}

impl CellGeometrySource for TestGeometry {
    fn bounds(&self) -> WorldRect {
        self.bounds
    }

    fn cell_count(&self) -> usize {
        self.declared_cell_count
    }

    fn polygon(&self, cell: CellId) -> Option<&[WorldPoint]> {
        self.polygons
            .get(cell.raw() as usize)
            .and_then(Option::as_deref)
    }
}

fn square(x: f64, y: f64) -> Vec<WorldPoint> {
    vec![
        point(x, y),
        point(x + 1.0, y),
        point(x + 1.0, y + 1.0),
        point(x, y + 1.0),
    ]
}

#[test]
fn spatial_snapshot_builds_stable_normalized_mesh() {
    let snapshot = four_cell_fixture_with_bounds(1_000_000.0, -2_000_000.0);
    let first = PreparedCellMesh::build(&snapshot, MeshCompleteness::RequireAll).unwrap();
    let second = PreparedCellMesh::build(&snapshot, MeshCompleteness::RequireAll).unwrap();

    assert_eq!(first.vertices(), second.vertices());
    assert_eq!(first.indices(), second.indices());
    assert_eq!(first.cell_count(), 4);
    assert_eq!(first.local_extent(), [2.0, 2.0]);
    assert!(first
        .vertices()
        .iter()
        .all(|vertex| (0.0..=1.0).contains(&vertex.position[0])
            && (0.0..=1.0).contains(&vertex.position[1])));
}

#[test]
fn mesh_keeps_cell_ids_aligned_with_field_indices() {
    let mesh = PreparedCellMesh::build(&four_cell_fixture(), MeshCompleteness::RequireAll).unwrap();
    for triangle in mesh.indices().chunks_exact(3) {
        let cells: Vec<_> = triangle
            .iter()
            .map(|index| mesh.vertices()[*index as usize].cell)
            .collect();
        assert_eq!(cells[0], cells[1]);
        assert_eq!(cells[1], cells[2]);
    }
}

#[test]
fn picker_returns_exact_cells_and_none_outside_bounds() {
    let mesh = PreparedCellMesh::build(&four_cell_fixture(), MeshCompleteness::RequireAll).unwrap();
    assert_eq!(
        mesh.pick_normalized([0.25, 0.25]),
        Some(CellId::from_raw(0))
    );
    assert_eq!(
        mesh.pick_normalized([0.75, 0.75]),
        Some(CellId::from_raw(3))
    );
    assert_eq!(mesh.pick_normalized([0.5, 0.5]), Some(CellId::from_raw(0)));
    assert_eq!(mesh.pick_normalized([-0.01, 0.5]), None);
    assert_eq!(mesh.pick_normalized([f32::NAN, 0.5]), None);
    assert_eq!(
        mesh.pick_local([mesh.local_extent()[0] * 0.25, mesh.local_extent()[1] * 0.25,]),
        Some(CellId::from_raw(0))
    );
    assert_eq!(mesh.pick_local([-0.1, 0.5]), None);
}

#[test]
fn completeness_is_explicit_and_present_malformed_geometry_is_never_skipped() {
    let bounds = WorldRect::new(point(0.0, 0.0), point(2.0, 1.0)).unwrap();
    let missing = TestGeometry {
        bounds,
        declared_cell_count: 2,
        polygons: vec![Some(square(0.0, 0.0)), None],
    };
    assert!(matches!(
        PreparedCellMesh::build(&missing, MeshCompleteness::RequireAll),
        Err(DisplayPrepareError::MissingCellGeometry { cell })
            if cell == CellId::from_raw(1)
    ));
    let partial = PreparedCellMesh::build(&missing, MeshCompleteness::AllowMissing).unwrap();
    assert_eq!(partial.cell_count(), 2);
    assert_eq!(partial.pick_local([1.5, 0.5]), None);

    let malformed = TestGeometry {
        bounds,
        declared_cell_count: 1,
        polygons: vec![Some(vec![point(0.0, 0.0), point(1.0, 0.0)])],
    };
    assert!(matches!(
        PreparedCellMesh::build(&malformed, MeshCompleteness::AllowMissing),
        Err(DisplayPrepareError::MalformedCellGeometry { .. })
    ));

    let repeated_vertex = TestGeometry {
        bounds,
        declared_cell_count: 1,
        polygons: vec![Some(vec![
            point(0.0, 0.0),
            point(1.0, 0.0),
            point(1.0, 0.0),
            point(1.0, 1.0),
            point(0.0, 1.0),
        ])],
    };
    assert!(matches!(
        PreparedCellMesh::build(&repeated_vertex, MeshCompleteness::AllowMissing),
        Err(DisplayPrepareError::MalformedCellGeometry { .. })
    ));
}

#[test]
fn mesh_rejects_cell_budget_and_out_of_bounds_geometry_before_output() {
    let bounds = WorldRect::new(point(0.0, 0.0), point(1.0, 1.0)).unwrap();
    let too_many = TestGeometry {
        bounds,
        declared_cell_count: MAX_DISPLAY_CELLS + 1,
        polygons: Vec::new(),
    };
    assert!(matches!(
        PreparedCellMesh::build(&too_many, MeshCompleteness::AllowMissing),
        Err(DisplayPrepareError::CellBudgetExceeded { .. })
    ));

    let outside = TestGeometry {
        bounds,
        declared_cell_count: 1,
        polygons: vec![Some(vec![
            point(0.0, 0.0),
            point(1.1, 0.0),
            point(0.0, 1.0),
        ])],
    };
    assert!(matches!(
        PreparedCellMesh::build(&outside, MeshCompleteness::RequireAll),
        Err(DisplayPrepareError::CoordinateOutOfBounds { .. })
    ));
}
