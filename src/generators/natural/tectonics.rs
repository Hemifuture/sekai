use std::collections::BTreeSet;

use rand::RngCore;
use thiserror::Error;

use super::random::{
    LabeledSubstreams, CRUST_SEEDS_LABEL, CRUST_SHAPE_LABEL, CRUST_THICKNESS_LABEL,
    PLATE_SEEDS_LABEL,
};
use super::topology::{
    farthest_point_seeds, multi_source_distance, multi_source_ownership, NaturalTopologyIndex,
};
use crate::engine::StageRng;
use crate::world::natural::{
    BoundaryKind, BoundaryRecord, BoundarySegment, CrustKind, CrustKindField, NaturalSpecError,
    Plate, PlateIdField, PlateVelocity, TectonicSnapshot, TectonicSpec, TectonicValidationError,
    CONTINENTAL_CRUST_MAX_THICKNESS_KM, CONTINENTAL_CRUST_MIN_THICKNESS_KM,
    OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM, TECTONIC_SNAPSHOT_SCHEMA_V1,
};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::{BoundarySegmentId, CellId, PlateId};

const FRACTION_QUANTIZATION: u64 = 1_000_000;
const CRUST_NOISE_SCALE: i64 = 1_000;
const CRUST_SMOOTHING_PASSES: usize = 3;
const THICKNESS_SMOOTHING_PASSES: usize = 4;
const PROVISIONAL_BOUNDARY_STRENGTH: f32 = 0.01;

/// Deterministic construction of plates, independent crust, and current tectonic state.
#[derive(Debug, Clone, Copy, Default)]
pub struct TectonicGenerator;

impl TectonicGenerator {
    /// Generates a complete V1 snapshot from validated planar topology and one stage stream.
    ///
    /// Plate and crust fields use labeled random substreams, so changing plate
    /// configuration cannot perturb the crust field.
    pub fn generate(
        spatial: &SpatialSnapshot,
        spec: &TectonicSpec,
        rng: &mut StageRng,
    ) -> Result<TectonicSnapshot, TectonicGenerationError> {
        spec.validate()?;
        if spec.plate_count as usize > spatial.cell_count() {
            return Err(TectonicGenerationError::PlateCountExceedsCells {
                plates: spec.plate_count,
                cells: spatial.cell_count(),
            });
        }

        let streams = LabeledSubstreams::capture(rng);
        let topology = NaturalTopologyIndex::new(spatial);
        let (plates, cell_plates) = generate_plates(&topology, spec, &streams);
        let (crust_kinds, crust_thickness_km) = generate_crust(&topology, spec, &streams);
        let (boundaries, segments) = provisional_boundaries(spatial, &cell_plates);
        let snapshot = TectonicSnapshot::new(
            TECTONIC_SNAPSHOT_SCHEMA_V1,
            spatial.cell_count() as u32,
            spatial.edges().len() as u32,
            plates,
            cell_plates,
            crust_kinds,
            crust_thickness_km,
            boundaries,
            segments,
        )?;
        snapshot.validate_against(spatial)?;
        Ok(snapshot)
    }
}

fn generate_plates(
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    streams: &LabeledSubstreams,
) -> (Vec<Plate>, PlateIdField) {
    let mut rng = streams.stream(PLATE_SEEDS_LABEL);
    let seeds = farthest_point_seeds(topology, spec.plate_count as usize, rng.next_u64());
    let assignment = multi_source_ownership(topology, &seeds);
    let plates = seeds
        .into_iter()
        .enumerate()
        .map(|(index, seed_cell)| Plate {
            id: PlateId::from_raw(index as u32),
            seed_cell,
            velocity: PlateVelocity::new(0, 0)
                .expect("zero velocity is inside the fixed physical bound"),
        })
        .collect();
    let cell_plates = PlateIdField::from_raw(assignment.owners);
    (plates, cell_plates)
}

fn generate_crust(
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    streams: &LabeledSubstreams,
) -> (CrustKindField, Vec<f32>) {
    let boundary_sources: Vec<_> = topology
        .boundary_cells()
        .iter()
        .enumerate()
        .filter_map(|(index, &boundary)| boundary.then_some(CellId::from_raw(index as u32)))
        .collect();
    let boundary_distance = multi_source_distance(topology, &boundary_sources, None);
    let nucleus_count = crust_nucleus_count(topology.arcs().len());
    let mut seed_rng = streams.stream(CRUST_SEEDS_LABEL);
    let continental_nuclei = select_nuclei(
        &boundary_distance,
        nucleus_count,
        true,
        &BTreeSet::new(),
        &mut seed_rng,
    );
    let excluded: BTreeSet<_> = continental_nuclei.iter().copied().collect();
    let oceanic_nuclei = select_nuclei(
        &boundary_distance,
        nucleus_count.saturating_add(1),
        false,
        &excluded,
        &mut seed_rng,
    );

    let continental_distance = multi_source_distance(topology, &continental_nuclei, None);
    let oceanic_distance = multi_source_distance(topology, &oceanic_nuclei, None);
    let mut shape_rng = streams.stream(CRUST_SHAPE_LABEL);
    let shape_noise = smooth_noise(
        topology,
        random_noise(topology.arcs().len(), &mut shape_rng),
        CRUST_SMOOTHING_PASSES,
    );
    let typical_cost = typical_traversal_cost(topology) as i128;
    let mut ranked_cells: Vec<_> = (0..topology.arcs().len())
        .map(|index| {
            let contrast = continental_distance[index] as i128 - oceanic_distance[index] as i128;
            let perturbation =
                i128::from(shape_noise[index]) * typical_cost / i128::from(CRUST_NOISE_SCALE * 2);
            (
                contrast.saturating_add(perturbation),
                CellId::from_raw(index as u32),
            )
        })
        .collect();
    ranked_cells.sort_by_key(|&(score, cell)| (score, cell));
    let continental_count = closest_area_prefix(
        &ranked_cells,
        topology.area_weights(),
        spec.continental_crust_fraction,
    );
    let mut kinds = vec![CrustKind::Oceanic; topology.arcs().len()];
    for &(_, cell) in ranked_cells.iter().take(continental_count) {
        kinds[cell.raw() as usize] = CrustKind::Continental;
    }

    let thickness = generate_crust_thickness(topology, &kinds, streams);
    (CrustKindField::from_kinds(kinds), thickness)
}

fn crust_nucleus_count(cell_count: usize) -> usize {
    let proposed = ((cell_count as f64).sqrt() / 18.0).round() as usize;
    proposed.clamp(2, 24).min(cell_count.saturating_sub(1) / 2)
}

fn select_nuclei(
    boundary_distance: &[u64],
    requested: usize,
    prefer_interior: bool,
    excluded: &BTreeSet<CellId>,
    rng: &mut impl RngCore,
) -> Vec<CellId> {
    let mut candidates: Vec<_> = boundary_distance
        .iter()
        .enumerate()
        .filter_map(|(index, &distance)| {
            let cell = CellId::from_raw(index as u32);
            (!excluded.contains(&cell)).then_some((distance, rng.next_u64(), cell))
        })
        .collect();
    if prefer_interior {
        candidates.sort_by(|first, second| {
            second
                .0
                .cmp(&first.0)
                .then_with(|| first.1.cmp(&second.1))
                .then_with(|| first.2.cmp(&second.2))
        });
    } else {
        candidates.sort_by_key(|&(distance, random, cell)| (distance, random, cell));
    }
    candidates
        .into_iter()
        .take(requested)
        .map(|(_, _, cell)| cell)
        .collect()
}

fn closest_area_prefix(
    ranked_cells: &[(i128, CellId)],
    area_weights: &[u64],
    fraction: f32,
) -> usize {
    let fraction = (f64::from(fraction) * FRACTION_QUANTIZATION as f64).round() as u64;
    let total: u128 = area_weights.iter().map(|&area| u128::from(area)).sum();
    let target = total * u128::from(fraction);
    let scale = u128::from(FRACTION_QUANTIZATION);
    let mut cumulative = 0_u128;
    let mut best_count = 1_usize;
    let mut best_error = u128::MAX;
    for (index, &(_, cell)) in ranked_cells
        .iter()
        .take(ranked_cells.len().saturating_sub(1))
        .enumerate()
    {
        cumulative += u128::from(area_weights[cell.raw() as usize]);
        let weighted = cumulative * scale;
        let error = weighted.abs_diff(target);
        if error < best_error {
            best_error = error;
            best_count = index + 1;
        }
    }
    best_count
}

fn generate_crust_thickness(
    topology: &NaturalTopologyIndex,
    kinds: &[CrustKind],
    streams: &LabeledSubstreams,
) -> Vec<f32> {
    let transition_cells: Vec<_> = topology
        .arcs()
        .iter()
        .enumerate()
        .filter_map(|(index, arcs)| {
            arcs.iter()
                .any(|arc| kinds[arc.neighbor.raw() as usize] != kinds[index])
                .then_some(CellId::from_raw(index as u32))
        })
        .collect();
    let transition_distance = multi_source_distance(topology, &transition_cells, None);
    let mut rng = streams.stream(CRUST_THICKNESS_LABEL);
    let variation = smooth_noise(
        topology,
        random_noise(kinds.len(), &mut rng),
        THICKNESS_SMOOTHING_PASSES,
    );
    let typical_cost = typical_traversal_cost(topology).max(1);

    kinds
        .iter()
        .enumerate()
        .map(|(index, &kind)| {
            let depth_steps = (transition_distance[index] / typical_cost).min(16) as f32;
            let regional = variation[index] as f32 / CRUST_NOISE_SCALE as f32;
            match kind {
                CrustKind::Oceanic => (6.5 + depth_steps * 0.25 + regional * 1.25).clamp(
                    OCEANIC_CRUST_MIN_THICKNESS_KM,
                    OCEANIC_CRUST_MAX_THICKNESS_KM,
                ),
                CrustKind::Continental => (30.0 + depth_steps * 1.35 + regional * 4.0).clamp(
                    CONTINENTAL_CRUST_MIN_THICKNESS_KM,
                    CONTINENTAL_CRUST_MAX_THICKNESS_KM,
                ),
            }
        })
        .collect()
}

fn random_noise(count: usize, rng: &mut impl RngCore) -> Vec<i64> {
    (0..count)
        .map(|_| i64::from(rng.next_u32() % (CRUST_NOISE_SCALE as u32 * 2 + 1)) - CRUST_NOISE_SCALE)
        .collect()
}

fn smooth_noise(topology: &NaturalTopologyIndex, mut values: Vec<i64>, passes: usize) -> Vec<i64> {
    for _ in 0..passes {
        let previous = values;
        values = topology
            .arcs()
            .iter()
            .enumerate()
            .map(|(index, arcs)| {
                let neighbor_sum: i128 = arcs
                    .iter()
                    .map(|arc| i128::from(previous[arc.neighbor.raw() as usize]))
                    .sum();
                let numerator = i128::from(previous[index]) * 2 + neighbor_sum;
                let denominator = (arcs.len() + 2) as i128;
                (numerator / denominator) as i64
            })
            .collect();
    }
    values
}

fn typical_traversal_cost(topology: &NaturalTopologyIndex) -> u64 {
    let mut costs: Vec<_> = topology
        .arcs()
        .iter()
        .flatten()
        .map(|arc| arc.traversal_cost)
        .collect();
    costs.sort_unstable();
    costs[costs.len() / 2]
}

fn provisional_boundaries(
    spatial: &SpatialSnapshot,
    cell_plates: &PlateIdField,
) -> (Vec<BoundaryRecord>, Vec<BoundarySegment>) {
    let mut boundaries = vec![BoundaryRecord::none(); spatial.edges().len()];
    let mut segments = Vec::new();
    for edge in spatial.edges() {
        let [Some(first), Some(second)] = edge.cells else {
            continue;
        };
        let first_plate = cell_plates
            .get(first.raw() as usize)
            .expect("plate field is cell aligned");
        let second_plate = cell_plates
            .get(second.raw() as usize)
            .expect("plate field is cell aligned");
        if first_plate == second_plate {
            continue;
        }
        let segment_id = BoundarySegmentId::from_raw(segments.len() as u32);
        boundaries[edge.id.raw() as usize] = BoundaryRecord::new(
            BoundaryKind::Weak,
            PROVISIONAL_BOUNDARY_STRENGTH,
            Some(segment_id),
            None,
        );
        segments.push(BoundarySegment {
            id: segment_id,
            plates: normalized_plate_pair(first_plate, second_plate),
            kind: BoundaryKind::Weak,
            member_edges: vec![edge.id],
            mean_strength: PROVISIONAL_BOUNDARY_STRENGTH,
            subducting_plate: None,
            direction: normalized_edge_direction(edge),
        });
    }
    (boundaries, segments)
}

fn normalized_plate_pair(first: PlateId, second: PlateId) -> [PlateId; 2] {
    if first < second {
        [first, second]
    } else {
        [second, first]
    }
}

fn normalized_edge_direction(edge: &crate::world::spatial::SpatialEdge) -> [f32; 2] {
    let dx = edge.end.x().get() - edge.start.x().get();
    let dy = edge.end.y().get() - edge.start.y().get();
    let length = dx.hypot(dy);
    [(dx / length) as f32, (dy / length) as f32]
}

/// Errors returned when a natural tectonic snapshot cannot be generated.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TectonicGenerationError {
    /// The requested tectonic specification is invalid.
    #[error("invalid tectonic specification: {0}")]
    InvalidSpec(#[from] NaturalSpecError),
    /// The requested plate count exceeds the available spatial cells.
    #[error("requested {plates} plates for only {cells} spatial cells")]
    PlateCountExceedsCells {
        /// The requested number of plates.
        plates: u16,
        /// The number of available cells.
        cells: usize,
    },
    /// Generated tectonic data violated a snapshot invariant.
    #[error("generated tectonic snapshot is invalid: {0}")]
    InvalidSnapshot(#[from] TectonicValidationError),
}

#[cfg(test)]
mod tests {
    use super::closest_area_prefix;
    use crate::world::CellId;

    #[test]
    fn area_prefix_selects_the_closest_non_extreme_partition() {
        let ranked = (0..4)
            .map(|index| (index as i128, CellId::from_raw(index)))
            .collect::<Vec<_>>();
        assert_eq!(closest_area_prefix(&ranked, &[2, 2, 2, 2], 0.38), 2);
        assert_eq!(closest_area_prefix(&ranked, &[7, 1, 1, 1], 0.10), 1);
        assert_eq!(closest_area_prefix(&ranked, &[1, 1, 1, 7], 0.75), 3);
    }
}
