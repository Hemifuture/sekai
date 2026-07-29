use sekai::rules::{
    core_capability_registry, default_rule_pack_set, earthlike_rule_pack,
    tectonic_controls_capability_id, tectonic_model_capability_id, CapabilityCardinality,
    CapabilityContribution, CoreSchemaRange, RulePack, RulePackId, RulePackKind, RulePackSet,
    RulePackSetError, RuleVersion, TectonicModel, EARTHLIKE_RULE_PACK_ID,
};
use sekai::world::WORLD_SPEC_SCHEMA_V1;

fn replacement_model(name: &str, kind: RulePackKind) -> RulePack {
    RulePack::new(
        RulePackId::new(format!("sekai.test.{name}")).unwrap(),
        RuleVersion::new(1, 0, 0).unwrap(),
        kind,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
        Vec::new(),
        Vec::new(),
        vec![CapabilityContribution::TectonicModel(
            TectonicModel::CurrentSliceV1,
        )],
    )
    .unwrap()
}

#[test]
fn builtin_capability_ids_are_exact_and_versioned() {
    let model = tectonic_model_capability_id();
    assert_eq!(model.namespace(), "sekai.core.natural");
    assert_eq!(model.name(), "tectonic-model");
    assert_eq!(model.version(), 1);

    let controls = tectonic_controls_capability_id();
    assert_eq!(controls.namespace(), "sekai.core.natural");
    assert_eq!(controls.name(), "tectonic-controls");
    assert_eq!(controls.version(), 1);
}

#[test]
fn builtin_capability_contracts_are_exact() {
    let registry = core_capability_registry().unwrap();
    let model = registry.get(&tectonic_model_capability_id()).unwrap();
    assert_eq!(model.cardinality(), CapabilityCardinality::UniqueRequired);
    assert_eq!(model.minimum_pack_kind(), RulePackKind::WorldLaw);
    assert!(!model.author_allowed());

    let controls = registry.get(&tectonic_controls_capability_id()).unwrap();
    assert_eq!(controls.cardinality(), CapabilityCardinality::Merge);
    assert_eq!(controls.minimum_pack_kind(), RulePackKind::Ordinary);
    assert!(controls.author_allowed());
    assert_eq!(registry.len(), 2);
}

#[test]
fn builtin_earthlike_pack_selects_exact_current_slice_model() {
    let earthlike = earthlike_rule_pack().unwrap();

    assert_eq!(earthlike.manifest().id().as_str(), EARTHLIKE_RULE_PACK_ID);
    assert_eq!(
        earthlike.manifest().version(),
        RuleVersion::new(1, 0, 0).unwrap()
    );
    assert_eq!(earthlike.manifest().kind(), RulePackKind::WorldLaw);
    assert_eq!(
        earthlike.manifest().core_schema(),
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap()
    );
    assert!(earthlike.manifest().dependencies().is_empty());
    assert!(earthlike.manifest().consumes().is_empty());
    assert_eq!(
        earthlike.manifest().provides(),
        &[tectonic_model_capability_id()]
    );
    assert_eq!(
        earthlike.contributions(),
        &[CapabilityContribution::TectonicModel(
            TectonicModel::CurrentSliceV1
        )]
    );
}

#[test]
fn builtin_default_set_contains_exactly_earthlike_and_resolves() {
    let registry = core_capability_registry().unwrap();
    let set = default_rule_pack_set().unwrap();

    assert_eq!(set.len(), 1);
    assert_eq!(
        set.packs()[0].manifest().id().as_str(),
        EARTHLIKE_RULE_PACK_ID
    );
    let resolved = set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(
        resolved
            .providers(&tectonic_model_capability_id())
            .first()
            .unwrap()
            .manifest()
            .id()
            .as_str(),
        EARTHLIKE_RULE_PACK_ID
    );
}

#[test]
fn builtin_ordinary_replacement_model_fails_permission() {
    let registry = core_capability_registry().unwrap();
    let set = RulePackSet::new(vec![replacement_model(
        "ordinary-model",
        RulePackKind::Ordinary,
    )])
    .unwrap();

    assert!(matches!(
        set.resolve(&registry, WORLD_SPEC_SCHEMA_V1),
        Err(RulePackSetError::InsufficientCapabilityPermission {
            capability_id,
            ..
        }) if capability_id == tectonic_model_capability_id()
    ));
}

#[test]
fn builtin_second_world_law_model_fails_unique_cardinality() {
    let registry = core_capability_registry().unwrap();
    let set = RulePackSet::new(vec![
        earthlike_rule_pack().unwrap(),
        replacement_model("second-law", RulePackKind::WorldLaw),
    ])
    .unwrap();

    assert!(matches!(
        set.resolve(&registry, WORLD_SPEC_SCHEMA_V1),
        Err(RulePackSetError::MultipleCapabilityProviders {
            capability_id,
            provider_ids,
        }) if capability_id == tectonic_model_capability_id() && provider_ids.len() == 2
    ));
}

#[test]
fn builtin_factories_have_deterministic_json_and_content_hashes() {
    let first_registry = core_capability_registry().unwrap();
    let second_registry = core_capability_registry().unwrap();
    assert_eq!(
        serde_json::to_vec(&first_registry).unwrap(),
        serde_json::to_vec(&second_registry).unwrap()
    );

    let first_pack = earthlike_rule_pack().unwrap();
    let second_pack = earthlike_rule_pack().unwrap();
    assert_eq!(
        first_pack.manifest().content_hash(),
        second_pack.manifest().content_hash()
    );
    assert_eq!(
        serde_json::to_vec(&first_pack).unwrap(),
        serde_json::to_vec(&second_pack).unwrap()
    );

    let first_set = default_rule_pack_set().unwrap();
    let second_set = default_rule_pack_set().unwrap();
    assert_eq!(
        serde_json::to_vec(&first_set).unwrap(),
        serde_json::to_vec(&second_set).unwrap()
    );
}
