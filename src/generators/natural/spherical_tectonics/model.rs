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

    pub(super) fn empty_with_capacity(
        sample_capacity: usize,
        plate_capacity: usize,
        next_lineage_raw: u32,
    ) -> Self {
        Self {
            samples: Vec::with_capacity(sample_capacity),
            plates: Vec::with_capacity(plate_capacity),
            next_lineage_raw,
        }
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
