#![allow(dead_code)]

use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::world::natural::{
    CirculationSnapshot, CirculationSolveStats, CirculationSolverId, CirculationSpec,
    PlanetForcing, CIRCULATION_SCHEMA_V1, CLIMATE_MONTH_COUNT,
};

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
        vec![[240.0; CLIMATE_MONTH_COUNT]; count],
        vec![[15.0; CLIMATE_MONTH_COUNT]; count],
        vec![[15.0; CLIMATE_MONTH_COUNT]; count],
        vec![[0.005; CLIMATE_MONTH_COUNT]; count],
    )
    .unwrap();
    (grid, forcing, spec)
}

pub fn artificial_snapshot() -> (CubedSphereGrid, CirculationSnapshot) {
    let (grid, forcing, spec) = uniform_fixture(2);
    let count = grid.cell_count();
    let mut wind = Vec::with_capacity(count);
    let mut current = Vec::with_capacity(count);
    let mut air = Vec::with_capacity(count);
    let mut surface = Vec::with_capacity(count);
    let mut humidity = Vec::with_capacity(count);
    let mut precipitation = Vec::with_capacity(count);
    let mut atmosphere_height = Vec::with_capacity(count);
    let mut sea_height = Vec::with_capacity(count);
    for (index, cell) in grid.cells().iter().enumerate() {
        let radial = cell.center_unit();
        let tangent_length = (radial[0] * radial[0] + radial[1] * radial[1]).sqrt();
        let tangent = [
            (-radial[1] / tangent_length) as f32,
            (radial[0] / tangent_length) as f32,
            0.0,
        ];
        wind.push(std::array::from_fn(|month| {
            let speed = 1.0 + 0.02 * index as f32 + 0.05 * month as f32;
            tangent.map(|component| component * speed)
        }));
        current.push(std::array::from_fn(|month| {
            let speed = 0.1 + 0.002 * index as f32 + 0.005 * month as f32;
            tangent.map(|component| component * speed)
        }));
        air.push(std::array::from_fn(|month| {
            -15.0 + 0.25 * index as f32 + month as f32
        }));
        surface.push(std::array::from_fn(|month| {
            -12.0 + 0.3 * index as f32 + 1.2 * month as f32
        }));
        humidity.push(std::array::from_fn(|month| {
            0.003 + 0.000_01 * index as f32 + 0.000_1 * month as f32
        }));
        precipitation.push(std::array::from_fn(|month| {
            0.5 + 0.03 * index as f32 + 0.1 * month as f32
        }));
        atmosphere_height.push(std::array::from_fn(|month| {
            -50.0 + index as f32 + month as f32
        }));
        sea_height.push(std::array::from_fn(|month| {
            -0.5 + 0.01 * index as f32 + 0.02 * month as f32
        }));
    }
    let snapshot = CirculationSnapshot::new(
        CIRCULATION_SCHEMA_V1,
        spec.fingerprint().unwrap(),
        *grid.fingerprint(),
        *forcing.fingerprint(),
        CirculationSolverId::BalancedSteadyV1,
        CirculationSolveStats {
            iterations_or_steps: 1,
            formation_years: 0,
            final_residual: 0.0,
            relative_mass_error: 0.0,
            dense_state_bytes: 1,
        },
        wind,
        current,
        air,
        surface,
        humidity,
        precipitation,
        atmosphere_height,
        sea_height,
    )
    .unwrap();
    (grid, snapshot)
}

pub fn mismatched_snapshots() -> (CubedSphereGrid, CirculationSnapshot, CirculationSnapshot) {
    let (grid, first) = artificial_snapshot();
    let mut mismatched_forcing = *first.forcing_fingerprint();
    mismatched_forcing[0] ^= 1;
    let second = CirculationSnapshot::new(
        first.schema_version(),
        *first.spec_fingerprint(),
        *first.grid_fingerprint(),
        mismatched_forcing,
        CirculationSolverId::TransientShallowWaterV1,
        *first.stats(),
        first.monthly_wind_m_s().to_vec(),
        first.monthly_ocean_current_m_s().to_vec(),
        first.monthly_air_temperature_c().to_vec(),
        first.monthly_surface_temperature_c().to_vec(),
        first.monthly_specific_humidity().to_vec(),
        first.monthly_precipitation_mm_day().to_vec(),
        first.monthly_atmosphere_height_anomaly_m().to_vec(),
        first.monthly_sea_surface_height_anomaly_m().to_vec(),
    )
    .unwrap();
    (grid, first, second)
}
