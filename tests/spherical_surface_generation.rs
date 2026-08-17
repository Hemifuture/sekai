use std::collections::BTreeSet;
use std::time::Instant;

use sekai::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceBuildError};
use sekai::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
use sekai::world::{
    Meters, SphericalSpaceSpec, SphericalSpecError, MAX_GEODESIC_FREQUENCY,
    MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_TARGET_CELL_COUNT, MIN_SPHERICAL_CELL_COUNT,
};
use serde_json::Value;

const RADIUS: f64 = 6_371_000.0;

#[test]
fn closed_surface_science_holds_from_small_refinements_through_the_production_preview() {
    for (target_cell_count, expected_frequency) in [(42, 2_u32), (92, 3), (642, 8), (20_000, 45)] {
        let started = Instant::now();
        let spec = spherical_spec(target_cell_count);
        assert_eq!(spec.resolved_frequency(), expected_frequency);
        let snapshot = GeodesicVoronoiBuilder::build(&spec).unwrap();

        let square = expected_frequency as usize * expected_frequency as usize;
        assert_eq!(snapshot.cells().len(), 10 * square + 2);
        assert_eq!(snapshot.edges().len(), 30 * square);
        assert_eq!(snapshot.vertices().len(), 20 * square);

        let pentagons = snapshot
            .cells()
            .iter()
            .filter(|cell| cell.boundary_vertices.len() == 5)
            .count();
        assert_eq!(pentagons, 12);
        assert!(snapshot.cells().iter().all(|cell| {
            cell.boundary_vertices.len() == 5 || cell.boundary_vertices.len() == 6
        }));

        let mut vertex_incidence = vec![0_usize; snapshot.vertices().len()];
        for cell in snapshot.cells() {
            assert_eq!(cell.boundary_vertices.len(), cell.boundary_edges.len());
            assert!(cell.area.get().is_finite() && cell.area.get() > 0.0);
            for &vertex in &cell.boundary_vertices {
                vertex_incidence[vertex.raw() as usize] += 1;
            }
            for side in 0..cell.boundary_vertices.len() {
                let first = snapshot
                    .vertex(cell.boundary_vertices[side])
                    .unwrap()
                    .position;
                let second = snapshot
                    .vertex(cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()])
                    .unwrap()
                    .position;
                assert!(dot(cross(first, second), cell.site) > 0.0);
            }
        }
        assert!(vertex_incidence.into_iter().all(|count| count == 3));

        for edge in snapshot.edges() {
            assert!(edge.cells[0] < edge.cells[1]);
            assert_ne!(edge.cells[0], edge.cells[1]);
            assert!(edge.vertices[0] < edge.vertices[1]);
            assert!(edge.length.get().is_finite() && edge.length.get() > 0.0);
            assert!(edge.center_distance.get().is_finite() && edge.center_distance.get() > 0.0);
            assert!(edge
                .center_distances_to_midpoint
                .iter()
                .all(|distance| distance.get().is_finite() && distance.get() > 0.0));

            let tangent_residual = edge.normal_from_first.dot(edge.midpoint).abs();
            assert!(tangent_residual <= 2.0e-15, "edge {:?}", edge.id);
            let first_site = snapshot.cell(edge.cells[0]).unwrap().site;
            let second_site = snapshot.cell(edge.cells[1]).unwrap().site;
            let toward_second = tangent_delta(first_site, second_site, edge.midpoint);
            assert!(dot_components(edge.normal_from_first.components(), toward_second) > 0.0);

            let first_vertex = snapshot.vertex(edge.vertices[0]).unwrap().position;
            let second_vertex = snapshot.vertex(edge.vertices[1]).unwrap().position;
            let arc_normal = normalized_components(cross(first_vertex, second_vertex));
            let arc_tangent =
                normalized_components(cross_components(arc_normal, edge.midpoint.components()));
            assert!(
                dot_components(edge.normal_from_first.components(), arc_tangent).abs() <= 2.0e-12,
                "edge {:?}",
                edge.id
            );
            assert!(
                dot_components(edge.normal_from_first.components(), arc_normal).abs()
                    >= 1.0 - 2.0e-12,
                "edge {:?}",
                edge.id
            );

            let site_delta = subtract_components(second_site.components(), first_site.components());
            let site_separation = norm(site_delta);
            for endpoint in [first_vertex, second_vertex] {
                let bisector_residual =
                    dot_components(endpoint.components(), site_delta).abs() / site_separation;
                assert!(bisector_residual <= 2.0e-12, "edge {:?}", edge.id);
            }
        }

        assert_eq!(
            snapshot.vertices().len() as i128 - snapshot.edges().len() as i128
                + snapshot.cells().len() as i128,
            2
        );
        let sphere_area = 4.0 * std::f64::consts::PI * RADIUS * RADIUS;
        let relative_area_error =
            (snapshot.total_cell_area().get() - sphere_area).abs() / sphere_area;
        assert!(relative_area_error <= 1.0e-10, "{relative_area_error:e}");

        let unique_positions = snapshot
            .vertices()
            .iter()
            .map(|vertex| vertex.position.components().map(f64::to_bits))
            .collect::<BTreeSet<_>>();
        assert_eq!(unique_positions.len(), snapshot.vertices().len());
        snapshot.validate().unwrap();
        println!(
            "frequency={expected_frequency} cells={} area_relative_error={relative_area_error:e} elapsed={:?}",
            snapshot.cells().len(),
            started.elapsed()
        );
    }
}

#[test]
fn closed_surface_output_has_only_authoritative_geometry_fields() {
    let snapshot = GeodesicVoronoiBuilder::build(&spherical_spec(642)).unwrap();
    let json = serde_json::to_value(snapshot).unwrap();
    let forbidden = [
        "face",
        "row",
        "column",
        "projection",
        "projection_coordinate",
        "boundary_marker",
        "seam",
        "pole",
        "neighbors",
        "neighbor_vectors",
        "rng",
    ];
    assert_no_forbidden_fields(&json, &forbidden);
}

#[test]
fn canonical_serialization_is_stable_across_frequency_and_radius_budgets() {
    for frequency in [2_u32, 8, 45] {
        for radius in [1.0, RADIUS, 100_000_000.0] {
            assert_canonical_serialization_case(frequency, radius);
        }
    }
}

#[test]
fn supported_cell_budget_endpoints_resolve_without_building_the_maximum_surface() {
    let minimum = spherical_spec_with_radius(MIN_SPHERICAL_CELL_COUNT, RADIUS);
    minimum.validate().unwrap();
    assert_eq!(minimum.resolved_frequency(), 2);
    assert_eq!(minimum.resolved_cell_count(), 42);
    let minimum_snapshot = GeodesicVoronoiBuilder::build(&minimum).unwrap();
    assert_eq!(minimum_snapshot.cells().len(), 42);
    minimum_snapshot.validate().unwrap();

    let maximum = spherical_spec_with_radius(MAX_SPHERICAL_CELL_COUNT, RADIUS);
    maximum.validate().unwrap();
    assert_eq!(maximum.resolved_frequency(), MAX_GEODESIC_FREQUENCY);
    assert_eq!(maximum.resolved_cell_count(), MAX_SPHERICAL_CELL_COUNT);
}

#[test]
fn builder_rejects_cell_counts_immediately_outside_the_request_budget() {
    for target_cell_count in [
        MIN_SPHERICAL_CELL_COUNT - 1,
        MAX_SPHERICAL_TARGET_CELL_COUNT + 1,
    ] {
        let error = GeodesicVoronoiBuilder::build(&spherical_spec(target_cell_count)).unwrap_err();

        assert_eq!(
            error,
            SphericalSurfaceBuildError::InvalidSpec(SphericalSpecError::CellCountOutOfRange {
                found: target_cell_count,
                min: MIN_SPHERICAL_CELL_COUNT,
                max: MAX_SPHERICAL_TARGET_CELL_COUNT,
            })
        );
    }
}

#[test]
fn closed_surface_builder_rejects_invalid_radius() {
    let error = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(0.0).unwrap(),
        target_cell_count: MIN_SPHERICAL_CELL_COUNT,
    })
    .unwrap_err();

    assert!(matches!(error, SphericalSurfaceBuildError::InvalidSpec(_)));
}

#[test]
#[ignore = "production-scale Release measurement"]
fn production_scale_measurement() {
    let spec = spherical_spec(20_000);
    assert_eq!(spec.resolved_frequency(), 45);
    assert_eq!(spec.resolved_cell_count(), 20_252);

    let started = Instant::now();
    let snapshot = GeodesicVoronoiBuilder::build(&spec).unwrap();
    let elapsed = started.elapsed();
    let validation_result = snapshot.validate();
    validation_result.as_ref().unwrap();
    let json = serde_json::to_vec(&snapshot).unwrap();
    let resolved_count = snapshot.cells().len();
    let bytes_per_cell = json.len() as f64 / resolved_count as f64;
    let sphere_area = 4.0 * std::f64::consts::PI * spec.radius.get() * spec.radius.get();
    let relative_area_residual =
        (snapshot.total_cell_area().get() - sphere_area).abs() / sphere_area;

    println!(
        "production_scale_measurement resolved_count={resolved_count} elapsed={elapsed:?} json_bytes={} bytes_per_cell={bytes_per_cell:.6} relative_area_residual={relative_area_residual:e} validation=ok",
        json.len()
    );
}

#[test]
#[ignore = "maximum supported f=141 Release build/validate memory and time measurement"]
fn maximum_supported_surface_measurement() {
    let spec = spherical_spec(MAX_SPHERICAL_CELL_COUNT);
    assert_eq!(spec.resolved_frequency(), MAX_GEODESIC_FREQUENCY);
    assert_eq!(spec.resolved_cell_count(), MAX_SPHERICAL_CELL_COUNT);

    let build_started = Instant::now();
    let snapshot = GeodesicVoronoiBuilder::build(&spec).unwrap();
    let build_elapsed = build_started.elapsed();
    let validation_started = Instant::now();
    snapshot.validate().unwrap();
    let validation_elapsed = validation_started.elapsed();
    let snapshot_heap_bytes = std::mem::size_of_val(snapshot.vertices())
        + std::mem::size_of_val(snapshot.cells())
        + std::mem::size_of_val(snapshot.edges())
        + snapshot
            .cells()
            .iter()
            .map(|cell| {
                cell.boundary_vertices.capacity()
                    * std::mem::size_of::<sekai::world::SurfaceVertexId>()
                    + cell.boundary_edges.capacity() * std::mem::size_of::<sekai::world::EdgeId>()
            })
            .sum::<usize>();

    println!(
        "maximum_supported_surface_measurement frequency={} cells={} build_elapsed={build_elapsed:?} validation_elapsed={validation_elapsed:?} snapshot_heap_bytes={snapshot_heap_bytes} validation=ok",
        spec.resolved_frequency(),
        snapshot.cells().len(),
    );
}

fn spherical_spec(target_cell_count: u32) -> SphericalSpaceSpec {
    spherical_spec_with_radius(target_cell_count, RADIUS)
}

fn spherical_spec_with_radius(target_cell_count: u32, radius: f64) -> SphericalSpaceSpec {
    SphericalSpaceSpec {
        radius: Meters::new(radius).unwrap(),
        target_cell_count,
    }
}

fn assert_canonical_serialization_case(frequency: u32, radius: f64) {
    let target_cell_count = 10 * frequency * frequency + 2;
    let spec = spherical_spec_with_radius(target_cell_count, radius);
    assert_eq!(spec.resolved_frequency(), frequency);

    let first = GeodesicVoronoiBuilder::build(&spec).unwrap();
    let first_fingerprint = first.fingerprint();
    let first_json = serde_json::to_vec(&first).unwrap();
    drop(first);

    let decoded: SphericalSurfaceSnapshot = serde_json::from_slice(&first_json).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.fingerprint(), first_fingerprint);
    let reserialized_json = serde_json::to_vec(&decoded).unwrap();
    drop(decoded);
    assert_eq!(reserialized_json, first_json);
    drop(reserialized_json);

    let second = GeodesicVoronoiBuilder::build(&spec).unwrap();
    assert_eq!(second.fingerprint(), first_fingerprint);
    let second_json = serde_json::to_vec(&second).unwrap();
    drop(second);
    assert_eq!(
        second_json, first_json,
        "frequency={frequency} radius={radius}"
    );
}

fn tangent_delta(first: UnitVector3, second: UnitVector3, midpoint: UnitVector3) -> [f64; 3] {
    let first = first.components();
    let second = second.components();
    let midpoint = midpoint.components();
    let delta = [
        second[0] - first[0],
        second[1] - first[1],
        second[2] - first[2],
    ];
    let radial = dot_components(delta, midpoint);
    [
        delta[0] - radial * midpoint[0],
        delta[1] - radial * midpoint[1],
        delta[2] - radial * midpoint[2],
    ]
}

fn cross(first: UnitVector3, second: UnitVector3) -> [f64; 3] {
    let first = first.components();
    let second = second.components();
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn dot(vector: [f64; 3], direction: UnitVector3) -> f64 {
    dot_components(vector, direction.components())
}

fn dot_components(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn cross_components(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn subtract_components(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[0] - second[0],
        first[1] - second[1],
        first[2] - second[2],
    ]
}

fn norm(vector: [f64; 3]) -> f64 {
    vector[0].hypot(vector[1]).hypot(vector[2])
}

fn normalized_components(vector: [f64; 3]) -> [f64; 3] {
    let length = norm(vector);
    [vector[0] / length, vector[1] / length, vector[2] / length]
}

fn assert_no_forbidden_fields(value: &Value, forbidden: &[&str]) {
    match value {
        Value::Object(object) => {
            for (field, value) in object {
                assert!(
                    !forbidden.contains(&field.as_str()),
                    "forbidden field {field}"
                );
                assert_no_forbidden_fields(value, forbidden);
            }
        }
        Value::Array(values) => {
            for value in values {
                assert_no_forbidden_fields(value, forbidden);
            }
        }
        _ => {}
    }
}
