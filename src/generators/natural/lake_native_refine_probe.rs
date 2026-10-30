//! Bounded native-TIN candidate fields; no production consumer or lake-height conditioning.
//!
//! Each d=0/1/2 field is a different final model. The old T1 spectral conditioning is an
//! analogy here: native-TIN edges have different physical lengths from old fan edges.
//! Construction uses the existing midpoint method, not exact resampling of the root radial P1.
//! A single Draft fixture covers the reproduced lake39 pit and normal lake33 control.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::mem::size_of;
use std::time::Instant;

use super::{barycentric, clip, direction, fixture, geometry, Tin};
use crate::generators::natural::hierarchical_derivation::{
    probe_cell_conditions, probe_mean_condition_amplitude, signed_unit_noise,
};
use crate::generators::natural::{
    geometric_midpoint, subdivision_children, FormationDerivationInputs, HierarchicalEvaluator,
    LocatedPrimitive,
};
use crate::world::natural::{Lake, NaturalQualityProfile, SurfaceWaterKind};
use crate::world::spatial::{central_angle, UnitVector3};
use crate::world::RootSeed;

// Approved research resource budget, excluding the existing world fixture. It never clips a graph.
const WORKING_BUDGET_BYTES: usize = 256 * 1024 * 1024;
const STUDY_DEPTHS: u32 = 2;
const STUDY_SEED: u64 = 42;
const PROBE_DOMAIN: &[u8] = b"sekai-native-tin-fixed-field-study-20261030-v1\0";

struct NativeField {
    points: Vec<UnitVector3>,
    elevation: Vec<f64>,
    conditions: Vec<[f64; 2]>,
    seeds: Vec<[u8; 32]>,
    faces: Vec<[u32; 3]>,
    offsets: Vec<usize>,
    neighbours: Vec<u32>,
    depth: u32,
}

struct Flood {
    height: Vec<f64>,
    next: Vec<u32>,
    heap_peak: usize,
}

fn edge_key(a: u32, b: u32) -> u64 {
    (u64::from(a.min(b)) << 32) | u64::from(a.max(b))
}

fn endpoints(key: u64) -> (usize, usize) {
    ((key >> 32) as usize, (key as u32) as usize)
}

fn root_seed(cell: u32) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(PROBE_DOMAIN);
    hash.update(&STUDY_SEED.to_le_bytes());
    hash.update(b"c");
    hash.update(&cell.to_le_bytes());
    *hash.finalize().as_bytes()
}

fn midpoint_seed(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let (low, high) = if a <= b { (a, b) } else { (b, a) };
    let mut hash = blake3::Hasher::new();
    hash.update(PROBE_DOMAIN);
    hash.update(&STUDY_SEED.to_le_bytes());
    hash.update(b"m");
    hash.update(low);
    hash.update(high);
    *hash.finalize().as_bytes()
}

impl NativeField {
    fn from_root(root: &Tin, conditions: Vec<[f64; 2]>) -> Self {
        let mut field = Self {
            points: root.directions.clone(),
            elevation: root.elevation.clone(),
            conditions,
            seeds: (0..root.directions.len())
                .map(|cell| root_seed(cell as u32))
                .collect(),
            faces: root
                .faces
                .iter()
                .map(|face| face.map(|i| i as u32))
                .collect(),
            offsets: Vec::new(),
            neighbours: Vec::new(),
            depth: 0,
        };
        field.rebuild_graph();
        field
    }

    fn keys(&self) -> Vec<u64> {
        let mut edges = Vec::with_capacity(self.faces.len() * 3);
        for &[a, b, c] in &self.faces {
            edges.extend([edge_key(a, b), edge_key(b, c), edge_key(c, a)]);
        }
        edges.sort_unstable();
        edges.dedup();
        edges
    }

    fn rebuild_graph(&mut self) {
        let edges = self.keys();
        self.offsets = vec![0; self.points.len() + 1];
        for &key in &edges {
            let (a, b) = endpoints(key);
            self.offsets[a + 1] += 1;
            self.offsets[b + 1] += 1;
        }
        for i in 1..self.offsets.len() {
            self.offsets[i] += self.offsets[i - 1];
        }
        self.neighbours = vec![0; edges.len() * 2];
        let mut cursor = self.offsets[..self.points.len()].to_vec();
        for &key in &edges {
            let (a, b) = endpoints(key);
            self.neighbours[cursor[a]] = b as u32;
            self.neighbours[cursor[b]] = a as u32;
            cursor[a] += 1;
            cursor[b] += 1;
        }
    }

    fn refine(&mut self) -> bool {
        let edges = self.keys();
        let nodes = self.points.len() + edges.len();
        let faces = self.faces.len() * 4;
        // Includes the retained root, sorting/CSR temporaries and a worst-case edge-count heap.
        // This is an allocation bound for the finite study, not a process-peak measurement.
        let estimated_bytes = self.storage_bytes()
            + nodes * (size_of::<UnitVector3>() + 8 + 16 + 32 + 8 + 16)
            + faces * (size_of::<[u32; 3]>() + 3 * size_of::<u64>() + 16)
            + edges.capacity() * size_of::<u64>();
        eprintln!(
            "NATIVE next_d={} planned_nodes={nodes} planned_faces={faces} allocation_bound_bytes={estimated_bytes} resource_budget_bytes={WORKING_BUDGET_BYTES}",
            self.depth + 1
        );
        if estimated_bytes > WORKING_BUDGET_BYTES {
            eprintln!("NATIVE STOP_RESOURCE_BUDGET: next candidate not constructed; no graph was truncated");
            return false;
        }
        let first_new = self.points.len() as u32;
        self.points.reserve(edges.len());
        self.elevation.reserve(edges.len());
        self.conditions.reserve(edges.len());
        self.seeds.reserve(edges.len());
        for &key in &edges {
            let (a, b) = endpoints(key);
            let seed = midpoint_seed(&self.seeds[a], &self.seeds[b]);
            let (condition, amplitude) = probe_mean_condition_amplitude(
                self.conditions[a],
                self.conditions[b],
                self.depth + 1,
            );
            self.points
                .push(geometric_midpoint(self.points[a], self.points[b]));
            self.elevation.push(
                0.5 * (self.elevation[a] + self.elevation[b])
                    + amplitude * signed_unit_noise(&seed),
            );
            self.conditions.push(condition);
            self.seeds.push(seed);
        }
        let midpoint = |a, b| {
            first_new
                + edges
                    .binary_search(&edge_key(a, b))
                    .expect("all parent edges were enumerated") as u32
        };
        let mut children = Vec::with_capacity(faces);
        for &[a, b, c] in &self.faces {
            let (ab, bc, ca) = (midpoint(a, b), midpoint(b, c), midpoint(c, a));
            children.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        }
        self.faces = children;
        self.depth += 1;
        drop(edges);
        self.rebuild_graph();
        true
    }

    fn storage_bytes(&self) -> usize {
        self.points.capacity() * size_of::<UnitVector3>()
            + self.elevation.capacity() * size_of::<f64>()
            + self.conditions.capacity() * size_of::<[f64; 2]>()
            + self.seeds.capacity() * size_of::<[u8; 32]>()
            + self.faces.capacity() * size_of::<[u32; 3]>()
            + self.offsets.capacity() * size_of::<usize>()
            + self.neighbours.capacity() * size_of::<u32>()
    }

    fn neighbours(&self, node: usize) -> &[u32] {
        &self.neighbours[self.offsets[node]..self.offsets[node + 1]]
    }

    fn corners(&self, face: usize) -> [UnitVector3; 3] {
        self.faces[face].map(|i| self.points[i as usize])
    }

    fn values(&self, face: usize) -> [f64; 3] {
        self.faces[face].map(|i| self.elevation[i as usize])
    }

    // The undirected minimax problem is solved once from all retained ocean terminals.
    // This is the same discrete sublevel connectivity as Tin::spill, using CSR and ranks.
    fn flood(&self, ocean: &[bool]) -> Flood {
        let mut order: Vec<u32> = (0..self.points.len() as u32).collect();
        order.sort_unstable_by(|&a, &b| {
            self.elevation[a as usize]
                .total_cmp(&self.elevation[b as usize])
                .then(a.cmp(&b))
        });
        let mut rank = vec![0_u32; order.len()];
        for (r, &node) in order.iter().enumerate() {
            rank[node as usize] = r as u32;
        }
        let mut cost = vec![u32::MAX; order.len()];
        let mut next = vec![u32::MAX; order.len()];
        let mut heap = BinaryHeap::new();
        for (node, &yes) in ocean.iter().enumerate().filter(|(_, yes)| **yes) {
            debug_assert!(yes);
            cost[node] = rank[node];
            next[node] = node as u32;
            heap.push(Reverse((rank[node], node as u32)));
        }
        let mut heap_peak = heap.len();
        while let Some(Reverse((level, node))) = heap.pop() {
            if cost[node as usize] != level {
                continue;
            }
            for &neighbour in self.neighbours(node as usize) {
                let value = level.max(rank[neighbour as usize]);
                if value < cost[neighbour as usize] {
                    cost[neighbour as usize] = value;
                    next[neighbour as usize] = node;
                    heap.push(Reverse((value, neighbour)));
                }
            }
            heap_peak = heap_peak.max(heap.len());
        }
        assert!(
            cost.iter().all(|&r| r != u32::MAX),
            "full graph must reach a terminal"
        );
        Flood {
            height: cost
                .iter()
                .map(|&r| self.elevation[order[r as usize] as usize])
                .collect(),
            next,
            heap_peak,
        }
    }

    fn component(&self, source: usize, h: f64) -> Vec<bool> {
        let mut reached = vec![false; self.points.len()];
        if self.elevation[source] >= h {
            return reached;
        }
        let mut queue = VecDeque::from([source]);
        reached[source] = true;
        while let Some(node) = queue.pop_front() {
            for &next in self.neighbours(node) {
                let next = next as usize;
                if !reached[next] && self.elevation[next] < h {
                    reached[next] = true;
                    queue.push_back(next);
                }
            }
        }
        reached
    }

    fn edge_lengths(&self, radius: f64) -> [f64; 3] {
        let mut stats = [f64::INFINITY, 0.0_f64, 0.0];
        let mut count = 0;
        for a in 0..self.points.len() {
            for &b in self.neighbours(a).iter().filter(|&&b| a < b as usize) {
                let length = central_angle(self.points[a], self.points[b as usize]) * radius;
                stats[0] = stats[0].min(length);
                stats[1] = stats[1].max(length);
                stats[2] += length;
                count += 1;
            }
        }
        stats[2] /= count as f64;
        stats
    }

    fn sample(
        &self,
        root: &Tin,
        locator: &HierarchicalEvaluator,
        p: UnitVector3,
    ) -> (f64, usize, f64) {
        let cell = match locator.locate(p, 0) {
            LocatedPrimitive::Cell(cell) | LocatedPrimitive::Triangle { cell, .. } => cell,
        };
        let (_, mut face, mut margin) = root.sample(cell.raw() as usize, p);
        let mut corners = root.corners(face);
        for _ in 0..self.depth {
            let candidates = subdivision_children(corners);
            let (child, child_margin) = candidates
                .iter()
                .enumerate()
                .map(|(i, &tri)| {
                    (
                        i,
                        barycentric(tri, p)
                            .into_iter()
                            .fold(f64::INFINITY, f64::min),
                    )
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .expect("four children exist");
            margin = margin.min(child_margin);
            corners = candidates[child];
            face = face * 4 + child;
        }
        let height = barycentric(self.corners(face), p)
            .into_iter()
            .zip(self.values(face))
            .map(|(w, z)| w * z)
            .sum();
        (height, face, margin)
    }
}

fn query_evidence(
    field: &NativeField,
    root: &Tin,
    locator: &HierarchicalEvaluator,
    wet: &[bool],
    h: f64,
) {
    let faces: Vec<_> = field
        .faces
        .iter()
        .enumerate()
        .filter(|(_, face)| face.iter().any(|&i| wet[i as usize]))
        .map(|(i, _)| i)
        .collect();
    let stride = (faces.len() / 32).max(1);
    let mut max_error = 0.0_f64;
    let mut minimum_margin = 0.0_f64;
    let mut mismatches = 0;
    let mut queries = 0;
    let start = Instant::now();
    for &face in faces.iter().step_by(stride).take(32) {
        let corners = field.corners(face);
        // Known final-primitive interiors exercise the independent root/child locator.
        let point = direction(std::array::from_fn(|axis| {
            corners.iter().map(|p| p.components()[axis]).sum()
        }));
        let direct = barycentric(corners, point)
            .into_iter()
            .zip(field.values(face))
            .map(|(w, z)| w * z)
            .sum::<f64>();
        let (sample, located, margin) = field.sample(root, locator, point);
        let direct_water = direct < h && field.faces[face].iter().any(|&i| wet[i as usize]);
        let sampled_water = sample < h && field.faces[located].iter().any(|&i| wet[i as usize]);
        max_error = max_error.max((direct - sample).abs());
        minimum_margin = minimum_margin.min(margin);
        mismatches += usize::from(direct_water != sampled_water);
        queries += 1;
    }
    eprintln!("NATIVE_QUERY d={} known_final_interiors={queries} max_direct_radial_error_m={max_error:.12e} water_mismatches={mismatches} minimum_locator_barycentric={minimum_margin:.12e} total_ms={:.3} note=fixed_field_locator_vs_known_final_piece_not_UI_or_cross_physical_depth_invariance",field.depth,start.elapsed().as_secs_f64()*1000.0);
}

struct InquiryContext<'a> {
    root: &'a Tin,
    locator: &'a HierarchicalEvaluator,
    flood: &'a Flood,
    ocean: &'a [bool],
    radius: f64,
}

fn report_lake(field: &NativeField, lake: &Lake, context: &InquiryContext<'_>) {
    let InquiryContext {
        root,
        locator,
        flood,
        ocean,
        radius,
    } = *context;
    // The old member minimum is only a stable inquiry source, not a required surviving pit.
    let source = lake
        .cells()
        .iter()
        .map(|c| c.raw() as usize)
        .min_by(|&a, &b| {
            field.elevation[a]
                .total_cmp(&field.elevation[b])
                .then(a.cmp(&b))
        })
        .expect("published lake has members");
    let h = flood.height[source];
    let wet = field.component(source, h);
    let reached = wet.iter().filter(|&&w| w).count();
    let ocean_reached = ocean
        .iter()
        .enumerate()
        .filter(|&(i, yes)| *yes && wet[i])
        .count();
    assert_eq!(
        ocean_reached, 0,
        "strict sublevel at its minimax critical value cannot reach ocean"
    );
    let mut path_length = 0;
    let mut node = source;
    while flood.next[node] != node as u32 {
        node = flood.next[node] as usize;
        path_length += 1;
        assert!(
            path_length <= field.points.len(),
            "minimax path cannot cycle"
        );
    }
    if reached == 0 {
        eprintln!("NATIVE lake={} d={} inquiry_source={source} source_z={} critical_H={h:.12} status=INQUIRY_SOURCE_NOT_BELOW_ITS_SPILL no_surviving_strict_basin=true path_edges={path_length}",lake.id().raw(),field.depth,field.elevation[source]);
        return;
    }
    let start = Instant::now();
    let (mut area, mut volume, mut perimeter, mut longest) = (0.0, 0.0, 0.0, 0.0_f64);
    let (mut basin_faces, mut segments) = (0, 0);
    for face in 0..field.faces.len() {
        if !field.faces[face].iter().any(|&i| wet[i as usize]) {
            continue;
        }
        basin_faces += 1;
        let corners = field.corners(face);
        let depths = field.values(face).map(|z| h - z);
        let (a, v) = geometry(corners, depths, radius * radius);
        area += a;
        volume += v;
        let boundary: Vec<_> = clip(corners, depths)
            .into_iter()
            .filter(|p| p.depth == 0.0)
            .collect();
        if boundary.len() == 2 {
            let length = central_angle(boundary[0].p, boundary[1].p) * radius;
            perimeter += length;
            longest = longest.max(length);
            segments += usize::from(length > 0.0);
        }
    }
    eprintln!("NATIVE lake={} d={} inquiry_source={source} source_z={} old_P5_H={} critical_H={h:.12} reached_nodes={reached} ocean_reached={ocean_reached} path_edges={path_length} basin_faces={basin_faces} radial_area_m2={area:.9} area_over_old_P5={:.9} volume_quad_m3={volume:.9} volume_over_old_geometric_P5={:.9} shoreline_segments={segments} perimeter_m={perimeter:.9} longest_segment_m={longest:.9} longest_over_perimeter={:.9} geometry_ms={:.3}",lake.id().raw(),field.depth,field.elevation[source],lake.surface_elevation_m(),area/(lake.area_km2()*1_000_000.0),volume/lake.volume_m3(),longest/perimeter,start.elapsed().as_secs_f64()*1000.0);
    query_evidence(field, root, locator, &wet, h);
}

#[test]
fn native_refinement_has_one_shared_midpoint_and_keeps_constant_roots() {
    // Two adjacent triangles are enough to detect a duplicate shared-edge midpoint.
    let points = [
        [-1.0, -1.0, 4.0],
        [1.0, -1.0, 4.0],
        [1.0, 1.0, 4.0],
        [-1.0, 1.0, 4.0],
    ]
    .map(direction)
    .to_vec();
    let mut field = NativeField {
        points,
        elevation: vec![2.0; 4],
        conditions: vec![[0.0, 0.5]; 4],
        seeds: (0..4).map(root_seed).collect(),
        faces: vec![[0, 1, 2], [0, 2, 3]],
        offsets: Vec::new(),
        neighbours: Vec::new(),
        depth: 0,
    };
    field.rebuild_graph();
    assert!(field.refine());
    assert_eq!(field.points.len(), 9);
    assert_eq!(field.faces.len(), 8);
    assert!(field.elevation.iter().all(|&z| z == 2.0));
    assert_eq!(field.faces.iter().filter(|f| f.contains(&5)).count(), 6);
}

#[test]
#[ignore = "bounded Draft native-TIN final fields; Release and exclusive compute window only"]
fn native_tin_fixed_field_draft_pits39_33() {
    let start = Instant::now();
    let (surface, artifact) = fixture(NaturalQualityProfile::Draft);
    let bundle = artifact.bundle();
    let inputs = FormationDerivationInputs {
        surface: &surface,
        compatibility: bundle.tectonics().compatibility(),
        substrate: bundle.substrate(),
        formation: bundle.surface_formation(),
        climate: bundle.climate(),
    };
    let conditions =
        probe_cell_conditions(inputs, RootSeed::new(STUDY_SEED)).expect("production conditions");
    let locator = HierarchicalEvaluator::from_formation_product(inputs, RootSeed::new(STUDY_SEED))
        .expect("production spatial locator");
    let terrain = bundle.surface_formation().terrain_fields();
    let hydrology = bundle.surface_formation().hydrology();
    let root = Tin::new(&surface, terrain.current_elevation_m());
    let ocean: Vec<_> = (0..root.directions.len())
        .map(|i| hydrology.surface_water().get(i) == Some(SurfaceWaterKind::Ocean))
        .collect();
    let base_started = Instant::now();
    let mut field = NativeField::from_root(&root, conditions);
    let base_build_ms = base_started.elapsed().as_secs_f64() * 1000.0;
    eprintln!("NATIVE identity=native-tin-fixed-field-study-20261030-v1 profile=Draft seed={STUDY_SEED} fixture_and_conditioning_ms={:.3} root_edge_min_max_mean_m={:?} resource_budget_bytes={WORKING_BUDGET_BYTES} note=no_height_conditioning_no_tuned_parameters_no_product_or_calibration_claim",start.elapsed().as_secs_f64()*1000.0,field.edge_lengths(surface.radius().get()));
    for depth in 0..=STUDY_DEPTHS {
        let build = Instant::now();
        if depth > 0 && !field.refine() {
            break;
        }
        let build_ms = if depth == 0 {
            base_build_ms
        } else {
            build.elapsed().as_secs_f64() * 1000.0
        };
        let flood_started = Instant::now();
        let flood = field.flood(&ocean);
        eprintln!("NATIVE d={depth} nodes={} faces={} edges={} storage_capacity_bytes={} build_ms={build_ms:.3} global_minimax_ms={:.3} heap_peak_entries={} edge_min_max_mean_m={:?}",field.points.len(),field.faces.len(),field.neighbours.len()/2,field.storage_bytes(),flood_started.elapsed().as_secs_f64()*1000.0,flood.heap_peak,field.edge_lengths(surface.radius().get()));
        for slot in [39, 33] {
            report_lake(
                &field,
                &hydrology.lakes()[slot],
                &InquiryContext {
                    root: &root,
                    locator: &locator,
                    flood: &flood,
                    ocean: &ocean,
                    radius: surface.radius().get(),
                },
            );
        }
    }
}
