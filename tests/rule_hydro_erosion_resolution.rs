use sekai::rules::{
    core_capability_registry, default_rule_pack_set, earthlike_rule_pack,
    hydro_erosion_model_capability_id, CapabilityContribution, ClimateModel, CoreSchemaRange,
    GeologicModel, HydroErosionModel, HydroErosionRuleResolution, HydroErosionRuleResolutionError,
    HydroErosionRuleResolver, RulePack, RulePackId, RulePackKind, RulePackSet, RulePackSetError,
    RuleVersion, TectonicModel, HYDRO_EROSION_RULE_RESOLUTION_SCHEMA_V1,
};
use sekai::world::natural::{HydroErosionSpec, MAX_EROSION_STRENGTH_PERMILLE};
use sekai::world::WORLD_SPEC_SCHEMA_V1;

fn alternate_world_law(id: &str) -> RulePack {
    RulePack::new(
        RulePackId::new(id).unwrap(),
        RuleVersion::new(1, 0, 0).unwrap(),
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

fn empty_ordinary_pack(id: &str) -> RulePack {
    RulePack::new(
        RulePackId::new(id).unwrap(),
        RuleVersion::new(1, 0, 0).unwrap(),
        RulePackKind::Ordinary,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

fn resolve(packs: &RulePackSet, spec: &HydroErosionSpec) -> HydroErosionRuleResolution {
    let registry = core_capability_registry().unwrap();
    let resolved_packs = packs.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap();
    HydroErosionRuleResolver::resolve(spec, &resolved_packs).unwrap()
}

#[test]
fn default_world_law_resolves_priority_flood_stream_power() {
    let resolution = resolve(
        &default_rule_pack_set().unwrap(),
        &HydroErosionSpec::default(),
    );

    assert_eq!(
        resolution.schema_version(),
        HYDRO_EROSION_RULE_RESOLUTION_SCHEMA_V1
    );
    assert_eq!(
        resolution.model(),
        HydroErosionModel::PriorityFloodStreamPowerV1
    );
    assert_eq!(resolution.spec(), &HydroErosionSpec::default());
    assert_eq!(resolution.resolved_packs().len(), 1);
    resolution.validate().unwrap();
}

#[test]
fn input_pack_order_produces_identical_audit_json() {
    let earthlike = earthlike_rule_pack().unwrap();
    let ordinary = empty_ordinary_pack("sekai.test.empty-hydro-content");
    let alternate = alternate_world_law("sekai.test.alternate-hydro-world-law");
    let first = resolve(
        &RulePackSet::new(vec![earthlike.clone(), ordinary.clone()]).unwrap(),
        &HydroErosionSpec::default(),
    );
    let second = resolve(
        &RulePackSet::new(vec![ordinary, earthlike]).unwrap(),
        &HydroErosionSpec::default(),
    );

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );

    let alternate_resolution = resolve(
        &RulePackSet::new(vec![alternate]).unwrap(),
        &HydroErosionSpec::default(),
    );
    assert_ne!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&alternate_resolution).unwrap()
    );
}

#[test]
fn missing_and_multiple_models_fail_in_the_pure_resolver() {
    let empty = RulePackSet::default();
    let dependency_order = empty.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap();
    assert!(matches!(
        HydroErosionRuleResolver::resolve(&HydroErosionSpec::default(), &dependency_order),
        Err(HydroErosionRuleResolutionError::MissingHydroErosionModel)
    ));

    let duplicates = RulePackSet::new(vec![
        earthlike_rule_pack().unwrap(),
        alternate_world_law("sekai.test.second-pure-hydro-law"),
    ])
    .unwrap();
    let dependency_order = duplicates
        .resolve_dependencies(WORLD_SPEC_SCHEMA_V1)
        .unwrap();
    assert!(matches!(
        HydroErosionRuleResolver::resolve(&HydroErosionSpec::default(), &dependency_order),
        Err(HydroErosionRuleResolutionError::MultipleHydroErosionModels)
    ));
}

#[test]
fn duplicate_model_fails_during_capability_resolution() {
    let packs = RulePackSet::new(vec![
        earthlike_rule_pack().unwrap(),
        RulePack::new(
            RulePackId::new("sekai.test.second-hydro-law").unwrap(),
            RuleVersion::new(1, 0, 0).unwrap(),
            RulePackKind::WorldLaw,
            CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
            Vec::new(),
            Vec::new(),
            vec![CapabilityContribution::HydroErosionModel(
                HydroErosionModel::PriorityFloodStreamPowerV1,
            )],
        )
        .unwrap(),
    ])
    .unwrap();

    assert!(matches!(
        packs.resolve(
            &core_capability_registry().unwrap(),
            WORLD_SPEC_SCHEMA_V1
        ),
        Err(RulePackSetError::MultipleCapabilityProviders { capability_id, .. })
            if capability_id == hydro_erosion_model_capability_id()
    ));
}

#[test]
fn invalid_base_spec_fails_before_model_resolution() {
    let invalid = HydroErosionSpec {
        erosion_strength_permille: MAX_EROSION_STRENGTH_PERMILLE + 1,
        ..HydroErosionSpec::default()
    };
    let empty = RulePackSet::default();
    let dependency_order = empty.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap();

    assert!(matches!(
        HydroErosionRuleResolver::resolve(&invalid, &dependency_order),
        Err(HydroErosionRuleResolutionError::InvalidBaseSpec(_))
    ));
}

#[test]
fn audit_round_trips_and_private_json_mutations_are_rejected() {
    let resolution = resolve(
        &default_rule_pack_set().unwrap(),
        &HydroErosionSpec::default(),
    );
    let encoded = serde_json::to_vec(&resolution).unwrap();
    let decoded: HydroErosionRuleResolution = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, resolution);

    let mut bad_schema = serde_json::to_value(&resolution).unwrap();
    bad_schema["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<HydroErosionRuleResolution>(bad_schema).is_err());

    let mut duplicate_pack = serde_json::to_value(&resolution).unwrap();
    let first_pack = duplicate_pack["resolved_packs"][0].clone();
    duplicate_pack["resolved_packs"]
        .as_array_mut()
        .unwrap()
        .push(first_pack);
    assert!(serde_json::from_value::<HydroErosionRuleResolution>(duplicate_pack).is_err());

    let mut invalid_spec = serde_json::to_value(&resolution).unwrap();
    invalid_spec["spec"]["erosion_strength_permille"] =
        serde_json::json!(u32::from(MAX_EROSION_STRENGTH_PERMILLE) + 1);
    assert!(serde_json::from_value::<HydroErosionRuleResolution>(invalid_spec).is_err());
}
