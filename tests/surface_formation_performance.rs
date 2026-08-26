use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sekai::engine::{
    derive_stage_seed, Artifact, BuildCancellation, BuildEngine, Diagnostic, ExternalArtifacts,
    MemoryStageCache, StageIdentity, StageRng,
};
use sekai::generators::natural::{
    surface_formation_graph, ClimateWorkDomainBuilder, EvolvedTectonicGenerator,
    GeologicSubstrateGenerator, GlobalCirculationGenerator, GlobalClimateForcingBuilder,
    NaturalQualityProfileArtifact, NaturalSurfaceFormationArtifact, PrimaryReliefGenerator,
    ReliefSpecArtifact, ResolvedClimateInput, ResolvedClimateInputArtifact, ResolvedGeologicInput,
    ResolvedGeologicInputArtifact, ResolvedHydroErosionInput, ResolvedHydroErosionInputArtifact,
    ResolvedTectonicInput, ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
    SurfaceFormationGenerationError, SurfaceFormationGenerator, SurfaceFormationInputs,
};
use sekai::generators::spatial::{
    ProfileSurfaceBuilder, ProfileSurfaceBundle, SphericalSurfaceArtifact,
};
use sekai::rules::{ClimateModel, GeologicModel, HydroErosionModel, TectonicModel};
use sekai::world::natural::{
    ClimateModelProfile, ClimateSpec, ClimateWorkDomainSnapshot, EvolvedTectonicSnapshot,
    GeologicSpec, GeologicSubstrateSnapshot, GlobalCirculationSnapshot, HydroErosionSpec,
    NaturalQualityProfile, PrimaryReliefSnapshot, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1, SURFACE_FORMATION_DENSE_STATE_BYTES_MAX,
};
use sekai::world::{Meters, RootSeed};
use serde::{Deserialize, Serialize};

const RADIUS_M: f64 = 6_371_000.0;
const MEMORY_LIMIT_BYTES: u64 = 1_024 * 1_024 * 1_024;
const CANCELLATION_LIMIT: Duration = Duration::from_millis(250);
const DRAFT_LIMIT_SECONDS: u64 = 15;
const STANDARD_LIMIT_SECONDS: u64 = 90;
const HIGH_LIMIT_SECONDS: u64 = 300;
/// Polls observed before the measurement cancels: high enough that the solve is
/// deep inside dense work, low enough to be reached long before it finishes.
const CANCELLATION_POLL_TARGET: u64 = 512;

#[derive(Serialize)]
struct PerformanceEvidence {
    schema_version: u16,
    machine_profile: &'static str,
    runs: Vec<FormationPerformance>,
    isolated_high_rss: FormationPerformance,
    cancellation: CancellationPerformance,
    cache: CachePerformance,
}

#[derive(Serialize, Deserialize)]
struct FormationPerformance {
    quality_profile: NaturalQualityProfile,
    authoritative_cells: usize,
    authoritative_edges: usize,
    upstream_setup_micros: u128,
    generation_micros: u128,
    generation_limit_micros: u128,
    within_declared_limit: bool,
    accepted_surface_substeps: u32,
    integrated_duration_years: f64,
    dense_state_bytes: u64,
    dense_state_limit_bytes: u64,
    baseline_rss_bytes: u64,
    peak_rss_bytes: u64,
    peak_rss_delta_bytes: u64,
}

#[derive(Serialize)]
struct CancellationPerformance {
    quality_profile: NaturalQualityProfile,
    observed_polls_before_cancel: u64,
    latency_micros: u128,
    latency_limit_micros: u128,
}

#[derive(Serialize)]
struct CachePerformance {
    cold_micros: u128,
    warm_micros: u128,
    cold_hits: usize,
    cold_misses: usize,
    warm_hits: usize,
    warm_misses: usize,
    identical_product_hash: bool,
}

struct PreparedFormation {
    bundle: ProfileSurfaceBundle,
    evolved: EvolvedTectonicSnapshot,
    substrate: GeologicSubstrateSnapshot,
    relief: PrimaryReliefSnapshot,
    domain: ClimateWorkDomainSnapshot,
    initial_climate: GlobalCirculationSnapshot,
    quality_profile: NaturalQualityProfile,
    climate_spec: ClimateSpec,
    formation_spec: HydroErosionSpec,
    setup_elapsed: Duration,
}

impl PreparedFormation {
    fn inputs(&self) -> SurfaceFormationInputs<'_> {
        SurfaceFormationInputs {
            surface: self.bundle.authoritative_surface(),
            quality_profile: self.quality_profile,
            tectonics: &self.evolved,
            substrate: &self.substrate,
            relief: &self.relief,
            domain: &self.domain,
            climate_spec: &self.climate_spec,
            initial_climate: &self.initial_climate,
            formation_spec: &self.formation_spec,
        }
    }
}

#[test]
#[ignore = "release-only P5 time, memory, cancellation, and graph-cache evidence"]
fn measure_surface_formation_performance() {
    let mut runs = Vec::new();
    for (profile, limit_secs) in [
        (NaturalQualityProfile::Draft, DRAFT_LIMIT_SECONDS),
        (NaturalQualityProfile::Standard, STANDARD_LIMIT_SECONDS),
        (NaturalQualityProfile::High, HIGH_LIMIT_SECONDS),
    ] {
        let prepared = prepare_formation(profile, 42);
        runs.push(measure_generation(
            &prepared,
            Duration::from_secs(limit_secs),
        ));
    }
    let draft = prepare_formation(NaturalQualityProfile::Draft, 42);
    let cancellation = measure_cancellation(&draft);
    let cache = measure_cache();
    let isolated_high_rss = measure_high_rss_in_fresh_process();

    let evidence = PerformanceEvidence {
        schema_version: 1,
        machine_profile: "release-wall-clock-plus-isolated-high-rss",
        runs,
        isolated_high_rss,
        cancellation,
        cache,
    };
    let bytes = serde_json::to_vec_pretty(&evidence).unwrap();
    let directory = output_directory();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("performance.json"), &bytes).unwrap();
    eprintln!(
        "P5 performance bytes={} hash={}",
        bytes.len(),
        blake3::hash(&bytes).to_hex()
    );
}

#[test]
#[ignore = "release-only Draft wall-clock gate"]
fn draft_wall_clock_stays_within_the_declared_gate() {
    assert_declared_wall_clock(NaturalQualityProfile::Draft, DRAFT_LIMIT_SECONDS);
}

#[test]
#[ignore = "release-only Standard wall-clock gate"]
fn standard_wall_clock_stays_within_the_declared_gate() {
    assert_declared_wall_clock(NaturalQualityProfile::Standard, STANDARD_LIMIT_SECONDS);
}

#[test]
#[ignore = "release-only High wall-clock gate"]
fn high_wall_clock_stays_within_the_declared_gate() {
    assert_declared_wall_clock(NaturalQualityProfile::High, HIGH_LIMIT_SECONDS);
}

#[test]
#[ignore = "release-only focused P5 active-cancellation regression gate"]
fn active_cancellation_stays_below_250_ms() {
    let prepared = prepare_formation(NaturalQualityProfile::High, 42);
    let evidence = measure_cancellation(&prepared);
    assert!(evidence.latency_micros <= evidence.latency_limit_micros);
}

#[test]
#[ignore = "release-only High P5 RSS gate executed in a fresh child process"]
fn high_rss_gate_runs_in_fresh_process() {
    const CHILD_ENV: &str = "SEKAI_P5_HIGH_RSS_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let prepared = prepare_formation(NaturalQualityProfile::High, 42);
        let evidence = measure_generation(&prepared, Duration::from_secs(300));
        let bytes = serde_json::to_vec_pretty(&evidence).unwrap();
        let directory = output_directory();
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("high-rss.json"), bytes).unwrap();
        return;
    }

    let evidence = measure_high_rss_in_fresh_process();
    assert_eq!(evidence.quality_profile, NaturalQualityProfile::High);
    assert!(evidence.peak_rss_delta_bytes <= MEMORY_LIMIT_BYTES);
    assert!(evidence.dense_state_bytes <= SURFACE_FORMATION_DENSE_STATE_BYTES_MAX);
}

#[test]
fn performance_evidence_path_is_isolated_under_target() {
    assert!(output_directory().ends_with("target/natural-quality/p5"));
}

fn measure_high_rss_in_fresh_process() -> FormationPerformance {
    const CHILD_ENV: &str = "SEKAI_P5_HIGH_RSS_CHILD";
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "high_rss_gate_runs_in_fresh_process",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .status()
        .expect("spawn the isolated High P5 RSS test process");
    assert!(status.success(), "isolated High P5 RSS gate failed");
    let bytes = std::fs::read(output_directory().join("high-rss.json"))
        .expect("isolated High RSS evidence");
    serde_json::from_slice(&bytes).expect("valid isolated High RSS evidence")
}

fn measure_generation(prepared: &PreparedFormation, limit: Duration) -> FormationPerformance {
    let surface = prepared.bundle.authoritative_surface();
    let baseline_rss_bytes = process_working_set_bytes()
        .expect("P5 memory acceptance requires Windows or Linux RSS counters");
    let running = Arc::new(AtomicBool::new(true));
    let peak = Arc::new(AtomicU64::new(baseline_rss_bytes));
    let sampler = {
        let running = Arc::clone(&running);
        let peak = Arc::clone(&peak);
        std::thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                if let Some(bytes) = process_working_set_bytes() {
                    peak.fetch_max(bytes, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let started = Instant::now();
    let artifact =
        NaturalSurfaceFormationArtifact::generate(prepared.inputs(), &BuildCancellation::new())
            .unwrap();
    let generation = started.elapsed();
    running.store(false, Ordering::Release);
    sampler.join().unwrap();
    artifact.validate().unwrap();

    let peak_rss_bytes = peak.load(Ordering::Relaxed);
    let report = artifact.snapshot().evolution_report();
    let evidence = FormationPerformance {
        quality_profile: prepared.quality_profile,
        authoritative_cells: surface.cells().len(),
        authoritative_edges: surface.edges().len(),
        upstream_setup_micros: prepared.setup_elapsed.as_micros(),
        generation_micros: generation.as_micros(),
        generation_limit_micros: limit.as_micros(),
        within_declared_limit: generation <= limit,
        accepted_surface_substeps: report.accepted_surface_substeps(),
        integrated_duration_years: report.integrated_duration_years(),
        dense_state_bytes: report.dense_state_bytes(),
        dense_state_limit_bytes: SURFACE_FORMATION_DENSE_STATE_BYTES_MAX,
        baseline_rss_bytes,
        peak_rss_bytes,
        peak_rss_delta_bytes: peak_rss_bytes.saturating_sub(baseline_rss_bytes),
    };
    eprintln!(
        "P5 {:?} generation={:?} limit={limit:?} within_limit={} substeps={} dense={} rss_delta={}",
        evidence.quality_profile,
        generation,
        evidence.within_declared_limit,
        evidence.accepted_surface_substeps,
        evidence.dense_state_bytes,
        evidence.peak_rss_delta_bytes
    );
    assert!(evidence.dense_state_bytes <= SURFACE_FORMATION_DENSE_STATE_BYTES_MAX);
    assert!(evidence.peak_rss_delta_bytes <= MEMORY_LIMIT_BYTES);
    evidence
}

/// Records one profile's wall clock and asserts only its declared gate, so a
/// missed time budget is visible on its own instead of hiding the evidence
/// writer's remaining measurements.
fn assert_declared_wall_clock(profile: NaturalQualityProfile, limit_secs: u64) {
    let prepared = prepare_formation(profile, 42);
    let evidence = measure_generation(&prepared, Duration::from_secs(limit_secs));
    assert!(
        evidence.within_declared_limit,
        "P5 {profile:?} generation took {} us; the declared gate is {} us",
        evidence.generation_micros, evidence.generation_limit_micros
    );
}

fn measure_cancellation(prepared: &PreparedFormation) -> CancellationPerformance {
    let cancellation = BuildCancellation::new();
    let (observed_polls, latency) = std::thread::scope(|scope| {
        let worker =
            scope.spawn(|| SurfaceFormationGenerator::generate(prepared.inputs(), &cancellation));
        let deadline = Instant::now() + Duration::from_secs(300);
        while cancellation.observation_count() < CANCELLATION_POLL_TARGET {
            assert!(
                Instant::now() < deadline,
                "the P5 solve never reached steady cancellation polling"
            );
            std::thread::yield_now();
        }
        let observed_polls = cancellation.observation_count();
        let started = Instant::now();
        cancellation.cancel();
        let result = worker.join().unwrap();
        let latency = started.elapsed();
        assert!(matches!(
            result,
            Err(SurfaceFormationGenerationError::Cancelled)
        ));
        (observed_polls, latency)
    });
    eprintln!("P5 cancellation latency={latency:?} polls={observed_polls}");
    assert!(
        latency <= CANCELLATION_LIMIT,
        "P5 cancellation took {latency:?}; limit is {CANCELLATION_LIMIT:?}"
    );
    CancellationPerformance {
        quality_profile: prepared.quality_profile,
        observed_polls_before_cancel: observed_polls,
        latency_micros: latency.as_micros(),
        latency_limit_micros: CANCELLATION_LIMIT.as_micros(),
    }
}

fn measure_cache() -> CachePerformance {
    let surface = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap()
    .authoritative_surface()
    .clone();
    let engine = BuildEngine::new(surface_formation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let cold_started = Instant::now();
    let cold = engine
        .build(RootSeed::new(42), external(&surface), &mut cache)
        .unwrap();
    let cold_micros = cold_started.elapsed().as_micros();
    let warm_started = Instant::now();
    let warm = engine
        .build(RootSeed::new(42), external(&surface), &mut cache)
        .unwrap();
    let warm_micros = warm_started.elapsed().as_micros();
    let identical_product_hash = cold
        .artifacts
        .hash::<NaturalSurfaceFormationArtifact>()
        .unwrap()
        .as_bytes()
        == warm
            .artifacts
            .hash::<NaturalSurfaceFormationArtifact>()
            .unwrap()
            .as_bytes();
    assert!(identical_product_hash);
    assert_eq!(warm.report.cache_misses(), 0);
    CachePerformance {
        cold_micros,
        warm_micros,
        cold_hits: cold.report.cache_hits(),
        cold_misses: cold.report.cache_misses(),
        warm_hits: warm.report.cache_hits(),
        warm_misses: warm.report.cache_misses(),
        identical_product_hash,
    }
}

fn external(surface: &sekai::world::spatial::SphericalSurfaceSnapshot) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(NaturalQualityProfileArtifact::new(
            NaturalQualityProfile::Draft,
        ))
        .unwrap();
    artifacts
        .insert(ResolvedTectonicInputArtifact::new(
            ResolvedTectonicInput::new(TectonicModel::CurrentSliceV1, TectonicSpec::default())
                .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedWorldFormationArtifact::new(formation()))
        .unwrap();
    artifacts
        .insert(ResolvedGeologicInputArtifact::new(
            ResolvedGeologicInput::new(GeologicModel::CurrentSliceV1, GeologicSpec::default())
                .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ReliefSpecArtifact::new(ReliefSpec::default()))
        .unwrap();
    artifacts
        .insert(ResolvedClimateInputArtifact::new(
            ResolvedClimateInput::new(
                ClimateModel::SeasonalEnergyMoistureV1,
                ClimateSpec::default(),
            )
            .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedHydroErosionInputArtifact::new(
            ResolvedHydroErosionInput::new(
                HydroErosionModel::PriorityFloodStreamPowerV1,
                HydroErosionSpec::default(),
            )
            .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(SphericalSurfaceArtifact::new(surface.clone()))
        .unwrap();
    artifacts
}

fn prepare_formation(profile: NaturalQualityProfile, seed: u64) -> PreparedFormation {
    let started = Instant::now();
    let cancellation = BuildCancellation::new();
    let bundle =
        ProfileSurfaceBuilder::build(profile, Meters::new(RADIUS_M).unwrap(), &cancellation)
            .unwrap();
    let formation = formation();
    let mut evolved_rng = stage_rng(seed, "natural.evolved-tectonics", 5);
    let evolved = EvolvedTectonicGenerator::generate(
        &bundle,
        &TectonicSpec::default(),
        &formation,
        &mut evolved_rng,
    )
    .unwrap();
    let mut substrate_rng = stage_rng(seed, "natural.geologic-substrate", 1);
    let substrate = GeologicSubstrateGenerator::generate(
        bundle.authoritative_surface(),
        &evolved,
        &GeologicSpec::default(),
        &formation,
        &mut substrate_rng,
    )
    .unwrap();
    let mut relief_rng = stage_rng(seed, "natural.primary-relief", 1);
    let mut diagnostics = Vec::<Diagnostic>::new();
    let relief = PrimaryReliefGenerator::generate(
        bundle.authoritative_surface(),
        &evolved,
        &substrate,
        &ReliefSpec::default(),
        &mut relief_rng,
        &mut diagnostics,
    )
    .unwrap();
    let domain =
        ClimateWorkDomainBuilder::build(bundle.authoritative_surface(), profile, &cancellation)
            .unwrap();
    let forcing = GlobalClimateForcingBuilder::build(
        bundle.authoritative_surface(),
        &relief,
        &ClimateSpec::default(),
        &domain,
        &cancellation,
    )
    .unwrap();
    let initial_climate = GlobalCirculationGenerator::generate(
        bundle.authoritative_surface(),
        &domain,
        &forcing,
        ClimateModelProfile::C2LayeredV1,
        &cancellation,
    )
    .unwrap();
    PreparedFormation {
        bundle,
        evolved,
        substrate,
        relief,
        domain,
        initial_climate,
        quality_profile: profile,
        climate_spec: ClimateSpec::default(),
        formation_spec: HydroErosionSpec::default(),
        setup_elapsed: started.elapsed(),
    }
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn stage_rng(seed: u64, stage: &'static str, version: u32) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(stage, version, "sekai.core"),
    ))
}

#[cfg(windows)]
fn process_working_set_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    let succeeded = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    (succeeded != 0).then_some(counters.WorkingSetSize as u64)
}

#[cfg(target_os = "linux")]
fn process_working_set_bytes() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
        .map(|kilobytes| kilobytes * 1_024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn process_working_set_bytes() -> Option<u64> {
    None
}

fn output_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p5")
}
