use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// The supported version of the serialized geologic specification schema.
pub const GEOLOGIC_SPEC_SCHEMA_V1: u16 = 1;
/// The largest supported number of mantle hotspots.
pub const MAX_HOTSPOT_COUNT: u16 = 16;

/// The broad present-day strength of mantle activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MantleActivity {
    /// Favors subdued heat-flow anomalies and compact volcanic influence.
    Quiet,
    /// Uses the balanced V1 mantle-activity baseline.
    Moderate,
    /// Favors stronger heat-flow anomalies and wider volcanic influence.
    Active,
}

/// A versioned description of current-slice mantle and surface-geology forcing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GeologicSpec {
    /// The schema version used to interpret this specification.
    pub schema_version: u16,
    /// The requested number of present-day mantle hotspots.
    pub hotspot_count: u16,
    /// The broad strength of present-day mantle activity.
    pub mantle_activity: MantleActivity,
}

impl Default for GeologicSpec {
    fn default() -> Self {
        Self {
            schema_version: GEOLOGIC_SPEC_SCHEMA_V1,
            hotspot_count: 4,
            mantle_activity: MantleActivity::Moderate,
        }
    }
}

impl GeologicSpec {
    /// Validates the V1 geologic allocation budget.
    pub fn validate(&self) -> Result<(), GeologicSpecError> {
        if self.schema_version != GEOLOGIC_SPEC_SCHEMA_V1 {
            return Err(GeologicSpecError::UnsupportedSchema {
                found: self.schema_version,
                supported: GEOLOGIC_SPEC_SCHEMA_V1,
            });
        }

        if self.hotspot_count > MAX_HOTSPOT_COUNT {
            return Err(GeologicSpecError::HotspotCountOutOfRange {
                found: self.hotspot_count,
                max: MAX_HOTSPOT_COUNT,
            });
        }

        Ok(())
    }
}

#[derive(Deserialize)]
struct GeologicSpecWire {
    schema_version: u16,
    hotspot_count: u16,
    mantle_activity: MantleActivity,
}

impl<'de> Deserialize<'de> for GeologicSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GeologicSpecWire::deserialize(deserializer)?;
        let spec = Self {
            schema_version: wire.schema_version,
            hotspot_count: wire.hotspot_count,
            mantle_activity: wire.mantle_activity,
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}

/// Errors returned when a geologic specification exceeds a V1 contract.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GeologicSpecError {
    /// The specification uses a schema version that this engine does not support.
    #[error("unsupported geologic schema version {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The schema version found in the specification.
        found: u16,
        /// The schema version supported by this engine.
        supported: u16,
    },
    /// The requested number of hotspots exceeds the V1 allocation budget.
    #[error("hotspot count {found} exceeds the maximum {max}")]
    HotspotCountOutOfRange {
        /// The hotspot count that failed validation.
        found: u16,
        /// The inclusive upper hotspot-count limit.
        max: u16,
    },
}
