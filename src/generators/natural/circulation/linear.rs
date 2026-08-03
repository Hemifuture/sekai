#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MatrixFreeSolve {
    pub(crate) values: Vec<f64>,
    pub(crate) iterations: u16,
    pub(crate) relative_residual: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MatrixFreeSolveError {
    InvalidInput,
    NumericalOverflow,
    Breakdown {
        iteration: u16,
    },
    NotConverged {
        iterations: u16,
        residual: f64,
        tolerance: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MatrixFreeSolveFailure<E> {
    Application(E),
    Solve(MatrixFreeSolveError),
}

pub(crate) fn solve_bicgstab<E>(
    initial: &[f64],
    right_hand_side: &[f64],
    max_iterations: u16,
    relative_tolerance: f64,
    mut apply: impl FnMut(&[f64], &mut [f64]) -> Result<(), E>,
) -> Result<MatrixFreeSolve, MatrixFreeSolveFailure<E>> {
    if initial.is_empty()
        || initial.len() != right_hand_side.len()
        || max_iterations == 0
        || !relative_tolerance.is_finite()
        || relative_tolerance <= 0.0
        || initial
            .iter()
            .chain(right_hand_side)
            .any(|value| !value.is_finite())
    {
        return Err(MatrixFreeSolveFailure::Solve(
            MatrixFreeSolveError::InvalidInput,
        ));
    }

    let count = initial.len();
    let mut values = initial.to_vec();
    let mut applied = vec![0.0_f64; count];
    apply(&values, &mut applied).map_err(MatrixFreeSolveFailure::Application)?;
    validate_finite(&applied)?;
    let mut residual = right_hand_side
        .iter()
        .zip(&applied)
        .map(|(right, applied)| right - applied)
        .collect::<Vec<_>>();
    let normalization = l2_norm(right_hand_side)
        .max(l2_norm(&applied))
        .max(f64::MIN_POSITIVE);
    let mut relative_residual = l2_norm(&residual) / normalization;
    if relative_residual <= relative_tolerance {
        return Ok(MatrixFreeSolve {
            values,
            iterations: 0,
            relative_residual,
        });
    }

    let shadow_residual = residual.clone();
    let mut search = vec![0.0_f64; count];
    let mut matrix_search = vec![0.0_f64; count];
    let mut intermediate = vec![0.0_f64; count];
    let mut matrix_intermediate = vec![0.0_f64; count];
    let mut previous_rho = 1.0_f64;
    let mut alpha = 1.0_f64;
    let mut omega = 1.0_f64;

    for iteration in 1..=max_iterations {
        let rho = vector_dot(&shadow_residual, &residual);
        ensure_denominator(rho, iteration)?;
        let beta = (rho / previous_rho) * (alpha / omega);
        for cell in 0..count {
            search[cell] = residual[cell] + beta * (search[cell] - omega * matrix_search[cell]);
        }
        apply(&search, &mut matrix_search).map_err(MatrixFreeSolveFailure::Application)?;
        validate_finite(&matrix_search)?;
        let alpha_denominator = vector_dot(&shadow_residual, &matrix_search);
        ensure_denominator(alpha_denominator, iteration)?;
        alpha = rho / alpha_denominator;
        for cell in 0..count {
            intermediate[cell] = residual[cell] - alpha * matrix_search[cell];
        }
        relative_residual = l2_norm(&intermediate) / normalization;
        if relative_residual <= relative_tolerance {
            for cell in 0..count {
                values[cell] += alpha * search[cell];
            }
            validate_finite(&values)?;
            return Ok(MatrixFreeSolve {
                values,
                iterations: iteration,
                relative_residual,
            });
        }

        apply(&intermediate, &mut matrix_intermediate)
            .map_err(MatrixFreeSolveFailure::Application)?;
        validate_finite(&matrix_intermediate)?;
        let omega_denominator = vector_dot(&matrix_intermediate, &matrix_intermediate);
        ensure_denominator(omega_denominator, iteration)?;
        omega = vector_dot(&matrix_intermediate, &intermediate) / omega_denominator;
        ensure_denominator(omega, iteration)?;
        for cell in 0..count {
            values[cell] += alpha * search[cell] + omega * intermediate[cell];
            residual[cell] = intermediate[cell] - omega * matrix_intermediate[cell];
        }
        validate_finite(&values)?;
        validate_finite(&residual)?;
        relative_residual = l2_norm(&residual) / normalization;
        if relative_residual <= relative_tolerance {
            return Ok(MatrixFreeSolve {
                values,
                iterations: iteration,
                relative_residual,
            });
        }
        previous_rho = rho;
    }

    Err(MatrixFreeSolveFailure::Solve(
        MatrixFreeSolveError::NotConverged {
            iterations: max_iterations,
            residual: relative_residual,
            tolerance: relative_tolerance,
        },
    ))
}

pub(crate) fn solve_gmres<E>(
    initial: &[f64],
    right_hand_side: &[f64],
    max_iterations: u16,
    restart: u16,
    relative_tolerance: f64,
    mut apply: impl FnMut(&[f64], &mut [f64]) -> Result<(), E>,
) -> Result<MatrixFreeSolve, MatrixFreeSolveFailure<E>> {
    if initial.is_empty()
        || initial.len() != right_hand_side.len()
        || max_iterations == 0
        || restart == 0
        || !relative_tolerance.is_finite()
        || relative_tolerance <= 0.0
        || initial
            .iter()
            .chain(right_hand_side)
            .any(|value| !value.is_finite())
    {
        return Err(MatrixFreeSolveFailure::Solve(
            MatrixFreeSolveError::InvalidInput,
        ));
    }

    let count = initial.len();
    let restart = usize::from(restart.min(max_iterations));
    let mut values = initial.to_vec();
    let mut applied = vec![0.0_f64; count];
    apply(&values, &mut applied).map_err(MatrixFreeSolveFailure::Application)?;
    validate_finite(&applied)?;
    let normalization = l2_norm(right_hand_side)
        .max(l2_norm(&applied))
        .max(f64::MIN_POSITIVE);
    let mut residual = right_hand_side
        .iter()
        .zip(&applied)
        .map(|(right, applied)| right - applied)
        .collect::<Vec<_>>();
    let mut relative_residual = l2_norm(&residual) / normalization;
    if relative_residual <= relative_tolerance {
        return Ok(MatrixFreeSolve {
            values,
            iterations: 0,
            relative_residual,
        });
    }

    let mut total_iterations = 0_u16;
    while total_iterations < max_iterations {
        let beta = l2_norm(&residual);
        ensure_denominator(beta, total_iterations.saturating_add(1))?;
        let mut basis = Vec::with_capacity(restart + 1);
        basis.push(
            residual
                .iter()
                .map(|value| value / beta)
                .collect::<Vec<_>>(),
        );
        let mut hessenberg = vec![vec![0.0_f64; restart]; restart + 1];
        let mut cosine = vec![0.0_f64; restart];
        let mut sine = vec![0.0_f64; restart];
        let mut projected_rhs = vec![0.0_f64; restart + 1];
        projected_rhs[0] = beta;
        let cycle_budget = restart.min(usize::from(max_iterations - total_iterations));
        let mut used_columns = 0_usize;

        for column in 0..cycle_budget {
            let mut candidate = vec![0.0_f64; count];
            apply(&basis[column], &mut candidate).map_err(MatrixFreeSolveFailure::Application)?;
            validate_finite(&candidate)?;
            for row in 0..=column {
                hessenberg[row][column] = vector_dot(&candidate, &basis[row]);
                for cell in 0..count {
                    candidate[cell] -= hessenberg[row][column] * basis[row][cell];
                }
            }
            hessenberg[column + 1][column] = l2_norm(&candidate);
            if hessenberg[column + 1][column] > f64::MIN_POSITIVE {
                let inverse_norm = hessenberg[column + 1][column].recip();
                basis.push(
                    candidate
                        .into_iter()
                        .map(|value| value * inverse_norm)
                        .collect(),
                );
            } else {
                basis.push(vec![0.0; count]);
            }

            for row in 0..column {
                let upper =
                    cosine[row] * hessenberg[row][column] + sine[row] * hessenberg[row + 1][column];
                let lower = -sine[row] * hessenberg[row][column]
                    + cosine[row] * hessenberg[row + 1][column];
                hessenberg[row][column] = upper;
                hessenberg[row + 1][column] = lower;
            }
            let magnitude = hessenberg[column][column].hypot(hessenberg[column + 1][column]);
            ensure_denominator(magnitude, total_iterations.saturating_add(1))?;
            cosine[column] = hessenberg[column][column] / magnitude;
            sine[column] = hessenberg[column + 1][column] / magnitude;
            hessenberg[column][column] = magnitude;
            hessenberg[column + 1][column] = 0.0;
            let upper_rhs =
                cosine[column] * projected_rhs[column] + sine[column] * projected_rhs[column + 1];
            let lower_rhs =
                -sine[column] * projected_rhs[column] + cosine[column] * projected_rhs[column + 1];
            projected_rhs[column] = upper_rhs;
            projected_rhs[column + 1] = lower_rhs;
            total_iterations += 1;
            used_columns = column + 1;
            relative_residual = lower_rhs.abs() / normalization;
            if relative_residual <= relative_tolerance
                || total_iterations >= max_iterations
                || basis[column + 1].iter().all(|value| *value == 0.0)
            {
                break;
            }
        }

        let coefficients =
            upper_triangular_solve(&hessenberg, &projected_rhs, used_columns, total_iterations)?;
        for column in 0..used_columns {
            for cell in 0..count {
                values[cell] += coefficients[column] * basis[column][cell];
            }
        }
        validate_finite(&values)?;
        apply(&values, &mut applied).map_err(MatrixFreeSolveFailure::Application)?;
        validate_finite(&applied)?;
        for cell in 0..count {
            residual[cell] = right_hand_side[cell] - applied[cell];
        }
        relative_residual = l2_norm(&residual) / normalization;
        if relative_residual <= relative_tolerance {
            return Ok(MatrixFreeSolve {
                values,
                iterations: total_iterations,
                relative_residual,
            });
        }
    }

    Err(MatrixFreeSolveFailure::Solve(
        MatrixFreeSolveError::NotConverged {
            iterations: max_iterations,
            residual: relative_residual,
            tolerance: relative_tolerance,
        },
    ))
}

fn upper_triangular_solve<E>(
    matrix: &[Vec<f64>],
    right_hand_side: &[f64],
    size: usize,
    iteration: u16,
) -> Result<Vec<f64>, MatrixFreeSolveFailure<E>> {
    let mut solution = vec![0.0_f64; size];
    for row in (0..size).rev() {
        let accumulated = ((row + 1)..size)
            .map(|column| matrix[row][column] * solution[column])
            .sum::<f64>();
        ensure_denominator(matrix[row][row], iteration)?;
        solution[row] = (right_hand_side[row] - accumulated) / matrix[row][row];
    }
    validate_finite(&solution)?;
    Ok(solution)
}

fn vector_dot(first: &[f64], second: &[f64]) -> f64 {
    first.iter().zip(second).map(|(a, b)| a * b).sum()
}

fn l2_norm(values: &[f64]) -> f64 {
    vector_dot(values, values).sqrt()
}

fn ensure_denominator<E>(value: f64, iteration: u16) -> Result<(), MatrixFreeSolveFailure<E>> {
    if !value.is_finite() || value.abs() <= f64::MIN_POSITIVE {
        return Err(MatrixFreeSolveFailure::Solve(
            MatrixFreeSolveError::Breakdown { iteration },
        ));
    }
    Ok(())
}

fn validate_finite<E>(values: &[f64]) -> Result<(), MatrixFreeSolveFailure<E>> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(MatrixFreeSolveFailure::Solve(
            MatrixFreeSolveError::NumericalOverflow,
        ));
    }
    Ok(())
}
