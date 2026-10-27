//! Lake water of the fine leaves (LOD-1 spec R2.3(b)): scale-recursive
//! connectivity over the fan subdivision, with the structural edge
//! adjacency it walks.
//!
//! A primitive of lake `W` is water when its uncarved value lies below the
//! lake surface, its root cell is on the lake bound `B_W`, and either
//! - at level 1 it is in the lake's level-1 water (a flood from the lake
//!   cells' fan faces through faces below the surface, inside `B_W`), or
//! - deeper, it is a *core* primitive — its parent is lake water, or it
//!   touches a lake cell's centroid anchor — or it shares an edge with one.
//!
//! So each level reaches at most one primitive of that level beyond the
//! children of the previous level's water: the lake at level `l` refines the
//! lake at level `l - 1`, and percolation through below-surface noise stays
//! bounded. Values are uncarved, so river carving (which the river clip
//! shapes) never feeds back into the classification.

use std::collections::{HashMap, VecDeque};

use rustc_hash::FxHashMap;

use super::{
    child_frame, Anchor, DerivedReading, HierarchicalEvaluator, SectorConditions, TriangleFrame,
    HIERARCHICAL_PATH_DEPTH_MAX,
};
use crate::world::natural::{Lake, SurfaceWaterKind};
use crate::world::CellId;

#[cfg(test)]
#[path = "lake_r5_probe.rs"]
mod r5_probe;

/// One primitive of the fan subdivision: root cell, fan sector, and the
/// subdivision path packed two bits per step (step `i` in bits `2i..2i+2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct PrimitiveKey {
    cell: u32,
    sector: u8,
    len: u8,
    steps: u32,
}

impl PrimitiveKey {
    /// Packs `(cell, sector, path)`.
    ///
    /// Panics on a path deeper than the spec §5 cap — IDs come from the
    /// engine's own enumeration, so that is a caller programming error.
    pub(super) fn new(cell: u32, sector: u8, path: &[u8]) -> Self {
        assert!(
            path.len() <= HIERARCHICAL_PATH_DEPTH_MAX,
            "hierarchical path depth {} exceeds the spec cap {}",
            path.len(),
            HIERARCHICAL_PATH_DEPTH_MAX
        );
        let steps = path
            .iter()
            .enumerate()
            .fold(0_u32, |packed, (index, &step)| {
                packed | u32::from(step & 3) << (2 * index)
            });
        Self {
            cell,
            sector,
            len: path.len() as u8,
            steps,
        }
    }

    pub(super) fn cell_index(self) -> usize {
        self.cell as usize
    }

    /// The primitive level: 1 for a fan sector, one more per step.
    pub(super) fn level(self) -> u8 {
        1 + self.len
    }

    fn last_step(self) -> u8 {
        (self.steps >> (2 * (u32::from(self.len) - 1)) & 3) as u8
    }

    fn parent(self) -> Self {
        let len = self.len - 1;
        Self {
            len,
            steps: (u64::from(self.steps) & ((1_u64 << (2 * u32::from(len))) - 1)) as u32,
            ..self
        }
    }

    pub(super) fn child(self, child: u8) -> Self {
        Self {
            len: self.len + 1,
            steps: self.steps | u32::from(child) << (2 * u32::from(self.len)),
            ..self
        }
    }

    /// The descendant `depth` levels below, along the packed relative path
    /// `rel` (first step in the lowest bits).
    fn descendant(self, rel: u32, depth: u8) -> Self {
        Self {
            len: self.len + depth,
            steps: self.steps | rel << (2 * u32::from(self.len)),
            ..self
        }
    }

    /// The packed relative path of `self` below `root`, if it descends from
    /// it.
    fn relative_to(self, root: Self) -> Option<u32> {
        let mask = ((1_u64 << (2 * u32::from(root.len))) - 1) as u32;
        (self.cell == root.cell
            && self.sector == root.sector
            && self.len >= root.len
            && self.steps & mask == root.steps)
            .then(|| (u64::from(self.steps) >> (2 * u32::from(root.len))) as u32)
    }

    /// Whether the primitive keeps the sector's first corner — the cell
    /// centroid anchor — which only the all-zero path does.
    fn touches_centroid(self) -> bool {
        self.steps == 0
    }
}

/// Edges `e` (corner `e` → corner `e + 1`) of child `c` shared with a
/// sibling inside the parent: `(sibling, sibling edge)`, for the frozen
/// child order 0 = (A, AB, CA), 1 = (AB, B, BC), 2 = (CA, BC, C),
/// 3 = (AB, BC, CA).
const INTERIOR_EDGE: [[Option<(u8, u8)>; 3]; 4] = [
    [None, Some((3, 2)), None],
    [None, None, Some((3, 0))],
    [Some((3, 1)), None, None],
    [Some((1, 2)), Some((2, 0)), Some((0, 1))],
];
/// For a child edge lying on the parent edge of the same index: 0 when it
/// is the half next to the parent edge's start corner, 1 for the far half.
const PARENT_EDGE_HALF: [[u8; 3]; 4] = [[0, 0, 1], [1, 0, 0], [0, 1, 0], [0, 0, 0]];
/// The child on each half of a parent edge.
const HALF_CHILD: [[u8; 2]; 3] = [[0, 1], [1, 2], [2, 0]];

impl HierarchicalEvaluator {
    /// The same-level primitive sharing edge `edge` of `key`, and that
    /// edge's index in it. Shared edges run in opposite directions on the
    /// two sides (fan sectors and cells are counter-clockwise), so a half
    /// near one side's start corner is the other side's far half.
    pub(super) fn edge_neighbour(&self, key: PrimitiveKey, edge: u8) -> (PrimitiveKey, u8) {
        if key.len == 0 {
            return self.sector_neighbour(key.cell, key.sector, edge);
        }
        let child = usize::from(key.last_step());
        let parent = key.parent();
        if let Some((sibling, sibling_edge)) = INTERIOR_EDGE[child][usize::from(edge)] {
            return (parent.child(sibling), sibling_edge);
        }
        let (across, across_edge) = self.edge_neighbour(parent, edge);
        let half = PARENT_EDGE_HALF[child][usize::from(edge)];
        (
            across.child(HALF_CHILD[usize::from(across_edge)][usize::from(1 - half)]),
            across_edge,
        )
    }

    /// Fan-sector adjacency: spokes to the neighbouring sectors of the same
    /// cell, the boundary edge to the edge-neighbour cell's sector.
    fn sector_neighbour(&self, cell: u32, sector: u8, edge: u8) -> (PrimitiveKey, u8) {
        let (start, len) = self.ring_bounds(cell);
        let sector = usize::from(sector);
        let key = |cell: u32, sector: usize| PrimitiveKey::new(cell, sector as u8, &[]);
        match edge {
            0 => (key(cell, (sector + len - 1) % len), 2),
            2 => (key(cell, (sector + 1) % len), 0),
            _ => {
                let near = self.ring_vertices[start + sector];
                let far = self.ring_vertices[start + (sector + 1) % len];
                let other = self.ring_neighbors[start + sector];
                let (other_start, other_len) = self.ring_bounds(other);
                let other_sector = (0..other_len)
                    .find(|&candidate| {
                        self.ring_vertices[other_start + candidate] == far
                            && self.ring_vertices[other_start + (candidate + 1) % other_len] == near
                    })
                    .expect("validated cells traverse a shared edge in opposite directions");
                (key(other, other_sector), 1)
            }
        }
    }

    /// The class the fine shoreline paints a level ≥ 1 primitive with: the
    /// first candidate of its root cell that holds it — the ocean when its
    /// published `value_m` lies below sea level, a lake by connectivity.
    pub(super) fn leaf_water(
        &self,
        walk: &mut LakeWalk<'_>,
        key: PrimitiveKey,
        value_m: f32,
    ) -> SurfaceWaterKind {
        let cell = CellId::from_raw(key.cell);
        for candidate in self.shoreline.row(cell) {
            let wet = match candidate.lake {
                None => value_m < candidate.surface_m,
                Some(lake) => walk.is_lake_water(lake, key),
            };
            if wet {
                return candidate.kind;
            }
        }
        SurfaceWaterKind::DryLand
    }

    /// The class of an L0 cell primitive: ocean by value, lake when the
    /// cell is one of the lake's P5 cells.
    pub(super) fn cell_water(&self, cell: CellId, value_m: f32) -> SurfaceWaterKind {
        for candidate in self.shoreline.row(cell) {
            let wet = match candidate.lake {
                None => value_m < candidate.surface_m,
                Some(lake) => self.shoreline.own_lake(cell) == Some(lake),
            };
            if wet {
                return candidate.kind;
            }
        }
        SurfaceWaterKind::DryLand
    }

    /// Floods each lake's level-1 water (R2.3(b)): from the lake cells' fan
    /// faces whose uncarved value lies below the surface, through
    /// edge-adjacent faces below it whose root is on the lake bound.
    pub(super) fn flood_level1_lakes(&mut self, lakes: &[Lake]) {
        let mut found: Vec<(u32, CellId, u64)> = Vec::new();
        for (slot, lake) in lakes.iter().enumerate() {
            let slot = slot as u32;
            let surface_m = lake.surface_elevation_m();
            let below = |key: PrimitiveKey| {
                let [a, b, c] = self.sector_frame(key.cell, key.sector).corners;
                (((a.value_m + b.value_m + c.value_m) / 3.0) as f32) < surface_m
            };
            let mut sectors: HashMap<u32, u64> = HashMap::new();
            let mut queue = VecDeque::new();
            for cell in lake.cells() {
                if !self.shoreline.on_lake_bound(*cell, slot) {
                    continue;
                }
                for sector in 0..self.sector_count(*cell) as u8 {
                    let key = PrimitiveKey::new(cell.raw(), sector, &[]);
                    if below(key) {
                        *sectors.entry(key.cell).or_default() |= 1 << sector;
                        queue.push_back(key);
                    }
                }
            }
            while let Some(key) = queue.pop_front() {
                for edge in 0..3 {
                    let (next, _) = self.edge_neighbour(key, edge);
                    let mask = sectors.get(&next.cell).copied().unwrap_or(0);
                    if mask >> next.sector & 1 == 0
                        && self
                            .shoreline
                            .on_lake_bound(CellId::from_raw(next.cell), slot)
                        && below(next)
                    {
                        *sectors.entry(next.cell).or_default() |= 1 << next.sector;
                        queue.push_back(next);
                    }
                }
            }
            found.extend(
                sectors
                    .into_iter()
                    .map(|(cell, mask)| (slot, CellId::from_raw(cell), mask)),
            );
        }
        for (slot, cell, mask) in found {
            self.shoreline.set_level1_water(cell, slot, mask);
        }
    }
}

/// A memo of one batch's or query's walk: primitive frames (each split
/// derived once and shared by its four children) and lake connectivity
/// decisions. Caches only accelerate — every answer is the pure function
/// of the primitive identity.
pub(super) struct LakeWalk<'a> {
    engine: &'a HierarchicalEvaluator,
    frames: FxHashMap<PrimitiveKey, TriangleFrame>,
    conditions: FxHashMap<(u32, u8), SectorConditions>,
    water: FxHashMap<(u32, PrimitiveKey), bool>,
    core: FxHashMap<(u32, PrimitiveKey), bool>,
}

impl<'a> LakeWalk<'a> {
    pub(super) fn new(engine: &'a HierarchicalEvaluator) -> Self {
        Self {
            engine,
            frames: FxHashMap::default(),
            conditions: FxHashMap::default(),
            water: FxHashMap::default(),
            core: FxHashMap::default(),
        }
    }

    /// The anchor frame of `key`, bit-identical to the engine's walk.
    pub(super) fn frame(&mut self, key: PrimitiveKey) -> TriangleFrame {
        if let Some(frame) = self.frames.get(&key) {
            return *frame;
        }
        let engine = self.engine;
        if key.len == 0 {
            let frame = engine.sector_frame(key.cell, key.sector);
            self.frames.insert(key, frame);
            return frame;
        }
        let parent_key = key.parent();
        let parent = self.frame(parent_key);
        let conditions = self.sector_conditions(key.cell, key.sector);
        // The split producing `key` is its path's last step, at level len.
        let midpoints = engine.split_midpoints(&parent, u32::from(key.len), &conditions);
        for child in 0..4 {
            self.frames.insert(
                parent_key.child(child),
                child_frame(parent, midpoints, child),
            );
        }
        self.frames[&key]
    }

    /// The split conditions of one fan sector, derived once per walk.
    fn sector_conditions(&mut self, cell: u32, sector: u8) -> SectorConditions {
        let engine = self.engine;
        *self
            .conditions
            .entry((cell, sector))
            .or_insert_with(|| engine.sector_conditions(cell, sector))
    }

    /// The uncarved face value of `key` as published (f32), the value the
    /// connectivity rule compares against lake surfaces.
    fn uncarved_m(&mut self, key: PrimitiveKey) -> f32 {
        frame_uncarved_m(&self.frame(key))
    }

    /// Whether `key` is water of lake `lake` (R2.3(b)).
    pub(super) fn is_lake_water(&mut self, lake: u32, key: PrimitiveKey) -> bool {
        if let Some(&water) = self.water.get(&(lake, key)) {
            return water;
        }
        let water = self.decide_water(lake, key);
        self.water.insert((lake, key), water);
        water
    }

    fn decide_water(&mut self, lake: u32, key: PrimitiveKey) -> bool {
        let shoreline = &self.engine.shoreline;
        let cell = CellId::from_raw(key.cell);
        if !shoreline.on_lake_bound(cell, lake)
            || self.uncarved_m(key) >= shoreline.lake_surface_m(lake)
        {
            return false;
        }
        if key.len == 0 {
            return shoreline.in_level1_water(cell, key.sector, lake);
        }
        if self.is_core(lake, key) {
            return true;
        }
        for edge in 0..3 {
            let (next, _) = self.engine.edge_neighbour(key, edge);
            if self.is_core(lake, next) {
                return true;
            }
        }
        false
    }

    /// A level ≥ 2 primitive below the surface on the bound whose parent is
    /// lake water, or which touches one of the lake cells' centroid anchors.
    fn is_core(&mut self, lake: u32, key: PrimitiveKey) -> bool {
        if let Some(&core) = self.core.get(&(lake, key)) {
            return core;
        }
        let shoreline = &self.engine.shoreline;
        let cell = CellId::from_raw(key.cell);
        let core = shoreline.on_lake_bound(cell, lake)
            && self.uncarved_m(key) < shoreline.lake_surface_m(lake)
            && ((key.touches_centroid() && shoreline.own_lake(cell) == Some(lake))
                || self.is_lake_water(lake, key.parent()));
        self.core.insert((lake, key), core);
        core
    }
}

/// Margin of the subtree value bound over the f32 publication rounding
/// and the f64 rounding of the anchors (the river cursor's margin).
const SUBTREE_BOUND_MARGIN_M: f64 = 0.01;

impl HierarchicalEvaluator {
    /// Streams the readings of every leaf `extra` levels below `root`, a
    /// primitive whose cell has lake candidates, in depth-first order.
    ///
    /// The subtree is classified level by level: each level's frames come
    /// from the previous level's splits (the same arithmetic as the value
    /// walk), its lake cores from the parents' water, and its water from
    /// the cores of edge neighbours — looked up in the level's arrays when
    /// inside the subtree, through the memo walk across its border. A
    /// subtree whose value bound stays above every lake surface skips the
    /// connectivity work: none of its leaves can be lake water.
    pub(super) fn for_each_lake_subtree_reading(
        &self,
        root: PrimitiveKey,
        extra: u8,
        sink: &mut dyn FnMut(DerivedReading),
    ) {
        let cell = CellId::from_raw(root.cell);
        let row = self.shoreline.row(cell);
        let mut walk = LakeWalk::new(self);
        let mut frames = vec![walk.frame(root)];
        let conditions = walk.sector_conditions(root.cell, root.sector);
        let remaining_m: f64 = (1..=u32::from(extra))
            .map(|step| {
                let level = u32::from(root.len) + step;
                conditions
                    .root
                    .amplitude_m(level)
                    .max(conditions.boundary.amplitude_m(level))
            })
            .sum();
        let lowest_m = frames[0]
            .corners
            .iter()
            .map(|anchor| anchor.value_m)
            .fold(f64::INFINITY, f64::min);
        let floor_m = lowest_m - remaining_m * (1.0 + 1.0e-9) - SUBTREE_BOUND_MARGIN_M;
        let lakes: Vec<(u32, f32)> = row
            .iter()
            .filter_map(|candidate| candidate.lake.map(|lake| (lake, candidate.surface_m)))
            .filter(|&(_, surface_m)| floor_m < f64::from(surface_m))
            .collect();
        // water[k][rel]: lake water of lake k at the current level, indexed
        // by the path below the root packed like PrimitiveKey (first step
        // in the lowest bits).
        let mut water: Vec<Vec<bool>> = lakes
            .iter()
            .map(|&(lake, _)| vec![walk.is_lake_water(lake, root)])
            .collect();
        for depth in 1..=extra {
            let parents = frames.len();
            let midpoints: Vec<[Anchor; 3]> = frames
                .iter()
                .map(|frame| self.split_midpoints(frame, u32::from(root.len + depth), &conditions))
                .collect();
            let mut children = Vec::with_capacity(parents * 4);
            for child in 0..4_u8 {
                children.extend(
                    frames
                        .iter()
                        .zip(&midpoints)
                        .map(|(frame, midpoints)| child_frame(*frame, *midpoints, child)),
                );
            }
            frames = children;
            if lakes.is_empty() {
                continue;
            }
            let below: Vec<Vec<bool>> = lakes
                .iter()
                .map(|&(_, surface_m)| {
                    frames
                        .iter()
                        .map(|frame| frame_uncarved_m(frame) < surface_m)
                        .collect()
                })
                .collect();
            for (index, &(lake, _)) in lakes.iter().enumerate() {
                let seeds_centroid =
                    root.touches_centroid() && self.shoreline.own_lake(cell) == Some(lake);
                let core: Vec<bool> = (0..frames.len())
                    .map(|rel| {
                        below[index][rel]
                            && (water[index][rel % parents] || (seeds_centroid && rel == 0))
                    })
                    .collect();
                let next: Vec<bool> = (0..frames.len())
                    .map(|rel| {
                        if !below[index][rel] {
                            return false;
                        }
                        if core[rel] {
                            return true;
                        }
                        let key = root.descendant(rel as u32, depth);
                        (0..3).any(|edge| {
                            let (next, _) = self.edge_neighbour(key, edge);
                            match next.relative_to(root) {
                                Some(inside) => core[inside as usize],
                                None => walk.is_core(lake, next),
                            }
                        })
                    })
                    .collect();
                water[index] = next;
            }
        }
        let leaf_level = root.level() + extra;
        let mut emit = |rel: usize| {
            let value = self.face_value(root.cell_index(), &frames[rel], leaf_level);
            let mut kind = SurfaceWaterKind::DryLand;
            for candidate in row {
                let wet = match candidate.lake {
                    None => value.elevation_m < candidate.surface_m,
                    Some(lake) => lakes
                        .iter()
                        .position(|&(slot, _)| slot == lake)
                        .is_some_and(|index| water[index][rel]),
                };
                if wet {
                    kind = candidate.kind;
                    break;
                }
            }
            sink(DerivedReading {
                cell,
                value,
                water: kind,
            });
        };
        // Depth-first order: the first step below the root varies slowest.
        for order in 0..frames.len() {
            let rel = (0..u32::from(extra)).fold(0_usize, |rel, step| {
                let digit = order >> (2 * (u32::from(extra) - 1 - step)) & 3;
                rel | digit << (2 * step)
            });
            emit(rel);
        }
    }
}

/// The uncarved face value of a frame as published (f32).
fn frame_uncarved_m(frame: &TriangleFrame) -> f32 {
    let [a, b, c] = &frame.corners;
    ((a.value_m + b.value_m + c.value_m) / 3.0) as f32
}

#[cfg(test)]
mod tests {
    use super::super::tests::wet_evaluator;
    use super::*;
    use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};

    fn enumerate(prefix: PrimitiveKey, remaining: u8, out: &mut Vec<PrimitiveKey>) {
        if remaining == 0 {
            out.push(prefix);
            return;
        }
        for child in 0..4 {
            enumerate(prefix.child(child), remaining - 1, out);
        }
    }

    /// The structural adjacency is an involution that pairs geometrically
    /// shared edges: across children, sectors and cells, the neighbour's
    /// returned edge has the same endpoints in the opposite direction.
    #[test]
    fn edge_neighbours_share_reversed_edges_across_the_whole_fan_mesh() {
        let (evaluator, surface) = wet_evaluator();
        let mut walk = LakeWalk::new(&evaluator);
        for cell in surface.cells().iter().step_by(17) {
            for sector in 0..evaluator.sector_count(cell.id) as u8 {
                for depth in 0..4 {
                    let mut keys = Vec::new();
                    enumerate(
                        PrimitiveKey::new(cell.id.raw(), sector, &[]),
                        depth,
                        &mut keys,
                    );
                    for key in keys {
                        let corners = walk.frame(key).corners.map(|anchor| anchor.position);
                        for edge in 0..3_u8 {
                            let (next, next_edge) = evaluator.edge_neighbour(key, edge);
                            assert_eq!(next.level(), key.level());
                            assert_eq!(evaluator.edge_neighbour(next, next_edge), (key, edge));
                            let other = walk.frame(next).corners.map(|anchor| anchor.position);
                            let e = usize::from(edge);
                            let f = usize::from(next_edge);
                            assert_eq!(corners[e], other[(f + 1) % 3]);
                            assert_eq!(corners[(e + 1) % 3], other[f]);
                        }
                    }
                }
            }
        }
    }

    /// The memo walk derives the same frames, hence values, as the plain
    /// path walk.
    #[test]
    fn walk_frames_match_the_engine_walk() {
        let (evaluator, surface) = wet_evaluator();
        let mut walk = LakeWalk::new(&evaluator);
        for cell in surface.cells().iter().step_by(23) {
            for path in [&[][..], &[3], &[0, 1, 2], &[2, 3, 1, 0, 3]] {
                let key = PrimitiveKey::new(cell.id.raw(), 1, path);
                let frame = walk.frame(key);
                let plain = evaluator.walk_frame(cell.id, 1, path);
                for (a, b) in frame.corners.iter().zip(&plain.corners) {
                    assert_eq!(a.value_m.to_bits(), b.value_m.to_bits());
                    assert_eq!(a.position, b.position);
                }
            }
        }
    }

    /// A lake on the synthetic land: the cell `lake` holds water 60 m
    /// above its own height, so the fine field crosses the surface near
    /// it; the fill is the one a priority flood leaves. (At 300 m every ring
    /// cell leaked, LOD-1 spec R4, leaving nothing for I11 to restrict.)
    fn lake_evaluator() -> (HierarchicalEvaluator, SphericalSurfaceSnapshot, CellId) {
        use super::super::tests::{lake_fill_surface, SyntheticFields};
        use crate::world::natural::{SurfaceWaterField, SurfaceWaterKind};
        use crate::world::{LakeId, RootSeed};

        let surface = super::super::tests::test_surface();
        let fields = SyntheticFields::new(&surface);
        let lake = surface
            .cells()
            .iter()
            .find(|cell| {
                let z = fields.elevation[cell.id.raw() as usize];
                (600.0..1_200.0).contains(&z)
            })
            .expect("the synthetic land has a mid-slope cell")
            .id;
        let surface_m = fields.elevation[lake.raw() as usize] + 60.0;
        let lakes = [Lake::new(
            LakeId::from_raw(0),
            vec![lake],
            surface_m,
            1.0,
            1.0,
            None,
            None,
        )
        .expect("a one-cell lake is valid")];
        let mut kinds: Vec<SurfaceWaterKind> = fields
            .elevation
            .iter()
            .map(|&z| {
                if z < 0.0 {
                    SurfaceWaterKind::Ocean
                } else {
                    SurfaceWaterKind::DryLand
                }
            })
            .collect();
        kinds[lake.raw() as usize] = SurfaceWaterKind::Lake;
        let water = SurfaceWaterField::from_kinds(kinds);
        let fill = lake_fill_surface(&surface, &fields.elevation, &lakes);
        let evaluator = HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(42))
            .expect("synthetic fields are valid")
            .with_hydrology(
                &surface,
                super::super::PublishedHydrology {
                    segments: &[],
                    surface_water: &water,
                    lakes: &lakes,
                    fill_surface_m: &fill,
                },
            )
            .expect("no rivers to validate");
        (evaluator, surface, lake)
    }

    /// LOD-1 spec R2.6 I10 and I11: lake water stays on the lake bound, and
    /// every lake primitive refines the previous level's lake — its parent
    /// is lake water, it touches the lake cell's centroid, or it shares an
    /// edge with such a primitive. Below-surface primitives outside that
    /// reach stay dry, so the rule actually restricts.
    #[test]
    fn lake_water_refines_the_previous_level_lake() {
        let (evaluator, surface, _) = lake_evaluator();
        let mut walk = LakeWalk::new(&evaluator);
        let surface_m = evaluator.shoreline.lake_surface_m(0);
        let (mut water, mut excluded) = (0, 0);
        for cell in surface.cells() {
            let on_bound = evaluator.shoreline.on_lake_bound(cell.id, 0);
            for sector in 0..evaluator.sector_count(cell.id) as u8 {
                for depth in 0..5 {
                    let mut keys = Vec::new();
                    enumerate(
                        PrimitiveKey::new(cell.id.raw(), sector, &[]),
                        depth,
                        &mut keys,
                    );
                    for key in keys {
                        let below = walk.uncarved_m(key) < surface_m;
                        if !walk.is_lake_water(0, key) {
                            excluded += usize::from(below && on_bound && depth > 0);
                            continue;
                        }
                        water += 1;
                        assert!(on_bound && below, "lake water off the bound or above it");
                        if key.len == 0 {
                            continue;
                        }
                        let reached = walk.is_core(0, key)
                            || (0..3).any(|edge| {
                                let (next, _) = evaluator.edge_neighbour(key, edge);
                                walk.is_core(0, next)
                            });
                        assert!(reached, "lake water must refine the parent-level lake");
                    }
                }
            }
        }
        assert!(
            water > 0 && excluded > 0,
            "{water} water, {excluded} excluded"
        );
    }

    /// LOD-1 spec R2.6 I13: the mesh stream and the picking read paint the
    /// same leaf with the same value bits and the same water class.
    #[test]
    fn mesh_stream_and_reads_classify_alike() {
        use crate::generators::natural::subdivision_children;
        use crate::world::natural::SurfaceWaterKind;

        fn leaves(corners: [UnitVector3; 3], depth: u8, out: &mut Vec<[UnitVector3; 3]>) {
            if depth == 0 {
                out.push(corners);
                return;
            }
            for child in subdivision_children(corners) {
                leaves(child, depth - 1, out);
            }
        }
        let (evaluator, surface, lake) = lake_evaluator();
        let ring: Vec<CellId> = std::iter::once(lake)
            .chain(
                surface.cells()[lake.raw() as usize]
                    .boundary_edges
                    .iter()
                    .filter_map(|&edge| surface.opposite_cell(lake, edge)),
            )
            .collect();
        let mut kinds = std::collections::HashSet::new();
        for cell in ring {
            for sector in 0..evaluator.sector_count(cell) as u8 {
                let mut geometry = Vec::new();
                leaves(evaluator.sector_corners(cell, sector), 3, &mut geometry);
                let mut geometry = geometry.into_iter();
                evaluator.for_each_leaf_reading(cell, sector, &[], 3, &mut |drawn| {
                    let [a, b, c] = geometry.next().expect("one geometry per leaf");
                    let [ax, ay, az] = a.components();
                    let [bx, by, bz] = b.components();
                    let [cx, cy, cz] = c.components();
                    let centroid = UnitVector3::new(ax + bx + cx, ay + by + cy, az + bz + cz)
                        .expect("a leaf centroid is not degenerate");
                    let read = evaluator.read(centroid, 4);
                    assert_eq!(
                        read.value.elevation_m.to_bits(),
                        drawn.value.elevation_m.to_bits()
                    );
                    assert_eq!(read.water, drawn.water);
                    kinds.insert(drawn.water);
                });
            }
        }
        assert!(
            kinds.contains(&SurfaceWaterKind::Lake) && kinds.contains(&SurfaceWaterKind::DryLand)
        );
    }

    /// LOD-1 spec R2.6 I12: a split edge lying wholly below the lake bed
    /// surface that applies to it takes no increment (its midpoint is
    /// exactly the endpoint mean); edges reaching the surface, and edges
    /// of ocean-seeing cells, keep their increments.
    #[test]
    fn lake_bed_edges_below_the_surface_carry_no_increment() {
        let (evaluator, _, lake) = lake_evaluator();
        let mut walk = LakeWalk::new(&evaluator);
        let (mut flat, mut rough) = (0, 0);
        for sector in 0..evaluator.sector_count(lake) as u8 {
            let conditions = evaluator.sector_conditions(lake.raw(), sector);
            for depth in 0..4 {
                let mut parents = Vec::new();
                enumerate(
                    PrimitiveKey::new(lake.raw(), sector, &[]),
                    depth,
                    &mut parents,
                );
                for parent in parents {
                    let frame = walk.frame(parent);
                    let [a, b, c] = frame.corners;
                    let first = walk.frame(parent.child(0)).corners;
                    let second = walk.frame(parent.child(1)).corners;
                    // Children 0 = (A, AB, CA) and 1 = (AB, B, BC).
                    let edges = [(a, b, first[1]), (b, c, second[2]), (c, a, first[2])];
                    for (index, (p, q, mid)) in edges.into_iter().enumerate() {
                        let (_, bed_surface_m) = conditions.pick(frame.boundary_edge[index]);
                        let mean = 0.5 * (p.value_m + q.value_m);
                        if p.value_m.max(q.value_m) < bed_surface_m {
                            assert_eq!(mid.value_m.to_bits(), mean.to_bits());
                            flat += 1;
                        } else if mid.value_m != mean {
                            rough += 1;
                        }
                    }
                }
            }
        }
        assert!(flat > 0 && rough > 0, "{flat} flat, {rough} rough");
    }
}
