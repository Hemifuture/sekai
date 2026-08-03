#![allow(dead_code)]

use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::world::natural::{CirculationSpec, PlanetForcing, CLIMATE_MONTH_COUNT};

pub fn magnitude(vector: [f32; 3]) -> f64 {
    let x = f64::from(vector[0]);
    let y = f64::from(vector[1]);
    let z = f64::from(vector[2]);
    (x * x + y * y + z * z).sqrt()
}

pub fn area_weighted_rms(grid: &CubedSphereGrid, values: &[f32]) -> f64 {
    assert_eq!(grid.cell_count(), values.len());
    let mut weighted_squares = 0.0_f64;
    let mut total_area = 0.0_f64;
    for (cell, value) in grid.cells().iter().zip(values) {
        weighted_squares += cell.area_m2() * f64::from(*value).powi(2);
        total_area += cell.area_m2();
    }
    (weighted_squares / total_area).sqrt()
}

pub fn uniform_fixture(face_resolution: u16) -> (CubedSphereGrid, PlanetForcing, CirculationSpec) {
    let spec = CirculationSpec {
        face_resolution,
        ..CirculationSpec::default()
    };
    let grid = CubedSphereGrid::new(face_resolution, spec.planet_radius_m).unwrap();
    let count = grid.cell_count();
    let forcing = PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.3; count],
        vec![1.0; count],
        vec![[15.0; CLIMATE_MONTH_COUNT]; count],
        vec![[15.0; CLIMATE_MONTH_COUNT]; count],
        vec![[0.005; CLIMATE_MONTH_COUNT]; count],
    )
    .unwrap();
    (grid, forcing, spec)
}
