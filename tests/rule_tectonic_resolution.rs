use sekai::rules::{
    tectonic_controls_capability_id, tectonic_model_capability_id, AuthorConstraint,
    AuthorConstraints, CapabilityCardinality, CapabilityContribution, CapabilityDescriptor,
    CapabilityRegistry, CapabilityRegistryBuilder, ConstraintAdoptionOutcome, ConstraintSource,
    ConstraintStrength, CoreSchemaRange, RuleItemId, RulePack, RulePackDependency, RulePackId,
    RulePackKind, RulePackSet, RuleTectonicConstraint, RuleVersion, RuleVersionRequirement,
    TectonicConstraintClause, TectonicControl, TectonicModel, TectonicRuleResolutionError,
    TectonicRuleResolver, AUTHOR_CONSTRAINTS_SCHEMA_V1, MAX_AUTHOR_CONSTRAINTS,
    MAX_RULE_PACK_CONTRIBUTIONS,
};
use sekai::world::natural::{
    NaturalSpecError, TectonicActivity, TectonicSpec, TECTONIC_SPEC_SCHEMA_V1,
};
use sekai::world::{AuthorObjectId, WORLD_SPEC_SCHEMA_V1};

fn pack_id(name: &str) -> RulePackId {
    RulePackId::new(format!("sekai.test.{name}")).unwrap()
}

fn capability_registry() -> CapabilityRegistry {
    let mut builder = CapabilityRegistryBuilder::new();
    builder
        .register(CapabilityDescriptor::new(
            tectonic_model_capability_id(),
            CapabilityCardinality::UniqueRequired,
            RulePackKind::WorldLaw,
            false,
        ))
        .unwrap();
    builder
        .register(CapabilityDescriptor::new(
            tectonic_controls_capability_id(),
            CapabilityCardinality::Merge,
            RulePackKind::Ordinary,
            true,
        ))
        .unwrap();
    builder.build()
}

fn rule_constraint(
    item: &str,
    strength: ConstraintStrength,
    clause: TectonicConstraintClause,
) -> CapabilityContribution {
    CapabilityContribution::TectonicConstraint(
        RuleTectonicConstraint::new(RuleItemId::new(item).unwrap(), strength, clause).unwrap(),
    )
}

fn world_law(constraints: Vec<CapabilityContribution>) -> RulePack {
    let mut contributions = vec![CapabilityContribution::TectonicModel(
        TectonicModel::CurrentSliceV1,
    )];
    contributions.extend(constraints);
    pack(
        "world-law",
        RulePackKind::WorldLaw,
        Vec::new(),
        contributions,
    )
}

fn pack(
    name: &str,
    kind: RulePackKind,
    dependencies: Vec<RulePackDependency>,
    contributions: Vec<CapabilityContribution>,
) -> RulePack {
    RulePack::new(
        pack_id(name),
        RuleVersion::new(1, 0, 0).unwrap(),
        kind,
        CoreSchemaRange::new(WORLD_SPEC_SCHEMA_V1, WORLD_SPEC_SCHEMA_V1).unwrap(),
        dependencies,
        Vec::new(),
        contributions,
    )
    .unwrap()
}

fn author_constraint(
    id: u64,
    strength: ConstraintStrength,
    clause: TectonicConstraintClause,
) -> AuthorConstraint {
    AuthorConstraint::new(AuthorObjectId::from_raw(id), strength, clause).unwrap()
}

fn authors(constraints: Vec<AuthorConstraint>) -> AuthorConstraints {
    AuthorConstraints::new(AUTHOR_CONSTRAINTS_SCHEMA_V1, constraints).unwrap()
}

fn resolve(
    base: &TectonicSpec,
    packs: Vec<RulePack>,
    author_constraints: &AuthorConstraints,
) -> Result<sekai::rules::TectonicRuleResolution, TectonicRuleResolutionError> {
    let set = RulePackSet::new(packs).unwrap();
    let registry = capability_registry();
    let resolved = set.resolve(&registry, WORLD_SPEC_SCHEMA_V1).unwrap();
    TectonicRuleResolver::resolve(base, &resolved, author_constraints)
}

#[test]
fn hard_no_controls_preserve_all_base_spec_values_bit_exactly() {
    let arbitrary_fraction = f32::from_bits(0x3e_c2_31_5a);
    let base = TectonicSpec {
        schema_version: TECTONIC_SPEC_SCHEMA_V1,
        plate_count: 37,
        continental_crust_fraction: arbitrary_fraction,
        activity: TectonicActivity::Active,
    };

    let resolution = resolve(&base, vec![world_law(Vec::new())], &authors(Vec::new())).unwrap();

    assert_eq!(resolution.model(), TectonicModel::CurrentSliceV1);
    assert_eq!(resolution.spec().schema_version, base.schema_version);
    assert_eq!(resolution.spec().plate_count, base.plate_count);
    assert_eq!(
        resolution.spec().continental_crust_fraction.to_bits(),
        arbitrary_fraction.to_bits()
    );
    assert_eq!(resolution.spec().activity, base.activity);
    assert!(resolution.adoptions().is_empty());
}

#[test]
fn hard_plate_range_narrows_to_nearest_feasible_value() {
    let base = TectonicSpec::default();
    let hard = rule_constraint(
        "at-least-twenty",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(20, 24).unwrap(),
    );

    let resolution = resolve(&base, vec![world_law(vec![hard])], &authors(Vec::new())).unwrap();

    assert_eq!(resolution.spec().plate_count, 20);
}

#[test]
fn hard_overlapping_ranges_intersect() {
    let base = TectonicSpec {
        plate_count: 30,
        ..TectonicSpec::default()
    };
    let first = rule_constraint(
        "first",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(8, 20).unwrap(),
    );
    let second = rule_constraint(
        "second",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(14, 24).unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![world_law(vec![second, first])],
        &authors(Vec::new()),
    )
    .unwrap();

    assert_eq!(resolution.spec().plate_count, 20);
}

#[test]
fn hard_disjoint_ranges_fail_with_every_sorted_hard_source_on_target() {
    let base = TectonicSpec::default();
    let alpha = rule_constraint(
        "alpha",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(2, 4).unwrap(),
    );
    let zeta = rule_constraint(
        "zeta",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(20, 24).unwrap(),
    );
    let irrelevant_soft = rule_constraint(
        "irrelevant-soft",
        ConstraintStrength::soft(10).unwrap(),
        TectonicConstraintClause::plate_count(10, 12).unwrap(),
    );

    let error = resolve(
        &base,
        vec![world_law(vec![zeta, irrelevant_soft, alpha])],
        &authors(Vec::new()),
    )
    .unwrap_err();

    assert_eq!(
        error,
        TectonicRuleResolutionError::HardConstraintConflict {
            target: TectonicControl::PlateCount,
            sources: vec![
                ConstraintSource::RulePack {
                    pack_id: pack_id("world-law"),
                    item_id: RuleItemId::new("alpha").unwrap(),
                },
                ConstraintSource::RulePack {
                    pack_id: pack_id("world-law"),
                    item_id: RuleItemId::new("zeta").unwrap(),
                },
            ],
        }
    );
}

#[test]
fn hard_activity_sets_intersect() {
    let base = TectonicSpec {
        activity: TectonicActivity::Quiet,
        ..TectonicSpec::default()
    };
    let first = rule_constraint(
        "first",
        ConstraintStrength::Hard,
        TectonicConstraintClause::activity([TectonicActivity::Quiet, TectonicActivity::Moderate])
            .unwrap(),
    );
    let second = rule_constraint(
        "second",
        ConstraintStrength::Hard,
        TectonicConstraintClause::activity([TectonicActivity::Moderate, TectonicActivity::Active])
            .unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![world_law(vec![first, second])],
        &authors(Vec::new()),
    )
    .unwrap();

    assert_eq!(resolution.spec().activity, TectonicActivity::Moderate);
}

#[test]
fn hard_rule_and_author_constraints_share_one_solver() {
    let base = TectonicSpec {
        plate_count: 8,
        ..TectonicSpec::default()
    };
    let rule = rule_constraint(
        "rule-range",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(10, 20).unwrap(),
    );
    let author = author_constraint(
        42,
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(16, 24).unwrap(),
    );

    let resolution = resolve(&base, vec![world_law(vec![rule])], &authors(vec![author])).unwrap();

    assert_eq!(resolution.spec().plate_count, 16);
}

#[test]
fn hard_invalid_base_spec_fails_before_rule_resolution() {
    let invalid = TectonicSpec {
        plate_count: 0,
        ..TectonicSpec::default()
    };
    let empty_set = RulePackSet::new(Vec::new()).unwrap();
    let dependency_only = empty_set
        .resolve_dependencies(WORLD_SPEC_SCHEMA_V1)
        .unwrap();

    let error = TectonicRuleResolver::resolve(&invalid, &dependency_only, &authors(Vec::new()))
        .unwrap_err();

    assert_eq!(
        error,
        TectonicRuleResolutionError::InvalidBaseSpec(NaturalSpecError::PlateCountOutOfRange {
            found: 0,
            min: 2,
            max: 64,
        })
    );
}

#[test]
fn preference_soft_constraints_choose_minimum_weighted_penalty() {
    let base = TectonicSpec::default();
    let strong = rule_constraint(
        "strong-twenty",
        ConstraintStrength::soft(2).unwrap(),
        TectonicConstraintClause::plate_count(20, 20).unwrap(),
    );
    let weak = rule_constraint(
        "weak-ten",
        ConstraintStrength::soft(1).unwrap(),
        TectonicConstraintClause::plate_count(10, 10).unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![world_law(vec![weak, strong])],
        &authors(Vec::new()),
    )
    .unwrap();

    assert_eq!(resolution.spec().plate_count, 20);
}

#[test]
fn preference_hint_cannot_defeat_any_soft_preference() {
    let base = TectonicSpec::default();
    let soft = rule_constraint(
        "soft-ten",
        ConstraintStrength::soft(1).unwrap(),
        TectonicConstraintClause::plate_count(10, 10).unwrap(),
    );
    let hint = rule_constraint(
        "hint-twenty",
        ConstraintStrength::hint(1000).unwrap(),
        TectonicConstraintClause::plate_count(20, 20).unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![world_law(vec![hint, soft])],
        &authors(Vec::new()),
    )
    .unwrap();

    assert_eq!(resolution.spec().plate_count, 10);
}

#[test]
fn preference_hint_breaks_a_soft_score_tie() {
    let base = TectonicSpec::default();
    let soft_low = rule_constraint(
        "soft-low",
        ConstraintStrength::soft(1).unwrap(),
        TectonicConstraintClause::plate_count(10, 10).unwrap(),
    );
    let soft_high = rule_constraint(
        "soft-high",
        ConstraintStrength::soft(1).unwrap(),
        TectonicConstraintClause::plate_count(20, 20).unwrap(),
    );
    let hint = rule_constraint(
        "hint-eighteen",
        ConstraintStrength::hint(1).unwrap(),
        TectonicConstraintClause::plate_count(18, 18).unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![world_law(vec![hint, soft_high, soft_low])],
        &authors(Vec::new()),
    )
    .unwrap();

    assert_eq!(resolution.spec().plate_count, 18);
}

#[test]
fn preference_base_value_breaks_a_soft_and_hint_tie() {
    let base = TectonicSpec {
        plate_count: 17,
        ..TectonicSpec::default()
    };
    let soft_low = rule_constraint(
        "soft-low",
        ConstraintStrength::soft(1).unwrap(),
        TectonicConstraintClause::plate_count(10, 10).unwrap(),
    );
    let soft_high = rule_constraint(
        "soft-high",
        ConstraintStrength::soft(1).unwrap(),
        TectonicConstraintClause::plate_count(20, 20).unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![world_law(vec![soft_high, soft_low])],
        &authors(Vec::new()),
    )
    .unwrap();

    assert_eq!(resolution.spec().plate_count, 17);
}

#[test]
fn preference_stable_candidate_order_breaks_a_complete_tie() {
    let base = TectonicSpec {
        activity: TectonicActivity::Moderate,
        ..TectonicSpec::default()
    };
    let allowed_extremes = rule_constraint(
        "extremes",
        ConstraintStrength::Hard,
        TectonicConstraintClause::activity([TectonicActivity::Active, TectonicActivity::Quiet])
            .unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![world_law(vec![allowed_extremes])],
        &authors(Vec::new()),
    )
    .unwrap();

    assert_eq!(resolution.spec().activity, TectonicActivity::Quiet);
}

#[test]
fn preference_scores_stay_bounded_at_maximum_constraint_counts() {
    let mut packs = Vec::new();
    for pack_index in 0..16 {
        let constraint_count = if pack_index == 0 {
            MAX_RULE_PACK_CONTRIBUTIONS - 1
        } else {
            MAX_RULE_PACK_CONTRIBUTIONS
        };
        let mut contributions: Vec<_> = (0..constraint_count)
            .map(|item_index| {
                rule_constraint(
                    &format!("prefer-active-{item_index:03}"),
                    ConstraintStrength::soft(1000).unwrap(),
                    TectonicConstraintClause::activity([TectonicActivity::Active]).unwrap(),
                )
            })
            .collect();
        if pack_index == 0 {
            contributions.push(CapabilityContribution::TectonicModel(
                TectonicModel::CurrentSliceV1,
            ));
        }
        packs.push(pack(
            &format!("max-{pack_index:02}"),
            if pack_index == 0 {
                RulePackKind::WorldLaw
            } else {
                RulePackKind::Ordinary
            },
            Vec::new(),
            contributions,
        ));
    }
    let author_constraints = authors(
        (0..MAX_AUTHOR_CONSTRAINTS)
            .map(|index| {
                author_constraint(
                    index as u64,
                    ConstraintStrength::soft(1000).unwrap(),
                    TectonicConstraintClause::activity([TectonicActivity::Active]).unwrap(),
                )
            })
            .collect(),
    );
    let base = TectonicSpec {
        activity: TectonicActivity::Quiet,
        ..TectonicSpec::default()
    };

    let resolution = resolve(&base, packs, &author_constraints).unwrap();

    assert_eq!(resolution.spec().activity, TectonicActivity::Active);
    assert_eq!(
        resolution.adoptions().len(),
        MAX_RULE_PACK_CONTRIBUTIONS * 16 - 1 + MAX_AUTHOR_CONSTRAINTS
    );
}

#[test]
fn preference_fraction_is_quantized_only_when_that_target_is_constrained() {
    let base = TectonicSpec {
        continental_crust_fraction: 0.3814,
        ..TectonicSpec::default()
    };
    let plate_only = rule_constraint(
        "plate-only",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(8, 16).unwrap(),
    );
    let plate_resolution = resolve(
        &base,
        vec![world_law(vec![plate_only])],
        &authors(Vec::new()),
    )
    .unwrap();
    assert_eq!(
        plate_resolution.spec().continental_crust_fraction.to_bits(),
        base.continental_crust_fraction.to_bits()
    );

    let fraction = rule_constraint(
        "fraction",
        ConstraintStrength::Hard,
        TectonicConstraintClause::continental_crust_permille(100, 750).unwrap(),
    );
    let fraction_resolution =
        resolve(&base, vec![world_law(vec![fraction])], &authors(Vec::new())).unwrap();
    assert_eq!(
        fraction_resolution
            .spec()
            .continental_crust_fraction
            .to_bits(),
        (381.0_f32 / 1000.0).to_bits()
    );
}

#[test]
fn preference_input_constraint_order_does_not_change_resolution() {
    let base = TectonicSpec::default();
    let model = world_law(Vec::new());
    let alpha = pack(
        "alpha-controls",
        RulePackKind::Ordinary,
        Vec::new(),
        vec![rule_constraint(
            "prefer-ten",
            ConstraintStrength::soft(2).unwrap(),
            TectonicConstraintClause::plate_count(10, 10).unwrap(),
        )],
    );
    let zeta = pack(
        "zeta-controls",
        RulePackKind::Ordinary,
        Vec::new(),
        vec![rule_constraint(
            "prefer-twenty",
            ConstraintStrength::soft(3).unwrap(),
            TectonicConstraintClause::plate_count(20, 20).unwrap(),
        )],
    );
    let first_author = author_constraint(
        1,
        ConstraintStrength::hint(3).unwrap(),
        TectonicConstraintClause::plate_count(16, 16).unwrap(),
    );
    let second_author = author_constraint(
        2,
        ConstraintStrength::hint(4).unwrap(),
        TectonicConstraintClause::plate_count(18, 18).unwrap(),
    );

    let forward = resolve(
        &base,
        vec![model.clone(), alpha.clone(), zeta.clone()],
        &authors(vec![first_author.clone(), second_author.clone()]),
    )
    .unwrap();
    let reverse = resolve(
        &base,
        vec![zeta, alpha, model],
        &authors(vec![second_author, first_author]),
    )
    .unwrap();

    assert_eq!(forward, reverse);
}

#[test]
fn adoption_hard_records_are_always_satisfied() {
    let base = TectonicSpec::default();
    let rule = rule_constraint(
        "hard-plates",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(18, 20).unwrap(),
    );
    let author = author_constraint(
        7,
        ConstraintStrength::Hard,
        TectonicConstraintClause::activity([TectonicActivity::Active]).unwrap(),
    );

    let resolution = resolve(&base, vec![world_law(vec![rule])], &authors(vec![author])).unwrap();

    assert_eq!(resolution.adoptions().len(), 2);
    assert!(resolution.adoptions().iter().all(|adoption| {
        adoption.strength() == ConstraintStrength::Hard
            && adoption.outcome() == ConstraintAdoptionOutcome::Satisfied
    }));
}

#[test]
fn adoption_soft_and_hint_records_distinguish_satisfied_and_compromised() {
    let base = TectonicSpec::default();
    let strong = rule_constraint(
        "strong-ten",
        ConstraintStrength::soft(2).unwrap(),
        TectonicConstraintClause::plate_count(10, 10).unwrap(),
    );
    let weak = rule_constraint(
        "weak-twenty",
        ConstraintStrength::soft(1).unwrap(),
        TectonicConstraintClause::plate_count(20, 20).unwrap(),
    );
    let hint = rule_constraint(
        "hint-thirty",
        ConstraintStrength::hint(1000).unwrap(),
        TectonicConstraintClause::plate_count(30, 30).unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![world_law(vec![weak, hint, strong])],
        &authors(Vec::new()),
    )
    .unwrap();

    assert_eq!(resolution.spec().plate_count, 10);
    let outcomes: Vec<_> = resolution
        .adoptions()
        .iter()
        .map(|adoption| {
            let ConstraintSource::RulePack { item_id, .. } = adoption.source() else {
                panic!("test uses only rule-pack constraints");
            };
            (item_id.as_str(), adoption.outcome())
        })
        .collect();
    assert_eq!(
        outcomes,
        vec![
            ("hint-thirty", ConstraintAdoptionOutcome::Compromised),
            ("strong-ten", ConstraintAdoptionOutcome::Satisfied),
            ("weak-twenty", ConstraintAdoptionOutcome::Compromised),
        ]
    );
}

#[test]
fn adoption_order_is_stable_by_source_then_target() {
    let base = TectonicSpec::default();
    let law = world_law(vec![rule_constraint(
        "zeta-law-item",
        ConstraintStrength::hint(1).unwrap(),
        TectonicConstraintClause::plate_count(12, 12).unwrap(),
    )]);
    let alpha = pack(
        "alpha-controls",
        RulePackKind::Ordinary,
        Vec::new(),
        vec![rule_constraint(
            "alpha-item",
            ConstraintStrength::hint(1).unwrap(),
            TectonicConstraintClause::activity([TectonicActivity::Moderate]).unwrap(),
        )],
    );
    let first_author = author_constraint(
        1,
        ConstraintStrength::hint(1).unwrap(),
        TectonicConstraintClause::continental_crust_permille(380, 380).unwrap(),
    );
    let second_author = author_constraint(
        2,
        ConstraintStrength::hint(1).unwrap(),
        TectonicConstraintClause::plate_count(12, 12).unwrap(),
    );

    let resolution = resolve(
        &base,
        vec![law, alpha],
        &authors(vec![second_author, first_author]),
    )
    .unwrap();
    let sources: Vec<_> = resolution
        .adoptions()
        .iter()
        .map(|adoption| adoption.source().clone())
        .collect();

    assert_eq!(
        sources,
        vec![
            ConstraintSource::RulePack {
                pack_id: pack_id("alpha-controls"),
                item_id: RuleItemId::new("alpha-item").unwrap(),
            },
            ConstraintSource::RulePack {
                pack_id: pack_id("world-law"),
                item_id: RuleItemId::new("zeta-law-item").unwrap(),
            },
            ConstraintSource::Author(AuthorObjectId::from_raw(1)),
            ConstraintSource::Author(AuthorObjectId::from_raw(2)),
        ]
    );
}

#[test]
fn audit_resolved_pack_references_keep_dependency_order_version_and_hash() {
    let law = world_law(Vec::new());
    let controls = pack(
        "alpha-controls",
        RulePackKind::Ordinary,
        vec![RulePackDependency::new(
            pack_id("world-law"),
            RuleVersionRequirement::new(1, 0, 0).unwrap(),
        )],
        vec![rule_constraint(
            "control",
            ConstraintStrength::hint(1).unwrap(),
            TectonicConstraintClause::plate_count(12, 12).unwrap(),
        )],
    );
    let expected_law_hash = law.manifest().content_hash();
    let expected_controls_hash = controls.manifest().content_hash();

    let resolution = resolve(
        &TectonicSpec::default(),
        vec![controls, law],
        &authors(Vec::new()),
    )
    .unwrap();
    let refs = resolution.resolved_packs();

    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].pack_id(), &pack_id("world-law"));
    assert_eq!(refs[0].version(), RuleVersion::new(1, 0, 0).unwrap());
    assert_eq!(refs[0].content_hash(), expected_law_hash);
    assert_eq!(refs[1].pack_id(), &pack_id("alpha-controls"));
    assert_eq!(refs[1].version(), RuleVersion::new(1, 0, 0).unwrap());
    assert_eq!(refs[1].content_hash(), expected_controls_hash);
}

#[test]
fn audit_json_round_trips_and_revalidates() {
    let hard = rule_constraint(
        "hard",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(14, 18).unwrap(),
    );
    let hint = author_constraint(
        9,
        ConstraintStrength::hint(3).unwrap(),
        TectonicConstraintClause::activity([TectonicActivity::Active]).unwrap(),
    );
    let resolution = resolve(
        &TectonicSpec::default(),
        vec![world_law(vec![hard])],
        &authors(vec![hint]),
    )
    .unwrap();

    let encoded = serde_json::to_vec(&resolution).unwrap();
    let decoded: sekai::rules::TectonicRuleResolution = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(decoded, resolution);
    decoded.validate().unwrap();
}

#[test]
fn audit_deserialization_rejects_invalid_private_state() {
    let hard = rule_constraint(
        "hard",
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(14, 18).unwrap(),
    );
    let author = author_constraint(
        9,
        ConstraintStrength::hint(3).unwrap(),
        TectonicConstraintClause::activity([TectonicActivity::Active]).unwrap(),
    );
    let resolution = resolve(
        &TectonicSpec::default(),
        vec![world_law(vec![hard])],
        &authors(vec![author]),
    )
    .unwrap();

    let mut schema = serde_json::to_value(&resolution).unwrap();
    schema["schema_version"] = serde_json::json!(9);
    assert!(serde_json::from_value::<sekai::rules::TectonicRuleResolution>(schema).is_err());

    let mut invalid_spec = serde_json::to_value(&resolution).unwrap();
    invalid_spec["spec"]["plate_count"] = serde_json::json!(0);
    assert!(serde_json::from_value::<sekai::rules::TectonicRuleResolution>(invalid_spec).is_err());

    let mut duplicate_pack = serde_json::to_value(&resolution).unwrap();
    let repeated = duplicate_pack["resolved_packs"][0].clone();
    duplicate_pack["resolved_packs"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    assert!(
        serde_json::from_value::<sekai::rules::TectonicRuleResolution>(duplicate_pack).is_err()
    );

    let mut compromised_hard = serde_json::to_value(&resolution).unwrap();
    let hard_index = compromised_hard["adoptions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|adoption| adoption["strength"] == serde_json::json!("Hard"))
        .unwrap();
    compromised_hard["adoptions"][hard_index]["outcome"] = serde_json::json!("Compromised");
    assert!(
        serde_json::from_value::<sekai::rules::TectonicRuleResolution>(compromised_hard).is_err()
    );

    let mut noncanonical = serde_json::to_value(&resolution).unwrap();
    noncanonical["adoptions"].as_array_mut().unwrap().reverse();
    assert!(serde_json::from_value::<sekai::rules::TectonicRuleResolution>(noncanonical).is_err());

    let mut unknown_pack = serde_json::to_value(&resolution).unwrap();
    let rule_index = unknown_pack["adoptions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|adoption| adoption["source"].get("RulePack").is_some())
        .unwrap();
    unknown_pack["adoptions"][rule_index]["source"]["RulePack"]["pack_id"] =
        serde_json::json!("sekai.test.absent");
    assert!(serde_json::from_value::<sekai::rules::TectonicRuleResolution>(unknown_pack).is_err());
}
