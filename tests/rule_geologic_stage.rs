use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraph,
    StageGraphBuilder,
};
use sekai::generators::natural::{
    GeologicRuleResolutionArtifact, GeologicSpecArtifact, ResolvedGeologicInput,
    ResolvedGeologicInputArtifact, ResolvedGeologicInputStage, RuleGeologicResolutionStage,
    RulePackSetArtifact,
};
use sekai::rules::{
    default_rule_pack_set, CapabilityContribution, ClimateModel, CoreSchemaRange, GeologicModel,
    HydroErosionModel, RulePack, RulePackId, RulePackKind, RulePackSet, RuleVersion, TectonicModel,
};
use sekai::world::natural::GeologicSpec;
use sekai::world::{RootSeed, WORLD_SPEC_SCHEMA_V1};

fn geologic_rule_graph() -> StageGraph {
    StageGraphBuilder::new()
        .external::<GeologicSpecArtifact>()
        .external::<RulePackSetArtifact>()
        .stage(RuleGeologicResolutionStage)
        .stage(ResolvedGeologicInputStage)
        .build()
        .unwrap()
}

fn inputs(spec: GeologicSpec, packs: RulePackSet) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts.insert(GeologicSpecArtifact::new(spec)).unwrap();
    artifacts.insert(RulePackSetArtifact::new(packs)).unwrap();
    artifacts
}

fn alternate_world_law() -> RulePack {
    RulePack::new(
        RulePackId::new("sekai.test.geologic-audit-alternate").unwrap(),
        RuleVersion::new(1, 4, 0).unwrap(),
        RulePackKind::WorldLaw,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
        Vec::new(),
        Vec::new(),
        vec![
            CapabilityContribution::TectonicModel(TectonicModel::CurrentSliceV1),
            CapabilityContribution::GeologicModel(GeologicModel::CurrentSliceV1),
            CapabilityContribution::ClimateModel(ClimateModel::SeasonalEnergyMoistureV1),
            CapabilityContribution::HydroErosionModel(
                HydroErosionModel::PriorityFloodStreamPowerV1,
            ),
        ],
    )
    .unwrap()
}

#[test]
fn geologic_rule_artifacts_have_exact_stable_keys() {
    assert_eq!(GeologicSpecArtifact::KEY.as_str(), "natural.geologic-spec");
    assert_eq!(
        GeologicRuleResolutionArtifact::KEY.as_str(),
        "rules.geologic-resolution"
    );
    assert_eq!(
        ResolvedGeologicInputArtifact::KEY.as_str(),
        "natural.resolved-geologic-input"
    );
}

#[test]
fn geologic_rule_stages_have_exact_identity_dependencies_and_outputs() {
    assert_eq!(
        RuleGeologicResolutionStage.id().as_str(),
        "natural.resolve-geologic-rules"
    );
    assert_eq!(RuleGeologicResolutionStage.version(), 1);
    assert_eq!(RuleGeologicResolutionStage.namespace(), "sekai.core");
    assert_eq!(
        ResolvedGeologicInputStage.id().as_str(),
        "natural.project-geologic-input"
    );
    assert_eq!(ResolvedGeologicInputStage.version(), 1);
    assert_eq!(ResolvedGeologicInputStage.namespace(), "sekai.core");

    let graph = geologic_rule_graph();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.resolve-geologic-rules",
            "natural.project-geologic-input"
        ]
    );
    let resolution = &graph.descriptors()[0];
    assert_eq!(
        resolution
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["natural.geologic-spec", "rules.pack-set"]
    );
    assert_eq!(resolution.output(), GeologicRuleResolutionArtifact::KEY);

    let projection = &graph.descriptors()[1];
    assert_eq!(
        projection
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["rules.geologic-resolution"]
    );
    assert_eq!(projection.output(), ResolvedGeologicInputArtifact::KEY);
}

#[test]
fn geologic_transport_round_trips_and_revalidates_private_state() {
    let spec = GeologicSpecArtifact::new(GeologicSpec::default());
    let encoded = serde_json::to_vec(&spec).unwrap();
    let decoded: GeologicSpecArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, spec);

    let projection = ResolvedGeologicInputArtifact::new(
        ResolvedGeologicInput::new(GeologicModel::CurrentSliceV1, GeologicSpec::default()).unwrap(),
    );
    let encoded = serde_json::to_vec(&projection).unwrap();
    let decoded: ResolvedGeologicInputArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, projection);

    let mut malformed = serde_json::to_value(&projection).unwrap();
    malformed["input"]["spec"]["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<ResolvedGeologicInputArtifact>(malformed).is_err());
}

#[test]
fn projection_contains_only_validated_model_and_spec() {
    let outcome = BuildEngine::new(geologic_rule_graph())
        .build(
            RootSeed::new(42),
            inputs(GeologicSpec::default(), default_rule_pack_set().unwrap()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let projection = outcome
        .artifacts
        .get::<ResolvedGeologicInputArtifact>()
        .unwrap();

    assert_eq!(projection.input().model(), GeologicModel::CurrentSliceV1);
    assert_eq!(projection.input().spec(), &GeologicSpec::default());
    let object = serde_json::to_value(projection.input()).unwrap();
    assert_eq!(object.as_object().unwrap().len(), 2);
    projection.validate().unwrap();
}

#[test]
fn stages_ignore_rng_for_identical_semantic_inputs() {
    let engine = BuildEngine::new(geologic_rule_graph());
    let first = engine
        .build(
            RootSeed::new(1),
            inputs(GeologicSpec::default(), default_rule_pack_set().unwrap()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let second = engine
        .build(
            RootSeed::new(2),
            inputs(GeologicSpec::default(), default_rule_pack_set().unwrap()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    assert_eq!(
        first
            .artifacts
            .hash::<GeologicRuleResolutionArtifact>()
            .unwrap(),
        second
            .artifacts
            .hash::<GeologicRuleResolutionArtifact>()
            .unwrap()
    );
    assert_eq!(
        first
            .artifacts
            .hash::<ResolvedGeologicInputArtifact>()
            .unwrap(),
        second
            .artifacts
            .hash::<ResolvedGeologicInputArtifact>()
            .unwrap()
    );
}

#[test]
fn audit_identity_changes_without_invalidating_projected_input() {
    let engine = BuildEngine::new(geologic_rule_graph());
    let default = engine
        .build(
            RootSeed::new(42),
            inputs(GeologicSpec::default(), default_rule_pack_set().unwrap()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let alternate = engine
        .build(
            RootSeed::new(42),
            inputs(
                GeologicSpec::default(),
                RulePackSet::new(vec![alternate_world_law()]).unwrap(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    assert_ne!(
        default
            .artifacts
            .hash::<GeologicRuleResolutionArtifact>()
            .unwrap(),
        alternate
            .artifacts
            .hash::<GeologicRuleResolutionArtifact>()
            .unwrap()
    );
    assert_eq!(
        default
            .artifacts
            .hash::<ResolvedGeologicInputArtifact>()
            .unwrap(),
        alternate
            .artifacts
            .hash::<ResolvedGeologicInputArtifact>()
            .unwrap()
    );
}
