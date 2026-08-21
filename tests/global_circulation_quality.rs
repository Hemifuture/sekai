mod support;

use sekai::engine::{derive_stage_seed, Artifact, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_global_circulation_quality, evaluate_global_circulation_quality_cancellable,
    GlobalCirculationArtifact, GlobalCirculationGenerationError, GlobalCirculationGenerator,
    GlobalCirculationProductError, GlobalClimateForcingBuilder, GlobalClimateForcingError,
    PrimaryReliefGenerator, QualityBuildError,
};
use sekai::world::natural::{ClimateModelProfile, ClimateSpec, QualityMetricStatus, ReliefSpec};
use sekai::world::RootSeed;

use support::global_circulation::global_circulation_fixture;

#[test]
fn public_product_error_flattens_cancellation_from_every_dense_phase() {
    assert_eq!(
        GlobalCirculationProductError::from(GlobalClimateForcingError::Cancelled),
        GlobalCirculationProductError::Cancelled
    );
    assert_eq!(
        GlobalCirculationProductError::from(GlobalCirculationGenerationError::Cancelled),
        GlobalCirculationProductError::Cancelled
    );
    assert_eq!(
        GlobalCirculationProductError::from(QualityBuildError::Cancelled),
        GlobalCirculationProductError::Cancelled
    );
}

#[test]
fn generated_c2_passes_locked_wind_ocean_vertical_and_moisture_gates() {
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
    let report =
        evaluate_global_circulation_quality(surface, &fixture.relief, &fixture.forcing, &snapshot)
            .unwrap();
    assert!(report.metrics().len() >= 10);
    for required in [
        "low-latitude-easterly-fraction",
        "midlatitude-westerly-fraction",
        "vertical-shear-rms-m-s",
        "ocean-current-land-leakage-max-m-s",
        "ocean-gyre-circulation-fraction",
        "mixed-layer-warmer-than-thermocline-fraction",
        "positive-thermocline-depth-fraction",
        "sea-surface-height-max-absolute-m",
        "warm-ocean-humidity-contrast",
        "warm-ocean-humidity-correlation",
        "orographic-precipitation-response",
        "orographic-rain-shadow-leeward-drying",
        "orographic-uplift-enrichment-ratio",
        "seasonal-hemisphere-phase-correlation",
        "seasonal-hemisphere-phase-fraction",
        "cubed-face-seam-speed-ratio",
    ] {
        let metric = report
            .metrics()
            .iter()
            .find(|metric| metric.id().name() == required)
            .unwrap_or_else(|| panic!("missing quality metric {required}"));
        assert_eq!(
            metric.status(),
            QualityMetricStatus::Pass,
            "{required}: value={:?}, bounds={:?}",
            metric.value(),
            metric.bounds(),
        );
        if required == "sea-surface-height-max-absolute-m" {
            assert_eq!(metric.bounds().min(), Some(0.01));
            assert_eq!(metric.bounds().max(), Some(6.0));
        }
    }
}

#[test]
fn quality_report_is_deterministic_and_bound_to_the_authoritative_surface() {
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
    let first =
        evaluate_global_circulation_quality(surface, &fixture.relief, &fixture.forcing, &snapshot)
            .unwrap();
    let second =
        evaluate_global_circulation_quality(surface, &fixture.relief, &fixture.forcing, &snapshot)
            .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.surface_ref(), snapshot.surface_ref());
    assert_eq!(
        first.subject_fingerprint(),
        Some(snapshot.checkpoint().fingerprint())
    );
}

#[test]
fn public_product_factory_owns_generation_and_selects_the_locked_integrator() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let independently_generated = GlobalCirculationGenerator::generate(
        surface,
        &fixture.domain,
        &fixture.forcing,
        ClimateModelProfile::C2LayeredV1,
        &BuildCancellation::new(),
    )
    .unwrap();
    let report = evaluate_global_circulation_quality(
        surface,
        &fixture.relief,
        &fixture.forcing,
        &independently_generated,
    )
    .unwrap();
    let artifact = GlobalCirculationArtifact::generate(
        surface,
        &fixture.domain,
        &fixture.forcing,
        &fixture.relief,
        &BuildCancellation::new(),
    )
    .unwrap();
    artifact.validate().unwrap();
    assert_eq!(artifact.quality_report(), &report);
    assert_eq!(artifact.snapshot(), &independently_generated);
}

#[test]
fn product_artifact_factory_remeasures_instead_of_accepting_forged_pass_values() {
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
    let report =
        evaluate_global_circulation_quality(surface, &fixture.relief, &fixture.forcing, &snapshot)
            .unwrap();
    let mut wire = serde_json::to_value(&report).unwrap();
    let metrics = wire
        .get_mut("metrics")
        .and_then(serde_json::Value::as_array_mut)
        .unwrap();
    for metric in metrics {
        let min = metric["bounds"]["min"].as_f64();
        let max = metric["bounds"]["max"].as_f64();
        metric["status"] = serde_json::json!("pass");
        metric["value"] = serde_json::json!(min.or(max).unwrap_or(0.0));
    }
    let forged: sekai::world::natural::NaturalQualityReport = serde_json::from_value(wire).unwrap();
    forged.validate().unwrap();
    assert_ne!(
        forged, report,
        "fixture must produce non-boundary measurements"
    );

    let artifact = GlobalCirculationArtifact::generate(
        surface,
        &fixture.domain,
        &fixture.forcing,
        &fixture.relief,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(artifact.quality_report(), &report);
    assert_ne!(artifact.quality_report(), &forged);
}

#[test]
fn quality_evaluator_rejects_same_surface_relief_not_used_by_the_forcing() {
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
    let mut relief_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(43),
        StageIdentity::new("natural.primary-relief", 1, "sekai.core"),
    ));
    let mut diagnostics = Vec::new();
    let other_relief = PrimaryReliefGenerator::generate(
        surface,
        &fixture.evolved,
        &fixture.substrate,
        &ReliefSpec::default(),
        &mut relief_rng,
        &mut diagnostics,
    )
    .unwrap();
    assert_eq!(other_relief.surface_ref(), fixture.relief.surface_ref());
    assert_ne!(other_relief.elevation_m(), fixture.relief.elevation_m());

    assert!(matches!(
        evaluate_global_circulation_quality(surface, &other_relief, &fixture.forcing, &snapshot,),
        Err(QualityBuildError::InvalidInput {
            input: "primary_relief",
            ..
        })
    ));
}

#[test]
fn cancelled_quality_evaluation_publishes_no_partial_report() {
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
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    assert_eq!(
        evaluate_global_circulation_quality_cancellable(
            surface,
            &fixture.relief,
            &fixture.forcing,
            &snapshot,
            &cancellation,
        ),
        Err(QualityBuildError::Cancelled)
    );

    let active_cancellation = BuildCancellation::new();
    let baseline = active_cancellation.observation_count();
    let latency = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            evaluate_global_circulation_quality_cancellable(
                surface,
                &fixture.relief,
                &fixture.forcing,
                &snapshot,
                &active_cancellation,
            )
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while active_cancellation.observation_count() < baseline + 8 {
            assert!(
                std::time::Instant::now() < deadline,
                "quality evaluator did not enter a cancellable dense scan"
            );
            std::thread::yield_now();
        }
        let started = std::time::Instant::now();
        active_cancellation.cancel();
        assert_eq!(worker.join().unwrap(), Err(QualityBuildError::Cancelled));
        started.elapsed()
    });
    assert!(
        latency <= std::time::Duration::from_millis(250),
        "active quality cancellation took {latency:?}"
    );
}

#[test]
fn zero_axial_tilt_marks_seasonal_phase_not_applicable_without_rejecting_product() {
    let fixture = global_circulation_fixture();
    let surface = fixture.bundle.authoritative_surface();
    let spec = ClimateSpec {
        axial_tilt_centideg: 0,
        ..ClimateSpec::default()
    };
    spec.validate().unwrap();
    let forcing = GlobalClimateForcingBuilder::build(
        surface,
        &fixture.relief,
        &spec,
        &fixture.domain,
        &BuildCancellation::new(),
    )
    .unwrap();
    let snapshot = GlobalCirculationGenerator::generate(
        surface,
        &fixture.domain,
        &forcing,
        ClimateModelProfile::C2LayeredV1,
        &BuildCancellation::new(),
    )
    .unwrap();
    let report =
        evaluate_global_circulation_quality(surface, &fixture.relief, &forcing, &snapshot).unwrap();
    assert!(
        report
            .metrics()
            .iter()
            .all(|metric| metric.status() != QualityMetricStatus::Fail),
        "a conditionally unavailable seasonal signal must not hide another failed product gate"
    );
    for name in [
        "seasonal-hemisphere-phase-correlation",
        "seasonal-hemisphere-phase-fraction",
    ] {
        let metric = report
            .metrics()
            .iter()
            .find(|metric| metric.id().name() == name)
            .unwrap();
        assert_eq!(metric.status(), QualityMetricStatus::Unavailable);
        assert_eq!(metric.sample_count(), 0);
        assert!(metric.reason().unwrap().contains("below 0.5 C"));
    }
    let artifact = GlobalCirculationArtifact::generate(
        surface,
        &fixture.domain,
        &forcing,
        &fixture.relief,
        &BuildCancellation::new(),
    )
    .unwrap();
    artifact.validate().unwrap();
}
