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
        assert!(edge
            .center_distances_to_midpoint_m()
            .iter()
            .all(|distance| *distance > 0.0));
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
fn cubed_sphere_small_grid_fingerprints_remain_stable() {
    let cases = [
        (
            2,
            [
                0x53, 0x61, 0xaf, 0x81, 0x08, 0xad, 0xc4, 0xaf, 0x97, 0xbd, 0x0b, 0x55, 0xda, 0x16,
                0xe2, 0xc8, 0x48, 0xd7, 0x7c, 0x7e, 0xeb, 0xc0, 0x09, 0xb6, 0x2d, 0xa9, 0xc5, 0xea,
                0x43, 0x1d, 0x6a, 0x50,
            ],
        ),
        (
            6,
            [
                0xf3, 0x18, 0xa9, 0x03, 0x51, 0xa8, 0xc3, 0x91, 0xa2, 0x21, 0xed, 0xd3, 0x7f, 0x4b,
                0x32, 0x40, 0x34, 0xf1, 0x13, 0x95, 0x7d, 0x23, 0xb0, 0xd1, 0xed, 0xa9, 0xc9, 0xc7,
                0x1d, 0xb8, 0xb3, 0x3e,
            ],
        ),
        (
            12,
            [
                0xf9, 0xe9, 0x43, 0x9c, 0x5f, 0x2e, 0x65, 0x5e, 0x56, 0x5b, 0x37, 0x06, 0x50, 0x0f,
                0xe6, 0xec, 0xee, 0x13, 0x4a, 0xa1, 0xd3, 0x87, 0x87, 0x38, 0x89, 0x37, 0x6e, 0x98,
                0x6b, 0xff, 0x66, 0x21,
            ],
        ),
    ];

    for (resolution, expected) in cases {
        let grid = CubedSphereGrid::new(resolution, 6_371_000.0).unwrap();
        assert_eq!(grid.fingerprint(), &expected, "resolution {resolution}");
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
