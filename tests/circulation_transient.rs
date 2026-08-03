mod support;

use sekai::generators::natural::circulation::{
    build_fixture, BalancedSteadySolver, CirculationFixture, CirculationSolver, CubedSphereGrid,
    TransientShallowWaterSolver,
};
use sekai::world::natural::{CirculationSpec, CLIMATE_MONTH_COUNT};
use support::circulation::{magnitude, uniform_fixture};

fn assert_tangent(grid: &CubedSphereGrid, field: &[[[f32; 3]; CLIMATE_MONTH_COUNT]]) {
    for (cell, months) in grid.cells().iter().zip(field) {
        let radial = cell.center_unit();
        for vector in months {
            let radial_component = radial[0] * f64::from(vector[0])
                + radial[1] * f64::from(vector[1])
                + radial[2] * f64::from(vector[2]);
            assert!(radial_component.abs() < 1.0e-7);
        }
    }
}

#[test]
fn transient_solver_uses_a_valid_quantized_cfl_step_and_preserves_uniform_equilibrium() {
    let (grid, forcing, spec) = uniform_fixture(8);
    let solver = TransientShallowWaterSolver::cold_start();
    let dt = solver.time_step_seconds(&grid, &spec).unwrap();
    assert!(dt >= 60 && dt % 60 == 0);
    assert!(solver.cfl(&grid, &spec, dt).unwrap() <= f64::from(spec.cfl_limit));

    let first = solver.solve(&grid, &forcing, &spec).unwrap();
    let second = solver.solve(&grid, &forcing, &spec).unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert!(first
        .monthly_wind_m_s()
        .iter()
        .flatten()
        .all(|velocity| magnitude(*velocity) < 1.0e-3));
    assert!(first
        .monthly_ocean_current_m_s()
        .iter()
        .flatten()
        .all(|velocity| magnitude(*velocity) < 1.0e-4));
}

#[test]
fn coarse_grid_time_step_respects_the_rk3_coriolis_stability_interval() {
    let (grid, _, spec) = uniform_fixture(1);
    let solver = TransientShallowWaterSolver::cold_start();
    let dt = solver.time_step_seconds(&grid, &spec).unwrap();
    let maximum_coriolis_radians = dt as f64 * 2.0 * spec.rotation_rate_rad_s.abs();
    assert!(maximum_coriolis_radians <= 0.9 * 3.0_f64.sqrt());
}

#[test]
fn transient_cold_and_steady_warm_starts_converge_on_all_fixtures() {
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
        let steady = BalancedSteadySolver.solve(&grid, &forcing, &spec).unwrap();
        let cold = TransientShallowWaterSolver::cold_start()
            .solve(&grid, &forcing, &spec)
            .unwrap();
        let warm = TransientShallowWaterSolver::warm_start(&steady)
            .solve(&grid, &forcing, &spec)
            .unwrap();

        cold.validate().unwrap();
        warm.validate().unwrap();
        assert_tangent(&grid, cold.monthly_wind_m_s());
        assert_tangent(&grid, cold.monthly_ocean_current_m_s());
        assert!(cold.stats().relative_mass_error < 1.0e-5);
        assert!(warm.stats().relative_mass_error < 1.0e-5);
        assert!(cold.stats().iterations_or_steps > 0);
        assert!(warm.stats().iterations_or_steps > 0);
        for (cell, land) in forcing.land_fraction().iter().enumerate() {
            if *land == 1.0 {
                assert!(cold.monthly_ocean_current_m_s()[cell]
                    .iter()
                    .all(|velocity| magnitude(*velocity) == 0.0));
            }
        }
    }
}

#[test]
fn transient_warm_start_rejects_mismatched_snapshot_identity() {
    let (grid, forcing, spec) = uniform_fixture(6);
    let steady = BalancedSteadySolver.solve(&grid, &forcing, &spec).unwrap();
    let other_spec = CirculationSpec {
        face_resolution: 7,
        ..CirculationSpec::default()
    };
    let other_grid =
        CubedSphereGrid::new(other_spec.face_resolution, other_spec.planet_radius_m).unwrap();
    let other_forcing = build_fixture(&other_grid, CirculationFixture::AquaPlanet).unwrap();

    assert!(TransientShallowWaterSolver::warm_start(&steady)
        .solve(&other_grid, &other_forcing, &other_spec)
        .is_err());
}
