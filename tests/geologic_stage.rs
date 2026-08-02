use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraphBuilder,
};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicArtifact,
    GeologicSpecArtifact, GeologicStage, HydroErosionSpecArtifact, MantleArtifact, ReliefArtifact,
    ResolvedGeologicInputArtifact, RulePackSetArtifact, TectonicArtifact, TectonicSpecArtifact,
    WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, MantleSnapshot, TectonicSpec, WorldFormationSpec,
    MANTLE_SNAPSHOT_SCHEMA_V1,
};
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

fn complete_external() -> ExternalArtifacts {
    let mut external = ExternalArtifacts::new();
    external
        .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
            width: Meters::new(1_000_000.0).unwrap(),
            height: Meters::new(600_000.0).unwrap(),
            target_cell_count: 256,
            boundary: BoundaryCondition::Closed,
        }))
        .unwrap();
    external
        .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
        .unwrap();
    external
        .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
        .unwrap();
    external
        .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
        .unwrap();
    external
        .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
        .unwrap();
    external
        .insert(WorldFormationSpecArtifact::new(
            WorldFormationSpec::default(),
        ))
        .unwrap();
    external
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    external
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();
    external
}

#[test]
fn geologic_artifact_and_stage_have_exact_stable_contracts() {
    assert_eq!(GeologicArtifact::KEY.as_str(), "world.geology");
    assert_eq!(GeologicStage.id().as_str(), "natural.geology");
    assert_eq!(GeologicStage.version(), 1);
    assert_eq!(GeologicStage.namespace(), "sekai.core");

    let graph = StageGraphBuilder::new()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<MantleArtifact>()
        .external::<ReliefArtifact>()
        .external::<SpatialArtifact>()
        .external::<TectonicArtifact>()
        .stage(GeologicStage)
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
            "world.mantle",
            "world.relief",
            "world.spatial",
            "world.tectonics",
        ]
    );
    assert_eq!(descriptor.output(), GeologicArtifact::KEY);
}

#[test]
fn production_stage_builds_and_publishes_a_valid_snapshot() {
    let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(42),
            complete_external(),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let geology = outcome.artifacts.get::<GeologicArtifact>().unwrap();
    let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();
    let tectonic = outcome.artifacts.get::<TectonicArtifact>().unwrap();
    let mantle = outcome.artifacts.get::<MantleArtifact>().unwrap();
    let relief = outcome.artifacts.get::<ReliefArtifact>().unwrap();

    geology.validate().unwrap();
    geology
        .snapshot()
        .validate_against(
            spatial.snapshot(),
            tectonic.snapshot(),
            mantle.snapshot(),
            relief.snapshot(),
        )
        .unwrap();
}

#[test]
fn complete_graph_second_build_hits_all_fifteen_stage_caches() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(RootSeed::new(42), complete_external(), &mut cache)
        .unwrap();
    let repeated = engine
        .build(RootSeed::new(42), complete_external(), &mut cache)
        .unwrap();

    assert_eq!(repeated.report.cache_hits(), 16);
    assert_eq!(repeated.report.cache_misses(), 0);
    assert_eq!(
        first.artifacts.hash::<GeologicArtifact>().unwrap(),
        repeated.artifacts.hash::<GeologicArtifact>().unwrap()
    );
}

#[test]
fn invalid_cross_artifact_input_does_not_poison_a_valid_cached_geology() {
    let upstream = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(42),
            complete_external(),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let graph = StageGraphBuilder::new()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<MantleArtifact>()
        .external::<ReliefArtifact>()
        .external::<SpatialArtifact>()
        .external::<TectonicArtifact>()
        .stage(GeologicStage)
        .build()
        .unwrap();
    let engine = BuildEngine::new(graph);
    let valid_mantle = upstream
        .artifacts
        .get::<MantleArtifact>()
        .unwrap()
        .as_ref()
        .clone();
    let inputs_with_mantle = |mantle: MantleArtifact| {
        let mut external = ExternalArtifacts::new();
        external
            .insert(
                upstream
                    .artifacts
                    .get::<ResolvedGeologicInputArtifact>()
                    .unwrap()
                    .as_ref()
                    .clone(),
            )
            .unwrap();
        external.insert(mantle).unwrap();
        external
            .insert(
                upstream
                    .artifacts
                    .get::<ReliefArtifact>()
                    .unwrap()
                    .as_ref()
                    .clone(),
            )
            .unwrap();
        external
            .insert(
                upstream
                    .artifacts
                    .get::<SpatialArtifact>()
                    .unwrap()
                    .as_ref()
                    .clone(),
            )
            .unwrap();
        external
            .insert(
                upstream
                    .artifacts
                    .get::<TectonicArtifact>()
                    .unwrap()
                    .as_ref()
                    .clone(),
            )
            .unwrap();
        external
    };
    let mut cache = MemoryStageCache::new();
    engine
        .build(
            RootSeed::new(42),
            inputs_with_mantle(valid_mantle.clone()),
            &mut cache,
        )
        .unwrap();

    let invalid = inputs_with_mantle(MantleArtifact::new(
        MantleSnapshot::new(
            MANTLE_SNAPSHOT_SCHEMA_V1,
            1,
            Vec::new(),
            vec![65.0],
            vec![0.0],
        )
        .unwrap(),
    ));
    let failure = engine
        .build(RootSeed::new(42), invalid, &mut cache)
        .unwrap_err();
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "natural.invalid-geologic-input"
    );

    let recovered = engine
        .build(
            RootSeed::new(42),
            inputs_with_mantle(valid_mantle),
            &mut cache,
        )
        .unwrap();
    assert_eq!(recovered.report.cache_hits(), 1);
    assert_eq!(recovered.report.cache_misses(), 0);
}
