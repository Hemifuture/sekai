use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::ReliefSnapshot;
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::CellId;

/// The supported serialized schema for preliminary monthly climate.
pub const PRELIMINARY_CLIMATE_SCHEMA_V1: u16 = 1;
/// The fixed number of climatological months in one current-slice year.
pub const CLIMATE_MONTH_COUNT: usize = 12;
/// The coldest supported monthly or annual-mean air temperature, in degrees Celsius.
pub const AIR_TEMPERATURE_MIN_C: f32 = -100.0;
/// The warmest supported monthly or annual-mean air temperature, in degrees Celsius.
pub const AIR_TEMPERATURE_MAX_C: f32 = 70.0;
/// The largest supported monthly precipitation total, in millimeters.
pub const MONTHLY_PRECIPITATION_MAX_MM: f32 = 4_000.0;
/// The largest supported annual precipitation total, in millimeters.
pub const ANNUAL_PRECIPITATION_MAX_MM: f32 = 20_000.0;
/// The largest supported peak-to-trough monthly temperature range, in degrees Celsius.
pub const TEMPERATURE_SEASONALITY_MAX_C: f32 = 120.0;
/// The largest magnitude of one prevailing-wind component, in meters per second.
pub const WIND_COMPONENT_MAX_M_S: f32 = 80.0;
/// The tolerance used when rechecking stored annual summaries.
pub const CLIMATE_SUMMARY_IDENTITY_TOLERANCE: f32 = 0.05;

/// A dense per-cell field containing one finite scalar value for each month.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MonthlyScalarField(Vec<[f32; CLIMATE_MONTH_COUNT]>);

impl MonthlyScalarField {
    /// Constructs a monthly field only when every value is finite.
    pub fn from_values(
        values: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    ) -> Result<Self, ClimateValidationError> {
        validate_monthly_scalars(
            "monthly_scalar_field",
            &values,
            f32::NEG_INFINITY,
            f32::INFINITY,
        )?;
        Ok(Self(values))
    }

    /// Returns all per-cell month arrays without copying.
    pub fn values(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.0
    }

    /// Returns one scalar for a dense cell index and zero-based month.
    pub fn value(&self, cell: usize, month: usize) -> Option<f32> {
        self.0
            .get(cell)
            .and_then(|months| months.get(month))
            .copied()
    }

    /// Returns the dense cell cardinality.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the field has no cells.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for MonthlyScalarField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<[f32; CLIMATE_MONTH_COUNT]>::deserialize(deserializer)?;
        Self::from_values(values).map_err(serde::de::Error::custom)
    }
}

/// A dense per-cell field containing one finite two-dimensional vector for each month.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MonthlyVectorField(Vec<[[f32; 2]; CLIMATE_MONTH_COUNT]>);

impl MonthlyVectorField {
    /// Constructs a monthly vector field only when every component is finite.
    pub fn from_values(
        values: Vec<[[f32; 2]; CLIMATE_MONTH_COUNT]>,
    ) -> Result<Self, ClimateValidationError> {
        validate_monthly_vectors(
            "monthly_vector_field",
            &values,
            f32::NEG_INFINITY,
            f32::INFINITY,
        )?;
        Ok(Self(values))
    }

    /// Returns all per-cell monthly vectors without copying.
    pub fn values(&self) -> &[[[f32; 2]; CLIMATE_MONTH_COUNT]] {
        &self.0
    }

    /// Returns one vector for a dense cell index and zero-based month.
    pub fn value(&self, cell: usize, month: usize) -> Option<[f32; 2]> {
        self.0
            .get(cell)
            .and_then(|months| months.get(month))
            .copied()
    }

    /// Returns the dense cell cardinality.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the field has no cells.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for MonthlyVectorField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<[[f32; 2]; CLIMATE_MONTH_COUNT]>::deserialize(deserializer)?;
        Self::from_values(values).map_err(serde::de::Error::custom)
    }
}

/// Immutable preliminary monthly climate forcing for the current world slice.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PreliminaryClimateSnapshot {
    schema_version: u16,
    cell_count: u32,
    latitude_degrees: Vec<f32>,
    maritime_influence: Vec<f32>,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_precipitation_mm: MonthlyScalarField,
    monthly_wind_m_s: MonthlyVectorField,
    mean_annual_air_temperature_c: Vec<f32>,
    temperature_seasonality_c: Vec<f32>,
    annual_precipitation_mm: Vec<f32>,
    prevailing_wind_m_s: Vec<[f32; 2]>,
}

#[derive(Deserialize)]
struct PreliminaryClimateSnapshotWire {
    schema_version: u16,
    cell_count: u32,
    latitude_degrees: Vec<f32>,
    maritime_influence: Vec<f32>,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_precipitation_mm: MonthlyScalarField,
    monthly_wind_m_s: MonthlyVectorField,
    mean_annual_air_temperature_c: Vec<f32>,
    temperature_seasonality_c: Vec<f32>,
    annual_precipitation_mm: Vec<f32>,
    prevailing_wind_m_s: Vec<[f32; 2]>,
}

impl PreliminaryClimateSnapshot {
    /// Constructs a snapshot only when all V1 monthly and summary invariants hold.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        cell_count: u32,
        latitude_degrees: Vec<f32>,
        maritime_influence: Vec<f32>,
        monthly_air_temperature_c: MonthlyScalarField,
        monthly_precipitation_mm: MonthlyScalarField,
        monthly_wind_m_s: MonthlyVectorField,
        mean_annual_air_temperature_c: Vec<f32>,
        temperature_seasonality_c: Vec<f32>,
        annual_precipitation_mm: Vec<f32>,
        prevailing_wind_m_s: Vec<[f32; 2]>,
    ) -> Result<Self, ClimateValidationError> {
        let snapshot = Self {
            schema_version,
            cell_count,
            latitude_degrees,
            maritime_influence,
            monthly_air_temperature_c,
            monthly_precipitation_mm,
            monthly_wind_m_s,
            mean_annual_air_temperature_c,
            temperature_seasonality_c,
            annual_precipitation_mm,
            prevailing_wind_m_s,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks every self-contained preliminary-climate invariant.
    pub fn validate(&self) -> Result<(), ClimateValidationError> {
        if self.schema_version != PRELIMINARY_CLIMATE_SCHEMA_V1 {
            return Err(ClimateValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: PRELIMINARY_CLIMATE_SCHEMA_V1,
            });
        }

        for (field, found) in [
            ("latitude_degrees", self.latitude_degrees.len()),
            ("maritime_influence", self.maritime_influence.len()),
            (
                "monthly_air_temperature_c",
                self.monthly_air_temperature_c.len(),
            ),
            (
                "monthly_precipitation_mm",
                self.monthly_precipitation_mm.len(),
            ),
            ("monthly_wind_m_s", self.monthly_wind_m_s.len()),
            (
                "mean_annual_air_temperature_c",
                self.mean_annual_air_temperature_c.len(),
            ),
            (
                "temperature_seasonality_c",
                self.temperature_seasonality_c.len(),
            ),
            (
                "annual_precipitation_mm",
                self.annual_precipitation_mm.len(),
            ),
            ("prevailing_wind_m_s", self.prevailing_wind_m_s.len()),
        ] {
            validate_length(field, found, self.cell_count)?;
        }

        validate_scalars("latitude_degrees", &self.latitude_degrees, -90.0, 90.0)?;
        validate_scalars("maritime_influence", &self.maritime_influence, 0.0, 1.0)?;
        validate_monthly_scalars(
            "monthly_air_temperature_c",
            self.monthly_air_temperature_c.values(),
            AIR_TEMPERATURE_MIN_C,
            AIR_TEMPERATURE_MAX_C,
        )?;
        validate_monthly_scalars(
            "monthly_precipitation_mm",
            self.monthly_precipitation_mm.values(),
            0.0,
            MONTHLY_PRECIPITATION_MAX_MM,
        )?;
        validate_monthly_vectors(
            "monthly_wind_m_s",
            self.monthly_wind_m_s.values(),
            -WIND_COMPONENT_MAX_M_S,
            WIND_COMPONENT_MAX_M_S,
        )?;
        validate_scalars(
            "mean_annual_air_temperature_c",
            &self.mean_annual_air_temperature_c,
            AIR_TEMPERATURE_MIN_C,
            AIR_TEMPERATURE_MAX_C,
        )?;
        validate_scalars(
            "temperature_seasonality_c",
            &self.temperature_seasonality_c,
            0.0,
            TEMPERATURE_SEASONALITY_MAX_C,
        )?;
        validate_scalars(
            "annual_precipitation_mm",
            &self.annual_precipitation_mm,
            0.0,
            ANNUAL_PRECIPITATION_MAX_MM,
        )?;
        validate_vectors(
            "prevailing_wind_m_s",
            &self.prevailing_wind_m_s,
            -WIND_COMPONENT_MAX_M_S,
            WIND_COMPONENT_MAX_M_S,
        )?;

        for index in 0..self.cell_count as usize {
            let temperatures = &self.monthly_air_temperature_c.values()[index];
            let precipitation = &self.monthly_precipitation_mm.values()[index];
            let wind = &self.monthly_wind_m_s.values()[index];
            let mean_temperature = temperatures.iter().sum::<f32>() / CLIMATE_MONTH_COUNT as f32;
            let minimum_temperature = temperatures.iter().copied().fold(f32::INFINITY, f32::min);
            let maximum_temperature = temperatures
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            let seasonality = maximum_temperature - minimum_temperature;
            let annual_precipitation = precipitation.iter().sum::<f32>();
            let wind_sum = wind.iter().fold([0.0_f32; 2], |sum, value| {
                [sum[0] + value[0], sum[1] + value[1]]
            });
            let prevailing_wind = [
                wind_sum[0] / CLIMATE_MONTH_COUNT as f32,
                wind_sum[1] / CLIMATE_MONTH_COUNT as f32,
            ];

            validate_summary_identity(
                "mean_annual_air_temperature_c",
                index,
                self.mean_annual_air_temperature_c[index],
                mean_temperature,
            )?;
            validate_summary_identity(
                "temperature_seasonality_c",
                index,
                self.temperature_seasonality_c[index],
                seasonality,
            )?;
            validate_summary_identity(
                "annual_precipitation_mm",
                index,
                self.annual_precipitation_mm[index],
                annual_precipitation,
            )?;
            for (component, calculated) in prevailing_wind.iter().enumerate() {
                if (self.prevailing_wind_m_s[index][component] - *calculated).abs()
                    > CLIMATE_SUMMARY_IDENTITY_TOLERANCE
                {
                    return Err(ClimateValidationError::VectorSummaryIdentityMismatch {
                        field: "prevailing_wind_m_s",
                        cell: CellId::from_raw(index as u32),
                        component,
                        stored: self.prevailing_wind_m_s[index][component],
                        calculated: *calculated,
                    });
                }
            }
        }
        Ok(())
    }

    /// Validates dense alignment against the exact spatial and relief inputs.
    pub fn validate_against(
        &self,
        spatial: &SpatialSnapshot,
        relief: &ReliefSnapshot,
    ) -> Result<(), ClimateValidationError> {
        self.validate()?;
        if spatial.cell_count() != self.cell_count as usize {
            return Err(ClimateValidationError::SpatialCellCountMismatch {
                climate: self.cell_count,
                spatial: spatial.cell_count(),
            });
        }
        if relief.cell_count() != self.cell_count {
            return Err(ClimateValidationError::ReliefCellCountMismatch {
                climate: self.cell_count,
                relief: relief.cell_count(),
            });
        }
        Ok(())
    }

    /// Returns the serialized snapshot schema.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact dense spatial-cell cardinality.
    pub const fn cell_count(&self) -> u32 {
        self.cell_count
    }

    /// Returns geographic latitude in degrees without copying.
    pub fn latitude_degrees(&self) -> &[f32] {
        &self.latitude_degrees
    }

    /// Returns normalized ocean moderation without copying.
    pub fn maritime_influence(&self) -> &[f32] {
        &self.maritime_influence
    }

    /// Returns all monthly air temperatures without copying.
    pub const fn monthly_air_temperature_c(&self) -> &MonthlyScalarField {
        &self.monthly_air_temperature_c
    }

    /// Returns all monthly precipitation totals without copying.
    pub const fn monthly_precipitation_mm(&self) -> &MonthlyScalarField {
        &self.monthly_precipitation_mm
    }

    /// Returns all monthly prevailing-wind vectors without copying.
    pub const fn monthly_wind_m_s(&self) -> &MonthlyVectorField {
        &self.monthly_wind_m_s
    }

    /// Returns annual-mean air temperatures without copying.
    pub fn mean_annual_air_temperature_c(&self) -> &[f32] {
        &self.mean_annual_air_temperature_c
    }

    /// Returns peak-to-trough monthly temperature ranges without copying.
    pub fn temperature_seasonality_c(&self) -> &[f32] {
        &self.temperature_seasonality_c
    }

    /// Returns annual precipitation totals without copying.
    pub fn annual_precipitation_mm(&self) -> &[f32] {
        &self.annual_precipitation_mm
    }

    /// Returns annual-mean prevailing-wind vectors without copying.
    pub fn prevailing_wind_m_s(&self) -> &[[f32; 2]] {
        &self.prevailing_wind_m_s
    }

    /// Returns air temperature for one stable cell and zero-based month.
    pub fn air_temperature_c(&self, cell: CellId, month: usize) -> Option<f32> {
        self.monthly_air_temperature_c
            .value(cell.raw() as usize, month)
    }

    /// Returns precipitation for one stable cell and zero-based month.
    pub fn precipitation_mm(&self, cell: CellId, month: usize) -> Option<f32> {
        self.monthly_precipitation_mm
            .value(cell.raw() as usize, month)
    }

    /// Returns prevailing wind for one stable cell and zero-based month.
    pub fn wind_m_s(&self, cell: CellId, month: usize) -> Option<[f32; 2]> {
        self.monthly_wind_m_s.value(cell.raw() as usize, month)
    }
}

impl<'de> Deserialize<'de> for PreliminaryClimateSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PreliminaryClimateSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.cell_count,
            wire.latitude_degrees,
            wire.maritime_influence,
            wire.monthly_air_temperature_c,
            wire.monthly_precipitation_mm,
            wire.monthly_wind_m_s,
            wire.mean_annual_air_temperature_c,
            wire.temperature_seasonality_c,
            wire.annual_precipitation_mm,
            wire.prevailing_wind_m_s,
        )
        .map_err(serde::de::Error::custom)
    }
}

fn validate_length(
    field: &'static str,
    found: usize,
    cell_count: u32,
) -> Result<(), ClimateValidationError> {
    let expected = cell_count as usize;
    if found != expected {
        return Err(ClimateValidationError::FieldLengthMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

fn validate_scalars(
    field: &'static str,
    values: &[f32],
    min: f32,
    max: f32,
) -> Result<(), ClimateValidationError> {
    for (index, &found) in values.iter().enumerate() {
        if !found.is_finite() {
            return Err(ClimateValidationError::NonFiniteScalarValue {
                field,
                cell: CellId::from_raw(index as u32),
                month: None,
                found,
            });
        }
        if found < min || found > max {
            return Err(ClimateValidationError::ScalarValueOutOfRange {
                field,
                cell: CellId::from_raw(index as u32),
                month: None,
                found,
                min,
                max,
            });
        }
    }
    Ok(())
}

fn validate_vectors(
    field: &'static str,
    values: &[[f32; 2]],
    min: f32,
    max: f32,
) -> Result<(), ClimateValidationError> {
    for (index, value) in values.iter().enumerate() {
        for (component, &found) in value.iter().enumerate() {
            if !found.is_finite() {
                return Err(ClimateValidationError::NonFiniteVectorValue {
                    field,
                    cell: CellId::from_raw(index as u32),
                    month: None,
                    component,
                    found,
                });
            }
            if found < min || found > max {
                return Err(ClimateValidationError::VectorValueOutOfRange {
                    field,
                    cell: CellId::from_raw(index as u32),
                    month: None,
                    component,
                    found,
                    min,
                    max,
                });
            }
        }
    }
    Ok(())
}

fn validate_monthly_scalars(
    field: &'static str,
    values: &[[f32; CLIMATE_MONTH_COUNT]],
    min: f32,
    max: f32,
) -> Result<(), ClimateValidationError> {
    for (index, months) in values.iter().enumerate() {
        for (month, &found) in months.iter().enumerate() {
            if !found.is_finite() {
                return Err(ClimateValidationError::NonFiniteScalarValue {
                    field,
                    cell: CellId::from_raw(index as u32),
                    month: Some(month),
                    found,
                });
            }
            if found < min || found > max {
                return Err(ClimateValidationError::ScalarValueOutOfRange {
                    field,
                    cell: CellId::from_raw(index as u32),
                    month: Some(month),
                    found,
                    min,
                    max,
                });
            }
        }
    }
    Ok(())
}

fn validate_monthly_vectors(
    field: &'static str,
    values: &[[[f32; 2]; CLIMATE_MONTH_COUNT]],
    min: f32,
    max: f32,
) -> Result<(), ClimateValidationError> {
    for (index, months) in values.iter().enumerate() {
        for (month, value) in months.iter().enumerate() {
            for (component, &found) in value.iter().enumerate() {
                if !found.is_finite() {
                    return Err(ClimateValidationError::NonFiniteVectorValue {
                        field,
                        cell: CellId::from_raw(index as u32),
                        month: Some(month),
                        component,
                        found,
                    });
                }
                if found < min || found > max {
                    return Err(ClimateValidationError::VectorValueOutOfRange {
                        field,
                        cell: CellId::from_raw(index as u32),
                        month: Some(month),
                        component,
                        found,
                        min,
                        max,
                    });
                }
            }
        }
    }
    Ok(())
}

fn validate_summary_identity(
    field: &'static str,
    index: usize,
    stored: f32,
    calculated: f32,
) -> Result<(), ClimateValidationError> {
    if (stored - calculated).abs() > CLIMATE_SUMMARY_IDENTITY_TOLERANCE {
        return Err(ClimateValidationError::SummaryIdentityMismatch {
            field,
            cell: CellId::from_raw(index as u32),
            stored,
            calculated,
        });
    }
    Ok(())
}

/// Errors returned when preliminary-climate fields violate the V1 contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateValidationError {
    /// The snapshot uses a schema version that this engine does not support.
    #[error("unsupported preliminary-climate schema {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
        /// The supported schema version.
        supported: u16,
    },
    /// A dense field length differs from the declared cell count.
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        /// The stable field name.
        field: &'static str,
        /// The required dense length.
        expected: usize,
        /// The actual dense length.
        found: usize,
    },
    /// A scalar field contains a non-finite value.
    #[error("field {field} has non-finite value {found} at {cell:?}, month {month:?}")]
    NonFiniteScalarValue {
        /// The stable field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The affected zero-based month for monthly fields.
        month: Option<usize>,
        /// The rejected value.
        found: f32,
    },
    /// A scalar field contains a finite value outside its semantic range.
    #[error("field {field} value {found} at {cell:?}, month {month:?}, is outside {min}..={max}")]
    ScalarValueOutOfRange {
        /// The stable field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The affected zero-based month for monthly fields.
        month: Option<usize>,
        /// The rejected value.
        found: f32,
        /// The inclusive minimum.
        min: f32,
        /// The inclusive maximum.
        max: f32,
    },
    /// A vector field contains a non-finite component.
    #[error(
        "field {field} has non-finite component {component} value {found} at {cell:?}, month {month:?}"
    )]
    NonFiniteVectorValue {
        /// The stable field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The affected zero-based month for monthly fields.
        month: Option<usize>,
        /// The vector component index.
        component: usize,
        /// The rejected value.
        found: f32,
    },
    /// A vector field contains a finite component outside its semantic range.
    #[error(
        "field {field} component {component} value {found} at {cell:?}, month {month:?}, is outside {min}..={max}"
    )]
    VectorValueOutOfRange {
        /// The stable field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The affected zero-based month for monthly fields.
        month: Option<usize>,
        /// The vector component index.
        component: usize,
        /// The rejected value.
        found: f32,
        /// The inclusive minimum.
        min: f32,
        /// The inclusive maximum.
        max: f32,
    },
    /// A stored scalar annual summary disagrees with its monthly source values.
    #[error("field {field} at {cell:?} stores {stored}; monthly values calculate {calculated}")]
    SummaryIdentityMismatch {
        /// The stable summary field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The stored annual summary.
        stored: f32,
        /// The summary recomputed from monthly values.
        calculated: f32,
    },
    /// A stored vector annual summary disagrees with its monthly source vectors.
    #[error(
        "field {field} component {component} at {cell:?} stores {stored}; monthly values calculate {calculated}"
    )]
    VectorSummaryIdentityMismatch {
        /// The stable summary field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The vector component index.
        component: usize,
        /// The stored annual component.
        stored: f32,
        /// The component recomputed from monthly values.
        calculated: f32,
    },
    /// Climate and spatial cell cardinalities differ.
    #[error("climate cell count {climate} does not match spatial count {spatial}")]
    SpatialCellCountMismatch {
        /// The climate snapshot count.
        climate: u32,
        /// The spatial topology count.
        spatial: usize,
    },
    /// Climate and relief cell cardinalities differ.
    #[error("climate cell count {climate} does not match relief count {relief}")]
    ReliefCellCountMismatch {
        /// The climate snapshot count.
        climate: u32,
        /// The relief snapshot count.
        relief: u32,
    },
}
