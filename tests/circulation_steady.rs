mod support;

use sekai::generators::natural::circulation::{
    build_fixture, BalancedSteadySolver, CirculationEdgePermeability, CirculationFixture,
    CirculationOperators, CirculationSolver, CubedSphereGrid,
};
use sekai::world::natural::{
    CirculationSnapshot, CirculationSolverId, CirculationSpec, PlanetForcing, CLIMATE_MONTH_COUNT,
};
use support::circulation::{area_weighted_rms, magnitude, uniform_fixture};

fn assert_tangent(grid: &CubedSphereGrid, field: &[[[f32; 3]; CLIMATE_MONTH_COUNT]]) {
    for (cell, months) in grid.cells().iter().zip(field) {
        let radial = cell.center_unit();
        for vector in months {
            let radial_component = radial[0] * f64::from(vector[0])
                + radial[1] * f64::from(vector[1])
                + radial[2] * f64::from(vector[2]);
            assert!(
                radial_component.abs() < 1.0e-7,
                "stored tangent vector {vector:?} has radial component {radial_component}"
            );
        }
    }
}

fn assert_atmosphere_mass_balance(
    grid: &CubedSphereGrid,
    snapshot: &CirculationSnapshot,
    spec: &CirculationSpec,
) {
    let total_area = grid.cells().iter().map(|cell| cell.area_m2()).sum::<f64>();
    for month in 0..CLIMATE_MONTH_COUNT {
        let air_temperature = snapshot
            .monthly_air_temperature_c()
            .iter()
            .map(|months| months[month])
            .collect::<Vec<_>>();
        let mean_kelvin = grid
            .cells()
            .iter()
            .zip(&air_temperature)
            .map(|(cell, temperature)| cell.area_m2() * (f64::from(*temperature) + 273.15))
            .sum::<f64>()
            / total_area;
        let equilibrium_height = air_temperature
            .iter()
            .map(|temperature| {
                (f64::from(spec.atmosphere_reference_depth_m)
                    * (f64::from(*temperature) + 273.15 - mean_kelvin)
                    / mean_kelvin) as f32
            })
            .collect::<Vec<_>>();
        let wind = snapshot
            .monthly_wind_m_s()
            .iter()
            .map(|months| months[month])
            .collect::<Vec<_>>();
        let divergence = CirculationOperators::new(grid).divergence(&wind).unwrap();
        let residual = divergence
            .iter()
            .zip(&equilibrium_height)
            .zip(snapshot.monthly_atmosphere_height_anomaly_m())
            .map(|((divergence, equilibrium), height)| {
                f64::from(spec.atmosphere_reference_depth_m) * f64::from(*divergence)
                    - f64::from(spec.layer_relaxation_s_inv)
                        * f64::from(*equilibrium - height[month])
            })
            .map(|value| value as f32)
            .collect::<Vec<_>>();
        let residual_rms = area_weighted_rms(grid, &residual);
        assert!(
            residual_rms < 2.0e-5,
            "month {month} violates the stationary atmosphere mass equation: RMS {residual_rms} m/s"
        );
    }
}

fn assert_ocean_mass_balance(
    grid: &CubedSphereGrid,
    forcing: &PlanetForcing,
    snapshot: &CirculationSnapshot,
    spec: &CirculationSpec,
) {
    const AIR_TO_WATER_DENSITY_RATIO: f64 = 1.2 / 1_025.0;
    let permeability = CirculationEdgePermeability::from_forcing(grid, forcing).unwrap();
    for month in 0..CLIMATE_MONTH_COUNT {
        let mut equilibrium_height = snapshot
            .monthly_atmosphere_height_anomaly_m()
            .iter()
            .zip(forcing.land_fraction())
            .map(|(height, land)| {
                if *land >= 1.0 {
                    0.0
                } else {
                    (-AIR_TO_WATER_DENSITY_RATIO * f64::from(height[month])) as f32
                }
            })
            .collect::<Vec<_>>();
        let (weighted_sum, ocean_area) = grid
            .cells()
            .iter()
            .zip(forcing.land_fraction())
            .zip(&equilibrium_height)
            .fold((0.0_f64, 0.0_f64), |(sum, area), ((cell, land), value)| {
                let ocean = f64::from(1.0 - *land);
                (
                    sum + cell.area_m2() * ocean * f64::from(*value),
                    area + cell.area_m2() * ocean,
                )
            });
        if ocean_area > 0.0 {
            let mean = weighted_sum / ocean_area;
            for (value, land) in equilibrium_height.iter_mut().zip(forcing.land_fraction()) {
                if *land < 1.0 {
                    *value = (f64::from(*value) - mean) as f32;
                }
            }
        }
        let current = snapshot
            .monthly_ocean_current_m_s()
            .iter()
            .map(|months| months[month])
            .collect::<Vec<_>>();
        let divergence = CirculationOperators::new(grid)
            .divergence_with_permeability(&current, permeability.ocean())
            .unwrap();
        let residual = divergence
            .iter()
            .zip(&equilibrium_height)
            .zip(snapshot.monthly_sea_surface_height_anomaly_m())
            .map(|((divergence, equilibrium), height)| {
                f64::from(spec.ocean_reference_depth_m) * f64::from(*divergence)
                    - f64::from(spec.layer_relaxation_s_inv)
                        * f64::from(*equilibrium - height[month])
            })
            .map(|value| value as f32)
            .collect::<Vec<_>>();
        let residual_rms = area_weighted_rms(grid, &residual);
        assert!(
            residual_rms < 2.0e-5,
            "month {month} violates the stationary ocean mass equation: RMS {residual_rms} m/s"
        );
    }
}

#[test]
fn balanced_solver_returns_deterministic_zero_flow_for_uniform_equilibrium() {
    let (grid, forcing, spec) = uniform_fixture(8);
    let first = BalancedSteadySolver.solve(&grid, &forcing, &spec).unwrap();
    let second = BalancedSteadySolver.solve(&grid, &forcing, &spec).unwrap();

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert!(first
        .monthly_wind_m_s()
        .iter()
        .flatten()
        .all(|velocity| magnitude(*velocity) < 1.0e-4));
    assert!(first
        .monthly_ocean_current_m_s()
        .iter()
        .flatten()
        .all(|velocity| magnitude(*velocity) < 1.0e-5));
    assert_eq!(first.solver_id(), CirculationSolverId::BalancedSteadyV1);
    assert_eq!(first.spec_fingerprint(), &spec.fingerprint().unwrap());
    assert_eq!(first.grid_fingerprint(), grid.fingerprint());
    assert_eq!(first.forcing_fingerprint(), forcing.fingerprint());
}

#[test]
fn balanced_solver_produces_finite_tangent_closed_coast_fields_for_all_fixtures() {
    let spec = CirculationSpec {
        face_resolution: 8,
        ..CirculationSpec::default()
    };
    let grid = CubedSphereGrid::new(spec.face_resolution, spec.planet_radius_m).unwrap();
    for fixture in [
        CirculationFixture::AquaPlanet,
        CirculationFixture::TwoBasins,
        CirculationFixture::EarthLikeHarmonics,
    ] {
        let forcing = build_fixture(&grid, fixture).unwrap();
        let snapshot = BalancedSteadySolver.solve(&grid, &forcing, &spec).unwrap();
        snapshot.validate().unwrap();
        assert_tangent(&grid, snapshot.monthly_wind_m_s());
        assert_tangent(&grid, snapshot.monthly_ocean_current_m_s());
        assert_atmosphere_mass_balance(&grid, &snapshot, &spec);
        assert_ocean_mass_balance(&grid, &forcing, &snapshot, &spec);
        assert!(snapshot
            .monthly_wind_m_s()
            .iter()
            .flatten()
            .any(|velocity| magnitude(*velocity) > 1.0e-3));
        assert!(snapshot
            .monthly_precipitation_mm_day()
            .iter()
            .flatten()
            .all(|value| value.is_finite() && *value >= 0.0));
        assert!(snapshot.stats().relative_mass_error < 1.0e-5);
        for (cell, land) in forcing.land_fraction().iter().enumerate() {
            if *land == 1.0 {
                assert!(snapshot.monthly_ocean_current_m_s()[cell]
                    .iter()
                    .all(|velocity| magnitude(*velocity) == 0.0));
            }
        }
    }
}

#[test]
fn balanced_solver_rejects_mismatched_grid_forcing_and_spec_identity() {
    let (grid, forcing, mut spec) = uniform_fixture(6);
    spec.face_resolution = 7;
    assert!(BalancedSteadySolver.solve(&grid, &forcing, &spec).is_err());

    let other_grid = CubedSphereGrid::new(6, spec.planet_radius_m + 1.0).unwrap();
    assert!(BalancedSteadySolver
        .solve(
            &other_grid,
            &forcing,
            &CirculationSpec {
                face_resolution: 6,
                planet_radius_m: spec.planet_radius_m + 1.0,
                ..CirculationSpec::default()
            }
        )
        .is_err());
}
