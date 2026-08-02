use std::collections::BTreeSet;

use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraphBuilder,
};
use sekai::generators::natural::{
    natural_foundation_graph, ResolvedWorldFormationArtifact, WorldFormationSpecArtifact,
    WorldFormationStage,
};
use sekai::world::natural::{
    ResolvedWorldFormationPreset, WorldFormationPreset, WorldFormationSpec,
};
use sekai::world::RootSeed;

fn graph() -> sekai::engine::StageGraph {
    StageGraphBuilder::new()
        .external::<WorldFormationSpecArtifact>()
        .stage(WorldFormationStage)
        .build()
        .unwrap()
}

fn external(preset: WorldFormationPreset) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(WorldFormationSpecArtifact::new(WorldFormationSpec {
            preset,
            ..WorldFormationSpec::default()
        }))
        .unwrap();
    artifacts
}

fn resolve(seed: u64, preset: WorldFormationPreset) -> ResolvedWorldFormationArtifact {
    let outcome = BuildEngine::new(graph())
        .build(
            RootSeed::new(seed),
            external(preset),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    outcome
        .artifacts
        .get::<ResolvedWorldFormationArtifact>()
        .unwrap()
        .as_ref()
        .clone()
}

#[test]
fn artifacts_and_stage_have_exact_engine_contracts() {
    assert_eq!(
        WorldFormationSpecArtifact::KEY.as_str(),
        "natural.world-formation-spec"
    );
    assert_eq!(
        ResolvedWorldFormationArtifact::KEY.as_str(),
        "natural.resolved-world-formation"
    );
    assert_eq!(
        WorldFormationStage.id().as_str(),
        "natural.resolve-world-formation"
    );
    assert_eq!(WorldFormationStage.version(), 1);
    assert_eq!(WorldFormationStage.namespace(), "sekai.core");

    let graph = graph();
    let descriptor = &graph.descriptors()[0];
    assert_eq!(
        descriptor
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["natural.world-formation-spec"]
    );
    assert_eq!(descriptor.output(), ResolvedWorldFormationArtifact::KEY);
}

#[test]
fn production_graph_registers_formation_resolution_once() {
    let graph = natural_foundation_graph().unwrap();
    assert_eq!(
        graph
            .stage_ids()
            .into_iter()
            .filter(|id| *id == "natural.resolve-world-formation")
            .count(),
        1
    );
    assert_eq!(graph.descriptors().len(), 16);
}

#[test]
fn named_presets_pass_through_without_seed_dependent_substitution() {
    let cases = [
        (
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        ),
        (
            WorldFormationPreset::Archipelago,
            ResolvedWorldFormationPreset::Archipelago,
        ),
        (
            WorldFormationPreset::Supercontinent,
            ResolvedWorldFormationPreset::Supercontinent,
        ),
        (
            WorldFormationPreset::GreatIsland,
            ResolvedWorldFormationPreset::GreatIsland,
        ),
        (
            WorldFormationPreset::VolcanicIslands,
            ResolvedWorldFormationPreset::VolcanicIslands,
        ),
    ];

    for (requested, expected) in cases {
        let first = resolve(1, requested);
        let second = resolve(u64::MAX, requested);
        assert_eq!(first.formation().requested(), requested);
        assert_eq!(first.formation().resolved(), expected);
        assert_eq!(second.formation().resolved(), expected);
    }
}

#[test]
fn random_resolution_is_repeatable_concrete_and_reaches_every_profile() {
    let first = resolve(91, WorldFormationPreset::Random);
    let second = resolve(91, WorldFormationPreset::Random);
    assert_eq!(first, second);
    assert_eq!(first.formation().requested(), WorldFormationPreset::Random);

    let reached: BTreeSet<_> = (0..512)
        .map(|seed| {
            resolve(seed, WorldFormationPreset::Random)
                .formation()
                .resolved()
        })
        .collect();
    assert_eq!(
        reached,
        BTreeSet::from([
            ResolvedWorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Archipelago,
            ResolvedWorldFormationPreset::Supercontinent,
            ResolvedWorldFormationPreset::GreatIsland,
            ResolvedWorldFormationPreset::VolcanicIslands,
        ])
    );
}

#[test]
fn artifact_round_trips_revalidate_and_cache_by_semantic_input() {
    let spec = WorldFormationSpecArtifact::new(WorldFormationSpec::default());
    let encoded = serde_json::to_vec(&spec).unwrap();
    let decoded: WorldFormationSpecArtifact = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, spec);
    decoded.validate().unwrap();

    let resolved = resolve(42, WorldFormationPreset::Random);
    let encoded = serde_json::to_vec(&resolved).unwrap();
    let decoded: ResolvedWorldFormationArtifact = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, resolved);
    decoded.validate().unwrap();

    let engine = BuildEngine::new(graph());
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(
            RootSeed::new(42),
            external(WorldFormationPreset::Random),
            &mut cache,
        )
        .unwrap();
    let repeated = engine
        .build(
            RootSeed::new(42),
            external(WorldFormationPreset::Random),
            &mut cache,
        )
        .unwrap();
    assert_eq!(repeated.report.cache_hits(), 1);
    assert_eq!(
        first
            .artifacts
            .hash::<ResolvedWorldFormationArtifact>()
            .unwrap(),
        repeated
            .artifacts
            .hash::<ResolvedWorldFormationArtifact>()
            .unwrap()
    );

    let mut malformed = serde_json::to_value(resolved).unwrap();
    malformed["formation"]["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<ResolvedWorldFormationArtifact>(malformed).is_err());
}
