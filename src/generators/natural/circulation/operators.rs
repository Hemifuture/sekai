use thiserror::Error;

use super::{
    linear::{solve_bicgstab, MatrixFreeSolveError, MatrixFreeSolveFailure},
    math::{add, cross, dot, project_tangent, scale},
    CubedSphereGrid, SphericalEdge,
};

/// One shared finite-volume operator set for every circulation solver.
#[derive(Debug, Clone, Copy)]
pub struct CirculationOperators<'grid> {
    grid: &'grid CubedSphereGrid,
}

impl<'grid> CirculationOperators<'grid> {
    pub const fn new(grid: &'grid CubedSphereGrid) -> Self {
        Self { grid }
    }

    pub const fn grid(&self) -> &'grid CubedSphereGrid {
        self.grid
    }

    /// Computes a tangent Green-Gauss gradient with exact constant preservation.
    pub fn gradient(&self, scalar: &[f32]) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        self.gradient_impl(scalar, None)
    }

    /// Computes a tangent gradient while closing selected shared edges.
    pub fn gradient_with_permeability(
        &self,
        scalar: &[f32],
        edge_permeability: &[f32],
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        self.gradient_impl(scalar, Some(edge_permeability))
    }

    fn gradient_impl(
        &self,
        scalar: &[f32],
        edge_permeability: Option<&[f32]>,
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        validate_scalar_field("scalar", scalar, self.grid.cell_count())?;
        if let Some(permeability) = edge_permeability {
            validate_permeability(permeability, self.grid.edges().len())?;
        }
        Ok(self.gradient_f32_validated_impl(scalar, edge_permeability))
    }

    pub(crate) fn gradient_validated(&self, scalar: &[f32]) -> Vec<[f32; 3]> {
        self.gradient_f32_validated_impl(scalar, None)
    }

    pub(crate) fn gradient_with_permeability_validated(
        &self,
        scalar: &[f32],
        edge_permeability: &[f32],
    ) -> Vec<[f32; 3]> {
        self.gradient_f32_validated_impl(scalar, Some(edge_permeability))
    }

    fn gradient_f32_validated_impl(
        &self,
        scalar: &[f32],
        edge_permeability: Option<&[f32]>,
    ) -> Vec<[f32; 3]> {
        debug_assert_eq!(scalar.len(), self.grid.cell_count());
        let mut accumulated = vec![[0.0_f64; 3]; self.grid.cell_count()];
        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            let [first, second] = edge.cells();
            let first = *first as usize;
            let second = *second as usize;
            let first_value = f64::from(scalar[first]);
            let second_value = f64::from(scalar[second]);
            let edge_value = interpolate_scalar_f64(edge, first_value, second_value);
            let normal = edge.normal_from_first();
            let permeability = edge_permeability
                .map(|values| f64::from(values[edge_index]))
                .unwrap_or(1.0);
            let length = edge.length_m() * permeability;
            accumulate_vector(
                &mut accumulated[first],
                normal,
                (edge_value - first_value) * length,
            );
            accumulate_vector(
                &mut accumulated[second],
                normal,
                -(edge_value - second_value) * length,
            );
        }
        self.grid
            .cells()
            .iter()
            .zip(accumulated)
            .map(|(cell, value)| {
                let gradient =
                    project_tangent(scale(value, cell.area_m2().recip()), cell.center_unit());
                to_quantized_tangent_f32(gradient, cell.center_unit())
            })
            .collect()
    }

    pub(crate) fn gradient_f64_with_permeability(
        &self,
        scalar: &[f64],
        edge_permeability: &[f32],
    ) -> Result<Vec<[f64; 3]>, CirculationOperatorError> {
        self.gradient_f64_impl(scalar, Some(edge_permeability))
    }

    fn gradient_f64_impl(
        &self,
        scalar: &[f64],
        edge_permeability: Option<&[f32]>,
    ) -> Result<Vec<[f64; 3]>, CirculationOperatorError> {
        validate_scalar_field_f64("scalar", scalar, self.grid.cell_count())?;
        if let Some(permeability) = edge_permeability {
            validate_permeability(permeability, self.grid.edges().len())?;
        }
        let mut accumulated = vec![[0.0_f64; 3]; self.grid.cell_count()];
        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            let [first, second] = edge.cells();
            let first = *first as usize;
            let second = *second as usize;
            let edge_value = interpolate_scalar_f64(edge, scalar[first], scalar[second]);
            let normal = edge.normal_from_first();
            let permeability = edge_permeability
                .map(|values| f64::from(values[edge_index]))
                .unwrap_or(1.0);
            let length = edge.length_m() * permeability;
            accumulate_vector(
                &mut accumulated[first],
                normal,
                (edge_value - scalar[first]) * length,
            );
            accumulate_vector(
                &mut accumulated[second],
                normal,
                -(edge_value - scalar[second]) * length,
            );
        }

        Ok(self
            .grid
            .cells()
            .iter()
            .zip(accumulated)
            .map(|(cell, value)| {
                project_tangent(scale(value, cell.area_m2().recip()), cell.center_unit())
            })
            .collect())
    }

    /// Computes cell divergence from one canonical flux per shared edge.
    pub fn divergence(&self, velocity: &[[f32; 3]]) -> Result<Vec<f32>, CirculationOperatorError> {
        self.divergence_impl(velocity, None)
    }

    /// Computes divergence while closing selected shared edges.
    pub fn divergence_with_permeability(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
    ) -> Result<Vec<f32>, CirculationOperatorError> {
        self.divergence_impl(velocity, Some(edge_permeability))
    }

    fn divergence_impl(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: Option<&[f32]>,
    ) -> Result<Vec<f32>, CirculationOperatorError> {
        validate_vector_field("velocity", velocity, self.grid.cell_count())?;
        if let Some(permeability) = edge_permeability {
            validate_permeability(permeability, self.grid.edges().len())?;
        }
        Ok(self.divergence_f32_validated_impl(velocity, edge_permeability))
    }

    pub(crate) fn divergence_validated(&self, velocity: &[[f32; 3]]) -> Vec<f32> {
        self.divergence_f32_validated_impl(velocity, None)
    }

    pub(crate) fn divergence_with_permeability_validated(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
    ) -> Vec<f32> {
        self.divergence_f32_validated_impl(velocity, Some(edge_permeability))
    }

    fn divergence_f32_validated_impl(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: Option<&[f32]>,
    ) -> Vec<f32> {
        debug_assert_eq!(velocity.len(), self.grid.cell_count());
        let mut extensive_flux = vec![0.0_f64; self.grid.cell_count()];
        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            let [first, second] = edge.cells();
            let first = *first as usize;
            let second = *second as usize;
            let edge_velocity = interpolate_vector(edge, velocity[first], velocity[second]);
            let permeability = edge_permeability
                .map(|values| f64::from(values[edge_index]))
                .unwrap_or(1.0);
            let flux =
                dot(edge_velocity, edge.normal_from_first()) * edge.length_m() * permeability;
            extensive_flux[first] += flux;
            extensive_flux[second] -= flux;
        }
        self.grid
            .cells()
            .iter()
            .zip(extensive_flux)
            .map(|(cell, flux)| (flux / cell.area_m2()) as f32)
            .collect()
    }

    pub(crate) fn divergence_f64_with_permeability(
        &self,
        velocity: &[[f64; 3]],
        edge_permeability: &[f32],
    ) -> Result<Vec<f64>, CirculationOperatorError> {
        self.divergence_f64_impl(velocity, Some(edge_permeability))
    }

    fn divergence_f64_impl(
        &self,
        velocity: &[[f64; 3]],
        edge_permeability: Option<&[f32]>,
    ) -> Result<Vec<f64>, CirculationOperatorError> {
        validate_vector_field_f64("velocity", velocity, self.grid.cell_count())?;
        if let Some(permeability) = edge_permeability {
            validate_permeability(permeability, self.grid.edges().len())?;
        }
        let mut extensive_flux = vec![0.0_f64; self.grid.cell_count()];
        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            let [first, second] = edge.cells();
            let first = *first as usize;
            let second = *second as usize;
            let edge_velocity = interpolate_vector_f64(edge, velocity[first], velocity[second]);
            let permeability = edge_permeability
                .map(|values| f64::from(values[edge_index]))
                .unwrap_or(1.0);
            let flux =
                dot(edge_velocity, edge.normal_from_first()) * edge.length_m() * permeability;
            extensive_flux[first] += flux;
            extensive_flux[second] -= flux;
        }
        Ok(self
            .grid
            .cells()
            .iter()
            .zip(extensive_flux)
            .map(|(cell, flux)| flux / cell.area_m2())
            .collect())
    }

    /// Applies the local traditional Coriolis acceleration on the sphere.
    pub fn coriolis(
        &self,
        velocity: &[[f32; 3]],
        rotation_rate_rad_s: f64,
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        validate_vector_field("velocity", velocity, self.grid.cell_count())?;
        if !rotation_rate_rad_s.is_finite() {
            return Err(CirculationOperatorError::InvalidRotationRate {
                found: rotation_rate_rad_s,
            });
        }
        Ok(self.coriolis_validated(velocity, rotation_rate_rad_s))
    }

    pub(crate) fn coriolis_validated(
        &self,
        velocity: &[[f32; 3]],
        rotation_rate_rad_s: f64,
    ) -> Vec<[f32; 3]> {
        self.grid
            .cells()
            .iter()
            .zip(velocity)
            .map(|(cell, value)| {
                let radial = cell.center_unit();
                let tangent_velocity = project_tangent(to_f64_vector(*value), radial);
                let coriolis_parameter = 2.0 * rotation_rate_rad_s * radial[2];
                let acceleration = scale(cross(radial, tangent_velocity), -coriolis_parameter);
                to_quantized_tangent_f32(acceleration, radial)
            })
            .collect()
    }

    /// Removes radial components from a dense vector field.
    pub fn tangentize(
        &self,
        vectors: &[[f32; 3]],
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        validate_vector_field("vectors", vectors, self.grid.cell_count())?;
        Ok(self.tangentize_validated(vectors))
    }

    pub(crate) fn tangentize_validated(&self, vectors: &[[f32; 3]]) -> Vec<[f32; 3]> {
        self.grid
            .cells()
            .iter()
            .zip(vectors)
            .map(|(cell, value)| {
                to_quantized_tangent_f32(to_f64_vector(*value), cell.center_unit())
            })
            .collect()
    }

    /// Advances one cell-mean scalar using conservative first-order upwind fluxes.
    pub fn advect_scalar_conservative(
        &self,
        scalar: &[f32],
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        dt_seconds: f64,
    ) -> Result<ConservativeTransport, CirculationOperatorError> {
        validate_scalar_field("scalar", scalar, self.grid.cell_count())?;
        validate_vector_field("velocity", velocity, self.grid.cell_count())?;
        validate_permeability(edge_permeability, self.grid.edges().len())?;
        if !dt_seconds.is_finite() || dt_seconds < 0.0 {
            return Err(CirculationOperatorError::InvalidTimeStep { found: dt_seconds });
        }

        let mut extensive_delta = vec![0.0_f64; self.grid.cell_count()];
        for (edge, permeability) in self.grid.edges().iter().zip(edge_permeability) {
            let [first, second] = edge.cells();
            let first = *first as usize;
            let second = *second as usize;
            let edge_velocity = interpolate_vector(edge, velocity[first], velocity[second]);
            let volume_flux = dot(edge_velocity, edge.normal_from_first())
                * edge.length_m()
                * f64::from(*permeability);
            let upstream = if volume_flux >= 0.0 {
                scalar[first]
            } else {
                scalar[second]
            };
            let transported = volume_flux * f64::from(upstream) * dt_seconds;
            extensive_delta[first] -= transported;
            extensive_delta[second] += transported;
        }

        let mut values = Vec::with_capacity(self.grid.cell_count());
        for ((cell, original), delta) in self.grid.cells().iter().zip(scalar).zip(extensive_delta) {
            let value = f64::from(*original) + delta / cell.area_m2();
            if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
                return Err(CirculationOperatorError::NumericalOverflow);
            }
            values.push(value as f32);
        }

        let before = extensive_total(self.grid, scalar, false);
        let after = extensive_total(self.grid, &values, false);
        let scale = extensive_total(self.grid, scalar, true).max(f64::MIN_POSITIVE);
        Ok(ConservativeTransport {
            values,
            relative_mass_error: (after - before).abs() / scale,
        })
    }

    /// Advances a cell-mean mixing ratio with constant-preserving upwind inflow.
    pub fn advect_scalar_upwind_tracer(
        &self,
        scalar: &[f32],
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        dt_seconds: f64,
    ) -> Result<UpwindTracerTransport, CirculationOperatorError> {
        validate_scalar_field("scalar", scalar, self.grid.cell_count())?;
        validate_vector_field("velocity", velocity, self.grid.cell_count())?;
        validate_permeability(edge_permeability, self.grid.edges().len())?;
        if !dt_seconds.is_finite() || dt_seconds < 0.0 {
            return Err(CirculationOperatorError::InvalidTimeStep { found: dt_seconds });
        }

        let fluxes = steady_upwind_fluxes(self.grid, velocity, edge_permeability);
        self.advect_scalar_upwind_tracer_from_fluxes_validated(scalar, &fluxes, dt_seconds)
    }

    pub(crate) fn upwind_fluxes_validated(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
    ) -> Vec<UpwindFlux> {
        steady_upwind_fluxes(self.grid, velocity, edge_permeability)
    }

    pub(crate) fn advect_scalar_upwind_tracer_from_fluxes_validated(
        &self,
        scalar: &[f32],
        fluxes: &[UpwindFlux],
        dt_seconds: f64,
    ) -> Result<UpwindTracerTransport, CirculationOperatorError> {
        debug_assert_eq!(scalar.len(), self.grid.cell_count());
        let mut intensive_delta = vec![0.0_f64; self.grid.cell_count()];
        for flux in fluxes {
            intensive_delta[flux.receiver] += flux.magnitude_m2_s
                * (f64::from(scalar[flux.donor]) - f64::from(scalar[flux.receiver]))
                * dt_seconds
                / self.grid.cells()[flux.receiver].area_m2();
        }
        let mut values = Vec::with_capacity(self.grid.cell_count());
        for (original, delta) in scalar.iter().zip(intensive_delta) {
            let value = f64::from(*original) + delta;
            if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
                return Err(CirculationOperatorError::NumericalOverflow);
            }
            values.push(value as f32);
        }
        let before = extensive_total(self.grid, scalar, false);
        let after = extensive_total(self.grid, &values, false);
        let scale = extensive_total(self.grid, scalar, true).max(f64::MIN_POSITIVE);
        Ok(UpwindTracerTransport {
            values,
            relative_mass_error: (after - before).abs() / scale,
        })
    }

    /// Solves the stationary constant-preserving upwind tracer equation
    /// `0 = transport(value) + source - sink_rate * value` with BiCGSTAB.
    #[allow(clippy::too_many_arguments)]
    pub fn solve_steady_upwind_tracer_source(
        &self,
        initial: &[f32],
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        sink_rate_s_inv: &[f32],
        source_per_s: &[f32],
        max_iterations: u16,
        relative_tolerance: f64,
    ) -> Result<SteadyTransportSolve, CirculationOperatorError> {
        let count = self.grid.cell_count();
        validate_scalar_field("initial", initial, count)?;
        validate_vector_field("velocity", velocity, count)?;
        validate_permeability(edge_permeability, self.grid.edges().len())?;
        validate_scalar_field("sink_rate_s_inv", sink_rate_s_inv, count)?;
        validate_scalar_field("source_per_s", source_per_s, count)?;
        for (index, rate) in sink_rate_s_inv.iter().copied().enumerate() {
            if rate < 0.0 {
                return Err(CirculationOperatorError::NegativeSinkRate { index, found: rate });
            }
        }
        if max_iterations == 0 {
            return Err(CirculationOperatorError::InvalidIterationBudget {
                found: max_iterations,
            });
        }
        if !relative_tolerance.is_finite() || relative_tolerance <= 0.0 {
            return Err(CirculationOperatorError::InvalidLinearTolerance {
                found: relative_tolerance,
            });
        }

        let inverse_areas = self
            .grid
            .cells()
            .iter()
            .map(|cell| cell.area_m2().recip())
            .collect::<Vec<_>>();
        let sink_rate_s_inv = sink_rate_s_inv
            .iter()
            .map(|value| f64::from(*value))
            .collect::<Vec<_>>();
        let fluxes = steady_upwind_fluxes(self.grid, velocity, edge_permeability);
        let right_hand_side = source_per_s
            .iter()
            .map(|value| f64::from(*value))
            .collect::<Vec<_>>();
        let initial = initial
            .iter()
            .map(|value| f64::from(*value))
            .collect::<Vec<_>>();
        let solved = solve_bicgstab(
            &initial,
            &right_hand_side,
            max_iterations,
            relative_tolerance,
            |values, output| {
                apply_steady_transport_matrix(
                    values,
                    output,
                    &sink_rate_s_inv,
                    &inverse_areas,
                    &fluxes,
                );
                Ok::<(), CirculationOperatorError>(())
            },
        )
        .map_err(|failure| match failure {
            MatrixFreeSolveFailure::Application(error) => error,
            MatrixFreeSolveFailure::Solve(MatrixFreeSolveError::InvalidInput) => {
                CirculationOperatorError::InvalidLinearTolerance {
                    found: relative_tolerance,
                }
            }
            MatrixFreeSolveFailure::Solve(MatrixFreeSolveError::NumericalOverflow) => {
                CirculationOperatorError::NumericalOverflow
            }
            MatrixFreeSolveFailure::Solve(MatrixFreeSolveError::Breakdown { iteration }) => {
                CirculationOperatorError::LinearSolveBreakdown { iteration }
            }
            MatrixFreeSolveFailure::Solve(MatrixFreeSolveError::NotConverged {
                iterations,
                residual,
                tolerance,
            }) => CirculationOperatorError::LinearSolveNotConverged {
                iterations,
                residual,
                tolerance,
            },
        })?;
        steady_transport_result(
            solved.values,
            solved.iterations.max(1),
            solved.relative_residual,
        )
    }
}

/// Result of one conservative transport step.
#[derive(Debug, Clone, PartialEq)]
pub struct ConservativeTransport {
    values: Vec<f32>,
    relative_mass_error: f64,
}

impl ConservativeTransport {
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub const fn relative_mass_error(&self) -> f64 {
        self.relative_mass_error
    }

    pub fn into_values(self) -> Vec<f32> {
        self.values
    }
}

/// One explicit constant-preserving upwind tracer update.
#[derive(Debug, Clone, PartialEq)]
pub struct UpwindTracerTransport {
    values: Vec<f32>,
    relative_mass_error: f64,
}

impl UpwindTracerTransport {
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub const fn relative_mass_error(&self) -> f64 {
        self.relative_mass_error
    }

    pub fn into_values(self) -> Vec<f32> {
        self.values
    }
}

/// Converged stationary scalar field and its discrete linear-solve diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct SteadyTransportSolve {
    values: Vec<f32>,
    iterations: u16,
    relative_residual: f64,
}

impl SteadyTransportSolve {
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub const fn iterations(&self) -> u16 {
        self.iterations
    }

    pub const fn relative_residual(&self) -> f64 {
        self.relative_residual
    }

    pub fn into_values(self) -> Vec<f32> {
        self.values
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct UpwindFlux {
    donor: usize,
    receiver: usize,
    magnitude_m2_s: f64,
}

fn steady_upwind_fluxes(
    grid: &CubedSphereGrid,
    velocity: &[[f32; 3]],
    edge_permeability: &[f32],
) -> Vec<UpwindFlux> {
    grid.edges()
        .iter()
        .zip(edge_permeability)
        .filter_map(|(edge, permeability)| {
            let [first, second] = edge.cells();
            let first = *first as usize;
            let second = *second as usize;
            let signed_flux = dot(
                interpolate_vector(edge, velocity[first], velocity[second]),
                edge.normal_from_first(),
            ) * edge.length_m()
                * f64::from(*permeability);
            if signed_flux > 0.0 {
                Some(UpwindFlux {
                    donor: first,
                    receiver: second,
                    magnitude_m2_s: signed_flux,
                })
            } else if signed_flux < 0.0 {
                Some(UpwindFlux {
                    donor: second,
                    receiver: first,
                    magnitude_m2_s: -signed_flux,
                })
            } else {
                None
            }
        })
        .collect()
}

fn apply_steady_transport_matrix(
    values: &[f64],
    output: &mut [f64],
    sink_rate_s_inv: &[f64],
    inverse_areas: &[f64],
    fluxes: &[UpwindFlux],
) {
    for ((output, value), sink) in output.iter_mut().zip(values).zip(sink_rate_s_inv) {
        *output = sink * value;
    }
    for flux in fluxes {
        let rate = flux.magnitude_m2_s * inverse_areas[flux.receiver];
        output[flux.receiver] += rate * (values[flux.receiver] - values[flux.donor]);
    }
}

fn steady_transport_result(
    values: Vec<f64>,
    iterations: u16,
    relative_residual: f64,
) -> Result<SteadyTransportSolve, CirculationOperatorError> {
    let mut converted = Vec::with_capacity(values.len());
    for value in values {
        if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
            return Err(CirculationOperatorError::NumericalOverflow);
        }
        converted.push(value as f32);
    }
    Ok(SteadyTransportSolve {
        values: converted,
        iterations,
        relative_residual,
    })
}

fn interpolate_scalar_f64(edge: &SphericalEdge, first: f64, second: f64) -> f64 {
    let distances = edge.center_distances_to_midpoint_m();
    (first * distances[1] + second * distances[0]) / (distances[0] + distances[1])
}

fn interpolate_vector(edge: &SphericalEdge, first: [f32; 3], second: [f32; 3]) -> [f64; 3] {
    interpolate_vector_f64(edge, to_f64_vector(first), to_f64_vector(second))
}

fn interpolate_vector_f64(edge: &SphericalEdge, first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    let distances = edge.center_distances_to_midpoint_m();
    let denominator = distances[0] + distances[1];
    let first_weight = distances[1] / denominator;
    let second_weight = distances[0] / denominator;
    let interpolated = add(scale(first, first_weight), scale(second, second_weight));
    project_tangent(interpolated, edge.midpoint_unit())
}

fn accumulate_vector(target: &mut [f64; 3], direction: [f64; 3], magnitude: f64) {
    for component in 0..3 {
        target[component] += direction[component] * magnitude;
    }
}

fn to_f64_vector(value: [f32; 3]) -> [f64; 3] {
    [
        f64::from(value[0]),
        f64::from(value[1]),
        f64::from(value[2]),
    ]
}

fn to_f32_vector(value: [f64; 3]) -> [f32; 3] {
    [value[0] as f32, value[1] as f32, value[2] as f32]
}

fn to_quantized_tangent_f32(value: [f64; 3], radial: [f64; 3]) -> [f32; 3] {
    let mut quantized = to_f32_vector(project_tangent(value, radial));
    for _ in 0..2 {
        for component in 0..3 {
            if radial[component].abs() <= f64::EPSILON {
                continue;
            }
            let error = dot(to_f64_vector(quantized), radial);
            let corrected = (f64::from(quantized[component]) - error / radial[component]) as f32;
            let candidates = [
                quantized[component],
                corrected,
                next_up_f32(corrected),
                next_down_f32(corrected),
            ];
            let mut best = quantized[component];
            let mut best_error = error.abs();
            for candidate in candidates {
                let mut trial = quantized;
                trial[component] = candidate;
                let trial_error = dot(to_f64_vector(trial), radial).abs();
                if trial_error < best_error {
                    best = candidate;
                    best_error = trial_error;
                }
            }
            quantized[component] = best;
        }
    }
    quantized
}

fn next_up_f32(value: f32) -> f32 {
    if value.is_nan() || value == f32::INFINITY {
        return value;
    }
    if value == 0.0 {
        return f32::from_bits(1);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits + 1 } else { bits - 1 })
}

fn next_down_f32(value: f32) -> f32 {
    if value.is_nan() || value == f32::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return -f32::from_bits(1);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}

fn extensive_total(grid: &CubedSphereGrid, values: &[f32], absolute: bool) -> f64 {
    let mut sum = 0.0_f64;
    let mut correction = 0.0_f64;
    for (cell, value) in grid.cells().iter().zip(values) {
        let scalar = if absolute { value.abs() } else { *value };
        let term = cell.area_m2() * f64::from(scalar);
        let adjusted = term - correction;
        let next = sum + adjusted;
        correction = (next - sum) - adjusted;
        sum = next;
    }
    sum
}

fn validate_scalar_field(
    field: &'static str,
    values: &[f32],
    expected: usize,
) -> Result<(), CirculationOperatorError> {
    validate_length(field, values.len(), expected)?;
    for (index, value) in values.iter().enumerate() {
        if !value.is_finite() {
            return Err(CirculationOperatorError::NonFiniteScalarValue { field, index });
        }
    }
    Ok(())
}

fn validate_scalar_field_f64(
    field: &'static str,
    values: &[f64],
    expected: usize,
) -> Result<(), CirculationOperatorError> {
    validate_length(field, values.len(), expected)?;
    for (index, value) in values.iter().enumerate() {
        if !value.is_finite() {
            return Err(CirculationOperatorError::NonFiniteScalarValue { field, index });
        }
    }
    Ok(())
}

fn validate_vector_field(
    field: &'static str,
    values: &[[f32; 3]],
    expected: usize,
) -> Result<(), CirculationOperatorError> {
    validate_length(field, values.len(), expected)?;
    for (index, value) in values.iter().enumerate() {
        for (component, scalar) in value.iter().enumerate() {
            if !scalar.is_finite() {
                return Err(CirculationOperatorError::NonFiniteVectorValue {
                    field,
                    index,
                    component,
                });
            }
        }
    }
    Ok(())
}

fn validate_vector_field_f64(
    field: &'static str,
    values: &[[f64; 3]],
    expected: usize,
) -> Result<(), CirculationOperatorError> {
    validate_length(field, values.len(), expected)?;
    for (index, value) in values.iter().enumerate() {
        for (component, scalar) in value.iter().enumerate() {
            if !scalar.is_finite() {
                return Err(CirculationOperatorError::NonFiniteVectorValue {
                    field,
                    index,
                    component,
                });
            }
        }
    }
    Ok(())
}

fn validate_permeability(values: &[f32], expected: usize) -> Result<(), CirculationOperatorError> {
    validate_length("edge_permeability", values.len(), expected)?;
    for (index, value) in values.iter().copied().enumerate() {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(CirculationOperatorError::PermeabilityOutOfRange {
                index,
                found: value,
            });
        }
    }
    Ok(())
}

fn validate_length(
    field: &'static str,
    found: usize,
    expected: usize,
) -> Result<(), CirculationOperatorError> {
    if found != expected {
        return Err(CirculationOperatorError::FieldLengthMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

/// Errors returned before or during a shared finite-volume operation.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CirculationOperatorError {
    #[error("operator field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("operator scalar field {field} is non-finite at index {index}")]
    NonFiniteScalarValue { field: &'static str, index: usize },
    #[error("operator vector field {field} is non-finite at index {index}, component {component}")]
    NonFiniteVectorValue {
        field: &'static str,
        index: usize,
        component: usize,
    },
    #[error("edge permeability {found} at index {index} is outside 0..=1")]
    PermeabilityOutOfRange { index: usize, found: f32 },
    #[error("rotation rate {found} must be finite")]
    InvalidRotationRate { found: f64 },
    #[error("transport time step {found} must be finite and nonnegative")]
    InvalidTimeStep { found: f64 },
    #[error("finite-volume update overflowed the dense f32 state")]
    NumericalOverflow,
    #[error("steady transport sink rate {found} at index {index} must be nonnegative")]
    NegativeSinkRate { index: usize, found: f32 },
    #[error("steady transport iteration budget {found} must be nonzero")]
    InvalidIterationBudget { found: u16 },
    #[error("steady transport relative tolerance {found} must be finite and positive")]
    InvalidLinearTolerance { found: f64 },
    #[error("steady transport linear solve broke down at iteration {iteration}")]
    LinearSolveBreakdown { iteration: u16 },
    #[error(
        "steady transport linear solve did not converge after {iterations} iterations: residual {residual} exceeds tolerance {tolerance}"
    )]
    LinearSolveNotConverged {
        iterations: u16,
        residual: f64,
        tolerance: f64,
    },
}
