mod support;

use std::time::{Duration, Instant};

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{SurfaceFormationGenerationError, SurfaceFormationGenerator};
use sekai::world::natural::{
    expected_surface_formation_dense_state_bytes, formation_elevation_from_components,
    SurfaceFormationCapabilityAvailability, SurfaceFormationCapabilityId, SurfaceFormationModelId,
    SURFACE_FORMATION_HORIZON_YEARS, SURFACE_FORMATION_MACRO_STEPS,
    SURFACE_FORMATION_MAX_OUTER_ITERATIONS,
};

use support::surface_formation::{published_formation, surface_formation_fixture};

const MILLIMETERS_PER_METER: f64 = 1_000.0;

#[test]
fn the_bounded_fixed_point_publishes_one_converged_atomic_product() {
    let fixture = surface_formation_fixture();
    let snapshot = published_formation();
    snapshot
        .validate_against(fixture.upstream.bundle.authoritative_surface())
        .unwrap();

    let report = snapshot.solve_report();
    assert!(report.converged());
    assert!(report.outer_iterations() >= 1);
    assert!(report.outer_iterations() <= SURFACE_FORMATION_MAX_OUTER_ITERATIONS);
    assert_eq!(
        report.geomorphic_macro_steps(),
        u16::from(report.outer_iterations()) * SURFACE_FORMATION_MACRO_STEPS
    );
    assert_eq!(
        report.outer_iterations(),
        snapshot.checkpoint().outer_iterations()
    );
    assert_eq!(
        snapshot.checkpoint().model(),
        SurfaceFormationModelId::PriorityFloodFastscapeSedimentHillslopeCoastIsostasyV1
    );
    for available in [
        SurfaceFormationCapabilityId::PriorityFloodHydrologyV2,
        SurfaceFormationCapabilityId::ImplicitStreamPowerV1,
        SurfaceFormationCapabilityId::NonlinearHillslopeTransportV1,
        SurfaceFormationCapabilityId::ProvenanceSedimentV1,
        SurfaceFormationCapabilityId::CoastalIsostaticResponseV1,
    ] {
        assert_eq!(
            snapshot.capabilities().availability(available),
            SurfaceFormationCapabilityAvailability::Available
        );
    }
    for unavailable in [
        SurfaceFormationCapabilityId::ExplicitEvapotranspirationV1,
        SurfaceFormationCapabilityId::GroundwaterFlowV1,
        SurfaceFormationCapabilityId::GlacialErosionV1,
    ] {
        assert_eq!(
            snapshot.capabilities().availability(unavailable),
            SurfaceFormationCapabilityAvailability::Unavailable
        );
    }
}

#[test]
fn the_exact_component_sum_reconstructs_every_published_elevation() {
    let snapshot = published_formation();
    let terrain = snapshot.terrain_fields();
    let components = terrain.elevation_components();
    for index in 0..terrain.final_elevation_m().len() {
        let expected = formation_elevation_from_components(
            components.primary_elevation_m()[index],
            components.tectonic_displacement_m()[index],
            components.fluvial_erosion_m()[index],
            components.hillslope_erosion_m()[index],
            components.hillslope_deposition_m()[index],
            components.routed_sediment_deposition_m()[index],
            components.coastal_erosion_m()[index],
            components.coastal_deposition_m()[index],
            components.isostatic_response_m()[index],
        );
        assert_eq!(
            terrain.final_elevation_m()[index].to_bits(),
            expected.to_bits(),
            "cell {index} does not reconstruct from its retained components"
        );
    }
    for (field, values) in [
        ("fluvial_erosion_m", components.fluvial_erosion_m()),
        ("hillslope_erosion_m", components.hillslope_erosion_m()),
        (
            "hillslope_deposition_m",
            components.hillslope_deposition_m(),
        ),
        (
            "routed_sediment_deposition_m",
            components.routed_sediment_deposition_m(),
        ),
        ("coastal_erosion_m", components.coastal_erosion_m()),
        ("coastal_deposition_m", components.coastal_deposition_m()),
    ] {
        assert!(
            values
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0),
            "{field} published a negative or non-finite magnitude"
        );
    }
    assert!(
        components
            .fluvial_erosion_m()
            .iter()
            .chain(components.hillslope_erosion_m())
            .any(|value| *value > 0.0),
        "the solve produced no erosion at all"
    );
}

#[test]
fn every_outer_iteration_restarts_from_the_immutable_primary_relief() {
    let fixture = surface_formation_fixture();
    let snapshot = published_formation();
    let components = snapshot.terrain_fields().elevation_components();
    let relief = &fixture.upstream.relief;
    assert_eq!(
        components.primary_elevation_m().len(),
        relief.elevation_m().len()
    );
    for (index, (published, primary)) in components
        .primary_elevation_m()
        .iter()
        .zip(relief.elevation_m())
        .enumerate()
    {
        assert_eq!(
            published.to_bits(),
            primary.to_bits(),
            "cell {index} replaced the immutable P3 primary elevation"
        );
    }

    // Three outer iterations must integrate one 100,000 yr horizon, not three.
    let forcing = fixture.upstream.evolved.forcing();
    assert!(snapshot.solve_report().outer_iterations() > 1);
    for (index, displacement) in components.tectonic_displacement_m().iter().enumerate() {
        let net_rate_m_per_year = (f64::from(forcing.uplift_rate_mm_per_year()[index])
            - f64::from(forcing.subsidence_rate_mm_per_year()[index]))
            / MILLIMETERS_PER_METER;
        let horizon_limit = net_rate_m_per_year.abs() * SURFACE_FORMATION_HORIZON_YEARS;
        assert!(
            f64::from(displacement.abs()) <= horizon_limit + 1.0e-3,
            "cell {index} displaced {displacement} m beyond the {horizon_limit} m horizon limit"
        );
    }
}

#[test]
fn the_production_climate_is_rebuilt_on_the_candidate_formation_terrain() {
    let fixture = surface_formation_fixture();
    let snapshot = published_formation();
    let published = snapshot.formation_climate().checkpoint();
    let frozen = fixture.initial_climate.checkpoint();

    assert_eq!(
        snapshot.formation_climate().profile(),
        fixture.initial_climate.profile()
    );
    assert_eq!(
        snapshot.formation_climate().integrator(),
        fixture.initial_climate.integrator()
    );
    assert_eq!(published.model_fingerprint(), frozen.model_fingerprint());
    assert_eq!(published.grid_fingerprint(), frozen.grid_fingerprint());
    assert_ne!(
        published.forcing_fingerprint(),
        frozen.forcing_fingerprint()
    );
    assert_ne!(published.input_fingerprint(), frozen.input_fingerprint());
    assert_ne!(published.state_fingerprint(), frozen.state_fingerprint());
    assert_eq!(
        snapshot.hydrology().surface_ref(),
        snapshot.formation_climate().surface_ref()
    );
}

#[test]
fn every_residual_component_is_reported_and_the_last_one_closes() {
    let snapshot = published_formation();
    let report = snapshot.solve_report();
    assert_eq!(report.residuals().len(), report.outer_iterations() as usize);
    for residual in report.residuals() {
        assert!(residual.elevation_rms_m().is_finite() && residual.elevation_rms_m() >= 0.0);
        assert!((0.0..=1.0).contains(&residual.receiver_changed_fraction()));
        assert!(residual.log_discharge_rms().is_finite() && residual.log_discharge_rms() >= 0.0);
        assert!(
            residual.sediment_thickness_rms_m().is_finite()
                && residual.sediment_thickness_rms_m() >= 0.0
        );
        assert!((0.0..=1.0).contains(&residual.coastline_area_changed_fraction()));
    }
    assert!(report.final_residual().normalized_max() <= 1.0);
    if report.outer_iterations() > 1 {
        let first = report.residuals()[0].normalized_max();
        assert!(
            first > report.final_residual().normalized_max(),
            "the fixed point did not reduce its normalized residual"
        );
    }
}

#[test]
fn the_solve_reports_its_conservative_dense_owner_inventory() {
    let fixture = surface_formation_fixture();
    let surface = fixture.upstream.bundle.authoritative_surface();
    let snapshot = published_formation();
    let expected = expected_surface_formation_dense_state_bytes(
        surface.cells().len() as u32,
        surface.edges().len() as u32,
    )
    .unwrap();
    assert_eq!(snapshot.solve_report().dense_state_bytes(), expected);
    let published_terrain_bytes = surface.cells().len() as u64 * 10 * size_of::<f32>() as u64;
    assert!(snapshot.solve_report().dense_state_bytes() > published_terrain_bytes);
}

#[test]
fn a_reduced_iteration_budget_fails_deterministically_without_publishing() {
    let inputs = surface_formation_fixture().inputs();
    let first = SurfaceFormationGenerator::generate_with_outer_iteration_limit(
        inputs,
        1,
        &BuildCancellation::new(),
    );
    let second = SurfaceFormationGenerator::generate_with_outer_iteration_limit(
        inputs,
        1,
        &BuildCancellation::new(),
    );
    let (first_iterations, first_residual) = match first {
        Err(SurfaceFormationGenerationError::NotConverged {
            outer_iterations,
            normalized_residual,
        }) => (outer_iterations, normalized_residual),
        other => panic!("a single outer iteration must not converge: {other:?}"),
    };
    let (second_iterations, second_residual) = match second {
        Err(SurfaceFormationGenerationError::NotConverged {
            outer_iterations,
            normalized_residual,
        }) => (outer_iterations, normalized_residual),
        other => panic!("a single outer iteration must not converge: {other:?}"),
    };
    assert_eq!(first_iterations, 1);
    assert_eq!(second_iterations, 1);
    assert!(first_residual > 1.0);
    assert_eq!(
        first_residual.to_bits(),
        second_residual.to_bits(),
        "one complete outer iteration is not deterministic"
    );

    assert!(matches!(
        SurfaceFormationGenerator::generate_with_outer_iteration_limit(
            inputs,
            0,
            &BuildCancellation::new()
        ),
        Err(SurfaceFormationGenerationError::InvalidIterationLimit { .. })
    ));
    assert!(matches!(
        SurfaceFormationGenerator::generate_with_outer_iteration_limit(
            inputs,
            SURFACE_FORMATION_MAX_OUTER_ITERATIONS + 1,
            &BuildCancellation::new()
        ),
        Err(SurfaceFormationGenerationError::InvalidIterationLimit { .. })
    ));
}

#[test]
fn active_cancellation_stops_the_solve_and_publishes_nothing() {
    let inputs = surface_formation_fixture().inputs();
    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker =
        std::thread::spawn(move || SurfaceFormationGenerator::generate(inputs, &worker_signal));
    let deadline = Instant::now() + Duration::from_secs(30);
    while signal.observation_count() < 4_096 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(signal.observation_count() >= 4_096);
    signal.cancel();
    let result = worker.join().unwrap();
    assert!(matches!(
        result,
        Err(SurfaceFormationGenerationError::Cancelled)
    ));
}

#[test]
#[ignore = "release-only byte-identical repeat of the complete P5 formation solve"]
fn repeated_complete_solves_are_byte_identical() {
    let inputs = surface_formation_fixture().inputs();
    let first = SurfaceFormationGenerator::generate(inputs, &BuildCancellation::new()).unwrap();
    let second = SurfaceFormationGenerator::generate(inputs, &BuildCancellation::new()).unwrap();
    assert_eq!(
        first.checkpoint().state_fingerprint(),
        second.checkpoint().state_fingerprint()
    );
    assert_eq!(
        first.checkpoint().fingerprint(),
        second.checkpoint().fingerprint()
    );
    assert!(first == second);
}
