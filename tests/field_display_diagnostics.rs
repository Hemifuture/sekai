use std::collections::BTreeMap;

use sekai::view::{
    format_field_value, CellDiagnosticRef, DiagnosticScope, DisplayPrepareError, DisplayRangeMode,
    FieldCatalog, FieldDisplayState, OwnedViewDiagnostic, PaletteId, PreparedDiagnosticMask,
    ViewDiagnosticSeverity,
};
use sekai::world::fields::{
    DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain, FieldId,
    FieldPaletteHint, FieldRegistry, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
    MissingValuePolicy, ValueRange,
};
use sekai::world::CellId;

fn field_id(name: &str) -> FieldId {
    FieldId::new("test.inspect", name, 1).unwrap()
}

fn diagnostic<'a>(
    severity: ViewDiagnosticSeverity,
    field_id: Option<&'a FieldId>,
    cell: Option<u32>,
) -> CellDiagnosticRef<'a> {
    CellDiagnosticRef {
        severity,
        code: "test.issue",
        field_id,
        cell_id: cell.map(CellId::from_raw),
        message: "fixture diagnostic",
    }
}

fn scalar_schema(
    name: &str,
    range: Option<(f32, f32)>,
    palette: FieldPaletteHint,
    decimals: u8,
    unit: FieldUnit,
) -> FieldSchema {
    FieldSchema {
        id: field_id(name),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::ScalarF32,
        unit,
        valid_range: range.map(|(min, max)| ValueRange::new(min, max).unwrap()),
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new(format!("field.test.{name}"), palette, decimals)
            .unwrap(),
    }
}

fn category_schema(name: &str) -> FieldSchema {
    FieldSchema {
        id: field_id(name),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::CategoryU32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::from([(7, "biome.temperate_forest".into())]),
        display: FieldDisplayMetadata::new(
            format!("field.test.{name}"),
            FieldPaletteHint::Categorical,
            0,
        )
        .unwrap(),
    }
}

fn fixture_with_renderable_fields() -> (FieldRegistry, ExtensionFieldSet) {
    let schemas = [
        scalar_schema(
            "elevation",
            Some((0.0, 100.0)),
            FieldPaletteHint::Sequential,
            1,
            FieldUnit::Unitless,
        ),
        scalar_schema(
            "rainfall",
            None,
            FieldPaletteHint::Diverging,
            2,
            FieldUnit::Unitless,
        ),
    ];
    let mut builder = FieldRegistryBuilder::new();
    for schema in schemas {
        builder.register(schema).unwrap();
    }
    let registry = builder.build().unwrap();
    let sizes = DomainSizes::new(4, 0);
    let mut fields = ExtensionFieldSet::new();
    fields
        .insert(
            &registry,
            field_id("elevation"),
            FieldData::ScalarF32(vec![0.0, 25.0, 50.0, 100.0]),
            &sizes,
        )
        .unwrap();
    fields
        .insert(
            &registry,
            field_id("rainfall"),
            FieldData::ScalarF32(vec![-2.0, -1.0, 1.0, 2.0]),
            &sizes,
        )
        .unwrap();
    (registry, fields)
}

#[test]
fn diagnostic_mask_keeps_highest_severity_and_respects_field_scope() {
    let selected = field_id("elevation");
    let other = field_id("rainfall");
    let diagnostics = [
        diagnostic(ViewDiagnosticSeverity::Info, Some(&selected), Some(0)),
        diagnostic(ViewDiagnosticSeverity::Warning, Some(&selected), Some(0)),
        diagnostic(ViewDiagnosticSeverity::Error, Some(&other), Some(1)),
        diagnostic(ViewDiagnosticSeverity::Warning, None, Some(2)),
    ];

    let selected_mask = PreparedDiagnosticMask::build(
        3,
        diagnostics,
        Some(&selected),
        DiagnosticScope::SelectedField,
    )
    .unwrap();
    assert_eq!(selected_mask.cells(), &[2, 0, 2]);

    let all_mask =
        PreparedDiagnosticMask::build(3, diagnostics, Some(&selected), DiagnosticScope::AllFields)
            .unwrap();
    assert_eq!(all_mask.cells(), &[2, 3, 2]);
}

#[test]
fn diagnostic_mask_rejects_out_of_range_cells_even_when_scope_would_hide_them() {
    let selected = field_id("elevation");
    let other = field_id("rainfall");
    let diagnostics = [diagnostic(
        ViewDiagnosticSeverity::Error,
        Some(&other),
        Some(3),
    )];

    assert_eq!(
        PreparedDiagnosticMask::build(
            3,
            diagnostics,
            Some(&selected),
            DiagnosticScope::SelectedField,
        ),
        Err(DisplayPrepareError::DiagnosticCellOutOfRange {
            cell: CellId::from_raw(3),
            cell_count: 3,
        })
    );
}

#[test]
fn owned_diagnostics_borrow_without_losing_identity() {
    let owned = OwnedViewDiagnostic {
        severity: ViewDiagnosticSeverity::Warning,
        code: "field.suspicious".into(),
        field_id: Some(field_id("elevation")),
        cell_id: Some(CellId::from_raw(2)),
        message: "suspicious value".into(),
    };

    let borrowed = owned.as_ref();
    assert_eq!(borrowed.severity, ViewDiagnosticSeverity::Warning);
    assert_eq!(borrowed.code, "field.suspicious");
    assert_eq!(borrowed.field_id, owned.field_id.as_ref());
    assert_eq!(borrowed.cell_id, Some(CellId::from_raw(2)));
    assert_eq!(borrowed.message, "suspicious value");
}

#[test]
fn state_reconciliation_uses_first_renderable_field_and_clears_invalid_cell() {
    let (registry, fields) = fixture_with_renderable_fields();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
    let mut state = FieldDisplayState::default();
    state.select_field(field_id("removed"));
    state.select_cell(Some(CellId::from_raw(99)));

    state.reconcile(&catalog, 4);

    assert_eq!(
        state.selected_field(),
        Some(&catalog.first_renderable().unwrap().schema().id)
    );
    assert_eq!(state.selected_cell(), None);
    assert_eq!(state.range_mode(), DisplayRangeMode::Schema);
}

#[test]
fn state_preserves_user_range_until_selection_changes_and_clears_wrong_palette() {
    let (registry, fields) = fixture_with_renderable_fields();
    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
    let mut state = FieldDisplayState::default();
    state.reconcile(&catalog, 4);
    state.set_range_mode(DisplayRangeMode::Manual(
        ValueRange::new(10.0, 20.0).unwrap(),
    ));
    state.set_palette_override(Some(PaletteId::Diverging));

    state.reconcile(&catalog, 4);
    assert_eq!(
        state.range_mode(),
        DisplayRangeMode::Manual(ValueRange::new(10.0, 20.0).unwrap())
    );
    assert_eq!(state.palette_override(), None);

    state.select_field(field_id("rainfall"));
    state.reconcile(&catalog, 4);
    assert_eq!(state.range_mode(), DisplayRangeMode::Data);
}

#[test]
fn inspector_uses_schema_precision_units_and_category_labels() {
    let scalar_schema = scalar_schema(
        "distance",
        None,
        FieldPaletteHint::Sequential,
        2,
        FieldUnit::Custom {
            namespace: "test.units".into(),
            name: "meter".into(),
            symbol: "m".into(),
        },
    );
    let scalar_data = FieldData::ScalarF32(vec![12.3456]);
    let scalar = sekai::view::FieldView::new(&scalar_schema, &scalar_data).unwrap();
    let formatted = format_field_value(&scalar, 0).unwrap();
    assert_eq!(formatted.text, "12.35");
    assert_eq!(formatted.unit, "m");
    assert_eq!(formatted.category_label_key, None);

    let category_schema = category_schema("biome");
    let category_data = FieldData::CategoryU32(vec![7]);
    let category = sekai::view::FieldView::new(&category_schema, &category_data).unwrap();
    let formatted = format_field_value(&category, 0).unwrap();
    assert_eq!(formatted.text, "7");
    assert_eq!(
        formatted.category_label_key.as_deref(),
        Some("biome.temperate_forest")
    );
    assert!(format_field_value(&category, 1).is_none());
}

#[test]
fn unsupported_inspection_selection_is_orthogonal_to_rendered_field_selection() {
    let (mut registry, mut fields) = fixture_with_renderable_fields();
    let vector_id = field_id("wind");
    let vector_schema = FieldSchema {
        id: vector_id.clone(),
        domain: FieldDomain::Cells,
        value_type: FieldValueType::Vector2F32,
        unit: FieldUnit::Unitless,
        valid_range: None,
        missing: MissingValuePolicy::Forbidden,
        dependencies: Vec::new(),
        category_labels: BTreeMap::new(),
        display: FieldDisplayMetadata::new("field.test.wind", FieldPaletteHint::Vector, 1).unwrap(),
    };
    let mut builder = FieldRegistryBuilder::new();
    for (_, schema) in registry.iter() {
        builder.register(schema.clone()).unwrap();
    }
    builder.register(vector_schema).unwrap();
    registry = builder.build().unwrap();
    fields
        .insert(
            &registry,
            vector_id.clone(),
            FieldData::Vector2F32(vec![[1.0, 0.0]; 4]),
            &DomainSizes::new(4, 0),
        )
        .unwrap();

    let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
    let mut state = FieldDisplayState::default();
    state.reconcile(&catalog, 4);
    let rendered = state.selected_field().cloned();
    state.inspect_field(vector_id.clone());
    state.reconcile(&catalog, 4);

    assert_eq!(state.selected_field(), rendered.as_ref());
    assert_eq!(state.inspected_field(), Some(&vector_id));
}
