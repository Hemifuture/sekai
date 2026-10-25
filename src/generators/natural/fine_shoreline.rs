//! Fine-domain shoreline of the T1 v2.2 spec §5.1: per-leaf water
//! classification against the published P5 water bodies.
//!
//! A derived leaf is water exactly when some P5 water body `W` both owns
//! the leaf's root cell or one of its edge neighbours, and has a surface
//! above the leaf's derived elevation (ocean: the formal sea level; lake:
//! its published surface). Candidates are tried root-owner first, then by
//! ascending surface, and the first one above the leaf wins (spec §9.3
//! ruling). The shoreline is therefore the water-surface contour of the
//! same derived field the terrain colors use, confined to the bodies and
//! their one ring, so a dry cell with no water neighbour never floods.
//! This is display derivation only: P5 schema, volumes and topology are
//! untouched.

use crate::world::natural::{Lake, SurfaceWaterField, SurfaceWaterKind};
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::{CellId, LakeId};

/// One P5 water body visible from a cell: its kind and its surface.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WaterCandidate {
    kind: SurfaceWaterKind,
    surface_m: f32,
}

/// Body identity for de-duplicating ring candidates; the ocean is one body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WaterBody {
    Ocean,
    Lake(LakeId),
}

/// Per-cell ordered water candidates (compressed rows: most cells are
/// inland dry land with an empty row).
#[derive(Debug, Clone, PartialEq)]
pub struct FineShoreline {
    row_start: Vec<u32>,
    candidates: Vec<WaterCandidate>,
}

impl FineShoreline {
    /// Builds the candidate rows from the published P5 hydrology.
    ///
    /// # Arguments
    ///
    /// * `surface` - The authoritative surface the hydrology indexes.
    /// * `water` - The published P5 per-cell water classification.
    /// * `lakes` - The published P5 lakes (members and surfaces).
    /// * `sea_level_m` - The formal sea level the ocean surface uses.
    ///
    /// # Returns
    ///
    /// The classifier; a cell missing from the hydrology contributes no
    /// candidate, which leaves its leaves dry.
    pub fn new(
        surface: &SphericalSurfaceSnapshot,
        water: &SurfaceWaterField,
        lakes: &[Lake],
        sea_level_m: f32,
    ) -> Self {
        let cell_count = surface.cells().len();
        let mut lake_of = vec![None; cell_count];
        for lake in lakes {
            for cell in lake.cells() {
                if let Some(slot) = lake_of.get_mut(cell.raw() as usize) {
                    *slot = Some((lake.id(), lake.surface_elevation_m()));
                }
            }
        }
        let body_of = |cell: usize| -> Option<(WaterBody, WaterCandidate)> {
            match water.get(cell)? {
                SurfaceWaterKind::DryLand => None,
                SurfaceWaterKind::Ocean => Some((
                    WaterBody::Ocean,
                    WaterCandidate {
                        kind: SurfaceWaterKind::Ocean,
                        surface_m: sea_level_m,
                    },
                )),
                SurfaceWaterKind::Lake => lake_of[cell].map(|(id, surface_m)| {
                    (
                        WaterBody::Lake(id),
                        WaterCandidate {
                            kind: SurfaceWaterKind::Lake,
                            surface_m,
                        },
                    )
                }),
            }
        };

        let mut row_start = Vec::with_capacity(cell_count + 1);
        let mut candidates = Vec::new();
        let mut ring = Vec::new();
        for (cell, record) in surface.cells().iter().enumerate() {
            row_start.push(candidates.len() as u32);
            let own = body_of(cell);
            if let Some((_, candidate)) = own {
                candidates.push(candidate);
            }
            ring.clear();
            ring.extend(
                record
                    .boundary_edges
                    .iter()
                    .filter_map(|&edge| surface.opposite_cell(record.id, edge))
                    .filter_map(|neighbour| body_of(neighbour.raw() as usize))
                    .filter(|(body, _)| own.is_none_or(|(own_body, _)| own_body != *body)),
            );
            // Ascending surface, ties broken by the stable body identity so
            // the order never depends on edge enumeration.
            ring.sort_by(|(left_body, left), (right_body, right)| {
                left.surface_m
                    .total_cmp(&right.surface_m)
                    .then(left_body.cmp(right_body))
            });
            ring.dedup_by_key(|(body, _)| *body);
            candidates.extend(ring.iter().map(|(_, candidate)| *candidate));
        }
        row_start.push(candidates.len() as u32);
        Self {
            row_start,
            candidates,
        }
    }

    /// Classifies one derived leaf of `cell` by its derived elevation.
    ///
    /// # Arguments
    ///
    /// * `cell` - The leaf's root cell.
    /// * `elevation_m` - The leaf's derived face value.
    ///
    /// # Returns
    ///
    /// The kind of the first candidate body whose surface lies strictly
    /// above the leaf, else [`SurfaceWaterKind::DryLand`].
    pub fn classify(&self, cell: CellId, elevation_m: f32) -> SurfaceWaterKind {
        let index = cell.raw() as usize;
        let (Some(&start), Some(&end)) = (self.row_start.get(index), self.row_start.get(index + 1))
        else {
            return SurfaceWaterKind::DryLand;
        };
        self.candidates[start as usize..end as usize]
            .iter()
            .find(|candidate| elevation_m < candidate.surface_m)
            .map_or(SurfaceWaterKind::DryLand, |candidate| candidate.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::{Meters, SphericalSpaceSpec};

    fn surface() -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build_cancellable(
            &SphericalSpaceSpec {
                radius: Meters::new(6_371_000.0).unwrap(),
                target_cell_count: 162,
            },
            || false,
        )
        .unwrap()
    }

    /// Picks an ocean cell, a lake cell two rings away from it, the dry
    /// cell between them, and a dry cell touching neither.
    fn layout(surface: &SphericalSurfaceSnapshot) -> (usize, usize, usize, usize) {
        let count = surface.cells().len();
        let mut neighbours = vec![Vec::new(); count];
        for edge in surface.edges() {
            let [a, b] = edge.cells.map(|cell| cell.raw() as usize);
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
        let ocean = 0;
        let between = neighbours[ocean][0];
        let lake = *neighbours[between]
            .iter()
            .find(|&&cell| cell != ocean && !neighbours[ocean].contains(&cell))
            .unwrap();
        let far = (0..count)
            .find(|&cell| {
                ![ocean, lake].contains(&cell)
                    && !neighbours[cell].contains(&ocean)
                    && !neighbours[cell].contains(&lake)
            })
            .unwrap();
        (ocean, between, lake, far)
    }

    fn classifier(surface: &SphericalSurfaceSnapshot) -> (FineShoreline, [usize; 4]) {
        let (ocean, between, lake, far) = layout(surface);
        let count = surface.cells().len();
        let mut kinds = vec![SurfaceWaterKind::DryLand; count];
        kinds[ocean] = SurfaceWaterKind::Ocean;
        kinds[lake] = SurfaceWaterKind::Lake;
        let lakes = vec![Lake::new(
            LakeId::from_raw(0),
            vec![CellId::from_raw(lake as u32)],
            50.0,
            1.0,
            1.0,
            None,
            None,
        )
        .unwrap()];
        (
            FineShoreline::new(surface, &SurfaceWaterField::from_kinds(kinds), &lakes, 0.0),
            [ocean, between, lake, far],
        )
    }

    #[test]
    fn leaves_follow_the_water_surface_inside_the_ring_only() {
        let surface = surface();
        let (shoreline, [ocean, between, lake, far]) = classifier(&surface);
        let at = |cell: usize, elevation: f32| {
            shoreline.classify(CellId::from_raw(cell as u32), elevation)
        };

        // Inside a body: below its surface is water, above is an island.
        assert_eq!(at(ocean, -1.0), SurfaceWaterKind::Ocean);
        assert_eq!(at(ocean, 0.0), SurfaceWaterKind::DryLand);
        assert_eq!(at(lake, 49.0), SurfaceWaterKind::Lake);
        assert_eq!(at(lake, 50.0), SurfaceWaterKind::DryLand);
        // A lake cell's leaf that dips below sea level next to no ocean
        // stays lake: the root owner comes first.
        assert_eq!(at(lake, -10.0), SurfaceWaterKind::Lake);
        // The ring: the lower surface is tried first, so a leaf below sea
        // level between ocean and lake joins the ocean, a leaf between the
        // two surfaces joins the lake.
        assert_eq!(at(between, -1.0), SurfaceWaterKind::Ocean);
        assert_eq!(at(between, 10.0), SurfaceWaterKind::Lake);
        assert_eq!(at(between, 60.0), SurfaceWaterKind::DryLand);
        // Outside every ring nothing floods, however low (closed basins
        // below sea level stay dry).
        assert_eq!(at(far, -500.0), SurfaceWaterKind::DryLand);
    }
}
