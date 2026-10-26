//! Fine-domain shoreline: the per-cell water bodies a derived leaf can
//! belong to (T1 v2.2 spec §5.1, revised for lakes by LOD-1 spec R2).
//!
//! The ocean keeps the §5.1 rule: a leaf of the ocean's cells or their edge
//! ring is ocean when its derived value lies below the formal sea level. A
//! lake `W` is a candidate exactly on its hydrological bound `B_W` (R2.3(a)):
//! the depression at the lake level — the lake cells plus the cells P5's
//! priority-flood fill raises to the same level with a flooded centroid —
//! and the dry ring that holds the water there. Cells past the spill (below
//! the fill level, or at it with a dry centroid), the ocean and other lakes'
//! cells are not on the bound. Whether a lake candidate's leaf is water is
//! the scale-recursive connectivity rule of the hierarchical evaluator
//! (R2.3(b)); this module only stores the bound and the level-1 seeds.
//! This is display derivation only: P5 schema, volumes and topology are
//! untouched.

use std::collections::VecDeque;

use super::hierarchical_derivation::PublishedHydrology;
use crate::world::natural::SurfaceWaterKind;
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::{CellId, LakeId};

/// One P5 water body visible from a cell: its kind, its surface and, for
/// a lake, its slot in the published lake list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct WaterCandidate {
    pub(super) kind: SurfaceWaterKind,
    pub(super) surface_m: f32,
    pub(super) lake: Option<u32>,
}

/// Body identity for ordering candidates; the ocean is one body.
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
    /// Per candidate: bit `s` set when sector `s` of the row's cell is in
    /// the lake's level-1 water (R2.3(b)); always 0 for the ocean.
    level1_water: Vec<u64>,
    /// Per cell: the surface of the water body owning the cell, if any.
    own_surface_m: Vec<Option<f32>>,
    /// Per cell: the lake slot when the cell is a P5 lake cell.
    own_lake: Vec<Option<u32>>,
    /// Per lake slot: the published surface.
    lake_surface_m: Vec<f32>,
    /// Per cell: whether the cell or an edge neighbour is a P5 water cell,
    /// or the cell is on a lake bound — the LOD-1 spec §3.2 D2 near-water
    /// partition as extended by R2.3(d).
    near_water: Vec<bool>,
}

impl FineShoreline {
    /// Builds the candidate rows and lake bounds from the published P5
    /// hydrology.
    ///
    /// # Arguments
    ///
    /// * `surface` - The authoritative surface the hydrology indexes.
    /// * `hydrology` - The published P5 water bodies and fill surface.
    /// * `elevation_m` - The authoritative cell elevations.
    /// * `sea_level_m` - The formal sea level the ocean surface uses.
    ///
    /// # Returns
    ///
    /// The classifier; a cell missing from the hydrology contributes no
    /// candidate, which leaves its leaves dry.
    pub(super) fn new(
        surface: &SphericalSurfaceSnapshot,
        hydrology: &PublishedHydrology<'_>,
        elevation_m: &[f32],
        sea_level_m: f32,
    ) -> Self {
        let cell_count = surface.cells().len();
        let water = hydrology.surface_water;
        let kind = |cell: usize| water.get(cell).unwrap_or(SurfaceWaterKind::DryLand);
        let neighbours: Vec<Vec<usize>> = surface
            .cells()
            .iter()
            .map(|record| {
                record
                    .boundary_edges
                    .iter()
                    .filter_map(|&edge| surface.opposite_cell(record.id, edge))
                    .map(|cell| cell.raw() as usize)
                    .collect()
            })
            .collect();
        let mut own_lake = vec![None; cell_count];
        for (slot, lake) in hydrology.lakes.iter().enumerate() {
            for cell in lake.cells() {
                if let Some(entry) = own_lake.get_mut(cell.raw() as usize) {
                    *entry = Some(slot as u32);
                }
            }
        }
        let fill = |cell: usize| hydrology.fill_surface_m.get(cell).copied();
        let elevation = |cell: usize| elevation_m.get(cell).copied();
        // Per cell: the lakes whose bound contains it (R2.3(a)).
        let mut bound_lakes: Vec<Vec<u32>> = vec![Vec::new(); cell_count];
        let mut on_bound = vec![false; cell_count];
        for (slot, lake) in hydrology.lakes.iter().enumerate() {
            let surface_m = lake.surface_elevation_m();
            let mut members: Vec<usize> = lake.cells().iter().map(|c| c.raw() as usize).collect();
            members.retain(|&cell| cell < cell_count);
            for &cell in &members {
                on_bound[cell] = true;
            }
            // The depression at the lake level: cells the fill raises to
            // the lake surface whose centroid lies below it.
            let mut queue: VecDeque<usize> = members.iter().copied().collect();
            while let Some(cell) = queue.pop_front() {
                for &next in &neighbours[cell] {
                    if !on_bound[next]
                        && kind(next) == SurfaceWaterKind::DryLand
                        && fill(next) == Some(surface_m)
                        && elevation(next).is_some_and(|z| z < surface_m)
                    {
                        on_bound[next] = true;
                        members.push(next);
                        queue.push_back(next);
                    }
                }
            }
            // The dry ring holding the water at the lake level.
            let depression = members.len();
            for index in 0..depression {
                for &next in &neighbours[members[index]] {
                    if !on_bound[next]
                        && kind(next) == SurfaceWaterKind::DryLand
                        && fill(next).is_some_and(|level| level > surface_m)
                    {
                        on_bound[next] = true;
                        members.push(next);
                    }
                }
            }
            for &cell in &members {
                on_bound[cell] = false;
                bound_lakes[cell].push(slot as u32);
            }
        }

        let lake_candidate = |slot: u32| {
            let lake = &hydrology.lakes[slot as usize];
            (
                WaterBody::Lake(lake.id()),
                WaterCandidate {
                    kind: SurfaceWaterKind::Lake,
                    surface_m: lake.surface_elevation_m(),
                    lake: Some(slot),
                },
            )
        };
        let ocean = (
            WaterBody::Ocean,
            WaterCandidate {
                kind: SurfaceWaterKind::Ocean,
                surface_m: sea_level_m,
                lake: None,
            },
        );
        let mut row_start = Vec::with_capacity(cell_count + 1);
        let mut candidates = Vec::new();
        let mut own_surface_m = Vec::with_capacity(cell_count);
        let mut near_water = Vec::with_capacity(cell_count);
        let mut others = Vec::new();
        for cell in 0..cell_count {
            row_start.push(candidates.len() as u32);
            let own = match kind(cell) {
                SurfaceWaterKind::DryLand => None,
                SurfaceWaterKind::Ocean => Some(ocean),
                SurfaceWaterKind::Lake => own_lake[cell].map(lake_candidate),
            };
            own_surface_m.push(own.map(|(_, candidate)| candidate.surface_m));
            near_water.push(
                kind(cell) != SurfaceWaterKind::DryLand
                    || !bound_lakes[cell].is_empty()
                    || neighbours[cell]
                        .iter()
                        .any(|&next| kind(next) != SurfaceWaterKind::DryLand),
            );
            if let Some((_, candidate)) = own {
                candidates.push(candidate);
            }
            others.clear();
            if neighbours[cell]
                .iter()
                .any(|&next| kind(next) == SurfaceWaterKind::Ocean)
            {
                others.push(ocean);
            }
            others.extend(bound_lakes[cell].iter().map(|&slot| lake_candidate(slot)));
            others.retain(|(body, _)| own.is_none_or(|(own_body, _)| own_body != *body));
            // Ascending surface, ties broken by the stable body identity so
            // the order never depends on enumeration.
            others.sort_by(|(left_body, left), (right_body, right)| {
                left.surface_m
                    .total_cmp(&right.surface_m)
                    .then(left_body.cmp(right_body))
            });
            others.dedup_by_key(|(body, _)| *body);
            candidates.extend(others.iter().map(|(_, candidate)| *candidate));
        }
        row_start.push(candidates.len() as u32);
        Self {
            row_start,
            level1_water: vec![0; candidates.len()],
            candidates,
            own_surface_m,
            own_lake,
            lake_surface_m: hydrology
                .lakes
                .iter()
                .map(|lake| lake.surface_elevation_m())
                .collect(),
            near_water,
        }
    }

    /// The classifier of a world without published water: every row is
    /// empty, so every leaf is dry and every cell is inland.
    pub(super) fn without_water(cell_count: usize) -> Self {
        Self {
            row_start: vec![0; cell_count + 1],
            candidates: Vec::new(),
            level1_water: Vec::new(),
            own_surface_m: vec![None; cell_count],
            own_lake: vec![None; cell_count],
            lake_surface_m: Vec::new(),
            near_water: vec![false; cell_count],
        }
    }

    fn row_range(&self, cell: CellId) -> std::ops::Range<usize> {
        let index = cell.raw() as usize;
        match (self.row_start.get(index), self.row_start.get(index + 1)) {
            (Some(&start), Some(&end)) => start as usize..end as usize,
            _ => 0..0,
        }
    }

    /// The ordered water candidates of `cell`: its own body first, then the
    /// others by ascending surface.
    pub(super) fn row(&self, cell: CellId) -> &[WaterCandidate] {
        &self.candidates[self.row_range(cell)]
    }

    /// Whether `cell` is on the bound of lake `lake`.
    pub(super) fn on_lake_bound(&self, cell: CellId, lake: u32) -> bool {
        self.row(cell)
            .iter()
            .any(|candidate| candidate.lake == Some(lake))
    }

    /// Whether sector `sector` of `cell` is in lake `lake`'s level-1 water.
    pub(super) fn in_level1_water(&self, cell: CellId, sector: u8, lake: u32) -> bool {
        let range = self.row_range(cell);
        self.candidates[range.clone()]
            .iter()
            .zip(&self.level1_water[range])
            .any(|(candidate, mask)| candidate.lake == Some(lake) && mask >> sector & 1 == 1)
    }

    /// Records the level-1 water of lake `lake` in `cell` (bit per sector).
    pub(super) fn set_level1_water(&mut self, cell: CellId, lake: u32, sectors: u64) {
        let range = self.row_range(cell);
        for (candidate, mask) in self.candidates[range.clone()]
            .iter()
            .zip(&mut self.level1_water[range])
        {
            if candidate.lake == Some(lake) {
                *mask = sectors;
            }
        }
    }

    /// The published surface of lake slot `lake`.
    pub(super) fn lake_surface_m(&self, lake: u32) -> f32 {
        self.lake_surface_m[lake as usize]
    }

    /// The lake slot of `cell` when it is a P5 lake cell.
    pub(super) fn own_lake(&self, cell: CellId) -> Option<u32> {
        self.own_lake.get(cell.raw() as usize).copied().flatten()
    }

    /// The surface of the P5 water body owning `cell` (ocean: the formal
    /// sea level; lake: its published surface), or `None` on dry land —
    /// the erosion base level of the LOD-1 spec §3.3 D3.
    pub(super) fn own_surface_m(&self, cell: CellId) -> Option<f32> {
        self.own_surface_m
            .get(cell.raw() as usize)
            .copied()
            .flatten()
    }

    /// Whether `cell` is inland in the LOD-1 spec §3.2 D2 partition.
    pub(super) fn is_inland(&self, cell: CellId) -> bool {
        !self
            .near_water
            .get(cell.raw() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// The lake surface below which the lake bed shared by `a` and `b`
    /// (pass the same cell twice for one cell's interior) is flattened
    /// (LOD-1 spec R2.3(c)): the highest surface among the lakes whose
    /// bound holds both cells, or `None` when either cell sees the ocean
    /// or no lake holds both. Symmetric in `a` and `b`.
    pub(super) fn bed_surface_m(&self, a: CellId, b: CellId) -> Option<f32> {
        let sees_ocean = |cell: CellId| {
            self.row(cell)
                .iter()
                .any(|candidate| candidate.lake.is_none())
        };
        if sees_ocean(a) || sees_ocean(b) {
            return None;
        }
        self.row(a)
            .iter()
            .filter_map(|candidate| candidate.lake)
            .filter(|&lake| self.on_lake_bound(b, lake))
            .map(|lake| self.lake_surface_m(lake))
            .max_by(f32::total_cmp)
    }

    /// Whether no leaf of `cell` can ever be water: it has no candidate.
    pub(super) fn never_wet(&self, cell: CellId) -> bool {
        self.row_range(cell).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{Lake, RiverSegment, SurfaceWaterField};
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

    fn neighbours(surface: &SphericalSurfaceSnapshot) -> Vec<Vec<usize>> {
        let mut neighbours = vec![Vec::new(); surface.cells().len()];
        for edge in surface.edges() {
            let [a, b] = edge.cells.map(|cell| cell.raw() as usize);
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
        neighbours
    }

    /// One lake cell at 50 m with a fill-level fringe cell (flooded
    /// centroid), a spill-side cell at the lake level with a dry centroid,
    /// a cell below the lake level past the spill, and an ocean cell two
    /// rings out; every other cell rises above the lake.
    struct Layout {
        lake: usize,
        fringe: usize,
        sill: usize,
        low: usize,
        rim: usize,
        fringe_rim: usize,
        ocean: usize,
        far: usize,
        shoreline: FineShoreline,
    }

    fn layout() -> Layout {
        let surface = surface();
        let count = surface.cells().len();
        let near = neighbours(&surface);
        let lake = 0;
        let (fringe, sill, low, rim) = (near[lake][0], near[lake][1], near[lake][2], near[lake][3]);
        let fringe_rim = *near[fringe]
            .iter()
            .find(|&&cell| cell != lake && !near[lake].contains(&cell))
            .unwrap();
        // Three rings out: no ring cell of the lake sees this ocean.
        let mut ring = vec![usize::MAX; count];
        ring[lake] = 0;
        let mut queue = VecDeque::from([lake]);
        while let Some(cell) = queue.pop_front() {
            for &next in &near[cell] {
                if ring[next] == usize::MAX {
                    ring[next] = ring[cell] + 1;
                    queue.push_back(next);
                }
            }
        }
        let ocean = (0..count)
            .find(|&cell| ring[cell] == 3 && !near[cell].contains(&fringe_rim))
            .unwrap();
        let far = (0..count)
            .find(|&cell| {
                ring[cell] >= 2
                    && cell != ocean
                    && !near[fringe].contains(&cell)
                    && !near[cell].contains(&ocean)
            })
            .unwrap();
        let mut kinds = vec![SurfaceWaterKind::DryLand; count];
        kinds[lake] = SurfaceWaterKind::Lake;
        kinds[ocean] = SurfaceWaterKind::Ocean;
        let mut elevation = vec![80.0_f32; count];
        let mut fill = vec![80.0_f32; count];
        (elevation[lake], fill[lake]) = (40.0, 50.0);
        (elevation[fringe], fill[fringe]) = (49.5, 50.0);
        (elevation[sill], fill[sill]) = (50.0, 50.0);
        (elevation[low], fill[low]) = (30.0, 30.0);
        (elevation[ocean], fill[ocean]) = (-500.0, -500.0);
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
        let water = SurfaceWaterField::from_kinds(kinds);
        let segments: [RiverSegment; 0] = [];
        let shoreline = FineShoreline::new(
            &surface,
            &PublishedHydrology {
                segments: &segments,
                surface_water: &water,
                lakes: &lakes,
                fill_surface_m: &fill,
            },
            &elevation,
            0.0,
        );
        Layout {
            lake,
            fringe,
            sill,
            low,
            rim,
            fringe_rim,
            ocean,
            far,
            shoreline,
        }
    }

    /// LOD-1 spec R2.3(a): the lake bound is the depression at the lake
    /// level (lake and flooded fringe cells) plus its dry ring above the
    /// level; the spill side, the ocean and cells beyond stay off it.
    #[test]
    fn the_lake_bound_is_the_depression_and_its_holding_ring() {
        let layout = layout();
        let on = |cell: usize| {
            layout
                .shoreline
                .on_lake_bound(CellId::from_raw(cell as u32), 0)
        };
        for cell in [layout.lake, layout.fringe, layout.rim, layout.fringe_rim] {
            assert!(on(cell), "cell {cell} holds the lake");
        }
        for cell in [layout.sill, layout.low, layout.ocean, layout.far] {
            assert!(!on(cell), "cell {cell} is not the lake's");
        }
        let own = layout.shoreline.row(CellId::from_raw(layout.lake as u32));
        assert_eq!(own[0].lake, Some(0), "the root's own body comes first");
        assert_eq!(
            layout
                .shoreline
                .own_lake(CellId::from_raw(layout.lake as u32)),
            Some(0)
        );
    }

    /// The D2 near-water partition is own or edge-neighbour water or a lake
    /// bound (R2.3(d)); a cell with no candidate can never be wet.
    #[test]
    fn near_water_and_candidates_follow_the_published_water() {
        let layout = layout();
        let shoreline = &layout.shoreline;
        let id = |cell: usize| CellId::from_raw(cell as u32);
        assert!(!shoreline.is_inland(id(layout.sill)));
        assert!(shoreline.never_wet(id(layout.sill)));
        assert!(!shoreline.never_wet(id(layout.fringe_rim)));
        assert!(shoreline.is_inland(id(layout.far)) && shoreline.never_wet(id(layout.far)));
        // R2.3(d): a bound cell away from any water body is near water too.
        assert!(!shoreline.is_inland(id(layout.fringe_rim)));
        // R2.3(c): the bed flattens on the bound only, symmetrically.
        let (lake, rim) = (id(layout.lake), id(layout.rim));
        assert_eq!(shoreline.bed_surface_m(lake, lake), Some(50.0));
        assert_eq!(
            shoreline.bed_surface_m(lake, rim),
            shoreline.bed_surface_m(rim, lake)
        );
        assert_eq!(shoreline.bed_surface_m(lake, id(layout.sill)), None);
    }
}
