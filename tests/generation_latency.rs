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
    let surface = ProfileSurfaceBuilder::build(
        profile,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap()
    .authoritative_surface()
    .clone();
    let surface_seconds = started.elapsed().as_secs_f64();
    let graph_started = Instant::now();
    let root_seed = RootSeed::new(SEED);
    let external = build_spherical_formation_external_artifacts(
        root_seed,
        profile,
        &surface,
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
    )
    .unwrap();
    let outcome = BuildEngine::new(causal_natural_formation_graph().unwrap())
        .build(root_seed, external, &mut MemoryStageCache::new())
        .unwrap();
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
