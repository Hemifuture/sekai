use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    CrustKind, CrustKindField, NaturalProfileError, NaturalResolutionPlan,
    SphericalTectonicSnapshot, SphericalTectonicValidationError,
    CONTINENTAL_CRUST_MAX_THICKNESS_KM, CONTINENTAL_CRUST_MIN_THICKNESS_KM, MAX_CRUST_AGE_MYR,
    MAX_PLATE_COUNT, NO_OROGENY_AGE_SENTINEL_MYR, OCEANIC_CRUST_MAX_THICKNESS_KM,
    OCEANIC_CRUST_MIN_THICKNESS_KM,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceRef,
};
use crate::world::{CellId, MAX_SPHERICAL_CELL_COUNT};

/// The only supported conservative evolved-tectonic snapshot schema.
pub const EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1: u16 = 1;
/// Maximum relative material-ledger residual on the tectonic control surface.
///
/// The control thickness field is stored in f32 and re-quantized at every
/// process step, while the expected ledger accumulates in f64; across ~1e5
/// cells and multiple evolution rounds that quantization alone reaches the
/// 1e-5 relative range (observed 9.9e-6 at 22 plates, active tectonics).
/// The bound sits at 1e-4: an order of magnitude of headroom over the f32
/// floor while still catching real accounting gaps, which manifest at
/// percent scale or as outright divergence. The prior 1e-9 bound assumed an
/// all-f64 ledger and rejected healthy worlds.
pub const MAX_TECTONIC_CONTROL_RELATIVE_BUDGET_ERROR: f64 = 1.0e-4;
/// Maximum relative material error after P1 control-to-authority remapping.
/// Same f32 quantization argument as the control bound.
pub const MAX_TECTONIC_AUTHORITY_RELATIVE_BUDGET_ERROR: f64 = 1.0e-4;
/// Largest finite instantaneous cause rate admitted by the public V5 contract.
pub const MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR: f32 = 500.0;

const MAX_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MATERIAL_THICKNESS_TOLERANCE_KM: f64 = 1.0e-6;
const COMPATIBILITY_THICKNESS_TOLERANCE_KM: f64 = 1.0e-3;
const CELL_AREA_RELATIVE_TOLERANCE: f64 = 1.0e-6;

/// One non-negative extensive material component.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TectonicMaterialAmount {
    reference_area_m2: f64,
    volume_m3: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TectonicMaterialAmountWire {
    reference_area_m2: f64,
    volume_m3: f64,
}

impl TectonicMaterialAmount {
    /// Constructs a finite non-negative area/volume pair.
    pub fn new(
        reference_area_m2: f64,
        volume_m3: f64,
    ) -> Result<Self, EvolvedTectonicValidationError> {
        validate_non_negative("reference_area_m2", reference_area_m2)?;
        validate_non_negative("volume_m3", volume_m3)?;
        if reference_area_m2 == 0.0 && volume_m3 != 0.0 {
            return Err(EvolvedTectonicValidationError::VolumeWithoutArea {
                field: "material amount",
                cell: None,
                volume_m3,
            });
        }
        if reference_area_m2 > 0.0 && volume_m3 == 0.0 {
            return Err(EvolvedTectonicValidationError::AreaWithoutVolume {
                field: "material amount",
                cell: None,
                reference_area_m2,
            });
        }
        Ok(Self {
            reference_area_m2,
            volume_m3,
        })
    }

    /// Returns the exact empty material amount.
    pub const fn zero() -> Self {
        Self {
            reference_area_m2: 0.0,
            volume_m3: 0.0,
        }
    }

    /// Returns reference footprint area in square metres.
    pub const fn reference_area_m2(self) -> f64 {
        self.reference_area_m2
    }

    /// Returns crustal material volume in cubic metres.
    pub const fn volume_m3(self) -> f64 {
        self.volume_m3
    }

    /// Returns mean component thickness in kilometres when the component exists.
    pub fn thickness_km(self) -> Option<f64> {
        (self.reference_area_m2 > 0.0).then_some(self.volume_m3 / self.reference_area_m2 / 1_000.0)
    }
}

impl<'de> Deserialize<'de> for TectonicMaterialAmount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = TectonicMaterialAmountWire::deserialize(deserializer)?;
        Self::new(wire.reference_area_m2, wire.volume_m3).map_err(D::Error::custom)
    }
}

/// Compensated totals for both crust-material components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CrustMaterialTotals {
    continental: TectonicMaterialAmount,
    oceanic: TectonicMaterialAmount,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CrustMaterialTotalsWire {
    continental: TectonicMaterialAmount,
    oceanic: TectonicMaterialAmount,
}

impl CrustMaterialTotals {
    /// Combines validated continental and oceanic extensive totals.
    pub const fn new(continental: TectonicMaterialAmount, oceanic: TectonicMaterialAmount) -> Self {
        Self {
            continental,
            oceanic,
        }
    }

    /// Returns all-zero material totals.
    pub const fn zero() -> Self {
        Self::new(
            TectonicMaterialAmount::zero(),
            TectonicMaterialAmount::zero(),
        )
    }

    /// Returns the continental component.
    pub const fn continental(self) -> TectonicMaterialAmount {
        self.continental
    }

    /// Returns the oceanic component.
    pub const fn oceanic(self) -> TectonicMaterialAmount {
        self.oceanic
    }
}

impl<'de> Deserialize<'de> for CrustMaterialTotals {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CrustMaterialTotalsWire::deserialize(deserializer)?;
        Ok(Self::new(wire.continental, wire.oceanic))
    }
}

/// Signed closure residuals for four extensive material quantities.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CrustMaterialResidual {
    continental_area_m2: f64,
    continental_volume_m3: f64,
    oceanic_area_m2: f64,
    oceanic_volume_m3: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CrustMaterialResidualWire {
    continental_area_m2: f64,
    continental_volume_m3: f64,
    oceanic_area_m2: f64,
    oceanic_volume_m3: f64,
}

impl CrustMaterialResidual {
    fn new(
        continental_area_m2: f64,
        continental_volume_m3: f64,
        oceanic_area_m2: f64,
        oceanic_volume_m3: f64,
    ) -> Result<Self, EvolvedTectonicValidationError> {
        for (field, found) in [
            ("continental_area_m2", continental_area_m2),
            ("continental_volume_m3", continental_volume_m3),
            ("oceanic_area_m2", oceanic_area_m2),
            ("oceanic_volume_m3", oceanic_volume_m3),
        ] {
            if !found.is_finite() {
                return Err(EvolvedTectonicValidationError::NonFiniteValue {
                    field,
                    cell: None,
                    found,
                });
            }
        }
        Ok(Self {
            continental_area_m2,
            continental_volume_m3,
            oceanic_area_m2,
            oceanic_volume_m3,
        })
    }

    /// Returns the exact zero residual.
    pub const fn zero() -> Self {
        Self {
            continental_area_m2: 0.0,
            continental_volume_m3: 0.0,
            oceanic_area_m2: 0.0,
            oceanic_volume_m3: 0.0,
        }
    }

    /// Returns continental reference-area residual in square metres.
    pub const fn continental_area_m2(self) -> f64 {
        self.continental_area_m2
    }

    /// Returns continental volume residual in cubic metres.
    pub const fn continental_volume_m3(self) -> f64 {
        self.continental_volume_m3
    }

    /// Returns oceanic reference-area residual in square metres.
    pub const fn oceanic_area_m2(self) -> f64 {
        self.oceanic_area_m2
    }

    /// Returns oceanic volume residual in cubic metres.
    pub const fn oceanic_volume_m3(self) -> f64 {
        self.oceanic_volume_m3
    }

    fn values(self) -> [f64; 4] {
        [
            self.continental_area_m2,
            self.continental_volume_m3,
            self.oceanic_area_m2,
            self.oceanic_volume_m3,
        ]
    }
}

impl<'de> Deserialize<'de> for CrustMaterialResidual {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CrustMaterialResidualWire::deserialize(deserializer)?;
        Self::new(
            wire.continental_area_m2,
            wire.continental_volume_m3,
            wire.oceanic_area_m2,
            wire.oceanic_volume_m3,
        )
        .map_err(D::Error::custom)
    }
}

/// Dense authoritative extensive crust-material fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalCrustMaterialState {
    continental_reference_area_m2: Vec<f64>,
    continental_volume_m3: Vec<f64>,
    oceanic_reference_area_m2: Vec<f64>,
    oceanic_volume_m3: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalCrustMaterialStateWire {
    #[serde(deserialize_with = "deserialize_cell_f64_values")]
    continental_reference_area_m2: Vec<f64>,
    #[serde(deserialize_with = "deserialize_cell_f64_values")]
    continental_volume_m3: Vec<f64>,
    #[serde(deserialize_with = "deserialize_cell_f64_values")]
    oceanic_reference_area_m2: Vec<f64>,
    #[serde(deserialize_with = "deserialize_cell_f64_values")]
    oceanic_volume_m3: Vec<f64>,
}

impl SphericalCrustMaterialState {
    /// Constructs and validates four dense extensive component fields.
    pub fn new(
        continental_reference_area_m2: Vec<f64>,
        continental_volume_m3: Vec<f64>,
        oceanic_reference_area_m2: Vec<f64>,
        oceanic_volume_m3: Vec<f64>,
    ) -> Result<Self, EvolvedTectonicValidationError> {
        let state = Self {
            continental_reference_area_m2,
            continental_volume_m3,
            oceanic_reference_area_m2,
            oceanic_volume_m3,
        };
        state.validate()?;
        Ok(state)
    }

    /// Rechecks cardinality, finiteness, and component thickness bounds.
    pub fn validate(&self) -> Result<(), EvolvedTectonicValidationError> {
        let cell_count = self.continental_reference_area_m2.len();
        validate_cell_count(cell_count)?;
        for (field, found) in [
            ("continental_volume_m3", self.continental_volume_m3.len()),
            (
                "oceanic_reference_area_m2",
                self.oceanic_reference_area_m2.len(),
            ),
            ("oceanic_volume_m3", self.oceanic_volume_m3.len()),
        ] {
            validate_length(field, found, cell_count)?;
        }

        for index in 0..cell_count {
            let cell = CellId::from_raw(index as u32);
            let continental = validate_component(
                "continental",
                cell,
                self.continental_reference_area_m2[index],
                self.continental_volume_m3[index],
                f64::from(CONTINENTAL_CRUST_MIN_THICKNESS_KM),
                f64::from(CONTINENTAL_CRUST_MAX_THICKNESS_KM),
            )?;
            let oceanic = validate_component(
                "oceanic",
                cell,
                self.oceanic_reference_area_m2[index],
                self.oceanic_volume_m3[index],
                f64::from(OCEANIC_CRUST_MIN_THICKNESS_KM),
                f64::from(OCEANIC_CRUST_MAX_THICKNESS_KM),
            )?;
            if continental.reference_area_m2() + oceanic.reference_area_m2() <= 0.0 {
                return Err(EvolvedTectonicValidationError::EmptyMaterialCell { cell });
            }
        }
        let totals = self.totals();
        for (field, found) in [
            (
                "continental_reference_area_total_m2",
                totals.continental().reference_area_m2(),
            ),
            (
                "continental_volume_total_m3",
                totals.continental().volume_m3(),
            ),
            (
                "oceanic_reference_area_total_m2",
                totals.oceanic().reference_area_m2(),
            ),
            ("oceanic_volume_total_m3", totals.oceanic().volume_m3()),
        ] {
            validate_non_negative(field, found)?;
        }
        Ok(())
    }

    /// Returns the number of dense material cells.
    pub fn len(&self) -> usize {
        self.continental_reference_area_m2.len()
    }

    /// Returns whether the material state is empty.
    pub fn is_empty(&self) -> bool {
        self.continental_reference_area_m2.is_empty()
    }

    /// Returns continental reference areas in square metres.
    pub fn continental_reference_area_m2(&self) -> &[f64] {
        &self.continental_reference_area_m2
    }

    /// Returns continental volumes in cubic metres.
    pub fn continental_volume_m3(&self) -> &[f64] {
        &self.continental_volume_m3
    }

    /// Returns oceanic reference areas in square metres.
    pub fn oceanic_reference_area_m2(&self) -> &[f64] {
        &self.oceanic_reference_area_m2
    }

    /// Returns oceanic volumes in cubic metres.
    pub fn oceanic_volume_m3(&self) -> &[f64] {
        &self.oceanic_volume_m3
    }

    /// Derives the dominant compatibility category for one cell.
    pub fn compatibility_kind(&self, index: usize) -> Option<CrustKind> {
        let continental = *self.continental_reference_area_m2.get(index)?;
        let oceanic = *self.oceanic_reference_area_m2.get(index)?;
        Some(if continental >= oceanic {
            CrustKind::Continental
        } else {
            CrustKind::Oceanic
        })
    }

    /// Derives dominant-component mean thickness for one cell.
    pub fn compatibility_thickness_km(&self, index: usize) -> Option<f32> {
        let kind = self.compatibility_kind(index)?;
        let (area, volume) = match kind {
            CrustKind::Continental => (
                *self.continental_reference_area_m2.get(index)?,
                *self.continental_volume_m3.get(index)?,
            ),
            CrustKind::Oceanic => (
                *self.oceanic_reference_area_m2.get(index)?,
                *self.oceanic_volume_m3.get(index)?,
            ),
        };
        (area > 0.0).then_some((volume / area / 1_000.0) as f32)
    }

    /// Computes deterministic compensated global component totals.
    pub fn totals(&self) -> CrustMaterialTotals {
        CrustMaterialTotals::new(
            TectonicMaterialAmount {
                reference_area_m2: compensated_sum(&self.continental_reference_area_m2),
                volume_m3: compensated_sum(&self.continental_volume_m3),
            },
            TectonicMaterialAmount {
                reference_area_m2: compensated_sum(&self.oceanic_reference_area_m2),
                volume_m3: compensated_sum(&self.oceanic_volume_m3),
            },
        )
    }
}

impl<'de> Deserialize<'de> for SphericalCrustMaterialState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalCrustMaterialStateWire::deserialize(deserializer)?;
        Self::new(
            wire.continental_reference_area_m2,
            wire.continental_volume_m3,
            wire.oceanic_reference_area_m2,
            wire.oceanic_volume_m3,
        )
        .map_err(D::Error::custom)
    }
}

/// Dense present-day tectonic cause fields, separate from accumulated relief.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalTectonicForcingState {
    uplift_rate_mm_per_year: Vec<f32>,
    subsidence_rate_mm_per_year: Vec<f32>,
    shortening_rate_mm_per_year: Vec<f32>,
    boundary_distance_m: Vec<f32>,
    event_age_myr: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalTectonicForcingStateWire {
    #[serde(deserialize_with = "deserialize_cell_f32_values")]
    uplift_rate_mm_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_cell_f32_values")]
    subsidence_rate_mm_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_cell_f32_values")]
    shortening_rate_mm_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_cell_f32_values")]
    boundary_distance_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_cell_f32_values")]
    event_age_myr: Vec<f32>,
}

impl SphericalTectonicForcingState {
    /// Constructs and validates all present-day dense forcing fields.
    pub fn new(
        uplift_rate_mm_per_year: Vec<f32>,
        subsidence_rate_mm_per_year: Vec<f32>,
        shortening_rate_mm_per_year: Vec<f32>,
        boundary_distance_m: Vec<f32>,
        event_age_myr: Vec<f32>,
    ) -> Result<Self, EvolvedTectonicValidationError> {
        let state = Self {
            uplift_rate_mm_per_year,
            subsidence_rate_mm_per_year,
            shortening_rate_mm_per_year,
            boundary_distance_m,
            event_age_myr,
        };
        state.validate()?;
        Ok(state)
    }

    /// Rechecks exact lengths, finite ranges, and event-age sentinel semantics.
    pub fn validate(&self) -> Result<(), EvolvedTectonicValidationError> {
        let cell_count = self.uplift_rate_mm_per_year.len();
        validate_cell_count(cell_count)?;
        for (field, found) in [
            (
                "subsidence_rate_mm_per_year",
                self.subsidence_rate_mm_per_year.len(),
            ),
            (
                "shortening_rate_mm_per_year",
                self.shortening_rate_mm_per_year.len(),
            ),
            ("boundary_distance_m", self.boundary_distance_m.len()),
            ("event_age_myr", self.event_age_myr.len()),
        ] {
            validate_length(field, found, cell_count)?;
        }
        for index in 0..cell_count {
            let cell = CellId::from_raw(index as u32);
            for (field, found) in [
                (
                    "uplift_rate_mm_per_year",
                    self.uplift_rate_mm_per_year[index],
                ),
                (
                    "subsidence_rate_mm_per_year",
                    self.subsidence_rate_mm_per_year[index],
                ),
                (
                    "shortening_rate_mm_per_year",
                    self.shortening_rate_mm_per_year[index],
                ),
            ] {
                if !found.is_finite()
                    || !(0.0..=MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR).contains(&found)
                {
                    return Err(EvolvedTectonicValidationError::ForcingRateOutOfRange {
                        field,
                        cell,
                        found,
                        max: MAX_TECTONIC_FORCING_RATE_MM_PER_YEAR,
                    });
                }
            }
            let distance = self.boundary_distance_m[index];
            if !distance.is_finite() || distance < 0.0 {
                return Err(EvolvedTectonicValidationError::BoundaryDistanceOutOfRange {
                    cell,
                    found: distance,
                    max: None,
                });
            }
            let age = self.event_age_myr[index];
            if age != NO_OROGENY_AGE_SENTINEL_MYR
                && (!age.is_finite() || !(0.0..=MAX_CRUST_AGE_MYR).contains(&age))
            {
                return Err(EvolvedTectonicValidationError::EventAgeOutOfRange {
                    cell,
                    found: age,
                });
            }
        }
        Ok(())
    }

    /// Returns the number of forcing cells.
    pub fn len(&self) -> usize {
        self.uplift_rate_mm_per_year.len()
    }

    /// Returns whether the forcing state is empty.
    pub fn is_empty(&self) -> bool {
        self.uplift_rate_mm_per_year.is_empty()
    }

    /// Returns non-negative uplift rates in millimetres per year.
    pub fn uplift_rate_mm_per_year(&self) -> &[f32] {
        &self.uplift_rate_mm_per_year
    }

    /// Returns non-negative subsidence rates in millimetres per year.
    pub fn subsidence_rate_mm_per_year(&self) -> &[f32] {
        &self.subsidence_rate_mm_per_year
    }

    /// Returns non-negative shortening rates in millimetres per year.
    pub fn shortening_rate_mm_per_year(&self) -> &[f32] {
        &self.shortening_rate_mm_per_year
    }

    /// Returns minimum present-day active-boundary distances in metres.
    pub fn boundary_distance_m(&self) -> &[f32] {
        &self.boundary_distance_m
    }

    /// Returns active/inherited event ages in millions of years or `-1`.
    pub fn event_age_myr(&self) -> &[f32] {
        &self.event_age_myr
    }
}

impl<'de> Deserialize<'de> for SphericalTectonicForcingState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalTectonicForcingStateWire::deserialize(deserializer)?;
        Self::new(
            wire.uplift_rate_mm_per_year,
            wire.subsidence_rate_mm_per_year,
            wire.shortening_rate_mm_per_year,
            wire.boundary_distance_m,
            wire.event_age_myr,
        )
        .map_err(D::Error::custom)
    }
}

/// Named non-negative sources and sinks accumulated during V5 evolution.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalTectonicMaterialProcesses {
    rift_extension_continental_area_gain_m2: f64,
    collision_shortening_continental_area_loss_m2: f64,
    continental_consumed: TectonicMaterialAmount,
    oceanic_subducted: TectonicMaterialAmount,
    oceanic_spreading_created: TectonicMaterialAmount,
    oceanic_coverage_created: TectonicMaterialAmount,
    oceanic_coverage_consumed: TectonicMaterialAmount,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalTectonicMaterialProcessesWire {
    rift_extension_continental_area_gain_m2: f64,
    collision_shortening_continental_area_loss_m2: f64,
    continental_consumed: TectonicMaterialAmount,
    oceanic_subducted: TectonicMaterialAmount,
    oceanic_spreading_created: TectonicMaterialAmount,
    oceanic_coverage_created: TectonicMaterialAmount,
    oceanic_coverage_consumed: TectonicMaterialAmount,
}

impl SphericalTectonicMaterialProcesses {
    /// Constructs named non-negative material process totals.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rift_extension_continental_area_gain_m2: f64,
        collision_shortening_continental_area_loss_m2: f64,
        continental_consumed: TectonicMaterialAmount,
        oceanic_subducted: TectonicMaterialAmount,
        oceanic_spreading_created: TectonicMaterialAmount,
        oceanic_coverage_created: TectonicMaterialAmount,
        oceanic_coverage_consumed: TectonicMaterialAmount,
    ) -> Result<Self, EvolvedTectonicValidationError> {
        validate_non_negative(
            "rift_extension_continental_area_gain_m2",
            rift_extension_continental_area_gain_m2,
        )?;
        validate_non_negative(
            "collision_shortening_continental_area_loss_m2",
            collision_shortening_continental_area_loss_m2,
        )?;
        Ok(Self {
            rift_extension_continental_area_gain_m2,
            collision_shortening_continental_area_loss_m2,
            continental_consumed,
            oceanic_subducted,
            oceanic_spreading_created,
            oceanic_coverage_created,
            oceanic_coverage_consumed,
        })
    }

    /// Returns pure-shear continental reference-area gain.
    pub const fn rift_extension_continental_area_gain_m2(self) -> f64 {
        self.rift_extension_continental_area_gain_m2
    }

    /// Returns pure-shear continental reference-area loss at collisions; the
    /// volume stays in the shortened, thickened columns.
    pub const fn collision_shortening_continental_area_loss_m2(self) -> f64 {
        self.collision_shortening_continental_area_loss_m2
    }

    /// Returns explicitly named continental consumption.
    pub const fn continental_consumed(self) -> TectonicMaterialAmount {
        self.continental_consumed
    }

    /// Returns subducted oceanic material.
    pub const fn oceanic_subducted(self) -> TectonicMaterialAmount {
        self.oceanic_subducted
    }

    /// Returns oceanic material created in spreading gaps.
    pub const fn oceanic_spreading_created(self) -> TectonicMaterialAmount {
        self.oceanic_spreading_created
    }

    /// Returns oceanic material added to close uncovered sphere area.
    pub const fn oceanic_coverage_created(self) -> TectonicMaterialAmount {
        self.oceanic_coverage_created
    }

    /// Returns oceanic material consumed to close overlapping sphere area.
    pub const fn oceanic_coverage_consumed(self) -> TectonicMaterialAmount {
        self.oceanic_coverage_consumed
    }
}

impl<'de> Deserialize<'de> for SphericalTectonicMaterialProcesses {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalTectonicMaterialProcessesWire::deserialize(deserializer)?;
        Self::new(
            wire.rift_extension_continental_area_gain_m2,
            wire.collision_shortening_continental_area_loss_m2,
            wire.continental_consumed,
            wire.oceanic_subducted,
            wire.oceanic_spreading_created,
            wire.oceanic_coverage_created,
            wire.oceanic_coverage_consumed,
        )
        .map_err(D::Error::custom)
    }
}

/// Recomputed material closure evidence from initialization through publication.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalTectonicMaterialBudget {
    initial_control: CrustMaterialTotals,
    processes: SphericalTectonicMaterialProcesses,
    final_control: CrustMaterialTotals,
    final_authoritative: CrustMaterialTotals,
    control_residual: CrustMaterialResidual,
    authority_remap_residual: CrustMaterialResidual,
    max_control_relative_error: f64,
    max_authority_relative_error: f64,
    categorical_area_quantization_m2: f64,
    category_ambiguity_area_fraction: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalTectonicMaterialBudgetWire {
    initial_control: CrustMaterialTotals,
    processes: SphericalTectonicMaterialProcesses,
    final_control: CrustMaterialTotals,
    final_authoritative: CrustMaterialTotals,
    control_residual: CrustMaterialResidual,
    authority_remap_residual: CrustMaterialResidual,
    max_control_relative_error: f64,
    max_authority_relative_error: f64,
    categorical_area_quantization_m2: f64,
    category_ambiguity_area_fraction: f64,
}

impl SphericalTectonicMaterialBudget {
    /// Constructs closure diagnostics by recomputing both conservation equations.
    pub fn new(
        initial_control: CrustMaterialTotals,
        processes: SphericalTectonicMaterialProcesses,
        final_control: CrustMaterialTotals,
        final_authoritative: CrustMaterialTotals,
        categorical_area_quantization_m2: f64,
        category_ambiguity_area_fraction: f64,
    ) -> Result<Self, EvolvedTectonicValidationError> {
        validate_non_negative(
            "categorical_area_quantization_m2",
            categorical_area_quantization_m2,
        )?;
        if !category_ambiguity_area_fraction.is_finite()
            || !(0.0..=1.0).contains(&category_ambiguity_area_fraction)
        {
            return Err(EvolvedTectonicValidationError::FractionOutOfRange {
                field: "category_ambiguity_area_fraction",
                found: category_ambiguity_area_fraction,
            });
        }

        let expected_control = expected_control_totals(initial_control, processes)?;
        let control_residual = residual(final_control, expected_control)?;
        let authority_remap_residual = residual(final_authoritative, final_control)?;
        let max_control_relative_error =
            max_relative_residual(control_residual, expected_control, final_control);
        let max_authority_relative_error =
            max_relative_residual(authority_remap_residual, final_control, final_authoritative);
        validate_budget_limit(
            "control",
            max_control_relative_error,
            MAX_TECTONIC_CONTROL_RELATIVE_BUDGET_ERROR,
        )?;
        validate_budget_limit(
            "authority",
            max_authority_relative_error,
            MAX_TECTONIC_AUTHORITY_RELATIVE_BUDGET_ERROR,
        )?;
        Ok(Self {
            initial_control,
            processes,
            final_control,
            final_authoritative,
            control_residual,
            authority_remap_residual,
            max_control_relative_error,
            max_authority_relative_error,
            categorical_area_quantization_m2,
            category_ambiguity_area_fraction,
        })
    }

    /// Returns initialized control-surface totals.
    pub const fn initial_control(self) -> CrustMaterialTotals {
        self.initial_control
    }

    /// Returns all named process sources and sinks.
    pub const fn processes(self) -> SphericalTectonicMaterialProcesses {
        self.processes
    }

    /// Returns final control-surface totals.
    pub const fn final_control(self) -> CrustMaterialTotals {
        self.final_control
    }

    /// Returns final authoritative totals.
    pub const fn final_authoritative(self) -> CrustMaterialTotals {
        self.final_authoritative
    }

    /// Returns signed control-evolution closure residuals.
    pub const fn control_residual(self) -> CrustMaterialResidual {
        self.control_residual
    }

    /// Returns signed P1 authority-remap residuals.
    pub const fn authority_remap_residual(self) -> CrustMaterialResidual {
        self.authority_remap_residual
    }

    /// Returns the largest component-relative control residual.
    pub const fn max_control_relative_error(self) -> f64 {
        self.max_control_relative_error
    }

    /// Returns the largest component-relative authority residual.
    pub const fn max_authority_relative_error(self) -> f64 {
        self.max_authority_relative_error
    }

    /// Returns compatibility-category area quantization in square metres.
    pub const fn categorical_area_quantization_m2(self) -> f64 {
        self.categorical_area_quantization_m2
    }

    /// Returns target area whose remapped category has no strict majority.
    pub const fn category_ambiguity_area_fraction(self) -> f64 {
        self.category_ambiguity_area_fraction
    }
}

impl<'de> Deserialize<'de> for SphericalTectonicMaterialBudget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalTectonicMaterialBudgetWire::deserialize(deserializer)?;
        let budget = Self::new(
            wire.initial_control,
            wire.processes,
            wire.final_control,
            wire.final_authoritative,
            wire.categorical_area_quantization_m2,
            wire.category_ambiguity_area_fraction,
        )
        .map_err(D::Error::custom)?;
        if budget.control_residual != wire.control_residual
            || budget.authority_remap_residual != wire.authority_remap_residual
            || budget.max_control_relative_error.to_bits()
                != wire.max_control_relative_error.to_bits()
            || budget.max_authority_relative_error.to_bits()
                != wire.max_authority_relative_error.to_bits()
        {
            return Err(D::Error::custom(
                EvolvedTectonicValidationError::StaleMaterialBudget,
            ));
        }
        Ok(budget)
    }
}

/// Exact transient-lineage creation and retirement evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalTectonicLineageBudget {
    initial_lineages: u32,
    allocated_lineages: u32,
    retired_lineages: u32,
    final_live_lineages: u32,
    terrane_transfer_count: u32,
    mechanical_fragmentation_count: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalTectonicLineageBudgetWire {
    initial_lineages: u32,
    allocated_lineages: u32,
    retired_lineages: u32,
    final_live_lineages: u32,
    terrane_transfer_count: u32,
    mechanical_fragmentation_count: u32,
}

impl SphericalTectonicLineageBudget {
    /// Constructs an exact never-reused lineage equation.
    pub fn new(
        initial_lineages: u32,
        allocated_lineages: u32,
        retired_lineages: u32,
        final_live_lineages: u32,
        terrane_transfer_count: u32,
        mechanical_fragmentation_count: u32,
    ) -> Result<Self, EvolvedTectonicValidationError> {
        if initial_lineages == 0 {
            return Err(EvolvedTectonicValidationError::EmptyInitialLineages);
        }
        if final_live_lineages == 0 || final_live_lineages > u32::from(MAX_PLATE_COUNT) {
            return Err(
                EvolvedTectonicValidationError::FinalLineageCountOutOfRange {
                    found: final_live_lineages,
                    max: u32::from(MAX_PLATE_COUNT),
                },
            );
        }
        let created = u64::from(initial_lineages) + u64::from(allocated_lineages);
        let accounted = u64::from(retired_lineages) + u64::from(final_live_lineages);
        if created != accounted {
            return Err(EvolvedTectonicValidationError::LineageBudgetMismatch {
                created,
                accounted,
            });
        }
        Ok(Self {
            initial_lineages,
            allocated_lineages,
            retired_lineages,
            final_live_lineages,
            terrane_transfer_count,
            mechanical_fragmentation_count,
        })
    }

    /// Returns initial lineage count.
    pub const fn initial_lineages(self) -> u32 {
        self.initial_lineages
    }

    /// Returns all never-reused lineages allocated after initialization.
    pub const fn allocated_lineages(self) -> u32 {
        self.allocated_lineages
    }

    /// Returns lineages no longer live at publication.
    pub const fn retired_lineages(self) -> u32 {
        self.retired_lineages
    }

    /// Returns final live lineage count.
    pub const fn final_live_lineages(self) -> u32 {
        self.final_live_lineages
    }

    /// Returns committed connected-terrane ownership transfers.
    pub const fn terrane_transfer_count(self) -> u32 {
        self.terrane_transfer_count
    }

    /// Returns deterministic oversized-plate fragmentations.
    pub const fn mechanical_fragmentation_count(self) -> u32 {
        self.mechanical_fragmentation_count
    }
}

impl<'de> Deserialize<'de> for SphericalTectonicLineageBudget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalTectonicLineageBudgetWire::deserialize(deserializer)?;
        Self::new(
            wire.initial_lineages,
            wire.allocated_lineages,
            wire.retired_lineages,
            wire.final_live_lineages,
            wire.terrane_transfer_count,
            wire.mechanical_fragmentation_count,
        )
        .map_err(D::Error::custom)
    }
}

/// Borrowed, cause-only P2 authority consumed by downstream generators.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AuthoritativeTectonicView<'a> {
    snapshot: &'a EvolvedTectonicSnapshot,
}

impl<'a> AuthoritativeTectonicView<'a> {
    pub(crate) const fn material(self) -> &'a SphericalCrustMaterialState {
        &self.snapshot.material
    }

    pub(crate) const fn forcing(self) -> &'a SphericalTectonicForcingState {
        &self.snapshot.forcing
    }

    pub(crate) fn crust_kinds(self) -> &'a CrustKindField {
        self.snapshot.compatibility.crust_kinds()
    }

    pub(crate) fn crust_thickness_km(self) -> &'a [f32] {
        self.snapshot.compatibility.crust_thickness_km()
    }

    pub(crate) fn crust_age_myr(self) -> &'a [f32] {
        self.snapshot.compatibility.crust_age_myr()
    }

    pub(crate) fn plates(self) -> &'a [super::SphericalPlate] {
        self.snapshot.compatibility.plates()
    }

    pub(crate) const fn cell_plates(self) -> &'a super::PlateIdField {
        self.snapshot.compatibility.cell_plates()
    }

    pub(crate) fn boundaries(self) -> &'a [super::BoundaryRecord] {
        self.snapshot.compatibility.boundaries()
    }

    pub(crate) fn lineation_east(self) -> &'a [f32] {
        self.snapshot.compatibility.lineation_east()
    }

    pub(crate) fn lineation_north(self) -> &'a [f32] {
        self.snapshot.compatibility.lineation_north()
    }

    pub(crate) fn orogeny_kind(self) -> &'a [super::SphericalOrogenyKind] {
        self.snapshot.compatibility.orogeny_kind()
    }

    pub(crate) fn orogeny_age_myr(self) -> &'a [f32] {
        self.snapshot.compatibility.orogeny_age_myr()
    }
}

/// Immutable authoritative V5 tectonic causes and conservative diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvolvedTectonicSnapshot {
    schema_version: u16,
    resolution_plan: NaturalResolutionPlan,
    compatibility: SphericalTectonicSnapshot,
    material: SphericalCrustMaterialState,
    forcing: SphericalTectonicForcingState,
    material_budget: SphericalTectonicMaterialBudget,
    lineage_budget: SphericalTectonicLineageBudget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvolvedTectonicSnapshotWire {
    schema_version: u16,
    resolution_plan: NaturalResolutionPlan,
    compatibility: SphericalTectonicSnapshot,
    material: SphericalCrustMaterialState,
    forcing: SphericalTectonicForcingState,
    material_budget: SphericalTectonicMaterialBudget,
    lineage_budget: SphericalTectonicLineageBudget,
}

impl EvolvedTectonicSnapshot {
    pub(crate) const fn authoritative_view(&self) -> AuthoritativeTectonicView<'_> {
        AuthoritativeTectonicView { snapshot: self }
    }

    /// Constructs a complete V5 snapshot only after all local invariants close.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        resolution_plan: NaturalResolutionPlan,
        compatibility: SphericalTectonicSnapshot,
        material: SphericalCrustMaterialState,
        forcing: SphericalTectonicForcingState,
        material_budget: SphericalTectonicMaterialBudget,
        lineage_budget: SphericalTectonicLineageBudget,
    ) -> Result<Self, EvolvedTectonicValidationError> {
        let snapshot = Self {
            schema_version,
            resolution_plan,
            compatibility,
            material,
            forcing,
            material_budget,
            lineage_budget,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks all invariants independent of the referenced surface records.
    pub fn validate(&self) -> Result<(), EvolvedTectonicValidationError> {
        if self.schema_version != EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1 {
            return Err(EvolvedTectonicValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1,
            });
        }
        self.resolution_plan.validate()?;
        self.compatibility.validate()?;
        self.material.validate()?;
        self.forcing.validate()?;
        let cell_count = self.compatibility.surface_ref().cell_count() as usize;
        validate_length("material", self.material.len(), cell_count)?;
        validate_length("forcing", self.forcing.len(), cell_count)?;
        if self.resolution_plan.authoritative_resolved_cell_count()
            != self.compatibility.surface_ref().cell_count()
        {
            return Err(
                EvolvedTectonicValidationError::ResolutionCellCountMismatch {
                    plan: self.resolution_plan.authoritative_resolved_cell_count(),
                    surface: self.compatibility.surface_ref().cell_count(),
                },
            );
        }
        if self.lineage_budget.final_live_lineages() as usize != self.compatibility.plates().len() {
            return Err(
                EvolvedTectonicValidationError::FinalLineageSnapshotMismatch {
                    budget: self.lineage_budget.final_live_lineages(),
                    snapshot: self.compatibility.plates().len(),
                },
            );
        }
        if self.material_budget.final_authoritative() != self.material.totals() {
            return Err(EvolvedTectonicValidationError::MaterialBudgetStateMismatch);
        }
        for index in 0..cell_count {
            let cell = CellId::from_raw(index as u32);
            let material_kind = self
                .material
                .compatibility_kind(index)
                .expect("validated material cardinality matches the snapshot");
            let snapshot_kind = self
                .compatibility
                .crust_kind(cell)
                .expect("validated compatibility fields are dense");
            if material_kind != snapshot_kind {
                return Err(EvolvedTectonicValidationError::CompatibilityKindMismatch {
                    cell,
                    material: material_kind,
                    compatibility: snapshot_kind,
                });
            }
            let material_thickness = self
                .material
                .compatibility_thickness_km(index)
                .expect("a validated material cell has a dominant component");
            let snapshot_thickness = self
                .compatibility
                .crust_thickness_for_cell(cell)
                .expect("validated compatibility thickness is dense");
            if (material_thickness - snapshot_thickness).abs()
                > COMPATIBILITY_THICKNESS_TOLERANCE_KM as f32
            {
                return Err(
                    EvolvedTectonicValidationError::CompatibilityThicknessMismatch {
                        cell,
                        material: material_thickness,
                        compatibility: snapshot_thickness,
                        tolerance: COMPATIBILITY_THICKNESS_TOLERANCE_KM as f32,
                    },
                );
            }
        }
        Ok(())
    }

    /// Cross-validates exact surface identity, radius, per-cell area, and distance bounds.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), EvolvedTectonicValidationError> {
        self.validate()?;
        surface.validate()?;
        self.compatibility.validate_against(surface)?;
        if self.resolution_plan.radius() != surface.radius() {
            return Err(EvolvedTectonicValidationError::ResolutionRadiusMismatch {
                plan: self.resolution_plan.radius().get(),
                surface: surface.radius().get(),
            });
        }
        if self.resolution_plan.authoritative_target_cell_count()
            != self
                .resolution_plan
                .profile()
                .authoritative_target_cell_count()
        {
            return Err(EvolvedTectonicValidationError::ProfileTargetMismatch);
        }
        for (index, cell) in surface.cells().iter().enumerate() {
            let material_area = self.material.continental_reference_area_m2[index]
                + self.material.oceanic_reference_area_m2[index];
            let relative = (material_area - cell.area.get()).abs() / cell.area.get();
            if !relative.is_finite() || relative > CELL_AREA_RELATIVE_TOLERANCE {
                return Err(EvolvedTectonicValidationError::CellMaterialAreaMismatch {
                    cell: cell.id,
                    found: material_area,
                    expected: cell.area.get(),
                    relative,
                    max: CELL_AREA_RELATIVE_TOLERANCE,
                });
            }
            let distance = self.forcing.boundary_distance_m[index];
            let maximum = std::f64::consts::PI * surface.radius().get();
            if f64::from(distance) > maximum {
                return Err(EvolvedTectonicValidationError::BoundaryDistanceOutOfRange {
                    cell: cell.id,
                    found: distance,
                    max: Some(maximum),
                });
            }
        }
        let max_cell_area = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .fold(0.0_f64, f64::max);
        if self.material_budget.categorical_area_quantization_m2() > max_cell_area {
            return Err(
                EvolvedTectonicValidationError::CategoryQuantizationExceeded {
                    found: self.material_budget.categorical_area_quantization_m2(),
                    max: max_cell_area,
                },
            );
        }
        Ok(())
    }

    /// Returns the evolved snapshot schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact P1 resolution choices used by this build.
    pub const fn resolution_plan(&self) -> &NaturalResolutionPlan {
        &self.resolution_plan
    }

    /// Returns the strict V3 compatibility/current-state view.
    pub const fn compatibility(&self) -> &SphericalTectonicSnapshot {
        &self.compatibility
    }

    /// Returns dense conservative material fields.
    pub const fn material(&self) -> &SphericalCrustMaterialState {
        &self.material
    }

    /// Returns dense present-day cause fields.
    pub const fn forcing(&self) -> &SphericalTectonicForcingState {
        &self.forcing
    }

    /// Returns recomputed material closure evidence.
    pub const fn material_budget(&self) -> &SphericalTectonicMaterialBudget {
        &self.material_budget
    }

    /// Returns exact transient-lineage closure evidence.
    pub const fn lineage_budget(&self) -> &SphericalTectonicLineageBudget {
        &self.lineage_budget
    }

    /// Returns the exact authoritative surface identity.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.compatibility.surface_ref()
    }
}

impl<'de> Deserialize<'de> for EvolvedTectonicSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = EvolvedTectonicSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.resolution_plan,
            wire.compatibility,
            wire.material,
            wire.forcing,
            wire.material_budget,
            wire.lineage_budget,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_component(
    field: &'static str,
    cell: CellId,
    area: f64,
    volume: f64,
    min_thickness_km: f64,
    max_thickness_km: f64,
) -> Result<TectonicMaterialAmount, EvolvedTectonicValidationError> {
    validate_non_negative_cell(field, "reference_area_m2", cell, area)?;
    validate_non_negative_cell(field, "volume_m3", cell, volume)?;
    if area == 0.0 {
        if volume != 0.0 {
            return Err(EvolvedTectonicValidationError::VolumeWithoutArea {
                field,
                cell: Some(cell),
                volume_m3: volume,
            });
        }
        return Ok(TectonicMaterialAmount::zero());
    }
    if volume == 0.0 {
        return Err(
            EvolvedTectonicValidationError::MaterialThicknessOutOfRange {
                field,
                cell,
                found: 0.0,
                min: min_thickness_km,
                max: max_thickness_km,
            },
        );
    }
    let thickness = volume / area / 1_000.0;
    if !thickness.is_finite()
        || thickness < min_thickness_km - MATERIAL_THICKNESS_TOLERANCE_KM
        || thickness > max_thickness_km + MATERIAL_THICKNESS_TOLERANCE_KM
    {
        return Err(
            EvolvedTectonicValidationError::MaterialThicknessOutOfRange {
                field,
                cell,
                found: thickness,
                min: min_thickness_km,
                max: max_thickness_km,
            },
        );
    }
    TectonicMaterialAmount::new(area, volume)
}

fn expected_control_totals(
    initial: CrustMaterialTotals,
    processes: SphericalTectonicMaterialProcesses,
) -> Result<CrustMaterialTotals, EvolvedTectonicValidationError> {
    let continental_area = validated_equation_total(
        "continental_reference_area_m2",
        initial.continental().reference_area_m2()
            + processes.rift_extension_continental_area_gain_m2()
            - processes.collision_shortening_continental_area_loss_m2()
            - processes.continental_consumed().reference_area_m2(),
    )?;
    let continental_volume = validated_equation_total(
        "continental_volume_m3",
        initial.continental().volume_m3() - processes.continental_consumed().volume_m3(),
    )?;
    let oceanic_area = validated_equation_total(
        "oceanic_reference_area_m2",
        initial.oceanic().reference_area_m2()
            + processes.oceanic_spreading_created().reference_area_m2()
            + processes.oceanic_coverage_created().reference_area_m2()
            - processes.oceanic_subducted().reference_area_m2()
            - processes.oceanic_coverage_consumed().reference_area_m2(),
    )?;
    let oceanic_volume = validated_equation_total(
        "oceanic_volume_m3",
        initial.oceanic().volume_m3()
            + processes.oceanic_spreading_created().volume_m3()
            + processes.oceanic_coverage_created().volume_m3()
            - processes.oceanic_subducted().volume_m3()
            - processes.oceanic_coverage_consumed().volume_m3(),
    )?;

    Ok(CrustMaterialTotals::new(
        TectonicMaterialAmount::new(continental_area, continental_volume)?,
        TectonicMaterialAmount::new(oceanic_area, oceanic_volume)?,
    ))
}

fn validated_equation_total(
    field: &'static str,
    value: f64,
) -> Result<f64, EvolvedTectonicValidationError> {
    if !value.is_finite() || value < 0.0 {
        return Err(EvolvedTectonicValidationError::InvalidMaterialEquation { field });
    }
    Ok(value)
}

fn residual(
    found: CrustMaterialTotals,
    expected: CrustMaterialTotals,
) -> Result<CrustMaterialResidual, EvolvedTectonicValidationError> {
    CrustMaterialResidual::new(
        found.continental().reference_area_m2() - expected.continental().reference_area_m2(),
        found.continental().volume_m3() - expected.continental().volume_m3(),
        found.oceanic().reference_area_m2() - expected.oceanic().reference_area_m2(),
        found.oceanic().volume_m3() - expected.oceanic().volume_m3(),
    )
}

fn max_relative_residual(
    residual: CrustMaterialResidual,
    expected: CrustMaterialTotals,
    found: CrustMaterialTotals,
) -> f64 {
    let expected = totals_values(expected);
    let found = totals_values(found);
    residual
        .values()
        .into_iter()
        .zip(expected)
        .zip(found)
        .map(|((residual, expected), found)| {
            let scale = expected.abs().max(found.abs());
            if scale == 0.0 {
                residual.abs()
            } else {
                residual.abs() / scale
            }
        })
        .fold(0.0_f64, f64::max)
}

fn totals_values(totals: CrustMaterialTotals) -> [f64; 4] {
    [
        totals.continental().reference_area_m2(),
        totals.continental().volume_m3(),
        totals.oceanic().reference_area_m2(),
        totals.oceanic().volume_m3(),
    ]
}

fn validate_budget_limit(
    domain: &'static str,
    found: f64,
    max: f64,
) -> Result<(), EvolvedTectonicValidationError> {
    if !found.is_finite() || found > max {
        return Err(
            EvolvedTectonicValidationError::MaterialBudgetErrorExceeded { domain, found, max },
        );
    }
    Ok(())
}

fn validate_cell_count(found: usize) -> Result<(), EvolvedTectonicValidationError> {
    if found == 0 || found > MAX_CELLS {
        return Err(EvolvedTectonicValidationError::InvalidCellCount {
            found,
            max: MAX_CELLS,
        });
    }
    Ok(())
}

fn validate_length(
    field: &'static str,
    found: usize,
    expected: usize,
) -> Result<(), EvolvedTectonicValidationError> {
    if found != expected {
        return Err(EvolvedTectonicValidationError::LengthMismatch {
            field,
            found,
            expected,
        });
    }
    Ok(())
}

fn validate_non_negative(
    field: &'static str,
    found: f64,
) -> Result<(), EvolvedTectonicValidationError> {
    if !found.is_finite() {
        return Err(EvolvedTectonicValidationError::NonFiniteValue {
            field,
            cell: None,
            found,
        });
    }
    if found < 0.0 {
        return Err(EvolvedTectonicValidationError::NegativeValue {
            field,
            cell: None,
            found,
        });
    }
    Ok(())
}

fn validate_non_negative_cell(
    domain: &'static str,
    quantity: &'static str,
    cell: CellId,
    found: f64,
) -> Result<(), EvolvedTectonicValidationError> {
    let field = match (domain, quantity) {
        ("continental", "reference_area_m2") => "continental_reference_area_m2",
        ("continental", "volume_m3") => "continental_volume_m3",
        ("oceanic", "reference_area_m2") => "oceanic_reference_area_m2",
        ("oceanic", "volume_m3") => "oceanic_volume_m3",
        _ => "material_component",
    };
    if !found.is_finite() {
        return Err(EvolvedTectonicValidationError::NonFiniteValue {
            field,
            cell: Some(cell),
            found,
        });
    }
    if found < 0.0 {
        return Err(EvolvedTectonicValidationError::NegativeValue {
            field,
            cell: Some(cell),
            found,
        });
    }
    Ok(())
}

fn compensated_sum(values: &[f64]) -> f64 {
    let mut sum = 0.0_f64;
    let mut correction = 0.0_f64;
    for &value in values {
        let next = sum + value;
        if sum.abs() >= value.abs() {
            correction += (sum - next) + value;
        } else {
            correction += (value - next) + sum;
        }
        sum = next;
    }
    sum + correction
}

fn deserialize_cell_f64_values<'de, D>(deserializer: D) -> Result<Vec<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_CELLS>(deserializer)
}

fn deserialize_cell_f32_values<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_CELLS>(deserializer)
}

/// Invalid or scientifically inconsistent evolved tectonic data.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EvolvedTectonicValidationError {
    /// The payload uses an unsupported evolved schema.
    #[error("unsupported evolved tectonic schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The recorded P1 resolution plan is invalid.
    #[error("invalid evolved tectonic resolution plan: {0}")]
    InvalidResolutionPlan(#[from] NaturalProfileError),
    /// The nested V3 compatibility snapshot is invalid.
    #[error("invalid evolved tectonic compatibility snapshot: {0}")]
    InvalidCompatibility(#[from] SphericalTectonicValidationError),
    /// The authoritative surface is invalid.
    #[error("invalid evolved tectonic authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// A dense field count is empty or exceeds the global allocation bound.
    #[error("evolved tectonic cell count {found} is outside 1..={max}")]
    InvalidCellCount { found: usize, max: usize },
    /// A dense field does not match its sibling fields.
    #[error("evolved tectonic field {field} has length {found}; expected {expected}")]
    LengthMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
    /// A material or budget scalar is non-finite.
    #[error("evolved tectonic field {field} at {cell:?} is non-finite: {found}")]
    NonFiniteValue {
        field: &'static str,
        cell: Option<CellId>,
        found: f64,
    },
    /// A material or budget scalar is negative.
    #[error("evolved tectonic field {field} at {cell:?} is negative: {found}")]
    NegativeValue {
        field: &'static str,
        cell: Option<CellId>,
        found: f64,
    },
    /// A non-zero volume has no reference footprint.
    #[error("{field} at {cell:?} stores volume {volume_m3} m3 with zero area")]
    VolumeWithoutArea {
        field: &'static str,
        cell: Option<CellId>,
        volume_m3: f64,
    },
    /// A non-zero reference footprint has no material volume.
    #[error("{field} at {cell:?} stores area {reference_area_m2} m2 with zero volume")]
    AreaWithoutVolume {
        field: &'static str,
        cell: Option<CellId>,
        reference_area_m2: f64,
    },
    /// A material cell contains neither continental nor oceanic footprint.
    #[error("material cell {cell:?} has zero total reference area")]
    EmptyMaterialCell { cell: CellId },
    /// Derived component thickness violates its material envelope.
    #[error("{field} material at {cell:?} has thickness {found} km; expected {min}..={max}")]
    MaterialThicknessOutOfRange {
        field: &'static str,
        cell: CellId,
        found: f64,
        min: f64,
        max: f64,
    },
    /// A present-day cause rate is non-finite, negative, or implausibly large.
    #[error("forcing {field} at {cell:?} is {found}; expected 0..={max} mm/year")]
    ForcingRateOutOfRange {
        field: &'static str,
        cell: CellId,
        found: f32,
        max: f32,
    },
    /// A boundary distance is non-finite, negative, or exceeds the antipode.
    #[error("boundary distance at {cell:?} is {found} m; maximum is {max:?}")]
    BoundaryDistanceOutOfRange {
        cell: CellId,
        found: f32,
        max: Option<f64>,
    },
    /// An event age is neither the sentinel nor a supported age.
    #[error("event age at {cell:?} is {found} Myr")]
    EventAgeOutOfRange { cell: CellId, found: f32 },
    /// A stored fraction lies outside the closed unit interval.
    #[error("evolved tectonic fraction {field} is {found}; expected 0..=1")]
    FractionOutOfRange { field: &'static str, found: f64 },
    /// A process equation would require a negative or non-finite material total.
    #[error("evolved tectonic material equation is invalid for {field}")]
    InvalidMaterialEquation { field: &'static str },
    /// A recomputed material residual exceeds its domain limit.
    #[error("{domain} material relative error {found} exceeds {max}")]
    MaterialBudgetErrorExceeded {
        domain: &'static str,
        found: f64,
        max: f64,
    },
    /// Serialized residual evidence does not match recomputation.
    #[error("serialized evolved tectonic material budget evidence is stale")]
    StaleMaterialBudget,
    /// A lineage ledger cannot start empty.
    #[error("evolved tectonic lineage budget requires an initial lineage")]
    EmptyInitialLineages,
    /// The final live lineage count violates the public plate bound.
    #[error("final live lineage count {found} is outside 1..={max}")]
    FinalLineageCountOutOfRange { found: u32, max: u32 },
    /// Created and retired/live lineage totals do not close.
    #[error("lineage budget created {created} identities but accounts for {accounted}")]
    LineageBudgetMismatch { created: u64, accounted: u64 },
    /// The resolution plan and nested surface allocate different cell counts.
    #[error("resolution plan has {plan} authoritative cells but snapshot has {surface}")]
    ResolutionCellCountMismatch { plan: u32, surface: u32 },
    /// The final lineage ledger and compatibility plate table disagree.
    #[error("lineage budget has {budget} final lineages but snapshot has {snapshot} plates")]
    FinalLineageSnapshotMismatch { budget: u32, snapshot: usize },
    /// The dense material totals and authoritative budget totals disagree.
    #[error("authoritative material state totals do not match the material budget")]
    MaterialBudgetStateMismatch,
    /// The derived compatibility category disagrees with the nested V3 view.
    #[error(
        "cell {cell:?} material category {material:?} disagrees with compatibility {compatibility:?}"
    )]
    CompatibilityKindMismatch {
        cell: CellId,
        material: CrustKind,
        compatibility: CrustKind,
    },
    /// The derived compatibility thickness disagrees with the nested V3 view.
    #[error(
        "cell {cell:?} material thickness {material} differs from compatibility {compatibility} by more than {tolerance} km"
    )]
    CompatibilityThicknessMismatch {
        cell: CellId,
        material: f32,
        compatibility: f32,
        tolerance: f32,
    },
    /// The plan radius and referenced spherical surface radius disagree.
    #[error("resolution plan radius {plan} m differs from surface radius {surface} m")]
    ResolutionRadiusMismatch { plan: f64, surface: f64 },
    /// The profile no longer maps to its stored authoritative target.
    #[error("evolved tectonic quality profile target is inconsistent")]
    ProfileTargetMismatch,
    /// A cell's two component reference areas do not close to its physical area.
    #[error(
        "cell {cell:?} material area {found} differs from physical area {expected}; relative {relative} exceeds {max}"
    )]
    CellMaterialAreaMismatch {
        cell: CellId,
        found: f64,
        expected: f64,
        relative: f64,
        max: f64,
    },
    /// Category quantization is larger than one authoritative cell.
    #[error("category area quantization {found} m2 exceeds largest cell {max} m2")]
    CategoryQuantizationExceeded { found: f64, max: f64 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::natural::{
        BoundaryRecord, CrustKindField, NaturalQualityProfile, PlateIdField, SphericalCrustState,
        SphericalOrogenyKind, SphericalPlate, SphericalPlateRotation, SphericalTectonicSnapshot,
        TECTONIC_SNAPSHOT_SCHEMA_V3,
    };
    use crate::world::spatial::{SurfaceGeometryKind, UnitVector3, SPHERICAL_SURFACE_SCHEMA_V1};
    use crate::world::{Meters, PlateId, SphericalSpaceSpec};

    struct EvolvedFixture {
        evolved: EvolvedTectonicSnapshot,
    }

    fn evolved_fixture() -> EvolvedFixture {
        let authoritative_space = SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: NaturalQualityProfile::Draft.authoritative_target_cell_count(),
        };
        let resolution_plan = NaturalQualityProfile::Draft
            .resolve(&authoritative_space)
            .unwrap();
        let cell_count = resolution_plan.authoritative_resolved_cell_count() as usize;
        let surface_ref = SurfaceRef::new(
            SurfaceGeometryKind::SphericalV1,
            SPHERICAL_SURFACE_SCHEMA_V1,
            cell_count as u32,
            1,
            [1; 32],
        )
        .unwrap();
        let compatibility = SphericalTectonicSnapshot::new(
            TECTONIC_SNAPSHOT_SCHEMA_V3,
            surface_ref,
            vec![SphericalPlate::new(
                PlateId::from_raw(0),
                CellId::from_raw(0),
                SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000)
                    .unwrap(),
            )],
            PlateIdField::from_ids(vec![PlateId::from_raw(0); cell_count]),
            SphericalCrustState::new(
                CrustKindField::from_kinds(vec![CrustKind::Oceanic; cell_count]),
                vec![3.0; cell_count],
                vec![0.0; cell_count],
                vec![0.0; cell_count],
                vec![0.0; cell_count],
                vec![0.0; cell_count],
                vec![SphericalOrogenyKind::None; cell_count],
                vec![NO_OROGENY_AGE_SENTINEL_MYR; cell_count],
            )
            .unwrap(),
            vec![BoundaryRecord::none()],
            Vec::new(),
        )
        .unwrap();
        let material = SphericalCrustMaterialState::new(
            vec![0.0; cell_count],
            vec![0.0; cell_count],
            vec![1.0; cell_count],
            vec![3_000.0; cell_count],
        )
        .unwrap();
        let totals = material.totals();
        let processes = SphericalTectonicMaterialProcesses::new(
            0.0,
            0.0,
            TectonicMaterialAmount::zero(),
            TectonicMaterialAmount::zero(),
            TectonicMaterialAmount::zero(),
            TectonicMaterialAmount::zero(),
            TectonicMaterialAmount::zero(),
        )
        .unwrap();
        let material_budget =
            SphericalTectonicMaterialBudget::new(totals, processes, totals, totals, 0.0, 0.0)
                .unwrap();
        let forcing = SphericalTectonicForcingState::new(
            vec![0.0; cell_count],
            vec![0.0; cell_count],
            vec![0.0; cell_count],
            vec![0.0; cell_count],
            vec![NO_OROGENY_AGE_SENTINEL_MYR; cell_count],
        )
        .unwrap();
        let evolved = EvolvedTectonicSnapshot::new(
            EVOLVED_TECTONIC_SNAPSHOT_SCHEMA_V1,
            resolution_plan,
            compatibility,
            material,
            forcing,
            material_budget,
            SphericalTectonicLineageBudget::new(1, 0, 0, 1, 0, 0).unwrap(),
        )
        .unwrap();
        EvolvedFixture { evolved }
    }

    #[test]
    fn authoritative_view_borrows_only_causal_fields() {
        let snapshot = evolved_fixture().evolved;
        let view = snapshot.authoritative_view();
        assert!(std::ptr::eq(
            view.crust_thickness_km().as_ptr(),
            snapshot.compatibility().crust_thickness_km().as_ptr(),
        ));
        assert!(std::ptr::eq(view.material(), snapshot.material()));
        assert!(std::ptr::eq(view.forcing(), snapshot.forcing()));
    }
}
