use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraphBuilder,
};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact,
    HydroErosionSpecArtifact, PreliminaryClimateArtifact, PreliminaryClimateStage, ReliefArtifact,
    ResolvedClimateInputArtifact, RulePackSetArtifact, TectonicSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateSpec, ElevationField, GeologicSpec, HydroErosionSpec, LandOceanField, ReliefSnapshot,
    TectonicSpec, RELIEF_SCHEMA_V2,
};
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

fn complete_external(climate_spec: ClimateSpec) -> ExternalArtifacts {
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
        .insert(ClimateSpecArtifact::new(climate_spec))
        .unwrap();
    external
        .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
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
    assert_eq!(
        PreliminaryClimateArtifact::KEY.as_str(),
        "world.preliminary-climate"
    );
    assert_eq!(
        PreliminaryClimateStage.id().as_str(),
        "natural.preliminary-climate"
    );
    assert_eq!(PreliminaryClimateStage.version(), 1);
    assert_eq!(PreliminaryClimateStage.namespace(), "sekai.core");

    let graph = StageGraphBuilder::new()
        .external::<ResolvedClimateInputArtifact>()
        .external::<SpatialArtifact>()
        .external::<ReliefArtifact>()
        .stage(PreliminaryClimateStage)
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
            "natural.resolved-climate-input",
            "world.relief",
            "world.spatial",
        ]
    );
    assert_eq!(descriptor.output(), PreliminaryClimateArtifact::KEY);
}

#[test]
fn production_graph_publishes_valid_climate_and_caches_all_fifteen_stages() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(
            RootSeed::new(42),
            complete_external(ClimateSpec::default()),
            &mut cache,
        )
        .unwrap();
    let repeated = engine
        .build(
            RootSeed::new(42),
            complete_external(ClimateSpec::default()),
            &mut cache,
        )
        .unwrap();
    let climate = first.artifacts.get::<PreliminaryClimateArtifact>().unwrap();
    let spatial = first.artifacts.get::<SpatialArtifact>().unwrap();
    let relief = first.artifacts.get::<ReliefArtifact>().unwrap();

    climate
        .snapshot()
        .validate_against(spatial.snapshot(), relief.snapshot())
        .unwrap();
    assert_eq!(repeated.report.cache_hits(), 15);
    assert_eq!(repeated.report.cache_misses(), 0);
    assert_eq!(
        first
            .artifacts
            .hash::<PreliminaryClimateArtifact>()
            .unwrap(),
        repeated
            .artifacts
            .hash::<PreliminaryClimateArtifact>()
            .unwrap()
    );
}

#[test]
fn climate_spec_change_reuses_every_non_climate_stage() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let baseline = engine
        .build(
            RootSeed::new(42),
            complete_external(ClimateSpec::default()),
            &mut cache,
        )
        .unwrap();
    let warm_spec = ClimateSpec {
        temperature_offset_deci_c: 80,
        ..ClimateSpec::default()
    };
    let changed = engine
        .build(RootSeed::new(42), complete_external(warm_spec), &mut cache)
        .unwrap();

    assert_eq!(changed.report.cache_hits(), 11);
    assert_eq!(changed.report.cache_misses(), 4);
    assert_eq!(
        baseline.artifacts.hash::<SpatialArtifact>().unwrap(),
        changed.artifacts.hash::<SpatialArtifact>().unwrap()
    );
    assert_eq!(
        baseline.artifacts.hash::<ReliefArtifact>().unwrap(),
        changed.artifacts.hash::<ReliefArtifact>().unwrap()
    );
    assert_ne!(
        baseline
            .artifacts
            .hash::<PreliminaryClimateArtifact>()
            .unwrap(),
        changed
            .artifacts
            .hash::<PreliminaryClimateArtifact>()
            .unwrap()
    );
}

#[test]
fn cross_artifact_failure_does_not_poison_valid_cached_climate() {
    let upstream = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(42),
            complete_external(ClimateSpec::default()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let graph = StageGraphBuilder::new()
        .external::<ResolvedClimateInputArtifact>()
        .external::<ReliefArtifact>()
        .external::<SpatialArtifact>()
        .stage(PreliminaryClimateStage)
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
                    .get::<ResolvedClimateInputArtifact>()
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
                    .get::<SpatialArtifact>()
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
        "natural.invalid-preliminary-climate-input"
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
