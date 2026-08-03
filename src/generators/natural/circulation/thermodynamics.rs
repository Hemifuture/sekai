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
const STEADY_LINEAR_ITERATIONS: u16 = 128;
const STEADY_LINEAR_TOLERANCE: f64 = 1.0e-6;

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

pub(crate) struct BalancedThermodynamics {
    pub(crate) state: ThermodynamicState,
    pub(crate) precipitation_mm_day: Vec<f32>,
    pub(crate) relative_moisture_transport_error: f64,
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
    thermodynamic_tendencies_validated(
        operators,
        forcing,
        spec,
        state,
        atmosphere_velocity,
        surface_velocity,
        permeability,
        month,
        transport_dt_seconds,
    )
}

/// Hot-path tendency evaluation after solver-boundary identity validation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn thermodynamic_tendencies_validated(
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
    let grid = operators.grid();
    debug_assert_eq!(state.cell_count(), grid.cell_count());
    debug_assert_eq!(atmosphere_velocity.len(), grid.cell_count());
    debug_assert_eq!(surface_velocity.len(), grid.cell_count());
    debug_assert_eq!(permeability.atmosphere().len(), grid.edges().len());
    debug_assert_eq!(permeability.ocean().len(), grid.edges().len());
    validate_month(month)?;
    if !transport_dt_seconds.is_finite() || transport_dt_seconds <= 0.0 {
        return Err(ThermodynamicError::InvalidTimeStep {
            found: transport_dt_seconds,
        });
    }

    let atmosphere_fluxes =
        operators.upwind_fluxes_validated(atmosphere_velocity, permeability.atmosphere());
    let surface_fluxes = operators.upwind_fluxes_validated(surface_velocity, permeability.ocean());
    let air_transport = operators.advect_scalar_upwind_tracer_from_fluxes_validated(
        state.air_temperature_c(),
        &atmosphere_fluxes,
        transport_dt_seconds,
    )?;
    let surface_transport = operators.advect_scalar_upwind_tracer_from_fluxes_validated(
        state.surface_temperature_c(),
        &surface_fluxes,
        transport_dt_seconds,
    )?;
    let humidity_transport = operators.advect_scalar_upwind_tracer_from_fluxes_validated(
        state.specific_humidity(),
        &atmosphere_fluxes,
        transport_dt_seconds,
    )?;

    let count = grid.cell_count();
    let mut air_temperature_c_per_s = Vec::with_capacity(count);
    let mut surface_temperature_c_per_s = Vec::with_capacity(count);
    let mut specific_humidity_per_s = Vec::with_capacity(count);
    let mut precipitation_mm_day = Vec::with_capacity(count);
    for cell in 0..count {
        let targets = thermodynamic_targets(forcing, cell, month);
        let transported_air_tendency =
            f64::from(air_transport.values()[cell] - state.air_temperature_c()[cell])
                / transport_dt_seconds;
        let transported_surface_tendency =
            f64::from(surface_transport.values()[cell] - state.surface_temperature_c()[cell])
                / transport_dt_seconds;
        let air_relaxation = f64::from(spec.thermal_relaxation_s_inv)
            * f64::from(targets.air_temperature_c - state.air_temperature_c()[cell]);
        let surface_rate_multiplier = f64::from(surface_relaxation_multiplier(targets.land));
        let surface_relaxation = f64::from(spec.thermal_relaxation_s_inv)
            * surface_rate_multiplier
            * f64::from(targets.surface_temperature_c - state.surface_temperature_c()[cell]);

        let transported_humidity_tendency =
            f64::from(humidity_transport.values()[cell] - state.specific_humidity()[cell])
                / transport_dt_seconds;
        let humidity = state.specific_humidity()[cell];
        let surface_moisture_exchange = f64::from(spec.layer_relaxation_s_inv)
            * f64::from(targets.specific_humidity - humidity);
        let saturation = saturation_specific_humidity(state.air_temperature_c()[cell])?;
        let condensation = f64::from((humidity - saturation).max(0.0)) / CONDENSATION_TIMESCALE_S;
        let precipitation = precipitation_from_condensation_mm_day(condensation);

        air_temperature_c_per_s.push((transported_air_tendency + air_relaxation) as f32);
        surface_temperature_c_per_s
            .push((transported_surface_tendency + surface_relaxation) as f32);
        specific_humidity_per_s.push(
            (transported_humidity_tendency + surface_moisture_exchange - condensation) as f32,
        );
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

/// Solves the same discrete transport-and-source equations at stationary balance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn balance_thermodynamics(
    operators: &CirculationOperators<'_>,
    forcing: &PlanetForcing,
    spec: &CirculationSpec,
    state: &ThermodynamicState,
    atmosphere_velocity: &[[f32; 3]],
    surface_velocity: &[[f32; 3]],
    permeability: &CirculationEdgePermeability,
    month: usize,
) -> Result<BalancedThermodynamics, ThermodynamicError> {
    spec.validate()?;
    let grid = operators.grid();
    validate_grid_forcing(grid, forcing)?;
    state.validate(grid.cell_count())?;
    permeability.validate_against(grid, forcing)?;
    validate_month(month)?;

    let count = grid.cell_count();
    let mut air_sink = Vec::with_capacity(count);
    let mut air_source = Vec::with_capacity(count);
    let mut surface_sink = Vec::with_capacity(count);
    let mut surface_source = Vec::with_capacity(count);
    for cell in 0..count {
        let targets = thermodynamic_targets(forcing, cell, month);
        let air_rate = spec.thermal_relaxation_s_inv;
        let surface_rate = air_rate * surface_relaxation_multiplier(targets.land);
        air_sink.push(air_rate);
        air_source.push(air_rate * targets.air_temperature_c);
        surface_sink.push(surface_rate);
        surface_source.push(surface_rate * targets.surface_temperature_c);
    }

    let air = operators
        .solve_steady_upwind_tracer_source(
            state.air_temperature_c(),
            atmosphere_velocity,
            permeability.atmosphere(),
            &air_sink,
            &air_source,
            STEADY_LINEAR_ITERATIONS,
            STEADY_LINEAR_TOLERANCE,
        )
        .map_err(|source| ThermodynamicError::SteadyLinearSolve {
            field: "air_temperature_c",
            source,
        })?;
    let surface = operators
        .solve_steady_upwind_tracer_source(
            state.surface_temperature_c(),
            surface_velocity,
            permeability.ocean(),
            &surface_sink,
            &surface_source,
            STEADY_LINEAR_ITERATIONS,
            STEADY_LINEAR_TOLERANCE,
        )
        .map_err(|source| ThermodynamicError::SteadyLinearSolve {
            field: "surface_temperature_c",
            source,
        })?;

    let mut humidity_sink = Vec::with_capacity(count);
    let mut humidity_source = Vec::with_capacity(count);
    for cell in 0..count {
        let humidity = state.specific_humidity()[cell];
        let target = thermodynamic_targets(forcing, cell, month).specific_humidity;
        let saturation = saturation_specific_humidity(air.values()[cell])?;
        let condensation_active = humidity >= saturation;
        let surface_exchange_rate = spec.layer_relaxation_s_inv;
        let condensation_rate = if condensation_active {
            (1.0 / CONDENSATION_TIMESCALE_S) as f32
        } else {
            0.0
        };
        humidity_sink.push(surface_exchange_rate + condensation_rate);
        humidity_source.push(surface_exchange_rate * target + condensation_rate * saturation);
    }
    let humidity = operators
        .solve_steady_upwind_tracer_source(
            state.specific_humidity(),
            atmosphere_velocity,
            permeability.atmosphere(),
            &humidity_sink,
            &humidity_source,
            STEADY_LINEAR_ITERATIONS,
            STEADY_LINEAR_TOLERANCE,
        )
        .map_err(|source| ThermodynamicError::SteadyLinearSolve {
            field: "specific_humidity",
            source,
        })?;
    let humidity_values = humidity
        .values()
        .iter()
        .map(|value| value.clamp(0.0, MAX_SPECIFIC_HUMIDITY))
        .collect::<Vec<_>>();
    let thermodynamic_state =
        ThermodynamicState::new(air.into_values(), surface.into_values(), humidity_values)?;
    let mut precipitation = Vec::with_capacity(count);
    for cell in 0..count {
        let saturation =
            saturation_specific_humidity(thermodynamic_state.air_temperature_c()[cell])?;
        let condensation =
            f64::from((thermodynamic_state.specific_humidity()[cell] - saturation).max(0.0))
                / CONDENSATION_TIMESCALE_S;
        precipitation.push(precipitation_from_condensation_mm_day(condensation) as f32);
    }
    let moisture_transport = operators.advect_scalar_upwind_tracer(
        thermodynamic_state.specific_humidity(),
        atmosphere_velocity,
        permeability.atmosphere(),
        1.0,
    )?;

    Ok(BalancedThermodynamics {
        state: thermodynamic_state,
        precipitation_mm_day: precipitation,
        relative_moisture_transport_error: moisture_transport.relative_mass_error(),
    })
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

pub(crate) fn advance_thermodynamics_weighted(
    state: &ThermodynamicState,
    weighted_tendencies: &[(&ThermodynamicTendencies, f64)],
    dt_seconds: f64,
) -> Result<ThermodynamicState, ThermodynamicError> {
    if weighted_tendencies.is_empty()
        || weighted_tendencies
            .iter()
            .any(|(_, weight)| !weight.is_finite())
    {
        return Err(ThermodynamicError::InvalidTendencyWeights);
    }
    debug_assert!(weighted_tendencies.iter().all(|(tendencies, _)| {
        tendencies.air_temperature_c_per_s.len() == state.cell_count()
            && tendencies.surface_temperature_c_per_s.len() == state.cell_count()
            && tendencies.specific_humidity_per_s.len() == state.cell_count()
    }));
    if !dt_seconds.is_finite() || dt_seconds <= 0.0 {
        return Err(ThermodynamicError::InvalidTimeStep { found: dt_seconds });
    }
    let mut air = Vec::with_capacity(state.cell_count());
    let mut surface = Vec::with_capacity(state.cell_count());
    let mut humidity = Vec::with_capacity(state.cell_count());
    for cell in 0..state.cell_count() {
        let air_tendency = weighted_tendencies
            .iter()
            .map(|(tendencies, weight)| {
                weight * f64::from(tendencies.air_temperature_c_per_s[cell])
            })
            .sum::<f64>();
        let surface_tendency = weighted_tendencies
            .iter()
            .map(|(tendencies, weight)| {
                weight * f64::from(tendencies.surface_temperature_c_per_s[cell])
            })
            .sum::<f64>();
        let humidity_tendency = weighted_tendencies
            .iter()
            .map(|(tendencies, weight)| {
                weight * f64::from(tendencies.specific_humidity_per_s[cell])
            })
            .sum::<f64>();
        air.push((f64::from(state.air_temperature_c[cell]) + dt_seconds * air_tendency) as f32);
        surface.push(
            (f64::from(state.surface_temperature_c[cell]) + dt_seconds * surface_tendency) as f32,
        );
        humidity.push(
            (f64::from(state.specific_humidity[cell]) + dt_seconds * humidity_tendency)
                .clamp(0.0, f64::from(MAX_SPECIFIC_HUMIDITY)) as f32,
        );
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

#[derive(Debug, Clone, Copy)]
struct CellThermodynamicTargets {
    air_temperature_c: f32,
    surface_temperature_c: f32,
    specific_humidity: f32,
    land: f32,
}

fn thermodynamic_targets(
    forcing: &PlanetForcing,
    cell: usize,
    month: usize,
) -> CellThermodynamicTargets {
    let elevation = forcing.elevation_m()[cell].max(0.0);
    let land = forcing.land_fraction()[cell];
    CellThermodynamicTargets {
        air_temperature_c: forcing.equilibrium_air_temperature_c()[cell][month]
            - LAPSE_RATE_C_PER_M * elevation,
        surface_temperature_c: forcing.equilibrium_surface_temperature_c()[cell][month]
            - land * LAPSE_RATE_C_PER_M * elevation,
        specific_humidity: forcing.equilibrium_specific_humidity()[cell][month],
        land,
    }
}

fn surface_relaxation_multiplier(land: f32) -> f32 {
    2.0 * land + 0.25 * (1.0 - land)
}

fn precipitation_from_condensation_mm_day(condensation_kg_kg_s: f64) -> f64 {
    condensation_kg_kg_s * AIR_COLUMN_MASS_KG_M2 * SECONDS_PER_DAY
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
    #[error("thermodynamic tendency weights must be nonempty and finite")]
    InvalidTendencyWeights,
    #[error("stationary solve for {field} failed: {source}")]
    SteadyLinearSolve {
        field: &'static str,
        source: CirculationOperatorError,
    },
}
