use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::{
    CirculationOperators, CubedSphereGrid, SecondOrderTransportWorkspace,
};
use sekai::generators::natural::{
    climate_state_formation_residual, climate_state_rms_difference,
    run_closed_split_annual_mass_fixture, ClimateIntegratorError, ExplicitRk3Integrator,
    ImexCrankNicolsonIntegrator, LayeredClimateState, LayeredTendencySystem,
    SplitExplicitRk3Integrator,
};
use sekai::world::natural::{
    ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
};

fn uniform_forcing(grid: &CubedSphereGrid, temperature_c: f32) -> PlanetForcing {
    let count = grid.cell_count();
    PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![1.0; count],
        vec![[temperature_c; 12]; count],
        vec![[temperature_c; 12]; count],
        vec![[0.008; 12]; count],
    )
    .unwrap()
}

fn state(
    grid: &CubedSphereGrid,
    profile: ClimateModelProfile,
    forcing: &PlanetForcing,
) -> LayeredClimateState {
    LayeredClimateState::from_forcing(grid, &ClimateLayerLayout::for_profile(profile), forcing, 0)
        .unwrap()
}

fn perturbed_state(grid: &CubedSphereGrid) -> (PlanetForcing, LayeredClimateState) {
    let forcing = uniform_forcing(grid, 15.0);
    let mut state = state(grid, ClimateModelProfile::C1SingleLayerV1, &forcing);
    for (cell, value) in grid.cells().iter().zip(
        state
            .height_anomaly_m_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        *value = (20.0 * cell.center_unit()[0]) as f32;
    }
    (forcing, state)
}

fn sharp_humidity_forcing(grid: &CubedSphereGrid) -> PlanetForcing {
    let humidity = grid
        .cells()
        .iter()
        .map(|cell| {
            let value = if cell.center_unit()[0] >= 0.0 {
                0.000_1
            } else {
                0.02
            };
            [value; 12]
        })
        .collect::<Vec<_>>();
    PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; grid.cell_count()],
        vec![0.0; grid.cell_count()],
        vec![0.0; grid.cell_count()],
        vec![1.0; grid.cell_count()],
        vec![[15.0; 12]; grid.cell_count()],
        vec![[15.0; 12]; grid.cell_count()],
        humidity,
    )
    .unwrap()
}

fn advance_reference(
    integrator: &ExplicitRk3Integrator<'_>,
    initial: &LayeredClimateState,
    forcing: &PlanetForcing,
    permeability: &[f32],
    step_seconds: f64,
    steps: usize,
) -> LayeredClimateState {
    let mut state = initial.clone();
    for _ in 0..steps {
        state = integrator
            .advance(
                &state,
                forcing,
                permeability,
                0,
                step_seconds,
                &BuildCancellation::new(),
            )
            .unwrap()
            .into_state();
    }
    state
}

#[test]
fn all_integrators_preserve_the_exact_uniform_c1_equilibrium() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let forcing = uniform_forcing(&grid, 15.0);
    let state = state(&grid, ClimateModelProfile::C1SingleLayerV1, &forcing);
    let permeability = vec![1.0; grid.edges().len()];
    let cancellation = BuildCancellation::new();

    let explicit = ExplicitRk3Integrator::new(&grid)
        .advance(&state, &forcing, &permeability, 0, 600.0, &cancellation)
        .unwrap();
    let imex = ImexCrankNicolsonIntegrator::new(&grid, 24, 1.0e-7)
        .unwrap()
        .advance(&state, &forcing, &permeability, 0, 21_600.0, &cancellation)
        .unwrap();
    let split = SplitExplicitRk3Integrator::new(&grid, 600.0)
        .unwrap()
        .advance(&state, &forcing, &permeability, 0, 21_600.0, &cancellation)
        .unwrap();

    assert_eq!(explicit.state(), &state);
    assert_eq!(imex.state(), &state);
    assert_eq!(split.state(), &state);
    assert_eq!(imex.diagnostics().final_linear_relative_residual(), 0.0);
}

#[test]
fn explicit_reference_exhibits_third_order_convergence_on_a_smooth_wave() {
    let grid = CubedSphereGrid::new(3, 6_371_000.0).unwrap();
    let (forcing, initial) = perturbed_state(&grid);
    let permeability = vec![1.0; grid.edges().len()];
    let integrator = ExplicitRk3Integrator::new(&grid);
    let fine = advance_reference(&integrator, &initial, &forcing, &permeability, 125.0, 64);
    let coarse = advance_reference(&integrator, &initial, &forcing, &permeability, 2_000.0, 4);
    let medium = advance_reference(&integrator, &initial, &forcing, &permeability, 1_000.0, 8);
    let coarse_error = climate_state_rms_difference(&grid, &coarse, &fine).unwrap();
    let medium_error = climate_state_rms_difference(&grid, &medium, &fine).unwrap();
    assert!(
        coarse_error / medium_error >= 5.5,
        "RK3 refinement ratio {}, coarse {coarse_error}, medium {medium_error}",
        coarse_error / medium_error,
    );
}

#[test]
fn formation_residual_exposes_unconverged_humidity_and_wind_independently() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let forcing = uniform_forcing(&grid, 15.0);
    let baseline = state(&grid, ClimateModelProfile::C2LayeredV1, &forcing);

    let mut humid = baseline.clone();
    for value in humid.specific_humidity_mut() {
        *value += 0.01;
    }
    let humidity_residual = climate_state_formation_residual(&grid, &baseline, &humid).unwrap();
    assert!(
        humidity_residual > 0.25,
        "an annual 0.01 kg/kg humidity drift must remain visible, got {humidity_residual}"
    );

    let mut windy = baseline.clone();
    for (cell, velocity) in grid.cells().iter().zip(
        windy
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        let center = cell.center_unit();
        let reference = if center[2].abs() < 0.9 {
            [0.0, 0.0, 1.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let tangent = [
            reference[1] * center[2] - reference[2] * center[1],
            reference[2] * center[0] - reference[0] * center[2],
            reference[0] * center[1] - reference[1] * center[0],
        ];
        let length = tangent
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        *velocity = std::array::from_fn(|component| (10.0 * tangent[component] / length) as f32);
    }
    let wind_residual = climate_state_formation_residual(&grid, &baseline, &windy).unwrap();
    assert!(
        wind_residual > 0.25,
        "an annual 10 m/s wind drift must remain visible, got {wind_residual}"
    );
}

#[test]
fn production_candidates_remain_finite_and_positive_beyond_explicit_macro_stability() {
    let grid = CubedSphereGrid::new(3, 6_371_000.0).unwrap();
    let (forcing, initial) = perturbed_state(&grid);
    let permeability = vec![1.0; grid.edges().len()];
    let cancellation = BuildCancellation::new();
    let imex = ImexCrankNicolsonIntegrator::new(&grid, 32, 1.0e-6)
        .unwrap()
        .advance(
            &initial,
            &forcing,
            &permeability,
            0,
            21_600.0,
            &cancellation,
        )
        .unwrap();
    let split = SplitExplicitRk3Integrator::new(&grid, 600.0)
        .unwrap()
        .advance(
            &initial,
            &forcing,
            &permeability,
            0,
            21_600.0,
            &cancellation,
        )
        .unwrap();

    imex.state().validate_against(&grid).unwrap();
    split.state().validate_against(&grid).unwrap();
    assert!(imex.diagnostics().linear_iterations() > 0);
    assert!(imex.diagnostics().final_linear_relative_residual() <= 1.0e-6);
    assert_eq!(split.diagnostics().fast_substeps(), 36);
}

#[test]
fn split_explicit_subcycles_against_the_actual_characteristic_speed() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let forcing = uniform_forcing(&grid, 15.0);
    let mut initial = state(&grid, ClimateModelProfile::C1SingleLayerV1, &forcing);
    for (cell, velocity) in grid.cells().iter().zip(
        initial
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        let [x, y, _] = cell.center_unit();
        *velocity = [-250.0 * y as f32, 250.0 * x as f32, 0.0];
    }
    let result = SplitExplicitRk3Integrator::new(&grid, 7_200.0)
        .unwrap()
        .advance(
            &initial,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    assert!(
        result.diagnostics().maximum_cfl() <= 0.20 + 1.0e-12,
        "dynamic split CFL was {}",
        result.diagnostics().maximum_cfl()
    );
    assert!(result.diagnostics().fast_substeps() > 1);
}

#[test]
fn moisture_transport_operator_is_positive_and_conservative_over_the_real_macro_horizon() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let forcing = sharp_humidity_forcing(&grid);
    let mut initial = state(&grid, ClimateModelProfile::C1SingleLayerV1, &forcing);
    for (cell, velocity) in grid.cells().iter().zip(
        initial
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        let [x, y, _] = cell.center_unit();
        *velocity = [-250.0 * y as f32, 250.0 * x as f32, 0.0];
    }
    let total = |values: &[f32]| {
        grid.cells()
            .iter()
            .zip(values)
            .map(|(cell, humidity)| cell.area_m2() * f64::from(*humidity))
            .sum::<f64>()
    };
    let before = total(initial.specific_humidity());
    let mut workspace = SecondOrderTransportWorkspace::for_grid(&grid);
    let result = CirculationOperators::new(&grid)
        .advect_scalar_monotone_second_order_into_cancellable(
            initial.specific_humidity(),
            initial
                .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
                .unwrap(),
            &vec![1.0; grid.edges().len()],
            7_200.0,
            true,
            &mut workspace,
            &BuildCancellation::new(),
        )
        .unwrap();
    let after = total(result.values());

    assert!(result.values().iter().all(|value| *value >= 0.0));
    assert!(
        (after - before).abs() / before <= 2.0e-7,
        "macro-horizon transport changed moisture: before={before}, after={after}"
    );
    assert!(result.relative_mass_error() <= 2.0e-7);
    assert_ne!(
        result.values(),
        initial.specific_humidity(),
        "fixture must exercise transport rather than a stationary field"
    );
}

#[test]
fn split_macro_step_moisture_delta_matches_declared_external_sources_minus_precipitation() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let forcing = sharp_humidity_forcing(&grid);
    let mut initial = state(&grid, ClimateModelProfile::C1SingleLayerV1, &forcing);
    for (cell, velocity) in grid.cells().iter().zip(
        initial
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        let [x, y, _] = cell.center_unit();
        *velocity = [-250.0 * y as f32, 250.0 * x as f32, 0.0];
    }
    let permeability = vec![1.0; grid.edges().len()];
    let cancellation = BuildCancellation::new();
    let declared = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(&initial, &forcing, &permeability, 0, 7_200.0, &cancellation)
        .unwrap();
    let result = SplitExplicitRk3Integrator::new(&grid, 1_200.0)
        .unwrap()
        .advance(&initial, &forcing, &permeability, 0, 7_200.0, &cancellation)
        .unwrap();
    let column_mass = 1.225 * 8_000.0;
    let actual_change = grid
        .cells()
        .iter()
        .zip(
            initial
                .specific_humidity()
                .iter()
                .zip(result.state().specific_humidity()),
        )
        .map(|(cell, (before, after))| {
            cell.area_m2() * column_mass * (f64::from(*after) - f64::from(*before))
        })
        .sum::<f64>();
    let expected_change = declared.budget().external_moisture_net_rate_kg_s() * 7_200.0;
    let scale = grid
        .cells()
        .iter()
        .zip(initial.specific_humidity())
        .map(|(cell, humidity)| cell.area_m2() * column_mass * f64::from(*humidity))
        .sum::<f64>();
    assert!(
        (actual_change - expected_change).abs() / scale <= 1.0e-6,
        "actual moisture delta={actual_change}, declared external delta={expected_change}"
    );
}

#[test]
fn closed_split_path_preserves_every_c2_layer_mass_over_an_analytic_year() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let report = run_closed_split_annual_mass_fixture(&grid, &BuildCancellation::new()).unwrap();

    assert_eq!(report.months(), 12);
    assert_eq!(report.layers().len(), 4);
    assert!(
        report.maximum_absolute_height_change_m() > 0.0,
        "the analytic fixture must exercise closed gravity-wave dynamics"
    );
    for layer in report.layers() {
        assert!(
            layer.relative_mass_drift() <= 1.0e-8,
            "{:?} annual mass drift was {}",
            layer.role(),
            layer.relative_mass_drift()
        );
    }
    assert!(report.maximum_relative_mass_drift() <= 1.0e-8);
}

#[test]
fn split_explicit_reports_the_frozen_slow_macro_step_precipitation() {
    let grid = CubedSphereGrid::new(3, 6_371_000.0).unwrap();
    let forcing = sharp_humidity_forcing(&grid);
    let mut initial = state(&grid, ClimateModelProfile::C1SingleLayerV1, &forcing);
    for (cell, velocity) in grid.cells().iter().zip(
        initial
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        let [x, y, _] = cell.center_unit();
        *velocity = [-120.0 * y as f32, 120.0 * x as f32, 0.0];
    }
    let permeability = vec![1.0; grid.edges().len()];
    let declared = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &initial,
            &forcing,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let result = SplitExplicitRk3Integrator::new(&grid, 600.0)
        .unwrap()
        .advance(
            &initial,
            &forcing,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    assert_eq!(
        result.mean_precipitation_rate_mm_s(),
        declared.precipitation_rate_mm_s()
    );
    let terminal = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            result.state(),
            &forcing,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    assert!(
        result
            .mean_precipitation_rate_mm_s()
            .iter()
            .zip(terminal.precipitation_rate_mm_s())
            .any(|(mean, terminal)| mean.to_bits() != terminal.to_bits()),
        "fixture must distinguish the emitted frozen diagnostic from terminal reevaluation"
    );
}

#[test]
fn explicit_rk3_reports_stage_integrated_not_terminal_precipitation() {
    let grid = CubedSphereGrid::new(3, 6_371_000.0).unwrap();
    let forcing = sharp_humidity_forcing(&grid);
    let mut initial = state(&grid, ClimateModelProfile::C1SingleLayerV1, &forcing);
    for (cell, velocity) in grid.cells().iter().zip(
        initial
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        let [x, y, _] = cell.center_unit();
        *velocity = [-120.0 * y as f32, 120.0 * x as f32, 0.0];
    }
    let permeability = vec![1.0; grid.edges().len()];
    let result = ExplicitRk3Integrator::new(&grid)
        .advance(
            &initial,
            &forcing,
            &permeability,
            0,
            300.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let terminal = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            result.state(),
            &forcing,
            &permeability,
            0,
            300.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    assert!(result
        .mean_precipitation_rate_mm_s()
        .iter()
        .all(|rate| rate.is_finite() && *rate >= 0.0));
    assert!(
        result
            .mean_precipitation_rate_mm_s()
            .iter()
            .zip(terminal.precipitation_rate_mm_s())
            .any(|(mean, terminal)| mean.to_bits() != terminal.to_bits()),
        "fixture must distinguish the RK3 stage integral from terminal reevaluation"
    );
}

#[test]
fn integrator_results_and_diagnostics_are_byte_deterministic() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let (forcing, initial) = perturbed_state(&grid);
    let permeability = vec![1.0; grid.edges().len()];
    let run = || {
        SplitExplicitRk3Integrator::new(&grid, 300.0)
            .unwrap()
            .advance(
                &initial,
                &forcing,
                &permeability,
                0,
                1_800.0,
                &BuildCancellation::new(),
            )
            .unwrap()
    };
    assert_eq!(run(), run());
}

#[test]
fn every_integrator_rejects_pre_cancelled_work_atomically() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let (forcing, initial) = perturbed_state(&grid);
    let permeability = vec![1.0; grid.edges().len()];
    let cancellation = BuildCancellation::new();
    cancellation.cancel();

    assert_eq!(
        ExplicitRk3Integrator::new(&grid).advance(
            &initial,
            &forcing,
            &permeability,
            0,
            300.0,
            &cancellation,
        ),
        Err(ClimateIntegratorError::Cancelled)
    );
    assert_eq!(
        ImexCrankNicolsonIntegrator::new(&grid, 8, 1.0e-5)
            .unwrap()
            .advance(&initial, &forcing, &permeability, 0, 3_600.0, &cancellation,),
        Err(ClimateIntegratorError::Cancelled)
    );
    assert_eq!(
        SplitExplicitRk3Integrator::new(&grid, 300.0)
            .unwrap()
            .advance(&initial, &forcing, &permeability, 0, 3_600.0, &cancellation,),
        Err(ClimateIntegratorError::Cancelled)
    );
}
