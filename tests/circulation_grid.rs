use sekai::generators::natural::circulation::CubedSphereGrid;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
fn update_raw_f64(hasher: &mut blake3::Hasher, value: f64) {
    hasher.update(&value.to_bits().to_le_bytes());
}

#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
fn raw_public_geometry_digest(grid: &CubedSphereGrid) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.cubed-sphere-grid.raw-public-geometry.v1\0");
    hasher.update(&grid.face_resolution().to_le_bytes());
    update_raw_f64(&mut hasher, grid.radius_m());
    hasher.update(&(grid.vertex_count() as u64).to_le_bytes());
    hasher.update(&(grid.cell_count() as u64).to_le_bytes());
    hasher.update(&(grid.edges().len() as u64).to_le_bytes());
    update_raw_f64(&mut hasher, grid.minimum_center_distance_m());

    for cell in grid.cells() {
        hasher.update(&cell.id().to_le_bytes());
        hasher.update(&[cell.face()]);
        hasher.update(&cell.row().to_le_bytes());
        hasher.update(&cell.column().to_le_bytes());
        for component in cell.center_unit() {
            update_raw_f64(&mut hasher, component);
        }
        update_raw_f64(&mut hasher, cell.area_m2());
        for edge in cell.edges() {
            hasher.update(&edge.to_le_bytes());
        }
        for neighbor in cell.neighbors() {
            hasher.update(&neighbor.to_le_bytes());
        }
    }

    for edge in grid.edges() {
        hasher.update(&edge.id().to_le_bytes());
        for vertex in edge.vertices() {
            hasher.update(&vertex.to_le_bytes());
        }
        for cell in edge.cells() {
            hasher.update(&cell.to_le_bytes());
        }
        for component in edge.midpoint_unit() {
            update_raw_f64(&mut hasher, component);
        }
        for component in edge.normal_from_first() {
            update_raw_f64(&mut hasher, component);
        }
        update_raw_f64(&mut hasher, edge.length_m());
        update_raw_f64(&mut hasher, edge.center_distance_m());
        for distance in edge.center_distances_to_midpoint_m() {
            update_raw_f64(&mut hasher, *distance);
        }
    }

    *hasher.finalize().as_bytes()
}

#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
fn running_reference_rustc() -> bool {
    const REFERENCE: &str = "rustc 1.97.1 (8bab26f4f 2026-07-14)";
    std::process::Command::new("rustc")
        .arg("-V")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|version| version.trim() == REFERENCE)
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
fn production_resolutions_use_exact_topological_seam_welding() {
    for resolution in [24_u16, 32, 48, 64] {
        let grid = CubedSphereGrid::new(resolution, 6_371_000.0).unwrap();
        let cells = 6 * usize::from(resolution) * usize::from(resolution);
        assert_eq!(grid.cell_count(), cells);
        assert_eq!(grid.edges().len(), 2 * cells);
        assert_eq!(grid.vertex_count(), cells + 2);
        assert_eq!(
            grid.vertex_count() as isize - grid.edges().len() as isize + grid.cell_count() as isize,
            2
        );
    }
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
#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
fn cubed_sphere_public_float_bits_match_the_task6_baseline() {
    if !running_reference_rustc() {
        eprintln!("raw-bit oracle is scoped to rustc 1.97.1 (8bab26f4f 2026-07-14), LLVM 22.1.6");
        return;
    }
    // Independently recorded from detached commit
    // 79a13500206b3de1f81c57394e497dffec2d4fff before the Task 6 refactor.
    let cases = [
        (
            2,
            [
                0x46, 0x4e, 0x02, 0x82, 0x1b, 0xf8, 0xa5, 0xee, 0xce, 0xd7, 0x3d, 0x78, 0x3e, 0xce,
                0x68, 0x99, 0x7c, 0xc2, 0x31, 0x7e, 0x46, 0x2d, 0xad, 0x12, 0xfb, 0xb2, 0x98, 0xe9,
                0xb2, 0x90, 0xea, 0x55,
            ],
        ),
        (
            6,
            [
                0x67, 0x89, 0x52, 0x0b, 0x2b, 0x40, 0x1e, 0xa3, 0xcc, 0xf5, 0xd2, 0x44, 0x3a, 0x4c,
                0x01, 0x1a, 0x47, 0xbb, 0x36, 0x60, 0xc1, 0x63, 0xba, 0x1f, 0xd9, 0xb9, 0xb9, 0x83,
                0xc3, 0xe0, 0x4e, 0xea,
            ],
        ),
        (
            12,
            [
                0x96, 0x98, 0x03, 0xeb, 0xc7, 0x74, 0xb7, 0xd6, 0x74, 0x1d, 0x31, 0x82, 0xcb, 0xb4,
                0x79, 0x42, 0xb1, 0xea, 0x87, 0x35, 0x33, 0x05, 0x5d, 0x4f, 0x95, 0x16, 0x96, 0x74,
                0xec, 0x95, 0xa8, 0x28,
            ],
        ),
    ];

    for (resolution, expected) in cases {
        let grid = CubedSphereGrid::new(resolution, 6_371_000.0).unwrap();
        assert_eq!(
            raw_public_geometry_digest(&grid),
            expected,
            "resolution {resolution}"
        );
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
