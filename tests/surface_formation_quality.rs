mod support;

use std::time::{Duration, Instant};

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_surface_formation_corpus_hypsometry, evaluate_surface_formation_quality,
    NaturalSurfaceFormationArtifact, PrimaryReliefGenerator, QualityBuildError,
};
use sekai::world::natural::{QualityMetricStatus, ReliefSpec};
use sekai::world::RootSeed;

use support::surface_formation::{published_formation, surface_formation_fixture};

const METRIC_NAMESPACE: &str = "sekai.surface-formation-v1";
const EXPECTED_METRIC_NAMES: [&str; 22] = [
    "component-identity-mismatch-count",
    "deposited-sediment-enrichment-ratio",
    "final-land-fraction-absolute-change",
    "fixed-point-normalized-residual",
    "fluvial-incision-support-enrichment-ratio",
    "land-area-share-below-100m",
    "land-outlet-path-area-fraction",
    "land-relief-mean-m",
    "land-relief-p05-m",
    "land-relief-p25-m",
    "land-relief-p50-m",
    "land-relief-p75-m",
    "land-relief-p95-m",
    "largest-network-strahler-order",
    "ocean-depth-p50-m",
    "primary-final-elevation-correlation",
    "provenance-mass-relative-error",
    "receiver-adjacency-violation-count",
    "river-reach-count",
    "sediment-mass-relative-error",
    "through-ocean-land-river-count",
    "water-volume-relative-error",
];

#[test]
fn the_locked_gate_inventory_passes_on_the_published_draft_product() {
    let fixture = surface_formation_fixture();
    let report = evaluate_surface_formation_quality(
        fixture.upstream.bundle.authoritative_surface(),
        &fixture.upstream.relief,
        published_formation(),
    )
    .unwrap();
    report.validate().unwrap();
    assert_eq!(report.metrics().len(), EXPECTED_METRIC_NAMES.len());
    for (metric, expected_name) in report.metrics().iter().zip(EXPECTED_METRIC_NAMES) {
        assert_eq!(metric.id().namespace(), METRIC_NAMESPACE);
        assert_eq!(metric.id().version(), 1);
        assert_eq!(metric.id().name(), expected_name);
        assert_eq!(
            metric.status(),
            QualityMetricStatus::Pass,
            "metric {expected_name} returned {:?} ({:?}) with value {:?}",
            metric.status(),
            metric.reason(),
            metric.value()
        );
    }
    assert_eq!(
        report.subject_fingerprint(),
        Some(published_formation().checkpoint().fingerprint())
    );
    assert_eq!(
        report.surface_ref(),
        published_formation().surface_ref(),
        "the report must be bound to formation authority"
    );
}

#[test]
fn repeated_evaluation_of_one_product_is_identical() {
    let fixture = surface_formation_fixture();
    let surface = fixture.upstream.bundle.authoritative_surface();
    let first = evaluate_surface_formation_quality(
        surface,
        &fixture.upstream.relief,
        published_formation(),
    )
    .unwrap();
    let second = evaluate_surface_formation_quality(
        surface,
        &fixture.upstream.relief,
        published_formation(),
    )
    .unwrap();
    assert_eq!(first, second);
}

#[test]
fn the_evaluator_rejects_a_same_surface_relief_that_did_not_produce_the_product() {
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
        evaluate_surface_formation_quality(surface, &other_relief, published_formation()),
        Err(QualityBuildError::InvalidInput {
            input: "primary_relief",
            ..
        })
    ));
}

#[test]
fn the_product_factory_remeasures_instead_of_accepting_a_forged_pass_report() {
    let fixture = surface_formation_fixture();
    let surface = fixture.upstream.bundle.authoritative_surface();
    let report = evaluate_surface_formation_quality(
        surface,
        &fixture.upstream.relief,
        published_formation(),
    )
    .unwrap();
    let mut wire = serde_json::to_value(&report).unwrap();
    for metric in wire
        .get_mut("metrics")
        .and_then(serde_json::Value::as_array_mut)
        .unwrap()
    {
        let min = metric["bounds"]["min"].as_f64();
        let max = metric["bounds"]["max"].as_f64();
        metric["status"] = serde_json::json!("pass");
        metric["value"] = serde_json::json!(min.or(max).unwrap_or(0.0));
    }
    let forged: sekai::world::natural::NaturalQualityReport = serde_json::from_value(wire).unwrap();
    forged.validate().unwrap();
    assert_ne!(
        forged, report,
        "the fixture must produce non-boundary measurements"
    );

    let artifact =
        NaturalSurfaceFormationArtifact::generate(fixture.inputs(), &BuildCancellation::new())
            .unwrap();
    assert_eq!(artifact.quality_report(), &report);
    assert_ne!(artifact.quality_report(), &forged);
    assert_eq!(
        artifact.snapshot().checkpoint().state_fingerprint(),
        published_formation().checkpoint().state_fingerprint()
    );
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
            published_formation(),
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
            published_formation(),
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

#[test]
fn the_hypsometric_envelope_is_a_corpus_median_gate_over_unbounded_world_measurements() {
    let fixture = surface_formation_fixture();
    let report = evaluate_surface_formation_quality(
        fixture.upstream.bundle.authoritative_surface(),
        &fixture.upstream.relief,
        published_formation(),
    )
    .unwrap();
    let hypsometric = report
        .metrics()
        .iter()
        .filter(|metric| {
            metric.id().name().starts_with("land-relief-")
                || metric.id().name() == "land-area-share-below-100m"
                || metric.id().name() == "ocean-depth-p50-m"
        })
        .collect::<Vec<_>>();
    assert_eq!(hypsometric.len(), 8);
    assert!(hypsometric.iter().all(|metric| {
        metric.bounds().min().is_none()
            && metric.bounds().max().is_none()
            && metric.status() == QualityMetricStatus::Pass
            && metric.value().is_some()
    }));

    // A corpus of identical worlds has every median equal to the world value,
    // and the corpus metrics carry the frozen envelope bounds.
    let corpus =
        evaluate_surface_formation_corpus_hypsometry(&[report.clone(), report.clone()]).unwrap();
    assert_eq!(corpus.metrics().len(), 8);
    for metric in corpus.metrics() {
        let name = metric.id().name().strip_prefix("corpus-median-").unwrap();
        let world = hypsometric
            .iter()
            .find(|candidate| candidate.id().name() == name)
            .unwrap();
        assert_eq!(metric.value(), world.value(), "{name}");
        assert!(metric.bounds().min().is_some(), "{name}");
        eprintln!(
            "corpus hypsometry {name}: value={:?} bounds={:?} status={:?}",
            metric.value(),
            metric.bounds(),
            metric.status()
        );
    }
    assert!(evaluate_surface_formation_corpus_hypsometry(&[]).is_err());
}
