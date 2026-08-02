use std::collections::{BTreeMap, BTreeSet};

use rand::RngCore;
use thiserror::Error;

use super::random::{
    LabeledSubstreams, CRUST_SEEDS_LABEL, CRUST_SHAPE_LABEL, CRUST_THICKNESS_LABEL,
    PLATE_MOTION_LABEL, PLATE_SEEDS_LABEL,
};
use super::topology::{
    farthest_point_seeds, multi_source_distance, multi_source_ownership, NaturalTopologyIndex,
};
use crate::engine::StageRng;
use crate::world::natural::{
    BoundaryKind, BoundaryRecord, BoundarySegment, CrustKind, CrustKindField, NaturalSpecError,
    Plate, PlateIdField, PlateVelocity, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    TectonicActivity, TectonicSnapshot, TectonicSpec, TectonicValidationError,
    WorldFormationSpecError, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, MAX_PLATE_VELOCITY_MM_PER_YEAR,
    OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM, TECTONIC_SNAPSHOT_SCHEMA_V1,
};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::{BoundarySegmentId, CellId, EdgeId, PlateId, WorldPoint};

const FRACTION_QUANTIZATION: u64 = 1_000_000;
const CRUST_NOISE_SCALE: i64 = 1_000;
const CRUST_SMOOTHING_PASSES: usize = 3;
const THICKNESS_SMOOTHING_PASSES: usize = 4;
const BOUNDARY_ENDPOINT_QUANTIZATION: f64 = 1_000_000_000.0;
const WEAK_RELATIVE_SPEED_MM_PER_YEAR: i64 = 8;

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
        formation: &ResolvedWorldFormation,
        rng: &mut StageRng,
    ) -> Result<TectonicSnapshot, TectonicGenerationError> {
        spec.validate()?;
        formation.validate()?;
        if spec.plate_count as usize > spatial.cell_count() {
            return Err(TectonicGenerationError::PlateCountExceedsCells {
                plates: spec.plate_count,
                cells: spatial.cell_count(),
            });
        }

        let streams = LabeledSubstreams::capture(rng);
        let topology = NaturalTopologyIndex::new(spatial);
        let (plates, cell_plates) = generate_plates(&topology, spec, &streams)?;
        let (crust_kinds, crust_thickness_km) =
            generate_crust(&topology, spec, formation.resolved(), &streams)?;
        let (boundaries, segments) = classify_and_aggregate_boundaries(
            spatial,
            &topology,
            &plates,
            &cell_plates,
            &crust_kinds,
            &crust_thickness_km,
        );
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
) -> Result<(Vec<Plate>, PlateIdField), TectonicGenerationError> {
    let mut rng = streams.stream(PLATE_SEEDS_LABEL);
    let seeds = farthest_point_seeds(topology, spec.plate_count as usize, rng.next_u64());
    let assignment = multi_source_ownership(topology, &seeds);
    let mut plates: Vec<_> = seeds
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
    assign_plate_velocities(topology, &cell_plates, spec.activity, streams, &mut plates)?;
    Ok((plates, cell_plates))
}

fn assign_plate_velocities(
    topology: &NaturalTopologyIndex,
    cell_plates: &PlateIdField,
    activity: TectonicActivity,
    streams: &LabeledSubstreams,
    plates: &mut [Plate],
) -> Result<(), TectonicGenerationError> {
    let adjacency = plate_adjacency(topology, cell_plates, plates.len());
    let candidates = velocity_candidates(activity);
    let mut rng = streams.stream(PLATE_MOTION_LABEL);
    for plate_index in 0..plates.len() {
        let assigned_neighbors: Vec<_> = adjacency[plate_index]
            .iter()
            .filter(|neighbor| (neighbor.raw() as usize) < plate_index)
            .map(|neighbor| plates[neighbor.raw() as usize].velocity)
            .collect();
        plates[plate_index].velocity =
            select_velocity_candidate(&candidates, rng.next_u64(), &assigned_neighbors);
    }

    let minimum = minimum_relative_speed(activity);
    let minimum_squared = i64::from(minimum) * i64::from(minimum);
    for (first_index, neighbors) in adjacency.iter().enumerate() {
        for &second in neighbors {
            if second.raw() as usize <= first_index {
                continue;
            }
            let first = plates[first_index].velocity;
            let second_velocity = plates[second.raw() as usize].velocity;
            if relative_speed_squared(first, second_velocity) < minimum_squared {
                return Err(TectonicGenerationError::UnsatisfiedRelativeMotion {
                    first: PlateId::from_raw(first_index as u32),
                    second,
                    minimum,
                });
            }
        }
    }
    Ok(())
}

fn plate_adjacency(
    topology: &NaturalTopologyIndex,
    cell_plates: &PlateIdField,
    plate_count: usize,
) -> Vec<BTreeSet<PlateId>> {
    let mut adjacency = vec![BTreeSet::new(); plate_count];
    for &[first, second] in topology.edge_owners() {
        let [Some(first), Some(second)] = [first, second] else {
            continue;
        };
        let first_plate = cell_plates
            .get(first.raw() as usize)
            .expect("plate field is cell aligned");
        let second_plate = cell_plates
            .get(second.raw() as usize)
            .expect("plate field is cell aligned");
        if first_plate != second_plate {
            adjacency[first_plate.raw() as usize].insert(second_plate);
            adjacency[second_plate.raw() as usize].insert(first_plate);
        }
    }
    adjacency
}

fn velocity_candidates(activity: TectonicActivity) -> Vec<PlateVelocity> {
    let (limit, step) = match activity {
        TectonicActivity::Quiet => (48_i16, 12_usize),
        TectonicActivity::Moderate => (96_i16, 24_usize),
        TectonicActivity::Active => (120_i16, 40_usize),
    };
    let mut candidates = Vec::new();
    for y in (-limit..=limit).step_by(step) {
        for x in (-limit..=limit).step_by(step) {
            candidates.push(
                PlateVelocity::new(x, y).expect("fixed velocity lattice is inside physical bounds"),
            );
        }
    }
    candidates
}

fn select_velocity_candidate(
    candidates: &[PlateVelocity],
    rotation: u64,
    assigned_neighbors: &[PlateVelocity],
) -> PlateVelocity {
    let start = rotation as usize % candidates.len();
    if assigned_neighbors.is_empty() {
        return candidates[start];
    }

    let mut best = candidates[start];
    let mut best_score = i64::MIN;
    for offset in 0..candidates.len() {
        let candidate = candidates[(start + offset) % candidates.len()];
        let score = assigned_neighbors
            .iter()
            .map(|&neighbor| relative_speed_squared(candidate, neighbor))
            .min()
            .expect("assigned neighbor list is non-empty");
        if score > best_score {
            best = candidate;
            best_score = score;
        }
    }
    best
}

fn minimum_relative_speed(activity: TectonicActivity) -> i16 {
    match activity {
        TectonicActivity::Quiet => 12,
        TectonicActivity::Moderate => 24,
        TectonicActivity::Active => 40,
    }
}

fn relative_speed_squared(first: PlateVelocity, second: PlateVelocity) -> i64 {
    let first = first.components_mm_per_year();
    let second = second.components_mm_per_year();
    let dx = i64::from(second[0]) - i64::from(first[0]);
    let dy = i64::from(second[1]) - i64::from(first[1]);
    dx * dx + dy * dy
}

fn generate_crust(
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
) -> Result<(CrustKindField, Vec<f32>), TectonicGenerationError> {
    let boundary_sources: Vec<_> = topology
        .boundary_cells()
        .iter()
        .enumerate()
        .filter_map(|(index, &boundary)| boundary.then_some(CellId::from_raw(index as u32)))
        .collect();
    let boundary_distance = multi_source_distance(topology, &boundary_sources, None);
    let profile = CrustFormationProfile::for_preset(preset);
    let desired_frame = topology.quantized_short_side_fraction(0.04);
    let mut seed_rng = streams.stream(CRUST_SEEDS_LABEL);
    let (continental_nuclei, maximum_frame) = spread_crust_nuclei(
        topology,
        &boundary_distance,
        desired_frame,
        profile.nucleus_count,
        profile.primary_interior,
        &mut seed_rng,
    );
    if continental_nuclei.is_empty() {
        return Err(TectonicGenerationError::InsufficientCrustFormationArea {
            requested_area_weight: crust_target_weight(
                topology.area_weights(),
                spec.continental_crust_fraction,
            ) / u128::from(FRACTION_QUANTIZATION),
            available_area_weight: 0,
        });
    }
    let assignment = multi_source_ownership(topology, &continental_nuclei);
    let nucleus_cells: BTreeSet<_> = continental_nuclei.iter().copied().collect();
    let divider_cells = ownership_dividers(topology, &assignment.owners);
    let mut shape_rng = streams.stream(CRUST_SHAPE_LABEL);
    let shape_noise = smooth_noise(
        topology,
        random_noise(topology.arcs().len(), &mut shape_rng),
        CRUST_SMOOTHING_PASSES,
    );
    let typical_cost = typical_traversal_cost(topology) as i128;
    let target_weight =
        crust_target_weight(topology.area_weights(), spec.continental_crust_fraction);
    let frame_options = [maximum_frame, maximum_frame * 2 / 3, maximum_frame / 3, 0];
    let mut best_available = 0_u128;
    let mut ranked_cells = None;
    'corridor_fallback: for preserve_corridors in [profile.hard_corridor, false] {
        for frame in frame_options {
            let scores: Vec<_> = (0..topology.arcs().len())
                .map(|index| {
                    let cell = CellId::from_raw(index as u32);
                    if boundary_distance[index] <= frame
                        || (preserve_corridors
                            && divider_cells[index]
                            && !nucleus_cells.contains(&cell))
                    {
                        return None;
                    }
                    let owner = assignment.owners[index] as usize;
                    let distance_score = i128::from(assignment.distances[index])
                        * i128::from(profile.owner_scale_permille(owner));
                    let perturbation = i128::from(shape_noise[index])
                        * typical_cost
                        * i128::from(profile.shape_noise_permille)
                        / i128::from(CRUST_NOISE_SCALE * 1_000);
                    let score = if nucleus_cells.contains(&cell) {
                        i128::MIN / 2 + owner as i128
                    } else {
                        distance_score.saturating_add(perturbation)
                    };
                    Some((score, cell))
                })
                .collect();
            let candidates = connected_crust_order(topology, &scores, &continental_nuclei);
            let available = candidates
                .iter()
                .map(|&(_, cell)| u128::from(topology.area_weights()[cell.raw() as usize]))
                .sum();
            best_available = best_available.max(available);
            if available * u128::from(FRACTION_QUANTIZATION) >= target_weight {
                ranked_cells = Some(candidates);
                break 'corridor_fallback;
            }
        }
    }
    let ranked_cells =
        ranked_cells.ok_or(TectonicGenerationError::InsufficientCrustFormationArea {
            requested_area_weight: target_weight / u128::from(FRACTION_QUANTIZATION),
            available_area_weight: best_available,
        })?;
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
    Ok((CrustKindField::from_kinds(kinds), thickness))
}

const CONTINENT_OWNER_SCALES: [u16; 4] = [1_000; 4];
const ARCHIPELAGO_OWNER_SCALES: [u16; 12] = [1_000; 12];
const SUPERCONTINENT_OWNER_SCALES: [u16; 1] = [1_000];
const GREAT_ISLAND_OWNER_SCALES: [u16; 4] = [800, 2_500, 2_200, 3_500];
const VOLCANIC_ISLAND_OWNER_SCALES: [u16; 10] = [1_000; 10];

#[derive(Debug, Clone, Copy)]
struct CrustFormationProfile {
    nucleus_count: usize,
    hard_corridor: bool,
    owner_scales_permille: &'static [u16],
    shape_noise_permille: i64,
    primary_interior: bool,
}

impl CrustFormationProfile {
    const fn for_preset(preset: ResolvedWorldFormationPreset) -> Self {
        match preset {
            ResolvedWorldFormationPreset::Continents => Self {
                nucleus_count: CONTINENT_OWNER_SCALES.len(),
                hard_corridor: true,
                owner_scales_permille: &CONTINENT_OWNER_SCALES,
                shape_noise_permille: 1_500,
                primary_interior: false,
            },
            ResolvedWorldFormationPreset::Archipelago => Self {
                nucleus_count: ARCHIPELAGO_OWNER_SCALES.len(),
                hard_corridor: true,
                owner_scales_permille: &ARCHIPELAGO_OWNER_SCALES,
                shape_noise_permille: 1_250,
                primary_interior: false,
            },
            ResolvedWorldFormationPreset::Supercontinent => Self {
                nucleus_count: SUPERCONTINENT_OWNER_SCALES.len(),
                hard_corridor: false,
                owner_scales_permille: &SUPERCONTINENT_OWNER_SCALES,
                shape_noise_permille: 1_500,
                primary_interior: true,
            },
            ResolvedWorldFormationPreset::GreatIsland => Self {
                nucleus_count: GREAT_ISLAND_OWNER_SCALES.len(),
                hard_corridor: true,
                owner_scales_permille: &GREAT_ISLAND_OWNER_SCALES,
                shape_noise_permille: 1_250,
                primary_interior: true,
            },
            ResolvedWorldFormationPreset::VolcanicIslands => Self {
                nucleus_count: VOLCANIC_ISLAND_OWNER_SCALES.len(),
                hard_corridor: true,
                owner_scales_permille: &VOLCANIC_ISLAND_OWNER_SCALES,
                shape_noise_permille: 1_000,
                primary_interior: false,
            },
        }
    }

    fn owner_scale_permille(self, owner: usize) -> u16 {
        self.owner_scales_permille[owner % self.owner_scales_permille.len()]
    }
}

fn spread_crust_nuclei(
    topology: &NaturalTopologyIndex,
    boundary_distance: &[u64],
    desired_frame: u64,
    requested: usize,
    primary_interior: bool,
    rng: &mut impl RngCore,
) -> (Vec<CellId>, u64) {
    let mut maximum_frame = desired_frame;
    let mut candidates: Vec<_> = boundary_distance
        .iter()
        .enumerate()
        .filter_map(|(index, &distance)| {
            (distance > maximum_frame).then_some(CellId::from_raw(index as u32))
        })
        .collect();
    if candidates.len() < requested {
        maximum_frame = 0;
        candidates = boundary_distance
            .iter()
            .enumerate()
            .filter_map(|(index, &distance)| {
                (distance > 0).then_some(CellId::from_raw(index as u32))
            })
            .collect();
    }
    if candidates.is_empty() {
        return (Vec::new(), maximum_frame);
    }

    let count = requested.min(candidates.len());
    let rotation = rng.next_u64() as usize % candidates.len();
    let first = if primary_interior {
        candidates
            .iter()
            .copied()
            .max_by_key(|cell| {
                (
                    boundary_distance[cell.raw() as usize],
                    std::cmp::Reverse(*cell),
                )
            })
            .expect("candidate list is non-empty")
    } else {
        candidates[rotation]
    };
    let mut nuclei = vec![first];
    let mut minimum_squared_distance = vec![u128::MAX; candidates.len()];
    while nuclei.len() < count {
        let newest = *nuclei.last().expect("one nucleus was selected");
        let newest_center = topology.quantized_centers()[newest.raw() as usize];
        for (candidate_index, &candidate) in candidates.iter().enumerate() {
            let center = topology.quantized_centers()[candidate.raw() as usize];
            let dx = i128::from(center[0]) - i128::from(newest_center[0]);
            let dy = i128::from(center[1]) - i128::from(newest_center[1]);
            let squared = (dx * dx + dy * dy) as u128;
            minimum_squared_distance[candidate_index] =
                minimum_squared_distance[candidate_index].min(squared);
        }
        let next = (0..candidates.len())
            .filter(|&index| !nuclei.contains(&candidates[index]))
            .max_by_key(|&index| {
                (
                    minimum_squared_distance[index],
                    std::cmp::Reverse((index + candidates.len() - rotation) % candidates.len()),
                )
            })
            .expect("at least one unselected candidate remains");
        nuclei.push(candidates[next]);
    }
    (nuclei, maximum_frame)
}

fn ownership_dividers(topology: &NaturalTopologyIndex, owners: &[u32]) -> Vec<bool> {
    topology
        .arcs()
        .iter()
        .enumerate()
        .map(|(index, arcs)| {
            arcs.iter()
                .any(|arc| owners[arc.neighbor.raw() as usize] != owners[index])
        })
        .collect()
}

fn connected_crust_order(
    topology: &NaturalTopologyIndex,
    scores: &[Option<(i128, CellId)>],
    nuclei: &[CellId],
) -> Vec<(i128, CellId)> {
    let mut frontier = BTreeSet::new();
    let mut queued = vec![false; scores.len()];
    for &nucleus in nuclei {
        let index = nucleus.raw() as usize;
        if let Some(candidate) = scores[index] {
            frontier.insert(candidate);
            queued[index] = true;
        }
    }

    let mut ordered = Vec::new();
    while let Some(candidate @ (_, cell)) = frontier.pop_first() {
        ordered.push(candidate);
        for arc in &topology.arcs()[cell.raw() as usize] {
            let neighbor_index = arc.neighbor.raw() as usize;
            if !queued[neighbor_index] {
                if let Some(neighbor) = scores[neighbor_index] {
                    frontier.insert(neighbor);
                    queued[neighbor_index] = true;
                }
            }
        }
    }
    ordered
}

fn crust_target_weight(area_weights: &[u64], fraction: f32) -> u128 {
    let fraction = (f64::from(fraction) * FRACTION_QUANTIZATION as f64).round() as u64;
    let total: u128 = area_weights.iter().map(|&area| u128::from(area)).sum();
    total * u128::from(fraction)
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

#[derive(Debug, Clone)]
struct BoundaryEventDraft {
    edge: EdgeId,
    plates: [PlateId; 2],
    kind: BoundaryKind,
    strength: f32,
    subducting_plate: Option<PlateId>,
    endpoints: [[i64; 2]; 2],
    direction: [i64; 2],
}

#[derive(Debug, Clone, Copy)]
struct KinematicClassification {
    kind: BoundaryKind,
    strength: f32,
    subducting_plate: Option<PlateId>,
}

fn classify_and_aggregate_boundaries(
    spatial: &SpatialSnapshot,
    topology: &NaturalTopologyIndex,
    plates: &[Plate],
    cell_plates: &PlateIdField,
    crust_kinds: &CrustKindField,
    crust_thickness_km: &[f32],
) -> (Vec<BoundaryRecord>, Vec<BoundarySegment>) {
    let mut events = Vec::new();
    for edge in spatial.edges() {
        let [Some(first), Some(second)] = topology.edge_owners()[edge.id.raw() as usize] else {
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
        let first_index = first.raw() as usize;
        let second_index = second.raw() as usize;
        let first_center = topology.quantized_centers()[first_index];
        let second_center = topology.quantized_centers()[second_index];
        let normal = [
            second_center[0] - first_center[0],
            second_center[1] - first_center[1],
        ];
        let classification = classify_kinematics(
            [first_plate, second_plate],
            [
                plates[first_plate.raw() as usize].velocity,
                plates[second_plate.raw() as usize].velocity,
            ],
            normal,
            [
                crust_kinds
                    .get(first_index)
                    .expect("crust field is cell aligned"),
                crust_kinds
                    .get(second_index)
                    .expect("crust field is cell aligned"),
            ],
            [
                crust_thickness_km[first_index],
                crust_thickness_km[second_index],
            ],
        );
        let endpoints = quantized_edge_endpoints(spatial, edge);
        events.push(BoundaryEventDraft {
            edge: edge.id,
            plates: normalized_plate_pair(first_plate, second_plate),
            kind: classification.kind,
            strength: classification.strength,
            subducting_plate: classification.subducting_plate,
            endpoints,
            direction: [
                endpoints[1][0] - endpoints[0][0],
                endpoints[1][1] - endpoints[0][1],
            ],
        });
    }

    aggregate_boundary_events(spatial.edges().len(), &events)
}

fn classify_kinematics(
    plates: [PlateId; 2],
    velocities: [PlateVelocity; 2],
    normal: [i64; 2],
    crust: [CrustKind; 2],
    thickness_km: [f32; 2],
) -> KinematicClassification {
    let first_velocity = velocities[0].components_mm_per_year();
    let second_velocity = velocities[1].components_mm_per_year();
    let relative = [
        i64::from(second_velocity[0]) - i64::from(first_velocity[0]),
        i64::from(second_velocity[1]) - i64::from(first_velocity[1]),
    ];
    let speed_squared = relative[0] * relative[0] + relative[1] * relative[1];
    let maximum_speed = f32::from(MAX_PLATE_VELOCITY_MM_PER_YEAR) * 2.0_f32.sqrt();
    let strength = ((speed_squared as f32).sqrt() / maximum_speed).clamp(0.0, 1.0);
    if speed_squared < WEAK_RELATIVE_SPEED_MM_PER_YEAR * WEAK_RELATIVE_SPEED_MM_PER_YEAR {
        return KinematicClassification {
            kind: BoundaryKind::Weak,
            strength,
            subducting_plate: None,
        };
    }

    let normal_squared = i128::from(normal[0]) * i128::from(normal[0])
        + i128::from(normal[1]) * i128::from(normal[1]);
    let projection = i128::from(relative[0]) * i128::from(normal[0])
        + i128::from(relative[1]) * i128::from(normal[1]);
    let has_strong_normal_component = normal_squared > 0
        && projection * projection * 100 >= i128::from(speed_squared) * normal_squared * 16;
    if !has_strong_normal_component {
        return KinematicClassification {
            kind: BoundaryKind::Transform,
            strength,
            subducting_plate: None,
        };
    }

    if projection < 0 {
        if crust == [CrustKind::Continental, CrustKind::Continental] {
            KinematicClassification {
                kind: BoundaryKind::ContinentalCollision,
                strength,
                subducting_plate: None,
            }
        } else {
            KinematicClassification {
                kind: BoundaryKind::Subduction,
                strength,
                subducting_plate: Some(select_subducting_plate(plates, crust, thickness_km)),
            }
        }
    } else {
        let kind = if crust == [CrustKind::Oceanic, CrustKind::Oceanic] {
            BoundaryKind::OceanicRidge
        } else {
            BoundaryKind::ContinentalRift
        };
        KinematicClassification {
            kind,
            strength,
            subducting_plate: None,
        }
    }
}

fn select_subducting_plate(
    plates: [PlateId; 2],
    crust: [CrustKind; 2],
    thickness_km: [f32; 2],
) -> PlateId {
    match crust {
        [CrustKind::Oceanic, CrustKind::Continental] => plates[0],
        [CrustKind::Continental, CrustKind::Oceanic] => plates[1],
        _ => {
            if thickness_km[0] < thickness_km[1] {
                plates[0]
            } else if thickness_km[1] < thickness_km[0] {
                plates[1]
            } else {
                plates[0].min(plates[1])
            }
        }
    }
}

fn quantized_edge_endpoints(
    spatial: &SpatialSnapshot,
    edge: &crate::world::spatial::SpatialEdge,
) -> [[i64; 2]; 2] {
    let bounds = spatial.bounds();
    let scale = bounds.width().get().max(bounds.height().get());
    let mut endpoints = [
        quantized_point(edge.start, bounds.min(), scale),
        quantized_point(edge.end, bounds.min(), scale),
    ];
    if endpoints[1] < endpoints[0] {
        endpoints.swap(0, 1);
    }
    endpoints
}

fn quantized_point(point: WorldPoint, origin: WorldPoint, scale: f64) -> [i64; 2] {
    [
        (((point.x().get() - origin.x().get()) / scale) * BOUNDARY_ENDPOINT_QUANTIZATION).round()
            as i64,
        (((point.y().get() - origin.y().get()) / scale) * BOUNDARY_ENDPOINT_QUANTIZATION).round()
            as i64,
    ]
}

fn aggregate_boundary_events(
    edge_count: usize,
    events: &[BoundaryEventDraft],
) -> (Vec<BoundaryRecord>, Vec<BoundarySegment>) {
    let mut endpoint_members = BTreeMap::<[i64; 2], Vec<usize>>::new();
    for (index, event) in events.iter().enumerate() {
        endpoint_members
            .entry(event.endpoints[0])
            .or_default()
            .push(index);
        endpoint_members
            .entry(event.endpoints[1])
            .or_default()
            .push(index);
    }
    let mut union = StableUnionFind::new(events.len());
    for members in endpoint_members.values() {
        for first_index in 0..members.len() {
            for second_index in (first_index + 1)..members.len() {
                let first = members[first_index];
                let second = members[second_index];
                if boundary_events_are_compatible(&events[first], &events[second]) {
                    union.union(first, second);
                }
            }
        }
    }

    let mut by_root = BTreeMap::<usize, Vec<usize>>::new();
    for index in 0..events.len() {
        let root = union.find(index);
        by_root.entry(root).or_default().push(index);
    }
    let mut components: Vec<_> = by_root.into_values().collect();
    for component in &mut components {
        component.sort_by_key(|&index| events[index].edge);
    }
    components.sort_by_key(|component| events[component[0]].edge);

    let mut boundaries = vec![BoundaryRecord::none(); edge_count];
    let mut segments = Vec::with_capacity(components.len());
    for (segment_index, component) in components.into_iter().enumerate() {
        let segment_id = BoundarySegmentId::from_raw(segment_index as u32);
        let first_event = &events[component[0]];
        let member_edges: Vec<_> = component.iter().map(|&index| events[index].edge).collect();
        let mean_strength = component
            .iter()
            .map(|&index| events[index].strength)
            .sum::<f32>()
            / component.len() as f32;
        for &index in &component {
            let event = &events[index];
            boundaries[event.edge.raw() as usize] = BoundaryRecord::new(
                event.kind,
                event.strength,
                Some(segment_id),
                event.subducting_plate,
            );
        }
        segments.push(BoundarySegment {
            id: segment_id,
            plates: first_event.plates,
            kind: first_event.kind,
            member_edges,
            mean_strength,
            subducting_plate: first_event.subducting_plate,
            direction: aggregate_direction(&component, events),
        });
    }
    (boundaries, segments)
}

fn boundary_events_are_compatible(first: &BoundaryEventDraft, second: &BoundaryEventDraft) -> bool {
    first.plates == second.plates
        && first.kind == second.kind
        && first.subducting_plate == second.subducting_plate
        && directions_are_compatible(first.direction, second.direction)
}

fn directions_are_compatible(first: [i64; 2], second: [i64; 2]) -> bool {
    let dot =
        i128::from(first[0]) * i128::from(second[0]) + i128::from(first[1]) * i128::from(second[1]);
    let first_squared =
        i128::from(first[0]) * i128::from(first[0]) + i128::from(first[1]) * i128::from(first[1]);
    let second_squared = i128::from(second[0]) * i128::from(second[0])
        + i128::from(second[1]) * i128::from(second[1]);
    dot * dot * 4 >= first_squared * second_squared
}

fn aggregate_direction(component: &[usize], events: &[BoundaryEventDraft]) -> [f32; 2] {
    let mut x = 0_i128;
    let mut y = 0_i128;
    for &index in component {
        x += i128::from(events[index].direction[0]);
        y += i128::from(events[index].direction[1]);
    }
    let length = ((x as f64).powi(2) + (y as f64).powi(2)).sqrt();
    if length == 0.0 {
        let fallback = events[component[0]].direction;
        let fallback_length = (fallback[0] as f64).hypot(fallback[1] as f64);
        [
            (fallback[0] as f64 / fallback_length) as f32,
            (fallback[1] as f64 / fallback_length) as f32,
        ]
    } else {
        [(x as f64 / length) as f32, (y as f64 / length) as f32]
    }
}

struct StableUnionFind {
    parent: Vec<usize>,
}

impl StableUnionFind {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count).collect(),
        }
    }

    fn find(&mut self, index: usize) -> usize {
        let parent = self.parent[index];
        if parent != index {
            self.parent[index] = self.find(parent);
        }
        self.parent[index]
    }

    fn union(&mut self, first: usize, second: usize) {
        let first = self.find(first);
        let second = self.find(second);
        if first == second {
            return;
        }
        let (root, child) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        self.parent[child] = root;
    }
}

fn normalized_plate_pair(first: PlateId, second: PlateId) -> [PlateId; 2] {
    if first < second {
        [first, second]
    } else {
        [second, first]
    }
}

/// Errors returned when a natural tectonic snapshot cannot be generated.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TectonicGenerationError {
    /// The requested tectonic specification is invalid.
    #[error("invalid tectonic specification: {0}")]
    InvalidSpec(#[from] NaturalSpecError),
    /// The supplied resolved formation selection is invalid.
    #[error("invalid resolved world formation: {0}")]
    InvalidFormation(#[from] WorldFormationSpecError),
    /// The requested plate count exceeds the available spatial cells.
    #[error("requested {plates} plates for only {cells} spatial cells")]
    PlateCountExceedsCells {
        /// The requested number of plates.
        plates: u16,
        /// The number of available cells.
        cells: usize,
    },
    /// The required land fraction cannot fit after preserving the formal ocean frame.
    #[error(
        "continental crust needs area weight {requested_area_weight}, but only {available_area_weight} remains inside the ocean frame"
    )]
    InsufficientCrustFormationArea {
        /// Quantized area required by the explicit tectonic specification.
        requested_area_weight: u128,
        /// Quantized non-frame area available to continental crust.
        available_area_weight: u128,
    },
    /// The fixed velocity lattice could not separate one adjacent plate pair.
    #[error(
        "plates {first:?} and {second:?} do not reach the required {minimum} mm/year relative speed"
    )]
    UnsatisfiedRelativeMotion {
        /// The first adjacent plate.
        first: PlateId,
        /// The second adjacent plate.
        second: PlateId,
        /// The required minimum relative speed.
        minimum: i16,
    },
    /// Generated tectonic data violated a snapshot invariant.
    #[error("generated tectonic snapshot is invalid: {0}")]
    InvalidSnapshot(#[from] TectonicValidationError),
}

#[cfg(test)]
mod tests {
    use super::{
        classify_kinematics, closest_area_prefix, select_velocity_candidate, velocity_candidates,
    };
    use crate::world::natural::{BoundaryKind, CrustKind, PlateVelocity, TectonicActivity};
    use crate::world::{CellId, PlateId};

    #[test]
    fn area_prefix_selects_the_closest_non_extreme_partition() {
        let ranked = (0..4)
            .map(|index| (index as i128, CellId::from_raw(index)))
            .collect::<Vec<_>>();
        assert_eq!(closest_area_prefix(&ranked, &[2, 2, 2, 2], 0.38), 2);
        assert_eq!(closest_area_prefix(&ranked, &[7, 1, 1, 1], 0.10), 1);
        assert_eq!(closest_area_prefix(&ranked, &[1, 1, 1, 7], 0.75), 3);
    }

    fn velocity(x: i16, y: i16) -> PlateVelocity {
        PlateVelocity::new(x, y).unwrap()
    }

    #[test]
    fn stable_candidate_order_breaks_equal_motion_scores() {
        let candidates = velocity_candidates(TectonicActivity::Moderate);
        assert_eq!(
            select_velocity_candidate(&candidates, 0, &[velocity(0, 0)]),
            velocity(-96, -96)
        );
    }

    #[test]
    fn classifies_handcrafted_relative_motion_and_subduction_polarity() {
        let first = PlateId::from_raw(0);
        let second = PlateId::from_raw(1);
        let normal = [1_000_i64, 0_i64];

        let collision = classify_kinematics(
            [first, second],
            [velocity(30, 0), velocity(-30, 0)],
            normal,
            [CrustKind::Continental, CrustKind::Continental],
            [35.0, 36.0],
        );
        assert_eq!(collision.kind, BoundaryKind::ContinentalCollision);

        let subduction = classify_kinematics(
            [first, second],
            [velocity(30, 0), velocity(-30, 0)],
            normal,
            [CrustKind::Oceanic, CrustKind::Continental],
            [7.0, 35.0],
        );
        assert_eq!(subduction.kind, BoundaryKind::Subduction);
        assert_eq!(subduction.subducting_plate, Some(first));

        let rift = classify_kinematics(
            [first, second],
            [velocity(-30, 0), velocity(30, 0)],
            normal,
            [CrustKind::Continental, CrustKind::Continental],
            [35.0, 36.0],
        );
        assert_eq!(rift.kind, BoundaryKind::ContinentalRift);

        let ridge = classify_kinematics(
            [first, second],
            [velocity(-30, 0), velocity(30, 0)],
            normal,
            [CrustKind::Oceanic, CrustKind::Oceanic],
            [7.0, 8.0],
        );
        assert_eq!(ridge.kind, BoundaryKind::OceanicRidge);

        let transform = classify_kinematics(
            [first, second],
            [velocity(0, -30), velocity(0, 30)],
            normal,
            [CrustKind::Oceanic, CrustKind::Continental],
            [7.0, 35.0],
        );
        assert_eq!(transform.kind, BoundaryKind::Transform);

        let weak = classify_kinematics(
            [first, second],
            [velocity(0, 0), velocity(2, 0)],
            normal,
            [CrustKind::Oceanic, CrustKind::Continental],
            [7.0, 35.0],
        );
        assert_eq!(weak.kind, BoundaryKind::Weak);
    }
}
