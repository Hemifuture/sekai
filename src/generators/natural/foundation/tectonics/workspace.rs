//! Capacity-reused assembly workspace for the bounded tectonic runner.
//!
//! This orchestration layer is the only place that combines the pure transient
//! data model, read-only contact scratch, and process action scratch. Keeping it
//! separate preserves the one-way dependency `processes -> model`; this module
//! alone assembles `{model, contacts, processes}` for the runner while retaining
//! exactly two reusable current/next state buffers and no history collection.

use super::contacts::{ContactEvent, CoverageScratch};
use super::model::TectonicState;
use super::processes::ProcessActions;

#[derive(Debug)]
pub(super) struct TectonicWorkspace {
    pub(super) current: TectonicState,
    pub(super) next: TectonicState,
    pub(super) coverage: CoverageScratch,
    pub(super) events: Vec<ContactEvent>,
    pub(super) actions: ProcessActions,
    steps_since_resample: u16,
}

impl TectonicWorkspace {
    pub(super) fn from_initial(current: TectonicState) -> Self {
        let sample_capacity = current.samples.len();
        let plate_capacity = current.plates.len();
        let next_lineage_raw = current.next_lineage_raw();
        Self {
            current,
            next: TectonicState::empty_with_capacity(
                sample_capacity,
                plate_capacity,
                next_lineage_raw,
            ),
            coverage: CoverageScratch::with_cell_capacity(sample_capacity),
            events: Vec::with_capacity(sample_capacity),
            actions: ProcessActions::with_sample_capacity(sample_capacity),
            steps_since_resample: 0,
        }
    }

    pub(super) fn step_parts(
        &mut self,
    ) -> (
        &TectonicState,
        &mut TectonicState,
        &mut CoverageScratch,
        &mut Vec<ContactEvent>,
        &mut ProcessActions,
    ) {
        (
            &self.current,
            &mut self.next,
            &mut self.coverage,
            &mut self.events,
            &mut self.actions,
        )
    }

    pub(super) fn swap_current_next(&mut self) {
        std::mem::swap(&mut self.current, &mut self.next);
        self.next.samples.clear();
        self.next.plates.clear();
        self.steps_since_resample = self.steps_since_resample.saturating_add(1);
    }

    pub(super) const fn steps_since_resample(&self) -> u16 {
        self.steps_since_resample
    }

    pub(super) const fn requires_resample(&self) -> bool {
        self.steps_since_resample != 0
    }

    pub(super) fn mark_resampled(&mut self) {
        self.steps_since_resample = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::TectonicWorkspace;
    use crate::generators::natural::foundation::tectonics::model::{
        ActivePlate, CrustSample, LineageId, MaterialColumn, TectonicState,
    };
    use crate::world::natural::{
        CrustKind, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::UnitVector3;
    use crate::world::CellId;

    #[test]
    fn workspace_keeps_only_current_and_reusable_next_state() {
        let lineage = LineageId::from_raw(7);
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let sample = CrustSample {
            position: UnitVector3::new(1.0, 0.0, 0.0).unwrap(),
            anchor: CellId::from_raw(0),
            owner: lineage,
            kind: CrustKind::Continental,
            thickness_km: 35.0,
            age_myr: CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
            tectonic_elevation_m: 800.0,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(CrustKind::Continental, 1.0, 35.0).unwrap(),
        };
        let state = TectonicState::new(
            vec![sample],
            vec![ActivePlate::new(lineage, CellId::from_raw(0), rotation)],
            8,
        )
        .unwrap();
        let mut workspace = TectonicWorkspace::from_initial(state);

        assert_eq!(workspace.current.samples.len(), 1);
        assert!(workspace.next.samples.is_empty());
        assert!(workspace.next.samples.capacity() >= 1);
        assert_eq!(workspace.coverage.count(CellId::from_raw(0)), 0);
        assert!(workspace.events.is_empty());
        assert!(workspace.actions.is_clear());
        let (_current, next, coverage, events, actions) = workspace.step_parts();
        assert!(next.samples.is_empty());
        assert_eq!(coverage.count(CellId::from_raw(0)), 0);
        assert!(events.is_empty());
        assert!(actions.is_clear());
        assert_eq!(workspace.current.next_lineage_raw(), 8);
        assert_eq!(workspace.current.initial_owners(), vec![lineage]);
        assert_eq!(
            workspace.current.plate(lineage),
            Some(&workspace.current.plates[0])
        );
    }
}
