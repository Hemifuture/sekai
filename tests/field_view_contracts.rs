use std::collections::BTreeMap;

use sekai::view::{
    CellFillKind, FieldCatalog, FieldPayloadRef, FieldValue, FieldView, FieldViewError,
};
use sekai::world::fields::{
    DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain, FieldId,
    FieldPaletteHint, FieldRegistry, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
    MissingValuePolicy, StableIdKind, ValueRange,
};

const CELL_COUNT: usize = 2;

fn field_id(name: &str) -> FieldId {
    FieldId::new("test.display", name, 1).unwrap()
}

fn schema(name: &str, value_type: FieldValueType) -> FieldSchema {
    let (palette, valid_range, category_labels) = match value_type {
        FieldValueType::ScalarF32 => (
            FieldPaletteHint::Sequential,
            Some(ValueRange::new(0.0, 1.0).unwrap()),
            BTreeMap::new(),
        ),
        FieldValueType::CategoryU32 => (
            FieldPaletteHint::Categorical,
            None,
            BTreeMap::from([
                (7, "field.test.category.seven".into()),
                (9, "field.test.category.nine".into()),
            ]),
        ),
        FieldValueType::Boolean => (FieldPaletteHint::Boolean, None, BTreeMap::new()),
        FieldValueType::Vector2F32 => (FieldPaletteHint::Vector, None, BTreeMap::new()),
        FieldValueType::StableIdU32(_) => (FieldPaletteHint::Categorical, None, BTreeMap::new()),
    };

    FieldSchema {
        id: field_id(name),
        domain: FieldDomain::Cells,
        value_type,
        unit: FieldUnit::Unitless,
        valid_range,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels,
        display: FieldDisplayMetadata::new(
            format!("field.test.{name}"),
            palette,
            if value_type == FieldValueType::ScalarF32 {
                2
            } else {
                0
            },
        )
        .unwrap(),
    }
}

fn registry_with_all_types() -> FieldRegistry {
    let mut builder = FieldRegistryBuilder::new();
    for schema in [
        schema("scalar", FieldValueType::ScalarF32),
        schema("category", FieldValueType::CategoryU32),
        schema("boolean", FieldValueType::Boolean),
        schema("vector", FieldValueType::Vector2F32),
        schema("stable", FieldValueType::StableIdU32(StableIdKind::Cell)),
        schema("missing", FieldValueType::ScalarF32),
    ] {
        builder.register(schema).unwrap();
    }
    builder.build().unwrap()
}

fn fixture_with_one_missing_payload() -> (FieldRegistry, ExtensionFieldSet) {
    let registry = registry_with_all_types();
    let sizes = DomainSizes::new(CELL_COUNT, 0);
    let mut fields = ExtensionFieldSet::new();

    for (name, data) in [
        ("scalar", FieldData::ScalarF32(vec![0.25, 0.75])),
        ("category", FieldData::CategoryU32(vec![7, 9])),
        ("boolean", FieldData::Boolean(vec![true, false])),
        (
            "vector",
            FieldData::Vector2F32(vec![[1.0, -2.0], [3.0, 4.0]]),
        ),
        (
            "stable",
            FieldData::StableIdU32 {
                target: StableIdKind::Cell,
                values: vec![1, 0],
            },
        ),
    ] {
        fields
            .insert(&registry, field_id(name), data, &sizes)
            .unwrap();
    }

    (registry, fields)
}

fn complete_fixture() -> (FieldRegistry, ExtensionFieldSet) {
    fixture_with_one_missing_payload()
}

#[test]
fn payload_cardinality_is_public_and_read_only() {
    let populated = FieldData::ScalarF32(vec![0.25, 0.75]);
    let empty = FieldData::Boolean(Vec::new());

    assert_eq!(populated.len(), 2);
    assert!(!populated.is_empty());
    assert_eq!(empty.len(), 0);
    assert!(empty.is_empty());
}

#[test]
fn catalog_is_id_ordered_and_keeps_missing_registered_fields() {
    let (registry, fields) = fixture_with_one_missing_payload();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
    let ids: Vec<_> = catalog
        .entries()
        .iter()
        .map(|entry| entry.schema().id.clone())
        .collect();

    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert!(catalog.get(&field_id("missing")).unwrap().view().is_none());
}

#[test]
fn field_view_reads_every_supported_payload_without_copying() {
    let (registry, fields) = complete_fixture();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();

    assert_eq!(
        catalog
            .get(&field_id("scalar"))
            .unwrap()
            .view()
            .unwrap()
            .value(1),
        Some(FieldValue::Scalar(0.75))
    );
    assert_eq!(
        catalog
            .get(&field_id("category"))
            .unwrap()
            .view()
            .unwrap()
            .value(0),
        Some(FieldValue::Category(7))
    );
    assert_eq!(
        catalog
            .get(&field_id("boolean"))
            .unwrap()
            .view()
            .unwrap()
            .value(1),
        Some(FieldValue::Boolean(false))
    );
    assert_eq!(
        catalog
            .get(&field_id("vector"))
            .unwrap()
            .view()
            .unwrap()
            .value(0),
        Some(FieldValue::Vector2([1.0, -2.0]))
    );
    assert_eq!(
        catalog
            .get(&field_id("stable"))
            .unwrap()
            .view()
            .unwrap()
            .value(1),
        Some(FieldValue::StableId {
            target: StableIdKind::Cell,
            value: 0,
        })
    );

    let FieldData::ScalarF32(source) = fields.get(&field_id("scalar")).unwrap() else {
        panic!("fixture scalar payload changed type");
    };
    let borrowed = catalog
        .get(&field_id("scalar"))
        .unwrap()
        .view()
        .unwrap()
        .scalar_values()
        .unwrap();
    assert!(std::ptr::eq(source.as_ptr(), borrowed.as_ptr()));
}

#[test]
fn cell_fill_support_matrix_is_explicit() {
    let (registry, fields) = complete_fixture();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();

    assert_eq!(
        catalog
            .get(&field_id("scalar"))
            .unwrap()
            .view()
            .unwrap()
            .cell_fill_kind(),
        Ok(CellFillKind::Scalar)
    );
    assert_eq!(
        catalog
            .get(&field_id("category"))
            .unwrap()
            .view()
            .unwrap()
            .cell_fill_kind(),
        Ok(CellFillKind::Category)
    );
    assert!(matches!(
        catalog
            .get(&field_id("vector"))
            .unwrap()
            .view()
            .unwrap()
            .cell_fill_kind(),
        Err(FieldViewError::UnsupportedCellFill { .. })
    ));
    assert_eq!(
        catalog.first_renderable().unwrap().schema().id,
        field_id("category")
    );
}

#[test]
fn direct_field_view_rejects_schema_payload_type_mismatch() {
    let schema = schema("scalar", FieldValueType::ScalarF32);
    let data = FieldData::Boolean(vec![true, false]);

    assert!(matches!(
        FieldView::new(&schema, &data),
        Err(FieldViewError::TypeMismatch { .. })
    ));
}

#[test]
fn borrowed_scalar_payload_reuses_the_source_slice() {
    let registry = registry_with_all_types();
    let scalar = vec![0.25_f32, 0.75];

    let catalog = FieldCatalog::from_payloads(
        &registry,
        [(field_id("scalar"), FieldPayloadRef::ScalarF32(&scalar))],
    )
    .unwrap();
    let borrowed = catalog
        .get(&field_id("scalar"))
        .unwrap()
        .view()
        .unwrap()
        .scalar_values()
        .unwrap();

    assert_eq!(borrowed, scalar);
    assert!(std::ptr::eq(borrowed.as_ptr(), scalar.as_ptr()));
}

#[test]
fn borrowed_payloads_read_every_controlled_value_type() {
    let registry = registry_with_all_types();
    let categories = vec![7_u32, 9];
    let booleans = vec![true, false];
    let vectors = vec![[1.0_f32, -2.0], [3.0, 4.0]];
    let stable_ids = vec![1_u32, 0];

    let catalog = FieldCatalog::from_payloads(
        &registry,
        [
            (
                field_id("category"),
                FieldPayloadRef::CategoryU32(&categories),
            ),
            (field_id("boolean"), FieldPayloadRef::Boolean(&booleans)),
            (field_id("vector"), FieldPayloadRef::Vector2F32(&vectors)),
            (
                field_id("stable"),
                FieldPayloadRef::StableIdU32 {
                    target: StableIdKind::Cell,
                    values: &stable_ids,
                },
            ),
        ],
    )
    .unwrap();

    let category = catalog.get(&field_id("category")).unwrap().view().unwrap();
    assert_eq!(category.value(1), Some(FieldValue::Category(9)));
    assert!(std::ptr::eq(
        category.category_values().unwrap().as_ptr(),
        categories.as_ptr()
    ));
    assert_eq!(
        catalog
            .get(&field_id("boolean"))
            .unwrap()
            .view()
            .unwrap()
            .value(1),
        Some(FieldValue::Boolean(false))
    );
    let vector = catalog.get(&field_id("vector")).unwrap().view().unwrap();
    assert_eq!(vector.value(0), Some(FieldValue::Vector2([1.0, -2.0])));
    assert!(std::ptr::eq(
        vector.vector_values().unwrap().as_ptr(),
        vectors.as_ptr()
    ));
    let stable = catalog.get(&field_id("stable")).unwrap().view().unwrap();
    assert_eq!(
        stable.stable_id_values(),
        Some((StableIdKind::Cell, stable_ids.as_slice()))
    );
}

#[test]
fn borrowed_catalog_rejects_unknown_and_duplicate_payload_ids() {
    let registry = registry_with_all_types();
    let values = vec![0.25_f32, 0.75];
    let unknown = field_id("unknown");

    assert!(matches!(
        FieldCatalog::from_payloads(
            &registry,
            [(unknown.clone(), FieldPayloadRef::ScalarF32(&values))]
        ),
        Err(FieldViewError::UnknownPayload { field }) if field == unknown
    ));
    assert!(matches!(
        FieldCatalog::from_payloads(
            &registry,
            [
                (
                    field_id("scalar"),
                    FieldPayloadRef::ScalarF32(&values)
                ),
                (
                    field_id("scalar"),
                    FieldPayloadRef::ScalarF32(&values)
                ),
            ]
        ),
        Err(FieldViewError::DuplicatePayload { field }) if field == field_id("scalar")
    ));
}

#[test]
fn borrowed_catalog_keeps_absent_schemas_and_rejects_type_mismatches() {
    let registry = registry_with_all_types();
    let values = vec![true, false];

    let catalog = FieldCatalog::from_payloads(
        &registry,
        [(field_id("boolean"), FieldPayloadRef::Boolean(&values))],
    )
    .unwrap();
    assert!(catalog.get(&field_id("missing")).unwrap().view().is_none());
    assert!(matches!(
        FieldCatalog::from_payloads(
            &registry,
            [(
                field_id("scalar"),
                FieldPayloadRef::Boolean(&values)
            )]
        ),
        Err(FieldViewError::TypeMismatch { field, .. }) if field == field_id("scalar")
    ));
}

#[test]
fn borrowed_stable_ids_require_the_schema_target_kind() {
    let registry = registry_with_all_types();
    let values = vec![1_u32, 0];

    assert!(matches!(
        FieldCatalog::from_payloads(
            &registry,
            [(
                field_id("stable"),
                FieldPayloadRef::StableIdU32 {
                    target: StableIdKind::Edge,
                    values: &values,
                },
            )]
        ),
        Err(FieldViewError::TypeMismatch { field, .. }) if field == field_id("stable")
    ));
}
