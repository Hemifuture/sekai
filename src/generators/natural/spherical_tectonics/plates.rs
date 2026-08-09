#![cfg_attr(not(test), allow(dead_code))]

use std::collections::VecDeque;

use rand::RngCore;
use thiserror::Error;

use crate::generators::natural::morphology::arrival::{
    assign_arrivals, ArrivalAssignment, ArrivalError, ArrivalSource, ArrivalWorkspace,
};
use crate::generators::natural::morphology::field::{
    sample_spherical_field, FieldBand, FieldRecipe, FieldShape, MorphologyFieldError,
    QuantizedScalarField,
};
use crate::generators::natural::morphology::metric::{
    build_plate_metric, EdgeMetricError, PositiveEdgeMetric,
};
use crate::generators::natural::random::{
    LabeledSubstreams, PLATE_FABRIC_FIELD_LABEL, PLATE_RESISTANCE_FIELD_LABEL,
    PLATE_SEED_PLACEMENT_LABEL, PLATE_TARGET_AREA_LABEL,
};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{PlateIdField, TectonicSpec};
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::{CellId, PlateId};

pub(super) const AREA_WEIGHT_TOTAL: u64 = 1_000_000_000;
const MAXIMUM_CALIBRATION_ROUNDS: usize = 6;
const MINIMUM_SEPARATION_SCORE: f64 = 0.20;
const TOP_CANDIDATE_DIVISOR: usize = 20;
const FIELD_CLAMP_SIGMA_MILLI: u16 = 3_000;
const METRIC_DISTANCE_SCALE: f64 = 1_000_000.0;

const SEED_PREFERENCE_BANDS: [FieldBand; 2] = [
    FieldBand {
        angular_scale_rad: 120.0_f64.to_radians(),
        weight_milli: 700,
        shape: FieldShape::Smooth,
    },
    FieldBand {
        angular_scale_rad: 55.0_f64.to_radians(),
        weight_milli: 300,
        shape: FieldShape::Smooth,
    },
];
const SEED_PREFERENCE_RECIPE: FieldRecipe = FieldRecipe {
    bands: &SEED_PREFERENCE_BANDS,
    clamp_sigma_milli: FIELD_CLAMP_SIGMA_MILLI,
};
const PLATE_RESISTANCE_BANDS: [FieldBand; 3] = [
    FieldBand {
        angular_scale_rad: 100.0_f64.to_radians(),
        weight_milli: 550,
        shape: FieldShape::Smooth,
    },
    FieldBand {
        angular_scale_rad: 42.0_f64.to_radians(),
        weight_milli: 300,
        shape: FieldShape::Smooth,
    },
    FieldBand {
        angular_scale_rad: 16.0_f64.to_radians(),
        weight_milli: 150,
        shape: FieldShape::Ridged,
    },
];
const PLATE_RESISTANCE_RECIPE: FieldRecipe = FieldRecipe {
    bands: &PLATE_RESISTANCE_BANDS,
    clamp_sigma_milli: FIELD_CLAMP_SIGMA_MILLI,
};
const PLATE_FABRIC_BANDS: [FieldBand; 2] = [
    FieldBand {
        angular_scale_rad: 75.0_f64.to_radians(),
        weight_milli: 650,
        shape: FieldShape::Smooth,
    },
    FieldBand {
        angular_scale_rad: 28.0_f64.to_radians(),
        weight_milli: 350,
        shape: FieldShape::Smooth,
    },
];
const PLATE_FABRIC_RECIPE: FieldRecipe = FieldRecipe {
    bands: &PLATE_FABRIC_BANDS,
    clamp_sigma_milli: FIELD_CLAMP_SIGMA_MILLI,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::generators::natural) struct PlatePartition {
    pub(in crate::generators::natural) seeds: Vec<CellId>,
    pub(in crate::generators::natural) target_area_weights: Box<[u64]>,
    pub(in crate::generators::natural) owners: PlateIdField,
    pub(in crate::generators::natural) achieved_area_weights: Box<[u64]>,
}

struct PlacedSeeds {
    seeds: Vec<CellId>,
    source_distances: Vec<Box<[u64]>>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(in crate::generators::natural) enum PlateMorphologyError {
    #[error("requested {plates} plates for only {cells} spherical cells")]
    PlateCountExceedsCells { plates: usize, cells: usize },
    #[error(
        "plate morphology surface has {surface_cells} cells but topology has {topology_cells}"
    )]
    CardinalityMismatch {
        surface_cells: usize,
        topology_cells: usize,
    },
    #[error("target-area weights cannot be normalized for {plates} plates")]
    InvalidTargetWeights { plates: usize },
    #[error("not enough separated spherical cells remain for plate {plate:?}")]
    SeedPlacementExhausted { plate: PlateId },
    #[error("plate {plate:?} seed {seed:?} lost its own arrival region")]
    SeedOwnershipLost { plate: PlateId, seed: CellId },
    #[error("plate {plate:?} has no cells")]
    EmptyPlate { plate: PlateId },
    #[error("plate {plate:?} is disconnected")]
    DisconnectedPlate { plate: PlateId },
    #[error("no valid plate partition was produced")]
    NoValidPartition,
    #[error("plate scalar field failed: {0}")]
    Field(#[from] MorphologyFieldError),
    #[error("plate edge metric failed: {0}")]
    Metric(#[from] EdgeMetricError),
    #[error("plate arrival propagation failed: {0}")]
    Arrival(#[from] ArrivalError),
}

pub(in crate::generators::natural) fn generate_plate_partition(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    streams: &LabeledSubstreams,
) -> Result<PlatePartition, PlateMorphologyError> {
    generate_plate_partition_observed(surface, topology, spec, streams, |_| {})
}

fn generate_plate_partition_observed(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    streams: &LabeledSubstreams,
    observe_error: impl FnMut(f64),
) -> Result<PlatePartition, PlateMorphologyError> {
    let plate_count = usize::from(spec.plate_count);
    if surface.cells().len() != topology.cell_count() {
        return Err(PlateMorphologyError::CardinalityMismatch {
            surface_cells: surface.cells().len(),
            topology_cells: topology.cell_count(),
        });
    }
    if plate_count == 0 || plate_count > topology.cell_count() {
        return Err(PlateMorphologyError::PlateCountExceedsCells {
            plates: plate_count,
            cells: topology.cell_count(),
        });
    }

    let mut target_rng = streams.stream(PLATE_TARGET_AREA_LABEL);
    let targets = generate_target_area_weights(plate_count, &mut target_rng)?;

    let mut placement_rng = streams.stream(PLATE_SEED_PLACEMENT_LABEL);
    let seed_preference =
        sample_spherical_field(surface, SEED_PREFERENCE_RECIPE, placement_rng.next_u32())?;
    let resistance_seed = streams.stream(PLATE_RESISTANCE_FIELD_LABEL).next_u32();
    let resistance = sample_spherical_field(surface, PLATE_RESISTANCE_RECIPE, resistance_seed)?;
    let fabric = sample_plate_fabric(surface, streams)?;
    let metric = build_plate_metric(topology, &resistance, &fabric)?;

    let placed = place_plate_seeds(
        topology,
        &metric,
        &seed_preference,
        &targets,
        &mut placement_rng,
    )?;
    calibrate_partition_observed(
        topology,
        &metric,
        placed.seeds,
        targets,
        &placed.source_distances,
        observe_error,
    )
}

pub(super) fn sample_plate_fabric(
    surface: &SphericalSurfaceSnapshot,
    streams: &LabeledSubstreams,
) -> Result<QuantizedScalarField, MorphologyFieldError> {
    let fabric_seed = streams.stream(PLATE_FABRIC_FIELD_LABEL).next_u32();
    sample_spherical_field(surface, PLATE_FABRIC_RECIPE, fabric_seed)
}

fn generate_target_area_weights(
    plate_count: usize,
    rng: &mut impl RngCore,
) -> Result<Box<[u64]>, PlateMorphologyError> {
    if plate_count == 0 || plate_count as u64 > AREA_WEIGHT_TOTAL {
        return Err(PlateMorphologyError::InvalidTargetWeights {
            plates: plate_count,
        });
    }
    let denominator = (plate_count - 1).max(1) as f64;
    let mut raw = (0..plate_count)
        .map(|rank| {
            let profile = if plate_count >= 8 {
                0.55 + 1.35 * rank as f64 / denominator
            } else {
                0.70 + 0.70 * rank as f64 / denominator
            };
            let unit = (rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64;
            let perturbation = 0.90 + 0.20 * unit;
            (profile * perturbation).clamp(0.45, 2.40)
        })
        .collect::<Vec<_>>();
    for index in (1..raw.len()).rev() {
        let swap = (rng.next_u64() % (index as u64 + 1)) as usize;
        raw.swap(index, swap);
    }

    let raw_total = raw.iter().sum::<f64>();
    if !raw_total.is_finite() || raw_total <= 0.0 {
        return Err(PlateMorphologyError::InvalidTargetWeights {
            plates: plate_count,
        });
    }
    let scaled = raw
        .iter()
        .map(|value| value / raw_total * AREA_WEIGHT_TOTAL as f64)
        .collect::<Vec<_>>();
    let mut weights = scaled
        .iter()
        .map(|value| value.floor() as u64)
        .collect::<Vec<_>>();
    let assigned = weights.iter().sum::<u64>();
    let remainder = (AREA_WEIGHT_TOTAL - assigned) as usize;
    let mut fractional_order = scaled
        .iter()
        .enumerate()
        .map(|(index, value)| (value - value.floor(), index))
        .collect::<Vec<_>>();
    fractional_order.sort_by(|first, second| {
        second
            .0
            .total_cmp(&first.0)
            .then_with(|| first.1.cmp(&second.1))
    });
    for &(_, index) in fractional_order.iter().take(remainder) {
        weights[index] += 1;
    }
    if weights.contains(&0) || weights.iter().sum::<u64>() != AREA_WEIGHT_TOTAL {
        return Err(PlateMorphologyError::InvalidTargetWeights {
            plates: plate_count,
        });
    }
    Ok(weights.into_boxed_slice())
}

fn place_plate_seeds(
    topology: &NaturalTopologyIndex,
    metric: &PositiveEdgeMetric,
    preference: &QuantizedScalarField,
    targets: &[u64],
    rng: &mut impl RngCore,
) -> Result<PlacedSeeds, PlateMorphologyError> {
    let plate_count = targets.len();
    let mut order = (0..plate_count).collect::<Vec<_>>();
    order.sort_by_key(|&plate| (std::cmp::Reverse(targets[plate]), plate));
    let radii = targets
        .iter()
        .map(|&target| (target as f64 / AREA_WEIGHT_TOTAL as f64).sqrt())
        .collect::<Vec<_>>();
    let mut seeds = vec![None; plate_count];
    let mut distance_by_plate: Vec<Option<Box<[u64]>>> = vec![None; plate_count];
    let mut selected = vec![false; topology.cell_count()];
    let mut selected_plates: Vec<usize> = Vec::with_capacity(plate_count);
    let mut workspace = ArrivalWorkspace::default();

    for &plate in &order {
        let mut candidates = Vec::with_capacity(topology.cell_count() - selected_plates.len());
        for (index, &is_selected) in selected.iter().enumerate() {
            if is_selected {
                continue;
            }
            let cell = CellId::from_raw(index as u32);
            let separation = if selected_plates.is_empty() {
                1.0
            } else {
                selected_plates
                    .iter()
                    .map(|&other| {
                        let distance = distance_by_plate[other]
                            .as_ref()
                            .expect("selected plates have source distances")[index]
                            as f64
                            / METRIC_DISTANCE_SCALE;
                        distance / (radii[plate] + radii[other])
                    })
                    .fold(f64::INFINITY, f64::min)
            };
            if selected_plates.is_empty() || separation >= MINIMUM_SEPARATION_SCORE {
                let score = separation + preference.normalized_f64(cell) * 0.12;
                candidates.push((score, cell));
            }
        }
        if candidates.is_empty() {
            return Err(PlateMorphologyError::SeedPlacementExhausted {
                plate: PlateId::from_raw(plate as u32),
            });
        }
        candidates.sort_by(|first, second| {
            second
                .0
                .total_cmp(&first.0)
                .then_with(|| first.1.cmp(&second.1))
        });
        let shortlist = candidates.len().div_ceil(TOP_CANDIDATE_DIVISOR).max(1);
        let chosen = candidates[(rng.next_u64() % shortlist as u64) as usize].1;
        selected[chosen.raw() as usize] = true;
        seeds[plate] = Some(chosen);
        selected_plates.push(plate);

        let assignment = assign_arrivals(
            topology,
            metric,
            &[ArrivalSource {
                owner: plate as u32,
                cell: chosen,
                initial_cost: 0,
            }],
            &mut workspace,
        )?;
        distance_by_plate[plate] = Some(assignment.costs);
    }

    Ok(PlacedSeeds {
        seeds: seeds
            .into_iter()
            .map(|seed| seed.expect("every plate was placed"))
            .collect(),
        source_distances: distance_by_plate
            .into_iter()
            .map(|distances| distances.expect("every plate was placed"))
            .collect(),
    })
}

fn calibrate_partition(
    topology: &NaturalTopologyIndex,
    metric: &PositiveEdgeMetric,
    seeds: Vec<CellId>,
    targets: Box<[u64]>,
    source_distances: &[Box<[u64]>],
) -> Result<PlatePartition, PlateMorphologyError> {
    calibrate_partition_observed(topology, metric, seeds, targets, source_distances, |_| {})
}

fn calibrate_partition_observed(
    topology: &NaturalTopologyIndex,
    metric: &PositiveEdgeMetric,
    seeds: Vec<CellId>,
    targets: Box<[u64]>,
    source_distances: &[Box<[u64]>],
    mut observe_error: impl FnMut(f64),
) -> Result<PlatePartition, PlateMorphologyError> {
    let characteristic_distance = median_nearest_seed_distance(&seeds, source_distances).max(1);
    let mut biases = vec![0_i64; seeds.len()];
    let bias_limit = (0.60 * characteristic_distance as f64).round() as i64;
    let delta_limit = (0.12 * characteristic_distance as f64).round() as i64;
    let mut workspace = ArrivalWorkspace::default();
    let mut best: Option<(f64, ArrivalAssignment, Box<[u64]>)> = None;
    let mut previous_error = f64::INFINITY;
    let mut small_improvements = 0;

    for _ in 0..MAXIMUM_CALIBRATION_ROUNDS {
        let minimum_bias = *biases.iter().min().expect("plate count is nonzero");
        let sources = seeds
            .iter()
            .enumerate()
            .map(|(owner, &cell)| ArrivalSource {
                owner: owner as u32,
                cell,
                initial_cost: (biases[owner] - minimum_bias) as u64,
            })
            .collect::<Vec<_>>();
        let assignment = assign_arrivals(topology, metric, &sources, &mut workspace)?;
        if validate_partition(topology, &seeds, &assignment).is_err() {
            break;
        }
        let achieved = achieved_area_weights(topology, &assignment, seeds.len())?;
        let maximum_error = maximum_relative_error(&achieved, &targets);
        observe_error(maximum_error);
        if best
            .as_ref()
            .is_none_or(|(best_error, _, _)| maximum_error < *best_error)
        {
            best = Some((maximum_error, assignment.clone(), achieved.clone()));
        }

        let improvement = previous_error - maximum_error;
        if improvement.is_finite() && improvement < 0.005 {
            small_improvements += 1;
        } else {
            small_improvements = 0;
        }
        if small_improvements >= 2 {
            break;
        }
        previous_error = maximum_error;

        for plate in 0..seeds.len() {
            let error = (achieved[plate] as f64 - targets[plate] as f64) / targets[plate] as f64;
            let delta = (0.35 * characteristic_distance as f64 * error)
                .round()
                .clamp(-(delta_limit as f64), delta_limit as f64) as i64;
            biases[plate] = biases[plate]
                .saturating_add(delta)
                .clamp(-bias_limit, bias_limit);
        }
    }

    let Some((_, assignment, achieved)) = best else {
        return Err(PlateMorphologyError::NoValidPartition);
    };
    validate_partition(topology, &seeds, &assignment)?;
    Ok(PlatePartition {
        seeds,
        target_area_weights: targets,
        owners: PlateIdField::from_raw(assignment.owners.into_vec()),
        achieved_area_weights: achieved,
    })
}

fn median_nearest_seed_distance(seeds: &[CellId], distances: &[Box<[u64]>]) -> u64 {
    let mut nearest = seeds
        .iter()
        .enumerate()
        .map(|(plate, _)| {
            seeds
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != plate)
                .map(|(_, seed)| distances[plate][seed.raw() as usize])
                .min()
                .expect("tectonic specifications contain at least two plates")
        })
        .collect::<Vec<_>>();
    nearest.sort_unstable();
    nearest[nearest.len() / 2]
}

fn achieved_area_weights(
    topology: &NaturalTopologyIndex,
    assignment: &ArrivalAssignment,
    plate_count: usize,
) -> Result<Box<[u64]>, PlateMorphologyError> {
    let mut achieved = vec![0_u64; plate_count];
    for (index, &owner) in assignment.owners.iter().enumerate() {
        let Some(total) = achieved.get_mut(owner as usize) else {
            return Err(PlateMorphologyError::EmptyPlate {
                plate: PlateId::from_raw(owner),
            });
        };
        *total = total.saturating_add(topology.area_weights()[index]);
    }
    for (plate, &area) in achieved.iter().enumerate() {
        if area == 0 {
            return Err(PlateMorphologyError::EmptyPlate {
                plate: PlateId::from_raw(plate as u32),
            });
        }
    }
    Ok(achieved.into_boxed_slice())
}

fn maximum_relative_error(achieved: &[u64], targets: &[u64]) -> f64 {
    achieved
        .iter()
        .zip(targets)
        .map(|(&actual, &target)| (actual as f64 - target as f64).abs() / target as f64)
        .fold(0.0, f64::max)
}

fn validate_partition(
    topology: &NaturalTopologyIndex,
    seeds: &[CellId],
    assignment: &ArrivalAssignment,
) -> Result<(), PlateMorphologyError> {
    for (owner, &seed) in seeds.iter().enumerate() {
        if assignment.owners[seed.raw() as usize] != owner as u32 {
            return Err(PlateMorphologyError::SeedOwnershipLost {
                plate: PlateId::from_raw(owner as u32),
                seed,
            });
        }
        let expected = assignment
            .owners
            .iter()
            .filter(|&&candidate| candidate as usize == owner)
            .count();
        if expected == 0 {
            return Err(PlateMorphologyError::EmptyPlate {
                plate: PlateId::from_raw(owner as u32),
            });
        }
        let mut visited = vec![false; topology.cell_count()];
        let mut queue = VecDeque::from([seed]);
        visited[seed.raw() as usize] = true;
        let mut reached = 0;
        while let Some(cell) = queue.pop_front() {
            reached += 1;
            for arc in &topology.arcs()[cell.raw() as usize] {
                let index = arc.neighbor.raw() as usize;
                if !visited[index] && assignment.owners[index] as usize == owner {
                    visited[index] = true;
                    queue.push_back(arc.neighbor);
                }
            }
        }
        if reached != expected {
            return Err(PlateMorphologyError::DisconnectedPlate {
                plate: PlateId::from_raw(owner as u32),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::f64::consts::PI;

    use rand::{RngCore, SeedableRng};
    use rand_chacha::ChaCha8Rng;

    use super::{
        calibrate_partition, generate_plate_partition, generate_plate_partition_observed,
        generate_target_area_weights, maximum_relative_error, AREA_WEIGHT_TOTAL,
    };
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::morphology::arrival::{
        assign_arrivals, ArrivalSource, ArrivalWorkspace,
    };
    use crate::generators::natural::morphology::metric::PositiveEdgeMetric;
    use crate::generators::natural::random::{LabeledSubstreams, PLATE_MOTION_LABEL};
    use crate::generators::natural::tectonics::generate_plate_partition as generate_uniform;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{TectonicActivity, TectonicSpec};
    use crate::world::spatial::SphericalNaturalSurface;
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    fn stage_rng(seed: u64) -> StageRng {
        StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("spherical-plate-morphology-test", 2, "sekai.test"),
        ))
    }

    fn fixture(
        target_cell_count: u32,
        seed: u64,
    ) -> (
        crate::world::spatial::SphericalSurfaceSnapshot,
        NaturalTopologyIndex,
        LabeledSubstreams,
    ) {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count,
        })
        .unwrap();
        let view = SphericalNaturalSurface::new(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let streams = LabeledSubstreams::capture(&mut stage_rng(seed));
        (surface, topology, streams)
    }

    fn area_coefficient_of_variation(values: &[u64]) -> f64 {
        let mean = values.iter().map(|&value| value as f64).sum::<f64>() / values.len() as f64;
        let variance = values
            .iter()
            .map(|&value| (value as f64 - mean).powi(2))
            .sum::<f64>()
            / values.len() as f64;
        variance.sqrt() / mean
    }

    fn median_normalized_perimeter(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        owners: &crate::world::natural::PlateIdField,
        plate_count: usize,
    ) -> f64 {
        let mut areas = vec![0.0; plate_count];
        let mut perimeters = vec![0.0; plate_count];
        for cell in surface.cells() {
            let owner = owners.get(cell.id.raw() as usize).unwrap().raw() as usize;
            areas[owner] += cell.area.get();
        }
        for edge in surface.edges() {
            let pair = edge
                .cells
                .map(|cell| owners.get(cell.raw() as usize).unwrap().raw() as usize);
            if pair[0] != pair[1] {
                perimeters[pair[0]] += edge.length.get();
                perimeters[pair[1]] += edge.length.get();
            }
        }
        let radius = surface.radius().get();
        let mut normalized = areas
            .into_iter()
            .zip(perimeters)
            .map(|(area, perimeter)| {
                let alpha = (1.0 - area / (2.0 * PI * radius * radius))
                    .clamp(-1.0, 1.0)
                    .acos();
                perimeter / (2.0 * PI * radius * alpha.sin())
            })
            .collect::<Vec<_>>();
        normalized.sort_by(f64::total_cmp);
        normalized[normalized.len() / 2]
    }

    fn assert_all_plates_connected_and_contain_seed(
        topology: &NaturalTopologyIndex,
        partition: &super::PlatePartition,
    ) {
        for (owner, &seed) in partition.seeds.iter().enumerate() {
            assert_eq!(
                partition.owners.get(seed.raw() as usize).unwrap().raw() as usize,
                owner
            );
            let expected = partition
                .owners
                .raw_values()
                .iter()
                .filter(|&&candidate| candidate as usize == owner)
                .count();
            let mut visited = vec![false; topology.cell_count()];
            let mut queue = VecDeque::from([seed]);
            visited[seed.raw() as usize] = true;
            let mut reached = 0;
            while let Some(cell) = queue.pop_front() {
                reached += 1;
                for arc in &topology.arcs()[cell.raw() as usize] {
                    let index = arc.neighbor.raw() as usize;
                    if !visited[index]
                        && partition.owners.get(index).unwrap().raw() as usize == owner
                    {
                        visited[index] = true;
                        queue.push_back(arc.neighbor);
                    }
                }
            }
            assert_eq!(reached, expected, "plate {owner} is disconnected");
        }
    }

    #[test]
    fn default_targets_are_bounded_diverse_and_area_normalized() {
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let targets = generate_target_area_weights(12, &mut rng).unwrap();
        let minimum = *targets.iter().min().unwrap();
        let maximum = *targets.iter().max().unwrap();

        assert_eq!(targets.iter().sum::<u64>(), AREA_WEIGHT_TOTAL);
        assert!(maximum as f64 / minimum as f64 >= 2.75);
        assert!(targets.iter().all(|&value| value > 0));
    }

    #[test]
    fn field_driven_partition_is_connected_and_not_uniform_voronoi() {
        let (surface, topology, streams) = fixture(642, 42);
        let spec = TectonicSpec::default();
        let partition = generate_plate_partition(&surface, &topology, &spec, &streams).unwrap();
        let (_, uniform) = generate_uniform(&topology, &spec, &streams);

        assert_all_plates_connected_and_contain_seed(&topology, &partition);
        assert!(median_normalized_perimeter(&surface, &partition.owners, 12) > 1.15);
        assert!(area_coefficient_of_variation(&partition.achieved_area_weights) >= 0.30);
        assert_ne!(partition.owners, uniform);
    }

    #[test]
    fn six_bias_rounds_keep_the_best_valid_area_fit() {
        let (surface, topology, streams) = fixture(2_562, 91);
        let partition =
            generate_plate_partition(&surface, &topology, &TectonicSpec::default(), &streams)
                .unwrap();
        let maximum_relative_error = partition
            .achieved_area_weights
            .iter()
            .zip(partition.target_area_weights.iter())
            .map(|(&actual, &target)| (actual as f64 - target as f64).abs() / target as f64)
            .fold(0.0, f64::max);

        assert!(
            maximum_relative_error <= 0.35,
            "maximum error was {maximum_relative_error}"
        );
        assert_all_plates_connected_and_contain_seed(&topology, &partition);
    }

    #[test]
    fn plate_shape_is_independent_of_activity_and_motion_stream_consumption() {
        let (surface, topology, streams) = fixture(642, 113);
        let quiet = generate_plate_partition(
            &surface,
            &topology,
            &TectonicSpec {
                activity: TectonicActivity::Quiet,
                ..TectonicSpec::default()
            },
            &streams,
        )
        .unwrap();
        let active = generate_plate_partition(
            &surface,
            &topology,
            &TectonicSpec {
                activity: TectonicActivity::Active,
                ..TectonicSpec::default()
            },
            &streams,
        )
        .unwrap();
        let expected_motion = {
            let mut motion = streams.stream(PLATE_MOTION_LABEL);
            (0..8).map(|_| motion.next_u64()).collect::<Vec<_>>()
        };

        assert_eq!(quiet, active);
        let mut motion = streams.stream(PLATE_MOTION_LABEL);
        assert_eq!(
            expected_motion,
            (0..8).map(|_| motion.next_u64()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn plate_fields_change_final_ownership_for_the_same_seeds_and_targets() {
        let (surface, topology, streams) = fixture(642, 197);
        let field_driven =
            generate_plate_partition(&surface, &topology, &TectonicSpec::default(), &streams)
                .unwrap();
        let base_metric = PositiveEdgeMetric::from_topology_lengths(&topology).unwrap();
        let mut workspace = ArrivalWorkspace::default();
        let base_distances = field_driven
            .seeds
            .iter()
            .enumerate()
            .map(|(owner, &cell)| {
                assign_arrivals(
                    &topology,
                    &base_metric,
                    &[ArrivalSource {
                        owner: owner as u32,
                        cell,
                        initial_cost: 0,
                    }],
                    &mut workspace,
                )
                .unwrap()
                .costs
            })
            .collect::<Vec<_>>();
        let base = calibrate_partition(
            &topology,
            &base_metric,
            field_driven.seeds.clone(),
            field_driven.target_area_weights.clone(),
            &base_distances,
        )
        .unwrap();

        assert_ne!(field_driven.owners, base.owners);
    }

    #[test]
    fn calibration_publishes_the_best_valid_round_not_merely_the_last_round() {
        let (surface, topology, _) = fixture(642, 0);
        let mut observed_rebound = false;
        for seed in 0..32 {
            let streams = LabeledSubstreams::capture(&mut stage_rng(seed));
            let mut errors = Vec::new();
            let partition = generate_plate_partition_observed(
                &surface,
                &topology,
                &TectonicSpec::default(),
                &streams,
                |error| errors.push(error),
            )
            .unwrap();
            let best = errors.iter().copied().fold(f64::INFINITY, f64::min);
            let returned = maximum_relative_error(
                &partition.achieved_area_weights,
                &partition.target_area_weights,
            );
            assert!((returned - best).abs() <= f64::EPSILON);
            if errors
                .last()
                .is_some_and(|last| *last > best + f64::EPSILON)
            {
                observed_rebound = true;
                break;
            }
        }
        assert!(
            observed_rebound,
            "fixed seed set must exercise a calibration rebound"
        );
    }
}
