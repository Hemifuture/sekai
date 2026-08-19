use crate::view::{
    resolve_display_range, DiagnosticScope, DisplayRangeMode, FieldCatalog, FieldCatalogEntry,
    FieldDisplayState, PaletteId,
};
use crate::world::fields::{FieldId, FieldPaletteHint, ValueRange};

use super::localization::{localized_field_key, localized_palette};

/// Declarative field-viewer changes emitted for the app composition layer.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldControlAction {
    /// Inspect any registered field without changing the rendered fill.
    InspectField(FieldId),
    /// Render one supported scalar or category cell field.
    SelectField(FieldId),
    /// Change the scalar display-range source.
    SetRangeMode(DisplayRangeMode),
    /// Select a compatible built-in palette or use the schema default.
    SetPaletteOverride(Option<PaletteId>),
    /// Enable or disable diagnostic overlays.
    SetDiagnosticsEnabled(bool),
    /// Select which field diagnostics participate in overlays.
    SetDiagnosticScope(DiagnosticScope),
}

/// Returns the built-in palettes compatible with one semantic schema hint.
pub fn compatible_palettes(hint: FieldPaletteHint) -> &'static [PaletteId] {
    match hint {
        FieldPaletteHint::Sequential => &[PaletteId::Sequential],
        FieldPaletteHint::Diverging => &[PaletteId::Diverging],
        FieldPaletteHint::Hypsometric => &[PaletteId::Hypsometric],
        FieldPaletteHint::Categorical => &[PaletteId::Categorical],
        FieldPaletteHint::LandOcean => &[PaletteId::LandOcean],
        FieldPaletteHint::Boolean | FieldPaletteHint::Vector => &[],
    }
}

/// Returns an explicit localized status for one catalog entry.
pub fn field_status_text(entry: &FieldCatalogEntry<'_>) -> &'static str {
    let Some(view) = entry.view() else {
        return "已注册，但当前快照没有字段数据";
    };
    if view.cell_fill_kind().is_ok() {
        "可用于单元格填色"
    } else {
        "V1 可检查该字段，但不支持单元格填色"
    }
}

/// Draws field, range, palette, and diagnostic controls as declarative actions.
pub fn show_field_controls(
    ui: &mut egui::Ui,
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
) -> Vec<FieldControlAction> {
    let mut actions = Vec::new();
    ui.heading("字段显示");

    for entry in catalog.entries() {
        let schema = entry.schema();
        let label = field_label(schema);
        let inspecting = state.inspected_field() == Some(&schema.id);
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(inspecting, label).clicked() {
                actions.push(FieldControlAction::InspectField(schema.id.clone()));
            }
            let renderable = entry
                .view()
                .is_some_and(|view| view.cell_fill_kind().is_ok());
            let selected = state.selected_field() == Some(&schema.id);
            if ui
                .add_enabled(
                    renderable && !selected,
                    egui::Button::new(if selected {
                        "当前填色"
                    } else {
                        "设为填色"
                    }),
                )
                .clicked()
            {
                actions.push(FieldControlAction::SelectField(schema.id.clone()));
            }
        });
        ui.small(field_status_text(entry));
    }

    let selected = state.selected_field().and_then(|id| catalog.get(id));
    let Some(selected) = selected else {
        return actions;
    };
    let schema = selected.schema();

    ui.separator();
    ui.label("标量范围");
    ui.horizontal(|ui| {
        let schema_selected = state.range_mode() == DisplayRangeMode::Schema;
        if ui
            .add_enabled(
                schema.valid_range.is_some() && !schema_selected,
                egui::Button::new(if schema_selected {
                    "字段定义 ✓"
                } else {
                    "字段定义"
                }),
            )
            .clicked()
        {
            actions.push(FieldControlAction::SetRangeMode(DisplayRangeMode::Schema));
        }
        let data_selected = state.range_mode() == DisplayRangeMode::Data;
        if ui
            .add_enabled(
                !data_selected,
                egui::Button::new(if data_selected {
                    "数据范围 ✓"
                } else {
                    "数据范围"
                }),
            )
            .clicked()
        {
            actions.push(FieldControlAction::SetRangeMode(DisplayRangeMode::Data));
        }
    });

    if let Some(view) = selected
        .view()
        .filter(|view| view.scalar_values().is_some())
    {
        let fallback = resolve_display_range(view, DisplayRangeMode::Data)
            .ok()
            .map_or((0.0, 1.0), |range| range.bounds());
        let (mut min, mut max) = match state.range_mode() {
            DisplayRangeMode::Manual(range) => (range.min(), range.max()),
            _ => fallback,
        };
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("手动:");
            changed |= ui.add(egui::DragValue::new(&mut min).speed(0.1)).changed();
            changed |= ui.add(egui::DragValue::new(&mut max).speed(0.1)).changed();
        });
        if changed {
            if let Ok(range) = ValueRange::new(min, max) {
                actions.push(FieldControlAction::SetRangeMode(DisplayRangeMode::Manual(
                    range,
                )));
            }
        }
    }

    let palettes = compatible_palettes(schema.display.palette());
    if !palettes.is_empty() {
        ui.label("调色板");
        ui.horizontal(|ui| {
            if ui
                .selectable_label(state.palette_override().is_none(), "字段默认")
                .clicked()
            {
                actions.push(FieldControlAction::SetPaletteOverride(None));
            }
            for palette in palettes {
                if ui
                    .selectable_label(
                        state.palette_override() == Some(*palette),
                        localized_palette(*palette),
                    )
                    .clicked()
                {
                    actions.push(FieldControlAction::SetPaletteOverride(Some(*palette)));
                }
            }
        });
    }

    ui.separator();
    let mut diagnostics_enabled = state.diagnostics_enabled();
    if ui
        .checkbox(&mut diagnostics_enabled, "显示诊断覆盖")
        .changed()
    {
        actions.push(FieldControlAction::SetDiagnosticsEnabled(
            diagnostics_enabled,
        ));
    }
    let mut scope = state.diagnostic_scope();
    ui.horizontal(|ui| {
        if ui
            .radio_value(&mut scope, DiagnosticScope::SelectedField, "当前字段")
            .changed()
            || ui
                .radio_value(&mut scope, DiagnosticScope::AllFields, "全部字段")
                .changed()
        {
            actions.push(FieldControlAction::SetDiagnosticScope(scope));
        }
    });

    actions
}

fn field_label(schema: &crate::world::fields::FieldSchema) -> String {
    let label = schema.display.label_key();
    if label.is_empty() {
        format!(
            "{}.{}@{}",
            schema.id.namespace(),
            schema.id.name(),
            schema.id.version()
        )
    } else {
        localized_field_key(label).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{compatible_palettes, field_status_text, show_field_controls};
    use crate::ui::field::localization::{
        localized_domain, localized_field_key, localized_palette, localized_value_type,
    };
    use crate::ui::field::show_field_inspector;
    use crate::view::{FieldCatalog, FieldDisplayState, PaletteId};
    use crate::world::fields::{
        DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain, FieldId,
        FieldPaletteHint, FieldRegistry, FieldRegistryBuilder, FieldSchema, FieldUnit,
        FieldValueType, MissingValuePolicy,
    };
    use crate::world::natural::natural_field_registry;

    fn field_id(name: &str) -> FieldId {
        FieldId::new("test.ui", name, 1).unwrap()
    }

    fn schema(name: &str, value_type: FieldValueType) -> FieldSchema {
        let (palette, labels) = match value_type {
            FieldValueType::ScalarF32 => (FieldPaletteHint::Sequential, BTreeMap::new()),
            FieldValueType::Vector2F32 => (FieldPaletteHint::Vector, BTreeMap::new()),
            FieldValueType::CategoryU32 => (
                FieldPaletteHint::Categorical,
                BTreeMap::from([(1, "field.test.category.one".into())]),
            ),
            _ => unreachable!("fixture uses scalar, vector, and category"),
        };
        FieldSchema {
            id: field_id(name),
            domain: FieldDomain::Cells,
            value_type,
            unit: FieldUnit::Unitless,
            valid_range: None,
            missing: MissingValuePolicy::Forbidden,
            dependencies: Vec::new(),
            category_labels: labels,
            display: FieldDisplayMetadata::new(format!("field.test.{name}"), palette, 1).unwrap(),
        }
    }

    fn fixture() -> (FieldRegistry, ExtensionFieldSet) {
        let mut builder = FieldRegistryBuilder::new();
        for schema in [
            schema("scalar", FieldValueType::ScalarF32),
            schema("vector", FieldValueType::Vector2F32),
            schema("missing", FieldValueType::CategoryU32),
        ] {
            builder.register(schema).unwrap();
        }
        let registry = builder.build().unwrap();
        let sizes = DomainSizes::new(4, 0);
        let mut fields = ExtensionFieldSet::new();
        fields
            .insert(
                &registry,
                field_id("scalar"),
                FieldData::ScalarF32(vec![0.0, 0.25, 0.5, 1.0]),
                &sizes,
            )
            .unwrap();
        fields
            .insert(
                &registry,
                field_id("vector"),
                FieldData::Vector2F32(vec![[1.0, 0.0]; 4]),
                &sizes,
            )
            .unwrap();
        (registry, fields)
    }

    #[test]
    fn palette_choices_match_field_schema_hint() {
        assert_eq!(
            compatible_palettes(FieldPaletteHint::Sequential),
            &[PaletteId::Sequential]
        );
        assert_eq!(
            compatible_palettes(FieldPaletteHint::Diverging),
            &[PaletteId::Diverging]
        );
        assert_eq!(
            compatible_palettes(FieldPaletteHint::Categorical),
            &[PaletteId::Categorical]
        );
        assert_eq!(compatible_palettes(FieldPaletteHint::Vector), &[]);
    }

    #[test]
    fn missing_and_unsupported_fields_have_explicit_status_text() {
        let (registry, fields) = fixture();
        let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
        assert_eq!(
            field_status_text(catalog.get(&field_id("missing")).unwrap()),
            "已注册，但当前快照没有字段数据"
        );
        assert_eq!(
            field_status_text(catalog.get(&field_id("vector")).unwrap()),
            "V1 可检查该字段，但不支持单元格填色"
        );
        assert_eq!(
            field_status_text(catalog.get(&field_id("scalar")).unwrap()),
            "可用于单元格填色"
        );
    }

    #[test]
    fn controls_and_inspector_render_fixture_without_mutating_fields() {
        let (registry, fields) = fixture();
        let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
        let mut state = FieldDisplayState::default();
        state.reconcile(&catalog, 4);
        let before = serde_json::to_vec(&fields).unwrap();
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let actions = show_field_controls(ui, &catalog, &state);
                assert!(actions.is_empty());
                show_field_inspector(ui, &catalog, &state, &[]);
            });
        });
        assert_eq!(serde_json::to_vec(&fields).unwrap(), before);
    }

    #[test]
    fn formal_natural_field_and_category_keys_have_chinese_labels() {
        assert_eq!(
            localized_field_key("field.sekai.core.natural.surface_elevation_m"),
            "当前地表高程"
        );
        assert_eq!(
            localized_field_key("field.sekai.core.natural.boundary_kind.subduction"),
            "俯冲"
        );
        assert_eq!(
            localized_field_key("field.sekai.core.natural.plate_id.plate-03"),
            "板块 03"
        );
        assert_eq!(
            localized_field_key("field.sekai.core.natural.strahler_stream_order.order-004"),
            "4 级河流"
        );
    }

    #[test]
    fn unknown_extension_label_keys_remain_inspectable() {
        assert_eq!(
            localized_field_key("field.example.magic_flux"),
            "field.example.magic_flux"
        );
        assert_eq!(
            localized_field_key("field.sekai.core.natural.plate_id.plate-bad"),
            "field.sekai.core.natural.plate_id.plate-bad"
        );
    }

    #[test]
    fn every_formal_natural_field_and_category_has_a_chinese_label() {
        let registry = natural_field_registry(12).unwrap();
        for (_, schema) in registry.iter() {
            let key = schema.display.label_key();
            assert_ne!(localized_field_key(key), key, "missing label for {key}");
            for category_key in schema.category_labels.values() {
                assert_ne!(
                    localized_field_key(category_key),
                    category_key.as_str(),
                    "missing label for {category_key}"
                );
            }
        }
    }

    #[test]
    fn schema_shape_and_palette_labels_are_chinese() {
        assert_eq!(localized_domain(FieldDomain::Global), "全局");
        assert_eq!(localized_domain(FieldDomain::Cells), "单元格");
        assert_eq!(localized_domain(FieldDomain::Edges), "边");
        assert_eq!(localized_value_type(FieldValueType::ScalarF32), "标量");
        assert_eq!(localized_value_type(FieldValueType::CategoryU32), "分类");
        assert_eq!(localized_value_type(FieldValueType::Vector2F32), "二维向量");
        assert_eq!(localized_palette(PaletteId::Sequential), "顺序");
        assert_eq!(localized_palette(PaletteId::Diverging), "发散");
        assert_eq!(localized_palette(PaletteId::Categorical), "分类");
    }
}
