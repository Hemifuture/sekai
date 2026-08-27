//! Shared transient-model constants for evolved spherical tectonics.
//!
//! Recipe presets select a coherent-noise spectrum, bounded integer process
//! multipliers, and which continental rifts may complete to ocean. Opening
//! nucleus counts live on `ResolvedWorldFormationPreset`.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeSet;

use crate::generators::natural::fractal::FractalProfile;
use crate::world::natural::{
    CrustKind, CrustMaterialTotals, ResolvedWorldFormationPreset, SphericalOrogenyKind,
    SphericalPlateRotation, SphericalTectonicLineageBudget, SphericalTectonicMaterialBudget,
    SphericalTectonicMaterialProcesses, TectonicMaterialAmount, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
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

impl CrustSample {
    pub(super) fn synchronize_compatibility_from_material(&mut self) {
        self.kind = self.material.compatibility_kind();
        self.thickness_km = self.material.compatibility_thickness_km();
    }
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

    pub(super) fn continental_amount(self) -> Result<TectonicMaterialAmount, MaterialColumnError> {
        material_amount(
            self.continental_reference_area_m2,
            self.continental_volume_m3,
        )
    }

    pub(super) fn continental_thickness_km(self) -> Option<f32> {
        (self.continental_reference_area_m2 > 0.0).then_some(
            (self.continental_volume_m3 / self.continental_reference_area_m2 / 1_000.0) as f32,
        )
    }

    pub(super) fn oceanic_thickness_km(self) -> Option<f32> {
        (self.oceanic_reference_area_m2 > 0.0)
            .then_some((self.oceanic_volume_m3 / self.oceanic_reference_area_m2 / 1_000.0) as f32)
    }

    pub(super) fn oceanic_amount(self) -> Result<TectonicMaterialAmount, MaterialColumnError> {
        material_amount(self.oceanic_reference_area_m2, self.oceanic_volume_m3)
    }

    /// Moves continental area at the donor's current thickness into a new
    /// column. Volume is bit-conserved. A pure-continental donor keeps a 1 m²
    /// residual so the moving sample stays non-empty; a mixed donor keeps
    /// continental majority so the donor cell does not flip to ocean.
    pub(super) fn extract_continental_area(
        self,
        requested_area_m2: f64,
    ) -> Result<(Self, Option<Self>), MaterialColumnError> {
        if !requested_area_m2.is_finite() || requested_area_m2 <= 0.0 {
            return Ok((self, None));
        }
        let available = self.continental_reference_area_m2;
        if available <= 0.0 {
            return Ok((self, None));
        }
        let max_take = if self.oceanic_reference_area_m2 == 0.0 {
            (available - 1.0).max(0.0)
        } else {
            (available - self.oceanic_reference_area_m2).max(0.0)
        };
        let take_area = requested_area_m2.min(max_take);
        if take_area <= 0.0 {
            return Ok((self, None));
        }
        let take_volume = self.continental_volume_m3 * (take_area / available);
        let remaining = Self::new(
            available - take_area,
            self.continental_volume_m3 - take_volume,
            self.oceanic_reference_area_m2,
            self.oceanic_volume_m3,
        )?;
        let taken = Self::new(take_area, take_volume, 0.0, 0.0)?;
        Ok((remaining, Some(taken)))
    }

    /// Removes the complete oceanic component, returning `None` when that
    /// leaves no represented column at all.
    pub(super) fn without_oceanic(self) -> Result<Option<Self>, MaterialColumnError> {
        if self.continental_reference_area_m2 == 0.0 {
            return Ok(None);
        }
        Self::new(
            self.continental_reference_area_m2,
            self.continental_volume_m3,
            0.0,
            0.0,
        )
        .map(Some)
    }

    /// Applies bounded pure-shear extension to continental material only.
    /// Volume is bit-preserved and the returned value is the exact reference
    /// area gained by the column.
    pub(super) fn extend_continental_pure_shear(
        self,
        requested_beta: f64,
    ) -> Result<(Self, f64), MaterialColumnError> {
        if !requested_beta.is_finite() || requested_beta < 1.0 {
            return Err(MaterialColumnError::InvalidStretchFactor {
                found: requested_beta,
            });
        }
        if self.continental_reference_area_m2 == 0.0 {
            return Ok((self, 0.0));
        }
        let maximum_area =
            self.continental_volume_m3 / (f64::from(CONTINENTAL_CRUST_MIN_THICKNESS_KM) * 1_000.0);
        let extended_area = (self.continental_reference_area_m2 * requested_beta)
            .min(maximum_area)
            .max(self.continental_reference_area_m2);
        let gain = extended_area - self.continental_reference_area_m2;
        let extended = Self::new(
            extended_area,
            self.continental_volume_m3,
            self.oceanic_reference_area_m2,
            self.oceanic_volume_m3,
        )?;
        Ok((extended, gain))
    }

    /// Applies bounded pure-shear shortening to continental material only: the
    /// inverse of [`Self::extend_continental_pure_shear`]. Volume is
    /// bit-preserved, the column thickens up to the public maximum, and the
    /// returned value is the exact reference area lost by the column.
    pub(super) fn shorten_continental_pure_shear(
        self,
        requested_beta: f64,
    ) -> Result<(Self, f64), MaterialColumnError> {
        if !requested_beta.is_finite() || requested_beta < 1.0 {
            return Err(MaterialColumnError::InvalidStretchFactor {
                found: requested_beta,
            });
        }
        if self.continental_reference_area_m2 == 0.0 {
            return Ok((self, 0.0));
        }
        let minimum_area =
            self.continental_volume_m3 / (f64::from(CONTINENTAL_CRUST_MAX_THICKNESS_KM) * 1_000.0);
        let shortened_area = (self.continental_reference_area_m2 / requested_beta)
            .max(minimum_area)
            .min(self.continental_reference_area_m2);
        let loss = self.continental_reference_area_m2 - shortened_area;
        let shortened = Self::new(
            shortened_area,
            self.continental_volume_m3,
            self.oceanic_reference_area_m2,
            self.oceanic_volume_m3,
        )?;
        Ok((shortened, loss))
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
    #[error("continental pure-shear stretch factor is invalid: {found}")]
    InvalidStretchFactor { found: f64 },
    #[error("material source/sink area and volume changes must have the same sign")]
    InconsistentCoverageChange,
    #[error("material totals are invalid: {0}")]
    InvalidTotals(String),
}

fn material_amount(
    reference_area_m2: f64,
    volume_m3: f64,
) -> Result<TectonicMaterialAmount, MaterialColumnError> {
    TectonicMaterialAmount::new(reference_area_m2, volume_m3)
        .map_err(|error| MaterialColumnError::InvalidTotals(error.to_string()))
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

/// Canonical unordered pair of live lineages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct LineagePair {
    first: LineageId,
    second: LineageId,
}

impl LineagePair {
    pub(super) fn new(first: LineageId, second: LineageId) -> Self {
        if first <= second {
            Self { first, second }
        } else {
            Self {
                first: second,
                second: first,
            }
        }
    }

    fn contains(self, lineage: LineageId) -> bool {
        self.first == lineage || self.second == lineage
    }

    fn other(self, lineage: LineageId) -> LineageId {
        if self.first == lineage {
            self.second
        } else {
            self.first
        }
    }
}

/// Private solver tags for Stern (2004) subduction initiation.
///
/// Not published: G1d forbids a new release schema. Trenches persist once
/// started; continental rift siblings mark this-cycle Atlantic-type oceans.
/// Dominant lineages are the plates of the largest opening continental
/// component (GreatIsland primary mass / Supercontinent body).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SubductionInitiation {
    established_trenches: BTreeSet<LineagePair>,
    continental_rift_pairs: BTreeSet<LineagePair>,
    dominant_continental_lineages: BTreeSet<LineageId>,
}

impl SubductionInitiation {
    pub(super) fn has_trench(&self, first: LineageId, second: LineageId) -> bool {
        self.established_trenches
            .contains(&LineagePair::new(first, second))
    }

    pub(super) fn is_this_cycle_rift_pair(&self, first: LineageId, second: LineageId) -> bool {
        self.continental_rift_pairs
            .contains(&LineagePair::new(first, second))
    }

    pub(super) fn record_trench(&mut self, first: LineageId, second: LineageId) {
        if first != second {
            self.established_trenches
                .insert(LineagePair::new(first, second));
        }
    }

    pub(super) fn record_rift_pair(&mut self, first: LineageId, second: LineageId) {
        if first != second {
            self.continental_rift_pairs
                .insert(LineagePair::new(first, second));
        }
    }

    pub(super) fn mark_dominant(&mut self, lineage: LineageId) {
        self.dominant_continental_lineages.insert(lineage);
    }

    pub(super) fn is_dominant(&self, lineage: LineageId) -> bool {
        self.dominant_continental_lineages.contains(&lineage)
    }

    pub(super) fn both_dominant(&self, first: LineageId, second: LineageId) -> bool {
        self.is_dominant(first) && self.is_dominant(second)
    }

    pub(super) fn involves_dominant(&self, first: LineageId, second: LineageId) -> bool {
        self.is_dominant(first) || self.is_dominant(second)
    }

    pub(super) fn replace_lineage(&mut self, parent: LineageId, children: &[LineageId]) {
        self.remap_inherited_lineage(parent, children);
        for (index, &first) in children.iter().enumerate() {
            for &second in &children[index + 1..] {
                self.continental_rift_pairs
                    .insert(LineagePair::new(first, second));
            }
        }
    }

    pub(super) fn remap_inherited_lineage(&mut self, parent: LineageId, children: &[LineageId]) {
        remap_pairs(&mut self.established_trenches, parent, children);
        remap_pairs(&mut self.continental_rift_pairs, parent, children);
        remap_lineage_set(&mut self.dominant_continental_lineages, parent, children);
    }
}

fn remap_lineage_set(set: &mut BTreeSet<LineageId>, parent: LineageId, children: &[LineageId]) {
    if set.remove(&parent) {
        set.extend(children.iter().copied());
    }
}

fn remap_pairs(pairs: &mut BTreeSet<LineagePair>, parent: LineageId, children: &[LineageId]) {
    let inherited = pairs
        .iter()
        .copied()
        .filter(|pair| pair.contains(parent))
        .collect::<Vec<_>>();
    for pair in inherited {
        pairs.remove(&pair);
        let other = pair.other(parent);
        for &child in children {
            if child != other {
                pairs.insert(LineagePair::new(child, other));
            }
        }
    }
}

/// The only logical state at one evolution step.
#[derive(Debug)]
pub(super) struct TectonicState {
    pub(super) samples: Vec<CrustSample>,
    pub(super) plates: Vec<ActivePlate>,
    pub(super) initiation: SubductionInitiation,
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
            initiation: SubductionInitiation::default(),
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
            initiation: SubductionInitiation::default(),
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
        self.initiation = current.initiation.clone();
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
    rift_extension_continental_area_gain: CompensatedSum,
    collision_shortening_continental_area_loss: CompensatedSum,
    continental_consumed: MaterialAccumulator,
    oceanic_subducted: MaterialAccumulator,
    oceanic_spreading_created: MaterialAccumulator,
    oceanic_coverage_created: MaterialAccumulator,
    oceanic_coverage_consumed: MaterialAccumulator,
}

impl EvolutionMaterialLedger {
    const MAXIMUM_RIFT_EXTENSION_AREA_FRACTION: f64 = 0.15 - 1.0e-12;
    /// Collision shortening may retire at most the same share of the initial
    /// continental area that rifting may add, so the two pure-shear processes
    /// stay symmetric and the inventory area cannot collapse (Wise 1974
    /// constant-area freeboard argument).
    const MAXIMUM_COLLISION_SHORTENING_AREA_FRACTION: f64 =
        Self::MAXIMUM_RIFT_EXTENSION_AREA_FRACTION;

    pub(super) fn capture_initial(state: &TectonicState) -> Result<Self, MaterialColumnError> {
        Ok(Self {
            initial_control: state.material_totals()?,
            rift_extension_continental_area_gain: CompensatedSum::default(),
            collision_shortening_continental_area_loss: CompensatedSum::default(),
            continental_consumed: MaterialAccumulator::default(),
            oceanic_subducted: MaterialAccumulator::default(),
            oceanic_spreading_created: MaterialAccumulator::default(),
            oceanic_coverage_created: MaterialAccumulator::default(),
            oceanic_coverage_consumed: MaterialAccumulator::default(),
        })
    }

    pub(super) const fn initial_control(self) -> CrustMaterialTotals {
        self.initial_control
    }

    pub(super) fn record_rift_extension_area_gain(&mut self, area_m2: f64) {
        debug_assert!(area_m2.is_finite() && area_m2 >= 0.0);
        self.rift_extension_continental_area_gain.add(area_m2);
    }

    pub(super) fn remaining_rift_extension_area_m2(self) -> f64 {
        let maximum = self.initial_control.continental().reference_area_m2()
            * Self::MAXIMUM_RIFT_EXTENSION_AREA_FRACTION;
        (maximum - self.rift_extension_continental_area_gain.total()).max(0.0)
    }

    pub(super) fn record_collision_shortening_area_loss(&mut self, area_m2: f64) {
        debug_assert!(area_m2.is_finite() && area_m2 >= 0.0);
        self.collision_shortening_continental_area_loss.add(area_m2);
    }

    pub(super) fn remaining_collision_shortening_area_m2(self) -> f64 {
        let maximum = self.initial_control.continental().reference_area_m2()
            * Self::MAXIMUM_COLLISION_SHORTENING_AREA_FRACTION;
        (maximum - self.collision_shortening_continental_area_loss.total()).max(0.0)
    }

    pub(super) fn record_oceanic_subduction(&mut self, amount: TectonicMaterialAmount) {
        self.oceanic_subducted.add(amount);
    }

    pub(super) fn record_oceanic_spreading(&mut self, amount: TectonicMaterialAmount) {
        self.oceanic_spreading_created.add(amount);
    }

    pub(super) fn record_coverage_change(
        &mut self,
        area_delta_m2: f64,
        volume_delta_m3: f64,
    ) -> Result<(), MaterialColumnError> {
        if !area_delta_m2.is_finite()
            || !volume_delta_m3.is_finite()
            || area_delta_m2.signum() != volume_delta_m3.signum()
        {
            return Err(MaterialColumnError::InconsistentCoverageChange);
        }
        if area_delta_m2 > 0.0 {
            self.oceanic_coverage_created
                .add(material_amount(area_delta_m2, volume_delta_m3)?);
        } else if area_delta_m2 < 0.0 {
            self.oceanic_coverage_consumed
                .add(material_amount(-area_delta_m2, -volume_delta_m3)?);
        } else if volume_delta_m3 != 0.0 {
            return Err(MaterialColumnError::InconsistentCoverageChange);
        }
        Ok(())
    }

    pub(super) fn processes(
        self,
    ) -> Result<SphericalTectonicMaterialProcesses, MaterialColumnError> {
        SphericalTectonicMaterialProcesses::new(
            self.rift_extension_continental_area_gain.total(),
            self.collision_shortening_continental_area_loss.total(),
            self.continental_consumed.amount()?,
            self.oceanic_subducted.amount()?,
            self.oceanic_spreading_created.amount()?,
            self.oceanic_coverage_created.amount()?,
            self.oceanic_coverage_consumed.amount()?,
        )
        .map_err(|error| MaterialColumnError::InvalidTotals(error.to_string()))
    }

    pub(super) fn control_budget(
        self,
        state: &TectonicState,
    ) -> Result<SphericalTectonicMaterialBudget, MaterialColumnError> {
        let final_control = state.material_totals()?;
        SphericalTectonicMaterialBudget::new(
            self.initial_control,
            self.processes()?,
            final_control,
            final_control,
            0.0,
            0.0,
        )
        .map_err(|error| MaterialColumnError::InvalidTotals(error.to_string()))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EvolutionLineageLedger {
    initial_lineages: u32,
    first_allocated_raw: u32,
    terrane_transfer_count: u32,
    mechanical_fragmentation_count: u32,
}

impl EvolutionLineageLedger {
    pub(super) fn capture_initial(state: &TectonicState) -> Result<Self, LineageLedgerError> {
        Ok(Self {
            initial_lineages: u32::try_from(state.plates.len())
                .map_err(|_| LineageLedgerError::CountOverflow)?,
            first_allocated_raw: state.next_lineage_raw,
            terrane_transfer_count: 0,
            mechanical_fragmentation_count: 0,
        })
    }

    pub(super) fn record_terrane_transfers(&mut self, count: u32) {
        self.terrane_transfer_count = self.terrane_transfer_count.saturating_add(count);
    }

    pub(super) fn record_mechanical_fragmentation(&mut self) {
        self.mechanical_fragmentation_count = self.mechanical_fragmentation_count.saturating_add(1);
    }

    pub(super) fn budget(
        self,
        state: &TectonicState,
    ) -> Result<SphericalTectonicLineageBudget, LineageLedgerError> {
        let allocated_lineages = state
            .next_lineage_raw
            .checked_sub(self.first_allocated_raw)
            .ok_or(LineageLedgerError::ReusedLineage)?;
        let mut live = state
            .samples
            .iter()
            .map(|sample| sample.owner)
            .collect::<Vec<_>>();
        live.sort_unstable();
        live.dedup();
        let final_live_lineages =
            u32::try_from(live.len()).map_err(|_| LineageLedgerError::CountOverflow)?;
        let created = self
            .initial_lineages
            .checked_add(allocated_lineages)
            .ok_or(LineageLedgerError::CountOverflow)?;
        let retired_lineages = created
            .checked_sub(final_live_lineages)
            .ok_or(LineageLedgerError::LiveCountExceedsAllocated)?;
        SphericalTectonicLineageBudget::new(
            self.initial_lineages,
            allocated_lineages,
            retired_lineages,
            final_live_lineages,
            self.terrane_transfer_count,
            self.mechanical_fragmentation_count,
        )
        .map_err(|error| LineageLedgerError::InvalidBudget(error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum LineageLedgerError {
    #[error("lineage count exceeded the supported integer range")]
    CountOverflow,
    #[error("a transient lineage identifier was reused")]
    ReusedLineage,
    #[error("live lineage count exceeds all allocated lineages")]
    LiveCountExceedsAllocated,
    #[error("lineage budget is invalid: {0}")]
    InvalidBudget(String),
}

#[derive(Clone, Copy, Debug, Default)]
struct MaterialAccumulator {
    area: CompensatedSum,
    volume: CompensatedSum,
}

impl MaterialAccumulator {
    fn add(&mut self, amount: TectonicMaterialAmount) {
        self.area.add(amount.reference_area_m2());
        self.volume.add(amount.volume_m3());
    }

    fn amount(self) -> Result<TectonicMaterialAmount, MaterialColumnError> {
        material_amount(self.area.total(), self.volume.total())
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

/// Which continental rifts may become persistent ocean (G1d §3.1).
///
/// This is the Wilson *release phase*, not a new engine: Cortial rift →
/// spreading fill still runs; only the fill product is gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::generators::natural) enum OceanizationPolicy {
    /// Continents / Archipelago: Atlantic-type rifts complete to ocean.
    Complete,
    /// Supercontinent stable phase (Nance & Murphy 2013) and VolcanicIslands:
    /// no continent-scale new ocean. Intra-continental rifts stay McKenzie
    /// thinning (McKenzie 1978).
    SuppressContinentalBreakup,
    /// GreatIsland: the dominant mass does not oceanize; satellites may.
    ExceptDominant,
}

impl OceanizationPolicy {
    pub(super) fn forbids_breakup_ocean(
        self,
        initiation: &SubductionInitiation,
        first: LineageId,
        second: LineageId,
    ) -> bool {
        match self {
            Self::Complete => false,
            Self::SuppressContinentalBreakup => {
                initiation.is_this_cycle_rift_pair(first, second)
                    || initiation.both_dominant(first, second)
            }
            Self::ExceptDominant => {
                initiation.both_dominant(first, second)
                    || (initiation.is_this_cycle_rift_pair(first, second)
                        && initiation.involves_dominant(first, second))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::generators::natural) struct FormationTectonicRecipe {
    pub(in crate::generators::natural) initial_crust_profile: FractalProfile,
    pub(in crate::generators::natural) base_scale_rad: f64,
    pub(in crate::generators::natural) rift_rate_permille: u16,
    pub(in crate::generators::natural) subduction_gain_permille: u16,
    pub(in crate::generators::natural) island_arc_gain_permille: u16,
    pub(in crate::generators::natural) oceanization: OceanizationPolicy,
}

impl FormationTectonicRecipe {
    pub(in crate::generators::natural) const fn for_preset(
        preset: ResolvedWorldFormationPreset,
    ) -> Self {
        use ResolvedWorldFormationPreset::{
            Archipelago, Continents, GreatIsland, Supercontinent, VolcanicIslands,
        };

        match preset {
            Continents => Self::new(4, 1.8, 0.75, 1_000, 1_000, 1_000)
                .with_oceanization(OceanizationPolicy::Complete),
            Archipelago => Self::new(5, 3.4, 0.40, 1_250, 950, 1_150)
                .with_oceanization(OceanizationPolicy::Complete),
            Supercontinent => Self::new(3, 1.1, 1.15, 700, 1_050, 850)
                .with_oceanization(OceanizationPolicy::SuppressContinentalBreakup),
            GreatIsland => Self::new(4, 1.5, 0.90, 850, 1_000, 950)
                .with_oceanization(OceanizationPolicy::ExceptDominant),
            VolcanicIslands => Self::new(5, 4.2, 0.32, 1_100, 1_200, 1_500)
                .with_oceanization(OceanizationPolicy::SuppressContinentalBreakup),
        }
    }

    const fn with_oceanization(self, oceanization: OceanizationPolicy) -> Self {
        Self {
            oceanization,
            ..self
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
            oceanization: OceanizationPolicy::Complete,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActivePlate, CrustSample, EvolutionMaterialLedger, FormationTectonicRecipe, LineageId,
        MaterialColumn, OceanizationPolicy, SubductionInitiation, TectonicState,
    };
    use crate::world::natural::{
        CrustKind, ResolvedWorldFormationPreset, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::UnitVector3;
    use crate::world::CellId;

    #[test]
    fn pure_shear_shortening_thickens_volume_preserving_up_to_the_public_maximum() {
        let column = MaterialColumn::pure(CrustKind::Continental, 1.0, 40.0).unwrap();
        let (shortened, loss) = column.shorten_continental_pure_shear(1.2).unwrap();
        assert_eq!(
            shortened.continental_volume_m3(),
            column.continental_volume_m3()
        );
        assert!((shortened.continental_reference_area_m2() - 1.0 / 1.2).abs() <= 1.0e-12);
        assert!((loss - (1.0 - 1.0 / 1.2)).abs() <= 1.0e-12);
        assert!((shortened.continental_thickness_km().unwrap() - 48.0).abs() <= 1.0e-3);

        let (capped, capped_loss) = column.shorten_continental_pure_shear(10.0).unwrap();
        assert_eq!(capped.continental_thickness_km(), Some(80.0));
        assert!((capped_loss - (1.0 - 0.5)).abs() <= 1.0e-12);
        assert!(column.shorten_continental_pure_shear(0.9).is_err());

        let oceanic = MaterialColumn::pure(CrustKind::Oceanic, 1.0, 7.0).unwrap();
        let (untouched, no_loss) = oceanic.shorten_continental_pure_shear(1.2).unwrap();
        assert_eq!(untouched.bits(), oceanic.bits());
        assert_eq!(no_loss, 0.0);
    }

    #[test]
    fn pure_and_mixed_columns_derive_material_without_independent_thickness() {
        let continental = MaterialColumn::pure(CrustKind::Continental, 2.5, 40.0).unwrap();
        assert_eq!(continental.continental_reference_area_m2(), 2.5);
        assert_eq!(continental.continental_volume_m3(), 100_000.0);
        assert_eq!(continental.oceanic_reference_area_m2(), 0.0);
        assert_eq!(continental.oceanic_volume_m3(), 0.0);
        assert_eq!(continental.compatibility_kind(), CrustKind::Continental);
        assert_eq!(continental.compatibility_thickness_km(), 40.0);
        assert_eq!(
            continental
                .continental_amount()
                .unwrap()
                .reference_area_m2(),
            2.5
        );

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

    #[test]
    fn extract_continental_area_conserves_volume_and_keeps_donor_continental() {
        let column = MaterialColumn::pure(CrustKind::Continental, 100.0, 40.0).unwrap();
        let volume = column.continental_volume_m3();
        let (remaining, taken) = column.extract_continental_area(30.0).unwrap();
        let taken = taken.expect("pure continent can donate");
        assert_eq!(
            remaining.continental_volume_m3() + taken.continental_volume_m3(),
            volume
        );
        assert!((taken.continental_reference_area_m2() - 30.0).abs() <= 1.0e-12);
        assert_eq!(remaining.compatibility_kind(), CrustKind::Continental);
        assert_eq!(taken.compatibility_kind(), CrustKind::Continental);

        let mixed = MaterialColumn::new(5.0, 150_000.0, 3.0, 21_000.0).unwrap();
        let (remaining_mixed, taken_mixed) = mixed.extract_continental_area(10.0).unwrap();
        let taken_mixed = taken_mixed.expect("continental majority can donate");
        assert!(taken_mixed.continental_reference_area_m2() <= 2.0);
        assert_eq!(remaining_mixed.compatibility_kind(), CrustKind::Continental);
        assert!(column.extract_continental_area(0.0).unwrap().1.is_none());
    }

    #[test]
    fn oceanization_policy_matches_wilson_release_phase() {
        use OceanizationPolicy::{Complete, ExceptDominant, SuppressContinentalBreakup};
        use ResolvedWorldFormationPreset::{
            Archipelago, Continents, GreatIsland, Supercontinent, VolcanicIslands,
        };

        assert_eq!(
            FormationTectonicRecipe::for_preset(Continents).oceanization,
            Complete
        );
        assert_eq!(
            FormationTectonicRecipe::for_preset(Archipelago).oceanization,
            Complete
        );
        assert_eq!(
            FormationTectonicRecipe::for_preset(Supercontinent).oceanization,
            SuppressContinentalBreakup
        );
        assert_eq!(
            FormationTectonicRecipe::for_preset(GreatIsland).oceanization,
            ExceptDominant
        );
        assert_eq!(
            FormationTectonicRecipe::for_preset(VolcanicIslands).oceanization,
            SuppressContinentalBreakup
        );

        let mut initiation = SubductionInitiation::default();
        let main = LineageId::from_raw(1);
        let main_child = LineageId::from_raw(2);
        let satellite_a = LineageId::from_raw(3);
        let satellite_b = LineageId::from_raw(4);
        initiation.mark_dominant(main);
        initiation.mark_dominant(main_child);
        initiation.record_rift_pair(main, main_child);
        initiation.record_rift_pair(satellite_a, satellite_b);

        assert!(!Complete.forbids_breakup_ocean(&initiation, main, main_child));
        assert!(SuppressContinentalBreakup.forbids_breakup_ocean(&initiation, main, main_child));
        assert!(SuppressContinentalBreakup.forbids_breakup_ocean(
            &initiation,
            satellite_a,
            satellite_b
        ));
        assert!(ExceptDominant.forbids_breakup_ocean(&initiation, main, main_child));
        assert!(!ExceptDominant.forbids_breakup_ocean(&initiation, satellite_a, satellite_b));
    }
}
