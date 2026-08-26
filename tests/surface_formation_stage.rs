use std::sync::OnceLock;

use sekai::engine::{Artifact, BuildCancellation, BuildEngine, MemoryStageCache, Stage};
use sekai::generators::natural::{
    global_circulation_graph, surface_formation_graph, NaturalSurfaceFormationArtifact,
    SurfaceFormationStage,
};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{GeologicSpec, NaturalQualityProfile, ReliefSpec, TectonicSpec};
use sekai::world::{Meters, RootSeed};

fn surface() -> &'static sekai::world::spatial::SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<sekai::world::spatial::SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap()
        .authoritative_surface()
        .clone()
    })
}

/// On-demand finite-time generation check through the exact app externals.
#[test]
#[ignore = "release-only single-seed finite-time P5 probe"]
fn probe_formation_finite_time_seed() {
    let seed: u64 = std::env::var("SEKAI_P5_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3_945_477_593_443_907_072);
    let profile = match std::env::var("SEKAI_P5_PROFILE").as_deref() {
        Ok("standard") => NaturalQualityProfile::Standard,
        Ok("high") => NaturalQualityProfile::High,
        _ => NaturalQualityProfile::Draft,
    };
    let built;
    let probe_surface = if profile == NaturalQualityProfile::Draft {
        surface()
    } else {
        built = ProfileSurfaceBuilder::build(
            profile,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap()
        .authoritative_surface()
        .clone();
        &built
    };
    let root_seed = RootSeed::new(seed);
    let external = sekai::app::build_spherical_formation_external_artifacts(
        root_seed,
        profile,
        probe_surface,
        &sekai::world::natural::WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
    )
    .unwrap();
    let result = BuildEngine::new(surface_formation_graph().unwrap()).build(
        root_seed,
        external,
        &mut MemoryStageCache::new(),
    );
    match &result {
        Ok(_) => eprintln!("seed {seed}: GENERATED"),
        Err(failure) => {
            for diagnostic in failure.report.diagnostics() {
                eprintln!(
                    "  [{:?}] {}: {}",
                    diagnostic.severity(),
                    diagnostic.code(),
                    diagnostic.message()
                );
            }
            panic!("seed {seed}: {failure}");
        }
    }
}

#[test]
fn the_p5_stage_publishes_a_locked_key_identity_and_exact_dependency_boundary() {
    assert_eq!(
        NaturalSurfaceFormationArtifact::KEY.as_str(),
        "world.natural-surface-formation"
    );
    assert_eq!(
        SurfaceFormationStage.id().as_str(),
        "natural.surface-formation"
    );
    assert_eq!(SurfaceFormationStage.version(), 3);
    assert_eq!(SurfaceFormationStage.namespace(), "sekai.core");

    let graph = surface_formation_graph().unwrap();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.climate-work-domain",
            "natural.evolved-tectonics",
            "natural.geologic-substrate",
            "natural.primary-relief",
            "natural.global-circulation",
            "natural.surface-formation",
        ]
    );
    let formation = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.surface-formation")
        .unwrap();
    assert_eq!(
        formation
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.quality-profile",
            "natural.resolved-climate-input",
            "natural.resolved-hydro-erosion-input",
            "world.climate-work-domain",
            "world.evolved-tectonics",
            "world.geologic-substrate",
            "world.global-circulation",
            "world.primary-relief",
            "world.spherical-surface",
        ]
    );

    // The frozen P0-P4 graph keeps its exact stage set.
    assert_eq!(
        global_circulation_graph().unwrap().stage_ids(),
        vec![
            "natural.climate-work-domain",
            "natural.evolved-tectonics",
            "natural.geologic-substrate",
            "natural.primary-relief",
            "natural.global-circulation",
        ]
    );
}
