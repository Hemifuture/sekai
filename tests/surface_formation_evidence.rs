use std::fmt::Write as _;
use std::time::Instant;

use sekai::engine::{
    derive_stage_seed, Artifact, BuildCancellation, Diagnostic, StageIdentity, StageRng,
};
use sekai::generators::natural::{
    evaluate_surface_formation_corpus_hypsometry, ClimateWorkDomainBuilder,
    EvolvedTectonicGenerator, GeologicSubstrateGenerator, GlobalCirculationGenerator,
    GlobalClimateForcingBuilder, NaturalSurfaceFormationArtifact, PrimaryReliefGenerator,
    SurfaceFormationInputs,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    ClimateModelProfile, ClimateSpec, ClimateWorkDomainSnapshot, GeologicSpec, HydroErosionSpec,
    NaturalQualityProfile, NaturalQualityReport, PrimaryReliefSnapshot, QualityMetricStatus,
    ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;
/// Envelope rows the frozen T0 calibration spec records as open (§11.3 R4):
/// their corpus medians are written to the evidence but not asserted. Both
/// are the lowest land: the P3 product meets them (p05 60 m, share 0.087) and
/// the first 100 kyr of P5 deposition raise the coastal cells by ~40 m.
const OPEN_ENVELOPE_ROWS: [&str; 2] = [
    "corpus-median-land-area-share-below-100m",
    "corpus-median-land-relief-p05-m",
];
const SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

#[derive(Serialize)]
struct P5Evidence {
    schema_version: u16,
    profile: NaturalQualityProfile,
    model: &'static str,
    horizon_years: f64,
    macro_step_years: f64,
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
    final_sea_level_m: f32,
    primary_land_fraction: f32,
    outer_iterations: u8,
    geomorphic_macro_steps: u16,
    final_elevation_rms_m: f64,
    final_receiver_changed_fraction: f64,
    final_log_discharge_rms: f64,
    final_sediment_thickness_rms_m: f64,
    final_coastline_area_changed_fraction: f64,
    final_normalized_residual: f64,
    dense_state_bytes: u64,
    produced_sediment_mass_kg: f64,
    land_lake_deposited_mass_kg: f64,
    shelf_deposited_mass_kg: f64,
    deep_ocean_delivery_mass_kg: f64,
    sediment_global_relative_error: f64,
    sediment_provenance_relative_error: f64,
    mean_fluvial_erosion_m: f64,
    mean_hillslope_erosion_m: f64,
    mean_routed_deposition_m: f64,
    mean_coastal_erosion_m: f64,
    mean_absolute_isostatic_response_m: f64,
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
    relief: PrimaryReliefSnapshot,
    artifact: NaturalSurfaceFormationArtifact,
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
    let domain =
        ClimateWorkDomainBuilder::build(surface, NaturalQualityProfile::Draft, &cancellation)
            .unwrap();

    let mut worlds = Vec::new();
    for seed in SEEDS {
        let world = generate_world(&bundle, &domain, seed);
        let report = world.artifact.snapshot().solve_report();
        eprintln!(
            "P5 evidence seed={seed} iterations={} residual={:.6}",
            report.outer_iterations(),
            report.final_residual().normalized_max()
        );
        for metric in world
            .artifact
            .quality_report()
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
                .quality_report()
                .metrics()
                .iter()
                .all(|metric| metric.status() == QualityMetricStatus::Pass),
            "P5 seed {seed} has a failed hard metric"
        );
        world.artifact.validate().unwrap();
        seeds.push(seed_evidence(surface, seed, world));
    }

    let repeated = generate_world(&bundle, &domain, SEEDS[0]);
    assert_eq!(
        worlds[0].artifact.snapshot().checkpoint().fingerprint(),
        repeated.artifact.snapshot().checkpoint().fingerprint()
    );
    assert_eq!(worlds[0].artifact, repeated.artifact);

    let corpus_metrics = corpus_metric_evidence(&worlds);
    assert!(corpus_metrics
        .iter()
        .all(|metric| metric.passing_seed_count == SEEDS.len()));
    let corpus_hypsometry = evaluate_surface_formation_corpus_hypsometry(
        &worlds
            .iter()
            .map(|world| world.artifact.quality_report().clone())
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
        schema_version: 1,
        profile: NaturalQualityProfile::Draft,
        model: "priority-flood-fastscape-sediment-hillslope-coast-isostasy-v1",
        horizon_years: sekai::world::natural::SURFACE_FORMATION_HORIZON_YEARS,
        macro_step_years: sekai::world::natural::SURFACE_FORMATION_MACRO_STEP_YEARS,
        algorithm_references: vec![
            "barnes-lehman-mulla-priority-flood",
            "braun-willett-o-n-implicit-downstream-stack-stream-power",
            "cordonnier-drainage-uplift-stream-power-coupling",
            "roering-kirchner-dietrich-nonlinear-hillslope-transport",
            "davy-lague-yuan-explicit-erosion-transport-deposition",
        ],
        procedural_closures: vec![
            "bounded-effective-formation-runoff-proxy",
            "bounded-annual-formation-precipitation-envelope",
            "thousand-year-endorheic-residence-horizon",
            "irregular-spherical-finite-volume-paired-hillslope-mass-packet",
            "capacity-limited-five-source-provenance-ledger",
            "map-scale-wind-current-coastal-exposure",
            "local-airy-loading-response-without-elastic-flexure",
            "bounded-four-iteration-climate-surface-fixed-point",
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
                metric: "fixed-point-normalized-residual",
                reason: "the two-pass modifier runs exactly one erosion pass between two \
                         hydrology solves and has no climate-surface fixed point",
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
    let snapshot = world.artifact.snapshot();
    let terrain = snapshot.terrain_fields();
    let components = terrain.elevation_components();
    let budget = snapshot.sediment_budget_report();
    let residual = snapshot.solve_report().final_residual();
    let bytes = serde_json::to_vec(&world.artifact).unwrap();
    SeedEvidence {
        seed,
        artifact_json_bytes: bytes.len(),
        artifact_json_hash: blake3::hash(&bytes).to_hex().to_string(),
        checkpoint_fingerprint: hex(*snapshot.checkpoint().fingerprint()),
        state_fingerprint: hex(*snapshot.checkpoint().state_fingerprint()),
        primary_sea_level_m: world.relief.sea_level_m(),
        final_sea_level_m: terrain.sea_level_m(),
        primary_land_fraction: world.relief.physical_land_fraction(),
        outer_iterations: snapshot.solve_report().outer_iterations(),
        geomorphic_macro_steps: snapshot.solve_report().geomorphic_macro_steps(),
        final_elevation_rms_m: residual.elevation_rms_m(),
        final_receiver_changed_fraction: residual.receiver_changed_fraction(),
        final_log_discharge_rms: residual.log_discharge_rms(),
        final_sediment_thickness_rms_m: residual.sediment_thickness_rms_m(),
        final_coastline_area_changed_fraction: residual.coastline_area_changed_fraction(),
        final_normalized_residual: residual.normalized_max(),
        dense_state_bytes: snapshot.solve_report().dense_state_bytes(),
        produced_sediment_mass_kg: budget.produced_mass_kg(),
        land_lake_deposited_mass_kg: budget.land_lake_deposited_mass_kg(),
        shelf_deposited_mass_kg: budget.shelf_deposited_mass_kg(),
        deep_ocean_delivery_mass_kg: budget.deep_ocean_delivery_mass_kg(),
        sediment_global_relative_error: budget.global_relative_error(),
        sediment_provenance_relative_error: budget
            .provenance_relative_errors()
            .iter()
            .copied()
            .fold(0.0_f64, f64::max),
        mean_fluvial_erosion_m: area_mean(surface, components.fluvial_erosion_m()),
        mean_hillslope_erosion_m: area_mean(surface, components.hillslope_erosion_m()),
        mean_routed_deposition_m: area_mean(surface, components.routed_sediment_deposition_m()),
        mean_coastal_erosion_m: area_mean(surface, components.coastal_erosion_m()),
        mean_absolute_isostatic_response_m: area_mean_abs(
            surface,
            components.isostatic_response_m(),
        ),
        basin_count: snapshot.hydrology().basins().len(),
        lake_count: snapshot.hydrology().lakes().len(),
        river_segment_count: snapshot.hydrology().river_segments().len(),
        metrics: metric_evidence(world.artifact.quality_report()),
    }
}

fn generate_world(
    bundle: &ProfileSurfaceBundle,
    domain: &ClimateWorkDomainSnapshot,
    seed: u64,
) -> GeneratedWorld {
    let cancellation = BuildCancellation::new();
    let surface = bundle.authoritative_surface();
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let mut evolved_rng = stage_rng(seed, "natural.evolved-tectonics", 5);
    let evolved = EvolvedTectonicGenerator::generate(
        bundle,
        &TectonicSpec::default(),
        &formation,
        &mut evolved_rng,
    )
    .unwrap();
    let mut substrate_rng = stage_rng(seed, "natural.geologic-substrate", 1);
    let substrate = GeologicSubstrateGenerator::generate(
        surface,
        &evolved,
        &GeologicSpec::default(),
        &formation,
        &mut substrate_rng,
    )
    .unwrap();
    let mut relief_rng = stage_rng(seed, "natural.primary-relief", 1);
    let mut diagnostics = Vec::<Diagnostic>::new();
    let relief = PrimaryReliefGenerator::generate(
        surface,
        &evolved,
        &substrate,
        &ReliefSpec::default(),
        &mut relief_rng,
        &mut diagnostics,
    )
    .unwrap();
    let forcing = GlobalClimateForcingBuilder::build(
        surface,
        &relief,
        &ClimateSpec::default(),
        domain,
        &cancellation,
    )
    .unwrap();
    let initial_climate = GlobalCirculationGenerator::generate(
        surface,
        domain,
        &forcing,
        ClimateModelProfile::C2LayeredV1,
        &cancellation,
    )
    .unwrap();
    let artifact = NaturalSurfaceFormationArtifact::generate(
        SurfaceFormationInputs {
            surface,
            quality_profile: NaturalQualityProfile::Draft,
            tectonics: &evolved,
            substrate: &substrate,
            relief: &relief,
            domain,
            climate_spec: &ClimateSpec::default(),
            initial_climate: &initial_climate,
            formation_spec: &HydroErosionSpec::default(),
        },
        &cancellation,
    )
    .unwrap();
    GeneratedWorld { relief, artifact }
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
    let first = worlds[0].artifact.quality_report();
    first
        .metrics()
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            let values = worlds
                .iter()
                .filter_map(|world| world.artifact.quality_report().metrics()[index].value())
                .collect::<Vec<_>>();
            let passing = worlds
                .iter()
                .filter(|world| {
                    world.artifact.quality_report().metrics()[index].status()
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
        "seed,outer_iterations,normalized_residual,elevation_rms_m,receiver_changed_fraction,\
         log_discharge_rms,sediment_rms_m,coastline_changed_fraction,produced_mass_kg,\
         sediment_relative_error,provenance_relative_error,mean_fluvial_erosion_m,\
         mean_hillslope_erosion_m,mean_routed_deposition_m,basin_count,lake_count,river_segments"
    )
    .unwrap();
    for seed in &evidence.seeds {
        writeln!(
            csv,
            "{},{},{:.9},{:.6},{:.9},{:.9},{:.6},{:.9},{:.6e},{:.3e},{:.3e},{:.6},{:.6},{:.6},{},{},{}",
            seed.seed,
            seed.outer_iterations,
            seed.final_normalized_residual,
            seed.final_elevation_rms_m,
            seed.final_receiver_changed_fraction,
            seed.final_log_discharge_rms,
            seed.final_sediment_thickness_rms_m,
            seed.final_coastline_area_changed_fraction,
            seed.produced_sediment_mass_kg,
            seed.sediment_global_relative_error,
            seed.sediment_provenance_relative_error,
            seed.mean_fluvial_erosion_m,
            seed.mean_hillslope_erosion_m,
            seed.mean_routed_deposition_m,
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

fn stage_rng(seed: u64, stage: &'static str, version: u32) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(stage, version, "sekai.core"),
    ))
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
