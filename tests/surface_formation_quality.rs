mod support;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_surface_formation_quality, FormationHydrologyGenerator, PrimaryReliefGenerator,
    QualityBuildError,
};
use sekai::world::natural::{
    surface_formation_state_fingerprint, FormationElevationComponents, FormationProcessRates,
    FormationResiduals, FormationSedimentFields, FormationSolveReport, FormationTerrainFields,
    HydroErosionSpec, NaturalQualityProfile, NaturalSurfaceFormationSnapshot, ReliefSpec,
    SedimentBudgetReport, SurfaceFormationCapabilitySet, SurfaceFormationCheckpoint,
    SurfaceFormationUpstreamFingerprints, FORMATION_TERRAIN_FIELDS_SCHEMA_V3,
    NATURAL_SURFACE_FORMATION_SCHEMA_V3,
};
use sekai::world::spatial::SurfaceRef;
use sekai::world::RootSeed;

use support::surface_formation::surface_formation_fixture;

fn zero_sediment(count: usize) -> FormationSedimentFields {
    FormationSedimentFields::new(
        vec![0.0; count],
        vec![[0.0; 5]; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

fn zero_process_rates(count: usize) -> FormationProcessRates {
    FormationProcessRates::new(
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

/// Minimal non-production snapshot used only to exercise evaluator failures
/// while the default absolute-steady-state product is correctly unavailable.
/// It is never treated as evidence that P5 generation succeeds.
fn synthetic_formation() -> &'static NaturalSurfaceFormationSnapshot {
    static SNAPSHOT: OnceLock<NaturalSurfaceFormationSnapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        let fixture = surface_formation_fixture();
        let surface = fixture.upstream.bundle.authoritative_surface();
        let relief = &fixture.upstream.relief;
        let count = surface.cells().len();
        let terrain = FormationTerrainFields::new(
            FORMATION_TERRAIN_FIELDS_SCHEMA_V3,
            FormationElevationComponents::new(
                relief.elevation_m().to_vec(),
                vec![0.0; count],
                relief.elevation_m().to_vec(),
            )
            .unwrap(),
            relief.surface_water_geometry().clone(),
            relief.water_inventory_m3(),
            zero_sediment(count),
        )
        .unwrap();
        let process_rates = zero_process_rates(count);
        let hydrology = FormationHydrologyGenerator::generate(
            surface,
            &terrain,
            &fixture.upstream.substrate,
            &fixture.initial_climate,
            &HydroErosionSpec::default(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let climate = fixture.initial_climate.clone();
        let state_fingerprint =
            surface_formation_state_fingerprint(&terrain, &process_rates, &hydrology, &climate);
        let checkpoint = SurfaceFormationCheckpoint::new(
            SurfaceRef::for_spherical(surface),
            NaturalQualityProfile::Draft,
            SurfaceFormationUpstreamFingerprints::new(
                [1; 32], [2; 32], [3; 32], [4; 32], [5; 32], [6; 32], [7; 32],
            )
            .unwrap(),
            state_fingerprint,
        )
        .unwrap();
        NaturalSurfaceFormationSnapshot::new(
            NATURAL_SURFACE_FORMATION_SCHEMA_V3,
            SurfaceRef::for_spherical(surface),
            checkpoint,
            terrain,
            process_rates,
            hydrology,
            climate,
            FormationSolveReport::new(
                8,
                1,
                FormationResiduals::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0).unwrap(),
                8_192,
            )
            .unwrap(),
            SedimentBudgetReport::new(0.0, 0.0, 0.0, 0.0, 0.0, [0.0; 5], [0.0; 5]).unwrap(),
            SurfaceFormationCapabilitySet::p5(),
        )
        .unwrap()
    })
}

#[test]
fn the_evaluator_rejects_a_same_surface_relief_that_did_not_produce_the_snapshot() {
    let fixture = surface_formation_fixture();
    let surface = fixture.upstream.bundle.authoritative_surface();
    let mut relief_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(43),
        StageIdentity::new("natural.primary-relief", 1, "sekai.core"),
    ));
    let mut diagnostics = Vec::new();
    let other_relief = PrimaryReliefGenerator::generate(
        surface,
        &fixture.upstream.evolved,
        &fixture.upstream.substrate,
        &ReliefSpec::default(),
        &mut relief_rng,
        &mut diagnostics,
    )
    .unwrap();
    assert_eq!(
        other_relief.surface_ref(),
        fixture.upstream.relief.surface_ref()
    );
    assert_ne!(
        other_relief.elevation_m(),
        fixture.upstream.relief.elevation_m()
    );
    assert!(matches!(
        evaluate_surface_formation_quality(surface, &other_relief, synthetic_formation()),
        Err(QualityBuildError::InvalidInput {
            input: "primary_relief",
            ..
        })
    ));
}

#[test]
fn cancelled_quality_evaluation_publishes_no_partial_report() {
    let fixture = surface_formation_fixture();
    let surface = fixture.upstream.bundle.authoritative_surface();
    let signal = BuildCancellation::new();
    signal.cancel();
    assert!(matches!(
        sekai::generators::natural::evaluate_surface_formation_quality_cancellable(
            surface,
            &fixture.upstream.relief,
            synthetic_formation(),
            &signal,
        ),
        Err(QualityBuildError::Cancelled)
    ));

    let signal = BuildCancellation::new();
    let worker_signal = signal.clone();
    let worker = std::thread::spawn(move || {
        sekai::generators::natural::evaluate_surface_formation_quality_cancellable(
            surface_formation_fixture()
                .upstream
                .bundle
                .authoritative_surface(),
            &surface_formation_fixture().upstream.relief,
            synthetic_formation(),
            &worker_signal,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while signal.observation_count() < 8 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    signal.cancel();
    match worker.join().unwrap() {
        Ok(report) => report.validate().unwrap(),
        Err(error) => assert_eq!(error, QualityBuildError::Cancelled),
    }
}

// Task 9 restores quality inventory and corpus-envelope assertions on a real
// default product; Task 0 retains only evaluator failure and cancellation
// behavior without manufacturing a successful scientific artifact.
