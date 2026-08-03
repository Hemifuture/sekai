use thiserror::Error;

use crate::world::natural::{
    CirculationSpec, CirculationSpecError, ForcingError, PlanetForcing, CLIMATE_MONTH_COUNT,
};

use super::{CirculationOperatorError, CirculationOperators, CubedSphereGrid};

pub(crate) const LAPSE_RATE_C_PER_M: f32 = 0.0065;
const STANDARD_PRESSURE_PA: f64 = 101_325.0;
const MAX_SPECIFIC_HUMIDITY: f32 = 0.2;
const CONDENSATION_TIMESCALE_S: f64 = 21_600.0;
const AIR_COLUMN_MASS_KG_M2: f64 = STANDARD_PRESSURE_PA / 9.806_65;
const SECONDS_PER_DAY: f64 = 86_400.0;

/// Prognostic scalar state shared by both circulation strategies.
#[derive(Debug, Clone, PartialEq)]
pub struct ThermodynamicState {
    air_temperature_c: Vec<f32>,
    surface_temperature_c: Vec<f32>,
    specific_humidity: Vec<f32>,
}

impl ThermodynamicState {
    pub fn new(
        air_temperature_c: Vec<f32>,
        surface_temperature_c: Vec<f32>,
        specific_humidity: Vec<f32>,
    ) -> Result<Self, ThermodynamicError> {
        let state = Self {
            air_temperature_c,
            surface_temperature_c,
            specific_humidity,
        };
        state.validate(state.air_temperature_c.len())?;
        Ok(state)
    }

    pub fn from_forcing(
        grid: &CubedSphereGrid,
        forcing: &PlanetForcing,
        month: usize,
    ) -> Result<Self, ThermodynamicError> {
        validate_grid_forcing(grid, forcing)?;
        validate_month(month)?;
        let mut air_temperature_c = Vec::with_capacity(grid.cell_count());
        let mut surface_temperature_c = Vec::with_capacity(grid.cell_count());
        let mut specific_humidity = Vec::with_capacity(grid.cell_count());
        for cell in 0..grid.cell_count() {
            let positive_elevation = forcing.elevation_m()[cell].max(0.0);
            let land = forcing.land_fraction()[cell];
            air_temperature_c.push(
                forcing.equilibrium_air_temperature_c()[cell][month]
                    - LAPSE_RATE_C_PER_M * positive_elevation,
            );
            surface_temperature_c.push(
                forcing.equilibrium_surface_temperature_c()[cell][month]
                    - land * LAPSE_RATE_C_PER_M * positive_elevation,
            );
            specific_humidity.push(forcing.equilibrium_specific_humidity()[cell][month]);
        }
        Self::new(air_temperature_c, surface_temperature_c, specific_humidity)
    }

    pub fn validate(&self, expected: usize) -> Result<(), ThermodynamicError> {
        if expected == 0 {
            return Err(ThermodynamicError::EmptyState);
        }
        validate_scalar_field("air_temperature_c", &self.air_temperature_c, expected, None)?;
        validate_scalar_field(
            "surface_temperature_c",
            &self.surface_temperature_c,
            expected,
            None,
        )?;
        validate_scalar_field(
            "specific_humidity",
            &self.specific_humidity,
            expected,
            Some((0.0, MAX_SPECIFIC_HUMIDITY)),
        )?;
        Ok(())
    }

    pub fn cell_count(&self) -> usize {
        self.air_temperature_c.len()
    }

    pub fn air_temperature_c(&self) -> &[f32] {
        &self.air_temperature_c
    }

    pub fn surface_temperature_c(&self) -> &[f32] {
        &self.surface_temperature_c
    }

    pub fn specific_humidity(&self) -> &[f32] {
        &self.specific_humidity
    }
}

/// Precomputed edge masks derived once from immutable surface forcing.
#[derive(Debug, Clone, PartialEq)]
pub struct CirculationEdgePermeability {
    grid_fingerprint: [u8; 32],
    forcing_fingerprint: [u8; 32],
    atmosphere: Vec<f32>,
    ocean: Vec<f32>,
}

impl CirculationEdgePermeability {
    pub fn from_forcing(
        grid: &CubedSphereGrid,
        forcing: &PlanetForcing,
    ) -> Result<Self, ThermodynamicError> {
        validate_grid_forcing(grid, forcing)?;
        let atmosphere = vec![1.0; grid.edges().len()];
        let ocean = grid
            .edges()
            .iter()
            .map(|edge| {
                let [first, second] = edge.cells();
                let first_ocean = 1.0 - forcing.land_fraction()[*first as usize];
                let second_ocean = 1.0 - forcing.land_fraction()[*second as usize];
                first_ocean * second_ocean
            })
            .collect();
        Ok(Self {
            grid_fingerprint: *grid.fingerprint(),
            forcing_fingerprint: *forcing.fingerprint(),
            atmosphere,
            ocean,
        })
    }

    pub fn atmosphere(&self) -> &[f32] {
        &self.atmosphere
    }

    pub fn ocean(&self) -> &[f32] {
        &self.ocean
    }

    fn validate_against(
        &self,
        grid: &CubedSphereGrid,
        forcing: &PlanetForcing,
    ) -> Result<(), ThermodynamicError> {
        if self.grid_fingerprint != *grid.fingerprint()
            || self.forcing_fingerprint != *forcing.fingerprint()
            || self.atmosphere.len() != grid.edges().len()
            || self.ocean.len() != grid.edges().len()
        {
            return Err(ThermodynamicError::PermeabilityIdentityMismatch);
        }
        Ok(())
    }
}

/// Pure tendencies and precipitation diagnosed from one immutable state.
#[derive(Debug, Clone, PartialEq)]
pub struct ThermodynamicTendencies {
    air_temperature_c_per_s: Vec<f32>,
    surface_temperature_c_per_s: Vec<f32>,
    specific_humidity_per_s: Vec<f32>,
    precipitation_mm_day: Vec<f32>,
    relative_moisture_transport_error: f64,
}

impl ThermodynamicTendencies {
    pub fn air_temperature_c_per_s(&self) -> &[f32] {
        &self.air_temperature_c_per_s
    }

    pub fn surface_temperature_c_per_s(&self) -> &[f32] {
        &self.surface_temperature_c_per_s
    }

    pub fn specific_humidity_per_s(&self) -> &[f32] {
        &self.specific_humidity_per_s
    }

    pub fn precipitation_mm_day(&self) -> &[f32] {
        &self.precipitation_mm_day
    }

    pub const fn relative_moisture_transport_error(&self) -> f64 {
        self.relative_moisture_transport_error
    }
}

/// Computes shared radiative, surface, moisture, and conservative transport tendencies.
#[allow(clippy::too_many_arguments)]
pub fn thermodynamic_tendencies(
    operators: &CirculationOperators<'_>,
    forcing: &PlanetForcing,
    spec: &CirculationSpec,
    state: &ThermodynamicState,
    atmosphere_velocity: &[[f32; 3]],
    surface_velocity: &[[f32; 3]],
    permeability: &CirculationEdgePermeability,
    month: usize,
    transport_dt_seconds: f64,
) -> Result<ThermodynamicTendencies, ThermodynamicError> {
    spec.validate()?;
    let grid = operators.grid();
    validate_grid_forcing(grid, forcing)?;
    state.validate(grid.cell_count())?;
    permeability.validate_against(grid, forcing)?;
    validate_month(month)?;
    if !transport_dt_seconds.is_finite() || transport_dt_seconds <= 0.0 {
        return Err(ThermodynamicError::InvalidTimeStep {
            found: transport_dt_seconds,
        });
    }

    let air_transport = operators.advect_scalar_conservative(
        state.air_temperature_c(),
        atmosphere_velocity,
        permeability.atmosphere(),
        transport_dt_seconds,
    )?;
    let surface_transport = operators.advect_scalar_conservative(
        state.surface_temperature_c(),
        surface_velocity,
        permeability.ocean(),
        transport_dt_seconds,
    )?;
    let humidity_transport = operators.advect_scalar_conservative(
        state.specific_humidity(),
        atmosphere_velocity,
        permeability.atmosphere(),
        transport_dt_seconds,
    )?;

    let count = grid.cell_count();
    let mut air_temperature_c_per_s = Vec::with_capacity(count);
    let mut surface_temperature_c_per_s = Vec::with_capacity(count);
    let mut specific_humidity_per_s = Vec::with_capacity(count);
    let mut precipitation_mm_day = Vec::with_capacity(count);
    for cell in 0..count {
        let elevation = forcing.elevation_m()[cell].max(0.0);
        let land = forcing.land_fraction()[cell];
        let target_air =
            forcing.equilibrium_air_temperature_c()[cell][month] - LAPSE_RATE_C_PER_M * elevation;
        let target_surface = forcing.equilibrium_surface_temperature_c()[cell][month]
            - land * LAPSE_RATE_C_PER_M * elevation;
        let transported_air_tendency =
            f64::from(air_transport.values()[cell] - state.air_temperature_c()[cell])
                / transport_dt_seconds;
        let transported_surface_tendency =
            f64::from(surface_transport.values()[cell] - state.surface_temperature_c()[cell])
                / transport_dt_seconds;
        let air_relaxation = f64::from(spec.thermal_relaxation_s_inv)
            * f64::from(target_air - state.air_temperature_c()[cell]);
        let surface_rate_multiplier = f64::from(2.0 * land + 0.25 * (1.0 - land));
        let surface_relaxation = f64::from(spec.thermal_relaxation_s_inv)
            * surface_rate_multiplier
            * f64::from(target_surface - state.surface_temperature_c()[cell]);

        let transported_humidity_tendency =
            f64::from(humidity_transport.values()[cell] - state.specific_humidity()[cell])
                / transport_dt_seconds;
        let humidity = state.specific_humidity()[cell];
        let humidity_target = forcing.equilibrium_specific_humidity()[cell][month];
        let evaporation = f64::from(spec.layer_relaxation_s_inv)
            * f64::from((humidity_target - humidity).max(0.0));
        let saturation = saturation_specific_humidity(state.air_temperature_c()[cell])?;
        let condensation = f64::from((humidity - saturation).max(0.0)) / CONDENSATION_TIMESCALE_S;
        let precipitation = condensation * AIR_COLUMN_MASS_KG_M2 * SECONDS_PER_DAY;

        air_temperature_c_per_s.push((transported_air_tendency + air_relaxation) as f32);
        surface_temperature_c_per_s
            .push((transported_surface_tendency + surface_relaxation) as f32);
        specific_humidity_per_s
            .push((transported_humidity_tendency + evaporation - condensation) as f32);
        precipitation_mm_day.push(precipitation as f32);
    }

    let tendencies = ThermodynamicTendencies {
        air_temperature_c_per_s,
        surface_temperature_c_per_s,
        specific_humidity_per_s,
        precipitation_mm_day,
        relative_moisture_transport_error: humidity_transport.relative_mass_error(),
    };
    validate_tendencies(&tendencies, count)?;
    Ok(tendencies)
}

/// Advances only the prognostic thermodynamic state using precomputed tendencies.
pub fn advance_thermodynamics(
    state: &ThermodynamicState,
    tendencies: &ThermodynamicTendencies,
    dt_seconds: f64,
) -> Result<ThermodynamicState, ThermodynamicError> {
    state.validate(state.cell_count())?;
    validate_tendencies(tendencies, state.cell_count())?;
    if !dt_seconds.is_finite() || dt_seconds <= 0.0 {
        return Err(ThermodynamicError::InvalidTimeStep { found: dt_seconds });
    }
    let mut air = Vec::with_capacity(state.cell_count());
    let mut surface = Vec::with_capacity(state.cell_count());
    let mut humidity = Vec::with_capacity(state.cell_count());
    for cell in 0..state.cell_count() {
        air.push(
            (f64::from(state.air_temperature_c[cell])
                + dt_seconds * f64::from(tendencies.air_temperature_c_per_s[cell]))
                as f32,
        );
        surface.push(
            (f64::from(state.surface_temperature_c[cell])
                + dt_seconds * f64::from(tendencies.surface_temperature_c_per_s[cell]))
                as f32,
        );
        let next_humidity = f64::from(state.specific_humidity[cell])
            + dt_seconds * f64::from(tendencies.specific_humidity_per_s[cell]);
        humidity.push(next_humidity.clamp(0.0, f64::from(MAX_SPECIFIC_HUMIDITY)) as f32);
    }
    ThermodynamicState::new(air, surface, humidity)
}

/// Tetens saturation specific humidity at standard pressure, in kg/kg.
pub fn saturation_specific_humidity(temperature_c: f32) -> Result<f32, ThermodynamicError> {
    if !temperature_c.is_finite() {
        return Err(ThermodynamicError::NonFiniteTemperature);
    }
    let temperature = f64::from(temperature_c.clamp(-80.0, 60.0));
    let saturation_vapor_pressure_pa =
        610.94 * (17.625 * temperature / (temperature + 243.04)).exp();
    let humidity = 0.622 * saturation_vapor_pressure_pa
        / (STANDARD_PRESSURE_PA - 0.378 * saturation_vapor_pressure_pa);
    Ok(humidity.clamp(0.0, f64::from(MAX_SPECIFIC_HUMIDITY)) as f32)
}

fn validate_grid_forcing(
    grid: &CubedSphereGrid,
    forcing: &PlanetForcing,
) -> Result<(), ThermodynamicError> {
    forcing.validate()?;
    if forcing.cell_count() != grid.cell_count() || forcing.grid_fingerprint() != grid.fingerprint()
    {
        return Err(ThermodynamicError::GridForcingMismatch);
    }
    Ok(())
}

fn validate_month(month: usize) -> Result<(), ThermodynamicError> {
    if month >= CLIMATE_MONTH_COUNT {
        return Err(ThermodynamicError::InvalidMonth { found: month });
    }
    Ok(())
}

fn validate_tendencies(
    tendencies: &ThermodynamicTendencies,
    expected: usize,
) -> Result<(), ThermodynamicError> {
    validate_scalar_field(
        "air_temperature_c_per_s",
        &tendencies.air_temperature_c_per_s,
        expected,
        None,
    )?;
    validate_scalar_field(
        "surface_temperature_c_per_s",
        &tendencies.surface_temperature_c_per_s,
        expected,
        None,
    )?;
    validate_scalar_field(
        "specific_humidity_per_s",
        &tendencies.specific_humidity_per_s,
        expected,
        None,
    )?;
    validate_scalar_field(
        "precipitation_mm_day",
        &tendencies.precipitation_mm_day,
        expected,
        Some((0.0, f32::MAX)),
    )?;
    if !tendencies.relative_moisture_transport_error.is_finite()
        || tendencies.relative_moisture_transport_error < 0.0
    {
        return Err(ThermodynamicError::InvalidTransportDiagnostic);
    }
    Ok(())
}

fn validate_scalar_field(
    field: &'static str,
    values: &[f32],
    expected: usize,
    bounds: Option<(f32, f32)>,
) -> Result<(), ThermodynamicError> {
    if values.len() != expected {
        return Err(ThermodynamicError::FieldLengthMismatch {
            field,
            expected,
            found: values.len(),
        });
    }
    for (index, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            return Err(ThermodynamicError::NonFiniteFieldValue { field, index });
        }
        if let Some((min, max)) = bounds {
            if !(min..=max).contains(&value) {
                return Err(ThermodynamicError::FieldValueOutOfRange {
                    field,
                    index,
                    found: value,
                    min,
                    max,
                });
            }
        }
    }
    Ok(())
}

/// Errors shared by thermodynamic initialization, tendency, and integration.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ThermodynamicError {
    #[error(transparent)]
    Spec(#[from] CirculationSpecError),
    #[error(transparent)]
    Forcing(#[from] ForcingError),
    #[error(transparent)]
    Operator(#[from] CirculationOperatorError),
    #[error("thermodynamic state cannot be empty")]
    EmptyState,
    #[error("forcing does not belong to the supplied cubed-sphere grid")]
    GridForcingMismatch,
    #[error("edge permeability does not belong to the supplied grid and forcing")]
    PermeabilityIdentityMismatch,
    #[error("climatological month {found} is outside 0..{CLIMATE_MONTH_COUNT}")]
    InvalidMonth { found: usize },
    #[error("thermodynamic field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("thermodynamic field {field} is non-finite at cell {index}")]
    NonFiniteFieldValue { field: &'static str, index: usize },
    #[error("thermodynamic field {field} value {found} at cell {index} is outside {min}..={max}")]
    FieldValueOutOfRange {
        field: &'static str,
        index: usize,
        found: f32,
        min: f32,
        max: f32,
    },
    #[error("thermodynamic time step {found} must be finite and positive")]
    InvalidTimeStep { found: f64 },
    #[error("temperature must be finite for saturation humidity")]
    NonFiniteTemperature,
    #[error("moisture transport diagnostic is invalid")]
    InvalidTransportDiagnostic,
}
