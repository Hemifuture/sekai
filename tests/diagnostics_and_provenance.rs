use std::time::Duration;

use sekai::engine::{
    BuildReport, Diagnostic, DiagnosticContext, DiagnosticSeverity, EntityRef, FactorContribution,
    ProvenanceIndex, SourceRef, StageReport,
};
use sekai::world::fields::FieldId;
use sekai::world::{AuthorObjectId, CellId};

fn field(name: &str) -> FieldId {
    FieldId::new("sekai.core", name, 1).unwrap()
}

fn contribution(code: &str, source: SourceRef, weight: f32, reason_id: &str) -> FactorContribution {
    FactorContribution::new(code, source, weight, reason_id).unwrap()
}

#[test]
fn diagnostics_keep_machine_readable_codes() {
    let diagnostic = Diagnostic::new(
        DiagnosticSeverity::Warning,
        "spatial.near-degenerate-cell",
        "cell polygon is close to the minimum area",
    )
    .unwrap();

    assert_eq!(diagnostic.code(), "spatial.near-degenerate-cell");
    assert_eq!(diagnostic.severity(), DiagnosticSeverity::Warning);
    assert_eq!(
        diagnostic.message(),
        "cell polygon is close to the minimum area"
    );
}

#[test]
fn diagnostics_reject_invalid_code_boundaries() {
    for code in ["", "A", "-bad", "bad-", "bad space", &"a".repeat(129)] {
        assert!(Diagnostic::new(DiagnosticSeverity::Error, code, "message").is_err());
    }

    assert!(Diagnostic::new(DiagnosticSeverity::Info, "a", "message").is_ok());
    assert!(Diagnostic::new(DiagnosticSeverity::Info, "a".repeat(128), "message").is_ok());
}

#[test]
fn diagnostics_deserialization_preserves_code_validation() {
    let encoded = r#"{"severity":"Warning","code":"Invalid","message":"message","context":{"stage_id":null,"field_id":null,"cell_id":null,"author_object_id":null}}"#;

    assert!(serde_json::from_str::<Diagnostic>(encoded).is_err());
}

#[test]
fn diagnostic_context_keeps_typed_locations() {
    let context = DiagnosticContext {
        stage_id: Some("spatial.planar-voronoi".into()),
        field_id: Some(field("elevation")),
        cell_id: Some(CellId::from_raw(7)),
        author_object_id: Some(AuthorObjectId::from_raw(11)),
    };
    let diagnostic = Diagnostic::with_context(
        DiagnosticSeverity::Error,
        "spatial.invalid-cell",
        "cell is invalid",
        context,
    )
    .unwrap();

    assert_eq!(diagnostic.context().cell_id, Some(CellId::from_raw(7)));
}

#[test]
fn build_report_exposes_stage_ids_cache_counts_and_errors() {
    let mut report = BuildReport::new();
    report.record_stage(StageReport::new(
        "test.cached",
        Duration::from_millis(3),
        true,
    ));
    report.record_stage(StageReport::new(
        "test.fresh",
        Duration::from_millis(5),
        false,
    ));
    report.push_diagnostic(
        Diagnostic::new(DiagnosticSeverity::Error, "test.failed", "failed").unwrap(),
    );

    assert_eq!(report.stage_ids(), vec!["test.cached", "test.fresh"]);
    assert_eq!(report.cache_hits(), 1);
    assert_eq!(report.cache_misses(), 1);
    assert!(report.has_errors());
    assert_eq!(report.stages()[0].duration(), Duration::from_millis(3));
    assert!(report.result_hash().is_none());
}

#[test]
fn field_dependencies_are_sorted_and_unique() {
    let elevation = field("elevation");
    let slope = field("slope");
    let aspect = field("aspect");
    let mut index = ProvenanceIndex::new();

    index.add_field_dependency(slope.clone(), elevation.clone());
    index.add_field_dependency(slope.clone(), aspect.clone());
    index.add_field_dependency(slope.clone(), elevation.clone());

    assert_eq!(index.field_dependencies(&slope), &[aspect, elevation]);
    assert_eq!(index.field_dependencies(&field("missing")), &[]);
}

#[test]
fn factors_reject_non_finite_weights_and_invalid_identifiers() {
    assert!(FactorContribution::new(
        "terrain.river-access",
        SourceRef::Stage("society.settlements".into()),
        f32::NAN,
        "reason.river-access",
    )
    .is_err());
    assert!(FactorContribution::new(
        "terrain.river-access",
        SourceRef::Stage("society.settlements".into()),
        f32::INFINITY,
        "reason.river-access",
    )
    .is_err());
    assert!(FactorContribution::new(
        "invalid code",
        SourceRef::Stage("society.settlements".into()),
        1.0,
        "reason.river-access",
    )
    .is_err());
    assert!(FactorContribution::new(
        "terrain.river-access",
        SourceRef::Stage("society.settlements".into()),
        1.0,
        "Reason.river-access",
    )
    .is_err());
}

#[test]
fn factor_deserialization_preserves_validation() {
    let encoded = r#"{"code":"Invalid","source":{"Stage":"society.settlements"},"weight":1.0,"reason_id":"reason.river-access"}"#;

    assert!(serde_json::from_str::<FactorContribution>(encoded).is_err());
}

#[test]
fn factors_merge_identical_reason_and_source() {
    let mut index = ProvenanceIndex::new();
    let entity = EntityRef::Cell(CellId::from_raw(4));
    index
        .replace_factors(
            entity,
            [
                contribution(
                    "terrain.river-access",
                    SourceRef::Stage("society.settlements".into()),
                    1.25,
                    "reason.river-access",
                ),
                contribution(
                    "terrain.river-access",
                    SourceRef::Stage("society.settlements".into()),
                    2.75,
                    "reason.river-access",
                ),
            ],
        )
        .unwrap();

    let factors = index.factors(&entity);
    assert_eq!(factors.len(), 1);
    assert_eq!(factors[0].weight(), 4.0);
}

#[test]
fn factors_are_ordered_by_absolute_weight_reason_and_source() {
    let mut index = ProvenanceIndex::new();
    let entity = EntityRef::Cell(CellId::from_raw(1));
    index
        .replace_factors(
            entity,
            [
                contribution(
                    "test.z",
                    SourceRef::Stage("z.stage".into()),
                    2.0,
                    "reason.b",
                ),
                contribution(
                    "test.a",
                    SourceRef::RulePack("a.pack".into()),
                    -3.0,
                    "reason.z",
                ),
                contribution(
                    "test.c",
                    SourceRef::Stage("z.stage".into()),
                    3.0,
                    "reason.a",
                ),
                contribution(
                    "test.b",
                    SourceRef::Stage("a.stage".into()),
                    -2.0,
                    "reason.b",
                ),
            ],
        )
        .unwrap();

    let factors = index.factors(&entity);
    assert_eq!(
        factors
            .iter()
            .map(FactorContribution::reason_id)
            .collect::<Vec<_>>(),
        vec!["reason.a", "reason.z", "reason.b", "reason.b"]
    );
    assert_eq!(factors[2].source(), &SourceRef::Stage("a.stage".into()));
    assert_eq!(factors[3].source(), &SourceRef::Stage("z.stage".into()));
}

#[test]
fn factors_truncate_to_the_sixteen_strongest() {
    let mut index = ProvenanceIndex::new();
    let entity = EntityRef::Cell(CellId::from_raw(2));
    let mut contributions = Vec::new();
    for raw_weight in 1..=17 {
        contributions.push(contribution(
            "test.factor",
            SourceRef::Stage(format!("stage.{raw_weight}")),
            raw_weight as f32,
            &format!("reason.{raw_weight}"),
        ));
    }
    index.replace_factors(entity, contributions).unwrap();

    let factors = index.factors(&entity);
    assert_eq!(factors.len(), 16);
    assert_eq!(factors[0].weight(), 17.0);
    assert_eq!(factors[15].weight(), 2.0);
}

#[test]
fn replacing_factors_rejects_overflow_without_changing_existing_factors() {
    let mut index = ProvenanceIndex::new();
    let entity = EntityRef::Cell(CellId::from_raw(3));
    let source = SourceRef::Stage("test.stage".into());
    index
        .replace_factors(
            entity,
            [contribution(
                "test.factor",
                source.clone(),
                f32::MAX,
                "reason.test",
            )],
        )
        .unwrap();

    assert!(index
        .replace_factors(
            entity,
            [
                contribution("test.factor", source.clone(), f32::MAX, "reason.test"),
                contribution("test.factor", source, f32::MAX, "reason.test"),
            ],
        )
        .is_err());
    assert_eq!(index.factors(&entity)[0].weight(), f32::MAX);
}

#[test]
fn factor_replacement_merges_before_retaining_the_top_sixteen() {
    let mut index = ProvenanceIndex::new();
    let entity = EntityRef::Cell(CellId::from_raw(5));
    let mut contributions = (0..16)
        .map(|index| {
            contribution(
                "test.strong",
                SourceRef::Stage(format!("stage.strong-{index}")),
                2.0,
                &format!("reason.strong-{index}"),
            )
        })
        .collect::<Vec<_>>();
    contributions.extend((0..3).map(|_| {
        contribution(
            "test.aggregate",
            SourceRef::Stage("stage.aggregate".into()),
            1.0,
            "reason.aggregate",
        )
    }));

    index.replace_factors(entity, contributions).unwrap();

    let factors = index.factors(&entity);
    assert_eq!(factors.len(), 16);
    assert_eq!(factors[0].reason_id(), "reason.aggregate");
    assert_eq!(factors[0].weight(), 3.0);
}

#[test]
fn factor_replacement_is_independent_of_input_order() {
    let entity = EntityRef::Cell(CellId::from_raw(6));
    let contributions = vec![
        contribution(
            "test.z",
            SourceRef::Stage("stage.shared".into()),
            1.0,
            "reason.shared",
        ),
        contribution(
            "test.a",
            SourceRef::Stage("stage.shared".into()),
            1.0e20,
            "reason.shared",
        ),
        contribution(
            "test.m",
            SourceRef::Stage("stage.shared".into()),
            -1.0e20,
            "reason.shared",
        ),
        contribution(
            "test.other",
            SourceRef::RulePack("pack.other".into()),
            -2.0,
            "reason.other",
        ),
    ];
    let mut forward = ProvenanceIndex::new();
    forward
        .replace_factors(entity, contributions.clone())
        .unwrap();
    let mut reverse = ProvenanceIndex::new();
    reverse
        .replace_factors(entity, contributions.into_iter().rev())
        .unwrap();

    assert_eq!(forward.factors(&entity), reverse.factors(&entity));
    assert_eq!(
        forward
            .factors(&entity)
            .iter()
            .find(|factor| factor.reason_id() == "reason.shared")
            .unwrap()
            .code(),
        "test.a"
    );
}

#[test]
fn missing_factor_lookups_borrow_an_empty_slice() {
    let index = ProvenanceIndex::new();

    assert_eq!(index.factors(&EntityRef::Cell(CellId::from_raw(999))), &[]);
}
