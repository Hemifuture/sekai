//! Capacity-reused actions shared by spherical tectonic processes.
//!
//! The process modules implement the phenomenological transfer curves from
//! Cortial et al. (Sections 4.1-4.2 and Appendix A). They accumulate mutations
//! against stable sample indices and perform one compaction only after every
//! process has inspected the immutable contact list. No history is retained.

#![cfg_attr(not(test), allow(dead_code))]

use thiserror::Error;

use super::contacts::ContactEvent;
use super::model::{CrustSample, LineageId, TectonicState};
use crate::world::natural::{ELEVATION_MAX_M, ELEVATION_MIN_M};
use crate::world::spatial::{
    canonical_east_north_basis, central_angle, project_tangent, SphericalSurfaceSnapshot,
    UnitVector3,
};
use crate::world::{CellId, EdgeId};

pub(super) mod constants {
    pub(super) const DEFAULT_DELTA_MYR: f64 = 2.0;
    pub(super) const OCEANIC_RIDGE_ELEVATION_M: f32 = -1_000.0;
    pub(super) const OCEANIC_TRENCH_ELEVATION_M: f32 = -10_000.0;
    pub(super) const HIGHEST_CONTINENTAL_ELEVATION_M: f32 = 10_000.0;
    pub(super) const ABYSSAL_PLAIN_ELEVATION_M: f32 = -6_000.0;
    pub(super) const SUBDUCTION_MAX_DISTANCE_M: f64 = 1_800_000.0;
    pub(super) const SUBDUCTION_PEAK_DISTANCE_M: f64 = 600_000.0;
    pub(super) const COLLISION_MAX_DISTANCE_M: f64 = 4_200_000.0;
    pub(super) const COLLISION_COEFFICIENT_PER_KM: f64 = 1.3e-5;
    pub(super) const REFERENCE_PLATE_SPEED_MM_PER_YEAR: f64 = 100.0;
    pub(super) const BASE_SUBDUCTION_UPLIFT_MM_PER_YEAR: f64 = 0.6;
    pub(super) const OCEANIC_ELEVATION_DAMPING_MM_PER_YEAR: f64 = 0.04;
    pub(super) const CONTINENTAL_EROSION_MM_PER_YEAR: f64 = 0.03;
    pub(super) const TRENCH_SEDIMENT_MM_PER_YEAR: f64 = 0.3;
    pub(super) const FORCED_SUBDUCTION_TERRANE_AREA_FRACTION: f64 = 0.2;
    pub(super) const COLLISION_TRANSFER_OVERLAP_DEPTH: u32 = 1;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SampleDisposition {
    Keep,
    Remove,
    Transfer(LineageId),
}

#[derive(Debug, Default)]
pub(super) struct ProcessActions {
    dispositions: Vec<SampleDisposition>,
    spawned: Vec<CrustSample>,
}

impl ProcessActions {
    pub(super) fn with_sample_capacity(sample_count: usize) -> Self {
        Self {
            dispositions: Vec::with_capacity(sample_count),
            spawned: Vec::with_capacity(sample_count / 16 + 1),
        }
    }

    pub(super) fn begin_step(&mut self, sample_count: usize) {
        self.dispositions.clear();
        self.dispositions
            .resize(sample_count, SampleDisposition::Keep);
        self.spawned.clear();
    }

    pub(super) fn mark_remove(&mut self, sample: usize) -> Result<(), ProcessError> {
        let action_count = self.dispositions.len();
        let disposition =
            self.dispositions
                .get_mut(sample)
                .ok_or(ProcessError::ActionIndexOutOfBounds {
                    sample,
                    actions: action_count,
                })?;
        *disposition = SampleDisposition::Remove;
        Ok(())
    }

    pub(super) fn mark_transfer(
        &mut self,
        sample: usize,
        owner: LineageId,
    ) -> Result<(), ProcessError> {
        let action_count = self.dispositions.len();
        let disposition =
            self.dispositions
                .get_mut(sample)
                .ok_or(ProcessError::ActionIndexOutOfBounds {
                    sample,
                    actions: action_count,
                })?;
        if *disposition == SampleDisposition::Keep {
            *disposition = SampleDisposition::Transfer(owner);
        }
        Ok(())
    }

    pub(super) fn push_spawned(&mut self, sample: CrustSample) {
        self.spawned.push(sample);
    }

    pub(super) fn is_clear(&self) -> bool {
        self.dispositions.is_empty() && self.spawned.is_empty()
    }

    fn validate_for(&self, sample_count: usize) -> Result<(), ProcessError> {
        if self.dispositions.len() != sample_count {
            return Err(ProcessError::ActionCardinalityMismatch {
                samples: sample_count,
                actions: self.dispositions.len(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ProcessStats {
    pub(super) subduction_events: u32,
    pub(super) collision_events: u32,
    pub(super) forced_subductions: u32,
    pub(super) affected_samples: u32,
    pub(super) removed_samples: u32,
    pub(super) transferred_samples: u32,
    pub(super) spawned_samples: u32,
    pub(super) rift_events: u32,
    pub(super) spawned_lineages: u32,
    pub(super) relaxed_samples: u32,
}

pub(super) fn commit_process_actions(
    next: &mut TectonicState,
    actions: &mut ProcessActions,
) -> Result<(), ProcessError> {
    actions.validate_for(next.samples.len())?;
    for (sample, disposition) in next.samples.iter_mut().zip(&actions.dispositions) {
        if let SampleDisposition::Transfer(owner) = disposition {
            sample.owner = *owner;
        }
    }
    let mut sample_index = 0;
    next.samples.retain(|_| {
        let keep = actions.dispositions[sample_index] != SampleDisposition::Remove;
        sample_index += 1;
        keep
    });
    next.samples.append(&mut actions.spawned);
    actions.dispositions.clear();
    Ok(())
}

pub(super) fn event_speed(event: &ContactEvent) -> f64 {
    f64::from(event.signed_normal_speed_mm_per_year)
        .abs()
        .hypot(f64::from(event.tangent_speed_mm_per_year))
}

pub(super) fn event_distance_m(
    surface: &SphericalSurfaceSnapshot,
    event: &ContactEvent,
    position: UnitVector3,
) -> Result<f64, ProcessError> {
    let reference = if let Some(edge) = event.edge {
        surface
            .edge(edge)
            .ok_or(ProcessError::UnknownEdge { edge })?
            .midpoint
    } else {
        surface
            .cell(event.cell)
            .ok_or(ProcessError::UnknownCell { cell: event.cell })?
            .centroid
    };
    Ok(central_angle(position, reference) * surface.radius().get())
}

pub(super) fn event_lineation(
    surface: &SphericalSurfaceSnapshot,
    event: &ContactEvent,
    radial: UnitVector3,
) -> Result<[f32; 2], ProcessError> {
    let edge_id = if let Some(edge) = event.edge {
        edge
    } else {
        *surface
            .cell(event.cell)
            .ok_or(ProcessError::UnknownCell { cell: event.cell })?
            .boundary_edges
            .first()
            .ok_or(ProcessError::CellWithoutBoundary { cell: event.cell })?
    };
    let edge = surface
        .edge(edge_id)
        .ok_or(ProcessError::UnknownEdge { edge: edge_id })?;
    let tangent = cross(
        edge.midpoint.components(),
        edge.normal_from_first.components(),
    );
    let tangent = project_tangent(tangent, radial);
    let tangent_norm = norm(tangent);
    if tangent_norm <= f64::EPSILON {
        return Ok([0.0; 2]);
    }
    let tangent = tangent.map(|component| component / tangent_norm);
    let (east, north) = canonical_east_north_basis(radial);
    let lineation = [dot(tangent, east), dot(tangent, north)];
    let length = lineation[0].hypot(lineation[1]);
    if length <= f64::EPSILON {
        Ok([0.0; 2])
    } else {
        Ok([
            (lineation[0] / length) as f32,
            (lineation[1] / length) as f32,
        ])
    }
}

pub(super) fn bounded_elevation(elevation_m: f32) -> f32 {
    elevation_m.clamp(ELEVATION_MIN_M, ELEVATION_MAX_M)
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first.into_iter().zip(second).map(|(a, b)| a * b).sum()
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum ProcessError {
    #[error("process has {actions} actions for {samples} samples")]
    ActionCardinalityMismatch { samples: usize, actions: usize },
    #[error("process action index {sample} is outside {actions} actions")]
    ActionIndexOutOfBounds { sample: usize, actions: usize },
    #[error("contact sample index {sample} is outside {samples} samples")]
    ContactSampleOutOfBounds { sample: usize, samples: usize },
    #[error("contact event has no complete pair of samples")]
    MissingContactParticipants,
    #[error("contact descending lineage {descending:?} is absent from its participants")]
    MissingDescendingSide { descending: LineageId },
    #[error("process current state has {current} samples but next has {next}")]
    StateCardinalityMismatch { current: usize, next: usize },
    #[error("cell {cell:?} is outside the authoritative surface")]
    UnknownCell { cell: CellId },
    #[error("edge {edge:?} is outside the authoritative surface")]
    UnknownEdge { edge: EdgeId },
    #[error("authoritative cell {cell:?} has no boundary edge")]
    CellWithoutBoundary { cell: CellId },
    #[error("collision participant is not continental")]
    NonContinentalCollision,
    #[error("terrane rooted at sample {sample} has no represented area")]
    EmptyTerrane { sample: usize },
    #[error("process requires a dense current sample for cell {cell:?}")]
    MissingDenseCurrentSample { cell: CellId },
    #[error("process references missing lineage {lineage:?}")]
    UnknownLineage { lineage: LineageId },
    #[error("process sample {sample} anchor {anchor:?} is outside {cells} cells")]
    InvalidAnchor {
        sample: usize,
        anchor: CellId,
        cells: usize,
    },
    #[error("process delta must be finite and non-negative, got {found}")]
    InvalidDeltaMyr { found: f32 },
    #[error("the transient lineage counter is exhausted")]
    LineageExhausted,
    #[error("rift rotation is invalid: {0}")]
    InvalidRotation(#[from] crate::world::natural::SphericalTectonicValidationError),
    #[error("rift direction is invalid: {0}")]
    InvalidDirection(#[from] crate::world::spatial::SphereGeometryError),
}

#[cfg(test)]
mod tests {
    use super::{commit_process_actions, ProcessActions};
    use crate::generators::natural::spherical_tectonics::model::{
        ActivePlate, CrustSample, LineageId, TectonicState,
    };
    use crate::world::natural::{
        CrustKind, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::UnitVector3;
    use crate::world::CellId;

    #[test]
    fn actions_commit_once_with_stable_compaction_transfer_and_spawn_order() {
        let first = LineageId::from_raw(0);
        let second = LineageId::from_raw(1);
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let make = |index, owner| CrustSample {
            position: UnitVector3::new(1.0, index as f64 * 0.01, 0.0).unwrap(),
            anchor: CellId::from_raw(index),
            owner,
            kind: CrustKind::Continental,
            thickness_km: 38.0,
            age_myr: CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
            tectonic_elevation_m: 500.0 + index as f32,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
        };
        let mut state = TectonicState::new(
            vec![make(0, first), make(1, first), make(2, first)],
            vec![
                ActivePlate::new(first, CellId::from_raw(0), rotation),
                ActivePlate::new(second, CellId::from_raw(2), rotation),
            ],
            2,
        )
        .unwrap();
        let spawned = make(3, second);
        let mut actions = ProcessActions::with_sample_capacity(3);
        actions.begin_step(3);
        actions.mark_transfer(0, second).unwrap();
        actions.mark_remove(1).unwrap();
        actions.push_spawned(spawned);

        commit_process_actions(&mut state, &mut actions).unwrap();
        assert_eq!(state.samples.len(), 3);
        assert_eq!(state.samples[0].anchor, CellId::from_raw(0));
        assert_eq!(state.samples[0].owner, second);
        assert_eq!(state.samples[1].anchor, CellId::from_raw(2));
        assert_eq!(state.samples[2], spawned);
        assert!(actions.is_clear());
    }
}

mod collision;
mod relaxation;
mod rifting;
mod spreading;
mod subduction;
