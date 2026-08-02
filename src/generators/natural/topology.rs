#![cfg_attr(not(test), allow(dead_code))]

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::world::spatial::{SpatialSnapshot, Topology};
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
    quantized_centers: Vec<[i64; 2]>,
    area_weights: Vec<u64>,
    boundary_cells: Vec<bool>,
    minimum_dimension_m: f64,
    maximum_dimension_m: f64,
}

impl NaturalTopologyIndex {
    pub(super) fn new(spatial: &SpatialSnapshot) -> Self {
        let cell_count = spatial.cell_count();
        let mut arcs = vec![Vec::new(); cell_count];
        let mut edge_owners = vec![[None, None]; spatial.edges().len()];
        let mut boundary_cells = vec![false; cell_count];
        let bounds = spatial.bounds();
        let coordinate_scale = bounds.width().get().max(bounds.height().get());

        for edge in spatial.edges() {
            let edge_index = edge.id.raw() as usize;
            let traversal_cost =
                quantize_positive(edge.length.get() / coordinate_scale, LENGTH_QUANTIZATION);
            match edge.cells {
                [Some(first), Some(second)] => {
                    let owners = normalized_owner_pair(first, second);
                    edge_owners[edge_index] = [Some(owners[0]), Some(owners[1])];
                    arcs[first.raw() as usize].push(NeighborArc {
                        neighbor: second,
                        edge: edge.id,
                        traversal_cost,
                    });
                    arcs[second.raw() as usize].push(NeighborArc {
                        neighbor: first,
                        edge: edge.id,
                        traversal_cost,
                    });
                }
                [Some(owner), None] | [None, Some(owner)] => {
                    edge_owners[edge_index] = [Some(owner), None];
                    boundary_cells[owner.raw() as usize] = true;
                }
                [None, None] => {
                    debug_assert!(false, "validated spatial edges always have an owner");
                }
            }
        }
        for neighbors in &mut arcs {
            neighbors.sort_by_key(|arc| (arc.neighbor, arc.edge));
        }

        let quantized_centers = (0..cell_count)
            .map(|index| {
                let cell = spatial
                    .cell(CellId::from_raw(index as u32))
                    .expect("validated spatial IDs are contiguous");
                [
                    quantize_coordinate(
                        (cell.centroid.x().get() - bounds.min().x().get()) / coordinate_scale,
                    ),
                    quantize_coordinate(
                        (cell.centroid.y().get() - bounds.min().y().get()) / coordinate_scale,
                    ),
                ]
            })
            .collect();
        let total_area = bounds.width().get() * bounds.height().get();
        let area_weights = (0..cell_count)
            .map(|index| {
                let cell = spatial
                    .cell(CellId::from_raw(index as u32))
                    .expect("validated spatial IDs are contiguous");
                quantize_positive(cell.area.get() / total_area, AREA_QUANTIZATION)
            })
            .collect();

        Self {
            arcs,
            edge_owners,
            quantized_centers,
            area_weights,
            boundary_cells,
            minimum_dimension_m: bounds.width().get().min(bounds.height().get()),
            maximum_dimension_m: coordinate_scale,
        }
    }

    pub(super) fn arcs(&self) -> &[Vec<NeighborArc>] {
        &self.arcs
    }

    pub(super) fn edge_owners(&self) -> &[[Option<CellId>; 2]] {
        &self.edge_owners
    }

    pub(super) fn quantized_centers(&self) -> &[[i64; 2]] {
        &self.quantized_centers
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

    fn cell_count(&self) -> usize {
        self.arcs.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GraphAssignment {
    pub(super) owners: Vec<u32>,
    pub(super) distances: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QueueEntry {
    distance: u64,
    owner: u32,
    cell: CellId,
}

impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .distance
            .cmp(&self.distance)
            .then_with(|| other.owner.cmp(&self.owner))
            .then_with(|| other.cell.cmp(&self.cell))
    }
}

impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
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
    let rotation = tie_rotation as usize % cell_count;
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

fn propagate(
    topology: &NaturalTopologyIndex,
    sources: &[CellId],
    maximum_distance: Option<u64>,
) -> GraphAssignment {
    let mut owners = vec![u32::MAX; topology.cell_count()];
    let mut distances = vec![u64::MAX; topology.cell_count()];
    let mut queue = BinaryHeap::new();

    for (owner, &source) in sources.iter().enumerate() {
        let index = source.raw() as usize;
        assert!(index < topology.cell_count(), "source cell must exist");
        let owner = owner as u32;
        if (0, owner) < (distances[index], owners[index]) {
            distances[index] = 0;
            owners[index] = owner;
            queue.push(QueueEntry {
                distance: 0,
                owner,
                cell: source,
            });
        }
    }

    while let Some(entry) = queue.pop() {
        let cell_index = entry.cell.raw() as usize;
        if (entry.distance, entry.owner) != (distances[cell_index], owners[cell_index]) {
            continue;
        }
        for arc in &topology.arcs[cell_index] {
            debug_assert!(arc.traversal_cost > 0);
            let candidate_distance = entry.distance.saturating_add(arc.traversal_cost);
            if maximum_distance.is_some_and(|maximum| candidate_distance > maximum) {
                continue;
            }
            let neighbor_index = arc.neighbor.raw() as usize;
            if (candidate_distance, entry.owner)
                < (distances[neighbor_index], owners[neighbor_index])
            {
                distances[neighbor_index] = candidate_distance;
                owners[neighbor_index] = entry.owner;
                queue.push(QueueEntry {
                    distance: candidate_distance,
                    owner: entry.owner,
                    cell: arc.neighbor,
                });
            }
        }
    }

    GraphAssignment { owners, distances }
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
    use super::{
        farthest_point_seeds, multi_source_distance, multi_source_ownership, NaturalTopologyIndex,
    };
    use crate::world::spatial::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
    use crate::world::{
        BoundaryCondition, CellId, EdgeId, Meters, SquareMeters, WorldPoint, WorldRect,
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
            .quantized_centers()
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
