//! Throwaway lake geometry experiment. No production classifier consumes this module.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Instant;

use super::integrate_positive_linear_triangle;
use crate::app::{build_spherical_formation_external_artifacts, default_spherical_space_spec};
use crate::engine::{BuildCancellation, BuildEngine, MemoryStageCache};
use crate::generators::natural::{
    causal_natural_formation_graph, subdivision_children, NaturalFormationBundleArtifact,
};
use crate::generators::spatial::ProfileSurfaceBuilder;
use crate::world::natural::{
    GeologicSpec, Lake, NaturalQualityProfile, ReliefSpec, SurfaceWaterKind, TectonicSpec,
    WorldFormationSpec,
};
use crate::world::spatial::{
    canonical_east_north_basis, central_angle, spherical_triangle_area_unit,
    SphericalSurfaceSnapshot, UnitVector3,
};
use crate::world::RootSeed;

// Resource budget, not a scientific tolerance: the largest previous R5 local
// fixture enumerated this many leaves. A global failed basin must not turn
// this three-lake experiment into an unrequested full-world refinement.
const L6_LEAF_BUDGET: usize = 196_608;

#[path = "lake_b1_probe.rs"]
mod b1_probe;
#[path = "lake_native_refine_probe.rs"]
mod native_refine_probe;

fn fixture(
    profile: NaturalQualityProfile,
) -> (
    SphericalSurfaceSnapshot,
    Arc<NaturalFormationBundleArtifact>,
) {
    let surface = ProfileSurfaceBuilder::build(
        profile,
        default_spherical_space_spec().radius,
        &BuildCancellation::new(),
    )
    .expect("fixture geometry builds")
    .authoritative_surface()
    .clone();
    let seed = RootSeed::new(42);
    let inputs = build_spherical_formation_external_artifacts(
        seed,
        profile,
        &surface,
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
    )
    .expect("fixture inputs validate");
    let artifact = BuildEngine::new(causal_natural_formation_graph().expect("graph validates"))
        .build(seed, inputs, &mut MemoryStageCache::new())
        .expect("fixture builds")
        .artifacts
        .get::<NaturalFormationBundleArtifact>()
        .expect("formation bundle is published");
    (surface, artifact)
}

fn determinant(a: UnitVector3, b: UnitVector3, c: UnitVector3) -> f64 {
    let [ax, ay, az] = a.components();
    let [bx, by, bz] = b.components();
    let [cx, cy, cz] = c.components();
    ax * (by * cz - bz * cy) + ay * (bz * cx - bx * cz) + az * (bx * cy - by * cx)
}

fn direction(values: [f64; 3]) -> UnitVector3 {
    UnitVector3::new(values[0], values[1], values[2]).expect("local radial projection is nonzero")
}

fn interpolate(a: UnitVector3, b: UnitVector3, t: f64) -> UnitVector3 {
    let a = a.components();
    let b = b.components();
    direction(std::array::from_fn(|i| a[i] + t * (b[i] - a[i])))
}

/// Radially projected affine coordinates, Schmidt, Pieper & Kobbelt (2023), §3.
/// Dividing the Cramer coefficients by their sum intersects the ray with the chord plane.
fn barycentric(corners: [UnitVector3; 3], p: UnitVector3) -> [f64; 3] {
    let [a, b, c] = corners;
    let numerators = [
        determinant(p, b, c),
        determinant(a, p, c),
        determinant(a, b, p),
    ];
    let sum: f64 = numerators.iter().sum();
    numerators.map(|value| value / sum)
}

struct Tin {
    faces: Vec<[usize; 3]>,
    star: Vec<Vec<usize>>,
    neighbours: Vec<Vec<usize>>,
    directions: Vec<UnitVector3>,
    elevation: Vec<f64>,
}

impl Tin {
    fn new(surface: &SphericalSurfaceSnapshot, elevation: &[f32]) -> Self {
        let mut incident = vec![Vec::with_capacity(3); surface.vertices().len()];
        for cell in surface.cells() {
            for vertex in &cell.boundary_vertices {
                incident[vertex.raw() as usize].push(cell.id.raw() as usize);
            }
        }
        let directions: Vec<_> = surface.cells().iter().map(|cell| cell.centroid).collect();
        let mut star = vec![Vec::new(); surface.cells().len()];
        let mut edges = HashMap::<(usize, usize), (usize, i32)>::new();
        let mut faces = Vec::with_capacity(incident.len());
        let mut area = 0.0;
        for cells in incident {
            assert_eq!(
                cells.len(),
                3,
                "this spike supports the trivalent Goldberg lattice"
            );
            let mut face = [cells[0], cells[1], cells[2]];
            if determinant(
                directions[face[0]],
                directions[face[1]],
                directions[face[2]],
            ) < 0.0
            {
                face.swap(1, 2);
            }
            assert!(
                determinant(
                    directions[face[0]],
                    directions[face[1]],
                    directions[face[2]]
                ) > 0.0
            );
            for &cell in &face {
                star[cell].push(faces.len());
            }
            for side in 0..3 {
                let (a, b) = (face[side], face[(side + 1) % 3]);
                let (count, balance) = edges.entry((a.min(b), a.max(b))).or_default();
                *count += 1;
                *balance += if a < b { 1 } else { -1 };
            }
            area += spherical_triangle_area_unit(
                directions[face[0]],
                directions[face[1]],
                directions[face[2]],
            );
            faces.push(face);
        }
        assert_eq!(faces.len(), 2 * directions.len() - 4);
        assert!(edges
            .values()
            .all(|&(count, balance)| count == 2 && balance == 0));
        assert_eq!(edges.len(), surface.edges().len());
        let mut neighbours = vec![Vec::new(); directions.len()];
        for edge in surface.edges() {
            let [a, b] = edge.cells.map(|cell| cell.raw() as usize);
            assert!(edges.contains_key(&(a.min(b), a.max(b))));
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
        eprintln!(
            "TIN global cells={} faces={} edges={} area_over_4pi={:.15}",
            directions.len(),
            faces.len(),
            edges.len(),
            area / (4.0 * std::f64::consts::PI)
        );
        Self {
            faces,
            star,
            neighbours,
            directions,
            elevation: elevation.iter().copied().map(f64::from).collect(),
        }
    }

    fn corners(&self, face: usize) -> [UnitVector3; 3] {
        self.faces[face].map(|cell| self.directions[cell])
    }

    fn values(&self, face: usize) -> [f64; 3] {
        self.faces[face].map(|cell| self.elevation[cell])
    }

    fn sample(&self, cell: usize, p: UnitVector3) -> (f64, usize, f64) {
        let (face, weights, minimum) = self.star[cell]
            .iter()
            .map(|&face| {
                let weights = barycentric(self.corners(face), p);
                (
                    face,
                    weights,
                    weights.into_iter().fold(f64::INFINITY, f64::min),
                )
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .expect("every cell has incident faces");
        let z = weights
            .into_iter()
            .zip(self.values(face))
            .map(|(w, z)| w * z)
            .sum();
        (z, face, minimum)
    }

    fn component(&self, lake: &Lake, h: f64) -> Vec<bool> {
        let first = lake.cells()[0].raw() as usize;
        assert!(self.elevation[first] < h);
        let mut reached = vec![false; self.directions.len()];
        let mut queue = VecDeque::from([first]);
        reached[first] = true;
        while let Some(cell) = queue.pop_front() {
            for &next in &self.neighbours[cell] {
                if !reached[next] && self.elevation[next] < h {
                    reached[next] = true;
                    queue.push_back(next);
                }
            }
        }
        reached
    }

    /// Minimax edge paths exactly give the critical sublevel connectivity of a P1 TIN.
    fn spill(&self, lake: &Lake, targets: &[bool]) -> (f64, Vec<usize>) {
        let source = lake.cells()[0].raw() as usize;
        let mut cost = vec![f64::INFINITY; self.elevation.len()];
        let mut parent = vec![usize::MAX; self.elevation.len()];
        // Elevations may be negative. total_cmp order is represented by sorted ranks.
        let mut order: Vec<_> = (0..self.elevation.len()).collect();
        order.sort_unstable_by(|&a, &b| self.elevation[a].total_cmp(&self.elevation[b]));
        let mut rank = vec![0; order.len()];
        for (r, &cell) in order.iter().enumerate() {
            rank[cell] = r;
        }
        let mut heap = BinaryHeap::from([Reverse((rank[source], source))]);
        cost[source] = self.elevation[source];
        while let Some(Reverse((r, cell))) = heap.pop() {
            if cost[cell] != self.elevation[order[r]] {
                continue;
            }
            if targets[cell] {
                let mut path = vec![cell];
                while *path.last().expect("path has one vertex") != source {
                    path.push(parent[*path.last().expect("path has one vertex")]);
                }
                path.reverse();
                return (cost[cell], path);
            }
            for &next in &self.neighbours[cell] {
                let value = cost[cell].max(self.elevation[next]);
                if value < cost[next] {
                    cost[next] = value;
                    parent[next] = cell;
                    heap.push(Reverse((r.max(rank[next]), next)));
                }
            }
        }
        (f64::INFINITY, Vec::new())
    }
}

#[derive(Clone, Copy)]
struct WetPoint {
    p: UnitVector3,
    depth: f64,
}

fn clip(corners: [UnitVector3; 3], depths: [f64; 3]) -> Vec<WetPoint> {
    let points = std::array::from_fn::<_, 3, _>(|i| WetPoint {
        p: corners[i],
        depth: depths[i],
    });
    let mut result = Vec::with_capacity(4);
    for side in 0..3 {
        let (a, b) = (points[side], points[(side + 1) % 3]);
        if a.depth > 0.0 {
            result.push(a);
        }
        if (a.depth > 0.0) != (b.depth > 0.0) {
            result.push(WetPoint {
                p: interpolate(a.p, b.p, a.depth / (a.depth - b.depth)),
                depth: 0.0,
            });
        }
    }
    result
}

/// Spherical polygon area is exact for the radial P1 H contour; volume uses
/// production P1 quadrature weighted by spherical triangle areas, not an exact sphere integral.
fn geometry(corners: [UnitVector3; 3], depths: [f64; 3], radius2: f64) -> (f64, f64) {
    let polygon = clip(corners, depths);
    polygon_geometry(&polygon, radius2)
}

fn polygon_geometry(polygon: &[WetPoint], radius2: f64) -> (f64, f64) {
    let (mut area, mut volume) = (0.0, 0.0);
    for i in 1..polygon.len().saturating_sub(1) {
        let tri = [polygon[0], polygon[i], polygon[i + 1]];
        let a = spherical_triangle_area_unit(tri[0].p, tri[1].p, tri[2].p) * radius2;
        let (_, depth) = integrate_positive_linear_triangle(tri.map(|point| point.depth));
        area += a;
        volume += a * depth;
    }
    (area, volume)
}

fn tin_geometry(
    root: [UnitVector3; 3],
    values: [f64; 3],
    child: [UnitVector3; 3],
    h: f64,
    radius2: f64,
) -> (f64, f64) {
    let [a, b, c] = root;
    let root_det = determinant(a, b, c);
    // A radial P1 field is a ratio of linear forms. Its zero contour is
    // linear only in the numerator; interpolating physical child depths
    // would replace the terrain and move the contour at each LOD.
    let signed = child.map(|p| {
        [
            determinant(p, b, c),
            determinant(a, p, c),
            determinant(a, b, p),
        ]
        .into_iter()
        .zip(values)
        .map(|(w, z)| w * (h - z))
        .sum::<f64>()
            / root_det
    });
    let polygon: Vec<_> = clip(child, signed)
        .into_iter()
        .map(|point| WetPoint {
            p: point.p,
            depth: if point.depth == 0.0 {
                0.0
            } else {
                h - barycentric(root, point.p)
                    .into_iter()
                    .zip(values)
                    .map(|(w, z)| w * z)
                    .sum::<f64>()
            },
        })
        .collect();
    polygon_geometry(&polygon, radius2)
}

fn subdivide(corners: [UnitVector3; 3], depth: u8, visit: &mut impl FnMut([UnitVector3; 3])) {
    if depth == 0 {
        visit(corners);
        return;
    }
    for child in subdivision_children(corners) {
        subdivide(child, depth - 1, visit);
    }
}

fn write_svg(surface: &SphericalSurfaceSnapshot, tin: &Tin, lake: &Lake, wet: &[bool], h: f64) {
    let Ok(root) = std::env::var("SEKAI_LAKE_TIN_EVIDENCE_DIR") else {
        return;
    };
    let center = tin.directions[lake.cells()[0].raw() as usize];
    let slot = lake.id().raw();
    let (east, north) = canonical_east_north_basis(center);
    let project = |p: UnitVector3| {
        let p = p.components();
        [
            p.iter().zip(east).map(|(p, q)| p * q).sum::<f64>(),
            p.iter().zip(north).map(|(p, q)| p * q).sum::<f64>(),
        ]
    };
    let faces: Vec<_> = (0..tin.faces.len())
        .filter(|&face| tin.faces[face].iter().any(|&i| wet[i]))
        .collect();
    let points: Vec<_> = faces
        .iter()
        .flat_map(|&face| tin.corners(face))
        .map(project)
        .collect();
    let min = std::array::from_fn::<_, 2, _>(|i| {
        points.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min)
    });
    let max = std::array::from_fn::<_, 2, _>(|i| {
        points
            .iter()
            .map(|p| p[i])
            .fold(f64::NEG_INFINITY, f64::max)
    });
    let scale = 700.0 / (max[0] - min[0]).max(max[1] - min[1]);
    let screen = |p| {
        let p = project(p);
        [
            50.0 + (p[0] - min[0]) * scale,
            770.0 - (p[1] - min[1]) * scale,
        ]
    };
    let mut svg = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"840\" height=\"840\" viewBox=\"0 0 840 840\"><rect width=\"840\" height=\"840\" fill=\"#f5f2e9\"/>");
    write!(svg, "<text x=\"24\" y=\"28\" font-family=\"sans-serif\" font-size=\"18\">Research TIN / lake {slot}; blue: z &lt; H connected; gray: P5 cells</text>").expect("string write");
    for &face in &faces {
        let poly = clip(tin.corners(face), tin.values(face).map(|z| h - z));
        if poly.len() < 3 {
            continue;
        }
        svg.push_str("<polygon fill=\"#3698bd\" stroke=\"#3698bd\" stroke-width=\"0.2\" points=\"");
        for point in poly {
            let [x, y] = screen(point.p);
            write!(svg, "{x:.4},{y:.4} ").expect("string write");
        }
        svg.push_str("\"/>");
    }
    for cell in lake.cells() {
        svg.push_str("<polygon fill=\"none\" stroke=\"#666\" stroke-width=\"0.8\" points=\"");
        for &vertex in &surface.cells()[cell.raw() as usize].boundary_vertices {
            let [x, y] = screen(surface.vertex(vertex).expect("canonical vertex").position);
            write!(svg, "{x:.4},{y:.4} ").expect("string write");
        }
        svg.push_str("\"/>");
    }
    svg.push_str("</svg>");
    std::fs::write(
        std::path::Path::new(&root).join(format!("lake-{slot}-tin.svg")),
        svg,
    )
    .expect("evidence SVG writes");
}

fn report(
    surface: &SphericalSurfaceSnapshot,
    artifact: &NaturalFormationBundleArtifact,
    tin: &Tin,
    slot: usize,
    candidate_h: Option<f64>,
) -> Option<f64> {
    let start = Instant::now();
    let hydrology = artifact.bundle().surface_formation().hydrology();
    let lake = &hydrology.lakes()[slot];
    let published_h = f64::from(lake.surface_elevation_m());
    let h = candidate_h.unwrap_or(published_h);
    let h_source = if candidate_h.is_some() {
        "same-terrain-minimax-spill-candidate"
    } else {
        "published-P5-centimetre-level"
    };
    eprintln!("TIN lake={slot} H_source={h_source} published_H={published_h:.12} evaluated_H={h:.12} candidate_delta_m={:.12} P5_area_km2={} P5_volume_m3={}",h-published_h,lake.area_km2(),lake.volume_m3());
    let wet = tin.component(lake, h);
    let disconnected = lake
        .cells()
        .iter()
        .filter(|cell| !wet[cell.raw() as usize])
        .count();
    let extra = wet
        .iter()
        .enumerate()
        .filter(|&(i, &yes)| {
            yes && lake
                .cells()
                .binary_search(&crate::world::CellId::from_raw(i as u32))
                .is_err()
        })
        .count();
    let ocean: Vec<_> = (0..wet.len())
        .map(|i| hydrology.surface_water().get(i) == Some(SurfaceWaterKind::Ocean))
        .collect();
    let ocean_wet = wet.iter().zip(&ocean).filter(|(w, o)| **w && **o).count();
    let (ocean_spill, ocean_path) = tin.spill(lake, &ocean);
    let mut downstream = vec![false; wet.len()];
    if let Some(cell) = lake.downstream_cell() {
        downstream[cell.raw() as usize] = true;
    }
    let (outlet_spill, outlet_path) = if lake.downstream_cell().is_some() {
        tin.spill(lake, &downstream)
    } else {
        (f64::NAN, Vec::new())
    };
    let bottlenecks: Vec<_> = outlet_path
        .iter()
        .copied()
        .filter(|&cell| tin.elevation[cell] == outlet_spill)
        .collect();
    let mut lowest_boundary = f64::INFINITY;
    let mut contacts_at_h = Vec::new();
    for (cell, &reached) in wet.iter().enumerate() {
        if !reached {
            continue;
        }
        for &next in &tin.neighbours[cell] {
            if wet[next] {
                continue;
            }
            lowest_boundary = lowest_boundary.min(tin.elevation[next]);
            if tin.elevation[next] == h {
                contacts_at_h.push((cell, next));
            }
        }
    }
    let p5_edge = lake
        .outlet_cell()
        .zip(lake.downstream_cell())
        .map(|(a, b)| {
            let (a, b) = (a.raw() as usize, b.raw() as usize);
            (
                a,
                b,
                tin.elevation[a],
                tin.elevation[b],
                wet[a],
                tin.neighbours[a].contains(&b),
                tin.elevation[a].max(tin.elevation[b]) - h,
            )
        });
    let setup_ms = start.elapsed().as_secs_f64() * 1_000.0;
    eprintln!("TIN lake={slot} H={h:.9} members={} component_cells={} extra_low_cells={extra} disconnected_members={disconnected} ocean_reached={ocean_wet} ocean_spill_minus_H={:.9} ocean_path_len={} outlet={:?} downstream={:?} outlet_spill_minus_H={:.9} outlet_path={outlet_path:?} setup_ms={setup_ms:.3}",
        lake.cells().len(), wet.iter().filter(|&&w| w).count(), ocean_spill-h, ocean_path.len(), lake.outlet_cell(), lake.downstream_cell(), outlet_spill-h);
    eprintln!("TIN lake={slot} minimax_downstream_bottleneck_cells={bottlenecks:?} strict_component_lowest_boundary_minus_H={:.9} boundary_contacts_at_H={contacts_at_h:?} P5_outlet_edge_a_b_za_zb_a_reached_adjacent_edge_saddle_minus_H={p5_edge:?}",lowest_boundary-h);
    if ocean_wet > 0 {
        eprintln!("TIN lake={slot} STOP_CURRENT_H_REACHES_OCEAN: fixed published H is incompatible with this terrain; skip pointless full-ocean LOD/fan enumeration");
        return Some(ocean_spill);
    }
    let radius2 = surface.radius().get().powi(2);
    let faces: Vec<_> = (0..tin.faces.len())
        .filter(|&face| tin.faces[face].iter().any(|&i| wet[i]))
        .collect();
    let p5area = lake.area_km2() * 1_000_000.0;
    let tin_l6_leaves = faces.len() * 4_usize.pow(5);
    eprintln!(
        "TIN lake={slot} planned_L6_leaves={tin_l6_leaves} offline_leaf_budget={L6_LEAF_BUDGET}"
    );
    if tin_l6_leaves > L6_LEAF_BUDGET {
        eprintln!("TIN lake={slot} STOP_DIAGNOSTIC_RESOURCE_BUDGET: global connectivity is measured; LOD enumeration is not measured");
        return None;
    }
    let mut root_area = 0.0;
    let mut root_volume = 0.0;
    let (mut perimeter, mut longest_segment, mut shoreline_segments) = (0.0_f64, 0.0_f64, 0_usize);
    for &face in &faces {
        let corners = tin.corners(face);
        let depths = tin.values(face).map(|z| h - z);
        let (fraction, depth) = integrate_positive_linear_triangle(depths);
        let a = spherical_triangle_area_unit(corners[0], corners[1], corners[2]) * radius2;
        root_area += fraction * a;
        root_volume += depth * a;
        let boundary: Vec<_> = clip(corners, depths)
            .into_iter()
            .filter(|point| point.depth == 0.0)
            .collect();
        if boundary.len() == 2 {
            let length = central_angle(boundary[0].p, boundary[1].p) * surface.radius().get();
            perimeter += length;
            longest_segment = longest_segment.max(length);
            shoreline_segments += usize::from(length > 0.0);
        }
    }
    eprintln!("TIN lake={slot} shoreline_segments={shoreline_segments} perimeter_m={perimeter:.9} longest_segment_m={longest_segment:.9} longest_segment_over_perimeter={:.9}",longest_segment/perimeter);
    let (mut reference_area, mut previous_volume) = (0.0, 0.0);
    for level in 1..=6 {
        let start = Instant::now();
        let (mut area, mut volume) = (0.0, 0.0);
        for &face in &faces {
            let corners = tin.corners(face);
            let values = tin.values(face);
            subdivide(corners, level - 1, &mut |child| {
                let (a, v) = tin_geometry(corners, values, child, h, radius2);
                area += a;
                volume += v;
            });
        }
        if level == 1 {
            reference_area = area;
        }
        eprintln!("TIN lake={slot} L{level} exact_radial_area_m2={area:.9} area_over_P5={:.9} area_relative_L1={:.12e} volume_quad_m3={volume:.9} volume_over_P5={:.9} volume_relative_previous={:.12e} root_projected_area_m2={root_area:.9} root_projected_volume_m3={root_volume:.9} leaves={} ms={:.3}",
            area/p5area, (area-reference_area)/reference_area, volume/lake.volume_m3(), if level>1 {(volume-previous_volume)/volume} else {f64::NAN}, faces.len() * 4_usize.pow(u32::from(level-1)), start.elapsed().as_secs_f64()*1_000.0);
        previous_volume = volume;
    }
    // This local fan diagnostic filters whole faces by the center's analytic
    // TIN component. It is not the fan's own basin, outlet or connectivity.
    // Its center disagreements alone cannot prove or refute a leak.
    let mut cells: Vec<_> = faces.iter().flat_map(|&face| tin.faces[face]).collect();
    for cell in cells.clone() {
        cells.extend(&tin.neighbours[cell]);
    }
    cells.sort_unstable();
    cells.dedup();
    let fan_l6_leaves: usize = cells
        .iter()
        .map(|&cell| surface.cells()[cell].boundary_vertices.len() * 4_usize.pow(5))
        .sum();
    if fan_l6_leaves > L6_LEAF_BUDGET {
        eprintln!("FAN_LOCAL_CENTER_FILTERED lake={slot} STOP_DIAGNOSTIC_RESOURCE_BUDGET planned_L6_leaves={fan_l6_leaves} budget={L6_LEAF_BUDGET}");
        write_svg(surface, tin, lake, &wet, h);
        return None;
    }
    for level in 1..=6 {
        let start = Instant::now();
        let (mut area, mut volume, mut false_wet, mut false_dry, mut minimum_weight) =
            (0.0, 0.0, 0.0, 0.0, 0.0_f64);
        let mut n = 0;
        for &cell in &cells {
            let cell_record = &surface.cells()[cell];
            for side in 0..cell_record.boundary_vertices.len() {
                let corners = [
                    cell_record.centroid,
                    surface
                        .vertex(cell_record.boundary_vertices[side])
                        .expect("vertex")
                        .position,
                    surface
                        .vertex(
                            cell_record.boundary_vertices
                                [(side + 1) % cell_record.boundary_vertices.len()],
                        )
                        .expect("vertex")
                        .position,
                ];
                subdivide(corners, level - 1, &mut |child| {
                    n += 1;
                    let samples = child.map(|p| tin.sample(cell, p));
                    for sample in samples {
                        minimum_weight = minimum_weight.min(sample.2);
                    }
                    let depths = samples.map(|sample| h - sample.0);
                    let (a, v) = geometry(child, depths, radius2);
                    // This is a center-conditioned resampling diagnostic;
                    // a mixed face is not a resolved basin partition.
                    let p = direction(std::array::from_fn(|i| {
                        child.iter().map(|p| p.components()[i]).sum()
                    }));
                    let (z, face, _) = tin.sample(cell, p);
                    let connected = tin.faces[face].iter().any(|&i| wet[i]);
                    if connected {
                        area += a;
                        volume += v;
                    }
                    let face_area =
                        spherical_triangle_area_unit(child[0], child[1], child[2]) * radius2;
                    let sampled = depths.iter().sum::<f64>() > 0.0 && connected;
                    let exact = z < h && connected;
                    false_wet += if sampled && !exact { face_area } else { 0.0 };
                    false_dry += if !sampled && exact { face_area } else { 0.0 };
                });
            }
        }
        eprintln!("FAN_LOCAL_CENTER_FILTERED lake={slot} L{level} diagnostic_area_over_exact={:.12} diagnostic_area_over_P5={:.9} diagnostic_volume_over_TIN_L6={:.12} wet_center_disagreement_area_over_P5={:.9} dry_center_disagreement_area_over_P5={:.9} minimum_star_barycentric={minimum_weight:.12e} leaves={n} ms={:.3}",
            area/reference_area,area/p5area,volume/previous_volume,false_wet/p5area,false_dry/p5area,start.elapsed().as_secs_f64()*1_000.0);
    }
    write_svg(surface, tin, lake, &wet, h);
    None
}

fn probe(profile: NaturalQualityProfile, slots: &[usize]) {
    let start = Instant::now();
    let (surface, artifact) = fixture(profile);
    eprintln!(
        "TIN identity=lake-tin-spike-20261029-v2 profile={profile:?} fixture_ms={:.3}",
        start.elapsed().as_secs_f64() * 1_000.0
    );
    let start = Instant::now();
    let tin = Tin::new(
        &surface,
        artifact
            .bundle()
            .surface_formation()
            .terrain_fields()
            .current_elevation_m(),
    );
    eprintln!(
        "TIN build_ms={:.3} face_vector_bytes={} elevation_vector_bytes={}",
        start.elapsed().as_secs_f64() * 1_000.0,
        tin.faces.capacity() * std::mem::size_of::<[usize; 3]>(),
        tin.elevation.capacity() * std::mem::size_of::<f64>()
    );
    for &slot in slots {
        if let Some(hstar) = report(&surface, &artifact, &tin, slot, None) {
            // One causal comparison, with no epsilon and no retry loop. The
            // published water level remains in the preceding failed record.
            let _ = report(&surface, &artifact, &tin, slot, Some(hstar));
        }
    }
}

#[test]
#[ignore = "throwaway unified-TIN lake geometry, Release and exclusive compute slot only"]
fn unified_tin_spike_standard_lake55() {
    probe(NaturalQualityProfile::Standard, &[55]);
}

#[test]
#[ignore = "throwaway reproduced Draft lake39 then normal lake33 control; exclusive slot"]
fn unified_tin_spike_draft_lake39_control33() {
    probe(NaturalQualityProfile::Draft, &[39, 33]);
}
