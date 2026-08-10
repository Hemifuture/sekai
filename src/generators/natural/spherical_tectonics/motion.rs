use rand::RngCore;
use thiserror::Error;

use super::plates::PlatePartition;
use crate::generators::natural::connectivity::normalized_plate_pair;
use crate::generators::natural::random::{LabeledSubstreams, PLATE_MOTION_LABEL};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{
    SphericalPlate, SphericalPlateRotation, SphericalTectonicValidationError, TectonicActivity,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
use crate::world::{EdgeId, Meters, PlateId};

const VELOCITY_QUANTIZATION: f64 = 1_000_000.0;
const EULER_POLES: [[i8; 3]; 26] = [
    [1, 0, 0],
    [-1, 0, 0],
    [0, 1, 0],
    [0, -1, 0],
    [0, 0, 1],
    [0, 0, -1],
    [1, 1, 0],
    [1, -1, 0],
    [-1, 1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [1, 0, -1],
    [-1, 0, 1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, 1, -1],
    [0, -1, 1],
    [0, -1, -1],
    [1, 1, 1],
    [1, 1, -1],
    [1, -1, 1],
    [1, -1, -1],
    [-1, 1, 1],
    [-1, 1, -1],
    [-1, -1, 1],
    [-1, -1, -1],
];

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum PlateMotionError {
    #[error(
        "plate motion has {seeds} seeds and {owners} owner cells for {topology_cells} topology cells"
    )]
    Cardinality {
        seeds: usize,
        owners: usize,
        topology_cells: usize,
    },
    #[error("invalid spherical plate rotation: {0}")]
    InvalidRotation(#[from] SphericalTectonicValidationError),
    #[error(
        "edge {edge:?} between plates {plates:?} reaches only {found_mm_per_year} mm/year, below the required {minimum_mm_per_year} mm/year"
    )]
    UnsatisfiedRelativeMotion {
        edge: EdgeId,
        plates: [PlateId; 2],
        minimum_mm_per_year: f64,
        found_mm_per_year: f64,
    },
}

pub(super) fn assign_plate_rotations(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    partition: &PlatePartition,
    activity: TectonicActivity,
    streams: &LabeledSubstreams,
) -> Result<Vec<SphericalPlate>, PlateMotionError> {
    if partition.seeds.len() != partition.target_area_weights.len()
        || partition.owners.len() != topology.cell_count()
    {
        return Err(PlateMotionError::Cardinality {
            seeds: partition.seeds.len(),
            owners: partition.owners.len(),
            topology_cells: topology.cell_count(),
        });
    }
    let rotations = rotation_candidates(surface.radius(), activity)?;
    let plate_count = partition.seeds.len();
    let mut boundary_edges = vec![Vec::<(EdgeId, PlateId)>::new(); plate_count];
    for edge in surface.edges() {
        let owner_plates = edge.cells.map(|cell| {
            partition
                .owners
                .get(cell.raw() as usize)
                .expect("plate ownership is aligned with the validated surface")
        });
        if owner_plates[0] == owner_plates[1] {
            continue;
        }
        boundary_edges[owner_plates[0].raw() as usize].push((edge.id, owner_plates[1]));
        boundary_edges[owner_plates[1].raw() as usize].push((edge.id, owner_plates[0]));
    }

    let mut rng = streams.stream(PLATE_MOTION_LABEL);
    let mut assigned = Vec::with_capacity(plate_count);
    for (plate_index, interfaces) in boundary_edges.iter().enumerate() {
        let start = (rng.next_u64() % rotations.len() as u64) as usize;
        let assigned_interfaces = interfaces
            .iter()
            .copied()
            .filter(|(_, neighbor)| (neighbor.raw() as usize) < plate_index)
            .collect::<Vec<_>>();
        if assigned_interfaces.is_empty() {
            assigned.push(rotations[start]);
            continue;
        }

        let mut best = rotations[start];
        let mut best_score = 0_u128;
        for offset in 0..rotations.len() {
            let candidate = rotations[(start + offset) % rotations.len()];
            let score = assigned_interfaces
                .iter()
                .map(|&(edge_id, neighbor)| {
                    let edge = surface
                        .edge(edge_id)
                        .expect("boundary edge came from the validated surface");
                    quantized_relative_speed_energy(
                        candidate,
                        assigned[neighbor.raw() as usize],
                        surface.radius(),
                        edge.midpoint,
                    )
                })
                .min()
                .expect("the assigned-interface set is non-empty");
            if score > best_score {
                best = candidate;
                best_score = score;
            }
        }
        assigned.push(best);
    }

    let minimum = minimum_relative_speed(activity);
    for edge in surface.edges() {
        let owner_plates = edge.cells.map(|cell| {
            partition
                .owners
                .get(cell.raw() as usize)
                .expect("plate ownership is aligned with the validated surface")
        });
        if owner_plates[0] == owner_plates[1] {
            continue;
        }
        let speed = relative_speed_at(
            assigned[owner_plates[0].raw() as usize],
            assigned[owner_plates[1].raw() as usize],
            surface.radius(),
            edge.midpoint,
        );
        if speed + 1.0e-9 < minimum {
            return Err(PlateMotionError::UnsatisfiedRelativeMotion {
                edge: edge.id,
                plates: normalized_plate_pair(owner_plates[0], owner_plates[1]),
                minimum_mm_per_year: minimum,
                found_mm_per_year: speed,
            });
        }
    }

    Ok(partition
        .seeds
        .iter()
        .copied()
        .zip(assigned)
        .enumerate()
        .map(|(index, (seed, rotation))| {
            SphericalPlate::new(PlateId::from_raw(index as u32), seed, rotation)
        })
        .collect())
}

fn rotation_candidates(
    radius: Meters,
    activity: TectonicActivity,
) -> Result<Vec<SphericalPlateRotation>, SphericalTectonicValidationError> {
    let speeds = match activity {
        TectonicActivity::Quiet => [24.0, 36.0, 48.0],
        TectonicActivity::Moderate => [48.0, 72.0, 96.0],
        TectonicActivity::Active => [60.0, 90.0, 120.0],
    };
    let mut candidates = Vec::with_capacity(EULER_POLES.len() * speeds.len());
    for components in EULER_POLES {
        let pole = UnitVector3::new(
            f64::from(components[0]),
            f64::from(components[1]),
            f64::from(components[2]),
        )
        .expect("the fixed Euler-pole set contains only nonzero finite vectors");
        for speed_mm_per_year in speeds {
            let angular_rate = (speed_mm_per_year * 1.0e9_f64 / radius.get()).floor() as u64;
            let rotation = SphericalPlateRotation::new(pole, angular_rate.max(1))?;
            rotation.validate_for_radius(radius)?;
            candidates.push(rotation);
        }
    }
    Ok(candidates)
}

fn minimum_relative_speed(activity: TectonicActivity) -> f64 {
    match activity {
        TectonicActivity::Quiet => 8.0,
        TectonicActivity::Moderate => 16.0,
        TectonicActivity::Active => 24.0,
    }
}

fn rotation_velocity(
    rotation: SphericalPlateRotation,
    radius: Meters,
    radial: UnitVector3,
) -> [f64; 3] {
    rotation
        .velocity_mm_per_year(radius, radial)
        .expect("generated Euler rotations were validated for the surface radius")
}

fn relative_velocity_at(
    first: SphericalPlateRotation,
    second: SphericalPlateRotation,
    radius: Meters,
    radial: UnitVector3,
) -> [f64; 3] {
    let first = rotation_velocity(first, radius, radial);
    let second = rotation_velocity(second, radius, radial);
    [
        second[0] - first[0],
        second[1] - first[1],
        second[2] - first[2],
    ]
}

fn relative_speed_at(
    first: SphericalPlateRotation,
    second: SphericalPlateRotation,
    radius: Meters,
    radial: UnitVector3,
) -> f64 {
    let relative = relative_velocity_at(first, second, radius, radial);
    (relative[0] * relative[0] + relative[1] * relative[1] + relative[2] * relative[2]).sqrt()
}

fn quantized_relative_speed_energy(
    first: SphericalPlateRotation,
    second: SphericalPlateRotation,
    radius: Meters,
    radial: UnitVector3,
) -> u128 {
    relative_velocity_at(first, second, radius, radial)
        .map(|component| (component * VELOCITY_QUANTIZATION).round() as i128)
        .into_iter()
        .map(|component| (component * component) as u128)
        .sum()
}
