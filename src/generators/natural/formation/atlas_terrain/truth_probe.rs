//! Offline truth-layer probe (`2026-11-03-world-truth-scope-design.md` §5 item 1):
//! Atlas water topology solved once on the source cell graph T, sub-cell
//! fractions from canonical fine-bed quadrature under the revised §3.1 rule
//! (T identity and level, local fine connectivity in mixed cells), versus the
//! published canonical support product.
//!
//! Reuses the production flood (`SupportGraph`, `initialize_water`), the
//! canonical fine evaluator and the authority probe's baseline publication.
//! Ignored, Release only; prints numbers and sets no gates.

use super::authority_probe::{
    components, dist, env_usize, publish_baseline, report_groups, Baseline, Feature, KM2, KM3, NONE,
};
use super::carrier_probe::{center, sph_area};
use super::geometry::{
    initialize_water, refine_triangles, source_fans, InitialWater, SupportGraph,
};
use super::*;
use crate::generators::natural::subdivision_children;
use crate::world::natural::SurfaceWaterKind;
use crate::world::spatial::{cross, dot};
use rayon::prelude::*;
use std::time::Instant;

/// Sample classes: ocean, lake, dry with bed < 0, dry with bed >= 0.
const CLASS: [&str; 4] = ["ocean", "lake", "dry<0", "dry>=0"];

/// Topology of T: the production flood on the Atlas site graph.
struct Truth {
    graph: SupportGraph,
    water: InitialWater,
    basin_of: Vec<u32>,
    basin_count: usize,
    lake_count: usize,
    lake_level: Vec<f64>,
}

/// One quadrature point: fan leaf centre, leaf area and the Voronoi vertex whose
/// dual Delaunay triangle contains the point (§3.1 corner set).
struct Point {
    direction: [f64; 3],
    weight: f64,
    vertex: usize,
}

/// Equal-partition leaves of every source fan of `cell` at `level` (>= 1).
/// The fan side facing neighbour `j` is split by the great circle through the
/// two sites: the Delaunay edge (cell, j), so each half lies in the triangle
/// dual to the Voronoi vertex on that side.
fn cell_points(
    surface: &SphericalSurfaceSnapshot,
    cell: usize,
    level: u8,
    radius2: f64,
) -> Vec<Point> {
    let cl = &surface.cells()[cell];
    let n = cl.boundary_vertices.len();
    let vertex_of = |k: usize| cl.boundary_vertices[k % n].raw() as usize;
    let position = |v: usize| surface.vertices()[v].position;
    let mut out = Vec::new();
    for side in 0..n {
        let edge = &surface.edges()[cl.boundary_edges[side].raw() as usize];
        let neighbour = edge
            .cells
            .iter()
            .map(|c| c.raw() as usize)
            .find(|&c| c != cell)
            .expect("an edge has two distinct cells");
        let normal = cross(
            cl.site.components(),
            surface.cells()[neighbour].site.components(),
        );
        let (v0, v1) = (vertex_of(side), vertex_of(side + 1));
        let v0_side = dot(normal, position(v0).components()) >= 0.0;
        let mut leaves = vec![[cl.site, position(v0), position(v1)]];
        for _ in 1..level {
            leaves = leaves
                .iter()
                .flat_map(|t| subdivision_children(*t))
                .collect();
        }
        out.extend(leaves.iter().map(|leaf| {
            let q = center(leaf);
            Point {
                direction: q.components(),
                weight: sph_area(leaf) * radius2,
                vertex: if (dot(normal, q.components()) >= 0.0) == v0_side {
                    v0
                } else {
                    v1
                },
            }
        }));
    }
    out
}

/// §3.1: wet in W iff a corner site belongs to W and the bed is below its
/// level; ocean (level 0) takes precedence over a lake on the same corner set.
/// Returns (class, feature) with feature = (0 basin | 1 lake, id, level).
fn classify(bed: f64, corners: &[u32], truth: &Truth) -> (usize, Option<(usize, usize, f64)>) {
    let w = &truth.water;
    if let Some(&s) = corners.iter().find(|&&s| w.ocean[s as usize]) {
        if bed < 0.0 {
            return (0, Some((0, truth.basin_of[s as usize] as usize, 0.0)));
        }
    }
    let lake = corners
        .iter()
        .filter_map(|&s| Some((w.lake[s as usize]?, w.level[s as usize]?)))
        .max_by(|a, b| a.1.total_cmp(&b.1));
    match lake {
        Some((l, level)) if bed < level => (1, Some((1, l, level))),
        _ => (2 + usize::from(bed >= 0.0), None),
    }
}

/// Water body of one T site: (0 basin | 1 lake, id, level); `None` is dry land.
type Body = Option<(usize, usize, f64)>;

fn body_of(truth: &Truth, s: usize) -> Body {
    if truth.water.ocean[s] {
        return Some((0, truth.basin_of[s] as usize, 0.0));
    }
    truth.water.lake[s].map(|l| (1, l, truth.lake_level[l]))
}

fn key(body: Body) -> Option<(usize, usize)> {
    body.map(|(kind, id, _)| (kind, id))
}

/// Revised §3.1 inside mixed cells (a cell whose T neighbourhood holds more than
/// one body): T still fixes each body's identity and level, but a fine point is
/// wet in W only if it is connected below W's level, through the refined fan
/// mesh of the mixed cells and within W's corner-eligible region, to a T site of
/// W or to a cell lying wholly inside W. Uniform cells keep the point rule.
struct LocalWater {
    active: Vec<bool>,
    leaf_offset: Vec<usize>,
    leaf_body: Vec<Body>,
}

fn local_water(
    snapshot: &AtlasTerrainSnapshot,
    truth: &Truth,
    corners: &[Vec<u32>],
    level: u8,
    pool: &rayon::ThreadPool,
    cancellation: &BuildCancellation,
) -> LocalWater {
    let surface = snapshot.surface();
    let n = surface.cells().len();
    let t = Instant::now();
    let site_body: Vec<Body> = (0..n).map(|s| body_of(truth, s)).collect();
    let active: Vec<bool> = (0..n)
        .map(|c| {
            truth
                .graph
                .neighbors(c)
                .iter()
                .any(|&j| key(site_body[j as usize]) != key(site_body[c]))
        })
        .collect();
    let (mut directions, fans) = source_fans(surface);
    let mut leaves: Vec<_> = fans
        .into_iter()
        .filter(|(_, owner)| active[*owner])
        .collect();
    for _ in 1..level {
        leaves = refine_triangles(&mut directions, leaves, cancellation).unwrap();
    }
    let mut leaf_offset = vec![0; n + 1];
    for (_, owner) in &leaves {
        leaf_offset[owner + 1] += 1;
    }
    for c in 0..n {
        leaf_offset[c + 1] += leaf_offset[c];
    }
    let mesh_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let mut used: Vec<usize> = leaves.iter().flat_map(|(nodes, _)| *nodes).collect();
    used.sort_unstable();
    used.dedup();
    let fine = snapshot.fine_terrain();
    let beds: Vec<Vec<f64>> = pool.install(|| {
        used.par_chunks(4096)
            .map(|chunk| {
                let query: Vec<[f64; 3]> =
                    chunk.iter().map(|&i| directions[i].components()).collect();
                fine.sample_surface_batch(&query, &|| false)
                    .unwrap()
                    .into_iter()
                    .map(|s| s.bed_elevation_m)
                    .collect()
            })
            .collect()
    });
    let mut bed = vec![f64::NAN; directions.len()];
    for (&i, b) in used.iter().zip(beds.into_iter().flatten()) {
        bed[i] = b;
    }
    let bed_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    let graph = SupportGraph::from_edges(
        directions.len(),
        leaves
            .iter()
            .flat_map(|(nodes, _)| (0..3).map(move |i| [nodes[i], nodes[(i + 1) % 3]])),
        cancellation,
    )
    .unwrap();
    let graph_ms = t.elapsed().as_secs_f64() * 1e3;

    let t = Instant::now();
    // Corner-eligible bodies of each leaf (same Delaunay split as `cell_points`)
    // and, through incidence, of each mesh node.
    let leaf_vertex: Vec<usize> = (0..n)
        .flat_map(|c| {
            let range = leaf_offset[c]..leaf_offset[c + 1];
            let points = if range.is_empty() {
                Vec::new()
            } else {
                cell_points(surface, c, level, 1.0)
            };
            assert_eq!(points.len(), range.len(), "leaf order matches cell_points");
            points.into_iter().map(|p| p.vertex)
        })
        .collect();
    let eligible_of = |v: usize| -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = corners[v]
            .iter()
            .filter_map(|&s| key(site_body[s as usize]))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    };
    let leaf_eligible: Vec<Vec<(usize, usize)>> =
        leaf_vertex.iter().map(|&v| eligible_of(v)).collect();
    let mut node_eligible: Vec<Vec<(usize, usize)>> = vec![Vec::new(); directions.len()];
    for ((nodes, _), el) in leaves.iter().zip(&leaf_eligible) {
        for &i in nodes {
            for &b in el {
                if !node_eligible[i].contains(&b) {
                    node_eligible[i].push(b);
                }
            }
        }
    }
    // Seeds: T sites of W inside mixed cells, and Voronoi-vertex nodes touching a
    // uniform cell of W (that cell is wet wherever B < level by the point rule).
    let mut seeds: Vec<(usize, (usize, usize, f64))> = Vec::new();
    for &i in &used {
        if i < n {
            if let Some(b) = site_body[i] {
                seeds.push((i, b));
            }
        } else if let Some(b) = corners
            .get(i - n)
            .into_iter()
            .flatten()
            .filter(|&&s| !active[s as usize])
            .find_map(|&s| site_body[s as usize])
        {
            seeds.push((i, b));
        }
    }
    // Oceans first (one shared level), then lakes from the highest level down.
    seeds.sort_by(|a, b| {
        a.1 .0
            .cmp(&b.1 .0)
            .then(b.1 .2.total_cmp(&a.1 .2))
            .then(a.1 .1.cmp(&b.1 .1))
    });
    let mut flooded: Vec<Body> = vec![None; directions.len()];
    let mut queue = std::collections::VecDeque::new();
    let mut start = 0;
    while start < seeds.len() {
        let group = seeds[start].1;
        let end = start
            + seeds[start..]
                .iter()
                .take_while(|(_, b)| b.0 == group.0 && (group.0 == 0 || b.1 == group.1))
                .count();
        for &(i, b) in &seeds[start..end] {
            if flooded[i].is_none() && bed[i] < b.2 {
                flooded[i] = Some(b);
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            let b = flooded[i].expect("queued nodes are flooded");
            for &j in graph.neighbors(i) {
                let j = j as usize;
                if flooded[j].is_none() && bed[j] < b.2 && node_eligible[j].contains(&(b.0, b.1)) {
                    flooded[j] = Some(b);
                    queue.push_back(j);
                }
            }
        }
        start = end;
    }
    let leaf_body: Vec<Body> = leaves
        .iter()
        .zip(&leaf_eligible)
        .map(|((nodes, _), el)| {
            nodes
                .iter()
                .filter_map(|&i| flooded[i])
                .filter(|b| el.contains(&(b.0, b.1)))
                .min_by(|a, b| a.0.cmp(&b.0).then(b.2.total_cmp(&a.2)))
        })
        .collect();
    let flood_ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!(
        "LOCAL m={level} active_cells={} mesh_nodes={} leaves={} seeds={} mesh_ms={mesh_ms:.0} \
         bed_ms={bed_ms:.0} graph_ms={graph_ms:.0} flood_ms={flood_ms:.0} total_ms={:.0}",
        active.iter().filter(|&&a| a).count(),
        used.len(),
        leaves.len(),
        seeds.len(),
        mesh_ms + bed_ms + graph_ms + flood_ms
    );
    LocalWater {
        active,
        leaf_offset,
        leaf_body,
    }
}

#[derive(Default)]
struct CellResult {
    area: f64,
    cand: [f64; 4],
    cand_volume: [f64; 2],
    base: [f64; 4],
    base_volume: [f64; 2],
    confusion: [[f64; 4]; 4],
    /// Area of cells classified by local fine connectivity.
    active_area: f64,
    /// (kind, id) -> (area, volume, weighted centre).
    features: Vec<((usize, usize), f64, f64, [f64; 3], f64)>,
    points: usize,
}

fn base_class(water: SurfaceWaterKind, bed: f64) -> usize {
    match water {
        SurfaceWaterKind::Ocean => 0,
        SurfaceWaterKind::Lake => 1,
        SurfaceWaterKind::DryLand => 2 + usize::from(bed >= 0.0),
    }
}

fn evaluate_cell(
    snapshot: &AtlasTerrainSnapshot,
    truth: &Truth,
    corners: &[Vec<u32>],
    local: &LocalWater,
    cell: usize,
    level: u8,
    radius2: f64,
) -> CellResult {
    let points = cell_points(snapshot.surface(), cell, level, radius2);
    let dirs: Vec<[f64; 3]> = points.iter().map(|p| p.direction).collect();
    let samples = snapshot.sample_surface_batch(&dirs, &|| false).unwrap();
    let mut out = CellResult {
        points: points.len(),
        ..Default::default()
    };
    let active = local.active[cell];
    for (index, (p, s)) in points.iter().zip(&samples).enumerate() {
        let bed = s.bed_elevation_m;
        let w = p.weight;
        let (cc, feature) = if active {
            match local.leaf_body[local.leaf_offset[cell] + index] {
                Some((kind, id, lv)) if bed < lv => (kind, Some((kind, id, lv))),
                _ => (2 + usize::from(bed >= 0.0), None),
            }
        } else {
            classify(bed, &corners[p.vertex], truth)
        };
        let bc = base_class(s.water, bed);
        out.area += w;
        out.cand[cc] += w;
        out.base[bc] += w;
        out.confusion[bc][cc] += w;
        if let Some(depth) = s.water_depth_m() {
            out.base_volume[bc] += w * depth;
        }
        if active {
            out.active_area += w;
        }
        if let Some((kind, id, level_m)) = feature {
            let depth = level_m - bed;
            out.cand_volume[kind] += w * depth;
            let entry = match out.features.iter_mut().find(|f| f.0 == (kind, id)) {
                Some(entry) => entry,
                None => {
                    out.features.push(((kind, id), 0.0, 0.0, [0.0; 3], level_m));
                    out.features.last_mut().expect("just pushed")
                }
            };
            entry.1 += w;
            entry.2 += w * depth;
            for i in 0..3 {
                entry.3[i] += p.direction[i] * w;
            }
        }
    }
    out
}

fn solve_truth(
    snapshot: &AtlasTerrainSnapshot,
    base: &Baseline,
    cancellation: &BuildCancellation,
) -> Truth {
    let surface = snapshot.surface();
    let n = surface.cells().len();
    let t = Instant::now();
    let sites: Vec<[f64; 3]> = surface
        .cells()
        .iter()
        .map(|c| c.site.components())
        .collect();
    let bed: Vec<f64> = snapshot
        .fine_terrain()
        .sample_surface_batch(&sites, &|| false)
        .unwrap()
        .into_iter()
        .map(|s| s.bed_elevation_m)
        .collect();
    let bed_ms = t.elapsed().as_secs_f64() * 1e3;
    // Production support node i is Atlas site i with the same canonical bed.
    let bed_mismatch = (0..n).filter(|&i| bed[i] != base.solved.bed[i]).count();
    let t = Instant::now();
    let graph = SupportGraph::from_edges(
        n,
        surface
            .edges()
            .iter()
            .map(|e| e.cells.map(|c| c.raw() as usize)),
        cancellation,
    )
    .unwrap();
    let graph_ms = t.elapsed().as_secs_f64() * 1e3;
    let mut seeds = vec![false; n];
    for &s in &base.marine {
        seeds[s] = true;
    }
    let t = Instant::now();
    let water = initialize_water(&bed, &graph, &seeds, &base.priors, cancellation).unwrap();
    let water_ms = t.elapsed().as_secs_f64() * 1e3;
    let (basin_of, basin_count) = components(&graph, n, |i| water.ocean[i]);
    let lake_count = water.lake.iter().flatten().max().map_or(0, |l| l + 1);
    let bw = &base.solved.water;
    let ocean_flip = (0..n).filter(|&i| water.ocean[i] != bw.ocean[i]).count();
    let lake_flip = (0..n)
        .filter(|&i| water.lake[i].is_some() != bw.lake[i].is_some())
        .count();
    eprintln!(
        "TRUTH sites={n} bed_ms={bed_ms:.1} graph_ms={graph_ms:.1} water_ms={water_ms:.1} \
         bed_mismatch_vs_support={bed_mismatch} valid_marine={} ocean_sites={} lake_sites={} \
         basins={basin_count} lakes={lake_count} lowered_priors={} \
         site_ocean_flip_vs_base={ocean_flip} site_lake_flip_vs_base={lake_flip}",
        base.marine.iter().filter(|&&s| water.ocean[s]).count(),
        water.ocean.iter().filter(|&&o| o).count(),
        water.lake.iter().flatten().count(),
        water.lowered_priors,
    );
    let mut lake_level = vec![f64::NAN; lake_count];
    for (l, level) in water.lake.iter().zip(&water.level) {
        if let (Some(l), Some(level)) = (l, level) {
            lake_level[*l] = *level;
        }
    }
    Truth {
        graph,
        water,
        basin_of,
        basin_count,
        lake_count,
        lake_level,
    }
}

fn features(results: &[CellResult], kind: usize, count: usize) -> Vec<Feature> {
    let mut out = vec![Feature::default(); count];
    for r in results {
        for &((k, id), area, volume, centre, level) in &r.features {
            if k == kind {
                let f = &mut out[id];
                f.area += area;
                f.volume += volume;
                f.level = level;
                f.add_centroid(centre, 1.0);
            }
        }
    }
    out
}

fn report_level(snapshot: &AtlasTerrainSnapshot, level: u8, results: &[CellResult], eval_ms: f64) {
    let fields = snapshot.fields();
    let cells = snapshot.surface().cells();
    let sum = |f: &dyn Fn(&CellResult) -> f64| results.iter().map(f).sum::<f64>();
    let pub_ocean_area: f64 = cells
        .iter()
        .zip(fields.ocean_fraction())
        .map(|(c, &f)| c.area.get() * f64::from(f))
        .sum();
    let pub_lake_area: f64 = cells
        .iter()
        .zip(fields.lake_fraction())
        .map(|(c, &f)| c.area.get() * f64::from(f))
        .sum();
    let pub_ocean_volume: f64 = fields.ocean_volume_m3().iter().sum();
    let pub_lake_volume: f64 = fields.lake_volume_m3().iter().sum();
    let points: usize = results.iter().map(|r| r.points).sum();
    eprintln!(
        "LEVEL m={level} points={points} eval_ms={eval_ms:.0} (fine bed + baseline bind, parallel)"
    );
    let rel = |a: f64, b: f64| (a - b) / b * 100.0;
    for (name, cand, sampled, published) in [
        (
            "ocean_km2",
            sum(&|r| r.cand[0]),
            sum(&|r| r.base[0]),
            pub_ocean_area,
        ),
        (
            "lake_km2",
            sum(&|r| r.cand[1]),
            sum(&|r| r.base[1]),
            pub_lake_area,
        ),
    ] {
        eprintln!(
            "TOTAL m={level} {name} published={:.1} base_sampled={:.1} ({:+.4}%) truth={:.1} ({:+.4}% vs published)",
            published / KM2,
            sampled / KM2,
            rel(sampled, published),
            cand / KM2,
            rel(cand, published)
        );
    }
    for (name, cand, sampled, published) in [
        (
            "ocean_km3",
            sum(&|r| r.cand_volume[0]),
            sum(&|r| r.base_volume[0]),
            pub_ocean_volume,
        ),
        (
            "lake_km3",
            sum(&|r| r.cand_volume[1]),
            sum(&|r| r.base_volume[1]),
            pub_lake_volume,
        ),
    ] {
        eprintln!(
            "TOTAL m={level} {name} published={:.3} base_sampled={:.3} ({:+.4}%) truth={:.3} ({:+.4}% vs published)",
            published / KM3,
            sampled / KM3,
            rel(sampled, published),
            cand / KM3,
            rel(cand, published)
        );
    }
    eprintln!(
        "ACTIVE_AREA m={level} locally_connected_cells_km2={:.1}",
        sum(&|r| r.active_area) / KM2
    );
    for (b, row) in CLASS.iter().enumerate() {
        let line = (0..4)
            .map(|c| format!("{}={:.1}", CLASS[c], sum(&|r| r.confusion[b][c]) / KM2))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("CONFUSION m={level} base={row} -> truth: {line}");
    }
    let mixed = |i: usize, r: &CellResult| {
        let of = f64::from(fields.ocean_fraction()[i]);
        let lf = f64::from(fields.lake_fraction()[i]);
        let co = r.cand[0] / r.area;
        let cl = r.cand[1] / r.area;
        (of > 0.0 && of < 1.0) || lf > 0.0 || (co > 0.0 && co < 1.0) || cl > 0.0
    };
    for (label, only_mixed) in [("all", false), ("mixed", true)] {
        let pick: Vec<usize> = (0..results.len())
            .filter(|&i| !only_mixed || mixed(i, &results[i]))
            .collect();
        let series = |f: &dyn Fn(usize, &CellResult) -> f64| -> Vec<f64> {
            pick.iter().map(|&i| f(i, &results[i])).collect()
        };
        let area = |i: usize| cells[i].area.get();
        dist(
            &format!("m={level} {label} ocean_frac truth-published"),
            &series(&|i, r| r.cand[0] / r.area - f64::from(fields.ocean_fraction()[i])),
        );
        dist(
            &format!("m={level} {label} ocean_frac base_sampled-published (noise floor)"),
            &series(&|i, r| r.base[0] / r.area - f64::from(fields.ocean_fraction()[i])),
        );
        dist(
            &format!("m={level} {label} lake_frac truth-published"),
            &series(&|i, r| r.cand[1] / r.area - f64::from(fields.lake_fraction()[i])),
        );
        dist(
            &format!("m={level} {label} ocean_depth_m truth-published"),
            &series(&|i, r| (r.cand_volume[0] - fields.ocean_volume_m3()[i]) / area(i)),
        );
        dist(
            &format!("m={level} {label} ocean_depth_m base_sampled-published (noise floor)"),
            &series(&|i, r| (r.base_volume[0] - fields.ocean_volume_m3()[i]) / area(i)),
        );
        dist(
            &format!("m={level} {label} lake_depth_m truth-published"),
            &series(&|i, r| (r.cand_volume[1] - fields.lake_volume_m3()[i]) / area(i)),
        );
    }
}

#[test]
#[ignore = "one canonical Atlas publication plus truth-layer quadrature; Release only"]
fn truth_layer_probe() {
    let max_level = env_usize("SEKAI_TRUTH_MAX_LEVEL", 4) as u8;
    let threads = env_usize("SEKAI_TRUTH_THREADS", 8);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();
    let cancellation = BuildCancellation::new();
    let (artifact, base) = publish_baseline(&cancellation);
    let snapshot = artifact.snapshot();
    let surface = snapshot.surface();
    let n = surface.cells().len();
    let radius2 = surface.radius().get().powi(2);
    let truth = solve_truth(snapshot, &base, &cancellation);

    // Delaunay corner set of every Voronoi vertex: the cells that reference it.
    let mut corners: Vec<Vec<u32>> = vec![Vec::new(); surface.vertices().len()];
    for cell in surface.cells() {
        for v in &cell.boundary_vertices {
            corners[v.raw() as usize].push(cell.id.raw());
        }
    }
    let degenerate = corners.iter().filter(|c| c.len() != 3).count();
    let side_mismatch = surface
        .cells()
        .iter()
        .flat_map(|cl| {
            let k = cl.boundary_vertices.len();
            (0..k).map(move |s| (cl, s, k))
        })
        .filter(|&(cl, s, k)| {
            let mut e = surface.edges()[cl.boundary_edges[s].raw() as usize].vertices;
            let mut v = [cl.boundary_vertices[s], cl.boundary_vertices[(s + 1) % k]];
            e.sort();
            v.sort();
            e != v
        })
        .count();
    eprintln!(
        "CARRIER vertices={} non_triangle_corner_sets={degenerate} side_edge_mismatch={side_mismatch} \
         threads={threads}",
        corners.len()
    );
    assert_eq!(
        side_mismatch, 0,
        "boundary edge k must join vertices k and k+1"
    );

    let mut finest: Vec<CellResult> = Vec::new();
    for level in 1..=max_level {
        // Bed-only cost, the part a production truth layer pays per level.
        let t = Instant::now();
        let bed_points: usize = pool.install(|| {
            (0..n)
                .into_par_iter()
                .map(|c| {
                    let dirs: Vec<[f64; 3]> = cell_points(surface, c, level, radius2)
                        .iter()
                        .map(|p| p.direction)
                        .collect();
                    snapshot
                        .fine_terrain()
                        .sample_surface_batch(&dirs, &|| false)
                        .unwrap()
                        .len()
                })
                .sum()
        });
        eprintln!(
            "BED_ONLY m={level} points={bed_points} ms={:.0}",
            t.elapsed().as_secs_f64() * 1e3
        );
        let local = local_water(snapshot, &truth, &corners, level, &pool, &cancellation);
        let t = Instant::now();
        let results: Vec<CellResult> = pool.install(|| {
            (0..n)
                .into_par_iter()
                .map(|c| evaluate_cell(snapshot, &truth, &corners, &local, c, level, radius2))
                .collect()
        });
        report_level(snapshot, level, &results, t.elapsed().as_secs_f64() * 1e3);
        if level == max_level {
            finest = results;
        }
    }

    // Feature identity at the finest level, paired through shared source anchors.
    let basins = features(&finest, 0, truth.basin_count);
    let lakes = features(&finest, 1, truth.lake_count);
    let sum_b = &base.summary;
    let basin_links: Vec<(Option<usize>, Vec<usize>)> = base
        .marine
        .iter()
        .map(|&s| {
            let b = sum_b.basin_of[s];
            let c = truth.basin_of[s];
            (
                (b != NONE).then_some(b as usize),
                if c == NONE { vec![] } else { vec![c as usize] },
            )
        })
        .collect();
    report_groups("basin", 0, &sum_b.basins, &basins, &basin_links);
    let lake_links: Vec<(Option<usize>, Vec<usize>)> = base
        .priors
        .iter()
        .map(|&(s, _)| {
            (
                base.solved.water.lake[s],
                truth.water.lake[s].into_iter().collect(),
            )
        })
        .collect();
    report_groups("lake", 0, &sum_b.lakes, &lakes, &lake_links);
}
