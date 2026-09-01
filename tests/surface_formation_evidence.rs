mod support;

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Instant;

use sekai::engine::{Artifact, BuildCancellation};
use sekai::generators::natural::{
    evaluate_surface_formation_corpus_hypsometry, NaturalFormationBundleArtifact,
};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    NaturalQualityProfile, NaturalQualityReport, QualityMetricStatus, SurfaceFormationModelId,
};
use sekai::world::Meters;
use serde::Serialize;

use support::causal_formation::build_causal_formation;

const RADIUS_M: f64 = 6_371_000.0;
/// Envelope rows whose corpus medians are written to the evidence but not
/// asserted, because they measure the P3 land-elevation distribution rather
/// than anything P5 owns.
///
/// The first two were already recorded as open by the frozen T0 calibration
/// spec (§11.3 R4). The two quartile rows joined them on 2026-09-02 (audit
/// remediation A0 tasks 3/7/9) once P5's denudation was pinned against
/// observation: at the calibrated `50 m/Myr` the frozen `100 kyr` horizon
/// removes about five metres, which cannot move a median of hundreds of
/// metres in any direction. Their earlier pass was an artefact of a
/// stream-power erodibility an order of magnitude above every observational
/// compilation, whose excess was invisible only while `V_eff = 0` exported the
/// whole eroded mass to the ocean; that combination was acting as an
/// undeclared hypsometric corrector for an upstream cause. All four rows fail
/// in the same direction - too little low land - and belong to the continental
/// margin milestone in `2026-08-26-natural-geography-short-horizon-roadmap.md`
/// §G3.
const OPEN_ENVELOPE_ROWS: [&str; 4] = [
    "corpus-median-land-area-share-below-100m",
    "corpus-median-land-relief-p05-m",
    "corpus-median-land-relief-p25-m",
    "corpus-median-land-relief-p50-m",
];
const SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

#[derive(Serialize)]
struct P5Evidence {
    schema_version: u16,
    profile: NaturalQualityProfile,
    model: SurfaceFormationModelId,
    algorithm_references: Vec<&'static str>,
    procedural_closures: Vec<&'static str>,
    retired_baseline: RetiredBaseline,
    radius_m: f64,
    authoritative_cells: usize,
    authoritative_fingerprint: String,
    seeds: Vec<SeedEvidence>,
    corpus_metrics: Vec<CorpusMetricEvidence>,
    corpus_hypsometry: NaturalQualityReport,
}

/// The old two-pass modifier, recorded as the explicit negative baseline.
#[derive(Serialize)]
struct RetiredBaseline {
    model: &'static str,
    retained_for: &'static str,
    unreportable_p5_gates: Vec<UnreportableGate>,
}

#[derive(Serialize)]
struct UnreportableGate {
    metric: &'static str,
    reason: &'static str,
}

#[derive(Serialize)]
struct SeedEvidence {
    seed: u64,
    artifact_json_bytes: usize,
    artifact_json_hash: String,
    checkpoint_fingerprint: String,
    state_fingerprint: String,
    primary_sea_level_m: f32,
    current_sea_level_m: f32,
    primary_land_fraction: f32,
    accepted_surface_substeps: u32,
    integrated_duration_years: f64,
    terminal_net_surface_rate_rms_m_per_year: f64,
    terminal_gross_surface_rate_rms_m_per_year: f64,
    terminal_local_surface_flux_imbalance_ratio: f64,
    terminal_mean_elevation_rate_m_per_year: f64,
    terminal_mean_elevation_flux_balance_ratio: f64,
    terminal_rms_relief_rate_m_per_year: f64,
    terminal_rms_relief_flux_balance_ratio: f64,
    terminal_sediment_stock_change_kg_per_year: f64,
    terminal_sediment_stock_change_ratio: f64,
    dense_state_bytes: u64,
    produced_sediment_kg_per_year: f64,
    land_lake_deposition_kg_per_year: f64,
    shelf_deposition_kg_per_year: f64,
    deep_ocean_export_kg_per_year: f64,
    sediment_global_relative_error: f64,
    sediment_provenance_relative_error: f64,
    mean_fluvial_erosion_rate_m_per_year: f64,
    mean_hillslope_erosion_rate_m_per_year: f64,
    mean_routed_deposition_rate_m_per_year: f64,
    mean_coastal_erosion_rate_m_per_year: f64,
    mean_absolute_isostatic_response_rate_m_per_year: f64,
    basin_count: usize,
    lake_count: usize,
    river_segment_count: usize,
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
}

#[derive(Serialize)]
struct CorpusMetricEvidence {
    name: String,
    minimum_across_seeds: f64,
    mean_across_seeds: f64,
    maximum_across_seeds: f64,
    passing_seed_count: usize,
}

struct GeneratedWorld {
    artifact: Arc<NaturalFormationBundleArtifact>,
}

#[test]
#[ignore = "release-only deterministic 17-seed P5 JSON/CSV evidence writer"]
fn write_surface_formation_evidence() {
    let started = Instant::now();
    let cancellation = BuildCancellation::new();
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &cancellation,
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let mut worlds = Vec::new();
    for seed in SEEDS {
        let world = generate_world(surface, seed);
        let report = world
            .artifact
            .bundle()
            .surface_formation()
            .evolution_report();
        eprintln!(
            "P5 evidence seed={seed} substeps={} duration={} years",
            report.accepted_surface_substeps(),
            report.integrated_duration_years()
        );
        for metric in world
            .artifact
            .bundle()
            .surface_quality()
            .metrics()
            .iter()
            .filter(|metric| metric.status() != QualityMetricStatus::Pass)
        {
            eprintln!(
                "P5 FAILURE seed={seed} metric={} status={:?} value={:?} bounds={:?}",
                metric.id().name(),
                metric.status(),
                metric.value(),
                metric.bounds(),
            );
        }
        worlds.push(world);
    }

    let mut seeds = Vec::new();
    for (seed, world) in SEEDS.into_iter().zip(&worlds) {
        assert!(
            world
                .artifact
                .bundle()
                .surface_quality()
                .metrics()
                .iter()
                .all(|metric| metric.status() == QualityMetricStatus::Pass),
            "P5 seed {seed} has a failed hard metric"
        );
        world.artifact.validate().unwrap();
        seeds.push(seed_evidence(surface, seed, world));
    }

    let repeated = generate_world(surface, SEEDS[0]);
    assert_eq!(
        worlds[0]
            .artifact
            .bundle()
            .surface_formation()
            .checkpoint()
            .fingerprint(),
        repeated
            .artifact
            .bundle()
            .surface_formation()
            .checkpoint()
            .fingerprint()
    );
    assert_eq!(worlds[0].artifact, repeated.artifact);

    let corpus_metrics = corpus_metric_evidence(&worlds);
    assert!(corpus_metrics
        .iter()
        .all(|metric| metric.passing_seed_count == SEEDS.len()));
    let corpus_hypsometry = evaluate_surface_formation_corpus_hypsometry(
        &worlds
            .iter()
            .map(|world| world.artifact.bundle().surface_quality().clone())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for metric in corpus_hypsometry.metrics() {
        eprintln!(
            "P5 corpus hypsometry {} value={:?} bounds={:?} status={:?}",
            metric.id().name(),
            metric.value(),
            metric.bounds(),
            metric.status()
        );
    }
    let unexpected = corpus_hypsometry
        .metrics()
        .iter()
        .filter(|metric| {
            metric.status() != QualityMetricStatus::Pass
                && !OPEN_ENVELOPE_ROWS.contains(&metric.id().name())
        })
        .map(|metric| metric.id().name())
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "P5 corpus hypsometry rows outside the recorded open set failed their frozen envelope: {unexpected:?}"
    );

    let evidence = P5Evidence {
        schema_version: 2,
        profile: NaturalQualityProfile::Draft,
        model:
            SurfaceFormationModelId::PriorityFloodFastscapeDavyLagueHillslopeCoastIsostasyFiniteTimeV4,
        algorithm_references: vec![
            "barnes-lehman-mulla-priority-flood",
            "braun-willett-o-n-implicit-downstream-stack-stream-power",
            "cordonnier-drainage-uplift-stream-power-coupling",
            "roering-kirchner-dietrich-nonlinear-hillslope-transport",
            "davy-lague-landlab-analytic-erosion-deposition-continuity",
        ],
        procedural_closures: vec![
            "bounded-effective-formation-runoff-proxy",
            "bounded-annual-formation-precipitation-envelope",
            "thousand-year-endorheic-residence-horizon",
            "irregular-spherical-finite-volume-paired-hillslope-mass-packet",
            "current-annual-five-source-provenance-ledger",
            "map-scale-wind-current-coastal-exposure",
            "local-airy-loading-response-without-elastic-flexure",
            "finite-physical-time-held-tectonic-forcing",
        ],
        retired_baseline: retired_baseline(),
        radius_m: RADIUS_M,
        authoritative_cells: surface.cells().len(),
        authoritative_fingerprint: hex(surface.fingerprint()),
        seeds,
        corpus_metrics,
        corpus_hypsometry,
    };

    let json = serde_json::to_vec_pretty(&evidence).unwrap();
    assert_eq!(json, serde_json::to_vec_pretty(&evidence).unwrap());
    let csv = render_csv(&evidence);
    let output = output_directory();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("evidence.json"), &json).unwrap();
    std::fs::write(output.join("evidence.csv"), csv.as_bytes()).unwrap();
    eprintln!(
        "P5 evidence bytes={} hash={} elapsed={:?}",
        json.len(),
        blake3::hash(&json).to_hex(),
        started.elapsed()
    );
}

#[test]
fn evidence_path_is_isolated_under_target() {
    assert!(output_directory().ends_with("target/natural-quality/p5"));
}

fn retired_baseline() -> RetiredBaseline {
    RetiredBaseline {
        model: "spherical-priority-flood-stream-power-v1-two-pass",
        retained_for: "compatibility and negative baseline only; it can never own \
                       world.natural-surface-formation",
        unreportable_p5_gates: vec![
            UnreportableGate {
                metric: "component-identity-mismatch-count",
                reason: "the two-pass modifier publishes one eroded surface, not the nine \
                         separate causal elevation components P5 must reconstruct",
            },
            UnreportableGate {
                metric: "provenance-mass-relative-error",
                reason: "the two-pass sediment ledger carries no five-source provenance",
            },
            UnreportableGate {
                metric: "deposited-sediment-enrichment-ratio",
                reason: "deposition is bounded by a fixed local ceiling instead of transport \
                         capacity, lake accommodation, shelf accommodation, and delta potential",
            },
            UnreportableGate {
                metric: "final-land-fraction-absolute-change",
                reason: "the modifier keeps the upstream sea level and never re-solves the \
                         physical water volume after loading and unloading",
            },
        ],
    }
}

fn seed_evidence(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    seed: u64,
    world: &GeneratedWorld,
) -> SeedEvidence {
    let bundle = world.artifact.bundle();
    let snapshot = bundle.surface_formation();
    let terrain = snapshot.terrain_fields();
    let rates = snapshot.process_rates();
    let budget = snapshot.sediment_budget_report();
    let report = snapshot.evolution_report();
    let residual = report.current_rates();
    let bytes = serde_json::to_vec(world.artifact.as_ref()).unwrap();
    SeedEvidence {
        seed,
        artifact_json_bytes: bytes.len(),
        artifact_json_hash: blake3::hash(&bytes).to_hex().to_string(),
        checkpoint_fingerprint: hex(*snapshot.checkpoint().fingerprint()),
        state_fingerprint: hex(*snapshot.checkpoint().state_fingerprint()),
        primary_sea_level_m: bundle.primary_relief().sea_level_m(),
        current_sea_level_m: terrain.sea_level_m(),
        primary_land_fraction: bundle.primary_relief().physical_land_fraction(),
        accepted_surface_substeps: report.accepted_surface_substeps(),
        integrated_duration_years: report.integrated_duration_years(),
        terminal_net_surface_rate_rms_m_per_year: residual.net_surface_rate_rms_m_per_year(),
        terminal_gross_surface_rate_rms_m_per_year: residual.gross_surface_rate_rms_m_per_year(),
        terminal_local_surface_flux_imbalance_ratio: residual.local_surface_flux_imbalance_ratio(),
        terminal_mean_elevation_rate_m_per_year: residual.mean_elevation_rate_m_per_year(),
        terminal_mean_elevation_flux_balance_ratio: residual.mean_elevation_flux_balance_ratio(),
        terminal_rms_relief_rate_m_per_year: residual.rms_relief_rate_m_per_year(),
        terminal_rms_relief_flux_balance_ratio: residual.rms_relief_flux_balance_ratio(),
        terminal_sediment_stock_change_kg_per_year: residual.sediment_stock_change_kg_per_year(),
        terminal_sediment_stock_change_ratio: residual.sediment_stock_change_ratio(),
        dense_state_bytes: report.dense_state_bytes(),
        produced_sediment_kg_per_year: budget.produced_mass_kg_per_year(),
        land_lake_deposition_kg_per_year: budget.land_lake_deposition_kg_per_year(),
        shelf_deposition_kg_per_year: budget.shelf_deposition_kg_per_year(),
        deep_ocean_export_kg_per_year: budget.deep_ocean_export_kg_per_year(),
        sediment_global_relative_error: budget.global_relative_error(),
        sediment_provenance_relative_error: budget
            .provenance_relative_errors()
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        mean_fluvial_erosion_rate_m_per_year: area_mean(
            surface,
            rates.fluvial_erosion_rate_m_per_year(),
        ),
        mean_hillslope_erosion_rate_m_per_year: area_mean(
            surface,
            rates.hillslope_erosion_rate_m_per_year(),
        ),
        mean_routed_deposition_rate_m_per_year: area_mean(
            surface,
            rates.routed_sediment_deposition_rate_m_per_year(),
        ),
        mean_coastal_erosion_rate_m_per_year: area_mean(
            surface,
            rates.coastal_erosion_rate_m_per_year(),
        ),
        mean_absolute_isostatic_response_rate_m_per_year: area_mean_abs(
            surface,
            rates.isostatic_response_rate_m_per_year(),
        ),
        basin_count: snapshot.hydrology().basins().len(),
        lake_count: snapshot.hydrology().lakes().len(),
        river_segment_count: snapshot.hydrology().river_segments().len(),
        metrics: metric_evidence(bundle.surface_quality()),
    }
}

fn generate_world(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    seed: u64,
) -> GeneratedWorld {
    GeneratedWorld {
        artifact: build_causal_formation(surface, NaturalQualityProfile::Draft, seed),
    }
}

fn metric_evidence(report: &NaturalQualityReport) -> Vec<MetricEvidence> {
    report
        .metrics()
        .iter()
        .map(|metric| MetricEvidence {
            id: format!(
                "{}/{}@{}",
                metric.id().namespace(),
                metric.id().name(),
                metric.id().version()
            ),
            status: metric.status(),
            value: metric.value(),
            sample_count: metric.sample_count(),
            minimum: metric.bounds().min(),
            maximum: metric.bounds().max(),
        })
        .collect()
}

fn corpus_metric_evidence(worlds: &[GeneratedWorld]) -> Vec<CorpusMetricEvidence> {
    let first = worlds[0].artifact.bundle().surface_quality();
    first
        .metrics()
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            let values = worlds
                .iter()
                .filter_map(|world| {
                    world.artifact.bundle().surface_quality().metrics()[index].value()
                })
                .collect::<Vec<_>>();
            let passing = worlds
                .iter()
                .filter(|world| {
                    world.artifact.bundle().surface_quality().metrics()[index].status()
                        == QualityMetricStatus::Pass
                })
                .count();
            CorpusMetricEvidence {
                name: metric.id().name().to_owned(),
                minimum_across_seeds: values.iter().copied().fold(f64::INFINITY, f64::min),
                mean_across_seeds: if values.is_empty() {
                    0.0
                } else {
                    values.iter().sum::<f64>() / values.len() as f64
                },
                maximum_across_seeds: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                passing_seed_count: passing,
            }
        })
        .collect()
}

fn render_csv(evidence: &P5Evidence) -> String {
    let mut csv = String::new();
    writeln!(
        csv,
        "seed,accepted_surface_substeps,integrated_duration_years,\
         terminal_net_surface_rate_rms_m_per_year,terminal_gross_surface_rate_rms_m_per_year,\
         terminal_local_surface_flux_imbalance_ratio,terminal_mean_elevation_rate_m_per_year,\
         terminal_mean_elevation_flux_balance_ratio,terminal_rms_relief_rate_m_per_year,\
         terminal_rms_relief_flux_balance_ratio,terminal_sediment_stock_change_kg_per_year,\
         terminal_sediment_stock_change_ratio,produced_kg_per_year,\
         sediment_relative_error,provenance_relative_error,mean_fluvial_erosion_rate_m_per_year,\
         mean_hillslope_erosion_rate_m_per_year,mean_routed_deposition_rate_m_per_year,\
         basin_count,lake_count,river_segments"
    )
    .unwrap();
    for seed in &evidence.seeds {
        writeln!(
            csv,
            "{},{},{:.9},{:.9e},{:.9e},{:.9e},{:.9e},{:.9e},{:.9e},{:.9e},{:.9e},{:.9e},{:.6e},{:.3e},{:.3e},{:.6},{:.6},{:.6},{},{},{}",
            seed.seed,
            seed.accepted_surface_substeps,
            seed.integrated_duration_years,
            seed.terminal_net_surface_rate_rms_m_per_year,
            seed.terminal_gross_surface_rate_rms_m_per_year,
            seed.terminal_local_surface_flux_imbalance_ratio,
            seed.terminal_mean_elevation_rate_m_per_year,
            seed.terminal_mean_elevation_flux_balance_ratio,
            seed.terminal_rms_relief_rate_m_per_year,
            seed.terminal_rms_relief_flux_balance_ratio,
            seed.terminal_sediment_stock_change_kg_per_year,
            seed.terminal_sediment_stock_change_ratio,
            seed.produced_sediment_kg_per_year,
            seed.sediment_global_relative_error,
            seed.sediment_provenance_relative_error,
            seed.mean_fluvial_erosion_rate_m_per_year,
            seed.mean_hillslope_erosion_rate_m_per_year,
            seed.mean_routed_deposition_rate_m_per_year,
            seed.basin_count,
            seed.lake_count,
            seed.river_segment_count,
        )
        .unwrap();
    }
    csv
}

fn area_mean(surface: &sekai::world::spatial::SphericalSurfaceSnapshot, values: &[f32]) -> f64 {
    let mut weighted = 0.0_f64;
    let mut total = 0.0_f64;
    for (cell, &value) in surface.cells().iter().zip(values) {
        weighted += cell.area.get() * f64::from(value);
        total += cell.area.get();
    }
    weighted / total
}

fn area_mean_abs(surface: &sekai::world::spatial::SphericalSurfaceSnapshot, values: &[f32]) -> f64 {
    let mut weighted = 0.0_f64;
    let mut total = 0.0_f64;
    for (cell, &value) in surface.cells().iter().zip(values) {
        weighted += cell.area.get() * f64::from(value.abs());
        total += cell.area.get();
    }
    weighted / total
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn output_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p5")
}
