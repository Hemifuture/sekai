//! Cold-start robustness sweep of the production causal chain over random
//! seeds, with the author-panel settings a user is likely to pick.
//!
//! The endpoint P4 warm start of milestone A1 passed every fixed-seed suite
//! and still failed 5 of 8 random Standard seeds in the application; this
//! probe is the evidence that would have caught it. Seeds and profile come
//! from the environment so a failing world can be replayed directly:
//! `SEKAI_SWEEP_PROFILE=draft|standard`, `SEKAI_SWEEP_SEEDS=a,b,c`,
//! `SEKAI_SWEEP_ACTIVITY=quiet|moderate|active`.

use std::time::Instant;

use sekai::app::build_spherical_formation_external_artifacts;
use sekai::engine::{BuildCancellation, BuildEngine, MemoryStageCache};
use sekai::generators::natural::causal_natural_formation_graph;
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    GeologicSpec, NaturalQualityProfile, ReliefSpec, TectonicActivity, TectonicSpec,
    WorldFormationSpec,
};
use sekai::world::{Meters, RootSeed};

/// Eight seeds drawn once (Python `random.seed(20260902)`, 64-bit) and frozen
/// so the default sweep is reproducible; five of them failed the reverted
/// warm start at Standard.
const DEFAULT_SEEDS: [u64; 8] = [
    14_500_982_625_797_044_708,
    11_728_661_556_832_718_318,
    12_307_902_622_372_036_327,
    4_153_584_564_816_370_805,
    7_499_274_601_121_372_060,
    5_744_981_634_611_769_885,
    5_828_067_323_727_643_059,
    7_755_195_920_647_622_459,
];

#[test]
#[ignore = "release-only random-seed robustness sweep of the complete causal chain"]
fn random_seeds_build_without_failure() {
    let profile = match std::env::var("SEKAI_SWEEP_PROFILE").as_deref() {
        Ok("standard") => NaturalQualityProfile::Standard,
        Ok("high") => NaturalQualityProfile::High,
        _ => NaturalQualityProfile::Draft,
    };
    let activity = match std::env::var("SEKAI_SWEEP_ACTIVITY").as_deref() {
        Ok("quiet") => TectonicActivity::Quiet,
        Ok("moderate") => TectonicActivity::Moderate,
        _ => TectonicActivity::Active,
    };
    let seeds: Vec<u64> = match std::env::var("SEKAI_SWEEP_SEEDS") {
        Ok(list) => list
            .split(',')
            .map(|seed| {
                seed.trim()
                    .parse()
                    .expect("SEKAI_SWEEP_SEEDS holds u64 seeds")
            })
            .collect(),
        Err(_) => DEFAULT_SEEDS.to_vec(),
    };
    let surface = ProfileSurfaceBuilder::build(
        profile,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap()
    .authoritative_surface()
    .clone();
    let tectonic = TectonicSpec {
        activity,
        ..TectonicSpec::default()
    };
    let mut failures = Vec::new();
    for seed in seeds {
        let started = Instant::now();
        let root_seed = RootSeed::new(seed);
        let external = build_spherical_formation_external_artifacts(
            root_seed,
            profile,
            &surface,
            &WorldFormationSpec::default(),
            &tectonic,
            &ReliefSpec::default(),
            &GeologicSpec::default(),
        )
        .unwrap();
        let result = BuildEngine::new(causal_natural_formation_graph().unwrap()).build(
            root_seed,
            external,
            &mut MemoryStageCache::new(),
        );
        match result {
            Ok(_) => eprintln!(
                "[sweep] seed={seed} {profile:?} OK {:.1} s",
                started.elapsed().as_secs_f64()
            ),
            Err(failure) => {
                eprintln!(
                    "[sweep] seed={seed} {profile:?} FAILED {:.1} s",
                    started.elapsed().as_secs_f64()
                );
                for diagnostic in failure.report.diagnostics() {
                    eprintln!("[sweep]   {diagnostic:?}");
                }
                failures.push(seed);
            }
        }
    }
    assert!(failures.is_empty(), "seeds failed to build: {failures:?}");
}
