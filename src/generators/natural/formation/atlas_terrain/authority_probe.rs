//! Offline authority-layer probe: Atlas water topology solved once on a
//! Goldberg fan level Lk versus the production canonical support.
//!
//! Every candidate reuses the production operators (`source_fans`,
//! `refine_triangles`, `SupportGraph`, `initialize_water`, the canonical fine
//! evaluator and `integrate_positive_linear_triangle`). Ignored, Release only;
//! prints numbers and sets no gates.

use super::carrier_probe::{center, sph_area, Carrier};
use super::geometry::{
    initialize_water, refine_triangles, source_fans, InitialWater, SupportGraph, SupportTriangle,
};
use super::*;
use crate::generators::natural::subdivision_children;
use crate::generators::natural::surface_water_geometry::integrate_positive_linear_triangle;
use crate::generators::spatial::GeodesicVoronoiBuilder;
use crate::world::natural::SurfaceWaterKind;
use crate::world::spatial::{
    central_angle, latitude_longitude_radians, spherical_triangle_area_unit,
};
use crate::world::SphericalSpaceSpec;
use rayon::prelude::*;
use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

const LEVELS: [u8; 3] = [1, 2, 3];
pub(super) const NONE: u32 = u32::MAX;
/// Sample classes: ocean, lake, dry with bed < 0, dry with bed >= 0.
const CLASS: [&str; 4] = ["ocean", "lake", "dry<0", "dry>=0"];

/// One support face under the production rule of `geometry::assemble_geometry`.
struct FaceWater {
    area: f64,
    ocean: bool,
    ocean_fraction: f64,
    lake_fraction: f64,
    level: Option<f64>,
    lake: Option<usize>,
    depth_integral: f64,
}

/// `assemble_geometry` is private to `geometry` and reads anchors by Atlas
/// site index, so the probe applies its per-face rule to adapted anchors.
/// The baseline rerun checks this copy against every stored production face.
fn face_water(
    nodes: [usize; 3],
    directions: &[UnitVector3],
    bed: &[f64],
    water: &InitialWater,
    radius2: f64,
) -> FaceWater {
    let [a, b, c] = nodes.map(|i| directions[i]);
    let area = spherical_triangle_area_unit(a, b, c) * radius2;
    let ocean = nodes.iter().any(|&i| water.ocean[i]);
    let lake = nodes.iter().find_map(|&i| water.lake[i]);
    let level = if ocean {
        Some(0.0)
    } else {
        nodes
            .iter()
            .filter_map(|&i| water.level[i])
            .max_by(f64::total_cmp)
    };
    let (wet, depth_integral) = level.map_or((0.0, 0.0), |level| {
        integrate_positive_linear_triangle(nodes.map(|i| level - bed[i]))
    });
    FaceWater {
        area,
        ocean,
        ocean_fraction: if ocean { wet } else { 0.0 },
        lake_fraction: if lake.is_some() { wet } else { 0.0 },
        level,
        lake,
        depth_integral,
    }
}

pub(super) struct Solved {
    directions: Vec<UnitVector3>,
    triangles: Vec<SupportTriangle>,
    pub(super) bed: Vec<f64>,
    graph: SupportGraph,
    pub(super) water: InitialWater,
    faces: Vec<FaceWater>,
}

#[derive(Default, Clone)]
pub(super) struct Feature {
    pub(super) area: f64,
    pub(super) volume: f64,
    extra: f64,
    pub(super) level: f64,
    centroid: [f64; 3],
}
impl Feature {
    pub(super) fn add_centroid(&mut self, v: [f64; 3], w: f64) {
        for i in 0..3 {
            self.centroid[i] += v[i] * w;
        }
    }
    fn latlon(&self) -> (f64, f64) {
        let [x, y, z] = self.centroid;
        UnitVector3::new(x, y, z).map_or((f64::NAN, f64::NAN), |d| {
            let (lat, lon) = latitude_longitude_radians(d);
            (lat.to_degrees(), lon.to_degrees())
        })
    }
}

pub(super) struct Summary {
    /// Per node: non-ocean B < 0 component id, or `NONE`.
    dep_of: Vec<u32>,
    ocean_area: f64,
    ocean_volume: f64,
    lake_area: f64,
    lake_volume: f64,
    dry_neg_area: f64,
    lake_neg_area: f64,
    pub(super) basin_of: Vec<u32>,
    pub(super) basins: Vec<Feature>,
    pub(super) lakes: Vec<Feature>,
    /// `area` = dry part below sea level, `extra` = lake-covered part below sea level.
    depressions: Vec<Feature>,
    /// Per owner: ocean, lake, dry area, total area, ocean volume, lake volume.
    cells: Vec<[f64; 6]>,
}

pub(super) fn components(
    graph: &SupportGraph,
    n: usize,
    member: impl Fn(usize) -> bool,
) -> (Vec<u32>, usize) {
    let mut id = vec![NONE; n];
    let mut count = 0_u32;
    let mut queue = VecDeque::new();
    for start in 0..n {
        if id[start] != NONE || !member(start) {
            continue;
        }
        id[start] = count;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            for &j in graph.neighbors(i) {
                let j = j as usize;
                if id[j] == NONE && member(j) {
                    id[j] = count;
                    queue.push_back(j);
                }
            }
        }
        count += 1;
    }
    (id, count as usize)
}

fn summarize(s: &Solved, owners: usize) -> Summary {
    let n = s.bed.len();
    let (basin_of, basin_count) = components(&s.graph, n, |i| s.water.ocean[i]);
    let (dep_of, dep_count) = components(&s.graph, n, |i| !s.water.ocean[i] && s.bed[i] < 0.0);
    let lake_count = s.water.lake.iter().flatten().max().map_or(0, |l| l + 1);
    let mut out = Summary {
        dep_of: Vec::new(),
        ocean_area: 0.0,
        ocean_volume: 0.0,
        lake_area: 0.0,
        lake_volume: 0.0,
        dry_neg_area: 0.0,
        lake_neg_area: 0.0,
        basin_of,
        basins: vec![Feature::default(); basin_count],
        lakes: vec![Feature::default(); lake_count],
        depressions: vec![Feature::default(); dep_count],
        cells: vec![[0.0; 6]; owners],
    };
    for ((nodes, owner), fw) in s.triangles.iter().zip(&s.faces) {
        let cv = center(&nodes.map(|i| s.directions[i])).components();
        let a = fw.area;
        let cell = &mut out.cells[*owner];
        cell[0] += a * fw.ocean_fraction;
        cell[1] += a * fw.lake_fraction;
        cell[2] += a * (1.0 - fw.ocean_fraction - fw.lake_fraction);
        cell[3] += a;
        if fw.ocean {
            cell[4] += a * fw.depth_integral;
            let b = nodes
                .iter()
                .map(|&i| out.basin_of[i])
                .find(|&b| b != NONE)
                .expect("ocean face has an ocean node") as usize;
            let basin = &mut out.basins[b];
            basin.area += a * fw.ocean_fraction;
            basin.volume += a * fw.depth_integral;
            basin.add_centroid(cv, a * fw.ocean_fraction);
            out.ocean_area += a * fw.ocean_fraction;
            out.ocean_volume += a * fw.depth_integral;
        }
        if let Some(l) = fw.lake {
            cell[5] += a * fw.depth_integral;
            let lake = &mut out.lakes[l];
            lake.area += a * fw.lake_fraction;
            lake.volume += a * fw.depth_integral;
            lake.level = fw.level.expect("lake face has a level");
            lake.add_centroid(cv, a * fw.lake_fraction);
            out.lake_area += a * fw.lake_fraction;
            out.lake_volume += a * fw.depth_integral;
        }
        let below = |t: f64| integrate_positive_linear_triangle(nodes.map(|i| t - s.bed[i])).0;
        let neg = below(0.0);
        if neg > 0.0 && !fw.ocean {
            let wet_neg = fw.level.map_or(0.0, |level| below(level.min(0.0)));
            let d = nodes
                .iter()
                .map(|&i| dep_of[i])
                .find(|&d| d != NONE)
                .expect("negative non-ocean face has a depression node")
                as usize;
            let dep = &mut out.depressions[d];
            dep.area += a * (neg - wet_neg);
            dep.extra += a * wet_neg;
            dep.add_centroid(cv, a * neg);
            out.dry_neg_area += a * (neg - wet_neg);
            out.lake_neg_area += a * wet_neg;
        }
    }
    out.dep_of = dep_of;
    out
}

pub(super) const KM2: f64 = 1.0e6;
pub(super) const KM3: f64 = 1.0e9;

fn print_summary(name: &str, s: &Summary) {
    let dry_deps = s.depressions.iter().filter(|d| d.extra == 0.0).count();
    eprintln!(
        "SUMMARY {name} ocean_km2={:.0} ocean_km3={:.0} basins={} lakes={} lake_km2={:.1} lake_km3={:.3} \
         neg_depressions={} dry_only={} lake_hosting={} dry_neg_km2={:.1} lake_neg_km2={:.1}",
        s.ocean_area / KM2,
        s.ocean_volume / KM3,
        s.basins.len(),
        s.lakes.len(),
        s.lake_area / KM2,
        s.lake_volume / KM3,
        s.depressions.len(),
        dry_deps,
        s.depressions.len() - dry_deps,
        s.dry_neg_area / KM2,
        s.lake_neg_area / KM2,
    );
    let mut order: Vec<usize> = (0..s.basins.len()).collect();
    order.sort_by(|a, b| s.basins[*b].area.total_cmp(&s.basins[*a].area));
    for &b in &order {
        let f = &s.basins[b];
        let (lat, lon) = f.latlon();
        eprintln!(
            "BASIN {name} id={b} area_km2={:.1} volume_km3={:.2} lat={lat:.2} lon={lon:.2}",
            f.area / KM2,
            f.volume / KM3
        );
    }
    let mut order: Vec<usize> = (0..s.depressions.len()).collect();
    order.sort_by(|a, b| {
        let total = |d: &Feature| d.area + d.extra;
        total(&s.depressions[*b]).total_cmp(&total(&s.depressions[*a]))
    });
    let mut sizes: Vec<f64> = s
        .depressions
        .iter()
        .map(|d| (d.area + d.extra) / KM2)
        .collect();
    sizes.sort_by(f64::total_cmp);
    if !sizes.is_empty() {
        eprintln!(
            "DEPRESSION_SIZES {name} km2 p50={:.2} p90={:.2} p99={:.2} max={:.1}",
            pct(&sizes, 0.5),
            pct(&sizes, 0.9),
            pct(&sizes, 0.99),
            sizes[sizes.len() - 1]
        );
    }
    for &d in order.iter().take(12) {
        let f = &s.depressions[d];
        let (lat, lon) = f.latlon();
        eprintln!(
            "DEPRESSION {name} id={d} dry_km2={:.1} lake_km2={:.1} lat={lat:.2} lon={lon:.2}",
            f.area / KM2,
            f.extra / KM2
        );
    }
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// Bipartite grouping of baseline features `b` and candidate features `c`
/// joined by shared source anchors. Returns `(baseline ids, candidate ids)`.
fn groups(
    nb: usize,
    nc: usize,
    links: &[(Option<usize>, Vec<usize>)],
) -> Vec<(Vec<usize>, Vec<usize>)> {
    let mut parent: Vec<usize> = (0..nb + nc).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut y = x;
        while p[y] != r {
            let next = p[y];
            p[y] = r;
            y = next;
        }
        r
    }
    for (b, cs) in links {
        let ids: Vec<usize> = b.iter().copied().chain(cs.iter().map(|c| nb + c)).collect();
        for w in ids.windows(2) {
            let (x, y) = (find(&mut parent, w[0]), find(&mut parent, w[1]));
            parent[x] = y;
        }
    }
    // Untouched features still form their own singleton groups.
    let mut by_root: BTreeMap<usize, (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for i in 0..nb + nc {
        let root = find(&mut parent, i);
        let entry = by_root.entry(root).or_default();
        if i < nb {
            entry.0.push(i);
        } else {
            entry.1.push(i - nb);
        }
    }
    by_root.into_values().collect()
}

pub(super) fn report_groups(
    kind: &str,
    k: u8,
    base: &[Feature],
    cand: &[Feature],
    links: &[(Option<usize>, Vec<usize>)],
) {
    let all = groups(base.len(), cand.len(), links);
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut matched_rel = Vec::new();
    for (bs, cs) in &all {
        let label = match (bs.len(), cs.len()) {
            (1, 1) => "kept",
            (0, _) => "new",
            (_, 0) => "lost",
            (1, _) => "split",
            (_, 1) => "merged",
            _ => "rewired",
        };
        *tally.entry(label).or_default() += 1;
        let ba: f64 = bs.iter().map(|&b| base[b].area).sum();
        let bv: f64 = bs.iter().map(|&b| base[b].volume).sum();
        let ca: f64 = cs.iter().map(|&c| cand[c].area).sum();
        let cv: f64 = cs.iter().map(|&c| cand[c].volume).sum();
        if label == "kept" {
            matched_rel.push(((cv - bv) / bv, ba));
        }
        let loc = bs
            .first()
            .map(|&b| base[b].latlon())
            .or_else(|| cs.first().map(|&c| cand[c].latlon()))
            .unwrap_or((f64::NAN, f64::NAN));
        let levels = |f: &[Feature], ids: &[usize]| {
            ids.iter()
                .map(|&i| format!("{:.1}", f[i].level))
                .collect::<Vec<_>>()
                .join("/")
        };
        if label != "kept" || kind == "lake" {
            eprintln!(
                "GROUP {kind} L{k} {label} base={bs:?} cand={cs:?} base_area_km2={:.1} cand_area_km2={:.1} \
                 base_km3={:.4} cand_km3={:.4} rel_vol={:+.4} lat={:.2} lon={:.2} base_level={} cand_level={}",
                ba / KM2,
                ca / KM2,
                bv / KM3,
                cv / KM3,
                if bv > 0.0 { (cv - bv) / bv } else { f64::NAN },
                loc.0,
                loc.1,
                levels(base, bs),
                levels(cand, cs),
            );
        }
    }
    matched_rel.sort_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));
    if let Some(worst) = matched_rel.last() {
        eprintln!(
            "GROUPS {kind} L{k} {tally:?} kept_abs_rel_vol p50={:.4} max={:.4} (area_km2 of max={:.1})",
            matched_rel[matched_rel.len() / 2].0.abs(),
            worst.0.abs(),
            worst.1 / KM2
        );
    } else {
        eprintln!("GROUPS {kind} L{k} {tally:?}");
    }
}

struct Candidate {
    k: u8,
    solved: Solved,
    summary: Summary,
    /// Containing Lk face corners of each marine Atlas site.
    marine_corners: Vec<[usize; 3]>,
    /// Lk nodes seeded by each lake prior (empty when the prior is invalid).
    prior_nodes: Vec<Vec<usize>>,
}

/// Lake-prior placement on a support that does not contain the prior site.
#[derive(Clone, Copy, PartialEq)]
enum PriorPlacement {
    /// All three corners of the containing face (first run).
    Face,
    /// Valid iff the canonical bed at the exact site is below the prior level
    /// (the A5b site rule applied to lakes); seeded at the pit reached by
    /// steepest descent from the containing face's lowest corner.
    Descent,
}

/// Steepest descent on the support graph (O'Callaghan & Mark 1984 D8 rule on
/// an irregular triangulation: largest drop per arc length) to a node without
/// a lower neighbor.
fn descend(start: usize, graph: &SupportGraph, bed: &[f64], directions: &[UnitVector3]) -> usize {
    let mut i = start;
    loop {
        let slope = |j: usize| (bed[i] - bed[j]) / central_angle(directions[i], directions[j]);
        let next = graph
            .neighbors(i)
            .iter()
            .map(|&j| j as usize)
            .filter(|&j| bed[j] < bed[i])
            .max_by(|&a, &b| slope(a).total_cmp(&slope(b)));
        match next {
            Some(j) => i = j,
            None => return i,
        }
    }
}

struct Shared<'a> {
    goldberg: &'a SphericalSurfaceSnapshot,
    carrier: &'a Carrier<'a>,
    fan_offset: &'a [usize],
    fine: &'a FineTerrain,
    sites: &'a [UnitVector3],
    marine: &'a [usize],
    priors: &'a [(usize, f64)],
    /// Canonical bed at each prior's exact site is below its level.
    prior_valid: &'a [bool],
    placement: PriorPlacement,
    radius2: f64,
}

fn face_index(fan_offset: &[usize], k: u8, cell: usize, sector: usize, path: usize) -> usize {
    (fan_offset[cell] + sector) * 4_usize.pow(u32::from(k) - 1) + path
}

fn build_candidate(k: u8, ctx: &Shared<'_>, cancellation: &BuildCancellation) -> Candidate {
    let t = Instant::now();
    let (mut directions, mut triangles) = source_fans(ctx.goldberg);
    // Frozen T1 v2 fans use the cell centroid as apex; `source_fans` uses the site.
    for (i, cell) in ctx.goldberg.cells().iter().enumerate() {
        assert_eq!(cell.id.raw() as usize, i);
        directions[i] = cell.centroid;
    }
    for _ in 1..k {
        triangles = refine_triangles(&mut directions, triangles, cancellation).unwrap();
    }
    let mesh_ms = t.elapsed().as_millis();
    let t = Instant::now();
    // Same serial bounded batches as `geometry::build_geometry`.
    let mut bed = Vec::with_capacity(directions.len());
    for chunk in directions.chunks(4096) {
        let query = chunk.iter().map(|p| p.components()).collect::<Vec<_>>();
        bed.extend(
            ctx.fine
                .sample_surface_batch(&query, &|| false)
                .unwrap()
                .into_iter()
                .map(|s| s.bed_elevation_m),
        );
    }
    let bed_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let graph = SupportGraph::from_edges(
        directions.len(),
        triangles
            .iter()
            .flat_map(|(nodes, _)| (0..3).map(move |i| [nodes[i], nodes[(i + 1) % 3]])),
        cancellation,
    )
    .unwrap();
    let graph_ms = t.elapsed().as_millis();
    let t = Instant::now();
    // Anchor adaptation: an Atlas site is not an Lk node, so all three corners of
    // the Lk face containing it become anchor candidates; `initialize_water`
    // itself keeps only marine corners with B < 0 and prior corners below level.
    let corners_of = |d: UnitVector3| {
        let (key, _) = ctx.carrier.locate(d, k);
        let cell = (key >> 44) as usize;
        let sector = ((key >> 36) & 0xff) as usize;
        let path = (key & ((1 << 36) - 1)) as usize;
        triangles[face_index(ctx.fan_offset, k, cell, sector, path)].0
    };
    let marine_corners: Vec<[usize; 3]> = ctx
        .marine
        .iter()
        .map(|&s| corners_of(ctx.sites[s]))
        .collect();
    let prior_nodes: Vec<Vec<usize>> = ctx
        .priors
        .iter()
        .zip(ctx.prior_valid)
        .map(|(&(s, _), &valid)| {
            let corners = corners_of(ctx.sites[s]);
            match ctx.placement {
                PriorPlacement::Face => corners.to_vec(),
                PriorPlacement::Descent if !valid => Vec::new(),
                PriorPlacement::Descent => {
                    let lowest = corners
                        .into_iter()
                        .min_by(|a, b| bed[*a].total_cmp(&bed[*b]))
                        .expect("three corners");
                    vec![descend(lowest, &graph, &bed, &directions)]
                }
            }
        })
        .collect();
    let mut seeds = vec![false; directions.len()];
    for corners in &marine_corners {
        for &i in corners {
            seeds[i] = true;
        }
    }
    let priors: Vec<(usize, f64)> = prior_nodes
        .iter()
        .zip(ctx.priors)
        .flat_map(|(nodes, &(_, level))| nodes.iter().map(move |&i| (i, level)))
        .collect();
    let anchor_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let water = initialize_water(&bed, &graph, &seeds, &priors, cancellation).unwrap();
    let water_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let faces: Vec<FaceWater> = triangles
        .iter()
        .map(|(nodes, _)| face_water(*nodes, &directions, &bed, &water, ctx.radius2))
        .collect();
    let face_ms = t.elapsed().as_millis();
    eprintln!(
        "CANDIDATE L{k} nodes={} faces={} mesh_ms={mesh_ms} bed_ms={bed_ms} graph_ms={graph_ms} \
         anchor_ms={anchor_ms} water_ms={water_ms} face_ms={face_ms} total_ms={}",
        directions.len(),
        triangles.len(),
        mesh_ms + bed_ms + graph_ms + anchor_ms + water_ms + face_ms
    );
    let solved = Solved {
        directions,
        triangles,
        bed,
        graph,
        water,
        faces,
    };
    let t = Instant::now();
    let summary = summarize(&solved, ctx.goldberg.cells().len());
    eprintln!("CANDIDATE L{k} summarize_ms={}", t.elapsed().as_millis());
    Candidate {
        k,
        solved,
        summary,
        marine_corners,
        prior_nodes,
    }
}

#[derive(Clone, Copy)]
struct CellSample {
    area: f64,
    base: [f64; 4],
    base_volume: [f64; 2],
    cand: [[f64; 4]; 3],
    cand_volume: [[f64; 2]; 3],
    confusion: [[[f64; 4]; 4]; 3],
    outside: usize,
}
impl Default for CellSample {
    fn default() -> Self {
        Self {
            area: 0.0,
            base: [0.0; 4],
            base_volume: [0.0; 2],
            cand: [[0.0; 4]; 3],
            cand_volume: [[0.0; 2]; 3],
            confusion: [[[0.0; 4]; 4]; 3],
            outside: 0,
        }
    }
}

fn class_of(water: SurfaceWaterKind, bed: f64) -> usize {
    match water {
        SurfaceWaterKind::Ocean => 0,
        SurfaceWaterKind::Lake => 1,
        SurfaceWaterKind::DryLand => 2 + usize::from(bed >= 0.0),
    }
}

/// Equal-partition sub-triangles of one carrier cell at `level` (>= 1):
/// `(sector, leaf index, triangle)`, leaf index in production child order.
fn cell_leaves(
    carrier: &Carrier<'_>,
    cell: usize,
    level: u8,
) -> Vec<(usize, usize, [UnitVector3; 3])> {
    let mut out = Vec::new();
    for s in 0..carrier.sectors(cell) {
        let mut tris = vec![carrier.corners(cell, s)];
        for _ in 1..level {
            tris = tris.iter().flat_map(|t| subdivision_children(*t)).collect();
        }
        out.extend(tris.into_iter().enumerate().map(|(l, t)| (s, l, t)));
    }
    out
}

fn sample_cell(
    snapshot: &AtlasTerrainSnapshot,
    ctx: &Shared<'_>,
    candidates: &[Candidate],
    cell: usize,
    level: u8,
) -> CellSample {
    let leaves = cell_leaves(ctx.carrier, cell, level);
    let dirs: Vec<[f64; 3]> = leaves
        .iter()
        .map(|(_, _, t)| center(t).components())
        .collect();
    let samples = snapshot.sample_surface_batch(&dirs, &|| false).unwrap();
    let mut out = CellSample::default();
    for (smp, (sector, leaf, tri)) in samples.iter().zip(&leaves) {
        let w = sph_area(tri) * ctx.radius2;
        let bed = smp.bed_elevation_m;
        let bc = class_of(smp.water, bed);
        out.area += w;
        out.base[bc] += w;
        if let Some(depth) = smp.water_depth_m() {
            out.base_volume[bc] += w * depth;
        }
        let point = center(tri);
        for (ki, cand) in candidates.iter().enumerate() {
            let k = cand.k;
            let f = face_index(ctx.fan_offset, k, cell, *sector, leaf >> (2 * (level - k)));
            let corners = cand.solved.triangles[f]
                .0
                .map(|i| cand.solved.directions[i]);
            out.outside +=
                usize::from(crate::generators::natural::interior_margin(&corners, point) < -1e-12);
            let fw = &cand.solved.faces[f];
            let wet_level = fw.level.filter(|&lv| bed < lv);
            let cc = match wet_level {
                None => 2 + usize::from(bed >= 0.0),
                Some(_) => usize::from(fw.lake.is_some()),
            };
            out.cand[ki][cc] += w;
            if let Some(lv) = wet_level {
                out.cand_volume[ki][cc] += w * (lv - bed);
            }
            out.confusion[ki][bc][cc] += w;
        }
    }
    out
}

pub(super) fn dist(label: &str, values: &[f64]) {
    let mut abs: Vec<f64> = values.iter().map(|v| v.abs()).collect();
    abs.sort_by(f64::total_cmp);
    let n = abs.len().max(1) as f64;
    let mean = abs.iter().sum::<f64>() / n;
    let rms = (abs.iter().map(|v| v * v).sum::<f64>() / n).sqrt();
    let signed = values.iter().sum::<f64>() / n;
    let nonzero = abs.iter().filter(|v| **v > 0.0).count();
    eprintln!(
        "DIST {label} n={} nonzero={nonzero} mean_abs={mean:.5} rms={rms:.5} mean_signed={signed:+.5} \
         p50={:.5} p90={:.5} p99={:.5} max={:.5}",
        abs.len(),
        pct(&abs, 0.5),
        pct(&abs, 0.9),
        pct(&abs, 0.99),
        abs[abs.len() - 1]
    );
}

pub(super) struct Baseline {
    pub(super) solved: Solved,
    pub(super) summary: Summary,
    /// Atlas sites the upstream generator classifies as ocean.
    pub(super) marine: Vec<usize>,
    pub(super) priors: Vec<(usize, f64)>,
}

/// Generate the production product and rerun its flood once on the published
/// support so node-level ocean/lake identities exist; checks the face rule copy.
pub(super) fn publish_baseline(
    cancellation: &BuildCancellation,
) -> (AtlasTerrainArtifact, Baseline) {
    let request = AtlasTerrainRequest {
        author: TerrainParams {
            seed: 42,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(matches!(request.quality, NaturalQualityProfile::Draft));
    let t = Instant::now();
    let artifact = AtlasTerrainGenerator::generate(&request, cancellation).unwrap();
    eprintln!("BASELINE generate_ms={}", t.elapsed().as_millis());
    let snapshot = artifact.snapshot();
    let fine = snapshot.fine_terrain();
    let radius2 = snapshot.surface().radius().get().powi(2);
    let n_atlas = snapshot.surface().cells().len();
    let marine: Vec<usize> = fine
        .coarse_terrain()
        .water_classification()
        .iter()
        .enumerate()
        .filter_map(|(i, &kind)| (kind == 1).then_some(i))
        .collect();
    let priors: Vec<(usize, f64)> = fine
        .lake_priors()
        .iter()
        .map(|p| (p.site, p.surface_elevation_m))
        .collect();
    eprintln!(
        "SOURCE atlas_cells={n_atlas} marine_sites={} lake_priors={} canonical_step_m={:.1}",
        marine.len(),
        priors.len(),
        fine.canonical_angular_step_radians() * snapshot.surface().radius().get()
    );

    // Baseline: rerun the production flood once on the published support so
    // node-level ocean/lake identities exist, then check the face rule copy.
    let support = &snapshot.support;
    let n_nodes = support.directions.len();
    let triangles_b: Vec<SupportTriangle> = support
        .faces
        .iter()
        .map(|f| (f.node_indices(), f.owner_index()))
        .collect();
    let t = Instant::now();
    let graph_b = SupportGraph::from_edges(
        n_nodes,
        triangles_b
            .iter()
            .flat_map(|(nodes, _)| (0..3).map(move |i| [nodes[i], nodes[(i + 1) % 3]])),
        cancellation,
    )
    .unwrap();
    let graph_ms = t.elapsed().as_millis();
    let mut seeds_b = vec![false; n_nodes];
    for &s in &marine {
        seeds_b[s] = true;
    }
    let t = Instant::now();
    let water_b =
        initialize_water(&support.bed, &graph_b, &seeds_b, &priors, cancellation).unwrap();
    let water_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let faces_b: Vec<FaceWater> = triangles_b
        .iter()
        .map(|(nodes, _)| face_water(*nodes, &support.directions, &support.bed, &water_b, radius2))
        .collect();
    let face_ms = t.elapsed().as_millis();
    let mismatched = faces_b
        .iter()
        .zip(&support.faces)
        .filter(|(a, b)| {
            a.ocean_fraction != b.ocean_fraction
                || a.lake_fraction != b.lake_fraction
                || a.level != b.water_level_m
                || a.lake != b.lake
                || a.area != b.area_m2
        })
        .count();
    eprintln!(
        "BASELINE nodes={n_nodes} faces={} single_pass graph_ms={graph_ms} water_ms={water_ms} face_ms={face_ms} \
         rerun_face_mismatches={mismatched}",
        triangles_b.len()
    );
    let baseline = Solved {
        directions: support.directions.clone(),
        triangles: triangles_b,
        bed: support.bed.clone(),
        graph: graph_b,
        water: water_b,
        faces: faces_b,
    };
    let sum_b = summarize(&baseline, n_atlas);
    let max_lake_rel = snapshot
        .lakes()
        .iter()
        .zip(&sum_b.lakes)
        .map(|(stored, rerun)| (stored.volume_m3() - rerun.volume).abs() / stored.volume_m3())
        .fold(0.0, f64::max);
    eprintln!(
        "BASELINE stored_lakes={} rerun_lakes={} max_lake_volume_rel_diff={max_lake_rel:.3e} \
         marine_anchor_valid={}",
        snapshot.lakes().len(),
        sum_b.lakes.len(),
        marine.iter().filter(|&&s| baseline.water.ocean[s]).count()
    );
    print_summary("base", &sum_b);
    for (l, f) in sum_b.lakes.iter().enumerate() {
        let (lat, lon) = f.latlon();
        eprintln!(
            "LAKE base id={l} area_km2={:.1} volume_km3={:.4} level_m={:.1} lat={lat:.2} lon={lon:.2}",
            f.area / KM2,
            f.volume / KM3,
            f.level
        );
    }
    (
        artifact,
        Baseline {
            solved: baseline,
            summary: sum_b,
            marine,
            priors,
        },
    )
}

#[test]
#[ignore = "one canonical Atlas publication plus Lk authority comparisons; Release only"]
fn authority_layer_probe() {
    let _ = env_logger::Builder::new()
        .filter_module(
            "sekai::generators::natural::formation::atlas_terrain",
            log::LevelFilter::Debug,
        )
        .is_test(true)
        .try_init();
    let sample_level: u8 = std::env::var("SEKAI_AUTHORITY_SAMPLE_LEVEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    assert!(sample_level >= LEVELS[LEVELS.len() - 1]);
    let threads: usize = std::env::var("SEKAI_AUTHORITY_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    let cancellation = BuildCancellation::new();
    let (artifact, base) = publish_baseline(&cancellation);
    let snapshot = artifact.snapshot();
    let fine = snapshot.fine_terrain();
    let radius2 = snapshot.surface().radius().get().powi(2);
    let n_atlas = snapshot.surface().cells().len();
    let Baseline {
        solved: baseline,
        summary: sum_b,
        marine,
        priors,
    } = base;

    let t = Instant::now();
    let goldberg = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
        target_cell_count: n_atlas as u32,
    })
    .unwrap();
    let carrier = Carrier::new("goldberg", &goldberg);
    let n_gold = goldberg.cells().len();
    let mut fan_offset = vec![0; n_gold + 1];
    for c in 0..n_gold {
        fan_offset[c + 1] = fan_offset[c] + carrier.sectors(c);
    }
    eprintln!(
        "GOLDBERG cells={n_gold} build_and_locator_ms={}",
        t.elapsed().as_millis()
    );
    let sites: Vec<UnitVector3> = snapshot.surface().cells().iter().map(|c| c.site).collect();
    // `site` (default): production A5b validity, the fine bed at the exact Atlas
    // site is below sea level; only the placement moves to the Lk face corners.
    // `corner`: any containing-face corner with B < 0 validates the anchor.
    let anchor_mode = std::env::var("SEKAI_AUTHORITY_ANCHOR").unwrap_or_else(|_| "site".into());
    let anchors: Vec<usize> = match anchor_mode.as_str() {
        "site" => marine
            .iter()
            .copied()
            .filter(|&s| baseline.bed[s] < 0.0)
            .collect(),
        "corner" => marine.clone(),
        other => panic!("unknown anchor mode {other}"),
    };
    eprintln!(
        "ANCHOR mode={anchor_mode} candidate_marine_anchors={}",
        anchors.len()
    );
    let placement = match std::env::var("SEKAI_AUTHORITY_PRIOR").as_deref() {
        Ok("descent") => PriorPlacement::Descent,
        Ok("face") | Err(_) => PriorPlacement::Face,
        Ok(other) => panic!("unknown prior placement {other}"),
    };
    let prior_valid: Vec<bool> = priors
        .iter()
        .map(|&(s, level)| baseline.bed[s] < level)
        .collect();
    eprintln!(
        "PRIOR placement={} priors={} valid_at_site={}",
        if placement == PriorPlacement::Face {
            "face"
        } else {
            "descent"
        },
        priors.len(),
        prior_valid.iter().filter(|&&v| v).count()
    );
    if placement == PriorPlacement::Descent {
        // The same rule on the production support, where each prior site is a
        // node: descend from the site itself; it must reproduce the production lakes.
        let seeds: Vec<(usize, f64)> = priors
            .iter()
            .zip(&prior_valid)
            .filter(|(_, &valid)| valid)
            .map(|(&(s, level), _)| {
                let pit = descend(s, &baseline.graph, &baseline.bed, &baseline.directions);
                (pit, level)
            })
            .collect();
        let mut seeds_b = vec![false; baseline.bed.len()];
        for &s in &marine {
            seeds_b[s] = true;
        }
        let water = initialize_water(
            &baseline.bed,
            &baseline.graph,
            &seeds_b,
            &seeds,
            &cancellation,
        )
        .unwrap();
        let n = baseline.bed.len();
        eprintln!(
            "PRIOR descent_on_baseline level_node_mismatch={} lake_id_node_mismatch={} \
             lowered_priors={} production_lowered_priors={}",
            (0..n)
                .filter(|&i| water.level[i] != baseline.water.level[i])
                .count(),
            (0..n)
                .filter(|&i| water.lake[i] != baseline.water.lake[i])
                .count(),
            water.lowered_priors,
            baseline.water.lowered_priors
        );
    }
    let ctx = Shared {
        goldberg: &goldberg,
        carrier: &carrier,
        fan_offset: &fan_offset,
        fine,
        sites: &sites,
        marine: &anchors,
        priors: &priors,
        prior_valid: &prior_valid,
        placement,
        radius2,
    };

    let mut candidates = Vec::new();
    for k in LEVELS {
        let cand = build_candidate(k, &ctx, &cancellation);
        let s = &cand.summary;
        eprintln!(
            "CANDIDATE L{k} marine_anchor_valid={} lowered_priors={}",
            cand.marine_corners
                .iter()
                .filter(|cs| cs.iter().any(|&i| cand.solved.water.ocean[i]))
                .count(),
            cand.solved.water.lowered_priors
        );
        print_summary(&format!("L{k}"), s);
        let basin_links: Vec<(Option<usize>, Vec<usize>)> = anchors
            .iter()
            .zip(&cand.marine_corners)
            .map(|(&site, corners)| {
                let b = sum_b.basin_of[site];
                let mut cs: Vec<usize> = corners
                    .iter()
                    .map(|&i| s.basin_of[i])
                    .filter(|&c| c != NONE)
                    .map(|c| c as usize)
                    .collect();
                cs.sort_unstable();
                cs.dedup();
                ((b != NONE).then_some(b as usize), cs)
            })
            .collect();
        report_groups("basin", k, &sum_b.basins, &s.basins, &basin_links);
        for (p, (&(site, level), nodes)) in priors.iter().zip(&cand.prior_nodes).enumerate() {
            let round = |v: f64| (v * 100.0).round() / 100.0;
            eprintln!(
                "PRIOR L{k} p={p} site={site} base_lake={:?} level_m={level:.2} site_bed_m={:.2} \
                 seeds={nodes:?} seed_bed_m={:?} seed_spill_m={:?} cand_lake={:?}",
                baseline.water.lake[site],
                baseline.bed[site],
                nodes
                    .iter()
                    .map(|&i| round(cand.solved.bed[i]))
                    .collect::<Vec<_>>(),
                nodes
                    .iter()
                    .map(|&i| round(cand.solved.water.spill[i].0))
                    .collect::<Vec<_>>(),
                nodes
                    .iter()
                    .map(|&i| cand.solved.water.lake[i])
                    .collect::<Vec<_>>(),
            );
        }
        let lake_links: Vec<(Option<usize>, Vec<usize>)> = priors
            .iter()
            .zip(&cand.prior_nodes)
            .map(|(&(site, _), nodes)| {
                let mut cs: Vec<usize> = nodes
                    .iter()
                    .filter_map(|&i| cand.solved.water.lake[i])
                    .collect();
                cs.sort_unstable();
                cs.dedup();
                (baseline.water.lake[site], cs)
            })
            .collect();
        report_groups("lake", k, &sum_b.lakes, &s.lakes, &lake_links);
        candidates.push(cand);
    }

    // Common equal-partition samples on Goldberg L0 cells: the baseline reads the
    // sealed product; candidates read the same fine bed under the Lk face level.
    let t = Instant::now();
    let cells: Vec<CellSample> = pool.install(|| {
        (0..n_gold)
            .into_par_iter()
            .map(|c| sample_cell(snapshot, &ctx, &candidates, c, sample_level))
            .collect()
    });
    let outside: usize = cells.iter().map(|c| c.outside).sum();
    eprintln!(
        "SAMPLES level={sample_level} count={} outside_ancestor={outside} ms={}",
        fan_offset[n_gold] * 4_usize.pow(u32::from(sample_level) - 1),
        t.elapsed().as_millis()
    );
    let total_area: f64 = cells.iter().map(|c| c.area).sum();
    let base_total: Vec<f64> = (0..4)
        .map(|i| cells.iter().map(|c| c.base[i]).sum())
        .collect();
    eprintln!(
        "SAMPLED base area_km2 {:?} ocean_km3={:.0} lake_km3={:.3}",
        base_total
            .iter()
            .map(|a| (a / KM2).round())
            .collect::<Vec<_>>(),
        cells.iter().map(|c| c.base_volume[0]).sum::<f64>() / KM3,
        cells.iter().map(|c| c.base_volume[1]).sum::<f64>() / KM3
    );
    for (ki, cand) in candidates.iter().enumerate() {
        let k = cand.k;
        let mut confusion = [[0.0_f64; 4]; 4];
        for c in &cells {
            for (b, row) in confusion.iter_mut().enumerate() {
                for (x, v) in row.iter_mut().enumerate() {
                    *v += c.confusion[ki][b][x];
                }
            }
        }
        eprintln!(
            "SAMPLED L{k} inherited area_km2 {:?} ocean_km3={:.0} lake_km3={:.3}",
            (0..4)
                .map(|i| (cells.iter().map(|c| c.cand[ki][i]).sum::<f64>() / KM2).round())
                .collect::<Vec<_>>(),
            cells.iter().map(|c| c.cand_volume[ki][0]).sum::<f64>() / KM3,
            cells.iter().map(|c| c.cand_volume[ki][1]).sum::<f64>() / KM3
        );
        for (b, row) in confusion.iter().enumerate() {
            eprintln!(
                "CONFUSION L{k} base={:>6} -> {}",
                CLASS[b],
                row.iter()
                    .zip(CLASS)
                    .map(|(v, name)| format!("{name}={:.1}km2({:.3e})", v / KM2, v / total_area))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        // Per-L0 fraction and equivalent-depth errors.
        let mut errs: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut ranked = Vec::new();
        for (c, cs) in cells.iter().enumerate() {
            let base = [cs.base[0], cs.base[1], cs.base[2] + cs.base[3]].map(|a| a / cs.area);
            let inh = [
                cs.cand[ki][0],
                cs.cand[ki][1],
                cs.cand[ki][2] + cs.cand[ki][3],
            ]
            .map(|a| a / cs.area);
            let p1 = cand.summary.cells[c];
            let publ = [p1[0], p1[1], p1[2]].map(|a| a / p1[3]);
            let mixed = base.iter().filter(|f| **f > 0.0).count() > 1
                || publ.iter().filter(|f| **f > 0.0).count() > 1
                || inh.iter().filter(|f| **f > 0.0).count() > 1;
            let depth_base = cs.base_volume.map(|v| v / cs.area);
            let depth_inh = cs.cand_volume[ki].map(|v| v / cs.area);
            let depth_pub = [p1[4] / p1[3], p1[5] / p1[3]];
            for (set, include) in [("all", true), ("mixed", mixed)] {
                if !include {
                    continue;
                }
                for (i, name) in ["ocean", "lake", "dry"].iter().enumerate() {
                    errs.entry(format!("L{k} {set} frac_{name} published_p1"))
                        .or_default()
                        .push(publ[i] - base[i]);
                    errs.entry(format!("L{k} {set} frac_{name} inherited"))
                        .or_default()
                        .push(inh[i] - base[i]);
                }
                for (i, name) in ["ocean", "lake"].iter().enumerate() {
                    errs.entry(format!("L{k} {set} depth_m_{name} published_p1"))
                        .or_default()
                        .push(depth_pub[i] - depth_base[i]);
                    errs.entry(format!("L{k} {set} depth_m_{name} inherited"))
                        .or_default()
                        .push(depth_inh[i] - depth_base[i]);
                }
            }
            let mut mismatch = 0.0;
            let mut worst = (0.0, 0, 0);
            for b in 0..4 {
                for x in 0..4 {
                    let v = cs.confusion[ki][b][x];
                    if b != x {
                        mismatch += v;
                        if v > worst.0 {
                            worst = (v, b, x);
                        }
                    }
                }
            }
            let pub_err = (publ[0] - base[0]).abs() + (publ[1] - base[1]).abs();
            ranked.push((mismatch, pub_err, c, worst, base));
        }
        for (label, values) in &errs {
            dist(label, values);
        }
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let cell_latlon = |c: usize| {
            let (lat, lon) = latitude_longitude_radians(goldberg.cells()[c].centroid);
            (lat.to_degrees(), lon.to_degrees())
        };
        for (mismatch, pub_err, c, (v, b, x), base) in ranked.iter().take(15) {
            let (lat, lon) = cell_latlon(*c);
            eprintln!(
                "TOPCELL L{k} inherited cell={c} lat={lat:.2} lon={lon:.2} mismatch_km2={:.1} \
                 main={}->{} {:.1}km2 base_frac=[{:.3},{:.3},{:.3}] pub_err={pub_err:.3}",
                mismatch / KM2,
                CLASS[*b],
                CLASS[*x],
                v / KM2,
                base[0],
                base[1],
                base[2]
            );
        }
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (mismatch, pub_err, c, _, base) in ranked.iter().take(10) {
            let (lat, lon) = cell_latlon(*c);
            eprintln!(
                "TOPCELL L{k} published cell={c} lat={lat:.2} lon={lon:.2} pub_err={pub_err:.3} \
                 inherited_mismatch_km2={:.1} base_frac=[{:.3},{:.3},{:.3}]",
                mismatch / KM2,
                base[0],
                base[1],
                base[2]
            );
        }
    }

    // Noise floor of the sampled reference: point-sampled fractions on every
    // eighth Atlas cell versus the production P1 cell fields of the same cell.
    let atlas_carrier = Carrier::new("atlas", snapshot.surface());
    let t = Instant::now();
    let floor: Vec<[f64; 3]> = pool.install(|| {
        (0..n_atlas)
            .into_par_iter()
            .step_by(8)
            .map(|c| {
                let leaves = cell_leaves(&atlas_carrier, c, sample_level);
                let dirs: Vec<[f64; 3]> = leaves
                    .iter()
                    .map(|(_, _, t)| center(t).components())
                    .collect();
                let samples = snapshot.sample_surface_batch(&dirs, &|| false).unwrap();
                let mut area = [0.0; 3];
                let mut total = 0.0;
                for (smp, (_, _, tri)) in samples.iter().zip(&leaves) {
                    let w = sph_area(tri);
                    total += w;
                    area[class_of(smp.water, smp.bed_elevation_m).min(2)] += w;
                }
                let f = snapshot.fields();
                [
                    area[0] / total - f64::from(f.ocean_fraction()[c]),
                    area[1] / total - f64::from(f.lake_fraction()[c]),
                    area[2] / total - f64::from(f.dry_fraction()[c]),
                ]
            })
            .collect()
    });
    eprintln!(
        "FLOOR atlas_cells={} ms={}",
        floor.len(),
        t.elapsed().as_millis()
    );
    for (i, name) in ["ocean", "lake", "dry"].iter().enumerate() {
        let values: Vec<f64> = floor.iter().map(|f| f[i]).collect();
        dist(&format!("FLOOR all frac_{name} sampled_vs_p1"), &values);
        let mixed: Vec<f64> = values.iter().copied().filter(|v| *v != 0.0).collect();
        if !mixed.is_empty() {
            dist(&format!("FLOOR nonzero frac_{name} sampled_vs_p1"), &mixed);
        }
    }
}

pub(super) fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Rerun the production flood on the published support under alternative
/// marine-anchor rules, and export the baseline's dry B < 0 points so
/// `civ-atlas-terrain`'s `upstream_class_probe` can read the upstream evaluator's
/// own drawn class there. A: the exact upstream marine site (production).
/// B: the lowest support node of each upstream marine source cell.
/// C: every support node of that cell (the flood keeps only B < 0 seeds).
#[test]
#[ignore = "one canonical Atlas publication plus anchor-rule reruns; Release only"]
fn anchor_rule_probe() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(env_usize("SEKAI_AUTHORITY_THREADS", 8))
        .build()
        .unwrap();
    let cancellation = BuildCancellation::new();
    let (artifact, base) = publish_baseline(&cancellation);
    let snapshot = artifact.snapshot();
    let support = &snapshot.support;
    let radius2 = snapshot.surface().radius().get().powi(2);
    let n_atlas = snapshot.surface().cells().len();
    let Baseline {
        mut solved,
        summary,
        marine,
        priors,
    } = base;
    let n = solved.bed.len();
    let deps = &summary.depressions;
    let mut touched = vec![0_u32; deps.len()];
    let mut touched_invalid = vec![0_u32; deps.len()];
    let mut seeds_b = vec![false; n];
    let mut seeds_c = vec![false; n];
    let mut invalid = 0;
    let offsets = &support.cell_face_offsets;
    for &s in &marine {
        let mut nodes: Vec<usize> = support.faces[offsets[s]..offsets[s + 1]]
            .iter()
            .flat_map(|f| f.node_indices())
            .collect();
        nodes.sort_unstable();
        nodes.dedup();
        let site_valid = solved.bed[s] < 0.0;
        invalid += usize::from(!site_valid);
        let mut ids: Vec<u32> = nodes
            .iter()
            .map(|&i| summary.dep_of[i])
            .filter(|&d| d != NONE)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        for d in ids {
            touched[d as usize] += 1;
            touched_invalid[d as usize] += u32::from(!site_valid);
        }
        let lowest = nodes
            .iter()
            .copied()
            .min_by(|a, b| solved.bed[*a].total_cmp(&solved.bed[*b]))
            .expect("a source cell owns support nodes");
        seeds_b[lowest] = true;
        for i in nodes {
            seeds_c[i] = true;
        }
    }
    eprintln!(
        "ANCHORS marine_sites={} invalid_at_site={invalid} baseline_depressions={}",
        marine.len(),
        deps.len()
    );
    let mut became_ocean = Vec::new();
    for (name, seeds) in [("B", &seeds_b), ("C", &seeds_c)] {
        let t = Instant::now();
        let water =
            initialize_water(&solved.bed, &solved.graph, seeds, &priors, &cancellation).unwrap();
        let faces: Vec<FaceWater> = solved
            .triangles
            .iter()
            .map(|(nodes, _)| face_water(*nodes, &solved.directions, &solved.bed, &water, radius2))
            .collect();
        // Swap the rule's water into the shared mesh instead of copying the graph.
        let base_water = std::mem::replace(&mut solved.water, water);
        let base_faces = std::mem::replace(&mut solved.faces, faces);
        let s = summarize(&solved, n_atlas);
        eprintln!(
            "RULE {name} ms={} lowered_priors={}",
            t.elapsed().as_millis(),
            solved.water.lowered_priors
        );
        print_summary(&format!("rule{name}"), &s);
        let mut flag = vec![false; deps.len()];
        for (i, &d) in summary.dep_of.iter().enumerate() {
            if d != NONE && solved.water.ocean[i] {
                flag[d as usize] = true;
            }
        }
        became_ocean.push(flag);
        solved.water = base_water;
        solved.faces = base_faces;
    }
    // Groups: touched by an upstream marine cell whose exact site is invalid;
    // touched only by valid marine cells; outside every upstream marine cell.
    let mut groups = [(0_usize, 0.0_f64, [0.0_f64; 2]); 3];
    for (d, dep) in deps.iter().enumerate() {
        let g = if touched_invalid[d] > 0 {
            0
        } else if touched[d] > 0 {
            1
        } else {
            2
        };
        let area = dep.area + dep.extra;
        groups[g].0 += 1;
        groups[g].1 += area;
        for (r, flags) in became_ocean.iter().enumerate() {
            if flags[d] {
                groups[g].2[r] += area;
            }
        }
    }
    for (name, (count, area, ocean)) in [
        "invalid_marine_cell",
        "valid_marine_cell_only",
        "no_marine_cell",
    ]
    .iter()
    .zip(groups)
    {
        eprintln!(
            "DEPGROUP {name} count={count} area_km2={:.0} becomes_ocean_km2 B={:.0} C={:.0}",
            area / KM2,
            ocean[0] / KM2,
            ocean[1] / KM2
        );
    }

    let Ok(path) = std::env::var("SEKAI_ANCHOR_EXPORT") else {
        return;
    };
    let probes = env_usize("SEKAI_ANCHOR_EXPORT_POINTS", 1 << 24);
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    let dirs: Vec<[f64; 3]> = (0..probes)
        .map(|i| {
            let z = 1.0 - (2.0 * i as f64 + 1.0) / probes as f64;
            let r = (1.0 - z * z).sqrt();
            let a = golden * i as f64;
            [r * a.cos(), r * a.sin(), z]
        })
        .collect();
    let t = Instant::now();
    let points: Vec<([f64; 3], i64)> = pool.install(|| {
        dirs.par_chunks(4096)
            .flat_map_iter(|chunk| {
                let samples = snapshot.sample_surface_batch(chunk, &|| false).unwrap();
                chunk
                    .iter()
                    .zip(samples)
                    .filter(|(_, s)| class_of(s.water, s.bed_elevation_m) == 2)
                    .map(|(d, s)| {
                        let face = crate::world::natural::atlas_terrain::support_face(
                            support,
                            s.source_cell,
                            *d,
                        );
                        let dep = face
                            .node_indices()
                            .iter()
                            .map(|&i| summary.dep_of[i])
                            .find(|&d| d != NONE)
                            .map_or(-1, i64::from);
                        (*d, dep)
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    });
    let weight_km2 = 4.0 * std::f64::consts::PI * radius2 / probes as f64 / KM2;
    let mut out = format!("W {weight_km2}\n");
    for (d, f) in deps.iter().enumerate() {
        let (lat, lon) = f.latlon();
        out += &format!(
            "D {d} {:.3} {:.3} {lat:.3} {lon:.3} {} {} {} {}\n",
            f.area / KM2,
            f.extra / KM2,
            touched[d],
            touched_invalid[d],
            u8::from(became_ocean[0][d]),
            u8::from(became_ocean[1][d])
        );
    }
    for ([x, y, z], dep) in &points {
        out += &format!("P {x:e} {y:e} {z:e} {dep}\n");
    }
    std::fs::write(&path, out).unwrap();
    eprintln!(
        "EXPORT points={} dry_neg_km2={:.0} ms={} path={path}",
        points.len(),
        points.len() as f64 * weight_km2,
        t.elapsed().as_millis()
    );
}
