use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};

use thiserror::Error;

use super::metric::PositiveEdgeMetric;
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::CellId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::generators::natural) struct ArrivalSource {
    pub(in crate::generators::natural) owner: u32,
    pub(in crate::generators::natural) cell: CellId,
    pub(in crate::generators::natural) initial_cost: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::generators::natural) struct ArrivalAssignment {
    pub(in crate::generators::natural) owners: Box<[u32]>,
    pub(in crate::generators::natural) costs: Box<[u64]>,
}

#[derive(Debug, Default)]
pub(in crate::generators::natural) struct ArrivalWorkspace {
    distances: Vec<u64>,
    owners: Vec<u32>,
    heap: BinaryHeap<ArrivalQueueEntry>,
}

impl ArrivalWorkspace {
    #[cfg(test)]
    fn capacities(&self) -> (usize, usize, usize) {
        (
            self.distances.capacity(),
            self.owners.capacity(),
            self.heap.capacity(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArrivalQueueEntry {
    cost: u64,
    owner: u32,
    cell: CellId,
}

impl Ord for ArrivalQueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.owner.cmp(&self.owner))
            .then_with(|| other.cell.cmp(&self.cell))
    }
}

impl PartialOrd for ArrivalQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(in crate::generators::natural) enum ArrivalError {
    #[error("arrival propagation requires at least one source")]
    EmptySources,
    #[error("arrival metric has {found} edges, expected {expected}")]
    MetricCardinality { expected: usize, found: usize },
    #[error("arrival source cell {cell:?} is outside {cell_count} cells")]
    SourceOutOfRange { cell: CellId, cell_count: usize },
    #[error("arrival source cell {cell:?} occurs more than once")]
    DuplicateSourceCell { cell: CellId },
    #[error("arrival owner {owner} occurs more than once")]
    DuplicateOwner { owner: u32 },
    #[error("arrival cost overflowed while traversing from cell {cell:?}")]
    CostOverflow { cell: CellId },
}

pub(in crate::generators::natural) fn assign_arrivals_bounded(
    topology: &NaturalTopologyIndex,
    metric: &PositiveEdgeMetric,
    sources: &[ArrivalSource],
    maximum_cost: Option<u64>,
    workspace: &mut ArrivalWorkspace,
) -> Result<ArrivalAssignment, ArrivalError> {
    validate_inputs(topology, metric, sources)?;

    workspace.distances.resize(topology.cell_count(), u64::MAX);
    workspace.distances.fill(u64::MAX);
    workspace.owners.resize(topology.cell_count(), u32::MAX);
    workspace.owners.fill(u32::MAX);
    workspace.heap.clear();

    let minimum_initial = sources
        .iter()
        .map(|source| source.initial_cost)
        .min()
        .expect("non-empty sources were validated");
    for source in sources {
        let index = source.cell.raw() as usize;
        let cost = source.initial_cost - minimum_initial;
        workspace.distances[index] = cost;
        workspace.owners[index] = source.owner;
        workspace.heap.push(ArrivalQueueEntry {
            cost,
            owner: source.owner,
            cell: source.cell,
        });
    }

    while let Some(entry) = workspace.heap.pop() {
        let cell_index = entry.cell.raw() as usize;
        if (entry.cost, entry.owner)
            != (
                workspace.distances[cell_index],
                workspace.owners[cell_index],
            )
        {
            continue;
        }
        for arc in &topology.arcs()[cell_index] {
            let edge_cost = metric
                .cost(arc.edge)
                .expect("metric cardinality was validated");
            let candidate_cost = entry
                .cost
                .checked_add(edge_cost)
                .ok_or(ArrivalError::CostOverflow { cell: entry.cell })?;
            if maximum_cost.is_some_and(|maximum| candidate_cost > maximum) {
                continue;
            }
            let neighbor = arc.neighbor.raw() as usize;
            if (candidate_cost, entry.owner)
                < (workspace.distances[neighbor], workspace.owners[neighbor])
            {
                workspace.distances[neighbor] = candidate_cost;
                workspace.owners[neighbor] = entry.owner;
                workspace.heap.push(ArrivalQueueEntry {
                    cost: candidate_cost,
                    owner: entry.owner,
                    cell: arc.neighbor,
                });
            }
        }
    }

    Ok(ArrivalAssignment {
        owners: workspace.owners.clone().into_boxed_slice(),
        costs: workspace.distances.clone().into_boxed_slice(),
    })
}

fn validate_inputs(
    topology: &NaturalTopologyIndex,
    metric: &PositiveEdgeMetric,
    sources: &[ArrivalSource],
) -> Result<(), ArrivalError> {
    if sources.is_empty() {
        return Err(ArrivalError::EmptySources);
    }
    if metric.len() != topology.edge_count() {
        return Err(ArrivalError::MetricCardinality {
            expected: topology.edge_count(),
            found: metric.len(),
        });
    }
    let mut cells = BTreeSet::new();
    let mut owners = BTreeSet::new();
    for source in sources {
        if source.cell.raw() as usize >= topology.cell_count() {
            return Err(ArrivalError::SourceOutOfRange {
                cell: source.cell,
                cell_count: topology.cell_count(),
            });
        }
        if !cells.insert(source.cell) {
            return Err(ArrivalError::DuplicateSourceCell { cell: source.cell });
        }
        if !owners.insert(source.owner) {
            return Err(ArrivalError::DuplicateOwner {
                owner: source.owner,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{assign_arrivals_bounded, ArrivalSource, ArrivalWorkspace};
    use crate::generators::natural::morphology::metric::PositiveEdgeMetric;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::spatial::SphericalNaturalSurface;
    use crate::world::{CellId, Meters, SphericalSpaceSpec};

    fn fixture() -> NaturalTopologyIndex {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        })
        .unwrap();
        let view = SphericalNaturalSurface::new(&surface).unwrap();
        NaturalTopologyIndex::from_surface(&view)
    }

    fn sources() -> [ArrivalSource; 3] {
        [
            ArrivalSource {
                owner: 0,
                cell: CellId::from_raw(0),
                initial_cost: 900,
            },
            ArrivalSource {
                owner: 1,
                cell: CellId::from_raw(53),
                initial_cost: 0,
            },
            ArrivalSource {
                owner: 2,
                cell: CellId::from_raw(107),
                initial_cost: 450,
            },
        ]
    }

    fn assert_connected_to_sources(
        topology: &NaturalTopologyIndex,
        assignment: &super::ArrivalAssignment,
        sources: &[ArrivalSource],
    ) {
        for source in sources {
            let mut reached = vec![false; topology.cell_count()];
            let mut queue = VecDeque::from([source.cell]);
            reached[source.cell.raw() as usize] = true;
            while let Some(cell) = queue.pop_front() {
                for arc in &topology.arcs()[cell.raw() as usize] {
                    let index = arc.neighbor.raw() as usize;
                    if !reached[index] && assignment.owners[index] == source.owner {
                        reached[index] = true;
                        queue.push_back(arc.neighbor);
                    }
                }
            }
            for (index, &owner) in assignment.owners.iter().enumerate() {
                if owner == source.owner {
                    assert!(
                        reached[index],
                        "owner {} cell {index} is disconnected",
                        source.owner
                    );
                }
            }
        }
    }

    fn assert_every_non_source_has_a_shortest_path_predecessor(
        topology: &NaturalTopologyIndex,
        metric: &PositiveEdgeMetric,
        assignment: &super::ArrivalAssignment,
        sources: &[ArrivalSource],
    ) {
        let source_flags = sources.iter().map(|source| source.cell).collect::<Vec<_>>();
        for (index, (&owner, &cost)) in assignment
            .owners
            .iter()
            .zip(assignment.costs.iter())
            .enumerate()
        {
            let cell = CellId::from_raw(index as u32);
            if source_flags.contains(&cell) {
                continue;
            }
            assert!(topology.arcs()[index].iter().any(|arc| {
                let neighbor = arc.neighbor.raw() as usize;
                assignment.owners[neighbor] == owner
                    && assignment.costs[neighbor].checked_add(metric.cost(arc.edge).unwrap())
                        == Some(cost)
            }));
        }
    }

    #[test]
    fn biased_arrival_is_stable_connected_and_workspace_reusable() {
        let topology = fixture();
        let metric = PositiveEdgeMetric::from_topology_lengths(&topology).unwrap();
        let sources = sources();
        let mut workspace = ArrivalWorkspace::default();

        let first =
            assign_arrivals_bounded(&topology, &metric, &sources, None, &mut workspace).unwrap();
        let capacities = workspace.capacities();
        let repeated =
            assign_arrivals_bounded(&topology, &metric, &sources, None, &mut workspace).unwrap();

        assert_eq!(first, repeated);
        assert_eq!(workspace.capacities(), capacities);
        assert_connected_to_sources(&topology, &first, &sources);
        assert_every_non_source_has_a_shortest_path_predecessor(
            &topology, &metric, &first, &sources,
        );
    }

    #[test]
    fn malformed_sources_are_rejected_before_workspace_mutation() {
        let topology = fixture();
        let metric = PositiveEdgeMetric::from_topology_lengths(&topology).unwrap();
        let mut workspace = ArrivalWorkspace::default();
        let empty_capacities = workspace.capacities();

        assert!(assign_arrivals_bounded(&topology, &metric, &[], None, &mut workspace).is_err());
        assert_eq!(workspace.capacities(), empty_capacities);

        let duplicate_cell = [
            ArrivalSource {
                owner: 0,
                cell: CellId::from_raw(1),
                initial_cost: 0,
            },
            ArrivalSource {
                owner: 1,
                cell: CellId::from_raw(1),
                initial_cost: 0,
            },
        ];
        assert!(
            assign_arrivals_bounded(&topology, &metric, &duplicate_cell, None, &mut workspace)
                .is_err()
        );
        assert_eq!(workspace.capacities(), empty_capacities);
    }
}
