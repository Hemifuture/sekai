use sekai::world::spatial::{
    central_angle, spherical_triangle_area_unit, SphericalSurfaceCell, SphericalSurfaceEdge,
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SphericalSurfaceVertex, UnitVector3,
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
fn deserialization_rejects_unknown_fields_at_every_surface_record_boundary() {
    let mut with_neighbors = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    with_neighbors["cells"][0]["neighbors"] = json!([1, 2, 3]);
    assert!(serde_json::from_value::<SphericalSurfaceSnapshot>(with_neighbors).is_err());

    let mut with_projection = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    with_projection["vertices"][0]["projection"] = json!([0.25, 0.75]);
    assert!(serde_json::from_value::<SphericalSurfaceSnapshot>(with_projection).is_err());

    let mut with_render_cache = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    with_render_cache["edges"][0]["render_cache"] = json!({"visible": true});
    assert!(serde_json::from_value::<SphericalSurfaceSnapshot>(with_render_cache).is_err());

    let mut with_stage_timing = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    with_stage_timing["stage_timing_ms"] = json!(4.5);
    assert!(serde_json::from_value::<SphericalSurfaceSnapshot>(with_stage_timing).is_err());
}

#[test]
fn deserialization_rejects_scaled_non_unit_surface_vectors() {
    for (records, field) in [
        ("vertices", "position"),
        ("cells", "site"),
        ("cells", "centroid"),
        ("edges", "midpoint"),
        ("edges", "normal_from_first"),
    ] {
        let mut json = serde_json::to_value(tetrahedral_snapshot()).unwrap();
        scale_serialized_vector(&mut json[records][0][field], 2.0);
        assert!(
            serde_json::from_value::<SphericalSurfaceSnapshot>(json).is_err(),
            "scaled {records}.{field} vector was accepted"
        );
    }
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
fn validation_rejects_an_unused_authoritative_vertex_link() {
    let error = mutated_snapshot(|json| {
        json["vertices"].as_array_mut().unwrap().push(json!({
            "id": 4,
            "position": [1.0, 0.0, 0.0]
        }));
    })
    .unwrap_err();
    let unexpected = format!("{error:?}");
    assert!(
        matches!(
            error,
            SphericalSurfaceValidationError::VertexLinkNotSingleCycle { vertex }
                if vertex == SurfaceVertexId::from_raw(4)
        ),
        "unexpected error: {unexpected}"
    );
}

#[test]
fn validation_rejects_an_altered_fingerprint() {
    let error = mutated_snapshot(|json| json["fingerprint"][0] = Value::from(255)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::FingerprintMismatch
    ));
}

#[test]
fn validation_rejects_duplicate_canonical_endpoint_pairs() {
    let error = mutated_snapshot(|json| {
        json["edges"][1]["vertices"] = json["edges"][0]["vertices"].clone();
    })
    .unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::DuplicateCanonicalEdge {
            edge,
            previous_edge,
        } if edge == EdgeId::from_raw(1) && previous_edge == EdgeId::from_raw(0)
    ));
}

#[test]
fn validation_rejects_same_direction_traversal_by_both_edge_owners() {
    let error = mutated_snapshot(|json| {
        json["cells"][0]["boundary_vertices"] = json!([1, 2, 3]);
        json["cells"][0]["boundary_edges"] = json!([3, 5, 4]);
    })
    .unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::EdgeTraversalMismatch { edge }
            if edge == EdgeId::from_raw(3)
    ));
}

#[test]
fn validation_rejects_a_vertex_pinch_with_two_disjoint_link_cycles() {
    let error = mutated_snapshot(|json| append_tetrahedral_component(json, true)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::VertexLinkNotSingleCycle { vertex }
            if vertex == SurfaceVertexId::from_raw(0)
    ));
}

#[test]
fn validation_rejects_disconnected_closed_components() {
    let error = mutated_snapshot(|json| append_tetrahedral_component(json, false)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::DisconnectedCellAdjacency {
            reached: 4,
            total: 8,
        }
    ));
}

#[test]
fn refined_short_edges_use_directional_orientation_not_cross_magnitude() {
    let (vertices, cells, edges) = refined_tetrahedral_records(RADIUS, 1.0e-14);

    SphericalSurfaceSnapshot::new(
        SPHERICAL_SURFACE_SCHEMA_V1,
        meters(RADIUS),
        vertices,
        cells,
        edges,
    )
    .unwrap();
}

#[test]
fn refined_metric_and_area_roundoff_floors_scale_with_radius() {
    for radius in [1.0, 6_371_000.0, 100_000_000.0] {
        let (vertices, mut cells, mut edges) = refined_tetrahedral_records(radius, 1.0e-14);
        edges[0].length = meters(edges[0].length.get() + 4.0 * f64::EPSILON * radius);
        cells[2].area = square_meters(cells[2].area.get() + 4.0 * f64::EPSILON * radius * radius);

        SphericalSurfaceSnapshot::new(
            SPHERICAL_SURFACE_SCHEMA_V1,
            meters(radius),
            vertices,
            cells,
            edges,
        )
        .unwrap();
    }
}

#[test]
fn validation_rejects_non_positive_stored_short_edge_metrics_before_tolerance() {
    let snapshot = close_site_short_edge_snapshot();
    let cases = [
        ("length", None),
        ("center_distance", None),
        ("center_distances_to_midpoint", Some(0_usize)),
        ("center_distances_to_midpoint", Some(1_usize)),
    ];

    for stored_value in [-f64::EPSILON, 0.0] {
        for (field, owner) in cases {
            let mut json = serde_json::to_value(&snapshot).unwrap();
            if let Some(owner) = owner {
                json["edges"][0][field][owner] = Value::from(stored_value);
            } else {
                json["edges"][0][field] = Value::from(stored_value);
            }
            let decoded: SphericalSurfaceSnapshot = serde_json::from_value(json).unwrap();
            let error = decoded.validate().unwrap_err();
            let unexpected = format!("{error:?}");
            match (field, owner) {
                ("length", None) => assert!(
                    matches!(
                        error,
                        SphericalSurfaceValidationError::EdgeLengthMismatch { edge, stored, .. }
                            if edge == EdgeId::from_raw(0) && stored == stored_value
                    ),
                    "unexpected error: {unexpected}"
                ),
                ("center_distance", None) => assert!(
                    matches!(
                        error,
                        SphericalSurfaceValidationError::EdgeCenterDistanceMismatch {
                            edge,
                            stored,
                            ..
                        } if edge == EdgeId::from_raw(0) && stored == stored_value
                    ),
                    "unexpected error: {unexpected}"
                ),
                ("center_distances_to_midpoint", Some(owner)) => assert!(
                    matches!(
                        error,
                        SphericalSurfaceValidationError::EdgeMidpointDistanceMismatch {
                            edge,
                            owner: found_owner,
                            stored,
                            ..
                        } if edge == EdgeId::from_raw(0)
                            && found_owner == owner
                            && stored == stored_value
                    ),
                    "unexpected error: {unexpected}"
                ),
                _ => unreachable!(),
            }
        }
    }
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

fn refined_tetrahedral_records(
    radius: f64,
    epsilon: f64,
) -> (
    Vec<SphericalSurfaceVertex>,
    Vec<SphericalSurfaceCell>,
    Vec<SphericalSurfaceEdge>,
) {
    let directions = [
        unit(1.0, epsilon, epsilon),
        unit(1.0, -epsilon, -epsilon),
        unit(-1.0, 1.0, -1.0),
        unit(-1.0, -1.0, 1.0),
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
            let polygon = boundary_vertices[id].map(|vertex| directions[vertex as usize]);
            let centroid = polygon_centroid(&polygon);
            SphericalSurfaceCell {
                id: CellId::from_raw(id as u32),
                site: centroid,
                centroid,
                area: square_meters(
                    spherical_triangle_area_unit(polygon[0], polygon[1], polygon[2])
                        * radius
                        * radius,
                ),
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
                let midpoint = normalized_sum(
                    directions[first_vertex as usize],
                    directions[second_vertex as usize],
                );
                let first_site = cells[first_cell as usize].site;
                let second_site = cells[second_cell as usize].site;
                SphericalSurfaceEdge {
                    id: EdgeId::from_raw(id as u32),
                    vertices: [
                        SurfaceVertexId::from_raw(first_vertex),
                        SurfaceVertexId::from_raw(second_vertex),
                    ],
                    cells: [CellId::from_raw(first_cell), CellId::from_raw(second_cell)],
                    midpoint,
                    length: meters(
                        radius
                            * central_angle(
                                directions[first_vertex as usize],
                                directions[second_vertex as usize],
                            ),
                    ),
                    center_distance: meters(radius * central_angle(first_site, second_site)),
                    center_distances_to_midpoint: [
                        meters(radius * central_angle(first_site, midpoint)),
                        meters(radius * central_angle(second_site, midpoint)),
                    ],
                    normal_from_first: direction_between(first_site, second_site, midpoint),
                }
            },
        )
        .collect();
    (vertices, cells, edges)
}

fn close_site_short_edge_snapshot() -> SphericalSurfaceSnapshot {
    let radius = 1.0;
    let (vertices, mut cells, mut edges) = refined_tetrahedral_records(radius, 1.0e-16);
    let midpoint = edges[0].midpoint.components();
    let offset = 1.0e-16;
    for (cell, toward_vertex) in [(2_usize, 3_usize), (3_usize, 2_usize)] {
        let toward = vertices[toward_vertex].position.components();
        cells[cell].site = unit(
            midpoint[0] + offset * toward[0],
            midpoint[1] + offset * toward[1],
            midpoint[2] + offset * toward[2],
        );
    }
    for edge in &mut edges {
        let first_site = cells[edge.cells[0].raw() as usize].site;
        let second_site = cells[edge.cells[1].raw() as usize].site;
        edge.center_distance = meters(radius * central_angle(first_site, second_site));
        edge.center_distances_to_midpoint = [
            meters(radius * central_angle(first_site, edge.midpoint)),
            meters(radius * central_angle(second_site, edge.midpoint)),
        ];
        edge.normal_from_first = direction_between(first_site, second_site, edge.midpoint);
    }
    SphericalSurfaceSnapshot::new(
        SPHERICAL_SURFACE_SCHEMA_V1,
        meters(radius),
        vertices,
        cells,
        edges,
    )
    .unwrap()
}

fn polygon_centroid(polygon: &[UnitVector3]) -> UnitVector3 {
    let mut vector_area = [0.0; 3];
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let first_components = first.components();
        let second_components = second.components();
        let edge_cross = cross(first_components, second_components);
        let sine = edge_cross[0].hypot(edge_cross[1]).hypot(edge_cross[2]);
        let weight = central_angle(first, second) / sine;
        for component in 0..3 {
            vector_area[component] += edge_cross[component] * weight;
        }
    }
    unit(vector_area[0], vector_area[1], vector_area[2])
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn mutated_snapshot(
    mutate: impl FnOnce(&mut Value),
) -> Result<(), SphericalSurfaceValidationError> {
    let mut json = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    mutate(&mut json);
    let snapshot: SphericalSurfaceSnapshot = serde_json::from_value(json).unwrap();
    snapshot.validate()
}

fn append_tetrahedral_component(json: &mut Value, identify_first_vertex: bool) {
    let source_vertices = json["vertices"].as_array().unwrap().clone();
    let source_cells = json["cells"].as_array().unwrap().clone();
    let source_edges = json["edges"].as_array().unwrap().clone();
    let vertex_map = if identify_first_vertex {
        [0_u64, 4, 5, 6]
    } else {
        [4_u64, 5, 6, 7]
    };

    for (source_id, mut vertex) in source_vertices.into_iter().enumerate() {
        if identify_first_vertex && source_id == 0 {
            continue;
        }
        vertex["id"] = Value::from(vertex_map[source_id]);
        json["vertices"].as_array_mut().unwrap().push(vertex);
    }
    for mut cell in source_cells {
        cell["id"] = Value::from(cell["id"].as_u64().unwrap() + 4);
        for vertex in cell["boundary_vertices"].as_array_mut().unwrap() {
            *vertex = Value::from(vertex_map[vertex.as_u64().unwrap() as usize]);
        }
        for edge in cell["boundary_edges"].as_array_mut().unwrap() {
            *edge = Value::from(edge.as_u64().unwrap() + 6);
        }
        json["cells"].as_array_mut().unwrap().push(cell);
    }
    for mut edge in source_edges {
        edge["id"] = Value::from(edge["id"].as_u64().unwrap() + 6);
        for vertex in edge["vertices"].as_array_mut().unwrap() {
            *vertex = Value::from(vertex_map[vertex.as_u64().unwrap() as usize]);
        }
        for cell in edge["cells"].as_array_mut().unwrap() {
            *cell = Value::from(cell.as_u64().unwrap() + 4);
        }
        json["edges"].as_array_mut().unwrap().push(edge);
    }
}

fn scale_serialized_vector(vector: &mut Value, factor: f64) {
    for component in vector.as_array_mut().unwrap() {
        *component = Value::from(component.as_f64().unwrap() * factor);
    }
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
