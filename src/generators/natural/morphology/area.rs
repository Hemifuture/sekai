#![cfg_attr(not(test), allow(dead_code))]

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

use thiserror::Error;

use crate::generators::natural::topology::{multi_source_distance, NaturalTopologyIndex};
use crate::world::CellId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::generators::natural) struct AreaMask {
    selected: Box<[bool]>,
    selected_area_weight: u128,
    component_count: usize,
}

impl AreaMask {
    pub(in crate::generators::natural) fn selected(&self) -> &[bool] {
        &self.selected
    }

    pub(in crate::generators::natural) fn is_selected(&self, cell: CellId) -> bool {
        self.selected
            .get(cell.raw() as usize)
            .copied()
            .unwrap_or(false)
    }

    pub(in crate::generators::natural) const fn selected_area_weight(&self) -> u128 {
        self.selected_area_weight
    }

    pub(in crate::generators::natural) const fn component_count(&self) -> usize {
        self.component_count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::generators::natural) struct ProtectedRegionSeed {
    pub(in crate::generators::natural) cell: CellId,
    pub(in crate::generators::natural) budget_weight: u128,
    pub(in crate::generators::natural) component: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(in crate::generators::natural) enum AreaSelectionError {
    #[error("area scores contain {found} cells, expected {expected}")]
    ScoreCardinality { expected: usize, found: usize },
    #[error("area selection requires at least one protected seed")]
    EmptyProtectedSeeds,
    #[error("protected seed cell {cell:?} is outside {cell_count} cells")]
    SeedOutOfRange { cell: CellId, cell_count: usize },
    #[error("protected seed cell {cell:?} occurs more than once")]
    DuplicateSeed { cell: CellId },
    #[error("protected component IDs must be contiguous; expected {expected}, found {found}")]
    NonContiguousComponent { expected: u16, found: u16 },
    #[error("protected component {component} has zero area budget")]
    ZeroComponentBudget { component: u16 },
    #[error("protected budgets total {protected_weight}, above target {target_weight}")]
    ProtectedBudgetExceedsTarget {
        protected_weight: u128,
        target_weight: u128,
    },
    #[error("area target {target_weight} exceeds available surface area {available_weight}")]
    TargetExceedsSurface {
        target_weight: u128,
        available_weight: u128,
    },
    #[error("component {component} cannot reach its protected budget")]
    ProtectedGrowthExhausted { component: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GrowthCandidate {
    score: i32,
    component: usize,
    cell: CellId,
}

impl Ord for GrowthCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .cmp(&other.score)
            .then_with(|| other.component.cmp(&self.component))
            .then_with(|| other.cell.cmp(&self.cell))
    }
}

impl PartialOrd for GrowthCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(in crate::generators::natural) fn build_area_constrained_mask(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    protected: &[ProtectedRegionSeed],
    target_weight: u128,
    minimum_component_weight: u128,
    maximum_hole_weight: u128,
) -> Result<AreaMask, AreaSelectionError> {
    build_area_constrained_mask_impl(
        topology,
        scores,
        protected,
        target_weight,
        minimum_component_weight,
        maximum_hole_weight,
        false,
    )
}

pub(in crate::generators::natural) fn build_component_budgeted_area_mask(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    protected: &[ProtectedRegionSeed],
    target_weight: u128,
    minimum_component_weight: u128,
    maximum_hole_weight: u128,
) -> Result<AreaMask, AreaSelectionError> {
    build_area_constrained_mask_impl(
        topology,
        scores,
        protected,
        target_weight,
        minimum_component_weight,
        maximum_hole_weight,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_area_constrained_mask_impl(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    protected: &[ProtectedRegionSeed],
    target_weight: u128,
    minimum_component_weight: u128,
    maximum_hole_weight: u128,
    enforce_component_budgets: bool,
) -> Result<AreaMask, AreaSelectionError> {
    validate_inputs(topology, scores, protected, target_weight)?;

    let mut selected = vec![false; topology.cell_count()];
    grow_protected_regions_independently(topology, scores, protected, &mut selected)?;
    let (mut labels, mut component_area) = label_selected_components(topology, &selected);
    let component_budgets = enforce_component_budgets.then(|| {
        let mut budgets = vec![0_u128; component_area.len()];
        for seed in protected {
            budgets[labels[seed.cell.raw() as usize]] += seed.budget_weight;
        }
        budgets
    });
    let mut total_area = selected
        .iter()
        .zip(topology.area_weights())
        .filter_map(|(&is_selected, &area)| is_selected.then_some(u128::from(area)))
        .sum::<u128>();

    let mut frontier = BinaryHeap::new();
    for (index, &is_selected) in selected.iter().enumerate() {
        if is_selected {
            push_neighbors(
                topology,
                scores,
                &selected,
                labels[index],
                CellId::from_raw(index as u32),
                &mut frontier,
            );
        }
    }
    grow_toward_target(
        topology,
        scores,
        target_weight,
        &mut selected,
        &mut labels,
        &mut component_area,
        &mut total_area,
        &mut frontier,
        component_budgets.as_deref(),
    );

    fill_small_holes_when_closer(
        topology,
        target_weight,
        maximum_hole_weight,
        &mut selected,
        &mut labels,
        &mut component_area,
        &mut total_area,
        component_budgets.as_deref(),
    );

    remove_unprotected_speckles(
        topology,
        protected,
        minimum_component_weight,
        &mut selected,
        &mut total_area,
    );
    let protected_cells = protected.iter().map(|seed| seed.cell).collect::<Vec<_>>();
    shrink_coast_toward_target(
        topology,
        scores,
        &protected_cells,
        target_weight,
        &mut selected,
        &mut total_area,
    );
    let component_count = count_selected_components(topology, &selected);
    Ok(AreaMask {
        selected: selected.into_boxed_slice(),
        selected_area_weight: total_area,
        component_count,
    })
}

fn shrink_coast_toward_target(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    protected_cells: &[CellId],
    target_weight: u128,
    selected: &mut [bool],
    selected_area_weight: &mut u128,
) {
    let mut protected = vec![false; topology.cell_count()];
    for &cell in protected_cells {
        protected[cell.raw() as usize] = true;
    }

    while *selected_area_weight > target_weight {
        let articulation = selected_articulation_points(topology, selected);
        let current_error = selected_area_weight.abs_diff(target_weight);
        let candidate = selected
            .iter()
            .enumerate()
            .filter(|&(index, &is_selected)| {
                is_selected
                    && !protected[index]
                    && !articulation[index]
                    && topology.arcs()[index]
                        .iter()
                        .any(|arc| !selected[arc.neighbor.raw() as usize])
            })
            .filter_map(|(index, _)| {
                let area = u128::from(topology.area_weights()[index]);
                let next = selected_area_weight.saturating_sub(area);
                (next.abs_diff(target_weight) <= current_error).then_some((
                    scores[index],
                    CellId::from_raw(index as u32),
                    area,
                ))
            })
            .min_by_key(|&(score, cell, _)| (score, cell));
        let Some((_, cell, area)) = candidate else {
            break;
        };
        selected[cell.raw() as usize] = false;
        *selected_area_weight -= area;
    }
}

fn selected_articulation_points(topology: &NaturalTopologyIndex, selected: &[bool]) -> Vec<bool> {
    #[derive(Clone, Copy)]
    struct Frame {
        cell: usize,
        next_arc: usize,
    }

    let cell_count = topology.cell_count();
    let unvisited = usize::MAX;
    let mut discovery = vec![unvisited; cell_count];
    let mut low = vec![0; cell_count];
    let mut parent = vec![unvisited; cell_count];
    let mut child_count = vec![0_usize; cell_count];
    let mut articulation = vec![false; cell_count];
    let mut clock = 0_usize;
    let mut stack = Vec::new();

    for root in 0..cell_count {
        if !selected[root] || discovery[root] != unvisited {
            continue;
        }
        discovery[root] = clock;
        low[root] = clock;
        clock += 1;
        stack.push(Frame {
            cell: root,
            next_arc: 0,
        });

        while let Some(frame) = stack.last_mut() {
            let cell = frame.cell;
            if frame.next_arc < topology.arcs()[cell].len() {
                let neighbor = topology.arcs()[cell][frame.next_arc].neighbor.raw() as usize;
                frame.next_arc += 1;
                if !selected[neighbor] {
                    continue;
                }
                if discovery[neighbor] == unvisited {
                    parent[neighbor] = cell;
                    child_count[cell] += 1;
                    discovery[neighbor] = clock;
                    low[neighbor] = clock;
                    clock += 1;
                    stack.push(Frame {
                        cell: neighbor,
                        next_arc: 0,
                    });
                } else if parent[cell] != neighbor {
                    low[cell] = low[cell].min(discovery[neighbor]);
                }
                continue;
            }

            stack.pop();
            let parent_cell = parent[cell];
            if parent_cell == unvisited {
                articulation[cell] = child_count[cell] > 1;
                continue;
            }
            low[parent_cell] = low[parent_cell].min(low[cell]);
            if parent[parent_cell] != unvisited && low[cell] >= discovery[parent_cell] {
                articulation[parent_cell] = true;
            }
        }
    }
    articulation
}

fn validate_inputs(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    protected: &[ProtectedRegionSeed],
    target_weight: u128,
) -> Result<(), AreaSelectionError> {
    if scores.len() != topology.cell_count() {
        return Err(AreaSelectionError::ScoreCardinality {
            expected: topology.cell_count(),
            found: scores.len(),
        });
    }
    if protected.is_empty() {
        return Err(AreaSelectionError::EmptyProtectedSeeds);
    }
    let available = topology
        .area_weights()
        .iter()
        .copied()
        .map(u128::from)
        .sum::<u128>();
    if target_weight > available {
        return Err(AreaSelectionError::TargetExceedsSurface {
            target_weight,
            available_weight: available,
        });
    }
    let mut seen = vec![false; topology.cell_count()];
    let mut budget_total = 0_u128;
    for (expected, seed) in protected.iter().enumerate() {
        let index = seed.cell.raw() as usize;
        if index >= topology.cell_count() {
            return Err(AreaSelectionError::SeedOutOfRange {
                cell: seed.cell,
                cell_count: topology.cell_count(),
            });
        }
        if seen[index] {
            return Err(AreaSelectionError::DuplicateSeed { cell: seed.cell });
        }
        seen[index] = true;
        if seed.component != expected as u16 {
            return Err(AreaSelectionError::NonContiguousComponent {
                expected: expected as u16,
                found: seed.component,
            });
        }
        if seed.budget_weight == 0 {
            return Err(AreaSelectionError::ZeroComponentBudget {
                component: seed.component,
            });
        }
        budget_total += seed.budget_weight;
    }
    if budget_total > target_weight {
        return Err(AreaSelectionError::ProtectedBudgetExceedsTarget {
            protected_weight: budget_total,
            target_weight,
        });
    }
    Ok(())
}

fn has_component_neighbor(
    topology: &NaturalTopologyIndex,
    selected: &[bool],
    labels: &[usize],
    candidate: GrowthCandidate,
) -> bool {
    let mut has_own_neighbor = false;
    for arc in &topology.arcs()[candidate.cell.raw() as usize] {
        let index = arc.neighbor.raw() as usize;
        if !selected[index] {
            continue;
        }
        if labels[index] != candidate.component {
            return false;
        }
        has_own_neighbor = true;
    }
    has_own_neighbor
}

fn grow_protected_regions_independently(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    protected: &[ProtectedRegionSeed],
    selected: &mut [bool],
) -> Result<(), AreaSelectionError> {
    // Each seed first establishes a non-overlapping identity-bearing core. The shared frontier
    // below then grows those cores to the requested total area. Growing every seed to its full
    // budget independently makes neighboring lobes overlap before they have labels, which merges
    // otherwise distinct continents; growing only tiny seed cells makes the final frontier overly
    // sensitive to tessellation resolution. A fixed fraction of physical area is the stable middle
    // ground, and contains no cell-count threshold.
    const PROTECTED_CORE_BUDGET_MILLI: u128 = 600;

    let growth_scores = protected
        .iter()
        .map(|seed| compact_growth_scores(topology, scores, seed.cell, seed.budget_weight))
        .collect::<Vec<_>>();
    let mut local = vec![false; topology.cell_count()];
    let mut frontier = BinaryHeap::new();
    for (component, seed) in protected.iter().enumerate() {
        local.fill(false);
        frontier.clear();
        local[seed.cell.raw() as usize] = true;
        let mut area = u128::from(topology.area_weights()[seed.cell.raw() as usize]);
        let core_budget = (seed.budget_weight * PROTECTED_CORE_BUDGET_MILLI / 1_000).max(area);
        push_neighbors(
            topology,
            &growth_scores[component],
            &local,
            component,
            seed.cell,
            &mut frontier,
        );
        while area < core_budget {
            let Some(candidate) = frontier.pop() else {
                return Err(AreaSelectionError::ProtectedGrowthExhausted {
                    component: component as u16,
                });
            };
            let index = candidate.cell.raw() as usize;
            if local[index] {
                continue;
            }
            local[index] = true;
            area += u128::from(topology.area_weights()[index]);
            push_neighbors(
                topology,
                &growth_scores[component],
                &local,
                component,
                candidate.cell,
                &mut frontier,
            );
        }
        for (target, &source) in selected.iter_mut().zip(&local) {
            *target |= source;
        }
    }
    Ok(())
}

fn label_selected_components(
    topology: &NaturalTopologyIndex,
    selected: &[bool],
) -> (Vec<usize>, Vec<u128>) {
    let mut labels = vec![usize::MAX; topology.cell_count()];
    let mut areas = Vec::new();
    for start in 0..topology.cell_count() {
        if !selected[start] || labels[start] != usize::MAX {
            continue;
        }
        let component = areas.len();
        labels[start] = component;
        let mut area = 0_u128;
        let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
        while let Some(cell) = queue.pop_front() {
            let index = cell.raw() as usize;
            area += u128::from(topology.area_weights()[index]);
            for arc in &topology.arcs()[index] {
                let neighbor = arc.neighbor.raw() as usize;
                if selected[neighbor] && labels[neighbor] == usize::MAX {
                    labels[neighbor] = component;
                    queue.push_back(arc.neighbor);
                }
            }
        }
        areas.push(area);
    }
    (labels, areas)
}

fn compact_growth_scores(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    seed: CellId,
    budget_weight: u128,
) -> Vec<i32> {
    const RADIAL_PENALTY_AT_BUDGET_RADIUS: u128 = 500_000;

    let distances = multi_source_distance(topology, &[seed], None);
    let mut by_distance = (0..topology.cell_count()).collect::<Vec<_>>();
    by_distance.sort_by_key(|&index| (distances[index], index));
    let mut accumulated = 0_u128;
    let mut budget_radius = 1_u64;
    for index in by_distance {
        accumulated += u128::from(topology.area_weights()[index]);
        budget_radius = distances[index].max(1);
        if accumulated >= budget_weight {
            break;
        }
    }
    scores
        .iter()
        .zip(distances)
        .map(|(&score, distance)| {
            let penalty =
                u128::from(distance) * RADIAL_PENALTY_AT_BUDGET_RADIUS / u128::from(budget_radius);
            i64_to_i32(i64::from(score) - penalty.min(i64::MAX as u128) as i64)
        })
        .collect()
}

fn i64_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[allow(clippy::too_many_arguments)]
fn select_candidate(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    selected: &mut [bool],
    labels: &mut [usize],
    component_area: &mut [u128],
    total_area: &mut u128,
    candidate: GrowthCandidate,
    frontier: &mut BinaryHeap<GrowthCandidate>,
) {
    let index = candidate.cell.raw() as usize;
    selected[index] = true;
    labels[index] = candidate.component;
    let area = u128::from(topology.area_weights()[index]);
    component_area[candidate.component] += area;
    *total_area += area;
    push_neighbors(
        topology,
        scores,
        selected,
        candidate.component,
        candidate.cell,
        frontier,
    );
}

fn push_neighbors(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    selected: &[bool],
    component: usize,
    cell: CellId,
    frontier: &mut BinaryHeap<GrowthCandidate>,
) {
    for arc in &topology.arcs()[cell.raw() as usize] {
        let index = arc.neighbor.raw() as usize;
        if !selected[index] {
            frontier.push(GrowthCandidate {
                score: scores[index],
                component,
                cell: arc.neighbor,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn grow_toward_target(
    topology: &NaturalTopologyIndex,
    scores: &[i32],
    target_weight: u128,
    selected: &mut [bool],
    labels: &mut [usize],
    component_area: &mut [u128],
    total_area: &mut u128,
    frontier: &mut BinaryHeap<GrowthCandidate>,
    component_budgets: Option<&[u128]>,
) {
    while let Some(candidate) = frontier.pop() {
        let index = candidate.cell.raw() as usize;
        if selected[index] || !has_component_neighbor(topology, selected, labels, candidate) {
            continue;
        }
        let next = *total_area + u128::from(topology.area_weights()[index]);
        if next.abs_diff(target_weight) > total_area.abs_diff(target_weight) {
            continue;
        }
        if let Some(budgets) = component_budgets {
            let area = u128::from(topology.area_weights()[index]);
            let current = component_area[candidate.component];
            let budget = budgets[candidate.component];
            if (current + area).abs_diff(budget) > current.abs_diff(budget) {
                continue;
            }
        }
        select_candidate(
            topology,
            scores,
            selected,
            labels,
            component_area,
            total_area,
            candidate,
            frontier,
        );
        if *total_area == target_weight {
            break;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_small_holes_when_closer(
    topology: &NaturalTopologyIndex,
    target_weight: u128,
    maximum_hole_weight: u128,
    selected: &mut [bool],
    labels: &mut [usize],
    component_area: &mut [u128],
    total_area: &mut u128,
    component_budgets: Option<&[u128]>,
) {
    if maximum_hole_weight == 0 {
        return;
    }
    let mut visited = vec![false; topology.cell_count()];
    for start in 0..topology.cell_count() {
        if visited[start] || selected[start] {
            continue;
        }
        visited[start] = true;
        let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
        let mut cells = Vec::new();
        let mut weight = 0_u128;
        let mut neighboring_component = usize::MAX;
        while let Some(cell) = queue.pop_front() {
            cells.push(cell);
            weight += u128::from(topology.area_weights()[cell.raw() as usize]);
            for arc in &topology.arcs()[cell.raw() as usize] {
                let index = arc.neighbor.raw() as usize;
                if selected[index] {
                    neighboring_component = neighboring_component.min(labels[index]);
                } else if !visited[index] {
                    visited[index] = true;
                    queue.push_back(arc.neighbor);
                }
            }
        }
        if weight > maximum_hole_weight || neighboring_component == usize::MAX {
            continue;
        }
        let next = *total_area + weight;
        if next.abs_diff(target_weight) > total_area.abs_diff(target_weight) {
            continue;
        }
        if let Some(budgets) = component_budgets {
            let current = component_area[neighboring_component];
            let budget = budgets[neighboring_component];
            if (current + weight).abs_diff(budget) > current.abs_diff(budget) {
                continue;
            }
        }
        for cell in cells {
            let index = cell.raw() as usize;
            selected[index] = true;
            labels[index] = neighboring_component;
        }
        component_area[neighboring_component] += weight;
        *total_area = next;
    }
}

fn remove_unprotected_speckles(
    topology: &NaturalTopologyIndex,
    protected: &[ProtectedRegionSeed],
    minimum_component_weight: u128,
    selected: &mut [bool],
    total_area: &mut u128,
) {
    if minimum_component_weight == 0 {
        return;
    }
    let protected_cells = protected
        .iter()
        .map(|seed| seed.cell.raw() as usize)
        .collect::<Vec<_>>();
    let mut visited = vec![false; topology.cell_count()];
    for start in 0..topology.cell_count() {
        if visited[start] || !selected[start] {
            continue;
        }
        visited[start] = true;
        let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
        let mut cells = Vec::new();
        let mut weight = 0_u128;
        let mut is_protected = false;
        while let Some(cell) = queue.pop_front() {
            let index = cell.raw() as usize;
            cells.push(index);
            weight += u128::from(topology.area_weights()[index]);
            is_protected |= protected_cells.contains(&index);
            for arc in &topology.arcs()[index] {
                let neighbor = arc.neighbor.raw() as usize;
                if selected[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push_back(arc.neighbor);
                }
            }
        }
        if !is_protected && weight < minimum_component_weight {
            for index in cells {
                selected[index] = false;
            }
            *total_area -= weight;
        }
    }
}

fn count_selected_components(topology: &NaturalTopologyIndex, selected: &[bool]) -> usize {
    let mut visited = vec![false; topology.cell_count()];
    let mut count = 0;
    for start in 0..topology.cell_count() {
        if visited[start] || !selected[start] {
            continue;
        }
        count += 1;
        visited[start] = true;
        let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
        while let Some(cell) = queue.pop_front() {
            for arc in &topology.arcs()[cell.raw() as usize] {
                let index = arc.neighbor.raw() as usize;
                if selected[index] && !visited[index] {
                    visited[index] = true;
                    queue.push_back(arc.neighbor);
                }
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};

    use super::{build_area_constrained_mask, shrink_coast_toward_target, ProtectedRegionSeed};
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

    fn protected() -> [ProtectedRegionSeed; 3] {
        [
            ProtectedRegionSeed {
                cell: CellId::from_raw(0),
                budget_weight: 80_000_000,
                component: 0,
            },
            ProtectedRegionSeed {
                cell: CellId::from_raw(53),
                budget_weight: 80_000_000,
                component: 1,
            },
            ProtectedRegionSeed {
                cell: CellId::from_raw(107),
                budget_weight: 30_000_000,
                component: 2,
            },
        ]
    }

    fn scores(topology: &NaturalTopologyIndex) -> Vec<i32> {
        (0..topology.cell_count())
            .map(|index| {
                let x = topology.quantized_shape_positions()[index][0];
                let y = topology.quantized_shape_positions()[index][1];
                ((x * 31 + y * 17 + index as i64 * 7_919) % 2_000_003) as i32 - 1_000_001
            })
            .collect()
    }

    fn selected_components(
        topology: &NaturalTopologyIndex,
        selected: &[bool],
    ) -> Vec<BTreeSet<CellId>> {
        let mut visited = vec![false; topology.cell_count()];
        let mut components = Vec::new();
        for start in 0..topology.cell_count() {
            if visited[start] || !selected[start] {
                continue;
            }
            visited[start] = true;
            let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
            let mut component = BTreeSet::new();
            while let Some(cell) = queue.pop_front() {
                component.insert(cell);
                for arc in &topology.arcs()[cell.raw() as usize] {
                    let index = arc.neighbor.raw() as usize;
                    if selected[index] && !visited[index] {
                        visited[index] = true;
                        queue.push_back(arc.neighbor);
                    }
                }
            }
            components.push(component);
        }
        components
    }

    #[test]
    fn protected_growth_is_connected_area_bounded_and_deterministic() {
        let topology = fixture();
        let target = 380_000_000;
        let minimum_component = 2_000_000;
        let maximum_hole = 4_000_000;
        let first = build_area_constrained_mask(
            &topology,
            &scores(&topology),
            &protected(),
            target,
            minimum_component,
            maximum_hole,
        )
        .unwrap();
        let repeated = build_area_constrained_mask(
            &topology,
            &scores(&topology),
            &protected(),
            target,
            minimum_component,
            maximum_hole,
        )
        .unwrap();

        assert_eq!(first, repeated);
        let maximum_cell = *topology.area_weights().iter().max().unwrap() as u128;
        assert!(first.selected_area_weight().abs_diff(target) <= maximum_cell);
        let components = selected_components(&topology, first.selected());
        assert_eq!(components.len(), first.component_count());
        assert!(components.iter().all(|component| protected()
            .iter()
            .any(|seed| component.contains(&seed.cell))));
    }

    #[test]
    fn cleanup_keeps_protected_seeds_and_has_no_unprotected_speckles() {
        let topology = fixture();
        let mask = build_area_constrained_mask(
            &topology,
            &scores(&topology),
            &protected(),
            420_000_000,
            8_000_000,
            16_000_000,
        )
        .unwrap();
        let protected_cells = protected()
            .iter()
            .map(|seed| seed.cell)
            .collect::<BTreeSet<_>>();

        assert!(protected_cells.iter().all(|&cell| mask.is_selected(cell)));
        for component in selected_components(&topology, mask.selected()) {
            assert!(component.iter().any(|cell| protected_cells.contains(cell)));
        }
    }

    #[test]
    fn coast_rebalance_never_removes_a_protected_articulation_cell() {
        let topology = fixture();
        let root = CellId::from_raw(0);
        let neck = topology.arcs()[root.raw() as usize][0].neighbor;
        let far = topology.arcs()[neck.raw() as usize]
            .iter()
            .map(|arc| arc.neighbor)
            .find(|&cell| cell != root && topology.edge_between(root, cell).is_none())
            .expect("the spherical dual graph has a two-hop cell outside the root fan");
        let leaf = topology.arcs()[root.raw() as usize]
            .iter()
            .map(|arc| arc.neighbor)
            .find(|&cell| {
                cell != neck
                    && cell != far
                    && topology.edge_between(cell, neck).is_none()
                    && topology.edge_between(cell, far).is_none()
            })
            .expect("the root fan has a leaf outside the protected narrow neck");

        let mut selected = vec![false; topology.cell_count()];
        for cell in [root, neck, far, leaf] {
            selected[cell.raw() as usize] = true;
        }
        let mut scores = vec![0; topology.cell_count()];
        scores[neck.raw() as usize] = -200;
        scores[leaf.raw() as usize] = -100;
        let mut selected_area_weight = [root, neck, far, leaf]
            .into_iter()
            .map(|cell| u128::from(topology.area_weights()[cell.raw() as usize]))
            .sum::<u128>();
        let target_weight =
            selected_area_weight - u128::from(topology.area_weights()[leaf.raw() as usize]);

        shrink_coast_toward_target(
            &topology,
            &scores,
            &[root, far],
            target_weight,
            &mut selected,
            &mut selected_area_weight,
        );

        assert!(selected[neck.raw() as usize]);
        assert!(!selected[leaf.raw() as usize]);
        assert_eq!(selected_area_weight, target_weight);
        let component = selected_components(&topology, &selected);
        assert_eq!(component.len(), 1);
        assert!(component[0].contains(&root));
        assert!(component[0].contains(&far));
    }

    #[test]
    fn invalid_cardinality_is_rejected_before_selection() {
        let topology = fixture();
        assert!(
            build_area_constrained_mask(&topology, &[0], &protected(), 380_000_000, 1, 2,).is_err()
        );
    }
}
