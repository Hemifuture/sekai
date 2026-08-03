use thiserror::Error;

use crate::world::natural::{
    CirculationSnapshot, CirculationSnapshotError, CirculationSolveStats, CirculationSolverId,
    CirculationSpec, CirculationSpecError, ForcingError, PlanetForcing, CIRCULATION_SCHEMA_V1,
    CLIMATE_MONTH_COUNT,
};

use super::{
    dynamics::{CirculationState, DynamicsError},
    linear::MatrixFreeSolveError,
    CirculationOperatorError, CubedSphereGrid, ThermodynamicError,
};

/// One strategy for producing the shared closed-sphere circulation snapshot.
pub trait CirculationSolver {
    fn id(&self) -> CirculationSolverId;

    fn solve(
        &self,
        grid: &CubedSphereGrid,
        forcing: &PlanetForcing,
        spec: &CirculationSpec,
    ) -> Result<CirculationSnapshot, CirculationSolveError>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SolverInputIdentity {
    pub(crate) spec_fingerprint: [u8; 32],
    pub(crate) grid_fingerprint: [u8; 32],
    pub(crate) forcing_fingerprint: [u8; 32],
}

pub(crate) fn validate_solver_inputs(
    grid: &CubedSphereGrid,
    forcing: &PlanetForcing,
    spec: &CirculationSpec,
) -> Result<SolverInputIdentity, CirculationSolveError> {
    let spec_fingerprint = spec.fingerprint()?;
    if grid.face_resolution() != spec.face_resolution {
        return Err(CirculationSolveError::GridResolutionMismatch {
            expected: spec.face_resolution,
            found: grid.face_resolution(),
        });
    }
    if grid.radius_m().to_bits() != spec.planet_radius_m.to_bits() {
        return Err(CirculationSolveError::GridRadiusMismatch {
            expected: spec.planet_radius_m,
            found: grid.radius_m(),
        });
    }
    forcing.validate()?;
    if forcing.cell_count() != grid.cell_count() {
        return Err(CirculationSolveError::ForcingCellCountMismatch {
            expected: grid.cell_count(),
            found: forcing.cell_count(),
        });
    }
    if forcing.grid_fingerprint() != grid.fingerprint() {
        return Err(CirculationSolveError::ForcingGridFingerprintMismatch);
    }
    Ok(SolverInputIdentity {
        spec_fingerprint,
        grid_fingerprint: *grid.fingerprint(),
        forcing_fingerprint: *forcing.fingerprint(),
    })
}

pub(crate) struct MonthlySnapshotBuilder {
    monthly_wind_m_s: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    monthly_ocean_current_m_s: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    monthly_air_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_surface_temperature_c: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_specific_humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_precipitation_mm_day: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_atmosphere_height_anomaly_m: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_sea_surface_height_anomaly_m: Vec<[f32; CLIMATE_MONTH_COUNT]>,
}

impl MonthlySnapshotBuilder {
    pub(crate) fn new(cell_count: usize) -> Self {
        Self {
            monthly_wind_m_s: vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; cell_count],
            monthly_ocean_current_m_s: vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; cell_count],
            monthly_air_temperature_c: vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count],
            monthly_surface_temperature_c: vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count],
            monthly_specific_humidity: vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count],
            monthly_precipitation_mm_day: vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count],
            monthly_atmosphere_height_anomaly_m: vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count],
            monthly_sea_surface_height_anomaly_m: vec![[0.0; CLIMATE_MONTH_COUNT]; cell_count],
        }
    }

    pub(crate) fn record(
        &mut self,
        month: usize,
        state: &CirculationState,
        precipitation_mm_day: &[f32],
    ) -> Result<(), CirculationSolveError> {
        if month >= CLIMATE_MONTH_COUNT {
            return Err(CirculationSolveError::InvalidOutputMonth { found: month });
        }
        let expected = self.monthly_wind_m_s.len();
        for (field, found) in [
            ("wind_m_s", state.wind_m_s.len()),
            ("ocean_current_m_s", state.ocean_current_m_s.len()),
            (
                "atmosphere_height_anomaly_m",
                state.atmosphere_height_anomaly_m.len(),
            ),
            (
                "sea_surface_height_anomaly_m",
                state.sea_surface_height_anomaly_m.len(),
            ),
            (
                "air_temperature_c",
                state.thermodynamics.air_temperature_c().len(),
            ),
            (
                "surface_temperature_c",
                state.thermodynamics.surface_temperature_c().len(),
            ),
            (
                "specific_humidity",
                state.thermodynamics.specific_humidity().len(),
            ),
            ("precipitation_mm_day", precipitation_mm_day.len()),
        ] {
            if found != expected {
                return Err(CirculationSolveError::OutputFieldLengthMismatch {
                    field,
                    expected,
                    found,
                });
            }
        }

        for (cell, precipitation) in precipitation_mm_day.iter().copied().enumerate() {
            self.monthly_wind_m_s[cell][month] = state.wind_m_s[cell];
            self.monthly_ocean_current_m_s[cell][month] = state.ocean_current_m_s[cell];
            self.monthly_air_temperature_c[cell][month] =
                state.thermodynamics.air_temperature_c()[cell];
            self.monthly_surface_temperature_c[cell][month] =
                state.thermodynamics.surface_temperature_c()[cell];
            self.monthly_specific_humidity[cell][month] =
                state.thermodynamics.specific_humidity()[cell];
            self.monthly_precipitation_mm_day[cell][month] = precipitation;
            self.monthly_atmosphere_height_anomaly_m[cell][month] =
                state.atmosphere_height_anomaly_m[cell];
            self.monthly_sea_surface_height_anomaly_m[cell][month] =
                state.sea_surface_height_anomaly_m[cell];
        }
        Ok(())
    }

    pub(crate) fn finish(
        self,
        identity: SolverInputIdentity,
        solver_id: CirculationSolverId,
        stats: CirculationSolveStats,
    ) -> Result<CirculationSnapshot, CirculationSolveError> {
        Ok(CirculationSnapshot::new(
            CIRCULATION_SCHEMA_V1,
            identity.spec_fingerprint,
            identity.grid_fingerprint,
            identity.forcing_fingerprint,
            solver_id,
            stats,
            self.monthly_wind_m_s,
            self.monthly_ocean_current_m_s,
            self.monthly_air_temperature_c,
            self.monthly_surface_temperature_c,
            self.monthly_specific_humidity,
            self.monthly_precipitation_mm_day,
            self.monthly_atmosphere_height_anomaly_m,
            self.monthly_sea_surface_height_anomaly_m,
        )?)
    }
}

/// Validation, convergence, and shared-kernel failures from either solver strategy.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CirculationSolveError {
    #[error(transparent)]
    Spec(#[from] CirculationSpecError),
    #[error(transparent)]
    Forcing(#[from] ForcingError),
    #[error(transparent)]
    Operator(#[from] CirculationOperatorError),
    #[error(transparent)]
    Thermodynamics(#[from] ThermodynamicError),
    #[error(transparent)]
    Snapshot(#[from] CirculationSnapshotError),
    #[error("grid face resolution {found} does not match specification value {expected}")]
    GridResolutionMismatch { expected: u16, found: u16 },
    #[error("grid radius {found} m does not match specification value {expected} m")]
    GridRadiusMismatch { expected: f64, found: f64 },
    #[error("forcing cell count {found} does not match grid cell count {expected}")]
    ForcingCellCountMismatch { expected: usize, found: usize },
    #[error("forcing was constructed for a different grid fingerprint")]
    ForcingGridFingerprintMismatch,
    #[error("output climatological month {found} is outside 0..{CLIMATE_MONTH_COUNT}")]
    InvalidOutputMonth { found: usize },
    #[error("solver output field {field} has length {found}; expected {expected}")]
    OutputFieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error(
        "solver {solver_id:?} did not converge for month {month} after {iterations} iterations: residual {residual} exceeds tolerance {tolerance}"
    )]
    NotConverged {
        solver_id: CirculationSolverId,
        month: usize,
        iterations: u64,
        residual: f64,
        tolerance: f64,
    },
    #[error("circulation state became non-finite")]
    NonFiniteState,
    #[error("stationary {layer} layer solve received invalid input")]
    LayerSolveInvalidInput { layer: &'static str },
    #[error("stationary {layer} layer solve overflowed")]
    LayerSolveNumericalOverflow { layer: &'static str },
    #[error("stationary {layer} layer solve broke down at iteration {iteration}")]
    LayerSolveBreakdown { layer: &'static str, iteration: u16 },
    #[error(
        "stationary {layer} layer solve did not converge after {iterations} iterations: residual {residual} exceeds tolerance {tolerance}"
    )]
    LayerSolveNotConverged {
        layer: &'static str,
        iterations: u16,
        residual: f64,
        tolerance: f64,
    },
    #[error("circulation dense-state allocation arithmetic overflowed")]
    AllocationOverflow,
}

impl From<DynamicsError> for CirculationSolveError {
    fn from(error: DynamicsError) -> Self {
        match error {
            DynamicsError::Operator(error) => Self::Operator(error),
            DynamicsError::Thermodynamics(error) => Self::Thermodynamics(error),
            DynamicsError::LayerLinearSolve { layer, reason } => match reason {
                MatrixFreeSolveError::InvalidInput => Self::LayerSolveInvalidInput { layer },
                MatrixFreeSolveError::NumericalOverflow => {
                    Self::LayerSolveNumericalOverflow { layer }
                }
                MatrixFreeSolveError::Breakdown { iteration } => {
                    Self::LayerSolveBreakdown { layer, iteration }
                }
                MatrixFreeSolveError::NotConverged {
                    iterations,
                    residual,
                    tolerance,
                } => Self::LayerSolveNotConverged {
                    layer,
                    iterations,
                    residual,
                    tolerance,
                },
            },
            DynamicsError::NonFiniteState => Self::NonFiniteState,
            DynamicsError::AllocationOverflow => Self::AllocationOverflow,
        }
    }
}
