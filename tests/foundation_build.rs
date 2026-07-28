use sekai::engine::{
    Artifact, ArtifactError, BuildEngine, BuildOutcome, ExternalArtifacts, MemoryStageCache,
};
use sekai::generators::spatial::{foundation_graph, PlanarSpaceArtifact, SpatialArtifact};
use sekai::world::{
    BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed, TechnologyBaseline, WorldSpec,
    WORLD_SPEC_SCHEMA_V1,
};

fn spec(seed: u64) -> WorldSpec {
    WorldSpec {
        schema_version: WORLD_SPEC_SCHEMA_V1,
        root_seed: RootSeed::new(seed),
        space: PlanarSpaceSpec {
            width: Meters::new(1_000.0).unwrap(),
            height: Meters::new(500.0).unwrap(),
            target_cell_count: 128,
            boundary: BoundaryCondition::Closed,
        },
        technology: TechnologyBaseline::PreIndustrialMedieval,
    }
}

fn external(space: PlanarSpaceSpec) -> ExternalArtifacts {
    let mut inputs = ExternalArtifacts::new();
    inputs.insert(PlanarSpaceArtifact::new(space)).unwrap();
    inputs
}

fn build_spatial(seed: u64) -> BuildOutcome {
    let world_spec = spec(seed);
    let engine = BuildEngine::new(foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    engine
        .build(world_spec.root_seed, external(world_spec.space), &mut cache)
        .unwrap()
}

fn build_twice_with_shared_cache(world_spec: WorldSpec) -> (BuildOutcome, BuildOutcome) {
    let engine = BuildEngine::new(foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(
            world_spec.root_seed,
            external(world_spec.space.clone()),
            &mut cache,
        )
        .unwrap();
    let second = engine
        .build(world_spec.root_seed, external(world_spec.space), &mut cache)
        .unwrap();
    (first, second)
}

#[test]
fn builds_valid_spatial_artifact_and_report() {
    let graph = foundation_graph().unwrap();
    let engine = BuildEngine::new(graph);
    let world_spec = spec(42);
    world_spec.validate().unwrap();
    let root_seed = world_spec.root_seed;
    let mut cache = MemoryStageCache::new();

    let result = engine
        .build(root_seed, external(world_spec.space), &mut cache)
        .unwrap();
    let spatial = result.artifacts.get::<SpatialArtifact>().unwrap();

    spatial.snapshot().validate().unwrap();
    assert_eq!(result.report.stage_ids(), vec!["spatial.planar-voronoi"]);
    assert!(!result.report.has_errors());
    assert!(result.report.diagnostics().is_empty());
}

#[test]
fn artifact_wrappers_round_trip_without_semantic_change() {
    let planar = PlanarSpaceArtifact::new(spec(42).space);
    let encoded = serde_json::to_vec(&planar).unwrap();
    let decoded: PlanarSpaceArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(planar, decoded);

    let outcome = build_spatial(42);
    let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();
    let encoded = serde_json::to_vec(spatial.as_ref()).unwrap();
    let decoded: SpatialArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    let reencoded = serde_json::to_vec(&decoded).unwrap();
    assert_eq!(encoded, reencoded);
    assert_eq!(spatial.as_ref(), &decoded);
}

#[test]
fn deserialized_spatial_artifact_is_validated_at_the_artifact_boundary() {
    let outcome = build_spatial(42);
    let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();
    let mut wire = serde_json::to_value(spatial.as_ref()).unwrap();
    wire["snapshot"]["schema_version"] = serde_json::json!(u16::MAX);
    let decoded: SpatialArtifact = serde_json::from_value(wire).unwrap();

    let error = decoded.validate().unwrap_err();

    assert_eq!(error.code(), "spatial.invalid-snapshot");
    assert_eq!(
        error.message(),
        "unsupported spatial schema version 65535; supported version is 1"
    );
}

#[test]
fn repeated_foundation_build_uses_cache_and_keeps_semantic_hashes() {
    let (first, second) = build_twice_with_shared_cache(spec(42));

    assert_eq!(
        first.artifacts.hash::<SpatialArtifact>().unwrap(),
        second.artifacts.hash::<SpatialArtifact>().unwrap(),
    );
    assert_eq!(first.report.result_hash(), second.report.result_hash());
    assert_eq!(first.report.cache_misses(), 1);
    assert_eq!(second.report.cache_hits(), 1);
}

#[test]
fn different_root_seed_invalidates_cache_and_changes_output() {
    let engine = BuildEngine::new(foundation_graph().unwrap());
    let world_spec = spec(42);
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(
            RootSeed::new(42),
            external(world_spec.space.clone()),
            &mut cache,
        )
        .unwrap();
    let second = engine
        .build(RootSeed::new(43), external(world_spec.space), &mut cache)
        .unwrap();

    assert_ne!(
        first.artifacts.hash::<SpatialArtifact>().unwrap(),
        second.artifacts.hash::<SpatialArtifact>().unwrap(),
    );
    assert_ne!(first.report.result_hash(), second.report.result_hash());
    assert_eq!(second.report.cache_hits(), 0);
    assert_eq!(second.report.cache_misses(), 1);
}

#[test]
fn invalid_external_planar_spec_is_rejected_before_build() {
    let mut invalid = spec(42).space;
    invalid.target_cell_count = 15;
    let mut inputs = ExternalArtifacts::new();

    let error = inputs
        .insert(PlanarSpaceArtifact::new(invalid))
        .unwrap_err();

    match error {
        ArtifactError::Validation {
            artifact_key,
            source,
        } => {
            assert_eq!(artifact_key, PlanarSpaceArtifact::KEY);
            assert_eq!(source.code(), "spatial.invalid-spec");
            assert_eq!(source.message(), "cell count 15 is outside 16..=200000");
        }
        other => panic!("expected validation failure, got {other:?}"),
    }
}

#[test]
fn foundation_graph_has_exact_stage_metadata_and_dependency() {
    let graph = foundation_graph().unwrap();

    assert_eq!(graph.stage_ids(), vec!["spatial.planar-voronoi"]);
    assert_eq!(graph.descriptors().len(), 1);
    let descriptor = &graph.descriptors()[0];
    assert_eq!(descriptor.id().as_str(), "spatial.planar-voronoi");
    assert_eq!(descriptor.namespace(), "sekai.core");
    assert_eq!(descriptor.version(), 1);
    assert_eq!(descriptor.output(), SpatialArtifact::KEY);
    assert_eq!(descriptor.dependencies(), &[PlanarSpaceArtifact::KEY]);
}
