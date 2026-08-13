use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The supported version of the serialized tectonic specification schema.
pub const TECTONIC_SPEC_SCHEMA_V1: u16 = 1;
/// The smallest supported initial number of tectonic plates.
pub const MIN_PLATE_COUNT: u16 = 2;
/// The largest supported initial number of tectonic plates.
pub const MAX_PLATE_COUNT: u16 = 64;
/// The smallest supported share of continental crust.
pub const MIN_CONTINENTAL_CRUST_FRACTION: f32 = 0.10;
/// The largest supported share of continental crust.
pub const MAX_CONTINENTAL_CRUST_FRACTION: f32 = 0.75;

/// The broad present-day strength of tectonic motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TectonicActivity {
    /// Favors relatively slow plate motion and subdued boundary relief.
    Quiet,
    /// Uses the balanced V1 plate-motion baseline.
    Moderate,
    /// Favors faster plate motion and stronger boundary relief.
    Active,
}

/// A versioned, deterministic description of the current tectonic state to generate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TectonicSpec {
    /// The schema version used to interpret this specification.
    pub schema_version: u16,
    /// The requested initial plate count before bounded evolution and rifting.
    pub plate_count: u16,
    /// The target share of spatial cells assigned continental crust.
    pub continental_crust_fraction: f32,
    /// The broad strength of generated plate motion.
    pub activity: TectonicActivity,
}

impl Default for TectonicSpec {
    fn default() -> Self {
        Self {
            schema_version: TECTONIC_SPEC_SCHEMA_V1,
            plate_count: 12,
            continental_crust_fraction: 0.38,
            activity: TectonicActivity::Moderate,
        }
    }
}

impl TectonicSpec {
    /// Validates the V1 tectonic numerical-safety and allocation budgets.
    pub fn validate(&self) -> Result<(), NaturalSpecError> {
        if self.schema_version != TECTONIC_SPEC_SCHEMA_V1 {
            return Err(NaturalSpecError::UnsupportedSchema {
                found: self.schema_version,
                supported: TECTONIC_SPEC_SCHEMA_V1,
            });
        }

        if !(MIN_PLATE_COUNT..=MAX_PLATE_COUNT).contains(&self.plate_count) {
            return Err(NaturalSpecError::PlateCountOutOfRange {
                found: self.plate_count,
                min: MIN_PLATE_COUNT,
                max: MAX_PLATE_COUNT,
            });
        }

        if !self.continental_crust_fraction.is_finite()
            || !(MIN_CONTINENTAL_CRUST_FRACTION..=MAX_CONTINENTAL_CRUST_FRACTION)
                .contains(&self.continental_crust_fraction)
        {
            return Err(NaturalSpecError::ContinentalCrustFractionOutOfRange {
                found: self.continental_crust_fraction,
                min: MIN_CONTINENTAL_CRUST_FRACTION,
                max: MAX_CONTINENTAL_CRUST_FRACTION,
            });
        }

        Ok(())
    }
}

/// Errors returned when a tectonic specification exceeds a V1 safety budget.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum NaturalSpecError {
    /// The specification uses a schema version that this engine does not support.
    #[error("unsupported tectonic schema version {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The schema version found in the specification.
        found: u16,
        /// The schema version supported by this engine.
        supported: u16,
    },
    /// The requested number of plates lies outside the V1 allocation budget.
    #[error("plate count {found} is outside {min}..={max}")]
    PlateCountOutOfRange {
        /// The plate count that failed validation.
        found: u16,
        /// The inclusive lower plate-count limit.
        min: u16,
        /// The inclusive upper plate-count limit.
        max: u16,
    },
    /// The requested share of continental crust is non-finite or outside the V1 range.
    #[error("continental crust fraction {found} is outside {min}..={max}")]
    ContinentalCrustFractionOutOfRange {
        /// The fraction that failed validation.
        found: f32,
        /// The inclusive lower fraction limit.
        min: f32,
        /// The inclusive upper fraction limit.
        max: f32,
    },
}
