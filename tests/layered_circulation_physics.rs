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
    assert!(layout.layers()[4].exchange_time_s() > layout.layers()[3].exchange_time_s());
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
    assert!(open.budget().paired_heat_absolute_w_m2() > 0.0);
    assert!(open.budget().paired_heat_residual_w_m2().abs() <= 1.0e-6);
    assert!(open.budget().paired_momentum_residual_n_m2() <= 1.0e-6);
    assert!(open.budget().physical_moisture_source_kg_m2_s().is_finite());
    assert!(open
        .budget()
        .physical_precipitation_sink_kg_m2_s()
        .is_finite());
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
