use sekai::world::natural::{
    NaturalQualityReport, QualityBounds, QualityMetric, QualityMetricId, QualityMetricStatus,
    NATURAL_QUALITY_REPORT_SCHEMA_V1,
};
use sekai::world::spatial::{SurfaceGeometryKind, SurfaceRef, SPHERICAL_SURFACE_SCHEMA_V1};

fn surface_ref() -> SurfaceRef {
    SurfaceRef::new(
        SurfaceGeometryKind::SphericalV1,
        SPHERICAL_SURFACE_SCHEMA_V1,
        42,
        120,
        [7; 32],
    )
    .unwrap()
}

fn metric(namespace: &str, name: &str, value: f64, bounds: QualityBounds) -> QualityMetric {
    QualityMetric::new(
        QualityMetricId::new(namespace, name, 1).unwrap(),
        QualityMetricStatus::Pass,
        Some(value),
        42,
        bounds,
        None,
    )
    .unwrap()
}

#[test]
fn report_sorts_metrics_and_round_trips_without_changing_semantics() {
    let later = metric(
        "tectonics",
        "continental-area-fraction",
        0.38,
        QualityBounds::between(0.30, 0.45).unwrap(),
    );
    let earlier = metric(
        "quality",
        "non-finite-value-count",
        0.0,
        QualityBounds::at_most(0.0).unwrap(),
    );
    let report = NaturalQualityReport::new(
        NATURAL_QUALITY_REPORT_SCHEMA_V1,
        surface_ref(),
        vec![later, earlier],
    )
    .unwrap();

    let ids = report
        .metrics()
        .iter()
        .map(|metric| (metric.id().namespace(), metric.id().name()))
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            ("quality", "non-finite-value-count"),
            ("tectonics", "continental-area-fraction"),
        ]
    );
    assert_eq!(report.surface_ref(), surface_ref());

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["schema_version"], NATURAL_QUALITY_REPORT_SCHEMA_V1);
    let exact_json = serde_json::to_string(&report).unwrap();
    let decoded: NaturalQualityReport = serde_json::from_str(&exact_json).unwrap();
    assert_eq!(decoded, report);
    assert_eq!(serde_json::to_string(&decoded).unwrap(), exact_json);
}

#[test]
fn metric_constructor_rejects_status_value_bound_and_sample_contradictions() {
    let id = QualityMetricId::new("tectonics", "continental-retention", 1).unwrap();
    let between = QualityBounds::between(0.75, 1.15).unwrap();

    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Pass,
        Some(0.50),
        17,
        between,
        None,
    )
    .is_err());
    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Fail,
        Some(0.90),
        17,
        between,
        None,
    )
    .is_err());
    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Pass,
        None,
        17,
        between,
        None,
    )
    .is_err());
    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Pass,
        Some(f64::NAN),
        17,
        between,
        None,
    )
    .is_err());
    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Pass,
        Some(0.90),
        17,
        between,
        Some("unexpected explanation".to_owned()),
    )
    .is_err());
    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Pass,
        Some(0.90),
        0,
        between,
        None,
    )
    .is_err());
    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Unavailable,
        Some(0.90),
        17,
        between,
        Some("no samples".to_owned()),
    )
    .is_err());
    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Unavailable,
        None,
        17,
        between,
        Some("no samples".to_owned()),
    )
    .is_err());
    assert!(QualityMetric::new(
        id.clone(),
        QualityMetricStatus::Unavailable,
        None,
        0,
        between,
        None,
    )
    .is_err());

    let unavailable = QualityMetric::new(
        id,
        QualityMetricStatus::Unavailable,
        None,
        0,
        between,
        Some("no positive finite sample weight".to_owned()),
    )
    .unwrap();
    assert_eq!(unavailable.status(), QualityMetricStatus::Unavailable);
    assert_eq!(unavailable.value(), None);
    assert_eq!(unavailable.sample_count(), 0);
}

#[test]
fn identifiers_and_bounds_reject_invalid_or_ambiguous_values() {
    for (namespace, name, version) in [
        ("", "metric", 1),
        ("Bad", "metric", 1),
        ("quality", "bad metric", 1),
        ("quality", "metric", 0),
    ] {
        assert!(QualityMetricId::new(namespace, name, version).is_err());
    }

    assert!(QualityBounds::between(2.0, 1.0).is_err());
    assert!(QualityBounds::between(f64::NEG_INFINITY, 1.0).is_err());
    assert!(QualityBounds::at_least(f64::NAN).is_err());
    assert!(QualityBounds::at_most(f64::INFINITY).is_err());
}

#[test]
fn report_rejects_duplicate_wrong_schema_unknown_fields_and_oversized_metric_lists() {
    let one = metric(
        "quality",
        "non-finite-value-count",
        0.0,
        QualityBounds::at_most(0.0).unwrap(),
    );
    assert!(NaturalQualityReport::new(
        NATURAL_QUALITY_REPORT_SCHEMA_V1,
        surface_ref(),
        vec![one.clone(), one],
    )
    .is_err());

    let valid = NaturalQualityReport::new(
        NATURAL_QUALITY_REPORT_SCHEMA_V1,
        surface_ref(),
        vec![metric(
            "quality",
            "non-finite-value-count",
            0.0,
            QualityBounds::at_most(0.0).unwrap(),
        )],
    )
    .unwrap();
    let encoded = serde_json::to_value(valid).unwrap();

    let mut wrong_schema = encoded.clone();
    wrong_schema["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<NaturalQualityReport>(wrong_schema).is_err());

    let mut invalid_surface = encoded.clone();
    invalid_surface["surface_ref"]["cell_count"] = serde_json::json!(0);
    assert!(serde_json::from_value::<NaturalQualityReport>(invalid_surface).is_err());

    let mut unknown = encoded.clone();
    unknown["renderer"] = serde_json::json!("globe");
    assert!(serde_json::from_value::<NaturalQualityReport>(unknown).is_err());

    let template = encoded["metrics"][0].clone();
    let oversized = (0..=4_096)
        .map(|index| {
            let mut metric = template.clone();
            metric["id"]["name"] = serde_json::json!(format!("metric-{index}"));
            metric
        })
        .collect::<Vec<_>>();
    let mut oversized_report = encoded;
    oversized_report["metrics"] = serde_json::Value::Array(oversized);
    assert!(serde_json::from_value::<NaturalQualityReport>(oversized_report).is_err());
}
