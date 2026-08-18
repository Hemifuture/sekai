use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::{
    CirculationOperators, CubedSphereGrid, SecondOrderTransportWorkspace,
};
use sekai::generators::natural::{
    paired_heat_exchange, paired_momentum_exchange, LayeredClimateState, LayeredTendencyError,
    LayeredTendencySystem, LayeredTendencyWorkspace,
};
use sekai::world::natural::{
    ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
};

fn forcing(grid: &CubedSphereGrid) -> PlanetForcing {
    let count = grid.cell_count();
    PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.1; count],
        vec![1.0; count],
        vec![[15.0; 12]; count],
        vec![[18.0; 12]; count],
        vec![[0.008; 12]; count],
    )
    .unwrap()
}

#[test]
fn state_uses_only_fixed_roles_and_positive_layer_thickness() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing(&grid), 0).unwrap();
    state.validate_against(&grid).unwrap();
    assert_eq!(state.profile(), ClimateModelProfile::C2LayeredV1);
    assert_eq!(
        state.active_roles(),
        &[
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
            ClimateLayerRole::OceanMixedLayer,
            ClimateLayerRole::OceanThermocline,
        ]
    );
    for role in state.active_roles() {
        assert!(state
            .actual_thickness_m(*role)
            .unwrap()
            .iter()
            .all(|value| *value > 0.0));
    }
    assert!(state.deep_ocean_temperature_c().is_some());
    assert!(state.upper_specific_humidity().is_some());
    assert!(state
        .upper_specific_humidity()
        .unwrap()
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0));
    assert_eq!(layout.exchanges().len(), 4);
    assert!(
        layout
            .exchange(
                ClimateLayerRole::OceanThermocline,
                ClimateLayerRole::DeepOceanReservoir
            )
            .unwrap()
            .heat_exchange_time_s()
            .unwrap()
            > layout
                .exchange(
                    ClimateLayerRole::OceanMixedLayer,
                    ClimateLayerRole::OceanThermocline
                )
                .unwrap()
                .heat_exchange_time_s()
                .unwrap()
    );
}

#[test]
fn paired_heat_and_momentum_exchange_close_extensive_budgets() {
    let heat = paired_heat_exchange(280.0, 300.0, 2.0e7, 4.0e9, 86_400.0).unwrap();
    assert!(heat.first_tendency_k_s() > 0.0);
    assert!(heat.second_tendency_k_s() < 0.0);
    assert!(
        (2.0e7 * heat.first_tendency_k_s() + 4.0e9 * heat.second_tendency_k_s()).abs() <= 1.0e-12
    );
    assert!(heat.extensive_residual_w_m2().abs() <= 1.0e-12);

    let momentum = paired_momentum_exchange(
        [10.0, -2.0, 0.0],
        [-1.0, 4.0, 0.0],
        10_000.0,
        100_000.0,
        43_200.0,
    )
    .unwrap();
    for component in 0..3 {
        assert!(
            (10_000.0 * momentum.first_acceleration_m_s2()[component]
                + 100_000.0 * momentum.second_acceleration_m_s2()[component])
                .abs()
                <= 1.0e-12
        );
    }
    assert!(momentum.extensive_residual_n_m2() <= 1.0e-12);
}

#[test]
fn final_c2_tendency_retains_mass_paired_vertical_moisture_exchange() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let count = grid.cell_count();
    let forcing = PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![1.0; count],
        vec![[15.0; 12]; count],
        vec![[15.0; 12]; count],
        vec![[0.001; 12]; count],
    )
    .unwrap();
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
    state.specific_humidity_mut().fill(0.001);
    state.upper_specific_humidity_mut().unwrap().fill(0.01);
    let tendency = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &state,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let lower = tendency.specific_humidity_tendency_s_inv();
    let upper = tendency.upper_specific_humidity_tendency_s_inv().unwrap();
    assert!(lower.iter().all(|value| *value > 0.0));
    assert!(upper.iter().all(|value| *value < 0.0));
    let lower_mass = 1.225 * 6_000.0;
    let upper_mass = 1.225 * 4_000.0;
    let residual = grid
        .cells()
        .iter()
        .zip(lower.iter().zip(upper))
        .map(|(cell, (lower, upper))| {
            cell.area_m2() * (lower_mass * f64::from(*lower) + upper_mass * f64::from(*upper))
        })
        .sum::<f64>();
    let scale = grid
        .cells()
        .iter()
        .zip(lower)
        .map(|(cell, lower)| cell.area_m2() * lower_mass * f64::from(*lower).abs())
        .sum::<f64>();
    let expected_external = tendency.budget().external_moisture_net_rate_kg_s();
    assert!(
        (residual - expected_external).abs() / scale <= 1.0e-6,
        "final moisture tendency disagrees with its external ledger: actual={residual} expected={expected_external} scale={scale}"
    );
    assert!(tendency.budget().paired_moisture_absolute_kg_s() > 0.0);
    assert!(
        tendency.budget().paired_moisture_residual_kg_s()
            / tendency.budget().paired_moisture_absolute_kg_s()
            <= 1.0e-6
    );

    let long_step = 100_000_000.0;
    let limited = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &state,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            long_step,
            &BuildCancellation::new(),
        )
        .unwrap();
    let limited_upper = limited.upper_specific_humidity_tendency_s_inv().unwrap();
    for (cell, &upper_tendency) in limited_upper.iter().enumerate().take(count) {
        assert!(
            f64::from(state.specific_humidity()[cell])
                + long_step * f64::from(limited.specific_humidity_tendency_s_inv()[cell])
                >= 0.0
        );
        assert!(
            f64::from(state.upper_specific_humidity().unwrap()[cell])
                + long_step * f64::from(upper_tendency)
                >= 0.0
        );
    }
    let limited_residual = grid
        .cells()
        .iter()
        .zip(
            limited
                .specific_humidity_tendency_s_inv()
                .iter()
                .zip(limited_upper),
        )
        .map(|(cell, (lower, upper))| {
            cell.area_m2() * (lower_mass * f64::from(*lower) + upper_mass * f64::from(*upper))
        })
        .sum::<f64>();
    let limited_scale = grid
        .cells()
        .iter()
        .zip(limited.specific_humidity_tendency_s_inv())
        .map(|(cell, lower)| cell.area_m2() * lower_mass * f64::from(*lower).abs())
        .sum::<f64>();
    let limited_external = limited.budget().external_moisture_net_rate_kg_s();
    assert!(
        (limited_residual - limited_external).abs() / limited_scale <= 1.0e-6,
        "limited final moisture disagrees with external ledger: actual={limited_residual} expected={limited_external} scale={limited_scale} paired_residual={} paired_scale={}",
        limited.budget().paired_moisture_residual_kg_s(),
        limited.budget().paired_moisture_absolute_kg_s(),
    );
    assert!(
        limited.budget().paired_moisture_residual_kg_s()
            / limited.budget().paired_moisture_absolute_kg_s()
            <= 1.0e-6
    );
}

#[test]
fn shared_tendency_is_tangent_budgeted_and_honors_closed_ocean_edges() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let forcing = forcing(&grid);
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
    let edge = &grid.edges()[0];
    let first = edge.cells()[0] as usize;
    state
        .velocity_m_s_mut(ClimateLayerRole::OceanMixedLayer)
        .unwrap()[first] = edge.normal_from_first().map(|value| value as f32);

    let system = LayeredTendencySystem::new(&grid);
    let cancellation = BuildCancellation::new();
    let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
    let allocation = workspace.allocation_signature();
    let closed = system
        .evaluate_with_workspace(
            &state,
            &forcing,
            &vec![0.0; grid.edges().len()],
            0,
            &cancellation,
            &mut workspace,
        )
        .unwrap();
    assert_eq!(workspace.allocation_signature(), allocation);
    assert!(closed
        .height_tendency_m_s(ClimateLayerRole::OceanMixedLayer)
        .unwrap()
        .iter()
        .all(|value| value.abs() <= 1.0e-12));

    let open = system
        .evaluate_with_workspace(
            &state,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            &cancellation,
            &mut workspace,
        )
        .unwrap();
    assert!(open
        .height_tendency_m_s(ClimateLayerRole::OceanMixedLayer)
        .unwrap()
        .iter()
        .any(|value| value.abs() > 0.0));
    for role in state.active_roles() {
        for (cell, tendency) in grid.cells().iter().zip(
            open.velocity_tendency_m_s2(*role)
                .expect("every active layer has momentum"),
        ) {
            let radial = cell.center_unit();
            let dot = f64::from(tendency[0]) * radial[0]
                + f64::from(tendency[1]) * radial[1]
                + f64::from(tendency[2]) * radial[2];
            assert!(dot.abs() <= 1.0e-6);
        }
    }
    assert!(open.budget().paired_heat_absolute_w() > 0.0);
    assert!(
        open.budget().paired_heat_residual_w() / open.budget().paired_heat_absolute_w() <= 1.0e-6
    );
    assert!(
        open.budget().paired_momentum_residual_n() / open.budget().paired_momentum_absolute_n()
            <= 1.0e-6,
        "momentum residual={} scale={} ratio={}",
        open.budget().paired_momentum_residual_n(),
        open.budget().paired_momentum_absolute_n(),
        open.budget().paired_momentum_residual_n() / open.budget().paired_momentum_absolute_n()
    );
    assert!(open.budget().paired_moisture_absolute_kg_s() > 0.0);
    assert!(
        open.budget().paired_moisture_residual_kg_s()
            / open.budget().paired_moisture_absolute_kg_s()
            <= 1.0e-6
    );
    assert!(open
        .budget()
        .external_moisture_source_rate_kg_s()
        .is_finite());
    assert!(open
        .budget()
        .external_precipitation_sink_rate_kg_s()
        .is_finite());
    assert!(open.budget().external_heat_rate_w().is_finite());
    assert!(open
        .budget()
        .external_atmosphere_amount_rate_m3_s()
        .is_finite());
    assert!(open.budget().external_ocean_amount_rate_m3_s().is_finite());
}

#[test]
fn fractional_coast_form_drag_lives_in_the_shared_momentum_tendency() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let count = grid.cell_count();
    let make_forcing = |land_fraction: f32| {
        PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![land_fraction; count],
            vec![0.1; count],
            vec![1.0; count],
            vec![[15.0; 12]; count],
            vec![[15.0; 12]; count],
            vec![[0.008; 12]; count],
        )
        .unwrap()
    };
    let open = make_forcing(0.0);
    let coast = make_forcing(0.5);
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let mut state = LayeredClimateState::from_forcing(&grid, &layout, &open, 0).unwrap();
    for role in state.active_roles() {
        for (cell, velocity) in grid
            .cells()
            .iter()
            .zip(state.velocity_m_s_mut(*role).unwrap())
        {
            let [x, y, _] = cell.center_unit();
            *velocity = [-y as f32, x as f32, 0.0];
        }
    }
    let permeability = vec![1.0; grid.edges().len()];
    let system = LayeredTendencySystem::new(&grid);
    let open_tendency = system
        .evaluate_for_step(
            &state,
            &open,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let coast_tendency = system
        .evaluate_for_step(
            &state,
            &coast,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();

    for role in [
        ClimateLayerRole::OceanMixedLayer,
        ClimateLayerRole::OceanThermocline,
    ] {
        let velocity = state.velocity_m_s(role).unwrap();
        let open_acceleration = open_tendency.velocity_tendency_m_s2(role).unwrap();
        let coast_acceleration = coast_tendency.velocity_tendency_m_s2(role).unwrap();
        for cell in 0..count {
            for component in 0..3 {
                let found = f64::from(coast_acceleration[cell][component])
                    - f64::from(open_acceleration[cell][component]);
                let expected = -0.5 / 86_400.0 * f64::from(velocity[cell][component]);
                assert!(
                    (found - expected).abs() <= 2.0e-10,
                    "{role:?} cell {cell} component {component}: {found} != {expected}"
                );
            }
        }
    }
}

#[test]
fn physical_bathymetry_controls_shared_thermocline_bottom_drag() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let count = grid.cell_count();
    let make_forcing = |ocean_depth_m: f32| {
        PlanetForcing::new_with_ocean_depth(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![ocean_depth_m; count],
            vec![0.1; count],
            vec![1.0; count],
            vec![[15.0; 12]; count],
            vec![[15.0; 12]; count],
            vec![[0.008; 12]; count],
        )
        .unwrap()
    };
    let shallow = make_forcing(500.0);
    let deep = make_forcing(4_000.0);
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let mut state = LayeredClimateState::from_forcing(&grid, &layout, &deep, 0).unwrap();
    for (cell, velocity) in grid.cells().iter().zip(
        state
            .velocity_m_s_mut(ClimateLayerRole::OceanThermocline)
            .unwrap(),
    ) {
        let [x, y, _] = cell.center_unit();
        *velocity = [-y as f32, x as f32, 0.0];
    }
    let system = LayeredTendencySystem::new(&grid);
    let permeability = vec![1.0; grid.edges().len()];
    let shallow_tendency = system
        .evaluate_for_step(
            &state,
            &shallow,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let deep_tendency = system
        .evaluate_for_step(
            &state,
            &deep,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let velocity = state
        .velocity_m_s(ClimateLayerRole::OceanThermocline)
        .unwrap();
    let shallow_acceleration = shallow_tendency
        .velocity_tendency_m_s2(ClimateLayerRole::OceanThermocline)
        .unwrap();
    let deep_acceleration = deep_tendency
        .velocity_tendency_m_s2(ClimateLayerRole::OceanThermocline)
        .unwrap();
    let expected_drag_difference_s_inv = 0.75 / (90.0 * 86_400.0);
    for cell in 0..count {
        for component in 0..3 {
            let found = f64::from(shallow_acceleration[cell][component])
                - f64::from(deep_acceleration[cell][component]);
            let expected = -expected_drag_difference_s_inv * f64::from(velocity[cell][component]);
            assert!(
                (found - expected).abs() <= 2.0e-10,
                "cell {cell} component {component}: {found} != {expected}"
            );
        }
    }
}

#[test]
fn warm_mixed_layer_steric_pressure_accelerates_toward_warm_water() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let count = grid.cell_count();
    let forcing = PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.1; count],
        vec![1.0; count],
        vec![[15.0; 12]; count],
        vec![[15.0; 12]; count],
        vec![[0.008; 12]; count],
    )
    .unwrap();
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
    for (cell, temperature) in grid.cells().iter().zip(
        state
            .temperature_c_mut(ClimateLayerRole::OceanMixedLayer)
            .unwrap(),
    ) {
        *temperature = 15.0 + 8.0 * cell.center_unit()[0] as f32;
    }
    let permeability = vec![1.0; grid.edges().len()];
    let tendency = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &state,
            &forcing,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let temperature = state
        .temperature_c(ClimateLayerRole::OceanMixedLayer)
        .unwrap();
    let gradient = CirculationOperators::new(&grid)
        .gradient_with_permeability(temperature, &permeability)
        .unwrap();
    let acceleration = tendency
        .velocity_tendency_m_s2(ClimateLayerRole::OceanMixedLayer)
        .unwrap();
    let coefficient = 0.5 * 9.806_65 * 2.0e-4 * 100.0;
    let mut exercised = 0;
    for cell in 0..count {
        for component in 0..3 {
            let expected = coefficient * f64::from(gradient[cell][component]);
            let found = f64::from(acceleration[cell][component]);
            assert!(
                (found - expected).abs() <= 2.0e-12,
                "cell {cell} component {component}: {found} != {expected}"
            );
            exercised += usize::from(expected.abs() > 1.0e-12);
        }
    }
    assert!(exercised > 0, "fixture must contain a thermal gradient");

    let thermocline = tendency
        .velocity_tendency_m_s2(ClimateLayerRole::OceanThermocline)
        .unwrap();
    assert!(thermocline
        .iter()
        .flatten()
        .all(|component| component.abs() <= 1.0e-12));
}

#[test]
fn two_layer_baroclinic_pressure_drives_low_level_return_and_upper_outflow() {
    let grid = CubedSphereGrid::new(4, 6_371_000.0).unwrap();
    let count = grid.cell_count();
    let air_temperature = grid
        .cells()
        .iter()
        .map(|cell| [15.0 + 8.0 * cell.center_unit()[0] as f32; 12])
        .collect::<Vec<_>>();
    let forcing = PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.1; count],
        vec![1.0; count],
        air_temperature,
        vec![[15.0; 12]; count],
        vec![[0.008; 12]; count],
    )
    .unwrap();
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
    let permeability = vec![1.0; grid.edges().len()];
    let tendency = LayeredTendencySystem::new(&grid)
        .evaluate_for_step(
            &state,
            &forcing,
            &permeability,
            0,
            7_200.0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let gradient = CirculationOperators::new(&grid)
        .gradient_with_permeability(
            state
                .temperature_c(ClimateLayerRole::LowerAtmosphere)
                .unwrap(),
            &permeability,
        )
        .unwrap();
    let lower = tendency
        .velocity_tendency_m_s2(ClimateLayerRole::LowerAtmosphere)
        .unwrap();
    let upper = tendency
        .velocity_tendency_m_s2(ClimateLayerRole::UpperAtmosphere)
        .unwrap();
    let expected_lower_coefficient = 25.0_f64 * 4_000.0 / 6_000.0;
    assert!((6_000.0 * expected_lower_coefficient - 4_000.0 * 25.0).abs() <= 1.0e-10);
    let mut exercised = 0;
    for cell in 0..count {
        for component in 0..3 {
            let grad = f64::from(gradient[cell][component]);
            let expected_lower = expected_lower_coefficient * grad;
            let expected_upper = -25.0 * grad;
            assert!((f64::from(lower[cell][component]) - expected_lower).abs() <= 2.0e-10);
            assert!((f64::from(upper[cell][component]) - expected_upper).abs() <= 2.0e-10);
            exercised += usize::from(grad.abs() > 1.0e-12);
        }
    }
    assert!(exercised > 0, "fixture must contain a thermal gradient");
}

#[test]
fn tendency_rejects_nonpositive_thickness_bad_permeability_and_cancellation() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let forcing = forcing(&grid);
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
    state
        .height_anomaly_m_mut(ClimateLayerRole::LowerAtmosphere)
        .unwrap()[0] = -6_000.0;
    assert!(state.validate_against(&grid).is_err());

    let valid = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
    let system = LayeredTendencySystem::new(&grid);
    assert!(matches!(
        system.evaluate(&valid, &forcing, &[1.0], 0, &BuildCancellation::new()),
        Err(LayeredTendencyError::PermeabilityLengthMismatch { .. })
    ));
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    assert_eq!(
        system.evaluate(
            &valid,
            &forcing,
            &vec![1.0; grid.edges().len()],
            0,
            &cancellation
        ),
        Err(LayeredTendencyError::Cancelled)
    );
}

#[test]
fn shared_tendency_uses_monotone_second_order_heat_and_moisture_transport() {
    let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
    let forcing = forcing(&grid);
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1);
    let mut still = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
    for (cell, temperature) in grid.cells().iter().zip(
        still
            .temperature_c_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        *temperature = (15.0 + 2.0 * cell.center_unit()[0]) as f32;
    }
    for (cell, humidity) in grid.cells().iter().zip(still.specific_humidity_mut()) {
        *humidity = (0.008 + 0.002 * cell.center_unit()[0]) as f32;
    }
    let mut moving = still.clone();
    for (cell, velocity) in grid.cells().iter().zip(
        moving
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        let [x, y, _] = cell.center_unit();
        *velocity = [-80.0 * y as f32, 80.0 * x as f32, 0.0];
    }
    let permeability = vec![1.0; grid.edges().len()];
    let system = LayeredTendencySystem::new(&grid);
    let still_tendency = system
        .evaluate(
            &still,
            &forcing,
            &permeability,
            0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let moving_tendency = system
        .evaluate(
            &moving,
            &forcing,
            &permeability,
            0,
            &BuildCancellation::new(),
        )
        .unwrap();
    let operators = CirculationOperators::new(&grid);
    let mut workspace = SecondOrderTransportWorkspace::for_grid(&grid);
    let expected_temperature = operators
        .advect_scalar_monotone_second_order_into(
            still
                .temperature_c(ClimateLayerRole::LowerAtmosphere)
                .unwrap(),
            moving
                .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
                .unwrap(),
            &permeability,
            1.0,
            false,
            &mut workspace,
        )
        .unwrap()
        .values()
        .to_vec();
    let expected_humidity = operators
        .advect_scalar_monotone_second_order_into(
            still.specific_humidity(),
            moving
                .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
                .unwrap(),
            &permeability,
            1.0,
            true,
            &mut workspace,
        )
        .unwrap()
        .values()
        .to_vec();
    for cell in 0..grid.cell_count() {
        let found_temperature = moving_tendency
            .temperature_tendency_k_s(ClimateLayerRole::LowerAtmosphere)
            .unwrap()[cell]
            - still_tendency
                .temperature_tendency_k_s(ClimateLayerRole::LowerAtmosphere)
                .unwrap()[cell];
        let expected_temperature = expected_temperature[cell]
            - still
                .temperature_c(ClimateLayerRole::LowerAtmosphere)
                .unwrap()[cell];
        assert!((found_temperature - expected_temperature).abs() <= 2.0e-8);

        let found_humidity = moving_tendency.specific_humidity_tendency_s_inv()[cell]
            - still_tendency.specific_humidity_tendency_s_inv()[cell];
        let expected_humidity = expected_humidity[cell] - still.specific_humidity()[cell];
        assert!((found_humidity - expected_humidity).abs() <= 2.0e-10);
    }
}
