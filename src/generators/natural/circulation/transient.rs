use crate::world::natural::{
    CirculationSnapshot, CirculationSolveStats, CirculationSolverId, CirculationSpec,
    PlanetForcing, CLIMATE_MONTH_COUNT,
};

use super::{
    dynamics::{
        dense_state_bytes, initial_state, inverse_barometer_height, layer_depth_from_anomaly,
        maximum_relative_mass_closure_error, remove_layer_mean, state_residual,
        thermal_height_target, CirculationState, DynamicsError, WIND_STRESS_RATE_S_INV,
    },
    solver::{validate_solver_inputs, MonthlySnapshotBuilder, SolverInputIdentity},
    thermodynamics::{
        advance_thermodynamics_weighted, thermodynamic_tendencies_validated, MAX_SPECIFIC_HUMIDITY,
    },
    CirculationEdgePermeability, CirculationOperators, CirculationSolveError, CirculationSolver,
    CubedSphereGrid, ThermodynamicState, ThermodynamicTendencies,
};

const SECONDS_PER_DAY: u64 = 86_400;
const DAYS_PER_CLIMATOLOGICAL_MONTH: u64 = 30;
const SECONDS_PER_CLIMATOLOGICAL_MONTH: u64 = SECONDS_PER_DAY * DAYS_PER_CLIMATOLOGICAL_MONTH;
const TRANSIENT_DENSE_STATE_MULTIPLIER: u64 = 6;
const RK3_TENDENCY_COUNT: u64 = 3;
const ROUND_OFF_TENDENCY_FLOOR: f32 = 1.0e-30;
const CLASSIC_RK3_IMAGINARY_STABILITY_RADIUS: f64 = 1.732_050_807_568_877_2;
const TEMPORAL_STABILITY_SAFETY_FACTOR: f64 = 0.9;

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
        let wave_limited_seconds = (f64::from(spec.cfl_limit) * grid.minimum_center_distance_m()
            / maximum_wave_speed)
            .floor();
        let maximum_coriolis_frequency = 2.0 * spec.rotation_rate_rad_s.abs();
        let rotation_limited_seconds = if maximum_coriolis_frequency > 0.0 {
            TEMPORAL_STABILITY_SAFETY_FACTOR * CLASSIC_RK3_IMAGINARY_STABILITY_RADIUS
                / maximum_coriolis_frequency
        } else {
            f64::INFINITY
        };
        let raw_seconds = wave_limited_seconds.min(rotation_limited_seconds).floor();
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
                    let step = advance_rk3_step(
                        &state,
                        &operators,
                        forcing,
                        spec,
                        &permeability,
                        month,
                        dt_seconds as f64,
                    )?;
                    accumulator.record(
                        &step.state,
                        &step.precipitation_mm_day,
                        dt_seconds as f64,
                    )?;
                    maximum_mass_error = maximum_mass_error.max(step.mass_errors.maximum());
                    state = step.state;
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

struct Rk3Step {
    state: CirculationState,
    precipitation_mm_day: Vec<f32>,
    mass_errors: Rk3MassErrors,
}

struct Rk3MassErrors {
    stages: [f64; 3],
    final_stored_state: f64,
}

impl Rk3MassErrors {
    fn maximum(&self) -> f64 {
        maximum_rk3_mass_error(self.stages, self.final_stored_state)
    }
}

fn maximum_rk3_mass_error(stage_errors: [f64; 3], final_stored_state: f64) -> f64 {
    stage_errors
        .into_iter()
        .chain(std::iter::once(final_stored_state))
        .fold(0.0_f64, f64::max)
}

fn advance_rk3_step(
    state: &CirculationState,
    operators: &CirculationOperators<'_>,
    forcing: &PlanetForcing,
    spec: &CirculationSpec,
    permeability: &CirculationEdgePermeability,
    month: usize,
    dt_seconds: f64,
) -> Result<Rk3Step, DynamicsError> {
    let first = evaluate_tendencies(
        state,
        operators,
        forcing,
        spec,
        permeability,
        month,
        dt_seconds,
    )?;
    let second_stage = advance_state(
        state,
        &[(&first, 0.5)],
        operators,
        forcing,
        spec,
        dt_seconds,
    )?;
    let second = evaluate_tendencies(
        &second_stage,
        operators,
        forcing,
        spec,
        permeability,
        month,
        dt_seconds,
    )?;
    let third_stage = advance_state(
        state,
        &[(&first, -1.0), (&second, 2.0)],
        operators,
        forcing,
        spec,
        dt_seconds,
    )?;
    let third = evaluate_tendencies(
        &third_stage,
        operators,
        forcing,
        spec,
        permeability,
        month,
        dt_seconds,
    )?;
    let final_tendencies = [
        (&first, 1.0 / 6.0),
        (&second, 4.0 / 6.0),
        (&third, 1.0 / 6.0),
    ];
    let next_state = advance_state(
        state,
        &final_tendencies,
        operators,
        forcing,
        spec,
        dt_seconds,
    )?;
    let final_stored_state = relative_column_moisture_rk_closure_error(
        operators.grid(),
        state,
        &next_state,
        &final_tendencies,
        spec,
        dt_seconds,
    )?;
    let precipitation_mm_day = first
        .thermodynamics
        .precipitation_mm_day()
        .iter()
        .zip(second.thermodynamics.precipitation_mm_day())
        .zip(third.thermodynamics.precipitation_mm_day())
        .map(|((first, second), third)| (*first + 4.0 * *second + *third) / 6.0)
        .collect();
    Ok(Rk3Step {
        state: next_state,
        precipitation_mm_day,
        mass_errors: Rk3MassErrors {
            stages: [
                first.relative_mass_error,
                second.relative_mass_error,
                third.relative_mass_error,
            ],
            final_stored_state,
        },
    })
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
        && tendencies
            .thermodynamics
            .column_moisture_m_per_s()
            .iter()
            .all(|value| value.abs() <= f64::from(ROUND_OFF_TENDENCY_FLOOR))
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
            // Current is the local wet-subcell velocity. Wet fraction belongs in the
            // finite-volume edge flux, so it cancels from this local momentum control
            // volume; only a cell with no wet area has no prognostic current.
            if *land >= 1.0 {
                [0.0; 3]
            } else {
                std::array::from_fn(|component| {
                    (-f64::from(spec.ocean_reduced_gravity_m_s2) * f64::from(gradient[component])
                        + f64::from(coriolis[component])
                        - f64::from(spec.ocean_drag_s_inv) * f64::from(current[component])
                        + WIND_STRESS_RATE_S_INV * f64::from(wind[component] - current[component]))
                        as f32
                })
            }
        })
        .collect::<Vec<_>>();
    let ocean_current_m_s2 = operators.tangentize_validated(&ocean_current_m_s2);
    let atmosphere_layer_depth_m = layer_depth_from_anomaly(
        spec.atmosphere_reference_depth_m,
        &state.atmosphere_height_anomaly_m,
    )?;
    let mut thermodynamics = thermodynamic_tendencies_validated(
        operators,
        forcing,
        spec,
        &state.thermodynamics,
        &atmosphere_layer_depth_m,
        &state.wind_m_s,
        &state.ocean_current_m_s,
        permeability,
        month,
        dt_seconds,
    )?;
    for (((moisture_tendency, humidity), equilibrium), height) in thermodynamics
        .column_moisture_m_per_s_mut()
        .iter_mut()
        .zip(state.thermodynamics.specific_humidity())
        .zip(&atmosphere_equilibrium)
        .zip(&state.atmosphere_height_anomaly_m)
    {
        let layer_relaxation =
            f64::from(spec.layer_relaxation_s_inv) * f64::from(*equilibrium - *height);
        *moisture_tendency += f64::from(*humidity) * layer_relaxation;
    }
    let relative_mass_error = maximum_relative_mass_closure_error(
        grid,
        &atmosphere_divergence,
        &ocean_divergence,
        thermodynamics.relative_moisture_transport_error(),
    );
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
    spec: &CirculationSpec,
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
        // Fractional wetness is not a time-independent damping operator.
        if *land >= 1.0 {
            *velocity = [0.0; 3];
        }
    }
    let ocean_current_m_s = operators.tangentize_validated(&ocean_current_m_s);
    let specific_humidity = advance_column_moisture(
        state,
        &atmosphere_height_anomaly_m,
        weighted_tendencies,
        spec,
        dt_seconds,
    )?;
    let thermodynamics = advance_thermodynamics_weighted(
        &state.thermodynamics,
        &thermodynamic_tendencies,
        specific_humidity,
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

fn advance_column_moisture(
    state: &CirculationState,
    next_atmosphere_height_anomaly_m: &[f32],
    weighted_tendencies: &[(&TransientTendencies, f64)],
    spec: &CirculationSpec,
    dt_seconds: f64,
) -> Result<Vec<f32>, DynamicsError> {
    state
        .atmosphere_height_anomaly_m
        .iter()
        .enumerate()
        .map(|(cell, _)| {
            let (next_layer, projected_column) = projected_column_moisture(
                state,
                next_atmosphere_height_anomaly_m[cell],
                weighted_tendencies,
                spec,
                dt_seconds,
                cell,
            )?;
            // Bounds are an explicit projection budget. RK itself combines the
            // extensive column quantity, then recovers the intensive mixing ratio.
            checked_f32(projected_column / next_layer)
        })
        .collect()
}

fn projected_column_moisture(
    state: &CirculationState,
    next_atmosphere_height_anomaly_m: f32,
    weighted_tendencies: &[(&TransientTendencies, f64)],
    spec: &CirculationSpec,
    dt_seconds: f64,
    cell: usize,
) -> Result<(f64, f64), DynamicsError> {
    let layer = f64::from(spec.atmosphere_reference_depth_m)
        + f64::from(state.atmosphere_height_anomaly_m[cell]);
    let next_layer =
        f64::from(spec.atmosphere_reference_depth_m) + f64::from(next_atmosphere_height_anomaly_m);
    if !layer.is_finite() || layer <= 0.0 || !next_layer.is_finite() || next_layer <= 0.0 {
        return Err(DynamicsError::NonFiniteState);
    }
    let tendency = weighted_tendencies
        .iter()
        .map(|(tendencies, weight)| {
            weight * tendencies.thermodynamics.column_moisture_m_per_s()[cell]
        })
        .sum::<f64>();
    let predicted_column =
        layer * f64::from(state.thermodynamics.specific_humidity()[cell]) + dt_seconds * tendency;
    if !predicted_column.is_finite() {
        return Err(DynamicsError::NonFiniteState);
    }
    let projected_column =
        next_layer * (predicted_column / next_layer).clamp(0.0, f64::from(MAX_SPECIFIC_HUMIDITY));
    Ok((next_layer, projected_column))
}

fn relative_column_moisture_rk_closure_error(
    grid: &CubedSphereGrid,
    state: &CirculationState,
    next_state: &CirculationState,
    weighted_tendencies: &[(&TransientTendencies, f64)],
    spec: &CirculationSpec,
    dt_seconds: f64,
) -> Result<f64, DynamicsError> {
    debug_assert_eq!(state.atmosphere_height_anomaly_m.len(), grid.cell_count());
    debug_assert_eq!(
        next_state.atmosphere_height_anomaly_m.len(),
        grid.cell_count()
    );
    debug_assert_eq!(state.thermodynamics.cell_count(), grid.cell_count());
    debug_assert_eq!(next_state.thermodynamics.cell_count(), grid.cell_count());

    let mut signed_error = CompensatedSum::default();
    let mut reference_total = CompensatedSum::default();
    for cell in 0..grid.cell_count() {
        // Apply the same explicit bound projection as `advance_column_moisture` before
        // measuring numerical RK closure, so the projection remains a separate budget.
        let (next_layer, projected_column) = projected_column_moisture(
            state,
            next_state.atmosphere_height_anomaly_m[cell],
            weighted_tendencies,
            spec,
            dt_seconds,
            cell,
        )?;
        let actual_column =
            next_layer * f64::from(next_state.thermodynamics.specific_humidity()[cell]);
        let area = grid.cells()[cell].area_m2();
        signed_error.add(area * (actual_column - projected_column));
        reference_total.add(area * projected_column.abs());
    }

    let scale = reference_total.total();
    let error = signed_error.total().abs();
    if scale == 0.0 {
        return if error == 0.0 {
            Ok(0.0)
        } else {
            Err(DynamicsError::NonFiniteState)
        };
    }
    let relative_error = error / scale;
    if !relative_error.is_finite() {
        return Err(DynamicsError::NonFiniteState);
    }
    Ok(relative_error)
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

#[derive(Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let adjusted = value - self.correction;
        let next = self.sum + adjusted;
        self.correction = (next - self.sum) - adjusted;
        self.sum = next;
    }

    const fn total(&self) -> f64 {
        self.sum
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
    let working_bytes = transient_dense_working_bytes(grid.cell_count())?;
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

fn transient_dense_working_bytes(cell_count: usize) -> Result<u64, DynamicsError> {
    let original_estimate = dense_state_bytes(cell_count)?
        .checked_mul(TRANSIENT_DENSE_STATE_MULTIPLIER)
        .ok_or(DynamicsError::AllocationOverflow)?;
    let column_tendency_bytes = u64::try_from(cell_count)
        .map_err(|_| DynamicsError::AllocationOverflow)?
        .checked_mul(std::mem::size_of::<f64>() as u64)
        .and_then(|bytes| bytes.checked_mul(RK3_TENDENCY_COUNT))
        .ok_or(DynamicsError::AllocationOverflow)?;
    original_estimate
        .checked_add(column_tendency_bytes)
        .ok_or(DynamicsError::AllocationOverflow)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_working_bytes_include_three_f64_column_tendency_arrays() {
        let cell_count = 24_576;
        let original_estimate =
            dense_state_bytes(cell_count).unwrap() * TRANSIENT_DENSE_STATE_MULTIPLIER;
        let column_tendencies =
            u64::try_from(cell_count).unwrap() * std::mem::size_of::<f64>() as u64 * 3;
        assert_eq!(
            transient_dense_working_bytes(cell_count).unwrap(),
            original_estimate + column_tendencies
        );
    }

    #[test]
    fn rk3_mass_statistic_includes_the_final_stored_state_closure() {
        assert_eq!(maximum_rk3_mass_error([0.1, 0.2, 0.3], 0.4), 0.4);
    }

    fn fractional_coast_forcing(grid: &CubedSphereGrid) -> PlanetForcing {
        let count = grid.cell_count();
        PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.5; count],
            vec![0.3; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.005; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap()
    }

    fn solid_rotation(grid: &CubedSphereGrid, speed: f32) -> Vec<[f32; 3]> {
        grid.cells()
            .iter()
            .map(|cell| {
                let radial = cell.center_unit();
                [-radial[1] as f32 * speed, radial[0] as f32 * speed, 0.0]
            })
            .collect()
    }

    fn divergent_flow(grid: &CubedSphereGrid, speed: f32) -> Vec<[f32; 3]> {
        grid.cells()
            .iter()
            .map(|cell| {
                let radial = cell.center_unit();
                let radial_projection = speed * radial[0] as f32;
                [
                    speed - radial_projection * radial[0] as f32,
                    -radial_projection * radial[1] as f32,
                    -radial_projection * radial[2] as f32,
                ]
            })
            .collect()
    }

    fn total_column_moisture(
        grid: &CubedSphereGrid,
        spec: &CirculationSpec,
        state: &CirculationState,
    ) -> f64 {
        grid.cells()
            .iter()
            .zip(&state.atmosphere_height_anomaly_m)
            .zip(state.thermodynamics.specific_humidity())
            .map(|((cell, anomaly), humidity)| {
                cell.area_m2()
                    * (f64::from(spec.atmosphere_reference_depth_m) + f64::from(*anomaly))
                    * f64::from(*humidity)
            })
            .sum()
    }

    fn fractional_coast_state(grid: &CubedSphereGrid, forcing: &PlanetForcing) -> CirculationState {
        CirculationState {
            wind_m_s: vec![[0.0; 3]; grid.cell_count()],
            ocean_current_m_s: solid_rotation(grid, 0.5),
            atmosphere_height_anomaly_m: vec![0.0; grid.cell_count()],
            sea_surface_height_anomaly_m: vec![0.0; grid.cell_count()],
            thermodynamics: ThermodynamicState::from_forcing(grid, forcing, 0).unwrap(),
        }
    }

    fn integrate_steps(
        mut state: CirculationState,
        operators: &CirculationOperators<'_>,
        forcing: &PlanetForcing,
        spec: &CirculationSpec,
        permeability: &CirculationEdgePermeability,
        dt_seconds: f64,
        steps: usize,
    ) -> CirculationState {
        for _ in 0..steps {
            let tendencies = evaluate_tendencies(
                &state,
                operators,
                forcing,
                spec,
                permeability,
                0,
                dt_seconds,
            )
            .unwrap();
            state = advance_state(
                &state,
                &[(&tendencies, 1.0)],
                operators,
                forcing,
                spec,
                dt_seconds,
            )
            .unwrap();
        }
        state
    }

    #[test]
    fn fractional_coast_current_has_a_finite_zero_dt_tendency_and_refines_in_time() {
        let spec = CirculationSpec {
            face_resolution: 4,
            ..CirculationSpec::default()
        };
        let grid = CubedSphereGrid::new(spec.face_resolution, spec.planet_radius_m).unwrap();
        let forcing = fractional_coast_forcing(&grid);
        let operators = CirculationOperators::new(&grid);
        let permeability = CirculationEdgePermeability::from_forcing(&grid, &forcing).unwrap();
        let initial = fractional_coast_state(&grid, &forcing);

        let tiny = integrate_steps(
            initial.clone(),
            &operators,
            &forcing,
            &spec,
            &permeability,
            1.0e-6,
            1,
        );
        for (before, after) in initial
            .ocean_current_m_s
            .iter()
            .zip(&tiny.ocean_current_m_s)
        {
            for component in 0..3 {
                assert!((after[component] - before[component]).abs() < 1.0e-6);
            }
        }

        let coarse = integrate_steps(
            initial.clone(),
            &operators,
            &forcing,
            &spec,
            &permeability,
            60.0,
            1,
        );
        let fine = integrate_steps(initial, &operators, &forcing, &spec, &permeability, 30.0, 2);
        let maximum_difference = coarse
            .ocean_current_m_s
            .iter()
            .zip(&fine.ocean_current_m_s)
            .flat_map(|(coarse, fine)| {
                (0..3).map(|component| (coarse[component] - fine[component]).abs())
            })
            .fold(0.0_f32, f32::max);
        assert!(maximum_difference < 1.0e-4);
    }

    #[test]
    fn transient_conservation_statistic_includes_paired_moisture_closure() {
        let spec = CirculationSpec {
            face_resolution: 4,
            ..CirculationSpec::default()
        };
        let grid = CubedSphereGrid::new(spec.face_resolution, spec.planet_radius_m).unwrap();
        let forcing = fractional_coast_forcing(&grid);
        let operators = CirculationOperators::new(&grid);
        let permeability = CirculationEdgePermeability::from_forcing(&grid, &forcing).unwrap();
        let mut state = fractional_coast_state(&grid, &forcing);
        state.wind_m_s = divergent_flow(&grid, 20.0);
        state.thermodynamics = ThermodynamicState::new(
            vec![15.0; grid.cell_count()],
            vec![15.0; grid.cell_count()],
            grid.cells()
                .iter()
                .map(|cell| (0.01 + 0.002 * cell.center_unit()[0]) as f32)
                .collect(),
        )
        .unwrap();
        let tendencies = evaluate_tendencies(
            &state,
            &operators,
            &forcing,
            &spec,
            &permeability,
            0,
            3_600.0,
        )
        .unwrap();
        let atmosphere_divergence = operators.divergence(&state.wind_m_s).unwrap();
        let ocean_divergence = operators
            .divergence_with_permeability(&state.ocean_current_m_s, permeability.ocean())
            .unwrap();
        let expected = maximum_relative_mass_closure_error(
            &grid,
            &atmosphere_divergence,
            &ocean_divergence,
            tendencies
                .thermodynamics
                .relative_moisture_transport_error(),
        );
        assert_eq!(tendencies.relative_mass_error.to_bits(), expected.to_bits());
    }

    #[test]
    fn actual_linearized_layer_and_moisture_share_one_edge_flux() {
        let spec = CirculationSpec {
            face_resolution: 4,
            ..CirculationSpec::default()
        };
        let grid = CubedSphereGrid::new(spec.face_resolution, spec.planet_radius_m).unwrap();
        let temperature = grid
            .cells()
            .iter()
            .map(|cell| (15.0 + 5.0 * cell.center_unit()[0]) as f32)
            .collect::<Vec<_>>();
        let humidity = grid
            .cells()
            .iter()
            .map(|cell| (0.004 + 0.0005 * cell.center_unit()[1]) as f32)
            .collect::<Vec<_>>();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; grid.cell_count()],
            vec![0.0; grid.cell_count()],
            vec![0.3; grid.cell_count()],
            vec![1.0; grid.cell_count()],
            vec![[240.0; CLIMATE_MONTH_COUNT]; grid.cell_count()],
            temperature
                .iter()
                .map(|value| [*value; CLIMATE_MONTH_COUNT])
                .collect(),
            temperature
                .iter()
                .map(|value| [*value; CLIMATE_MONTH_COUNT])
                .collect(),
            humidity
                .iter()
                .map(|value| [*value; CLIMATE_MONTH_COUNT])
                .collect(),
        )
        .unwrap();
        let mut atmosphere_height_anomaly_m = thermal_height_target(&grid, &temperature, &spec);
        remove_layer_mean(&grid, &mut atmosphere_height_anomaly_m, None);
        let state = CirculationState {
            wind_m_s: divergent_flow(&grid, 20.0),
            ocean_current_m_s: vec![[0.0; 3]; grid.cell_count()],
            sea_surface_height_anomaly_m: inverse_barometer_height(
                &grid,
                &forcing,
                &atmosphere_height_anomaly_m,
            ),
            atmosphere_height_anomaly_m,
            thermodynamics: ThermodynamicState::new(temperature.clone(), temperature, humidity)
                .unwrap(),
        };
        let operators = CirculationOperators::new(&grid);
        let permeability = CirculationEdgePermeability::from_forcing(&grid, &forcing).unwrap();
        let dt_seconds = 3_600.0;
        let step = advance_rk3_step(
            &state,
            &operators,
            &forcing,
            &spec,
            &permeability,
            0,
            dt_seconds,
        )
        .unwrap();

        let before = total_column_moisture(&grid, &spec, &state);
        let after = total_column_moisture(&grid, &spec, &step.state);
        assert!(
            (after - before).abs() / before.abs() < 5.0e-9,
            "actual coupled state changed column moisture: before={before}, after={after}"
        );
        assert!(
            step.mass_errors.final_stored_state < 5.0e-9,
            "final stored RK state failed moisture closure: {}",
            step.mass_errors.final_stored_state
        );
    }
}
