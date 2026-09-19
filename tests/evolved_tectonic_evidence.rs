use std::fmt::Write as _;
use std::time::Instant;

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_evolved_tectonic_corpus_quality, evaluate_evolved_tectonic_quality,
    EvolvedTectonicGenerator,
};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    NaturalQualityProfile, NaturalQualityReport, QualityMetricStatus, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, SphericalTectonicLineageBudget, SphericalTectonicMaterialBudget,
    TectonicSpec, WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;
const SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

#[derive(Serialize)]
struct P2Evidence {
    schema_version: u16,
    algorithm_reference: &'static str,
    extension_classification: &'static str,
    profile: NaturalQualityProfile,
    radius_m: f64,
    authoritative_cells: usize,
    control_cells: usize,
    authoritative_fingerprint: String,
    control_fingerprint: String,
    conservative_map_overlaps: usize,
    seeds: Vec<SeedEvidence>,
    corpus_metrics: Vec<MetricEvidence>,
}

#[derive(Serialize)]
struct SeedEvidence {
    seed: u64,
    snapshot_json_bytes: usize,
    snapshot_json_hash: String,
    plate_count: usize,
    boundary_segment_count: usize,
    material_budget: SphericalTectonicMaterialBudget,
    lineage_budget: SphericalTectonicLineageBudget,
    metrics: Vec<MetricEvidence>,
}

#[derive(Serialize)]
struct MetricEvidence {
    id: String,
    status: QualityMetricStatus,
    value: Option<f64>,
    sample_count: u32,
    minimum: Option<f64>,
    maximum: Option<f64>,
    unavailable_reason: Option<String>,
}

#[test]
#[ignore = "release-only deterministic 17-seed P2 JSON/CSV evidence writer"]
fn write_evolved_tectonic_evidence() {
    let started = Instant::now();
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let formation = formation();
    let spec = TectonicSpec::default();
    let mut snapshots = Vec::new();
    let mut reports = Vec::new();
    let mut seeds = Vec::new();
    for seed in SEEDS {
        let snapshot = generate(&bundle, &spec, &formation, seed);
        let report =
            evaluate_evolved_tectonic_quality(bundle.authoritative_surface(), &snapshot).unwrap();
        snapshot.validate().unwrap();
        report.validate().unwrap();
        let snapshot_bytes = serde_json::to_vec(&snapshot).unwrap();
        eprintln!(
            "P2 seed={seed} plates={} snapshot_bytes={} hash={}",
            snapshot.compatibility().plates().len(),
            snapshot_bytes.len(),
            blake3::hash(&snapshot_bytes).to_hex(),
        );
        seeds.push(SeedEvidence {
            seed,
            snapshot_json_bytes: snapshot_bytes.len(),
            snapshot_json_hash: blake3::hash(&snapshot_bytes).to_hex().to_string(),
            plate_count: snapshot.compatibility().plates().len(),
            boundary_segment_count: snapshot.compatibility().boundary_segments().len(),
            material_budget: *snapshot.material_budget(),
            lineage_budget: *snapshot.lineage_budget(),
            metrics: metric_evidence(&report),
        });
        snapshots.push(snapshot);
        reports.push(report);
    }

    let references = snapshots.iter().collect::<Vec<_>>();
    let corpus =
        evaluate_evolved_tectonic_corpus_quality(bundle.authoritative_surface(), &references)
            .unwrap();
    assert!(corpus
        .metrics()
        .iter()
        .all(|metric| metric.status() == QualityMetricStatus::Pass));
    assert!(reports.iter().all(|report| {
        report
            .metrics()
            .iter()
            .filter(|metric| {
                matches!(
                    metric.id().name(),
                    "continental-area-retention"
                        | "maximum-plate-area-fraction"
                        | "control-material-relative-error"
                        | "authority-material-relative-error"
                        | "lineage-closure-error"
                        | "non-finite-value-count"
                )
            })
            .all(|metric| metric.status() == QualityMetricStatus::Pass)
    }));

    let repeated_snapshot = generate(&bundle, &spec, &formation, SEEDS[0]);
    let repeated_report =
        evaluate_evolved_tectonic_quality(bundle.authoritative_surface(), &repeated_snapshot)
            .unwrap();
    assert_eq!(snapshots[0], repeated_snapshot);
    assert_eq!(reports[0], repeated_report);

    let evidence = P2Evidence {
        schema_version: 1,
        algorithm_reference: "Cortial-Peytavie-Galin-Guerin-2019",
        extension_classification:
            "conservative-testable-procedural-extension-not-predictive-geodynamics",
        profile: NaturalQualityProfile::Draft,
        radius_m: RADIUS_M,
        authoritative_cells: bundle.authoritative_surface().cells().len(),
        control_cells: bundle.tectonic_control_surface().cells().len(),
        authoritative_fingerprint: hex(bundle.authoritative_surface().fingerprint()),
        control_fingerprint: hex(bundle.tectonic_control_surface().fingerprint()),
        conservative_map_overlaps: bundle.control_to_authoritative_map().overlap_count(),
        seeds,
        corpus_metrics: metric_evidence(&corpus),
    };
    let json = serde_json::to_vec_pretty(&evidence).unwrap();
    let deterministic = serde_json::to_vec_pretty(&evidence).unwrap();
    assert_eq!(json, deterministic);
    let csv = render_csv(&evidence);
    let output = output_directory();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("evidence.json"), &json).unwrap();
    std::fs::write(output.join("metrics.csv"), csv.as_bytes()).unwrap();
    eprintln!(
        "P2 evidence json_bytes={} json_hash={} csv_bytes={} csv_hash={} elapsed={:?}",
        json.len(),
        blake3::hash(&json).to_hex(),
        csv.len(),
        blake3::hash(csv.as_bytes()).to_hex(),
        started.elapsed(),
    );
    for metric in corpus.metrics() {
        eprintln!(
            "P2 corpus {}.{}.v{}={:?} samples={} status={:?}",
            metric.id().namespace(),
            metric.id().name(),
            metric.id().version(),
            metric.value(),
            metric.sample_count(),
            metric.status(),
        );
    }
}

fn generate(
    bundle: &sekai::generators::spatial::ProfileSurfaceBundle,
    spec: &TectonicSpec,
    formation: &ResolvedWorldFormation,
    seed: u64,
) -> sekai::world::natural::EvolvedTectonicSnapshot {
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
    ));
    EvolvedTectonicGenerator::generate(bundle, spec, formation, &mut rng).unwrap()
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn metric_evidence(report: &NaturalQualityReport) -> Vec<MetricEvidence> {
    report
        .metrics()
        .iter()
        .map(|metric| MetricEvidence {
            id: format!(
                "{}.{}.v{}",
                metric.id().namespace(),
                metric.id().name(),
                metric.id().version()
            ),
            status: metric.status(),
            value: metric.value(),
            sample_count: metric.sample_count(),
            minimum: metric.bounds().min(),
            maximum: metric.bounds().max(),
            unavailable_reason: metric.reason().map(str::to_owned),
        })
        .collect()
}

fn render_csv(evidence: &P2Evidence) -> String {
    let mut csv = String::from(
        "scope,seed,metric_id,status,value,sample_count,minimum,maximum,unavailable_reason\n",
    );
    for seed in &evidence.seeds {
        for metric in &seed.metrics {
            write_metric_csv(&mut csv, "seed", &seed.seed.to_string(), metric);
        }
    }
    for metric in &evidence.corpus_metrics {
        write_metric_csv(&mut csv, "corpus", "", metric);
    }
    csv
}

fn write_metric_csv(csv: &mut String, scope: &str, seed: &str, metric: &MetricEvidence) {
    writeln!(
        csv,
        "{scope},{seed},{},{:?},{},{},{},{},{}",
        metric.id,
        metric.status,
        option(metric.value),
        metric.sample_count,
        option(metric.minimum),
        option(metric.maximum),
        metric.unavailable_reason.as_deref().unwrap_or("")
    )
    .expect("writing P2 CSV into a string cannot fail");
}

fn option(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.17}"))
}

fn output_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p2")
}

fn hex(bytes: [u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing into a string cannot fail");
    }
    encoded
}

#[test]
fn evidence_paths_are_isolated_under_target() {
    let directory = output_directory();
    assert!(directory.ends_with("target/natural-quality/p2"));
}
