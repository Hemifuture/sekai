use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    spherical_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
    GeologicSpecArtifact, HydroErosionSpecArtifact, NaturalQualityArtifact, ReliefSpecArtifact,
    RulePackSetArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::SphericalSpaceArtifact;
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, NaturalQualityReport, QualityMetric,
    QualityMetricStatus, ReliefSpec, TectonicSpec, WorldFormationPreset, WorldFormationSpec,
    NATURAL_QUALITY_REPORT_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const QUALITY_SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

pub const EXPECTED_P0_METRIC_IDS: [&str; 9] = [
    "hydrology.outlet-area-coverage.v1",
    "hydrology.river-segment-count.v1",
    "quality.non-finite-value-count.v1",
    "relief.actual-land-area-fraction.v1",
    "relief.land-crust-jaccard.v1",
    "relief.oceanic-emergent-area-fraction.v1",
    "relief.requested-land-area-fraction.v1",
    "tectonics.continental-area-fraction.v1",
    "tectonics.continental-retention.v1",
];

const BASELINE_SCHEMA_V1: u16 = 1;
const REQUESTED_CELL_COUNT: u32 = 20_000;
const RESOLVED_CELL_COUNT: u32 = 20_252;
const INITIAL_PLATE_COUNT: u16 = 12;
const INITIAL_CONTINENTAL_FRACTION: f32 = 0.38;
const TARGET_LAND_FRACTION: f32 = 0.38;
const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NaturalQualityBaseline {
    pub schema_version: u16,
    pub scenario: NaturalQualityScenario,
    pub reports: Vec<SeedQualityReport>,
    pub aggregates: Vec<MetricAggregate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NaturalQualityScenario {
    pub protocol: String,
    pub profile: String,
    pub formation: String,
    pub radius_m: f64,
    pub requested_cell_count: u32,
    pub resolved_cell_count: u32,
    pub initial_plate_count: u16,
    pub initial_continental_crust_fraction: f32,
    pub target_land_fraction: f32,
    pub quality_report_schema_version: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeedQualityReport {
    pub seed: u64,
    pub report: NaturalQualityReport,
}

impl SeedQualityReport {
    pub const fn new(seed: u64, report: NaturalQualityReport) -> Self {
        Self { seed, report }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricAggregate {
    pub metric_id: String,
    pub min: Option<f64>,
    pub median: Option<f64>,
    pub max: Option<f64>,
    pub sample_count_sum: u64,
    pub pass_seed_count: u32,
    pub fail_seed_count: u32,
    pub unavailable_seed_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedNaturalQualityBaseline {
    json: Vec<u8>,
    csv: Vec<u8>,
}

impl RenderedNaturalQualityBaseline {
    pub fn json(&self) -> &[u8] {
        &self.json
    }

    pub fn csv(&self) -> &[u8] {
        &self.csv
    }
}

pub fn build_v4_quality_reports() -> Result<Vec<SeedQualityReport>, NaturalQualityBaselineError> {
    let graph = spherical_natural_foundation_graph()
        .map_err(|error| NaturalQualityBaselineError::Engine(error.to_string()))?;
    let engine = BuildEngine::new(graph);
    let mut reports = Vec::with_capacity(QUALITY_SEEDS.len());
    for seed in QUALITY_SEEDS {
        let started = Instant::now();
        let outcome = engine
            .build(
                RootSeed::new(seed),
                scenario_external_artifacts()?,
                &mut MemoryStageCache::new(),
            )
            .map_err(|error| NaturalQualityBaselineError::Build {
                seed,
                reason: error.to_string(),
            })?;
        let artifact = outcome
            .artifacts
            .get::<NaturalQualityArtifact>()
            .map_err(|error| NaturalQualityBaselineError::Engine(error.to_string()))?;
        artifact
            .report()
            .validate()
            .map_err(|error| NaturalQualityBaselineError::Invariant(error.to_string()))?;
        if artifact.report().surface_ref().cell_count() != RESOLVED_CELL_COUNT {
            return Err(NaturalQualityBaselineError::Invariant(format!(
                "seed {seed} resolved {} cells; expected {RESOLVED_CELL_COUNT}",
                artifact.report().surface_ref().cell_count()
            )));
        }
        let continental =
            metric_by_id(artifact.report(), "tectonics.continental-area-fraction.v1")?;
        let jaccard = metric_by_id(artifact.report(), "relief.land-crust-jaccard.v1")?;
        let failed = artifact
            .report()
            .metrics()
            .iter()
            .filter(|metric| metric.status() == QualityMetricStatus::Fail)
            .count();
        println!(
            "natural_quality_seed seed={seed} cells={RESOLVED_CELL_COUNT} continental={:?} land_crust_jaccard={:?} failed_metrics={failed} graph_ms={:.3}",
            continental.value(),
            jaccard.value(),
            started.elapsed().as_secs_f64() * 1_000.0,
        );
        reports.push(SeedQualityReport::new(seed, artifact.report().clone()));
    }
    Ok(reports)
}

pub fn render_v4_natural_quality_baseline(
    reports: &[SeedQualityReport],
) -> Result<RenderedNaturalQualityBaseline, NaturalQualityBaselineError> {
    validate_report_corpus(reports)?;
    let baseline = NaturalQualityBaseline {
        schema_version: BASELINE_SCHEMA_V1,
        scenario: NaturalQualityScenario {
            protocol: "sekai.spherical-natural-quality.v1".to_owned(),
            profile: "draft".to_owned(),
            formation: "continents".to_owned(),
            radius_m: EARTH_RADIUS_M,
            requested_cell_count: REQUESTED_CELL_COUNT,
            resolved_cell_count: RESOLVED_CELL_COUNT,
            initial_plate_count: INITIAL_PLATE_COUNT,
            initial_continental_crust_fraction: INITIAL_CONTINENTAL_FRACTION,
            target_land_fraction: TARGET_LAND_FRACTION,
            quality_report_schema_version: NATURAL_QUALITY_REPORT_SCHEMA_V1,
        },
        reports: reports.to_vec(),
        aggregates: aggregate_metrics(reports)?,
    };
    let mut json = serde_json::to_vec_pretty(&baseline)?;
    json.push(b'\n');
    let csv = render_csv(reports)?.into_bytes();
    Ok(RenderedNaturalQualityBaseline { json, csv })
}

pub fn natural_quality_output_paths() -> [PathBuf; 2] {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality");
    [root.join("v4-baseline.json"), root.join("v4-metrics.csv")]
}

pub fn write_v4_natural_quality_baseline(
    rendered: &RenderedNaturalQualityBaseline,
) -> Result<[PathBuf; 2], NaturalQualityBaselineError> {
    let paths = natural_quality_output_paths();
    std::fs::create_dir_all(paths[0].parent().expect("baseline path has a parent"))?;
    std::fs::write(&paths[0], rendered.json())?;
    std::fs::write(&paths[1], rendered.csv())?;
    Ok(paths)
}

fn scenario_external_artifacts() -> Result<ExternalArtifacts, NaturalQualityBaselineError> {
    let mut artifacts = ExternalArtifacts::new();
    insert(
        &mut artifacts,
        SphericalSpaceArtifact::new(SphericalSpaceSpec {
            radius: Meters::new(EARTH_RADIUS_M)
                .map_err(|error| NaturalQualityBaselineError::Engine(error.to_string()))?,
            target_cell_count: REQUESTED_CELL_COUNT,
        }),
    )?;
    insert(
        &mut artifacts,
        TectonicSpecArtifact::new(TectonicSpec {
            plate_count: INITIAL_PLATE_COUNT,
            continental_crust_fraction: INITIAL_CONTINENTAL_FRACTION,
            ..TectonicSpec::default()
        }),
    )?;
    insert(
        &mut artifacts,
        GeologicSpecArtifact::new(GeologicSpec::default()),
    )?;
    insert(
        &mut artifacts,
        ClimateSpecArtifact::new(ClimateSpec::default()),
    )?;
    insert(
        &mut artifacts,
        HydroErosionSpecArtifact::new(HydroErosionSpec::default()),
    )?;
    insert(
        &mut artifacts,
        ReliefSpecArtifact::new(ReliefSpec {
            target_land_fraction: TARGET_LAND_FRACTION,
            ..ReliefSpec::default()
        }),
    )?;
    insert(
        &mut artifacts,
        WorldFormationSpecArtifact::new(WorldFormationSpec {
            preset: WorldFormationPreset::Continents,
            ..WorldFormationSpec::default()
        }),
    )?;
    insert(
        &mut artifacts,
        RulePackSetArtifact::new(
            default_rule_pack_set()
                .map_err(|error| NaturalQualityBaselineError::Engine(error.to_string()))?,
        ),
    )?;
    insert(
        &mut artifacts,
        AuthorConstraintsArtifact::new(AuthorConstraints::default()),
    )?;
    Ok(artifacts)
}

fn insert<T: sekai::engine::Artifact>(
    artifacts: &mut ExternalArtifacts,
    artifact: T,
) -> Result<(), NaturalQualityBaselineError> {
    artifacts
        .insert(artifact)
        .map_err(|error| NaturalQualityBaselineError::Engine(error.to_string()))
}

fn validate_report_corpus(
    reports: &[SeedQualityReport],
) -> Result<(), NaturalQualityBaselineError> {
    if reports.len() != QUALITY_SEEDS.len() {
        return Err(NaturalQualityBaselineError::Invariant(format!(
            "baseline has {} seed reports; expected {}",
            reports.len(),
            QUALITY_SEEDS.len()
        )));
    }
    for (report, expected_seed) in reports.iter().zip(QUALITY_SEEDS) {
        if report.seed != expected_seed {
            return Err(NaturalQualityBaselineError::Invariant(format!(
                "baseline seed {} appears where {expected_seed} was expected",
                report.seed
            )));
        }
        report
            .report
            .validate()
            .map_err(|error| NaturalQualityBaselineError::Invariant(error.to_string()))?;
        let ids = report
            .report
            .metrics()
            .iter()
            .map(metric_id)
            .collect::<Vec<_>>();
        if ids != EXPECTED_P0_METRIC_IDS {
            return Err(NaturalQualityBaselineError::Invariant(format!(
                "seed {} metric inventory differs: {ids:?}",
                report.seed
            )));
        }
    }
    Ok(())
}

fn aggregate_metrics(
    reports: &[SeedQualityReport],
) -> Result<Vec<MetricAggregate>, NaturalQualityBaselineError> {
    EXPECTED_P0_METRIC_IDS
        .iter()
        .map(|&id| {
            let mut values = Vec::with_capacity(reports.len());
            let mut sample_count_sum = 0_u64;
            let mut pass_seed_count = 0_u32;
            let mut fail_seed_count = 0_u32;
            let mut unavailable_seed_count = 0_u32;
            for seed in reports {
                let metric = metric_by_id(&seed.report, id)?;
                sample_count_sum = sample_count_sum
                    .checked_add(u64::from(metric.sample_count()))
                    .ok_or_else(|| {
                        NaturalQualityBaselineError::Invariant(format!(
                            "metric {id} sample count overflowed"
                        ))
                    })?;
                match metric.status() {
                    QualityMetricStatus::Pass => pass_seed_count += 1,
                    QualityMetricStatus::Fail => fail_seed_count += 1,
                    QualityMetricStatus::Unavailable => unavailable_seed_count += 1,
                }
                if let Some(value) = metric.value() {
                    values.push(value);
                }
            }
            values.sort_by(f64::total_cmp);
            Ok(MetricAggregate {
                metric_id: id.to_owned(),
                min: values.first().copied(),
                median: median(&values),
                max: values.last().copied(),
                sample_count_sum,
                pass_seed_count,
                fail_seed_count,
                unavailable_seed_count,
            })
        })
        .collect()
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    })
}

fn render_csv(reports: &[SeedQualityReport]) -> Result<String, NaturalQualityBaselineError> {
    let mut csv = String::from("metric_id,seed,status,value,sample_count,min,max,reason\n");
    for id in EXPECTED_P0_METRIC_IDS {
        let mut by_seed = reports.iter().collect::<Vec<_>>();
        by_seed.sort_unstable_by_key(|report| report.seed);
        for seed in by_seed {
            let metric = metric_by_id(&seed.report, id)?;
            writeln!(
                csv,
                "{},{},{},{},{},{},{},{}",
                csv_field(id),
                seed.seed,
                status_name(metric.status()),
                metric
                    .value()
                    .map_or_else(String::new, |value| value.to_string()),
                metric.sample_count(),
                metric
                    .bounds()
                    .min()
                    .map_or_else(String::new, |value| value.to_string()),
                metric
                    .bounds()
                    .max()
                    .map_or_else(String::new, |value| value.to_string()),
                csv_field(metric.reason().unwrap_or("")),
            )
            .expect("writing to a String cannot fail");
        }
    }
    Ok(csv)
}

fn status_name(status: QualityMetricStatus) -> &'static str {
    match status {
        QualityMetricStatus::Pass => "pass",
        QualityMetricStatus::Fail => "fail",
        QualityMetricStatus::Unavailable => "unavailable",
    }
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn metric_by_id<'a>(
    report: &'a NaturalQualityReport,
    expected: &str,
) -> Result<&'a QualityMetric, NaturalQualityBaselineError> {
    report
        .metrics()
        .iter()
        .find(|metric| metric_id(metric) == expected)
        .ok_or_else(|| NaturalQualityBaselineError::Invariant(format!("missing metric {expected}")))
}

fn metric_id(metric: &QualityMetric) -> String {
    format!(
        "{}.{}.v{}",
        metric.id().namespace(),
        metric.id().name(),
        metric.id().version()
    )
}

#[derive(Debug, Error)]
pub enum NaturalQualityBaselineError {
    #[error("quality baseline engine failure: {0}")]
    Engine(String),
    #[error("quality baseline build failed for seed {seed}: {reason}")]
    Build { seed: u64, reason: String },
    #[error("quality baseline invariant failed: {0}")]
    Invariant(String),
    #[error("quality baseline serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("quality baseline output failed: {0}")]
    Io(#[from] std::io::Error),
}
