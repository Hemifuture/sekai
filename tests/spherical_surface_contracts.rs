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
    let (vertices, cells, edges) = refined_tetrahedral_records(RADIUS, 1.0e-5);

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
fn validation_rejects_an_owner_derived_normal_oblique_to_the_endpoint_arc() {
    let (mut vertices, mut cells, mut edges) = tetrahedral_records();
    let moved = vertices[0].position.components();
    vertices[0].position = unit(moved[0], moved[1] + 0.05, moved[2]);
    recompute_tetrahedral_metrics(&vertices, &mut cells, &mut edges);

    let edge = &edges[0];
    let first = vertices[edge.vertices[0].raw() as usize]
        .position
        .components();
    let second = vertices[edge.vertices[1].raw() as usize]
        .position
        .components();
    let arc_normal = normalized_cross(first, second);
    let arc_tangent = normalized_cross(arc_normal.components(), edge.midpoint.components());
    assert!(edge.normal_from_first.dot(edge.midpoint).abs() <= 2.0e-15);
    assert!(edge.normal_from_first.dot(arc_tangent).abs() >= 1.0e-3);

    let error = SphericalSurfaceSnapshot::new(
        SPHERICAL_SURFACE_SCHEMA_V1,
        meters(RADIUS),
        vertices,
        cells,
        edges,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::EdgeNormalMismatch { edge }
            if edge == EdgeId::from_raw(0)
    ));
}

#[test]
fn refined_metric_and_area_roundoff_floors_scale_with_radius() {
    for radius in [1.0, 6_371_000.0, 100_000_000.0] {
        let (vertices, mut cells, mut edges) = refined_tetrahedral_records(radius, 1.0e-5);
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
    let sites = [
        unit(1.0, epsilon, epsilon),
        unit(1.0, -epsilon, -epsilon),
        unit(-1.0, 1.0, -1.0),
        unit(-1.0, -1.0, 1.0),
    ];
    let directions = [
        spherical_circumcenter(sites[1], sites[3], sites[2]),
        spherical_circumcenter(sites[0], sites[2], sites[3]),
        spherical_circumcenter(sites[0], sites[3], sites[1]),
        spherical_circumcenter(sites[0], sites[1], sites[2]),
    ];
    let vertices = directions
        .iter()
        .enumerate()
        .map(|(id, &position)| SphericalSurfaceVertex {
            id: SurfaceVertexId::from_raw(id as u32),
            position,
        })
        .collect::<Vec<_>>();
    let boundary_vertices = [[1, 2, 3], [0, 3, 2], [0, 1, 3], [0, 2, 1]];
    let boundary_edges = [[3, 5, 4], [2, 5, 1], [0, 4, 2], [1, 3, 0]];
    let cells = (0..4)
        .map(|id| {
            let polygon = boundary_vertices[id].map(|vertex| directions[vertex as usize]);
            let site = sites[id];
            let (unit_area, centroid) = robust_fan_metrics(site, &polygon);
            SphericalSurfaceCell {
                id: CellId::from_raw(id as u32),
                site,
                centroid,
                area: square_meters(unit_area * radius * radius),
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
                    normal_from_first: endpoint_normal_from_first(
                        directions[first_vertex as usize],
                        directions[second_vertex as usize],
                        first_site,
                        second_site,
                    ),
                }
            },
        )
        .collect();
    (vertices, cells, edges)
}

fn close_site_short_edge_snapshot() -> SphericalSurfaceSnapshot {
    let radius = 1.0;
    let (vertices, cells, edges) = refined_tetrahedral_records(radius, 1.0e-5);
    SphericalSurfaceSnapshot::new(
        SPHERICAL_SURFACE_SCHEMA_V1,
        meters(radius),
        vertices,
        cells,
        edges,
    )
    .unwrap()
}

fn recompute_tetrahedral_metrics(
    vertices: &[SphericalSurfaceVertex],
    cells: &mut [SphericalSurfaceCell],
    edges: &mut [SphericalSurfaceEdge],
) {
    for cell in cells.iter_mut() {
        let polygon = cell
            .boundary_vertices
            .iter()
            .map(|id| vertices[id.raw() as usize].position)
            .collect::<Vec<_>>();
        let (unit_area, centroid) = robust_fan_metrics(cell.site, &polygon);
        cell.area = square_meters(unit_area * RADIUS * RADIUS);
        cell.centroid = centroid;
    }
    for edge in edges {
        let first_vertex = vertices[edge.vertices[0].raw() as usize].position;
        let second_vertex = vertices[edge.vertices[1].raw() as usize].position;
        edge.midpoint = normalized_sum(first_vertex, second_vertex);
        edge.length = meters(RADIUS * central_angle(first_vertex, second_vertex));
        let first_site = cells[edge.cells[0].raw() as usize].site;
        let second_site = cells[edge.cells[1].raw() as usize].site;
        edge.center_distance = meters(RADIUS * central_angle(first_site, second_site));
        edge.center_distances_to_midpoint = [
            meters(RADIUS * central_angle(first_site, edge.midpoint)),
            meters(RADIUS * central_angle(second_site, edge.midpoint)),
        ];
        edge.normal_from_first = direction_between(first_site, second_site, edge.midpoint);
    }
}

fn normalized_cross(first: [f64; 3], second: [f64; 3]) -> UnitVector3 {
    let cross = cross(first, second);
    unit(cross[0], cross[1], cross[2])
}

fn spherical_circumcenter(a: UnitVector3, b: UnitVector3, c: UnitVector3) -> UnitVector3 {
    let a = a.components();
    let b = b.components();
    let c = c.components();
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let mut normal = cross(ab, ac);
    let sum = [a[0] + b[0] + c[0], a[1] + b[1] + c[1], a[2] + b[2] + c[2]];
    if dot_components(normal, sum) < 0.0 {
        normal = [-normal[0], -normal[1], -normal[2]];
    }
    unit(normal[0], normal[1], normal[2])
}

fn endpoint_normal_from_first(
    first_endpoint: UnitVector3,
    second_endpoint: UnitVector3,
    first_owner: UnitVector3,
    second_owner: UnitVector3,
) -> UnitVector3 {
    let first = first_endpoint.components();
    let second = second_endpoint.components();
    let endpoint_delta = [
        second[0] - first[0],
        second[1] - first[1],
        second[2] - first[2],
    ];
    let mut normal = normalized_cross(first, endpoint_delta);
    let first_owner = first_owner.components();
    let second_owner = second_owner.components();
    let owner_delta = [
        second_owner[0] - first_owner[0],
        second_owner[1] - first_owner[1],
        second_owner[2] - first_owner[2],
    ];
    if dot_components(normal.components(), owner_delta) < 0.0 {
        let components = normal.components();
        normal = unit(-components[0], -components[1], -components[2]);
    }
    normal
}

fn robust_fan_metrics(site: UnitVector3, polygon: &[UnitVector3]) -> (f64, UnitVector3) {
    let mut area = 0.0;
    let mut weighted_centroid = [0.0; 3];
    for side in 0..polygon.len() {
        let first = polygon[side];
        let second = polygon[(side + 1) % polygon.len()];
        let triangle_area = robust_triangle_area(site, first, second);
        let site = site.components();
        let first = first.components();
        let second = second.components();
        let triangle_centroid = unit(
            site[0] + first[0] + second[0],
            site[1] + first[1] + second[1],
            site[2] + first[2] + second[2],
        );
        area += triangle_area;
        for (sum, component) in weighted_centroid
            .iter_mut()
            .zip(triangle_centroid.components())
        {
            *sum += triangle_area * component;
        }
    }
    (
        area,
        unit(
            weighted_centroid[0],
            weighted_centroid[1],
            weighted_centroid[2],
        ),
    )
}

fn robust_triangle_area(a: UnitVector3, b: UnitVector3, c: UnitVector3) -> f64 {
    let a = a.components();
    let b = b.components();
    let c = c.components();
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let numerator = (a[0] * (ab[1] * ac[2] - ab[2] * ac[1])
        + a[1] * (ab[2] * ac[0] - ab[0] * ac[2])
        + a[2] * (ab[0] * ac[1] - ab[1] * ac[0]))
        .abs();
    let denominator = 1.0 + dot_components(a, b) + dot_components(b, c) + dot_components(c, a);
    2.0 * numerator.atan2(denominator)
}

fn dot_components(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
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
