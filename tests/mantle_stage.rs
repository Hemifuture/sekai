use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraphBuilder,
};
use sekai::generators::natural::{
    MantleArtifact, MantleStage, ResolvedGeologicInput, ResolvedGeologicInputArtifact,
    ResolvedWorldFormationArtifact,
};
use sekai::generators::spatial::{PlanarVoronoiBuilder, SpatialArtifact};
use sekai::rules::GeologicModel;
use sekai::world::natural::{
    GeologicSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

fn spatial_artifact() -> SpatialArtifact {
    SpatialArtifact::new(
        PlanarVoronoiBuilder::build(
            &PlanarSpaceSpec {
                width: Meters::new(1_000_000.0).unwrap(),
                height: Meters::new(600_000.0).unwrap(),
                target_cell_count: 144,
                boundary: BoundaryCondition::Closed,
            },
            &mut ChaCha8Rng::seed_from_u64(55),
        )
        .unwrap(),
    )
}

fn external_inputs() -> ExternalArtifacts {
    external_inputs_for(ResolvedWorldFormationPreset::Continents)
}

fn external_inputs_for(resolved: ResolvedWorldFormationPreset) -> ExternalArtifacts {
    let mut external = ExternalArtifacts::new();
    external.insert(spatial_artifact()).unwrap();
    external
        .insert(ResolvedGeologicInputArtifact::new(
            ResolvedGeologicInput::new(GeologicModel::CurrentSliceV1, GeologicSpec::default())
                .unwrap(),
        ))
        .unwrap();
    external
        .insert(ResolvedWorldFormationArtifact::new(
            ResolvedWorldFormation::new(
                RESOLVED_WORLD_FORMATION_SCHEMA_V1,
                match resolved {
                    ResolvedWorldFormationPreset::Continents => WorldFormationPreset::Continents,
                    ResolvedWorldFormationPreset::Archipelago => WorldFormationPreset::Archipelago,
                    ResolvedWorldFormationPreset::Supercontinent => {
                        WorldFormationPreset::Supercontinent
                    }
                    ResolvedWorldFormationPreset::GreatIsland => WorldFormationPreset::GreatIsland,
                    ResolvedWorldFormationPreset::VolcanicIslands => {
                        WorldFormationPreset::VolcanicIslands
                    }
                },
                resolved,
            )
            .unwrap(),
        ))
        .unwrap();
    external
}

#[test]
fn volcanic_formation_bias_reaches_the_mantle_stage() {
    let graph = StageGraphBuilder::new()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<SpatialArtifact>()
        .stage(MantleStage)
        .build()
        .unwrap();
    let outcome = BuildEngine::new(graph)
        .build(
            RootSeed::new(42),
            external_inputs_for(ResolvedWorldFormationPreset::VolcanicIslands),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    assert!(
        outcome
            .artifacts
            .get::<MantleArtifact>()
            .unwrap()
            .snapshot()
            .hotspots()
            .len()
            >= 9
    );
}

#[test]
fn mantle_artifact_and_stage_have_exact_stable_contracts() {
    assert_eq!(MantleArtifact::KEY.as_str(), "world.mantle");
    assert_eq!(MantleStage.id().as_str(), "natural.mantle");
    assert_eq!(MantleStage.version(), 2);
    assert_eq!(MantleStage.namespace(), "sekai.core");

    let graph = StageGraphBuilder::new()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<SpatialArtifact>()
        .stage(MantleStage)
        .build()
        .unwrap();
    let descriptor = &graph.descriptors()[0];
    assert_eq!(
        descriptor
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.resolved-geologic-input",
            "natural.resolved-world-formation",
            "world.spatial",
        ]
    );
    assert_eq!(descriptor.output(), MantleArtifact::KEY);
}

#[test]
fn mantle_stage_builds_and_publishes_a_valid_snapshot() {
    let graph = StageGraphBuilder::new()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<SpatialArtifact>()
        .stage(MantleStage)
        .build()
        .unwrap();
    let outcome = BuildEngine::new(graph)
        .build(
            RootSeed::new(42),
            external_inputs(),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let mantle = outcome.artifacts.get::<MantleArtifact>().unwrap();
    let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();

    mantle.validate().unwrap();
    mantle
        .snapshot()
        .validate_against(spatial.snapshot())
        .unwrap();
}

#[test]
fn mantle_stage_output_is_seeded_and_cacheable() {
    let graph = StageGraphBuilder::new()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<SpatialArtifact>()
        .stage(MantleStage)
        .build()
        .unwrap();
    let engine = BuildEngine::new(graph);
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(RootSeed::new(42), external_inputs(), &mut cache)
        .unwrap();
    let repeated = engine
        .build(RootSeed::new(42), external_inputs(), &mut cache)
        .unwrap();
    let changed = engine
        .build(RootSeed::new(43), external_inputs(), &mut cache)
        .unwrap();

    assert_eq!(repeated.report.cache_hits(), 1);
    assert_eq!(
        first.artifacts.hash::<MantleArtifact>().unwrap(),
        repeated.artifacts.hash::<MantleArtifact>().unwrap()
    );
    assert_ne!(
        first.artifacts.hash::<MantleArtifact>().unwrap(),
        changed.artifacts.hash::<MantleArtifact>().unwrap()
    );
}
