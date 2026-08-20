//! Zoom-driven hierarchical subdivision display (plan M2 Task 3).
//!
//! The visible surface is a mosaic of T1 v2 hierarchical primitives: every
//! leaf triangle is one data atom rendered as one solid patch whose color
//! is its `PrimitiveValue` through the shared hypsometric palette, carried
//! by a dedicated provoking vertex and flat-interpolated on the GPU.
//!
//! A *selection* replaces the M1 uniform global depth (Ulrich 2002 chunked
//! LOD) on a physical ladder: levels halve the spec §5 primitive edge from
//! the cell spacing down to [`MIN_PRIMITIVE_EDGE_M`], each level engaging
//! when [`UNITS_ACROSS_VIEW`] primitives span the viewport (one level per
//! zoom octave), so the camera zooms past the ladder floor and the deepest
//! atoms become plainly visible. Cell sizes are measured by projected
//! sector **area** (orientation-free and exact under equal-area
//! projections, so levels stay uniform across the map), off-viewport
//! cells fall off geometrically with distance toward
//! [`OFFSCREEN_LEAF_LEVEL`], very near cells subdivide per subtree so only
//! their visible portion deepens, and everything is bounded by
//! [`VIEW_LEAF_BUDGET`]. Selections compile into batches — one subtree
//! each — that a worker thread evaluates through
//! `HierarchicalEvaluator::for_each_leaf_value` (paired by the shared
//! depth-first child order 0..4) and caches for incremental rebuilds.
//! Edge midpoints depend symmetrically on their two endpoint directions
//! only, so shared borders stay crack-free across batches and cells.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::generators::natural::{
    HierarchicalEvaluator, HierarchicalPath, PrimitiveValue, HIERARCHICAL_PATH_DEPTH_MAX,
};
use crate::view::{
    built_in_palette, project_unit_direction, sample_palette, AmplifiedSurfaceMesh, GlobeCamera,
    MapScreenTransform, PaletteId, ProjectionPoint, RiverPolylineSegment,
    SphericalPresentationViewState, SphericalProjection, SphericalViewMode,
};
use crate::world::natural::SphericalHydrologySnapshot;
use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
use crate::world::CellId;

/// The physical primitive ladder floor (user calibration, 2026-08-20):
/// per tier, the deepest level is the first whose spec §5 edge reaches
/// this scale — draft 14 levels, standard and high 13. The camera keeps
/// zooming past the floor down to the player-view span, so the deepest
/// atoms become plainly visible instead of chasing the pixel grid.
const MIN_PRIMITIVE_EDGE_M: f64 = 10.0;
/// How many primitives span the viewport at every mid-zoom level (the
/// physical zoom↔level link: one level per zoom octave). On common
/// canvases this keeps units in the visible 8–17 px band.
const UNITS_ACROSS_VIEW: f64 = 96.0;
/// Viewport leaf budget bounding every selection (chunked-LOD budget
/// discipline; the M1 Task 4R budget carried forward).
const VIEW_LEAF_BUDGET: usize = 5_000_000;
/// The floor level for cells far outside the padded viewport.
const OFFSCREEN_LEAF_LEVEL: u8 = 1;
/// Off-viewport levels drop this much per doubling of the distance in
/// viewport widths — a geometric falloff that keeps zoom-out reveals and
/// pans near-correct while bounding the off-screen leaf count.
const DISTANCE_FALLOFF_LEVELS_PER_OCTAVE: f64 = 2.0;
/// Whole-cell uniform batches up to this leaf level; deeper cells walk
/// their sector subtrees so only the visible portion deepens.
const CELL_UNIFORM_MAX_LEVEL: u8 = 6;
/// Sub-cell walk batches stop this many levels above their leaves.
const WALK_BATCH_EXTRA: u8 = 4;
/// Viewport padding fraction: small pans stay inside one selection.
const VIEW_MARGIN_FRACTION: f64 = 0.25;
/// Camera-space depth beyond which a globe direction counts as hidden;
/// slightly behind the limb so silhouette cells keep detail.
const GLOBE_HIDDEN_DEPTH: f64 = -0.15;
/// Batch-cache bound in cached leaves (≈36 bytes per leaf).
const CACHE_LEAF_CAPACITY: usize = 4_000_000;
/// The world-install selection before any camera drives the detail: the
/// M1 Task 4R uniform global density.
const INITIAL_UNIFORM_LEAF_LEVEL: u8 = 2;
/// Placeholder for shared corner vertices; flat interpolation only ever
/// reads the provoking vertex, and validation wants full opacity.
const UNREAD_CORNER_COLOR: [u8; 4] = [0, 0, 0, 255];

const TRIANGLE_CORNERS: usize = 3;

/// Everything the detail worker needs per published world.
pub(super) struct AmplifiedDetailContext {
    /// The T1 v2 hierarchical engine (owns geometry and values).
    pub(super) evaluator: HierarchicalEvaluator,
    /// The shared hypsometric color anchor: sea level in metres.
    pub(super) sea_level_m: f64,
    /// The shared hypsometric color anchor: display radius in metres.
    pub(super) display_radius_m: f64,
}

/// One renderable subtree: the leaves `extra` levels below
/// `(cell, sector, prefix)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DetailBatch {
    cell: u32,
    sector: u8,
    prefix: HierarchicalPath,
    extra: u8,
}

impl DetailBatch {
    fn leaves(&self) -> usize {
        4_usize.pow(u32::from(self.extra))
    }
}

/// One camera-resolved selection of batches, hashed for change detection.
pub(super) struct DetailSelection {
    batches: Vec<DetailBatch>,
    /// Total leaf primitives across all batches.
    pub(super) leaves: usize,
    /// Order-sensitive content hash for latest-wins scheduling.
    pub(super) hash: u64,
}

fn finish_selection(batches: Vec<DetailBatch>, leaves: usize) -> DetailSelection {
    let mut hasher = DefaultHasher::new();
    for batch in &batches {
        batch.hash(&mut hasher);
    }
    DetailSelection {
        hash: hasher.finish(),
        batches,
        leaves,
    }
}

/// The camera-free uniform selection (world install and fallbacks).
pub(super) fn uniform_selection(
    context: &AmplifiedDetailContext,
    leaf_level: u8,
) -> DetailSelection {
    let mut batches = Vec::new();
    let mut leaves = 0_usize;
    for cell_index in 0..context.evaluator.cell_count() as u32 {
        let cell = CellId::from_raw(cell_index);
        for sector in 0..context.evaluator.sector_count(cell) as u8 {
            let batch = DetailBatch {
                cell: cell_index,
                sector,
                prefix: HierarchicalPath::new(&[]),
                extra: leaf_level.saturating_sub(1),
            };
            leaves += batch.leaves();
            batches.push(batch);
        }
    }
    finish_selection(batches, leaves)
}

/// The initial selection installed with a freshly built world.
pub(super) fn initial_selection(context: &AmplifiedDetailContext) -> DetailSelection {
    uniform_selection(context, INITIAL_UNIFORM_LEAF_LEVEL)
}

/// Maps unit directions into logical screen pixels for the active view.
enum ScreenMapper {
    Map {
        projection: SphericalProjection,
        transform: MapScreenTransform,
        wrap_px: f64,
    },
    Globe {
        camera: GlobeCamera,
        canvas_size: [f64; 2],
    },
}

impl ScreenMapper {
    fn new(view: &SphericalPresentationViewState, canvas_size: [f64; 2]) -> Option<Self> {
        if canvas_size
            .into_iter()
            .any(|component| !component.is_finite() || component <= 0.0)
        {
            return None;
        }
        Some(match view.mode() {
            SphericalViewMode::Map => {
                let transform =
                    MapScreenTransform::new(view.projection(), view.map_camera(), canvas_size)?;
                ScreenMapper::Map {
                    projection: view.projection(),
                    wrap_px: transform.wrap_width_px(),
                    transform,
                }
            }
            SphericalViewMode::Globe => ScreenMapper::Globe {
                camera: view.globe_camera(),
                canvas_size,
            },
        })
    }

    fn canvas_size(&self) -> [f64; 2] {
        match self {
            ScreenMapper::Map { transform, .. } => transform.canvas_size(),
            ScreenMapper::Globe { canvas_size, .. } => *canvas_size,
        }
    }

    /// The screen position of one direction; `None` when hidden (globe
    /// far side) or unprojectable.
    fn screen(&self, direction: UnitVector3) -> Option<[f64; 2]> {
        match self {
            ScreenMapper::Map {
                projection,
                transform,
                ..
            } => {
                let point = project_unit_direction(*projection, direction.components())?;
                Some(transform.to_screen(ProjectionPoint::new(point[0], point[1])))
            }
            ScreenMapper::Globe {
                camera,
                canvas_size,
            } => {
                let (screen, depth) =
                    camera.project_point_with_depth(direction.components(), *canvas_size);
                (depth >= GLOBE_HIDDEN_DEPTH).then_some(screen)
            }
        }
    }

    /// The wrap-aware screen distance between two projected points.
    fn distance_px(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        let mut dx = (a[0] - b[0]).abs();
        if let ScreenMapper::Map { wrap_px, .. } = self {
            if *wrap_px > 0.0 && dx > *wrap_px * 0.5 {
                dx = *wrap_px - dx;
            }
        }
        dx.hypot(a[1] - b[1])
    }

    /// Shifts `x` by whole seam wraps until it lies nearest `reference`,
    /// so seam-straddling geometry measures contiguously on the map.
    fn unwrap_x_toward(&self, reference: f64, x: f64) -> f64 {
        if let ScreenMapper::Map { wrap_px, .. } = self {
            if *wrap_px > 0.0 {
                let offset = ((x - reference) / wrap_px).round();
                return x - offset * wrap_px;
            }
        }
        x
    }

    /// The screen-space distance from a span around `screen` to the
    /// padded viewport (zero when they intersect), wrap-aware on the map.
    ///
    /// The half-extent makes this a rectangle test, so a primitive much
    /// larger than the viewport (its anchor far outside) still measures
    /// zero — the deep-zoom containment case.
    fn distance_to_padded_viewport(&self, screen: [f64; 2], half_extent_px: f64) -> f64 {
        let [width, height] = self.canvas_size();
        let margin_x = width * VIEW_MARGIN_FRACTION + half_extent_px;
        let margin_y = height * VIEW_MARGIN_FRACTION + half_extent_px;
        let overhang = |value: f64, low: f64, high: f64| (low - value).max(value - high).max(0.0);
        let overhang_x = |x: f64| overhang(x, -margin_x, width + margin_x);
        let dx = match self {
            ScreenMapper::Map { wrap_px, .. } if *wrap_px > 0.0 => overhang_x(screen[0])
                .min(overhang_x(screen[0] - wrap_px))
                .min(overhang_x(screen[0] + wrap_px)),
            _ => overhang_x(screen[0]),
        };
        dx.hypot(overhang(screen[1], -margin_y, height + margin_y))
    }

    /// Whether a screen-space span around `screen` intersects the padded
    /// viewport, wrap-aware on the map.
    fn span_visible(&self, screen: [f64; 2], half_extent_px: f64) -> bool {
        self.distance_to_padded_viewport(screen, half_extent_px) == 0.0
    }
}

/// Resolves the camera-driven selection for the active view.
///
/// Every cell's leaf level tracks its projected size toward the target
/// leaf pixel size; off-viewport cells stay shallow; cells past
/// [`CELL_UNIFORM_MAX_LEVEL`] subdivide per subtree with hidden subtrees
/// pruned coarse. When the budget still overflows, every level demotes
/// uniformly until it fits — the "budget depth" the plan verifies.
pub(super) fn select_detail_batches(
    context: &AmplifiedDetailContext,
    view: &SphericalPresentationViewState,
    canvas_size: [f64; 2],
) -> DetailSelection {
    let Some(mapper) = ScreenMapper::new(view, canvas_size) else {
        return initial_selection(context);
    };
    let evaluator = &context.evaluator;
    // The physical ladder floor for this tier (spec §5 edge ≈ spacing/2^k)
    // and the physical zoom↔level link expressed in this canvas's pixels.
    let floor_level = (1.0
        + (evaluator.cell_spacing_m() / MIN_PRIMITIVE_EDGE_M)
            .log2()
            .ceil())
    .clamp(1.0, 1.0 + HIERARCHICAL_PATH_DEPTH_MAX as f64) as u8;
    let target_px = (canvas_size[0] / UNITS_ACROSS_VIEW).max(1.0);
    let mut shrink = 0_u8;
    loop {
        let mut batches = Vec::new();
        let mut leaves = 0_usize;
        for cell_index in 0..evaluator.cell_count() as u32 {
            let cell = CellId::from_raw(cell_index);
            let corners = evaluator.sector_corners(cell, 0);
            let level = cell_leaf_level(&mapper, corners, target_px, floor_level, shrink);
            for sector in 0..evaluator.sector_count(cell) as u8 {
                if level <= CELL_UNIFORM_MAX_LEVEL {
                    let batch = DetailBatch {
                        cell: cell_index,
                        sector,
                        prefix: HierarchicalPath::new(&[]),
                        extra: level - 1,
                    };
                    leaves += batch.leaves();
                    batches.push(batch);
                } else {
                    walk_sector_subtrees(
                        &mapper,
                        evaluator.sector_corners(cell, sector),
                        cell_index,
                        sector,
                        level,
                        &mut batches,
                        &mut leaves,
                    );
                }
            }
        }
        if leaves <= VIEW_LEAF_BUDGET || shrink >= floor_level {
            return finish_selection(batches, leaves);
        }
        shrink += 1;
    }
}

/// One cell's leaf level from its projected first-sector size.
///
/// The size measure is the sector triangle's projected **area**
/// (`√(2·area)`), which is orientation-free and exact under the
/// equal-area Equal Earth projection — a single edge length would read
/// up to a level low or high depending on how the edge happens to align
/// with the projection's latitude-dependent stretch, leaving persistent
/// level patches that panning cannot heal.
///
/// Off-viewport cells fall off smoothly instead of collapsing to the
/// floor: [`DISTANCE_FALLOFF_LEVELS_PER_OCTAVE`] levels per doubling of
/// their distance in viewport widths (chunked-LOD distance falloff), so
/// zoom-out reveals and pans land on near-correct coarse content while
/// the far side of the world still costs almost nothing.
fn cell_leaf_level(
    mapper: &ScreenMapper,
    corners: [UnitVector3; 3],
    target_px: f64,
    floor_level: u8,
    shrink: u8,
) -> u8 {
    let (Some(a), Some(b), Some(c)) = (
        mapper.screen(corners[0]),
        mapper.screen(corners[1]),
        mapper.screen(corners[2]),
    ) else {
        return OFFSCREEN_LEAF_LEVEL;
    };
    let b = [mapper.unwrap_x_toward(a[0], b[0]), b[1]];
    let c = [mapper.unwrap_x_toward(a[0], c[0]), c[1]];
    let doubled_area = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
    let size_px = doubled_area.sqrt();
    let extent_px = mapper.distance_px(a, b).max(mapper.distance_px(a, c));
    if !size_px.is_finite() || !extent_px.is_finite() {
        return OFFSCREEN_LEAF_LEVEL;
    }
    let measured = if size_px <= target_px {
        1.0
    } else {
        (1.0 + (size_px / target_px).log2().ceil()).clamp(1.0, f64::from(floor_level))
    };
    // Twice the sector extent covers the whole cell, so a cell containing
    // the deep-zoom viewport measures distance zero.
    let distance_px = mapper.distance_to_padded_viewport(a, 2.0 * extent_px);
    let falloff = if distance_px <= 0.0 {
        0.0
    } else {
        let viewport_width = mapper.canvas_size()[0].max(1.0);
        (DISTANCE_FALLOFF_LEVELS_PER_OCTAVE * (1.0 + distance_px / viewport_width).log2()).ceil()
    };
    ((measured - falloff).max(f64::from(OFFSCREEN_LEAF_LEVEL)) as u8)
        .saturating_sub(shrink)
        .max(1)
}

/// Emits the sub-cell batches of one very near sector: visible subtrees
/// split until [`WALK_BATCH_EXTRA`] levels above the target, hidden
/// subtrees emit one coarse leaf so the mosaic stays complete.
fn walk_sector_subtrees(
    mapper: &ScreenMapper,
    corners: [UnitVector3; 3],
    cell: u32,
    sector: u8,
    target_level: u8,
    batches: &mut Vec<DetailBatch>,
    leaves: &mut usize,
) {
    let mut prefix = [0_u8; 16];
    walk_node(
        mapper,
        corners,
        cell,
        sector,
        target_level,
        &mut prefix,
        0,
        batches,
        leaves,
    );
}

#[allow(clippy::too_many_arguments)]
fn walk_node(
    mapper: &ScreenMapper,
    corners: [UnitVector3; 3],
    cell: u32,
    sector: u8,
    target_level: u8,
    prefix: &mut [u8; 16],
    depth: u8,
    batches: &mut Vec<DetailBatch>,
    leaves: &mut usize,
) {
    let node_level = 1 + depth;
    let emit = |extra: u8, batches: &mut Vec<DetailBatch>, leaves: &mut usize| {
        let batch = DetailBatch {
            cell,
            sector,
            prefix: HierarchicalPath::new(&prefix[..usize::from(depth)]),
            extra,
        };
        *leaves += batch.leaves();
        batches.push(batch);
    };
    if node_level + WALK_BATCH_EXTRA >= target_level {
        emit(target_level - node_level, batches, leaves);
        return;
    }
    if !node_intersects_viewport(mapper, &corners) {
        emit(0, batches, leaves);
        return;
    }
    let ab = direction_midpoint(corners[0], corners[1]);
    let bc = direction_midpoint(corners[1], corners[2]);
    let ca = direction_midpoint(corners[2], corners[0]);
    let children = [
        [corners[0], ab, ca],
        [ab, corners[1], bc],
        [ca, bc, corners[2]],
        [ab, bc, ca],
    ];
    for (child, child_corners) in children.into_iter().enumerate() {
        prefix[usize::from(depth)] = child as u8;
        walk_node(
            mapper,
            child_corners,
            cell,
            sector,
            target_level,
            prefix,
            depth + 1,
            batches,
            leaves,
        );
    }
}

/// Conservative visibility: any projectable corner within the padded
/// viewport inflated by the node's own screen extent keeps the node, so
/// a subtree containing the viewport never prunes; a node with no
/// projectable corner (globe far side) is hidden.
fn node_intersects_viewport(mapper: &ScreenMapper, corners: &[UnitVector3; 3]) -> bool {
    let projected: Vec<[f64; 2]> = corners
        .iter()
        .filter_map(|&corner| mapper.screen(corner))
        .collect();
    if projected.is_empty() {
        return false;
    }
    let mut extent = 0.0_f64;
    for (index, a) in projected.iter().enumerate() {
        for b in &projected[index + 1..] {
            extent = extent.max(mapper.distance_px(*a, *b));
        }
    }
    projected
        .iter()
        .any(|&screen| mapper.span_visible(screen, extent))
}

/// One cached batch mesh in local index space.
struct CachedBatch {
    directions: Vec<[f64; 3]>,
    colors: Vec<[u8; 4]>,
    indices: Vec<u32>,
    leaves: u32,
    last_used: u64,
}

/// The worker-owned incremental batch cache with a leaf-count bound.
#[derive(Default)]
pub(super) struct BatchCache {
    entries: HashMap<DetailBatch, CachedBatch>,
    generation: u64,
    cached_leaves: usize,
}

impl BatchCache {
    fn evict_unused(&mut self) {
        if self.cached_leaves <= CACHE_LEAF_CAPACITY {
            return;
        }
        let generation = self.generation;
        let mut stale: Vec<(DetailBatch, u64, u32)> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.last_used != generation)
            .map(|(key, entry)| (*key, entry.last_used, entry.leaves))
            .collect();
        stale.sort_by_key(|(_, last_used, _)| *last_used);
        for (key, _, leaves) in stale {
            if self.cached_leaves <= CACHE_LEAF_CAPACITY {
                break;
            }
            self.entries.remove(&key);
            self.cached_leaves -= leaves as usize;
        }
    }
}

/// Assembles one selection into a renderable mesh through the cache.
///
/// Deterministic: identical selections yield bit-identical meshes, cold
/// or cached; the cache only accelerates (spec §6 caching semantics).
pub(super) fn build_detail_mesh(
    context: &AmplifiedDetailContext,
    selection: &DetailSelection,
    cache: &mut BatchCache,
) -> Option<AmplifiedSurfaceMesh> {
    cache.generation += 1;
    let mut directions = Vec::with_capacity(selection.leaves * 2);
    let mut colors = Vec::with_capacity(selection.leaves * 2);
    let mut indices = Vec::with_capacity(selection.leaves * TRIANGLE_CORNERS);
    for batch in &selection.batches {
        if !cache.entries.contains_key(batch) {
            let built = build_batch(context, batch)?;
            cache.cached_leaves += built.leaves as usize;
            cache.entries.insert(*batch, built);
        }
        let entry = cache
            .entries
            .get_mut(batch)
            .expect("the batch was just ensured");
        entry.last_used = cache.generation;
        let base = u32::try_from(directions.len()).ok()?;
        directions.extend_from_slice(&entry.directions);
        colors.extend_from_slice(&entry.colors);
        indices.extend(entry.indices.iter().map(|&index| base + index));
    }
    cache.evict_unused();
    AmplifiedSurfaceMesh::new(directions, colors, indices).ok()
}

/// Builds one batch: geometry by recursive midpoint subdivision, colors
/// by the engine's leaf-value stream paired in the shared DFS order.
fn build_batch(context: &AmplifiedDetailContext, batch: &DetailBatch) -> Option<CachedBatch> {
    let cell = CellId::from_raw(batch.cell);
    let corners = descend_prefix(
        context.evaluator.sector_corners(cell, batch.sector),
        batch.prefix.steps(),
    );
    let mut vertices = vec![corners[0], corners[1], corners[2]];
    let mut midpoints = HashMap::new();
    let mut triangles = Vec::with_capacity(batch.leaves() * TRIANGLE_CORNERS);
    subdivide_triangle(
        0,
        1,
        2,
        u32::from(batch.extra),
        &mut vertices,
        &mut midpoints,
        &mut triangles,
    )?;
    let mut values = Vec::with_capacity(batch.leaves());
    context.evaluator.for_each_leaf_value(
        cell,
        batch.sector,
        batch.prefix.steps(),
        batch.extra,
        &mut |value| values.push(value),
    );
    if values.len() * TRIANGLE_CORNERS != triangles.len() {
        return None;
    }
    let mut directions: Vec<[f64; 3]> = vertices
        .iter()
        .map(|direction| direction.components())
        .collect();
    let mut colors = vec![UNREAD_CORNER_COLOR; directions.len()];
    for (leaf, triangle) in triangles.chunks_exact_mut(TRIANGLE_CORNERS).enumerate() {
        let provoking = u32::try_from(directions.len()).ok()?;
        let corner = directions[triangle[0] as usize];
        directions.push(corner);
        colors.push(flat_color(context, values[leaf]));
        triangle[0] = provoking;
    }
    Some(CachedBatch {
        directions,
        colors,
        indices: triangles,
        leaves: batch.leaves() as u32,
        last_used: 0,
    })
}

/// Walks a prefix geometrically with the shared child order and the
/// commutative renormalized-sum midpoint.
fn descend_prefix(mut corners: [UnitVector3; 3], steps: &[u8]) -> [UnitVector3; 3] {
    for &child in steps {
        let ab = direction_midpoint(corners[0], corners[1]);
        let bc = direction_midpoint(corners[1], corners[2]);
        let ca = direction_midpoint(corners[2], corners[0]);
        corners = match child {
            0 => [corners[0], ab, ca],
            1 => [ab, corners[1], bc],
            2 => [ca, bc, corners[2]],
            _ => [ab, bc, ca],
        };
    }
    corners
}

fn direction_midpoint(a: UnitVector3, b: UnitVector3) -> UnitVector3 {
    let [ax, ay, az] = a.components();
    let [bx, by, bz] = b.components();
    UnitVector3::new(ax + bx, ay + by, az + bz)
        .expect("fan subdivision midpoints stay strictly inside one hemisphere")
}

/// Recursive four-way subdivision preserving winding order; leaves emit
/// in the depth-first child order 0, 1, 2, 3 shared with the engine.
fn subdivide_triangle(
    a: u32,
    b: u32,
    c: u32,
    levels: u32,
    vertices: &mut Vec<UnitVector3>,
    midpoints: &mut HashMap<(u32, u32), u32>,
    out: &mut Vec<u32>,
) -> Option<()> {
    if levels == 0 {
        out.extend_from_slice(&[a, b, c]);
        return Some(());
    }
    let ab = midpoint_index(a, b, vertices, midpoints)?;
    let bc = midpoint_index(b, c, vertices, midpoints)?;
    let ca = midpoint_index(c, a, vertices, midpoints)?;
    subdivide_triangle(a, ab, ca, levels - 1, vertices, midpoints, out)?;
    subdivide_triangle(ab, b, bc, levels - 1, vertices, midpoints, out)?;
    subdivide_triangle(ca, bc, c, levels - 1, vertices, midpoints, out)?;
    subdivide_triangle(ab, bc, ca, levels - 1, vertices, midpoints, out)
}

/// Returns the renormalized midpoint, deduplicated per batch.
///
/// The midpoint depends only on the two endpoint direction values through
/// a commutative sum, so all batches sharing a border edge derive
/// bit-identical midpoint chains independently.
fn midpoint_index(
    a: u32,
    b: u32,
    vertices: &mut Vec<UnitVector3>,
    midpoints: &mut HashMap<(u32, u32), u32>,
) -> Option<u32> {
    let key = (a.min(b), a.max(b));
    if let Some(&existing) = midpoints.get(&key) {
        return Some(existing);
    }
    let direction = direction_midpoint(vertices[a as usize], vertices[b as usize]);
    let index = u32::try_from(vertices.len()).ok()?;
    vertices.push(direction);
    midpoints.insert(key, index);
    Some(index)
}

/// One leaf primitive's solid hypsometric color from its face value.
fn flat_color(context: &AmplifiedDetailContext, value: PrimitiveValue) -> [u8; 4] {
    let t = ((f64::from(value.elevation_m) - (context.sea_level_m - context.display_radius_m))
        / (2.0 * context.display_radius_m))
        .clamp(0.0, 1.0);
    let base = sample_palette(built_in_palette(PaletteId::Hypsometric), t as f32);
    let components = base.components();
    [
        encode_srgb(f64::from(components[0])),
        encode_srgb(f64::from(components[1])),
        encode_srgb(f64::from(components[2])),
        255,
    ]
}

fn encode_srgb(linear: f64) -> u8 {
    let clamped = linear.clamp(0.0, 1.0);
    let encoded = if clamped <= 0.003_130_8 {
        12.92 * clamped
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

/// Converts the published river network into display polylines (Task 5).
pub(super) fn river_display_polylines(
    surface: &SphericalSurfaceSnapshot,
    hydrology: &SphericalHydrologySnapshot,
) -> Vec<RiverPolylineSegment> {
    let cells = surface.cells();
    hydrology
        .river_segments()
        .iter()
        .filter_map(|segment| {
            let from = cells
                .get(segment.from().raw() as usize)?
                .centroid
                .components();
            let to = cells
                .get(segment.to().raw() as usize)?
                .centroid
                .components();
            Some(RiverPolylineSegment {
                start: from,
                end: to,
                strahler_order: segment.strahler_order(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generators::natural::{AmplificationFieldsView, LocatedPrimitive};
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::view::{MapCamera, SphericalProjectionKind};
    use crate::world::natural::SphericalOrogenyKind;
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    fn test_context() -> AmplifiedDetailContext {
        let surface = GeodesicVoronoiBuilder::build_cancellable(
            &SphericalSpaceSpec {
                radius: Meters::new(6_371_000.0).unwrap(),
                target_cell_count: 162,
            },
            || false,
        )
        .unwrap();
        let count = surface.cells().len();
        let elevation: Vec<f32> = surface
            .cells()
            .iter()
            .map(|cell| (2_000.0 * cell.centroid.components()[2]) as f32)
            .collect();
        let zeros = vec![0.0_f32; count];
        let ones = vec![1.0_f32; count];
        let kinds = vec![SphericalOrogenyKind::None; count];
        let fields = AmplificationFieldsView {
            final_elevation_m: &elevation,
            sea_level_m: 0.0,
            sediment_thickness_m: &zeros,
            erodibility: &zeros,
            annual_precipitation_mm: &ones,
            crust_age_myr: &zeros,
            lineation_east: &ones,
            lineation_north: &zeros,
            orogeny_kind: &kinds,
            orogeny_age_myr: &zeros,
        };
        AmplifiedDetailContext {
            evaluator: HierarchicalEvaluator::new(&surface, fields, RootSeed::new(7)).unwrap(),
            sea_level_m: 0.0,
            display_radius_m: 2_000.0,
        }
    }

    fn map_view(zoom: f64) -> SphericalPresentationViewState {
        let projection =
            SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
        let mut camera = MapCamera::default();
        assert!(camera.zoom_by(SphericalProjectionKind::EqualEarth, zoom));
        SphericalPresentationViewState::new(
            SphericalViewMode::Map,
            projection,
            camera,
            crate::view::GlobeCamera::default(),
        )
    }

    const CANVAS: [f64; 2] = [1_600.0, 900.0];

    fn leaf_level(batch: &DetailBatch) -> u8 {
        1 + batch.prefix.steps().len() as u8 + batch.extra
    }

    #[test]
    fn initial_selection_is_the_uniform_task4r_density() {
        let context = test_context();
        let selection = initial_selection(&context);
        let sectors: usize = (0..context.evaluator.cell_count() as u32)
            .map(|cell| context.evaluator.sector_count(CellId::from_raw(cell)))
            .sum();
        assert_eq!(selection.batches.len(), sectors);
        assert_eq!(
            selection.leaves,
            sectors * 4_usize.pow(u32::from(INITIAL_UNIFORM_LEAF_LEVEL) - 1)
        );
    }

    #[test]
    fn selection_deepens_with_zoom_and_respects_the_budget() {
        let context = test_context();
        let level_at = |zoom: f64| {
            let selection = select_detail_batches(&context, &map_view(zoom), CANVAS);
            assert!(selection.leaves <= VIEW_LEAF_BUDGET, "budget at {zoom}x");
            assert!(!selection.batches.is_empty());
            selection.batches.iter().map(leaf_level).max().unwrap()
        };
        let global = level_at(1.0);
        let near = level_at(64.0);
        let nearest = level_at(1_024.0);
        assert!(
            global < near && near <= nearest,
            "leaf levels must deepen with zoom: {global} -> {near} -> {nearest}"
        );
        assert!(usize::from(nearest) <= 1 + HIERARCHICAL_PATH_DEPTH_MAX);

        // Deep-zoom containment: with the viewport inside one cell, that
        // cell must stay visible and subdivide deep, not fall off-screen.
        let deepest = level_at(4_096.0);
        assert!(
            deepest >= 10,
            "the viewport-containing cell must subdivide deep, got {deepest}"
        );

        // Zoomed in, the off-viewport majority stays shallow.
        let selection = select_detail_batches(&context, &map_view(1_024.0), CANVAS);
        assert!(selection
            .batches
            .iter()
            .any(|batch| leaf_level(batch) == OFFSCREEN_LEAF_LEVEL));

        // Determinism: the same camera reselects identically.
        let again = select_detail_batches(&context, &map_view(1_024.0), CANVAS);
        assert_eq!(selection.hash, again.hash);
        assert_eq!(selection.batches, again.batches);
    }

    /// Symptom regression (user acceptance, 2026-08-20): the level must be
    /// a location-fair function of physical size — under the equal-area
    /// projection every same-sized cell reads the same level regardless of
    /// its latitude or its ring orientation, so no region can lag behind
    /// its neighbours in a way panning cannot heal.
    #[test]
    fn visible_levels_are_uniform_across_the_equal_area_map() {
        let context = test_context();
        let selection = select_detail_batches(&context, &map_view(1.0), CANVAS);
        let mut per_cell: HashMap<u32, u8> = HashMap::new();
        for batch in &selection.batches {
            let level = leaf_level(batch);
            per_cell
                .entry(batch.cell)
                .and_modify(|current| *current = (*current).max(level))
                .or_insert(level);
        }
        // Polar wedges bend so strongly on this 20°-cell fixture that the
        // straight-edge triangle undercounts their area; the fairness
        // property under test is the low- and mid-latitude field.
        let mut low_mid = per_cell.iter().filter(|(cell, _)| {
            let corners = context
                .evaluator
                .sector_corners(CellId::from_raw(**cell), 0);
            corners[0].components()[2].abs() <= 0.7
        });
        let first = *low_mid.next().unwrap().1;
        let (min, max) = low_mid.fold((first, first), |(min, max), (_, &level)| {
            (min.min(level), max.max(level))
        });
        assert!(
            max - min <= 1,
            "same-sized cells must sit within one ceil bucket, got {min}..{max}"
        );
    }

    /// Symptom regression (user acceptance, 2026-08-20): zooming out only
    /// ever merges what stays on screen — for every cell inside the padded
    /// viewport at two consecutive zooms, the shallower zoom's level is
    /// never finer.
    #[test]
    fn zooming_out_never_refines_visible_cells() {
        let context = test_context();
        let evaluator = &context.evaluator;
        let floor_level = 1
            + (evaluator.cell_spacing_m() / MIN_PRIMITIVE_EDGE_M)
                .log2()
                .ceil()
                .min(HIERARCHICAL_PATH_DEPTH_MAX as f64) as u8;
        let target_px = CANVAS[0] / UNITS_ACROSS_VIEW;
        let mut zooms = Vec::new();
        let mut zoom = 4_096.0_f64;
        while zoom >= 1.0 {
            zooms.push(zoom);
            zoom /= 2.0_f64.sqrt();
        }
        for cell_index in (0..evaluator.cell_count() as u32).step_by(7) {
            let corners = evaluator.sector_corners(CellId::from_raw(cell_index), 0);
            let mut previous: Option<(u8, bool)> = None;
            for &zoom in &zooms {
                let mapper = ScreenMapper::new(&map_view(zoom), CANVAS).unwrap();
                let level = cell_leaf_level(&mapper, corners, target_px, floor_level, 0);
                let inside = mapper
                    .screen(corners[0])
                    .is_some_and(|screen| mapper.distance_to_padded_viewport(screen, 0.0) == 0.0);
                if let Some((previous_level, previous_inside)) = previous {
                    if inside && previous_inside {
                        assert!(
                            level <= previous_level,
                            "cell {cell_index} refined from {previous_level} to {level} \
                             while zooming out to {zoom}x"
                        );
                    }
                }
                previous = Some((level, inside));
            }
        }
    }

    #[test]
    fn global_view_keeps_every_cell_visible() {
        let context = test_context();
        let selection = select_detail_batches(&context, &map_view(1.0), CANVAS);
        assert!(selection.batches.iter().all(|batch| leaf_level(batch) > 0));
        // At zoom 1 the whole outline fits the canvas: nothing may be
        // classified off-screen (seam wrap included).
        assert!(selection
            .batches
            .iter()
            .all(|batch| leaf_level(batch) >= OFFSCREEN_LEAF_LEVEL));
        let deepest = selection.batches.iter().map(leaf_level).max().unwrap();
        assert!(deepest >= 2, "the global view keeps visible detail");
    }

    #[test]
    fn detail_mesh_leaves_carry_engine_values() {
        let context = test_context();
        let mut cache = BatchCache::default();
        let selection = select_detail_batches(&context, &map_view(32.0), CANVAS);
        let mesh = build_detail_mesh(&context, &selection, &mut cache).unwrap();
        assert_eq!(mesh.triangle_count(), selection.leaves);

        // Walk mesh triangles per batch (assembly preserves order) and
        // check a sample of leaves against the engine through locate():
        // geometry, shell, value, and palette all agree end to end.
        let mut triangle = 0_usize;
        for batch in &selection.batches {
            for leaf in 0..batch.leaves() {
                if (triangle + leaf) % 97 == 0 {
                    let base = (triangle + leaf) * TRIANGLE_CORNERS;
                    let indices = mesh.indices();
                    let corner = |slot: usize| mesh.directions()[indices[base + slot] as usize];
                    let [ax, ay, az] = corner(0);
                    let [bx, by, bz] = corner(1);
                    let [cx, cy, cz] = corner(2);
                    let centroid =
                        UnitVector3::new(ax + bx + cx, ay + by + cy, az + bz + cz).unwrap();
                    let located = match context.evaluator.locate(centroid, leaf_level(batch)) {
                        LocatedPrimitive::Cell(cell) => context.evaluator.cell_value(cell),
                        LocatedPrimitive::Triangle { cell, sector, path } => {
                            context.evaluator.value(cell, sector, path.steps())
                        }
                    };
                    let expected = flat_color(&context, located);
                    assert_eq!(
                        mesh.colors()[indices[base] as usize],
                        expected,
                        "leaf color must equal the engine value's palette color"
                    );
                }
            }
            triangle += batch.leaves();
        }
    }

    #[test]
    fn cached_and_cold_builds_are_bit_identical() {
        let context = test_context();
        let selection = select_detail_batches(&context, &map_view(16.0), CANVAS);
        let mut cold_cache = BatchCache::default();
        let cold = build_detail_mesh(&context, &selection, &mut cold_cache).unwrap();
        let warm = build_detail_mesh(&context, &selection, &mut cold_cache).unwrap();
        let mut fresh_cache = BatchCache::default();
        let fresh = build_detail_mesh(&context, &selection, &mut fresh_cache).unwrap();
        for other in [&warm, &fresh] {
            assert_eq!(cold.directions(), other.directions());
            assert_eq!(cold.colors(), other.colors());
            assert_eq!(cold.indices(), other.indices());
        }
        assert!(cold_cache.cached_leaves <= CACHE_LEAF_CAPACITY);
    }
}
