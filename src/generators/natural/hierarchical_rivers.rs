//! Hierarchical river rerouting and meandering (plan M2 Task 4).
//!
//! Implements spec §10 amendment A6: river existence, topology,
//! discharge, and the monotone beds stay L0/P5 authority (M1 §7 and A4
//! untouched); levels below only refine the channel geometry along a
//! binary path tree congruent with the primitive tree. Each L0 reach
//! keeps its cell-centroid endpoints fixed and recursively displaces
//! sub-segment midpoints laterally — candidates scored by the uncarved
//! derived field plus seeded jitter (valley-following on slopes, free
//! meandering on plains) — with the amplitude cut off below half the
//! Leopold–Wolman meander wavelength (≈12 channel widths) and the whole
//! path confined to the parent chord's corridor. The §4 carve reads the
//! level's path through a bound-pruned descent of the same tree, so
//! valleys and displayed channels stay one geometry.

use std::sync::{Arc, RwLock};

use super::hierarchical_derivation::{signed_unit_noise, HierarchicalEvaluator};
use super::terrain_amplification::{arc_nearest_point, RiverReach, TerrainAmplifier};
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

/// Lazy per-reach memo of the A6 path tree: the bit-exact node points
/// of the deepest depth materialized so far (`2^d + 1` of them).
///
/// The tree is a pure function of the reach and the frozen seeds, and
/// deeper levels never move shallower nodes, so one array serves every
/// depth up to its own through a stride — and deepening only computes
/// the new levels' midpoints. Purely an accelerator (spec §6): every
/// consumer keeps deriving exactly the values the uncached recursion
/// produced.
#[derive(Default)]
pub(super) struct ReachPathCache {
    deepest: RwLock<Option<Arc<Vec<[f64; 3]>>>>,
}

/// One cache slot per reach, all empty (engine construction).
pub(super) fn fresh_reach_path_caches(reach_count: usize) -> Vec<ReachPathCache> {
    (0..reach_count)
        .map(|_| ReachPathCache::default())
        .collect()
}

/// The reach's path points at `depth` or deeper (`depth` pre-clamped to
/// the reach cap and ≥ 1), deepening the memo level by level.
fn cached_path(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    reach: &RiverReach,
    depth: u8,
) -> Arc<Vec<[f64; 3]>> {
    let slot = evaluator
        .river_path_slot(reach_index)
        .expect("reach indices come from the engine's own reach lists");
    let needed = (1_usize << depth) + 1;
    let lock_clean = "the reach path lock only guards infallible derivation";
    if let Some(points) = slot
        .deepest
        .read()
        .expect(lock_clean)
        .as_ref()
        .filter(|points| points.len() >= needed)
    {
        return Arc::clone(points);
    }
    let mut guard = slot.deepest.write().expect(lock_clean);
    if let Some(points) = guard.as_ref().filter(|points| points.len() >= needed) {
        return Arc::clone(points);
    }
    let mut points: Vec<[f64; 3]> = guard
        .as_ref()
        .map(|points| points.as_ref().clone())
        .unwrap_or_else(|| vec![reach.from, reach.to]);
    if let Some(walk) = ReachWalk::new(evaluator, reach_index, reach) {
        while points.len() < needed {
            // The nodes between consecutive points of the depth-d array
            // sit at depth d, enumerated left to right — which is
            // exactly their `bits` path word.
            let parent_depth = (points.len() - 1).trailing_zeros() as u8;
            let mut next = Vec::with_capacity(points.len() * 2 - 1);
            for (bits, pair) in points.windows(2).enumerate() {
                next.push(pair[0]);
                next.push(walk.node_midpoint(pair[0], pair[1], parent_depth, bits as u32));
            }
            next.push(*points.last().expect("a path holds both endpoints"));
            points = next;
        }
    }
    let points = Arc::new(points);
    *guard = Some(Arc::clone(&points));
    points
}

/// Everything constant per reach during a tree walk.
struct ReachWalk<'a> {
    evaluator: &'a HierarchicalEvaluator,
    reach_index: u32,
    /// Great-circle normal of the L0 chord (corridor reference).
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
        reach: &RiverReach,
    ) -> Option<ReachWalk<'a>> {
        let chord_normal = normalized(cross(reach.from, reach.to))?;
        let radius_m = evaluator.amplifier().radius_m();
        let chord_rad = arc_angle(reach.from, reach.to);
        Some(ReachWalk {
            evaluator,
            reach_index,
            chord_normal,
            corridor_rad: MEANDER_CORRIDOR_FRACTION * chord_rad,
            wavelength_m: MEANDER_WAVELENGTH_PER_WIDTH * 2.0 * reach.half_width_m,
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
            // Corridor: lateral distance to the L0 chord's great circle.
            let lateral_rad = dot(candidate, self.chord_normal)
                .clamp(-1.0, 1.0)
                .asin()
                .abs();
            if offset != 0.0 && lateral_rad > self.corridor_rad {
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

    /// Canonical candidate seed: `blake3(域 ∥ "r" ∥ reach ∥ depth ∥
    /// bits ∥ candidate)` (amendment A6.1/A6.3).
    fn candidate_seed(&self, depth: u8, bits: u32, candidate: u8) -> [u8; 32] {
        let mut hasher = self.evaluator.seed_hasher();
        hasher.update(b"r");
        hasher.update(&self.reach_index.to_le_bytes());
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
    let chord_m = arc_angle(reach.from, reach.to) * evaluator.amplifier().radius_m();
    let half_wavelength = MEANDER_WAVELENGTH_PER_WIDTH * reach.half_width_m;
    if !(chord_m.is_finite() && half_wavelength.is_finite()) || chord_m < half_wavelength {
        return 0;
    }
    ((chord_m / half_wavelength).log2().floor() as u8).min(PATH_DEPTH_LIMIT)
}

/// Materializes one reach's rerouted polyline at `depth` (clamped to
/// the reach cap): `2^depth + 1` points, endpoints the cell centroids.
pub(super) fn materialize_path(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    depth: u8,
) -> Vec<UnitVector3> {
    let reaches = evaluator.amplifier().river_reaches();
    let Some(reach) = reaches.get(reach_index as usize) else {
        return Vec::new();
    };
    let depth = depth.min(path_depth_cap(evaluator, reach_index));
    let as_unit = |p: [f64; 3]| {
        UnitVector3::new(p[0], p[1], p[2]).expect("reach path points are unit directions")
    };
    if depth == 0 {
        return vec![as_unit(reach.from), as_unit(reach.to)];
    }
    let points = cached_path(evaluator, reach_index, reach, depth);
    // A degenerate reach memoizes only its chord; read what exists.
    let depth = u32::from(depth).min((points.len() - 1).trailing_zeros());
    let stride = (points.len() - 1) >> depth;
    (0..=(1_usize << depth))
        .map(|index| as_unit(points[index * stride]))
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
            let depth = leaf_level
                .saturating_sub(1)
                .min(path_depth_cap(evaluator, reach_index));
            let Some((lateral_m, fraction)) =
                nearest_on_path(evaluator, reach_index, reach, depth, p)
            else {
                continue;
            };
            let bed = reach.bed_from_m + fraction * (reach.bed_to_m - reach.bed_from_m);
            let value = bed + (lateral_m - reach.half_width_m).max(0.0) * wall_slope;
            carve = Some(carve.map_or(value, |current: f64| current.min(value)));
        }
    }
    carve
}

/// Nearest point of `p` on the reach's depth-`depth` path: a bound-
/// pruned descent of the path tree — a subtree is skipped when even its
/// remaining lateral slack cannot beat the current best (A6.5).
fn nearest_on_path(
    evaluator: &HierarchicalEvaluator,
    reach_index: u32,
    reach: &RiverReach,
    depth: u8,
    p: [f64; 3],
) -> Option<(f64, f64)> {
    let radius_m = evaluator.amplifier().radius_m();
    if depth == 0 {
        return arc_nearest_point(reach.from, reach.to, p, radius_m);
    }
    let points = cached_path(evaluator, reach_index, reach, depth);
    // A degenerate reach memoizes only its chord.
    let depth = u32::from(depth).min((points.len() - 1).trailing_zeros());
    if depth == 0 {
        return arc_nearest_point(reach.from, reach.to, p, radius_m);
    }
    let stride = (points.len() - 1) >> depth;
    let mut best: Option<(f64, f64)> = None;
    descend_nearest(
        &points,
        stride,
        (0, points.len() - 1),
        (0.0, 1.0),
        p,
        radius_m,
        &mut best,
    );
    best
}

fn descend_nearest(
    points: &[[f64; 3]],
    stride: usize,
    (start, end): (usize, usize),
    (fraction_a, fraction_b): (f64, f64),
    p: [f64; 3],
    radius_m: f64,
    best: &mut Option<(f64, f64)>,
) {
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
    // Every deeper displacement stays within the geometric series
    // Σ 0.25·ℓ/2^k = 0.25·ℓ of this segment's arc.
    let slack_m = MEANDER_SINUOSITY_FRACTION * arc_angle(a, b) * radius_m;
    if let Some((current, _)) = best {
        if chord_lateral - slack_m > *current {
            return;
        }
    }
    let middle = (start + end) / 2;
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
        descend_nearest(points, stride, span, fractions, p, radius_m, best);
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

fn arc_angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    dot(a, b).clamp(-1.0, 1.0).acos()
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
    use crate::world::natural::{RiverSegment, RiverSegmentKind, SphericalOrogenyKind};
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
            .with_rivers(&surface, &segments)
            .unwrap();
        (evaluator, surface)
    }

    /// Amendment A6.1: depth 0 is exactly the L0 chain and endpoints stay
    /// the cell centroids at every depth.
    #[test]
    fn depth_zero_is_the_l0_chain_with_fixed_endpoints() {
        let (evaluator, _surface) = evaluator_with_rivers();
        let reaches = evaluator.amplifier().river_reaches();
        for reach_index in 0..evaluator.river_reach_count() as u32 {
            let reach = &reaches[reach_index as usize];
            let from = UnitVector3::new(reach.from[0], reach.from[1], reach.from[2]).unwrap();
            let to = UnitVector3::new(reach.to[0], reach.to[1], reach.to[2]).unwrap();
            let chain = evaluator.river_path(reach_index, 0);
            assert_eq!(chain.len(), 2);
            assert_eq!(chain[0], from);
            assert_eq!(chain[1], to);
            for depth in 1..=evaluator.river_path_depth_cap(reach_index).min(8) {
                let path = evaluator.river_path(reach_index, depth);
                assert_eq!(path.len(), (1 << depth) + 1);
                assert_eq!(*path.first().unwrap(), from);
                assert_eq!(*path.last().unwrap(), to);
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

    /// Amendment A6.4: no path point strays beyond the corridor fraction
    /// of the chord from the chord's great circle.
    #[test]
    fn paths_stay_inside_the_parent_corridor() {
        let (evaluator, _surface) = evaluator_with_rivers();
        let reaches = evaluator.amplifier().river_reaches();
        let radius = evaluator.amplifier().radius_m();
        for reach_index in 0..evaluator.river_reach_count() as u32 {
            let reach = &reaches[reach_index as usize];
            let normal = normalized(cross(reach.from, reach.to)).unwrap();
            let chord_rad = arc_angle(reach.from, reach.to);
            let bound_rad = MEANDER_CORRIDOR_FRACTION * chord_rad + 1.0e-12;
            let depth = evaluator.river_path_depth_cap(reach_index).min(8);
            for point in evaluator.river_path(reach_index, depth) {
                let lateral = dot(point.components(), normal)
                    .clamp(-1.0, 1.0)
                    .asin()
                    .abs();
                assert!(
                    lateral <= bound_rad,
                    "point strays {:.1} m beyond the corridor",
                    (lateral - bound_rad) * radius
                );
            }
        }
    }

    /// Amendment A6.2: the wavelength cutoff — the wide reach caps
    /// shallower than the narrow one, and beyond the cap the polyline
    /// stops changing.
    #[test]
    fn wavelength_cutoff_caps_the_tree() {
        let (evaluator, _surface) = evaluator_with_rivers();
        let narrow_cap = evaluator.river_path_depth_cap(0);
        let wide_cap = evaluator.river_path_depth_cap(1);
        assert!(
            narrow_cap > wide_cap,
            "narrow {narrow_cap} must out-refine wide {wide_cap}"
        );
        let capped = evaluator.river_path(1, wide_cap);
        let beyond = evaluator.river_path(1, wide_cap + 3);
        assert_eq!(capped.len(), beyond.len());
        for (left, right) in capped.iter().zip(&beyond) {
            assert_eq!(
                left.components().map(f64::to_bits),
                right.components().map(f64::to_bits)
            );
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
            let Some(walk) = ReachWalk::new(&evaluator, reach_index, reach) else {
                continue;
            };
            let chosen = walk.node_midpoint(reach.from, reach.to, 0, 0);
            let geometric = normalized(add(reach.from, reach.to)).unwrap();
            let length_m = arc_angle(reach.from, reach.to) * walk.radius_m;
            if length_m < walk.wavelength_m * 0.5 {
                assert_eq!(chosen, geometric);
                continue;
            }
            let amplitude_rad = MEANDER_SINUOSITY_FRACTION * length_m / walk.radius_m;
            let normal = normalized(cross(reach.from, reach.to)).unwrap();
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

        // Deep carve along the deep path: bed-dominated and monotone
        // downstream on the wide reach.
        let reach_index = 1_u32;
        let reach = &reaches[reach_index as usize];
        let depth = evaluator.river_path_depth_cap(reach_index);
        let path = evaluator.river_path(reach_index, depth);
        let level = depth + 1;
        let mut previous = f64::INFINITY;
        for point in &path {
            let carve = carve_elevation_m(&evaluator, *point, level, 0.0)
                .expect("points on the channel are inside the carve corridor");
            assert!(
                carve <= previous + 1.0e-6,
                "carve rose downstream: {carve} > {previous}"
            );
            previous = carve;
        }
        assert!(reach.bed_from_m >= reach.bed_to_m);
    }
}
