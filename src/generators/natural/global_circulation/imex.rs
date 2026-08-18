use super::rk3::{
    combine_state, estimate_cfl, validate_step, ClimateDerivative, ClimateIntegratorDiagnostics,
    ClimateIntegratorError, ClimateStepResult,
};
use super::{
    ClimateConservationInterpretation, FormationProcedureIdentity, LayeredClimateState,
    LayeredTendencySystem, LayeredTendencyWorkspace,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
use crate::world::natural::{ClimateCapabilitySet, ClimateModelProfile, PlanetForcing};

#[derive(Debug, Clone, Copy)]
pub struct ImexCrankNicolsonIntegrator<'grid> {
    grid: &'grid CubedSphereGrid,
    maximum_linear_iterations: u16,
    linear_relative_tolerance: f64,
}

impl<'grid> ImexCrankNicolsonIntegrator<'grid> {
    pub fn new(
        grid: &'grid CubedSphereGrid,
        maximum_linear_iterations: u16,
        linear_relative_tolerance: f64,
    ) -> Result<Self, ClimateIntegratorError> {
        if maximum_linear_iterations == 0 {
            return Err(ClimateIntegratorError::InvalidLinearIterationBudget);
        }
        if !linear_relative_tolerance.is_finite() || linear_relative_tolerance <= 0.0 {
            return Err(ClimateIntegratorError::InvalidLinearTolerance {
                found: linear_relative_tolerance,
            });
        }
        Ok(Self {
            grid,
            maximum_linear_iterations,
            linear_relative_tolerance,
        })
    }

    /// Declares the scientific capabilities and conservation ledger owned by
    /// the actual IMEX comparison implementation.
    pub fn formation_procedure_identity(
        &self,
        profile: ClimateModelProfile,
    ) -> FormationProcedureIdentity {
        FormationProcedureIdentity::new(
            ClimateCapabilitySet::for_profile(profile),
            ClimateConservationInterpretation::SharedTendencyExtensiveV1,
            super::global_circulation_model_fingerprint(profile),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        dt_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<ClimateStepResult, ClimateIntegratorError> {
        validate_step(self.grid, state, dt_seconds, cancellation)?;
        let system = LayeredTendencySystem::new(self.grid);
        let mut workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        let base_tendency = system.evaluate_with_workspace_for_step(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            dt_seconds,
            cancellation,
            &mut workspace,
        )?;
        let mut mean_precipitation_rate_mm_s = base_tendency.precipitation_rate_mm_s().to_vec();
        let base_implicit_tendency = system.evaluate_linear_implicit_with_workspace(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            &mut workspace,
        )?;
        let base_derivative =
            ClimateDerivative::from_tendency(state, &base_tendency, cancellation)?;
        let implicit_derivative =
            ClimateDerivative::from_tendency(state, &base_implicit_tendency, cancellation)?;
        let explicit_derivative = base_derivative.subtract(&implicit_derivative, cancellation)?;
        let base_implicit_derivative = flatten_implicit_derivative(state, &implicit_derivative);
        let right_hand_side = base_implicit_derivative
            .iter()
            .copied()
            .map(|value| dt_seconds * value)
            .collect::<Vec<_>>();
        let mut tendency_evaluations = 2_u64;
        let initial_norm = norm(&right_hand_side);
        let (increment, iterations, relative_residual) = if initial_norm == 0.0 {
            (vec![0.0; right_hand_side.len()], 0, 0.0)
        } else {
            // The rejected V1 candidate declares a unit diagonal
            // preconditioner. Its application is therefore the identity in
            // both the Krylov basis and residual norm.
            let (increment, iterations, _krylov_residual) = gmres(
                &right_hand_side,
                self.maximum_linear_iterations,
                self.linear_relative_tolerance * 0.05,
                cancellation,
                |vector| {
                    let perturbed = state_with_implicit_increment(self.grid, state, vector)?;
                    let tendency = system.evaluate_linear_implicit_with_workspace(
                        &perturbed,
                        forcing,
                        ocean_edge_permeability,
                        month,
                        cancellation,
                        &mut workspace,
                    )?;
                    tendency_evaluations += 1;
                    let derivative =
                        ClimateDerivative::from_tendency(&perturbed, &tendency, cancellation)?;
                    let linear_action = flatten_implicit_derivative(&perturbed, &derivative)
                        .into_iter()
                        .zip(&base_implicit_derivative)
                        .map(|(perturbed, base)| perturbed - base)
                        .collect::<Vec<_>>();
                    Ok(vector
                        .iter()
                        .zip(linear_action)
                        .map(|(identity, linear)| identity - 0.5 * dt_seconds * linear)
                        .collect())
                },
            )?;
            let perturbed = state_with_implicit_increment(self.grid, state, &increment)?;
            let tendency = system.evaluate_linear_implicit_with_workspace(
                &perturbed,
                forcing,
                ocean_edge_permeability,
                month,
                cancellation,
                &mut workspace,
            )?;
            tendency_evaluations += 1;
            let derivative = ClimateDerivative::from_tendency(&perturbed, &tendency, cancellation)?;
            let residual = increment
                .iter()
                .zip(
                    flatten_implicit_derivative(&perturbed, &derivative)
                        .into_iter()
                        .zip(&base_implicit_derivative),
                )
                .zip(&right_hand_side)
                .map(|((identity, (perturbed, base)), right_hand_side)| {
                    identity - 0.5 * dt_seconds * (perturbed - base) - right_hand_side
                })
                .collect::<Vec<_>>();
            let actual_relative_residual = norm(&residual) / initial_norm;
            if !actual_relative_residual.is_finite()
                || actual_relative_residual > self.linear_relative_tolerance
            {
                return Err(ClimateIntegratorError::LinearSolveNotConverged {
                    iterations,
                    residual: actual_relative_residual,
                    tolerance: self.linear_relative_tolerance,
                });
            }
            (increment, iterations, actual_relative_residual)
        };

        let implicit_advanced = state_with_implicit_increment(self.grid, state, &increment)?;
        let mut explicit_non_humidity = explicit_derivative.clone();
        explicit_non_humidity.humidity.fill(0.0);
        if let Some(upper) = &mut explicit_non_humidity.upper_humidity {
            upper.fill(0.0);
        }
        let mut advanced = combine_state(
            self.grid,
            &implicit_advanced,
            &[(dt_seconds, &explicit_non_humidity)],
            cancellation,
        )?;
        let humidity_predictor = state
            .specific_humidity()
            .iter()
            .zip(&explicit_derivative.humidity)
            .map(|(value, tendency)| {
                (f64::from(*value) + dt_seconds * f64::from(*tendency)).max(0.0) as f32
            })
            .collect::<Vec<_>>();
        advanced
            .specific_humidity_mut()
            .copy_from_slice(&humidity_predictor);
        if let (Some(base_upper), Some(derivative_upper)) = (
            state.upper_specific_humidity(),
            explicit_derivative.upper_humidity.as_ref(),
        ) {
            let predictor = base_upper
                .iter()
                .zip(derivative_upper)
                .map(|(value, tendency)| {
                    (f64::from(*value) + dt_seconds * f64::from(*tendency)).max(0.0) as f32
                })
                .collect::<Vec<_>>();
            advanced
                .upper_specific_humidity_mut()
                .expect("C2 upper moisture")
                .copy_from_slice(&predictor);
        }
        let humidity_is_active = explicit_derivative
            .humidity
            .iter()
            .any(|value| *value != 0.0)
            || explicit_derivative
                .upper_humidity
                .iter()
                .flatten()
                .any(|value| *value != 0.0);
        if humidity_is_active {
            let predicted_tendency = system.evaluate_with_workspace_for_step(
                &advanced,
                forcing,
                ocean_edge_permeability,
                month,
                dt_seconds,
                cancellation,
                &mut workspace,
            )?;
            tendency_evaluations += 1;
            for (mean, (base, predicted)) in mean_precipitation_rate_mm_s.iter_mut().zip(
                base_tendency
                    .precipitation_rate_mm_s()
                    .iter()
                    .zip(predicted_tendency.precipitation_rate_mm_s()),
            ) {
                *mean = (0.5 * (f64::from(*base) + f64::from(*predicted))) as f32;
            }
            for (index, target) in advanced.specific_humidity_mut().iter_mut().enumerate() {
                *target = (f64::from(state.specific_humidity()[index])
                    + 0.5
                        * dt_seconds
                        * (f64::from(explicit_derivative.humidity[index])
                            + f64::from(
                                predicted_tendency.specific_humidity_tendency_s_inv()[index],
                            )))
                .max(0.0) as f32;
            }
            if let (Some(base_upper), Some(derivative_upper), Some(predicted_upper)) = (
                state.upper_specific_humidity(),
                explicit_derivative.upper_humidity.as_ref(),
                predicted_tendency.upper_specific_humidity_tendency_s_inv(),
            ) {
                for (index, target) in advanced
                    .upper_specific_humidity_mut()
                    .expect("C2 upper moisture")
                    .iter_mut()
                    .enumerate()
                {
                    *target = (f64::from(base_upper[index])
                        + 0.5
                            * dt_seconds
                            * (f64::from(derivative_upper[index])
                                + f64::from(predicted_upper[index])))
                    .max(0.0) as f32;
                }
            }
        }
        advanced.validate_against(self.grid)?;
        Ok(ClimateStepResult::new(
            advanced,
            ClimateIntegratorDiagnostics::imex(
                tendency_evaluations,
                iterations,
                if initial_norm > 0.0 { 1.0 } else { 0.0 },
                relative_residual,
                estimate_cfl(self.grid, state, dt_seconds, cancellation)?,
            ),
            mean_precipitation_rate_mm_s,
        ))
    }
}

fn flatten_implicit_derivative(
    state: &LayeredClimateState,
    derivative: &ClimateDerivative,
) -> Vec<f64> {
    let mut values = Vec::with_capacity(implicit_len(state));
    for role in state.active_roles() {
        let layer = derivative.layer(*role);
        values.extend(layer.height.iter().map(|value| f64::from(*value)));
        for velocity in &layer.velocity {
            values.extend(velocity.iter().map(|value| f64::from(*value)));
        }
        values.extend(layer.temperature.iter().map(|value| f64::from(*value)));
    }
    if let Some(deep) = &derivative.deep_temperature {
        values.extend(deep.iter().map(|value| f64::from(*value)));
    }
    values
}

fn implicit_len(state: &LayeredClimateState) -> usize {
    let active = state.active_roles().len();
    let deep = usize::from(state.deep_ocean_temperature_c().is_some());
    (5 * active + deep) * state.cell_count()
}

fn state_with_implicit_increment(
    grid: &CubedSphereGrid,
    base: &LayeredClimateState,
    increment: &[f64],
) -> Result<LayeredClimateState, ClimateIntegratorError> {
    if increment.len() != implicit_len(base) {
        return Err(ClimateIntegratorError::StateMismatch);
    }
    let mut result = base.clone();
    let mut offset = 0_usize;
    for role in base.active_roles() {
        for (target, original) in result
            .height_anomaly_m_mut(*role)
            .expect("active role")
            .iter_mut()
            .zip(base.height_anomaly_m(*role).expect("active role"))
        {
            *target = checked_f32(f64::from(*original) + increment[offset])?;
            offset += 1;
        }
        let mut velocity = Vec::with_capacity(base.cell_count());
        for original in base.velocity_m_s(*role).expect("active role") {
            let mut value = [0.0_f32; 3];
            for component in 0..3 {
                value[component] = checked_f32(f64::from(original[component]) + increment[offset])?;
                offset += 1;
            }
            velocity.push(value);
        }
        let tangent = CirculationOperators::new(grid)
            .tangentize(&velocity)
            .map_err(|error| {
                ClimateIntegratorError::Tendency(super::LayeredTendencyError::Operator(error))
            })?;
        result
            .velocity_m_s_mut(*role)
            .expect("active role")
            .copy_from_slice(&tangent);
        for (target, original) in result
            .temperature_c_mut(*role)
            .expect("active role")
            .iter_mut()
            .zip(base.temperature_c(*role).expect("active role"))
        {
            *target = checked_f32(f64::from(*original) + increment[offset])?;
            offset += 1;
        }
    }
    if let (Some(result_deep), Some(base_deep)) = (
        result.deep_ocean_temperature_c_mut(),
        base.deep_ocean_temperature_c(),
    ) {
        for (target, original) in result_deep.iter_mut().zip(base_deep) {
            *target = checked_f32(f64::from(*original) + increment[offset])?;
            offset += 1;
        }
    }
    debug_assert_eq!(offset, increment.len());
    result.validate_against(grid)?;
    Ok(result)
}

fn checked_f32(value: f64) -> Result<f32, ClimateIntegratorError> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        Err(ClimateIntegratorError::LinearSolveBreakdown)
    } else {
        Ok(value as f32)
    }
}

fn gmres<F>(
    right_hand_side: &[f64],
    maximum_iterations: u16,
    tolerance: f64,
    cancellation: &BuildCancellation,
    mut apply: F,
) -> Result<(Vec<f64>, u16, f64), ClimateIntegratorError>
where
    F: FnMut(&[f64]) -> Result<Vec<f64>, ClimateIntegratorError>,
{
    let beta = norm(right_hand_side);
    if beta == 0.0 {
        return Ok((vec![0.0; right_hand_side.len()], 0, 0.0));
    }
    let budget = usize::from(maximum_iterations);
    let mut basis: Vec<Vec<f64>> = Vec::with_capacity(budget + 1);
    basis.push(
        right_hand_side
            .iter()
            .map(|value| value / beta)
            .collect::<Vec<_>>(),
    );
    let mut hessenberg = vec![vec![0.0_f64; budget]; budget + 1];
    let mut cosines = vec![0.0_f64; budget];
    let mut sines = vec![0.0_f64; budget];
    let mut rotated_rhs = vec![0.0_f64; budget + 1];
    rotated_rhs[0] = beta;

    for column in 0..budget {
        if cancellation.is_cancelled() {
            return Err(ClimateIntegratorError::Cancelled);
        }
        let mut vector = apply(&basis[column])?;
        if vector.len() != right_hand_side.len() || vector.iter().any(|value| !value.is_finite()) {
            return Err(ClimateIntegratorError::LinearSolveBreakdown);
        }
        for row in 0..=column {
            let projection = dot(&vector, &basis[row]);
            hessenberg[row][column] = projection;
            for (value, basis_value) in vector.iter_mut().zip(&basis[row]) {
                *value -= projection * basis_value;
            }
        }
        hessenberg[column + 1][column] = norm(&vector);
        if hessenberg[column + 1][column] > f64::MIN_POSITIVE {
            let inverse = hessenberg[column + 1][column].recip();
            basis.push(
                vector
                    .into_iter()
                    .map(|value| value * inverse)
                    .collect::<Vec<_>>(),
            );
        } else {
            basis.push(vec![0.0; right_hand_side.len()]);
        }

        for row in 0..column {
            let upper = hessenberg[row][column];
            let lower = hessenberg[row + 1][column];
            hessenberg[row][column] = cosines[row] * upper + sines[row] * lower;
            hessenberg[row + 1][column] = -sines[row] * upper + cosines[row] * lower;
        }
        let diagonal = hessenberg[column][column];
        let subdiagonal = hessenberg[column + 1][column];
        let magnitude = diagonal.hypot(subdiagonal);
        if magnitude <= f64::MIN_POSITIVE {
            return Err(ClimateIntegratorError::LinearSolveBreakdown);
        }
        cosines[column] = diagonal / magnitude;
        sines[column] = subdiagonal / magnitude;
        hessenberg[column][column] = magnitude;
        hessenberg[column + 1][column] = 0.0;
        rotated_rhs[column + 1] = -sines[column] * rotated_rhs[column];
        rotated_rhs[column] *= cosines[column];
        let relative_residual = rotated_rhs[column + 1].abs() / beta;
        if relative_residual <= tolerance {
            let used = column + 1;
            let coefficients = back_substitute(&hessenberg, &rotated_rhs, used)?;
            let solution = combine_basis(&basis, &coefficients, right_hand_side.len());
            return Ok((solution, used as u16, relative_residual));
        }
    }
    let residual = rotated_rhs[budget].abs() / beta;
    Err(ClimateIntegratorError::LinearSolveNotConverged {
        iterations: maximum_iterations,
        residual,
        tolerance,
    })
}

fn back_substitute(
    matrix: &[Vec<f64>],
    right_hand_side: &[f64],
    count: usize,
) -> Result<Vec<f64>, ClimateIntegratorError> {
    let mut values = vec![0.0_f64; count];
    for row in (0..count).rev() {
        let known = (row + 1..count)
            .map(|column| matrix[row][column] * values[column])
            .sum::<f64>();
        let diagonal = matrix[row][row];
        if !diagonal.is_finite() || diagonal.abs() <= f64::MIN_POSITIVE {
            return Err(ClimateIntegratorError::LinearSolveBreakdown);
        }
        values[row] = (right_hand_side[row] - known) / diagonal;
    }
    Ok(values)
}

fn combine_basis(basis: &[Vec<f64>], coefficients: &[f64], length: usize) -> Vec<f64> {
    let mut solution = vec![0.0_f64; length];
    for (basis, coefficient) in basis.iter().zip(coefficients) {
        for (target, value) in solution.iter_mut().zip(basis) {
            *target += coefficient * value;
        }
    }
    solution
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum()
}

fn norm(values: &[f64]) -> f64 {
    dot(values, values).sqrt()
}
