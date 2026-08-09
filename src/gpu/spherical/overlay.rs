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
    use super::{
        prepare_globe_overlay_instances, prepare_map_overlay_instances, scalar_edge_style,
        scalar_edge_width, EDGE_KIND, VECTOR_KIND,
    };
    use crate::app::{build_spherical_presentation_candidate, SphericalPresentationCandidate};
    use crate::engine::MemoryStageCache;
    use crate::view::{
        built_in_palette, category_color, sample_palette, scalar_color, DisplayRevisionClock,
        PaletteId, PreparedEdgeField, PreparedFieldKind, PreparedSphericalOverlay,
        PreparedVectorGlyphs, ResolvedDisplayRange, SphericalFieldDisplayState,
    };
    use crate::world::natural::{
        boundary_kind_field_id, boundary_strength_field_id,
        preliminary_prevailing_wind_m_s_field_id, surface_elevation_m_field_id, GeologicSpec,
        TectonicSpec, WorldFormationSpec,
    };
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

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

    #[test]
    fn formal_edge_and_vector_gpu_instances_exhaustively_preserve_prepared_semantics() {
        let mut cache = MemoryStageCache::new();
        for overlay in [boundary_strength_field_id(), boundary_kind_field_id()] {
            let candidate = formal_candidate(overlay, &mut cache);
            let field = match candidate.layers().overlay().unwrap() {
                PreparedSphericalOverlay::Edge(field) => field,
                PreparedSphericalOverlay::Vector(_) => panic!("expected a formal edge field"),
            };
            assert_formal_edge_instances(&candidate, field);
        }

        let candidate = formal_candidate(preliminary_prevailing_wind_m_s_field_id(), &mut cache);
        assert_formal_vector_instances(&candidate);
    }

    fn formal_candidate(
        overlay: crate::world::fields::FieldId,
        cache: &mut MemoryStageCache,
    ) -> SphericalPresentationCandidate {
        let mut state = SphericalFieldDisplayState::default();
        state.select_fill(surface_elevation_m_field_id());
        state.select_overlay(Some(overlay));
        build_spherical_presentation_candidate(
            RootSeed::new(0x60_01),
            &SphericalSpaceSpec {
                radius: Meters::new(6_371_000.0).unwrap(),
                target_cell_count: 162,
            },
            &WorldFormationSpec::default(),
            &TectonicSpec::default(),
            &GeologicSpec::default(),
            cache,
            &state,
            &DisplayRevisionClock::default(),
        )
        .unwrap()
    }

    fn assert_formal_edge_instances(
        candidate: &SphericalPresentationCandidate,
        field: &PreparedEdgeField,
    ) {
        let palette = candidate.layers().overlay_palette().unwrap();
        let expected_map = candidate
            .map()
            .edge_segments()
            .iter()
            .filter_map(|segment| {
                expected_edge_style(field, segment.edge().raw() as usize, palette)
                    .map(|style| (segment, style))
            })
            .collect::<Vec<_>>();
        let actual_map =
            prepare_map_overlay_instances(candidate.map(), candidate.globe(), candidate.layers())
                .unwrap();
        assert!(actual_map.vector_diagnostics().is_empty());
        assert_eq!(actual_map.instances.len(), expected_map.len());
        for (actual, (segment, (width, color))) in actual_map.instances.iter().zip(expected_map) {
            assert_eq!(
                actual.start.map(f32::to_bits),
                [
                    (segment.start().x() as f32).to_bits(),
                    (segment.start().y() as f32).to_bits(),
                ]
            );
            assert_eq!(
                actual.end.map(f32::to_bits),
                [
                    (segment.end().x() as f32).to_bits(),
                    (segment.end().y() as f32).to_bits(),
                ]
            );
            assert_eq!(actual.color.map(f32::to_bits), color.map(f32::to_bits));
            assert_eq!(actual.width.to_bits(), width.to_bits());
            assert_eq!(actual.kind, EDGE_KIND);
            assert_eq!(actual.padding, [0; 2]);
        }

        let expected_globe = candidate
            .globe()
            .edge_segments()
            .iter()
            .filter_map(|segment| {
                expected_edge_style(field, segment.edge().raw() as usize, palette)
                    .map(|style| (segment, style))
            })
            .collect::<Vec<_>>();
        let actual_globe =
            prepare_globe_overlay_instances(candidate.globe(), candidate.layers()).unwrap();
        assert_eq!(actual_globe.instances.len(), expected_globe.len());
        for (actual, (segment, (width, color))) in actual_globe.instances.iter().zip(expected_globe)
        {
            assert_eq!(
                actual.start.map(f32::to_bits),
                segment.start().map(f32::to_bits)
            );
            assert_eq!(
                actual.end_or_direction.map(f32::to_bits),
                segment.end().map(f32::to_bits)
            );
            assert_eq!(actual.color.map(f32::to_bits), color.map(f32::to_bits));
            assert_eq!(actual.width.to_bits(), width.to_bits());
            assert_eq!(actual.length.to_bits(), 0.0_f32.to_bits());
            assert_eq!(actual.kind, EDGE_KIND);
            assert_eq!(actual.padding, [0; 3]);
        }
    }

    fn expected_edge_style(
        field: &PreparedEdgeField,
        edge: usize,
        palette: &[crate::view::LinearRgba],
    ) -> Option<(f32, [f32; 4])> {
        let raw = field.raw_values()[edge];
        match field.kind() {
            PreparedFieldKind::Scalar => {
                let value = f32::from_bits(raw);
                if value == 0.0 {
                    return None;
                }
                let range = field.display_range().unwrap();
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
                    ((value.abs() - min_magnitude) / (max_magnitude - min_magnitude))
                        .clamp(0.0, 1.0)
                };
                Some((
                    1.0 + 3.0 * normalized,
                    scalar_color(value, range, palette).components(),
                ))
            }
            PreparedFieldKind::Category => (field.category_keys()[raw as usize] != 0)
                .then(|| (2.0, category_color(raw, palette).components())),
        }
    }

    fn assert_formal_vector_instances(candidate: &SphericalPresentationCandidate) {
        let field = match candidate.layers().overlay().unwrap() {
            PreparedSphericalOverlay::Vector(field) => field,
            PreparedSphericalOverlay::Edge(_) => panic!("expected a formal vector field"),
        };
        let glyphs = PreparedVectorGlyphs::build(
            candidate.source(),
            candidate.map(),
            candidate.globe(),
            field,
            candidate.layers().selected_vector_cell(),
            candidate.layers().glyph_lod_key(),
        )
        .unwrap();
        let palette = candidate.layers().overlay_palette().unwrap();

        let actual_map =
            prepare_map_overlay_instances(candidate.map(), candidate.globe(), candidate.layers())
                .unwrap();
        assert!(actual_map.vector_diagnostics().is_empty());
        assert_eq!(actual_map.instances.len(), glyphs.map().len());
        for (actual, glyph) in actual_map.instances.iter().zip(glyphs.map()) {
            let origin = glyph.origin();
            let direction = glyph.direction();
            assert_eq!(actual.start.map(f32::to_bits), origin.map(f32::to_bits));
            assert_eq!(
                actual.end.map(f32::to_bits),
                [
                    (origin[0] + direction[0] * glyph.length()).to_bits(),
                    (origin[1] + direction[1] * glyph.length()).to_bits(),
                ]
            );
            assert_eq!(
                actual.color.map(f32::to_bits),
                sample_palette(palette, glyph.color_position())
                    .components()
                    .map(f32::to_bits)
            );
            assert_eq!(actual.width.to_bits(), 2.0_f32.to_bits());
            assert_eq!(actual.kind, VECTOR_KIND);
            assert_eq!(actual.padding, [0; 2]);
        }

        let actual_globe =
            prepare_globe_overlay_instances(candidate.globe(), candidate.layers()).unwrap();
        assert_eq!(actual_globe.instances.len(), glyphs.globe().len());
        for (actual, glyph) in actual_globe.instances.iter().zip(glyphs.globe()) {
            assert_eq!(
                actual.start.map(f32::to_bits),
                glyph
                    .radial()
                    .components()
                    .map(|value| (value as f32).to_bits())
            );
            assert_eq!(
                actual.end_or_direction.map(f32::to_bits),
                glyph.direction().map(f32::to_bits)
            );
            assert_eq!(actual.length.to_bits(), glyph.length().to_bits());
            assert_eq!(
                actual.color.map(f32::to_bits),
                sample_palette(palette, glyph.color_position())
                    .components()
                    .map(f32::to_bits)
            );
            assert_eq!(actual.width.to_bits(), 2.0_f32.to_bits());
            assert_eq!(actual.kind, VECTOR_KIND);
            assert_eq!(actual.padding, [0; 3]);
        }
    }
}
