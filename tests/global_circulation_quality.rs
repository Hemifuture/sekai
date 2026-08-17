mod support;

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{evaluate_global_circulation_quality, GlobalCirculationGenerator};
use sekai::world::natural::{ClimateModelProfile, QualityMetricStatus};

use support::global_circulation::global_circulation_fixture;

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
    let report = evaluate_global_circulation_quality(surface, &fixture.relief, &snapshot).unwrap();
    for metric in report.metrics() {
        println!(
            "{} status={:?} value={:?} bounds={:?}",
            metric.id().name(),
            metric.status(),
            metric.value(),
            metric.bounds()
        );
    }
    assert!(report.metrics().len() >= 10);
    for required in [
        "low-latitude-easterly-fraction",
        "midlatitude-westerly-fraction",
        "vertical-shear-rms-m-s",
        "ocean-current-land-leakage-max-m-s",
        "ocean-gyre-circulation-fraction",
        "mixed-layer-warmer-than-thermocline-fraction",
        "positive-thermocline-depth-fraction",
        "warm-ocean-humidity-correlation",
        "orographic-precipitation-response",
        "orographic-rain-shadow-correlation",
        "seasonal-hemisphere-phase-correlation",
        "seasonal-hemisphere-phase-fraction",
        "cubed-face-seam-speed-ratio",
    ] {
        let metric = report
            .metrics()
            .iter()
            .find(|metric| metric.id().name() == required)
            .unwrap_or_else(|| panic!("missing quality metric {required}"));
        assert_eq!(metric.status(), QualityMetricStatus::Pass, "{required}");
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
    let first = evaluate_global_circulation_quality(surface, &fixture.relief, &snapshot).unwrap();
    let second = evaluate_global_circulation_quality(surface, &fixture.relief, &snapshot).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.surface_ref(), snapshot.surface_ref());
}
