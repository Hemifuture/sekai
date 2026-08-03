use sekai::engine::{
    Artifact, ArtifactError, BuildEngine, BuildOutcome, DiagnosticSeverity, ExternalArtifacts,
    MemoryStageCache,
};
use sekai::generators::spatial::{
    spherical_foundation_graph, SphericalSpaceArtifact, SphericalSurfaceArtifact,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

const RADIUS: f64 = 6_371_000.0;

fn space(target_cell_count: u32) -> SphericalSpaceSpec {
    SphericalSpaceSpec {
        radius: Meters::new(RADIUS).unwrap(),
        target_cell_count,
    }
}

fn external(space: SphericalSpaceSpec) -> ExternalArtifacts {
    let mut inputs = ExternalArtifacts::new();
    inputs.insert(SphericalSpaceArtifact::new(space)).unwrap();
    inputs
}

fn build_surface(seed: u64, target_cell_count: u32) -> BuildOutcome {
    let engine = BuildEngine::new(spherical_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    engine
        .build(
            RootSeed::new(seed),
            external(space(target_cell_count)),
            &mut cache,
        )
        .unwrap()
}

#[test]
fn builds_valid_spherical_surface_artifact_and_exact_stage_metadata() {
    let graph = spherical_foundation_graph().unwrap();
    assert_eq!(
        SphericalSpaceArtifact::KEY.as_str(),
        "spatial.spherical-spec"
    );
    assert_eq!(
        SphericalSurfaceArtifact::KEY.as_str(),
        "world.spherical-surface"
    );
    assert_eq!(graph.stage_ids(), vec!["spatial.spherical-voronoi"]);
    let descriptor = &graph.descriptors()[0];
    assert_eq!(descriptor.id().as_str(), "spatial.spherical-voronoi");
    assert_eq!(descriptor.namespace(), "sekai.core");
    assert_eq!(descriptor.version(), 1);
    assert_eq!(descriptor.output(), SphericalSurfaceArtifact::KEY);
    assert_eq!(descriptor.dependencies(), &[SphericalSpaceArtifact::KEY]);

    let engine = BuildEngine::new(graph);
    let mut cache = MemoryStageCache::new();
    let result = engine
        .build(RootSeed::new(42), external(space(42)), &mut cache)
        .unwrap();
    let surface = result.artifacts.get::<SphericalSurfaceArtifact>().unwrap();

    surface.snapshot().validate().unwrap();
    assert_eq!(result.report.stage_ids(), vec!["spatial.spherical-voronoi"]);
    assert!(!result.report.has_errors());
    assert!(result.report.diagnostics().is_empty());
}

#[test]
fn spherical_artifact_wrappers_round_trip_with_strict_validation() {
    let input = SphericalSpaceArtifact::new(space(42));
    let encoded = serde_json::to_vec(&input).unwrap();
    let decoded: SphericalSpaceArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(input, decoded);
    let mut unknown_input_field = serde_json::to_value(&input).unwrap();
    unknown_input_field["hidden_seed"] = serde_json::json!(42);
    assert!(serde_json::from_value::<SphericalSpaceArtifact>(unknown_input_field).is_err());
    let mut unknown_nested_spec_field = serde_json::to_value(&input).unwrap();
    unknown_nested_spec_field["space"]["hidden_seed"] = serde_json::json!(42);
    assert!(serde_json::from_value::<SphericalSpaceArtifact>(unknown_nested_spec_field).is_err());

    let outcome = build_surface(42, 42);
    let surface = outcome.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
    let encoded = serde_json::to_vec(surface.as_ref()).unwrap();
    let decoded: SphericalSurfaceArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(encoded, serde_json::to_vec(&decoded).unwrap());
    assert_eq!(surface.as_ref(), &decoded);
    let mut unknown_surface_field = serde_json::to_value(surface.as_ref()).unwrap();
    unknown_surface_field["render_cache"] = serde_json::json!({});
    assert!(serde_json::from_value::<SphericalSurfaceArtifact>(unknown_surface_field).is_err());

    let mut malformed = serde_json::to_value(surface.as_ref()).unwrap();
    malformed["snapshot"]["schema_version"] = serde_json::json!(u16::MAX);
    let malformed: SphericalSurfaceArtifact = serde_json::from_value(malformed).unwrap();
    let error = malformed.validate().unwrap_err();
    assert_eq!(error.code(), "spherical-spatial.invalid-snapshot");
    assert_eq!(
        error.message(),
        "unsupported spherical surface schema version 65535; supported version is 1"
    );
}

#[test]
fn identical_spherical_rebuild_hits_cache_with_stable_content_hash() {
    let engine = BuildEngine::new(spherical_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(RootSeed::new(42), external(space(42)), &mut cache)
        .unwrap();
    let second = engine
        .build(RootSeed::new(42), external(space(42)), &mut cache)
        .unwrap();

    assert_eq!(
        first.artifacts.hash::<SphericalSurfaceArtifact>().unwrap(),
        second.artifacts.hash::<SphericalSurfaceArtifact>().unwrap(),
    );
    assert_eq!(first.report.result_hash(), second.report.result_hash());
    assert_eq!(first.report.cache_misses(), 1);
    assert_eq!(second.report.cache_hits(), 1);
}

#[test]
fn root_seed_changes_do_not_change_spherical_surface_semantic_bytes() {
    let engine = BuildEngine::new(spherical_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(RootSeed::new(42), external(space(42)), &mut cache)
        .unwrap();
    let second = engine
        .build(RootSeed::new(43), external(space(42)), &mut cache)
        .unwrap();

    let first_surface = first.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
    let second_surface = second.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
    assert_eq!(
        serde_json::to_vec(first_surface.as_ref()).unwrap(),
        serde_json::to_vec(second_surface.as_ref()).unwrap(),
    );
    assert_eq!(
        first.artifacts.hash::<SphericalSurfaceArtifact>().unwrap(),
        second.artifacts.hash::<SphericalSurfaceArtifact>().unwrap(),
    );
    // The generic engine cache key includes the root-seed-derived stage seed even
    // though this deterministic stage consumes no random draws.
    assert_eq!(second.report.cache_hits(), 0);
    assert_eq!(second.report.cache_misses(), 1);
}

#[test]
fn resolution_emits_one_stable_info_diagnostic_without_semantic_contamination() {
    let engine = BuildEngine::new(spherical_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let outcome = engine
        .build(RootSeed::new(42), external(space(43)), &mut cache)
        .unwrap();

    let diagnostics = outcome.report.diagnostics();
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.severity(), DiagnosticSeverity::Info);
    assert_eq!(diagnostic.code(), "spherical-spatial.resolved-cell-count");
    assert_eq!(
        diagnostic.message(),
        "resolved spherical cell count 42 differs from requested target 43"
    );
    assert_eq!(
        diagnostic.context().stage_id.as_deref(),
        Some("spatial.spherical-voronoi")
    );
    assert!(diagnostic.context().field_id.is_none());
    assert!(diagnostic.context().cell_id.is_none());
    assert!(diagnostic.context().author_object_id.is_none());
    assert!(outcome.report.result_hash().is_some());

    let cached = engine
        .build(RootSeed::new(42), external(space(43)), &mut cache)
        .unwrap();
    assert_eq!(cached.report.cache_hits(), 1);
    assert_eq!(cached.report.diagnostics(), diagnostics);
    assert_eq!(cached.report.result_hash(), outcome.report.result_hash());
}

#[test]
fn invalid_external_spherical_spec_is_rejected_before_stage_execution() {
    let mut inputs = ExternalArtifacts::new();
    let error = inputs
        .insert(SphericalSpaceArtifact::new(space(42 - 1)))
        .unwrap_err();

    match error {
        ArtifactError::Validation {
            artifact_key,
            source,
        } => {
            assert_eq!(artifact_key, SphericalSpaceArtifact::KEY);
            assert_eq!(source.code(), "spherical-spatial.invalid-spec");
            assert_eq!(source.message(), "cell count 41 is outside 42..=198812");
        }
        other => panic!("expected validation failure, got {other:?}"),
    }
}
