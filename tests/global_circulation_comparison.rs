use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::{
    annual_precipitation_total_bias, compare_climate_states,
    compare_formation_procedure_identities, formation_procedure_identity_matches,
    global_circulation_model_fingerprint, run_closed_split_annual_mass_fixture,
    run_formation_cycle_comparison, run_integrator_comparison, AnnualLayerMassConservationReport,
    ClimateAgreementThresholds, ClimateConservationInterpretation, ExplicitRk3Integrator,
    FormationProcedureIdentity, ImexCrankNicolsonIntegrator, LayeredClimateState,
    ProductionCandidateSelection, SplitExplicitRk3Integrator, SELECTED_PRODUCTION_INTEGRATOR,
};
use sekai::world::natural::{
    ClimateCapabilitySet, ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
    ProductionIntegratorId,
};
use serde::Serialize;

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
        vec![[240.0; 12]; count],
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
    for (cell, humidity) in grid.cells().iter().zip(state.specific_humidity_mut()) {
        *humidity = (0.008 + 0.003 * cell.center_unit()[0] as f32).max(0.000_1);
    }
    if let Some(upper_humidity) = state.upper_specific_humidity_mut() {
        for (cell, humidity) in grid.cells().iter().zip(upper_humidity) {
            *humidity = (0.0028 + 0.0008 * cell.center_unit()[1] as f32).max(0.000_1);
        }
    }
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
        vec![[240.0; 12]; count],
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
    for (cell, humidity) in grid.cells().iter().zip(state.specific_humidity_mut()) {
        *humidity = (0.008 + 0.003 * cell.center_unit()[0] as f32).max(0.000_1);
    }
    if let Some(upper_humidity) = state.upper_specific_humidity_mut() {
        for (cell, humidity) in grid.cells().iter().zip(upper_humidity) {
            *humidity = (0.0028 + 0.0008 * cell.center_unit()[1] as f32).max(0.000_1);
        }
    }
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
    assert!(comparison.failures().iter().any(|failure| {
        failure.field() == "lower_atmosphere_wind" && failure.metric().contains("vector")
    }));
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
    for (profile, coastal) in [
        (ClimateModelProfile::C1SingleLayerV1, false),
        (ClimateModelProfile::C1SingleLayerV1, true),
        (ClimateModelProfile::C2LayeredV1, false),
        (ClimateModelProfile::C2LayeredV1, true),
    ] {
        let grid = CubedSphereGrid::new(3, 6_371_000.0).unwrap();
        let (forcing, initial, permeability) = layered_fixture(&grid, profile, coastal);
        let mut reports = Vec::new();
        for month in 0..12 {
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
            assert!(report
                .split_explicit()
                .agreement()
                .unwrap()
                .precipitation()
                .is_some());
            reports.push(report);
        }
        let annual_bias =
            annual_precipitation_total_bias(&reports, ProductionIntegratorId::SplitExplicitRk3V1)
                .unwrap();
        assert!(
            annual_bias
                <= ClimateAgreementThresholds::LOCKED
                    .maximum_annual_precipitation_total_bias_fraction(),
            "{profile:?} coastal={coastal} annual precipitation bias {annual_bias}"
        );
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

#[test]
fn formation_capability_and_conservation_identity_are_locked_for_every_fixture() {
    for (profile, coastal) in [
        (ClimateModelProfile::C1SingleLayerV1, false),
        (ClimateModelProfile::C1SingleLayerV1, true),
        (ClimateModelProfile::C2LayeredV1, false),
        (ClimateModelProfile::C2LayeredV1, true),
    ] {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let (forcing, initial, permeability) = layered_fixture(&grid, profile, coastal);
        let report = run_formation_cycle_comparison(
            &grid,
            &initial,
            &forcing,
            &permeability,
            8,
            7_200.0,
            300.0,
            &BuildCancellation::new(),
        )
        .unwrap();
        assert!(report.reference().cycles().is_some());
        assert!(
            report.split_explicit_cycle_match(),
            "{profile:?} coastal={coastal}: reference={:?} split={:?}",
            report.reference(),
            report.split_explicit()
        );
        assert!(report.capability_set_match());
        assert!(report.conservation_interpretation_match());
        assert!(report.model_fingerprint_match());
    }
}

#[test]
fn formation_procedure_identity_gate_rejects_capability_and_budget_mismatches() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let profile = ClimateModelProfile::C2LayeredV1;
    let expected = ExplicitRk3Integrator::new(&grid).formation_procedure_identity(profile);
    let imex = ImexCrankNicolsonIntegrator::new(&grid, 32, 1.0e-6)
        .unwrap()
        .formation_procedure_identity(profile);
    let split = SplitExplicitRk3Integrator::new(&grid, 300.0)
        .unwrap()
        .formation_procedure_identity(profile);
    assert!(compare_formation_procedure_identities(&expected, &imex, &split).qualifies());

    let wrong_capabilities = FormationProcedureIdentity::new(
        ClimateCapabilitySet::for_profile(ClimateModelProfile::C1SingleLayerV1),
        ClimateConservationInterpretation::SharedTendencyExtensiveV1,
        global_circulation_model_fingerprint(profile),
    );
    let wrong_budget_semantics = FormationProcedureIdentity::new(
        ClimateCapabilitySet::for_profile(ClimateModelProfile::C2LayeredV1),
        ClimateConservationInterpretation::IntegratorInternalStateDeltaV1,
        global_circulation_model_fingerprint(profile),
    );
    let mut forged_model = global_circulation_model_fingerprint(profile);
    forged_model[0] ^= 1;
    let wrong_model = FormationProcedureIdentity::new(
        ClimateCapabilitySet::for_profile(profile),
        ClimateConservationInterpretation::SharedTendencyExtensiveV1,
        forged_model,
    );

    assert!(formation_procedure_identity_matches(&expected, &expected));
    assert!(!formation_procedure_identity_matches(
        &expected,
        &wrong_capabilities
    ));
    assert!(!formation_procedure_identity_matches(
        &expected,
        &wrong_budget_semantics
    ));
    assert!(!formation_procedure_identity_matches(
        &expected,
        &wrong_model
    ));

    let capability_mismatch =
        compare_formation_procedure_identities(&expected, &imex, &wrong_capabilities);
    assert!(!capability_mismatch.qualifies());
    assert!(!capability_mismatch.split_explicit_capability_set_match());
    let budget_mismatch =
        compare_formation_procedure_identities(&expected, &wrong_budget_semantics, &split);
    assert!(!budget_mismatch.qualifies());
    assert!(!budget_mismatch.imex_conservation_interpretation_match());
    let model_mismatch = compare_formation_procedure_identities(&expected, &wrong_model, &split);
    assert!(!model_mismatch.qualifies());
    assert!(!model_mismatch.imex_model_fingerprint_match());
}

#[test]
fn formation_comparison_rejects_invalid_time_steps_before_running() {
    let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let (forcing, initial, permeability) =
        layered_fixture(&grid, ClimateModelProfile::C2LayeredV1, true);
    let cancellation = BuildCancellation::new();
    let invalid_macro = run_formation_cycle_comparison(
        &grid,
        &initial,
        &forcing,
        &permeability,
        8,
        f64::NAN,
        300.0,
        &cancellation,
    )
    .unwrap_err();
    assert!(matches!(
        invalid_macro,
        sekai::generators::natural::ClimateIntegratorError::InvalidTimeStep { found }
            if found.is_nan()
    ));
    let invalid_reference = run_formation_cycle_comparison(
        &grid,
        &initial,
        &forcing,
        &permeability,
        8,
        7_200.0,
        0.0,
        &cancellation,
    )
    .unwrap_err();
    assert!(matches!(
        invalid_reference,
        sekai::generators::natural::ClimateIntegratorError::InvalidFastStep { found }
            if found == 0.0
    ));
}

#[derive(Serialize)]
struct ComparisonEvidence {
    schema: &'static str,
    thresholds: ClimateAgreementThresholds,
    selected: ProductionIntegratorId,
    closed_annual_layer_mass: AnnualLayerMassConservationReport,
    fixtures: Vec<FixtureComparisonEvidence>,
}

#[derive(Serialize)]
struct FixtureComparisonEvidence {
    profile: ClimateModelProfile,
    coastal: bool,
    monthly: Vec<sekai::generators::natural::IntegratorComparisonReport>,
    split_annual_precipitation_total_bias_fraction: f64,
    formation: sekai::generators::natural::FormationCycleComparisonReport,
}

fn build_comparison_evidence() -> ComparisonEvidence {
    let mut fixtures = Vec::new();
    for (profile, coastal) in [
        (ClimateModelProfile::C1SingleLayerV1, false),
        (ClimateModelProfile::C1SingleLayerV1, true),
        (ClimateModelProfile::C2LayeredV1, false),
        (ClimateModelProfile::C2LayeredV1, true),
    ] {
        let grid = CubedSphereGrid::new(3, 6_371_000.0).unwrap();
        let (forcing, initial, permeability) = layered_fixture(&grid, profile, coastal);
        let monthly = (0..12)
            .map(|month| {
                run_integrator_comparison(
                    &grid,
                    &initial,
                    &forcing,
                    &permeability,
                    month,
                    21_600.0,
                    300.0,
                    &BuildCancellation::new(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let annual_bias =
            annual_precipitation_total_bias(&monthly, ProductionIntegratorId::SplitExplicitRk3V1)
                .unwrap();
        let formation = run_formation_cycle_comparison(
            &grid,
            &initial,
            &forcing,
            &permeability,
            8,
            7_200.0,
            300.0,
            &BuildCancellation::new(),
        )
        .unwrap();
        fixtures.push(FixtureComparisonEvidence {
            profile,
            coastal,
            monthly,
            split_annual_precipitation_total_bias_fraction: annual_bias,
            formation,
        });
    }
    let conservation_grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
    let evidence = ComparisonEvidence {
        schema: "sekai.p4-integrator-comparison.v1",
        thresholds: ClimateAgreementThresholds::LOCKED,
        selected: SELECTED_PRODUCTION_INTEGRATOR,
        closed_annual_layer_mass: run_closed_split_annual_mass_fixture(
            &conservation_grid,
            &BuildCancellation::new(),
        )
        .unwrap(),
        fixtures,
    };
    assert_comparison_evidence_passes_locked_gates(&evidence);
    evidence
}

fn assert_comparison_evidence_passes_locked_gates(evidence: &ComparisonEvidence) {
    assert_eq!(
        evidence.selected,
        ProductionIntegratorId::SplitExplicitRk3V1
    );
    assert_eq!(evidence.closed_annual_layer_mass.months(), 12);
    assert_eq!(evidence.closed_annual_layer_mass.layers().len(), 4);
    assert!(
        evidence
            .closed_annual_layer_mass
            .maximum_absolute_height_change_m()
            > 0.0
    );
    assert!(
        evidence
            .closed_annual_layer_mass
            .maximum_relative_mass_drift()
            <= 1.0e-8
    );
    for fixture in &evidence.fixtures {
        assert_eq!(fixture.monthly.len(), 12);
        for report in &fixture.monthly {
            assert!(
                report.split_explicit().qualifies(),
                "{:?} coastal={} month={} split candidate failed: {:?}",
                fixture.profile,
                fixture.coastal,
                report.month(),
                report.split_explicit().agreement()
            );
            assert_eq!(
                report.selection(),
                ProductionCandidateSelection::Selected(ProductionIntegratorId::SplitExplicitRk3V1),
                "{:?} coastal={} month={} selected the wrong candidate",
                fixture.profile,
                fixture.coastal,
                report.month()
            );
        }
        assert!(
            fixture.split_annual_precipitation_total_bias_fraction
                <= evidence
                    .thresholds
                    .maximum_annual_precipitation_total_bias_fraction(),
            "{:?} coastal={} annual precipitation bias {}",
            fixture.profile,
            fixture.coastal,
            fixture.split_annual_precipitation_total_bias_fraction
        );
        assert!(fixture.formation.reference().cycles().is_some());
        assert!(fixture.formation.split_explicit_cycle_match());
        assert!(fixture.formation.imex_capability_set_match());
        assert!(fixture.formation.split_explicit_capability_set_match());
        assert!(fixture.formation.imex_conservation_interpretation_match());
        assert!(fixture
            .formation
            .split_explicit_conservation_interpretation_match());
        assert!(fixture.formation.capability_set_match());
        assert!(fixture.formation.conservation_interpretation_match());
        assert!(fixture.formation.imex_model_fingerprint_match());
        assert!(fixture.formation.split_explicit_model_fingerprint_match());
        assert!(fixture.formation.model_fingerprint_match());
    }
}

#[test]
#[ignore = "writes deterministic P4 comparison evidence"]
fn write_global_circulation_comparison_evidence() {
    let evidence = build_comparison_evidence();
    let mut bytes = serde_json::to_vec_pretty(&evidence).unwrap();
    bytes.push(b'\n');
    let directory = std::path::Path::new("target/p4");
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(directory.join("integrator-comparison.json"), &bytes).unwrap();
    eprintln!(
        "P4 comparison bytes={} hash={}",
        bytes.len(),
        blake3::hash(&bytes).to_hex()
    );
}
