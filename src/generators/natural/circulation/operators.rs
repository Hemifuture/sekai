use thiserror::Error;

use super::{
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
        let mut accumulated = vec![[0.0_f64; 3]; self.grid.cell_count()];
        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            let [first, second] = edge.cells();
            let first = *first as usize;
            let second = *second as usize;
            let edge_value = interpolate_scalar(edge, scalar[first], scalar[second]);
            let normal = edge.normal_from_first();
            let permeability = edge_permeability
                .map(|values| f64::from(values[edge_index]))
                .unwrap_or(1.0);
            let length = edge.length_m() * permeability;
            accumulate_vector(
                &mut accumulated[first],
                normal,
                (edge_value - f64::from(scalar[first])) * length,
            );
            accumulate_vector(
                &mut accumulated[second],
                normal,
                -(edge_value - f64::from(scalar[second])) * length,
            );
        }

        Ok(self
            .grid
            .cells()
            .iter()
            .zip(accumulated)
            .map(|(cell, value)| {
                let tangent =
                    project_tangent(scale(value, cell.area_m2().recip()), cell.center_unit());
                to_f32_vector(tangent)
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
        Ok(self
            .grid
            .cells()
            .iter()
            .zip(extensive_flux)
            .map(|(cell, flux)| (flux / cell.area_m2()) as f32)
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
        Ok(self
            .grid
            .cells()
            .iter()
            .zip(velocity)
            .map(|(cell, value)| {
                let radial = cell.center_unit();
                let tangent_velocity = project_tangent(to_f64_vector(*value), radial);
                let coriolis_parameter = 2.0 * rotation_rate_rad_s * radial[2];
                let acceleration = scale(cross(radial, tangent_velocity), -coriolis_parameter);
                to_f32_vector(project_tangent(acceleration, radial))
            })
            .collect())
    }

    /// Removes radial components from a dense vector field.
    pub fn tangentize(
        &self,
        vectors: &[[f32; 3]],
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        validate_vector_field("vectors", vectors, self.grid.cell_count())?;
        Ok(self
            .grid
            .cells()
            .iter()
            .zip(vectors)
            .map(|(cell, value)| {
                to_f32_vector(project_tangent(to_f64_vector(*value), cell.center_unit()))
            })
            .collect())
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

fn interpolate_scalar(edge: &SphericalEdge, first: f32, second: f32) -> f64 {
    let distances = edge.center_distances_to_midpoint_m();
    (f64::from(first) * distances[1] + f64::from(second) * distances[0])
        / (distances[0] + distances[1])
}

fn interpolate_vector(edge: &SphericalEdge, first: [f32; 3], second: [f32; 3]) -> [f64; 3] {
    let distances = edge.center_distances_to_midpoint_m();
    let denominator = distances[0] + distances[1];
    let first_weight = distances[1] / denominator;
    let second_weight = distances[0] / denominator;
    let interpolated = add(
        scale(to_f64_vector(first), first_weight),
        scale(to_f64_vector(second), second_weight),
    );
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
}
