use std::fmt::Write as _;
use std::time::Instant;

use sekai::engine::{derive_stage_seed, BuildCancellation, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_primary_relief_corpus_quality, evaluate_primary_relief_quality,
    EvolvedTectonicGenerator, GeologicSubstrateGenerator, PrimaryReliefGenerator,
    PrimaryReliefQualitySample,
};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    BedrockKind, GeologicSpec, NaturalQualityProfile, NaturalQualityReport, QualityMetricStatus,
    ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;
const SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];
const HARD_METRICS: [&str; 6] = [
    "component-closure-max-error-m",
    "elevation-safety-violation-count",
    "maximum-plate-area-fraction",
    "non-finite-value-count",
    "upstream-p2-hard-failure-count",
    "water-volume-relative-error",
];

#[derive(Serialize)]
struct P3Evidence {
    schema_version: u16,
    algorithm_references: [&'static str; 3],
    extension_classification: &'static str,
    profile: NaturalQualityProfile,
    radius_m: f64,
    authoritative_cells: usize,
    authoritative_fingerprint: String,
    seeds: Vec<SeedEvidence>,
    corpus_metrics: Vec<MetricEvidence>,
}

#[derive(Serialize)]
struct SeedEvidence {
    seed: u64,
    substrate_json_bytes: usize,
    substrate_json_hash: String,
    primary_snapshot_json_bytes: usize,
    primary_snapshot_json_hash: String,
    sea_level_m: f32,
    physical_land_fraction: f32,
    requested_land_fraction: f32,
    water_volume_relative_error: f64,
    diagnostics: usize,
    bedrock_counts: BedrockCounts,
    metrics: Vec<MetricEvidence>,
}

#[derive(Default, Serialize)]
struct BedrockCounts {
    oceanic_mafic: usize,
    continental_crystalline: usize,
    sedimentary: usize,
    metamorphic: usize,
    volcanic: usize,
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

struct GeneratedWorld {
    evolved: sekai::world::natural::EvolvedTectonicSnapshot,
    substrate: sekai::world::natural::GeologicSubstrateSnapshot,
    relief: sekai::world::natural::PrimaryReliefSnapshot,
    report: NaturalQualityReport,
    diagnostics: Vec<Diagnostic>,
}

#[test]
#[ignore = "release-only deterministic 17-seed P3 JSON/CSV evidence writer"]
fn write_primary_relief_evidence() {
    let started = Instant::now();
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let formation = formation();
    let mut worlds = Vec::new();
    for seed in SEEDS {
        worlds.push(generate_world(
            bundle.authoritative_surface(),
            &bundle,
            &formation,
            seed,
        ));
    }

    let samples = worlds
        .iter()
        .map(|world| {
            PrimaryReliefQualitySample::new(&world.evolved, &world.substrate, &world.relief)
        })
        .collect::<Vec<_>>();
    let corpus =
        evaluate_primary_relief_corpus_quality(bundle.authoritative_surface(), &samples).unwrap();
    assert!(corpus
        .metrics()
        .iter()
        .all(|metric| metric.status() == QualityMetricStatus::Pass));

    let mut seeds = Vec::new();
    for (seed, world) in SEEDS.into_iter().zip(&worlds) {
        assert!(world
            .report
            .metrics()
            .iter()
            .filter(|metric| HARD_METRICS.contains(&metric.id().name()))
            .all(|metric| metric.status() == QualityMetricStatus::Pass));
        world.relief.validate().unwrap();
        world.report.validate().unwrap();
        let seed_evidence = seed_evidence(seed, world, &world.report);
        eprintln!(
            "P3 seed={seed} sea={:.2} land={:.6} substrate_hash={} relief_hash={}",
            world.relief.sea_level_m(),
            world.relief.physical_land_fraction(),
            seed_evidence.substrate_json_hash,
            seed_evidence.primary_snapshot_json_hash,
        );
        seeds.push(seed_evidence);
    }

    let repeated = generate_world(
        bundle.authoritative_surface(),
        &bundle,
        &formation,
        SEEDS[0],
    );
    assert_eq!(
        serde_json::to_vec(&worlds[0].substrate).unwrap(),
        serde_json::to_vec(&repeated.substrate).unwrap(),
        "the fixed-seed substrate changed within one evidence run"
    );
    assert_eq!(
        serde_json::to_vec(&worlds[0].relief).unwrap(),
        serde_json::to_vec(&repeated.relief).unwrap(),
        "the fixed-seed primary relief changed within one evidence run"
    );

    let evidence = P3Evidence {
        schema_version: 1,
        algorithm_references: [
            "Airy-local-isostasy-density-aware",
            "Parsons-Sclater-1977-oceanic-subsidence",
            "Shepard-1968-Dunavant-1985-P1-surface-water-geometry-v1",
        ],
        extension_classification: "causal-testable-procedural-extension-not-predictive-geodynamics",
        profile: NaturalQualityProfile::Draft,
        radius_m: RADIUS_M,
        authoritative_cells: bundle.authoritative_surface().cells().len(),
        authoritative_fingerprint: hex(bundle.authoritative_surface().fingerprint()),
        seeds,
        corpus_metrics: metric_evidence(&corpus),
    };
    let json = serde_json::to_vec_pretty(&evidence).unwrap();
    assert_eq!(json, serde_json::to_vec_pretty(&evidence).unwrap());
    let csv = render_csv(&evidence);
    let output = output_directory();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("evidence.json"), &json).unwrap();
    std::fs::write(output.join("metrics.csv"), csv.as_bytes()).unwrap();
    eprintln!(
        "P3 evidence json_bytes={} json_hash={} csv_bytes={} csv_hash={} elapsed={:?}",
        json.len(),
        blake3::hash(&json).to_hex(),
        csv.len(),
        blake3::hash(csv.as_bytes()).to_hex(),
        started.elapsed(),
    );
    for metric in corpus.metrics() {
        eprintln!(
            "P3 corpus {}.{}.v{}={:?} samples={} status={:?}",
            metric.id().namespace(),
            metric.id().name(),
            metric.id().version(),
            metric.value(),
            metric.sample_count(),
            metric.status(),
        );
    }
}

fn generate_world(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    bundle: &sekai::generators::spatial::ProfileSurfaceBundle,
    formation: &ResolvedWorldFormation,
    seed: u64,
) -> GeneratedWorld {
    let mut evolved_rng = stage_rng(seed, "natural.evolved-tectonics", 5);
    let evolved = EvolvedTectonicGenerator::generate(
        bundle,
        &TectonicSpec::default(),
        formation,
        &mut evolved_rng,
    )
    .unwrap();
    let mut substrate_rng = stage_rng(seed, "natural.geologic-substrate", 1);
    let substrate = GeologicSubstrateGenerator::generate(
        surface,
        &evolved,
        &GeologicSpec::default(),
        formation,
        &mut substrate_rng,
    )
    .unwrap();
    let mut relief_rng = stage_rng(seed, "natural.primary-relief", 1);
    let mut diagnostics = Vec::new();
    let relief = PrimaryReliefGenerator::generate(
        surface,
        &evolved,
        &substrate,
        &ReliefSpec::default(),
        &mut relief_rng,
        &mut diagnostics,
    )
    .unwrap();
    let report = evaluate_primary_relief_quality(surface, &evolved, &substrate, &relief).unwrap();
    GeneratedWorld {
        evolved,
        substrate,
        relief,
        report,
        diagnostics,
    }
}

fn stage_rng(seed: u64, name: &'static str, version: u32) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(name, version, "sekai.core"),
    ))
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn bedrock_counts(substrate: &sekai::world::natural::GeologicSubstrateSnapshot) -> BedrockCounts {
    let mut counts = BedrockCounts::default();
    for index in 0..substrate.cell_count() as usize {
        match substrate.bedrock_kind(index).unwrap() {
            BedrockKind::OceanicMafic => counts.oceanic_mafic += 1,
            BedrockKind::ContinentalCrystalline => counts.continental_crystalline += 1,
            BedrockKind::Sedimentary => counts.sedimentary += 1,
            BedrockKind::Metamorphic => counts.metamorphic += 1,
            BedrockKind::Volcanic => counts.volcanic += 1,
        }
    }
    counts
}

fn seed_evidence(seed: u64, world: &GeneratedWorld, report: &NaturalQualityReport) -> SeedEvidence {
    world.substrate.validate().unwrap();
    world.relief.validate().unwrap();
    report.validate().unwrap();
    let substrate_bytes = serde_json::to_vec(&world.substrate).unwrap();
    let snapshot_bytes = serde_json::to_vec(&world.relief).unwrap();
    SeedEvidence {
        seed,
        substrate_json_bytes: substrate_bytes.len(),
        substrate_json_hash: blake3::hash(&substrate_bytes).to_hex().to_string(),
        primary_snapshot_json_bytes: snapshot_bytes.len(),
        primary_snapshot_json_hash: blake3::hash(&snapshot_bytes).to_hex().to_string(),
        sea_level_m: world.relief.sea_level_m(),
        physical_land_fraction: world.relief.physical_land_fraction(),
        requested_land_fraction: world.relief.requested_land_fraction(),
        water_volume_relative_error: world.relief.water_volume_relative_error(),
        diagnostics: world.diagnostics.len(),
        bedrock_counts: bedrock_counts(&world.substrate),
        metrics: metric_evidence(report),
    }
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

fn render_csv(evidence: &P3Evidence) -> String {
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
    .expect("writing P3 CSV into a string cannot fail");
}

fn option(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.17}"))
}

fn output_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p3")
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
    assert!(output_directory().ends_with("target/natural-quality/p3"));
}
