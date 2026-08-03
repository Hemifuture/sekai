use sekai::world::spatial::{
    central_angle, SphericalSurfaceCell, SphericalSurfaceEdge, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SphericalSurfaceVertex, UnitVector3,
    SPHERICAL_SURFACE_SCHEMA_V1,
};
use sekai::world::{CellId, EdgeId, Meters, SquareMeters, SurfaceVertexId};
use serde_json::{json, Value};

const RADIUS: f64 = 2.0;
const VALIDATION_RELATIVE_TOLERANCE: f64 = 1.0e-10;
const VALIDATION_ABSOLUTE_SCALE_ULPS: f64 = 16.0;

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
fn construction_canonicalizes_equivalent_cyclic_boundary_rotations() {
    let canonical = tetrahedral_snapshot();
    let canonical_bytes = serde_json::to_vec(&canonical).unwrap();
    let (vertices, mut cells, edges) = tetrahedral_records();
    for cell in &mut cells {
        let rotation = cell.id.raw() as usize % cell.boundary_vertices.len();
        cell.boundary_vertices.rotate_left(rotation);
        cell.boundary_edges.rotate_left(rotation);
    }

    let rotated = SphericalSurfaceSnapshot::new(
        SPHERICAL_SURFACE_SCHEMA_V1,
        meters(RADIUS),
        vertices,
        cells,
        edges,
    )
    .unwrap();

    assert_eq!(rotated.fingerprint(), canonical.fingerprint());
    assert_eq!(serde_json::to_vec(&rotated).unwrap(), canonical_bytes);
}

#[test]
fn construction_reports_mismatched_boundary_lengths_before_canonical_rotation() {
    let (vertices, mut cells, edges) = tetrahedral_records();
    cells[0].boundary_vertices = [3, 2, 1].map(SurfaceVertexId::from_raw).to_vec();
    cells[0].boundary_edges.clear();

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
        SphericalSurfaceValidationError::CellBoundaryLengthMismatch { cell, .. }
            if cell == CellId::from_raw(0)
    ));
}

#[test]
fn validation_rejects_a_noncanonical_cyclic_boundary_start() {
    let mut wire = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    wire["cells"][0]["boundary_vertices"] = json!([3, 2, 1]);
    wire["cells"][0]["boundary_edges"] = json!([5, 3, 4]);
    let malformed: SphericalSurfaceSnapshot = serde_json::from_value(wire.clone()).unwrap();
    // Recompute the malformed wire's semantic hash independently in the test,
    // proving the canonical-start check does not merely rely on a stale hash.
    wire["fingerprint"] =
        serde_json::to_value(independent_surface_fingerprint(&malformed)).unwrap();
    let snapshot: SphericalSurfaceSnapshot = serde_json::from_value(wire).unwrap();

    let error = snapshot.validate().unwrap_err();
    assert!(
        error.to_string().contains("canonical boundary start"),
        "unexpected error: {error:?}"
    );
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
fn deserialization_rejects_cell_boundary_vectors_over_the_v1_degree_bound() {
    let mut wire = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    wire["cells"][0]["boundary_vertices"] = json!([0, 1, 2, 3, 0, 1, 2]);
    wire["cells"][0]["boundary_edges"] = json!([0, 1, 2, 3, 4, 5, 0]);

    let error = serde_json::from_value::<SphericalSurfaceSnapshot>(wire).unwrap_err();
    assert!(
        error.to_string().contains("at most 6 elements"),
        "unexpected serde error: {error}"
    );
}

#[test]
fn construction_rejects_cell_boundaries_that_cannot_round_trip_through_v1() {
    let (vertices, mut cells, edges) = tetrahedral_records();
    cells[0].boundary_vertices = [0, 1, 2, 3, 0, 1, 2]
        .map(SurfaceVertexId::from_raw)
        .to_vec();
    cells[0].boundary_edges = [0, 1, 2, 3, 4, 5, 0].map(EdgeId::from_raw).to_vec();

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
        SphericalSurfaceValidationError::CellBoundaryDegreeOutOfRange {
            cell,
            found: 7,
            max: 6,
        } if cell == CellId::from_raw(0)
    ));
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
fn fallible_total_cell_area_reports_unvalidated_sum_overflow() {
    let mut wire = serde_json::to_value(tetrahedral_snapshot()).unwrap();
    for cell in wire["cells"].as_array_mut().unwrap() {
        cell["area"] = json!(1.0e308);
    }
    let snapshot: SphericalSurfaceSnapshot = serde_json::from_value(wire).unwrap();

    let error = snapshot.try_total_cell_area().unwrap_err();
    assert!(error.to_string().contains("finite"));
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
fn validation_rejects_euler_invalid_unused_authoritative_vertex_before_topology() {
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
            cells: 4,
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
fn validation_rejects_euler_invalid_vertex_pinch_before_topology() {
    let error = mutated_snapshot(|json| append_tetrahedral_component(json, true)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::EulerCharacteristicMismatch {
            vertices: 7,
            edges: 12,
            cells: 8,
        }
    ));
}

#[test]
fn validation_still_rejects_an_euler_valid_vertex_link_pinch() {
    let snapshot = pinched_octahedra_snapshot();
    let error = snapshot.validate().unwrap_err();

    assert!(matches!(
        error,
        SphericalSurfaceValidationError::VertexLinkNotSingleCycle { vertex }
            if vertex == SurfaceVertexId::from_raw(0)
    ));
}

#[test]
fn validation_rejects_euler_invalid_disconnected_components_before_topology() {
    let error = mutated_snapshot(|json| append_tetrahedral_component(json, false)).unwrap_err();
    assert!(matches!(
        error,
        SphericalSurfaceValidationError::EulerCharacteristicMismatch {
            vertices: 8,
            edges: 12,
            cells: 8,
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
fn refined_metric_and_area_roundoff_stays_within_combined_tolerance() {
    for radius in [1.0, 6_371_000.0, 100_000_000.0] {
        let (vertices, mut cells, mut edges) = refined_tetrahedral_records(radius, 1.0e-5);
        let metric_perturbation = 4.0 * f64::EPSILON * radius;
        let metric_absolute_floor = VALIDATION_ABSOLUTE_SCALE_ULPS * f64::EPSILON * radius;
        let perturbed_length = edges[0].length.get() + metric_perturbation;
        let metric_relative_allowance =
            VALIDATION_RELATIVE_TOLERANCE * perturbed_length.abs().max(edges[0].length.get().abs());
        assert!(metric_perturbation < metric_absolute_floor);
        assert!(metric_perturbation < metric_absolute_floor + metric_relative_allowance);

        let radius_squared = radius * radius;
        let area_perturbation = 4.0 * f64::EPSILON * radius_squared;
        let area_absolute_floor = VALIDATION_ABSOLUTE_SCALE_ULPS * f64::EPSILON * radius_squared;
        let perturbed_area = cells[2].area.get() + area_perturbation;
        let area_relative_allowance =
            VALIDATION_RELATIVE_TOLERANCE * perturbed_area.abs().max(cells[2].area.get().abs());
        assert!(area_perturbation < area_absolute_floor);
        assert!(area_perturbation < area_absolute_floor + area_relative_allowance);

        edges[0].length = meters(perturbed_length);
        cells[2].area = square_meters(perturbed_area);

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
fn validation_rejects_non_positive_stored_edge_metrics() {
    let snapshot = refined_tetrahedral_snapshot();
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

fn refined_tetrahedral_snapshot() -> SphericalSurfaceSnapshot {
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

fn independent_surface_fingerprint(snapshot: &SphericalSurfaceSnapshot) -> [u8; 32] {
    fn hash_u16(hasher: &mut blake3::Hasher, value: u16) {
        hasher.update(&value.to_le_bytes());
    }
    fn hash_u32(hasher: &mut blake3::Hasher, value: u32) {
        hasher.update(&value.to_le_bytes());
    }
    fn hash_len(hasher: &mut blake3::Hasher, value: usize) {
        hasher.update(&(value as u64).to_le_bytes());
    }
    fn hash_f64(hasher: &mut blake3::Hasher, value: f64) {
        hasher.update(&value.to_bits().to_le_bytes());
    }
    fn hash_vector(hasher: &mut blake3::Hasher, vector: UnitVector3) {
        for component in vector.components() {
            hash_f64(hasher, component);
        }
    }

    let mut hasher = blake3::Hasher::new();
    hash_u16(&mut hasher, snapshot.schema_version());
    hash_f64(&mut hasher, snapshot.radius().get());

    hash_len(&mut hasher, snapshot.vertices().len());
    for vertex in snapshot.vertices() {
        hash_u32(&mut hasher, vertex.id.raw());
        hash_vector(&mut hasher, vertex.position);
    }

    hash_len(&mut hasher, snapshot.cells().len());
    for cell in snapshot.cells() {
        hash_u32(&mut hasher, cell.id.raw());
        hash_vector(&mut hasher, cell.site);
        hash_vector(&mut hasher, cell.centroid);
        hash_f64(&mut hasher, cell.area.get());
        hash_len(&mut hasher, cell.boundary_vertices.len());
        for vertex in &cell.boundary_vertices {
            hash_u32(&mut hasher, vertex.raw());
        }
        hash_len(&mut hasher, cell.boundary_edges.len());
        for edge in &cell.boundary_edges {
            hash_u32(&mut hasher, edge.raw());
        }
    }

    hash_len(&mut hasher, snapshot.edges().len());
    for edge in snapshot.edges() {
        hash_u32(&mut hasher, edge.id.raw());
        for vertex in edge.vertices {
            hash_u32(&mut hasher, vertex.raw());
        }
        for cell in edge.cells {
            hash_u32(&mut hasher, cell.raw());
        }
        hash_vector(&mut hasher, edge.midpoint);
        hash_f64(&mut hasher, edge.length.get());
        hash_f64(&mut hasher, edge.center_distance.get());
        for distance in edge.center_distances_to_midpoint {
            hash_f64(&mut hasher, distance.get());
        }
        hash_vector(&mut hasher, edge.normal_from_first);
    }

    *hasher.finalize().as_bytes()
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

fn pinched_octahedra_snapshot() -> SphericalSurfaceSnapshot {
    const FACES: [[u32; 3]; 8] = [
        [0, 2, 3],
        [0, 3, 4],
        [0, 4, 5],
        [0, 5, 2],
        [1, 3, 2],
        [1, 4, 3],
        [1, 5, 4],
        [1, 2, 5],
    ];
    const EDGE_PAIRS: [[u32; 2]; 12] = [
        [0, 2],
        [0, 3],
        [0, 4],
        [0, 5],
        [1, 2],
        [1, 3],
        [1, 4],
        [1, 5],
        [2, 3],
        [2, 5],
        [3, 4],
        [4, 5],
    ];

    let vertices = (0..10_u32)
        .map(|id| json!({ "id": id, "position": [1.0, 0.0, 0.0] }))
        .collect::<Vec<_>>();
    let mut cells = Vec::new();
    let mut edges = Vec::new();

    for (mapping, cell_offset, edge_offset) in [
        ([0_u32, 1, 2, 3, 4, 5], 0_u32, 0_u32),
        ([0_u32, 1, 6, 7, 8, 9], 8_u32, 12_u32),
    ] {
        let global_pairs = EDGE_PAIRS.map(|[first, second]| {
            let mut pair = [mapping[first as usize], mapping[second as usize]];
            pair.sort_unstable();
            pair
        });

        for (local_cell, face) in FACES.into_iter().enumerate() {
            let mut boundary_vertices = face.map(|vertex| mapping[vertex as usize]);
            let start = boundary_vertices
                .iter()
                .enumerate()
                .min_by_key(|(_, vertex)| **vertex)
                .unwrap()
                .0;
            boundary_vertices.rotate_left(start);
            let boundary_edges: [u32; 3] = std::array::from_fn(|side| {
                let mut pair = [
                    boundary_vertices[side],
                    boundary_vertices[(side + 1) % boundary_vertices.len()],
                ];
                pair.sort_unstable();
                edge_offset
                    + global_pairs
                        .iter()
                        .position(|candidate| *candidate == pair)
                        .unwrap() as u32
            });
            cells.push(json!({
                "id": cell_offset + local_cell as u32,
                "site": [1.0, 0.0, 0.0],
                "centroid": [1.0, 0.0, 0.0],
                "area": 1.0,
                "boundary_vertices": boundary_vertices,
                "boundary_edges": boundary_edges,
            }));
        }

        for (local_edge, vertices) in global_pairs.into_iter().enumerate() {
            let owners = FACES
                .iter()
                .enumerate()
                .filter_map(|(local_cell, face)| {
                    let mapped = face.map(|vertex| mapping[vertex as usize]);
                    (0..mapped.len())
                        .any(|side| {
                            let mut pair = [mapped[side], mapped[(side + 1) % mapped.len()]];
                            pair.sort_unstable();
                            pair == vertices
                        })
                        .then_some(cell_offset + local_cell as u32)
                })
                .collect::<Vec<_>>();
            edges.push(json!({
                "id": edge_offset + local_edge as u32,
                "vertices": vertices,
                "cells": owners,
                "midpoint": [1.0, 0.0, 0.0],
                "length": 1.0,
                "center_distance": 1.0,
                "center_distances_to_midpoint": [1.0, 1.0],
                "normal_from_first": [1.0, 0.0, 0.0],
            }));
        }
    }

    serde_json::from_value(json!({
        "schema_version": SPHERICAL_SURFACE_SCHEMA_V1,
        "radius": 1.0,
        "vertices": vertices,
        "cells": cells,
        "edges": edges,
        "fingerprint": vec![0_u8; 32],
    }))
    .unwrap()
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
