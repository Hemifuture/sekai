use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use crate::view::{
    category_color, sample_palette, scalar_color, FieldLayerError, PreparedFieldKind,
    PreparedFieldLayers, PreparedGlobeMesh, PreparedProjectedMap, PreparedSphericalOverlay,
    PreparedVectorGlyphs,
};

const EDGE_KIND: u32 = 0;
const VECTOR_KIND: u32 = 1;

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

pub(super) struct PreparedOverlayInstances {
    pub(super) map: Vec<GpuMapOverlayInstance>,
    pub(super) globe: Vec<GpuGlobeOverlayInstance>,
    pub(super) vector_glyphs: Option<Arc<PreparedVectorGlyphs>>,
}

pub(super) fn prepare_overlay_instances(
    map: &PreparedProjectedMap,
    globe: &PreparedGlobeMesh,
    layers: &PreparedFieldLayers,
) -> Result<PreparedOverlayInstances, FieldLayerError> {
    let Some(overlay) = layers.overlay() else {
        return Ok(PreparedOverlayInstances {
            map: Vec::new(),
            globe: Vec::new(),
            vector_glyphs: None,
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
            Ok(PreparedOverlayInstances {
                map: map_instances,
                globe: globe_instances,
                vector_glyphs: None,
            })
        }
        PreparedSphericalOverlay::Vector(field) => {
            let glyphs = Arc::new(PreparedVectorGlyphs::build(
                layers.source(),
                map,
                globe,
                field,
                layers.selected_vector_cell(),
                layers.glyph_lod_key(),
            )?);
            let map_instances = glyphs
                .map()
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
                .collect();
            let globe_instances = glyphs
                .globe()
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
                .collect();
            Ok(PreparedOverlayInstances {
                map: map_instances,
                globe: globe_instances,
                vector_glyphs: Some(glyphs),
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
            let (min, max) = range.bounds();
            let normalized = if max == min {
                0.5
            } else {
                ((value - min) / (max - min)).clamp(0.0, 1.0)
            };
            Some((
                1.0 + 3.0 * normalized,
                scalar_color(value, range, palette).components(),
            ))
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
