use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::{
    compare_climate_states, run_integrator_comparison, ClimateAgreementThresholds,
    LayeredClimateState, ProductionCandidateSelection, SELECTED_PRODUCTION_INTEGRATOR,
};
use sekai::world::natural::{
    ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
    ProductionIntegratorId,
};

fn fixture(grid: &CubedSphereGrid) -> (PlanetForcing, LayeredClimateState) {
    let count = grid.cell_count();
    let temperature = grid
        .cells()
        .iter()
        .map(|cell| [15.0 - 20.0 * cell.center_unit()[2].abs() as f32; 12])
        .collect::<Vec<_>>();
    let forcing = PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.25; count],
        vec![1.0; count],
        temperature.clone(),
        temperature,
        vec![[0.008; 12]; count],
    )
    .unwrap();
    let mut state = LayeredClimateState::from_forcing(
        grid,
        &ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1),
        &forcing,
        0,
    )
    .unwrap();
    for (cell, height) in grid.cells().iter().zip(
        state
            .height_anomaly_m_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap(),
    ) {
        *height = (8.0 * cell.center_unit()[0]) as f32;
    }
    (forcing, state)
}

fn layered_fixture(
    grid: &CubedSphereGrid,
    profile: ClimateModelProfile,
    coastal: bool,
) -> (PlanetForcing, LayeredClimateState, Vec<f32>) {
    let count = grid.cell_count();
    let air_temperature = grid
        .cells()
        .iter()
        .map(|cell| {
            std::array::from_fn(|month| {
                let seasonal = if month < 6 { 2.0 } else { -2.0 };
                (16.0 - 24.0 * cell.center_unit()[2].abs() + seasonal * cell.center_unit()[2])
                    as f32
            })
        })
        .collect::<Vec<_>>();
    let surface_temperature = air_temperature
        .iter()
        .map(|months| months.map(|value| value + 1.5))
        .collect::<Vec<_>>();
    let land_fraction = grid
        .cells()
        .iter()
        .map(|cell| {
            if coastal && cell.center_unit()[0] > 0.2 {
                1.0
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();
    let forcing = PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        land_fraction.clone(),
        vec![1.0; count],
        air_temperature,
        surface_temperature,
        vec![[0.008; 12]; count],
    )
    .unwrap();
    let mut state = LayeredClimateState::from_forcing(
        grid,
        &ClimateLayerLayout::for_profile(profile),
        &forcing,
        0,
    )
    .unwrap();
    for role in state.active_roles().to_vec() {
        let amplitude = match role {
            ClimateLayerRole::LowerAtmosphere => 15.0,
            ClimateLayerRole::UpperAtmosphere => 8.0,
            ClimateLayerRole::OceanMixedLayer => 1.0,
            ClimateLayerRole::OceanThermocline => 0.5,
            ClimateLayerRole::DeepOceanReservoir => unreachable!(),
        };
        for (cell, height) in grid
            .cells()
            .iter()
            .zip(state.height_anomaly_m_mut(role).unwrap())
        {
            *height = (amplitude * cell.center_unit()[0]) as f32;
        }
    }
    let permeability = grid
        .edges()
        .iter()
        .map(|edge| {
            let first = edge.cells()[0] as usize;
            let second = edge.cells()[1] as usize;
            (1.0 - land_fraction[first]).min(1.0 - land_fraction[second])
        })
        .collect();
    (forcing, state, permeability)
}

#[test]
fn locked_comparison_accepts_identity_and_rejects_a_known_vector_bias() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let (_, reference) = fixture(&grid);
    let thresholds = ClimateAgreementThresholds::LOCKED;
    let identity = compare_climate_states(&grid, &reference, &reference, thresholds).unwrap();
    assert!(identity.qualifies());
    assert_eq!(identity.vector_correlation(), 1.0);
    assert_eq!(identity.scalar_correlation(), 1.0);

    let mut biased = reference.clone();
    for velocity in biased
        .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
        .unwrap()
    {
        velocity[0] += 100.0;
    }
    let comparison = compare_climate_states(&grid, &reference, &biased, thresholds).unwrap();
    assert!(!comparison.qualifies());
    assert!(comparison
        .failures()
        .iter()
        .any(|failure| failure.metric().contains("vector")));
}

#[test]
fn same_equation_suite_selects_only_a_candidate_that_passes_every_gate() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let (forcing, initial) = fixture(&grid);
    let report = run_integrator_comparison(
        &grid,
        &initial,
        &forcing,
        &vec![1.0; grid.edges().len()],
        0,
        1_800.0,
        300.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert!(report.split_explicit().qualifies());
    match report.selection() {
        ProductionCandidateSelection::Selected(ProductionIntegratorId::SplitExplicitRk3V1) => {
            assert!(report.split_explicit().qualifies());
        }
        ProductionCandidateSelection::Selected(ProductionIntegratorId::ImexCrankNicolsonV1) => {
            assert!(report.imex().qualifies());
        }
        ProductionCandidateSelection::NoQualifiedCandidate => {
            panic!("comparison selected no passing production candidate")
        }
    }
    assert!(report.reference_steps() > 1);
    assert!(report.imex().final_linear_relative_residual().is_finite());
}

#[test]
fn release_candidate_corpus_has_at_least_one_universally_qualified_integrator() {
    let mut imex_all = true;
    let mut split_all = true;
    for (profile, coastal, month) in [
        (ClimateModelProfile::C1SingleLayerV1, false, 0),
        (ClimateModelProfile::C1SingleLayerV1, true, 6),
        (ClimateModelProfile::C2LayeredV1, false, 0),
        (ClimateModelProfile::C2LayeredV1, true, 6),
    ] {
        let grid = CubedSphereGrid::new(3, 6_371_000.0).unwrap();
        let (forcing, initial, permeability) = layered_fixture(&grid, profile, coastal);
        let report = run_integrator_comparison(
            &grid,
            &initial,
            &forcing,
            &permeability,
            month,
            21_600.0,
            300.0,
            &BuildCancellation::new(),
        )
        .unwrap();
        println!(
            "{profile:?} coastal={coastal} month={month}: imex={:?} imex_failure={:?} imex_residual={} split={:?} selection={:?}",
            report.imex().agreement(),
            report.imex().integration_failure(),
            report.imex().final_linear_relative_residual(),
            report.split_explicit().agreement(),
            report.selection()
        );
        imex_all &= report.imex().qualifies();
        split_all &= report.split_explicit().qualifies();
        assert!(report.imex().integration_failure().is_some());
        assert!(report.imex().final_linear_relative_residual() > 1.0e-6);
    }
    assert!(
        imex_all || split_all,
        "no production candidate passed every locked corpus case"
    );
    assert!(
        split_all,
        "selected split-explicit candidate failed the corpus"
    );
    assert!(
        !imex_all,
        "IMEX unexpectedly passed every locked corpus case"
    );
    assert_eq!(
        SELECTED_PRODUCTION_INTEGRATOR,
        ProductionIntegratorId::SplitExplicitRk3V1
    );
}
