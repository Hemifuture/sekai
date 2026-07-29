use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraph,
    StageGraphBuilder,
};
use sekai::generators::natural::{
    ClimateRuleResolutionArtifact, ClimateSpecArtifact, ResolvedClimateInput,
    ResolvedClimateInputArtifact, ResolvedClimateInputStage, RuleClimateResolutionStage,
    RulePackSetArtifact,
};
use sekai::rules::{
    default_rule_pack_set, CapabilityContribution, ClimateModel, CoreSchemaRange, GeologicModel,
    RulePack, RulePackId, RulePackKind, RulePackSet, RuleVersion, TectonicModel,
};
use sekai::world::natural::ClimateSpec;
use sekai::world::{RootSeed, WORLD_SPEC_SCHEMA_V1};

fn climate_rule_graph() -> StageGraph {
    StageGraphBuilder::new()
        .external::<ClimateSpecArtifact>()
        .external::<RulePackSetArtifact>()
        .stage(RuleClimateResolutionStage)
        .stage(ResolvedClimateInputStage)
        .build()
        .unwrap()
}

fn inputs(spec: ClimateSpec, packs: RulePackSet) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts.insert(ClimateSpecArtifact::new(spec)).unwrap();
    artifacts.insert(RulePackSetArtifact::new(packs)).unwrap();
    artifacts
}

fn alternate_world_law() -> RulePack {
    RulePack::new(
        RulePackId::new("sekai.test.climate-audit-alternate").unwrap(),
        RuleVersion::new(1, 5, 0).unwrap(),
        RulePackKind::WorldLaw,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
        Vec::new(),
        Vec::new(),
        vec![
            CapabilityContribution::TectonicModel(TectonicModel::CurrentSliceV1),
            CapabilityContribution::GeologicModel(GeologicModel::CurrentSliceV1),
            CapabilityContribution::ClimateModel(ClimateModel::SeasonalEnergyMoistureV1),
        ],
    )
    .unwrap()
}

#[test]
fn climate_rule_artifacts_have_exact_stable_keys() {
    assert_eq!(ClimateSpecArtifact::KEY.as_str(), "natural.climate-spec");
    assert_eq!(
        ClimateRuleResolutionArtifact::KEY.as_str(),
        "rules.climate-resolution"
    );
    assert_eq!(
        ResolvedClimateInputArtifact::KEY.as_str(),
        "natural.resolved-climate-input"
    );
}

#[test]
fn climate_rule_stages_have_exact_identity_dependencies_and_outputs() {
    assert_eq!(
        RuleClimateResolutionStage.id().as_str(),
        "natural.resolve-climate-rules"
    );
    assert_eq!(RuleClimateResolutionStage.version(), 1);
    assert_eq!(RuleClimateResolutionStage.namespace(), "sekai.core");
    assert_eq!(
        ResolvedClimateInputStage.id().as_str(),
        "natural.project-climate-input"
    );
    assert_eq!(ResolvedClimateInputStage.version(), 1);
    assert_eq!(ResolvedClimateInputStage.namespace(), "sekai.core");

    let graph = climate_rule_graph();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.resolve-climate-rules",
            "natural.project-climate-input"
        ]
    );
    let resolution = &graph.descriptors()[0];
    assert_eq!(
        resolution
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["natural.climate-spec", "rules.pack-set"]
    );
    assert_eq!(resolution.output(), ClimateRuleResolutionArtifact::KEY);

    let projection = &graph.descriptors()[1];
    assert_eq!(
        projection
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["rules.climate-resolution"]
    );
    assert_eq!(projection.output(), ResolvedClimateInputArtifact::KEY);
}

#[test]
fn climate_transport_round_trips_and_revalidates_private_state() {
    let spec = ClimateSpecArtifact::new(ClimateSpec::default());
    let encoded = serde_json::to_vec(&spec).unwrap();
    let decoded: ClimateSpecArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, spec);

    let projection = ResolvedClimateInputArtifact::new(
        ResolvedClimateInput::new(
            ClimateModel::SeasonalEnergyMoistureV1,
            ClimateSpec::default(),
        )
        .unwrap(),
    );
    let encoded = serde_json::to_vec(&projection).unwrap();
    let decoded: ResolvedClimateInputArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, projection);

    let mut malformed = serde_json::to_value(&projection).unwrap();
    malformed["input"]["spec"]["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<ResolvedClimateInputArtifact>(malformed).is_err());
}

#[test]
fn climate_projection_contains_only_validated_model_and_spec() {
    let outcome = BuildEngine::new(climate_rule_graph())
        .build(
            RootSeed::new(42),
            inputs(ClimateSpec::default(), default_rule_pack_set().unwrap()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let projection = outcome
        .artifacts
        .get::<ResolvedClimateInputArtifact>()
        .unwrap();

    assert_eq!(
        projection.input().model(),
        ClimateModel::SeasonalEnergyMoistureV1
    );
    assert_eq!(projection.input().spec(), &ClimateSpec::default());
    let object = serde_json::to_value(projection.input()).unwrap();
    assert_eq!(object.as_object().unwrap().len(), 2);
    projection.validate().unwrap();
}

#[test]
fn climate_rule_stages_ignore_rng_for_identical_semantic_inputs() {
    let engine = BuildEngine::new(climate_rule_graph());
    let first = engine
        .build(
            RootSeed::new(1),
            inputs(ClimateSpec::default(), default_rule_pack_set().unwrap()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let second = engine
        .build(
            RootSeed::new(2),
            inputs(ClimateSpec::default(), default_rule_pack_set().unwrap()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    assert_eq!(
        first
            .artifacts
            .hash::<ClimateRuleResolutionArtifact>()
            .unwrap(),
        second
            .artifacts
            .hash::<ClimateRuleResolutionArtifact>()
            .unwrap()
    );
    assert_eq!(
        first
            .artifacts
            .hash::<ResolvedClimateInputArtifact>()
            .unwrap(),
        second
            .artifacts
            .hash::<ResolvedClimateInputArtifact>()
            .unwrap()
    );
}

#[test]
fn climate_audit_identity_changes_without_invalidating_projected_input() {
    let engine = BuildEngine::new(climate_rule_graph());
    let default = engine
        .build(
            RootSeed::new(42),
            inputs(ClimateSpec::default(), default_rule_pack_set().unwrap()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let alternate = engine
        .build(
            RootSeed::new(42),
            inputs(
                ClimateSpec::default(),
                RulePackSet::new(vec![alternate_world_law()]).unwrap(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    assert_ne!(
        default
            .artifacts
            .hash::<ClimateRuleResolutionArtifact>()
            .unwrap(),
        alternate
            .artifacts
            .hash::<ClimateRuleResolutionArtifact>()
            .unwrap()
    );
    assert_eq!(
        default
            .artifacts
            .hash::<ResolvedClimateInputArtifact>()
            .unwrap(),
        alternate
            .artifacts
            .hash::<ResolvedClimateInputArtifact>()
            .unwrap()
    );
}
