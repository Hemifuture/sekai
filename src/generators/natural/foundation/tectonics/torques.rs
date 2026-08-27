//! Boundary-torque balance for rigid spherical plates.
//!
//! Each plate solves \(C\boldsymbol{\omega}=\boldsymbol{\tau}\) for its three-component
//! angular velocity. Linear drag (oceanic, stronger continental, and
//! collision dashpots) lives in \(C\); slab pull and ridge push live in
//! \(\boldsymbol{\tau}\). On the sphere \(\mathbf{v}=\boldsymbol{\omega}\times\mathbf{r}\),
//! so drag depends on the unknown \(\boldsymbol{\omega}\) and must sit on the left.
//! Neighbor velocity in the collision term is lagged from the previous
//! rotation, which is the operator-split already used by the 2 Myr stepper.

use thiserror::Error;

use super::contacts::{ContactEvent, ContactKind};
use super::kinematics::{rigid_velocity, KinematicsError};
use super::model::{LineageId, TectonicState};
use crate::world::natural::{
    SphericalPlateRotation, SphericalTectonicValidationError, PLATE_COLLISION_RESISTANCE_PER_M,
    PLATE_CONTINENT_BASAL_DRAG_PER_M2, PLATE_LOCKED_MARGIN_RESISTANCE_PER_M,
    PLATE_OCEAN_BASAL_DRAG_PER_M2, PLATE_RIDGE_PUSH_FORCE_PER_M, PLATE_SLAB_PULL_FORCE_PER_M,
    PLATE_SLAB_SUCTION_FORCE_PER_M,
};
use crate::world::spatial::{cross, dot, scale, SphericalSurfaceSnapshot, UnitVector3};
use crate::world::{EdgeId, Meters};

const MM_PER_M: f64 = 1_000.0;

#[derive(Clone, Copy)]
struct PlateForceSystem {
    drag: [[f64; 3]; 3],
    active_torque: [f64; 3],
}

impl PlateForceSystem {
    fn new() -> Self {
        Self {
            drag: [[0.0; 3]; 3],
            active_torque: [0.0; 3],
        }
    }
}

/// Overwrites each live plate rotation with the torque-balance \(\boldsymbol{\omega}\).
///
/// Opening `activity_band` rates are consumed only as the previous-step
/// fallback when a plate has no usable drag matrix. Force ranking follows
/// Forsyth & Uyeda (1975) and Conrad & Lithgow-Bertelloni (2002); coefficients
/// in `src/world/` remain ranking placeholders until G1d task 4.
pub(super) fn update_rotations_from_boundary_torques(
    surface: &SphericalSurfaceSnapshot,
    state: &mut TectonicState,
    events: &[ContactEvent],
) -> Result<(), TorqueError> {
    let radius = surface.radius();
    let mut systems = vec![PlateForceSystem::new(); state.plates.len()];
    accumulate_basal_drag(state, radius, &mut systems);
    accumulate_boundary_forces(surface, state, events, radius, &mut systems)?;
    for (plate, system) in state.plates.iter_mut().zip(systems) {
        plate.rotation = match solve_linear3(system.drag, system.active_torque) {
            Some(omega) => {
                SphericalPlateRotation::from_angular_velocity_rad_per_year(omega, radius)
                    .unwrap_or(plate.rotation)
            }
            None => plate.rotation,
        };
        plate.rotation.validate_for_radius(radius)?;
    }
    Ok(())
}

fn accumulate_basal_drag(state: &TectonicState, radius: Meters, systems: &mut [PlateForceSystem]) {
    let radius_sq = radius.get() * radius.get();
    for sample in &state.samples {
        let Some(index) = plate_index(state, sample.owner) else {
            continue;
        };
        let n = sample.position.components();
        add_projected_drag(
            &mut systems[index].drag,
            PLATE_CONTINENT_BASAL_DRAG_PER_M2,
            radius_sq,
            sample.material.continental_reference_area_m2(),
            n,
        );
        add_projected_drag(
            &mut systems[index].drag,
            PLATE_OCEAN_BASAL_DRAG_PER_M2,
            radius_sq,
            sample.material.oceanic_reference_area_m2(),
            n,
        );
    }
}

fn accumulate_boundary_forces(
    surface: &SphericalSurfaceSnapshot,
    state: &TectonicState,
    events: &[ContactEvent],
    radius: Meters,
    systems: &mut [PlateForceSystem],
) -> Result<(), TorqueError> {
    for event in events {
        let Some(edge_id) = event.edge else {
            continue;
        };
        let edge = surface
            .edge(edge_id)
            .ok_or(TorqueError::UnknownEdge { edge: edge_id })?;
        let first = event.lineages[0].ok_or(TorqueError::MissingContactLineage)?;
        let second = event.lineages[1].ok_or(TorqueError::MissingContactLineage)?;
        let first_index =
            plate_index(state, first).ok_or(TorqueError::UnknownLineage { lineage: first })?;
        let second_index =
            plate_index(state, second).ok_or(TorqueError::UnknownLineage { lineage: second })?;
        let outward = edge.normal_from_first.components();
        let inward = scale(outward, -1.0);
        let radial = scale(edge.midpoint.components(), radius.get());
        let length = edge.length.get();
        match event.kind {
            ContactKind::OceanicSubduction { descending } => {
                // Pull acts on the descending plate toward the trench; suction
                // acts on the overriding plate, also toward the trench.
                let (index, direction, overriding, toward_trench) = if descending == first {
                    (first_index, outward, second_index, inward)
                } else if descending == second {
                    (second_index, inward, first_index, outward)
                } else {
                    return Err(TorqueError::UnknownLineage {
                        lineage: descending,
                    });
                };
                add_force(
                    &mut systems[index].active_torque,
                    radial,
                    scale(direction, PLATE_SLAB_PULL_FORCE_PER_M * length),
                );
                add_force(
                    &mut systems[overriding].active_torque,
                    radial,
                    scale(toward_trench, PLATE_SLAB_SUCTION_FORCE_PER_M * length),
                );
            }
            ContactKind::Divergence => {
                let push = PLATE_RIDGE_PUSH_FORCE_PER_M * length;
                add_force(
                    &mut systems[first_index].active_torque,
                    radial,
                    scale(inward, push),
                );
                add_force(
                    &mut systems[second_index].active_torque,
                    radial,
                    scale(outward, push),
                );
            }
            ContactKind::ContinentalCollision | ContactKind::LockedConvergence => {
                let resistance_per_m = if event.kind == ContactKind::ContinentalCollision {
                    PLATE_COLLISION_RESISTANCE_PER_M
                } else {
                    PLATE_LOCKED_MARGIN_RESISTANCE_PER_M
                };
                let lagged = LaggedCollision {
                    other: state.plates[second_index].rotation,
                    radial_m: radial,
                    toward_other: outward,
                    length_m: length,
                    midpoint: edge.midpoint,
                };
                add_collision_resistance(
                    &mut systems[first_index],
                    lagged,
                    resistance_per_m,
                    radius,
                )?;
                let lagged = LaggedCollision {
                    other: state.plates[first_index].rotation,
                    radial_m: radial,
                    toward_other: inward,
                    length_m: length,
                    midpoint: edge.midpoint,
                };
                add_collision_resistance(
                    &mut systems[second_index],
                    lagged,
                    resistance_per_m,
                    radius,
                )?;
            }
            ContactKind::Gap | ContactKind::Transform => {}
        }
    }
    Ok(())
}

struct LaggedCollision {
    other: SphericalPlateRotation,
    radial_m: [f64; 3],
    toward_other: [f64; 3],
    length_m: f64,
    midpoint: UnitVector3,
}

fn add_collision_resistance(
    system: &mut PlateForceSystem,
    lagged: LaggedCollision,
    resistance_per_m: f64,
    radius: Meters,
) -> Result<(), TorqueError> {
    let other_mm = rigid_velocity(lagged.other, radius, lagged.midpoint)?;
    let other_v = scale(other_mm, 1.0 / MM_PER_M);
    let kappa = resistance_per_m * lagged.length_m;
    let u = cross(lagged.radial_m, lagged.toward_other);
    add_outer(&mut system.drag, kappa, u);
    let known = scale(u, kappa * dot(other_v, lagged.toward_other));
    system.active_torque[0] += known[0];
    system.active_torque[1] += known[1];
    system.active_torque[2] += known[2];
    Ok(())
}

fn add_projected_drag(drag: &mut [[f64; 3]; 3], k: f64, radius_sq: f64, area: f64, n: [f64; 3]) {
    if area <= 0.0 || k <= 0.0 {
        return;
    }
    let scale = k * radius_sq * area;
    for (i, drag_row) in drag.iter_mut().enumerate() {
        for (j, slot) in drag_row.iter_mut().enumerate() {
            let identity = if i == j { 1.0 } else { 0.0 };
            *slot += scale * (identity - n[i] * n[j]);
        }
    }
}

fn add_force(torque: &mut [f64; 3], radial_m: [f64; 3], force: [f64; 3]) {
    let added = cross(radial_m, force);
    torque[0] += added[0];
    torque[1] += added[1];
    torque[2] += added[2];
}

fn add_outer(matrix: &mut [[f64; 3]; 3], scale: f64, vector: [f64; 3]) {
    for (i, row) in matrix.iter_mut().enumerate() {
        for (j, slot) in row.iter_mut().enumerate() {
            *slot += scale * vector[i] * vector[j];
        }
    }
}

fn plate_index(state: &TectonicState, lineage: LineageId) -> Option<usize> {
    state
        .plates
        .binary_search_by_key(&lineage, |plate| plate.lineage)
        .ok()
}

/// Gaussian elimination with partial pivoting for the 3×3 plate system.
///
/// A full sparse solver would be the N-plate coupled problem; each plate is
/// independently 3×3 because neighbor velocity is lagged.
fn solve_linear3(matrix: [[f64; 3]; 3], rhs: [f64; 3]) -> Option<[f64; 3]> {
    let scale = matrix
        .iter()
        .flatten()
        .fold(0.0_f64, |acc, value| acc.max(value.abs()));
    if scale == 0.0 {
        return None;
    }
    let mut row = [
        [matrix[0][0], matrix[0][1], matrix[0][2], rhs[0]],
        [matrix[1][0], matrix[1][1], matrix[1][2], rhs[1]],
        [matrix[2][0], matrix[2][1], matrix[2][2], rhs[2]],
    ];
    for col in 0..3 {
        let pivot_row = (col..3)
            .max_by(|&left, &right| row[left][col].abs().total_cmp(&row[right][col].abs()))
            .expect("a 3x3 column always has a pivot candidate");
        if row[pivot_row][col].abs() <= 1.0e-12 * scale {
            return None;
        }
        if pivot_row != col {
            row.swap(col, pivot_row);
        }
        let pivot = row[col][col];
        for other in 0..3 {
            if other == col {
                continue;
            }
            let factor = row[other][col] / pivot;
            let pivot_values = row[col];
            for (dst, src) in row[other].iter_mut().zip(pivot_values.iter()).skip(col) {
                *dst -= factor * *src;
            }
        }
    }
    Some([
        row[0][3] / row[0][0],
        row[1][3] / row[1][1],
        row[2][3] / row[2][2],
    ])
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum TorqueError {
    #[error("invalid spherical rotation: {0}")]
    InvalidRotation(#[from] SphericalTectonicValidationError),
    #[error("contact kinematics failed: {0}")]
    Kinematics(#[from] KinematicsError),
    #[error("torque contact is missing a plate lineage")]
    MissingContactLineage,
    #[error("torque contact references missing lineage {lineage:?}")]
    UnknownLineage { lineage: LineageId },
    #[error("torque contact references missing edge {edge:?}")]
    UnknownEdge { edge: EdgeId },
}

#[cfg(test)]
mod tests {
    use super::{solve_linear3, update_rotations_from_boundary_torques, PlateForceSystem};
    use crate::generators::natural::foundation::tectonics::contacts::{ContactEvent, ContactKind};
    use crate::generators::natural::foundation::tectonics::model::{
        ActivePlate, CrustSample, LineageId, MaterialColumn, TectonicState,
    };
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::{dot, SphericalSurfaceSnapshot, UnitVector3};
    use crate::world::{CellId, Meters, SphericalSpaceSpec};

    fn fixture_surface() -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap()
    }

    fn sample(
        owner: LineageId,
        kind: CrustKind,
        area: f64,
        position: UnitVector3,
        cell: CellId,
    ) -> CrustSample {
        CrustSample {
            position,
            anchor: cell,
            owner,
            kind,
            thickness_km: match kind {
                CrustKind::Continental => 38.0,
                CrustKind::Oceanic => 7.0,
            },
            age_myr: match kind {
                CrustKind::Continental => CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                CrustKind::Oceanic => 80.0,
            },
            tectonic_elevation_m: 0.0,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(
                kind,
                area,
                match kind {
                    CrustKind::Continental => 38.0,
                    CrustKind::Oceanic => 7.0,
                },
            )
            .unwrap(),
        }
    }

    fn dummy_rotation() -> SphericalPlateRotation {
        SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap()
    }

    fn trench_state(
        surface: &SphericalSurfaceSnapshot,
        descending_kind: CrustKind,
    ) -> (TectonicState, [f64; 3]) {
        let edge = &surface.edges()[0];
        let first = LineageId::from_raw(0);
        let second = LineageId::from_raw(1);
        let samples = surface
            .cells()
            .iter()
            .map(|cell| {
                let (owner, kind) = if cell.id == edge.cells[1] {
                    (second, CrustKind::Continental)
                } else {
                    (first, descending_kind)
                };
                sample(owner, kind, cell.area.get(), cell.centroid, cell.id)
            })
            .collect();
        let plates = vec![
            ActivePlate::new(first, edge.cells[0], dummy_rotation()),
            ActivePlate::new(second, edge.cells[1], dummy_rotation()),
        ];
        (
            TectonicState::new(samples, plates, 2).unwrap(),
            edge.normal_from_first.components(),
        )
    }

    fn trench_event(surface: &SphericalSurfaceSnapshot) -> ContactEvent {
        let edge = &surface.edges()[0];
        ContactEvent {
            cell: edge.cells[0],
            edge: Some(edge.id),
            sample_indices: [Some(edge.cells[0].raw()), Some(edge.cells[1].raw())],
            lineages: [Some(LineageId::from_raw(0)), Some(LineageId::from_raw(1))],
            kind: ContactKind::OceanicSubduction {
                descending: LineageId::from_raw(0),
            },
            signed_normal_speed_mm_per_year: -48.0,
            tangent_speed_mm_per_year: 2.0,
            overlap_depth: 0,
        }
    }

    #[test]
    fn three_by_three_solve_recovers_a_known_right_hand_side() {
        let matrix = [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
        let omega = solve_linear3(matrix, [2.0, 6.0, 12.0]).unwrap();
        assert!((omega[0] - 1.0).abs() < 1.0e-12);
        assert!((omega[1] - 2.0).abs() < 1.0e-12);
        assert!((omega[2] - 3.0).abs() < 1.0e-12);
        assert_eq!(PlateForceSystem::new().active_torque, [0.0; 3]);
    }

    #[test]
    fn toy_trench_pulls_the_descending_plate_toward_the_trench() {
        let surface = fixture_surface();
        let (mut state, toward_overriding) = trench_state(&surface, CrustKind::Oceanic);
        let event = trench_event(&surface);
        update_rotations_from_boundary_torques(&surface, &mut state, &[event]).unwrap();
        let descending = state.plate(LineageId::from_raw(0)).unwrap();
        let midpoint = surface.edges()[0].midpoint;
        let velocity = descending
            .rotation
            .velocity_mm_per_year(surface.radius(), midpoint)
            .unwrap();
        assert!(
            dot(velocity, toward_overriding) > 0.0,
            "descending velocity {velocity:?} should point along {toward_overriding:?}"
        );
    }

    #[test]
    fn slab_suction_pulls_the_overriding_plate_toward_the_trench() {
        let surface = fixture_surface();
        let (mut state, toward_overriding) = trench_state(&surface, CrustKind::Oceanic);
        update_rotations_from_boundary_torques(&surface, &mut state, &[trench_event(&surface)])
            .unwrap();
        let overriding = state.plate(LineageId::from_raw(1)).unwrap();
        let velocity = overriding
            .rotation
            .velocity_mm_per_year(surface.radius(), surface.edges()[0].midpoint)
            .unwrap();
        assert!(
            dot(velocity, toward_overriding) < 0.0,
            "overriding velocity {velocity:?} should point back toward the trench"
        );
    }

    #[test]
    fn equal_active_torque_slows_a_continental_plate_relative_to_oceanic() {
        let surface = fixture_surface();
        let event = trench_event(&surface);
        let (mut oceanic, _) = trench_state(&surface, CrustKind::Oceanic);
        let (mut continental, _) = trench_state(&surface, CrustKind::Continental);
        update_rotations_from_boundary_torques(
            &surface,
            &mut oceanic,
            std::slice::from_ref(&event),
        )
        .unwrap();
        update_rotations_from_boundary_torques(
            &surface,
            &mut continental,
            std::slice::from_ref(&event),
        )
        .unwrap();
        let oceanic_rate = oceanic
            .plate(LineageId::from_raw(0))
            .unwrap()
            .rotation
            .angular_rate_rad_per_year();
        let continental_rate = continental
            .plate(LineageId::from_raw(0))
            .unwrap()
            .rotation
            .angular_rate_rad_per_year();
        assert!(
            continental_rate < oceanic_rate,
            "continental {continental_rate} should be slower than oceanic {oceanic_rate}"
        );
    }

    #[test]
    fn locked_convergence_resists_the_plate_pushed_into_it() {
        let surface = fixture_surface();
        let edge = &surface.edges()[0];
        let other_cell = edge.cells[1];
        let locked_edge = surface
            .edges()
            .iter()
            .find(|candidate| candidate.id != edge.id && candidate.cells.contains(&other_cell))
            .unwrap();
        let (first, second) = if locked_edge.cells[0] == other_cell {
            (LineageId::from_raw(1), LineageId::from_raw(0))
        } else {
            (LineageId::from_raw(0), LineageId::from_raw(1))
        };
        let locked = ContactEvent {
            cell: locked_edge.cells[0],
            edge: Some(locked_edge.id),
            sample_indices: [
                Some(locked_edge.cells[0].raw()),
                Some(locked_edge.cells[1].raw()),
            ],
            lineages: [Some(first), Some(second)],
            kind: ContactKind::LockedConvergence,
            signed_normal_speed_mm_per_year: -30.0,
            tangent_speed_mm_per_year: 1.0,
            overlap_depth: 0,
        };
        let normal_speed = |state: &TectonicState| {
            let plate = state.plate(LineageId::from_raw(0)).unwrap();
            let velocity = plate
                .rotation
                .velocity_mm_per_year(surface.radius(), locked_edge.midpoint)
                .unwrap();
            dot(velocity, locked_edge.normal_from_first.components()).abs()
        };
        let (mut free, _) = trench_state(&surface, CrustKind::Oceanic);
        let (mut resisted, _) = trench_state(&surface, CrustKind::Oceanic);
        let stationary =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 1).unwrap();
        for state in [&mut free, &mut resisted] {
            state.plates[1].rotation = stationary;
        }
        update_rotations_from_boundary_torques(&surface, &mut free, &[trench_event(&surface)])
            .unwrap();
        update_rotations_from_boundary_torques(
            &surface,
            &mut resisted,
            &[trench_event(&surface), locked],
        )
        .unwrap();
        let free_speed = normal_speed(&free);
        let resisted_speed = normal_speed(&resisted);
        assert!(
            free_speed > 0.0 && resisted_speed < free_speed,
            "locked boundary must reduce normal speed: free={free_speed} resisted={resisted_speed}"
        );
    }
}
