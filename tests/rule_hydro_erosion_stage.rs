use sekai::engine::{
    Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage, StageGraph,
    StageGraphBuilder,
};
use sekai::generators::natural::{
    HydroErosionRuleResolutionArtifact, HydroErosionSpecArtifact, ResolvedHydroErosionInput,
    ResolvedHydroErosionInputArtifact, ResolvedHydroErosionInputStage,
    RuleHydroErosionResolutionStage, RulePackSetArtifact,
};
use sekai::rules::{
    default_rule_pack_set, CapabilityContribution, ClimateModel, CoreSchemaRange, GeologicModel,
    HydroErosionModel, RulePack, RulePackId, RulePackKind, RulePackSet, RuleVersion, TectonicModel,
};
use sekai::world::natural::HydroErosionSpec;
use sekai::world::{RootSeed, WORLD_SPEC_SCHEMA_V1};

fn hydro_rule_graph() -> StageGraph {
    StageGraphBuilder::new()
        .external::<HydroErosionSpecArtifact>()
        .external::<RulePackSetArtifact>()
        .stage(RuleHydroErosionResolutionStage)
        .stage(ResolvedHydroErosionInputStage)
        .build()
        .unwrap()
}

fn inputs(spec: HydroErosionSpec, packs: RulePackSet) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(HydroErosionSpecArtifact::new(spec))
        .unwrap();
    artifacts.insert(RulePackSetArtifact::new(packs)).unwrap();
    artifacts
}

fn alternate_world_law() -> RulePack {
    RulePack::new(
        RulePackId::new("sekai.test.hydro-audit-alternate").unwrap(),
        RuleVersion::new(1, 6, 0).unwrap(),
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
fn artifacts_have_exact_stable_keys() {
    assert_eq!(
        HydroErosionSpecArtifact::KEY.as_str(),
        "natural.hydro-erosion-spec"
    );
    assert_eq!(
        HydroErosionRuleResolutionArtifact::KEY.as_str(),
        "rules.hydro-erosion-resolution"
    );
    assert_eq!(
        ResolvedHydroErosionInputArtifact::KEY.as_str(),
        "natural.resolved-hydro-erosion-input"
    );
}

#[test]
fn stages_have_exact_identity_dependencies_and_outputs() {
    assert_eq!(
        RuleHydroErosionResolutionStage.id().as_str(),
        "natural.resolve-hydro-erosion-rules"
    );
    assert_eq!(RuleHydroErosionResolutionStage.version(), 1);
    assert_eq!(RuleHydroErosionResolutionStage.namespace(), "sekai.core");
    assert_eq!(
        ResolvedHydroErosionInputStage.id().as_str(),
        "natural.project-hydro-erosion-input"
    );
    assert_eq!(ResolvedHydroErosionInputStage.version(), 1);
    assert_eq!(ResolvedHydroErosionInputStage.namespace(), "sekai.core");

    let graph = hydro_rule_graph();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.resolve-hydro-erosion-rules",
            "natural.project-hydro-erosion-input",
        ]
    );
    let resolution = &graph.descriptors()[0];
    assert_eq!(
        resolution
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["natural.hydro-erosion-spec", "rules.pack-set"]
    );
    assert_eq!(resolution.output(), HydroErosionRuleResolutionArtifact::KEY);

    let projection = &graph.descriptors()[1];
    assert_eq!(
        projection
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["rules.hydro-erosion-resolution"]
    );
    assert_eq!(projection.output(), ResolvedHydroErosionInputArtifact::KEY);
}

#[test]
fn transport_round_trips_and_revalidates_private_state() {
    let spec = HydroErosionSpecArtifact::new(HydroErosionSpec::default());
    let encoded = serde_json::to_vec(&spec).unwrap();
    let decoded: HydroErosionSpecArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, spec);

    let projection = ResolvedHydroErosionInputArtifact::new(
        ResolvedHydroErosionInput::new(
            HydroErosionModel::PriorityFloodStreamPowerV1,
            HydroErosionSpec::default(),
        )
        .unwrap(),
    );
    let encoded = serde_json::to_vec(&projection).unwrap();
    let decoded: ResolvedHydroErosionInputArtifact = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, projection);

    let mut malformed = serde_json::to_value(&projection).unwrap();
    malformed["input"]["spec"]["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<ResolvedHydroErosionInputArtifact>(malformed).is_err());
}

#[test]
fn projection_contains_only_validated_model_and_spec() {
    let outcome = BuildEngine::new(hydro_rule_graph())
        .build(
            RootSeed::new(42),
            inputs(
                HydroErosionSpec::default(),
                default_rule_pack_set().unwrap(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let projection = outcome
        .artifacts
        .get::<ResolvedHydroErosionInputArtifact>()
        .unwrap();

    assert_eq!(
        projection.input().model(),
        HydroErosionModel::PriorityFloodStreamPowerV1
    );
    assert_eq!(projection.input().spec(), &HydroErosionSpec::default());
    let object = serde_json::to_value(projection.input()).unwrap();
    assert_eq!(object.as_object().unwrap().len(), 2);
    projection.validate().unwrap();
}

#[test]
fn stages_ignore_rng_for_identical_semantic_inputs() {
    let engine = BuildEngine::new(hydro_rule_graph());
    let first = engine
        .build(
            RootSeed::new(1),
            inputs(
                HydroErosionSpec::default(),
                default_rule_pack_set().unwrap(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let second = engine
        .build(
            RootSeed::new(2),
            inputs(
                HydroErosionSpec::default(),
                default_rule_pack_set().unwrap(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    assert_eq!(
        first
            .artifacts
            .hash::<HydroErosionRuleResolutionArtifact>()
            .unwrap(),
        second
            .artifacts
            .hash::<HydroErosionRuleResolutionArtifact>()
            .unwrap()
    );
    assert_eq!(
        first
            .artifacts
            .hash::<ResolvedHydroErosionInputArtifact>()
            .unwrap(),
        second
            .artifacts
            .hash::<ResolvedHydroErosionInputArtifact>()
            .unwrap()
    );
}

#[test]
fn audit_identity_changes_without_invalidating_projected_input() {
    let engine = BuildEngine::new(hydro_rule_graph());
    let default = engine
        .build(
            RootSeed::new(42),
            inputs(
                HydroErosionSpec::default(),
                default_rule_pack_set().unwrap(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let alternate = engine
        .build(
            RootSeed::new(42),
            inputs(
                HydroErosionSpec::default(),
                RulePackSet::new(vec![alternate_world_law()]).unwrap(),
            ),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    assert_ne!(
        default
            .artifacts
            .hash::<HydroErosionRuleResolutionArtifact>()
            .unwrap(),
        alternate
            .artifacts
            .hash::<HydroErosionRuleResolutionArtifact>()
            .unwrap()
    );
    assert_eq!(
        default
            .artifacts
            .hash::<ResolvedHydroErosionInputArtifact>()
            .unwrap(),
        alternate
            .artifacts
            .hash::<ResolvedHydroErosionInputArtifact>()
            .unwrap()
    );
}
