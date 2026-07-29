use std::collections::BTreeMap;

use sekai::view::{
    built_in_palette, category_color, prepare_cell_field, resolve_display_range, sample_palette,
    scalar_color, DisplayPrepareError, DisplayRangeMode, FieldView, LinearRgba, PaletteId,
    PreparedFieldKind, ResolvedDisplayRange,
};
use sekai::world::fields::{
    DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain, FieldId,
    FieldPaletteHint, FieldRegistry, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
    MissingValuePolicy, ValueRange,
};

struct FieldFixture {
    registry: FieldRegistry,
    fields: ExtensionFieldSet,
    id: FieldId,
}

impl FieldFixture {
    fn view(&self) -> FieldView<'_> {
        FieldView::new(
            self.registry.get(&self.id).unwrap(),
            self.fields.get(&self.id).unwrap(),
        )
        .unwrap()
    }
}

fn field_id(name: &str) -> FieldId {
    FieldId::new("test.palette", name, 1).unwrap()
}

fn scalar_schema(name: &str, range: Option<(f32, f32)>) -> FieldSchema {
    FieldSchema {
        id: field_id(name),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::ScalarF32,
        unit: FieldUnit::Unitless,
        valid_range: range.map(|(min, max)| ValueRange::new(min, max).unwrap()),
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new(
            format!("field.test.{name}"),
            FieldPaletteHint::Sequential,
            2,
        )
        .unwrap(),
    }
}

fn scalar_fixture(values: &[f32], range: Option<(f32, f32)>) -> FieldFixture {
    let schema = scalar_schema("scalar", range);
    fixture(schema, FieldData::ScalarF32(values.to_vec()))
}

fn category_fixture(values: &[u32], labels: &[(u32, &str)]) -> FieldFixture {
    let schema = FieldSchema {
        id: field_id("category"),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::CategoryU32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: labels
            .iter()
            .map(|(key, label)| (*key, (*label).to_owned()))
            .collect(),
        display: FieldDisplayMetadata::new("field.test.category", FieldPaletteHint::Categorical, 0)
            .unwrap(),
    };
    fixture(schema, FieldData::CategoryU32(values.to_vec()))
}

fn vector_fixture(values: &[[f32; 2]]) -> FieldFixture {
    let schema = FieldSchema {
        id: field_id("vector"),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::Vector2F32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new("field.test.vector", FieldPaletteHint::Vector, 2)
            .unwrap(),
    };
    fixture(schema, FieldData::Vector2F32(values.to_vec()))
}

fn fixture(schema: FieldSchema, data: FieldData) -> FieldFixture {
    let id = schema.id.clone();
    let value_count = data.len();
    let mut builder = FieldRegistryBuilder::new();
    builder.register(schema).unwrap();
    let registry = builder.build().unwrap();
    let mut fields = ExtensionFieldSet::new();
    fields
        .insert(
            &registry,
            id.clone(),
            data,
            &DomainSizes::new(value_count, 0),
        )
        .unwrap();
    FieldFixture {
        registry,
        fields,
        id,
    }
}

#[test]
fn schema_data_and_manual_ranges_resolve_explicitly() {
    let fixture = scalar_fixture(&[-2.0, 0.0, 8.0], Some((-10.0, 10.0)));
    let field = fixture.view();

    assert_eq!(
        resolve_display_range(&field, DisplayRangeMode::Schema)
            .unwrap()
            .bounds(),
        (-10.0, 10.0)
    );
    assert_eq!(
        resolve_display_range(&field, DisplayRangeMode::Data)
            .unwrap()
            .bounds(),
        (-2.0, 8.0)
    );
    assert_eq!(
        resolve_display_range(
            &field,
            DisplayRangeMode::Manual(ValueRange::new(-1.0, 1.0).unwrap())
        )
        .unwrap()
        .bounds(),
        (-1.0, 1.0)
    );
}

#[test]
fn invalid_and_missing_ranges_return_structured_errors() {
    assert_eq!(
        ResolvedDisplayRange::new(f32::NAN, 1.0),
        Err(DisplayPrepareError::InvalidRange)
    );
    assert_eq!(
        ResolvedDisplayRange::new(2.0, 1.0),
        Err(DisplayPrepareError::InvalidRange)
    );

    let fixture = scalar_fixture(&[1.0], None);
    assert!(matches!(
        resolve_display_range(&fixture.view(), DisplayRangeMode::Schema),
        Err(DisplayPrepareError::MissingSchemaRange { .. })
    ));

    let schema = scalar_schema("non-finite", None);
    let data = FieldData::ScalarF32(vec![f32::NAN]);
    let view = FieldView::new(&schema, &data).unwrap();
    assert!(matches!(
        resolve_display_range(&view, DisplayRangeMode::Data),
        Err(DisplayPrepareError::NoFiniteScalarValues { .. })
    ));
}

#[test]
fn palette_sampling_has_exact_endpoints_and_constant_midpoint_behavior() {
    let palette = built_in_palette(PaletteId::Sequential);
    assert!(palette.len() >= 5);
    assert_eq!(sample_palette(palette, -1.0), palette[0]);
    assert_eq!(sample_palette(palette, 2.0), palette[palette.len() - 1]);

    let range = ResolvedDisplayRange::new(4.0, 4.0).unwrap();
    assert_eq!(
        scalar_color(4.0, range, palette),
        sample_palette(palette, 0.5)
    );

    let expected_midpoint = LinearRgba::new(0.25, 0.5, 0.5, 1.0);
    assert_eq!(
        sample_palette(
            &[
                LinearRgba::new(0.0, 0.0, 0.0, 1.0),
                LinearRgba::new(0.5, 1.0, 1.0, 1.0),
            ],
            0.5,
        ),
        expected_midpoint
    );
}

#[test]
fn prepared_scalars_store_ieee_bits_and_categories_store_sorted_compact_indices() {
    let scalar_fixture = scalar_fixture(&[0.25, 0.75], Some((0.0, 1.0)));
    let scalar = prepare_cell_field(&scalar_fixture.view(), 2, DisplayRangeMode::Schema).unwrap();
    assert_eq!(scalar.kind(), PreparedFieldKind::Scalar);
    assert_eq!(
        scalar.raw_values(),
        &[0.25_f32.to_bits(), 0.75_f32.to_bits()]
    );
    assert_eq!(scalar.source_range().unwrap().bounds(), (0.25, 0.75));
    assert_eq!(scalar.display_range().unwrap().bounds(), (0.0, 1.0));

    let category_fixture = category_fixture(&[9, 4, 9], &[(4, "cat.four"), (9, "cat.nine")]);
    let category = prepare_cell_field(&category_fixture.view(), 3, DisplayRangeMode::Data).unwrap();
    assert_eq!(category.kind(), PreparedFieldKind::Category);
    assert_eq!(category.category_keys(), &[4, 9]);
    assert_eq!(category.raw_values(), &[1, 0, 1]);
    assert_eq!(
        category_color(3, built_in_palette(PaletteId::Categorical)),
        built_in_palette(PaletteId::Categorical)[3]
    );
}

#[test]
fn field_prepare_rejects_length_and_unsupported_fill_without_partial_output() {
    let scalar = scalar_fixture(&[1.0], None);
    assert_eq!(
        prepare_cell_field(&scalar.view(), 2, DisplayRangeMode::Data),
        Err(DisplayPrepareError::CellCountMismatch {
            expected: 2,
            actual: 1,
        })
    );

    let vector = vector_fixture(&[[1.0, 0.0]]);
    assert!(matches!(
        prepare_cell_field(&vector.view(), 1, DisplayRangeMode::Data),
        Err(DisplayPrepareError::UnsupportedCellFill { .. })
    ));
}
