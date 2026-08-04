use std::collections::BTreeMap;

use rand::RngCore;
use thiserror::Error;

use super::random::{LabeledSubstreams, PLATE_MOTION_LABEL};
use super::tectonics::{
    classify_local_kinematics, generate_crust, generate_plate_partition, normalized_plate_pair,
    CrustDomain, InsufficientCrustFormationArea, KinematicClassification, LocalKinematics,
    StableUnionFind, TectonicGenerator,
};
use super::topology::NaturalTopologyIndex;
use crate::engine::StageRng;
use crate::world::natural::{
    BoundaryKind, BoundaryRecord, CrustKind, CrustKindField, NaturalSpecError, PlateIdField,
    ResolvedWorldFormation, SphericalBoundarySegment, SphericalPlate, SphericalPlateRotation,
    SphericalTectonicSnapshot, SphericalTectonicValidationError, TectonicActivity, TectonicSpec,
    WorldFormationSpecError, TECTONIC_SNAPSHOT_SCHEMA_V2,
};
use crate::world::spatial::{
    NaturalSurface, SphericalNaturalSurface, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRefError, UnitVector3,
};
use crate::world::{BoundarySegmentId, EdgeId, Meters, PlateId, SurfaceVertexId};

const VELOCITY_QUANTIZATION: f64 = 1_000_000.0;
const WEAK_RELATIVE_SPEED_MM_PER_YEAR: f64 = 8.0;
const MAX_RELATIVE_SPEED_MM_PER_YEAR: f32 = 240.0;

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

impl TectonicGenerator {
    /// Generates a surface-bound V2 snapshot on a validated closed spherical world.
    ///
    /// Plate motion is stored as one rigid Euler rotation per plate. Boundary
    /// kinematics are evaluated in each authoritative edge's local tangent frame.
    pub fn generate_spherical(
        surface: &SphericalSurfaceSnapshot,
        spec: &TectonicSpec,
        formation: &ResolvedWorldFormation,
        rng: &mut StageRng,
    ) -> Result<SphericalTectonicSnapshot, SphericalTectonicGenerationError> {
        spec.validate()?;
        formation.validate()?;
        surface.validate()?;
        if spec.plate_count as usize > surface.cells().len() {
            return Err(SphericalTectonicGenerationError::PlateCountExceedsCells {
                plates: spec.plate_count,
                cells: surface.cells().len(),
            });
        }

        let view = SphericalNaturalSurface::from_validated(surface)?;
        let topology = NaturalTopologyIndex::from_surface(&view);
        let streams = LabeledSubstreams::capture(rng);
        let (seeds, cell_plates) = generate_plate_partition(&topology, spec, &streams);
        let rotations =
            assign_plate_rotations(surface, &cell_plates, spec.activity, &streams, seeds.len())?;
        let plates = seeds
            .into_iter()
            .zip(rotations)
            .enumerate()
            .map(|(index, (seed, rotation))| {
                SphericalPlate::new(PlateId::from_raw(index as u32), seed, rotation)
            })
            .collect::<Vec<_>>();
        let (crust_kinds, crust_thickness_km) = generate_crust(
            &topology,
            spec,
            formation.resolved(),
            &streams,
            CrustDomain::ClosedSurface,
        )?;
        let (boundaries, boundary_segments) = classify_and_aggregate_boundaries(
            surface,
            &plates,
            &cell_plates,
            &crust_kinds,
            &crust_thickness_km,
        );
        let snapshot = SphericalTectonicSnapshot::new(
            TECTONIC_SNAPSHOT_SCHEMA_V2,
            view.surface_ref(),
            plates,
            cell_plates,
            crust_kinds,
            crust_thickness_km,
            boundaries,
            boundary_segments,
        )?;
        snapshot.validate_against(surface)?;
        Ok(snapshot)
    }
}

fn assign_plate_rotations(
    surface: &SphericalSurfaceSnapshot,
    cell_plates: &PlateIdField,
    activity: TectonicActivity,
    streams: &LabeledSubstreams,
    plate_count: usize,
) -> Result<Vec<SphericalPlateRotation>, SphericalTectonicGenerationError> {
    let candidates = rotation_candidates(surface.radius(), activity)?;
    let mut boundary_edges = vec![Vec::<(EdgeId, PlateId)>::new(); plate_count];
    for edge in surface.edges() {
        let owner_plates = edge.cells.map(|cell| {
            cell_plates
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
        let start = (rng.next_u64() % candidates.len() as u64) as usize;
        let assigned_interfaces = interfaces
            .iter()
            .copied()
            .filter(|(_, neighbor)| (neighbor.raw() as usize) < plate_index)
            .collect::<Vec<_>>();
        if assigned_interfaces.is_empty() {
            assigned.push(candidates[start]);
            continue;
        }

        let mut best = candidates[start];
        let mut best_score = 0_u128;
        for offset in 0..candidates.len() {
            let candidate = candidates[(start + offset) % candidates.len()];
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
            cell_plates
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
            return Err(
                SphericalTectonicGenerationError::UnsatisfiedRelativeMotion {
                    edge: edge.id,
                    plates: normalized_plate_pair(owner_plates[0], owner_plates[1]),
                    minimum_mm_per_year: minimum,
                    found_mm_per_year: speed,
                },
            );
        }
    }
    Ok(assigned)
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

#[derive(Debug, Clone)]
struct BoundaryEventDraft {
    edge: EdgeId,
    vertices: [SurfaceVertexId; 2],
    plates: [PlateId; 2],
    kind: BoundaryKind,
    strength: f32,
    subducting_plate: Option<PlateId>,
}

fn classify_and_aggregate_boundaries(
    surface: &SphericalSurfaceSnapshot,
    plates: &[SphericalPlate],
    cell_plates: &PlateIdField,
    crust_kinds: &CrustKindField,
    crust_thickness_km: &[f32],
) -> (Vec<BoundaryRecord>, Vec<SphericalBoundarySegment>) {
    let mut events = Vec::new();
    for edge in surface.edges() {
        let owner_plates = edge.cells.map(|cell| {
            cell_plates
                .get(cell.raw() as usize)
                .expect("plate field is aligned with the validated surface")
        });
        if owner_plates[0] == owner_plates[1] {
            continue;
        }
        let indices = edge.cells.map(|cell| cell.raw() as usize);
        let classification = classify_kinematics(
            owner_plates,
            owner_plates.map(|plate| plates[plate.raw() as usize].rotation()),
            surface.radius(),
            edge.midpoint,
            edge.normal_from_first,
            indices.map(|index| {
                crust_kinds
                    .get(index)
                    .expect("crust field is aligned with the validated surface")
            }),
            indices.map(|index| crust_thickness_km[index]),
        );
        events.push(BoundaryEventDraft {
            edge: edge.id,
            vertices: edge.vertices,
            plates: normalized_plate_pair(owner_plates[0], owner_plates[1]),
            kind: classification.kind,
            strength: classification.strength,
            subducting_plate: classification.subducting_plate,
        });
    }
    aggregate_boundary_events(surface.edges().len(), &events)
}

#[allow(clippy::too_many_arguments)]
fn classify_kinematics(
    plates: [PlateId; 2],
    rotations: [SphericalPlateRotation; 2],
    radius: Meters,
    midpoint: UnitVector3,
    normal_from_first: UnitVector3,
    crust: [CrustKind; 2],
    thickness_km: [f32; 2],
) -> KinematicClassification {
    let relative = relative_velocity_at(rotations[0], rotations[1], radius, midpoint);
    let speed =
        (relative[0] * relative[0] + relative[1] * relative[1] + relative[2] * relative[2]).sqrt();
    let normal = normal_from_first.components();
    let signed_normal_speed =
        relative[0] * normal[0] + relative[1] * normal[1] + relative[2] * normal[2];
    let normal_speed = signed_normal_speed.abs();
    let tangent_speed = (speed * speed - normal_speed * normal_speed)
        .max(0.0)
        .sqrt();
    classify_local_kinematics(
        plates,
        crust,
        thickness_km,
        LocalKinematics {
            speed: speed as f32,
            normal_speed: normal_speed as f32,
            tangent_speed: tangent_speed as f32,
            maximum_relative_speed: MAX_RELATIVE_SPEED_MM_PER_YEAR,
            weak: speed < WEAK_RELATIVE_SPEED_MM_PER_YEAR,
            strong_normal_component: normal_speed >= speed * 0.4,
            converging: signed_normal_speed < 0.0,
        },
    )
}

fn aggregate_boundary_events(
    edge_count: usize,
    events: &[BoundaryEventDraft],
) -> (Vec<BoundaryRecord>, Vec<SphericalBoundarySegment>) {
    let mut vertex_members = BTreeMap::<SurfaceVertexId, Vec<usize>>::new();
    for (index, event) in events.iter().enumerate() {
        vertex_members
            .entry(event.vertices[0])
            .or_default()
            .push(index);
        vertex_members
            .entry(event.vertices[1])
            .or_default()
            .push(index);
    }
    let mut union = StableUnionFind::new(events.len());
    for members in vertex_members.values() {
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
    let mut components = by_root.into_values().collect::<Vec<_>>();
    for component in &mut components {
        component.sort_by_key(|&index| events[index].edge);
    }
    components.sort_by_key(|component| events[component[0]].edge);

    let mut boundaries = vec![BoundaryRecord::none(); edge_count];
    let mut segments = Vec::with_capacity(components.len());
    for (segment_index, component) in components.into_iter().enumerate() {
        let segment_id = BoundarySegmentId::from_raw(segment_index as u32);
        let first_event = &events[component[0]];
        let member_edges = component
            .iter()
            .map(|&index| events[index].edge)
            .collect::<Vec<_>>();
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
        segments.push(SphericalBoundarySegment::new(
            segment_id,
            first_event.plates,
            first_event.kind,
            member_edges,
            mean_strength,
            first_event.subducting_plate,
        ));
    }
    (boundaries, segments)
}

fn boundary_events_are_compatible(first: &BoundaryEventDraft, second: &BoundaryEventDraft) -> bool {
    first.plates == second.plates
        && first.kind == second.kind
        && first.subducting_plate == second.subducting_plate
}

/// Errors returned when a closed spherical tectonic snapshot cannot be generated.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalTectonicGenerationError {
    /// The requested tectonic specification is invalid.
    #[error("invalid tectonic specification: {0}")]
    InvalidSpec(#[from] NaturalSpecError),
    /// The supplied resolved formation selection is invalid.
    #[error("invalid resolved world formation: {0}")]
    InvalidFormation(#[from] WorldFormationSpecError),
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The authoritative spherical surface identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The requested plate count exceeds the available surface cells.
    #[error("requested {plates} plates for only {cells} spherical surface cells")]
    PlateCountExceedsCells {
        /// The requested number of plates.
        plates: u16,
        /// The number of available cells.
        cells: usize,
    },
    /// The required continental fraction cannot fit in the eligible surface area.
    #[error(
        "continental crust needs area weight {requested_area_weight}, but only {available_area_weight} is eligible"
    )]
    InsufficientCrustFormationArea {
        /// Quantized area required by the explicit tectonic specification.
        requested_area_weight: u128,
        /// Quantized surface area available to continental crust.
        available_area_weight: u128,
    },
    /// No fixed Euler candidate kept one plate interface above the activity floor.
    #[error(
        "edge {edge:?} between plates {plates:?} reaches only {found_mm_per_year} mm/year, below the required {minimum_mm_per_year} mm/year"
    )]
    UnsatisfiedRelativeMotion {
        /// The authoritative cross-plate edge.
        edge: EdgeId,
        /// The adjacent plate pair in ascending identifier order.
        plates: [PlateId; 2],
        /// The activity-dependent minimum relative speed.
        minimum_mm_per_year: f64,
        /// The generated local relative speed.
        found_mm_per_year: f64,
    },
    /// Generated spherical tectonic data violated a snapshot invariant.
    #[error("generated spherical tectonic snapshot is invalid: {0}")]
    InvalidSnapshot(#[from] SphericalTectonicValidationError),
}

impl From<InsufficientCrustFormationArea> for SphericalTectonicGenerationError {
    fn from(error: InsufficientCrustFormationArea) -> Self {
        Self::InsufficientCrustFormationArea {
            requested_area_weight: error.requested_area_weight,
            available_area_weight: error.available_area_weight,
        }
    }
}
