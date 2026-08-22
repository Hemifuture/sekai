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
//! atoms become plainly visible. The level is one explicit function of
//! the camera zoom — the mean §5 primitive size sampled once at the
//! view-centre direction ([`view_leaf_level`]) — so every visible cell
//! displays the same level and zooming in never coarsens the view (user
//! guarantee, 2026-08-21). Off-viewport cells fall off geometrically with
//! distance toward [`OFFSCREEN_LEAF_LEVEL`], very near cells subdivide
//! per subtree so only their visible portion deepens, and everything is
//! bounded by [`VIEW_LEAF_BUDGET`] by demoting only off-viewport cells.
//! Selections compile into batches — one subtree
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
use crate::world::spatial::UnitVector3;
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
    /// Per reach (published segment order): the from/to cell ids, for
    /// looking the reach's display depth up in a selection.
    pub(super) river_cells: Vec<(u32, u32)>,
    /// Per reach: the published Strahler order for scale selection.
    pub(super) river_orders: Vec<u8>,
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

    /// Test-only since rivers read [`DetailSelection::cell_levels`]:
    /// batch levels understate walked cells (hidden subtrees collapse
    /// to shallow single leaves).
    #[cfg(test)]
    fn leaf_level(&self) -> u8 {
        1 + self.prefix.steps().len() as u8 + self.extra
    }
}

/// One camera-resolved selection of batches, hashed for change detection.
pub(super) struct DetailSelection {
    batches: Vec<DetailBatch>,
    /// Total leaf primitives across all batches.
    pub(super) leaves: usize,
    /// Order-sensitive content hash for latest-wins scheduling.
    pub(super) hash: u64,
    /// Every cell's classified leaf level, indexed by cell id — the
    /// nominal level authority for consumers that must not depend on
    /// batch shapes (hidden subtrees collapse to shallow single-leaf
    /// batches, so batch levels understate a walked cell).
    cell_levels: Vec<u8>,
}

fn finish_selection(
    batches: Vec<DetailBatch>,
    leaves: usize,
    cell_levels: Vec<u8>,
) -> DetailSelection {
    let mut hasher = DefaultHasher::new();
    for batch in &batches {
        batch.hash(&mut hasher);
    }
    DetailSelection {
        hash: hasher.finish(),
        batches,
        leaves,
        cell_levels,
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
    let cell_levels = vec![leaf_level; context.evaluator.cell_count()];
    finish_selection(batches, leaves, cell_levels)
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

    /// The world direction under the canvas centre: the map's inverse
    /// projection of the centre pixel (falling back to the outline
    /// centre when the camera pans past the world), or the globe front
    /// direction.
    fn view_center_direction(&self) -> Option<UnitVector3> {
        match self {
            ScreenMapper::Map {
                projection,
                transform,
                ..
            } => {
                let [width, height] = transform.canvas_size();
                let center = transform.to_projection([width * 0.5, height * 0.5]);
                projection.inverse(center).ok().or_else(|| {
                    let bounds = projection.bounds();
                    projection
                        .inverse(ProjectionPoint::new(
                            (bounds.min_x() + bounds.max_x()) * 0.5,
                            (bounds.min_y() + bounds.max_y()) * 0.5,
                        ))
                        .ok()
                })
            }
            ScreenMapper::Globe { camera, .. } => {
                let [x, y, z] = camera.front_direction();
                UnitVector3::new(x, y, z).ok()
            }
        }
    }

    /// Samples the view-centre scale: a probe triangle with orthonormal
    /// [`CENTER_PROBE_RADIANS`] tangent legs at the view-centre direction
    /// is projected (seam-unwrapped on the map), yielding the local area
    /// scale and both leg scales where the user is looking.
    fn center_probe(&self) -> Option<CenterProbe> {
        let center = self.view_center_direction()?;
        let [cx, cy, cz] = center.components();
        let helper = if cx.abs() < 0.6 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let u = [
            cy * helper[2] - cz * helper[1],
            cz * helper[0] - cx * helper[2],
            cx * helper[1] - cy * helper[0],
        ];
        let u_length = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
        let u = [u[0] / u_length, u[1] / u_length, u[2] / u_length];
        let v = [
            cy * u[2] - cz * u[1],
            cz * u[0] - cx * u[2],
            cx * u[1] - cy * u[0],
        ];
        let offset = |tangent: [f64; 3]| {
            UnitVector3::new(
                cx + CENTER_PROBE_RADIANS * tangent[0],
                cy + CENTER_PROBE_RADIANS * tangent[1],
                cz + CENTER_PROBE_RADIANS * tangent[2],
            )
            .ok()
        };
        let a = self.screen(center)?;
        let b = self.screen(offset(u)?)?;
        let c = self.screen(offset(v)?)?;
        let b = [self.unwrap_x_toward(a[0], b[0]), b[1]];
        let c = [self.unwrap_x_toward(a[0], c[0]), c[1]];
        let doubled = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
        let legs = [
            (b[0] - a[0]).hypot(b[1] - a[1]),
            (c[0] - a[0]).hypot(c[1] - a[1]),
        ];
        (doubled.is_finite() && legs.iter().all(|leg| leg.is_finite())).then_some(CenterProbe {
            center,
            doubled_area_px: doubled,
            leg_px: legs,
        })
    }
}

/// The view-centre scale sample backing both the zoom→level authority
/// and the viewport's spherical footprint.
struct CenterProbe {
    /// The unit direction under the canvas centre.
    center: UnitVector3,
    /// Doubled projected area of the ε-leg probe triangle, in px².
    doubled_area_px: f64,
    /// Projected length of each ε tangent leg, in px.
    leg_px: [f64; 2],
}

/// The padded viewport's spherical footprint: visibility and distance
/// tests run against this cap with plain dot products, never against
/// projected spans.
///
/// Screen-space span tests break down at the map projection's poles —
/// Equal Earth maps each pole to a line half the outline wide, so a
/// pole-touching node projects with corners smeared across it and an
/// unbounded "extent" that defeated every span test: the pole ring of
/// cells expanded to millions of leaves regardless of where the camera
/// looked (probe 2026-08-21: 12.9 M of 13.4 M leaves). On the sphere
/// every node is genuinely small, so the cap test is singularity-free —
/// and seam wrap and the globe's far side come out right with no special
/// cases.
struct ViewCap {
    /// The unit direction under the canvas centre.
    center: [f64; 3],
    /// Conservative angular radius of the padded viewport footprint.
    radius_rad: f64,
    /// The angular span of one viewport width at the centre scale — the
    /// distance-falloff octave unit.
    width_rad: f64,
}

impl ViewCap {
    /// Safety factor on the footprint radius: covers the probe legs not
    /// aligning with the projection's principal axes (≤ √2) and the
    /// scale drifting away from the centre across the footprint.
    const RADIUS_SAFETY: f64 = 2.0;

    /// Builds the footprint from the centre probe; without a probe the
    /// whole sphere is visible (the conservative fallback).
    ///
    /// The *smaller* leg scale converts pixels to angle, which yields
    /// the *larger* — conservative — radius; at a pole-centred viewport
    /// the longitude-smeared leg is the large one, so the footprint
    /// stays anchored to the healthy meridian scale.
    fn new(probe: Option<&CenterProbe>, canvas_size: [f64; 2]) -> Self {
        let Some(probe) = probe else {
            return Self {
                center: [0.0, 0.0, 1.0],
                radius_rad: std::f64::consts::PI,
                width_rad: std::f64::consts::FRAC_PI_2,
            };
        };
        let min_px_per_rad =
            (probe.leg_px[0].min(probe.leg_px[1]) / CENTER_PROBE_RADIANS).max(f64::MIN_POSITIVE);
        let [width, height] = canvas_size;
        let padded_half_diagonal = 0.5
            * (width * (1.0 + 2.0 * VIEW_MARGIN_FRACTION))
                .hypot(height * (1.0 + 2.0 * VIEW_MARGIN_FRACTION));
        Self {
            center: probe.center.components(),
            radius_rad: (Self::RADIUS_SAFETY * padded_half_diagonal / min_px_per_rad)
                .min(std::f64::consts::PI),
            width_rad: (width / min_px_per_rad).min(std::f64::consts::PI),
        }
    }

    /// The angle between the cap centre and one unit direction.
    fn angle_to(&self, direction: UnitVector3) -> f64 {
        let [x, y, z] = direction.components();
        (self.center[0] * x + self.center[1] * y + self.center[2] * z)
            .clamp(-1.0, 1.0)
            .acos()
    }
}

/// Resolves the camera-driven selection for the active view.
///
/// Every visible cell displays the zoom-mapped [`view_leaf_level`];
/// off-viewport cells stay shallow; cells past
/// [`CELL_UNIFORM_MAX_LEVEL`] subdivide per subtree with hidden subtrees
/// pruned coarse. When the budget overflows, only the off-viewport
/// falloff demotes until it fits — visible levels are never reduced, so
/// zooming in cannot merge the view (user guarantee, 2026-08-21).
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
    let probe = mapper.center_probe();
    let view_level = view_leaf_level(evaluator, probe.as_ref(), target_px, floor_level);
    let cap = ViewCap::new(probe.as_ref(), canvas_size);
    let mut shrink = 0_u8;
    loop {
        let (batches, leaves, cell_levels) =
            classify_cells_striped(context, &cap, view_level, shrink);
        if leaves <= VIEW_LEAF_BUDGET || shrink >= view_level {
            return finish_selection(batches, leaves, cell_levels);
        }
        shrink += 1;
    }
}

/// Probe leg angle for the view-centre scale sample: small enough to
/// read the local projection scale, large enough for well-conditioned
/// f64 differences at every supported zoom.
const CENTER_PROBE_RADIANS: f64 = 1e-3;

/// The single zoom↔level authority: the leaf level every visible cell
/// displays under the current camera (user requirement, 2026-08-21).
///
/// The mean spec §5 primitive size is sampled once at the view-centre
/// direction: a probe triangle with orthonormal [`CENTER_PROBE_RADIANS`]
/// tangent legs reads the local px²-per-steradian scale where the user
/// is looking (exact everywhere under the equal-area map; centre-anchored
/// on the globe and the equirectangular map so the focused region sits on
/// the physical ladder). One level engages per zoom octave, clamped to
/// the physical floor. Deriving the level from the camera zoom alone —
/// instead of per-cell measurements — removes per-cell threshold
/// patchwork and makes the level monotone in zoom by construction.
fn view_leaf_level(
    evaluator: &HierarchicalEvaluator,
    probe: Option<&CenterProbe>,
    target_px: f64,
    floor_level: u8,
) -> u8 {
    let Some(doubled_area_px) = probe.map(|probe| probe.doubled_area_px) else {
        return 1;
    };
    let total_sectors: usize = (0..evaluator.cell_count() as u32)
        .map(|cell| evaluator.sector_count(CellId::from_raw(cell)))
        .sum();
    if total_sectors == 0 {
        return 1;
    }
    // The mean doubled sector area is 8π/N steradians; the probe scale
    // turns it into the same `√(2·area)` pixel measure the classifier
    // used per cell before. The sphere radius cancels.
    let size_px = (8.0 * std::f64::consts::PI / total_sectors as f64 * doubled_area_px).sqrt()
        / CENTER_PROBE_RADIANS;
    if !size_px.is_finite() || size_px <= target_px {
        return 1;
    }
    (1.0 + (size_px / target_px).log2().ceil()).clamp(1.0, f64::from(floor_level)) as u8
}

/// One classification pass over every cell, striped across the cores
/// in contiguous chunks (chunk `c` goes to worker `c mod workers`, so
/// the deep-zoom hotspot spreads); chunks merge back in index order,
/// keeping the batch order — and therefore the selection hash —
/// bit-identical to the sequential pass.
fn classify_cells_striped(
    context: &AmplifiedDetailContext,
    cap: &ViewCap,
    view_level: u8,
    shrink: u8,
) -> (Vec<DetailBatch>, usize, Vec<u8>) {
    const SELECT_CHUNKS: usize = 64;
    let evaluator = &context.evaluator;
    let cell_count = evaluator.cell_count();
    let chunk_len = cell_count.div_ceil(SELECT_CHUNKS).max(1);
    let chunk_count = cell_count.div_ceil(chunk_len);
    let classify_chunk = |chunk: usize| {
        let mut batches = Vec::new();
        let mut leaves = 0_usize;
        let start = chunk * chunk_len;
        let end = (start + chunk_len).min(cell_count);
        let mut levels = Vec::with_capacity(end - start);
        for cell_index in start as u32..end as u32 {
            let cell = CellId::from_raw(cell_index);
            let corners = evaluator.sector_corners(cell, 0);
            let level = cell_leaf_level(cap, corners, view_level, shrink);
            levels.push(level);
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
                        cap,
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
        (batches, leaves, levels)
    };
    let workers = std::thread::available_parallelism()
        .map(|cores| cores.get().saturating_sub(1).max(1))
        .unwrap_or(1)
        .min(chunk_count)
        .min(12);
    type ChunkResult = (Vec<DetailBatch>, usize, Vec<u8>);
    let chunks: Vec<ChunkResult> = if workers <= 1 {
        (0..chunk_count).map(classify_chunk).collect()
    } else {
        let mut striped: Vec<Vec<ChunkResult>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|worker| {
                    let classify_chunk = &classify_chunk;
                    scope.spawn(move || {
                        (worker..chunk_count)
                            .step_by(workers)
                            .map(classify_chunk)
                            .collect()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("a cell classifier thread panicked"))
                .collect()
        });
        (0..chunk_count)
            .map(|chunk| std::mem::take(&mut striped[chunk % workers][chunk / workers]))
            .collect()
    };
    let mut batches = Vec::with_capacity(chunks.iter().map(|(chunk, _, _)| chunk.len()).sum());
    let mut leaves = 0_usize;
    let mut cell_levels = Vec::with_capacity(cell_count);
    for (chunk_batches, chunk_leaves, chunk_levels) in chunks {
        batches.extend(chunk_batches);
        leaves += chunk_leaves;
        cell_levels.extend(chunk_levels);
    }
    (batches, leaves, cell_levels)
}

/// One cell's leaf level: the zoom-mapped `view_level` when the cell
/// reaches the viewport footprint cap, the distance falloff below it
/// when it does not — all measured on the sphere, so the map
/// projection's poles and seam and the globe's far side need no cases.
///
/// Off-viewport cells fall off smoothly instead of collapsing to the
/// floor: [`DISTANCE_FALLOFF_LEVELS_PER_OCTAVE`] levels per doubling of
/// their distance in viewport widths (chunked-LOD distance falloff), so
/// zoom-out reveals and pans land on near-correct coarse content while
/// the far side of the world still costs almost nothing. The budget
/// `shrink` demotes only this off-viewport arm; a visible cell always
/// keeps the full `view_level`.
fn cell_leaf_level(cap: &ViewCap, corners: [UnitVector3; 3], view_level: u8, shrink: u8) -> u8 {
    // Twice the first sector's circumradius covers the whole cell (the
    // sector fans from the cell anchor), so a cell containing the
    // deep-zoom viewport measures distance zero.
    let circumradius_rad =
        angle_between(corners[0], corners[1]).max(angle_between(corners[0], corners[2])) * 2.0;
    let distance_rad = cap.angle_to(corners[0]) - circumradius_rad - cap.radius_rad;
    if distance_rad <= 0.0 {
        return view_level;
    }
    let falloff =
        (DISTANCE_FALLOFF_LEVELS_PER_OCTAVE * (1.0 + distance_rad / cap.width_rad).log2()).ceil();
    ((f64::from(view_level) - falloff).max(f64::from(OFFSCREEN_LEAF_LEVEL)) as u8)
        .saturating_sub(shrink)
        .max(OFFSCREEN_LEAF_LEVEL)
}

/// The angle in radians between two unit directions.
fn angle_between(a: UnitVector3, b: UnitVector3) -> f64 {
    let [ax, ay, az] = a.components();
    let [bx, by, bz] = b.components();
    (ax * bx + ay * by + az * bz).clamp(-1.0, 1.0).acos()
}

/// Emits the sub-cell batches of one very near sector: visible subtrees
/// split until [`WALK_BATCH_EXTRA`] levels above the target, hidden
/// subtrees emit one coarse leaf so the mosaic stays complete.
fn walk_sector_subtrees(
    cap: &ViewCap,
    corners: [UnitVector3; 3],
    cell: u32,
    sector: u8,
    target_level: u8,
    batches: &mut Vec<DetailBatch>,
    leaves: &mut usize,
) {
    let mut prefix = [0_u8; 16];
    walk_node(
        cap,
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
    cap: &ViewCap,
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
    // Visibility prunes before the depth check emits: a hidden subtree
    // collapses to one coarse leaf at any depth. With the checks the
    // other way round, every node reached at `target − WALK_BATCH_EXTRA`
    // emitted its full 4^extra block unseen, and a deep viewport paid
    // ~256 leaves apiece for strips of off-screen nodes — the budget
    // overflow that used to demote the whole view (probe, 2026-08-21:
    // 3.2M of 3.7M leaves at maximum zoom).
    if !node_intersects_viewport(cap, &corners) {
        emit(0, batches, leaves);
        return;
    }
    if node_level + WALK_BATCH_EXTRA >= target_level {
        emit(target_level - node_level, batches, leaves);
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
            cap,
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

/// Conservative spherical visibility: the node's anchored circumcap
/// (every triangle point lies within the anchor-to-corner angles)
/// against the viewport footprint cap. A subtree containing the view
/// centre never prunes — the centre's angle to the anchor is at most
/// the circumradius. Plain dot products, no projection: immune to the
/// map's pole singularity and seam and to the globe's far side.
fn node_intersects_viewport(cap: &ViewCap, corners: &[UnitVector3; 3]) -> bool {
    let circumradius_rad =
        angle_between(corners[0], corners[1]).max(angle_between(corners[0], corners[2]));
    cap.angle_to(corners[0]) <= circumradius_rad + cap.radius_rad
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
/// Missing batches build in parallel across the available cores — each
/// batch is a pure function of the context, so striping only shares
/// the load, never the outcome.
pub(super) fn build_detail_mesh(
    context: &AmplifiedDetailContext,
    selection: &DetailSelection,
    cache: &mut BatchCache,
) -> Option<AmplifiedSurfaceMesh> {
    cache.generation += 1;
    let missing: Vec<DetailBatch> = selection
        .batches
        .iter()
        .filter(|batch| !cache.entries.contains_key(batch))
        .copied()
        .collect();
    for (batch, built) in missing
        .iter()
        .zip(build_batches_striped(context, &missing)?)
    {
        cache.cached_leaves += built.leaves as usize;
        cache.entries.insert(*batch, built);
    }
    let mut directions = Vec::with_capacity(selection.leaves * 2);
    let mut colors = Vec::with_capacity(selection.leaves * 2);
    let mut indices = Vec::with_capacity(selection.leaves * TRIANGLE_CORNERS);
    for batch in &selection.batches {
        let entry = cache
            .entries
            .get_mut(batch)
            .expect("missing batches were just built");
        entry.last_used = cache.generation;
        let base = u32::try_from(directions.len()).ok()?;
        directions.extend_from_slice(&entry.directions);
        colors.extend_from_slice(&entry.colors);
        indices.extend(entry.indices.iter().map(|&index| base + index));
    }
    cache.evict_unused();
    AmplifiedSurfaceMesh::new(directions, colors, indices).ok()
}

/// Builds the missing batches with interleaved striping over the
/// available cores: batch `i` goes to worker `i mod workers`, so
/// river-dense hotspots spread instead of landing on one straggler.
/// Results return in input order; per-reach path memo locks serialize
/// same-reach materialization across workers and share it after.
fn build_batches_striped(
    context: &AmplifiedDetailContext,
    batches: &[DetailBatch],
) -> Option<Vec<CachedBatch>> {
    let workers = std::thread::available_parallelism()
        .map(|cores| cores.get().saturating_sub(1).max(1))
        .unwrap_or(1)
        .min(batches.len())
        .min(12);
    if workers <= 1 {
        return batches
            .iter()
            .map(|batch| build_batch(context, batch))
            .collect();
    }
    let mut striped: Vec<Vec<Option<CachedBatch>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                scope.spawn(move || {
                    batches
                        .iter()
                        .skip(worker)
                        .step_by(workers)
                        .map(|batch| build_batch(context, batch))
                        .collect()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("a batch builder thread panicked"))
            .collect()
    });
    let mut ordered = Vec::with_capacity(batches.len());
    for index in 0..batches.len() {
        ordered.push(striped[index % workers][index / workers].take()?);
    }
    Some(ordered)
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

/// Builds display river polylines for one terrain selection. Each deeper
/// leaf level reveals one lower Strahler tier; visible reaches use the
/// deeper endpoint's path depth so their geometry follows the terrain.
pub(super) fn build_river_polylines(
    context: &AmplifiedDetailContext,
    selection: &DetailSelection,
) -> Vec<RiverPolylineSegment> {
    if context.river_cells.is_empty() {
        return Vec::new();
    }
    // The reach follows the classified level of its deeper endpoint cell
    // (never the batch shapes: a walked cell's hidden subtrees collapse
    // to shallow single-leaf batches, and at deep zoom one endpoint of an
    // 80 km reach is always off-screen — reading batches made the river
    // coarser than its terrain and unstable under small pans). The
    // deeper endpoint governs so the visible portion always matches the
    // terrain; the off-screen half over-subdivides harmlessly (paths are
    // memoized per reach).
    let cell_level = |cell: u32| {
        selection
            .cell_levels
            .get(cell as usize)
            .copied()
            .unwrap_or(1)
    };
    let max_order = context.river_orders.iter().copied().max().unwrap_or(0);
    let mut polylines = Vec::with_capacity(context.river_cells.len());
    for (reach, (&(from, to), &order)) in context
        .river_cells
        .iter()
        .zip(&context.river_orders)
        .enumerate()
    {
        let level = cell_level(from).max(cell_level(to));
        if !river_order_is_visible(order, level, max_order) {
            continue;
        }
        let width_m = context
            .evaluator
            .river_width_m(reach as u32)
            .expect("the reach metadata is evaluator-aligned");
        let path = context
            .evaluator
            .river_path(reach as u32, level.saturating_sub(1));
        for pair in path.windows(2) {
            polylines.push(RiverPolylineSegment {
                start: pair[0].components(),
                end: pair[1].components(),
                width_m,
                strahler_order: order,
            });
        }
    }
    polylines
}

/// Multi-scale river selection: level one keeps only the trunk order and
/// every deeper terrain level reveals exactly one lower order.
fn river_order_is_visible(order: u8, leaf_level: u8, max_order: u8) -> bool {
    u16::from(order) + u16::from(leaf_level.saturating_sub(1)) >= u16::from(max_order)
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
            river_cells: Vec::new(),
            river_orders: Vec::new(),
        }
    }

    /// A context with a two-reach chain for the polyline builder tests.
    fn river_context() -> AmplifiedDetailContext {
        use crate::world::natural::{
            RiverSegment, RiverSegmentKind, SurfaceWaterField, SurfaceWaterKind,
        };
        use crate::world::RiverSegmentId;

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
        let edge = &surface.edges()[0];
        let (a, b) = (edge.cells[0], edge.cells[1]);
        let next = surface
            .edges()
            .iter()
            .find(|candidate| candidate.cells.contains(&b) && !candidate.cells.contains(&a))
            .unwrap();
        let c = if next.cells[0] == b {
            next.cells[1]
        } else {
            next.cells[0]
        };
        let segments = vec![
            RiverSegment::new(
                RiverSegmentId::from_raw(0),
                a,
                b,
                RiverSegmentKind::Channel,
                1,
                8.0,
            )
            .unwrap(),
            RiverSegment::new(
                RiverSegmentId::from_raw(1),
                b,
                c,
                RiverSegmentKind::Channel,
                2,
                90.0,
            )
            .unwrap(),
        ];
        let evaluator = HierarchicalEvaluator::new(&surface, fields, RootSeed::new(7))
            .unwrap()
            .with_rivers(
                &surface,
                &segments,
                &SurfaceWaterField::from_kinds(vec![
                    SurfaceWaterKind::DryLand;
                    surface.cells().len()
                ]),
            )
            .unwrap();
        AmplifiedDetailContext {
            evaluator,
            sea_level_m: 0.0,
            display_radius_m: 2_000.0,
            river_cells: segments
                .iter()
                .map(|segment| (segment.from().raw(), segment.to().raw()))
                .collect(),
            river_orders: segments
                .iter()
                .map(|segment| segment.strahler_order())
                .collect(),
        }
    }

    /// Amendments A6.6/A10: river polylines follow the selection's cell
    /// levels while preserving portal-split dry-land legs.
    #[test]
    fn river_polylines_follow_selection_levels() {
        let context = river_context();
        let coarse = build_river_polylines(&context, &uniform_selection(&context, 1));
        let max_order = context.river_orders.iter().copied().max().unwrap();
        let coarse_expected: usize = context
            .river_orders
            .iter()
            .enumerate()
            .filter(|&(_, &order)| river_order_is_visible(order, 1, max_order))
            .map(|(reach, _)| context.evaluator.river_path(reach as u32, 0).len() - 1)
            .sum();
        assert_eq!(coarse.len(), coarse_expected, "level one keeps the trunk");
        let deeper = build_river_polylines(&context, &uniform_selection(&context, 4));
        let deep_expected: usize = (0..context.evaluator.river_reach_count() as u32)
            .map(|reach| context.evaluator.river_path(reach, 3).len() - 1)
            .sum();
        assert_eq!(deeper.len(), deep_expected);
        for (first, second) in coarse.iter().zip(&coarse) {
            assert_eq!(first, second);
        }
        let again = build_river_polylines(&context, &uniform_selection(&context, 4));
        assert_eq!(deeper, again, "polylines are deterministic");
        // Chain junction stays welded: reach 0 ends where reach 1 begins.
        let first_reach_segments = context.evaluator.river_path(0, 3).len() - 1;
        let mid = deeper[first_reach_segments - 1].end;
        assert_eq!(mid, deeper[first_reach_segments].start);
    }

    #[test]
    fn river_polylines_carry_the_production_reach_width() {
        let context = river_context();
        let polylines = build_river_polylines(&context, &uniform_selection(&context, 4));
        let mut cursor = 0;
        for reach in 0..context.evaluator.river_reach_count() as u32 {
            let segment_count = context.evaluator.river_path(reach, 3).len() - 1;
            let width_m = context
                .evaluator
                .river_width_m(reach)
                .expect("the production reach owns one physical width");
            assert!(polylines[cursor..cursor + segment_count]
                .iter()
                .all(|segment| segment.width_m.to_bits() == width_m.to_bits()));
            cursor += segment_count;
        }
        assert_eq!(cursor, polylines.len());
    }

    #[test]
    fn river_visibility_reveals_one_order_per_level() {
        let visible = |level| {
            (1_u8..=4)
                .filter(|&order| river_order_is_visible(order, level, 4))
                .collect::<Vec<_>>()
        };
        assert_eq!(visible(1), vec![4]);
        assert_eq!(visible(2), vec![3, 4]);
        assert_eq!(visible(3), vec![2, 3, 4]);
        assert_eq!(visible(4), vec![1, 2, 3, 4]);
    }

    #[test]
    fn visible_river_selection_keeps_downstream_continuity() {
        let downstream_orders = [1_u8, 1, 2, 3, 3, 4];
        for level in 1..=4 {
            for start in 0..downstream_orders.len() {
                if river_order_is_visible(downstream_orders[start], level, 4) {
                    assert!(downstream_orders[start..]
                        .iter()
                        .all(|&order| river_order_is_visible(order, level, 4)));
                }
            }
        }
    }

    /// Symptom regression (user acceptance, 2026-08-21): a reach follows
    /// the classified level of its *deeper* endpoint cell. At deep zoom
    /// one endpoint of an 80 km reach is always off-screen; reading the
    /// batch soup (or the shallower endpoint) rendered rivers coarser
    /// than their terrain and unstable under small pans.
    #[test]
    fn river_depth_follows_the_deeper_endpoint() {
        let context = river_context();
        let (from, _) = context.river_cells[0];
        let mut cell_levels = vec![1_u8; context.evaluator.cell_count()];
        cell_levels[from as usize] = 4;
        let selection = DetailSelection {
            batches: Vec::new(),
            leaves: 0,
            hash: 0,
            cell_levels,
        };
        let polylines = build_river_polylines(&context, &selection);
        // Reach 0 renders at its deeper endpoint (level 4 → depth 3)
        // although its other endpoint sits at level 1; reach 1 touches
        // no deep cell and stays at its portal-split depth-zero path.
        let expected = context.evaluator.river_path(0, 3).len() - 1
            + context.evaluator.river_path(1, 0).len()
            - 1;
        assert_eq!(polylines.len(), expected);
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
            selection
                .batches
                .iter()
                .map(DetailBatch::leaf_level)
                .max()
                .unwrap()
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
            .any(|batch| batch.leaf_level() == OFFSCREEN_LEAF_LEVEL));

        // Determinism: the same camera reselects identically.
        let again = select_detail_batches(&context, &map_view(1_024.0), CANVAS);
        assert_eq!(selection.hash, again.hash);
        assert_eq!(selection.batches, again.batches);
    }

    /// Symptom regression (user acceptance, 2026-08-20, strengthened
    /// 2026-08-21): the level is one explicit function of the camera —
    /// with the whole world in the viewport, every cell (polar wedges
    /// included) displays exactly the same level, no per-cell threshold
    /// patchwork.
    #[test]
    fn visible_levels_are_uniform_across_the_equal_area_map() {
        let context = test_context();
        let selection = select_detail_batches(&context, &map_view(1.0), CANVAS);
        let mut per_cell: HashMap<u32, u8> = HashMap::new();
        for batch in &selection.batches {
            let level = batch.leaf_level();
            per_cell
                .entry(batch.cell)
                .and_modify(|current| *current = (*current).max(level))
                .or_insert(level);
        }
        let mut levels = per_cell.values();
        let first = *levels.next().unwrap();
        assert!(
            levels.all(|&level| level == first),
            "the global view must display one uniform level"
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
                let probe = mapper.center_probe();
                let view_level = view_leaf_level(evaluator, probe.as_ref(), target_px, floor_level);
                let cap = ViewCap::new(probe.as_ref(), CANVAS);
                let level = cell_leaf_level(&cap, corners, view_level, 0);
                let inside = cap.angle_to(corners[0]) <= cap.radius_rad;
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

    /// Symptom regression (user acceptance, 2026-08-21): zooming in must
    /// never merge the view. Through the full selector — budget pass
    /// included — the deepest displayed level equals the zoom-mapped view
    /// level exactly at every zoom, monotone all the way up the ladder to
    /// the physical floor: the budget may demote only off-viewport cells.
    #[test]
    fn zooming_in_never_coarsens_and_reaches_the_ladder_floor() {
        let context = test_context();
        let evaluator = &context.evaluator;
        let floor_level = (1.0
            + (evaluator.cell_spacing_m() / MIN_PRIMITIVE_EDGE_M)
                .log2()
                .ceil())
        .clamp(1.0, 1.0 + HIERARCHICAL_PATH_DEPTH_MAX as f64) as u8;
        let target_px = CANVAS[0] / UNITS_ACROSS_VIEW;
        let mut previous_max = 0_u8;
        let mut zoom = 1.0_f64;
        while zoom <= MapCamera::MAX_ZOOM {
            let view = map_view(zoom);
            let mapper = ScreenMapper::new(&view, CANVAS).unwrap();
            let view_level = view_leaf_level(
                evaluator,
                mapper.center_probe().as_ref(),
                target_px,
                floor_level,
            );
            let selection = select_detail_batches(&context, &view, CANVAS);
            assert!(selection.leaves <= VIEW_LEAF_BUDGET, "budget at {zoom}x");
            let max_level = selection
                .batches
                .iter()
                .map(DetailBatch::leaf_level)
                .max()
                .unwrap();
            assert_eq!(
                max_level, view_level,
                "the deepest displayed level must be the zoom-mapped level at {zoom}x"
            );
            assert!(
                max_level >= previous_max,
                "zooming in to {zoom}x merged {previous_max} -> {max_level}"
            );
            previous_max = max_level;
            zoom *= 2.0_f64.sqrt();
        }
        assert_eq!(
            previous_max, floor_level,
            "maximum zoom must reach the physical ladder floor"
        );
    }

    #[test]
    fn global_view_keeps_every_cell_visible() {
        let context = test_context();
        let selection = select_detail_batches(&context, &map_view(1.0), CANVAS);
        assert!(selection.batches.iter().all(|batch| batch.leaf_level() > 0));
        // At zoom 1 the whole outline fits the canvas: nothing may be
        // classified off-screen (seam wrap included).
        assert!(selection
            .batches
            .iter()
            .all(|batch| batch.leaf_level() >= OFFSCREEN_LEAF_LEVEL));
        let deepest = selection
            .batches
            .iter()
            .map(DetailBatch::leaf_level)
            .max()
            .unwrap();
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
                    let located = match context.evaluator.locate(centroid, batch.leaf_level()) {
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
