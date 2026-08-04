use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::climate::{
    validate_length, validate_monthly_scalars, validate_monthly_vectors, validate_scalars,
    validate_summary_identity, validate_vectors,
};
use super::{
    ClimateValidationError, LandOceanKind, MonthlyScalarField, MonthlyVector3Field,
    SphericalReliefSnapshot, SphericalReliefValidationError, AIR_TEMPERATURE_MAX_C,
    AIR_TEMPERATURE_MIN_C, ANNUAL_PRECIPITATION_MAX_MM, CLIMATE_MONTH_COUNT,
    CLIMATE_SUMMARY_IDENTITY_TOLERANCE, MONTHLY_PRECIPITATION_MAX_MM,
    PRELIMINARY_CLIMATE_SCHEMA_V2, TEMPERATURE_SEASONALITY_MAX_C, WIND_COMPONENT_MAX_M_S,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceGeometryKind, SurfaceRef,
    SurfaceRefError,
};
use crate::world::{CellId, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT};

/// Maximum accepted error when re-deriving latitude from an authoritative unit radial.
pub const SPHERICAL_LATITUDE_IDENTITY_TOLERANCE_DEGREES: f32 = 1.0e-4;
/// Maximum accepted radial wind component after f32 publication, in meters per second.
pub const SPHERICAL_WIND_TANGENCY_TOLERANCE_M_S: f64 = 1.0e-4;

const MAX_SPHERICAL_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_SPHERICAL_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;
const MARITIME_IDENTITY_TOLERANCE: f32 = 1.0e-6;

/// Immutable preliminary monthly climate forcing bound to one authoritative spherical surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalPreliminaryClimateSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    latitude_degrees: Vec<f32>,
    maritime_influence: Vec<f32>,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_precipitation_mm: MonthlyScalarField,
    monthly_wind_m_s: MonthlyVector3Field,
    mean_annual_air_temperature_c: Vec<f32>,
    temperature_seasonality_c: Vec<f32>,
    annual_precipitation_mm: Vec<f32>,
    prevailing_wind_m_s: Vec<[f32; 3]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalPreliminaryClimateSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    #[serde(deserialize_with = "deserialize_spherical_climate_scalars")]
    latitude_degrees: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_climate_scalars")]
    maritime_influence: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_climate_monthly_scalars")]
    monthly_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    #[serde(deserialize_with = "deserialize_spherical_climate_monthly_scalars")]
    monthly_precipitation_mm: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    #[serde(deserialize_with = "deserialize_spherical_climate_monthly_vectors")]
    monthly_wind_m_s: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    #[serde(deserialize_with = "deserialize_spherical_climate_scalars")]
    mean_annual_air_temperature_c: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_climate_scalars")]
    temperature_seasonality_c: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_climate_scalars")]
    annual_precipitation_mm: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_climate_vectors")]
    prevailing_wind_m_s: Vec<[f32; 3]>,
}

fn deserialize_spherical_climate_scalars<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_climate_monthly_scalars<'de, D>(
    deserializer: D,
) -> Result<Vec<[f32; CLIMATE_MONTH_COUNT]>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_climate_monthly_vectors<'de, D>(
    deserializer: D,
) -> Result<Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_climate_vectors<'de, D>(deserializer: D) -> Result<Vec<[f32; 3]>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

impl SphericalPreliminaryClimateSnapshot {
    /// Constructs a snapshot only when every self-contained V2 invariant holds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        latitude_degrees: Vec<f32>,
        maritime_influence: Vec<f32>,
        monthly_air_temperature_c: MonthlyScalarField,
        monthly_precipitation_mm: MonthlyScalarField,
        monthly_wind_m_s: MonthlyVector3Field,
        mean_annual_air_temperature_c: Vec<f32>,
        temperature_seasonality_c: Vec<f32>,
        annual_precipitation_mm: Vec<f32>,
        prevailing_wind_m_s: Vec<[f32; 3]>,
    ) -> Result<Self, SphericalClimateValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
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

    /// Rechecks all invariants that do not require authoritative surface geometry.
    pub fn validate(&self) -> Result<(), SphericalClimateValidationError> {
        if self.schema_version != PRELIMINARY_CLIMATE_SCHEMA_V2 {
            return Err(SphericalClimateValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: PRELIMINARY_CLIMATE_SCHEMA_V2,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(SphericalClimateValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        validate_allocation_limit(
            "surface_ref.cell_count",
            self.surface_ref.cell_count() as usize,
            MAX_SPHERICAL_CELLS,
        )?;
        validate_allocation_limit(
            "surface_ref.edge_count",
            self.surface_ref.edge_count() as usize,
            MAX_SPHERICAL_EDGES,
        )?;
        self.validate_fields()?;
        Ok(())
    }

    fn validate_fields(&self) -> Result<(), SphericalClimateValidationError> {
        let cell_count = self.surface_ref.cell_count();
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
            validate_length(field, found, cell_count)?;
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

        for index in 0..cell_count as usize {
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
            let wind_sum = wind.iter().fold([0.0_f32; 3], |sum, value| {
                [sum[0] + value[0], sum[1] + value[1], sum[2] + value[2]]
            });
            let prevailing_wind = wind_sum.map(|component| component / CLIMATE_MONTH_COUNT as f32);

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
            for (component, &calculated) in prevailing_wind.iter().enumerate() {
                let stored = self.prevailing_wind_m_s[index][component];
                if (stored - calculated).abs() > CLIMATE_SUMMARY_IDENTITY_TOLERANCE {
                    return Err(SphericalClimateValidationError::InvalidClimateFields(
                        ClimateValidationError::VectorSummaryIdentityMismatch {
                            field: "prevailing_wind_m_s",
                            cell: CellId::from_raw(index as u32),
                            component,
                            stored,
                            calculated,
                        },
                    ));
                }
            }
            validate_wind_speed(index, None, self.prevailing_wind_m_s[index])?;
            for (month, &monthly_wind) in wind.iter().enumerate() {
                validate_wind_speed(index, Some(month), monthly_wind)?;
            }
        }
        Ok(())
    }

    /// Rechecks the exact surface and relief identities plus spherical semantic relations.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
    ) -> Result<(), SphericalClimateValidationError> {
        surface.validate()?;
        relief.validate_against_validated_surface(surface)?;
        self.validate_against_validated_surface(surface, relief)
    }

    pub(crate) fn validate_against_validated_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
    ) -> Result<(), SphericalClimateValidationError> {
        self.validate()?;
        let authoritative = SurfaceRef::from_validated_spherical(surface)?;
        if self.surface_ref != authoritative {
            return Err(SphericalClimateValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        if relief.surface_ref() != authoritative {
            return Err(SphericalClimateValidationError::ReliefSurfaceMismatch {
                relief: relief.surface_ref(),
                authoritative,
            });
        }

        let has_ocean = relief
            .land_ocean()
            .raw_values()
            .iter()
            .any(|&kind| kind == LandOceanKind::Ocean.raw());
        for (index, cell) in surface.cells().iter().enumerate() {
            let expected_latitude = cell.centroid.components()[2].asin().to_degrees() as f32;
            let stored_latitude = self.latitude_degrees[index];
            if (stored_latitude - expected_latitude).abs()
                > SPHERICAL_LATITUDE_IDENTITY_TOLERANCE_DEGREES
            {
                return Err(SphericalClimateValidationError::LatitudeMismatch {
                    cell: cell.id,
                    stored: stored_latitude,
                    expected: expected_latitude,
                });
            }
            let radial = cell.centroid.components();
            for (month, &wind) in self.monthly_wind_m_s.values()[index].iter().enumerate() {
                validate_tangency(cell.id, Some(month), wind, radial)?;
            }
            validate_tangency(cell.id, None, self.prevailing_wind_m_s[index], radial)?;

            let maritime = self.maritime_influence[index];
            let kind = relief
                .land_ocean_kind(cell.id)
                .expect("validated relief is surface aligned");
            if kind == LandOceanKind::Ocean && (maritime - 1.0).abs() > MARITIME_IDENTITY_TOLERANCE
            {
                return Err(SphericalClimateValidationError::OceanMaritimeMismatch {
                    cell: cell.id,
                    found: maritime,
                });
            }
            if !has_ocean && maritime.abs() > MARITIME_IDENTITY_TOLERANCE {
                return Err(SphericalClimateValidationError::AllLandMaritimeMismatch {
                    cell: cell.id,
                    found: maritime,
                });
            }
        }
        Ok(())
    }

    /// Returns the V2 schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact authoritative surface identity.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the dense cell allocation encoded by the surface identity.
    pub const fn cell_count(&self) -> u32 {
        self.surface_ref.cell_count()
    }

    /// Returns geographic latitude derived from the canonical planetary spin axis.
    pub fn latitude_degrees(&self) -> &[f32] {
        &self.latitude_degrees
    }

    /// Returns normalized ocean moderation.
    pub fn maritime_influence(&self) -> &[f32] {
        &self.maritime_influence
    }

    /// Returns all monthly air temperatures.
    pub const fn monthly_air_temperature_c(&self) -> &MonthlyScalarField {
        &self.monthly_air_temperature_c
    }

    /// Returns all monthly precipitation totals.
    pub const fn monthly_precipitation_mm(&self) -> &MonthlyScalarField {
        &self.monthly_precipitation_mm
    }

    /// Returns all monthly global tangent wind vectors.
    pub const fn monthly_wind_m_s(&self) -> &MonthlyVector3Field {
        &self.monthly_wind_m_s
    }

    /// Returns annual-mean air temperatures.
    pub fn mean_annual_air_temperature_c(&self) -> &[f32] {
        &self.mean_annual_air_temperature_c
    }

    /// Returns peak-to-trough monthly temperature ranges.
    pub fn temperature_seasonality_c(&self) -> &[f32] {
        &self.temperature_seasonality_c
    }

    /// Returns annual precipitation totals.
    pub fn annual_precipitation_mm(&self) -> &[f32] {
        &self.annual_precipitation_mm
    }

    /// Returns annual-mean global tangent wind vectors.
    pub fn prevailing_wind_m_s(&self) -> &[[f32; 3]] {
        &self.prevailing_wind_m_s
    }

    /// Returns air temperature for one cell and zero-based month.
    pub fn air_temperature_c(&self, cell: CellId, month: usize) -> Option<f32> {
        self.monthly_air_temperature_c
            .value(cell.raw() as usize, month)
    }

    /// Returns precipitation for one cell and zero-based month.
    pub fn precipitation_mm(&self, cell: CellId, month: usize) -> Option<f32> {
        self.monthly_precipitation_mm
            .value(cell.raw() as usize, month)
    }

    /// Returns the global tangent wind for one cell and zero-based month.
    pub fn wind_m_s(&self, cell: CellId, month: usize) -> Option<[f32; 3]> {
        self.monthly_wind_m_s.value(cell.raw() as usize, month)
    }
}

impl<'de> Deserialize<'de> for SphericalPreliminaryClimateSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalPreliminaryClimateSnapshotWire::deserialize(deserializer)?;
        let temperature = MonthlyScalarField::from_values(wire.monthly_air_temperature_c)
            .map_err(D::Error::custom)?;
        let precipitation = MonthlyScalarField::from_values(wire.monthly_precipitation_mm)
            .map_err(D::Error::custom)?;
        let wind =
            MonthlyVector3Field::from_values(wire.monthly_wind_m_s).map_err(D::Error::custom)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.latitude_degrees,
            wire.maritime_influence,
            temperature,
            precipitation,
            wind,
            wire.mean_annual_air_temperature_c,
            wire.temperature_seasonality_c,
            wire.annual_precipitation_mm,
            wire.prevailing_wind_m_s,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_wind_speed(
    index: usize,
    month: Option<usize>,
    wind: [f32; 3],
) -> Result<(), SphericalClimateValidationError> {
    let speed = wind
        .iter()
        .map(|&component| f64::from(component).powi(2))
        .sum::<f64>()
        .sqrt();
    if speed > f64::from(WIND_COMPONENT_MAX_M_S) {
        return Err(SphericalClimateValidationError::WindSpeedOutOfRange {
            cell: CellId::from_raw(index as u32),
            month,
            found: speed,
            max: WIND_COMPONENT_MAX_M_S,
        });
    }
    Ok(())
}

fn validate_tangency(
    cell: CellId,
    month: Option<usize>,
    wind: [f32; 3],
    radial: [f64; 3],
) -> Result<(), SphericalClimateValidationError> {
    let radial_component = f64::from(wind[0]) * radial[0]
        + f64::from(wind[1]) * radial[1]
        + f64::from(wind[2]) * radial[2];
    if radial_component.abs() > SPHERICAL_WIND_TANGENCY_TOLERANCE_M_S {
        return Err(SphericalClimateValidationError::WindNotTangent {
            cell,
            month,
            radial_component_m_s: radial_component,
        });
    }
    Ok(())
}

fn validate_allocation_limit(
    field: &'static str,
    found: usize,
    max: usize,
) -> Result<(), SphericalClimateValidationError> {
    if found > max {
        return Err(SphericalClimateValidationError::AllocationExceedsLimit { field, found, max });
    }
    Ok(())
}

/// Failures in the surface-bound spherical preliminary-climate contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalClimateValidationError {
    /// The snapshot uses an unsupported schema version.
    #[error("unsupported spherical preliminary-climate schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The stored surface identity is malformed.
    #[error("invalid surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    /// The stored identity does not describe spherical V1 geometry.
    #[error("spherical climate requires spherical_v1 geometry, found {found:?}")]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    /// A surface identity exceeds the spherical allocation budget.
    #[error("{field} allocation {found} exceeds spherical limit {max}")]
    AllocationExceedsLimit {
        field: &'static str,
        found: usize,
        max: usize,
    },
    /// A geometry-independent climate-field invariant failed.
    #[error("invalid climate fields: {0}")]
    InvalidClimateFields(#[from] ClimateValidationError),
    /// A published wind vector exceeds the supported speed envelope.
    #[error("wind speed at {cell:?}, month {month:?}, is {found} m/s; maximum is {max} m/s")]
    WindSpeedOutOfRange {
        cell: CellId,
        month: Option<usize>,
        found: f64,
        max: f32,
    },
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The exact stored surface identity differs from the authoritative surface.
    #[error("climate surface identity {snapshot:?} does not match {authoritative:?}")]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    /// The supplied relief belongs to a different surface.
    #[error("relief surface identity {relief:?} does not match {authoritative:?}")]
    ReliefSurfaceMismatch {
        relief: SurfaceRef,
        authoritative: SurfaceRef,
    },
    /// The relief upstream failed its authoritative spherical contract.
    #[error("invalid spherical relief upstream: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
    /// Stored latitude differs from latitude derived from the authoritative radial.
    #[error("latitude at {cell:?} is {stored} degrees; authoritative radial derives {expected}")]
    LatitudeMismatch {
        cell: CellId,
        stored: f32,
        expected: f32,
    },
    /// A monthly or annual global wind vector has a non-tangent radial component.
    #[error("wind at {cell:?}, month {month:?}, has radial component {radial_component_m_s} m/s")]
    WindNotTangent {
        cell: CellId,
        month: Option<usize>,
        radial_component_m_s: f64,
    },
    /// Ocean cells must retain exact maximum maritime influence.
    #[error("ocean cell {cell:?} has maritime influence {found}; expected 1")]
    OceanMaritimeMismatch { cell: CellId, found: f32 },
    /// A world without ocean has no physical maritime source.
    #[error("all-land cell {cell:?} has maritime influence {found}; expected 0")]
    AllLandMaritimeMismatch { cell: CellId, found: f32 },
}

#[cfg(test)]
mod tests {
    use serde::de::value::SeqDeserializer;
    use serde_json::json;

    use super::*;

    #[test]
    fn every_spherical_climate_sequence_category_rejects_max_plus_one_before_visiting_elements() {
        let scalars = SeqDeserializer::<_, serde_json::Error>::new(
            std::iter::repeat_with(|| json!(null)).take(MAX_SPHERICAL_CELLS + 1),
        );
        let monthly_scalars = SeqDeserializer::<_, serde_json::Error>::new(
            std::iter::repeat_with(|| json!(null)).take(MAX_SPHERICAL_CELLS + 1),
        );
        let monthly_vectors = SeqDeserializer::<_, serde_json::Error>::new(
            std::iter::repeat_with(|| json!(null)).take(MAX_SPHERICAL_CELLS + 1),
        );
        let vectors = SeqDeserializer::<_, serde_json::Error>::new(
            std::iter::repeat_with(|| json!(null)).take(MAX_SPHERICAL_CELLS + 1),
        );

        for error in [
            deserialize_spherical_climate_scalars(scalars).unwrap_err(),
            deserialize_spherical_climate_monthly_scalars(monthly_scalars).unwrap_err(),
            deserialize_spherical_climate_monthly_vectors(monthly_vectors).unwrap_err(),
            deserialize_spherical_climate_vectors(vectors).unwrap_err(),
        ] {
            assert!(
                error
                    .to_string()
                    .contains(&format!("at most {MAX_SPHERICAL_CELLS} elements")),
                "{error}"
            );
        }
    }
}
