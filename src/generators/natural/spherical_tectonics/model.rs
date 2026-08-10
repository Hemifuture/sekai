//! Shared transient-model constants for evolved spherical tectonics.
//!
//! Presets select only a coherent-noise spectrum and bounded integer process
//! multipliers. They do not branch the process model or prescribe the final
//! number or shape of continents.

#![cfg_attr(not(test), allow(dead_code))]

use crate::generators::natural::fractal::FractalProfile;
use crate::world::natural::{
    CrustKind, ResolvedWorldFormationPreset, SphericalOrogenyKind, SphericalPlateRotation,
};
use crate::world::spatial::UnitVector3;
use crate::world::CellId;
use thiserror::Error;

use super::contacts::{ContactEvent, CoverageScratch};
use super::processes::ProcessActions;

/// Stable identity of one transient plate lineage during the bounded evolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct LineageId(u32);

impl LineageId {
    pub(super) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub(super) const fn raw(self) -> u32 {
        self.0
    }
}

/// One moving, attributed crust sample in the current or next work buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CrustSample {
    pub(super) position: UnitVector3,
    pub(super) anchor: CellId,
    pub(super) owner: LineageId,
    pub(super) kind: CrustKind,
    pub(super) thickness_km: f32,
    pub(super) age_myr: f32,
    pub(super) tectonic_elevation_m: f32,
    pub(super) lineation: [f32; 2],
    pub(super) orogeny: SphericalOrogenyKind,
    pub(super) orogeny_age_myr: f32,
}

/// Current rigid motion and representative cell of one live lineage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ActivePlate {
    pub(super) lineage: LineageId,
    pub(super) representative: CellId,
    pub(super) rotation: SphericalPlateRotation,
}

impl ActivePlate {
    pub(super) const fn new(
        lineage: LineageId,
        representative: CellId,
        rotation: SphericalPlateRotation,
    ) -> Self {
        Self {
            lineage,
            representative,
            rotation,
        }
    }
}

/// The only logical state at one evolution step.
#[derive(Debug)]
pub(super) struct TectonicState {
    pub(super) samples: Vec<CrustSample>,
    pub(super) plates: Vec<ActivePlate>,
    next_lineage_raw: u32,
}

impl TectonicState {
    pub(super) fn new(
        samples: Vec<CrustSample>,
        mut plates: Vec<ActivePlate>,
        next_lineage_raw: u32,
    ) -> Result<Self, TectonicModelError> {
        plates.sort_by_key(|plate| plate.lineage);
        if samples.is_empty() {
            return Err(TectonicModelError::EmptySamples);
        }
        if plates.is_empty() {
            return Err(TectonicModelError::EmptyPlates);
        }
        for pair in plates.windows(2) {
            if pair[0].lineage == pair[1].lineage {
                return Err(TectonicModelError::DuplicateLineage {
                    lineage: pair[0].lineage,
                });
            }
        }
        let maximum = plates
            .iter()
            .map(|plate| plate.lineage.raw())
            .max()
            .expect("non-empty plate table has a maximum lineage");
        if next_lineage_raw <= maximum {
            return Err(TectonicModelError::NextLineageNotAdvanced {
                next: next_lineage_raw,
                maximum,
            });
        }
        for (index, sample) in samples.iter().enumerate() {
            if plates
                .binary_search_by_key(&sample.owner, |plate| plate.lineage)
                .is_err()
            {
                return Err(TectonicModelError::UnknownSampleOwner {
                    sample: index,
                    owner: sample.owner,
                });
            }
        }
        Ok(Self {
            samples,
            plates,
            next_lineage_raw,
        })
    }

    pub(super) fn initial_owners(&self) -> Vec<LineageId> {
        self.samples.iter().map(|sample| sample.owner).collect()
    }

    pub(super) const fn next_lineage_raw(&self) -> u32 {
        self.next_lineage_raw
    }

    pub(super) fn plate(&self, lineage: LineageId) -> Option<&ActivePlate> {
        self.plates
            .binary_search_by_key(&lineage, |plate| plate.lineage)
            .ok()
            .map(|index| &self.plates[index])
    }

    pub(super) fn copy_current_into_reusable_next(&mut self, current: &Self) {
        self.samples.clear();
        self.samples.extend_from_slice(&current.samples);
        self.copy_plate_table_into_reusable_next(current);
    }

    pub(super) fn copy_plate_table_into_reusable_next(&mut self, current: &Self) {
        self.plates.clear();
        self.plates.extend_from_slice(&current.plates);
        self.next_lineage_raw = current.next_lineage_raw;
    }

    pub(super) fn allocate_lineage(&mut self) -> Option<LineageId> {
        let lineage = LineageId::from_raw(self.next_lineage_raw);
        self.next_lineage_raw = self.next_lineage_raw.checked_add(1)?;
        Some(lineage)
    }
}

/// Capacity-reused double buffer. It deliberately contains no history collection.
#[derive(Debug)]
pub(super) struct TectonicWorkspace {
    pub(super) current: TectonicState,
    pub(super) next: TectonicState,
    pub(super) coverage: CoverageScratch,
    pub(super) events: Vec<ContactEvent>,
    pub(super) actions: ProcessActions,
}

impl TectonicWorkspace {
    pub(super) fn from_initial(current: TectonicState) -> Self {
        let sample_capacity = current.samples.len();
        let plate_capacity = current.plates.len();
        let next_lineage_raw = current.next_lineage_raw;
        Self {
            current,
            next: TectonicState {
                samples: Vec::with_capacity(sample_capacity),
                plates: Vec::with_capacity(plate_capacity),
                next_lineage_raw,
            },
            coverage: CoverageScratch::with_cell_capacity(sample_capacity),
            events: Vec::with_capacity(sample_capacity),
            actions: ProcessActions::with_sample_capacity(sample_capacity),
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
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum TectonicModelError {
    #[error("a tectonic state requires at least one crust sample")]
    EmptySamples,
    #[error("a tectonic state requires at least one active plate")]
    EmptyPlates,
    #[error("transient lineage {lineage:?} appears more than once")]
    DuplicateLineage { lineage: LineageId },
    #[error("sample {sample} references unknown lineage {owner:?}")]
    UnknownSampleOwner { sample: usize, owner: LineageId },
    #[error("next lineage {next} does not advance beyond live maximum {maximum}")]
    NextLineageNotAdvanced { next: u32, maximum: u32 },
}

#[derive(Debug, Clone, Copy)]
pub(in crate::generators::natural) struct FormationTectonicRecipe {
    pub(in crate::generators::natural) initial_crust_profile: FractalProfile,
    pub(in crate::generators::natural) base_scale_rad: f64,
    pub(in crate::generators::natural) rift_rate_permille: u16,
    pub(in crate::generators::natural) subduction_gain_permille: u16,
    pub(in crate::generators::natural) island_arc_gain_permille: u16,
}

impl FormationTectonicRecipe {
    pub(in crate::generators::natural) const fn for_preset(
        preset: ResolvedWorldFormationPreset,
    ) -> Self {
        use ResolvedWorldFormationPreset::{
            Archipelago, Continents, GreatIsland, Supercontinent, VolcanicIslands,
        };

        match preset {
            Continents => Self::new(4, 1.8, 0.75, 1_000, 1_000, 1_000),
            Archipelago => Self::new(5, 3.4, 0.40, 1_250, 950, 1_150),
            Supercontinent => Self::new(3, 1.1, 1.15, 700, 1_050, 850),
            GreatIsland => Self::new(4, 1.5, 0.90, 850, 1_000, 950),
            VolcanicIslands => Self::new(5, 4.2, 0.32, 1_100, 1_200, 1_500),
        }
    }

    const fn new(
        octaves: usize,
        frequency: f64,
        base_scale_rad: f64,
        rift_rate_permille: u16,
        subduction_gain_permille: u16,
        island_arc_gain_permille: u16,
    ) -> Self {
        Self {
            initial_crust_profile: FractalProfile {
                octaves,
                frequency,
                lacunarity: 2.03,
                persistence: 0.5,
            },
            base_scale_rad,
            rift_rate_permille,
            subduction_gain_permille,
            island_arc_gain_permille,
        }
    }
}

#[cfg(test)]
mod state_tests {
    use super::{ActivePlate, CrustSample, LineageId, TectonicState, TectonicWorkspace};
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
