use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// The supported version of the serialized preliminary-climate specification.
pub const CLIMATE_SPEC_SCHEMA_V1: u16 = 1;
/// The southernmost or northernmost supported latitude, in hundredths of a degree.
pub const MIN_LATITUDE_CENTIDEG: i16 = -9_000;
/// The northernmost or southernmost supported latitude, in hundredths of a degree.
pub const MAX_LATITUDE_CENTIDEG: i16 = 9_000;
/// The smallest supported north-south latitude span, in hundredths of a degree.
pub const MIN_LATITUDE_SPAN_CENTIDEG: i16 = 1_000;
/// The largest supported axial tilt, in hundredths of a degree.
pub const MAX_AXIAL_TILT_CENTIDEG: u16 = 6_000;
/// The coldest supported global temperature offset, in tenths of a degree Celsius.
pub const MIN_TEMPERATURE_OFFSET_DECI_C: i16 = -300;
/// The warmest supported global temperature offset, in tenths of a degree Celsius.
pub const MAX_TEMPERATURE_OFFSET_DECI_C: i16 = 300;
/// The smallest supported atmospheric-moisture multiplier, in parts per thousand.
pub const MIN_MOISTURE_SCALE_PERMILLE: u16 = 250;
/// The largest supported atmospheric-moisture multiplier, in parts per thousand.
pub const MAX_MOISTURE_SCALE_PERMILLE: u16 = 2_500;

/// A versioned fixed-point description of the preliminary climate forcing to generate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClimateSpec {
    /// The schema version used to interpret this specification.
    pub schema_version: u16,
    /// Latitude mapped to the lower edge of planar world space, in centidegrees.
    pub south_latitude_centideg: i16,
    /// Latitude mapped to the upper edge of planar world space, in centidegrees.
    pub north_latitude_centideg: i16,
    /// Planetary axial tilt controlling seasonal forcing, in centidegrees.
    pub axial_tilt_centideg: u16,
    /// Global sea-level temperature offset, in tenths of a degree Celsius.
    pub temperature_offset_deci_c: i16,
    /// Initial/reference relative-humidity multiplier, in parts per thousand.
    pub moisture_scale_permille: u16,
}

impl Default for ClimateSpec {
    fn default() -> Self {
        Self {
            schema_version: CLIMATE_SPEC_SCHEMA_V1,
            south_latitude_centideg: -7_000,
            north_latitude_centideg: 7_000,
            axial_tilt_centideg: 2_340,
            temperature_offset_deci_c: 0,
            moisture_scale_permille: 1_000,
        }
    }
}

impl ClimateSpec {
    /// Validates the V1 climate-model and numerical-safety budget.
    pub fn validate(&self) -> Result<(), ClimateSpecError> {
        if self.schema_version != CLIMATE_SPEC_SCHEMA_V1 {
            return Err(ClimateSpecError::UnsupportedSchema {
                found: self.schema_version,
                supported: CLIMATE_SPEC_SCHEMA_V1,
            });
        }
        if !(MIN_LATITUDE_CENTIDEG..=MAX_LATITUDE_CENTIDEG).contains(&self.south_latitude_centideg)
        {
            return Err(ClimateSpecError::SouthLatitudeOutOfRange {
                found: self.south_latitude_centideg,
                min: MIN_LATITUDE_CENTIDEG,
                max: MAX_LATITUDE_CENTIDEG,
            });
        }
        if !(MIN_LATITUDE_CENTIDEG..=MAX_LATITUDE_CENTIDEG).contains(&self.north_latitude_centideg)
        {
            return Err(ClimateSpecError::NorthLatitudeOutOfRange {
                found: self.north_latitude_centideg,
                min: MIN_LATITUDE_CENTIDEG,
                max: MAX_LATITUDE_CENTIDEG,
            });
        }
        let span =
            i32::from(self.north_latitude_centideg) - i32::from(self.south_latitude_centideg);
        if span < i32::from(MIN_LATITUDE_SPAN_CENTIDEG) {
            return Err(ClimateSpecError::LatitudeSpanOutOfRange {
                south: self.south_latitude_centideg,
                north: self.north_latitude_centideg,
                minimum_span: MIN_LATITUDE_SPAN_CENTIDEG,
            });
        }
        if self.axial_tilt_centideg > MAX_AXIAL_TILT_CENTIDEG {
            return Err(ClimateSpecError::AxialTiltOutOfRange {
                found: self.axial_tilt_centideg,
                max: MAX_AXIAL_TILT_CENTIDEG,
            });
        }
        if !(MIN_TEMPERATURE_OFFSET_DECI_C..=MAX_TEMPERATURE_OFFSET_DECI_C)
            .contains(&self.temperature_offset_deci_c)
        {
            return Err(ClimateSpecError::TemperatureOffsetOutOfRange {
                found: self.temperature_offset_deci_c,
                min: MIN_TEMPERATURE_OFFSET_DECI_C,
                max: MAX_TEMPERATURE_OFFSET_DECI_C,
            });
        }
        if !(MIN_MOISTURE_SCALE_PERMILLE..=MAX_MOISTURE_SCALE_PERMILLE)
            .contains(&self.moisture_scale_permille)
        {
            return Err(ClimateSpecError::MoistureScaleOutOfRange {
                found: self.moisture_scale_permille,
                min: MIN_MOISTURE_SCALE_PERMILLE,
                max: MAX_MOISTURE_SCALE_PERMILLE,
            });
        }
        Ok(())
    }

    /// Returns the latitude mapped to the lower world boundary, in degrees.
    pub fn south_latitude_degrees(&self) -> f32 {
        f32::from(self.south_latitude_centideg) / 100.0
    }

    /// Returns the latitude mapped to the upper world boundary, in degrees.
    pub fn north_latitude_degrees(&self) -> f32 {
        f32::from(self.north_latitude_centideg) / 100.0
    }

    /// Returns the planetary axial tilt, in degrees.
    pub fn axial_tilt_degrees(&self) -> f32 {
        f32::from(self.axial_tilt_centideg) / 100.0
    }

    /// Returns the global temperature offset, in degrees Celsius.
    pub fn temperature_offset_c(&self) -> f32 {
        f32::from(self.temperature_offset_deci_c) / 10.0
    }

    /// Returns the dimensionless atmospheric moisture multiplier.
    pub fn moisture_scale(&self) -> f32 {
        f32::from(self.moisture_scale_permille) / 1_000.0
    }
}

#[derive(Deserialize)]
struct ClimateSpecWire {
    schema_version: u16,
    south_latitude_centideg: i16,
    north_latitude_centideg: i16,
    axial_tilt_centideg: u16,
    temperature_offset_deci_c: i16,
    moisture_scale_permille: u16,
}

impl<'de> Deserialize<'de> for ClimateSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateSpecWire::deserialize(deserializer)?;
        let spec = Self {
            schema_version: wire.schema_version,
            south_latitude_centideg: wire.south_latitude_centideg,
            north_latitude_centideg: wire.north_latitude_centideg,
            axial_tilt_centideg: wire.axial_tilt_centideg,
            temperature_offset_deci_c: wire.temperature_offset_deci_c,
            moisture_scale_permille: wire.moisture_scale_permille,
        };
        spec.validate().map_err(serde::de::Error::custom)?;
        Ok(spec)
    }
}

/// Errors returned when a preliminary-climate specification violates V1 limits.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClimateSpecError {
    /// The specification uses an unsupported serialized schema.
    #[error("unsupported climate schema version {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
        /// The only supported schema version.
        supported: u16,
    },
    /// The lower map boundary lies outside supported geographic latitude.
    #[error("south latitude {found} centidegrees is outside {min}..={max}")]
    SouthLatitudeOutOfRange {
        /// The rejected fixed-point latitude.
        found: i16,
        /// The inclusive minimum.
        min: i16,
        /// The inclusive maximum.
        max: i16,
    },
    /// The upper map boundary lies outside supported geographic latitude.
    #[error("north latitude {found} centidegrees is outside {min}..={max}")]
    NorthLatitudeOutOfRange {
        /// The rejected fixed-point latitude.
        found: i16,
        /// The inclusive minimum.
        min: i16,
        /// The inclusive maximum.
        max: i16,
    },
    /// The map does not cover a strictly northward, sufficiently wide latitude band.
    #[error(
        "latitude band {south}..{north} centidegrees is smaller than {minimum_span} centidegrees"
    )]
    LatitudeSpanOutOfRange {
        /// The rejected southern boundary.
        south: i16,
        /// The rejected northern boundary.
        north: i16,
        /// The minimum supported span.
        minimum_span: i16,
    },
    /// Axial tilt exceeds the model's supported range.
    #[error("axial tilt {found} centidegrees exceeds maximum {max}")]
    AxialTiltOutOfRange {
        /// The rejected tilt.
        found: u16,
        /// The inclusive maximum.
        max: u16,
    },
    /// The global temperature offset lies outside the supported range.
    #[error("temperature offset {found} deci-C is outside {min}..={max}")]
    TemperatureOffsetOutOfRange {
        /// The rejected fixed-point offset.
        found: i16,
        /// The inclusive minimum.
        min: i16,
        /// The inclusive maximum.
        max: i16,
    },
    /// The atmospheric moisture multiplier lies outside the supported range.
    #[error("moisture scale {found} permille is outside {min}..={max}")]
    MoistureScaleOutOfRange {
        /// The rejected fixed-point multiplier.
        found: u16,
        /// The inclusive minimum.
        min: u16,
        /// The inclusive maximum.
        max: u16,
    },
}
