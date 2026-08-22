use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// The current serialized relief-authoring schema.
pub const RELIEF_SPEC_SCHEMA_V2: u16 = 2;
/// The smallest supported authored share of emergent land.
pub const MIN_TARGET_LAND_FRACTION: f32 = 0.05;
/// The largest supported authored share of emergent land.
pub const MAX_TARGET_LAND_FRACTION: f32 = 0.75;
/// The T0b 17-seed water-response probe's lower authored-inventory bound.
///
/// The probe and bound selection are recorded in the T0b design §2.4 and §8.
pub const MIN_WATER_INVENTORY_RATIO: f32 = 0.05;
/// The T0b 17-seed water-response probe's upper authored-inventory bound.
///
/// The probe and bound selection are recorded in the T0b design §2.4 and §8.
pub const MAX_WATER_INVENTORY_RATIO: f32 = 5.0;
/// Lower edge of the non-blocking surface-water authoring guidance.
///
/// Frozen from the T0b 17-seed response probe and the order-of-magnitude
/// planetary-water range cited in the T0b design §2.4, §3.3, and §5.
pub const WATER_INVENTORY_RATIO_ADVISORY_MIN: f64 = 0.5;
/// Upper edge of the non-blocking surface-water authoring guidance.
///
/// Frozen from the T0b 17-seed response probe and the order-of-magnitude
/// planetary-water range cited in the T0b design §2.4, §3.3, and §5.
pub const WATER_INVENTORY_RATIO_ADVISORY_MAX: f64 = 2.0;
/// Fraction of evolved continental-crust area used for the ocean-floor hint.
///
/// This deliberately conservative UI threshold is the measured crust-exposure
/// rule frozen from T0b design §2.4 reading 4 and §8; it is not a solver gate.
pub const OCEAN_FLOOR_EXPOSURE_HINT_FRACTION: f64 = 0.9;

/// Selects which authored quantity determines global sea level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeaLevelPolicy {
    /// Solves sea level from the authored water inventory.
    WaterInventory,
    /// Selects sea level from the authored emergent-land fraction.
    TargetLandFraction,
}

/// A versioned author request for the final emergent land area.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReliefSpec {
    /// The schema version used to interpret this request.
    pub schema_version: u16,
    /// The requested share of authoritative spherical area above sea level.
    pub target_land_fraction: f32,
    /// The authored quantity that determines sea level on the formation chain.
    pub sea_level_policy: SeaLevelPolicy,
    /// Surface-water volume relative to the area-scaled Earth inventory.
    pub water_inventory_ratio: f32,
}

impl Default for ReliefSpec {
    fn default() -> Self {
        Self {
            schema_version: RELIEF_SPEC_SCHEMA_V2,
            target_land_fraction: 0.38,
            sea_level_policy: SeaLevelPolicy::WaterInventory,
            water_inventory_ratio: 1.0,
        }
    }
}

impl ReliefSpec {
    /// Validates the schema and finite land-area range.
    pub fn validate(&self) -> Result<(), ReliefSpecError> {
        if self.schema_version != RELIEF_SPEC_SCHEMA_V2 {
            return Err(ReliefSpecError::UnsupportedSchema {
                found: self.schema_version,
                supported: RELIEF_SPEC_SCHEMA_V2,
            });
        }
        if !self.target_land_fraction.is_finite()
            || !(MIN_TARGET_LAND_FRACTION..=MAX_TARGET_LAND_FRACTION)
                .contains(&self.target_land_fraction)
        {
            return Err(ReliefSpecError::TargetLandFractionOutOfRange {
                found: self.target_land_fraction,
                min: MIN_TARGET_LAND_FRACTION,
                max: MAX_TARGET_LAND_FRACTION,
            });
        }
        if !self.water_inventory_ratio.is_finite()
            || !(MIN_WATER_INVENTORY_RATIO..=MAX_WATER_INVENTORY_RATIO)
                .contains(&self.water_inventory_ratio)
        {
            return Err(ReliefSpecError::WaterInventoryRatioOutOfRange {
                found: self.water_inventory_ratio,
                min: MIN_WATER_INVENTORY_RATIO,
                max: MAX_WATER_INVENTORY_RATIO,
            });
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReliefSpecWire {
    schema_version: u16,
    target_land_fraction: f32,
    sea_level_policy: SeaLevelPolicy,
    water_inventory_ratio: f32,
}

impl<'de> Deserialize<'de> for ReliefSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ReliefSpecWire::deserialize(deserializer)?;
        let spec = Self {
            schema_version: wire.schema_version,
            target_land_fraction: wire.target_land_fraction,
            sea_level_policy: wire.sea_level_policy,
            water_inventory_ratio: wire.water_inventory_ratio,
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}

/// Errors returned for an unsupported or unsafe relief request.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ReliefSpecError {
    /// The serialized schema is not supported by this build.
    #[error("unsupported relief spec schema {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// Schema found in the request.
        found: u16,
        /// Schema supported by this build.
        supported: u16,
    },
    /// The requested land share is non-finite or outside the product range.
    #[error("target land fraction {found} is outside {min}..={max}")]
    TargetLandFractionOutOfRange {
        /// Invalid authored value.
        found: f32,
        /// Inclusive minimum.
        min: f32,
        /// Inclusive maximum.
        max: f32,
    },
    /// The authored water inventory ratio is non-finite or outside the product range.
    #[error("water inventory ratio {found} is outside {min}..={max}")]
    WaterInventoryRatioOutOfRange {
        /// Invalid authored value.
        found: f32,
        /// Inclusive minimum.
        min: f32,
        /// Inclusive maximum.
        max: f32,
    },
}
