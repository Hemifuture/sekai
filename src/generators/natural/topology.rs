#![cfg_attr(not(test), allow(dead_code))]

use super::morphology::arrival::{assign_arrivals_bounded, ArrivalSource, ArrivalWorkspace};
use super::morphology::metric::PositiveEdgeMetric;
use crate::world::spatial::{NaturalSurface, PlanarNaturalSurface, SpatialSnapshot};
use crate::world::{CellId, EdgeId};

const LENGTH_QUANTIZATION: f64 = 1_000_000.0;
const CENTER_QUANTIZATION: f64 = 1_000_000.0;
const AREA_QUANTIZATION: f64 = 1_000_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NeighborArc {
    pub(super) neighbor: CellId,
    pub(super) edge: EdgeId,
    pub(super) traversal_cost: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct NaturalTopologyIndex {
    arcs: Vec<Vec<NeighborArc>>,
    edge_owners: Vec<[Option<CellId>; 2]>,
    edge_traversal_costs: Vec<u64>,
    quantized_shape_positions: Vec<[i64; 3]>,
    area_weights: Vec<u64>,
    boundary_cells: Vec<bool>,
    minimum_dimension_m: f64,
    maximum_dimension_m: f64,
}

impl NaturalTopologyIndex {
    pub(super) fn new(spatial: &SpatialSnapshot) -> Self {
        Self::from_surface(&PlanarNaturalSurface::from_validated(spatial))
    }

    pub(super) fn from_surface(surface: &impl NaturalSurface) -> Self {
        let cell_count = surface.cell_count();
        let mut arcs = vec![Vec::new(); cell_count];
        let mut edge_owners = vec![[None, None]; surface.edge_count()];
        let mut edge_traversal_costs = vec![0; surface.edge_count()];
        let mut boundary_cells = vec![false; cell_count];
        let coordinate_scale = surface.long_length_scale().get();

        for edge_index in 0..surface.edge_count() {
            let edge = surface
                .edge(EdgeId::from_raw(edge_index as u32))
                .expect("validated natural-surface edge IDs are contiguous");
            let edge_index = edge.id().raw() as usize;
            let traversal_cost = quantize_positive(
                edge.traversal_length().get() / coordinate_scale,
                LENGTH_QUANTIZATION,
            );
            edge_traversal_costs[edge_index] = traversal_cost;
            match edge.owners() {
                [Some(first), Some(second)] => {
                    let owners = normalized_owner_pair(first, second);
                    edge_owners[edge_index] = [Some(owners[0]), Some(owners[1])];
                    arcs[first.raw() as usize].push(NeighborArc {
                        neighbor: second,
                        edge: edge.id(),
                        traversal_cost,
                    });
                    arcs[second.raw() as usize].push(NeighborArc {
                        neighbor: first,
                        edge: edge.id(),
                        traversal_cost,
                    });
                }
                [Some(owner), None] | [None, Some(owner)] => {
                    edge_owners[edge_index] = [Some(owner), None];
                    boundary_cells[owner.raw() as usize] = true;
                }
                [None, None] => {
                    debug_assert!(
                        false,
                        "validated natural-surface edges always have an owner"
                    );
                }
            }
        }
        for neighbors in &mut arcs {
            neighbors.sort_by_key(|arc| (arc.neighbor, arc.edge));
        }

        let quantized_shape_positions = (0..cell_count)
            .map(|index| {
                let cell = surface
                    .cell(CellId::from_raw(index as u32))
                    .expect("validated natural-surface cell IDs are contiguous");
                cell.shape_position().map(quantize_coordinate)
            })
            .collect();
        let total_area = surface.total_area().get();
        let area_weights = (0..cell_count)
            .map(|index| {
                let cell = surface
                    .cell(CellId::from_raw(index as u32))
                    .expect("validated natural-surface cell IDs are contiguous");
                quantize_positive(cell.area().get() / total_area, AREA_QUANTIZATION)
            })
            .collect();

        Self {
            arcs,
            edge_owners,
            edge_traversal_costs,
            quantized_shape_positions,
            area_weights,
            boundary_cells,
            minimum_dimension_m: surface.short_length_scale().get(),
            maximum_dimension_m: coordinate_scale,
        }
    }

    pub(super) fn arcs(&self) -> &[Vec<NeighborArc>] {
        &self.arcs
    }

    pub(super) fn edge_owners(&self) -> &[[Option<CellId>; 2]] {
        &self.edge_owners
    }

    pub(super) fn edge_traversal_costs(&self) -> &[u64] {
        &self.edge_traversal_costs
    }

    pub(super) fn quantized_shape_positions(&self) -> &[[i64; 3]] {
        &self.quantized_shape_positions
    }

    pub(super) fn area_weights(&self) -> &[u64] {
        &self.area_weights
    }

    pub(super) fn boundary_cells(&self) -> &[bool] {
        &self.boundary_cells
    }

    pub(super) fn quantized_distance_for_meters(&self, distance_m: f64) -> u64 {
        debug_assert!(distance_m.is_finite() && distance_m >= 0.0);
        if distance_m <= 0.0 {
            0
        } else {
            quantize_positive(distance_m / self.maximum_dimension_m, LENGTH_QUANTIZATION)
        }
    }

    pub(super) fn quantized_short_side_fraction(&self, fraction: f64) -> u64 {
        debug_assert!(fraction.is_finite() && fraction >= 0.0);
        self.quantized_distance_for_meters(self.minimum_dimension_m * fraction)
    }

    pub(super) fn edge_between(&self, first: CellId, second: CellId) -> Option<EdgeId> {
        self.arcs
            .get(first.raw() as usize)?
            .binary_search_by_key(&second, |arc| arc.neighbor)
            .ok()
            .map(|index| self.arcs[first.raw() as usize][index].edge)
    }

    pub(super) fn cell_count(&self) -> usize {
        self.arcs.len()
    }

    pub(super) fn edge_count(&self) -> usize {
        self.edge_traversal_costs.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GraphAssignment {
    pub(super) owners: Vec<u32>,
    pub(super) distances: Vec<u64>,
}

pub(super) fn multi_source_ownership(
    topology: &NaturalTopologyIndex,
    sources: &[CellId],
) -> GraphAssignment {
    propagate(topology, sources, None)
}

pub(super) fn multi_source_distance(
    topology: &NaturalTopologyIndex,
    sources: &[CellId],
    maximum_distance: Option<u64>,
) -> Vec<u64> {
    propagate(topology, sources, maximum_distance).distances
}

pub(super) fn farthest_point_seeds(
    topology: &NaturalTopologyIndex,
    count: usize,
    tie_rotation: u64,
) -> Vec<CellId> {
    assert!(
        count <= topology.cell_count(),
        "seed count cannot exceed cell count"
    );
    if count == 0 {
        return Vec::new();
    }

    let cell_count = topology.cell_count();
    let rotation = stable_rotation_index(tie_rotation, cell_count);
    let mut selected = vec![false; cell_count];
    let mut seeds = Vec::with_capacity(count);
    seeds.push(CellId::from_raw(rotation as u32));
    selected[rotation] = true;

    while seeds.len() < count {
        let distances = multi_source_distance(topology, &seeds, None);
        let mut best = None;
        for offset in 0..cell_count {
            let index = (rotation + offset) % cell_count;
            if selected[index] {
                continue;
            }
            if best.is_none_or(|current: usize| distances[index] > distances[current]) {
                best = Some(index);
            }
        }
        let next = best.expect("at least one unselected cell remains");
        selected[next] = true;
        seeds.push(CellId::from_raw(next as u32));
    }
    seeds
}

pub(super) fn farthest_point_seeds_from_candidates(
    topology: &NaturalTopologyIndex,
    candidates: &[CellId],
    count: usize,
    tie_rotation: u64,
) -> Vec<CellId> {
    assert!(
        count <= candidates.len(),
        "seed count cannot exceed candidate count"
    );
    let mut candidate_flags = vec![false; topology.cell_count()];
    for &candidate in candidates {
        let index = candidate.raw() as usize;
        assert!(index < topology.cell_count(), "candidate cell must exist");
        assert!(!candidate_flags[index], "candidate cells must be unique");
        candidate_flags[index] = true;
    }
    if count == 0 {
        return Vec::new();
    }

    let rotation = stable_rotation_index(tie_rotation, candidates.len());
    let mut selected = vec![false; topology.cell_count()];
    let mut seeds = Vec::with_capacity(count);
    let first = candidates[rotation];
    seeds.push(first);
    selected[first.raw() as usize] = true;

    while seeds.len() < count {
        let distances = multi_source_distance(topology, &seeds, None);
        let mut best = None;
        for offset in 0..candidates.len() {
            let candidate = candidates[(rotation + offset) % candidates.len()];
            let index = candidate.raw() as usize;
            if selected[index] {
                continue;
            }
            if best
                .is_none_or(|current: CellId| distances[index] > distances[current.raw() as usize])
            {
                best = Some(candidate);
            }
        }
        let next = best.expect("at least one unselected candidate remains");
        selected[next.raw() as usize] = true;
        seeds.push(next);
    }
    seeds
}

fn stable_rotation_index(tie_rotation: u64, domain_len: usize) -> usize {
    assert!(domain_len > 0, "rotation domain cannot be empty");
    let domain_len = u64::try_from(domain_len).expect("cell domains fit in u64");
    usize::try_from(tie_rotation % domain_len).expect("remainder fits in the source domain")
}

fn propagate(
    topology: &NaturalTopologyIndex,
    sources: &[CellId],
    maximum_distance: Option<u64>,
) -> GraphAssignment {
    if sources.is_empty() {
        return GraphAssignment {
            owners: vec![u32::MAX; topology.cell_count()],
            distances: vec![u64::MAX; topology.cell_count()],
        };
    }
    let mut seen = vec![false; topology.cell_count()];
    let mut arrival_sources = Vec::with_capacity(sources.len());
    for (owner, &source) in sources.iter().enumerate() {
        let index = source.raw() as usize;
        assert!(index < topology.cell_count(), "source cell must exist");
        if seen[index] {
            continue;
        }
        seen[index] = true;
        arrival_sources.push(ArrivalSource {
            owner: owner as u32,
            cell: source,
            initial_cost: 0,
        });
    }
    let metric = PositiveEdgeMetric::from_topology_lengths(topology)
        .expect("validated topology traversal costs are positive");
    let mut workspace = ArrivalWorkspace::default();
    let assignment = assign_arrivals_bounded(
        topology,
        &metric,
        &arrival_sources,
        maximum_distance,
        &mut workspace,
    )
    .expect("legacy topology sources and costs are valid");
    GraphAssignment {
        owners: assignment.owners.into_vec(),
        distances: assignment.costs.into_vec(),
    }
}

fn quantize_positive(normalized: f64, scale: f64) -> u64 {
    debug_assert!(normalized.is_finite() && normalized > 0.0);
    (normalized * scale).round().max(1.0) as u64
}

fn quantize_coordinate(normalized: f64) -> i64 {
    debug_assert!(normalized.is_finite() && normalized >= 0.0);
    (normalized * CENTER_QUANTIZATION).round() as i64 + 1
}

fn normalized_owner_pair(first: CellId, second: CellId) -> [CellId; 2] {
    if first < second {
        [first, second]
    } else {
        [second, first]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        farthest_point_seeds, farthest_point_seeds_from_candidates, multi_source_distance,
        multi_source_ownership, stable_rotation_index, NaturalTopologyIndex,
    };
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::spatial::{
        NaturalSurface, PlanarNaturalSurface, SpatialCell, SpatialEdge, SpatialSnapshot,
        SphericalNaturalSurface, SPATIAL_SCHEMA_V1,
    };
    use crate::world::{
        BoundaryCondition, CellId, EdgeId, Meters, SphericalSpaceSpec, SquareMeters, WorldPoint,
        WorldRect,
    };

    fn meters(value: f64) -> Meters {
        Meters::new(value).unwrap()
    }

    fn point(x: f64, y: f64) -> WorldPoint {
        WorldPoint::new(meters(x), meters(y))
    }

    fn cell(id: u32, center: (f64, f64), polygon: &[(f64, f64)], neighbors: &[u32]) -> SpatialCell {
        SpatialCell {
            id: CellId::from_raw(id),
            site: point(center.0, center.1),
            centroid: point(center.0, center.1),
            area: SquareMeters::new(1.0).unwrap(),
            polygon: polygon.iter().map(|&(x, y)| point(x, y)).collect(),
            neighbors: neighbors.iter().copied().map(CellId::from_raw).collect(),
        }
    }

    fn edge(id: u32, start: (f64, f64), end: (f64, f64), owners: [Option<u32>; 2]) -> SpatialEdge {
        let start = point(start.0, start.1);
        let end = point(end.0, end.1);
        SpatialEdge {
            id: EdgeId::from_raw(id),
            start,
            end,
            length: meters(
                (end.x().get() - start.x().get()).hypot(end.y().get() - start.y().get()),
            ),
            cells: owners.map(|owner| owner.map(CellId::from_raw)),
        }
    }

    fn fixture(reverse_edges: bool) -> SpatialSnapshot {
        let cells = vec![
            cell(
                0,
                (0.5, 0.5),
                &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
                &[1, 2],
            ),
            cell(
                1,
                (1.5, 0.5),
                &[(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0)],
                &[0, 3],
            ),
            cell(
                2,
                (0.5, 1.5),
                &[(0.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)],
                &[0, 3],
            ),
            cell(
                3,
                (1.5, 1.5),
                &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)],
                &[1, 2],
            ),
        ];
        let mut edges = vec![
            edge(0, (0.0, 0.0), (1.0, 0.0), [Some(0), None]),
            edge(1, (0.0, 1.0), (0.0, 0.0), [Some(0), None]),
            edge(2, (1.0, 0.0), (2.0, 0.0), [Some(1), None]),
            edge(3, (2.0, 0.0), (2.0, 1.0), [Some(1), None]),
            edge(4, (0.0, 2.0), (0.0, 1.0), [Some(2), None]),
            edge(5, (1.0, 2.0), (0.0, 2.0), [Some(2), None]),
            edge(6, (2.0, 1.0), (2.0, 2.0), [Some(3), None]),
            edge(7, (2.0, 2.0), (1.0, 2.0), [Some(3), None]),
            edge(8, (1.0, 0.0), (1.0, 1.0), [Some(0), Some(1)]),
            edge(9, (1.0, 1.0), (0.0, 1.0), [Some(0), Some(2)]),
            edge(10, (1.0, 1.0), (2.0, 1.0), [Some(1), Some(3)]),
            edge(11, (1.0, 1.0), (1.0, 2.0), [Some(2), Some(3)]),
        ];
        if reverse_edges {
            edges.reverse();
        }
        SpatialSnapshot::new(
            SPATIAL_SCHEMA_V1,
            WorldRect::new(point(0.0, 0.0), point(2.0, 2.0)).unwrap(),
            BoundaryCondition::Closed,
            cells,
            edges,
        )
        .unwrap()
    }

    fn three_cell_fixture() -> SpatialSnapshot {
        let cells = vec![
            cell(
                0,
                (0.5, 0.5),
                &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
                &[1],
            ),
            cell(
                1,
                (1.5, 0.5),
                &[(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0)],
                &[0, 2],
            ),
            cell(
                2,
                (2.5, 0.5),
                &[(2.0, 0.0), (3.0, 0.0), (3.0, 1.0), (2.0, 1.0)],
                &[1],
            ),
        ];
        let edges = vec![
            edge(0, (0.0, 0.0), (1.0, 0.0), [Some(0), None]),
            edge(1, (0.0, 1.0), (0.0, 0.0), [Some(0), None]),
            edge(2, (1.0, 1.0), (0.0, 1.0), [Some(0), None]),
            edge(3, (1.0, 0.0), (2.0, 0.0), [Some(1), None]),
            edge(4, (2.0, 1.0), (1.0, 1.0), [Some(1), None]),
            edge(5, (2.0, 0.0), (3.0, 0.0), [Some(2), None]),
            edge(6, (3.0, 0.0), (3.0, 1.0), [Some(2), None]),
            edge(7, (3.0, 1.0), (2.0, 1.0), [Some(2), None]),
            edge(8, (1.0, 0.0), (1.0, 1.0), [Some(0), Some(1)]),
            edge(9, (2.0, 0.0), (2.0, 1.0), [Some(1), Some(2)]),
        ];
        SpatialSnapshot::new(
            SPATIAL_SCHEMA_V1,
            WorldRect::new(point(0.0, 0.0), point(3.0, 1.0)).unwrap(),
            BoundaryCondition::Closed,
            cells,
            edges,
        )
        .unwrap()
    }

    #[test]
    fn index_maps_topology_and_quantizes_positive_values() {
        let index = NaturalTopologyIndex::new(&fixture(false));

        for (first, second, edge) in [(0, 1, 8), (0, 2, 9), (1, 3, 10), (2, 3, 11)] {
            assert_eq!(
                index.edge_between(CellId::from_raw(first), CellId::from_raw(second)),
                Some(EdgeId::from_raw(edge))
            );
        }
        assert_eq!(
            index.edge_owners()[8],
            [Some(CellId::from_raw(0)), Some(CellId::from_raw(1))]
        );
        assert_eq!(index.edge_owners()[0], [Some(CellId::from_raw(0)), None]);
        assert!(index
            .arcs()
            .iter()
            .flatten()
            .all(|arc| arc.traversal_cost > 0));
        assert!(index
            .quantized_shape_positions()
            .iter()
            .flatten()
            .all(|coordinate| *coordinate > 0));
        assert!(index.area_weights().iter().all(|weight| *weight > 0));
        assert_eq!(index.boundary_cells(), &[true, true, true, true]);
        assert_eq!(
            index.quantized_distance_for_meters(1.0),
            index.arcs()[0][0].traversal_cost
        );
        assert_eq!(index.quantized_distance_for_meters(f64::EPSILON), 1);
    }

    #[test]
    fn generic_planar_index_matches_the_frozen_v1_quantization() {
        let snapshot = fixture(false);
        let surface = PlanarNaturalSurface::new(&snapshot).unwrap();
        let generic = NaturalTopologyIndex::from_surface(&surface);

        assert_eq!(generic, NaturalTopologyIndex::new(&snapshot));
        assert_eq!(
            generic.quantized_shape_positions(),
            &[
                [250_001, 250_001, 1],
                [750_001, 250_001, 1],
                [250_001, 750_001, 1],
                [750_001, 750_001, 1],
            ]
        );
        assert_eq!(generic.area_weights(), &[250_000_000; 4]);
        assert!(generic
            .arcs()
            .iter()
            .flatten()
            .all(|arc| arc.traversal_cost == 500_000));
        assert_eq!(generic.minimum_dimension_m, 2.0);
        assert_eq!(generic.maximum_dimension_m, 2.0);
    }

    #[test]
    fn closed_spherical_index_has_symmetric_arcs_and_no_boundary_cells() {
        let snapshot = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: meters(1_000.0),
            target_cell_count: 42,
        })
        .unwrap();
        let checked_surface = SphericalNaturalSurface::new(&snapshot).unwrap();
        let surface = SphericalNaturalSurface::from_validated(&snapshot).unwrap();
        assert_eq!(surface.surface_ref(), checked_surface.surface_ref());
        let first = NaturalTopologyIndex::from_surface(&surface);
        let second = NaturalTopologyIndex::from_surface(&surface);

        assert_eq!(first, second);
        assert_eq!(first.arcs().len(), 42);
        assert_eq!(first.edge_owners().len(), 120);
        assert!(first
            .edge_owners()
            .iter()
            .all(|owners| owners[0].is_some() && owners[1].is_some()));
        assert!(first.boundary_cells().iter().all(|&boundary| !boundary));
        assert_eq!(
            first.arcs().iter().map(Vec::len).sum::<usize>(),
            snapshot.edges().len() * 2
        );
        for (cell_index, arcs) in first.arcs().iter().enumerate() {
            let cell = CellId::from_raw(cell_index as u32);
            for arc in arcs {
                assert!(arc.traversal_cost > 0);
                assert!(first.arcs()[arc.neighbor.raw() as usize]
                    .iter()
                    .any(|reverse| reverse.neighbor == cell
                        && reverse.edge == arc.edge
                        && reverse.traversal_cost == arc.traversal_cost));
            }
        }
        assert!(first
            .quantized_shape_positions()
            .iter()
            .flatten()
            .all(|&coordinate| (1..=1_000_001).contains(&coordinate)));
        assert!(first.area_weights().iter().all(|&weight| weight > 0));

        let seeds = farthest_point_seeds(&first, 8, u64::MAX);
        assert_eq!(seeds, farthest_point_seeds(&second, 8, u64::MAX));
        assert_eq!(seeds.iter().copied().collect::<BTreeSet<_>>().len(), 8);
    }

    #[test]
    fn input_edge_order_is_normalized_before_indexing() {
        assert_eq!(
            NaturalTopologyIndex::new(&fixture(false)),
            NaturalTopologyIndex::new(&fixture(true))
        );
    }

    #[test]
    fn farthest_point_seed_selection_is_unique_and_exact() {
        let index = NaturalTopologyIndex::new(&fixture(false));
        assert_eq!(
            farthest_point_seeds(&index, 3, 0),
            vec![
                CellId::from_raw(0),
                CellId::from_raw(3),
                CellId::from_raw(1)
            ]
        );
    }

    #[test]
    fn farthest_point_seed_selection_respects_the_candidate_domain() {
        let index = NaturalTopologyIndex::new(&fixture(false));
        let candidates = [CellId::from_raw(1), CellId::from_raw(2)];

        assert_eq!(
            farthest_point_seeds_from_candidates(&index, &candidates, 2, 0),
            candidates
        );
        assert_eq!(
            farthest_point_seeds_from_candidates(&index, &candidates, 1, 1),
            vec![CellId::from_raw(2)]
        );
    }

    #[test]
    fn rotation_uses_the_full_u64_seed_before_narrowing_to_an_index() {
        assert_eq!(stable_rotation_index(u64::MAX, 3), 0);
        assert_eq!(stable_rotation_index(1_u64 << 32, 3), 1);

        let index = NaturalTopologyIndex::new(&three_cell_fixture());
        let candidates = [
            CellId::from_raw(0),
            CellId::from_raw(1),
            CellId::from_raw(2),
        ];
        let high_bit_seed = (1_u64 << 32) + 1;
        assert_eq!(
            farthest_point_seeds(&index, 1, high_bit_seed),
            vec![CellId::from_raw(2)]
        );
        assert_eq!(
            farthest_point_seeds_from_candidates(&index, &candidates, 1, high_bit_seed),
            vec![CellId::from_raw(2)]
        );
    }

    #[test]
    fn multi_source_ownership_uses_stable_tie_breaks_and_positive_costs() {
        let index = NaturalTopologyIndex::new(&fixture(false));
        let assignment =
            multi_source_ownership(&index, &[CellId::from_raw(0), CellId::from_raw(3)]);
        let cost = index.arcs()[0][0].traversal_cost;

        assert_eq!(assignment.owners, vec![0, 0, 0, 1]);
        assert_eq!(assignment.distances, vec![0, cost, cost, 0]);
        assert!(index
            .arcs()
            .iter()
            .flatten()
            .all(|arc| arc.traversal_cost > 0));
    }

    #[test]
    fn optional_max_distance_stops_propagation_without_fake_zero_costs() {
        let index = NaturalTopologyIndex::new(&fixture(false));
        assert_eq!(
            multi_source_distance(&index, &[CellId::from_raw(0)], Some(0)),
            vec![0, u64::MAX, u64::MAX, u64::MAX]
        );
    }
}
