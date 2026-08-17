use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::{
    climate_state_rms_difference, ClimateIntegratorError, ExplicitRk3Integrator,
    ImexCrankNicolsonIntegrator, LayeredClimateState, SplitExplicitRk3Integrator,
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
        result.diagnostics().maximum_cfl() <= 0.35 + 1.0e-12,
        "dynamic split CFL was {}",
        result.diagnostics().maximum_cfl()
    );
    assert!(result.diagnostics().fast_substeps() > 1);
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
