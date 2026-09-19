use std::fmt::Write as _;
use std::time::Instant;

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    evaluate_global_circulation_quality, ClimateWorkDomainBuilder, EvolvedTectonicGenerator,
    GeologicSubstrateGenerator, GlobalCirculationGenerator, GlobalClimateForcingBuilder,
    PrimaryReliefGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    latent_heat_flux_w_m2_from_evaporation_mm_day, ClimateModelProfile, ClimateSpec,
    ClimateWorkDomainSnapshot, GeologicSpec, GlobalCirculationSnapshot, NaturalQualityProfile,
    NaturalQualityReport, QualityMetricStatus, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2, CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2,
    CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2, CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2,
    CERES_EBAF_TOA_NET_RADIATION_GLOBAL_MEAN_W_M2, EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN,
    EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE,
    EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
    STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2, STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
    WILD_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2, WILD_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
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
    earth_references: EarthReferenceEvidence,
    corpus_earth_observations: CorpusEarthObservations,
}

#[derive(Serialize)]
struct SeedEvidence {
    seed: u64,
    snapshot_json_bytes: usize,
    snapshot_json_hash: String,
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
    published_global_precipitation_mm_day: f64,
    global_evaporation_mm_day: f64,
    evaporation_precipitation_relative_imbalance: f64,
    absorbed_shortwave_global_mean_w_m2: f64,
    outgoing_longwave_global_mean_w_m2: f64,
    toa_net_radiation_global_mean_w_m2: f64,
    planetary_albedo_global_mean: f64,
    latent_heat_flux_global_mean_w_m2: f64,
    precipitation_low_to_high_latitude_ratio: f64,
    precipitation_seasonal_hemisphere_phase_fraction: f64,
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

#[derive(Serialize)]
struct EarthReferenceEvidence {
    gpcp_global_precipitation_mm_day: f64,
    gpcp_relative_evidence_tolerance: f64,
    ceres_incoming_shortwave_global_mean_w_m2: f64,
    ceres_reflected_shortwave_global_mean_w_m2: f64,
    ceres_absorbed_shortwave_global_mean_w_m2: f64,
    ceres_outgoing_longwave_global_mean_w_m2: f64,
    ceres_toa_net_radiation_global_mean_w_m2: f64,
    ceres_planetary_albedo_global_mean: f64,
    wild_latent_heat_flux_min_w_m2: f64,
    wild_latent_heat_flux_max_w_m2: f64,
    stephens_latent_heat_flux_min_w_m2: f64,
    stephens_latent_heat_flux_max_w_m2: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct RangeEvidence {
    minimum_across_seeds: f64,
    mean_across_seeds: f64,
    maximum_across_seeds: f64,
}

#[derive(Serialize)]
struct ReferenceComparison {
    generated: RangeEvidence,
    reference: f64,
    mean_signed_deviation: f64,
    mean_relative_deviation: f64,
}

#[derive(Serialize)]
struct PrecipitationComparison {
    comparison: ReferenceComparison,
    evidence_minimum_mm_day: f64,
    evidence_maximum_mm_day: f64,
    seeds_inside_evidence_envelope: usize,
    corpus_mean_inside_evidence_envelope: bool,
}

#[derive(Serialize)]
struct LatentHeatComparison {
    generated: RangeEvidence,
    wild_minimum_w_m2: f64,
    wild_maximum_w_m2: f64,
    corpus_mean_inside_wild_range: bool,
    stephens_minimum_w_m2: f64,
    stephens_maximum_w_m2: f64,
    corpus_mean_inside_stephens_range: bool,
}

#[derive(Serialize)]
struct CorpusEarthObservations {
    precipitation: PrecipitationComparison,
    evaporation: RangeEvidence,
    latent_heat_flux: LatentHeatComparison,
    absorbed_shortwave: ReferenceComparison,
    outgoing_longwave: ReferenceComparison,
    toa_net_radiation: ReferenceComparison,
    planetary_albedo: ReferenceComparison,
    precipitation_low_to_high_latitude_ratio: RangeEvidence,
    precipitation_seasonal_hemisphere_phase_fraction: RangeEvidence,
}

struct GeneratedWorld {
    relief: sekai::world::natural::PrimaryReliefSnapshot,
    snapshot: GlobalCirculationSnapshot,
    report: NaturalQualityReport,
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
        let budget = world.snapshot.budget_report();
        eprintln!(
            "P4 evidence seed={seed} land={:.4} residual={:.9} precipitation={:.6} evaporation={:.6} latent={:.3} toa={:.3} wind={:.6} current={:.6}",
            {
                let mut land_area = 0.0_f64;
                let mut total_area = 0.0_f64;
                for (cell, elevation) in surface
                    .cells()
                    .iter()
                    .zip(world.relief.elevation_m().iter().copied())
                {
                    let area = cell.area.get();
                    total_area += area;
                    if elevation >= 0.0 {
                        land_area += area;
                    }
                }
                land_area / total_area
            },
            world.snapshot.solve_report().final_residual(),
            budget.precipitation_global_mean_mm_day(),
            budget.evaporation_global_mean_mm_day(),
            latent_heat_flux_w_m2_from_evaporation_mm_day(
                budget.evaporation_global_mean_mm_day()
            ),
            budget.toa_net_radiation_global_mean_w_m2(),
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
                "P4 EVIDENCE-DEVIATION seed={seed} metric={} status={:?} value={:?} bounds={:?}",
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
        world.snapshot.validate().unwrap();
        world.report.validate().unwrap();
        let bytes = serde_json::to_vec(&world.snapshot).unwrap();
        let solve = world.snapshot.solve_report();
        let budget = world.snapshot.budget_report();
        seeds.push(SeedEvidence {
            seed,
            snapshot_json_bytes: bytes.len(),
            snapshot_json_hash: blake3::hash(&bytes).to_hex().to_string(),
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
            global_precipitation_mm_day: budget.precipitation_global_mean_mm_day(),
            published_global_precipitation_mm_day: scalar_mean(
                surface,
                world
                    .snapshot
                    .fields()
                    .monthly_precipitation_mm_day()
                    .values(),
            ),
            global_evaporation_mm_day: budget.evaporation_global_mean_mm_day(),
            evaporation_precipitation_relative_imbalance: budget
                .evaporation_precipitation_relative_imbalance(),
            absorbed_shortwave_global_mean_w_m2: budget.absorbed_shortwave_global_mean_w_m2(),
            outgoing_longwave_global_mean_w_m2: budget.outgoing_longwave_global_mean_w_m2(),
            toa_net_radiation_global_mean_w_m2: budget.toa_net_radiation_global_mean_w_m2(),
            planetary_albedo_global_mean: budget.planetary_albedo_global_mean(),
            latent_heat_flux_global_mean_w_m2: latent_heat_flux_w_m2_from_evaporation_mm_day(
                budget.evaporation_global_mean_mm_day(),
            ),
            precipitation_low_to_high_latitude_ratio: metric_value(
                &world.report,
                "precipitation-low-to-high-latitude-ratio",
            ),
            precipitation_seasonal_hemisphere_phase_fraction: metric_value(
                &world.report,
                "precipitation-seasonal-hemisphere-phase-fraction",
            ),
            metrics: metric_evidence(&world.report),
        });
    }

    let repeated = generate_world(&bundle, &domain, &formation, SEEDS[0]);
    assert_eq!(worlds[0].snapshot, repeated.snapshot);
    assert_eq!(worlds[0].report, repeated.report);
    let corpus_metrics = corpus_metric_evidence(&worlds);
    let earth_references = earth_reference_evidence();
    let corpus_earth_observations = corpus_earth_observations(&seeds);
    let evidence = P4Evidence {
        schema_version: 2,
        profile: NaturalQualityProfile::Draft,
        model: ClimateModelProfile::C2LayeredV1,
        integrator: "split-explicit-rk3-v1",
        algorithm_references: vec![
            "classic-third-order-runge-kutta",
            "thermodynamic-endpoint-before-frozen-slow-dynamics-split-explicit-rk3",
            "green-gauss-barth-jespersen-component-local-second-order-finite-volume",
            "pair-specific-extensive-layer-exchange",
            "conservative-spherical-polygon-remap",
            "depth-mean-full-gravity-boussinesq-steric-free-surface",
            "annual-mean-ape-eady-column-regular-reynolds-stress-with-zero-global-axial-torque",
            "positive-permeability-finite-volume-horizontal-eddy-viscosity",
            "paired-f32-exchange-projection-5e-7-balance-1e-3-flux-accuracy",
            "signed-quantized-external-source-sink-ledger",
            "bolton-saturation-and-lifting-condensation-level",
            "large-pond-neutral-bulk-surface-evaporation",
            "smith-barstad-lcl-limited-upslope-condensation",
            "speedy-large-scale-condensation-with-moist-enthalpy-saturation-adjustment",
            "machine-converged-bracketed-newton-phase-change-root",
        ],
        procedural_closures: vec![
            "accelerated-monthly-climatological-continuation",
            "continuous-saturated-path-orographic-condensation",
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
        earth_references,
        corpus_earth_observations,
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
        substrate.relative_permeability(),
        &ClimateSpec::default(),
        domain,
        &BuildCancellation::new(),
    )
    .unwrap();
    let snapshot = GlobalCirculationGenerator::generate(
        surface,
        domain,
        &forcing,
        ClimateModelProfile::C2LayeredV1,
        &BuildCancellation::new(),
    )
    .unwrap();
    let report =
        evaluate_global_circulation_quality(surface, &relief, &forcing, &snapshot).unwrap();
    GeneratedWorld {
        relief,
        snapshot,
        report,
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

fn metric_value(report: &NaturalQualityReport, name: &str) -> f64 {
    report
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == name)
        .and_then(|metric| metric.value())
        .unwrap_or_else(|| panic!("P4 evidence metric {name} is unavailable"))
}

fn earth_reference_evidence() -> EarthReferenceEvidence {
    EarthReferenceEvidence {
        gpcp_global_precipitation_mm_day: EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY,
        gpcp_relative_evidence_tolerance: EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE,
        ceres_incoming_shortwave_global_mean_w_m2: CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2,
        ceres_reflected_shortwave_global_mean_w_m2: CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2,
        ceres_absorbed_shortwave_global_mean_w_m2: CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2,
        ceres_outgoing_longwave_global_mean_w_m2: CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2,
        ceres_toa_net_radiation_global_mean_w_m2: CERES_EBAF_TOA_NET_RADIATION_GLOBAL_MEAN_W_M2,
        ceres_planetary_albedo_global_mean: EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN,
        wild_latent_heat_flux_min_w_m2: WILD_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
        wild_latent_heat_flux_max_w_m2: WILD_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2,
        stephens_latent_heat_flux_min_w_m2: STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
        stephens_latent_heat_flux_max_w_m2: STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2,
    }
}

fn corpus_earth_observations(seeds: &[SeedEvidence]) -> CorpusEarthObservations {
    let precipitation = seeds
        .iter()
        .map(|seed| seed.global_precipitation_mm_day)
        .collect::<Vec<_>>();
    let evaporation = seeds
        .iter()
        .map(|seed| seed.global_evaporation_mm_day)
        .collect::<Vec<_>>();
    let latent_heat = seeds
        .iter()
        .map(|seed| seed.latent_heat_flux_global_mean_w_m2)
        .collect::<Vec<_>>();
    let precipitation_evidence_minimum = EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY
        * (1.0 - EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE);
    let precipitation_evidence_maximum = EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY
        * (1.0 + EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE);
    let precipitation_comparison = ReferenceComparison::new(
        range_evidence(&precipitation),
        EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY,
    );
    let latent_heat_range = range_evidence(&latent_heat);
    CorpusEarthObservations {
        precipitation: PrecipitationComparison {
            seeds_inside_evidence_envelope: precipitation
                .iter()
                .filter(|value| {
                    (precipitation_evidence_minimum..=precipitation_evidence_maximum)
                        .contains(value)
                })
                .count(),
            corpus_mean_inside_evidence_envelope: (precipitation_evidence_minimum
                ..=precipitation_evidence_maximum)
                .contains(&precipitation_comparison.generated.mean_across_seeds),
            comparison: precipitation_comparison,
            evidence_minimum_mm_day: precipitation_evidence_minimum,
            evidence_maximum_mm_day: precipitation_evidence_maximum,
        },
        evaporation: range_evidence(&evaporation),
        latent_heat_flux: LatentHeatComparison {
            corpus_mean_inside_wild_range: (WILD_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2
                ..=WILD_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2)
                .contains(&latent_heat_range.mean_across_seeds),
            corpus_mean_inside_stephens_range: (STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2
                ..=STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2)
                .contains(&latent_heat_range.mean_across_seeds),
            generated: latent_heat_range,
            wild_minimum_w_m2: WILD_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
            wild_maximum_w_m2: WILD_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2,
            stephens_minimum_w_m2: STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2,
            stephens_maximum_w_m2: STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2,
        },
        absorbed_shortwave: ReferenceComparison::from_seeds(
            seeds,
            |seed| seed.absorbed_shortwave_global_mean_w_m2,
            CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2,
        ),
        outgoing_longwave: ReferenceComparison::from_seeds(
            seeds,
            |seed| seed.outgoing_longwave_global_mean_w_m2,
            CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2,
        ),
        toa_net_radiation: ReferenceComparison::from_seeds(
            seeds,
            |seed| seed.toa_net_radiation_global_mean_w_m2,
            CERES_EBAF_TOA_NET_RADIATION_GLOBAL_MEAN_W_M2,
        ),
        planetary_albedo: ReferenceComparison::from_seeds(
            seeds,
            |seed| seed.planetary_albedo_global_mean,
            EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN,
        ),
        precipitation_low_to_high_latitude_ratio: range_evidence(
            &seeds
                .iter()
                .map(|seed| seed.precipitation_low_to_high_latitude_ratio)
                .collect::<Vec<_>>(),
        ),
        precipitation_seasonal_hemisphere_phase_fraction: range_evidence(
            &seeds
                .iter()
                .map(|seed| seed.precipitation_seasonal_hemisphere_phase_fraction)
                .collect::<Vec<_>>(),
        ),
    }
}

impl ReferenceComparison {
    fn new(generated: RangeEvidence, reference: f64) -> Self {
        Self {
            mean_signed_deviation: generated.mean_across_seeds - reference,
            mean_relative_deviation: (generated.mean_across_seeds - reference) / reference,
            generated,
            reference,
        }
    }

    fn from_seeds(
        seeds: &[SeedEvidence],
        value: impl Fn(&SeedEvidence) -> f64,
        reference: f64,
    ) -> Self {
        Self::new(
            range_evidence(&seeds.iter().map(value).collect::<Vec<_>>()),
            reference,
        )
    }
}

fn range_evidence(values: &[f64]) -> RangeEvidence {
    assert!(!values.is_empty(), "P4 corpus evidence needs observations");
    RangeEvidence {
        minimum_across_seeds: values.iter().copied().fold(f64::INFINITY, f64::min),
        mean_across_seeds: values.iter().sum::<f64>() / values.len() as f64,
        maximum_across_seeds: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

fn corpus_metric_evidence(worlds: &[GeneratedWorld]) -> Vec<CorpusMetricEvidence> {
    worlds[0]
        .report
        .metrics()
        .iter()
        .map(|metric| {
            let name = metric.id().name();
            let values = worlds
                .iter()
                .filter_map(|world| {
                    world
                        .report
                        .metrics()
                        .iter()
                        .find(|candidate| candidate.id().name() == name)
                        .and_then(|candidate| candidate.value())
                })
                .collect::<Vec<_>>();
            let range = range_evidence(&values);
            CorpusMetricEvidence {
                name: name.to_owned(),
                minimum_across_seeds: range.minimum_across_seeds,
                mean_across_seeds: range.mean_across_seeds,
                maximum_across_seeds: range.maximum_across_seeds,
                passing_seed_count: worlds
                    .iter()
                    .filter(|world| {
                        world.report.metrics().iter().any(|candidate| {
                            candidate.id().name() == name
                                && candidate.status() == QualityMetricStatus::Pass
                        })
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
            "corpus,,{},,{:.17},{},,,{:?}",
            metric.name,
            metric.mean_across_seeds,
            evidence.seeds.len(),
            metric.passing_seed_count,
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
