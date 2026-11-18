//! Marine reachability on the source sphere. Compartments share passages,
//! including all subsegments of a coarse/fine edge. Only the exact fixed bed
//! sets water depth; a compartment boundary cannot stop a resolved submerged passage.
mod basins;
mod ports;
mod refinement;
mod support;

use std::collections::{BTreeMap, VecDeque};

use civ_atlas_terrain::FineTerrain;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use self::support::{Mesh, Passage, Support};
use super::{check_cancelled, AtlasGenerationError};
use crate::engine::BuildCancellation;
use crate::world::natural::atlas_terrain::{
    coast::CoastMap, AtlasSiteBody, ATLAS_COAST_CLEARANCE_SUBDIVISIONS,
};
use crate::world::spatial::{central_angle, UnitVector3};

/// Reconstruct the ocean footprints before any inventory or climate forcing is built.
pub(super) fn build(
    fine: &FineTerrain,
    sites: &[Option<AtlasSiteBody>],
    cancel: &BuildCancellation,
) -> Result<CoastMap, AtlasGenerationError> {
    Ok(build_with_mesh(fine, sites, cancel)?.0)
}

fn build_with_mesh(
    fine: &FineTerrain,
    sites: &[Option<AtlasSiteBody>],
    cancel: &BuildCancellation,
) -> Result<(CoastMap, Mesh), AtlasGenerationError> {
    let timer = crate::engine::StageTimer::now();
    let terrain = fine.coarse_terrain();
    let mut mesh = Mesh::new(terrain)?;
    let mut bed = vertex_beds(fine, &mesh.directions, cancel)?;
    let region = potentially_marine(fine, &mesh, sites, &bed, cancel)?;
    let footprint_ms = timer.elapsed().as_millis();
    let is_source = |i: usize| sites[i] == Some(AtlasSiteBody::Ocean);
    let coastal: Vec<_> = (0..sites.len())
        .map(|i| {
            super::geometry::site_neighbors(terrain, i)
                .iter()
                .any(|&j| is_source(i) != is_source(j) || (bed[i] < 0.0) != (bed[j] < 0.0))
        })
        .collect();
    let mut assessed = Vec::with_capacity(region.len());
    let mut pending = Vec::new();
    for (face, ids) in terrain.site_triangles().chunks_exact(3).enumerate() {
        let needed =
            region[face] && (ids.iter().any(|&i| coastal[i]) || ids.iter().all(|&i| !is_source(i)));
        assessed.push(needed);
        if needed {
            pending.push(face as u32);
        }
    }
    refinement::adapt(fine, &mut mesh, &mut bed, pending, cancel)?;
    log::debug!(
        "Atlas coastal footprint_ms={footprint_ms} local_refinement_ms={}",
        timer.elapsed().as_millis() - footprint_ms
    );
    let mut readings = PassageReadings::default();
    loop {
        let (map, support) = mesh.assemble(cancel)?;
        let field = WorkingBed {
            mesh: &mesh,
            support: &support,
            bed: &bed,
        };
        let (finished, mut split) = resolve((fine, sites), &field, map, &mut readings, cancel)?;
        let mut inland = Vec::new();
        for (cell, &node) in support.leaf_nodes.iter().enumerate() {
            let face = mesh.source_face(node);
            if node as usize != face || assessed[face] || !finished.cell_is_marine(cell) {
                continue;
            }
            assessed[face] = true;
            if terrain.site_triangles()[3 * face..3 * face + 3]
                .iter()
                .all(|&i| !is_source(i))
            {
                inland.push(node);
            }
        }
        drop(support);
        if split.is_empty() && inland.is_empty() {
            log::debug!(
                "Atlas coastal connectivity total_ms={}",
                timer.elapsed().as_millis()
            );
            return Ok((finished, mesh));
        }
        log::debug!(
            "Atlas local coast refinement: {} multi-port leaves, {} newly reached source faces",
            split.len(),
            inland.len()
        );
        // Split just the affected leaves. Already sampled vertices and all other patches survive.
        // A previously measured disconnection merits explicit native-scale support in this leaf.
        while let Some(node) = split.pop() {
            check_cancelled(cancel)?;
            if leaf_diameter(&mesh, node) > fine.canonical_angular_step_radians() {
                split.extend(mesh.split(node)?);
            }
        }
        bed.extend(vertex_beds(fine, &mesh.directions[bed.len()..], cancel)?);
        if !inland.is_empty() {
            refinement::adapt(fine, &mut mesh, &mut bed, inland, cancel)?;
        }
    }
}

fn leaf_diameter(mesh: &Mesh, node: u32) -> f64 {
    let [a, b, c] = mesh.node_vertices(node).map(|id| {
        let [x, y, z] = mesh.directions[id as usize];
        UnitVector3::new(x, y, z).expect("validated coastal mesh direction")
    });
    central_angle(a, b)
        .max(central_angle(b, c))
        .max(central_angle(c, a))
}

/// The coarse flood only selects where to refine. A real water path may traverse
/// more than one originally inland source face.
fn potentially_marine(
    fine: &FineTerrain,
    mesh: &Mesh,
    sites: &[Option<AtlasSiteBody>],
    beds: &[f64],
    cancel: &BuildCancellation,
) -> Result<Vec<bool>, AtlasGenerationError> {
    let (map, support) = mesh.assemble(cancel)?;
    let adjacency = Adjacency::new(&support.passages, map.cell_count(), cancel)?;
    let mut band = Inundation::new(map.cell_count());
    for (f, ids) in fine
        .coarse_terrain()
        .site_triangles()
        .chunks_exact(3)
        .enumerate()
    {
        if ids
            .iter()
            .any(|&i| sites[i] == Some(AtlasSiteBody::Ocean) && beds[i] < 0.0)
        {
            band.admit(f);
        }
    }
    // This preliminary footprint is allowed to overestimate. The verified final flood
    // can still promote any additional inland face that actually admits water.
    let step = fine.canonical_angular_step_radians();
    band.flood(&adjacency, beds, cancel, &|_, p| {
        let [a, b] = p.vertices.map(|v| mesh.directions[v as usize]);
        edge_has_water(fine, a, b, step, cancel)
    })?;
    Ok(band.reached)
}

struct WorkingBed<'a> {
    mesh: &'a Mesh,
    support: &'a Support<'a>,
    bed: &'a [f64],
}

/// Vertex IDs remain stable across local splits. Only identical arcs from this
/// fixed world can reuse a terrain survey; native and fine divide checks stay distinct.
#[derive(Default)]
struct PassageReadings {
    native: FxHashMap<[u32; 2], bool>,
    divides: FxHashMap<[u32; 2], Vec<[f64; 3]>>,
    native_windows: FxHashMap<[u32; 2], Vec<[f64; 3]>>,
}

impl PassageReadings {
    fn inherited(&self, mesh: &Mesh) -> FxHashMap<[u32; 2], Vec<[f64; 3]>> {
        let mut result = FxHashMap::<[u32; 2], Vec<[f64; 3]>>::default();
        for (&edge, points) in self.native_windows.iter().chain(&self.divides) {
            for &point in points {
                result
                    .entry(mesh.segment_at(edge, point))
                    .or_default()
                    .push(point);
            }
        }
        for points in result.values_mut() {
            points.sort_unstable_by_key(|p| p.map(f64::to_bits));
            points.dedup();
        }
        result
    }
}

fn resolve(
    (fine, sites): (&FineTerrain, &[Option<AtlasSiteBody>]),
    field: &WorkingBed<'_>,
    mut map: CoastMap,
    readings: &mut PassageReadings,
    cancel: &BuildCancellation,
) -> Result<(CoastMap, Vec<u32>), AtlasGenerationError> {
    let timer = crate::engine::StageTimer::now();
    let native_step = fine.canonical_angular_step_radians();
    let support = field.support;
    let bed = field.bed;
    let seeded =
        |v: usize| v < sites.len() && sites[v] == Some(AtlasSiteBody::Ocean) && bed[v] < 0.0;
    let mut ocean = Inundation::new(map.cell_count());
    let mut floor = vec![false; map.cell_count()];
    let inherited = readings.inherited(field.mesh);
    let mut openings = BTreeMap::<usize, Vec<[f64; 3]>>::new();
    for (i, passage) in support.passages.iter().enumerate() {
        if i % 4096 == 0 {
            check_cancelled(cancel)?;
        }
        if passage.vertices.iter().any(|&v| seeded(v as usize)) {
            for &cell in &passage.cells {
                ocean.admit(cell as usize);
            }
        }
        let known = inherited.get(&support::edge_key(passage.vertices));
        if let Some(points) = known {
            openings.insert(i, points.clone());
        }
        if known.is_some() || passage.vertices.iter().any(|&v| bed[v as usize] < 0.0) {
            for &cell in &passage.cells {
                floor[cell as usize] = true;
            }
        }
    }
    let adjacency = Adjacency::new(&support.passages, map.cell_count(), cancel)?;
    let mut clearance = vec![None; support.passages.len()];
    let inspect = |edge: &Passage| {
        let key = support::edge_key(edge.vertices);
        if inherited.contains_key(&key) {
            return Ok(true);
        }
        if let Some(&wet) = readings.native.get(&key) {
            return Ok(wet);
        }
        let [a, b] = key.map(|v| support.directions[v as usize]);
        edge_has_water(fine, a, b, native_step, cancel)
    };
    let mut tested = ocean.flood(&adjacency, bed, cancel, &|_, edge| inspect(edge))?;
    mark_inland_floors(fine, field, &ocean.reached, &mut floor, cancel)?;
    let flood_ms = timer.elapsed().as_millis();
    let closed_markers = |floor: &[bool], marine: &[bool]| -> Vec<bool> {
        floor
            .iter()
            .zip(marine)
            .map(|(&has_floor, &sea)| has_floor && !sea)
            .collect()
    };
    let closed = closed_markers(&floor, &ocean.reached);
    let forest =
        basins::MarkerForest::new(&support.passages, bed, &ocean.reached, &closed, cancel)?;
    let forest_ms = timer.elapsed().as_millis() - flood_ms;
    let mut checked = 0;
    let mut wet_divides = Vec::new();
    let mut iterations = 0;
    loop {
        check_cancelled(cancel)?;
        iterations += 1;
        let closed = closed_markers(&floor, &ocean.reached);
        let owners = forest.owners(&ocean.reached, &closed, cancel)?;
        let boundary: Vec<_> = support
            .passages
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let [a, b] = p.cells.map(|v| v as usize);
                (owners[a] != owners[b] && clearance[i].is_none()).then_some(i)
            })
            .collect();
        checked += boundary.len();
        #[cfg(not(target_arch = "wasm32"))]
        let edges = boundary.par_iter();
        #[cfg(target_arch = "wasm32")]
        let edges = boundary.iter();
        let wet: Result<Vec<_>, _> = edges
            .map(|&i| {
                let key = support::edge_key(support.passages[i].vertices);
                if let Some(points) = readings.divides.get(&key) {
                    return Ok(points.clone());
                }
                let vertices = key.map(|v| v as usize);
                let arc = BedArc::new(vertices.map(|v| support.directions[v]));
                arc.divide_openings(fine, vertices.map(|v| bed[v]), native_step, cancel)
            })
            .collect();
        let mut new_floor = false;
        for (i, points) in boundary.into_iter().zip(wet?) {
            readings
                .divides
                .entry(support::edge_key(support.passages[i].vertices))
                .or_insert_with(|| points.clone());
            clearance[i] = Some(!points.is_empty());
            if !points.is_empty() {
                let [a, b] = support.passages[i].cells.map(|v| v as usize);
                floor[a] = true;
                floor[b] = true;
                new_floor = true;
                if ocean.reached[a] {
                    ocean.admit(b);
                }
                if ocean.reached[b] {
                    ocean.admit(a);
                }
                wet_divides.push((i, points));
            }
        }
        if !new_floor {
            for &i in openings.keys().chain(wet_divides.iter().map(|(i, _)| i)) {
                let [a, b] = support.passages[i].cells.map(|c| c as usize);
                if owners[a] != owners[b] {
                    return Err(AtlasGenerationError::Geometry(format!(
                        "submerged coast passage {i} still separates source faces {} and {}",
                        field.mesh.source_face(support.leaf_nodes[a]),
                        field.mesh.source_face(support.leaf_nodes[b])
                    )));
                }
            }
            for (i, reached) in owners.into_iter().enumerate() {
                if reached {
                    map.set_marine(i);
                }
            }
            break;
        }
        if !ocean.frontier.is_empty() {
            tested += ocean.flood(&adjacency, bed, cancel, &|i, edge| match clearance[i] {
                Some(wet) => Ok(wet),
                None => inspect(edge),
            })?;
        }
    }
    log::debug!("Atlas coast sites={} support_vertices={} compartments={} passages={} flooded_edges={} divide_edges={} added_wet_passages={} divide_rounds={} elapsed_ms={}",
        sites.len(), bed.len(), map.cell_count(), support.passages.len(), tested,checked,wet_divides.len(),iterations,timer.elapsed().as_millis());
    log::debug!("Atlas coast initial_flood_and_markers_ms={flood_ms} forest_ms={forest_ms} divide_clearance_ms={}",
        timer.elapsed().as_millis()-flood_ms-forest_ms);
    let surveyed = MarineSurvey {
        adj: &adjacency,
        field,
    };
    for (i, points) in wet_divides {
        openings.entry(i).or_default().extend(points);
    }
    for &(i, wet) in &ocean.tested {
        let key = support::edge_key(support.passages[i].vertices);
        if inherited.contains_key(&key) || clearance[i].is_some() {
            continue;
        }
        readings.native.insert(key, wet);
        if !wet {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = openings.entry(i) {
            if let Some(points) = readings.native_windows.get(&key) {
                entry.insert(points.clone());
                continue;
            }
            let vertices = key.map(|v| v as usize);
            let arc = BedArc::new(vertices.map(|v| support.directions[v]));
            let n = arc.intervals(native_step);
            let samples = arc.interior(fine, n, cancel)?;
            let stations = std::iter::once((0, bed[vertices[0]]))
                .chain(
                    samples
                        .into_iter()
                        .enumerate()
                        .map(|(i, s)| (i + 1, s.bed_elevation_m)),
                )
                .chain(std::iter::once((n, bed[vertices[1]])));
            let points = arc.wet_runs(stations, n);
            readings.native_windows.insert(key, points.clone());
            entry.insert(points);
        }
    }
    let split = surveyed.disconnected_faces(fine, &map, &openings, cancel)?;
    Ok((map, split))
}

/// Late-discovered openings between dry vertices must not turn two distinct
/// submerged lobes within a coarse compartment into a single through-route.
struct MarineSurvey<'a> {
    adj: &'a Adjacency<'a>,
    field: &'a WorkingBed<'a>,
}
impl MarineSurvey<'_> {
    fn disconnected_faces(
        &self,
        fine: &FineTerrain,
        map: &CoastMap,
        openings: &BTreeMap<usize, Vec<[f64; 3]>>,
        cancel: &BuildCancellation,
    ) -> Result<Vec<u32>, AtlasGenerationError> {
        let mut cells = BTreeMap::<usize, Vec<[f64; 3]>>::new();
        for (&i, points) in openings {
            for cell in self.field.support.passages[i].cells.map(|v| v as usize) {
                if map.cell_is_marine(cell) {
                    cells.entry(cell).or_default().extend_from_slice(points);
                }
            }
        }
        let step = fine.canonical_angular_step_radians();
        #[cfg(not(target_arch = "wasm32"))]
        let entries = cells.into_par_iter();
        #[cfg(target_arch = "wasm32")]
        let entries = cells.into_iter();
        let results: Result<Vec<_>, AtlasGenerationError> = entries
            .map(|(cell, mut points)| {
                check_cancelled(cancel)?;
                let node = self.field.support.leaf_nodes[cell];
                if leaf_diameter(self.field.mesh, node) <= step {
                    return Ok(None);
                }
                let mut vertices = Vec::new();
                for &id in &self.adj.edges[self.adj.offsets[cell]..self.adj.offsets[cell + 1]] {
                    vertices.extend(
                        self.field.support.passages[id as usize]
                            .vertices
                            .into_iter()
                            .map(|v| v as usize)
                            .filter(|&v| self.field.bed[v] < 0.0),
                    );
                }
                vertices.sort_unstable();
                vertices.dedup();
                points.extend(vertices.into_iter().map(|v| self.field.mesh.directions[v]));
                // The prospective midpoints and centroid count even when a leaf stayed coarse:
                // two independently reached neighbours need never have tested their common edge.
                let [a, b, c] = self.field.mesh.node_vertices(node);
                let stencil = [
                    self.field.mesh.edge_center([a, b]),
                    self.field.mesh.edge_center([b, c]),
                    self.field.mesh.edge_center([c, a]),
                    self.field.mesh.node_center(node),
                ];
                let local = fine.sample_surface_batch(&stencil, &|| cancel.is_cancelled())?;
                points.extend(
                    stencil
                        .into_iter()
                        .zip(local)
                        .filter_map(|(p, s)| (s.bed_elevation_m < 0.0).then_some(p)),
                );
                Ok((!ports::connected(fine, &points, step, cancel)?).then_some(node))
            })
            .collect();
        Ok(results?.into_iter().flatten().collect())
    }
}

fn vertex_beds(
    fine: &FineTerrain,
    directions: &[[f64; 3]],
    cancel: &BuildCancellation,
) -> Result<Vec<f64>, AtlasGenerationError> {
    // A chunk owns a bounded evaluator scratch and never crosses threads.
    const BATCH: usize = 256;
    let mut bed = vec![0.0; directions.len()];
    #[cfg(not(target_arch = "wasm32"))]
    let chunks = bed.par_chunks_mut(BATCH).zip(directions.par_chunks(BATCH));
    #[cfg(target_arch = "wasm32")]
    let mut chunks = bed.chunks_mut(BATCH).zip(directions.chunks(BATCH));
    chunks.try_for_each(|(values, chunk)| {
        let samples = fine.sample_surface_batch(chunk, &|| cancel.is_cancelled())?;
        for (value, sample) in values.iter_mut().zip(samples) {
            *value = sample.bed_elevation_m;
        }
        Ok::<_, AtlasGenerationError>(())
    })?;
    Ok(bed)
}

/// A low with no marine path remains a dry marker even when its three support
/// vertices are positive. Outside the coastal working area the source-land
/// background supplies the dry boundary condition.
fn mark_inland_floors(
    fine: &FineTerrain,
    field: &WorkingBed<'_>,
    open: &[bool],
    floor: &mut [bool],
    cancel: &BuildCancellation,
) -> Result<(), AtlasGenerationError> {
    const BATCH: usize = 256;
    #[cfg(not(target_arch = "wasm32"))]
    let batches = floor.par_chunks_mut(BATCH);
    #[cfg(target_arch = "wasm32")]
    let batches = floor.chunks_mut(BATCH);
    batches.enumerate().try_for_each(|(batch, slots)| {
        check_cancelled(cancel)?;
        let mut points = Vec::new();
        let mut positions = Vec::new();
        for (j, slot) in slots.iter_mut().enumerate() {
            let i = batch * BATCH + j;
            if open[i] || *slot {
                continue;
            }
            let node = field.support.leaf_nodes[i];
            if node as usize == field.mesh.source_face(node) {
                *slot = true;
                continue;
            }
            points.push(field.mesh.node_center(node));
            positions.push(j);
        }
        let samples = fine.sample_surface_batch(&points, &|| cancel.is_cancelled())?;
        for (j, sample) in positions.into_iter().zip(samples) {
            slots[j] = sample.bed_elevation_m < 0.0;
        }
        Ok(())
    })
}

struct Adjacency<'a> {
    passages: &'a [Passage],
    offsets: Vec<usize>,
    edges: Vec<u32>,
}
impl<'a> Adjacency<'a> {
    fn new(
        passages: &'a [Passage],
        count: usize,
        cancel: &BuildCancellation,
    ) -> Result<Self, AtlasGenerationError> {
        let mut offsets = vec![0usize; count + 1];
        for (index, edge) in passages.iter().enumerate() {
            if index % 4096 == 0 {
                check_cancelled(cancel)?;
            }
            for &cell in &edge.cells {
                offsets[cell as usize + 1] += 1;
            }
        }
        for i in 1..offsets.len() {
            offsets[i] += offsets[i - 1];
        }
        let mut cursor = offsets[..count].to_vec();
        let mut adjacency = vec![0_u32; offsets[count]];
        for (index, edge) in passages.iter().enumerate() {
            if index % 4096 == 0 {
                check_cancelled(cancel)?;
            }
            let index = u32::try_from(index)
                .map_err(|_| AtlasGenerationError::Geometry("too many coastal passages".into()))?;
            for &cell in &edge.cells {
                let pos = &mut cursor[cell as usize];
                adjacency[*pos] = index;
                *pos += 1;
            }
        }
        Ok(Self {
            passages,
            offsets,
            edges: adjacency,
        })
    }
}

struct Inundation {
    reached: Vec<bool>,
    frontier: VecDeque<usize>,
    /// Completed passage observations, including successful openings between dry vertices.
    tested: Vec<(usize, bool)>,
}
impl Inundation {
    fn new(count: usize) -> Self {
        Self {
            reached: vec![false; count],
            frontier: VecDeque::new(),
            tested: Vec::new(),
        }
    }
    fn admit(&mut self, cell: usize) {
        if !self.reached[cell] {
            self.reached[cell] = true;
            self.frontier.push_back(cell);
        }
    }
    /// Drain the endpoint-connected frontier first, then check the actual interior
    /// bed of the remaining passages. A newly discovered submerged divide resumes
    /// this queue; each compartment enters it at most once in the entire build.
    fn flood(
        &mut self,
        adjacency: &Adjacency<'_>,
        bed: &[f64],
        cancel: &BuildCancellation,
        inspect: &(impl Fn(usize, &Passage) -> Result<bool, AtlasGenerationError> + Sync),
    ) -> Result<usize, AtlasGenerationError> {
        let mut inspected = 0;
        let mut fringe = Vec::new();
        while !self.frontier.is_empty() {
            fringe.clear();
            while let Some(i) = self.frontier.pop_front() {
                check_cancelled(cancel)?;
                for &k in &adjacency.edges[adjacency.offsets[i]..adjacency.offsets[i + 1]] {
                    let edge = &adjacency.passages[k as usize];
                    let [left, right] = edge.cells.map(|cell| cell as usize);
                    let j = if left == i { right } else { left };
                    if self.reached[j] {
                        continue;
                    }
                    if edge.vertices.iter().any(|&v| bed[v as usize] < 0.0) {
                        self.admit(j);
                    } else {
                        fringe.push((k, j));
                    }
                }
            }
            fringe.retain(|&(_, j)| !self.reached[j]);
            inspected += fringe.len();
            #[cfg(not(target_arch = "wasm32"))]
            let front = fringe.par_iter();
            #[cfg(target_arch = "wasm32")]
            let front = fringe.iter();
            let crossings: Result<Vec<_>, _> = front
                .map(|&(k, _)| inspect(k as usize, &adjacency.passages[k as usize]))
                .collect();
            for ((k, j), wet) in fringe.iter().copied().zip(crossings?) {
                self.tested.push((k as usize, wet));
                if wet {
                    self.admit(j);
                }
            }
        }
        Ok(inspected)
    }
}

fn edge_has_water(
    fine: &FineTerrain,
    a: [f64; 3],
    b: [f64; 3],
    spacing: f64,
    cancel: &BuildCancellation,
) -> Result<bool, AtlasGenerationError> {
    check_cancelled(cancel)?;
    let arc = BedArc::new([a, b]);
    let samples = arc.interior(fine, arc.intervals(spacing), cancel)?;
    Ok(samples.iter().any(|s| s.bed_elevation_m < 0.0))
}

struct BedArc {
    ends: [UnitVector3; 2],
    angle: f64,
}

impl BedArc {
    fn new(directions: [[f64; 3]; 2]) -> Self {
        let ends = directions
            .map(|[x, y, z]| UnitVector3::new(x, y, z).expect("finite coastal support vertex"));
        Self {
            ends,
            angle: central_angle(ends[0], ends[1]),
        }
    }

    fn direction(&self, t: f64) -> [f64; 3] {
        super::minor_arc_direction(self.ends[0], self.ends[1], self.angle, t)
    }

    fn intervals(&self, spacing: f64) -> usize {
        // Nest all coarser samples even when two callers request different resolutions.
        ((self.angle / spacing).ceil() as usize)
            .max(2)
            .next_power_of_two()
    }

    fn interior(
        &self,
        fine: &FineTerrain,
        n: usize,
        cancel: &BuildCancellation,
    ) -> Result<Vec<civ_atlas_terrain::SurfaceSample>, AtlasGenerationError> {
        if self.angle == 0.0 {
            return Ok(Vec::new());
        }
        let directions: Vec<_> = (1..n)
            .map(|i| self.direction(i as f64 / n as f64))
            .collect();
        Ok(fine.sample_surface_batch(&directions, &|| cancel.is_cancelled())?)
    }

    /// One actual submerged station per distinct measured wet interval, in arc order.
    fn wet_runs(
        &self,
        stations: impl IntoIterator<Item = (usize, f64)>,
        denominator: usize,
    ) -> Vec<[f64; 3]> {
        let mut result = Vec::new();
        let mut minimum: Option<(usize, f64)> = None;
        for (i, z) in stations {
            if z < 0.0 {
                if minimum.is_none_or(|(_, low)| z < low) {
                    minimum = Some((i, z));
                }
            } else if let Some((wet, _)) = minimum.take() {
                result.push(self.direction(wet as f64 / denominator as f64));
            }
        }
        if let Some((wet, _)) = minimum {
            result.push(self.direction(wet as f64 / denominator as f64));
        }
        result
    }

    fn divide_openings(
        &self,
        fine: &FineTerrain,
        end_beds: [f64; 2],
        native_step: f64,
        cancel: &BuildCancellation,
    ) -> Result<Vec<[f64; 3]>, AtlasGenerationError> {
        if self.angle == 0.0 {
            return Ok(Vec::new());
        }
        let factor = usize::from(ATLAS_COAST_CLEARANCE_SUBDIVISIONS);
        let n = self.intervals(native_step / factor as f64);
        let samples = self.interior(fine, n, cancel)?;
        // Extra samples bracket measured lows on separating ridges. The same exact
        // bed supplies these probes, so a narrow channel can reopen a candidate divide.
        let height = |i: usize| {
            if i == 0 {
                end_beds[0]
            } else if i == n {
                end_beds[1]
            } else {
                samples[i - 1].bed_elevation_m
            }
        };
        let is_low = |i: usize| {
            let z = height(i);
            let left = height(i.saturating_sub(1));
            let right = height((i + 1).min(n));
            z <= left && z <= right && (z < left || z < right)
        };
        // With an arcwise slope L, the two endpoint cones can reach zero only if
        // their heights sum to at most L times the interval width. The measured
        // steepest secant is an extra sampling indicator, not a global derivative bound.
        let largest_step = (0..n)
            .map(|i| (height(i + 1) - height(i)).abs())
            .fold(0.0, f64::max);
        let mut extra = Vec::new();
        for i in 0..n {
            if (height(i) >= 0.0 && is_low(i))
                || (height(i + 1) >= 0.0 && is_low(i + 1))
                || (height(i) >= 0.0
                    && height(i + 1) >= 0.0
                    && height(i) + height(i + 1) <= largest_step)
            {
                extra.extend((1..factor).map(|j| i * factor + j));
            }
        }
        let total = n * factor;
        let directions: Vec<_> = extra
            .iter()
            .map(|&j| self.direction(j as f64 / total as f64))
            .collect();
        let values = fine.sample_surface_batch(&directions, &|| cancel.is_cancelled())?;
        if samples.iter().all(|s| s.bed_elevation_m >= 0.0)
            && values.iter().all(|s| s.bed_elevation_m >= 0.0)
            && end_beds.iter().all(|&b| b >= 0.0)
        {
            return Ok(Vec::new());
        }
        let mut stations: Vec<_> = (0..=n).map(|i| (i * factor, height(i))).collect();
        stations.extend(
            extra
                .into_iter()
                .zip(values)
                .map(|(i, s)| (i, s.bed_elevation_m)),
        );
        stations.sort_unstable_by_key(|&(i, _)| i);
        Ok(self.wet_runs(stations, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::natural::ATLAS_FINE_SCALE;
    use civ_atlas_terrain::TerrainParams;

    #[test]
    #[ignore = "inspect seawater catchment boundaries on the reported coast world; Release only"]
    fn marine_boundary_clearance_probe() {
        let cancel = BuildCancellation::new();
        let params = TerrainParams {
            seed: 27_276_201,
            ..Default::default()
        };
        let (fine, _) = super::super::prepare_source(&params, ATLAS_FINE_SCALE, &cancel).unwrap();
        let (sites, _) = super::super::geometry::site_water(fine.coarse_terrain());
        let (map, mesh) = build_with_mesh(&fine, &sites, &cancel).unwrap();
        let (_, support) = mesh.assemble(&cancel).unwrap();
        let cuts: Vec<_> = support
            .passages
            .iter()
            .filter(|edge| {
                map.cell_is_marine(edge.cells[0] as usize)
                    != map.cell_is_marine(edge.cells[1] as usize)
            })
            .collect();
        // Compare the nominal author scale with the denser sampling of the original
        // 55.5 km witness. This measures aliasing of the new support, independently of
        // whether the old 183 pairs pass.
        for divisor in [1.0, 16.0, 64.0, 256.0] {
            let step = fine.canonical_angular_step_radians() / divisor;
            #[cfg(not(target_arch = "wasm32"))]
            let edges = cuts.par_iter();
            #[cfg(target_arch = "wasm32")]
            let edges = cuts.iter();
            let hits: Vec<_> = edges
                .filter(|edge| {
                    let [a, b] = edge.vertices.map(|v| support.directions[v as usize]);
                    edge_has_water(&fine, a, b, step, &cancel).unwrap()
                })
                .collect();
            let count = hits.len();
            eprintln!("coast_boundary_audit cuts={} native_step_divisor={divisor} submerged_cut_edges={count}",cuts.len());
            for edge in hits.iter().take(3) {
                let [a, b] = edge.vertices.map(|v| support.directions[v as usize]);
                let arc = BedArc::new([a, b]);
                let samples = arc.interior(&fine, arc.intervals(step), &cancel).unwrap();
                let low = samples
                    .iter()
                    .map(|s| s.bed_elevation_m)
                    .fold(f64::INFINITY, f64::min);
                eprintln!(
                    "unresolved divide cells={:?} ends={:?} minimum_m={low}",
                    edge.cells,
                    [a, b]
                );
            }
            assert_eq!(count, 0);
        }
    }
}
