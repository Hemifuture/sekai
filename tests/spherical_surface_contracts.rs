use sekai::world::spatial::{
    central_angle, SphericalSurfaceCell, SphericalSurfaceEdge, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SphericalSurfaceVertex, UnitVector3,
    SPHERICAL_SURFACE_SCHEMA_V1,
};
use sekai::world::{CellId, EdgeId, Meters, SquareMeters, SurfaceVertexId};
use serde_json::{json, Value};

const RADIUS: f64 = 2.0;

#[test]
fn snapshot_is_the_single_source_of_surface_geometry_and_adjacency() {
    let snapshot = tetrahedral_snapshot();

    assert_eq!(snapshot.schema_version(), SPHERICAL_SURFACE_SCHEMA_V1);
    assert_eq!(snapshot.radius().get(), RADIUS);
    assert_eq!(snapshot.vertices().len(), 4);
    assert_eq!(snapshot.cells().len(), 4);
    assert_eq!(snapshot.edges().len(), 6);
    assert_eq!(
        snapshot
            .vertex(SurfaceVertexId::from_raw(2))
            .unwrap()
            .id
            .raw(),
        2
    );
    assert_eq!(snapshot.cell(CellId::from_raw(3)).unwrap().id.raw(), 3);
    assert_eq!(snapshot.edge(EdgeId::from_raw(5)).unwrap().id.raw(), 5);
    assert!(snapshot.vertex(SurfaceVertexId::from_raw(4)).is_none());
    assert!(snapshot.cell(CellId::from_raw(4)).is_none());
    assert!(snapshot.edge(EdgeId::from_raw(6)).is_none());
    assert_eq!(
        snapshot.cell_edges(CellId::from_raw(0)).unwrap(),
        &[
            EdgeId::from_raw(4),
            EdgeId::from_raw(5),
            EdgeId::from_raw(3)
        ]
    );
    assert_eq!(
        snapshot.opposite_cell(CellId::from_raw(0), EdgeId::from_raw(4)),
        Some(CellId::from_raw(2))
    );
    assert_eq!(
        snapshot.opposite_cell(CellId::from_raw(1), EdgeId::from_raw(4)),
        None
    );
    assert!(
        (snapshot.total_cell_area().get() - 4.0 * std::f64::consts::PI * RADIUS * RADIUS).abs()
            < 1.0e-12
    );

    let json = serde_json::to_value(&snapshot).unwrap();
    assert!(json["cells"].as_array().unwrap().iter().all(|cell| {
        cell.get("neighbors").is_none()
            && cell.get("site").is_some()
            && cell.get("boundary_vertices").is_some()
            && cell.get("boundary_edges").is_some()
    }));
    assert!(json.get("vertices").is_some());
    assert!(json.get("fingerprint").is_some());
}

#[test]
fn construction_sorts_records_into_contiguous_id_order() {
    let canonical_fingerprint = tetrahedral_snapshot().fingerprint();
    let (mut vertices, mut cells, mut edges) = tetrahedral_records();
    vertices.reverse();
    cells.reverse();
    edges.reverse();

    let snapshot = SphericalSurfaceSnapshot::new(
        SPHERICAL_SURFACE_SCHEMA_V1,
        meters(RADIUS),
        vertices,
        cells,
        edges,
    )
    .unwrap();

    assert_eq!(
        snapshot
            .vertices()
            .iter()
            .map(|record| record.id.raw())
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        snapshot
            .cells()
            .iter()
            .map(|record| record.id.raw())
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        snapshot
            .edges()
            .iter()
            .map(|record| record.id.raw())
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5]
    );
    assert_eq!(snapshot.fingerprint(), canonical_fingerprint);
}

#[test]
fn canonical_fingerprint_survives_a_json_round_trip() {
    let snapshot = tetrahedral_snapshot();
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: SphericalSurfaceSnapshot = serde_json::from_slice(&encoded).unwrap();

    decoded.validate().unwrap();
    assert_eq!(decoded.fingerprint(), snapshot.fingerprint());
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn validation_rejects_unsupported_schema_first() {
    let error = mutated_snapshot(|json| json["schema_version"] = Value::from(7)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::UnsupportedSchema {
            found: 7,
            supported: 1
        }
    ));
}

#[test]
fn validation_rejects_non_contiguous_ids() {
    let error = mutated_snapshot(|json| json["vertices"][2]["id"] = Value::from(7)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::NonContiguousVertexId { position: 2, .. }
    ));
}

#[test]
fn validation_rejects_an_edge_with_only_one_distinct_owner() {
    let error = mutated_snapshot(|json| json["edges"][0]["cells"][1] = Value::from(2)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::DuplicateEdgeOwner { edge, owner }
            if edge == EdgeId::from_raw(0) && owner == CellId::from_raw(2)
    ));
}

#[test]
fn validation_rejects_invalid_vertex_references() {
    let error = mutated_snapshot(|json| json["cells"][0]["boundary_vertices"][0] = Value::from(9))
        .unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::InvalidCellVertex { cell, vertex }
            if cell == CellId::from_raw(0) && vertex == SurfaceVertexId::from_raw(9)
    ));
}

#[test]
fn validation_rejects_an_edge_missing_from_one_owner() {
    let error = mutated_snapshot(|json| {
        json["edges"][0]["cells"] = json!([1, 3]);
    })
    .unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::EdgeIncidenceMismatch {
            edge,
            first_owner_count: 0,
            second_owner_count: 1,
            other_count: 1,
        } if edge == EdgeId::from_raw(0)
    ));
}

#[test]
fn validation_rejects_incorrect_cell_area() {
    let error = mutated_snapshot(|json| json["cells"][0]["area"] = Value::from(1.0)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::CellAreaMismatch { cell, .. }
            if cell == CellId::from_raw(0)
    ));
}

#[test]
fn validation_rejects_incorrect_tangent_normal() {
    let error =
        mutated_snapshot(|json| json["edges"][0]["normal_from_first"] = json!([1.0, 0.0, 0.0]))
            .unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::EdgeNormalMismatch { edge, .. }
            if edge == EdgeId::from_raw(0)
    ));
}

#[test]
fn validation_rejects_broken_euler_topology() {
    let error = mutated_snapshot(|json| {
        json["vertices"].as_array_mut().unwrap().push(json!({
            "id": 4,
            "position": [1.0, 0.0, 0.0]
        }));
    })
    .unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::EulerCharacteristicMismatch {
            vertices: 5,
            edges: 6,
            cells: 4
        }
    ));
}

#[test]
fn validation_rejects_an_altered_fingerprint() {
    let error = mutated_snapshot(|json| json["fingerprint"][0] = Value::from(255)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::FingerprintMismatch
    ));
}

fn tetrahedral_snapshot() -> SphericalSurfaceSnapshot {
    let (vertices, cells, edges) = tetrahedral_records();
    SphericalSurfaceSnapshot::new(
        SPHERICAL_SURFACE_SCHEMA_V1,
        meters(RADIUS),
        vertices,
        cells,
        edges,
    )
    .unwrap()
}

fn tetrahedral_records() -> (
    Vec<SphericalSurfaceVertex>,
    Vec<SphericalSurfaceCell>,
    Vec<SphericalSurfaceEdge>,
) {
    let s = 1.0 / 3.0_f64.sqrt();
    let directions = [
        unit(s, s, s),
        unit(s, -s, -s),
        unit(-s, s, -s),
        unit(-s, -s, s),
    ];
    let vertices = directions
        .iter()
        .enumerate()
        .map(|(id, &position)| SphericalSurfaceVertex {
            id: SurfaceVertexId::from_raw(id as u32),
            position,
        })
        .collect::<Vec<_>>();

    let boundary_vertices = [[1, 3, 2], [0, 2, 3], [0, 3, 1], [0, 1, 2]];
    let boundary_edges = [[4, 5, 3], [1, 5, 2], [2, 4, 0], [0, 3, 1]];
    let cells = (0..4)
        .map(|id| {
            let site = negate(directions[id]);
            SphericalSurfaceCell {
                id: CellId::from_raw(id as u32),
                site,
                centroid: site,
                area: square_meters(std::f64::consts::PI * RADIUS * RADIUS),
                boundary_vertices: boundary_vertices[id]
                    .map(SurfaceVertexId::from_raw)
                    .to_vec(),
                boundary_edges: boundary_edges[id].map(EdgeId::from_raw).to_vec(),
            }
        })
        .collect::<Vec<_>>();

    let definitions = [
        (0, 1, 2, 3),
        (0, 2, 1, 3),
        (0, 3, 1, 2),
        (1, 2, 0, 3),
        (1, 3, 0, 2),
        (2, 3, 0, 1),
    ];
    let edges = definitions
        .iter()
        .enumerate()
        .map(
            |(id, &(first_vertex, second_vertex, first_cell, second_cell))| {
                let first_site = cells[first_cell].site;
                let second_site = cells[second_cell].site;
                let midpoint = normalized_sum(directions[first_vertex], directions[second_vertex]);
                SphericalSurfaceEdge {
                    id: EdgeId::from_raw(id as u32),
                    vertices: [
                        SurfaceVertexId::from_raw(first_vertex as u32),
                        SurfaceVertexId::from_raw(second_vertex as u32),
                    ],
                    cells: [
                        CellId::from_raw(first_cell as u32),
                        CellId::from_raw(second_cell as u32),
                    ],
                    midpoint,
                    length: meters(
                        RADIUS * central_angle(directions[first_vertex], directions[second_vertex]),
                    ),
                    center_distance: meters(RADIUS * central_angle(first_site, second_site)),
                    center_distances_to_midpoint: [
                        meters(RADIUS * central_angle(first_site, midpoint)),
                        meters(RADIUS * central_angle(second_site, midpoint)),
                    ],
                    normal_from_first: direction_between(first_site, second_site, midpoint),
                }
            },
        )
        .collect();
    (vertices, cells, edges)
}

fn mutated_snapshot(
    mutate: impl FnOnce(&mut Value),
) -> Result<(), SphericalSurfaceValidationError> {
    let mut json = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    mutate(&mut json);
    let snapshot: SphericalSurfaceSnapshot = serde_json::from_value(json).unwrap();
    snapshot.validate()
}

fn direction_between(
    first: UnitVector3,
    second: UnitVector3,
    midpoint: UnitVector3,
) -> UnitVector3 {
    let first = first.components();
    let second = second.components();
    let midpoint = midpoint.components();
    let delta = [
        second[0] - first[0],
        second[1] - first[1],
        second[2] - first[2],
    ];
    let radial = delta[0] * midpoint[0] + delta[1] * midpoint[1] + delta[2] * midpoint[2];
    unit(
        delta[0] - radial * midpoint[0],
        delta[1] - radial * midpoint[1],
        delta[2] - radial * midpoint[2],
    )
}

fn normalized_sum(first: UnitVector3, second: UnitVector3) -> UnitVector3 {
    let first = first.components();
    let second = second.components();
    unit(
        first[0] + second[0],
        first[1] + second[1],
        first[2] + second[2],
    )
}

fn negate(vector: UnitVector3) -> UnitVector3 {
    let [x, y, z] = vector.components();
    unit(-x, -y, -z)
}

fn unit(x: f64, y: f64, z: f64) -> UnitVector3 {
    UnitVector3::new(x, y, z).unwrap()
}

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn square_meters(value: f64) -> SquareMeters {
    SquareMeters::new(value).unwrap()
}
