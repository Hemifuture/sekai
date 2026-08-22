//! Hierarchical river rerouting and meandering (plan M2 Task 4).
//!
//! Implements spec §10 amendment A6: river existence, topology,
//! discharge, and the monotone beds stay L0/P5 authority (M1 §7 and A4
//! untouched); levels below only refine the channel geometry along binary
//! path trees congruent with the primitive tree. Each L0 reach is split at
//! its authoritative shared-edge portal into dry-land sector legs, whose
//! fixed endpoints recursively receive lateral midpoint displacement —
//! candidates scored by the uncarved
//! derived field plus seeded jitter (valley-following on slopes, free
//! meandering on plains) — with the amplitude cut off below half the
//! Leopold–Wolman meander wavelength (≈12 channel widths) and the whole
//! path confined to the parent chord's corridor. The §4 carve reads the
//! level's path through a bound-pruned descent of the same tree, so
//! valleys and displayed channels stay one geometry.

use std::sync::{Arc, RwLock};

use super::hierarchical_derivation::{signed_unit_noise, HierarchicalEvaluator};
use super::terrain_amplification::{
    arc_angle, arc_nearest_point, spherical_triangle_contains, spherical_triangle_margin, RiverLeg,
    RiverReach, TerrainAmplifier,
};
use crate::world::spatial::UnitVector3;

/// Lateral displacement amplitude as a fraction of the sub-segment
/// length (sinuosity starting value; amendment A6.2).
pub(super) const MEANDER_SINUOSITY_FRACTION: f64 = 0.25;
/// Meander wavelength in channel widths (Leopold & Wolman 1960 band
/// 10–14, mid value; amendment A6.2).
pub(super) const MEANDER_WAVELENGTH_PER_WIDTH: f64 = 12.0;
/// Seeded score jitter in metres: dominates on flat ground (free
/// meandering), loses to real valley relief (amendment A6.3).
pub(super) const MEANDER_GUIDANCE_JITTER_M: f64 = 20.0;
/// Hard corridor: no path point strays farther than this fraction of
/// the L0 chord length from the chord's great circle (amendment A6.4).
pub(super) const MEANDER_CORRIDOR_FRACTION: f64 = 0.25;
/// Guidance sampling level for a node at depth d: `min(d + 2, cap)` —
/// two levels finer than the scale being decided, cost-bounded, and
/// independent of who queries the tree (amendment A6.3).
const GUIDANCE_LEVEL_AHEAD: u8 = 2;
const GUIDANCE_LEVEL_CAP: u8 = 8;
/// Path depth never exceeds the primitive path cap.
const PATH_DEPTH_LIMIT: u8 = 16;
/// Smooth refinement below the meander cutoff (amendment A8): levels of
/// four-point interpolatory subdivision past the stochastic cap, so a
/// close-up channel reads as the smooth bend the meander vertices trace
/// (Langbein–Leopold sine-generated curves) instead of λ/2 chords.
const SMOOTH_DEPTH_EXTRA: u8 = 4;
/// Multiplicative and absolute safety margins on the per-node deviation
/// bounds (floating-point slack; keeps pruning provably conservative).
const NODE_BOUND_SAFETY: f64 = 1.0 + 1.0e-9;
const NODE_BOUND_SAFETY_M: f64 = 1.0e-6;

/// One leg's materialized path tree: the node points of the deepest
/// depth so far (`2^d + 1`) and, for every internal node (identified by
/// its midpoint index), a proven upper bound in metres on how far any
/// deeper path point strays from that node's chord — the exact pruning
/// bound of the nearest-point descent.
pub(super) struct ReachPathData {
    points: Vec<[f64; 3]>,
    bounds: Vec<f64>,
}

/// Lazy per-reach memo of the two possible A10 leg path trees.
///
/// The tree is a pure function of the reach and the frozen seeds, and
/// deeper levels never move shallower nodes, so one array serves every
/// depth up to its own through a stride — and deepening only computes
/// the new levels' midpoints. Purely an accelerator (spec §6): every
/// consumer keeps deriving exactly the values the uncached recursion
/// produced.
pub(super) struct ReachPathCache {
    deepest: [RwLock<Option<Arc<ReachPathData>>>; 2],
}

impl Default for ReachPathCache {
    fn default() -> Self {
        Self {
            deepest: std::array::from_fn(|_| RwLock::new(None)),
        }
    }
}

impl ReachPathCache {
    fn leg(&self, leg_index: usize) -> Option<&RwLock<Option<Arc<ReachPathData>>>> {
        self.deepest.get(leg_index)
    }
}

/// One cache slot per reach, all empty (engine construction).
pub(super) fn fresh_reach_path_caches(reach_count: usize) -> Vec<ReachPathCache> {
    (0..reach_count)
        .map(|_| ReachPathCache::default())
        .collect()
}

/// The deepest refinable depth of one reach: the stochastic meander cap
/// plus the A8 smooth-refinement levels (degenerate reaches stay flat).
fn smooth_depth_cap(evaluator: &HierarchicalEvaluator, reach_index: u32) -> u8 {
    evaluator
        .amplifier()
        .river_reaches()
        .get(reach_index as usize)
        .map_or(0, |reach| {
            reach
                .legs
                .iter()
                .flatten()
                .map(|leg| smooth_leg_depth_cap(evaluator, reach, leg))
                .max()
                .unwrap_or(0)
        })
}

fn smooth_leg_depth_cap(
    evaluator: &HierarchicalEvaluator,
    reach: &RiverReach,
    leg: &RiverLeg,
) -> u8 {
    let cap = leg_depth_cap(evaluator, reach, leg);
    if cap == 0 {
        0
    } else {
        (cap + SMOOTH_DEPTH_EXTRA).min(PATH_DEPTH_LIMIT)
    }
}

/// The A8 smooth midpoint of segment `k → k+1`: four-point
/// interpolatory subdivision (Dyn–Levin–Gregory 1987, w = 1/16) with
/// clamped endpoints, renormalized onto the sphere. Deterministic,
/// keeps every existing vertex, and converges to a C¹ curve through
/// the meander points.
fn four_point_midpoint(points: &[[f64; 3]], k: usize) -> [f64; 3] {
    let p0 = points[k.saturating_sub(1)];
    let p1 = points[k];
    let p2 = points[k + 1];
    let p3 = points[(k + 2).min(points.len() - 1)];
    normalized([
        9.0 * (p1[0] + p2[0]) - (p0[0] + p3[0]),
        9.0 * (p1[1] + p2[1]) - (p0[1] + p3[1]),
        9.0 * (p1[2] + p2[2]) - (p0[2] + p3[2]),
    ])
    .or_else(|| normalized(add(p1, p2)))
    .expect("reach sub-segments stay strictly inside one hemisphere")
}

/// Proven per-node deviation bounds, bottom-up: for the node spanning
/// `i..j` with midpoint `m`, every deeper path point stays within
/// `d(points[m], chord(i,j)) + max(child bounds)` of the node's chord —
/// the distance to a short great-arc chord is quasi-convex along
/// another short arc, so a child chord's worst deviation is at its
/// endpoints, which are the node's endpoints (0) and its midpoint.
fn node_bounds(points: &[[f64; 3]], radius_m: f64) -> Vec<f64> {
    let mut bounds = vec![0.0_f64; points.len()];
    let mut span = 2_usize;
    while span < points.len() {
        let mut start = 0_usize;
        while start + span < points.len() {
            let end = start + span;
            let middle = (start + end) / 2;
            let lateral = arc_nearest_point(points[start], points[end], points[middle], radius_m)
                .map_or(0.0, |(lateral, _)| lateral);
            let children = if span >= 4 {
                bounds[(start + middle) / 2].max(bounds[(middle + end) / 2])
            } else {
                0.0
            };
            bounds[middle] = (lateral + children) * NODE_BOUND_SAFETY + NODE_BOUND_SAFETY_M;
            start = end;
        }
        span *= 2;
    }
    bounds
}

/// The reach's path data at `depth` or deeper (`depth` pre-clamped to
/// the smooth cap and ≥ 1), deepening the memo level by level:
/// stochastic meander midpoints up to the wavelength cap (A6), smooth
/// interpolatory midpoints beyond it (A8), then fresh node bounds.
fn cached_path(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    leg_index: usize,
    reach: &RiverReach,
    leg: &RiverLeg,
    depth: u8,
) -> Arc<ReachPathData> {
    let cache = evaluator
        .river_path_slot(reach_index)
        .expect("reach indices come from the engine's own reach lists");
    let slot = cache
        .leg(leg_index)
        .expect("a reach has at most two authoritative legs");
    let needed = (1_usize << depth) + 1;
    let lock_clean = "the reach path lock only guards infallible derivation";
    if let Some(data) = slot
        .read()
        .expect(lock_clean)
        .as_ref()
        .filter(|data| data.points.len() >= needed)
    {
        return Arc::clone(data);
    }
    let mut guard = slot.write().expect(lock_clean);
    if let Some(data) = guard.as_ref().filter(|data| data.points.len() >= needed) {
        return Arc::clone(data);
    }
    let mut points: Vec<[f64; 3]> = guard
        .as_ref()
        .map(|data| data.points.clone())
        .unwrap_or_else(|| vec![leg.from, leg.to]);
    if let Some(walk) = ReachWalk::new(evaluator, reach_index, leg_index, reach, leg) {
        let meander_cap = leg_depth_cap(evaluator, reach, leg);
        while points.len() < needed {
            // The nodes between consecutive points of the depth-d array
            // sit at depth d, enumerated left to right — which is
            // exactly their `bits` path word.
            let parent_depth = (points.len() - 1).trailing_zeros() as u8;
            let mut next = Vec::with_capacity(points.len() * 2 - 1);
            for (bits, pair) in points.windows(2).enumerate() {
                next.push(pair[0]);
                if parent_depth < meander_cap {
                    next.push(walk.node_midpoint(pair[0], pair[1], parent_depth, bits as u32));
                } else {
                    let smooth = four_point_midpoint(&points, bits);
                    next.push(if walk.accepts(pair[0], pair[1], smooth) {
                        smooth
                    } else {
                        normalized(add(pair[0], pair[1]))
                            .expect("reach sub-segments stay strictly inside one hemisphere")
                    });
                }
            }
            next.push(*points.last().expect("a path holds both endpoints"));
            points = next;
        }
    }
    let bounds = node_bounds(&points, evaluator.amplifier().radius_m());
    let data = Arc::new(ReachPathData { points, bounds });
    *guard = Some(Arc::clone(&data));
    data
}

/// Gnomonic longitudinal frame for one authoritative dry-land leg.
struct LegFrame {
    center: [f64; 3],
    axis: [f64; 3],
    start: f64,
    inverse_span: f64,
}

impl LegFrame {
    fn new(leg: &RiverLeg) -> Option<Self> {
        let center = leg.sector[0];
        let portal = if leg.from == center { leg.to } else { leg.from };
        let projected = [
            portal[0] - dot(portal, center) * center[0],
            portal[1] - dot(portal, center) * center[1],
            portal[2] - dot(portal, center) * center[2],
        ];
        let axis = normalized(projected)?;
        let coordinate = |point: [f64; 3]| {
            let denominator = dot(point, center);
            (denominator > f64::EPSILON).then(|| dot(point, axis) / denominator)
        };
        let start = coordinate(leg.from)?;
        let end = coordinate(leg.to)?;
        let span = end - start;
        (span.abs() > f64::EPSILON).then_some(Self {
            center,
            axis,
            start,
            inverse_span: span.recip(),
        })
    }

    fn progress(&self, point: [f64; 3]) -> Option<f64> {
        let denominator = dot(point, self.center);
        (denominator > f64::EPSILON)
            .then(|| (dot(point, self.axis) / denominator - self.start) * self.inverse_span)
    }

    fn strictly_between(&self, a: [f64; 3], b: [f64; 3], point: [f64; 3]) -> bool {
        let Some(a) = self.progress(a) else {
            return false;
        };
        let Some(b) = self.progress(b) else {
            return false;
        };
        let Some(point) = self.progress(point) else {
            return false;
        };
        let lower = a.min(b);
        let upper = a.max(b);
        let tolerance = 64.0 * f64::EPSILON * (1.0 + lower.abs().max(upper.abs()));
        point > lower + tolerance && point < upper - tolerance
    }
}

/// Everything constant per leg during a tree walk.
struct ReachWalk<'a> {
    evaluator: &'a HierarchicalEvaluator,
    reach_index: u32,
    leg_index: usize,
    frame: LegFrame,
    sector: [[f64; 3]; 3],
    /// Great-circle normal of the leg chord (corridor reference).
    chord_normal: [f64; 3],
    /// Corridor half-width in radians.
    corridor_rad: f64,
    /// Meander wavelength in metres.
    wavelength_m: f64,
    radius_m: f64,
}

impl ReachWalk<'_> {
    fn new<'a>(
        evaluator: &'a HierarchicalEvaluator,
        reach_index: u32,
        leg_index: usize,
        reach: &RiverReach,
        leg: &RiverLeg,
    ) -> Option<ReachWalk<'a>> {
        let chord_normal = normalized(cross(leg.from, leg.to))?;
        let radius_m = evaluator.amplifier().radius_m();
        let chord_rad = arc_angle(leg.from, leg.to);
        Some(ReachWalk {
            evaluator,
            reach_index,
            leg_index,
            frame: LegFrame::new(leg)?,
            sector: leg.sector,
            chord_normal,
            corridor_rad: MEANDER_CORRIDOR_FRACTION * chord_rad,
            wavelength_m: MEANDER_WAVELENGTH_PER_WIDTH * 2.0 * reach.half_width_m(),
            radius_m,
        })
    }

    /// The displaced midpoint of one tree node (amendment A6.1–A6.4).
    ///
    /// `depth` is the node's depth (0 splits the chord) and `bits` the
    /// binary path from the root, both part of the canonical seed.
    fn node_midpoint(&self, a: [f64; 3], b: [f64; 3], depth: u8, bits: u32) -> [f64; 3] {
        let geometric =
            normalized(add(a, b)).expect("reach sub-segments stay strictly inside one hemisphere");
        let length_m = arc_angle(a, b) * self.radius_m;
        if length_m < self.wavelength_m * 0.5 {
            return geometric;
        }
        let amplitude_rad = MEANDER_SINUOSITY_FRACTION * length_m / self.radius_m;
        // For a point on the arc, the segment's great-circle normal lies
        // in that point's tangent plane and is perpendicular to the arc —
        // it *is* the lateral displacement direction.
        let Some(segment_normal) = normalized(cross(a, b)) else {
            return geometric;
        };
        let guidance_level = (depth + 1 + GUIDANCE_LEVEL_AHEAD).min(GUIDANCE_LEVEL_CAP);
        let mut best: Option<(f64, [f64; 3])> = None;
        for (slot, offset) in [-1.0_f64, 0.0, 1.0].into_iter().enumerate() {
            let candidate = normalized([
                geometric[0] + segment_normal[0] * amplitude_rad * offset,
                geometric[1] + segment_normal[1] * amplitude_rad * offset,
                geometric[2] + segment_normal[2] * amplitude_rad * offset,
            ])
            .unwrap_or(geometric);
            if !self.accepts(a, b, candidate) {
                continue;
            }
            let direction = UnitVector3::new(candidate[0], candidate[1], candidate[2])
                .expect("a normalized candidate is a unit vector");
            let elevation = self
                .evaluator
                .uncarved_sample_elevation_m(direction, guidance_level);
            let jitter = signed_unit_noise(&self.candidate_seed(depth, bits, slot as u8))
                * MEANDER_GUIDANCE_JITTER_M;
            let score = elevation + jitter;
            if best.is_none_or(|(best_score, _)| score < best_score) {
                best = Some((score, candidate));
            }
        }
        best.map(|(_, point)| point).unwrap_or(geometric)
    }

    fn accepts(&self, a: [f64; 3], b: [f64; 3], point: [f64; 3]) -> bool {
        spherical_triangle_margin(point, self.sector) > 64.0 * f64::EPSILON
            && self.frame.strictly_between(a, b, point)
            && dot(point, self.chord_normal).clamp(-1.0, 1.0).asin().abs() <= self.corridor_rad
    }

    /// Canonical candidate seed: `blake3(域 ∥ "r" ∥ reach ∥ depth ∥
    /// bits ∥ candidate)` (amendment A6.1/A6.3).
    fn candidate_seed(&self, depth: u8, bits: u32, candidate: u8) -> [u8; 32] {
        let mut hasher = self.evaluator.seed_hasher();
        hasher.update(b"r");
        hasher.update(&self.reach_index.to_le_bytes());
        hasher.update(&[self.leg_index as u8]);
        hasher.update(&[depth]);
        hasher.update(&bits.to_le_bytes());
        hasher.update(&[candidate]);
        *hasher.finalize().as_bytes()
    }
}

/// The deepest meaningful rerouting depth of one reach: the level where
/// the sub-segment length falls below half the meander wavelength.
pub(super) fn path_depth_cap(evaluator: &HierarchicalEvaluator, reach_index: u32) -> u8 {
    let reaches = evaluator.amplifier().river_reaches();
    let Some(reach) = reaches.get(reach_index as usize) else {
        return 0;
    };
    reach
        .legs
        .iter()
        .flatten()
        .map(|leg| leg_depth_cap(evaluator, reach, leg))
        .max()
        .unwrap_or(0)
}

fn leg_depth_cap(evaluator: &HierarchicalEvaluator, reach: &RiverReach, leg: &RiverLeg) -> u8 {
    let chord_m = arc_angle(leg.from, leg.to) * evaluator.amplifier().radius_m();
    let half_wavelength = MEANDER_WAVELENGTH_PER_WIDTH * reach.half_width_m();
    if !(chord_m.is_finite() && half_wavelength.is_finite()) || chord_m < half_wavelength {
        return 0;
    }
    ((chord_m / half_wavelength).log2().floor() as u8).min(PATH_DEPTH_LIMIT)
}

/// Materializes one reach's dry-land polyline at `depth`, concatenating
/// its authoritative legs in flow order and welding their shared portal.
pub(super) fn materialize_path(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    depth: u8,
) -> Vec<UnitVector3> {
    let reaches = evaluator.amplifier().river_reaches();
    let Some(reach) = reaches.get(reach_index as usize) else {
        return Vec::new();
    };
    let as_unit = UnitVector3::from_verified_unit_components;
    let depth = depth.min(smooth_depth_cap(evaluator, reach_index));
    let mut path = Vec::new();
    for (leg_index, leg) in reach.legs.iter().enumerate() {
        let Some(leg) = leg else {
            continue;
        };
        let leg_depth = depth.min(smooth_leg_depth_cap(evaluator, reach, leg));
        let points = materialize_leg(evaluator, reach_index, leg_index, reach, leg, leg_depth);
        let skip = usize::from(
            path.last()
                .is_some_and(|last: &UnitVector3| last.components() == points[0]),
        );
        path.extend(points[skip..].iter().copied().map(as_unit));
    }
    path
}

fn materialize_leg(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    leg_index: usize,
    reach: &RiverReach,
    leg: &RiverLeg,
    depth: u8,
) -> Vec<[f64; 3]> {
    let depth = depth.min(smooth_leg_depth_cap(evaluator, reach, leg));
    if depth == 0 {
        return vec![leg.from, leg.to];
    }
    let data = cached_path(evaluator, reach_index, leg_index, reach, leg, depth);
    let depth = u32::from(depth).min((data.points.len() - 1).trailing_zeros());
    let stride = (data.points.len() - 1) >> depth;
    (0..=(1_usize << depth))
        .map(|index| data.points[index * stride])
        .collect()
}

/// The §4/A4 carve elevation at one position for a primitive of
/// `leaf_level`, reading each nearby reach's path at depth
/// `min(leaf_level − 1, cap)` (amendment A6.5). Formula unchanged from
/// A4: monotone bed by path fraction plus the relief-blended wall.
pub(super) fn carve_elevation_m(
    evaluator: &HierarchicalEvaluator,
    position: UnitVector3,
    leaf_level: u8,
    local_relief_norm: f64,
) -> Option<f64> {
    let amplifier = evaluator.amplifier();
    let reaches = amplifier.river_reaches();
    if reaches.is_empty() {
        return None;
    }
    let corners = amplifier.locate_corner_cells(position);
    let (offsets, indices) = amplifier.reach_lists();
    let wall_slope = TerrainAmplifier::carve_wall_slope(local_relief_norm);
    let p = position.components();
    let mut carve: Option<f64> = None;
    for &cell in &corners {
        let start = offsets[cell as usize] as usize;
        let end = offsets[cell as usize + 1] as usize;
        for &reach_index in &indices[start..end] {
            let reach = &reaches[reach_index as usize];
            let depth = leaf_level.saturating_sub(1);
            let Some((lateral_m, fraction)) =
                nearest_on_path(evaluator, reach_index, reach, depth, p)
            else {
                continue;
            };
            let bed = reach.bed_from_m + fraction * (reach.bed_to_m - reach.bed_from_m);
            let value = bed + (lateral_m - reach.half_width_m()).max(0.0) * wall_slope;
            carve = Some(carve.map_or(value, |current: f64| current.min(value)));
        }
    }
    carve
}

/// Nearest point of `p` on the reach's dry-land legs at `depth`.
fn nearest_on_path(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    reach: &RiverReach,
    depth: u8,
    p: [f64; 3],
) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    for (leg_index, leg) in reach.legs.iter().enumerate() {
        let Some(leg) = leg else {
            continue;
        };
        if !spherical_triangle_contains(p, leg.sector) {
            continue;
        }
        let depth = depth.min(smooth_leg_depth_cap(evaluator, reach, leg));
        let Some((lateral, local_fraction)) =
            nearest_on_leg(evaluator, reach_index, leg_index, reach, leg, depth, p)
        else {
            continue;
        };
        let fraction = leg.fraction_from + local_fraction * (leg.fraction_to - leg.fraction_from);
        if best.is_none_or(|(current, _)| lateral < current) {
            best = Some((lateral, fraction));
        }
    }
    best
}

fn nearest_on_leg(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    leg_index: usize,
    reach: &RiverReach,
    leg: &RiverLeg,
    depth: u8,
    p: [f64; 3],
) -> Option<(f64, f64)> {
    let radius_m = evaluator.amplifier().radius_m();
    if depth == 0 {
        return arc_nearest_point(leg.from, leg.to, p, radius_m);
    }
    let data = cached_path(evaluator, reach_index, leg_index, reach, leg, depth);
    let depth = u32::from(depth).min((data.points.len() - 1).trailing_zeros());
    if depth == 0 {
        return arc_nearest_point(leg.from, leg.to, p, radius_m);
    }
    let stride = (data.points.len() - 1) >> depth;
    let mut best: Option<(f64, f64)> = None;
    descend_nearest(
        &data,
        stride,
        (0, data.points.len() - 1),
        (0.0, 1.0),
        p,
        radius_m,
        &mut best,
    );
    best
}

fn descend_nearest(
    data: &ReachPathData,
    stride: usize,
    (start, end): (usize, usize),
    (fraction_a, fraction_b): (f64, f64),
    p: [f64; 3],
    radius_m: f64,
    best: &mut Option<(f64, f64)>,
) {
    let points = &data.points;
    let a = points[start];
    let b = points[end];
    let Some((chord_lateral, chord_along)) = arc_nearest_point(a, b, p, radius_m) else {
        return;
    };
    if end - start <= stride {
        let fraction = fraction_a + chord_along * (fraction_b - fraction_a);
        if best.is_none_or(|(current, _)| chord_lateral < current) {
            *best = Some((chord_lateral, fraction));
        }
        return;
    }
    let middle = (start + end) / 2;
    // Exact pruning: no deeper path point of this span strays farther
    // than the node's proven bound from its chord.
    if let Some((current, _)) = best {
        if chord_lateral - data.bounds[middle] > *current {
            return;
        }
    }
    let mid_fraction = 0.5 * (fraction_a + fraction_b);
    // Visit the nearer half first so the bound prunes the other.
    let first_distance = arc_nearest_point(a, points[middle], p, radius_m)
        .map_or(f64::INFINITY, |(lateral, _)| lateral);
    let second_distance = arc_nearest_point(points[middle], b, p, radius_m)
        .map_or(f64::INFINITY, |(lateral, _)| lateral);
    let first = ((start, middle), (fraction_a, mid_fraction));
    let second = ((middle, end), (mid_fraction, fraction_b));
    let ordered = if first_distance <= second_distance {
        [first, second]
    } else {
        [second, first]
    };
    for (span, fractions) in ordered {
        descend_nearest(data, stride, span, fractions, p, radius_m, best);
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn normalized(v: [f64; 3]) -> Option<[f64; 3]> {
    let len = dot(v, v).sqrt();
    (len > f64::EPSILON).then(|| [v[0] / len, v[1] / len, v[2] / len])
}

#[cfg(test)]
mod tests {
    use super::super::hierarchical_derivation::HierarchicalEvaluator;
    use super::super::terrain_amplification::AmplificationFieldsView;
    use super::*;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        RiverSegment, RiverSegmentKind, SphericalOrogenyKind, SurfaceWaterField, SurfaceWaterKind,
    };
    use crate::world::spatial::SphericalSurfaceSnapshot;
    use crate::world::{Meters, RiverSegmentId, RootSeed, SphericalSpaceSpec};

    fn test_surface() -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build_cancellable(
            &SphericalSpaceSpec {
                radius: Meters::new(6_371_000.0).unwrap(),
                target_cell_count: 162,
            },
            || false,
        )
        .unwrap()
    }

    fn dry_surface_water(surface: &SphericalSurfaceSnapshot) -> SurfaceWaterField {
        SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; surface.cells().len()])
    }

    struct Fields {
        elevation: Vec<f32>,
        zeros: Vec<f32>,
        ones: Vec<f32>,
        kinds: Vec<SphericalOrogenyKind>,
    }

    impl Fields {
        /// A tilted field: elevation rises with +y so the downhill side of
        /// any west-east reach is unambiguous for the guidance test.
        fn new(surface: &SphericalSurfaceSnapshot) -> Self {
            let count = surface.cells().len();
            Self {
                elevation: surface
                    .cells()
                    .iter()
                    .map(|cell| (2_000.0 * cell.centroid.components()[1]) as f32)
                    .collect(),
                zeros: vec![0.0; count],
                ones: vec![1.0; count],
                kinds: vec![SphericalOrogenyKind::None; count],
            }
        }

        fn view(&self) -> AmplificationFieldsView<'_> {
            AmplificationFieldsView {
                final_elevation_m: &self.elevation,
                sea_level_m: -3_000.0,
                sediment_thickness_m: &self.zeros,
                erodibility: &self.zeros,
                annual_precipitation_mm: &self.ones,
                crust_age_myr: &self.zeros,
                lineation_east: &self.ones,
                lineation_north: &self.zeros,
                orogeny_kind: &self.kinds,
                orogeny_age_myr: &self.zeros,
            }
        }
    }

    /// Two chained reaches (a → b → c) with contrasting discharges.
    fn evaluator_with_rivers() -> (HierarchicalEvaluator, SphericalSurfaceSnapshot) {
        let surface = test_surface();
        let fields = Fields::new(&surface);
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
                1.0,
            )
            .unwrap(),
            RiverSegment::new(
                RiverSegmentId::from_raw(1),
                b,
                c,
                RiverSegmentKind::Channel,
                3,
                50_000.0,
            )
            .unwrap(),
        ];
        let evaluator = HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(11))
            .unwrap()
            .with_rivers(&surface, &segments, &dry_surface_water(&surface))
            .unwrap();
        (evaluator, surface)
    }

    fn evaluator_with_water_transition(
        from_water: SurfaceWaterKind,
        to_water: SurfaceWaterKind,
    ) -> (HierarchicalEvaluator, SphericalSurfaceSnapshot, usize) {
        let surface = test_surface();
        let fields = Fields::new(&surface);
        let edge_index = 0;
        let edge = &surface.edges()[edge_index];
        let [from, to] = edge.cells;
        let segment = RiverSegment::new(
            RiverSegmentId::from_raw(0),
            from,
            to,
            RiverSegmentKind::Channel,
            1,
            25.0,
        )
        .unwrap();
        let mut water = vec![SurfaceWaterKind::DryLand; surface.cells().len()];
        water[from.raw() as usize] = from_water;
        water[to.raw() as usize] = to_water;
        let evaluator = HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(13))
            .unwrap()
            .with_rivers(&surface, &[segment], &SurfaceWaterField::from_kinds(water))
            .unwrap();
        (evaluator, surface, edge_index)
    }

    #[test]
    fn river_path_uses_shared_portal_and_omits_water_interiors() {
        use SurfaceWaterKind::{DryLand, Lake};

        for (from_water, to_water, expected_points) in [
            (DryLand, DryLand, 3_usize),
            (DryLand, Lake, 2),
            (Lake, DryLand, 2),
            (Lake, Lake, 0),
        ] {
            let (evaluator, surface, edge_index) =
                evaluator_with_water_transition(from_water, to_water);
            let edge = &surface.edges()[edge_index];
            let [from, to] = edge.cells;
            let path = evaluator.river_path(0, 0);
            assert_eq!(path.len(), expected_points);
            if from_water == DryLand {
                assert_eq!(
                    path.first().copied(),
                    Some(surface.cells()[from.raw() as usize].centroid)
                );
            }
            if to_water == DryLand {
                assert_eq!(
                    path.last().copied(),
                    Some(surface.cells()[to.raw() as usize].centroid)
                );
            }
            if expected_points > 0 {
                assert!(
                    path.contains(&edge.midpoint),
                    "every rendered water transition is split at the shoreline portal"
                );
            }
        }
    }

    #[test]
    fn every_path_segment_stays_in_one_authoritative_sector() {
        let (evaluator, _surface) = evaluator_with_rivers();
        for reach_index in 0..evaluator.river_reach_count() as u32 {
            let reach = &evaluator.amplifier().river_reaches()[reach_index as usize];
            let depth = smooth_depth_cap(&evaluator, reach_index).min(9);
            let path = evaluator.river_path(reach_index, depth);
            for pair in path.windows(2) {
                assert!(
                    reach.legs.iter().flatten().any(|leg| {
                        super::super::terrain_amplification::spherical_triangle_contains(
                            pair[0].components(),
                            leg.sector,
                        ) && super::super::terrain_amplification::spherical_triangle_contains(
                            pair[1].components(),
                            leg.sector,
                        )
                    }),
                    "a path segment crossed an authoritative sector boundary"
                );
            }
        }
    }

    #[test]
    fn river_path_progress_is_strictly_monotone_inside_each_leg() {
        let (evaluator, _surface) = evaluator_with_rivers();
        for reach_index in 0..evaluator.river_reach_count() as u32 {
            let reach = &evaluator.amplifier().river_reaches()[reach_index as usize];
            let path = evaluator.river_path(reach_index, 9);
            for leg in reach.legs.iter().flatten() {
                let frame = LegFrame::new(leg).expect("a surface leg is non-degenerate");
                let start = path
                    .iter()
                    .position(|point| point.components() == leg.from)
                    .expect("materialized path includes the leg start");
                let end = path
                    .iter()
                    .position(|point| point.components() == leg.to)
                    .expect("materialized path includes the leg end");
                let progress: Vec<_> = path[start..=end]
                    .iter()
                    .map(|point| frame.progress(point.components()).unwrap())
                    .collect();
                assert!(
                    progress.windows(2).all(|pair| pair[0] < pair[1]),
                    "a leg cut back in gnomonic longitudinal order"
                );
            }
        }
    }

    #[test]
    fn different_reaches_only_meet_at_authoritative_cell_nodes() {
        let (evaluator, _surface) = evaluator_with_rivers();
        let upstream = evaluator.river_path(0, 9);
        let downstream = evaluator.river_path(1, 9);
        let junction = *upstream.last().expect("the upstream reach is visible");
        assert_eq!(downstream.first().copied(), Some(junction));

        let shared: Vec<_> = upstream
            .iter()
            .filter(|point| downstream.contains(point))
            .copied()
            .collect();
        assert_eq!(shared, vec![junction]);
    }

    /// Amendment A10: depth zero is the portal-split dry-land chain and
    /// every leg endpoint stays fixed at all deeper levels.
    #[test]
    fn depth_zero_is_the_l0_chain_with_fixed_endpoints() {
        let (evaluator, _surface) = evaluator_with_rivers();
        let reaches = evaluator.amplifier().river_reaches();
        for reach_index in 0..evaluator.river_reach_count() as u32 {
            let reach = &reaches[reach_index as usize];
            let chain = evaluator.river_path(reach_index, 0);
            let mut expected = Vec::new();
            for leg in reach.legs.iter().flatten() {
                if expected.last().copied() != Some(leg.from) {
                    expected.push(leg.from);
                }
                expected.push(leg.to);
            }
            assert_eq!(
                chain
                    .iter()
                    .map(|point| point.components())
                    .collect::<Vec<_>>(),
                expected
            );
            for depth in 1..=evaluator.river_path_depth_cap(reach_index).min(8) {
                let path = evaluator.river_path(reach_index, depth);
                for endpoint in &expected {
                    assert!(path.iter().any(|point| point.components() == *endpoint));
                }
            }
        }
    }

    /// Amendment A6.1/A6.3: the path tree is a pure function — two
    /// independently built engines derive bit-identical polylines.
    #[test]
    fn paths_are_deterministic_across_builds() {
        let (first, _surface_a) = evaluator_with_rivers();
        let (second, _surface_b) = evaluator_with_rivers();
        for reach_index in 0..first.river_reach_count() as u32 {
            let depth = first.river_path_depth_cap(reach_index).min(6);
            let path_a = first.river_path(reach_index, depth);
            let path_b = second.river_path(reach_index, depth);
            assert_eq!(path_a.len(), path_b.len());
            for (left, right) in path_a.iter().zip(&path_b) {
                assert_eq!(
                    left.components().map(f64::to_bits),
                    right.components().map(f64::to_bits)
                );
            }
        }
    }

    /// The memo is pure acceleration: whichever order depths are asked
    /// in — shallow-first (deepening) or deep-first (stride reads) —
    /// every path and every carve stays bit-identical.
    #[test]
    fn memo_results_are_query_order_independent() {
        let (shallow_first, surface) = evaluator_with_rivers();
        let (deep_first, _surface) = evaluator_with_rivers();
        for reach_index in 0..shallow_first.river_reach_count() as u32 {
            let cap = shallow_first.river_path_depth_cap(reach_index).min(6);
            let ascending: Vec<_> = (0..=cap)
                .map(|depth| shallow_first.river_path(reach_index, depth))
                .collect();
            let mut descending: Vec<_> = (0..=cap)
                .rev()
                .map(|depth| deep_first.river_path(reach_index, depth))
                .collect();
            descending.reverse();
            for (left, right) in ascending.iter().zip(&descending) {
                assert_eq!(left.len(), right.len());
                for (a, b) in left.iter().zip(right) {
                    assert_eq!(
                        a.components().map(f64::to_bits),
                        b.components().map(f64::to_bits)
                    );
                }
            }
        }
        for cell in surface.cells().iter().step_by(7) {
            let ours = carve_elevation_m(&shallow_first, cell.centroid, 7, 0.3);
            let theirs = carve_elevation_m(&deep_first, cell.centroid, 7, 0.3);
            assert_eq!(ours.map(f64::to_bits), theirs.map(f64::to_bits));
        }
    }

    /// Amendment A10: neither stochastic nor smooth vertices may leave
    /// the authoritative leg corridor.
    #[test]
    fn paths_stay_inside_the_parent_corridor() {
        let (evaluator, _surface) = evaluator_with_rivers();
        let reaches = evaluator.amplifier().river_reaches();
        let radius = evaluator.amplifier().radius_m();
        for reach_index in 0..evaluator.river_reach_count() as u32 {
            let reach = &reaches[reach_index as usize];
            for (leg_index, leg) in reach.legs.iter().enumerate() {
                let Some(leg) = leg else {
                    continue;
                };
                let normal = normalized(cross(leg.from, leg.to)).unwrap();
                let chord_rad = arc_angle(leg.from, leg.to);
                let cap = leg_depth_cap(&evaluator, reach, leg);
                let corridor_rad = MEANDER_CORRIDOR_FRACTION * chord_rad;
                for depth in [cap.min(8), smooth_leg_depth_cap(&evaluator, reach, leg)] {
                    for point in
                        materialize_leg(&evaluator, reach_index, leg_index, reach, leg, depth)
                    {
                        let lateral = dot(point, normal).clamp(-1.0, 1.0).asin().abs();
                        assert!(
                            lateral <= corridor_rad + 1.0e-12,
                            "point strays {:.1} m beyond the corridor at depth {depth}",
                            (lateral - corridor_rad) * radius
                        );
                    }
                }
            }
        }
    }

    /// Amendment A6.2 + A8: the wavelength cutoff stops *stochastic*
    /// refinement — the wide reach caps shallower than the narrow one —
    /// while the A8 smooth refinement below the cap keeps every meander
    /// vertex bit-exactly (interpolatory) and freezes past the smooth
    /// cap.
    #[test]
    fn wavelength_cutoff_caps_the_tree() {
        let (evaluator, _surface) = evaluator_with_rivers();
        let narrow_cap = evaluator.river_path_depth_cap(0);
        let wide_cap = evaluator.river_path_depth_cap(1);
        assert!(
            narrow_cap > wide_cap,
            "narrow {narrow_cap} must out-refine wide {wide_cap}"
        );
        let reach = &evaluator.amplifier().river_reaches()[1];
        for (leg_index, leg) in reach.legs.iter().enumerate() {
            let Some(leg) = leg else {
                continue;
            };
            let cap = leg_depth_cap(&evaluator, reach, leg);
            let smooth_cap = smooth_leg_depth_cap(&evaluator, reach, leg);
            let capped = materialize_leg(&evaluator, 1, leg_index, reach, leg, cap);
            let smooth = materialize_leg(&evaluator, 1, leg_index, reach, leg, smooth_cap);
            let stride = 1_usize << (smooth_cap - cap);
            assert_eq!(smooth.len(), (capped.len() - 1) * stride + 1);
            for (index, vertex) in capped.iter().enumerate() {
                assert_eq!(
                    vertex.map(f64::to_bits),
                    smooth[index * stride].map(f64::to_bits),
                    "smooth refinement must interpolate the meander vertices"
                );
            }
            let beyond = materialize_leg(&evaluator, 1, leg_index, reach, leg, smooth_cap + 3);
            assert_eq!(smooth, beyond);
        }
    }

    /// Amendment A6.3: the chosen midpoint minimizes the guidance score,
    /// so its uncarved elevation can exceed the best candidate's by at
    /// most the two-sided jitter band.
    #[test]
    fn guidance_never_picks_a_meaningfully_higher_candidate() {
        let (evaluator, _surface) = evaluator_with_rivers();
        let reaches = evaluator.amplifier().river_reaches();
        for reach_index in 0..evaluator.river_reach_count() as u32 {
            let reach = &reaches[reach_index as usize];
            for (leg_index, leg) in reach.legs.iter().enumerate() {
                let Some(leg) = leg else {
                    continue;
                };
                let Some(walk) = ReachWalk::new(&evaluator, reach_index, leg_index, reach, leg)
                else {
                    continue;
                };
                let chosen = walk.node_midpoint(leg.from, leg.to, 0, 0);
                let geometric = normalized(add(leg.from, leg.to)).unwrap();
                let length_m = arc_angle(leg.from, leg.to) * walk.radius_m;
                if length_m < walk.wavelength_m * 0.5 {
                    assert_eq!(chosen, geometric);
                    continue;
                }
                let amplitude_rad = MEANDER_SINUOSITY_FRACTION * length_m / walk.radius_m;
                let normal = normalized(cross(leg.from, leg.to)).unwrap();
                let guidance_level = (1 + GUIDANCE_LEVEL_AHEAD).min(GUIDANCE_LEVEL_CAP);
                let elevation_at = |point: [f64; 3]| {
                    evaluator.uncarved_sample_elevation_m(
                        UnitVector3::new(point[0], point[1], point[2]).unwrap(),
                        guidance_level,
                    )
                };
                let mut best = f64::INFINITY;
                let mut matched = false;
                for offset in [-1.0_f64, 0.0, 1.0] {
                    let candidate = normalized([
                        geometric[0] + normal[0] * amplitude_rad * offset,
                        geometric[1] + normal[1] * amplitude_rad * offset,
                        geometric[2] + normal[2] * amplitude_rad * offset,
                    ])
                    .unwrap();
                    if !walk.accepts(leg.from, leg.to, candidate) {
                        continue;
                    }
                    best = best.min(elevation_at(candidate));
                    matched |= candidate
                        .iter()
                        .zip(&chosen)
                        .all(|(a, b)| (a - b).abs() < 1.0e-15);
                }
                assert!(matched, "the chosen midpoint must be one of the candidates");
                assert!(
                    elevation_at(chosen) <= best + 2.0 * MEANDER_GUIDANCE_JITTER_M + 1.0e-6,
                    "the choice can lose to the best candidate only within the jitter band"
                );
            }
        }
    }

    /// Amendment A6.5: the level-aware carve is min-only against the
    /// uncarved field, matches the A4 chord carve at level 1, and the
    /// bed stays monotone along the deep path.
    #[test]
    fn level_aware_carve_stays_faithful_to_a4() {
        let (evaluator, surface) = evaluator_with_rivers();
        let reaches = evaluator.amplifier().river_reaches();

        // Level 1 reads depth-0 paths: identical to the A4 chord carve.
        for cell in surface.cells().iter().step_by(11) {
            let probe = cell.centroid;
            let ours = carve_elevation_m(&evaluator, probe, 1, 0.3);
            let a4 = evaluator.amplifier().river_carve_m(probe, 0.3);
            match (ours, a4) {
                (Some(left), Some(right)) => {
                    assert!((left - right).abs() < 1.0e-9, "{left} != {right}")
                }
                (None, None) => {}
                other => panic!("carve availability diverged: {other:?}"),
            }
        }

        // Deep carve along the deep path: the reach's OWN bed-plus-wall
        // stays monotone downstream. (The displayed carve is the min
        // over every nearby reach, and walking a tributary through a
        // junction overlap can legitimately climb a neighbour's rising
        // valley wall — the invariant belongs to the reach itself.)
        let reach_index = 1_u32;
        let reach = &reaches[reach_index as usize];
        let depth = smooth_depth_cap(&evaluator, reach_index);
        let path = evaluator.river_path(reach_index, depth);
        let wall_slope = TerrainAmplifier::carve_wall_slope(0.0);
        let mut previous = f64::INFINITY;
        for point in &path {
            let (lateral_m, fraction) =
                nearest_on_path(&evaluator, reach_index, reach, depth, point.components())
                    .expect("a path point projects onto its own path");
            let bed = reach.bed_from_m + fraction * (reach.bed_to_m - reach.bed_from_m);
            let own = bed + (lateral_m - reach.half_width_m()).max(0.0) * wall_slope;
            assert!(
                own <= previous + 1.0e-6,
                "own carve rose downstream: {own} > {previous}"
            );
            previous = own;
        }
        assert!(reach.bed_from_m >= reach.bed_to_m);
    }
}
