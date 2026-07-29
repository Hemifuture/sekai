use sekai::rules::{
    climate_model_capability_id, geologic_model_capability_id, tectonic_controls_capability_id,
    tectonic_model_capability_id, CapabilityCardinality, CapabilityContribution,
    CapabilityDescriptor, CapabilityRegistry, CapabilityRegistryBuilder, CapabilityRegistryError,
    ClimateModel, ConstraintStrength, GeologicModel, RuleItemId, RulePackKind,
    RuleTectonicConstraint, TectonicConstraintClause, TectonicModel,
};

fn model_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor::new(
        tectonic_model_capability_id(),
        CapabilityCardinality::UniqueRequired,
        RulePackKind::WorldLaw,
        false,
    )
}

fn geologic_model_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor::new(
        geologic_model_capability_id(),
        CapabilityCardinality::UniqueRequired,
        RulePackKind::WorldLaw,
        false,
    )
}

fn climate_model_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor::new(
        climate_model_capability_id(),
        CapabilityCardinality::UniqueRequired,
        RulePackKind::WorldLaw,
        false,
    )
}

fn controls_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor::new(
        tectonic_controls_capability_id(),
        CapabilityCardinality::Merge,
        RulePackKind::Ordinary,
        true,
    )
}

#[test]
fn capability_descriptors_keep_cardinality_permission_and_author_access() {
    let model = model_descriptor();
    let geologic_model = geologic_model_descriptor();
    let climate_model = climate_model_descriptor();
    let controls = controls_descriptor();

    assert_eq!(model.id(), &tectonic_model_capability_id());
    assert_eq!(model.cardinality(), CapabilityCardinality::UniqueRequired);
    assert_eq!(model.minimum_pack_kind(), RulePackKind::WorldLaw);
    assert!(!model.author_allowed());
    assert!(!model.allows_pack_kind(RulePackKind::Ordinary));
    assert!(model.allows_pack_kind(RulePackKind::WorldLaw));

    assert_eq!(geologic_model.id(), &geologic_model_capability_id());
    assert_eq!(
        geologic_model.cardinality(),
        CapabilityCardinality::UniqueRequired
    );
    assert_eq!(geologic_model.minimum_pack_kind(), RulePackKind::WorldLaw);
    assert!(!geologic_model.author_allowed());
    assert!(!geologic_model.allows_pack_kind(RulePackKind::Ordinary));
    assert!(geologic_model.allows_pack_kind(RulePackKind::WorldLaw));

    assert_eq!(climate_model.id(), &climate_model_capability_id());
    assert_eq!(
        climate_model.cardinality(),
        CapabilityCardinality::UniqueRequired
    );
    assert_eq!(climate_model.minimum_pack_kind(), RulePackKind::WorldLaw);
    assert!(!climate_model.author_allowed());
    assert!(!climate_model.allows_pack_kind(RulePackKind::Ordinary));
    assert!(climate_model.allows_pack_kind(RulePackKind::WorldLaw));

    assert_eq!(controls.cardinality(), CapabilityCardinality::Merge);
    assert!(controls.author_allowed());
    assert!(controls.allows_pack_kind(RulePackKind::Ordinary));
    assert!(controls.allows_pack_kind(RulePackKind::WorldLaw));
}

#[test]
fn capability_registry_is_frozen_sorted_and_duplicate_safe() {
    let mut builder = CapabilityRegistryBuilder::new();
    builder.register(model_descriptor()).unwrap();
    let duplicate = builder.register(model_descriptor()).unwrap_err();
    assert!(matches!(
        duplicate,
        CapabilityRegistryError::DuplicateCapability { .. }
    ));
    builder.register(geologic_model_descriptor()).unwrap();
    builder.register(climate_model_descriptor()).unwrap();
    builder.register(controls_descriptor()).unwrap();
    let registry = builder.build();

    let ids: Vec<_> = registry
        .iter()
        .map(|descriptor| descriptor.id().clone())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(registry.len(), 4);
    assert_eq!(
        registry
            .get(&tectonic_model_capability_id())
            .unwrap()
            .cardinality(),
        CapabilityCardinality::UniqueRequired
    );
    assert_eq!(
        registry
            .get(&tectonic_controls_capability_id())
            .unwrap()
            .cardinality(),
        CapabilityCardinality::Merge
    );
}

#[test]
fn empty_capability_registry_is_valid_and_read_only() {
    let registry = CapabilityRegistryBuilder::new().build();

    assert!(registry.is_empty());
    assert!(registry.iter().next().is_none());
    assert!(registry.get(&tectonic_model_capability_id()).is_none());
}

#[test]
fn closed_contributions_report_their_exact_capability() {
    let model = CapabilityContribution::TectonicModel(TectonicModel::CurrentSliceV1);
    let geologic_model = CapabilityContribution::GeologicModel(GeologicModel::CurrentSliceV1);
    let climate_model =
        CapabilityContribution::ClimateModel(ClimateModel::SeasonalEnergyMoistureV1);
    let constraint = CapabilityContribution::TectonicConstraint(
        RuleTectonicConstraint::new(
            RuleItemId::new("prefer-twelve-plates").unwrap(),
            ConstraintStrength::soft(10).unwrap(),
            TectonicConstraintClause::plate_count(12, 12).unwrap(),
        )
        .unwrap(),
    );

    assert_eq!(model.capability_id(), tectonic_model_capability_id());
    assert_eq!(
        geologic_model.capability_id(),
        geologic_model_capability_id()
    );
    assert_eq!(climate_model.capability_id(), climate_model_capability_id());
    assert_eq!(
        constraint.capability_id(),
        tectonic_controls_capability_id()
    );
    assert_eq!(model.rule_item_id(), None);
    assert_eq!(geologic_model.rule_item_id(), None);
    assert_eq!(climate_model.rule_item_id(), None);
    assert_eq!(
        constraint.rule_item_id().unwrap().as_str(),
        "prefer-twelve-plates"
    );
    model.validate().unwrap();
    geologic_model.validate().unwrap();
    climate_model.validate().unwrap();
    constraint.validate().unwrap();
}

#[test]
fn registry_serialization_is_stable_and_revalidates_duplicates() {
    let mut forward = CapabilityRegistryBuilder::new();
    forward.register(model_descriptor()).unwrap();
    forward.register(geologic_model_descriptor()).unwrap();
    forward.register(climate_model_descriptor()).unwrap();
    forward.register(controls_descriptor()).unwrap();
    let forward = forward.build();

    let mut reverse = CapabilityRegistryBuilder::new();
    reverse.register(controls_descriptor()).unwrap();
    reverse.register(climate_model_descriptor()).unwrap();
    reverse.register(geologic_model_descriptor()).unwrap();
    reverse.register(model_descriptor()).unwrap();
    let reverse = reverse.build();

    let forward_json = serde_json::to_vec(&forward).unwrap();
    let reverse_json = serde_json::to_vec(&reverse).unwrap();
    assert_eq!(forward_json, reverse_json);
    assert_eq!(
        serde_json::from_slice::<CapabilityRegistry>(&forward_json).unwrap(),
        forward
    );

    let mut duplicate = serde_json::to_value(&forward).unwrap();
    let first = duplicate.as_array().unwrap().first().unwrap().clone();
    duplicate.as_array_mut().unwrap().push(first);
    assert!(serde_json::from_value::<CapabilityRegistry>(duplicate).is_err());
}
