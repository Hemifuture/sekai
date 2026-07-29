use sekai::rules::{
    tectonic_controls_capability_id, tectonic_model_capability_id, CapabilityCardinality,
    CapabilityContribution, CapabilityDescriptor, CapabilityId, CapabilityRegistry,
    CapabilityRegistryBuilder, ConstraintStrength, CoreSchemaRange, RuleItemId, RulePack,
    RulePackDependency, RulePackId, RulePackKind, RulePackSet, RulePackSetError,
    RuleTectonicConstraint, RuleVersion, RuleVersionRequirement, TectonicConstraintClause,
    TectonicModel, MAX_RULE_PACKS, MAX_RULE_SET_CONTRIBUTIONS,
};
use sekai::world::WORLD_SPEC_SCHEMA_V1;

fn pack_id(name: &str) -> RulePackId {
    RulePackId::new(format!("sekai.test.{name}")).unwrap()
}

fn dependency(name: &str, major: u16, minor: u16, patch: u16) -> RulePackDependency {
    RulePackDependency::new(
        pack_id(name),
        RuleVersionRequirement::new(major, minor, patch).unwrap(),
    )
}

fn pack(name: &str, version: (u16, u16, u16), dependencies: Vec<RulePackDependency>) -> RulePack {
    pack_with(
        name,
        version,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
        dependencies,
        Vec::new(),
    )
}

fn pack_with(
    name: &str,
    version: (u16, u16, u16),
    core_schema: CoreSchemaRange,
    dependencies: Vec<RulePackDependency>,
    contributions: Vec<CapabilityContribution>,
) -> RulePack {
    RulePack::new(
        pack_id(name),
        RuleVersion::new(version.0, version.1, version.2).unwrap(),
        RulePackKind::Ordinary,
        core_schema,
        dependencies,
        Vec::new(),
        contributions,
    )
    .unwrap()
}

fn typed_pack(
    name: &str,
    kind: RulePackKind,
    dependencies: Vec<RulePackDependency>,
    consumes: Vec<CapabilityId>,
    contributions: Vec<CapabilityContribution>,
) -> RulePack {
    RulePack::new(
        pack_id(name),
        RuleVersion::new(1, 0, 0).unwrap(),
        kind,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
        dependencies,
        consumes,
        contributions,
    )
    .unwrap()
}

fn plate_constraint(index: usize) -> CapabilityContribution {
    CapabilityContribution::TectonicConstraint(
        RuleTectonicConstraint::new(
            RuleItemId::new(format!("constraint-{index:04}")).unwrap(),
            ConstraintStrength::soft(1).unwrap(),
            TectonicConstraintClause::plate_count(12, 12).unwrap(),
        )
        .unwrap(),
    )
}

fn model_contribution() -> CapabilityContribution {
    CapabilityContribution::TectonicModel(TectonicModel::CurrentSliceV1)
}

fn model_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor::new(
        tectonic_model_capability_id(),
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

fn capability_registry(
    descriptors: impl IntoIterator<Item = CapabilityDescriptor>,
) -> CapabilityRegistry {
    let mut builder = CapabilityRegistryBuilder::new();
    for descriptor in descriptors {
        builder.register(descriptor).unwrap();
    }
    builder.build()
}

fn pack_names<'a>(packs: impl IntoIterator<Item = &'a RulePack>) -> Vec<&'a str> {
    packs
        .into_iter()
        .map(|pack| pack.manifest().id().as_str())
        .collect()
}

#[test]
fn dependency_set_normalizes_input_order_by_pack_id() {
    let set = RulePackSet::new(vec![
        pack("zeta", (1, 0, 0), Vec::new()),
        pack("alpha", (1, 0, 0), Vec::new()),
    ])
    .unwrap();

    assert_eq!(
        pack_names(set.packs()),
        vec!["sekai.test.alpha", "sekai.test.zeta"]
    );
}

#[test]
fn dependency_set_rejects_duplicate_pack_ids() {
    let error = RulePackSet::new(vec![
        pack("duplicate", (1, 0, 0), Vec::new()),
        pack("duplicate", (1, 1, 0), Vec::new()),
    ])
    .unwrap_err();

    assert_eq!(
        error,
        RulePackSetError::DuplicatePack {
            pack_id: pack_id("duplicate")
        }
    );
}

#[test]
fn dependency_set_enforces_pack_count_boundary() {
    let exact: Vec<_> = (0..MAX_RULE_PACKS)
        .map(|index| pack(&format!("pack-{index:02}"), (1, 0, 0), Vec::new()))
        .collect();
    assert_eq!(RulePackSet::new(exact).unwrap().len(), MAX_RULE_PACKS);

    let overflow: Vec<_> = (0..=MAX_RULE_PACKS)
        .map(|index| pack(&format!("overflow-{index:02}"), (1, 0, 0), Vec::new()))
        .collect();
    assert_eq!(
        RulePackSet::new(overflow).unwrap_err(),
        RulePackSetError::TooManyPacks {
            found: MAX_RULE_PACKS + 1
        }
    );
}

#[test]
fn dependency_set_enforces_total_contribution_boundary() {
    let full_contributions: Vec<_> = (0..256).map(plate_constraint).collect();
    let exact: Vec<_> = (0..16)
        .map(|index| {
            pack_with(
                &format!("full-{index:02}"),
                (1, 0, 0),
                CoreSchemaRange::new(1, 1).unwrap(),
                Vec::new(),
                full_contributions.clone(),
            )
        })
        .collect();
    assert_eq!(
        RulePackSet::new(exact).unwrap().contribution_count(),
        MAX_RULE_SET_CONTRIBUTIONS
    );

    let mut overflow: Vec<_> = (0..16)
        .map(|index| {
            pack_with(
                &format!("overflow-full-{index:02}"),
                (1, 0, 0),
                CoreSchemaRange::new(1, 1).unwrap(),
                Vec::new(),
                full_contributions.clone(),
            )
        })
        .collect();
    overflow.push(pack_with(
        "overflow-extra",
        (1, 0, 0),
        CoreSchemaRange::new(1, 1).unwrap(),
        Vec::new(),
        vec![plate_constraint(0)],
    ));
    assert_eq!(
        RulePackSet::new(overflow).unwrap_err(),
        RulePackSetError::TooManyContributions {
            found: MAX_RULE_SET_CONTRIBUTIONS + 1
        }
    );
}

#[test]
fn dependency_resolution_rejects_missing_dependency() {
    let set = RulePackSet::new(vec![pack(
        "consumer",
        (1, 0, 0),
        vec![dependency("missing", 1, 0, 0)],
    )])
    .unwrap();

    assert_eq!(
        set.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::MissingDependency {
            pack_id: pack_id("consumer"),
            dependency_id: pack_id("missing"),
        }
    );
}

#[test]
fn dependency_resolution_rejects_incompatible_dependency_version() {
    let set = RulePackSet::new(vec![
        pack("consumer", (1, 0, 0), vec![dependency("provider", 1, 2, 4)]),
        pack("provider", (1, 2, 3), Vec::new()),
    ])
    .unwrap();

    assert_eq!(
        set.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::IncompatibleDependencyVersion {
            pack_id: pack_id("consumer"),
            dependency_id: pack_id("provider"),
            required: RuleVersionRequirement::new(1, 2, 4).unwrap(),
            found: RuleVersion::new(1, 2, 3).unwrap(),
        }
    );
}

#[test]
fn dependency_resolution_rejects_direct_self_dependency() {
    let set = RulePackSet::new(vec![pack(
        "self",
        (1, 0, 0),
        vec![dependency("self", 1, 0, 0)],
    )])
    .unwrap();

    assert_eq!(
        set.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::SelfDependency {
            pack_id: pack_id("self")
        }
    );
}

#[test]
fn dependency_resolution_rejects_two_node_and_longer_cycles() {
    let two_node = RulePackSet::new(vec![
        pack("a", (1, 0, 0), vec![dependency("b", 1, 0, 0)]),
        pack("b", (1, 0, 0), vec![dependency("a", 1, 0, 0)]),
    ])
    .unwrap();
    assert_eq!(
        two_node
            .resolve_dependencies(WORLD_SPEC_SCHEMA_V1)
            .unwrap_err(),
        RulePackSetError::DependencyCycle {
            pack_id: pack_id("a")
        }
    );

    let longer = RulePackSet::new(vec![
        pack("delta", (1, 0, 0), vec![dependency("echo", 1, 0, 0)]),
        pack("charlie", (1, 0, 0), vec![dependency("delta", 1, 0, 0)]),
        pack("echo", (1, 0, 0), vec![dependency("charlie", 1, 0, 0)]),
    ])
    .unwrap();
    assert_eq!(
        longer
            .resolve_dependencies(WORLD_SPEC_SCHEMA_V1)
            .unwrap_err(),
        RulePackSetError::DependencyCycle {
            pack_id: pack_id("charlie")
        }
    );
}

#[test]
fn dependency_cycle_error_names_actual_stable_minimum_cycle_member() {
    let set = RulePackSet::new(vec![
        pack(
            "blocked-alpha",
            (1, 0, 0),
            vec![dependency("cycle-zeta", 1, 0, 0)],
        ),
        pack(
            "cycle-zeta",
            (1, 0, 0),
            vec![dependency("cycle-mu", 1, 0, 0)],
        ),
        pack(
            "cycle-mu",
            (1, 0, 0),
            vec![dependency("cycle-zeta", 1, 0, 0)],
        ),
    ])
    .unwrap();

    assert_eq!(
        set.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::DependencyCycle {
            pack_id: pack_id("cycle-mu")
        }
    );
}

#[test]
fn dependency_resolution_rejects_incompatible_core_schema() {
    let set = RulePackSet::new(vec![pack_with(
        "future",
        (1, 0, 0),
        CoreSchemaRange::new(2, 3).unwrap(),
        Vec::new(),
        Vec::new(),
    )])
    .unwrap();

    assert_eq!(
        set.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::IncompatibleCoreSchema {
            pack_id: pack_id("future"),
            supported: CoreSchemaRange::new(2, 3).unwrap(),
            found: WORLD_SPEC_SCHEMA_V1,
        }
    );
}

#[test]
fn dependency_resolution_sorts_independent_packs_by_id() {
    let set = RulePackSet::new(vec![
        pack("zeta", (1, 0, 0), Vec::new()),
        pack("alpha", (1, 0, 0), Vec::new()),
        pack("mu", (1, 0, 0), Vec::new()),
    ])
    .unwrap();
    let resolved = set.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap();

    assert_eq!(
        pack_names(resolved.packs()),
        vec!["sekai.test.alpha", "sekai.test.mu", "sekai.test.zeta"]
    );
}

#[test]
fn dependency_resolution_always_places_dependency_before_consumer() {
    let set = RulePackSet::new(vec![
        pack(
            "alpha-consumer",
            (1, 0, 0),
            vec![dependency("zeta-provider", 1, 0, 0)],
        ),
        pack("zeta-provider", (1, 0, 0), Vec::new()),
        pack("beta-independent", (1, 0, 0), Vec::new()),
    ])
    .unwrap();
    let resolved = set.resolve_dependencies(WORLD_SPEC_SCHEMA_V1).unwrap();

    assert_eq!(
        pack_names(resolved.packs()),
        vec![
            "sekai.test.beta-independent",
            "sekai.test.zeta-provider",
            "sekai.test.alpha-consumer",
        ]
    );
}

#[test]
fn dependency_reverse_input_has_identical_set_bytes_and_resolved_order() {
    let alpha = pack("alpha", (1, 0, 0), Vec::new());
    let beta = pack("beta", (1, 0, 0), vec![dependency("alpha", 1, 0, 0)]);
    let forward = RulePackSet::new(vec![alpha.clone(), beta.clone()]).unwrap();
    let reverse = RulePackSet::new(vec![beta, alpha]).unwrap();

    assert_eq!(
        serde_json::to_vec(&forward).unwrap(),
        serde_json::to_vec(&reverse).unwrap()
    );
    assert_eq!(
        pack_names(
            forward
                .resolve_dependencies(WORLD_SPEC_SCHEMA_V1)
                .unwrap()
                .packs()
        ),
        pack_names(
            reverse
                .resolve_dependencies(WORLD_SPEC_SCHEMA_V1)
                .unwrap()
                .packs()
        )
    );
}

#[test]
fn dependency_set_deserialization_revalidates_private_invariants() {
    let original = RulePackSet::new(vec![pack("alpha", (1, 0, 0), Vec::new())]).unwrap();
    let encoded = serde_json::to_vec(&original).unwrap();
    assert_eq!(
        serde_json::from_slice::<RulePackSet>(&encoded).unwrap(),
        original
    );

    let mut duplicate = serde_json::to_value(&original).unwrap();
    let repeated = duplicate["packs"][0].clone();
    duplicate["packs"].as_array_mut().unwrap().push(repeated);
    assert!(serde_json::from_value::<RulePackSet>(duplicate).is_err());

    let mut tampered = serde_json::to_value(&original).unwrap();
    tampered["packs"][0]["manifest"]["version"]["patch"] = serde_json::json!(9);
    assert!(serde_json::from_value::<RulePackSet>(tampered).is_err());
}

#[test]
fn capability_resolution_rejects_unknown_provided_capability() {
    let registry = capability_registry([controls_descriptor()]);
    let set = RulePackSet::new(vec![typed_pack(
        "world-law",
        RulePackKind::WorldLaw,
        Vec::new(),
        Vec::new(),
        vec![model_contribution()],
    )])
    .unwrap();

    assert_eq!(
        set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::UnknownProvidedCapability {
            pack_id: pack_id("world-law"),
            capability_id: tectonic_model_capability_id(),
        }
    );
}

#[test]
fn capability_resolution_rejects_unknown_consumed_capability() {
    let unknown = CapabilityId::new("sekai.test", "unknown", 1).unwrap();
    let registry = capability_registry([controls_descriptor()]);
    let set = RulePackSet::new(vec![typed_pack(
        "consumer",
        RulePackKind::Ordinary,
        Vec::new(),
        vec![unknown.clone()],
        Vec::new(),
    )])
    .unwrap();

    assert_eq!(
        set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::UnknownConsumedCapability {
            pack_id: pack_id("consumer"),
            capability_id: unknown,
        }
    );
}

#[test]
fn capability_resolution_rejects_ordinary_provider_of_world_law_capability() {
    let registry = capability_registry([model_descriptor()]);
    let set = RulePackSet::new(vec![typed_pack(
        "ordinary",
        RulePackKind::Ordinary,
        Vec::new(),
        Vec::new(),
        vec![model_contribution()],
    )])
    .unwrap();

    assert_eq!(
        set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::InsufficientCapabilityPermission {
            pack_id: pack_id("ordinary"),
            capability_id: tectonic_model_capability_id(),
            found: RulePackKind::Ordinary,
            required: RulePackKind::WorldLaw,
        }
    );
}

#[test]
fn capability_resolution_allows_world_law_provider_of_ordinary_capability() {
    let registry = capability_registry([controls_descriptor()]);
    let set = RulePackSet::new(vec![typed_pack(
        "world-law-controls",
        RulePackKind::WorldLaw,
        Vec::new(),
        Vec::new(),
        vec![plate_constraint(1)],
    )])
    .unwrap();
    let resolved = set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap();

    assert_eq!(
        pack_names(
            resolved
                .providers(&tectonic_controls_capability_id())
                .iter()
                .copied()
        ),
        vec!["sekai.test.world-law-controls"]
    );
}

#[test]
fn capability_resolution_rejects_missing_consumed_capability() {
    let registry = capability_registry([controls_descriptor()]);
    let set = RulePackSet::new(vec![typed_pack(
        "consumer",
        RulePackKind::Ordinary,
        Vec::new(),
        vec![tectonic_controls_capability_id()],
        Vec::new(),
    )])
    .unwrap();

    assert_eq!(
        set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::MissingConsumedCapability {
            pack_id: pack_id("consumer"),
            capability_id: tectonic_controls_capability_id(),
        }
    );
}

#[test]
fn capability_resolution_rejects_missing_required_unique_capability() {
    let registry = capability_registry([model_descriptor()]);
    let set = RulePackSet::new(Vec::new()).unwrap();

    assert_eq!(
        set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::MissingRequiredCapability {
            capability_id: tectonic_model_capability_id(),
        }
    );
}

#[test]
fn capability_resolution_rejects_multiple_unique_providers_stably() {
    let registry = capability_registry([model_descriptor()]);
    let set = RulePackSet::new(vec![
        typed_pack(
            "zeta-law",
            RulePackKind::WorldLaw,
            Vec::new(),
            Vec::new(),
            vec![model_contribution()],
        ),
        typed_pack(
            "alpha-law",
            RulePackKind::WorldLaw,
            Vec::new(),
            Vec::new(),
            vec![model_contribution()],
        ),
    ])
    .unwrap();

    assert_eq!(
        set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap_err(),
        RulePackSetError::MultipleCapabilityProviders {
            capability_id: tectonic_model_capability_id(),
            provider_ids: vec![pack_id("alpha-law"), pack_id("zeta-law")],
        }
    );
}

#[test]
fn capability_resolution_merges_and_indexes_providers_by_pack_id() {
    let registry = capability_registry([controls_descriptor()]);
    let set = RulePackSet::new(vec![
        typed_pack(
            "alpha-controls",
            RulePackKind::Ordinary,
            vec![dependency("zeta-controls", 1, 0, 0)],
            Vec::new(),
            vec![plate_constraint(1)],
        ),
        typed_pack(
            "zeta-controls",
            RulePackKind::Ordinary,
            Vec::new(),
            Vec::new(),
            vec![plate_constraint(2)],
        ),
    ])
    .unwrap();
    let resolved = set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap();

    assert_eq!(
        pack_names(resolved.packs()),
        vec!["sekai.test.zeta-controls", "sekai.test.alpha-controls"]
    );
    assert_eq!(
        pack_names(
            resolved
                .providers(&tectonic_controls_capability_id())
                .iter()
                .copied()
        ),
        vec!["sekai.test.alpha-controls", "sekai.test.zeta-controls"]
    );
    assert!(resolved
        .providers(&tectonic_model_capability_id())
        .is_empty());
}

#[test]
fn capability_payload_cannot_masquerade_as_another_capability() {
    let pack = typed_pack(
        "controls",
        RulePackKind::Ordinary,
        Vec::new(),
        Vec::new(),
        vec![plate_constraint(1)],
    );
    let mut tampered = serde_json::to_value(pack).unwrap();
    tampered["manifest"]["provides"] = serde_json::json!([tectonic_model_capability_id()]);

    assert!(serde_json::from_value::<RulePack>(tampered).is_err());
}

#[test]
fn capability_registry_input_order_does_not_change_resolution() {
    let forward = capability_registry([model_descriptor(), controls_descriptor()]);
    let reverse = capability_registry([controls_descriptor(), model_descriptor()]);
    let set = RulePackSet::new(vec![
        typed_pack(
            "world-law",
            RulePackKind::WorldLaw,
            Vec::new(),
            Vec::new(),
            vec![model_contribution()],
        ),
        typed_pack(
            "controls",
            RulePackKind::Ordinary,
            Vec::new(),
            Vec::new(),
            vec![plate_constraint(1)],
        ),
    ])
    .unwrap();

    assert_eq!(
        set.resolve(&forward, WORLD_SPEC_SCHEMA_V1).unwrap(),
        set.resolve(&reverse, WORLD_SPEC_SCHEMA_V1).unwrap()
    );
}
