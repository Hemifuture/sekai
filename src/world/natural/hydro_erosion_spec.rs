use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// The supported version of the serialized hydro-erosion specification.
pub const HYDRO_EROSION_SPEC_SCHEMA_V1: u16 = 1;
/// The smallest supported river-publication threshold, in tenths of a cubic meter per second.
pub const MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S: u32 = 1;
/// The largest supported river-publication threshold, in tenths of a cubic meter per second.
pub const MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S: u32 = 1_000_000;
/// The largest supported formation-strength multiplier, in parts per thousand.
pub const MAX_EROSION_STRENGTH_PERMILLE: u16 = 2_000;
/// The smallest supported published-lake depth, in centimeters.
pub const MIN_LAKE_DEPTH_CM: u16 = 1;
/// The largest supported published-lake depth, in centimeters.
pub const MAX_LAKE_DEPTH_CM: u16 = 10_000;

/// A versioned fixed-point description of the current-slice hydro-erosion model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HydroErosionSpec {
    /// The schema version used to interpret this specification.
    pub schema_version: u16,
    /// Minimum mean discharge published as a river, in deci-m³/s.
    pub river_discharge_threshold_deci_m3_s: u32,
    /// Dimensionless bounded formation strength, in parts per thousand.
    pub erosion_strength_permille: u16,
    /// Minimum depression depth published as a lake, in centimeters.
    pub minimum_lake_depth_cm: u16,
}

impl Default for HydroErosionSpec {
    fn default() -> Self {
        Self {
            schema_version: HYDRO_EROSION_SPEC_SCHEMA_V1,
            river_discharge_threshold_deci_m3_s: 2_500,
            erosion_strength_permille: 1_000,
            minimum_lake_depth_cm: 100,
        }
    }
}

impl HydroErosionSpec {
    /// Validates the V1 model and numerical-safety limits.
    pub fn validate(&self) -> Result<(), HydroErosionSpecError> {
        if self.schema_version != HYDRO_EROSION_SPEC_SCHEMA_V1 {
            return Err(HydroErosionSpecError::UnsupportedSchema {
                found: self.schema_version,
                supported: HYDRO_EROSION_SPEC_SCHEMA_V1,
            });
        }
        if !(MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S..=MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S)
            .contains(&self.river_discharge_threshold_deci_m3_s)
        {
            return Err(HydroErosionSpecError::RiverDischargeThresholdOutOfRange {
                found: self.river_discharge_threshold_deci_m3_s,
                min: MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S,
                max: MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S,
            });
        }
        if self.erosion_strength_permille > MAX_EROSION_STRENGTH_PERMILLE {
            return Err(HydroErosionSpecError::ErosionStrengthOutOfRange {
                found: self.erosion_strength_permille,
                max: MAX_EROSION_STRENGTH_PERMILLE,
            });
        }
        if !(MIN_LAKE_DEPTH_CM..=MAX_LAKE_DEPTH_CM).contains(&self.minimum_lake_depth_cm) {
            return Err(HydroErosionSpecError::MinimumLakeDepthOutOfRange {
                found: self.minimum_lake_depth_cm,
                min: MIN_LAKE_DEPTH_CM,
                max: MAX_LAKE_DEPTH_CM,
            });
        }
        Ok(())
    }

    /// Returns the river-publication threshold in cubic meters per second.
    pub fn river_discharge_threshold_m3_s(&self) -> f32 {
        self.river_discharge_threshold_deci_m3_s as f32 / 10.0
    }

    /// Returns the dimensionless bounded formation-strength multiplier.
    pub fn erosion_strength(&self) -> f32 {
        f32::from(self.erosion_strength_permille) / 1_000.0
    }

    /// Returns the minimum published-lake depth in meters.
    pub fn minimum_lake_depth_m(&self) -> f32 {
        f32::from(self.minimum_lake_depth_cm) / 100.0
    }
}

#[derive(Deserialize)]
struct HydroErosionSpecWire {
    schema_version: u16,
    river_discharge_threshold_deci_m3_s: u32,
    erosion_strength_permille: u16,
    minimum_lake_depth_cm: u16,
}

impl<'de> Deserialize<'de> for HydroErosionSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = HydroErosionSpecWire::deserialize(deserializer)?;
        let spec = Self {
            schema_version: wire.schema_version,
            river_discharge_threshold_deci_m3_s: wire.river_discharge_threshold_deci_m3_s,
            erosion_strength_permille: wire.erosion_strength_permille,
            minimum_lake_depth_cm: wire.minimum_lake_depth_cm,
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}

/// Errors returned when a hydro-erosion specification violates V1 limits.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HydroErosionSpecError {
    /// The specification uses an unsupported serialized schema.
    #[error("unsupported hydro-erosion schema version {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
        /// The only supported schema version.
        supported: u16,
    },
    /// The river-publication threshold lies outside the supported range.
    #[error("river discharge threshold {found} deci-m3/s is outside {min}..={max}")]
    RiverDischargeThresholdOutOfRange {
        /// The rejected fixed-point threshold.
        found: u32,
        /// The inclusive minimum.
        min: u32,
        /// The inclusive maximum.
        max: u32,
    },
    /// The formation-strength multiplier exceeds the supported range.
    #[error("erosion strength {found} permille exceeds maximum {max}")]
    ErosionStrengthOutOfRange {
        /// The rejected fixed-point strength.
        found: u16,
        /// The inclusive maximum.
        max: u16,
    },
    /// The minimum published-lake depth lies outside the supported range.
    #[error("minimum lake depth {found} cm is outside {min}..={max}")]
    MinimumLakeDepthOutOfRange {
        /// The rejected fixed-point depth.
        found: u16,
        /// The inclusive minimum.
        min: u16,
        /// The inclusive maximum.
        max: u16,
    },
}
