//! Rigid spherical kinematics and local authoritative-cell location.
//!
//! This is the shared Euler-rotation path used by the bounded Cortial-style
//! evolution. It applies `v = radius * (omega x p)` and Rodrigues' formula on
//! unit directions. The engineering adaptation is a deterministic greedy walk
//! over the authoritative spherical Delaunay adjacency; it changes only lookup
//! cost, not the nearest-site definition or the unit-sphere geometry.

use thiserror::Error;

use super::model::{LineageId, TectonicState};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{SphericalPlateRotation, SphericalTectonicValidationError};
use crate::world::spatial::{SphereGeometryError, SphericalSurfaceSnapshot, UnitVector3};
use crate::world::{CellId, Meters};

const YEARS_PER_MILLION_YEARS: f64 = 1_000_000.0;

pub(super) fn rigid_velocity(
    rotation: SphericalPlateRotation,
    radius: Meters,
    radial: UnitVector3,
) -> Result<[f64; 3], KinematicsError> {
    rotation
        .velocity_mm_per_year(radius, radial)
        .map_err(Into::into)
}

pub(super) fn rotate_direction(
    direction: UnitVector3,
    rotation: SphericalPlateRotation,
    delta_myr: f64,
) -> Result<UnitVector3, KinematicsError> {
    if !delta_myr.is_finite() || delta_myr < 0.0 {
        return Err(KinematicsError::InvalidDeltaMyr { found: delta_myr });
    }
    let angle = rotation.angular_rate_rad_per_year() * delta_myr * YEARS_PER_MILLION_YEARS;
    if !angle.is_finite() {
        return Err(KinematicsError::RotationAngleOverflow { delta_myr });
    }
    if angle == 0.0 {
        return Ok(direction);
    }

    let [px, py, pz] = direction.components();
    let [kx, ky, kz] = rotation.pole().components();
    let cosine = angle.cos();
    let sine = angle.sin();
    let one_minus_cosine = 1.0 - cosine;
    let axis_dot_position = kx * px + ky * py + kz * pz;
    let axis_cross_position = [ky * pz - kz * py, kz * px - kx * pz, kx * py - ky * px];
    UnitVector3::new(
        cosine * px + sine * axis_cross_position[0] + one_minus_cosine * axis_dot_position * kx,
        cosine * py + sine * axis_cross_position[1] + one_minus_cosine * axis_dot_position * ky,
        cosine * pz + sine * axis_cross_position[2] + one_minus_cosine * axis_dot_position * kz,
    )
    .map_err(Into::into)
}

pub(super) fn walk_nearest_cell(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    start: CellId,
    direction: UnitVector3,
) -> Result<CellId, KinematicsError> {
    let cell_count = surface.cells().len();
    if topology.cell_count() != cell_count {
        return Err(KinematicsError::CardinalityMismatch {
            surface_cells: cell_count,
            topology_cells: topology.cell_count(),
        });
    }
    let mut current = start;
    if current.raw() as usize >= cell_count {
        return Err(KinematicsError::UnknownCell { cell: current });
    }

    for _ in 0..cell_count {
        let current_cell = surface
            .cell(current)
            .ok_or(KinematicsError::UnknownCell { cell: current })?;
        let mut best = (current, current_cell.site.dot(direction));
        for arc in &topology.arcs()[current.raw() as usize] {
            let neighbor = surface
                .cell(arc.neighbor)
                .ok_or(KinematicsError::UnknownCell { cell: arc.neighbor })?;
            let candidate = (arc.neighbor, neighbor.site.dot(direction));
            if prefer_candidate(candidate, best) {
                best = candidate;
            }
        }
        if best.0 == current {
            return Ok(current);
        }
        current = best.0;
    }

    Err(KinematicsError::WalkStepLimit {
        start,
        limit: cell_count,
    })
}

pub(super) fn advance_samples(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    current: &TectonicState,
    next: &mut TectonicState,
    delta_myr: f64,
) -> Result<(), KinematicsError> {
    if !delta_myr.is_finite() || delta_myr < 0.0 {
        return Err(KinematicsError::InvalidDeltaMyr { found: delta_myr });
    }
    if topology.cell_count() != surface.cells().len() {
        return Err(KinematicsError::CardinalityMismatch {
            surface_cells: surface.cells().len(),
            topology_cells: topology.cell_count(),
        });
    }

    next.copy_current_into_reusable_next(current);
    for (index, sample) in current.samples.iter().enumerate() {
        let plate = current
            .plate(sample.owner)
            .ok_or(KinematicsError::UnknownLineage {
                sample: index,
                lineage: sample.owner,
            })?;
        let _velocity = rigid_velocity(plate.rotation, surface.radius(), sample.position)?;
        let position = rotate_direction(sample.position, plate.rotation, delta_myr)?;
        let anchor = walk_nearest_cell(surface, topology, sample.anchor, position)?;
        next.samples[index].position = position;
        next.samples[index].anchor = anchor;
    }
    Ok(())
}

pub(super) fn prefer_candidate(candidate: (CellId, f64), incumbent: (CellId, f64)) -> bool {
    candidate.1 > incumbent.1 || (candidate.1 == incumbent.1 && candidate.0 < incumbent.0)
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum KinematicsError {
    #[error("invalid spherical rotation: {0}")]
    InvalidRotation(#[from] SphericalTectonicValidationError),
    #[error("rigid rotation produced an invalid unit direction: {0}")]
    InvalidDirection(#[from] SphereGeometryError),
    #[error("tectonic delta must be finite and non-negative, got {found}")]
    InvalidDeltaMyr { found: f64 },
    #[error("rotation angle overflowed for delta {delta_myr} My")]
    RotationAngleOverflow { delta_myr: f64 },
    #[error("surface has {surface_cells} cells but topology has {topology_cells}")]
    CardinalityMismatch {
        surface_cells: usize,
        topology_cells: usize,
    },
    #[error("cell {cell:?} is outside the authoritative surface")]
    UnknownCell { cell: CellId },
    #[error("sample {sample} references missing lineage {lineage:?}")]
    UnknownLineage { sample: usize, lineage: LineageId },
    #[error("nearest-cell walk from {start:?} exceeded its {limit}-step bound")]
    WalkStepLimit { start: CellId, limit: usize },
}

#[cfg(test)]
mod tests {
    use super::{
        advance_samples, prefer_candidate, rigid_velocity, rotate_direction, walk_nearest_cell,
        KinematicsError,
    };
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::foundation::tectonics::initial_state::build_initial_state;
    use crate::generators::natural::foundation::tectonics::model::LineageId;
    use crate::generators::natural::foundation::tectonics::workspace::TectonicWorkspace;
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        ResolvedFormationTimeline, ResolvedWorldFormationPreset, SphericalPlateRotation,
        TectonicSpec, MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR,
    };
    use crate::world::spatial::{SphericalNaturalSurface, SphericalSurfaceSnapshot, UnitVector3};
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    fn fixture(cells: u32) -> (SphericalSurfaceSnapshot, NaturalTopologyIndex) {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: cells,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        (surface, topology)
    }

    fn streams(seed: u64) -> LabeledSubstreams {
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("kinematics-test", 1, "sekai.test"),
        ));
        LabeledSubstreams::capture(&mut rng)
    }

    fn nearest_by_dot(surface: &SphericalSurfaceSnapshot, direction: UnitVector3) -> CellId {
        surface
            .cells()
            .iter()
            .fold((CellId::from_raw(0), f64::NEG_INFINITY), |best, cell| {
                let candidate = (cell.id, cell.site.dot(direction));
                if prefer_candidate(candidate, best) {
                    candidate
                } else {
                    best
                }
            })
            .0
    }

    fn component_distance(first: UnitVector3, second: UnitVector3) -> f64 {
        first
            .components()
            .into_iter()
            .zip(second.components())
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f64>()
            .sqrt()
    }

    #[test]
    fn rigid_velocity_and_rodrigues_rotation_match_analytic_motion() {
        let radius = Meters::new(6_371_000.0).unwrap();
        let pole = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
        let radial = UnitVector3::new(1.0, 0.0, 0.0).unwrap();
        let rotation = SphericalPlateRotation::new(pole, 10_000).unwrap();
        let velocity = rigid_velocity(rotation, radius, radial).unwrap();
        let expected = [0.0, 63.71, 0.0];
        for (actual, expected) in velocity.into_iter().zip(expected) {
            assert!((actual - expected).abs() <= 1.0e-9);
        }

        let moved = rotate_direction(radial, rotation, 2.0).unwrap();
        let angle = 0.02_f64;
        let analytic = UnitVector3::new(angle.cos(), angle.sin(), 0.0).unwrap();
        assert!(component_distance(moved, analytic) <= 1.0e-14);
        assert!((moved.norm() - 1.0).abs() <= 16.0 * f64::EPSILON);

        let timeline = ResolvedFormationTimeline::sekai_reference();
        let mut repeated = radial;
        for _ in 0..timeline.step_count() {
            repeated = rotate_direction(repeated, rotation, timeline.step_duration_myr()).unwrap();
        }
        let once = rotate_direction(radial, rotation, timeline.total_duration_myr()).unwrap();
        assert!(component_distance(repeated, once) <= 2.0e-14);
    }

    #[test]
    fn local_walk_finds_seam_poles_and_uses_lowest_id_for_exact_ties() {
        let (surface, topology) = fixture(162);
        let directions = [
            UnitVector3::new(-1.0, 0.0, 0.0).unwrap(),
            UnitVector3::new(0.0, 0.0, 1.0).unwrap(),
            UnitVector3::new(0.0, 0.0, -1.0).unwrap(),
        ];
        for (index, direction) in directions.into_iter().enumerate() {
            let expected = nearest_by_dot(&surface, direction);
            let start = CellId::from_raw(((index * 53 + 79) % surface.cells().len()) as u32);
            assert_eq!(
                walk_nearest_cell(&surface, &topology, start, direction).unwrap(),
                expected
            );
        }

        assert!(prefer_candidate(
            (CellId::from_raw(3), 0.75),
            (CellId::from_raw(8), 0.75)
        ));
        assert!(!prefer_candidate(
            (CellId::from_raw(8), 0.75),
            (CellId::from_raw(3), 0.75)
        ));

        let edge = &surface.edges()[17];
        let expected = nearest_by_dot(&surface, edge.midpoint);
        for start in edge.cells {
            assert_eq!(
                walk_nearest_cell(&surface, &topology, start, edge.midpoint).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn advance_samples_reuses_next_buffer_and_changes_only_position_and_anchor() {
        let (surface, topology) = fixture(162);
        let state = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
            &streams(42),
        )
        .unwrap();
        let mut workspace = TectonicWorkspace::from_initial(state);
        let capacity = workspace.next.samples.capacity();

        advance_samples(
            &surface,
            &topology,
            &workspace.current,
            &mut workspace.next,
            2.0,
        )
        .unwrap();

        assert_eq!(
            workspace.next.samples.len(),
            workspace.current.samples.len()
        );
        assert_eq!(workspace.next.samples.capacity(), capacity);
        assert_eq!(workspace.next.plates, workspace.current.plates);
        assert_eq!(
            workspace.next.next_lineage_raw(),
            workspace.current.next_lineage_raw()
        );
        for (before, after) in workspace
            .current
            .samples
            .iter()
            .zip(&workspace.next.samples)
        {
            let plate = workspace.current.plate(before.owner).unwrap();
            let expected_position = rotate_direction(before.position, plate.rotation, 2.0).unwrap();
            let expected_anchor =
                walk_nearest_cell(&surface, &topology, before.anchor, expected_position).unwrap();
            assert!(component_distance(after.position, expected_position) <= 1.0e-14);
            assert_eq!(after.anchor, expected_anchor);

            let mut restored = *after;
            restored.position = before.position;
            restored.anchor = before.anchor;
            assert_eq!(&restored, before);
        }
    }

    #[test]
    fn advance_samples_accepts_transient_extra_crust_between_global_resamples() {
        let (surface, topology) = fixture(42);
        let mut state = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
            &streams(91),
        )
        .unwrap();
        let mut spreading_sample = state.samples[0];
        spreading_sample.anchor = state.samples[1].anchor;
        spreading_sample.position = state.samples[1].position;
        state.samples.push(spreading_sample);
        let mut workspace = TectonicWorkspace::from_initial(state);

        advance_samples(
            &surface,
            &topology,
            &workspace.current,
            &mut workspace.next,
            2.0,
        )
        .unwrap();

        assert_eq!(workspace.next.samples.len(), surface.cells().len() + 1);
        assert_eq!(workspace.next.plates, workspace.current.plates);
    }

    #[test]
    fn kinematics_rejects_non_finite_steps_speed_caps_and_invalid_anchors() {
        let axis = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
        let radial = UnitVector3::new(1.0, 0.0, 0.0).unwrap();
        let rotation = SphericalPlateRotation::new(axis, 10_000).unwrap();
        assert!(matches!(
            rotate_direction(radial, rotation, f64::NAN),
            Err(KinematicsError::InvalidDeltaMyr { .. })
        ));
        assert!(matches!(
            rotate_direction(radial, rotation, f64::INFINITY),
            Err(KinematicsError::InvalidDeltaMyr { .. })
        ));
        assert!(matches!(
            rotate_direction(radial, rotation, -2.0),
            Err(KinematicsError::InvalidDeltaMyr { .. })
        ));

        let excessive =
            SphericalPlateRotation::new(axis, MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR)
                .unwrap();
        assert!(matches!(
            rigid_velocity(excessive, Meters::new(6_371_000.0).unwrap(), radial),
            Err(KinematicsError::InvalidRotation(_))
        ));

        let (surface, topology) = fixture(42);
        assert!(matches!(
            walk_nearest_cell(
                &surface,
                &topology,
                CellId::from_raw(surface.cells().len() as u32),
                radial,
            ),
            Err(KinematicsError::UnknownCell { .. })
        ));

        let state = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
            &streams(18),
        )
        .unwrap();
        let mut workspace = TectonicWorkspace::from_initial(state);
        workspace.current.samples[0].owner = LineageId::from_raw(u32::MAX);
        assert!(matches!(
            advance_samples(
                &surface,
                &topology,
                &workspace.current,
                &mut workspace.next,
                2.0,
            ),
            Err(KinematicsError::UnknownLineage { .. })
        ));
    }
}
