//! Canonical bed quadrature and connected water on the declared support mesh.

use super::AtlasGenerationError;
use crate::engine::BuildCancellation;
use crate::generators::natural::hydrology::{
    priority_flood_from_terminals, HydrologyGenerationError,
};
use crate::generators::natural::surface_water_geometry::integrate_positive_linear_triangle;
use crate::generators::natural::{geometric_midpoint, subdivision_children_from_midpoints};
use crate::world::natural::atlas_terrain::{AtlasSupportFace, AtlasWaterSupport};
use crate::world::natural::{AtlasCellFields, AtlasLake, BedrockKind, CLIMATE_MONTH_COUNT};
use crate::world::spatial::{
    central_angle, spherical_triangle_area_unit, SphericalSurfaceSnapshot, UnitVector3,
};
use civ_atlas_terrain::FineTerrain;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

pub(super) type SupportTriangle = ([usize; 3], usize);
struct SampledSupport {
    directions: Vec<UnitVector3>,
    triangles: Vec<SupportTriangle>,
    bed: Vec<f64>,
}

pub(super) struct BuiltGeometry {
    pub support: AtlasWaterSupport,
    pub fields: AtlasCellFields,
    pub lakes: Vec<AtlasLake>,
    pub neighbors: SupportGraph,
    pub water: InitialWater,
}

// A fine support graph has millions of nodes; CSR avoids one allocation and
// retained Vec growth capacity per node. Neighbor order remains canonical.
pub(super) struct SupportGraph {
    offsets: Vec<usize>,
    neighbors: Vec<u32>,
}
impl SupportGraph {
    pub(super) fn from_edges(
        node_count: usize,
        edges: impl Iterator<Item = [usize; 2]>,
        cancellation: &BuildCancellation,
    ) -> Result<Self, AtlasGenerationError> {
        let mut unique = FxHashSet::default();
        for (index, [a, b]) in edges.enumerate() {
            if index % 256 == 0 {
                super::check_cancelled(cancellation)?;
            }
            unique.insert([a.min(b), a.max(b)]);
        }
        log::debug!(
            "Atlas topology nodes={node_count} unique_edges={}",
            unique.len()
        );
        let mut offsets = vec![0; node_count + 1];
        for &[a, b] in &unique {
            offsets[a + 1] += 1;
            offsets[b + 1] += 1;
        }
        for i in 0..node_count {
            offsets[i + 1] += offsets[i];
        }
        let mut neighbors = vec![0; offsets[node_count]];
        let mut cursor = offsets[..node_count].to_vec();
        for [a, b] in unique {
            neighbors[cursor[a]] = u32::try_from(b).expect("bounded support node ID");
            cursor[a] += 1;
            neighbors[cursor[b]] = u32::try_from(a).expect("bounded support node ID");
            cursor[b] += 1;
        }
        for i in 0..node_count {
            if i % 256 == 0 {
                super::check_cancelled(cancellation)?;
            }
            neighbors[offsets[i]..offsets[i + 1]].sort_unstable();
        }
        Ok(Self { offsets, neighbors })
    }
    pub(super) fn neighbors(&self, node: usize) -> &[u32] {
        &self.neighbors[self.offsets[node]..self.offsets[node + 1]]
    }
}

pub(super) fn build_geometry(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    depth: u8,
    cancellation: &BuildCancellation,
) -> Result<BuiltGeometry, AtlasGenerationError> {
    let (mut directions, mut triangles) = source_fans(surface);
    for _ in 0..depth {
        guard_support_budget(triangles.len() * 4)?;
        triangles = refine_triangles(&mut directions, triangles, cancellation)?;
    }
    let sample_count = directions.len();
    let mut bed = Vec::with_capacity(sample_count);
    // Bounded query scratch; no global fine raster and no per-point noise cache.
    for chunk in directions.chunks(4096) {
        let query = chunk.iter().map(|p| p.components()).collect::<Vec<_>>();
        bed.extend(
            fine.sample_surface_batch(&query, &|| cancellation.is_cancelled())?
                .into_iter()
                .map(|s| s.bed_elevation_m),
        );
    }
    assemble_geometry(
        fine,
        surface,
        SampledSupport {
            directions,
            triangles,
            bed,
        },
        cancellation,
    )
}

/// Resolve lake pools, controlling spills and coastal mixed faces at the fixed source
/// angular interval. No vertex-sign early exit is used inside a selected source
/// cell. New connected pool cells are included before publication.
pub(super) fn refine_water_support(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    geometry: BuiltGeometry,
    cancellation: &BuildCancellation,
) -> Result<BuiltGeometry, AtlasGenerationError> {
    let mut active = vec![false; surface.cells().len()];
    let terrain = fine.coarse_terrain();
    for prior in fine.lake_priors() {
        active[prior.site] = true;
        for &neighbor in &terrain.site_neighbors()[terrain.site_neighbor_offsets()[prior.site]
            ..terrain.site_neighbor_offsets()[prior.site + 1]]
        {
            active[neighbor] = true;
        }
    }
    mark_lakes_and_spills(fine, &geometry, &mut active, cancellation)?;
    let BuiltGeometry {
        support,
        fields,
        lakes,
        neighbors,
        water,
    } = geometry;
    drop((fields, lakes, neighbors, water));
    let AtlasWaterSupport {
        mut directions,
        mut bed,
        faces,
        ..
    } = support;
    let mut red = super::red_refinement::RedRefinement::new(
        faces
            .into_iter()
            .map(|f| (f.node_indices(), f.owner_index()))
            .collect(),
        &directions,
        fine.canonical_angular_step_radians(),
    )?;
    let spacing = fine.canonical_angular_step_radians();
    loop {
        let mesh = resolve_red_geometry(
            fine,
            &mut red,
            &mut directions,
            &mut bed,
            (&active, cancellation),
        )?;
        log::debug!(
            "Atlas reconstruct nodes={} faces={}",
            directions.len(),
            mesh.triangles.len()
        );
        let candidate = assemble_geometry(
            fine,
            surface,
            SampledSupport {
                directions,
                triangles: mesh.triangles,
                bed,
            },
            cancellation,
        )?;
        mark_lakes_and_spills(fine, &candidate, &mut active, cancellation)?;
        let mut requested = vec![false; red.leaf_count()];
        for (face, &parent) in candidate.support.faces.iter().zip(&mesh.parents) {
            if active[face.owner_index()]
                && triangle_exceeds(face.node_indices(), &candidate.support.directions, spacing)
            {
                requested[parent] = true;
            }
        }
        if !requested.iter().any(|&v| v) {
            return Ok(candidate);
        }
        let BuiltGeometry {
            support,
            fields,
            lakes,
            neighbors,
            water,
        } = candidate;
        drop((fields, lakes, neighbors, water));
        directions = support.directions;
        bed = support.bed;
        red.refine(&mut directions, &requested, cancellation)?;
    }
}

fn triangle_exceeds(nodes: [usize; 3], directions: &[UnitVector3], spacing: f64) -> bool {
    (0..3).any(|i| central_angle(directions[nodes[i]], directions[nodes[(i + 1) % 3]]) > spacing)
}

fn resolve_red_geometry(
    fine: &FineTerrain,
    red: &mut super::red_refinement::RedRefinement,
    directions: &mut Vec<UnitVector3>,
    bed: &mut Vec<f64>,
    inputs: (&[bool], &BuildCancellation),
) -> Result<super::red_refinement::ConformingMesh, AtlasGenerationError> {
    let (active, cancellation) = inputs;
    let spacing = fine.canonical_angular_step_radians();
    loop {
        super::check_cancelled(cancellation)?;
        for chunk in directions[bed.len()..].chunks(4096) {
            bed.extend(
                fine.sample_surface_batch(
                    &chunk.iter().map(|p| p.components()).collect::<Vec<_>>(),
                    &|| cancellation.is_cancelled(),
                )?
                .into_iter()
                .map(|s| s.bed_elevation_m),
            );
        }
        let mut requested = vec![false; red.leaf_count()];
        red.visit_unresolved(|parent, (nodes, owner)| {
            if (active[owner] || red.inherited_resolution(parent) || crosses_sea_level(nodes, bed))
                && triangle_exceeds(nodes, directions, spacing)
            {
                requested[parent] = true;
            }
        });
        let count = requested.iter().filter(|&&v| v).count();
        log::debug!(
            "Atlas red parents={} nodes={} requested={count}",
            red.leaf_count(),
            directions.len()
        );
        if count == 0 {
            return red.conforming_mesh();
        }
        red.refine(directions, &requested, cancellation)?;
    }
}

fn crosses_sea_level(nodes: [usize; 3], bed: &[f64]) -> bool {
    nodes.iter().any(|&i| bed[i] < 0.0) && nodes.iter().any(|&i| bed[i] >= 0.0)
}

fn mark_lakes_and_spills(
    fine: &FineTerrain,
    geometry: &BuiltGeometry,
    active: &mut [bool],
    cancellation: &BuildCancellation,
) -> Result<(), AtlasGenerationError> {
    let mut controls_spill = vec![false; geometry.support.bed.len()];
    for prior in fine.lake_priors() {
        let mut node = prior.site;
        let spill = geometry.water.spill[node].0;
        while !geometry.water.ocean[node] {
            if geometry.support.bed[node] == spill {
                controls_spill[node] = true;
            }
            let next = geometry
                .neighbors
                .neighbors(node)
                .iter()
                .map(|&j| j as usize)
                .filter(|&j| {
                    geometry.water.rank[j] < geometry.water.rank[node]
                        && geometry.water.spill[j] <= geometry.water.spill[node]
                })
                .min_by_key(|&j| (geometry.water.spill[j], geometry.water.rank[j]))
                .ok_or_else(|| {
                    AtlasGenerationError::Geometry("spill path is disconnected".into())
                })?;
            node = next;
        }
    }
    for (index, face) in geometry.support.faces.iter().enumerate() {
        if index % 256 == 0 {
            super::check_cancelled(cancellation)?;
        }
        if face.lake.is_some() || face.node_indices().iter().any(|&i| controls_spill[i]) {
            active[face.owner_index()] = true;
        }
    }
    Ok(())
}

pub(super) fn guard_support_budget(face_count: usize) -> Result<(), AtlasGenerationError> {
    use crate::world::CellId;
    use std::mem::size_of;
    // Closed triangular sphere: F=2V-4; its CSR contains 3F directed arcs.
    // Bound construction and streaming current routing separately, including
    // initialized geometry, flood evidence, CSR, queue/order and flow buffers.
    let nodes = face_count / 2 + 2;
    let node_bytes = size_of::<UnitVector3>()
        + size_of::<f64>()
        + size_of::<bool>()
        + size_of::<Option<f64>>()
        + size_of::<Option<usize>>()
        + size_of::<OrderedHeight>()
        + size_of::<u32>()
        + size_of::<usize>();
    let routing_bytes = nodes as u64
        * (node_bytes
            + 2 * size_of::<f64>()
            + size_of::<Option<CellId>>()
            + 3 * size_of::<CellId>()) as u64
        + face_count as u64 * (size_of::<AtlasSupportFace>() + 3 * size_of::<u32>()) as u64;
    let construction_bytes = nodes as u64 * node_bytes as u64
        + face_count as u64
            * (size_of::<AtlasSupportFace>()
                + 3 * size_of::<u32>()
                + size_of::<SupportTriangle>()
                + 3 * size_of::<bool>()) as u64;
    let bytes = construction_bytes.max(routing_bytes);
    let maximum = crate::world::natural::atlas_terrain::ATLAS_SUPPORT_WORKSPACE_BYTES_MAX;
    if bytes > maximum {
        return Err(AtlasGenerationError::SupportBudget {
            required: bytes,
            maximum,
        });
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn triangle_sample_direction(corners: [UnitVector3; 3]) -> UnitVector3 {
    let p = corners.map(UnitVector3::components);
    let center = crate::world::spatial::add(crate::world::spatial::add(p[0], p[1]), p[2]);
    UnitVector3::new(center[0], center[1], center[2])
        .expect("a spherical fan triangle lies within one hemisphere")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refining_a_green_child_promotes_its_original_parent() {
        let mut points = [
            [-0.01, 0.0, 1.0],
            [0.01, 0.0, 1.0],
            [0.01, 0.02, 1.0],
            [0.03, 0.02, 1.0],
        ]
        .map(|p| UnitVector3::new(p[0], p[1], p[2]).unwrap())
        .to_vec();
        let mut red = super::super::red_refinement::RedRefinement::new(
            vec![([0, 1, 2], 0), ([2, 1, 3], 1)],
            &points,
            0.002,
        )
        .unwrap();
        red.refine(&mut points, &[false, true], &BuildCancellation::new())
            .unwrap();
        let mesh = red.conforming_mesh().unwrap();
        let child = mesh
            .triangles
            .iter()
            .position(|(nodes, owner)| *owner == 0 && !(nodes.contains(&0) && nodes.contains(&1)))
            .unwrap();
        let mut requested = vec![false; red.leaf_count()];
        requested[mesh.parents[child]] = true;
        red.refine(&mut points, &requested, &BuildCancellation::new())
            .unwrap();
        let second = red.conforming_mesh().unwrap();
        assert!(!second
            .triangles
            .iter()
            .any(|(nodes, owner)| *owner == 0 && nodes.contains(&0) && nodes.contains(&1)));
        let target = 0.002;
        for _ in 0..8 {
            let mesh = red.conforming_mesh().unwrap();
            let mut requested = vec![false; red.leaf_count()];
            for (&(nodes, _), &parent) in mesh.triangles.iter().zip(&mesh.parents) {
                if triangle_exceeds(nodes, &points, target) {
                    requested[parent] = true;
                }
            }
            if !requested.iter().any(|&v| v) {
                let mut edges = FxHashMap::<[usize; 2], usize>::default();
                let mut used = FxHashSet::default();
                for (nodes, _) in &mesh.triangles {
                    used.extend(nodes.iter().copied());
                    for i in 0..3 {
                        let a = nodes[i];
                        let b = nodes[(i + 1) % 3];
                        *edges.entry([a.min(b), a.max(b)]).or_default() += 1;
                    }
                }
                assert!(edges.values().all(|&count| count <= 2));
                assert_eq!(used.len() + mesh.triangles.len(), edges.len() + 1);
                return;
            }
            red.refine(&mut points, &requested, &BuildCancellation::new())
                .unwrap();
        }
        panic!("local supported spacing must close through red parents");
    }
}

pub(super) fn source_fans(
    surface: &SphericalSurfaceSnapshot,
) -> (Vec<UnitVector3>, Vec<SupportTriangle>) {
    let n = surface.cells().len();
    let mut directions = surface.cells().iter().map(|c| c.site).collect::<Vec<_>>();
    directions.extend(surface.vertices().iter().map(|v| v.position));
    let mut triangles = Vec::with_capacity(surface.edges().len() * 2);
    for cell in surface.cells() {
        for side in 0..cell.boundary_vertices.len() {
            triangles.push((
                [
                    cell.id.raw() as usize,
                    n + cell.boundary_vertices[side].raw() as usize,
                    n + cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()].raw()
                        as usize,
                ],
                cell.id.raw() as usize,
            ));
        }
    }
    (directions, triangles)
}

fn assemble_geometry(
    fine: &FineTerrain,
    surface: &SphericalSurfaceSnapshot,
    sampled: SampledSupport,
    cancellation: &BuildCancellation,
) -> Result<BuiltGeometry, AtlasGenerationError> {
    let SampledSupport {
        directions,
        triangles,
        bed,
    } = sampled;
    let n = surface.cells().len();
    let sample_count = directions.len();
    let neighbors = SupportGraph::from_edges(
        sample_count,
        triangles
            .iter()
            .flat_map(|(nodes, _)| (0..3).map(move |i| [nodes[i], nodes[(i + 1) % 3]])),
        cancellation,
    )?;
    let radius2 = surface.radius().get().powi(2);
    let priors = fine
        .lake_priors()
        .iter()
        .map(|p| (p.site, p.surface_elevation_m))
        .collect::<Vec<_>>();
    let mut marine_seeds = vec![false; sample_count];
    for (i, &kind) in fine
        .coarse_terrain()
        .water_classification()
        .iter()
        .enumerate()
    {
        marine_seeds[i] = kind == 1;
    }
    let water = initialize_water(&bed, &neighbors, &marine_seeds, &priors, cancellation)?;
    log::debug!("Atlas water initialized nodes={sample_count}");
    let lake_count = water
        .lake
        .iter()
        .flatten()
        .copied()
        .max()
        .map_or(0, |i| i + 1);
    let mut lakes = vec![
        AtlasLake {
            surface_elevation_m: 0.0,
            spill_elevation_m: f64::INFINITY,
            area_m2: 0.0,
            volume_m3: 0.0,
            resolved_monthly_inflow_m3_s: [0.0; CLIMATE_MONTH_COUNT],
            resolved_monthly_outflow_m3_s: [0.0; CLIMATE_MONTH_COUNT],
            direct_monthly_precipitation_m3_s: [0.0; CLIMATE_MONTH_COUNT]
        };
        lake_count
    ];
    for (i, lake) in water.lake.iter().enumerate() {
        if let Some(lake) = *lake {
            lakes[lake].surface_elevation_m = water.level[i].expect("lake has constant surface");
            lakes[lake].spill_elevation_m = lakes[lake].spill_elevation_m.min(water.spill[i].0);
        }
    }
    let mut cell_area = vec![0.0; n];
    let mut cell_bed = vec![0.0; n];
    let mut cell_ocean_area = vec![0.0; n];
    let mut cell_lake_area = vec![0.0; n];
    let mut cell_dry_area = vec![0.0; n];
    let mut ocean_volume_m3 = vec![0.0; n];
    let mut lake_volume_m3 = vec![0.0; n];
    let mut faces = Vec::with_capacity(triangles.len());
    let mut cell_face_offsets = vec![0; n + 1];
    for (index, (nodes, owner)) in triangles.into_iter().enumerate() {
        if index % 256 == 0 {
            super::check_cancelled(cancellation)?;
        }
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
        let ocean_fraction = if ocean { wet } else { 0.0 };
        let lake_fraction = if lake.is_some() { wet } else { 0.0 };
        if ocean && lake.is_some() {
            return Err(AtlasGenerationError::Geometry(
                "lake and ocean share a support face".into(),
            ));
        }
        cell_area[owner] += area;
        cell_bed[owner] += area * nodes.map(|i| bed[i]).iter().sum::<f64>() / 3.0;
        cell_ocean_area[owner] += area * ocean_fraction;
        cell_lake_area[owner] += area * lake_fraction;
        cell_dry_area[owner] += area * (1.0 - wet);
        if ocean {
            ocean_volume_m3[owner] += area * depth_integral;
        }
        if let Some(lake) = lake {
            lake_volume_m3[owner] += area * depth_integral;
            lakes[lake].area_m2 += area * wet;
            lakes[lake].volume_m3 += area * depth_integral;
        }
        cell_face_offsets[owner + 1] += 1;
        faces.push(AtlasSupportFace {
            nodes: nodes.map(|id| u32::try_from(id).expect("bounded support node ID")),
            owner: u32::try_from(owner).expect("bounded source cell ID"),
            area_m2: area,
            ocean_fraction,
            lake_fraction,
            water_level_m: level,
            lake,
        });
    }
    for i in 0..n {
        cell_face_offsets[i + 1] += cell_face_offsets[i];
    }
    let average = |values: &[f64]| {
        values
            .iter()
            .zip(&cell_area)
            .map(|(v, a)| (v / a) as f32)
            .collect()
    };
    let exposed_elevation_m = (0..n)
        .map(|i| ((cell_bed[i] + ocean_volume_m3[i] + lake_volume_m3[i]) / cell_area[i]) as f32)
        .collect();
    let relative_permeability = fine
        .coarse_terrain()
        .continental_origin()
        .iter()
        .map(|&c| {
            (if c == 0 {
                BedrockKind::OceanicMafic
            } else {
                BedrockKind::ContinentalCrystalline
            })
            .base_transport_properties()
            .1
        })
        .collect();
    let fields = AtlasCellFields {
        land_ocean: crate::world::natural::LandOceanField::from_kinds(
            water.ocean[..n]
                .iter()
                .map(|&ocean| {
                    if ocean {
                        crate::world::natural::LandOceanKind::Ocean
                    } else {
                        crate::world::natural::LandOceanKind::Land
                    }
                })
                .collect(),
        ),
        bed_elevation_m: average(&cell_bed),
        exposed_elevation_m,
        ocean_fraction: average(&cell_ocean_area),
        lake_fraction: average(&cell_lake_area),
        dry_fraction: average(&cell_dry_area),
        ocean_volume_m3,
        lake_volume_m3,
        relative_permeability,
    };
    let support = AtlasWaterSupport {
        directions,
        bed,
        faces,
        cell_face_offsets,
    };
    Ok(BuiltGeometry {
        support,
        fields,
        lakes,
        neighbors,
        water,
    })
}

pub(super) fn refine_triangles(
    directions: &mut Vec<UnitVector3>,
    triangles: Vec<SupportTriangle>,
    cancellation: &BuildCancellation,
) -> Result<Vec<SupportTriangle>, AtlasGenerationError> {
    super::check_cancelled(cancellation)?;
    let mut midpoints = FxHashMap::default();
    let mut refined = Vec::with_capacity(triangles.len() * 4);
    for (index, ([a, b, c], owner)) in triangles.into_iter().enumerate() {
        if index % 256 == 0 {
            super::check_cancelled(cancellation)?;
        }
        let mut midpoint = |a: usize, b: usize| {
            let key = [a.min(b), a.max(b)];
            *midpoints.entry(key).or_insert_with(|| {
                let id = directions.len();
                directions.push(geometric_midpoint(directions[a], directions[b]));
                id
            })
        };
        let ab = midpoint(a, b);
        let bc = midpoint(b, c);
        let ca = midpoint(c, a);
        refined.extend(
            subdivision_children_from_midpoints([a, b, c], [ab, bc, ca])
                .map(|nodes| (nodes, owner)),
        );
    }
    Ok(refined)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct OrderedHeight(pub f64);
impl Eq for OrderedHeight {}
impl PartialOrd for OrderedHeight {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrderedHeight {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

pub(super) struct InitialWater {
    pub ocean: Vec<bool>,
    pub level: Vec<Option<f64>>,
    pub lake: Vec<Option<usize>>,
    pub spill: Vec<OrderedHeight>,
    pub rank: Vec<u32>,
    pub lowered_priors: usize,
}

/// Original marine sites are source anchors, never final water masks. Flood only
/// their actual connected B < 0 support components; independent source seas survive,
/// while new unseeded negative depressions remain inland.
/// Lake initialization is the union of all reachable, spill-limited prior pools.
/// The highest reachable constant surface wins; nested pools are counted once.
pub(super) fn initialize_water(
    bed: &[f64],
    neighbors: &SupportGraph,
    marine_seeds: &[bool],
    priors: &[(usize, f64)],
    cancellation: &BuildCancellation,
) -> Result<InitialWater, HydrologyGenerationError> {
    let n = bed.len();
    let mut ocean = marine_seeds
        .iter()
        .zip(bed)
        .map(|(&seed, &bed)| seed && bed < 0.0)
        .collect::<Vec<_>>();
    let mut queue = ocean
        .iter()
        .enumerate()
        .filter_map(|(i, &wet)| wet.then_some(i))
        .collect::<VecDeque<_>>();
    if queue.is_empty() {
        return Err(HydrologyGenerationError::MissingClosedTerminal);
    }
    let mut steps = 0;
    while let Some(i) = queue.pop_front() {
        steps += 1;
        if steps % 256 == 0 && cancellation.is_cancelled() {
            return Err(HydrologyGenerationError::Cancelled);
        }
        for &j in neighbors.neighbors(i) {
            let j = j as usize;
            if !ocean[j] && bed[j] < 0.0 {
                ocean[j] = true;
                queue.push_back(j);
            }
        }
    }
    let heights = bed.iter().copied().map(OrderedHeight).collect::<Vec<_>>();
    let (spill, rank) = priority_flood_from_terminals(
        &heights,
        &ocean,
        |i| neighbors.neighbors(i).iter().map(|&j| j as usize),
        Some(cancellation),
    )?;
    log::debug!("Atlas minimax flood complete nodes={n}");
    let mut level = vec![None; n];
    let mut heap = BinaryHeap::new();
    let mut lowered_priors = 0;
    for &(site, original) in priors {
        let water = original.min(spill[site].0);
        lowered_priors += usize::from(water < original);
        if !ocean[site] && water > bed[site] {
            heap.push((OrderedHeight(water), site));
        }
    }
    let mut steps = 0;
    while let Some((water, i)) = heap.pop() {
        steps += 1;
        if steps % 256 == 0 && cancellation.is_cancelled() {
            return Err(HydrologyGenerationError::Cancelled);
        }
        if level[i].is_some_and(|old| old >= water.0) {
            continue;
        }
        level[i] = Some(water.0);
        for &j in neighbors.neighbors(i) {
            let j = j as usize;
            if !ocean[j] && bed[j] < water.0 && level[j].is_none_or(|old| old < water.0) {
                heap.push((water, j));
            }
        }
    }
    let mut lake = vec![None; n];
    let mut count = 0;
    for start in 0..n {
        if start % 256 == 0 && cancellation.is_cancelled() {
            return Err(HydrologyGenerationError::Cancelled);
        }
        if lake[start].is_some() || level[start].is_none() {
            continue;
        }
        lake[start] = Some(count);
        let mut queue = VecDeque::from([start]);
        while let Some(i) = queue.pop_front() {
            for &j in neighbors.neighbors(i) {
                let j = j as usize;
                if lake[j].is_none() && level[j] == level[start] {
                    lake[j] = Some(count);
                    queue.push_back(j);
                }
            }
        }
        count += 1;
    }
    Ok(InitialWater {
        ocean,
        level,
        lake,
        spill,
        rank,
        lowered_priors,
    })
}
