use sekai::rules::{
    ActivitySet, AuthorConstraint, AuthorConstraints, ConstraintError, ConstraintStrength,
    InclusiveU16Range, RuleItemId, RuleTectonicConstraint, TectonicConstraintClause,
    TectonicControl, AUTHOR_CONSTRAINTS_SCHEMA_V1, MAX_AUTHOR_CONSTRAINTS,
};
use sekai::world::natural::{TectonicActivity, MAX_PLATE_COUNT, MIN_PLATE_COUNT};
use sekai::world::AuthorObjectId;

fn author(
    id: u64,
    strength: ConstraintStrength,
    clause: TectonicConstraintClause,
) -> AuthorConstraint {
    AuthorConstraint::new(AuthorObjectId::from_raw(id), strength, clause).unwrap()
}

#[test]
fn constraint_strength_distinguishes_hard_soft_and_hint() {
    assert_eq!(ConstraintStrength::Hard.weight(), None);
    assert_eq!(ConstraintStrength::soft(1).unwrap().weight(), Some(1));
    assert_eq!(ConstraintStrength::soft(1000).unwrap().weight(), Some(1000));
    assert_eq!(ConstraintStrength::hint(27).unwrap().weight(), Some(27));
    assert!(matches!(
        ConstraintStrength::soft(0),
        Err(ConstraintError::WeightOutOfRange { .. })
    ));
    assert!(ConstraintStrength::hint(1001).is_err());
}

#[test]
fn tectonic_ranges_enforce_order_and_physical_bounds() {
    assert!(InclusiveU16Range::new(7, 7).is_ok());
    assert!(InclusiveU16Range::new(8, 7).is_err());

    let plates = TectonicConstraintClause::plate_count(MIN_PLATE_COUNT, MAX_PLATE_COUNT).unwrap();
    assert_eq!(plates.target(), TectonicControl::PlateCount);
    assert!(TectonicConstraintClause::plate_count(MIN_PLATE_COUNT - 1, 12).is_err());
    assert!(TectonicConstraintClause::plate_count(12, MAX_PLATE_COUNT + 1).is_err());

    let crust = TectonicConstraintClause::continental_crust_permille(100, 750).unwrap();
    assert_eq!(crust.target(), TectonicControl::ContinentalCrustFraction);
    assert!(TectonicConstraintClause::continental_crust_permille(99, 400).is_err());
    assert!(TectonicConstraintClause::continental_crust_permille(400, 751).is_err());
}

#[test]
fn activity_sets_normalize_order_and_reject_empty_or_duplicate_values() {
    let activities = ActivitySet::new([
        TectonicActivity::Active,
        TectonicActivity::Quiet,
        TectonicActivity::Moderate,
    ])
    .unwrap();

    assert_eq!(
        activities.values(),
        &[
            TectonicActivity::Quiet,
            TectonicActivity::Moderate,
            TectonicActivity::Active,
        ]
    );
    assert!(ActivitySet::new([]).is_err());
    assert!(ActivitySet::new([TectonicActivity::Quiet, TectonicActivity::Quiet]).is_err());

    let clause =
        TectonicConstraintClause::activity([TectonicActivity::Active, TectonicActivity::Quiet])
            .unwrap();
    assert_eq!(clause.target(), TectonicControl::Activity);
}

#[test]
fn rule_and_author_constraints_share_the_same_typed_clause() {
    let clause = TectonicConstraintClause::plate_count(8, 16).unwrap();
    let rule = RuleTectonicConstraint::new(
        RuleItemId::new("balanced-plates").unwrap(),
        ConstraintStrength::soft(25).unwrap(),
        clause.clone(),
    )
    .unwrap();
    let authored = AuthorConstraint::new(
        AuthorObjectId::from_raw(99),
        ConstraintStrength::Hard,
        clause.clone(),
    )
    .unwrap();

    assert_eq!(rule.clause(), &clause);
    assert_eq!(authored.clause(), &clause);
    assert_eq!(rule.strength().weight(), Some(25));
    assert_eq!(authored.strength(), ConstraintStrength::Hard);
}

#[test]
fn author_constraint_sets_sort_ids_and_reject_duplicates() {
    let first = author(
        2,
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(10, 10).unwrap(),
    );
    let second = author(
        1,
        ConstraintStrength::hint(3).unwrap(),
        TectonicConstraintClause::activity([TectonicActivity::Moderate]).unwrap(),
    );
    let constraints =
        AuthorConstraints::new(AUTHOR_CONSTRAINTS_SCHEMA_V1, vec![first, second]).unwrap();

    assert_eq!(constraints.constraints()[0].id().raw(), 1);
    assert_eq!(constraints.constraints()[1].id().raw(), 2);
    assert!(AuthorConstraints::new(
        AUTHOR_CONSTRAINTS_SCHEMA_V1,
        vec![
            author(
                7,
                ConstraintStrength::Hard,
                TectonicConstraintClause::plate_count(8, 8).unwrap(),
            ),
            author(
                7,
                ConstraintStrength::Hard,
                TectonicConstraintClause::plate_count(9, 9).unwrap(),
            ),
        ],
    )
    .is_err());
}

#[test]
fn author_constraint_sets_enforce_schema_and_allocation_budget() {
    assert!(matches!(
        AuthorConstraints::new(2, Vec::new()),
        Err(ConstraintError::UnsupportedAuthorSchema { .. })
    ));

    let template = author(
        0,
        ConstraintStrength::Hard,
        TectonicConstraintClause::plate_count(12, 12).unwrap(),
    );
    let at_limit = (0..MAX_AUTHOR_CONSTRAINTS)
        .map(|index| {
            AuthorConstraint::new(
                AuthorObjectId::from_raw(index as u64),
                template.strength(),
                template.clause().clone(),
            )
            .unwrap()
        })
        .collect();
    assert!(AuthorConstraints::new(AUTHOR_CONSTRAINTS_SCHEMA_V1, at_limit).is_ok());

    let over_limit = (0..=MAX_AUTHOR_CONSTRAINTS)
        .map(|index| {
            AuthorConstraint::new(
                AuthorObjectId::from_raw(index as u64),
                template.strength(),
                template.clause().clone(),
            )
            .unwrap()
        })
        .collect();
    assert!(matches!(
        AuthorConstraints::new(AUTHOR_CONSTRAINTS_SCHEMA_V1, over_limit),
        Err(ConstraintError::TooManyAuthorConstraints { .. })
    ));
}

#[test]
fn constraint_contracts_round_trip_and_revalidate_json() {
    let constraints = AuthorConstraints::new(
        AUTHOR_CONSTRAINTS_SCHEMA_V1,
        vec![
            author(
                1,
                ConstraintStrength::soft(40).unwrap(),
                TectonicConstraintClause::continental_crust_permille(350, 450).unwrap(),
            ),
            author(
                2,
                ConstraintStrength::hint(5).unwrap(),
                TectonicConstraintClause::activity([
                    TectonicActivity::Moderate,
                    TectonicActivity::Active,
                ])
                .unwrap(),
            ),
        ],
    )
    .unwrap();
    let encoded = serde_json::to_vec(&constraints).unwrap();
    let decoded: AuthorConstraints = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(decoded, constraints);

    let mut invalid_weight = serde_json::to_value(ConstraintStrength::soft(7).unwrap()).unwrap();
    *invalid_weight
        .get_mut("Soft")
        .expect("externally tagged strength") = serde_json::json!(0);
    assert!(serde_json::from_value::<ConstraintStrength>(invalid_weight).is_err());

    let duplicate_activity = serde_json::json!(["Quiet", "Quiet"]);
    assert!(serde_json::from_value::<ActivitySet>(duplicate_activity).is_err());

    let mut invalid_schema = serde_json::to_value(&constraints).unwrap();
    invalid_schema["schema_version"] = serde_json::json!(9);
    assert!(serde_json::from_value::<AuthorConstraints>(invalid_schema).is_err());

    let mut duplicate_ids = serde_json::to_value(&constraints).unwrap();
    duplicate_ids["constraints"][1]["id"] = serde_json::json!(1);
    assert!(serde_json::from_value::<AuthorConstraints>(duplicate_ids).is_err());
}

#[test]
fn empty_author_constraints_are_the_valid_v1_default() {
    let constraints = AuthorConstraints::default();

    assert_eq!(constraints.schema_version(), AUTHOR_CONSTRAINTS_SCHEMA_V1);
    assert!(constraints.constraints().is_empty());
    constraints.validate().unwrap();
}
