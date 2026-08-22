use std::fmt::Write as _;
use std::time::Instant;

use sekai::engine::{derive_stage_seed, Artifact, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_global_circulation_quality, ClimateWorkDomainBuilder, EvolvedTectonicGenerator,
    GeologicSubstrateGenerator, GlobalCirculationArtifact, GlobalCirculationGenerator,
    GlobalClimateForcingBuilder, PrimaryReliefGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    ClimateModelProfile, ClimateSpec, ClimateWorkDomainSnapshot, GeologicSpec,
    GlobalCirculationSnapshot, NaturalQualityProfile, NaturalQualityReport, QualityMetricStatus,
    ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;
const SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

#[derive(Serialize)]
struct P4Evidence {
    schema_version: u16,
    profile: NaturalQualityProfile,
    model: ClimateModelProfile,
    integrator: &'static str,
    algorithm_references: Vec<&'static str>,
    procedural_closures: Vec<&'static str>,
    radius_m: f64,
    authoritative_cells: usize,
    climate_cells: usize,
    authoritative_fingerprint: String,
    climate_grid_fingerprint: String,
    seeds: Vec<SeedEvidence>,
    corpus_metrics: Vec<CorpusMetricEvidence>,
}

#[derive(Serialize)]
struct SeedEvidence {
    seed: u64,
    artifact_json_bytes: usize,
    artifact_json_hash: String,
    sea_level_m: f32,
    physical_land_fraction: f32,
    formation_cycles: u16,
    continuation_steps: u64,
    fast_substeps: u64,
    final_residual: f64,
    maximum_cfl: f64,
    dense_state_bytes: u64,
    atmosphere_mass_relative_error: f64,
    ocean_volume_relative_error: f64,
    moisture_relative_error: f64,
    energy_relative_error: f64,
    paired_exchange_relative_error: f64,
    near_surface_wind_rms_m_s: f64,
    surface_current_rms_m_s: f64,
    global_air_temperature_c: f64,
    global_precipitation_mm_day: f64,
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
    relief: sekai::world::natural::PrimaryReliefSnapshot,
    snapshot: GlobalCirculationSnapshot,
    report: NaturalQualityReport,
    artifact: GlobalCirculationArtifact,
}

#[test]
#[ignore = "release-only deterministic 17-seed P4 JSON/CSV evidence writer"]
fn write_global_circulation_evidence() {
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
    let formation = formation();
    let mut worlds = Vec::new();
    for seed in SEEDS {
        let world = generate_world(&bundle, &domain, &formation, seed);
        eprintln!(
            "P4 evidence seed={seed} residual={:.9} wind={:.6} current={:.6}",
            world.snapshot.solve_report().final_residual(),
            vector_rms(
                surface,
                world.snapshot.fields().near_surface_wind_m_s().values()
            ),
            vector_rms(
                surface,
                world.snapshot.fields().surface_ocean_current_m_s().values()
            ),
        );
        for metric in world
            .report
            .metrics()
            .iter()
            .filter(|metric| metric.status() != QualityMetricStatus::Pass)
        {
            eprintln!(
                "P4 FAILURE seed={seed} metric={} status={:?} value={:?} bounds={:?}",
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
                .report
                .metrics()
                .iter()
                .all(|metric| metric.status() == QualityMetricStatus::Pass),
            "P4 seed {seed} has a failed hard metric"
        );
        world.artifact.validate().unwrap();
        let bytes = serde_json::to_vec(&world.artifact).unwrap();
        let solve = world.snapshot.solve_report();
        let budget = world.snapshot.budget_report();
        seeds.push(SeedEvidence {
            seed,
            artifact_json_bytes: bytes.len(),
            artifact_json_hash: blake3::hash(&bytes).to_hex().to_string(),
            sea_level_m: world.relief.sea_level_m(),
            physical_land_fraction: world.relief.physical_land_fraction(),
            formation_cycles: solve.formation_cycles(),
            continuation_steps: solve.continuation_steps(),
            fast_substeps: solve.fast_substeps(),
            final_residual: solve.final_residual(),
            maximum_cfl: solve.maximum_cfl(),
            dense_state_bytes: solve.dense_state_bytes(),
            atmosphere_mass_relative_error: budget.atmosphere_mass_relative_error(),
            ocean_volume_relative_error: budget.ocean_volume_relative_error(),
            moisture_relative_error: budget.moisture_relative_error(),
            energy_relative_error: budget.energy_relative_error(),
            paired_exchange_relative_error: budget.paired_exchange_relative_error(),
            near_surface_wind_rms_m_s: vector_rms(
                surface,
                world.snapshot.fields().near_surface_wind_m_s().values(),
            ),
            surface_current_rms_m_s: vector_rms(
                surface,
                world.snapshot.fields().surface_ocean_current_m_s().values(),
            ),
            global_air_temperature_c: scalar_mean(
                surface,
                world.snapshot.fields().monthly_air_temperature_c().values(),
            ),
            global_precipitation_mm_day: scalar_mean(
                surface,
                world
                    .snapshot
                    .fields()
                    .monthly_precipitation_mm_day()
                    .values(),
            ),
            metrics: metric_evidence(&world.report),
        });
    }

    let repeated = generate_world(&bundle, &domain, &formation, SEEDS[0]);
    assert_eq!(worlds[0].snapshot, repeated.snapshot);
    assert_eq!(worlds[0].report, repeated.report);
    let corpus_metrics = corpus_metric_evidence(&worlds);
    assert!(corpus_metrics
        .iter()
        .all(|metric| metric.passing_seed_count == SEEDS.len()));
    let evidence = P4Evidence {
        schema_version: 1,
        profile: NaturalQualityProfile::Draft,
        model: ClimateModelProfile::C2LayeredV1,
        integrator: "split-explicit-rk3-v1",
        algorithm_references: vec![
            "classic-third-order-runge-kutta",
            "split-explicit-frozen-slow-dynamic-momentum-and-viscosity-rk3",
            "green-gauss-barth-jespersen-component-local-second-order-finite-volume",
            "pair-specific-extensive-layer-exchange",
            "conservative-spherical-polygon-remap",
            "depth-mean-full-gravity-boussinesq-steric-free-surface",
            "annual-mean-ape-eady-column-regular-reynolds-stress-with-zero-global-axial-torque",
            "positive-permeability-finite-volume-horizontal-eddy-viscosity",
            "paired-f32-exchange-projection-5e-7-balance-1e-3-flux-accuracy",
            "signed-quantized-external-source-sink-ledger",
        ],
        procedural_closures: vec![
            "accelerated-monthly-climatological-continuation",
            "resolved-upslope-orographic-condensation",
            "liquid-ocean-temperature-bound-with-sea-ice-unavailable",
            "bathymetry-scaled-thermocline-bottom-drag",
            "column-neutral-c2-first-baroclinic-pressure-mode",
            "area-weighted-fieldwise-nondimensional-annual-cycle-residual",
        ],
        radius_m: RADIUS_M,
        authoritative_cells: surface.cells().len(),
        climate_cells: domain.climate_surface().cells().len(),
        authoritative_fingerprint: hex(surface.fingerprint()),
        climate_grid_fingerprint: hex(*domain.climate_grid_fingerprint()),
        seeds,
        corpus_metrics,
    };
    let json = serde_json::to_vec_pretty(&evidence).unwrap();
    assert_eq!(json, serde_json::to_vec_pretty(&evidence).unwrap());
    let csv = render_csv(&evidence);
    let output = output_directory();
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("evidence.json"), &json).unwrap();
    std::fs::write(output.join("metrics.csv"), csv.as_bytes()).unwrap();
    eprintln!(
        "P4 evidence json_bytes={} json_hash={} csv_bytes={} csv_hash={} elapsed={:?}",
        json.len(),
        blake3::hash(&json).to_hex(),
        csv.len(),
        blake3::hash(csv.as_bytes()).to_hex(),
        started.elapsed(),
    );
}

fn generate_world(
    bundle: &ProfileSurfaceBundle,
    domain: &ClimateWorkDomainSnapshot,
    formation: &ResolvedWorldFormation,
    seed: u64,
) -> GeneratedWorld {
    let surface = bundle.authoritative_surface();
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
    let forcing = GlobalClimateForcingBuilder::build(
        surface,
        &relief,
        &ClimateSpec::default(),
        domain,
        &BuildCancellation::new(),
    )
    .unwrap();
    let artifact = GlobalCirculationArtifact::generate(
        surface,
        domain,
        &forcing,
        &relief,
        &BuildCancellation::new(),
    )
    .unwrap_or_else(|error| {
        let snapshot = GlobalCirculationGenerator::generate(
            surface,
            domain,
            &forcing,
            ClimateModelProfile::C2LayeredV1,
            &BuildCancellation::new(),
        )
        .unwrap_or_else(|diagnostic_error| {
            panic!(
                "P4 product seed {seed} rejected: {error}; diagnostic raw generation also failed: {diagnostic_error}"
            )
        });
        let report = evaluate_global_circulation_quality(surface, &relief, &forcing, &snapshot)
            .expect("diagnostic quality evaluation after product rejection");
        for metric in report.metrics() {
            eprintln!(
                "P4 diagnostic seed={seed} metric={} status={:?} value={:?} bounds={:?}",
                metric.id().name(),
                metric.status(),
                metric.value(),
                metric.bounds(),
            );
        }
        let wind = snapshot.fields().near_surface_wind_m_s().values();
        for month in 0..12 {
            let mut positive = 0_u32;
            let mut total = 0_u32;
            let mut zonal_sum = 0.0_f64;
            for (cell, values) in surface.cells().iter().zip(wind) {
                let radial = cell.centroid.components();
                let latitude = radial[2].asin().to_degrees().abs();
                if !(35.0..=60.0).contains(&latitude) {
                    continue;
                }
                let cosine = radial[0].hypot(radial[1]);
                let east = [-radial[1] / cosine, radial[0] / cosine, 0.0];
                let zonal = values[month]
                    .iter()
                    .zip(east)
                    .map(|(value, basis)| f64::from(*value) * basis)
                    .sum::<f64>();
                total += 1;
                positive += u32::from(zonal > 0.0);
                zonal_sum += zonal;
            }
            eprintln!(
                "P4 diagnostic seed={seed} month={month} mid_westerly_fraction={} mid_mean_zonal_m_s={}",
                f64::from(positive) / f64::from(total),
                zonal_sum / f64::from(total),
            );
        }
        panic!("P4 product seed {seed} rejected: {error}");
    });
    let snapshot = artifact.snapshot().clone();
    let report = artifact.quality_report().clone();
    GeneratedWorld {
        relief,
        snapshot,
        report,
        artifact,
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
        })
        .collect()
}

fn corpus_metric_evidence(worlds: &[GeneratedWorld]) -> Vec<CorpusMetricEvidence> {
    worlds[0]
        .report
        .metrics()
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            let values = worlds
                .iter()
                .map(|world| world.report.metrics()[index].value().unwrap())
                .collect::<Vec<_>>();
            CorpusMetricEvidence {
                name: metric.id().name().to_owned(),
                minimum_across_seeds: values.iter().copied().fold(f64::INFINITY, f64::min),
                mean_across_seeds: values.iter().sum::<f64>() / values.len() as f64,
                maximum_across_seeds: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                passing_seed_count: worlds
                    .iter()
                    .filter(|world| {
                        world.report.metrics()[index].status() == QualityMetricStatus::Pass
                    })
                    .count(),
            }
        })
        .collect()
}

fn scalar_mean(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    values: &[[f32; 12]],
) -> f64 {
    let area = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    surface
        .cells()
        .iter()
        .zip(values)
        .map(|(cell, months)| {
            cell.area.get() * months.iter().map(|value| f64::from(*value)).sum::<f64>() / 12.0
        })
        .sum::<f64>()
        / area
}

fn vector_rms(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    values: &[[[f32; 3]; 12]],
) -> f64 {
    let area = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    (surface
        .cells()
        .iter()
        .zip(values)
        .map(|(cell, months)| {
            cell.area.get()
                * months
                    .iter()
                    .map(|vector| {
                        vector
                            .iter()
                            .map(|value| f64::from(*value).powi(2))
                            .sum::<f64>()
                    })
                    .sum::<f64>()
                / 12.0
        })
        .sum::<f64>()
        / area)
        .sqrt()
}

fn render_csv(evidence: &P4Evidence) -> String {
    let mut csv = String::from(
        "scope,seed,metric_id,status,value,sample_count,minimum,maximum,passing_seed_count\n",
    );
    for seed in &evidence.seeds {
        for metric in &seed.metrics {
            writeln!(
                csv,
                "seed,{},{},{:?},{},{},{},{},",
                seed.seed,
                metric.id,
                metric.status,
                option(metric.value),
                metric.sample_count,
                option(metric.minimum),
                option(metric.maximum),
            )
            .unwrap();
        }
    }
    for metric in &evidence.corpus_metrics {
        writeln!(
            csv,
            "corpus,,{},Pass,{:.17},17,,,{:?}",
            metric.name, metric.mean_across_seeds, metric.passing_seed_count,
        )
        .unwrap();
    }
    csv
}

fn option(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.17}"))
}

fn output_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p4")
}

fn hex(bytes: [u8; 32]) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").unwrap();
    }
    encoded
}

#[test]
fn evidence_paths_are_isolated_under_target() {
    assert!(output_directory().ends_with("target/natural-quality/p4"));
}
