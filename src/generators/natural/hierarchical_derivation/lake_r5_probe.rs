//! R5 falsification experiment, not a replacement lake classifier.
//! Reuses production geometry, uncarved values, connectivity and P1 integration.
//! Two reported seed-42 defects and one normal control justify the fixtures here.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::app::{build_spherical_formation_external_artifacts, default_spherical_space_spec};
use crate::engine::{BuildCancellation, BuildEngine, MemoryStageCache};
use crate::generators::natural::surface_water_geometry::build_surface_water_working_geometry;
use crate::generators::natural::{
    causal_natural_formation_graph, FormationDerivationInputs, NaturalFormationBundleArtifact,
};
use crate::generators::spatial::ProfileSurfaceBuilder;
use crate::world::natural::{
    GeologicSpec, NaturalQualityProfile, ReliefSpec, TectonicSpec, WorldFormationSpec,
};
use crate::world::spatial::{spherical_triangle_area_unit, SphericalSurfaceSnapshot};
use crate::world::RootSeed;

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

struct Leaf {
    key: PrimitiveKey,
    // Production face-mean sampling, not an all-points-below-H predicate.
    below: bool,
    area: f64,
    neighbours: [Option<usize>; 3],
}

fn enumerate(key: PrimitiveKey, depth: u8, keys: &mut Vec<PrimitiveKey>) {
    if depth == 0 {
        keys.push(key);
        return;
    }
    for child in 0..4 {
        enumerate(key.child(child), depth - 1, keys);
    }
}

/// Two rings cover the production lake candidate domain and its dry side.
fn region(engine: &HierarchicalEvaluator, lake: &Lake) -> Vec<u32> {
    let mut cells: HashSet<u32> = lake.cells().iter().map(|cell| cell.raw()).collect();
    for _ in 0..2 {
        let ring: Vec<u32> = cells.iter().copied().collect();
        for cell in ring {
            let (start, len) = engine.ring_bounds(cell);
            cells.extend(&engine.ring_neighbors[start..start + len]);
        }
    }
    let mut cells: Vec<u32> = cells.into_iter().collect();
    cells.sort_unstable();
    cells
}

/// The least same-cell closure of the existing wet set. Each vertex is
/// enqueued at most once; traversal order cannot change reachability.
fn close_in_cell(leaves: &[Leaf], original: &[bool]) -> Vec<bool> {
    let mut water = original.to_vec();
    let mut queue: VecDeque<usize> = water
        .iter()
        .enumerate()
        .filter_map(|(index, &wet)| wet.then_some(index))
        .collect();
    while let Some(index) = queue.pop_front() {
        for next in leaves[index].neighbours.into_iter().flatten() {
            if !water[next] && leaves[next].below && leaves[index].key.cell == leaves[next].key.cell
            {
                water[next] = true;
                queue.push_back(next);
            }
        }
    }
    water
}

#[derive(Default)]
struct Shore {
    area: f64,
    edges: usize,
    cell_edges: usize,
    low_cross_cell: usize,
    in_cell_fronts: usize,
    open_boundary: usize,
}

fn measure(leaves: &[Leaf], wet: &[bool]) -> Shore {
    let mut stats = Shore::default();
    for (index, leaf) in leaves.iter().enumerate().filter(|(index, _)| wet[*index]) {
        stats.area += leaf.area;
        for next in leaf.neighbours {
            let Some(next) = next else {
                stats.open_boundary += 1;
                continue;
            };
            if wet[next] {
                continue;
            }
            stats.edges += 1;
            let cross_cell = leaf.key.cell != leaves[next].key.cell;
            stats.cell_edges += usize::from(cross_cell);
            stats.low_cross_cell += usize::from(cross_cell && leaves[next].below);
            stats.in_cell_fronts += usize::from(!cross_cell && leaves[next].below);
        }
        assert!(
            leaf.below,
            "wet leaf {index} must lie below the lake surface"
        );
    }
    stats
}

fn report_lake(
    surface: &SphericalSurfaceSnapshot,
    artifact: &NaturalFormationBundleArtifact,
    engine: &HierarchicalEvaluator,
    slot: u32,
) {
    let formation = artifact.bundle().surface_formation();
    let lake = &formation.hydrology().lakes()[slot as usize];
    let h = lake.surface_elevation_m();
    let elevations: Vec<f64> = formation
        .terrain_fields()
        .current_elevation_m()
        .iter()
        .copied()
        .map(f64::from)
        .collect();
    let p1 = build_surface_water_working_geometry(
        surface,
        &elevations,
        f64::from(h),
        &BuildCancellation::new(),
    )
    .expect("production P1 integration validates");
    let p1_area: f64 = lake
        .cells()
        .iter()
        .map(|cell| {
            let index = cell.raw() as usize;
            p1.ocean_area_fraction()[index] * surface.cells()[index].area.get()
        })
        .sum();
    let lake_area_m2 = lake.area_km2() * 1_000_000.0;
    let radius_squared = surface.radius().get().powi(2);
    let cells = region(engine, lake);
    eprintln!(
        "R5 lake={slot} H={h} P5_area_km2={} P1_members_over_P5={:.9} region_cells={}",
        lake.area_km2(),
        p1_area / lake_area_m2,
        cells.len()
    );
    let mut previous = HashSet::new();
    let mut previous_area = None;
    for level in 1..=6 {
        let start = Instant::now();
        let mut keys = Vec::new();
        for &cell in &cells {
            for sector in 0..engine.sector_count(CellId::from_raw(cell)) as u8 {
                enumerate(PrimitiveKey::new(cell, sector, &[]), level - 1, &mut keys);
            }
        }
        let index: FxHashMap<PrimitiveKey, usize> = keys
            .iter()
            .enumerate()
            .map(|(index, &key)| (key, index))
            .collect();
        let mut walk = LakeWalk::new(engine);
        let mut original = Vec::with_capacity(keys.len());
        let mut leaves = Vec::with_capacity(keys.len());
        for &key in &keys {
            let frame = walk.frame(key);
            let corners = frame.corners.map(|anchor| anchor.position);
            original.push(walk.is_lake_water(slot, key));
            leaves.push(Leaf {
                key,
                below: frame_uncarved_m(&frame) < h,
                area: spherical_triangle_area_unit(corners[0], corners[1], corners[2])
                    * radius_squared,
                neighbours: [None; 3],
            });
        }
        let sampling_ms = start.elapsed().as_secs_f64() * 1_000.0;
        let start = Instant::now();
        for leaf in &mut leaves {
            leaf.neighbours = std::array::from_fn(|edge| {
                index
                    .get(&engine.edge_neighbour(leaf.key, edge as u8).0)
                    .copied()
            });
        }
        let adjacency_ms = start.elapsed().as_secs_f64() * 1_000.0;
        let start = Instant::now();
        let closed = close_in_cell(&leaves, &original);
        let closure_ms = start.elapsed().as_secs_f64() * 1_000.0;
        assert!(original.iter().zip(&closed).all(|(&a, &b)| !a || b));
        let base = measure(&leaves, &original);
        let closure = measure(&leaves, &closed);
        assert_eq!(closure.in_cell_fronts, 0, "BFS must reach its fixed point");
        assert_eq!(
            closure.open_boundary, 0,
            "region must contain all wet edges"
        );
        let new_parent_area: f64 = leaves
            .iter()
            .zip(&closed)
            .filter(|(leaf, wet)| **wet && level > 1 && !previous.contains(&leaf.key.parent()))
            .map(|(leaf, _)| leaf.area)
            .sum();
        let dried_parent_area: f64 = leaves
            .iter()
            .zip(&closed)
            .filter(|(leaf, wet)| !**wet && level > 1 && previous.contains(&leaf.key.parent()))
            .map(|(leaf, _)| leaf.area)
            .sum();
        let ratio = previous_area.map_or(f64::NAN, |area| closure.area / area);
        // Counts accompany fractions: a smaller denominator can increase a
        // fraction even when fewer bad edges remain.
        for (name, stats) in [("production", &base), ("closure", &closure)] {
            let n = stats.edges as f64;
            eprintln!(
                "R5 lake={slot} L{level} {name} area_over_P5={:.9} edges={} \
                 cell_edges={} ({:.6}) low_cross_cell={} ({:.6}) fronts={} ({:.6})",
                stats.area / lake_area_m2,
                stats.edges,
                stats.cell_edges,
                stats.cell_edges as f64 / n,
                stats.low_cross_cell,
                stats.low_cross_cell as f64 / n,
                stats.in_cell_fronts,
                stats.in_cell_fronts as f64 / n,
            );
        }
        eprintln!(
            "R5 lake={slot} L{level} leaves={} sample_ms={sampling_ms:.3} \
             adjacency_ms={adjacency_ms:.3} closure_ms={closure_ms:.3} \
             closure_adjacent_area_ratio={ratio:.9} new_parent_area_over_P5={:.9} \
             dried_parent_area_over_P5={:.9} leaf_vector_bytes={}",
            leaves.len(),
            new_parent_area / lake_area_m2,
            dried_parent_area / lake_area_m2,
            leaves.capacity() * std::mem::size_of::<Leaf>(),
        );
        previous = leaves
            .iter()
            .zip(&closed)
            .filter_map(|(leaf, &wet)| wet.then_some(leaf.key))
            .collect();
        previous_area = Some(closure.area);
    }
}

fn probe(profile: NaturalQualityProfile, slots: &[u32]) {
    let (surface, artifact) = fixture(profile);
    let bundle = artifact.bundle();
    let engine = HierarchicalEvaluator::from_formation_product(
        FormationDerivationInputs {
            surface: &surface,
            compatibility: bundle.tectonics().compatibility(),
            substrate: bundle.substrate(),
            formation: bundle.surface_formation(),
            climate: bundle.climate(),
        },
        RootSeed::new(42),
    )
    .expect("production evaluator validates");
    eprintln!(
        "R5 profile={profile:?} derivation_version={}",
        super::super::T1_DERIVATION_VERSION
    );
    for &slot in slots {
        report_lake(&surface, &artifact, &engine, slot);
    }
}

#[test]
#[ignore = "R5 offline: Draft lake 39 front and lake 33 normal control; Release, exclusive slot"]
fn r5_frontier_probe_draft() {
    probe(NaturalQualityProfile::Draft, &[33, 39]);
}

#[test]
#[ignore = "R5 offline falsification: reproduced Standard lake 55 only; Release, exclusive slot"]
fn r5_frontier_probe_standard() {
    probe(NaturalQualityProfile::Standard, &[55]);
}
