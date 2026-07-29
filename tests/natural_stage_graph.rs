use sekai::engine::{
    Artifact, ArtifactError, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage,
    StageGraphBuilder,
};
use sekai::generators::natural::{TectonicArtifact, TectonicSpecArtifact, TectonicStage};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact, SpatialStage};
use sekai::world::natural::{TectonicActivity, TectonicSpec, TECTONIC_SPEC_SCHEMA_V1};
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

fn space() -> PlanarSpaceSpec {
    PlanarSpaceSpec {
        width: Meters::new(1_000_000.0).unwrap(),
        height: Meters::new(600_000.0).unwrap(),
        target_cell_count: 256,
        boundary: BoundaryCondition::Closed,
    }
}

fn tectonic_spec(plate_count: u16) -> TectonicSpec {
    TectonicSpec {
        schema_version: TECTONIC_SPEC_SCHEMA_V1,
        plate_count,
        continental_crust_fraction: 0.38,
        activity: TectonicActivity::Moderate,
    }
}

fn external(plate_count: u16) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts.insert(PlanarSpaceArtifact::new(space())).unwrap();
    artifacts
        .insert(TectonicSpecArtifact::new(tectonic_spec(plate_count)))
        .unwrap();
    artifacts
}

fn graph() -> sekai::engine::StageGraph {
    StageGraphBuilder::new()
        .external::<PlanarSpaceArtifact>()
        .external::<TectonicSpecArtifact>()
        .stage(TectonicStage)
        .stage(SpatialStage)
        .build()
        .unwrap()
}

#[test]
fn tectonic_artifacts_have_stable_keys_and_round_trip() {
    assert_eq!(TectonicSpecArtifact::KEY.as_str(), "natural.tectonic-spec");
    assert_eq!(TectonicArtifact::KEY.as_str(), "world.tectonics");

    let spec = TectonicSpecArtifact::new(tectonic_spec(12));
    let encoded = serde_json::to_vec(&spec).unwrap();
    let decoded: TectonicSpecArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, spec);

    let outcome = BuildEngine::new(graph())
        .build(
            RootSeed::new(42),
            external(12),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let tectonic = outcome.artifacts.get::<TectonicArtifact>().unwrap();
    let encoded = serde_json::to_vec(tectonic.as_ref()).unwrap();
    let decoded: TectonicArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn tectonic_stage_declares_exact_identity_and_dependencies() {
    let stage = TectonicStage;
    assert_eq!(stage.id().as_str(), "natural.tectonics");
    assert_eq!(stage.namespace(), "sekai.core");
    assert_eq!(stage.version(), 1);

    let graph = graph();
    assert_eq!(
        graph.stage_ids(),
        vec!["spatial.planar-voronoi", "natural.tectonics"]
    );
    let descriptor = &graph.descriptors()[1];
    assert_eq!(
        descriptor
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["natural.tectonic-spec", "world.spatial"]
    );
    assert_eq!(descriptor.output(), TectonicArtifact::KEY);
}

#[test]
fn invalid_tectonic_spec_is_rejected_before_stage_execution() {
    let mut invalid = tectonic_spec(12);
    invalid.plate_count = 1;
    let mut artifacts = ExternalArtifacts::new();
    let error = artifacts
        .insert(TectonicSpecArtifact::new(invalid))
        .unwrap_err();

    assert!(matches!(
        error,
        ArtifactError::Validation { source, .. }
            if source.code() == "natural.invalid-tectonic-spec"
    ));
}

#[test]
fn successful_tectonic_stage_publishes_a_complete_valid_snapshot() {
    let outcome = BuildEngine::new(graph())
        .build(
            RootSeed::new(42),
            external(12),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();
    let tectonic = outcome.artifacts.get::<TectonicArtifact>().unwrap();

    tectonic.validate().unwrap();
    tectonic
        .snapshot()
        .validate_against(spatial.snapshot())
        .unwrap();
    assert_eq!(
        outcome.report.stage_ids(),
        vec!["spatial.planar-voronoi", "natural.tectonics"]
    );
    assert_eq!(outcome.report.cache_misses(), 2);
}

#[test]
fn repeated_tectonic_build_hits_both_stage_caches() {
    let engine = BuildEngine::new(graph());
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), external(12), &mut cache)
        .unwrap();
    let repeated = engine
        .build(RootSeed::new(42), external(12), &mut cache)
        .unwrap();

    assert_eq!(repeated.report.cache_hits(), 2);
    assert_eq!(repeated.report.cache_misses(), 0);
}

#[test]
fn changing_only_tectonic_spec_reuses_spatial_stage() {
    let engine = BuildEngine::new(graph());
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), external(12), &mut cache)
        .unwrap();
    let changed = engine
        .build(RootSeed::new(42), external(17), &mut cache)
        .unwrap();

    assert_eq!(changed.report.cache_hits(), 1);
    assert_eq!(changed.report.cache_misses(), 1);
}

#[test]
fn changing_root_seed_reruns_both_tectonic_graph_stages() {
    let engine = BuildEngine::new(graph());
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), external(12), &mut cache)
        .unwrap();
    let changed = engine
        .build(RootSeed::new(43), external(12), &mut cache)
        .unwrap();

    assert_eq!(changed.report.cache_hits(), 0);
    assert_eq!(changed.report.cache_misses(), 2);
}
