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
//!
//! LOD-1 spec §3.4 D4: reaches touching dry land keep both legs, and the
//! drawn line and the carve keep only the path's fine-dry vertices — each
//! centreline vertex classified by the production fine shoreline on the
//! uncarved field floored by the reach bed — so lines end on the drawn
//! water edge instead of at the L0 portal.

use std::sync::{Arc, RwLock};

use super::hierarchical_derivation::{signed_unit_noise, HierarchicalEvaluator};
use super::terrain_amplification::{
    arc_angle, arc_nearest_point, spherical_triangle_contains, spherical_triangle_margin,
    ReachFlow, RiverLeg, RiverReach, TerrainAmplifier,
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
    /// Per display depth: the fine-dry clip of the path (D4), memoized as
    /// a pure function of the reach and the depth.
    clips: [RwLock<Option<Arc<ReachClip>>>; PATH_DEPTH_LIMIT as usize + 1],
}

impl Default for ReachPathCache {
    fn default() -> Self {
        Self {
            deepest: std::array::from_fn(|_| RwLock::new(None)),
            clips: std::array::from_fn(|_| RwLock::new(None)),
        }
    }
}

impl ReachPathCache {
    fn leg(&self, leg_index: usize) -> Option<&RwLock<Option<Arc<ReachPathData>>>> {
        self.deepest.get(leg_index)
    }
}

/// The kept vertices of one reach's centreline at one display depth: one
/// flag per vertex of the welded leg chain, plus each leg's range in it.
/// A path segment is drawn and carved only when both its ends are kept.
pub(super) struct ReachClip {
    keep: Vec<bool>,
    /// `kept_before[k]`: kept chain segments among segments `0..k`, so a
    /// descent prunes spans with nothing kept in O(1).
    kept_before: Vec<u32>,
    legs: [(usize, usize); 2],
}

impl ReachClip {
    fn new(keep: Vec<bool>, legs: [(usize, usize); 2]) -> Self {
        let mut kept_before = Vec::with_capacity(keep.len());
        let mut count = 0_u32;
        kept_before.push(count);
        for pair in keep.windows(2) {
            count += u32::from(pair[0] && pair[1]);
            kept_before.push(count);
        }
        Self {
            keep,
            kept_before,
            legs,
        }
    }

    /// One leg's view of the clip, in leg-local vertex indices.
    fn leg(&self, leg_index: usize) -> LegClip<'_> {
        LegClip {
            clip: self,
            offset: self.legs[leg_index].0,
        }
    }
}

/// One leg's kept segments (leg-local vertex indices).
#[derive(Clone, Copy)]
struct LegClip<'a> {
    clip: &'a ReachClip,
    offset: usize,
}

impl LegClip<'_> {
    /// Whether any segment between leg vertices `start..end` is kept.
    fn any_kept(self, start: usize, end: usize) -> bool {
        self.clip.kept_before[self.offset + end] > self.clip.kept_before[self.offset + start]
    }
}

/// The welded centreline of one reach at one display depth.
pub(super) struct Centreline {
    /// The same points as [`materialize_path`], each with its reach-bed
    /// fraction (uniform in vertex index, as the carve's path fraction is).
    vertices: Vec<([f64; 3], f64)>,
    /// Each leg's `(start, len)` vertex range in `vertices`.
    legs: [(usize, usize); 2],
}

/// The welded [`Centreline`] of one reach at display depth `depth`.
pub(super) fn centreline_vertices(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    reach: &RiverReach,
    depth: u8,
) -> Centreline {
    let depth = depth.min(smooth_depth_cap(evaluator, reach_index));
    let mut chain: Vec<([f64; 3], f64)> = Vec::new();
    let mut ranges = [(0, 0); 2];
    for (leg_index, leg) in reach.legs.iter().enumerate() {
        let Some(leg) = leg else {
            continue;
        };
        let leg_depth = depth.min(smooth_leg_depth_cap(evaluator, reach, leg));
        let points = materialize_leg(evaluator, reach_index, leg_index, reach, leg, leg_depth);
        let welded = chain.last().is_some_and(|(last, _)| *last == points[0]);
        ranges[leg_index] = (chain.len() - usize::from(welded), points.len());
        let span = (points.len() - 1) as f64;
        chain.extend(
            points
                .iter()
                .enumerate()
                .skip(usize::from(welded))
                .map(|(index, &point)| {
                    let local = index as f64 / span;
                    (
                        point,
                        leg.fraction_from + local * (leg.fraction_to - leg.fraction_from),
                    )
                }),
        );
    }
    Centreline {
        vertices: chain,
        legs: ranges,
    }
}

/// The reach bed at path fraction `fraction` (the carve's A4 bed law).
fn bed_at_m(reach: &RiverReach, fraction: f64) -> f64 {
    reach.bed_from_m + fraction * (reach.bed_to_m - reach.bed_from_m)
}

/// The memoized D4.3 clip of one reach at `depth`, or `None` when no
/// vertex can be water (both owner cells see no water body at all).
fn reach_clip(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    reach: &RiverReach,
    depth: u8,
) -> Option<Arc<ReachClip>> {
    let shoreline = evaluator.shoreline();
    if reach.flow == ReachFlow::Submerged
        || (shoreline.never_wet(reach.from_cell) && shoreline.never_wet(reach.to_cell))
    {
        return None;
    }
    let depth = depth.min(PATH_DEPTH_LIMIT);
    let slot = &evaluator
        .river_path_slot(reach_index)
        .expect("reach indices come from the engine's own reach lists")
        .clips[usize::from(depth)];
    let lock_clean = "the reach clip lock only guards infallible derivation";
    if let Some(clip) = slot.read().expect(lock_clean).as_ref() {
        return Some(Arc::clone(clip));
    }
    let mut guard = slot.write().expect(lock_clean);
    if let Some(clip) = guard.as_ref() {
        return Some(Arc::clone(clip));
    }
    let Centreline {
        vertices: chain,
        legs,
    } = centreline_vertices(evaluator, reach_index, reach, depth);
    // D4.2: a vertex is fine-dry when the production fine shoreline paints
    // `min(uncarved(p, depth + 1), bed(f))` dry — the display level the
    // line is drawn for and the bed the carve cuts to, without reading any
    // carved value (so the clip never feeds back).
    let mut cursor = evaluator.centreline_cursor(depth.saturating_add(1));
    let mut dry = |&(point, fraction): &([f64; 3], f64)| {
        cursor.is_dry(
            UnitVector3::from_verified_unit_components(point),
            bed_at_m(reach, fraction),
        )
    };
    let keep = match reach.flow {
        // A river on land is drawn wherever the land is (USGS EDH: the
        // network breaks at water-body boundaries).
        ReachFlow::Overland | ReachFlow::Submerged => chain.iter().map(&mut dry).collect(),
        // Into a water body: up to the last dry vertex before the first
        // water vertex (the scan stops there).
        ReachFlow::Inflow => {
            let reached = chain.iter().position(|vertex| !dry(vertex));
            let kept = reached.unwrap_or(chain.len());
            (0..chain.len()).map(|index| index < kept).collect()
        }
        // Out of a water body: from the first dry vertex after the last
        // water vertex, scanning upstream from the dry end.
        ReachFlow::Outflow => {
            let left = chain.iter().rposition(|vertex| !dry(vertex));
            (0..chain.len())
                .map(|index| left.is_none_or(|left| index > left))
                .collect()
        }
    };
    let clip = Arc::new(ReachClip::new(keep, legs));
    *guard = Some(Arc::clone(&clip));
    Some(clip)
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

/// Materializes one reach's drawn polyline pieces at `depth` (D4.3): the
/// runs of consecutive kept centreline vertices, each at least one
/// segment long, in flow order.
pub(super) fn materialize_pieces(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    depth: u8,
) -> Vec<Vec<UnitVector3>> {
    let reaches = evaluator.amplifier().river_reaches();
    let Some(reach) = reaches.get(reach_index as usize) else {
        return Vec::new();
    };
    let Some(clip) = reach_clip(evaluator, reach_index, reach, depth) else {
        let path = materialize_path(evaluator, reach_index, depth);
        return if path.len() >= 2 {
            vec![path]
        } else {
            Vec::new()
        };
    };
    let chain = centreline_vertices(evaluator, reach_index, reach, depth).vertices;
    let mut pieces = Vec::new();
    let mut piece: Vec<UnitVector3> = Vec::new();
    for ((point, _), &kept) in chain.iter().zip(&clip.keep) {
        if kept {
            piece.push(UnitVector3::from_verified_unit_components(*point));
        } else if !piece.is_empty() {
            if piece.len() >= 2 {
                pieces.push(std::mem::take(&mut piece));
            }
            piece.clear();
        }
    }
    if piece.len() >= 2 {
        pieces.push(piece);
    }
    pieces
}

/// Materializes one reach's full centreline at `depth`, concatenating its
/// authoritative legs in flow order and welding their shared portal.
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
            let clip = reach_clip(evaluator, reach_index, reach, depth);
            let Some((lateral_m, fraction)) =
                nearest_on_path(evaluator, reach_index, reach, depth, clip.as_deref(), p)
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

/// Nearest point of `p` on the reach's kept path segments at `depth`.
fn nearest_on_path(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    reach: &RiverReach,
    depth: u8,
    clip: Option<&ReachClip>,
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
        let keep = clip.map(|clip| clip.leg(leg_index));
        let Some((lateral, local_fraction)) = nearest_on_leg(
            evaluator,
            LegQuery {
                reach_index,
                leg_index,
                reach,
                leg,
                depth,
                keep,
            },
            p,
        ) else {
            continue;
        };
        let fraction = leg.fraction_from + local_fraction * (leg.fraction_to - leg.fraction_from);
        if best.is_none_or(|(current, _)| lateral < current) {
            best = Some((lateral, fraction));
        }
    }
    best
}

/// One leg's nearest-point query: which leg, at which depth, and which of
/// its vertices the D4 clip keeps (`None`: all).
struct LegQuery<'a> {
    reach_index: u32,
    leg_index: usize,
    reach: &'a RiverReach,
    leg: &'a RiverLeg,
    depth: u8,
    keep: Option<LegClip<'a>>,
}

fn nearest_on_leg(
    evaluator: &HierarchicalEvaluator,
    query: LegQuery<'_>,
    p: [f64; 3],
) -> Option<(f64, f64)> {
    let LegQuery {
        reach_index,
        leg_index,
        reach,
        leg,
        depth,
        keep,
    } = query;
    let radius_m = evaluator.amplifier().radius_m();
    let chord = || {
        keep.is_none_or(|keep| keep.any_kept(0, 1))
            .then(|| arc_nearest_point(leg.from, leg.to, p, radius_m))
            .flatten()
    };
    if depth == 0 {
        return chord();
    }
    let data = cached_path(evaluator, reach_index, leg_index, reach, leg, depth);
    let depth = u32::from(depth).min((data.points.len() - 1).trailing_zeros());
    if depth == 0 {
        return chord();
    }
    let stride = (data.points.len() - 1) >> depth;
    if keep.is_some_and(|keep| !keep.any_kept(0, 1 << depth)) {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    descend_nearest(
        &data,
        Descent {
            stride,
            keep,
            p,
            radius_m,
        },
        (0, data.points.len() - 1),
        (0.0, 1.0),
        &mut best,
    );
    best
}

/// The per-query constants of one bound-pruned nearest-point descent.
#[derive(Clone, Copy)]
struct Descent<'a> {
    stride: usize,
    keep: Option<LegClip<'a>>,
    p: [f64; 3],
    radius_m: f64,
}

fn descend_nearest(
    data: &ReachPathData,
    descent: Descent<'_>,
    (start, end): (usize, usize),
    (fraction_a, fraction_b): (f64, f64),
    best: &mut Option<(f64, f64)>,
) {
    let Descent {
        stride,
        keep,
        p,
        radius_m,
    } = descent;
    // A clipped segment is neither drawn nor carved (D4.3): skip every
    // span holding none of the kept ones.
    if keep.is_some_and(|keep| !keep.any_kept(start / stride, end / stride)) {
        return;
    }
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
        descend_nearest(data, descent, span, fractions, best);
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
    use crate::generators::natural::hierarchical_derivation::tests::lake_fill_surface;
    use crate::generators::natural::PublishedHydrology;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        Lake, RiverSegment, RiverSegmentKind, SphericalOrogenyKind, SurfaceWaterField,
        SurfaceWaterKind,
    };
    use crate::world::spatial::SphericalSurfaceSnapshot;
    use crate::world::{LakeId, Meters, RiverSegmentId, RootSeed, SphericalSpaceSpec};

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
            .with_hydrology(
                &surface,
                PublishedHydrology {
                    segments: &segments,
                    surface_water: &dry_surface_water(&surface),
                    lakes: &[],
                    fill_surface_m: &fields.elevation,
                },
            )
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
        let mut water = vec![SurfaceWaterKind::DryLand; surface.cells().len()];
        water[from.raw() as usize] = from_water;
        water[to.raw() as usize] = to_water;
        let evaluator = HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(13))
            .unwrap()
            .with_hydrology(
                &surface,
                PublishedHydrology {
                    segments: &[RiverSegment::new(
                        RiverSegmentId::from_raw(0),
                        from,
                        to,
                        RiverSegmentKind::Channel,
                        1,
                        25.0,
                    )
                    .unwrap()],
                    surface_water: &SurfaceWaterField::from_kinds(water),
                    lakes: &[],
                    fill_surface_m: &fields.elevation,
                },
            )
            .unwrap();
        (evaluator, surface, edge_index)
    }

    /// One downhill reach between two adjacent cells, one of them (or a
    /// third cell in the ring) a lake whose published surface lies at
    /// `surface_fraction` of the way from the low cell's value up to the
    /// high cell's, so the fine shoreline cuts across the reach.
    struct LakeReach {
        evaluator: HierarchicalEvaluator,
        low: usize,
    }

    fn lake_reach(flow: ReachFlow, surface_fraction: f32) -> LakeReach {
        let surface = test_surface();
        let fields = Fields::new(&surface);
        let edge = surface
            .edges()
            .iter()
            .max_by(|a, b| {
                let drop = |edge: &&crate::world::spatial::SphericalSurfaceEdge| {
                    let [p, q] = edge.cells.map(|cell| fields.elevation[cell.raw() as usize]);
                    (p - q).abs()
                };
                drop(a).total_cmp(&drop(b))
            })
            .unwrap();
        let [p, q] = edge.cells;
        let (high, low) = if fields.elevation[p.raw() as usize] > fields.elevation[q.raw() as usize]
        {
            (p, q)
        } else {
            (q, p)
        };
        let (z_high, z_low) = (
            fields.elevation[high.raw() as usize],
            fields.elevation[low.raw() as usize],
        );
        let lake_surface = z_low + surface_fraction * (z_high - z_low);
        // Inflow and overland run downhill; outflow leaves the lake uphill
        // of nothing — it drains the low lake cell onto the high cell, so
        // the lake surface sits above the low cell only.
        let (from, to, lake_cell) = match flow {
            ReachFlow::Inflow => (high, low, low),
            ReachFlow::Outflow => (low, high, low),
            ReachFlow::Overland | ReachFlow::Submerged => {
                let ring = surface
                    .edges()
                    .iter()
                    .filter(|candidate| candidate.cells.contains(&low))
                    .flat_map(|candidate| candidate.cells)
                    .find(|cell| ![high, low].contains(cell))
                    .unwrap();
                (high, low, ring)
            }
        };
        let mut water = vec![SurfaceWaterKind::DryLand; surface.cells().len()];
        water[lake_cell.raw() as usize] = SurfaceWaterKind::Lake;
        let lakes = [Lake::new(
            LakeId::from_raw(0),
            vec![lake_cell],
            lake_surface,
            1.0,
            1.0,
            None,
            None,
        )
        .unwrap()];
        let segment = RiverSegment::new(
            RiverSegmentId::from_raw(0),
            from,
            to,
            RiverSegmentKind::Channel,
            1,
            25.0,
        )
        .unwrap();
        let evaluator = HierarchicalEvaluator::new(&surface, fields.view(), RootSeed::new(13))
            .unwrap()
            .with_hydrology(
                &surface,
                PublishedHydrology {
                    segments: &[segment],
                    surface_water: &SurfaceWaterField::from_kinds(water),
                    lakes: &lakes,
                    fill_surface_m: &lake_fill_surface(&surface, &fields.elevation, &lakes),
                },
            )
            .unwrap();
        LakeReach {
            evaluator,
            low: low.raw() as usize,
        }
    }

    /// The unclipped centreline of reach 0 at `depth` with each vertex's
    /// production centreline wetness (LOD-1 spec §3.4 D4.2).
    fn classified_centreline(
        evaluator: &HierarchicalEvaluator,
        depth: u8,
    ) -> Vec<(UnitVector3, bool)> {
        let reach = &evaluator.amplifier().river_reaches()[0];
        let mut cursor = evaluator.centreline_cursor(depth + 1);
        centreline_vertices(evaluator, 0, reach, depth)
            .vertices
            .into_iter()
            .map(|(point, fraction)| {
                let direction = UnitVector3::from_verified_unit_components(point);
                (
                    direction,
                    !cursor.is_dry(direction, bed_at_m(reach, fraction)),
                )
            })
            .collect()
    }

    /// The bound-pruned, walk-sharing centreline cursor decides exactly
    /// what the plain definitions decide, whether its points arrive
    /// scattered or in path order: the ocean by `min(uncarved, bed) as f32`
    /// against sea level (LOD-1 spec §3.4 D4.2), a lake by the connectivity
    /// class the mesh and the picking read paint the primitive with (R2).
    #[test]
    fn pruned_centreline_test_matches_the_plain_classification() {
        use crate::generators::natural::fibonacci_probe;
        use crate::generators::natural::LocatedPrimitive;

        let lake = lake_reach(ReachFlow::Inflow, 0.5).evaluator;
        let (ocean, _, _) =
            evaluator_with_water_transition(SurfaceWaterKind::DryLand, SurfaceWaterKind::Ocean);
        for evaluator in [&lake, &ocean] {
            let river = &evaluator.amplifier().river_reaches()[0];
            let scattered: Vec<UnitVector3> = (0..4_096)
                .map(|index| fibonacci_probe(index, 4_096))
                .collect();
            let in_path_order: Vec<UnitVector3> = centreline_vertices(evaluator, 0, river, 9)
                .vertices
                .into_iter()
                .map(|(point, _)| UnitVector3::from_verified_unit_components(point))
                .collect();
            let (mut dry, mut wet) = (0, 0);
            for level in [1_u8, 4, 9, 14] {
                for bed_m in [f64::INFINITY, -4_000.0, 0.0, 800.0] {
                    let mut cursor = evaluator.centreline_cursor(level);
                    for &point in scattered.iter().chain(&in_path_order) {
                        let LocatedPrimitive::Cell(cell) = evaluator.locate(point, 0) else {
                            unreachable!("level 0 locates a cell")
                        };
                        let value = evaluator
                            .uncarved_sample_elevation_m(point, level)
                            .min(bed_m) as f32;
                        let ocean_wet = evaluator.shoreline().row(cell).iter().any(|candidate| {
                            candidate.lake.is_none() && value < candidate.surface_m
                        });
                        let lake_wet = evaluator.read(point, level).water == SurfaceWaterKind::Lake;
                        let plain = !(ocean_wet || lake_wet);
                        assert_eq!(cursor.is_dry(point, bed_m), plain);
                        dry += usize::from(plain);
                        wet += usize::from(!plain);
                    }
                }
            }
            assert!(dry > 0 && wet > 0, "{dry} dry, {wet} wet");
        }
    }

    /// LOD-1 spec §3.4 D4.3 / §4 I8: an inflow line ends on the last
    /// fine-dry centreline vertex and the next centreline vertex is water;
    /// the drawn line is a prefix of the full centreline (absent while
    /// that prefix is a single vertex — the resolution is one vertex).
    #[test]
    fn inflow_line_stops_at_the_fine_shoreline() {
        let reach = lake_reach(ReachFlow::Inflow, 0.5);
        let mut truncated = false;
        for depth in 0..=6 {
            let centreline = classified_centreline(&reach.evaluator, depth);
            let pieces = reach.evaluator.river_path(0, depth);
            let first_wet = centreline
                .iter()
                .position(|&(_, wet)| wet)
                .expect("the lake cell centroid lies below its surface");
            assert_eq!(pieces.len(), usize::from(first_wet >= 2), "depth {depth}");
            let drawn = pieces.concat();
            assert_eq!(drawn.len(), if first_wet >= 2 { first_wet } else { 0 });
            for (index, point) in drawn.iter().enumerate() {
                assert_eq!(*point, centreline[index].0);
                assert!(!centreline[index].1, "a drawn vertex is fine water");
            }
            truncated |= first_wet + 1 < centreline.len();
        }
        assert!(truncated, "the shoreline must cut the reach somewhere");
    }

    /// The outflow mirror: the line starts on the first fine-dry vertex
    /// after the last water vertex.
    #[test]
    fn outflow_line_starts_at_the_fine_shoreline() {
        let reach = lake_reach(ReachFlow::Outflow, 0.5);
        for depth in 0..=6 {
            let centreline = classified_centreline(&reach.evaluator, depth);
            let pieces = reach.evaluator.river_path(0, depth);
            let last_wet = centreline
                .iter()
                .rposition(|&(_, wet)| wet)
                .expect("the lake cell centroid lies below its surface");
            let kept = centreline.len() - last_wet - 1;
            assert_eq!(pieces.len(), usize::from(kept >= 2), "depth {depth}");
            if kept >= 2 {
                assert_eq!(pieces[0].len(), kept);
                assert_eq!(pieces[0][0], centreline[last_wet + 1].0);
            }
        }
    }

    /// An overland reach drops exactly its fine-water vertices: nothing
    /// drawn lies on water, nothing dry is lost, and every piece is a line.
    #[test]
    fn overland_line_drops_only_fine_water_vertices() {
        let reach = lake_reach(ReachFlow::Overland, 0.5);
        let mut dropped = 0;
        for depth in 0..=6 {
            let centreline = classified_centreline(&reach.evaluator, depth);
            let pieces = reach.evaluator.river_path(0, depth);
            let drawn: Vec<UnitVector3> = pieces.concat();
            assert!(pieces.iter().all(|piece| piece.len() >= 2));
            for (index, &(point, wet)) in centreline.iter().enumerate() {
                let dry_neighbour = [index.wrapping_sub(1), index + 1].iter().any(|&other| {
                    centreline
                        .get(other)
                        .is_some_and(|&(_, other_wet)| !other_wet)
                });
                assert_eq!(
                    drawn.contains(&point),
                    !wet && dry_neighbour,
                    "a vertex is drawn iff it and a neighbour are dry"
                );
                dropped += usize::from(wet);
            }
        }
        assert!(dropped > 0, "the ring lake must reach the reach");
    }

    /// LOD-1 spec §3.4 D4.5: an inflow whose whole centreline stays dry is
    /// drawn to the water body's P5 node instead of vanishing.
    #[test]
    fn dry_inflow_falls_back_to_the_water_node() {
        let reach = lake_reach(ReachFlow::Inflow, -0.5);
        let surface = test_surface();
        let pieces = reach.evaluator.river_path(0, 4);
        assert_eq!(pieces.len(), 1);
        assert_eq!(
            pieces[0].last().copied(),
            Some(surface.cells()[reach.low].centroid)
        );
    }

    /// LOD-1 spec §3.4 D4.3: the carve follows the drawn line — nothing is
    /// carved along the clipped water part, so a clipped vertex is never
    /// cut below the bed of the last drawn vertex.
    #[test]
    fn carve_skips_the_clipped_water_part() {
        let reach = lake_reach(ReachFlow::Inflow, 0.5);
        let evaluator = &reach.evaluator;
        let river = &evaluator.amplifier().river_reaches()[0];
        let depth = 5;
        let vertices = centreline_vertices(evaluator, 0, river, depth).vertices;
        let classified = classified_centreline(evaluator, depth);
        let first_wet = classified.iter().position(|&(_, wet)| wet).unwrap();
        assert!(first_wet > 0);
        let last_kept_bed = bed_at_m(river, vertices[first_wet - 1].1);
        for &(point, _) in &classified[first_wet..] {
            if let Some(carve) = carve_elevation_m(evaluator, point, depth + 1, 0.3) {
                assert!(
                    carve >= last_kept_bed - 1.0e-9,
                    "carved {carve} below the drawn bed {last_kept_bed} on water"
                );
            }
        }
    }

    /// LOD-1 spec §3.4 D4.1: a reach touching dry land is the full
    /// centroid–portal–centroid chain on both sides (here the water cells
    /// publish no lake record, so no shoreline clips it); water to water
    /// draws nothing.
    #[test]
    fn river_path_uses_shared_portal_on_both_sides() {
        use SurfaceWaterKind::{DryLand, Lake};

        for (from_water, to_water, expected_points) in [
            (DryLand, DryLand, 3_usize),
            (DryLand, Lake, 3),
            (Lake, DryLand, 3),
            (Lake, Lake, 0),
        ] {
            let (evaluator, surface, edge_index) =
                evaluator_with_water_transition(from_water, to_water);
            let edge = &surface.edges()[edge_index];
            let [from, to] = edge.cells;
            let path = evaluator.river_path(0, 0).concat();
            assert_eq!(path.len(), expected_points);
            if expected_points > 0 {
                assert_eq!(
                    path.first().copied(),
                    Some(surface.cells()[from.raw() as usize].centroid)
                );
                assert_eq!(
                    path.last().copied(),
                    Some(surface.cells()[to.raw() as usize].centroid)
                );
                assert!(
                    path.contains(&edge.midpoint),
                    "every rendered reach is split at the shared-edge portal"
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
            let path = materialize_path(&evaluator, reach_index, depth);
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
            let path = materialize_path(&evaluator, reach_index, 9);
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
        let upstream = materialize_path(&evaluator, 0, 9);
        let downstream = materialize_path(&evaluator, 1, 9);
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
            let chain = materialize_path(&evaluator, reach_index, 0);
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
                let path = materialize_path(&evaluator, reach_index, depth);
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
            let path_a = materialize_path(&first, reach_index, depth);
            let path_b = materialize_path(&second, reach_index, depth);
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
                .map(|depth| materialize_path(&shallow_first, reach_index, depth))
                .collect();
            let mut descending: Vec<_> = (0..=cap)
                .rev()
                .map(|depth| materialize_path(&deep_first, reach_index, depth))
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
        let path = materialize_path(&evaluator, reach_index, depth);
        let wall_slope = TerrainAmplifier::carve_wall_slope(0.0);
        let mut previous = f64::INFINITY;
        for point in &path {
            let (lateral_m, fraction) = nearest_on_path(
                &evaluator,
                reach_index,
                reach,
                depth,
                None,
                point.components(),
            )
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
