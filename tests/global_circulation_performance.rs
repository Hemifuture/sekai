use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use sekai::engine::{
    derive_stage_seed, BuildCancellation, BuildEngine, Diagnostic, ExternalArtifacts,
    MemoryStageCache, StageIdentity, StageRng,
};
use sekai::generators::natural::{
    global_circulation_graph, ClimateWorkDomainArtifact, ClimateWorkDomainBuilder,
    EvolvedTectonicGenerator, GeologicSubstrateGenerator, GlobalCirculationArtifact,
    GlobalCirculationGenerationError, GlobalCirculationGenerator, GlobalCirculationPhase,
    GlobalClimateForcing, GlobalClimateForcingBuilder, NaturalQualityProfileArtifact,
    PrimaryReliefArtifact, PrimaryReliefGenerator, ReliefSpecArtifact, ResolvedClimateInput,
    ResolvedClimateInputArtifact, ResolvedGeologicInput, ResolvedGeologicInputArtifact,
    ResolvedTectonicInput, ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
};
use sekai::generators::spatial::{
    ProfileSurfaceBuilder, ProfileSurfaceBundle, SphericalSurfaceArtifact,
};
use sekai::rules::{ClimateModel, GeologicModel, TectonicModel};
use sekai::world::natural::{
    ClimateModelProfile, ClimateSpec, ClimateWorkDomainSnapshot, EvolvedTectonicSnapshot,
    GeologicSpec, GeologicSubstrateSnapshot, GlobalCirculationSnapshot, NaturalQualityProfile,
    PrimaryReliefSnapshot, ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    TectonicSpec, WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};
use serde::{Deserialize, Serialize};

const RADIUS_M: f64 = 6_371_000.0;
const MEMORY_LIMIT_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Serialize)]
struct PerformanceEvidence {
    schema_version: u16,
    machine_profile: &'static str,
    runs: Vec<ClimatePerformance>,
    isolated_high_c2_rss: ClimatePerformance,
    cancellation: CancellationPerformance,
    cache: CachePerformance,
}

#[derive(Serialize, Deserialize)]
struct ClimatePerformance {
    quality_profile: NaturalQualityProfile,
    climate_profile: ClimateModelProfile,
    authoritative_cells: usize,
    climate_face_resolution: u16,
    climate_cells: usize,
    upstream_setup_micros: u128,
    generation_micros: u128,
    generation_limit_micros: u128,
    formation_cycles: u16,
    final_residual: f64,
    dense_state_bytes: u64,
    dense_state_limit_bytes: u64,
    baseline_rss_bytes: u64,
    peak_rss_bytes: u64,
    peak_rss_delta_bytes: u64,
}

#[derive(Serialize)]
struct CancellationPerformance {
    quality_profile: NaturalQualityProfile,
    climate_profile: ClimateModelProfile,
    requested_after_micros: u128,
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
    result_hash: String,
}

struct PreparedClimate {
    bundle: ProfileSurfaceBundle,
    domain: ClimateWorkDomainSnapshot,
    forcing: GlobalClimateForcing,
    _upstream_evolved: Option<EvolvedTectonicSnapshot>,
    _upstream_substrate: Option<GeologicSubstrateSnapshot>,
    upstream_relief: Option<PrimaryReliefSnapshot>,
    _upstream_diagnostics: Vec<Diagnostic>,
    setup_elapsed: Duration,
}

#[test]
#[ignore = "release-only C1/C2 time, memory, cancellation, and graph-cache evidence"]
fn measure_global_circulation_performance() {
    let (draft, cache) = prepare_draft_from_cold_graph();
    let mut runs = vec![measure_generation(
        NaturalQualityProfile::Draft,
        ClimateModelProfile::C1SingleLayerV1,
        &draft,
        Duration::from_secs(10),
    )];

    let standard = prepare_climate(NaturalQualityProfile::Standard, 42);
    runs.push(measure_generation(
        NaturalQualityProfile::Standard,
        ClimateModelProfile::C2LayeredV1,
        &standard,
        Duration::from_secs(30),
    ));
    drop(standard);

    let high = prepare_climate(NaturalQualityProfile::High, 42);
    runs.push(measure_generation(
        NaturalQualityProfile::High,
        ClimateModelProfile::C2LayeredV1,
        &high,
        Duration::from_secs(120),
    ));
    let cancellation = measure_high_cancellation(&high);
    let isolated_high_c2_rss = measure_high_c2_rss_in_fresh_process();

    let evidence = PerformanceEvidence {
        schema_version: 1,
        machine_profile: "release-wall-clock-plus-isolated-high-rss",
        runs,
        isolated_high_c2_rss,
        cancellation,
        cache,
    };
    let bytes = serde_json::to_vec_pretty(&evidence).unwrap();
    let directory = output_directory();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("performance.json"), &bytes).unwrap();
    eprintln!(
        "P4 performance bytes={} hash={}",
        bytes.len(),
        blake3::hash(&bytes).to_hex()
    );
}

#[test]
#[ignore = "release-only focused High/C2 active-cancellation regression gate"]
fn high_c2_active_cancellation_stays_below_250_ms() {
    let high = prepare_climate(NaturalQualityProfile::High, 42);
    let evidence = measure_high_cancellation(&high);
    assert!(evidence.latency_micros <= evidence.latency_limit_micros);
}

#[test]
#[ignore = "release-only High/C2 RSS gate executed in a fresh child process"]
fn high_c2_rss_gate_runs_in_fresh_process() {
    const CHILD_ENV: &str = "SEKAI_P4_HIGH_RSS_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let high = prepare_climate(NaturalQualityProfile::High, 42);
        let evidence = measure_generation(
            NaturalQualityProfile::High,
            ClimateModelProfile::C2LayeredV1,
            &high,
            Duration::from_secs(120),
        );
        let bytes = serde_json::to_vec_pretty(&evidence).unwrap();
        let directory = output_directory();
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("high-c2-rss.json"), bytes).unwrap();
        return;
    }

    let evidence = measure_high_c2_rss_in_fresh_process();
    assert_eq!(evidence.quality_profile, NaturalQualityProfile::High);
    assert_eq!(evidence.climate_profile, ClimateModelProfile::C2LayeredV1);
    assert!(evidence.peak_rss_delta_bytes <= MEMORY_LIMIT_BYTES);
}

fn measure_high_c2_rss_in_fresh_process() -> ClimatePerformance {
    const CHILD_ENV: &str = "SEKAI_P4_HIGH_RSS_CHILD";
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "high_c2_rss_gate_runs_in_fresh_process",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .status()
        .expect("spawn the isolated High/C2 RSS test process");
    assert!(status.success(), "isolated High/C2 RSS gate failed");
    let bytes = std::fs::read(output_directory().join("high-c2-rss.json"))
        .expect("isolated High/C2 RSS evidence");
    serde_json::from_slice(&bytes).expect("valid isolated High/C2 RSS evidence")
}

fn measure_generation(
    quality_profile: NaturalQualityProfile,
    climate_profile: ClimateModelProfile,
    prepared: &PreparedClimate,
    limit: Duration,
) -> ClimatePerformance {
    enum MeasuredClimate {
        Raw(GlobalCirculationSnapshot),
        Product(GlobalCirculationArtifact),
    }

    let surface = prepared.bundle.authoritative_surface();
    let baseline_rss_bytes = process_working_set_bytes()
        .expect("P4 memory acceptance requires Windows or Linux RSS counters");
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
    let cancellation = BuildCancellation::new();
    let measured = match climate_profile {
        ClimateModelProfile::C1SingleLayerV1 => MeasuredClimate::Raw(
            GlobalCirculationGenerator::generate(
                surface,
                &prepared.domain,
                &prepared.forcing,
                climate_profile,
                &cancellation,
            )
            .unwrap(),
        ),
        ClimateModelProfile::C2LayeredV1 => MeasuredClimate::Product(
            GlobalCirculationArtifact::generate(
                surface,
                &prepared.domain,
                &prepared.forcing,
                prepared
                    .upstream_relief
                    .as_ref()
                    .expect("C2 performance retains its authoritative relief"),
                &cancellation,
            )
            .unwrap(),
        ),
    };
    let snapshot = match &measured {
        MeasuredClimate::Raw(snapshot) => snapshot,
        MeasuredClimate::Product(artifact) => artifact.snapshot(),
    };
    let elapsed = started.elapsed();
    if let Some(bytes) = process_working_set_bytes() {
        peak.fetch_max(bytes, Ordering::Relaxed);
    }
    running.store(false, Ordering::Release);
    sampler.join().unwrap();
    let peak_rss_bytes = peak.load(Ordering::Relaxed);
    let peak_rss_delta_bytes = peak_rss_bytes
        .checked_sub(baseline_rss_bytes)
        .expect("sampled RSS peak must not precede its baseline");
    assert!(
        peak_rss_delta_bytes > 0,
        "P4 RSS gate requires an observed live-allocation increase above the retained-input baseline"
    );
    assert!(
        elapsed <= limit,
        "{quality_profile:?}/{climate_profile:?} generation took {elapsed:?}, limit {limit:?}"
    );
    assert!(
        snapshot.solve_report().dense_state_bytes() <= MEMORY_LIMIT_BYTES,
        "{quality_profile:?}/{climate_profile:?} reported {} bytes, limit {MEMORY_LIMIT_BYTES}",
        snapshot.solve_report().dense_state_bytes()
    );
    assert!(
        peak_rss_delta_bytes <= MEMORY_LIMIT_BYTES,
        "{quality_profile:?}/{climate_profile:?} sampled RSS delta {peak_rss_delta_bytes} bytes, limit {MEMORY_LIMIT_BYTES}"
    );
    eprintln!(
        "P4 performance profile={quality_profile:?} climate={climate_profile:?} authority={} n={} climate_cells={} setup={:?} generation={elapsed:?} cycles={} residual={} bytes={} rss_baseline={} rss_peak={} rss_delta={}",
        surface.cells().len(),
        prepared.domain.face_resolution(),
        prepared.domain.climate_surface().cells().len(),
        prepared.setup_elapsed,
        snapshot.solve_report().formation_cycles(),
        snapshot.solve_report().final_residual(),
        snapshot.solve_report().dense_state_bytes(),
        baseline_rss_bytes,
        peak_rss_bytes,
        peak_rss_delta_bytes,
    );
    ClimatePerformance {
        quality_profile,
        climate_profile,
        authoritative_cells: surface.cells().len(),
        climate_face_resolution: prepared.domain.face_resolution(),
        climate_cells: prepared.domain.climate_surface().cells().len(),
        upstream_setup_micros: prepared.setup_elapsed.as_micros(),
        generation_micros: elapsed.as_micros(),
        generation_limit_micros: limit.as_micros(),
        formation_cycles: snapshot.solve_report().formation_cycles(),
        final_residual: snapshot.solve_report().final_residual(),
        dense_state_bytes: snapshot.solve_report().dense_state_bytes(),
        dense_state_limit_bytes: MEMORY_LIMIT_BYTES,
        baseline_rss_bytes,
        peak_rss_bytes,
        peak_rss_delta_bytes,
    }
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

fn measure_high_cancellation(prepared: &PreparedClimate) -> CancellationPerformance {
    let cancellation = BuildCancellation::new();
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
    let latency_limit = Duration::from_millis(250);
    let (requested_after, latency) = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let mut triggered = false;
            GlobalCirculationGenerator::generate_with_phase_observer(
                prepared.bundle.authoritative_surface(),
                &prepared.domain,
                &prepared.forcing,
                ClimateModelProfile::C2LayeredV1,
                &cancellation,
                |phase| {
                    if phase == GlobalCirculationPhase::FastSubstepCompleted && !triggered {
                        triggered = true;
                        entered_tx.send(()).unwrap();
                    }
                },
            )
        });
        entered_rx
            .recv_timeout(Duration::from_secs(120))
            .expect("High/C2 solver completed a fast substep");
        let active_since = Instant::now();
        let observations = cancellation.observation_count();
        let progress_deadline = Instant::now() + Duration::from_secs(5);
        while cancellation.observation_count() < observations + 8 {
            assert!(
                Instant::now() < progress_deadline,
                "High/C2 solver stopped polling after a completed fast substep"
            );
            std::thread::yield_now();
        }
        std::thread::sleep(Duration::from_micros(100));
        let requested_after = active_since.elapsed();
        let cancellation_started = Instant::now();
        cancellation.cancel();
        let result = worker.join().unwrap();
        let latency = cancellation_started.elapsed();
        assert_eq!(result, Err(GlobalCirculationGenerationError::Cancelled));
        (requested_after, latency)
    });
    assert!(
        latency <= latency_limit,
        "High/C2 active cancellation took {latency:?}, limit {latency_limit:?}"
    );
    eprintln!("P4 High/C2 cancellation requested_after={requested_after:?} latency={latency:?}");
    CancellationPerformance {
        quality_profile: NaturalQualityProfile::High,
        climate_profile: ClimateModelProfile::C2LayeredV1,
        requested_after_micros: requested_after.as_micros(),
        latency_micros: latency.as_micros(),
        latency_limit_micros: latency_limit.as_micros(),
    }
}

fn prepare_draft_from_cold_graph() -> (PreparedClimate, CachePerformance) {
    let setup_started = Instant::now();
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let engine = BuildEngine::new(global_circulation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let cold_started = Instant::now();
    let cold = engine
        .build(
            RootSeed::new(42),
            p4_external(NaturalQualityProfile::Draft, surface.clone()),
            &mut cache,
        )
        .unwrap();
    let cold_elapsed = cold_started.elapsed();
    let warm_started = Instant::now();
    let warm = engine
        .build(
            RootSeed::new(42),
            p4_external(NaturalQualityProfile::Draft, surface.clone()),
            &mut cache,
        )
        .unwrap();
    let warm_elapsed = warm_started.elapsed();
    assert_eq!(
        (cold.report.cache_hits(), cold.report.cache_misses()),
        (0, 5)
    );
    assert_eq!(
        (warm.report.cache_hits(), warm.report.cache_misses()),
        (5, 0)
    );
    assert_eq!(cold.report.result_hash(), warm.report.result_hash());

    let relief = cold
        .artifacts
        .get::<PrimaryReliefArtifact>()
        .unwrap()
        .snapshot()
        .clone();
    let domain = cold
        .artifacts
        .get::<ClimateWorkDomainArtifact>()
        .unwrap()
        .snapshot()
        .clone();
    let forcing = GlobalClimateForcingBuilder::build(
        surface,
        &relief,
        &ClimateSpec::default(),
        &domain,
        &BuildCancellation::new(),
    )
    .unwrap();
    let artifact = cold.artifacts.get::<GlobalCirculationArtifact>().unwrap();
    artifact.snapshot().validate_against(surface).unwrap();
    let result_hash = hex(cold.report.result_hash().unwrap().as_bytes());
    eprintln!(
        "P4 graph cache cold={cold_elapsed:?} hits={} misses={} warm={warm_elapsed:?} hits={} misses={} hash={result_hash}",
        cold.report.cache_hits(),
        cold.report.cache_misses(),
        warm.report.cache_hits(),
        warm.report.cache_misses(),
    );
    let prepared = PreparedClimate {
        bundle,
        domain,
        forcing,
        _upstream_evolved: None,
        _upstream_substrate: None,
        upstream_relief: Some(relief),
        _upstream_diagnostics: Vec::new(),
        setup_elapsed: setup_started.elapsed(),
    };
    let cache = CachePerformance {
        cold_micros: cold_elapsed.as_micros(),
        warm_micros: warm_elapsed.as_micros(),
        cold_hits: cold.report.cache_hits(),
        cold_misses: cold.report.cache_misses(),
        warm_hits: warm.report.cache_hits(),
        warm_misses: warm.report.cache_misses(),
        result_hash,
    };
    (prepared, cache)
}

fn prepare_climate(profile: NaturalQualityProfile, seed: u64) -> PreparedClimate {
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
    PreparedClimate {
        bundle,
        domain,
        forcing,
        _upstream_evolved: Some(evolved),
        _upstream_substrate: Some(substrate),
        upstream_relief: Some(relief),
        _upstream_diagnostics: diagnostics,
        setup_elapsed: started.elapsed(),
    }
}

fn p4_external(
    profile: NaturalQualityProfile,
    surface: sekai::world::spatial::SphericalSurfaceSnapshot,
) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(NaturalQualityProfileArtifact::new(profile))
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
        .insert(SphericalSurfaceArtifact::new(surface))
        .unwrap();
    artifacts
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

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").unwrap();
    }
    output
}

fn output_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("p4")
}

#[test]
fn performance_evidence_path_is_isolated_under_target() {
    assert!(output_directory().ends_with("target/natural-quality/p4"));
}
