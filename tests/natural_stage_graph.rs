use sekai::engine::{
    Artifact, ArtifactError, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage,
    StageGraphBuilder,
};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ReliefArtifact, ReliefStage,
    ResolvedTectonicInput, ResolvedTectonicInputArtifact, RulePackSetArtifact, TectonicArtifact,
    TectonicRuleResolutionArtifact, TectonicSpecArtifact, TectonicStage,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact, SpatialStage};
use sekai::rules::{
    default_rule_pack_set, earthlike_rule_pack, AuthorConstraint, AuthorConstraints,
    CapabilityContribution, ConstraintStrength, CoreSchemaRange, RuleItemId, RulePack, RulePackId,
    RulePackKind, RulePackSet, RuleTectonicConstraint, RuleVersion, TectonicConstraintClause,
    TectonicModel, AUTHOR_CONSTRAINTS_SCHEMA_V1,
};
use sekai::world::natural::{TectonicActivity, TectonicSpec, TECTONIC_SPEC_SCHEMA_V1};
use sekai::world::AuthorObjectId;
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

fn complete_external(plate_count: u16) -> ExternalArtifacts {
    complete_external_with(
        plate_count,
        default_rule_pack_set().unwrap(),
        AuthorConstraints::default(),
    )
}

fn complete_external_with(
    plate_count: u16,
    packs: RulePackSet,
    authors: AuthorConstraints,
) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts.insert(PlanarSpaceArtifact::new(space())).unwrap();
    artifacts
        .insert(TectonicSpecArtifact::new(tectonic_spec(plate_count)))
        .unwrap();
    artifacts.insert(RulePackSetArtifact::new(packs)).unwrap();
    artifacts
        .insert(AuthorConstraintsArtifact::new(authors))
        .unwrap();
    artifacts
}

fn plate_control_pack(
    name: &str,
    strength: ConstraintStrength,
    minimum: u16,
    maximum: u16,
) -> RulePack {
    RulePack::new(
        RulePackId::new(format!("sekai.test.{name}")).unwrap(),
        RuleVersion::new(1, 0, 0).unwrap(),
        RulePackKind::Ordinary,
        CoreSchemaRange::new(1, 1).unwrap(),
        Vec::new(),
        Vec::new(),
        vec![CapabilityContribution::TectonicConstraint(
            RuleTectonicConstraint::new(
                RuleItemId::new("plate-control").unwrap(),
                strength,
                TectonicConstraintClause::plate_count(minimum, maximum).unwrap(),
            )
            .unwrap(),
        )],
    )
    .unwrap()
}

fn tectonic_external(plate_count: u16) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts.insert(PlanarSpaceArtifact::new(space())).unwrap();
    artifacts
        .insert(ResolvedTectonicInputArtifact::new(
            ResolvedTectonicInput::new(TectonicModel::CurrentSliceV1, tectonic_spec(plate_count))
                .unwrap(),
        ))
        .unwrap();
    artifacts
}

fn graph() -> sekai::engine::StageGraph {
    StageGraphBuilder::new()
        .external::<PlanarSpaceArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
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
            tectonic_external(12),
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
        vec!["natural.resolved-tectonic-input", "world.spatial"]
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
            tectonic_external(12),
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
        .build(RootSeed::new(42), tectonic_external(12), &mut cache)
        .unwrap();
    let repeated = engine
        .build(RootSeed::new(42), tectonic_external(12), &mut cache)
        .unwrap();

    assert_eq!(repeated.report.cache_hits(), 2);
    assert_eq!(repeated.report.cache_misses(), 0);
}

#[test]
fn changing_only_tectonic_spec_reuses_spatial_stage() {
    let engine = BuildEngine::new(graph());
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), tectonic_external(12), &mut cache)
        .unwrap();
    let changed = engine
        .build(RootSeed::new(42), tectonic_external(17), &mut cache)
        .unwrap();

    assert_eq!(changed.report.cache_hits(), 1);
    assert_eq!(changed.report.cache_misses(), 1);
}

#[test]
fn changing_root_seed_reruns_both_tectonic_graph_stages() {
    let engine = BuildEngine::new(graph());
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), tectonic_external(12), &mut cache)
        .unwrap();
    let changed = engine
        .build(RootSeed::new(43), tectonic_external(12), &mut cache)
        .unwrap();

    assert_eq!(changed.report.cache_hits(), 0);
    assert_eq!(changed.report.cache_misses(), 2);
}

#[test]
fn complete_natural_graph_publishes_relief_with_exact_stage_metadata() {
    assert_eq!(ReliefArtifact::KEY.as_str(), "world.relief");
    let stage = ReliefStage;
    assert_eq!(stage.id().as_str(), "natural.relief");
    assert_eq!(stage.namespace(), "sekai.core");
    assert_eq!(stage.version(), 1);

    let graph = natural_foundation_graph().unwrap();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.resolve-tectonic-rules",
            "natural.project-tectonic-input",
            "spatial.planar-voronoi",
            "natural.tectonics",
            "natural.relief"
        ]
    );
    let descriptor = &graph.descriptors()[4];
    assert_eq!(
        descriptor
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["world.spatial", "world.tectonics"]
    );
    assert_eq!(descriptor.output(), ReliefArtifact::KEY);
}

#[test]
fn complete_natural_graph_artifacts_and_hashes_are_deterministic() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let first = engine
        .build(
            RootSeed::new(42),
            complete_external(12),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let second = engine
        .build(
            RootSeed::new(42),
            complete_external(12),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let spatial = first.artifacts.get::<SpatialArtifact>().unwrap();
    let tectonic = first.artifacts.get::<TectonicArtifact>().unwrap();
    let relief = first.artifacts.get::<ReliefArtifact>().unwrap();

    tectonic
        .snapshot()
        .validate_against(spatial.snapshot())
        .unwrap();
    relief
        .snapshot()
        .validate_against(spatial.snapshot())
        .unwrap();
    assert_eq!(
        first.artifacts.hash::<SpatialArtifact>().unwrap(),
        second.artifacts.hash::<SpatialArtifact>().unwrap()
    );
    assert_eq!(
        first.artifacts.hash::<TectonicArtifact>().unwrap(),
        second.artifacts.hash::<TectonicArtifact>().unwrap()
    );
    assert_eq!(
        first.artifacts.hash::<ReliefArtifact>().unwrap(),
        second.artifacts.hash::<ReliefArtifact>().unwrap()
    );
    assert_eq!(first.report.result_hash(), second.report.result_hash());
}

#[test]
fn complete_natural_graph_cache_tracks_transitive_tectonic_changes() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), complete_external(12), &mut cache)
        .unwrap();
    let repeated = engine
        .build(RootSeed::new(42), complete_external(12), &mut cache)
        .unwrap();
    assert_eq!(repeated.report.cache_hits(), 5);
    assert_eq!(repeated.report.cache_misses(), 0);

    let changed = engine
        .build(RootSeed::new(42), complete_external(17), &mut cache)
        .unwrap();
    assert_eq!(changed.report.cache_hits(), 1);
    assert_eq!(changed.report.cache_misses(), 4);
}

#[test]
fn complete_natural_graph_requires_both_new_rule_external_artifacts() {
    let mut missing_both = ExternalArtifacts::new();
    missing_both
        .insert(PlanarSpaceArtifact::new(space()))
        .unwrap();
    missing_both
        .insert(TectonicSpecArtifact::new(tectonic_spec(12)))
        .unwrap();
    let failure = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(42),
            missing_both,
            &mut MemoryStageCache::new(),
        )
        .unwrap_err();
    assert!(failure.report.stage_ids().is_empty());
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "engine.external-artifact"
    );

    let mut missing_authors = ExternalArtifacts::new();
    missing_authors
        .insert(PlanarSpaceArtifact::new(space()))
        .unwrap();
    missing_authors
        .insert(TectonicSpecArtifact::new(tectonic_spec(12)))
        .unwrap();
    missing_authors
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    let failure = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(42),
            missing_authors,
            &mut MemoryStageCache::new(),
        )
        .unwrap_err();
    assert!(failure.report.stage_ids().is_empty());
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "engine.external-artifact"
    );
}

#[test]
fn malformed_relief_artifact_cannot_publish() {
    let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(42),
            complete_external(12),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let relief = outcome.artifacts.get::<ReliefArtifact>().unwrap();
    let mut wire = serde_json::to_value(relief.as_ref()).unwrap();
    wire["snapshot"]["elevation_m"][0] = serde_json::json!(50_000.0);
    let invalid: ReliefArtifact = serde_json::from_value(wire).unwrap();
    let error = invalid.validate().unwrap_err();

    assert_eq!(error.code(), "natural.invalid-relief");
}

#[test]
fn cache_audit_only_rule_change_does_not_invalidate_tectonics_or_relief() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let baseline = engine
        .build(RootSeed::new(42), complete_external(12), &mut cache)
        .unwrap();
    let satisfied = plate_control_pack("satisfied-control", ConstraintStrength::Hard, 10, 14);
    let changed_set = RulePackSet::new(vec![earthlike_rule_pack().unwrap(), satisfied]).unwrap();
    let changed = engine
        .build(
            RootSeed::new(42),
            complete_external_with(12, changed_set, AuthorConstraints::default()),
            &mut cache,
        )
        .unwrap();

    assert_eq!(changed.report.cache_hits(), 3);
    assert_eq!(changed.report.cache_misses(), 2);
    assert_ne!(
        baseline
            .artifacts
            .hash::<TectonicRuleResolutionArtifact>()
            .unwrap(),
        changed
            .artifacts
            .hash::<TectonicRuleResolutionArtifact>()
            .unwrap()
    );
    assert_eq!(
        baseline
            .artifacts
            .hash::<ResolvedTectonicInputArtifact>()
            .unwrap(),
        changed
            .artifacts
            .hash::<ResolvedTectonicInputArtifact>()
            .unwrap()
    );
    assert_eq!(
        baseline.artifacts.hash::<TectonicArtifact>().unwrap(),
        changed.artifacts.hash::<TectonicArtifact>().unwrap()
    );
    assert_eq!(
        baseline.artifacts.hash::<ReliefArtifact>().unwrap(),
        changed.artifacts.hash::<ReliefArtifact>().unwrap()
    );
    assert_ne!(baseline.report.result_hash(), changed.report.result_hash());
}

#[test]
fn cache_projected_spec_change_invalidates_only_natural_downstream() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let baseline = engine
        .build(RootSeed::new(42), complete_external(12), &mut cache)
        .unwrap();
    let force_seventeen = plate_control_pack("force-seventeen", ConstraintStrength::Hard, 17, 17);
    let changed_set =
        RulePackSet::new(vec![earthlike_rule_pack().unwrap(), force_seventeen]).unwrap();
    let changed = engine
        .build(
            RootSeed::new(42),
            complete_external_with(12, changed_set, AuthorConstraints::default()),
            &mut cache,
        )
        .unwrap();

    assert_eq!(changed.report.cache_hits(), 1);
    assert_eq!(changed.report.cache_misses(), 4);
    assert_eq!(
        changed
            .artifacts
            .get::<ResolvedTectonicInputArtifact>()
            .unwrap()
            .input()
            .spec()
            .plate_count,
        17
    );
    assert_ne!(
        baseline
            .artifacts
            .hash::<ResolvedTectonicInputArtifact>()
            .unwrap(),
        changed
            .artifacts
            .hash::<ResolvedTectonicInputArtifact>()
            .unwrap()
    );
    assert_ne!(
        baseline.artifacts.hash::<TectonicArtifact>().unwrap(),
        changed.artifacts.hash::<TectonicArtifact>().unwrap()
    );
    assert_ne!(
        baseline.artifacts.hash::<ReliefArtifact>().unwrap(),
        changed.artifacts.hash::<ReliefArtifact>().unwrap()
    );
}

#[test]
fn cache_rule_failure_publishes_nothing_and_preserves_prior_valid_entries() {
    let engine = BuildEngine::new(natural_foundation_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), complete_external(12), &mut cache)
        .unwrap();
    assert_eq!(cache.len(), 5);

    let pack_hard = plate_control_pack("low-only", ConstraintStrength::Hard, 2, 4);
    let conflict_set = RulePackSet::new(vec![earthlike_rule_pack().unwrap(), pack_hard]).unwrap();
    let author_hard = AuthorConstraint::new(
        AuthorObjectId::from_raw(99),
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(20, 24).unwrap(),
    )
    .unwrap();
    let conflict_authors =
        AuthorConstraints::new(AUTHOR_CONSTRAINTS_SCHEMA_V1, vec![author_hard]).unwrap();
    let failure = engine
        .build(
            RootSeed::new(42),
            complete_external_with(12, conflict_set, conflict_authors),
            &mut cache,
        )
        .unwrap_err();

    assert_eq!(failure.report.diagnostics().len(), 1);
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "rules.hard-constraint-conflict"
    );
    assert_eq!(cache.len(), 5);

    let recovered = engine
        .build(RootSeed::new(42), complete_external(12), &mut cache)
        .unwrap();
    assert_eq!(recovered.report.cache_hits(), 5);
    assert_eq!(recovered.report.cache_misses(), 0);
}
