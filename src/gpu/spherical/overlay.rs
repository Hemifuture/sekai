use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use crate::view::{
    category_color, prepare_globe_vector_glyphs, prepare_map_vector_glyphs, sample_palette,
    scalar_color, FieldLayerError, OwnedViewDiagnostic, PreparedFieldKind, PreparedFieldLayers,
    PreparedGlobeMesh, PreparedProjectedMap, PreparedSphericalOverlay, ResolvedDisplayRange,
};

const EDGE_KIND: u32 = 0;
const VECTOR_KIND: u32 = 1;

#[cfg(test)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct OverlayPreparationCounts {
    pub(super) map: u64,
    pub(super) globe: u64,
}

#[cfg(test)]
thread_local! {
    static PREPARATION_COUNTS: std::cell::Cell<OverlayPreparationCounts> =
        std::cell::Cell::new(OverlayPreparationCounts::default());
}

#[cfg(test)]
pub(super) fn reset_overlay_preparation_counts() {
    PREPARATION_COUNTS.set(OverlayPreparationCounts::default());
}

#[cfg(test)]
pub(super) fn overlay_preparation_counts() -> OverlayPreparationCounts {
    PREPARATION_COUNTS.get()
}

#[cfg(test)]
fn record_map_preparation() {
    PREPARATION_COUNTS.with(|slot| {
        let mut counts = slot.get();
        counts.map += 1;
        slot.set(counts);
    });
}

#[cfg(test)]
fn record_globe_preparation() {
    PREPARATION_COUNTS.with(|slot| {
        let mut counts = slot.get();
        counts.globe += 1;
        slot.set(counts);
    });
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct GpuMapOverlayInstance {
    pub(super) start: [f32; 2],
    pub(super) end: [f32; 2],
    pub(super) color: [f32; 4],
    pub(super) width: f32,
    pub(super) kind: u32,
    pub(super) padding: [u32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct GpuGlobeOverlayInstance {
    pub(super) start: [f32; 3],
    pub(super) width: f32,
    pub(super) end_or_direction: [f32; 3],
    pub(super) length: f32,
    pub(super) color: [f32; 4],
    pub(super) kind: u32,
    pub(super) padding: [u32; 3],
}

pub(super) struct PreparedMapOverlayInstances {
    pub(super) instances: Arc<[GpuMapOverlayInstance]>,
    vector_diagnostics: Arc<[OwnedViewDiagnostic]>,
}

impl PreparedMapOverlayInstances {
    pub(super) fn vector_diagnostics(&self) -> &[OwnedViewDiagnostic] {
        &self.vector_diagnostics
    }
}

pub(super) struct PreparedGlobeOverlayInstances {
    pub(super) instances: Arc<[GpuGlobeOverlayInstance]>,
}

pub(super) fn prepare_map_overlay_instances(
    map: &PreparedProjectedMap,
    globe: &PreparedGlobeMesh,
    layers: &PreparedFieldLayers,
) -> Result<PreparedMapOverlayInstances, FieldLayerError> {
    #[cfg(test)]
    record_map_preparation();
    let Some(overlay) = layers.overlay() else {
        return Ok(PreparedMapOverlayInstances {
            instances: Arc::from([]),
            vector_diagnostics: Arc::from([]),
        });
    };
    let palette = layers
        .overlay_palette()
        .expect("prepared overlays always retain their selected palette");
    match overlay {
        PreparedSphericalOverlay::Edge(field) => {
            let mut map_instances = Vec::new();
            for segment in map.edge_segments() {
                let edge = segment.edge().raw() as usize;
                let Some((width, color)) = edge_style(field, edge, palette) else {
                    continue;
                };
                map_instances.push(GpuMapOverlayInstance {
                    start: [segment.start().x() as f32, segment.start().y() as f32],
                    end: [segment.end().x() as f32, segment.end().y() as f32],
                    color,
                    width,
                    kind: EDGE_KIND,
                    padding: [0; 2],
                });
            }
            Ok(PreparedMapOverlayInstances {
                instances: Arc::from(map_instances),
                vector_diagnostics: Arc::from([]),
            })
        }
        PreparedSphericalOverlay::Vector(field) => {
            let (glyphs, diagnostics) = prepare_map_vector_glyphs(
                layers.source(),
                map,
                globe,
                field,
                layers.selected_vector_cell(),
                layers.glyph_lod_key(),
            )?;
            let map_instances = glyphs
                .iter()
                .map(|glyph| {
                    let origin = glyph.origin();
                    let direction = glyph.direction();
                    GpuMapOverlayInstance {
                        start: origin,
                        end: [
                            origin[0] + direction[0] * glyph.length(),
                            origin[1] + direction[1] * glyph.length(),
                        ],
                        color: sample_palette(palette, glyph.color_position()).components(),
                        width: 2.0,
                        kind: VECTOR_KIND,
                        padding: [0; 2],
                    }
                })
                .collect::<Vec<_>>();
            Ok(PreparedMapOverlayInstances {
                instances: Arc::from(map_instances),
                vector_diagnostics: Arc::from(diagnostics),
            })
        }
    }
}

pub(super) fn prepare_globe_overlay_instances(
    globe: &PreparedGlobeMesh,
    layers: &PreparedFieldLayers,
) -> Result<PreparedGlobeOverlayInstances, FieldLayerError> {
    #[cfg(test)]
    record_globe_preparation();
    let Some(overlay) = layers.overlay() else {
        return Ok(PreparedGlobeOverlayInstances {
            instances: Arc::from([]),
        });
    };
    let palette = layers
        .overlay_palette()
        .expect("prepared overlays always retain their selected palette");
    match overlay {
        PreparedSphericalOverlay::Edge(field) => {
            let mut globe_instances = Vec::new();
            for segment in globe.edge_segments() {
                let edge = segment.edge().raw() as usize;
                let Some((width, color)) = edge_style(field, edge, palette) else {
                    continue;
                };
                globe_instances.push(GpuGlobeOverlayInstance {
                    start: segment.start(),
                    width,
                    end_or_direction: segment.end(),
                    length: 0.0,
                    color,
                    kind: EDGE_KIND,
                    padding: [0; 3],
                });
            }
            Ok(PreparedGlobeOverlayInstances {
                instances: Arc::from(globe_instances),
            })
        }
        PreparedSphericalOverlay::Vector(field) => {
            let glyphs = prepare_globe_vector_glyphs(
                layers.source(),
                globe,
                field,
                layers.selected_vector_cell(),
                layers.glyph_lod_key(),
            )?;
            let globe_instances = glyphs
                .iter()
                .map(|glyph| GpuGlobeOverlayInstance {
                    start: glyph
                        .radial()
                        .components()
                        .map(|component| component as f32),
                    width: 2.0,
                    end_or_direction: glyph.direction(),
                    length: glyph.length(),
                    color: sample_palette(palette, glyph.color_position()).components(),
                    kind: VECTOR_KIND,
                    padding: [0; 3],
                })
                .collect::<Vec<_>>();
            Ok(PreparedGlobeOverlayInstances {
                instances: Arc::from(globe_instances),
            })
        }
    }
}

fn edge_style(
    field: &crate::view::PreparedEdgeField,
    index: usize,
    palette: &[crate::view::LinearRgba],
) -> Option<(f32, [f32; 4])> {
    let raw = *field.raw_values().get(index)?;
    match field.kind() {
        PreparedFieldKind::Scalar => {
            let value = f32::from_bits(raw);
            if value == 0.0 {
                return None;
            }
            let range = field
                .display_range()
                .expect("prepared scalar edge fields retain a display range");
            Some(scalar_edge_style(value, range, palette))
        }
        PreparedFieldKind::Category => {
            let key = *field.category_keys().get(raw as usize)?;
            if key == 0 {
                return None;
            }
            Some((2.0, category_color(raw, palette).components()))
        }
    }
}

fn scalar_edge_style(
    value: f32,
    range: ResolvedDisplayRange,
    palette: &[crate::view::LinearRgba],
) -> (f32, [f32; 4]) {
    (
        scalar_edge_width(value, range),
        scalar_color(value, range, palette).components(),
    )
}

fn scalar_edge_width(value: f32, range: ResolvedDisplayRange) -> f32 {
    let (min, max) = range.bounds();
    let max_magnitude = min.abs().max(max.abs());
    let min_magnitude = if min <= 0.0 && max >= 0.0 {
        0.0
    } else {
        min.abs().min(max.abs())
    };
    let normalized = if max_magnitude == min_magnitude {
        0.5
    } else {
        ((value.abs() - min_magnitude) / (max_magnitude - min_magnitude)).clamp(0.0, 1.0)
    };
    1.0 + 3.0 * normalized
}

#[cfg(test)]
mod tests {
    use super::{scalar_edge_style, scalar_edge_width};
    use crate::view::{built_in_palette, PaletteId, ResolvedDisplayRange};

    #[test]
    fn negative_only_scalar_edges_get_wider_with_absolute_magnitude() {
        let range = ResolvedDisplayRange::new(-8.0, -2.0).unwrap();

        assert_eq!(scalar_edge_width(-2.0, range), 1.0);
        assert_eq!(scalar_edge_width(-8.0, range), 4.0);
        assert!(scalar_edge_width(-6.0, range) > scalar_edge_width(-4.0, range));
    }

    #[test]
    fn symmetric_scalar_edges_give_equal_magnitudes_equal_widths() {
        let range = ResolvedDisplayRange::new(-4.0, 4.0).unwrap();

        assert_eq!(
            scalar_edge_width(-2.0, range),
            scalar_edge_width(2.0, range)
        );
        assert_eq!(scalar_edge_width(-4.0, range), 4.0);
        assert!(scalar_edge_width(3.0, range) > scalar_edge_width(-1.0, range));
    }

    #[test]
    fn symmetric_scalar_edges_keep_signed_colors_separate_from_absolute_widths() {
        let range = ResolvedDisplayRange::new(-4.0, 4.0).unwrap();
        let palette = built_in_palette(PaletteId::Diverging);

        let negative = scalar_edge_style(-2.0, range, palette);
        let positive = scalar_edge_style(2.0, range, palette);

        assert_eq!(negative.0, positive.0);
        assert_ne!(negative.1, positive.1);
    }
}
