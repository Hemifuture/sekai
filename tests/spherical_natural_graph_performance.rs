use std::mem::size_of_val;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sekai::engine::{
    derive_stage_seed, BuildEngine, BuildReport, ExternalArtifacts, MemoryStageCache,
    StageIdentity, StageRng,
};
use sekai::generators::natural::{
    legacy_planar_natural_foundation_graph, spherical_natural_foundation_graph,
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, HydroErosionSpecArtifact,
    NaturalQualityArtifact, ReliefSpecArtifact, ResolvedWorldFormationArtifact,
    RulePackSetArtifact, SphericalGeologicArtifact, SphericalHydroErosionArtifact,
    SphericalMantleArtifact, SphericalPreliminaryClimateArtifact, SphericalReliefArtifact,
    SphericalTectonicArtifact, TectonicGenerator, TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{
    GeodesicVoronoiBuilder, PlanarSpaceArtifact, SphericalSpaceArtifact, SphericalSurfaceArtifact,
};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    spherical_natural_field_registry, ClimateSpec, GeologicSpec, HydroErosionSpec, LandOceanKind,
    ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, WorldFormationSpec, MAX_PLATE_COUNT, MIN_PLATE_COUNT,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed, SphericalSpaceSpec};
use serde::Serialize;

const ROOT_SEED: RootSeed = RootSeed::new(42);
const LAND_COMPLIANCE_SEEDS: [RootSeed; 5] = [
    RootSeed::new(3),
    RootSeed::new(7),
    RootSeed::new(11),
    RootSeed::new(19),
    RootSeed::new(42),
];
const TARGET_CELL_COUNT: u32 = 20_000;
const EARTH_RADIUS_M: f64 = 6_371_000.0;
const SPHERE_TIME_BUDGET: Duration = Duration::from_secs(5);
const SPHERE_TO_PLANAR_TIME_RATIO_BUDGET: f64 = 2.5;
const ADDITIONAL_PEAK_WORKING_SET_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
const MORPHOLOGY_TO_BASELINE_TIME_RATIO_BUDGET: f64 = 1.25;
const TECTONIC_TIME_BUDGET: Duration = Duration::from_millis(300);
const TECTONIC_PUBLICATION_TIME_BUDGET: Duration = Duration::from_secs(1);
const MORPHOLOGY_PEAK_WORKING_SET_BUDGET_BYTES: u64 = 64 * 1024 * 1024;
const BASELINE_DURATION_ENV: &str = "SEKAI_SPHERICAL_BASELINE_MS";
const MORPHOLOGY_PROBE_CHILD_ENV: &str = "SEKAI_MORPHOLOGY_PROBE_CHILD";
const MORPHOLOGY_PROBE_PREFIX: &str = "sekai_morphology_probe";

fn morphology_probe_child_requested(value: Option<&std::ffi::OsStr>) -> bool {
    value == Some(std::ffi::OsStr::new("1"))
}

#[derive(Debug, Clone, Copy)]
struct MorphologyPerformanceEvidence {
    tectonic_elapsed: Duration,
    formal_tectonic_elapsed: Duration,
    quality_elapsed: Duration,
    full_graph_elapsed: Duration,
    morphology_peak_delta_bytes: u64,
    cell_count: usize,
    plate_count: usize,
}

#[derive(Debug, Clone, Copy)]
struct MorphologyChildProbe {
    elapsed: Duration,
    peak_delta_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
struct ArtifactBytes {
    persistent: usize,
    serialized: usize,
}

impl ArtifactBytes {
    fn measure<T: Serialize>(artifact: &T, persistent: usize) -> Self {
        Self {
            persistent,
            serialized: serde_json::to_vec(artifact).unwrap().len(),
        }
    }
}

#[cfg(windows)]
fn process_working_set_bytes() -> Option<u64> {
    windows_process_memory_counters().map(|counters| counters.WorkingSetSize as u64)
}

#[cfg(windows)]
fn process_peak_working_set_bytes() -> Option<u64> {
    windows_process_memory_counters().map(|counters| counters.PeakWorkingSetSize as u64)
}

#[cfg(windows)]
fn windows_process_memory_counters(
) -> Option<windows_sys::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // Both calls target this test process and write exactly the declared C layout.
    let succeeded = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    (succeeded != 0).then_some(counters)
}

#[cfg(target_os = "linux")]
fn process_working_set_bytes() -> Option<u64> {
    linux_process_status_bytes("VmRSS:")
}

#[cfg(target_os = "linux")]
fn process_peak_working_set_bytes() -> Option<u64> {
    linux_process_status_bytes("VmHWM:")
}

#[cfg(target_os = "linux")]
fn linux_process_status_bytes(field: &str) -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix(field))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
        .map(|kilobytes| kilobytes * 1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn process_working_set_bytes() -> Option<u64> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
fn process_peak_working_set_bytes() -> Option<u64> {
    None
}

fn collect_morphology_performance_evidence(
    report: &BuildReport,
    full_graph_elapsed: Duration,
    child_probe: MorphologyChildProbe,
    cell_count: usize,
    plate_count: usize,
) -> MorphologyPerformanceEvidence {
    let formal_tectonic_elapsed = report
        .stages()
        .iter()
        .find(|stage| stage.stage_id() == "natural.spherical-tectonics")
        .expect("formal spherical graph reports its tectonic stage")
        .duration();
    let quality_elapsed = report
        .stages()
        .iter()
        .find(|stage| stage.stage_id() == "natural.spherical-quality")
        .expect("formal spherical graph reports its quality stage")
        .duration();
    MorphologyPerformanceEvidence {
        tectonic_elapsed: child_probe.elapsed,
        formal_tectonic_elapsed,
        quality_elapsed,
        full_graph_elapsed,
        morphology_peak_delta_bytes: child_probe.peak_delta_bytes,
        cell_count,
        plate_count,
    }
}

fn recorded_baseline_duration() -> Duration {
    let raw = std::env::var(BASELINE_DURATION_ENV).unwrap_or_else(|_| {
        panic!(
            "ignored Release acceptance requires {BASELINE_DURATION_ENV}=1418.187 from the untouched f00466ce baseline"
        )
    });
    let milliseconds = raw
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("{BASELINE_DURATION_ENV} must be finite milliseconds"));
    assert!(milliseconds.is_finite() && milliseconds > 0.0);
    Duration::from_secs_f64(milliseconds / 1_000.0)
}

fn run_morphology_probe_child() -> MorphologyChildProbe {
    let output = Command::new(std::env::current_exe().expect("test executable path is available"))
        .args([
            "--exact",
            "release_spherical_natural_full_graph_budget",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MORPHOLOGY_PROBE_CHILD_ENV, "1")
        .output()
        .expect("morphology performance child starts");
    assert!(
        output.status.success(),
        "morphology performance child failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("child output is UTF-8");
    let line = stdout
        .lines()
        .find_map(|line| {
            line.find(MORPHOLOGY_PROBE_PREFIX)
                .map(|start| &line[start..])
        })
        .expect("child emitted morphology evidence");
    let value = |name: &str| {
        line.split_ascii_whitespace()
            .find_map(|field| field.strip_prefix(&format!("{name}=")))
            .unwrap_or_else(|| panic!("child evidence omitted {name}"))
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("child evidence {name} is not u64"))
    };
    MorphologyChildProbe {
        elapsed: Duration::from_micros(value("elapsed_us")),
        peak_delta_bytes: value("peak_delta_bytes"),
    }
}

fn emit_morphology_probe_child() {
    let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(EARTH_RADIUS_M).unwrap(),
        target_cell_count: TARGET_CELL_COUNT,
    })
    .unwrap();
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let mut rng = StageRng::from_seed(derive_stage_seed(
        ROOT_SEED,
        StageIdentity::new("natural.spherical-tectonics", 4, "sekai.core"),
    ));
    let baseline = process_working_set_bytes()
        .expect("morphology memory probe requires Windows or Linux process metrics");
    let running = Arc::new(AtomicBool::new(true));
    let maximum = Arc::new(AtomicU64::new(baseline));
    let sampler = {
        let running = Arc::clone(&running);
        let maximum = Arc::clone(&maximum);
        thread::spawn(move || {
            while running.load(Ordering::Acquire) {
                if let Some(bytes) = process_working_set_bytes() {
                    maximum.fetch_max(bytes, Ordering::Relaxed);
                }
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let started = Instant::now();
    let snapshot = TectonicGenerator::generate_spherical(
        &surface,
        &TectonicSpec::default(),
        &formation,
        &mut rng,
    )
    .unwrap();
    let elapsed = started.elapsed();
    if let Some(bytes) = process_working_set_bytes() {
        maximum.fetch_max(bytes, Ordering::Relaxed);
    }
    running.store(false, Ordering::Release);
    sampler.join().unwrap();
    let peak_delta_bytes = maximum.load(Ordering::Relaxed).saturating_sub(baseline);
    println!(
        "{MORPHOLOGY_PROBE_PREFIX} elapsed_us={} peak_delta_bytes={peak_delta_bytes} cells={} plates={}",
        elapsed.as_micros(),
        surface.cells().len(),
        snapshot.plates().len()
    );
}

#[test]
#[ignore = "release-only 20,000-cell planar/spherical full-graph acceptance"]
fn release_spherical_natural_full_graph_budget() {
    if morphology_probe_child_requested(std::env::var_os(MORPHOLOGY_PROBE_CHILD_ENV).as_deref()) {
        emit_morphology_probe_child();
        return;
    }
    let child_probe = run_morphology_probe_child();
    let baseline_duration = recorded_baseline_duration();
    let planar_engine = BuildEngine::new(legacy_planar_natural_foundation_graph().unwrap());
    let planar_external = planar_external_artifacts();
    let mut planar_cache = MemoryStageCache::new();
    let planar_started = Instant::now();
    let planar_outcome = planar_engine
        .build(ROOT_SEED, planar_external, &mut planar_cache)
        .unwrap();
    let planar_elapsed = planar_started.elapsed();
    assert_eq!(planar_outcome.report.stages().len(), 16);
    drop(planar_outcome);
    drop(planar_cache);
    let planar_peak_working_set = process_peak_working_set_bytes();

    let sphere_engine = BuildEngine::new(spherical_natural_foundation_graph().unwrap());
    let sphere_external = spherical_external_artifacts();
    let mut sphere_cache = MemoryStageCache::new();
    let baseline_working_set = process_working_set_bytes();
    let sphere_started = Instant::now();
    let sphere_outcome = sphere_engine
        .build(ROOT_SEED, sphere_external, &mut sphere_cache)
        .unwrap();
    let sphere_elapsed = sphere_started.elapsed();
    let sphere_peak_working_set = process_peak_working_set_bytes();

    let surface = sphere_outcome
        .artifacts
        .get::<SphericalSurfaceArtifact>()
        .unwrap();
    let formation = sphere_outcome
        .artifacts
        .get::<ResolvedWorldFormationArtifact>()
        .unwrap();
    let tectonic = sphere_outcome
        .artifacts
        .get::<SphericalTectonicArtifact>()
        .unwrap();
    let mantle = sphere_outcome
        .artifacts
        .get::<SphericalMantleArtifact>()
        .unwrap();
    let relief = sphere_outcome
        .artifacts
        .get::<SphericalReliefArtifact>()
        .unwrap();
    let geology = sphere_outcome
        .artifacts
        .get::<SphericalGeologicArtifact>()
        .unwrap();
    let climate = sphere_outcome
        .artifacts
        .get::<SphericalPreliminaryClimateArtifact>()
        .unwrap();
    let hydro = sphere_outcome
        .artifacts
        .get::<SphericalHydroErosionArtifact>()
        .unwrap();
    let quality = sphere_outcome
        .artifacts
        .get::<NaturalQualityArtifact>()
        .unwrap();

    validate_final_product(
        &surface, &formation, &tectonic, &mantle, &relief, &geology, &climate, &hydro, &quality,
    );
    assert_eq!(sphere_outcome.report.stages().len(), 17);
    let provenance = sphere_outcome.verified_provenance().unwrap();
    assert_eq!(provenance.root_seed(), ROOT_SEED);
    assert_eq!(
        Some(provenance.result_hash()),
        sphere_outcome.report.result_hash()
    );
    let final_working_set = process_working_set_bytes();
    let additional_working_set_bytes = baseline_working_set
        .zip(final_working_set)
        .map(|(before, after)| after.saturating_sub(before));
    let additional_peak_working_set_bytes = planar_peak_working_set
        .zip(sphere_peak_working_set)
        .map(|(planar_peak, sphere_peak)| sphere_peak.saturating_sub(planar_peak));
    let morphology_evidence = collect_morphology_performance_evidence(
        &sphere_outcome.report,
        sphere_elapsed,
        child_probe,
        surface.snapshot().cells().len(),
        tectonic.snapshot().plates().len(),
    );

    eprintln!(
        "spherical_natural_budget_probe full_ms={:.3} baseline_limit_ms={:.3} isolated_tectonic_ms={:.3} formal_tectonic_ms={:.3} quality_ms={:.3}",
        morphology_evidence.full_graph_elapsed.as_secs_f64() * 1_000.0,
        baseline_duration.as_secs_f64() * MORPHOLOGY_TO_BASELINE_TIME_RATIO_BUDGET * 1_000.0,
        morphology_evidence.tectonic_elapsed.as_secs_f64() * 1_000.0,
        morphology_evidence.formal_tectonic_elapsed.as_secs_f64() * 1_000.0,
        morphology_evidence.quality_elapsed.as_secs_f64() * 1_000.0,
    );

    assert_eq!(morphology_evidence.cell_count, 20_252);
    assert!(
        (usize::from(MIN_PLATE_COUNT)..=usize::from(MAX_PLATE_COUNT))
            .contains(&morphology_evidence.plate_count),
        "the authored count is initial; the evolved active count must remain within product bounds"
    );
    assert!(
        morphology_evidence.tectonic_elapsed <= TECTONIC_TIME_BUDGET,
        "isolated tectonic construction {:?} exceeded {:?}",
        morphology_evidence.tectonic_elapsed,
        TECTONIC_TIME_BUDGET
    );
    assert!(
        morphology_evidence.formal_tectonic_elapsed <= TECTONIC_PUBLICATION_TIME_BUDGET,
        "tectonic validation and publication {:?} exceeded {:?}",
        morphology_evidence.formal_tectonic_elapsed,
        TECTONIC_PUBLICATION_TIME_BUDGET
    );
    assert!(
        morphology_evidence.full_graph_elapsed <= SPHERE_TIME_BUDGET,
        "full spherical graph {:?} exceeded {:?}",
        morphology_evidence.full_graph_elapsed,
        SPHERE_TIME_BUDGET
    );
    assert!(
        morphology_evidence.full_graph_elapsed.as_secs_f64()
            <= baseline_duration.as_secs_f64() * MORPHOLOGY_TO_BASELINE_TIME_RATIO_BUDGET
    );
    assert!(
        morphology_evidence.morphology_peak_delta_bytes <= MORPHOLOGY_PEAK_WORKING_SET_BUDGET_BYTES
    );

    let surface_bytes = ArtifactBytes::measure(
        surface.as_ref(),
        spherical_surface_persistent_bytes(surface.as_ref()),
    );
    let formation_bytes =
        ArtifactBytes::measure(formation.as_ref(), size_of_val(formation.as_ref()));
    let tectonic_bytes = ArtifactBytes::measure(
        tectonic.as_ref(),
        spherical_tectonic_persistent_bytes(tectonic.as_ref()),
    );
    let mantle_bytes = ArtifactBytes::measure(
        mantle.as_ref(),
        spherical_mantle_persistent_bytes(mantle.as_ref()),
    );
    let relief_bytes = ArtifactBytes::measure(
        relief.as_ref(),
        spherical_relief_persistent_bytes(relief.as_ref()),
    );
    let geology_bytes = ArtifactBytes::measure(
        geology.as_ref(),
        spherical_geology_persistent_bytes(geology.as_ref()),
    );
    let climate_bytes = ArtifactBytes::measure(
        climate.as_ref(),
        spherical_climate_persistent_bytes(climate.as_ref()),
    );
    let hydro_bytes = ArtifactBytes::measure(
        hydro.as_ref(),
        spherical_hydro_persistent_bytes(hydro.as_ref()),
    );
    let quality_bytes = ArtifactBytes::measure(
        quality.as_ref(),
        natural_quality_persistent_bytes(quality.as_ref()),
    );
    let artifact_bytes = [
        surface_bytes,
        formation_bytes,
        tectonic_bytes,
        mantle_bytes,
        relief_bytes,
        geology_bytes,
        climate_bytes,
        hydro_bytes,
        quality_bytes,
    ];
    let persistent_total_bytes = artifact_bytes
        .iter()
        .map(|bytes| bytes.persistent)
        .sum::<usize>();
    let serialized_total_bytes = artifact_bytes
        .iter()
        .map(|bytes| bytes.serialized)
        .sum::<usize>();
    let sphere_to_planar_ratio = sphere_elapsed.as_secs_f64() / planar_elapsed.as_secs_f64();
    let stage_timings_ms = sphere_outcome
        .report
        .stages()
        .iter()
        .map(|stage| {
            format!(
                "{}:{:.3}",
                stage.stage_id(),
                stage.duration().as_secs_f64() * 1_000.0
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let surface_snapshot = surface.snapshot();
    let tectonic_snapshot = tectonic.snapshot();
    let mantle_snapshot = mantle.snapshot();
    let hydro_snapshot = hydro.snapshot().hydrology();

    eprintln!(
        "spherical_natural_graph_performance planar_ms={:.3} sphere_ms={:.3} baseline_ms={:.3} morphology_tectonic_ms={:.3} formal_tectonic_ms={:.3} quality_ms={:.3} morphology_peak_delta_bytes={} sphere_to_planar_ratio={sphere_to_planar_ratio:.6} stages={} cells={} vertices={} edges={} plates={} boundary_segments={} hotspots={} basins={} lakes={} rivers={} persistent_surface_bytes={} persistent_formation_bytes={} persistent_tectonic_bytes={} persistent_mantle_bytes={} persistent_relief_bytes={} persistent_geology_bytes={} persistent_climate_bytes={} persistent_hydro_bytes={} persistent_quality_bytes={} persistent_total_bytes={persistent_total_bytes} serialized_surface_bytes={} serialized_formation_bytes={} serialized_tectonic_bytes={} serialized_mantle_bytes={} serialized_relief_bytes={} serialized_geology_bytes={} serialized_climate_bytes={} serialized_hydro_bytes={} serialized_quality_bytes={} serialized_total_bytes={serialized_total_bytes} baseline_working_set_bytes={baseline_working_set:?} final_working_set_bytes={final_working_set:?} additional_working_set_bytes={additional_working_set_bytes:?} planar_peak_working_set_bytes={planar_peak_working_set:?} sphere_peak_working_set_bytes={sphere_peak_working_set:?} additional_peak_working_set_bytes={additional_peak_working_set_bytes:?} stage_timings_ms={stage_timings_ms}",
        planar_elapsed.as_secs_f64() * 1_000.0,
        sphere_elapsed.as_secs_f64() * 1_000.0,
        baseline_duration.as_secs_f64() * 1_000.0,
        morphology_evidence.tectonic_elapsed.as_secs_f64() * 1_000.0,
        morphology_evidence.formal_tectonic_elapsed.as_secs_f64() * 1_000.0,
        morphology_evidence.quality_elapsed.as_secs_f64() * 1_000.0,
        morphology_evidence.morphology_peak_delta_bytes,
        sphere_outcome.report.stages().len(),
        surface_snapshot.cells().len(),
        surface_snapshot.vertices().len(),
        surface_snapshot.edges().len(),
        tectonic_snapshot.plates().len(),
        tectonic_snapshot.boundary_segments().len(),
        mantle_snapshot.hotspots().len(),
        hydro_snapshot.basins().len(),
        hydro_snapshot.lakes().len(),
        hydro_snapshot.river_segments().len(),
        surface_bytes.persistent,
        formation_bytes.persistent,
        tectonic_bytes.persistent,
        mantle_bytes.persistent,
        relief_bytes.persistent,
        geology_bytes.persistent,
        climate_bytes.persistent,
        hydro_bytes.persistent,
        quality_bytes.persistent,
        surface_bytes.serialized,
        formation_bytes.serialized,
        tectonic_bytes.serialized,
        mantle_bytes.serialized,
        relief_bytes.serialized,
        geology_bytes.serialized,
        climate_bytes.serialized,
        hydro_bytes.serialized,
        quality_bytes.serialized,
    );

    assert!(
        sphere_elapsed <= SPHERE_TIME_BUDGET,
        "spherical graph took {:.3} ms; budget is {:.3} ms",
        sphere_elapsed.as_secs_f64() * 1_000.0,
        SPHERE_TIME_BUDGET.as_secs_f64() * 1_000.0
    );
    assert!(
        sphere_elapsed.as_secs_f64()
            <= planar_elapsed.as_secs_f64() * SPHERE_TO_PLANAR_TIME_RATIO_BUDGET,
        "spherical graph took {:.3} ms versus planar {:.3} ms ({sphere_to_planar_ratio:.3}x); budget is {:.3}x",
        sphere_elapsed.as_secs_f64() * 1_000.0,
        planar_elapsed.as_secs_f64() * 1_000.0,
        SPHERE_TO_PLANAR_TIME_RATIO_BUDGET,
    );
    if let Some(additional_peak_working_set_bytes) = additional_peak_working_set_bytes {
        assert!(
            additional_peak_working_set_bytes <= ADDITIONAL_PEAK_WORKING_SET_BUDGET_BYTES,
            "spherical graph added {additional_peak_working_set_bytes} peak working-set bytes above the planar peak; budget is {ADDITIONAL_PEAK_WORKING_SET_BUDGET_BYTES}"
        );
    }
}

#[test]
fn morphology_probe_child_sentinel_requires_exact_one() {
    use std::ffi::OsStr;

    assert!(morphology_probe_child_requested(Some(OsStr::new("1"))));
    assert!(!morphology_probe_child_requested(None));
    assert!(!morphology_probe_child_requested(Some(OsStr::new("0"))));
    assert!(!morphology_probe_child_requested(Some(OsStr::new("true"))));
}

#[test]
#[ignore = "release-only five-seed 20,252-cell land-area compliance"]
fn release_spherical_land_fraction_compliance_for_five_seeds() {
    let engine = BuildEngine::new(spherical_natural_foundation_graph().unwrap());
    let target = f64::from(ReliefSpec::default().target_land_fraction);
    for root_seed in LAND_COMPLIANCE_SEEDS {
        let mut cache = MemoryStageCache::new();
        let started = Instant::now();
        let outcome = engine
            .build(root_seed, spherical_external_artifacts(), &mut cache)
            .unwrap();
        let surface = outcome.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
        let relief = outcome.artifacts.get::<SphericalReliefArtifact>().unwrap();
        let actual = weighted_land_fraction(surface.snapshot(), relief.snapshot());
        assert!(
            (actual - target).abs() <= 0.01,
            "seed {}: target {target:.6}, actual {actual:.6}",
            root_seed.raw()
        );
        eprintln!(
            "spherical_land_compliance seed={} cells={} target={target:.6} actual={actual:.6} sea_level_m={:.2} graph_ms={:.3}",
            root_seed.raw(),
            surface.snapshot().cells().len(),
            relief.snapshot().sea_level_m(),
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
}

fn weighted_land_fraction(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    relief: &sekai::world::natural::SphericalReliefSnapshot,
) -> f64 {
    let land_area = surface
        .cells()
        .iter()
        .zip(relief.land_ocean().raw_values())
        .filter(|(_, kind)| **kind == LandOceanKind::Land.raw())
        .map(|(cell, _)| cell.area.get())
        .sum::<f64>();
    land_area / surface.total_cell_area().get()
}

fn planar_external_artifacts() -> ExternalArtifacts {
    let mut artifacts = common_external_artifacts();
    artifacts
        .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
            width: Meters::new(20_000_000.0).unwrap(),
            height: Meters::new(10_000_000.0).unwrap(),
            target_cell_count: TARGET_CELL_COUNT,
            boundary: BoundaryCondition::Closed,
        }))
        .unwrap();
    artifacts
}

fn spherical_external_artifacts() -> ExternalArtifacts {
    let mut artifacts = common_external_artifacts();
    artifacts
        .insert(ReliefSpecArtifact::new(ReliefSpec::default()))
        .unwrap();
    artifacts
        .insert(SphericalSpaceArtifact::new(SphericalSpaceSpec {
            radius: Meters::new(EARTH_RADIUS_M).unwrap(),
            target_cell_count: TARGET_CELL_COUNT,
        }))
        .unwrap();
    artifacts
}

fn common_external_artifacts() -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
        .unwrap();
    artifacts
        .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
        .unwrap();
    artifacts
        .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
        .unwrap();
    artifacts
        .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
        .unwrap();
    artifacts
        .insert(WorldFormationSpecArtifact::new(
            WorldFormationSpec::default(),
        ))
        .unwrap();
    artifacts
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    artifacts
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();
    artifacts
}

#[allow(clippy::too_many_arguments)]
fn validate_final_product(
    surface: &SphericalSurfaceArtifact,
    formation: &ResolvedWorldFormationArtifact,
    tectonic: &SphericalTectonicArtifact,
    mantle: &SphericalMantleArtifact,
    relief: &SphericalReliefArtifact,
    geology: &SphericalGeologicArtifact,
    climate: &SphericalPreliminaryClimateArtifact,
    hydro: &SphericalHydroErosionArtifact,
    quality: &NaturalQualityArtifact,
) {
    let surface_snapshot = surface.snapshot();
    surface_snapshot.validate().unwrap();
    formation.formation().validate().unwrap();
    tectonic
        .snapshot()
        .validate_against(surface_snapshot)
        .unwrap();
    mantle
        .snapshot()
        .validate_against(surface_snapshot)
        .unwrap();
    relief
        .snapshot()
        .validate_against(surface_snapshot, tectonic.snapshot(), mantle.snapshot())
        .unwrap();
    geology
        .snapshot()
        .validate_against(
            surface_snapshot,
            tectonic.snapshot(),
            mantle.snapshot(),
            relief.snapshot(),
        )
        .unwrap();
    climate
        .snapshot()
        .validate_against(surface_snapshot, relief.snapshot())
        .unwrap();
    hydro
        .snapshot()
        .validate_against(
            surface_snapshot,
            relief.snapshot(),
            geology.snapshot(),
            climate.snapshot(),
        )
        .unwrap();
    quality.report().validate().unwrap();
    assert_eq!(
        quality.report().surface_ref(),
        sekai::world::spatial::SurfaceRef::for_spherical(surface_snapshot)
    );
    let plate_count = u16::try_from(tectonic.snapshot().plates().len()).unwrap();
    let registry =
        spherical_natural_field_registry(plate_count, surface_snapshot.total_cell_area().get())
            .unwrap();
    assert_eq!(registry.len(), 36);
}

fn spherical_surface_persistent_bytes(artifact: &SphericalSurfaceArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.vertices())
        + size_of_val(snapshot.cells())
        + snapshot
            .cells()
            .iter()
            .map(|cell| {
                size_of_val(cell.boundary_vertices.as_slice())
                    + size_of_val(cell.boundary_edges.as_slice())
            })
            .sum::<usize>()
        + size_of_val(snapshot.edges())
}

fn spherical_tectonic_persistent_bytes(artifact: &SphericalTectonicArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.plates())
        + size_of_val(snapshot.cell_plates().raw_values())
        + size_of_val(snapshot.crust_kinds().raw_values())
        + size_of_val(snapshot.crust_thickness_km())
        + size_of_val(snapshot.crust_age_myr())
        + size_of_val(snapshot.tectonic_elevation_m())
        + size_of_val(snapshot.lineation_east())
        + size_of_val(snapshot.lineation_north())
        + size_of_val(snapshot.orogeny_kind())
        + size_of_val(snapshot.orogeny_age_myr())
        + size_of_val(snapshot.boundaries())
        + size_of_val(snapshot.boundary_segments())
        + snapshot
            .boundary_segments()
            .iter()
            .map(|segment| size_of_val(segment.member_edges()))
            .sum::<usize>()
}

fn spherical_mantle_persistent_bytes(artifact: &SphericalMantleArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.hotspots())
        + size_of_val(snapshot.heat_flow_mw_m2())
        + size_of_val(snapshot.volcanic_influence())
}

fn spherical_relief_persistent_bytes(artifact: &SphericalReliefArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.crust_base_elevation_m().values())
        + size_of_val(snapshot.tectonic_offset_m().values())
        + size_of_val(snapshot.volcanic_offset_m().values())
        + size_of_val(snapshot.regional_offset_m().values())
        + size_of_val(snapshot.elevation_m().values())
        + size_of_val(snapshot.land_ocean().raw_values())
}

fn spherical_geology_persistent_bytes(artifact: &SphericalGeologicArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.bedrock_kinds().raw_values())
        + size_of_val(snapshot.fracture_intensity())
        + size_of_val(snapshot.erosion_resistance())
        + size_of_val(snapshot.relative_permeability())
        + size_of_val(snapshot.metallic_mineral_potential())
        + size_of_val(snapshot.geothermal_potential())
        + size_of_val(snapshot.sedimentary_basin_potential())
}

fn spherical_climate_persistent_bytes(artifact: &SphericalPreliminaryClimateArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.latitude_degrees())
        + size_of_val(snapshot.maritime_influence())
        + size_of_val(snapshot.monthly_air_temperature_c().values())
        + size_of_val(snapshot.monthly_precipitation_mm().values())
        + size_of_val(snapshot.monthly_wind_m_s().values())
        + size_of_val(snapshot.mean_annual_air_temperature_c())
        + size_of_val(snapshot.temperature_seasonality_c())
        + size_of_val(snapshot.annual_precipitation_mm())
        + size_of_val(snapshot.prevailing_wind_m_s())
}

fn spherical_hydro_persistent_bytes(artifact: &SphericalHydroErosionArtifact) -> usize {
    let snapshot = artifact.snapshot();
    let surface = snapshot.surface();
    let hydrology = snapshot.hydrology();
    size_of_val(artifact)
        + size_of_val(surface.erosion_depth_m())
        + size_of_val(surface.deposition_thickness_m())
        + size_of_val(surface.surface_elevation_m().values())
        + size_of_val(surface.sediment_throughput_m3())
        + size_of_val(hydrology.monthly_local_runoff_mm())
        + size_of_val(hydrology.monthly_discharge_m3_s())
        + size_of_val(hydrology.annual_local_runoff_mm())
        + size_of_val(hydrology.mean_annual_discharge_m3_s())
        + size_of_val(hydrology.drainage_area_km2())
        + size_of_val(hydrology.drainage_surface_elevation_m().values())
        + size_of_val(hydrology.lake_depth_m())
        + size_of_val(hydrology.surface_water().raw_values())
        + size_of_val(hydrology.flow_receiver())
        + size_of_val(hydrology.basin_id())
        + size_of_val(hydrology.strahler_order().raw_values())
        + size_of_val(hydrology.basins())
        + size_of_val(hydrology.lakes())
        + hydrology
            .lakes()
            .iter()
            .map(|lake| size_of_val(lake.cells()))
            .sum::<usize>()
        + size_of_val(hydrology.river_segments())
        + size_of_val(hydrology.river_segment_length_m())
}

fn natural_quality_persistent_bytes(artifact: &NaturalQualityArtifact) -> usize {
    size_of_val(artifact)
        + size_of_val(artifact.report().metrics())
        + artifact
            .report()
            .metrics()
            .iter()
            .map(|metric| {
                metric.id().namespace().len()
                    + metric.id().name().len()
                    + metric.reason().map_or(0, str::len)
            })
            .sum::<usize>()
}
