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
    large_scale_condensation_kg_m2_s, saturation_specific_humidity_kg_kg, ClimateLayerLayout,
    ClimateLayerRole, ClimateModelProfile, PlanetForcing, P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K,
    P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY, P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS,
    WATER_VAPORIZATION_LATENT_HEAT_J_KG,
};

fn uniform_forcing(grid: &CubedSphereGrid, temperature_c: f32) -> PlanetForcing {
    uniform_forcing_with_absorbed_shortwave(grid, temperature_c, 240.0)
}

fn uniform_forcing_with_absorbed_shortwave(
    grid: &CubedSphereGrid,
    temperature_c: f32,
    absorbed_shortwave_w_m2: f32,
) -> PlanetForcing {
    let count = grid.cell_count();
    PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![1.0; count],
        vec![[absorbed_shortwave_w_m2; 12]; count],
        vec![[temperature_c; 12]; count],
        vec![[temperature_c; 12]; count],
        vec![[0.008; 12]; count],
    )
    .unwrap()
}

fn uniform_forcing_with_surface_water(
    grid: &CubedSphereGrid,
    temperature_c: f32,
    water_fraction: f32,
) -> PlanetForcing {
    let count = grid.cell_count();
    PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![water_fraction; count],
        vec![[240.0; 12]; count],
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
        vec![[240.0; 12]; grid.cell_count()],
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
fn retained_radiative_power_is_the_same_power_that_advances_temperature() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let profile = ClimateModelProfile::C1SingleLayerV1;
    let forcing = uniform_forcing(&grid, 18.0);
    let mut initial = state(&grid, profile, &forcing);
    let layout = ClimateLayerLayout::for_profile(profile);
    for layer in layout
        .layers()
        .iter()
        .filter(|layer| layer.dynamically_active())
    {
        for temperature in initial.temperature_c_mut(layer.role()).unwrap() {
            *temperature -= 2.0;
        }
    }
    let tendency = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &initial,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();

    for cell in 0..grid.cell_count() {
        let temperature_power_w_m2 = layout
            .layers()
            .iter()
            .filter(|layer| layer.dynamically_active())
            .map(|layer| {
                layer.density_kg_m3()
                    * layer.reference_thickness_m()
                    * layer.heat_capacity_j_kg_k()
                    * f64::from(tendency.temperature_tendency_k_s(layer.role()).unwrap()[cell])
            })
            .sum::<f64>();
        let declared_power_w_m2 = tendency.external_radiative_heat_flux_w_m2()[cell];
        let scale = declared_power_w_m2.abs().max(1.0);
        assert!(
            (temperature_power_w_m2 - declared_power_w_m2).abs() / scale <= 1.0e-5,
            "cell {cell}: temperature power {temperature_power_w_m2}, declared radiative power {declared_power_w_m2}"
        );
    }
}

#[test]
fn zero_absorbed_shortwave_prevents_positive_radiative_heating_in_the_equation() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let profile = ClimateModelProfile::C1SingleLayerV1;
    let forcing = uniform_forcing_with_absorbed_shortwave(&grid, 18.0, 0.0);
    let mut initial = state(&grid, profile, &forcing);
    for role in initial.active_roles().to_vec() {
        for temperature in initial.temperature_c_mut(role).unwrap() {
            *temperature -= 2.0;
        }
    }
    let tendency = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &initial,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();

    assert!(tendency
        .external_radiative_heat_flux_w_m2()
        .iter()
        .all(|power| *power <= 0.0));
    for role in initial.active_roles() {
        assert!(tendency
            .temperature_tendency_k_s(*role)
            .unwrap()
            .iter()
            .all(|value| *value == 0.0));
    }
}

#[test]
fn retained_evaporation_draws_only_surface_heat_and_closes_moist_energy() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let profile = ClimateModelProfile::C1SingleLayerV1;
    let wet_forcing = uniform_forcing_with_surface_water(&grid, 15.0, 1.0);
    let dry_forcing = uniform_forcing_with_surface_water(&grid, 15.0, 0.0);
    let mut initial = state(&grid, profile, &wet_forcing);
    let saturation = saturation_specific_humidity_kg_kg(15.0) as f32;
    initial.specific_humidity_mut().fill(0.5 * saturation);
    for (cell, velocity) in grid.cells().iter().zip(
        initial
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        let [x, y, _] = cell.center_unit();
        *velocity = [-12.0 * y as f32, 12.0 * x as f32, 0.0];
    }
    let system = LayeredTendencySystem::new(&grid);
    let permeability = vec![1.0; grid.edges().len()];
    let cancellation = BuildCancellation::new();
    let wet = system
        .evaluate_for_step(
            &initial,
            &wet_forcing,
            &permeability,
            0,
            7_200.0,
            &cancellation,
        )
        .unwrap();
    let dry = system
        .evaluate_for_step(
            &initial,
            &dry_forcing,
            &permeability,
            0,
            7_200.0,
            &cancellation,
        )
        .unwrap();
    let layout = ClimateLayerLayout::for_profile(profile);
    let lower = layout
        .layers()
        .iter()
        .find(|layer| layer.role() == ClimateLayerRole::LowerAtmosphere)
        .unwrap();
    let surface = layout
        .layers()
        .iter()
        .find(|layer| layer.role() == ClimateLayerRole::OceanMixedLayer)
        .unwrap();
    let lower_mass = lower.density_kg_m3() * lower.reference_thickness_m();
    let lower_capacity = lower_mass * lower.heat_capacity_j_kg_k();
    let surface_capacity =
        surface.density_kg_m3() * surface.reference_thickness_m() * surface.heat_capacity_j_kg_k();

    for cell in 0..grid.cell_count() {
        let evaporation = f64::from(wet.evaporation_rate_mm_s()[cell]);
        assert!(evaporation > 0.0);
        assert_eq!(dry.evaporation_rate_mm_s()[cell], 0.0);
        assert_eq!(wet.precipitation_rate_mm_s()[cell], 0.0);
        let humidity_power = WATER_VAPORIZATION_LATENT_HEAT_J_KG
            * lower_mass
            * f64::from(
                wet.specific_humidity_tendency_s_inv()[cell]
                    - dry.specific_humidity_tendency_s_inv()[cell],
            );
        let sensible_power = lower_capacity
            * f64::from(
                wet.temperature_tendency_k_s(ClimateLayerRole::LowerAtmosphere)
                    .unwrap()[cell]
                    - dry
                        .temperature_tendency_k_s(ClimateLayerRole::LowerAtmosphere)
                        .unwrap()[cell],
            )
            + surface_capacity
                * f64::from(
                    wet.temperature_tendency_k_s(ClimateLayerRole::OceanMixedLayer)
                        .unwrap()[cell]
                        - dry
                            .temperature_tendency_k_s(ClimateLayerRole::OceanMixedLayer)
                            .unwrap()[cell],
                );
        let scale = (WATER_VAPORIZATION_LATENT_HEAT_J_KG * evaporation).max(1.0);
        assert!((sensible_power + humidity_power).abs() / scale <= 2.0e-4);
        assert!(
            (lower_mass * f64::from(wet.specific_humidity_tendency_s_inv()[cell]) - evaporation)
                .abs()
                / evaporation
                <= 2.0e-6
        );
    }
}

#[test]
fn saturation_adjustment_removes_supersaturation_within_one_physical_step() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let forcing = uniform_forcing_with_surface_water(&grid, 15.0, 0.0);
    let mut initial = state(&grid, ClimateModelProfile::C1SingleLayerV1, &forcing);
    let saturation = saturation_specific_humidity_kg_kg(15.0) as f32;
    initial.specific_humidity_mut().fill(saturation + 0.002);
    let step_seconds = 7_200.0;
    let tendency = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &initial,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            step_seconds,
            &BuildCancellation::new(),
        )
        .unwrap();

    for cell in 0..grid.cell_count() {
        assert_eq!(tendency.evaporation_rate_mm_s()[cell], 0.0);
        assert!(tendency.precipitation_rate_mm_s()[cell] > 0.0);
        let adjusted = f64::from(initial.specific_humidity()[cell])
            + step_seconds * f64::from(tendency.specific_humidity_tendency_s_inv()[cell]);
        let adjusted_temperature = f64::from(
            initial
                .temperature_c(ClimateLayerRole::LowerAtmosphere)
                .unwrap()[cell],
        ) + step_seconds
            * f64::from(
                tendency
                    .temperature_tendency_k_s(ClimateLayerRole::LowerAtmosphere)
                    .unwrap()[cell],
            );
        assert!(adjusted <= saturation_specific_humidity_kg_kg(adjusted_temperature) + 2.0e-9);
        assert!(adjusted >= 0.0);
    }
}

#[test]
fn upper_layer_saturation_adjustment_cannot_store_cloud_water_without_rain() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let forcing = uniform_forcing_with_surface_water(&grid, 15.0, 0.0);
    let mut initial = state(&grid, ClimateModelProfile::C2LayeredV1, &forcing);
    initial.specific_humidity_mut().fill(0.0);
    let upper_temperature = initial
        .temperature_c(ClimateLayerRole::UpperAtmosphere)
        .unwrap()[0];
    let saturation = saturation_specific_humidity_kg_kg(f64::from(upper_temperature)) as f32;
    let mut saturated_control = initial.clone();
    saturated_control
        .upper_specific_humidity_mut()
        .unwrap()
        .fill(saturation);
    initial
        .upper_specific_humidity_mut()
        .unwrap()
        .fill(saturation + 0.002);
    let step_seconds = 7_200.0;
    let tendency = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &initial,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            step_seconds,
            &BuildCancellation::new(),
        )
        .unwrap();
    let control = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &saturated_control,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            step_seconds,
            &BuildCancellation::new(),
        )
        .unwrap();
    let upper_tendency = tendency.upper_specific_humidity_tendency_s_inv().unwrap();
    let initial_upper = initial.upper_specific_humidity().unwrap();
    let precipitation = tendency.precipitation_rate_mm_s();
    let upper_heat = tendency
        .temperature_tendency_k_s(ClimateLayerRole::UpperAtmosphere)
        .unwrap();
    let control_upper_heat = control
        .temperature_tendency_k_s(ClimateLayerRole::UpperAtmosphere)
        .unwrap();

    for (cell, &humidity_tendency) in upper_tendency.iter().enumerate() {
        let adjusted = f64::from(initial_upper[cell]) + step_seconds * f64::from(humidity_tendency);
        let adjusted_temperature = f64::from(
            initial
                .temperature_c(ClimateLayerRole::UpperAtmosphere)
                .unwrap()[cell],
        ) + step_seconds * f64::from(upper_heat[cell]);
        assert!(adjusted <= saturation_specific_humidity_kg_kg(adjusted_temperature) + 2.0e-9);
        assert!(adjusted >= 0.0);
        assert!(precipitation[cell] > 0.0);
        assert!(
            upper_heat[cell] > control_upper_heat[cell],
            "upper condensation must release latent heat into the upper layer"
        );
    }
}

#[test]
fn coarse_grid_condensation_relaxes_cloudy_humidity_without_overshoot() {
    let temperature_c = 15.0;
    let saturation = saturation_specific_humidity_kg_kg(temperature_c);
    let threshold = P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY * saturation;
    let initial = 0.95 * saturation;
    let column_mass = 1.225 * 8_000.0;
    let step_seconds = 7_200.0;
    let rate = large_scale_condensation_kg_m2_s(initial, temperature_c, column_mass, step_seconds);
    let adjusted = initial - step_seconds * rate / column_mass;
    let adjusted_temperature = temperature_c
        + WATER_VAPORIZATION_LATENT_HEAT_J_KG * (initial - adjusted)
            / P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K;
    let initial_cloud_excess = initial - threshold;
    let adjusted_cloud_excess = adjusted
        - P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY
            * saturation_specific_humidity_kg_kg(adjusted_temperature);
    let expected_cloud_excess = initial_cloud_excess
        * (-step_seconds / P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS).exp();

    assert!((adjusted_cloud_excess - expected_cloud_excess).abs() <= 1.0e-14);
    assert!(adjusted_cloud_excess >= 0.0);
    assert!(adjusted < initial);
}

#[test]
fn coupled_saturation_adjustment_is_invariant_to_physical_step_partition() {
    fn advance(
        mut humidity: f64,
        mut temperature_c: f64,
        column_mass: f64,
        step_seconds: f64,
        steps: usize,
    ) -> (f64, f64) {
        for _ in 0..steps {
            let rate = large_scale_condensation_kg_m2_s(
                humidity,
                temperature_c,
                column_mass,
                step_seconds,
            );
            let condensed = step_seconds * rate / column_mass;
            humidity -= condensed;
            temperature_c += WATER_VAPORIZATION_LATENT_HEAT_J_KG * condensed
                / P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K;
        }
        (humidity, temperature_c)
    }

    let temperature_c = 15.0;
    let initial = 1.2 * saturation_specific_humidity_kg_kg(temperature_c);
    let column_mass = 1.225 * 8_000.0;
    let one_step = advance(initial, temperature_c, column_mass, 7_200.0, 1);
    let partitioned = advance(initial, temperature_c, column_mass, 300.0, 24);

    assert!((one_step.0 - partitioned.0).abs() <= 1.0e-14);
    assert!((one_step.1 - partitioned.1).abs() <= 1.0e-11);
    assert!(
        one_step.0 <= saturation_specific_humidity_kg_kg(one_step.1) + f64::EPSILON,
        "coupled adjustment must end at or below saturation: {one_step:?}"
    );
    let initial_moist_enthalpy = P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K * temperature_c
        + WATER_VAPORIZATION_LATENT_HEAT_J_KG * initial;
    let final_moist_enthalpy = P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K * one_step.1
        + WATER_VAPORIZATION_LATENT_HEAT_J_KG * one_step.0;
    assert!((initial_moist_enthalpy - final_moist_enthalpy).abs() <= 1.0e-9);
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
fn explicit_reference_reports_one_endpoint_precipitation_before_classic_rk3() {
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
    let endpoint = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &initial,
            &forcing,
            &permeability,
            0,
            300.0,
            &BuildCancellation::new(),
        )
        .unwrap();
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
    assert_eq!(result.diagnostics().endpoint_evaluations(), 1);
    assert_eq!(
        result.mean_precipitation_rate_mm_s(),
        endpoint.precipitation_rate_mm_s(),
        "the reference diagnostic must come from the one executed endpoint"
    );
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
        "fixture must distinguish the initial endpoint from terminal reevaluation"
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
