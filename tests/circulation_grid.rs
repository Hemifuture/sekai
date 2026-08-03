use sekai::generators::natural::circulation::CubedSphereGrid;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[test]
fn cubed_sphere_is_closed_and_satisfies_euler_counts() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();

    assert_eq!(grid.cell_count(), 24);
    assert_eq!(grid.edges().len(), 48);
    assert_eq!(grid.vertex_count(), 26);
    assert!(grid.edges().iter().all(|edge| edge.cells().len() == 2));
    for cell in grid.cells() {
        assert_eq!(cell.edges().len(), 4);
        assert_eq!(cell.neighbors().len(), 4);
    }
    assert_eq!(
        grid.vertex_count() as isize - grid.edges().len() as isize + grid.cell_count() as isize,
        2
    );
}

#[test]
fn cubed_sphere_cell_areas_close_to_the_analytic_sphere() {
    let radius = 6_371_000.0;
    let grid = CubedSphereGrid::new(12, radius).unwrap();
    let found: f64 = grid.cells().iter().map(|cell| cell.area_m2()).sum();
    let expected = 4.0 * std::f64::consts::PI * radius * radius;

    assert!(
        ((found - expected) / expected).abs() <= 1.0e-10,
        "area relative error was {}",
        (found - expected) / expected
    );
}

#[test]
fn seam_adjacency_is_reciprocal_and_edge_normals_are_tangent() {
    let grid = CubedSphereGrid::new(6, 6_371_000.0).unwrap();

    for cell in grid.cells() {
        for neighbor in cell.neighbors() {
            assert!(grid.cells()[*neighbor as usize]
                .neighbors()
                .contains(&cell.id()));
        }
    }
    for edge in grid.edges() {
        assert!(dot(edge.midpoint_unit(), edge.normal_from_first()).abs() < 1.0e-12);
        let first = grid.cells()[edge.cells()[0] as usize].center_unit();
        let second = grid.cells()[edge.cells()[1] as usize].center_unit();
        let toward_second = [
            second[0] - first[0],
            second[1] - first[1],
            second[2] - first[2],
        ];
        assert!(dot(edge.normal_from_first(), toward_second) > 0.0);
    }
}

#[test]
fn cubed_sphere_build_is_deterministic_and_rejects_unsafe_inputs() {
    let first = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let second = CubedSphereGrid::new(12, 6_371_000.0).unwrap();

    assert_eq!(first.fingerprint(), second.fingerprint());
    assert!(first.minimum_center_distance_m() > 0.0);
    assert!(CubedSphereGrid::new(0, 6_371_000.0).is_err());
    assert!(CubedSphereGrid::new(65, 6_371_000.0).is_err());
    assert!(CubedSphereGrid::new(12, f64::NAN).is_err());
    assert!(CubedSphereGrid::new(12, 0.0).is_err());
}
