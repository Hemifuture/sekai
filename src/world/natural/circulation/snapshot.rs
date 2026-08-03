use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::world::natural::CLIMATE_MONTH_COUNT;

use super::{
    bounded_vec::BoundedVec, CIRCULATION_SCHEMA_V1, MAX_CIRCULATION_CELL_COUNT,
    MAX_CIRCULATION_MONTHLY_VALUE_COUNT,
};

type BoundedMonthlyVec<T> = BoundedVec<T, MAX_CIRCULATION_MONTHLY_VALUE_COUNT, CLIMATE_MONTH_COUNT>;

/// Stable identities for the two solver strategies under comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CirculationSolverId {
    BalancedSteadyV1,
    TransientShallowWaterV1,
}

/// Solver work, convergence, conservation, and dense-state diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CirculationSolveStats {
    pub iterations_or_steps: u64,
    pub formation_years: u16,
    pub final_residual: f64,
    /// Maximum relative numerical closure error across atmosphere volume, ocean volume,
    /// paired-column moisture transport, and the column moisture written by a complete
    /// transient RK step. Physical source and sink terms such as evaporation, condensation,
    /// precipitation, relaxation, and explicit humidity-bound projection are excluded.
    pub relative_mass_error: f64,
    pub dense_state_bytes: u64,
}

/// Immutable, solver-neutral monthly state on one closed spherical grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CirculationSnapshot {
    schema_version: u16,
    cell_count: u32,
    spec_fingerprint: [u8; 32],
    grid_fingerprint: [u8; 32],
    forcing_fingerprint: [u8; 32],
    solver_id: CirculationSolverId,
    stats: CirculationSolveStats,
    monthly_wind_m_s: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    monthly_ocean_current_m_s: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    monthly_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_precipitation_mm_day: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_atmosphere_height_anomaly_m: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_sea_surface_height_anomaly_m: Vec<[f32; CLIMATE_MONTH_COUNT]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CirculationSnapshotWire {
    schema_version: u16,
    cell_count: u32,
    spec_fingerprint: [u8; 32],
    grid_fingerprint: [u8; 32],
    forcing_fingerprint: [u8; 32],
    solver_id: CirculationSolverId,
    stats: CirculationSolveStats,
    monthly_wind_m_s: BoundedMonthlyVec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    monthly_ocean_current_m_s: BoundedMonthlyVec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    monthly_air_temperature_c: BoundedMonthlyVec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_surface_temperature_c: BoundedMonthlyVec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_specific_humidity: BoundedMonthlyVec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_precipitation_mm_day: BoundedMonthlyVec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_atmosphere_height_anomaly_m: BoundedMonthlyVec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_sea_surface_height_anomaly_m: BoundedMonthlyVec<[f32; CLIMATE_MONTH_COUNT]>,
}

impl CirculationSnapshot {
    /// Constructs a shared snapshot only when every monthly field is aligned and finite.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        spec_fingerprint: [u8; 32],
        grid_fingerprint: [u8; 32],
        forcing_fingerprint: [u8; 32],
        solver_id: CirculationSolverId,
        stats: CirculationSolveStats,
        monthly_wind_m_s: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
        monthly_ocean_current_m_s: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
        monthly_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        monthly_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        monthly_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        monthly_precipitation_mm_day: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        monthly_atmosphere_height_anomaly_m: Vec<[f32; CLIMATE_MONTH_COUNT]>,
        monthly_sea_surface_height_anomaly_m: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    ) -> Result<Self, CirculationSnapshotError> {
        let cell_count = u32::try_from(monthly_wind_m_s.len()).map_err(|_| {
            CirculationSnapshotError::CellCountOutOfRange {
                found: monthly_wind_m_s.len(),
                min: 1,
                max: MAX_CIRCULATION_CELL_COUNT,
            }
        })?;
        let snapshot = Self {
            schema_version,
            cell_count,
            spec_fingerprint,
            grid_fingerprint,
            forcing_fingerprint,
            solver_id,
            stats,
            monthly_wind_m_s,
            monthly_ocean_current_m_s,
            monthly_air_temperature_c,
            monthly_surface_temperature_c,
            monthly_specific_humidity,
            monthly_precipitation_mm_day,
            monthly_atmosphere_height_anomaly_m,
            monthly_sea_surface_height_anomaly_m,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks all self-contained V1 snapshot invariants.
    pub fn validate(&self) -> Result<(), CirculationSnapshotError> {
        if self.schema_version != CIRCULATION_SCHEMA_V1 {
            return Err(CirculationSnapshotError::UnsupportedSchema {
                found: self.schema_version,
                supported: CIRCULATION_SCHEMA_V1,
            });
        }
        let expected = self.cell_count as usize;
        if expected == 0 || expected > MAX_CIRCULATION_CELL_COUNT {
            return Err(CirculationSnapshotError::CellCountOutOfRange {
                found: expected,
                min: 1,
                max: MAX_CIRCULATION_CELL_COUNT,
            });
        }
        for (field, found) in [
            ("monthly_wind_m_s", self.monthly_wind_m_s.len()),
            (
                "monthly_ocean_current_m_s",
                self.monthly_ocean_current_m_s.len(),
            ),
            (
                "monthly_air_temperature_c",
                self.monthly_air_temperature_c.len(),
            ),
            (
                "monthly_surface_temperature_c",
                self.monthly_surface_temperature_c.len(),
            ),
            (
                "monthly_specific_humidity",
                self.monthly_specific_humidity.len(),
            ),
            (
                "monthly_precipitation_mm_day",
                self.monthly_precipitation_mm_day.len(),
            ),
            (
                "monthly_atmosphere_height_anomaly_m",
                self.monthly_atmosphere_height_anomaly_m.len(),
            ),
            (
                "monthly_sea_surface_height_anomaly_m",
                self.monthly_sea_surface_height_anomaly_m.len(),
            ),
        ] {
            if found != expected {
                return Err(CirculationSnapshotError::FieldLengthMismatch {
                    field,
                    expected,
                    found,
                });
            }
        }
        validate_stats(self.stats)?;
        validate_vectors("monthly_wind_m_s", &self.monthly_wind_m_s)?;
        validate_vectors("monthly_ocean_current_m_s", &self.monthly_ocean_current_m_s)?;
        validate_scalars(
            "monthly_air_temperature_c",
            &self.monthly_air_temperature_c,
            None,
        )?;
        validate_scalars(
            "monthly_surface_temperature_c",
            &self.monthly_surface_temperature_c,
            None,
        )?;
        validate_scalars(
            "monthly_specific_humidity",
            &self.monthly_specific_humidity,
            Some(0.0),
        )?;
        validate_scalars(
            "monthly_precipitation_mm_day",
            &self.monthly_precipitation_mm_day,
            Some(0.0),
        )?;
        validate_scalars(
            "monthly_atmosphere_height_anomaly_m",
            &self.monthly_atmosphere_height_anomaly_m,
            None,
        )?;
        validate_scalars(
            "monthly_sea_surface_height_anomaly_m",
            &self.monthly_sea_surface_height_anomaly_m,
            None,
        )?;
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn cell_count(&self) -> u32 {
        self.cell_count
    }

    pub const fn spec_fingerprint(&self) -> &[u8; 32] {
        &self.spec_fingerprint
    }

    pub const fn grid_fingerprint(&self) -> &[u8; 32] {
        &self.grid_fingerprint
    }

    pub const fn forcing_fingerprint(&self) -> &[u8; 32] {
        &self.forcing_fingerprint
    }

    pub const fn solver_id(&self) -> CirculationSolverId {
        self.solver_id
    }

    pub const fn stats(&self) -> &CirculationSolveStats {
        &self.stats
    }

    pub fn monthly_wind_m_s(&self) -> &[[[f32; 3]; CLIMATE_MONTH_COUNT]] {
        &self.monthly_wind_m_s
    }

    pub fn monthly_ocean_current_m_s(&self) -> &[[[f32; 3]; CLIMATE_MONTH_COUNT]] {
        &self.monthly_ocean_current_m_s
    }

    pub fn monthly_air_temperature_c(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.monthly_air_temperature_c
    }

    pub fn monthly_surface_temperature_c(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.monthly_surface_temperature_c
    }

    pub fn monthly_specific_humidity(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.monthly_specific_humidity
    }

    pub fn monthly_precipitation_mm_day(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.monthly_precipitation_mm_day
    }

    pub fn monthly_atmosphere_height_anomaly_m(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.monthly_atmosphere_height_anomaly_m
    }

    pub fn monthly_sea_surface_height_anomaly_m(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        &self.monthly_sea_surface_height_anomaly_m
    }
}

impl<'de> Deserialize<'de> for CirculationSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CirculationSnapshotWire::deserialize(deserializer)?;
        let snapshot = Self {
            schema_version: wire.schema_version,
            cell_count: wire.cell_count,
            spec_fingerprint: wire.spec_fingerprint,
            grid_fingerprint: wire.grid_fingerprint,
            forcing_fingerprint: wire.forcing_fingerprint,
            solver_id: wire.solver_id,
            stats: wire.stats,
            monthly_wind_m_s: wire.monthly_wind_m_s.into_vec(),
            monthly_ocean_current_m_s: wire.monthly_ocean_current_m_s.into_vec(),
            monthly_air_temperature_c: wire.monthly_air_temperature_c.into_vec(),
            monthly_surface_temperature_c: wire.monthly_surface_temperature_c.into_vec(),
            monthly_specific_humidity: wire.monthly_specific_humidity.into_vec(),
            monthly_precipitation_mm_day: wire.monthly_precipitation_mm_day.into_vec(),
            monthly_atmosphere_height_anomaly_m: wire
                .monthly_atmosphere_height_anomaly_m
                .into_vec(),
            monthly_sea_surface_height_anomaly_m: wire
                .monthly_sea_surface_height_anomaly_m
                .into_vec(),
        };
        snapshot.validate().map_err(serde::de::Error::custom)?;
        Ok(snapshot)
    }
}

fn validate_stats(stats: CirculationSolveStats) -> Result<(), CirculationSnapshotError> {
    for (field, value) in [
        ("final_residual", stats.final_residual),
        ("relative_mass_error", stats.relative_mass_error),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(CirculationSnapshotError::InvalidSolveStatistic { field, value });
        }
    }
    if stats.iterations_or_steps == 0 {
        return Err(CirculationSnapshotError::ZeroSolveWork {
            field: "iterations_or_steps",
        });
    }
    if stats.dense_state_bytes == 0 {
        return Err(CirculationSnapshotError::ZeroSolveWork {
            field: "dense_state_bytes",
        });
    }
    Ok(())
}

fn validate_vectors(
    field: &'static str,
    values: &[[[f32; 3]; CLIMATE_MONTH_COUNT]],
) -> Result<(), CirculationSnapshotError> {
    for (cell, months) in values.iter().enumerate() {
        for (month, vector) in months.iter().enumerate() {
            for (component, value) in vector.iter().copied().enumerate() {
                if !value.is_finite() {
                    return Err(CirculationSnapshotError::NonFiniteVectorValue {
                        field,
                        cell,
                        month,
                        component,
                    });
                }
            }
        }
    }
    Ok(())
}

fn validate_scalars(
    field: &'static str,
    values: &[[f32; CLIMATE_MONTH_COUNT]],
    minimum: Option<f32>,
) -> Result<(), CirculationSnapshotError> {
    for (cell, months) in values.iter().enumerate() {
        for (month, value) in months.iter().copied().enumerate() {
            if !value.is_finite() {
                return Err(CirculationSnapshotError::NonFiniteScalarValue { field, cell, month });
            }
            if minimum.is_some_and(|bound| value < bound) {
                return Err(CirculationSnapshotError::ScalarBelowMinimum {
                    field,
                    cell,
                    month,
                    found: value,
                    minimum: minimum.expect("checked as some"),
                });
            }
        }
    }
    Ok(())
}

/// Errors returned by solver-neutral monthly snapshot validation.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CirculationSnapshotError {
    #[error("unsupported circulation snapshot schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("circulation snapshot cell count {found} is outside {min}..={max}")]
    CellCountOutOfRange {
        found: usize,
        min: usize,
        max: usize,
    },
    #[error("snapshot field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("solve statistic {field} has invalid value {value}")]
    InvalidSolveStatistic { field: &'static str, value: f64 },
    #[error("solve statistic {field} must be nonzero")]
    ZeroSolveWork { field: &'static str },
    #[error(
        "snapshot vector {field} has a non-finite component at cell {cell}, month {month}, component {component}"
    )]
    NonFiniteVectorValue {
        field: &'static str,
        cell: usize,
        month: usize,
        component: usize,
    },
    #[error("snapshot scalar {field} has a non-finite value at cell {cell}, month {month}")]
    NonFiniteScalarValue {
        field: &'static str,
        cell: usize,
        month: usize,
    },
    #[error(
        "snapshot scalar {field} value {found} at cell {cell}, month {month} is below {minimum}"
    )]
    ScalarBelowMinimum {
        field: &'static str,
        cell: usize,
        month: usize,
        found: f32,
        minimum: f32,
    },
}
