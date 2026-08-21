use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// The supported serialized relief-authoring schema.
pub const RELIEF_SPEC_SCHEMA_V1: u16 = 1;
/// The smallest supported authored share of emergent land.
pub const MIN_TARGET_LAND_FRACTION: f32 = 0.05;
/// The largest supported authored share of emergent land.
pub const MAX_TARGET_LAND_FRACTION: f32 = 0.75;

/// A versioned author request for the final emergent land area.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReliefSpec {
    /// The schema version used to interpret this request.
    pub schema_version: u16,
    /// The requested share of authoritative spherical area above sea level.
    pub target_land_fraction: f32,
}

impl Default for ReliefSpec {
    fn default() -> Self {
        Self {
            schema_version: RELIEF_SPEC_SCHEMA_V1,
            target_land_fraction: 0.38,
        }
    }
}

impl ReliefSpec {
    /// Validates the schema and finite land-area range.
    pub fn validate(&self) -> Result<(), ReliefSpecError> {
        if self.schema_version != RELIEF_SPEC_SCHEMA_V1 {
            return Err(ReliefSpecError::UnsupportedSchema {
                found: self.schema_version,
                supported: RELIEF_SPEC_SCHEMA_V1,
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
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReliefSpecWire {
    schema_version: u16,
    target_land_fraction: f32,
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
}
