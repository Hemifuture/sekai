use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraph,
    StageGraphBuilder,
};
use sekai::generators::natural::{
    AuthorConstraintsArtifact, RulePackSetArtifact, RuleTectonicResolutionStage,
    TectonicRuleResolutionArtifact, TectonicSpecArtifact,
};
use sekai::rules::{
    default_rule_pack_set, earthlike_rule_pack, AuthorConstraints, CapabilityContribution,
    ConstraintStrength, CoreSchemaRange, RuleItemId, RulePack, RulePackId, RulePackKind,
    RulePackSet, RuleTectonicConstraint, RuleVersion, TectonicConstraintClause, TectonicModel,
};
use sekai::world::natural::{TectonicSpec, TECTONIC_SPEC_SCHEMA_V1};
use sekai::world::{RootSeed, WORLD_SPEC_SCHEMA_V1};

fn resolution_graph() -> StageGraph {
    StageGraphBuilder::new()
        .external::<TectonicSpecArtifact>()
        .external::<RulePackSetArtifact>()
        .external::<AuthorConstraintsArtifact>()
        .stage(RuleTectonicResolutionStage)
        .build()
        .unwrap()
}

fn resolution_inputs(
    spec: TectonicSpec,
    packs: RulePackSet,
    authors: AuthorConstraints,
) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts.insert(TectonicSpecArtifact::new(spec)).unwrap();
    artifacts.insert(RulePackSetArtifact::new(packs)).unwrap();
    artifacts
        .insert(AuthorConstraintsArtifact::new(authors))
        .unwrap();
    artifacts
}

fn hard_constraint_pack() -> RulePack {
    RulePack::new(
        RulePackId::new("sekai.test.conflict").unwrap(),
        RuleVersion::new(1, 0, 0).unwrap(),
        RulePackKind::Ordinary,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
        Vec::new(),
        Vec::new(),
        vec![
            CapabilityContribution::TectonicConstraint(
                RuleTectonicConstraint::new(
                    RuleItemId::new("high-range").unwrap(),
                    ConstraintStrength::Hard,
                    TectonicConstraintClause::plate_count(20, 24).unwrap(),
                )
                .unwrap(),
            ),
            CapabilityContribution::TectonicConstraint(
                RuleTectonicConstraint::new(
                    RuleItemId::new("low-range").unwrap(),
                    ConstraintStrength::Hard,
                    TectonicConstraintClause::plate_count(2, 4).unwrap(),
                )
                .unwrap(),
            ),
        ],
    )
    .unwrap()
}

#[test]
fn resolution_artifacts_have_exact_stable_keys() {
    assert_eq!(RulePackSetArtifact::KEY.as_str(), "rules.pack-set");
    assert_eq!(
        AuthorConstraintsArtifact::KEY.as_str(),
        "rules.author-constraints"
    );
    assert_eq!(
        TectonicRuleResolutionArtifact::KEY.as_str(),
        "natural.tectonic-rule-resolution"
    );
}

#[test]
fn resolution_artifacts_round_trip_and_validate() {
    let packs = RulePackSetArtifact::new(default_rule_pack_set().unwrap());
    let packs_json = serde_json::to_vec(&packs).unwrap();
    let packs_decoded: RulePackSetArtifact = serde_json::from_slice(&packs_json).unwrap();
    packs_decoded.validate().unwrap();
    assert_eq!(packs_decoded, packs);

    let authors = AuthorConstraintsArtifact::new(AuthorConstraints::default());
    let authors_json = serde_json::to_vec(&authors).unwrap();
    let authors_decoded: AuthorConstraintsArtifact = serde_json::from_slice(&authors_json).unwrap();
    authors_decoded.validate().unwrap();
    assert_eq!(authors_decoded, authors);

    let outcome = BuildEngine::new(resolution_graph())
        .build(
            RootSeed::new(42),
            resolution_inputs(
                TectonicSpec::default(),
                default_rule_pack_set().unwrap(),
                AuthorConstraints::default(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let resolution = outcome
        .artifacts
        .get::<TectonicRuleResolutionArtifact>()
        .unwrap();
    let resolution_json = serde_json::to_vec(resolution.as_ref()).unwrap();
    let resolution_decoded: TectonicRuleResolutionArtifact =
        serde_json::from_slice(&resolution_json).unwrap();
    resolution_decoded.validate().unwrap();
    assert_eq!(resolution_decoded, *resolution);
}

#[test]
fn resolution_artifact_deserialization_revalidates_malformed_inner_contracts() {
    let packs = RulePackSetArtifact::new(default_rule_pack_set().unwrap());
    let mut bad_packs = serde_json::to_value(&packs).unwrap();
    bad_packs["pack_set"]["packs"][0]["manifest"]["content_hash"][0] = serde_json::json!(255);
    assert!(serde_json::from_value::<RulePackSetArtifact>(bad_packs).is_err());

    let authors = AuthorConstraintsArtifact::new(AuthorConstraints::default());
    let mut bad_authors = serde_json::to_value(&authors).unwrap();
    bad_authors["constraints"]["schema_version"] = serde_json::json!(9);
    assert!(serde_json::from_value::<AuthorConstraintsArtifact>(bad_authors).is_err());

    let outcome = BuildEngine::new(resolution_graph())
        .build(
            RootSeed::new(42),
            resolution_inputs(
                TectonicSpec::default(),
                default_rule_pack_set().unwrap(),
                AuthorConstraints::default(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let resolution = outcome
        .artifacts
        .get::<TectonicRuleResolutionArtifact>()
        .unwrap();
    let mut bad_resolution = serde_json::to_value(resolution.as_ref()).unwrap();
    bad_resolution["resolution"]["spec"]["schema_version"] = serde_json::json!(9);
    assert!(serde_json::from_value::<TectonicRuleResolutionArtifact>(bad_resolution).is_err());
}

#[test]
fn resolution_stage_has_exact_identity_dependencies_and_output() {
    let stage = RuleTectonicResolutionStage;
    assert_eq!(stage.id().as_str(), "natural.resolve-tectonic-rules");
    assert_eq!(stage.version(), 1);
    assert_eq!(stage.namespace(), "sekai.core");

    let graph = resolution_graph();
    assert_eq!(graph.stage_ids(), vec!["natural.resolve-tectonic-rules"]);
    let descriptor = &graph.descriptors()[0];
    assert_eq!(
        descriptor
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.tectonic-spec",
            "rules.author-constraints",
            "rules.pack-set",
        ]
    );
    assert_eq!(descriptor.output(), TectonicRuleResolutionArtifact::KEY);
}

#[test]
fn resolution_default_stage_preserves_base_spec_and_selects_current_slice_v1() {
    let base = TectonicSpec {
        schema_version: TECTONIC_SPEC_SCHEMA_V1,
        plate_count: 31,
        continental_crust_fraction: f32::from_bits(0x3e_c2_31_5a),
        activity: sekai::world::natural::TectonicActivity::Active,
    };
    let outcome = BuildEngine::new(resolution_graph())
        .build(
            RootSeed::new(42),
            resolution_inputs(
                base.clone(),
                default_rule_pack_set().unwrap(),
                AuthorConstraints::default(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let resolution = outcome
        .artifacts
        .get::<TectonicRuleResolutionArtifact>()
        .unwrap();

    assert_eq!(
        resolution.resolution().model(),
        TectonicModel::CurrentSliceV1
    );
    assert_eq!(resolution.resolution().spec().plate_count, base.plate_count);
    assert_eq!(
        resolution
            .resolution()
            .spec()
            .continental_crust_fraction
            .to_bits(),
        base.continental_crust_fraction.to_bits()
    );
    assert_eq!(resolution.resolution().spec().activity, base.activity);
    assert!(resolution.resolution().adoptions().is_empty());
}

#[test]
fn resolution_ignores_stage_rng_for_identical_semantic_inputs() {
    let engine = BuildEngine::new(resolution_graph());
    let first = engine
        .build(
            RootSeed::new(1),
            resolution_inputs(
                TectonicSpec::default(),
                default_rule_pack_set().unwrap(),
                AuthorConstraints::default(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let second = engine
        .build(
            RootSeed::new(2),
            resolution_inputs(
                TectonicSpec::default(),
                default_rule_pack_set().unwrap(),
                AuthorConstraints::default(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    assert_eq!(
        first
            .artifacts
            .hash::<TectonicRuleResolutionArtifact>()
            .unwrap(),
        second
            .artifacts
            .hash::<TectonicRuleResolutionArtifact>()
            .unwrap()
    );
}

#[test]
fn resolution_hard_conflict_has_stable_rule_code_and_publishes_no_output() {
    let packs =
        RulePackSet::new(vec![earthlike_rule_pack().unwrap(), hard_constraint_pack()]).unwrap();
    let mut cache = MemoryStageCache::new();
    let failure = BuildEngine::new(resolution_graph())
        .build(
            RootSeed::new(42),
            resolution_inputs(TectonicSpec::default(), packs, AuthorConstraints::default()),
            &mut cache,
        )
        .unwrap_err();

    assert_eq!(failure.report.diagnostics().len(), 1);
    let diagnostic = &failure.report.diagnostics()[0];
    assert_eq!(diagnostic.code(), "rules.hard-constraint-conflict");
    assert!(diagnostic.message().contains("high-range"));
    assert!(diagnostic.message().contains("low-range"));
    assert!(cache.is_empty());
}
