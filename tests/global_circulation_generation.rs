mod support;

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    ClimateWorkDomainBuilder, GlobalCirculationGenerationError, GlobalCirculationGenerator,
    GlobalCirculationPhase, GlobalClimateForcingBuilder, SELECTED_PRODUCTION_INTEGRATOR,
};
use sekai::world::natural::{
    expected_global_circulation_dense_state_bytes, ClimateCapabilityAvailability,
    ClimateCapabilityId, ClimateLayerRole, ClimateModelProfile, ClimateWorkDomainSnapshot,
    NaturalQualityProfile, GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2,
    GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX,
};
use sekai::world::spatial::{ConservativeSurfaceMap, SurfaceOverlapWeight, TangentTransform};

use support::global_circulation::global_circulation_fixture;

#[test]
fn c2_generation_publishes_every_semantic_field_and_exact_component_identity() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let snapshot = GlobalCirculationGenerator::generate(
        surface,
        &fixture.domain,
        &fixture.forcing,
        ClimateModelProfile::C2LayeredV1,
        &BuildCancellation::new(),
    )
    .unwrap();
    snapshot.validate_against(surface).unwrap();
    assert_eq!(snapshot.integrator(), SELECTED_PRODUCTION_INTEGRATOR);
    assert_eq!(snapshot.profile(), ClimateModelProfile::C2LayeredV1);
    assert_eq!(snapshot.fields().cell_count(), surface.cells().len());
    assert_eq!(
        snapshot.solve_report().dense_state_bytes(),
        expected_global_circulation_dense_state_bytes(
            NaturalQualityProfile::Draft,
            ClimateModelProfile::C2LayeredV1,
            surface.cells().len() as u32,
        )
        .unwrap(),
        "the report must reuse the production dense-owner inventory"
    );
    assert!(snapshot.fields().upper_wind_m_s().is_some());
    assert!(snapshot.fields().vertical_wind_shear_m_s().is_some());
    assert!(snapshot
        .fields()
        .monthly_thermocline_temperature_c()
        .is_some());
    assert!(snapshot.fields().monthly_thermocline_depth_m().is_some());
    assert!(snapshot
        .fields()
        .monthly_deep_ocean_temperature_c()
        .is_some());
    assert_eq!(
        snapshot.fields().surface_albedo().len(),
        surface.cells().len()
    );
    assert!(snapshot
        .fields()
        .surface_albedo()
        .iter()
        .all(|value| (0.0..=1.0).contains(value)));
    for field in [
        snapshot.fields().monthly_absorbed_shortwave_w_m2(),
        snapshot.fields().monthly_outgoing_longwave_w_m2(),
        snapshot.fields().monthly_evaporation_mm_day(),
    ] {
        assert_eq!(field.len(), surface.cells().len());
        assert!(field
            .values()
            .iter()
            .flatten()
            .all(|value| value.is_finite() && *value >= 0.0));
    }
    assert_eq!(
        snapshot
            .capabilities()
            .availability(ClimateCapabilityId::VerticalStructureV1),
        ClimateCapabilityAvailability::Available
    );
    for unavailable in [
        ClimateCapabilityId::SeaIceV1,
        ClimateCapabilityId::LandSurfaceFeedbackV1,
        ClimateCapabilityId::EquatorialVariabilityV1,
        ClimateCapabilityId::TropicalCycloneClimatologyV1,
    ] {
        assert_eq!(
            snapshot.capabilities().availability(unavailable),
            ClimateCapabilityAvailability::Unavailable
        );
    }

    let lower = snapshot.fields().near_surface_wind_m_s().values();
    let upper = snapshot.fields().upper_wind_m_s().unwrap().values();
    let shear = snapshot
        .fields()
        .vertical_wind_shear_m_s()
        .unwrap()
        .values();
    for cell in 0..lower.len() {
        for month in 0..12 {
            for component in 0..3 {
                assert_eq!(
                    shear[cell][month][component],
                    upper[cell][month][component] - lower[cell][month][component]
                );
            }
        }
    }

    let thermocline_depth = snapshot
        .fields()
        .monthly_thermocline_depth_m()
        .unwrap()
        .values();
    let thermocline_height = snapshot
        .fields()
        .monthly_thermocline_height_anomaly_m()
        .unwrap()
        .values();
    let reference_depth = snapshot
        .layout()
        .layers()
        .iter()
        .find(|layer| layer.role() == ClimateLayerRole::OceanThermocline)
        .unwrap()
        .reference_thickness_m() as f32;
    for cell in 0..thermocline_depth.len() {
        for month in 0..12 {
            assert_eq!(
                thermocline_depth[cell][month],
                reference_depth + thermocline_height[cell][month]
            );
        }
    }

    assert_eq!(
        snapshot.checkpoint().completed_phase_steps(),
        u32::from(snapshot.solve_report().formation_cycles()) * 12
    );
    assert_eq!(
        snapshot.solve_report().integrated_model_seconds(),
        snapshot.solve_report().continuation_steps()
            * sekai::world::natural::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS as u64
    );
    assert_eq!(
        snapshot.checkpoint().state_fingerprint(),
        &snapshot.fields().fingerprint()
    );
    assert!(
        snapshot
            .remap_report()
            .published_precipitation_relative_error()
            <= sekai::world::natural::GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX
    );

    let currents = snapshot.fields().surface_ocean_current_m_s().values();
    for (cell, &ocean_fraction) in fixture
        .relief
        .surface_water_geometry()
        .ocean_area_fraction()
        .iter()
        .enumerate()
    {
        if ocean_fraction == 0.0 {
            for current in &currents[cell] {
                assert_eq!(*current, [0.0; 3]);
            }
        }
    }
}

#[test]
fn public_generator_rejects_noncanonical_remap_even_when_its_fingerprint_changes() {
    let fixture = global_circulation_fixture();
    let original_map = fixture.domain.climate_to_source();
    let mut weights = original_map.weights().to_vec();
    let first = weights[0];
    let mut coefficients = first.tangent_transform().coefficients();
    coefficients[0] *= 0.999;
    weights[0] = SurfaceOverlapWeight::new(
        first.source_cell(),
        first.area_m2(),
        TangentTransform::new(coefficients).unwrap(),
    )
    .unwrap();
    let stats = original_map.solve_stats();
    let changed_reverse = ConservativeSurfaceMap::new(
        original_map.schema_version(),
        original_map.source_ref(),
        original_map.target_ref(),
        original_map.source_cell_areas_m2().to_vec(),
        original_map.target_cell_areas_m2().to_vec(),
        original_map.target_row_offsets().to_vec(),
        weights,
        stats.balance_iterations(),
        stats.max_relative_geometric_adjustment(),
    )
    .unwrap();
    let changed_domain = ClimateWorkDomainSnapshot::new(
        fixture.domain.schema_version(),
        fixture.domain.profile(),
        fixture.domain.face_resolution(),
        fixture.domain.source_ref(),
        *fixture.domain.climate_grid_fingerprint(),
        fixture.domain.climate_surface().clone(),
        fixture.domain.source_to_climate().clone(),
        changed_reverse,
    )
    .unwrap();
    assert_ne!(fixture.domain.fingerprint(), changed_domain.fingerprint());

    let surface = fixture.bundle.authoritative_surface();
    assert!(changed_domain.validate_against(surface).is_err());
    assert!(
        GlobalCirculationGenerator::generate(
            surface,
            &changed_domain,
            &fixture.forcing,
            ClimateModelProfile::C1SingleLayerV1,
            &BuildCancellation::new(),
        )
        .is_err(),
        "a changed map fingerprint identifies the forgery; it does not legalize it"
    );
}

#[test]
fn formation_is_convergent_budgeted_causal_and_deterministic() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let run = || {
        GlobalCirculationGenerator::generate(
            surface,
            &fixture.domain,
            &fixture.forcing,
            ClimateModelProfile::C2LayeredV1,
            &BuildCancellation::new(),
        )
        .unwrap()
    };
    let first = run();
    let second = run();
    assert_eq!(first, second);
    println!(
        "solve={:?} budget={:?}",
        first.solve_report(),
        first.budget_report()
    );
    assert!(first.solve_report().formation_cycles() > 0);
    assert!(first.solve_report().continuation_steps() >= 12);
    assert!(first.solve_report().fast_substeps() >= first.solve_report().continuation_steps());
    assert!(
        first.solve_report().final_residual() <= 0.25,
        "formation residual {}",
        first.solve_report().final_residual()
    );
    first.budget_report().validate().unwrap();
    let budget = first.budget_report();
    assert!(budget.evaporation_global_mean_mm_day() >= 0.0);
    assert!(budget.precipitation_global_mean_mm_day() >= 0.0);
    assert!(
        budget.evaporation_precipitation_relative_imbalance()
            <= GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX
    );
    assert!(budget.absorbed_shortwave_global_mean_w_m2() >= 0.0);
    assert!(budget.outgoing_longwave_global_mean_w_m2() >= 0.0);
    assert!(budget.planetary_albedo_global_mean() >= 0.0);
    assert!(budget.planetary_albedo_global_mean() <= 1.0);
    assert!(
        budget.toa_net_radiation_global_mean_w_m2().abs()
            <= GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2
    );
    assert!(
        (budget.toa_net_radiation_global_mean_w_m2()
            - (budget.absorbed_shortwave_global_mean_w_m2()
                - budget.outgoing_longwave_global_mean_w_m2()))
        .abs()
            <= 1.0e-12
    );

    let fields = first.fields();
    let evaporation = fields.monthly_evaporation_mm_day().values();
    let precipitation = fields.monthly_precipitation_mm_day().values();
    let orographic = fields.monthly_orographic_precipitation_mm_day().values();
    assert!(evaporation.iter().flatten().any(|value| *value > 0.0));
    assert!(precipitation.iter().flatten().any(|value| *value > 0.01));
    assert!(orographic.iter().flatten().any(|value| *value > 0.01));
    for (total, orographic) in precipitation.iter().zip(orographic) {
        for month in 0..12 {
            assert!(orographic[month] <= total[month]);
        }
    }
    let mixed = fields.monthly_sea_surface_temperature_c().values();
    let thermocline = fields.monthly_thermocline_temperature_c().unwrap().values();
    assert!(mixed.iter().zip(thermocline).any(|(mixed, thermocline)| {
        mixed
            .iter()
            .zip(thermocline)
            .any(|(mixed, thermocline)| mixed > thermocline)
    }));
    assert!(fields
        .monthly_thermocline_depth_m()
        .unwrap()
        .values()
        .iter()
        .flatten()
        .all(|value| *value > 0.0));
    assert!(first
        .fields()
        .near_surface_wind_m_s()
        .values()
        .iter()
        .flatten()
        .any(|vector| vector.iter().any(|value| value.abs() > 0.05)));
    assert!(first
        .fields()
        .surface_ocean_current_m_s()
        .values()
        .iter()
        .flatten()
        .any(|vector| vector.iter().any(|value| value.abs() > 0.001)));

    for role in [
        ClimateLayerRole::LowerAtmosphere,
        ClimateLayerRole::UpperAtmosphere,
        ClimateLayerRole::OceanMixedLayer,
        ClimateLayerRole::OceanThermocline,
    ] {
        assert!(first
            .layout()
            .layers()
            .iter()
            .any(|layer| layer.role() == role));
    }
}

#[test]
fn pre_cancelled_generation_publishes_no_partial_snapshot() {
    let fixture = global_circulation_fixture();
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    assert_eq!(
        GlobalCirculationGenerator::generate(
            fixture.bundle.authoritative_surface(),
            &fixture.domain,
            &fixture.forcing,
            ClimateModelProfile::C2LayeredV1,
            &cancellation,
        ),
        Err(GlobalCirculationGenerationError::Cancelled)
    );
}

#[test]
fn active_cancellation_is_synchronized_after_completed_solver_work_units() {
    let fixture = global_circulation_fixture();
    for phase in [
        GlobalCirculationPhase::TransportCompleted,
        GlobalCirculationPhase::FastSubstepCompleted,
        GlobalCirculationPhase::ProjectionFieldCompleted,
        GlobalCirculationPhase::PublicationStarted,
        GlobalCirculationPhase::StateFingerprintCompleted,
    ] {
        let cancellation = BuildCancellation::new();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let latency = std::thread::scope(|scope| {
            let worker_tx = entered_tx.clone();
            let surface = fixture.bundle.authoritative_surface();
            let domain = &fixture.domain;
            let forcing = &fixture.forcing;
            let cancellation_ref = &cancellation;
            let worker = scope.spawn(move || {
                let mut triggered = false;
                GlobalCirculationGenerator::generate_with_phase_observer(
                    surface,
                    domain,
                    forcing,
                    ClimateModelProfile::C2LayeredV1,
                    cancellation_ref,
                    |observed| {
                        if observed == phase && !triggered {
                            triggered = true;
                            worker_tx.send(()).unwrap();
                        }
                    },
                )
            });
            drop(entered_tx);
            entered_rx
                .recv()
                .unwrap_or_else(|_| panic!("solver never entered {phase:?}"));
            // The observer fires only after a real work unit. Wait until the
            // following unit has itself crossed several cooperative polls;
            // cancellation is therefore requested from inside active work,
            // not at the observer boundary.
            let observations = cancellation.observation_count();
            let progress_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while cancellation.observation_count() < observations + 4 {
                assert!(
                    std::time::Instant::now() < progress_deadline,
                    "solver stopped polling after {phase:?}"
                );
                std::thread::yield_now();
            }
            let started = std::time::Instant::now();
            cancellation.cancel();
            let result = worker.join().unwrap();
            assert_eq!(result, Err(GlobalCirculationGenerationError::Cancelled));
            started.elapsed()
        });
        assert!(
            latency <= std::time::Duration::from_millis(250),
            "{phase:?} cancellation took {latency:?}"
        );
    }
}

#[test]
fn c2_cross_resolution_climatology_is_statistically_stable() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let cancellation = BuildCancellation::new();
    let draft = GlobalCirculationGenerator::generate(
        surface,
        &fixture.domain,
        &fixture.forcing,
        ClimateModelProfile::C2LayeredV1,
        &cancellation,
    )
    .unwrap();
    let standard_domain =
        ClimateWorkDomainBuilder::build(surface, NaturalQualityProfile::Standard, &cancellation)
            .unwrap();
    let standard_forcing = GlobalClimateForcingBuilder::build(
        surface,
        &fixture.relief,
        &sekai::world::natural::ClimateSpec::default(),
        &standard_domain,
        &cancellation,
    )
    .unwrap();
    let standard = GlobalCirculationGenerator::generate(
        surface,
        &standard_domain,
        &standard_forcing,
        ClimateModelProfile::C2LayeredV1,
        &cancellation,
    )
    .unwrap();

    let draft_fields = draft.fields();
    let standard_fields = standard.fields();
    let comparisons = [
        (
            "air-temperature",
            scalar_mean(surface, draft_fields.monthly_air_temperature_c().values()),
            scalar_mean(
                surface,
                standard_fields.monthly_air_temperature_c().values(),
            ),
            2.0,
            false,
        ),
        (
            "specific-humidity",
            scalar_mean(surface, draft_fields.monthly_specific_humidity().values()),
            scalar_mean(
                surface,
                standard_fields.monthly_specific_humidity().values(),
            ),
            0.25,
            true,
        ),
        (
            "precipitation",
            scalar_mean(
                surface,
                draft_fields.monthly_precipitation_mm_day().values(),
            ),
            scalar_mean(
                surface,
                standard_fields.monthly_precipitation_mm_day().values(),
            ),
            0.30,
            true,
        ),
        (
            "near-surface-wind-rms",
            vector_rms(surface, draft_fields.near_surface_wind_m_s().values()),
            vector_rms(surface, standard_fields.near_surface_wind_m_s().values()),
            0.35,
            true,
        ),
        (
            "ocean-current-rms",
            vector_rms(surface, draft_fields.surface_ocean_current_m_s().values()),
            vector_rms(
                surface,
                standard_fields.surface_ocean_current_m_s().values(),
            ),
            0.45,
            true,
        ),
    ];
    for (name, draft_value, standard_value, tolerance, relative) in comparisons {
        let difference = if relative {
            (standard_value - draft_value).abs() / draft_value.abs().max(1.0e-9)
        } else {
            (standard_value - draft_value).abs()
        };
        println!(
            "{name}: draft={draft_value:.9} standard={standard_value:.9} difference={difference:.9}"
        );
        assert!(
            difference <= tolerance,
            "{name} cross-resolution difference {difference} exceeds {tolerance}"
        );
    }
}

fn scalar_mean(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    values: &[[f32; 12]],
) -> f64 {
    let total_area = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    surface
        .cells()
        .iter()
        .zip(values)
        .map(|(cell, months)| {
            cell.area.get() * months.iter().map(|value| f64::from(*value)).sum::<f64>() / 12.0
        })
        .sum::<f64>()
        / total_area
}

fn vector_rms(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    values: &[[[f32; 3]; 12]],
) -> f64 {
    let total_area = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    (surface
        .cells()
        .iter()
        .zip(values)
        .map(|(cell, months)| {
            cell.area.get()
                * months
                    .iter()
                    .map(|vector| {
                        vector
                            .iter()
                            .map(|value| f64::from(*value).powi(2))
                            .sum::<f64>()
                    })
                    .sum::<f64>()
                / 12.0
        })
        .sum::<f64>()
        / total_area)
        .sqrt()
}
