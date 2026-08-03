use std::collections::BTreeSet;
use std::time::Instant;

use sekai::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceBuildError};
use sekai::world::spatial::UnitVector3;
use sekai::world::{Meters, SphericalSpaceSpec};
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
fn closed_surface_serialization_is_canonical_and_deterministic() {
    let spec = spherical_spec(642);
    let first = GeodesicVoronoiBuilder::build(&spec).unwrap();
    let second = GeodesicVoronoiBuilder::build(&spec).unwrap();

    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}

#[test]
fn closed_surface_builder_rejects_invalid_spherical_specs() {
    let error = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(0.0).unwrap(),
        target_cell_count: 42,
    })
    .unwrap_err();

    assert!(matches!(error, SphericalSurfaceBuildError::InvalidSpec(_)));
}

fn spherical_spec(target_cell_count: u32) -> SphericalSpaceSpec {
    SphericalSpaceSpec {
        radius: Meters::new(RADIUS).unwrap(),
        target_cell_count,
    }
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
