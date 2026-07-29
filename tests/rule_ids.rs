use sekai::rules::{
    CapabilityId, CoreSchemaRange, RuleContentHash, RuleIdentityError, RuleItemId, RulePackId,
    RuleVersion, RuleVersionRequirement,
};

#[test]
fn stable_rule_identifiers_accept_the_v1_grammar() {
    let pack = RulePackId::new("sekai.builtin.earthlike").unwrap();
    let item = RuleItemId::new("prefer-moderate_activity").unwrap();
    let capability = CapabilityId::new("sekai.core.natural", "tectonic-model", 1).unwrap();

    assert_eq!(pack.as_str(), "sekai.builtin.earthlike");
    assert_eq!(item.as_str(), "prefer-moderate_activity");
    assert_eq!(capability.namespace(), "sekai.core.natural");
    assert_eq!(capability.name(), "tectonic-model");
    assert_eq!(capability.version(), 1);
}

#[test]
fn stable_rule_identifiers_reject_invalid_boundaries() {
    for invalid in [
        "",
        ".leading",
        "trailing.",
        "Uppercase",
        "has space",
        "has/slash",
    ] {
        assert!(RulePackId::new(invalid).is_err(), "{invalid:?}");
        assert!(RuleItemId::new(invalid).is_err(), "{invalid:?}");
    }
    assert!(RulePackId::new("a".repeat(128)).is_ok());
    assert!(matches!(
        RulePackId::new("a".repeat(129)),
        Err(RuleIdentityError::InvalidIdentifier { .. })
    ));
    assert!(CapabilityId::new("sekai.core", "tectonics", 0).is_err());
    assert!(CapabilityId::new("Bad", "tectonics", 1).is_err());
}

#[test]
fn rule_versions_use_a_bounded_semver_compatibility_rule() {
    let version = RuleVersion::new(2, 4, 7).unwrap();

    assert_eq!(version.major(), 2);
    assert_eq!(version.minor(), 4);
    assert_eq!(version.patch(), 7);
    assert!(RuleVersion::new(0, 1, 0).is_err());
    assert!(RuleVersionRequirement::new(0, 0, 0).is_err());

    assert!(RuleVersionRequirement::new(2, 4, 7)
        .unwrap()
        .matches(version));
    assert!(RuleVersionRequirement::new(2, 3, 99)
        .unwrap()
        .matches(version));
    assert!(!RuleVersionRequirement::new(2, 4, 8)
        .unwrap()
        .matches(version));
    assert!(!RuleVersionRequirement::new(3, 0, 0)
        .unwrap()
        .matches(version));
}

#[test]
fn core_schema_ranges_are_non_zero_ordered_and_inclusive() {
    let range = CoreSchemaRange::new(1, 3).unwrap();

    assert_eq!(range.minimum(), 1);
    assert_eq!(range.maximum(), 3);
    assert!(range.contains(1));
    assert!(range.contains(3));
    assert!(!range.contains(4));
    assert!(CoreSchemaRange::new(0, 1).is_err());
    assert!(CoreSchemaRange::new(2, 1).is_err());
}

#[test]
fn identity_contracts_round_trip_and_revalidate_json() {
    let pack = RulePackId::new("sekai.test.pack").unwrap();
    let capability = CapabilityId::new("sekai.test", "capability", 7).unwrap();
    let version = RuleVersion::new(1, 2, 3).unwrap();
    let requirement = RuleVersionRequirement::new(1, 1, 9).unwrap();
    let range = CoreSchemaRange::new(1, 2).unwrap();

    for value in [
        serde_json::to_value(&pack).unwrap(),
        serde_json::to_value(&capability).unwrap(),
        serde_json::to_value(version).unwrap(),
        serde_json::to_value(requirement).unwrap(),
        serde_json::to_value(range).unwrap(),
    ] {
        assert!(!value.is_null());
    }

    assert_eq!(
        serde_json::from_value::<RulePackId>(serde_json::to_value(&pack).unwrap()).unwrap(),
        pack
    );
    assert_eq!(
        serde_json::from_value::<CapabilityId>(serde_json::to_value(&capability).unwrap()).unwrap(),
        capability
    );
    assert_eq!(
        serde_json::from_value::<RuleVersion>(serde_json::to_value(version).unwrap()).unwrap(),
        version
    );
    assert_eq!(
        serde_json::from_value::<RuleVersionRequirement>(
            serde_json::to_value(requirement).unwrap()
        )
        .unwrap(),
        requirement
    );
    assert_eq!(
        serde_json::from_value::<CoreSchemaRange>(serde_json::to_value(range).unwrap()).unwrap(),
        range
    );

    assert!(serde_json::from_str::<RulePackId>(r#""Bad Pack""#).is_err());
    assert!(serde_json::from_str::<CapabilityId>(
        r#"{"namespace":"sekai.test","name":"capability","version":0}"#
    )
    .is_err());
    assert!(serde_json::from_str::<RuleVersion>(r#"{"major":0,"minor":1,"patch":0}"#).is_err());
    assert!(serde_json::from_str::<CoreSchemaRange>(r#"{"minimum":3,"maximum":2}"#).is_err());
}

#[test]
fn content_hash_exposes_exact_bytes_and_round_trips() {
    let hash = RuleContentHash::from_bytes([0x5a; 32]);
    let encoded = serde_json::to_vec(&hash).unwrap();
    let decoded: RuleContentHash = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(hash.as_bytes(), &[0x5a; 32]);
    assert_eq!(decoded, hash);
}
