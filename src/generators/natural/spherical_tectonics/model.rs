//! Shared transient-model constants for evolved spherical tectonics.
//!
//! Presets select only a coherent-noise spectrum and bounded integer process
//! multipliers. They do not branch the process model or prescribe the final
//! number or shape of continents.

#![cfg_attr(not(test), allow(dead_code))]

use crate::generators::natural::fractal::FractalProfile;
use crate::world::natural::{
    CrustKind, CrustMaterialTotals, ResolvedWorldFormationPreset, SphericalOrogenyKind,
    SphericalPlateRotation, TectonicMaterialAmount, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, OCEANIC_CRUST_MAX_THICKNESS_KM,
    OCEANIC_CRUST_MIN_THICKNESS_KM,
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
    pub(super) material: MaterialColumn,
}

/// Extensive continental and oceanic material carried by one moving sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MaterialColumn {
    continental_reference_area_m2: f64,
    continental_volume_m3: f64,
    oceanic_reference_area_m2: f64,
    oceanic_volume_m3: f64,
}

impl MaterialColumn {
    pub(super) fn new(
        continental_reference_area_m2: f64,
        continental_volume_m3: f64,
        oceanic_reference_area_m2: f64,
        oceanic_volume_m3: f64,
    ) -> Result<Self, MaterialColumnError> {
        validate_material_component(
            CrustKind::Continental,
            continental_reference_area_m2,
            continental_volume_m3,
        )?;
        validate_material_component(
            CrustKind::Oceanic,
            oceanic_reference_area_m2,
            oceanic_volume_m3,
        )?;
        if continental_reference_area_m2 + oceanic_reference_area_m2 <= 0.0 {
            return Err(MaterialColumnError::Empty);
        }
        Ok(Self {
            continental_reference_area_m2,
            continental_volume_m3,
            oceanic_reference_area_m2,
            oceanic_volume_m3,
        })
    }

    pub(super) fn pure(
        kind: CrustKind,
        reference_area_m2: f64,
        thickness_km: f32,
    ) -> Result<Self, MaterialColumnError> {
        if !reference_area_m2.is_finite() || reference_area_m2 <= 0.0 {
            return Err(MaterialColumnError::InvalidArea {
                kind,
                found: reference_area_m2,
            });
        }
        if !thickness_km.is_finite() {
            return Err(MaterialColumnError::InvalidThickness {
                kind,
                found: thickness_km,
            });
        }
        let volume_m3 = reference_area_m2 * f64::from(thickness_km) * 1_000.0;
        match kind {
            CrustKind::Continental => Self::new(reference_area_m2, volume_m3, 0.0, 0.0),
            CrustKind::Oceanic => Self::new(0.0, 0.0, reference_area_m2, volume_m3),
        }
    }

    pub(super) const fn continental_reference_area_m2(self) -> f64 {
        self.continental_reference_area_m2
    }

    pub(super) const fn continental_volume_m3(self) -> f64 {
        self.continental_volume_m3
    }

    pub(super) const fn oceanic_reference_area_m2(self) -> f64 {
        self.oceanic_reference_area_m2
    }

    pub(super) const fn oceanic_volume_m3(self) -> f64 {
        self.oceanic_volume_m3
    }

    pub(super) fn compatibility_kind(self) -> CrustKind {
        if self.continental_reference_area_m2 >= self.oceanic_reference_area_m2 {
            CrustKind::Continental
        } else {
            CrustKind::Oceanic
        }
    }

    pub(super) fn compatibility_thickness_km(self) -> f32 {
        let (area, volume) = match self.compatibility_kind() {
            CrustKind::Continental => (
                self.continental_reference_area_m2,
                self.continental_volume_m3,
            ),
            CrustKind::Oceanic => (self.oceanic_reference_area_m2, self.oceanic_volume_m3),
        };
        debug_assert!(area > 0.0);
        (volume / area / 1_000.0) as f32
    }

    #[cfg(test)]
    pub(super) fn bits(self) -> [u64; 4] {
        [
            self.continental_reference_area_m2.to_bits(),
            self.continental_volume_m3.to_bits(),
            self.oceanic_reference_area_m2.to_bits(),
            self.oceanic_volume_m3.to_bits(),
        ]
    }
}

fn validate_material_component(
    kind: CrustKind,
    reference_area_m2: f64,
    volume_m3: f64,
) -> Result<(), MaterialColumnError> {
    if !reference_area_m2.is_finite() || reference_area_m2 < 0.0 {
        return Err(MaterialColumnError::InvalidArea {
            kind,
            found: reference_area_m2,
        });
    }
    if !volume_m3.is_finite() || volume_m3 < 0.0 {
        return Err(MaterialColumnError::InvalidVolume {
            kind,
            found: volume_m3,
        });
    }
    if reference_area_m2 == 0.0 || volume_m3 == 0.0 {
        if reference_area_m2 == 0.0 && volume_m3 == 0.0 {
            return Ok(());
        }
        return Err(MaterialColumnError::AreaVolumeMismatch { kind });
    }
    let thickness_km = (volume_m3 / reference_area_m2 / 1_000.0) as f32;
    let (min, max) = match kind {
        CrustKind::Continental => (
            CONTINENTAL_CRUST_MIN_THICKNESS_KM,
            CONTINENTAL_CRUST_MAX_THICKNESS_KM,
        ),
        CrustKind::Oceanic => (
            OCEANIC_CRUST_MIN_THICKNESS_KM,
            OCEANIC_CRUST_MAX_THICKNESS_KM,
        ),
    };
    if !thickness_km.is_finite() || !(min..=max).contains(&thickness_km) {
        return Err(MaterialColumnError::InvalidThickness {
            kind,
            found: thickness_km,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum MaterialColumnError {
    #[error("{kind:?} material reference area is invalid: {found}")]
    InvalidArea { kind: CrustKind, found: f64 },
    #[error("{kind:?} material volume is invalid: {found}")]
    InvalidVolume { kind: CrustKind, found: f64 },
    #[error("{kind:?} material area and volume must both be zero or both be positive")]
    AreaVolumeMismatch { kind: CrustKind },
    #[error("{kind:?} material thickness is invalid: {found}")]
    InvalidThickness { kind: CrustKind, found: f32 },
    #[error("a moving crust sample cannot carry zero total reference area")]
    Empty,
    #[error("material totals are invalid: {0}")]
    InvalidTotals(String),
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

    pub(super) fn material_totals(&self) -> Result<CrustMaterialTotals, MaterialColumnError> {
        let mut continental_area = CompensatedSum::default();
        let mut continental_volume = CompensatedSum::default();
        let mut oceanic_area = CompensatedSum::default();
        let mut oceanic_volume = CompensatedSum::default();
        for sample in &self.samples {
            continental_area.add(sample.material.continental_reference_area_m2());
            continental_volume.add(sample.material.continental_volume_m3());
            oceanic_area.add(sample.material.oceanic_reference_area_m2());
            oceanic_volume.add(sample.material.oceanic_volume_m3());
        }
        let continental =
            TectonicMaterialAmount::new(continental_area.total(), continental_volume.total())
                .map_err(|error| MaterialColumnError::InvalidTotals(error.to_string()))?;
        let oceanic = TectonicMaterialAmount::new(oceanic_area.total(), oceanic_volume.total())
            .map_err(|error| MaterialColumnError::InvalidTotals(error.to_string()))?;
        Ok(CrustMaterialTotals::new(continental, oceanic))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EvolutionMaterialLedger {
    initial_control: CrustMaterialTotals,
}

impl EvolutionMaterialLedger {
    pub(super) fn capture_initial(state: &TectonicState) -> Result<Self, MaterialColumnError> {
        Ok(Self {
            initial_control: state.material_totals()?,
        })
    }

    pub(super) const fn initial_control(self) -> CrustMaterialTotals {
        self.initial_control
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let next = self.sum + value;
        if self.sum.abs() >= value.abs() {
            self.correction += (self.sum - next) + value;
        } else {
            self.correction += (value - next) + self.sum;
        }
        self.sum = next;
    }

    fn total(self) -> f64 {
        self.sum + self.correction
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
mod tests {
    use super::{
        ActivePlate, CrustSample, EvolutionMaterialLedger, LineageId, MaterialColumn, TectonicState,
    };
    use crate::world::natural::{
        CrustKind, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::UnitVector3;
    use crate::world::CellId;

    #[test]
    fn pure_and_mixed_columns_derive_material_without_independent_thickness() {
        let continental = MaterialColumn::pure(CrustKind::Continental, 2.5, 40.0).unwrap();
        assert_eq!(continental.continental_reference_area_m2(), 2.5);
        assert_eq!(continental.continental_volume_m3(), 100_000.0);
        assert_eq!(continental.oceanic_reference_area_m2(), 0.0);
        assert_eq!(continental.oceanic_volume_m3(), 0.0);
        assert_eq!(continental.compatibility_kind(), CrustKind::Continental);
        assert_eq!(continental.compatibility_thickness_km(), 40.0);

        let oceanic = MaterialColumn::pure(CrustKind::Oceanic, 4.0, 7.0).unwrap();
        assert_eq!(oceanic.oceanic_reference_area_m2(), 4.0);
        assert_eq!(oceanic.oceanic_volume_m3(), 28_000.0);
        assert_eq!(oceanic.compatibility_kind(), CrustKind::Oceanic);
        assert_eq!(oceanic.compatibility_thickness_km(), 7.0);

        let mixed = MaterialColumn::new(3.0, 90_000.0, 3.0, 21_000.0).unwrap();
        assert_eq!(mixed.compatibility_kind(), CrustKind::Continental);
        assert_eq!(mixed.compatibility_thickness_km(), 30.0);
        assert!(MaterialColumn::new(1.0, 0.0, 0.0, 0.0).is_err());
        assert!(MaterialColumn::new(0.0, 1.0, 1.0, 7_000.0).is_err());
    }

    #[test]
    fn state_totals_and_initial_ledger_preserve_every_material_bit_through_copy() {
        let owner = LineageId::from_raw(0);
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let make = |index, kind, material: MaterialColumn| CrustSample {
            position: UnitVector3::new(1.0, 0.0, 0.0).unwrap(),
            anchor: CellId::from_raw(index),
            owner,
            kind,
            thickness_km: material.compatibility_thickness_km(),
            age_myr: if kind == CrustKind::Continental {
                CONTINENTAL_CRUST_AGE_SENTINEL_MYR
            } else {
                20.0
            },
            tectonic_elevation_m: 0.0,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material,
        };
        let state = TectonicState::new(
            vec![
                make(
                    0,
                    CrustKind::Continental,
                    MaterialColumn::pure(CrustKind::Continental, 2.0, 30.0).unwrap(),
                ),
                make(
                    1,
                    CrustKind::Oceanic,
                    MaterialColumn::pure(CrustKind::Oceanic, 3.0, 8.0).unwrap(),
                ),
            ],
            vec![ActivePlate::new(owner, CellId::from_raw(0), rotation)],
            1,
        )
        .unwrap();
        let totals = state.material_totals().unwrap();
        assert_eq!(totals.continental().reference_area_m2(), 2.0);
        assert_eq!(totals.continental().volume_m3(), 60_000.0);
        assert_eq!(totals.oceanic().reference_area_m2(), 3.0);
        assert_eq!(totals.oceanic().volume_m3(), 24_000.0);
        let ledger = EvolutionMaterialLedger::capture_initial(&state).unwrap();
        assert_eq!(ledger.initial_control(), totals);

        let mut copied = TectonicState::empty_with_capacity(2, 1, 1);
        copied.copy_current_into_reusable_next(&state);
        assert_eq!(copied.material_totals().unwrap(), totals);
        for (actual, expected) in copied.samples.iter().zip(&state.samples) {
            assert_eq!(actual.material.bits(), expected.material.bits());
        }
    }
}
