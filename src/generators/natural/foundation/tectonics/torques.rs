//! Boundary-torque balance for rigid spherical plates.
//!
//! All live plates solve one coupled linear system \(M\boldsymbol{\omega}=\boldsymbol{\tau}\)
//! for their three-component angular velocities. Linear basal drag (oceanic,
//! stronger continental) sits on each plate's diagonal block; a convergent
//! boundary that cannot consume (continental collision, positively buoyant
//! lithosphere) couples the two plates with a dashpot on their relative
//! normal velocity, which fills the off-diagonal blocks; slab pull, slab
//! suction and ridge push are the right-hand side. On the sphere
//! \(\mathbf{v}=\boldsymbol{\omega}\times\mathbf{r}\), so every velocity term is linear in
//! the unknowns and the balance is quasi-static (Forsyth & Uyeda 1975). Solving
//! the plates together instead of one at a time with a lagged neighbor is what
//! lets a stiff dashpot actually lock a boundary: the lagged Jacobi sweep
//! oscillates between the two plates' velocities when the coupling outweighs
//! drag.

use thiserror::Error;

use super::contacts::{ContactEvent, ContactKind};
use super::kinematics::KinematicsError;
use super::model::{LineageId, TectonicState};
use crate::world::natural::{
    SphericalPlateRotation, SphericalTectonicValidationError, PLATE_COLLISION_RESISTANCE_PER_M,
    PLATE_CONTINENT_BASAL_DRAG_PER_M2, PLATE_LOCKED_MARGIN_RESISTANCE_PER_M,
    PLATE_OCEAN_BASAL_DRAG_PER_M2, PLATE_RIDGE_PUSH_FORCE_PER_M, PLATE_SLAB_PULL_FORCE_PER_M,
    PLATE_SLAB_SUCTION_FORCE_PER_M,
};
use crate::world::spatial::{cross, scale, SphericalSurfaceSnapshot};
use crate::world::{EdgeId, Meters};

/// Assembled quasi-static balance: `matrix` is the dense `3n x 3n` system in
/// row-major order and `torque` its right-hand side.
struct PlateForceSystem {
    plates: usize,
    matrix: Vec<f64>,
    torque: Vec<f64>,
}

impl PlateForceSystem {
    fn new(plates: usize) -> Self {
        Self {
            plates,
            matrix: vec![0.0; 9 * plates * plates],
            torque: vec![0.0; 3 * plates],
        }
    }

    fn add_block(
        &mut self,
        row_plate: usize,
        column_plate: usize,
        scale: f64,
        block: [[f64; 3]; 3],
    ) {
        let n = 3 * self.plates;
        for (i, row) in block.iter().enumerate() {
            for (j, value) in row.iter().enumerate() {
                self.matrix[(3 * row_plate + i) * n + 3 * column_plate + j] += scale * value;
            }
        }
    }

    fn add_torque(&mut self, plate: usize, radial_m: [f64; 3], force: [f64; 3]) {
        let added = cross(radial_m, force);
        for (axis, value) in added.into_iter().enumerate() {
            self.torque[3 * plate + axis] += value;
        }
    }
}

/// Overwrites each live plate rotation with the coupled torque-balance
/// \(\boldsymbol{\omega}\).
///
/// Opening `activity_band` rates survive only when the system is singular,
/// which needs a plate with no drag at all. Force ranking follows Forsyth &
/// Uyeda (1975) and Conrad & Lithgow-Bertelloni (2002, 2004); coefficients in
/// `src/world/` are pinned by the G1e probes.
pub(super) fn update_rotations_from_boundary_torques(
    surface: &SphericalSurfaceSnapshot,
    state: &mut TectonicState,
    events: &[ContactEvent],
) -> Result<(), TorqueError> {
    let radius = surface.radius();
    let mut system = PlateForceSystem::new(state.plates.len());
    accumulate_basal_drag(state, radius, &mut system);
    regularize_spin_about_sample_normals(&mut system);
    accumulate_boundary_forces(surface, state, events, radius, &mut system)?;
    let solved = solve_dense(
        &mut system.matrix,
        &mut system.torque,
        3 * state.plates.len(),
    );
    for (index, plate) in state.plates.iter_mut().enumerate() {
        if solved {
            let omega = [
                system.torque[3 * index],
                system.torque[3 * index + 1],
                system.torque[3 * index + 2],
            ];
            plate.rotation =
                SphericalPlateRotation::from_angular_velocity_rad_per_year(omega, radius)
                    .unwrap_or(plate.rotation);
        }
        plate.rotation.validate_for_radius(radius)?;
    }
    Ok(())
}

fn accumulate_basal_drag(state: &TectonicState, radius: Meters, system: &mut PlateForceSystem) {
    let radius_sq = radius.get() * radius.get();
    for sample in &state.samples {
        let Some(index) = plate_index(state, sample.owner) else {
            continue;
        };
        let n = sample.position.components();
        for (coefficient, area) in [
            (
                PLATE_CONTINENT_BASAL_DRAG_PER_M2,
                sample.material.continental_reference_area_m2(),
            ),
            (
                PLATE_OCEAN_BASAL_DRAG_PER_M2,
                sample.material.oceanic_reference_area_m2(),
            ),
        ] {
            if area <= 0.0 || coefficient <= 0.0 {
                continue;
            }
            let mut block = [[0.0; 3]; 3];
            for (i, row) in block.iter_mut().enumerate() {
                for (j, slot) in row.iter_mut().enumerate() {
                    let identity = if i == j { 1.0 } else { 0.0 };
                    *slot = identity - n[i] * n[j];
                }
            }
            system.add_block(index, index, coefficient * radius_sq * area, block);
        }
    }
}

/// Projected basal drag \(I-\mathbf{n}\mathbf{n}^{\mathsf T}\) has no component
/// along a sample's own normal, so a plate covering a single cell could spin
/// freely about it and make the system singular. A relative
/// \(10^{-9}\) isotropic drag removes that null space without measurable
/// effect on any plate that covers more than one cell.
const SPIN_REGULARIZATION_RELATIVE: f64 = 1.0e-9;

fn regularize_spin_about_sample_normals(system: &mut PlateForceSystem) {
    let n = 3 * system.plates;
    if n == 0 {
        return;
    }
    let mean_diagonal = (0..n).map(|i| system.matrix[i * n + i]).sum::<f64>() / n as f64;
    let epsilon = SPIN_REGULARIZATION_RELATIVE * mean_diagonal;
    for i in 0..n {
        system.matrix[i * n + i] += epsilon;
    }
}

fn accumulate_boundary_forces(
    surface: &SphericalSurfaceSnapshot,
    state: &TectonicState,
    events: &[ContactEvent],
    radius: Meters,
    system: &mut PlateForceSystem,
) -> Result<(), TorqueError> {
    for event in events {
        if event.kind == ContactKind::Gap {
            continue;
        }
        let first = event.lineages[0].ok_or(TorqueError::MissingContactLineage)?;
        let second = event.lineages[1].ok_or(TorqueError::MissingContactLineage)?;
        let first_index =
            plate_index(state, first).ok_or(TorqueError::UnknownLineage { lineage: first })?;
        let second_index =
            plate_index(state, second).ok_or(TorqueError::UnknownLineage { lineage: second })?;
        let Some(edge_id) = event.edge else {
            // Overlap contacts have no edge: two plates already share a cell
            // and interpenetrate. Resistance there opposes their whole
            // relative velocity over one cell width; the active forces stay
            // on edges, where the boundary geometry is defined.
            if matches!(
                event.kind,
                ContactKind::ContinentalCollision | ContactKind::LockedConvergence
            ) {
                let cell = surface.cell(event.cell).ok_or(TorqueError::UnknownEdge {
                    edge: EdgeId::from_raw(0),
                })?;
                let resistance_per_m = if event.kind == ContactKind::ContinentalCollision {
                    PLATE_COLLISION_RESISTANCE_PER_M
                } else {
                    PLATE_LOCKED_MARGIN_RESISTANCE_PER_M
                };
                add_isotropic_dashpot(
                    system,
                    first_index,
                    second_index,
                    resistance_per_m * cell.area.get().sqrt(),
                    scale(cell.centroid.components(), radius.get()),
                );
            }
            continue;
        };
        let edge = surface
            .edge(edge_id)
            .ok_or(TorqueError::UnknownEdge { edge: edge_id })?;
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
                system.add_torque(
                    index,
                    radial,
                    scale(direction, PLATE_SLAB_PULL_FORCE_PER_M * length),
                );
                system.add_torque(
                    overriding,
                    radial,
                    scale(toward_trench, PLATE_SLAB_SUCTION_FORCE_PER_M * length),
                );
            }
            ContactKind::Divergence => {
                let push = PLATE_RIDGE_PUSH_FORCE_PER_M * length;
                system.add_torque(first_index, radial, scale(inward, push));
                system.add_torque(second_index, radial, scale(outward, push));
            }
            ContactKind::ContinentalCollision | ContactKind::LockedConvergence => {
                let resistance_per_m = if event.kind == ContactKind::ContinentalCollision {
                    PLATE_COLLISION_RESISTANCE_PER_M
                } else {
                    PLATE_LOCKED_MARGIN_RESISTANCE_PER_M
                };
                add_relative_dashpot(
                    system,
                    first_index,
                    second_index,
                    resistance_per_m * length,
                    cross(radial, outward),
                );
            }
            ContactKind::Gap | ContactKind::Transform => {}
        }
    }
    Ok(())
}

/// Dashpot on the relative normal velocity of two plates across one edge.
///
/// With \(\mathbf{u}=\mathbf{r}\times\mathbf{n}\), the normal velocity of plate
/// \(i\) at the edge is \(\boldsymbol{\omega}_i\cdot\mathbf{u}\), so the force
/// \(\kappa((\mathbf{v}_j-\mathbf{v}_i)\cdot\mathbf{n})\mathbf{n}\) on \(i\) and its
/// opposite on \(j\) become the symmetric blocks
/// \(\pm\kappa\,\mathbf{u}\mathbf{u}^{\mathsf T}\) of the coupled system.
fn add_relative_dashpot(
    system: &mut PlateForceSystem,
    first: usize,
    second: usize,
    kappa: f64,
    u: [f64; 3],
) {
    let mut block = [[0.0; 3]; 3];
    for (i, row) in block.iter_mut().enumerate() {
        for (j, slot) in row.iter_mut().enumerate() {
            *slot = u[i] * u[j];
        }
    }
    system.add_block(first, first, kappa, block);
    system.add_block(second, second, kappa, block);
    system.add_block(first, second, -kappa, block);
    system.add_block(second, first, -kappa, block);
}

/// Dashpot on the whole relative velocity of two plates at one point `r`:
/// the force `-kappa (omega_i - omega_j) x r` gives the torque block
/// `kappa (|r|^2 I - r r^T)`, the same projected form as basal drag.
fn add_isotropic_dashpot(
    system: &mut PlateForceSystem,
    first: usize,
    second: usize,
    kappa: f64,
    radial_m: [f64; 3],
) {
    let radius_sq =
        radial_m[0] * radial_m[0] + radial_m[1] * radial_m[1] + radial_m[2] * radial_m[2];
    let mut block = [[0.0; 3]; 3];
    for (i, row) in block.iter_mut().enumerate() {
        for (j, slot) in row.iter_mut().enumerate() {
            let identity = if i == j { radius_sq } else { 0.0 };
            *slot = identity - radial_m[i] * radial_m[j];
        }
    }
    system.add_block(first, first, kappa, block);
    system.add_block(second, second, kappa, block);
    system.add_block(first, second, -kappa, block);
    system.add_block(second, first, -kappa, block);
}

fn plate_index(state: &TectonicState, lineage: LineageId) -> Option<usize> {
    state
        .plates
        .binary_search_by_key(&lineage, |plate| plate.lineage)
        .ok()
}

/// Gaussian elimination with partial pivoting on a dense row-major `n x n`
/// system; the solution overwrites `rhs`. Returns `false` on a singular pivot.
/// At most 64 plates the system is 192 unknowns, far below the cost of any
/// other step of the loop.
fn solve_dense(matrix: &mut [f64], rhs: &mut [f64], n: usize) -> bool {
    debug_assert_eq!(matrix.len(), n * n);
    debug_assert_eq!(rhs.len(), n);
    for column in 0..n {
        let pivot_row = (column..n)
            .max_by(|&a, &b| {
                matrix[a * n + column]
                    .abs()
                    .total_cmp(&matrix[b * n + column].abs())
            })
            .expect("non-empty column");
        let pivot = matrix[pivot_row * n + column];
        if !pivot.is_finite() || pivot.abs() <= f64::MIN_POSITIVE {
            return false;
        }
        if pivot_row != column {
            for k in 0..n {
                matrix.swap(pivot_row * n + k, column * n + k);
            }
            rhs.swap(pivot_row, column);
        }
        for row in column + 1..n {
            let factor = matrix[row * n + column] / pivot;
            if factor == 0.0 {
                continue;
            }
            for k in column..n {
                matrix[row * n + k] -= factor * matrix[column * n + k];
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    for column in (0..n).rev() {
        let mut value = rhs[column];
        for k in column + 1..n {
            value -= matrix[column * n + k] * rhs[k];
        }
        rhs[column] = value / matrix[column * n + column];
    }
    rhs.iter().all(|value| value.is_finite())
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
    use super::{solve_dense, update_rotations_from_boundary_torques};
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
    fn dense_solve_recovers_a_known_right_hand_side() {
        let mut matrix = vec![2.0, 1.0, 0.0, 1.0, 3.0, 0.0, 0.0, 0.0, 4.0];
        let mut rhs = vec![4.0, 7.0, 12.0];
        assert!(solve_dense(&mut matrix, &mut rhs, 3));
        assert!((rhs[0] - 1.0).abs() < 1.0e-12);
        assert!((rhs[1] - 2.0).abs() < 1.0e-12);
        assert!((rhs[2] - 3.0).abs() < 1.0e-12);
        let mut singular = vec![1.0, 2.0, 2.0, 4.0];
        let mut rhs = vec![1.0, 2.0];
        assert!(!solve_dense(&mut singular, &mut rhs, 2));
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
        eprintln!("[locked-test] free={free_speed} resisted={resisted_speed}");
        assert!(
            free_speed > 0.0 && resisted_speed < free_speed,
            "locked boundary must reduce normal speed: free={free_speed} resisted={resisted_speed}"
        );
    }
}
