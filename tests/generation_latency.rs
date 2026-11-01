//! Product-level wall-clock budget of the complete causal formation graph.
//!
//! This is the number the user waits for after 「按当前参数重建」: P1 surface
//! bundle plus the engine-scheduled P2→P5 chain including quality evaluation
//! and bundle validation. The budgets are the user's targets from milestone
//! A1 (`docs/superpowers/specs/2026-09-02-generation-latency-design.md`).

use std::time::Instant;

use sekai::app::build_spherical_formation_external_artifacts;
use sekai::engine::{BuildCancellation, BuildEngine, MemoryStageCache};
use sekai::generators::natural::{causal_natural_formation_graph, NaturalFormationBundleArtifact};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    GeologicSpec, NaturalQualityProfile, ReliefSpec, TectonicSpec, WorldFormationSpec,
};
use sekai::world::{Meters, RootSeed};

const SEED: u64 = 42;

fn measure(profile: NaturalQualityProfile, budget_seconds: f64) {
    let started = Instant::now();
    let surface = build_surface(profile).unwrap();
    let surface_seconds = started.elapsed().as_secs_f64();
    let graph_started = Instant::now();
    let outcome = build_graph(profile, &surface, &mut MemoryStageCache::new()).unwrap();
    let graph_seconds = graph_started.elapsed().as_secs_f64();
    let total_seconds = started.elapsed().as_secs_f64();
    for stage in outcome.report.stages() {
        eprintln!(
            "[latency {profile:?}]    stage {:<40} {:>8.2} s",
            stage.stage_id(),
            stage.duration().as_secs_f64()
        );
    }
    let artifact = outcome
        .artifacts
        .get::<NaturalFormationBundleArtifact>()
        .unwrap();
    let climate = artifact.bundle().climate().solve_report();
    eprintln!(
        "[latency {profile:?}] surface {surface_seconds:.2} s, graph {graph_seconds:.2} s, total {total_seconds:.2} s (endpoint P4 {} cycles, {} fast substeps)",
        climate.formation_cycles(),
        climate.fast_substeps()
    );
    assert!(
        total_seconds <= budget_seconds,
        "{profile:?} full chain took {total_seconds:.2} s, budget {budget_seconds} s"
    );
}

#[test]
#[ignore = "release-only wall-clock budget of the complete Draft chain"]
fn draft_full_chain_within_twenty_seconds() {
    measure(NaturalQualityProfile::Draft, 20.0);
}

#[test]
#[ignore = "release-only wall-clock budget of the complete Standard chain"]
fn standard_full_chain_within_one_minute() {
    measure(NaturalQualityProfile::Standard, 60.0);
}

// Shared production path for both the unchanged budgets and non-gating observations.
fn build_surface(
    profile: NaturalQualityProfile,
) -> Result<sekai::world::spatial::SphericalSurfaceSnapshot, String> {
    ProfileSurfaceBuilder::build(
        profile,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .map(|bundle| bundle.authoritative_surface().clone())
    .map_err(|error| error.to_string())
}

fn build_graph(
    profile: NaturalQualityProfile,
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    cache: &mut MemoryStageCache,
) -> Result<sekai::engine::BuildOutcome, String> {
    let root_seed = RootSeed::new(SEED);
    let external = build_spherical_formation_external_artifacts(
        root_seed,
        profile,
        surface,
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
    )
    .map_err(|error| error.to_string())?;
    BuildEngine::new(causal_natural_formation_graph().map_err(|error| error.to_string())?)
        .build(root_seed, external, cache)
        .map_err(|failure| format!("{failure}; report={:?}", failure.report))
}

#[test]
#[ignore = "release-only non-gating Phase A cold and in-memory stage-cache observations"]
fn observe_phase_a_latency() {
    let profile = match std::env::var("SEKAI_PHASE_A_PROFILE")
        .as_deref()
        .unwrap_or("draft")
    {
        "draft" => NaturalQualityProfile::Draft,
        "standard" => NaturalQualityProfile::Standard,
        other => panic!("unsupported SEKAI_PHASE_A_PROFILE={other}; expected draft or standard"),
    };
    let runs = std::env::var("SEKAI_PHASE_A_RUNS").map_or(3, |value| {
        value
            .parse::<usize>()
            .expect("SEKAI_PHASE_A_RUNS must be a positive integer")
    });
    assert!(runs > 0, "SEKAI_PHASE_A_RUNS must be positive");
    eprintln!("PHASE_A_CONFIG profile={profile:?} seed={SEED} radius_m=6371000 inputs=domain_defaults cold_runs={runs} threads={:?} available_parallelism={:?} rustflags={:?} validation=included_in_graph", std::env::var("RAYON_NUM_THREADS"), std::thread::available_parallelism(), std::env::var("RUSTFLAGS"));
    let mut cache = MemoryStageCache::new();
    let mut last_identity = None;
    let mut failures = 0;
    for run in 0..=runs {
        let hot = run == runs;
        if !hot {
            cache = MemoryStageCache::new();
        }
        let started = Instant::now();
        let surface_result = build_surface(profile);
        let surface_seconds = started.elapsed().as_secs_f64();
        let graph_started = Instant::now();
        let outcome = surface_result
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|surface| build_graph(profile, surface, &mut cache));
        let graph_seconds = graph_started.elapsed().as_secs_f64();
        let total_seconds = started.elapsed().as_secs_f64();
        let cache_kind = if hot { "hot_stage_cache" } else { "cold" };
        match outcome {
            Ok(outcome) => {
                let artifact = outcome
                    .artifacts
                    .get::<NaturalFormationBundleArtifact>()
                    .expect("the validated formation product exists");
                let climate = artifact.bundle().climate().solve_report();
                let identity = *outcome
                    .report
                    .result_hash()
                    .expect("successful builds have a result hash");
                if let Some(previous) = &last_identity {
                    assert_eq!(
                        &identity, previous,
                        "unchanged input/cache must preserve identity"
                    );
                }
                last_identity = Some(identity);
                eprintln!("PHASE_A_SAMPLE run={} profile={profile:?} cache={cache_kind} surface_cache=cold outcome=success surface_s={surface_seconds:.6} graph_s={graph_seconds:.6} total_s={total_seconds:.6} identity={identity:?} surface_fingerprint={:?} cells={} cache_hits={} cache_misses={} endpoint_cycles={} endpoint_fast_substeps={}", run + 1, surface_result.as_ref().unwrap().fingerprint(), surface_result.as_ref().unwrap().cells().len(), outcome.report.cache_hits(), outcome.report.cache_misses(), climate.formation_cycles(), climate.fast_substeps());
                for stage in outcome.report.stages() {
                    eprintln!("PHASE_A_STAGE run={} stage={} duration_s={:.6} cache_hit={} source=build_report", run + 1, stage.stage_id(), stage.duration().as_secs_f64(), stage.cache_hit());
                }
            }
            Err(error) => {
                failures += 1;
                eprintln!("PHASE_A_SAMPLE run={} profile={profile:?} cache={cache_kind} outcome=failed surface_s={surface_seconds:.6} graph_s={graph_seconds:.6} total_s={total_seconds:.6} error={error}", run + 1);
            }
        }
    }
    assert_eq!(
        failures, 0,
        "failed builds are recorded, never counted as successful samples"
    );
}
