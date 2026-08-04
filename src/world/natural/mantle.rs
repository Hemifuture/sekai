use std::collections::BTreeSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::{CellId, HotspotId, Meters};

use super::MAX_HOTSPOT_COUNT;

/// The supported version of the serialized mantle snapshot schema.
pub const MANTLE_SNAPSHOT_SCHEMA_V1: u16 = 1;
/// The minimum supported surface heat flow, in milliwatts per square meter.
pub const HEAT_FLOW_MIN_MW_M2: f32 = 20.0;
/// The maximum supported surface heat flow, in milliwatts per square meter.
pub const HEAT_FLOW_MAX_MW_M2: f32 = 400.0;
/// The minimum nonzero V1 hotspot strength in permille.
pub const MIN_HOTSPOT_STRENGTH_PERMILLE: u16 = 1;
/// The maximum V1 hotspot strength in permille.
pub const MAX_HOTSPOT_STRENGTH_PERMILLE: u16 = 1_000;

/// One present-day mantle heat anomaly and its surface support.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hotspot {
    id: HotspotId,
    source_cell: CellId,
    strength_permille: u16,
    support_radius_m: Meters,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HotspotWire {
    id: HotspotId,
    source_cell: CellId,
    strength_permille: u16,
    support_radius_m: Meters,
}

impl Hotspot {
    /// Constructs a hotspot only when its local physical values are valid.
    pub fn new(
        id: HotspotId,
        source_cell: CellId,
        strength_permille: u16,
        support_radius_m: Meters,
    ) -> Result<Self, MantleValidationError> {
        let hotspot = Self {
            id,
            source_cell,
            strength_permille,
            support_radius_m,
        };
        hotspot.validate()?;
        Ok(hotspot)
    }

    /// Returns the contiguous stable hotspot identifier.
    pub const fn id(&self) -> HotspotId {
        self.id
    }

    /// Returns the spatial cell at the center of this current anomaly.
    pub const fn source_cell(&self) -> CellId {
        self.source_cell
    }

    /// Returns the normalized anomaly strength in permille.
    pub const fn strength_permille(&self) -> u16 {
        self.strength_permille
    }

    /// Returns the positive radial support of the anomaly.
    pub const fn support_radius_m(&self) -> Meters {
        self.support_radius_m
    }

    pub(super) fn validate(&self) -> Result<(), MantleValidationError> {
        if !(MIN_HOTSPOT_STRENGTH_PERMILLE..=MAX_HOTSPOT_STRENGTH_PERMILLE)
            .contains(&self.strength_permille)
        {
            return Err(MantleValidationError::HotspotStrengthOutOfRange {
                hotspot_id: self.id,
                found: self.strength_permille,
                min: MIN_HOTSPOT_STRENGTH_PERMILLE,
                max: MAX_HOTSPOT_STRENGTH_PERMILLE,
            });
        }
        if !self.support_radius_m.get().is_finite() || self.support_radius_m.get() <= 0.0 {
            return Err(MantleValidationError::InvalidSupportRadius {
                hotspot_id: self.id,
                found: self.support_radius_m.get(),
            });
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Hotspot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = HotspotWire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.source_cell,
            wire.strength_permille,
            wire.support_radius_m,
        )
        .map_err(D::Error::custom)
    }
}

/// Immutable present-day mantle forcing fields over spatial cells.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MantleSnapshot {
    schema_version: u16,
    cell_count: u32,
    hotspots: Vec<Hotspot>,
    heat_flow_mw_m2: Vec<f32>,
    volcanic_influence: Vec<f32>,
}

#[derive(Deserialize)]
struct MantleSnapshotWire {
    schema_version: u16,
    cell_count: u32,
    hotspots: Vec<Hotspot>,
    heat_flow_mw_m2: Vec<f32>,
    volcanic_influence: Vec<f32>,
}

impl MantleSnapshot {
    /// Constructs a snapshot only when all V1 mantle invariants hold.
    pub fn new(
        schema_version: u16,
        cell_count: u32,
        mut hotspots: Vec<Hotspot>,
        heat_flow_mw_m2: Vec<f32>,
        volcanic_influence: Vec<f32>,
    ) -> Result<Self, MantleValidationError> {
        hotspots.sort_by_key(Hotspot::id);
        let snapshot = Self {
            schema_version,
            cell_count,
            hotspots,
            heat_flow_mw_m2,
            volcanic_influence,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks every self-contained mantle invariant.
    pub fn validate(&self) -> Result<(), MantleValidationError> {
        if self.schema_version != MANTLE_SNAPSHOT_SCHEMA_V1 {
            return Err(MantleValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: MANTLE_SNAPSHOT_SCHEMA_V1,
            });
        }
        if self.hotspots.len() > usize::from(MAX_HOTSPOT_COUNT) {
            return Err(MantleValidationError::TooManyHotspots {
                found: self.hotspots.len(),
                max: MAX_HOTSPOT_COUNT,
            });
        }

        let mut source_cells = BTreeSet::new();
        for (index, hotspot) in self.hotspots.iter().enumerate() {
            hotspot.validate()?;
            let expected = HotspotId::from_raw(index as u32);
            if hotspot.id != expected {
                return Err(MantleValidationError::NonContiguousHotspotId {
                    expected,
                    found: hotspot.id,
                });
            }
            if hotspot.source_cell.raw() >= self.cell_count {
                return Err(MantleValidationError::HotspotSourceCellOutOfRange {
                    hotspot_id: hotspot.id,
                    source_cell: hotspot.source_cell,
                    cell_count: self.cell_count,
                });
            }
            if !source_cells.insert(hotspot.source_cell) {
                return Err(MantleValidationError::DuplicateHotspotSourceCell {
                    source_cell: hotspot.source_cell,
                });
            }
        }

        validate_length(
            "heat_flow_mw_m2",
            self.heat_flow_mw_m2.len(),
            self.cell_count,
        )?;
        validate_length(
            "volcanic_influence",
            self.volcanic_influence.len(),
            self.cell_count,
        )?;
        for (index, &value) in self.heat_flow_mw_m2.iter().enumerate() {
            if !value.is_finite() || !(HEAT_FLOW_MIN_MW_M2..=HEAT_FLOW_MAX_MW_M2).contains(&value) {
                return Err(MantleValidationError::HeatFlowOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: value,
                    min: HEAT_FLOW_MIN_MW_M2,
                    max: HEAT_FLOW_MAX_MW_M2,
                });
            }
        }
        for (index, &value) in self.volcanic_influence.iter().enumerate() {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(MantleValidationError::VolcanicInfluenceOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: value,
                });
            }
        }
        Ok(())
    }

    /// Rechecks this snapshot against the exact spatial allocation and extent.
    pub fn validate_against(&self, spatial: &SpatialSnapshot) -> Result<(), MantleValidationError> {
        self.validate()?;
        if spatial.cell_count() != self.cell_count as usize {
            return Err(MantleValidationError::SpatialCellCountMismatch {
                snapshot: self.cell_count,
                spatial: spatial.cell_count(),
            });
        }

        let bounds = spatial.bounds();
        let diagonal_m = bounds.width().get().hypot(bounds.height().get());
        for hotspot in &self.hotspots {
            if hotspot.support_radius_m.get() > diagonal_m {
                return Err(MantleValidationError::SupportRadiusExceedsWorldDiagonal {
                    hotspot_id: hotspot.id,
                    found_m: hotspot.support_radius_m.get(),
                    diagonal_m,
                });
            }
        }
        Ok(())
    }

    /// Returns the serialized schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact dense spatial-cell cardinality.
    pub const fn cell_count(&self) -> u32 {
        self.cell_count
    }

    /// Returns current mantle hotspots in contiguous ID order.
    pub fn hotspots(&self) -> &[Hotspot] {
        &self.hotspots
    }

    /// Returns surface heat flow in milliwatts per square meter without copying.
    pub fn heat_flow_mw_m2(&self) -> &[f32] {
        &self.heat_flow_mw_m2
    }

    /// Returns normalized present-day volcanic influence without copying.
    pub fn volcanic_influence(&self) -> &[f32] {
        &self.volcanic_influence
    }
}

impl<'de> Deserialize<'de> for MantleSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = MantleSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.cell_count,
            wire.hotspots,
            wire.heat_flow_mw_m2,
            wire.volcanic_influence,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_length(
    field: &'static str,
    found: usize,
    cell_count: u32,
) -> Result<(), MantleValidationError> {
    let expected = cell_count as usize;
    if found != expected {
        return Err(MantleValidationError::FieldLengthMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

/// Errors returned when mantle forcing violates a V1 contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum MantleValidationError {
    /// The snapshot uses a schema version that this engine does not support.
    #[error("unsupported mantle schema version {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
        /// The supported schema version.
        supported: u16,
    },
    /// The snapshot exceeds the V1 hotspot allocation budget.
    #[error("hotspot count {found} exceeds the maximum {max}")]
    TooManyHotspots {
        /// The rejected hotspot count.
        found: usize,
        /// The inclusive upper hotspot-count limit.
        max: u16,
    },
    /// A hotspot strength is zero or above the normalized V1 maximum.
    #[error("hotspot {hotspot_id:?} strength {found} is outside {min}..={max}")]
    HotspotStrengthOutOfRange {
        /// The hotspot with invalid strength.
        hotspot_id: HotspotId,
        /// The rejected strength.
        found: u16,
        /// The inclusive lower strength limit.
        min: u16,
        /// The inclusive upper strength limit.
        max: u16,
    },
    /// A hotspot support radius is non-positive or non-finite.
    #[error("hotspot {hotspot_id:?} support radius must be positive and finite, got {found}")]
    InvalidSupportRadius {
        /// The hotspot with invalid support.
        hotspot_id: HotspotId,
        /// The rejected support radius in meters.
        found: f64,
    },
    /// Hotspot identifiers do not form the exact contiguous range from zero.
    #[error("expected hotspot ID {expected:?}, found {found:?}")]
    NonContiguousHotspotId {
        /// The required ID at this canonical position.
        expected: HotspotId,
        /// The stored non-contiguous ID.
        found: HotspotId,
    },
    /// Several hotspots use the same source cell.
    #[error("several hotspots use source cell {source_cell:?}")]
    DuplicateHotspotSourceCell {
        /// The duplicated source cell.
        source_cell: CellId,
    },
    /// A hotspot source cell lies outside the dense snapshot allocation.
    #[error("hotspot {hotspot_id:?} source {source_cell:?} lies outside cell count {cell_count}")]
    HotspotSourceCellOutOfRange {
        /// The hotspot with an invalid source.
        hotspot_id: HotspotId,
        /// The rejected source cell.
        source_cell: CellId,
        /// The snapshot cell count.
        cell_count: u32,
    },
    /// A dense field length differs from the snapshot cell count.
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        /// The stable field name.
        field: &'static str,
        /// The required dense length.
        expected: usize,
        /// The actual dense length.
        found: usize,
    },
    /// A surface heat-flow value is non-finite or outside its physical envelope.
    #[error("heat flow at {cell:?} is {found}; expected finite {min}..={max}")]
    HeatFlowOutOfRange {
        /// The cell containing the rejected value.
        cell: CellId,
        /// The rejected value.
        found: f32,
        /// The inclusive lower heat-flow limit.
        min: f32,
        /// The inclusive upper heat-flow limit.
        max: f32,
    },
    /// A volcanic-influence value is non-finite or outside zero to one.
    #[error("volcanic influence at {cell:?} is {found}; expected finite 0..=1")]
    VolcanicInfluenceOutOfRange {
        /// The cell containing the rejected value.
        cell: CellId,
        /// The rejected value.
        found: f32,
    },
    /// The snapshot and topology have different dense cell counts.
    #[error("mantle cell count {snapshot} does not match spatial cell count {spatial}")]
    SpatialCellCountMismatch {
        /// The mantle snapshot cell count.
        snapshot: u32,
        /// The spatial topology cell count.
        spatial: usize,
    },
    /// A hotspot support radius exceeds the maximum useful spatial distance.
    #[error(
        "hotspot {hotspot_id:?} support radius {found_m} m exceeds world diagonal {diagonal_m} m"
    )]
    SupportRadiusExceedsWorldDiagonal {
        /// The hotspot with excessive support.
        hotspot_id: HotspotId,
        /// The rejected support radius in meters.
        found_m: f64,
        /// The spatial bounds diagonal in meters.
        diagonal_m: f64,
    },
}
