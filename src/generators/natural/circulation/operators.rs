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

    /// Advances a cell-mean scalar with monotone piecewise-linear donor
    /// reconstruction and one paired flux per shared edge. A supplied
    /// workspace owns every cell/edge-sized temporary allocation.
    pub fn advect_scalar_monotone_second_order_into<'workspace>(
        &self,
        scalar: &[f32],
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        dt_seconds: f64,
        enforce_nonnegative: bool,
        workspace: &'workspace mut SecondOrderTransportWorkspace,
    ) -> Result<SecondOrderTransport<'workspace>, CirculationOperatorError> {
        validate_scalar_field("second_order_scalar", scalar, self.grid.cell_count())?;
        validate_vector_field("second_order_velocity", velocity, self.grid.cell_count())?;
        validate_permeability(edge_permeability, self.grid.edges().len())?;
        if !dt_seconds.is_finite() || dt_seconds < 0.0 {
            return Err(CirculationOperatorError::InvalidTimeStep { found: dt_seconds });
        }
        if enforce_nonnegative {
            if let Some((index, found)) = scalar
                .iter()
                .copied()
                .enumerate()
                .find(|(_, value)| *value < 0.0)
            {
                return Err(CirculationOperatorError::NegativePositiveTransportInput {
                    index,
                    found,
                });
            }
        }
        workspace.validate_for_grid(self.grid)?;
        workspace.reset();

        // The same Green-Gauss gradient used by the shared pressure operator,
        // written directly into reusable f64 scratch storage.
        for edge in self.grid.edges() {
            let [first, second] = *edge.cells();
            let first = first as usize;
            let second = second as usize;
            let first_value = f64::from(scalar[first]);
            let second_value = f64::from(scalar[second]);
            let edge_value = interpolate_scalar_f64(edge, first_value, second_value);
            accumulate_vector(
                &mut workspace.gradients[first],
                edge.normal_from_first(),
                (edge_value - first_value) * edge.length_m(),
            );
            accumulate_vector(
                &mut workspace.gradients[second],
                edge.normal_from_first(),
                -(edge_value - second_value) * edge.length_m(),
            );
        }
        for (index, cell) in self.grid.cells().iter().enumerate() {
            workspace.gradients[index] = project_tangent(
                scale(workspace.gradients[index], cell.area_m2().recip()),
                cell.center_unit(),
            );
            let mut minimum = scalar[index];
            let mut maximum = scalar[index];
            for neighbor in cell.neighbors() {
                minimum = minimum.min(scalar[*neighbor as usize]);
                maximum = maximum.max(scalar[*neighbor as usize]);
            }
            workspace.local_min[index] = f64::from(minimum);
            workspace.local_max[index] = f64::from(maximum);
        }

        // Barth-Jespersen limiter: every owner-side edge reconstruction stays
        // inside that cell's one-ring extrema before a donor is selected.
        for edge in self.grid.edges() {
            for owner in 0..2 {
                let cell = edge.cells()[owner] as usize;
                let displacement = edge_displacement_m(self.grid, edge, owner);
                let increment = dot(workspace.gradients[cell], displacement);
                let center = f64::from(scalar[cell]);
                let ratio = if increment > 0.0 {
                    (workspace.local_max[cell] - center) / increment
                } else if increment < 0.0 {
                    (workspace.local_min[cell] - center) / increment
                } else {
                    1.0
                };
                workspace.limiter[cell] = workspace.limiter[cell].min(ratio.clamp(0.0, 1.0));
            }
        }

        for (edge_index, (edge, permeability)) in
            self.grid.edges().iter().zip(edge_permeability).enumerate()
        {
            let [first, second] = *edge.cells();
            let first = first as usize;
            let second = second as usize;
            let signed_volume_flux = dot(
                interpolate_vector(edge, velocity[first], velocity[second]),
                edge.normal_from_first(),
            ) * edge.length_m()
                * f64::from(*permeability);
            let (donor, owner) = if signed_volume_flux >= 0.0 {
                (first, 0)
            } else {
                (second, 1)
            };
            let displacement = edge_displacement_m(self.grid, edge, owner);
            let reconstructed = f64::from(scalar[donor])
                + workspace.limiter[donor] * dot(workspace.gradients[donor], displacement);
            let mut face_value =
                reconstructed.clamp(workspace.local_min[donor], workspace.local_max[donor]);
            if enforce_nonnegative {
                face_value = face_value.max(0.0);
            }
            workspace.edge_volume_flux_m2_s[edge_index] = signed_volume_flux;
            workspace.edge_face_value[edge_index] = face_value;
            workspace.outgoing_amount[donor] +=
                signed_volume_flux.abs() * face_value.abs() * dt_seconds;
        }

        let mut positivity_scaled_cells = 0_usize;
        if enforce_nonnegative {
            for (index, cell) in self.grid.cells().iter().enumerate() {
                let available = cell.area_m2() * f64::from(scalar[index]);
                let outgoing = workspace.outgoing_amount[index];
                if outgoing > available && outgoing > 0.0 {
                    workspace.outgoing_scale[index] = (available / outgoing).clamp(0.0, 1.0);
                    positivity_scaled_cells += 1;
                }
            }
        }

        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            let signed_flux = workspace.edge_volume_flux_m2_s[edge_index];
            let [first, second] = *edge.cells();
            let first = first as usize;
            let second = second as usize;
            let donor = if signed_flux >= 0.0 { first } else { second };
            let transported = signed_flux
                * workspace.edge_face_value[edge_index]
                * workspace.outgoing_scale[donor]
                * dt_seconds;
            workspace.extensive_delta[first] -= transported;
            workspace.extensive_delta[second] += transported;
        }
        for (index, cell) in self.grid.cells().iter().enumerate() {
            let value =
                f64::from(scalar[index]) + workspace.extensive_delta[index] / cell.area_m2();
            if !value.is_finite() {
                return Err(CirculationOperatorError::NumericalOverflow);
            }
            let lower = if enforce_nonnegative {
                workspace.local_min[index].max(0.0)
            } else {
                workspace.local_min[index]
            };
            workspace.bounded_values[index] =
                value.clamp(lower, workspace.local_max[index].max(lower));
        }
        let before = extensive_total(self.grid, scalar, false);
        conservative_bound_redistribution(self.grid, before, enforce_nonnegative, workspace)?;
        for (target, value) in workspace.output.iter_mut().zip(&workspace.bounded_values) {
            if *value < f64::from(f32::MIN) || *value > f64::from(f32::MAX) {
                return Err(CirculationOperatorError::NumericalOverflow);
            }
            *target = *value as f32;
        }
        let after = extensive_total(self.grid, &workspace.output, false);
        let mass_scale = extensive_total(self.grid, scalar, true).max(f64::MIN_POSITIVE);
        Ok(SecondOrderTransport {
            values: &workspace.output,
            relative_mass_error: (after - before).abs() / mass_scale,
            positivity_scaled_cells,
        })
    }

    /// Advances an intensive scalar with constant-preserving upwind inflow.
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

    /// Advances a mixing ratio consistently with a linearized shallow-water layer.
    ///
    /// The stored carrier is `layer_amount = H + eta`, while every edge transports the
    /// uniform linearization depth `reference_transport_depth`. The same signed edge
    /// flux advances both the stored layer and `layer * mixing_ratio`, exactly matching
    /// the solver's `-H * div(u)` continuity equation.
    pub fn advect_linearized_layer_mixing_ratio_conservative(
        &self,
        layer_amount: &[f32],
        reference_transport_depth: f32,
        mixing_ratio: &[f32],
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        dt_seconds: f64,
    ) -> Result<UpwindTracerTransport, CirculationOperatorError> {
        validate_scalar_field("layer_amount", layer_amount, self.grid.cell_count())?;
        for (index, value) in layer_amount.iter().copied().enumerate() {
            if value <= 0.0 {
                return Err(CirculationOperatorError::NonPositiveLayerAmount {
                    index,
                    found: value,
                });
            }
        }
        validate_positive_layer_amount("reference_transport_depth", reference_transport_depth)?;
        validate_scalar_field("mixing_ratio", mixing_ratio, self.grid.cell_count())?;
        validate_vector_field("velocity", velocity, self.grid.cell_count())?;
        validate_permeability(edge_permeability, self.grid.edges().len())?;
        if !dt_seconds.is_finite() || dt_seconds < 0.0 {
            return Err(CirculationOperatorError::InvalidTimeStep { found: dt_seconds });
        }

        let fluxes = steady_upwind_fluxes(self.grid, velocity, edge_permeability);
        self.advect_linearized_layer_mixing_ratio_from_fluxes_validated(
            layer_amount,
            reference_transport_depth,
            mixing_ratio,
            &fluxes,
            dt_seconds,
        )
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
            layer_amounts: vec![1.0; self.grid.cell_count()],
            relative_mass_error: (after - before).abs() / scale,
        })
    }

    pub(crate) fn advect_linearized_layer_mixing_ratio_from_fluxes_validated(
        &self,
        layer_amount: &[f32],
        reference_transport_depth: f32,
        scalar: &[f32],
        fluxes: &[UpwindFlux],
        dt_seconds: f64,
    ) -> Result<UpwindTracerTransport, CirculationOperatorError> {
        debug_assert_eq!(layer_amount.len(), self.grid.cell_count());
        debug_assert!(layer_amount.iter().all(|value| *value > 0.0));
        debug_assert!(reference_transport_depth.is_finite());
        debug_assert!(reference_transport_depth > 0.0);
        debug_assert_eq!(scalar.len(), self.grid.cell_count());
        let mut layer_delta = vec![0.0_f64; self.grid.cell_count()];
        let mut tracer_delta = vec![0.0_f64; self.grid.cell_count()];
        let mut outgoing_layer = vec![0.0_f64; self.grid.cell_count()];
        for flux in fluxes {
            let transported_layer =
                flux.magnitude_m2_s * f64::from(reference_transport_depth) * dt_seconds;
            let transported_tracer = transported_layer * f64::from(scalar[flux.donor]);
            outgoing_layer[flux.donor] += transported_layer;
            layer_delta[flux.donor] -= transported_layer;
            layer_delta[flux.receiver] += transported_layer;
            tracer_delta[flux.donor] -= transported_tracer;
            tracer_delta[flux.receiver] += transported_tracer;
        }
        let mut values = Vec::with_capacity(self.grid.cell_count());
        let mut transported_layer_amounts = Vec::with_capacity(self.grid.cell_count());
        for (index, ((((cell, original_layer), original), layer_delta), tracer_delta)) in self
            .grid
            .cells()
            .iter()
            .zip(layer_amount)
            .zip(scalar)
            .zip(layer_delta)
            .zip(tracer_delta)
            .enumerate()
        {
            let initial_extensive_layer = cell.area_m2() * f64::from(*original_layer);
            if outgoing_layer[index] > initial_extensive_layer {
                return Err(CirculationOperatorError::TransportCflViolation { index });
            }
            let transported_layer = f64::from(*original_layer) + layer_delta / cell.area_m2();
            let layer_tracer =
                f64::from(*original_layer) * f64::from(*original) + tracer_delta / cell.area_m2();
            if !transported_layer.is_finite() || transported_layer <= 0.0 {
                return Err(CirculationOperatorError::TransportCflViolation { index });
            }
            if transported_layer > f64::from(f32::MAX) {
                return Err(CirculationOperatorError::NumericalOverflow);
            }
            let value = layer_tracer / transported_layer;
            if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
                return Err(CirculationOperatorError::NumericalOverflow);
            }
            values.push(value as f32);
            transported_layer_amounts.push(transported_layer as f32);
        }
        let before = extensive_layer_tracer_total(self.grid, layer_amount, scalar, false);
        let after =
            extensive_layer_tracer_total(self.grid, &transported_layer_amounts, &values, false);
        let scale = extensive_layer_tracer_total(self.grid, layer_amount, scalar, true)
            .max(f64::MIN_POSITIVE);
        Ok(UpwindTracerTransport {
            values,
            layer_amounts: transported_layer_amounts,
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

        let fluxes = steady_upwind_fluxes(self.grid, velocity, edge_permeability);
        self.solve_steady_upwind_tracer_source_from_fluxes(
            initial,
            &fluxes,
            sink_rate_s_inv,
            source_per_s,
            max_iterations,
            relative_tolerance,
        )
    }

    /// Solves the stationary mixing-ratio equation paired with the linearized
    /// shallow-water continuity flux `H * u`.
    #[allow(clippy::too_many_arguments)]
    pub fn solve_steady_linearized_layer_mixing_ratio_source(
        &self,
        initial: &[f32],
        layer_amount: &[f32],
        reference_transport_depth: f32,
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        sink_rate_s_inv: &[f32],
        source_per_s: &[f32],
        max_iterations: u16,
        relative_tolerance: f64,
    ) -> Result<SteadyTransportSolve, CirculationOperatorError> {
        let count = self.grid.cell_count();
        validate_scalar_field("initial", initial, count)?;
        validate_scalar_field("layer_amount", layer_amount, count)?;
        for (index, value) in layer_amount.iter().copied().enumerate() {
            if value <= 0.0 {
                return Err(CirculationOperatorError::NonPositiveLayerAmount {
                    index,
                    found: value,
                });
            }
        }
        validate_positive_layer_amount("reference_transport_depth", reference_transport_depth)?;
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

        let mut fluxes = steady_upwind_fluxes(self.grid, velocity, edge_permeability);
        for flux in &mut fluxes {
            flux.magnitude_m2_s *=
                f64::from(reference_transport_depth) / f64::from(layer_amount[flux.receiver]);
        }
        self.solve_steady_upwind_tracer_source_from_fluxes(
            initial,
            &fluxes,
            sink_rate_s_inv,
            source_per_s,
            max_iterations,
            relative_tolerance,
        )
    }

    fn solve_steady_upwind_tracer_source_from_fluxes(
        &self,
        initial: &[f32],
        fluxes: &[UpwindFlux],
        sink_rate_s_inv: &[f32],
        source_per_s: &[f32],
        max_iterations: u16,
        relative_tolerance: f64,
    ) -> Result<SteadyTransportSolve, CirculationOperatorError> {
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
                    fluxes,
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

/// Reusable cell/edge scratch storage for monotone piecewise-linear transport.
///
/// The workspace is tied to one grid shape. Reusing it across time steps keeps
/// the hot transport path allocation-free while still validating accidental
/// use with a different grid.
#[derive(Debug, Clone)]
pub struct SecondOrderTransportWorkspace {
    cell_count: usize,
    edge_count: usize,
    gradients: Vec<[f64; 3]>,
    local_min: Vec<f64>,
    local_max: Vec<f64>,
    limiter: Vec<f64>,
    edge_volume_flux_m2_s: Vec<f64>,
    edge_face_value: Vec<f64>,
    outgoing_amount: Vec<f64>,
    outgoing_scale: Vec<f64>,
    extensive_delta: Vec<f64>,
    bounded_values: Vec<f64>,
    output: Vec<f32>,
}

impl SecondOrderTransportWorkspace {
    pub fn for_grid(grid: &CubedSphereGrid) -> Self {
        let cell_count = grid.cell_count();
        let edge_count = grid.edges().len();
        Self {
            cell_count,
            edge_count,
            gradients: vec![[0.0; 3]; cell_count],
            local_min: vec![0.0; cell_count],
            local_max: vec![0.0; cell_count],
            limiter: vec![1.0; cell_count],
            edge_volume_flux_m2_s: vec![0.0; edge_count],
            edge_face_value: vec![0.0; edge_count],
            outgoing_amount: vec![0.0; cell_count],
            outgoing_scale: vec![1.0; cell_count],
            extensive_delta: vec![0.0; cell_count],
            bounded_values: vec![0.0; cell_count],
            output: vec![0.0; cell_count],
        }
    }

    /// Capacity fingerprint used by tests and diagnostics to prove reuse.
    pub fn allocation_signature(&self) -> [usize; 11] {
        [
            self.gradients.capacity(),
            self.local_min.capacity(),
            self.local_max.capacity(),
            self.limiter.capacity(),
            self.edge_volume_flux_m2_s.capacity(),
            self.edge_face_value.capacity(),
            self.outgoing_amount.capacity(),
            self.outgoing_scale.capacity(),
            self.extensive_delta.capacity(),
            self.bounded_values.capacity(),
            self.output.capacity(),
        ]
    }

    fn validate_for_grid(&self, grid: &CubedSphereGrid) -> Result<(), CirculationOperatorError> {
        let expected_cells = grid.cell_count();
        let expected_edges = grid.edges().len();
        if self.cell_count != expected_cells
            || self.edge_count != expected_edges
            || self.gradients.len() != expected_cells
            || self.local_min.len() != expected_cells
            || self.local_max.len() != expected_cells
            || self.limiter.len() != expected_cells
            || self.edge_volume_flux_m2_s.len() != expected_edges
            || self.edge_face_value.len() != expected_edges
            || self.outgoing_amount.len() != expected_cells
            || self.outgoing_scale.len() != expected_cells
            || self.extensive_delta.len() != expected_cells
            || self.bounded_values.len() != expected_cells
            || self.output.len() != expected_cells
        {
            return Err(CirculationOperatorError::WorkspaceGridMismatch {
                expected_cells,
                found_cells: self.cell_count,
                expected_edges,
                found_edges: self.edge_count,
            });
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.gradients.fill([0.0; 3]);
        self.local_min.fill(0.0);
        self.local_max.fill(0.0);
        self.limiter.fill(1.0);
        self.edge_volume_flux_m2_s.fill(0.0);
        self.edge_face_value.fill(0.0);
        self.outgoing_amount.fill(0.0);
        self.outgoing_scale.fill(1.0);
        self.extensive_delta.fill(0.0);
        self.bounded_values.fill(0.0);
        self.output.fill(0.0);
    }
}

/// Borrowed result of a second-order update stored in its caller workspace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SecondOrderTransport<'workspace> {
    values: &'workspace [f32],
    relative_mass_error: f64,
    positivity_scaled_cells: usize,
}

impl SecondOrderTransport<'_> {
    pub const fn values(&self) -> &[f32] {
        self.values
    }

    pub const fn relative_mass_error(&self) -> f64 {
        self.relative_mass_error
    }

    pub const fn positivity_scaled_cells(&self) -> usize {
        self.positivity_scaled_cells
    }
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
    layer_amounts: Vec<f32>,
    relative_mass_error: f64,
}

impl UpwindTracerTransport {
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub const fn relative_mass_error(&self) -> f64 {
        self.relative_mass_error
    }

    /// Transported carrier-layer amount in the units supplied by the caller.
    pub fn layer_amounts(&self) -> &[f32] {
        &self.layer_amounts
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
    if first == second {
        return first;
    }
    let distances = edge.center_distances_to_midpoint_m();
    (first * distances[1] + second * distances[0]) / (distances[0] + distances[1])
}

fn edge_displacement_m(grid: &CubedSphereGrid, edge: &SphericalEdge, owner: usize) -> [f64; 3] {
    let cell = edge.cells()[owner] as usize;
    let radial = grid.cells()[cell].center_unit();
    let chord = add(edge.midpoint_unit(), scale(radial, -1.0));
    let toward_midpoint = project_tangent(chord, radial);
    let norm = dot(toward_midpoint, toward_midpoint).sqrt();
    if norm <= f64::MIN_POSITIVE {
        [0.0; 3]
    } else {
        scale(
            toward_midpoint,
            edge.center_distances_to_midpoint_m()[owner] / norm,
        )
    }
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

fn conservative_bound_redistribution(
    grid: &CubedSphereGrid,
    target_total: f64,
    enforce_nonnegative: bool,
    workspace: &mut SecondOrderTransportWorkspace,
) -> Result<(), CirculationOperatorError> {
    let bounded_total = extensive_total_f64(grid, &workspace.bounded_values);
    let correction = target_total - bounded_total;
    let roundoff = 128.0 * f64::EPSILON * target_total.abs().max(bounded_total.abs()).max(1.0);
    if correction.abs() <= roundoff {
        return Ok(());
    }

    let adding = correction > 0.0;
    let capacity = grid
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let lower = if enforce_nonnegative {
                workspace.local_min[index].max(0.0)
            } else {
                workspace.local_min[index]
            };
            let bound = if adding {
                workspace.local_max[index].max(lower)
            } else {
                lower
            };
            cell.area_m2() * (bound - workspace.bounded_values[index]).abs()
        })
        .sum::<f64>();
    if !capacity.is_finite() || capacity + roundoff < correction.abs() {
        return Err(CirculationOperatorError::NumericalOverflow);
    }
    if capacity <= roundoff {
        return Ok(());
    }

    let fraction = (correction.abs() / capacity).clamp(0.0, 1.0);
    for (index, cell) in grid.cells().iter().enumerate() {
        let lower = if enforce_nonnegative {
            workspace.local_min[index].max(0.0)
        } else {
            workspace.local_min[index]
        };
        let bound = if adding {
            workspace.local_max[index].max(lower)
        } else {
            lower
        };
        let extensive_capacity = cell.area_m2() * (bound - workspace.bounded_values[index]).abs();
        let signed_adjustment = extensive_capacity * fraction * correction.signum();
        workspace.bounded_values[index] += signed_adjustment / cell.area_m2();
    }

    // Remove the last few summation ulps without allocating or crossing a
    // local bound. This deterministic cell-order sweep normally exits after
    // one cell; it also covers nearly saturated bound sets.
    let mut residual = target_total - extensive_total_f64(grid, &workspace.bounded_values);
    for (index, cell) in grid.cells().iter().enumerate() {
        if residual.abs() <= roundoff {
            break;
        }
        let lower = if enforce_nonnegative {
            workspace.local_min[index].max(0.0)
        } else {
            workspace.local_min[index]
        };
        let upper = workspace.local_max[index].max(lower);
        let available = if residual > 0.0 {
            cell.area_m2() * (upper - workspace.bounded_values[index])
        } else {
            cell.area_m2() * (workspace.bounded_values[index] - lower)
        };
        let adjustment = residual.abs().min(available.max(0.0)) * residual.signum();
        workspace.bounded_values[index] += adjustment / cell.area_m2();
        residual -= adjustment;
    }
    if residual.abs() > roundoff {
        return Err(CirculationOperatorError::NumericalOverflow);
    }
    Ok(())
}

fn extensive_total_f64(grid: &CubedSphereGrid, values: &[f64]) -> f64 {
    let mut sum = 0.0_f64;
    let mut correction = 0.0_f64;
    for (cell, value) in grid.cells().iter().zip(values) {
        let term = cell.area_m2() * value;
        let adjusted = term - correction;
        let next = sum + adjusted;
        correction = (next - sum) - adjusted;
        sum = next;
    }
    sum
}

fn extensive_layer_tracer_total(
    grid: &CubedSphereGrid,
    layer_amounts: &[f32],
    values: &[f32],
    absolute: bool,
) -> f64 {
    let mut sum = 0.0_f64;
    let mut correction = 0.0_f64;
    for ((cell, layer), value) in grid.cells().iter().zip(layer_amounts).zip(values) {
        let value = if absolute { value.abs() } else { *value };
        let term = cell.area_m2() * f64::from(*layer) * f64::from(value);
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

fn validate_positive_layer_amount(
    field: &'static str,
    value: f32,
) -> Result<(), CirculationOperatorError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(CirculationOperatorError::InvalidReferenceLayerAmount {
            field,
            found: value,
        });
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
    #[error("nonnegative transport input is negative at cell {index}: {found}")]
    NegativePositiveTransportInput { index: usize, found: f32 },
    #[error(
        "second-order workspace belongs to {found_cells} cells/{found_edges} edges; expected {expected_cells} cells/{expected_edges} edges"
    )]
    WorkspaceGridMismatch {
        expected_cells: usize,
        found_cells: usize,
        expected_edges: usize,
        found_edges: usize,
    },
    #[error("finite-volume update overflowed the dense f32 state")]
    NumericalOverflow,
    #[error("tracer carrier layer amount {found} at cell {index} must be positive")]
    NonPositiveLayerAmount { index: usize, found: f32 },
    #[error("linearized transport layer amount {field}={found} must be finite and positive")]
    InvalidReferenceLayerAmount { field: &'static str, found: f32 },
    #[error("upwind tracer transport exhausted the donor layer at cell {index}")]
    TransportCflViolation { index: usize },
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
