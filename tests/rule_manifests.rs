use sekai::rules::{
    tectonic_controls_capability_id, CapabilityContribution, ConstraintStrength, CoreSchemaRange,
    GeologicModel, RuleItemId, RulePack, RulePackDependency, RulePackError, RulePackId,
    RulePackKind, RuleTectonicConstraint, RuleVersion, RuleVersionRequirement,
    TectonicConstraintClause, TectonicModel, MAX_RULE_PACK_CAPABILITY_REQUIREMENTS,
    MAX_RULE_PACK_CONTRIBUTIONS, MAX_RULE_PACK_DEPENDENCIES, RULE_PACK_SCHEMA_V1,
};

fn dependency(index: usize) -> RulePackDependency {
    RulePackDependency::new(
        RulePackId::new(format!("sekai.test.dependency-{index}")).unwrap(),
        RuleVersionRequirement::new(1, 0, 0).unwrap(),
    )
}

fn plate_constraint(item: &str, plates: u16) -> CapabilityContribution {
    CapabilityContribution::TectonicConstraint(
        RuleTectonicConstraint::new(
            RuleItemId::new(item).unwrap(),
            ConstraintStrength::soft(10).unwrap(),
            TectonicConstraintClause::plate_count(plates, plates).unwrap(),
        )
        .unwrap(),
    )
}

fn pack(
    dependencies: Vec<RulePackDependency>,
    consumes: Vec<sekai::rules::CapabilityId>,
    contributions: Vec<CapabilityContribution>,
) -> Result<RulePack, RulePackError> {
    RulePack::new(
        RulePackId::new("sekai.test.pack").unwrap(),
        RuleVersion::new(1, 2, 3).unwrap(),
        RulePackKind::Ordinary,
        CoreSchemaRange::new(1, 2).unwrap(),
        dependencies,
        consumes,
        contributions,
    )
}

#[test]
fn manifest_normalizes_dependencies_consumes_contributions_and_provides() {
    let first_dependency = dependency(1);
    let second_dependency = dependency(2);
    let first_constraint = plate_constraint("prefer-eight", 8);
    let second_constraint = plate_constraint("prefer-twelve", 12);
    let rule_pack = pack(
        vec![second_dependency.clone(), first_dependency.clone()],
        vec![tectonic_controls_capability_id()],
        vec![second_constraint.clone(), first_constraint.clone()],
    )
    .unwrap();

    assert_eq!(rule_pack.manifest().schema_version(), RULE_PACK_SCHEMA_V1);
    assert_eq!(
        rule_pack.manifest().dependencies(),
        &[first_dependency, second_dependency]
    );
    assert_eq!(
        rule_pack.manifest().consumes(),
        &[tectonic_controls_capability_id()]
    );
    assert_eq!(
        rule_pack.manifest().provides(),
        &[tectonic_controls_capability_id()]
    );
    assert_eq!(
        rule_pack.contributions(),
        &[first_constraint, second_constraint]
    );
    rule_pack.validate().unwrap();
}

#[test]
fn semantically_equal_input_orders_produce_identical_bytes_and_hashes() {
    let forward = pack(
        vec![dependency(1), dependency(2)],
        vec![tectonic_controls_capability_id()],
        vec![
            plate_constraint("prefer-eight", 8),
            plate_constraint("prefer-twelve", 12),
        ],
    )
    .unwrap();
    let reverse = pack(
        vec![dependency(2), dependency(1)],
        vec![tectonic_controls_capability_id()],
        vec![
            plate_constraint("prefer-twelve", 12),
            plate_constraint("prefer-eight", 8),
        ],
    )
    .unwrap();

    assert_eq!(forward, reverse);
    assert_eq!(
        forward.manifest().content_hash(),
        reverse.manifest().content_hash()
    );
    assert_eq!(
        serde_json::to_vec(&forward).unwrap(),
        serde_json::to_vec(&reverse).unwrap()
    );
}

#[test]
fn semantic_changes_change_the_rule_content_hash() {
    let first = pack(
        Vec::new(),
        Vec::new(),
        vec![plate_constraint("preference", 8)],
    )
    .unwrap();
    let second = pack(
        Vec::new(),
        Vec::new(),
        vec![plate_constraint("preference", 9)],
    )
    .unwrap();

    assert_ne!(
        first.manifest().content_hash(),
        second.manifest().content_hash()
    );
}

#[test]
fn manifest_rejects_duplicate_structural_entries() {
    assert!(matches!(
        pack(vec![dependency(1), dependency(1)], Vec::new(), Vec::new()),
        Err(RulePackError::DuplicateDependency { .. })
    ));
    assert!(matches!(
        pack(
            Vec::new(),
            vec![
                tectonic_controls_capability_id(),
                tectonic_controls_capability_id()
            ],
            Vec::new()
        ),
        Err(RulePackError::DuplicateConsumedCapability { .. })
    ));
    assert!(matches!(
        pack(
            Vec::new(),
            Vec::new(),
            vec![
                plate_constraint("duplicate-item", 8),
                plate_constraint("duplicate-item", 9)
            ]
        ),
        Err(RulePackError::DuplicateRuleItem { .. })
    ));
    assert!(matches!(
        pack(
            Vec::new(),
            Vec::new(),
            vec![
                CapabilityContribution::TectonicModel(TectonicModel::CurrentSliceV1),
                CapabilityContribution::TectonicModel(TectonicModel::CurrentSliceV1)
            ]
        ),
        Err(RulePackError::DuplicateUniqueContribution { .. })
    ));
    assert!(matches!(
        pack(
            Vec::new(),
            Vec::new(),
            vec![
                CapabilityContribution::GeologicModel(GeologicModel::CurrentSliceV1),
                CapabilityContribution::GeologicModel(GeologicModel::CurrentSliceV1)
            ]
        ),
        Err(RulePackError::DuplicateUniqueContribution { .. })
    ));
}

#[test]
fn manifest_enforces_per_pack_allocation_budgets() {
    let dependencies = (0..=MAX_RULE_PACK_DEPENDENCIES).map(dependency).collect();
    assert!(matches!(
        pack(dependencies, Vec::new(), Vec::new()),
        Err(RulePackError::TooManyDependencies { .. })
    ));

    let consumes = (0..=MAX_RULE_PACK_CAPABILITY_REQUIREMENTS)
        .map(|index| {
            sekai::rules::CapabilityId::new("sekai.test", format!("capability-{index}"), 1).unwrap()
        })
        .collect();
    assert!(matches!(
        pack(Vec::new(), consumes, Vec::new()),
        Err(RulePackError::TooManyCapabilityRequirements { .. })
    ));

    let contributions = (0..=MAX_RULE_PACK_CONTRIBUTIONS)
        .map(|index| plate_constraint(&format!("item-{index}"), 12))
        .collect();
    assert!(matches!(
        pack(Vec::new(), Vec::new(), contributions),
        Err(RulePackError::TooManyContributions { .. })
    ));
}

#[test]
fn complete_rule_pack_round_trips_and_revalidates_content_hash() {
    let rule_pack = pack(
        vec![dependency(1)],
        vec![tectonic_controls_capability_id()],
        vec![plate_constraint("prefer-twelve", 12)],
    )
    .unwrap();
    let encoded = serde_json::to_vec(&rule_pack).unwrap();
    let decoded: RulePack = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, rule_pack);
    let manifest_encoded = serde_json::to_vec(rule_pack.manifest()).unwrap();
    let manifest_decoded: sekai::rules::RulePackManifest =
        serde_json::from_slice(&manifest_encoded).unwrap();
    assert_eq!(&manifest_decoded, rule_pack.manifest());

    let mut content_tamper = serde_json::to_value(&rule_pack).unwrap();
    content_tamper["contributions"][0]["TectonicConstraint"]["clause"]["PlateCount"]["minimum"] =
        serde_json::json!(11);
    assert!(serde_json::from_value::<RulePack>(content_tamper).is_err());

    let mut hash_tamper = serde_json::to_value(&rule_pack).unwrap();
    hash_tamper["manifest"]["content_hash"][0] = serde_json::json!(255);
    assert!(serde_json::from_value::<RulePack>(hash_tamper).is_err());

    let mut provides_tamper = serde_json::to_value(&rule_pack).unwrap();
    provides_tamper["manifest"]["provides"] = serde_json::json!([]);
    assert!(serde_json::from_value::<RulePack>(provides_tamper).is_err());

    let mut schema_tamper = serde_json::to_value(&rule_pack).unwrap();
    schema_tamper["manifest"]["schema_version"] = serde_json::json!(9);
    assert!(serde_json::from_value::<RulePack>(schema_tamper).is_err());
}

#[test]
fn empty_content_pack_is_valid_but_provides_no_capability() {
    let empty = pack(Vec::new(), Vec::new(), Vec::new()).unwrap();

    assert!(empty.manifest().provides().is_empty());
    assert!(empty.contributions().is_empty());
    empty.validate().unwrap();
}
