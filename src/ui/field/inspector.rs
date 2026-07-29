use crate::view::{
    built_in_palette, category_color, format_field_value, resolve_display_range, sample_palette,
    CellDiagnosticRef, FieldCatalog, FieldDisplayState, LinearRgba, PaletteId,
    ViewDiagnosticSeverity,
};
use crate::world::fields::FieldPaletteHint;

use super::controls::field_status_text;

/// Draws schema metadata, a legend, one selected value, and matching diagnostics.
pub fn show_field_inspector(
    ui: &mut egui::Ui,
    catalog: &FieldCatalog<'_>,
    state: &FieldDisplayState,
    diagnostics: &[CellDiagnosticRef<'_>],
) {
    ui.heading("字段检查");
    let inspected = state.inspected_field().or_else(|| state.selected_field());
    let Some(entry) = inspected.and_then(|id| catalog.get(id)) else {
        ui.label("当前没有可检查字段");
        return;
    };
    let schema = entry.schema();
    ui.strong(schema.display.label_key());
    ui.monospace(format!(
        "{}.{}@{}",
        schema.id.namespace(),
        schema.id.name(),
        schema.id.version()
    ));
    ui.label(field_status_text(entry));
    ui.label(format!("域: {:?}", schema.domain));
    ui.label(format!("类型: {:?}", schema.value_type));
    if !schema.unit.symbol().is_empty() {
        ui.label(format!("单位: {}", schema.unit.symbol()));
    }
    if let Some(range) = schema.valid_range {
        ui.label(format!("Schema 范围: {} – {}", range.min(), range.max()));
    }

    let Some(view) = entry.view() else {
        return;
    };
    match schema.display.palette() {
        FieldPaletteHint::Sequential | FieldPaletteHint::Diverging
            if view.scalar_values().is_some() =>
        {
            let palette = state
                .palette_override()
                .unwrap_or_else(|| palette_for_hint(schema.display.palette()));
            if let Ok(range) = resolve_display_range(view, state.range_mode()) {
                let (min, max) = range.bounds();
                ui.label(format!("显示范围: {min} – {max}"));
                draw_scalar_legend(ui, built_in_palette(palette));
            }
        }
        FieldPaletteHint::Categorical if view.category_values().is_some() => {
            let palette = built_in_palette(PaletteId::Categorical);
            let total = schema.category_labels.len();
            for (compact, (key, label)) in schema.category_labels.iter().take(256).enumerate() {
                let color = category_color(compact as u32, palette);
                ui.horizontal(|ui| {
                    color_swatch(ui, color);
                    ui.label(format!("{key}: {label}"));
                });
            }
            if total > 256 {
                ui.label(format!("共 {total} 项，仅显示前 256 项"));
            }
        }
        _ => {}
    }

    let Some(cell) = state.selected_cell() else {
        ui.label("点击地图单元格以检查值");
        return;
    };
    ui.separator();
    ui.label(format!("Cell {}", cell.raw()));
    if let Some(value) = format_field_value(view, cell.raw() as usize) {
        let unit = if value.unit.is_empty() {
            String::new()
        } else {
            format!(" {}", value.unit)
        };
        ui.label(format!("值: {}{unit}", value.text));
        if let Some(label) = value.category_label_key {
            ui.label(format!("分类: {label}"));
        }
    } else {
        ui.label("该字段在此单元格没有值");
    }

    let mut matching: Vec<_> = diagnostics
        .iter()
        .copied()
        .filter(|diagnostic| {
            diagnostic.cell_id == Some(cell)
                && diagnostic.field_id.is_none_or(|field| field == &schema.id)
        })
        .collect();
    matching.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| left.code.cmp(right.code))
    });
    for diagnostic in matching {
        let prefix = match diagnostic.severity {
            ViewDiagnosticSeverity::Info => "Info",
            ViewDiagnosticSeverity::Warning => "Warning",
            ViewDiagnosticSeverity::Error => "Error",
        };
        ui.label(format!(
            "{prefix} · {} · {}",
            diagnostic.code, diagnostic.message
        ));
    }
}

fn palette_for_hint(hint: FieldPaletteHint) -> PaletteId {
    match hint {
        FieldPaletteHint::Diverging => PaletteId::Diverging,
        FieldPaletteHint::Categorical => PaletteId::Categorical,
        FieldPaletteHint::Sequential | FieldPaletteHint::Boolean | FieldPaletteHint::Vector => {
            PaletteId::Sequential
        }
    }
}

fn draw_scalar_legend(ui: &mut egui::Ui, palette: &[LinearRgba]) {
    ui.horizontal(|ui| {
        for step in 0..=8 {
            color_swatch(ui, sample_palette(palette, step as f32 / 8.0));
        }
    });
}

fn color_swatch(ui: &mut egui::Ui, color: LinearRgba) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 12.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 1.0, linear_color32(color));
}

fn linear_color32(color: LinearRgba) -> egui::Color32 {
    let [red, green, blue, alpha] = color.components();
    egui::Color32::from_rgba_unmultiplied(
        linear_channel_to_srgb8(red),
        linear_channel_to_srgb8(green),
        linear_channel_to_srgb8(blue),
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn linear_channel_to_srgb8(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let srgb = if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
}
