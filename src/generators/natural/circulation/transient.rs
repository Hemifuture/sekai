use crate::world::natural::{
    CirculationSnapshot, CirculationSolveStats, CirculationSolverId, CirculationSpec,
    PlanetForcing, CLIMATE_MONTH_COUNT,
};

use super::{
    dynamics::{
        dense_state_bytes, initial_state, inverse_barometer_height, remove_layer_mean,
        state_residual, thermal_height_target, CirculationState, DynamicsError,
        WIND_STRESS_RATE_S_INV,
    },
    solver::{validate_solver_inputs, MonthlySnapshotBuilder, SolverInputIdentity},
    thermodynamics::{advance_thermodynamics_weighted, thermodynamic_tendencies_validated},
    CirculationEdgePermeability, CirculationOperators, CirculationSolveError, CirculationSolver,
    CubedSphereGrid, ThermodynamicState, ThermodynamicTendencies,
};

const SECONDS_PER_DAY: u64 = 86_400;
const DAYS_PER_CLIMATOLOGICAL_MONTH: u64 = 30;
const SECONDS_PER_CLIMATOLOGICAL_MONTH: u64 = SECONDS_PER_DAY * DAYS_PER_CLIMATOLOGICAL_MONTH;
const TRANSIENT_DENSE_STATE_MULTIPLIER: u64 = 6;
const ROUND_OFF_TENDENCY_FLOOR: f32 = 1.0e-30;

/// Time-dependent reduced shallow-water solver using deterministic classic RK3 steps.
#[derive(Debug, Clone, Default)]
pub struct TransientShallowWaterSolver {
    warm_start: Option<CirculationSnapshot>,
}

impl TransientShallowWaterSolver {
    pub const fn cold_start() -> Self {
        Self { warm_start: None }
    }

    pub fn warm_start(snapshot: &CirculationSnapshot) -> Self {
        Self {
            warm_start: Some(snapshot.clone()),
        }
    }

    pub fn time_step_seconds(
        &self,
        grid: &CubedSphereGrid,
        spec: &CirculationSpec,
    ) -> Result<u64, CirculationSolveError> {
        validate_grid_spec(grid, spec)?;
        let maximum_wave_speed = maximum_wave_speed(spec);
        let raw_seconds = (f64::from(spec.cfl_limit) * grid.minimum_center_distance_m()
            / maximum_wave_speed)
            .floor();
        if !raw_seconds.is_finite() || raw_seconds < 60.0 {
            return Err(CirculationSolveError::InvalidTimeStep {
                found_seconds: raw_seconds.max(0.0) as u64,
            });
        }
        let quantized = (raw_seconds as u64 / 60) * 60;
        if quantized < 60 {
            return Err(CirculationSolveError::InvalidTimeStep {
                found_seconds: quantized,
            });
        }
        Ok(quantized)
    }

    pub fn cfl(
        &self,
        grid: &CubedSphereGrid,
        spec: &CirculationSpec,
        dt_seconds: u64,
    ) -> Result<f64, CirculationSolveError> {
        validate_grid_spec(grid, spec)?;
        if dt_seconds == 0 {
            return Err(CirculationSolveError::InvalidTimeStep {
                found_seconds: dt_seconds,
            });
        }
        Ok(dt_seconds as f64 * maximum_wave_speed(spec) / grid.minimum_center_distance_m())
    }

    fn validate_warm_start(
        &self,
        identity: SolverInputIdentity,
        grid: &CubedSphereGrid,
    ) -> Result<(), CirculationSolveError> {
        let Some(snapshot) = &self.warm_start else {
            return Ok(());
        };
        snapshot.validate()?;
        if snapshot.cell_count() as usize != grid.cell_count() {
            return Err(CirculationSolveError::WarmStartIdentityMismatch {
                field: "cell_count",
            });
        }
        for (field, matches) in [
            (
                "spec_fingerprint",
                snapshot.spec_fingerprint() == &identity.spec_fingerprint,
            ),
            (
                "grid_fingerprint",
                snapshot.grid_fingerprint() == &identity.grid_fingerprint,
            ),
            (
                "forcing_fingerprint",
                snapshot.forcing_fingerprint() == &identity.forcing_fingerprint,
            ),
        ] {
            if !matches {
                return Err(CirculationSolveError::WarmStartIdentityMismatch { field });
            }
        }
        Ok(())
    }
}

impl CirculationSolver for TransientShallowWaterSolver {
    fn id(&self) -> CirculationSolverId {
        CirculationSolverId::TransientShallowWaterV1
    }

    fn solve(
        &self,
        grid: &CubedSphereGrid,
        forcing: &PlanetForcing,
        spec: &CirculationSpec,
    ) -> Result<CirculationSnapshot, CirculationSolveError> {
        let identity = validate_solver_inputs(grid, forcing, spec)?;
        self.validate_warm_start(identity, grid)?;
        let base_dt_seconds = self.time_step_seconds(grid, spec)?;
        let operators = CirculationOperators::new(grid);
        let permeability = CirculationEdgePermeability::from_forcing(grid, forcing)?;
        let mut state = match &self.warm_start {
            Some(snapshot) => state_from_snapshot(snapshot, CLIMATE_MONTH_COUNT - 1)?,
            None => initial_state(grid, forcing, spec, 0)?,
        };
        if let Some(snapshot) = exact_periodic_equilibrium_snapshot(
            &state,
            &operators,
            forcing,
            spec,
            &permeability,
            identity,
            self.id(),
            base_dt_seconds as f64,
        )? {
            return Ok(snapshot);
        }
        let mut previous_year = match &self.warm_start {
            Some(snapshot) => Some(states_from_snapshot(snapshot)?),
            None => None,
        };
        let mut total_steps = 0_u64;
        let mut maximum_mass_error = 0.0_f64;
        let mut last_residual = f64::INFINITY;

        for formation_year in 1..=spec.max_formation_years {
            let mut monthly_states = Vec::with_capacity(CLIMATE_MONTH_COUNT);
            let mut monthly_precipitation = Vec::with_capacity(CLIMATE_MONTH_COUNT);
            for month in 0..CLIMATE_MONTH_COUNT {
                let mut accumulator = MonthlyAccumulator::new(grid.cell_count());
                let mut elapsed_seconds = 0_u64;
                while elapsed_seconds < SECONDS_PER_CLIMATOLOGICAL_MONTH {
                    let dt_seconds =
                        base_dt_seconds.min(SECONDS_PER_CLIMATOLOGICAL_MONTH - elapsed_seconds);
                    let first = evaluate_tendencies(
                        &state,
                        &operators,
                        forcing,
                        spec,
                        &permeability,
                        month,
                        dt_seconds as f64,
                    )?;
                    let second_stage = advance_state(
                        &state,
                        &[(&first, 0.5)],
                        &operators,
                        forcing,
                        dt_seconds as f64,
                    )?;
                    let second = evaluate_tendencies(
                        &second_stage,
                        &operators,
                        forcing,
                        spec,
                        &permeability,
                        month,
                        dt_seconds as f64,
                    )?;
                    let third_stage = advance_state(
                        &state,
                        &[(&first, -1.0), (&second, 2.0)],
                        &operators,
                        forcing,
                        dt_seconds as f64,
                    )?;
                    let third = evaluate_tendencies(
                        &third_stage,
                        &operators,
                        forcing,
                        spec,
                        &permeability,
                        month,
                        dt_seconds as f64,
                    )?;
                    state = advance_state(
                        &state,
                        &[
                            (&first, 1.0 / 6.0),
                            (&second, 4.0 / 6.0),
                            (&third, 1.0 / 6.0),
                        ],
                        &operators,
                        forcing,
                        dt_seconds as f64,
                    )?;
                    let precipitation = first
                        .thermodynamics
                        .precipitation_mm_day()
                        .iter()
                        .zip(second.thermodynamics.precipitation_mm_day())
                        .zip(third.thermodynamics.precipitation_mm_day())
                        .map(|((first, second), third)| (*first + 4.0 * *second + *third) / 6.0)
                        .collect::<Vec<_>>();
                    accumulator.record(&state, &precipitation, dt_seconds as f64)?;
                    maximum_mass_error = maximum_mass_error
                        .max(first.relative_mass_error)
                        .max(second.relative_mass_error)
                        .max(third.relative_mass_error);
                    elapsed_seconds += dt_seconds;
                    total_steps += 1;
                }
                let (monthly_state, precipitation) = accumulator.finish(&operators, forcing)?;
                monthly_states.push(monthly_state);
                monthly_precipitation.push(precipitation);
            }

            if let Some(previous) = &previous_year {
                last_residual = previous
                    .iter()
                    .zip(&monthly_states)
                    .map(|(previous, current)| state_residual(grid, previous, current))
                    .fold(0.0_f64, f64::max);
                if last_residual <= f64::from(spec.convergence_tolerance) {
                    return transient_snapshot(
                        grid,
                        identity,
                        self.id(),
                        formation_year,
                        total_steps,
                        last_residual,
                        maximum_mass_error,
                        &monthly_states,
                        &monthly_precipitation,
                    );
                }
            }
            previous_year = Some(monthly_states);
        }

        Err(CirculationSolveError::FormationNotConverged {
            solver_id: self.id(),
            formation_years: spec.max_formation_years,
            steps: total_steps,
            residual: last_residual,
            tolerance: f64::from(spec.convergence_tolerance),
        })
    }
}

struct TransientTendencies {
    wind_m_s2: Vec<[f32; 3]>,
    ocean_current_m_s2: Vec<[f32; 3]>,
    atmosphere_height_m_s: Vec<f32>,
    sea_surface_height_m_s: Vec<f32>,
    thermodynamics: ThermodynamicTendencies,
    relative_mass_error: f64,
}

#[allow(clippy::too_many_arguments)]
fn exact_periodic_equilibrium_snapshot(
    state: &CirculationState,
    operators: &CirculationOperators<'_>,
    forcing: &PlanetForcing,
    spec: &CirculationSpec,
    permeability: &CirculationEdgePermeability,
    identity: SolverInputIdentity,
    solver_id: CirculationSolverId,
    dt_seconds: f64,
) -> Result<Option<CirculationSnapshot>, CirculationSolveError> {
    let mut monthly_precipitation = Vec::with_capacity(CLIMATE_MONTH_COUNT);
    let mut maximum_mass_error = 0.0_f64;
    for month in 0..CLIMATE_MONTH_COUNT {
        let tendencies = evaluate_tendencies(
            state,
            operators,
            forcing,
            spec,
            permeability,
            month,
            dt_seconds,
        )?;
        if !tendencies_are_round_off_zero(&tendencies) {
            return Ok(None);
        }
        maximum_mass_error = maximum_mass_error.max(tendencies.relative_mass_error);
        monthly_precipitation.push(tendencies.thermodynamics.precipitation_mm_day().to_vec());
    }
    let monthly_states = vec![state.clone(); CLIMATE_MONTH_COUNT];
    transient_snapshot(
        operators.grid(),
        identity,
        solver_id,
        1,
        CLIMATE_MONTH_COUNT as u64,
        0.0,
        maximum_mass_error,
        &monthly_states,
        &monthly_precipitation,
    )
    .map(Some)
}

fn tendencies_are_round_off_zero(tendencies: &TransientTendencies) -> bool {
    tendencies
        .wind_m_s2
        .iter()
        .chain(&tendencies.ocean_current_m_s2)
        .flatten()
        .all(|value| value.abs() <= ROUND_OFF_TENDENCY_FLOOR)
        && tendencies
            .atmosphere_height_m_s
            .iter()
            .chain(&tendencies.sea_surface_height_m_s)
            .all(|value| value.abs() <= ROUND_OFF_TENDENCY_FLOOR)
        && tendencies
            .thermodynamics
            .air_temperature_c_per_s()
            .iter()
            .chain(tendencies.thermodynamics.surface_temperature_c_per_s())
            .chain(tendencies.thermodynamics.specific_humidity_per_s())
            .all(|value| value.abs() <= ROUND_OFF_TENDENCY_FLOOR)
}

#[allow(clippy::too_many_arguments)]
fn evaluate_tendencies(
    state: &CirculationState,
    operators: &CirculationOperators<'_>,
    forcing: &PlanetForcing,
    spec: &CirculationSpec,
    permeability: &CirculationEdgePermeability,
    month: usize,
    dt_seconds: f64,
) -> Result<TransientTendencies, DynamicsError> {
    let grid = operators.grid();
    let atmosphere_equilibrium =
        thermal_height_target(grid, state.thermodynamics.air_temperature_c(), spec);
    let atmosphere_divergence = operators.divergence_validated(&state.wind_m_s);
    let atmosphere_height_m_s = atmosphere_equilibrium
        .iter()
        .zip(&state.atmosphere_height_anomaly_m)
        .zip(&atmosphere_divergence)
        .map(|((equilibrium, height), divergence)| {
            f64::from(spec.layer_relaxation_s_inv) * f64::from(*equilibrium - *height)
                - f64::from(spec.atmosphere_reference_depth_m) * f64::from(*divergence)
        })
        .map(|value| value as f32)
        .collect::<Vec<_>>();
    let atmosphere_gradient = operators.gradient_validated(&state.atmosphere_height_anomaly_m);
    let atmosphere_coriolis =
        operators.coriolis_validated(&state.wind_m_s, spec.rotation_rate_rad_s);
    let wind_m_s2 = atmosphere_gradient
        .iter()
        .zip(&atmosphere_coriolis)
        .zip(&state.wind_m_s)
        .map(|((gradient, coriolis), velocity)| {
            std::array::from_fn(|component| {
                (-f64::from(spec.atmosphere_reduced_gravity_m_s2) * f64::from(gradient[component])
                    + f64::from(coriolis[component])
                    - f64::from(spec.atmosphere_drag_s_inv) * f64::from(velocity[component]))
                    as f32
            })
        })
        .collect::<Vec<_>>();
    let wind_m_s2 = operators.tangentize_validated(&wind_m_s2);

    let sea_surface_equilibrium =
        inverse_barometer_height(grid, forcing, &state.atmosphere_height_anomaly_m);
    let ocean_divergence = operators
        .divergence_with_permeability_validated(&state.ocean_current_m_s, permeability.ocean());
    let sea_surface_height_m_s = sea_surface_equilibrium
        .iter()
        .zip(&state.sea_surface_height_anomaly_m)
        .zip(&ocean_divergence)
        .map(|((equilibrium, height), divergence)| {
            f64::from(spec.layer_relaxation_s_inv) * f64::from(*equilibrium - *height)
                - f64::from(spec.ocean_reference_depth_m) * f64::from(*divergence)
        })
        .map(|value| value as f32)
        .collect::<Vec<_>>();
    let ocean_gradient = operators.gradient_with_permeability_validated(
        &state.sea_surface_height_anomaly_m,
        permeability.ocean(),
    );
    let ocean_coriolis =
        operators.coriolis_validated(&state.ocean_current_m_s, spec.rotation_rate_rad_s);
    let ocean_current_m_s2 = ocean_gradient
        .iter()
        .zip(&ocean_coriolis)
        .zip(&state.ocean_current_m_s)
        .zip(&state.wind_m_s)
        .zip(forcing.land_fraction())
        .map(|((((gradient, coriolis), current), wind), land)| {
            let ocean = f64::from(1.0 - *land);
            std::array::from_fn(|component| {
                (ocean
                    * (-f64::from(spec.ocean_reduced_gravity_m_s2)
                        * f64::from(gradient[component])
                        + f64::from(coriolis[component])
                        - f64::from(spec.ocean_drag_s_inv) * f64::from(current[component])
                        + WIND_STRESS_RATE_S_INV * f64::from(wind[component] - current[component])))
                    as f32
            })
        })
        .collect::<Vec<_>>();
    let ocean_current_m_s2 = operators.tangentize_validated(&ocean_current_m_s2);
    let thermodynamics = thermodynamic_tendencies_validated(
        operators,
        forcing,
        spec,
        &state.thermodynamics,
        &state.wind_m_s,
        &state.ocean_current_m_s,
        permeability,
        month,
        dt_seconds,
    )?;
    let relative_mass_error = relative_divergence_closure(grid, &atmosphere_divergence)
        .max(relative_divergence_closure(grid, &ocean_divergence));
    Ok(TransientTendencies {
        wind_m_s2,
        ocean_current_m_s2,
        atmosphere_height_m_s,
        sea_surface_height_m_s,
        thermodynamics,
        relative_mass_error,
    })
}

fn advance_state(
    state: &CirculationState,
    weighted_tendencies: &[(&TransientTendencies, f64)],
    operators: &CirculationOperators<'_>,
    forcing: &PlanetForcing,
    dt_seconds: f64,
) -> Result<CirculationState, DynamicsError> {
    let grid = operators.grid();
    if weighted_tendencies.is_empty()
        || weighted_tendencies
            .iter()
            .any(|(_, weight)| !weight.is_finite())
    {
        return Err(DynamicsError::NonFiniteState);
    }
    let atmosphere_tendencies = weighted_tendencies
        .iter()
        .map(|(tendencies, weight)| (tendencies.atmosphere_height_m_s.as_slice(), *weight))
        .collect::<Vec<_>>();
    let sea_tendencies = weighted_tendencies
        .iter()
        .map(|(tendencies, weight)| (tendencies.sea_surface_height_m_s.as_slice(), *weight))
        .collect::<Vec<_>>();
    let wind_tendencies = weighted_tendencies
        .iter()
        .map(|(tendencies, weight)| (tendencies.wind_m_s2.as_slice(), *weight))
        .collect::<Vec<_>>();
    let current_tendencies = weighted_tendencies
        .iter()
        .map(|(tendencies, weight)| (tendencies.ocean_current_m_s2.as_slice(), *weight))
        .collect::<Vec<_>>();
    let thermodynamic_tendencies = weighted_tendencies
        .iter()
        .map(|(tendencies, weight)| (&tendencies.thermodynamics, *weight))
        .collect::<Vec<_>>();
    let mut atmosphere_height_anomaly_m = advance_scalar_field(
        &state.atmosphere_height_anomaly_m,
        &atmosphere_tendencies,
        dt_seconds,
    )?;
    let mut sea_surface_height_anomaly_m = advance_scalar_field(
        &state.sea_surface_height_anomaly_m,
        &sea_tendencies,
        dt_seconds,
    )?;
    remove_layer_mean(grid, &mut atmosphere_height_anomaly_m, None);
    remove_layer_mean(
        grid,
        &mut sea_surface_height_anomaly_m,
        Some(forcing.land_fraction()),
    );
    let wind_m_s = advance_vector_field(&state.wind_m_s, &wind_tendencies, dt_seconds)?;
    let wind_m_s = operators.tangentize_validated(&wind_m_s);
    let mut ocean_current_m_s =
        advance_vector_field(&state.ocean_current_m_s, &current_tendencies, dt_seconds)?;
    for (velocity, land) in ocean_current_m_s.iter_mut().zip(forcing.land_fraction()) {
        let ocean = 1.0 - *land;
        for component in velocity {
            *component *= ocean;
        }
    }
    let ocean_current_m_s = operators.tangentize_validated(&ocean_current_m_s);
    let thermodynamics = advance_thermodynamics_weighted(
        &state.thermodynamics,
        &thermodynamic_tendencies,
        dt_seconds,
    )?;
    Ok(CirculationState {
        wind_m_s,
        ocean_current_m_s,
        atmosphere_height_anomaly_m,
        sea_surface_height_anomaly_m,
        thermodynamics,
    })
}

fn advance_scalar_field(
    state: &[f32],
    weighted_tendencies: &[(&[f32], f64)],
    dt_seconds: f64,
) -> Result<Vec<f32>, DynamicsError> {
    state
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let tendency = weighted_tendencies
                .iter()
                .map(|(values, weight)| weight * f64::from(values[index]))
                .sum::<f64>();
            checked_f32(f64::from(*state) + dt_seconds * tendency)
        })
        .collect()
}

fn advance_vector_field(
    state: &[[f32; 3]],
    weighted_tendencies: &[(&[[f32; 3]], f64)],
    dt_seconds: f64,
) -> Result<Vec<[f32; 3]>, DynamicsError> {
    state
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let mut value = [0.0_f32; 3];
            for component in 0..3 {
                let tendency = weighted_tendencies
                    .iter()
                    .map(|(values, weight)| weight * f64::from(values[index][component]))
                    .sum::<f64>();
                value[component] =
                    checked_f32(f64::from(state[component]) + dt_seconds * tendency)?;
            }
            Ok(value)
        })
        .collect()
}

fn checked_f32(value: f64) -> Result<f32, DynamicsError> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(DynamicsError::NonFiniteState);
    }
    Ok(value as f32)
}

fn relative_divergence_closure(grid: &CubedSphereGrid, divergence: &[f32]) -> f64 {
    let (signed, absolute) = grid.cells().iter().zip(divergence).fold(
        (0.0_f64, 0.0_f64),
        |(signed, absolute), (cell, divergence)| {
            let extensive = cell.area_m2() * f64::from(*divergence);
            (signed + extensive, absolute + extensive.abs())
        },
    );
    if absolute == 0.0 {
        0.0
    } else {
        signed.abs() / absolute
    }
}

struct MonthlyAccumulator {
    wind: Vec<[f64; 3]>,
    current: Vec<[f64; 3]>,
    atmosphere_height: Vec<f64>,
    sea_height: Vec<f64>,
    air_temperature: Vec<f64>,
    surface_temperature: Vec<f64>,
    humidity: Vec<f64>,
    precipitation: Vec<f64>,
    total_seconds: f64,
}

impl MonthlyAccumulator {
    fn new(cell_count: usize) -> Self {
        Self {
            wind: vec![[0.0; 3]; cell_count],
            current: vec![[0.0; 3]; cell_count],
            atmosphere_height: vec![0.0; cell_count],
            sea_height: vec![0.0; cell_count],
            air_temperature: vec![0.0; cell_count],
            surface_temperature: vec![0.0; cell_count],
            humidity: vec![0.0; cell_count],
            precipitation: vec![0.0; cell_count],
            total_seconds: 0.0,
        }
    }

    fn record(
        &mut self,
        state: &CirculationState,
        precipitation_mm_day: &[f32],
        dt_seconds: f64,
    ) -> Result<(), CirculationSolveError> {
        if precipitation_mm_day.len() != self.wind.len() {
            return Err(CirculationSolveError::OutputFieldLengthMismatch {
                field: "precipitation_mm_day",
                expected: self.wind.len(),
                found: precipitation_mm_day.len(),
            });
        }
        for (cell, precipitation) in precipitation_mm_day.iter().enumerate() {
            for component in 0..3 {
                self.wind[cell][component] +=
                    dt_seconds * f64::from(state.wind_m_s[cell][component]);
                self.current[cell][component] +=
                    dt_seconds * f64::from(state.ocean_current_m_s[cell][component]);
            }
            self.atmosphere_height[cell] +=
                dt_seconds * f64::from(state.atmosphere_height_anomaly_m[cell]);
            self.sea_height[cell] +=
                dt_seconds * f64::from(state.sea_surface_height_anomaly_m[cell]);
            self.air_temperature[cell] +=
                dt_seconds * f64::from(state.thermodynamics.air_temperature_c()[cell]);
            self.surface_temperature[cell] +=
                dt_seconds * f64::from(state.thermodynamics.surface_temperature_c()[cell]);
            self.humidity[cell] +=
                dt_seconds * f64::from(state.thermodynamics.specific_humidity()[cell]);
            self.precipitation[cell] += dt_seconds * f64::from(*precipitation);
        }
        self.total_seconds += dt_seconds;
        Ok(())
    }

    fn finish(
        self,
        operators: &CirculationOperators<'_>,
        forcing: &PlanetForcing,
    ) -> Result<(CirculationState, Vec<f32>), CirculationSolveError> {
        if !self.total_seconds.is_finite() || self.total_seconds <= 0.0 {
            return Err(CirculationSolveError::InvalidTimeStep { found_seconds: 0 });
        }
        let inverse = self.total_seconds.recip();
        let wind = average_vectors(self.wind, inverse)?;
        let wind = operators.tangentize(&wind)?;
        let mut current = average_vectors(self.current, inverse)?;
        for (velocity, land) in current.iter_mut().zip(forcing.land_fraction()) {
            if *land >= 1.0 {
                *velocity = [0.0; 3];
            }
        }
        let current = operators.tangentize(&current)?;
        let atmosphere_height_anomaly_m = average_scalars(self.atmosphere_height, inverse)?;
        let sea_surface_height_anomaly_m = average_scalars(self.sea_height, inverse)?;
        let thermodynamics = ThermodynamicState::new(
            average_scalars(self.air_temperature, inverse)?,
            average_scalars(self.surface_temperature, inverse)?,
            average_scalars(self.humidity, inverse)?,
        )?;
        let precipitation = average_scalars(self.precipitation, inverse)?;
        Ok((
            CirculationState {
                wind_m_s: wind,
                ocean_current_m_s: current,
                atmosphere_height_anomaly_m,
                sea_surface_height_anomaly_m,
                thermodynamics,
            },
            precipitation,
        ))
    }
}

fn average_scalars(values: Vec<f64>, inverse: f64) -> Result<Vec<f32>, DynamicsError> {
    values
        .into_iter()
        .map(|value| checked_f32(value * inverse))
        .collect()
}

fn average_vectors(values: Vec<[f64; 3]>, inverse: f64) -> Result<Vec<[f32; 3]>, DynamicsError> {
    values
        .into_iter()
        .map(|value| {
            Ok([
                checked_f32(value[0] * inverse)?,
                checked_f32(value[1] * inverse)?,
                checked_f32(value[2] * inverse)?,
            ])
        })
        .collect()
}

fn state_from_snapshot(
    snapshot: &CirculationSnapshot,
    month: usize,
) -> Result<CirculationState, CirculationSolveError> {
    let thermodynamics = ThermodynamicState::new(
        snapshot
            .monthly_air_temperature_c()
            .iter()
            .map(|months| months[month])
            .collect(),
        snapshot
            .monthly_surface_temperature_c()
            .iter()
            .map(|months| months[month])
            .collect(),
        snapshot
            .monthly_specific_humidity()
            .iter()
            .map(|months| months[month])
            .collect(),
    )?;
    Ok(CirculationState {
        wind_m_s: snapshot
            .monthly_wind_m_s()
            .iter()
            .map(|months| months[month])
            .collect(),
        ocean_current_m_s: snapshot
            .monthly_ocean_current_m_s()
            .iter()
            .map(|months| months[month])
            .collect(),
        atmosphere_height_anomaly_m: snapshot
            .monthly_atmosphere_height_anomaly_m()
            .iter()
            .map(|months| months[month])
            .collect(),
        sea_surface_height_anomaly_m: snapshot
            .monthly_sea_surface_height_anomaly_m()
            .iter()
            .map(|months| months[month])
            .collect(),
        thermodynamics,
    })
}

fn states_from_snapshot(
    snapshot: &CirculationSnapshot,
) -> Result<Vec<CirculationState>, CirculationSolveError> {
    (0..CLIMATE_MONTH_COUNT)
        .map(|month| state_from_snapshot(snapshot, month))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn transient_snapshot(
    grid: &CubedSphereGrid,
    identity: SolverInputIdentity,
    solver_id: CirculationSolverId,
    formation_years: u16,
    total_steps: u64,
    residual: f64,
    relative_mass_error: f64,
    monthly_states: &[CirculationState],
    monthly_precipitation: &[Vec<f32>],
) -> Result<CirculationSnapshot, CirculationSolveError> {
    let mut builder = MonthlySnapshotBuilder::new(grid.cell_count());
    for month in 0..CLIMATE_MONTH_COUNT {
        builder.record(month, &monthly_states[month], &monthly_precipitation[month])?;
    }
    let working_bytes = dense_state_bytes(grid.cell_count())?
        .checked_mul(TRANSIENT_DENSE_STATE_MULTIPLIER)
        .ok_or(CirculationSolveError::AllocationOverflow)?;
    builder.finish(
        identity,
        solver_id,
        CirculationSolveStats {
            iterations_or_steps: total_steps,
            formation_years,
            final_residual: residual,
            relative_mass_error,
            dense_state_bytes: working_bytes,
        },
    )
}

fn validate_grid_spec(
    grid: &CubedSphereGrid,
    spec: &CirculationSpec,
) -> Result<(), CirculationSolveError> {
    spec.validate()?;
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
    Ok(())
}

fn maximum_wave_speed(spec: &CirculationSpec) -> f64 {
    (f64::from(spec.atmosphere_reduced_gravity_m_s2) * f64::from(spec.atmosphere_reference_depth_m))
        .sqrt()
        .max(
            (f64::from(spec.ocean_reduced_gravity_m_s2) * f64::from(spec.ocean_reference_depth_m))
                .sqrt(),
        )
}
