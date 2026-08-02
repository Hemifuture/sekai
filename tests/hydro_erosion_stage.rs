use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraphBuilder,
};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicArtifact,
    GeologicSpecArtifact, HydroErosionArtifact, HydroErosionSpecArtifact, HydroErosionStage,
    PreliminaryClimateArtifact, ReliefArtifact, ResolvedHydroErosionInputArtifact,
    RulePackSetArtifact, TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateSpec, ElevationField, GeologicSpec, HydroErosionSpec, LandOceanField, ReliefSnapshot,
    TectonicSpec, WorldFormationSpec, RELIEF_SCHEMA_V2,
};
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

fn complete_external(spec: HydroErosionSpec) -> ExternalArtifacts {
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
        .insert(HydroErosionSpecArtifact::new(spec))
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
fn artifact_and_stage_have_exact_stable_contracts() {
    assert_eq!(HydroErosionArtifact::KEY.as_str(), "world.hydro-erosion");
    assert_eq!(HydroErosionStage.id().as_str(), "natural.hydro-erosion");
    assert_eq!(HydroErosionStage.version(), 1);
    assert_eq!(HydroErosionStage.namespace(), "sekai.core");

    let graph = StageGraphBuilder::new()
        .external::<ResolvedHydroErosionInputArtifact>()
        .external::<SpatialArtifact>()
        .external::<ReliefArtifact>()
        .external::<GeologicArtifact>()
        .external::<PreliminaryClimateArtifact>()
        .stage(HydroErosionStage)
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
            "natural.resolved-hydro-erosion-input",
            "world.geology",
            "world.preliminary-climate",
            "world.relief",
            "world.spatial",
        ]
    );
    assert_eq!(descriptor.output(), HydroErosionArtifact::KEY);
}

#[test]
fn production_stage_publishes_cross_validated_atomic_snapshot() {
    let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(42),
            complete_external(HydroErosionSpec::default()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let hydro = outcome.artifacts.get::<HydroErosionArtifact>().unwrap();
    let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();
    let relief = outcome.artifacts.get::<ReliefArtifact>().unwrap();
    let geology = outcome.artifacts.get::<GeologicArtifact>().unwrap();
    let climate = outcome
        .artifacts
        .get::<PreliminaryClimateArtifact>()
        .unwrap();

    hydro.validate().unwrap();
    hydro
        .snapshot()
        .validate_against(
            spatial.snapshot(),
            relief.snapshot(),
            geology.snapshot(),
            climate.snapshot(),
        )
        .unwrap();
    let encoded = serde_json::to_vec(hydro.as_ref()).unwrap();
    let decoded: HydroErosionArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn repeated_build_hits_all_fifteen_stages_and_hydro_spec_change_reruns_three() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let baseline = engine
        .build(
            RootSeed::new(42),
            complete_external(HydroErosionSpec::default()),
            &mut cache,
        )
        .unwrap();
    let repeated = engine
        .build(
            RootSeed::new(42),
            complete_external(HydroErosionSpec::default()),
            &mut cache,
        )
        .unwrap();
    assert_eq!(repeated.report.cache_hits(), 16);
    assert_eq!(repeated.report.cache_misses(), 0);

    let changed_spec = HydroErosionSpec {
        erosion_strength_permille: 500,
        ..HydroErosionSpec::default()
    };
    let changed = engine
        .build(
            RootSeed::new(42),
            complete_external(changed_spec),
            &mut cache,
        )
        .unwrap();
    assert_eq!(changed.report.cache_hits(), 13);
    assert_eq!(changed.report.cache_misses(), 3);
    assert_ne!(
        baseline.artifacts.hash::<HydroErosionArtifact>().unwrap(),
        changed.artifacts.hash::<HydroErosionArtifact>().unwrap()
    );
}

#[test]
fn cross_artifact_failure_cannot_poison_valid_cached_hydro_output() {
    let upstream = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(42),
            complete_external(HydroErosionSpec::default()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let graph = StageGraphBuilder::new()
        .external::<ResolvedHydroErosionInputArtifact>()
        .external::<SpatialArtifact>()
        .external::<ReliefArtifact>()
        .external::<GeologicArtifact>()
        .external::<PreliminaryClimateArtifact>()
        .stage(HydroErosionStage)
        .build()
        .unwrap();
    let engine = BuildEngine::new(graph);
    let valid_relief = upstream
        .artifacts
        .get::<ReliefArtifact>()
        .unwrap()
        .as_ref()
        .clone();
    let inputs_with_relief = |relief: ReliefArtifact| {
        let mut external = ExternalArtifacts::new();
        external
            .insert(
                upstream
                    .artifacts
                    .get::<ResolvedHydroErosionInputArtifact>()
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
        external.insert(relief).unwrap();
        external
            .insert(
                upstream
                    .artifacts
                    .get::<GeologicArtifact>()
                    .unwrap()
                    .as_ref()
                    .clone(),
            )
            .unwrap();
        external
            .insert(
                upstream
                    .artifacts
                    .get::<PreliminaryClimateArtifact>()
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
            inputs_with_relief(valid_relief.clone()),
            &mut cache,
        )
        .unwrap();

    let field = || ElevationField::from_values(vec![0.0]).unwrap();
    let invalid_relief = ReliefArtifact::new(
        ReliefSnapshot::new(
            RELIEF_SCHEMA_V2,
            1,
            0.0,
            field(),
            field(),
            field(),
            field(),
            field(),
            LandOceanField::classify(&field(), 0.0),
        )
        .unwrap(),
    );
    let failure = engine
        .build(
            RootSeed::new(42),
            inputs_with_relief(invalid_relief),
            &mut cache,
        )
        .unwrap_err();
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "natural.invalid-hydro-erosion-input"
    );

    let recovered = engine
        .build(
            RootSeed::new(42),
            inputs_with_relief(valid_relief),
            &mut cache,
        )
        .unwrap();
    assert_eq!(recovered.report.cache_hits(), 1);
    assert_eq!(recovered.report.cache_misses(), 0);
}
