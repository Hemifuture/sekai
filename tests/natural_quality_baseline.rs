#[path = "support/natural_quality.rs"]
mod natural_quality;

use std::time::Instant;

use sekai::world::natural::{
    NaturalQualityReport, QualityBounds, QualityMetric, QualityMetricId, QualityMetricStatus,
    NATURAL_QUALITY_REPORT_SCHEMA_V1,
};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, SPHERICAL_SURFACE_SCHEMA_V1};
use natural_quality::{
    build_v4_quality_reports, natural_quality_output_paths, render_v4_natural_quality_baseline,
    write_v4_natural_quality_baseline as write_v4_natural_quality_baseline_files,
    NaturalQualityBaseline, SeedQualityReport, EXPECTED_P0_METRIC_IDS, QUALITY_SEEDS,
};

fn synthetic_reports() -> Vec<SeedQualityReport> {
    QUALITY_SEEDS
        .iter()
        .map(|&seed| {
            let mut fingerprint = [0_u8; 32];
            fingerprint[..8].copy_from_slice(&seed.to_le_bytes());
            let surface_ref = SurfaceRef::new(
                SurfaceGeometryKind::SphericalV1,
                SPHERICAL_SURFACE_SCHEMA_V1,
                20_252,
                60_750,
                fingerprint,
            )
            .unwrap();
            let metrics = EXPECTED_P0_METRIC_IDS
                .iter()
                .enumerate()
                .map(|(index, id)| {
                    let (qualified_name, version) = id.rsplit_once(".v").unwrap();
                    let (namespace, name) = qualified_name.split_once('.').unwrap();
                    QualityMetric::new(
                        QualityMetricId::new(namespace, name, version.parse().unwrap()).unwrap(),
                        QualityMetricStatus::Pass,
                        Some(index as f64 + seed as f64 / 1_000.0),
                        u32::try_from(index + 1).unwrap(),
                        QualityBounds::unbounded(),
                        None,
                    )
                    .unwrap()
                })
                .collect();
            SeedQualityReport::new(
                seed,
                NaturalQualityReport::new(NATURAL_QUALITY_REPORT_SCHEMA_V1, surface_ref, metrics)
                    .unwrap(),
            )
        })
        .collect()
}

#[test]
fn baseline_renderer_is_deterministic_complete_and_conservative() {
    let reports = synthetic_reports();
    let first = render_v4_natural_quality_baseline(&reports).unwrap();
    let second = render_v4_natural_quality_baseline(&reports).unwrap();
    assert_eq!(blake3::hash(first.json()), blake3::hash(second.json()));
    assert_eq!(blake3::hash(first.csv()), blake3::hash(second.csv()));

    let decoded: NaturalQualityBaseline = serde_json::from_slice(first.json()).unwrap();
    assert_eq!(decoded.reports.len(), QUALITY_SEEDS.len());
    assert_eq!(decoded.aggregates.len(), EXPECTED_P0_METRIC_IDS.len());
    assert_eq!(
        decoded
            .reports
            .iter()
            .map(|seed| seed.seed)
            .collect::<Vec<_>>(),
        QUALITY_SEEDS
    );
    for report in &decoded.reports {
        let ids = report
            .report
            .metrics()
            .iter()
            .map(|metric| {
                format!(
                    "{}.{}.v{}",
                    metric.id().namespace(),
                    metric.id().name(),
                    metric.id().version()
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, EXPECTED_P0_METRIC_IDS);
    }
    for aggregate in &decoded.aggregates {
        let expected_samples = decoded
            .reports
            .iter()
            .flat_map(|seed| seed.report.metrics())
            .filter(|metric| {
                format!(
                    "{}.{}.v{}",
                    metric.id().namespace(),
                    metric.id().name(),
                    metric.id().version()
                ) == aggregate.metric_id
            })
            .map(|metric| u64::from(metric.sample_count()))
            .sum::<u64>();
        assert_eq!(aggregate.sample_count_sum, expected_samples);
        assert_eq!(
            aggregate.pass_seed_count
                + aggregate.fail_seed_count
                + aggregate.unavailable_seed_count,
            QUALITY_SEEDS.len() as u32
        );
    }
    assert_eq!(
        first.csv().split(|byte| *byte == b'\n').count() - 1,
        1 + QUALITY_SEEDS.len() * EXPECTED_P0_METRIC_IDS.len()
    );
}

#[test]
fn baseline_output_paths_are_isolated_under_target() {
    let [json, csv] = natural_quality_output_paths();
    let expected_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality");
    assert_eq!(json.parent(), Some(expected_root.as_path()));
    assert_eq!(csv.parent(), Some(expected_root.as_path()));
    assert_eq!(json.file_name().unwrap(), "v4-baseline.json");
    assert_eq!(csv.file_name().unwrap(), "v4-metrics.csv");
}

#[test]
#[ignore = "release-only 17-seed, 20,252-cell V4 quality baseline writer"]
fn write_v4_natural_quality_baseline() {
    let started = Instant::now();
    let reports = build_v4_quality_reports().unwrap();
    let first = render_v4_natural_quality_baseline(&reports).unwrap();
    let second = render_v4_natural_quality_baseline(&reports).unwrap();
    assert_eq!(blake3::hash(first.json()), blake3::hash(second.json()));
    assert_eq!(blake3::hash(first.csv()), blake3::hash(second.csv()));

    let paths = write_v4_natural_quality_baseline_files(&first).unwrap();
    let baseline: NaturalQualityBaseline = serde_json::from_slice(first.json()).unwrap();
    for id in [
        "tectonics.continental-area-fraction.v1",
        "relief.land-crust-jaccard.v1",
    ] {
        let aggregate = baseline
            .aggregates
            .iter()
            .find(|aggregate| aggregate.metric_id == id)
            .unwrap();
        println!(
            "known_v4_mismatch metric={} min={:?} median={:?} max={:?} failed_seeds={}",
            id, aggregate.min, aggregate.median, aggregate.max, aggregate.fail_seed_count,
        );
        assert!(aggregate.fail_seed_count > 0);
    }
    println!(
        "natural_quality_baseline reports={} elapsed_ms={:.3} json={} csv={} json_hash={} csv_hash={}",
        reports.len(),
        started.elapsed().as_secs_f64() * 1_000.0,
        paths[0].display(),
        paths[1].display(),
        blake3::hash(first.json()),
        blake3::hash(first.csv()),
    );
}
