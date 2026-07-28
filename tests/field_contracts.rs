use std::collections::BTreeMap;

use sekai::world::fields::{
    DomainSizes, EntityKind, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain,
    FieldId, FieldPaletteHint, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
    MissingValuePolicy, StableIdKind, ValueRange,
};

fn mana_schema() -> FieldSchema {
    FieldSchema {
        id: FieldId::new("example.magic", "mana-density", 1).unwrap(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::ScalarF32,
        unit: FieldUnit::Custom {
            namespace: "example.magic".into(),
            name: "mana-density".into(),
            symbol: "M".into(),
        },
        valid_range: Some(ValueRange::new(0.0, 1.0).unwrap()),
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new(
            "field.example.magic.mana-density",
            FieldPaletteHint::Sequential,
            3,
        )
        .unwrap(),
    }
}

fn temperature_schema() -> FieldSchema {
    FieldSchema {
        id: FieldId::new("example.climate", "temperature", 1).unwrap(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::ScalarF32,
        unit: FieldUnit::Custom {
            namespace: "example.climate".into(),
            name: "temperature".into(),
            symbol: "K".into(),
        },
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new(
            "field.example.climate.temperature",
            FieldPaletteHint::Diverging,
            1,
        )
        .unwrap(),
    }
}

fn settlement_owner_schema() -> FieldSchema {
    FieldSchema {
        id: FieldId::new("example.social", "settlement-owner", 1).unwrap(),
        domain: FieldDomain::Entities(EntityKind::Settlement),
        value_type: FieldValueType::StableIdU32(StableIdKind::Polity),
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new(
            "field.example.social.settlement-owner",
            FieldPaletteHint::Categorical,
            0,
        )
        .unwrap(),
    }
}

fn category_schema() -> FieldSchema {
    FieldSchema {
        id: FieldId::new("example.climate", "biome", 1).unwrap(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::CategoryU32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::from([
            (1, "field.example.climate.biome.forest".into()),
            (3, "field.example.climate.biome.desert".into()),
        ]),
        display: FieldDisplayMetadata::new(
            "field.example.climate.biome",
            FieldPaletteHint::Categorical,
            0,
        )
        .unwrap(),
    }
}

fn register_one(schema: FieldSchema) -> sekai::world::fields::FieldRegistry {
    let mut builder = FieldRegistryBuilder::new();
    builder.register(schema).unwrap();
    builder.build().unwrap()
}

#[test]
fn rejects_invalid_field_identifiers() {
    assert!(FieldId::new("Bad Namespace", "mana", 1).is_err());
    assert!(FieldId::new("example.magic", "", 1).is_err());
    assert!(FieldId::new(".example", "mana", 1).is_err());
    assert!(FieldId::new("example", "mana-", 1).is_err());
    assert!(FieldId::new("example", "mana", 0).is_err());
    assert!(FieldId::new("a".repeat(129), "mana", 1).is_err());

    let boundary = FieldId::new("a".repeat(128), "mana", 1).unwrap();
    assert_eq!(boundary.namespace().len(), 128);
    assert_eq!(boundary.name(), "mana");
    assert_eq!(boundary.version(), 1);
}

#[test]
fn private_validated_values_reject_invalid_deserialization() {
    assert!(serde_json::from_str::<FieldId>(
        r#"{"namespace":"Bad Namespace","name":"mana","version":1}"#
    )
    .is_err());
    assert!(serde_json::from_str::<ValueRange>(r#"{"min":2.0,"max":1.0}"#).is_err());
    assert!(serde_json::from_str::<FieldDisplayMetadata>(
        r#"{"label_key":"Bad Label","palette":"Sequential","decimal_places":3}"#
    )
    .is_err());
}

#[test]
fn rejects_invalid_ranges_and_display_metadata() {
    assert!(ValueRange::new(f32::NAN, 1.0).is_err());
    assert!(ValueRange::new(0.0, f32::INFINITY).is_err());
    assert!(ValueRange::new(2.0, 1.0).is_err());
    let range = ValueRange::new(-2.0, 4.0).unwrap();
    assert_eq!(range.min(), -2.0);
    assert_eq!(range.max(), 4.0);

    assert!(FieldDisplayMetadata::new("", FieldPaletteHint::Sequential, 0).is_err());
    assert!(FieldDisplayMetadata::new("a".repeat(129), FieldPaletteHint::Sequential, 0).is_err());
    assert!(
        FieldDisplayMetadata::new("field.example.mana", FieldPaletteHint::Sequential, 10).is_err()
    );
}

#[test]
fn rejects_duplicate_schema_registration() {
    let mut registry = FieldRegistryBuilder::new();
    let schema = mana_schema();
    registry.register(schema.clone()).unwrap();
    assert!(registry.register(schema).is_err());
}

#[test]
fn registration_validates_schema_metadata_and_compatibility() {
    let mut invalid_unit = mana_schema();
    invalid_unit.unit = FieldUnit::Custom {
        namespace: "Bad Namespace".into(),
        name: "mana".into(),
        symbol: String::new(),
    };
    assert!(FieldRegistryBuilder::new().register(invalid_unit).is_err());

    let mut range_on_boolean = mana_schema();
    range_on_boolean.value_type = FieldValueType::Boolean;
    range_on_boolean.display =
        FieldDisplayMetadata::new("field.example.flag", FieldPaletteHint::Boolean, 0).unwrap();
    assert!(FieldRegistryBuilder::new()
        .register(range_on_boolean)
        .is_err());

    let mut wrong_palette = mana_schema();
    wrong_palette.display =
        FieldDisplayMetadata::new("field.example.mana", FieldPaletteHint::Categorical, 0).unwrap();
    assert!(FieldRegistryBuilder::new().register(wrong_palette).is_err());

    let mut empty_categories = category_schema();
    empty_categories.category_labels.clear();
    assert!(FieldRegistryBuilder::new()
        .register(empty_categories)
        .is_err());

    let mut labels_on_scalar = mana_schema();
    labels_on_scalar
        .category_labels
        .insert(1, "field.example.unexpected".into());
    assert!(FieldRegistryBuilder::new()
        .register(labels_on_scalar)
        .is_err());

    let mut invalid_label = category_schema();
    invalid_label.category_labels.insert(2, "Bad Label".into());
    assert!(FieldRegistryBuilder::new().register(invalid_label).is_err());
}

#[test]
fn dependencies_are_normalized_and_missing_dependencies_are_rejected() {
    let dependency = mana_schema();
    let dependency_id = dependency.id.clone();
    let mut dependent = temperature_schema();
    dependent.dependencies = vec![dependency_id.clone(), dependency_id.clone()];
    let dependent_id = dependent.id.clone();

    let mut complete = FieldRegistryBuilder::new();
    complete.register(dependent).unwrap();
    complete.register(dependency).unwrap();
    let complete = complete.build().unwrap();
    assert_eq!(
        complete.get(&dependent_id).unwrap().dependencies,
        vec![dependency_id.clone()]
    );

    let mut missing = temperature_schema();
    missing.dependencies.push(dependency_id);
    let mut incomplete = FieldRegistryBuilder::new();
    incomplete.register(missing).unwrap();
    assert!(incomplete.build().is_err());
}

#[test]
fn dependency_cycles_are_rejected() {
    let mut mana = mana_schema();
    let mut temperature = temperature_schema();
    mana.dependencies.push(temperature.id.clone());
    temperature.dependencies.push(mana.id.clone());

    let mut builder = FieldRegistryBuilder::new();
    builder.register(mana).unwrap();
    builder.register(temperature).unwrap();
    assert!(builder.build().is_err());
}

#[test]
fn immutable_registry_deserialization_revalidates_schemas() {
    let invalid_registry = serde_json::json!([{
        "id": {
            "namespace": "example.magic",
            "name": "mana-density",
            "version": 1
        },
        "domain": "Cells",
        "value_type": "Boolean",
        "unit": "Unitless",
        "valid_range": null,
        "missing": "Forbidden",
        "dependencies": [{
            "namespace": "example.missing",
            "name": "dependency",
            "version": 1
        }],
        "category_labels": {},
        "display": {
            "label_key": "field.example.magic.mana-density",
            "palette": "Boolean",
            "decimal_places": 0
        }
    }]);

    assert!(
        serde_json::from_value::<sekai::world::fields::FieldRegistry>(invalid_registry).is_err()
    );
}

#[test]
fn validates_payload_type_length_and_range() {
    let schema = mana_schema();
    let id = schema.id.clone();
    let registry = register_one(schema);
    let sizes = DomainSizes::new(3, 0);

    let mut valid = ExtensionFieldSet::new();
    assert!(valid
        .insert(
            &registry,
            id.clone(),
            FieldData::ScalarF32(vec![0.1, 0.5, 0.9]),
            &sizes,
        )
        .is_ok());
    assert_eq!(
        valid.get(&id),
        Some(&FieldData::ScalarF32(vec![0.1, 0.5, 0.9]))
    );

    let mut wrong_type = ExtensionFieldSet::new();
    assert!(wrong_type
        .insert(
            &registry,
            id.clone(),
            FieldData::Boolean(vec![true, false, true]),
            &sizes,
        )
        .is_err());

    let mut wrong_length = ExtensionFieldSet::new();
    assert!(wrong_length
        .insert(
            &registry,
            id.clone(),
            FieldData::ScalarF32(vec![0.1, 0.5]),
            &sizes,
        )
        .is_err());

    let mut out_of_range = ExtensionFieldSet::new();
    assert!(out_of_range
        .insert(
            &registry,
            id.clone(),
            FieldData::ScalarF32(vec![0.1, 2.0, 0.9]),
            &sizes,
        )
        .is_err());

    let mut non_finite = ExtensionFieldSet::new();
    assert!(non_finite
        .insert(
            &registry,
            id,
            FieldData::ScalarF32(vec![0.1, f32::NAN, 0.9]),
            &sizes,
        )
        .is_err());
}

#[test]
fn validates_entity_lengths_and_stable_id_targets() {
    let sizes = DomainSizes::new(0, 0)
        .with_entities(EntityKind::Settlement, 2)
        .with_entities(EntityKind::Polity, 6);
    let schema = settlement_owner_schema();
    let id = schema.id.clone();
    let registry = register_one(schema);

    let mut valid = ExtensionFieldSet::new();
    assert!(valid
        .insert(
            &registry,
            id.clone(),
            FieldData::StableIdU32 {
                target: StableIdKind::Polity,
                values: vec![3, 5],
            },
            &sizes,
        )
        .is_ok());

    let mut wrong_length = ExtensionFieldSet::new();
    assert!(wrong_length
        .insert(
            &registry,
            id.clone(),
            FieldData::StableIdU32 {
                target: StableIdKind::Polity,
                values: vec![3],
            },
            &sizes,
        )
        .is_err());

    let mut wrong_target = ExtensionFieldSet::new();
    assert!(wrong_target
        .insert(
            &registry,
            id.clone(),
            FieldData::StableIdU32 {
                target: StableIdKind::Species,
                values: vec![3, 5],
            },
            &sizes,
        )
        .is_err());

    let mut out_of_range = ExtensionFieldSet::new();
    assert!(out_of_range
        .insert(
            &registry,
            id,
            FieldData::StableIdU32 {
                target: StableIdKind::Polity,
                values: vec![3, 6],
            },
            &sizes,
        )
        .is_err());
}

#[test]
fn entity_domains_require_explicit_sizes() {
    let schema = settlement_owner_schema();
    let id = schema.id.clone();
    let registry = register_one(schema);
    let sizes = DomainSizes::new(0, 0).with_entities(EntityKind::Polity, 1);
    let mut fields = ExtensionFieldSet::new();

    assert!(fields
        .insert(
            &registry,
            id,
            FieldData::StableIdU32 {
                target: StableIdKind::Polity,
                values: Vec::new(),
            },
            &sizes,
        )
        .is_err());
}

#[test]
fn validates_global_category_boolean_and_vector_payloads() {
    let mut category = category_schema();
    category.domain = FieldDomain::Global;
    let category_id = category.id.clone();
    let category_registry = register_one(category);

    let mut valid_category = ExtensionFieldSet::new();
    assert!(valid_category
        .insert(
            &category_registry,
            category_id.clone(),
            FieldData::CategoryU32(vec![3]),
            &DomainSizes::new(0, 0),
        )
        .is_ok());

    let mut invalid_category = ExtensionFieldSet::new();
    assert!(invalid_category
        .insert(
            &category_registry,
            category_id,
            FieldData::CategoryU32(vec![2]),
            &DomainSizes::new(0, 0),
        )
        .is_err());

    let boolean = FieldSchema {
        id: FieldId::new("example", "flag", 1).unwrap(),
        domain: FieldDomain::Edges,
        value_type: FieldValueType::Boolean,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new("field.example.flag", FieldPaletteHint::Boolean, 0)
            .unwrap(),
    };
    let boolean_id = boolean.id.clone();
    let boolean_registry = register_one(boolean);
    let mut boolean_fields = ExtensionFieldSet::new();
    assert!(boolean_fields
        .insert(
            &boolean_registry,
            boolean_id,
            FieldData::Boolean(vec![true, false]),
            &DomainSizes::new(0, 2),
        )
        .is_ok());

    let vector = FieldSchema {
        id: FieldId::new("example", "wind", 1).unwrap(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::Vector2F32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new("field.example.wind", FieldPaletteHint::Vector, 2)
            .unwrap(),
    };
    let vector_id = vector.id.clone();
    let vector_registry = register_one(vector);
    let mut invalid_vector = ExtensionFieldSet::new();
    assert!(invalid_vector
        .insert(
            &vector_registry,
            vector_id,
            FieldData::Vector2F32(vec![[1.0, f32::INFINITY]]),
            &DomainSizes::new(1, 0),
        )
        .is_err());
}

#[test]
fn rejects_duplicate_payload_insertion() {
    let schema = mana_schema();
    let id = schema.id.clone();
    let registry = register_one(schema);
    let sizes = DomainSizes::new(3, 0);
    let mut fields = ExtensionFieldSet::new();

    fields
        .insert(
            &registry,
            id.clone(),
            FieldData::ScalarF32(vec![0.1, 0.5, 0.9]),
            &sizes,
        )
        .unwrap();
    assert!(fields
        .insert(
            &registry,
            id,
            FieldData::ScalarF32(vec![0.2, 0.4, 0.8]),
            &sizes,
        )
        .is_err());
}

#[test]
fn rejects_payload_for_unknown_schema() {
    let registry = FieldRegistryBuilder::new().build().unwrap();
    let mut fields = ExtensionFieldSet::new();

    assert!(fields
        .insert(
            &registry,
            FieldId::new("example", "missing", 1).unwrap(),
            FieldData::ScalarF32(vec![0.5]),
            &DomainSizes::new(1, 0),
        )
        .is_err());
}

#[test]
fn registry_serialization_is_ordered() {
    let mut forward = FieldRegistryBuilder::new();
    forward.register(mana_schema()).unwrap();
    forward.register(temperature_schema()).unwrap();

    let mut reverse = FieldRegistryBuilder::new();
    reverse.register(temperature_schema()).unwrap();
    reverse.register(mana_schema()).unwrap();

    assert_eq!(
        serde_json::to_string(&forward.build().unwrap()).unwrap(),
        serde_json::to_string(&reverse.build().unwrap()).unwrap(),
    );
}
