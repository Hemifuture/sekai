use thiserror::Error;

use crate::engine::BuildCancellation;

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
        self.gradient_impl(scalar, None, None)
    }

    /// Cancellation-aware tangent Green-Gauss gradient used while constructing
    /// climate forcing on the work grid.
    pub fn gradient_cancellable(
        &self,
        scalar: &[f32],
        cancellation: &BuildCancellation,
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        self.gradient_impl(scalar, None, Some(cancellation))
    }

    /// Computes a tangent gradient while closing selected shared edges.
    pub fn gradient_with_permeability(
        &self,
        scalar: &[f32],
        edge_permeability: &[f32],
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        self.gradient_impl(scalar, Some(edge_permeability), None)
    }

    pub fn gradient_with_permeability_cancellable(
        &self,
        scalar: &[f32],
        edge_permeability: &[f32],
        cancellation: &BuildCancellation,
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        self.gradient_impl(scalar, Some(edge_permeability), Some(cancellation))
    }

    fn gradient_impl(
        &self,
        scalar: &[f32],
        edge_permeability: Option<&[f32]>,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        check_operator_cancelled(cancellation)?;
        validate_scalar_field("scalar", scalar, self.grid.cell_count())?;
        if let Some(permeability) = edge_permeability {
            validate_permeability(permeability, self.grid.edges().len())?;
        }
        self.gradient_f32_validated_impl(scalar, edge_permeability, cancellation)
    }

    pub(crate) fn gradient_validated(&self, scalar: &[f32]) -> Vec<[f32; 3]> {
        self.gradient_f32_validated_impl(scalar, None, None)
            .expect("uncancellable validated gradient cannot fail")
    }

    pub(crate) fn gradient_with_permeability_validated(
        &self,
        scalar: &[f32],
        edge_permeability: &[f32],
    ) -> Vec<[f32; 3]> {
        self.gradient_f32_validated_impl(scalar, Some(edge_permeability), None)
            .expect("uncancellable validated gradient cannot fail")
    }

    fn gradient_f32_validated_impl(
        &self,
        scalar: &[f32],
        edge_permeability: Option<&[f32]>,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        debug_assert_eq!(scalar.len(), self.grid.cell_count());
        let mut accumulated = vec![[0.0_f64; 3]; self.grid.cell_count()];
        let mut result = vec![[0.0_f32; 3]; self.grid.cell_count()];
        self.gradient_f32_into_validated_impl(
            scalar,
            edge_permeability,
            &mut accumulated,
            &mut result,
            cancellation,
        )?;
        Ok(result)
    }

    fn gradient_f32_into_validated_impl(
        &self,
        scalar: &[f32],
        edge_permeability: Option<&[f32]>,
        accumulated: &mut [[f64; 3]],
        result: &mut [[f32; 3]],
        cancellation: Option<&BuildCancellation>,
    ) -> Result<(), CirculationOperatorError> {
        debug_assert_eq!(scalar.len(), self.grid.cell_count());
        debug_assert_eq!(accumulated.len(), self.grid.cell_count());
        debug_assert_eq!(result.len(), self.grid.cell_count());
        accumulated.fill([0.0; 3]);
        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            poll_operator_cancelled(edge_index, cancellation)?;
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
        for (index, ((cell, value), target)) in self
            .grid
            .cells()
            .iter()
            .zip(accumulated)
            .zip(result)
            .enumerate()
        {
            poll_operator_cancelled(index, cancellation)?;
            let gradient =
                project_tangent(scale(*value, cell.area_m2().recip()), cell.center_unit());
            *target = to_quantized_tangent_f32(gradient, cell.center_unit());
        }
        check_operator_cancelled(cancellation)?;
        Ok(())
    }

    pub(crate) fn gradient_into_cancellable_validated(
        &self,
        scalar: &[f32],
        edge_permeability: &[f32],
        output: &mut [[f32; 3]],
        workspace: &mut SecondOrderTransportWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<(), CirculationOperatorError> {
        debug_assert_eq!(edge_permeability.len(), self.grid.edges().len());
        debug_assert_eq!(workspace.cell_count, self.grid.cell_count());
        debug_assert_eq!(workspace.edge_count, self.grid.edges().len());
        self.gradient_f32_into_validated_impl(
            scalar,
            Some(edge_permeability),
            &mut workspace.gradients,
            output,
            Some(cancellation),
        )
    }

    /// Fuses the two finite-volume edge traversals needed by the fast
    /// pressure-gradient and donor-upwind layer-continuity operators.
    ///
    /// Each accumulator preserves the same canonical edge order and f64
    /// arithmetic as its standalone production operator; only the shared
    /// geometry/permeability traversal is removed.
    ///
    /// The gradient output feeds one transient RK stage acceleration and is
    /// never published, so it is written with the plain f32 cast of the exact
    /// f64 tangent rather than the iterative representable-vector correction
    /// that the public operators keep for final fields. Measured on Draft
    /// seed 42 (2026-09-02, milestone A1): the correction was 15 % of the P4
    /// solve with no effect on cycle counts or residuals.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn gradient_and_donor_layer_thickness_tendency_into_cancellable_validated(
        &self,
        height_anomaly_m: &[f32],
        velocity_m_s: &[[f32; 3]],
        edge_permeability: &[f32],
        reference_thickness_m: f64,
        gradient_output: &mut [[f32; 3]],
        thickness_tendency_m_s: &mut [f64],
        workspace: &mut SecondOrderTransportWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<(), CirculationOperatorError> {
        debug_assert_eq!(height_anomaly_m.len(), self.grid.cell_count());
        debug_assert_eq!(velocity_m_s.len(), self.grid.cell_count());
        debug_assert_eq!(edge_permeability.len(), self.grid.edges().len());
        debug_assert!(reference_thickness_m.is_finite() && reference_thickness_m > 0.0);
        debug_assert_eq!(gradient_output.len(), self.grid.cell_count());
        debug_assert_eq!(thickness_tendency_m_s.len(), self.grid.cell_count());
        debug_assert_eq!(workspace.cell_count, self.grid.cell_count());
        debug_assert_eq!(workspace.edge_count, self.grid.edges().len());
        check_operator_cancelled(Some(cancellation))?;
        workspace.gradients.fill([0.0; 3]);
        thickness_tendency_m_s.fill(0.0);
        for (edge_index, (edge, permeability)) in
            self.grid.edges().iter().zip(edge_permeability).enumerate()
        {
            poll_operator_cancelled(edge_index, Some(cancellation))?;
            let [first, second] = *edge.cells();
            let first = first as usize;
            let second = second as usize;
            let first_value = f64::from(height_anomaly_m[first]);
            let second_value = f64::from(height_anomaly_m[second]);
            let edge_value = interpolate_scalar_f64(edge, first_value, second_value);
            let normal = edge.normal_from_first();
            let permeability = f64::from(*permeability);
            let edge_length_m = edge.length_m();
            let length = edge_length_m * permeability;
            accumulate_vector(
                &mut workspace.gradients[first],
                normal,
                (edge_value - first_value) * length,
            );
            accumulate_vector(
                &mut workspace.gradients[second],
                normal,
                -(edge_value - second_value) * length,
            );

            if permeability > 0.0 {
                let normal_velocity_m_s = dot(
                    interpolate_vector(edge, velocity_m_s[first], velocity_m_s[second]),
                    normal,
                );
                let donor = if normal_velocity_m_s >= 0.0 {
                    first
                } else {
                    second
                };
                let donor_thickness_m =
                    (reference_thickness_m + f64::from(height_anomaly_m[donor])).max(0.0);
                let amount_rate_m3_s =
                    normal_velocity_m_s * edge_length_m * permeability * donor_thickness_m;
                thickness_tendency_m_s[first] -=
                    amount_rate_m3_s / self.grid.cells()[first].area_m2();
                thickness_tendency_m_s[second] +=
                    amount_rate_m3_s / self.grid.cells()[second].area_m2();
            }
        }
        for (index, ((cell, value), target)) in self
            .grid
            .cells()
            .iter()
            .zip(&workspace.gradients)
            .zip(gradient_output)
            .enumerate()
        {
            poll_operator_cancelled(index, Some(cancellation))?;
            let gradient =
                project_tangent(scale(*value, cell.area_m2().recip()), cell.center_unit());
            *target = to_f32_vector(gradient);
        }
        check_operator_cancelled(Some(cancellation))
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
        self.divergence_impl(velocity, None, None)
    }

    /// Computes divergence while closing selected shared edges.
    pub fn divergence_with_permeability(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
    ) -> Result<Vec<f32>, CirculationOperatorError> {
        self.divergence_impl(velocity, Some(edge_permeability), None)
    }

    pub fn divergence_with_permeability_cancellable(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        cancellation: &BuildCancellation,
    ) -> Result<Vec<f32>, CirculationOperatorError> {
        self.divergence_impl(velocity, Some(edge_permeability), Some(cancellation))
    }

    fn divergence_impl(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: Option<&[f32]>,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<Vec<f32>, CirculationOperatorError> {
        check_operator_cancelled(cancellation)?;
        validate_vector_field("velocity", velocity, self.grid.cell_count())?;
        if let Some(permeability) = edge_permeability {
            validate_permeability(permeability, self.grid.edges().len())?;
        }
        self.divergence_f32_validated_impl(velocity, edge_permeability, cancellation)
    }

    pub(crate) fn divergence_validated(&self, velocity: &[[f32; 3]]) -> Vec<f32> {
        self.divergence_f32_validated_impl(velocity, None, None)
            .expect("uncancellable validated divergence cannot fail")
    }

    pub(crate) fn divergence_with_permeability_validated(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
    ) -> Vec<f32> {
        self.divergence_f32_validated_impl(velocity, Some(edge_permeability), None)
            .expect("uncancellable validated divergence cannot fail")
    }

    fn divergence_f32_validated_impl(
        &self,
        velocity: &[[f32; 3]],
        edge_permeability: Option<&[f32]>,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<Vec<f32>, CirculationOperatorError> {
        debug_assert_eq!(velocity.len(), self.grid.cell_count());
        let mut extensive_flux = vec![0.0_f64; self.grid.cell_count()];
        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            poll_operator_cancelled(edge_index, cancellation)?;
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
        let mut result = Vec::with_capacity(self.grid.cell_count());
        for (index, (cell, flux)) in self.grid.cells().iter().zip(extensive_flux).enumerate() {
            poll_operator_cancelled(index, cancellation)?;
            result.push((flux / cell.area_m2()) as f32);
        }
        check_operator_cancelled(cancellation)?;
        Ok(result)
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
        self.coriolis_impl(velocity, rotation_rate_rad_s, None)
    }

    pub fn coriolis_cancellable(
        &self,
        velocity: &[[f32; 3]],
        rotation_rate_rad_s: f64,
        cancellation: &BuildCancellation,
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        self.coriolis_impl(velocity, rotation_rate_rad_s, Some(cancellation))
    }

    fn coriolis_impl(
        &self,
        velocity: &[[f32; 3]],
        rotation_rate_rad_s: f64,
        cancellation: Option<&BuildCancellation>,
    ) -> Result<Vec<[f32; 3]>, CirculationOperatorError> {
        check_operator_cancelled(cancellation)?;
        validate_vector_field("velocity", velocity, self.grid.cell_count())?;
        if !rotation_rate_rad_s.is_finite() {
            return Err(CirculationOperatorError::InvalidRotationRate {
                found: rotation_rate_rad_s,
            });
        }
        let mut result = Vec::with_capacity(self.grid.cell_count());
        for (index, (cell, value)) in self.grid.cells().iter().zip(velocity).enumerate() {
            poll_operator_cancelled(index, cancellation)?;
            let _ = cell;
            result.push(self.coriolis_cell_validated(index, *value, rotation_rate_rad_s));
        }
        check_operator_cancelled(cancellation)?;
        Ok(result)
    }

    #[inline]
    pub(crate) fn coriolis_cell_validated(
        &self,
        cell: usize,
        velocity: [f32; 3],
        rotation_rate_rad_s: f64,
    ) -> [f32; 3] {
        let (acceleration, radial) =
            self.coriolis_cell_f64_validated(cell, velocity, rotation_rate_rad_s);
        to_quantized_tangent_f32(acceleration, radial)
    }

    #[inline]
    pub(crate) fn coriolis_cell_projected_validated(
        &self,
        cell: usize,
        velocity: [f32; 3],
        rotation_rate_rad_s: f64,
    ) -> [f32; 3] {
        let (acceleration, _) =
            self.coriolis_cell_f64_validated(cell, velocity, rotation_rate_rad_s);
        to_f32_vector(acceleration)
    }

    #[inline]
    fn coriolis_cell_f64_validated(
        &self,
        cell: usize,
        velocity: [f32; 3],
        rotation_rate_rad_s: f64,
    ) -> ([f64; 3], [f64; 3]) {
        debug_assert!(cell < self.grid.cell_count());
        let radial = self.grid.cells()[cell].center_unit();
        let tangent_velocity = project_tangent(to_f64_vector(velocity), radial);
        let coriolis_parameter = 2.0 * rotation_rate_rad_s * radial[2];
        let acceleration = scale(cross(radial, tangent_velocity), -coriolis_parameter);
        (acceleration, radial)
    }

    pub(crate) fn coriolis_validated(
        &self,
        velocity: &[[f32; 3]],
        rotation_rate_rad_s: f64,
    ) -> Vec<[f32; 3]> {
        self.coriolis_impl(velocity, rotation_rate_rad_s, None)
            .expect("uncancellable validated Coriolis operation cannot fail")
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
        vectors
            .iter()
            .enumerate()
            .map(|(cell, value)| self.tangentize_cell_validated(cell, *value))
            .collect()
    }

    #[inline]
    pub(crate) fn tangentize_cell_validated(&self, cell: usize, value: [f32; 3]) -> [f32; 3] {
        debug_assert!(cell < self.grid.cell_count());
        to_quantized_tangent_f32(to_f64_vector(value), self.grid.cells()[cell].center_unit())
    }

    /// Orthogonally projects one transient RK stage vector before its next
    /// tendency evaluation. Final/public operator paths retain the stricter
    /// representable-vector correction above.
    #[inline]
    pub(crate) fn project_tangent_cell_validated(&self, cell: usize, value: [f32; 3]) -> [f32; 3] {
        debug_assert!(cell < self.grid.cell_count());
        to_f32_vector(project_tangent(
            to_f64_vector(value),
            self.grid.cells()[cell].center_unit(),
        ))
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
        self.advect_scalar_monotone_second_order_with_cancel(
            scalar,
            velocity,
            edge_permeability,
            dt_seconds,
            enforce_nonnegative,
            workspace,
            || false,
        )
    }

    /// Cancellation-aware form used by long-running climate solves.
    #[allow(clippy::too_many_arguments)]
    pub fn advect_scalar_monotone_second_order_into_cancellable<'workspace>(
        &self,
        scalar: &[f32],
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        dt_seconds: f64,
        enforce_nonnegative: bool,
        workspace: &'workspace mut SecondOrderTransportWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<SecondOrderTransport<'workspace>, CirculationOperatorError> {
        self.advect_scalar_monotone_second_order_with_cancel(
            scalar,
            velocity,
            edge_permeability,
            dt_seconds,
            enforce_nonnegative,
            workspace,
            || cancellation.is_cancelled(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn advect_scalar_monotone_second_order_with_cancel<'workspace, F>(
        &self,
        scalar: &[f32],
        velocity: &[[f32; 3]],
        edge_permeability: &[f32],
        dt_seconds: f64,
        enforce_nonnegative: bool,
        workspace: &'workspace mut SecondOrderTransportWorkspace,
        mut cancelled: F,
    ) -> Result<SecondOrderTransport<'workspace>, CirculationOperatorError>
    where
        F: FnMut() -> bool,
    {
        if cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
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
        label_open_components(
            self.grid,
            edge_permeability,
            &mut workspace.component_root,
            &mut cancelled,
        )?;

        // The same Green-Gauss gradient used by the shared pressure operator,
        // written directly into reusable f64 scratch storage. Closed edges are
        // absent from both the stencil and the flux graph, so disconnected
        // basins remain numerically independent.
        for (edge_index, (edge, permeability)) in
            self.grid.edges().iter().zip(edge_permeability).enumerate()
        {
            if edge_index % 256 == 0 && cancelled() {
                return Err(CirculationOperatorError::Cancelled);
            }
            if *permeability <= 0.0 {
                continue;
            }
            let [first, second] = *edge.cells();
            let first = first as usize;
            let second = second as usize;
            let first_value = f64::from(scalar[first]);
            let second_value = f64::from(scalar[second]);
            let edge_value = interpolate_scalar_f64(edge, first_value, second_value);
            accumulate_vector(
                &mut workspace.gradients[first],
                edge.normal_from_first(),
                (edge_value - first_value) * edge.length_m() * f64::from(*permeability),
            );
            accumulate_vector(
                &mut workspace.gradients[second],
                edge.normal_from_first(),
                -(edge_value - second_value) * edge.length_m() * f64::from(*permeability),
            );
        }
        for (index, cell) in self.grid.cells().iter().enumerate() {
            if index % 256 == 0 && cancelled() {
                return Err(CirculationOperatorError::Cancelled);
            }
            workspace.gradients[index] = project_tangent(
                scale(workspace.gradients[index], cell.area_m2().recip()),
                cell.center_unit(),
            );
            let mut minimum = scalar[index];
            let mut maximum = scalar[index];
            for edge_id in cell.edges() {
                if edge_permeability[*edge_id as usize] <= 0.0 {
                    continue;
                }
                let edge = &self.grid.edges()[*edge_id as usize];
                let [first, second] = *edge.cells();
                let neighbor = if first as usize == index {
                    second
                } else {
                    first
                } as usize;
                minimum = minimum.min(scalar[neighbor]);
                maximum = maximum.max(scalar[neighbor]);
            }
            workspace.local_min[index] = f64::from(minimum);
            workspace.local_max[index] = f64::from(maximum);
        }

        // Barth-Jespersen limiter: every owner-side edge reconstruction stays
        // inside that cell's one-ring extrema before a donor is selected.
        for (edge_index, (edge, permeability)) in
            self.grid.edges().iter().zip(edge_permeability).enumerate()
        {
            if edge_index % 256 == 0 && cancelled() {
                return Err(CirculationOperatorError::Cancelled);
            }
            if *permeability <= 0.0 {
                continue;
            }
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
            if edge_index % 256 == 0 && cancelled() {
                return Err(CirculationOperatorError::Cancelled);
            }
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
                if index % 256 == 0 && cancelled() {
                    return Err(CirculationOperatorError::Cancelled);
                }
                let available = cell.area_m2() * f64::from(scalar[index]);
                let outgoing = workspace.outgoing_amount[index];
                if outgoing > available && outgoing > 0.0 {
                    workspace.outgoing_scale[index] = (available / outgoing).clamp(0.0, 1.0);
                    positivity_scaled_cells += 1;
                }
            }
        }

        for (edge_index, edge) in self.grid.edges().iter().enumerate() {
            if edge_index % 256 == 0 && cancelled() {
                return Err(CirculationOperatorError::Cancelled);
            }
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
            if index % 256 == 0 && cancelled() {
                return Err(CirculationOperatorError::Cancelled);
            }
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
        let before = extensive_total_cancellable(self.grid, scalar, false, &mut cancelled)?;
        if cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        conservative_bound_redistribution(
            self.grid,
            scalar,
            enforce_nonnegative,
            workspace,
            &mut cancelled,
        )?;
        for (index, (target, value)) in workspace
            .output
            .iter_mut()
            .zip(&workspace.bounded_values)
            .enumerate()
        {
            if index % 256 == 0 && cancelled() {
                return Err(CirculationOperatorError::Cancelled);
            }
            if *value < f64::from(f32::MIN) || *value > f64::from(f32::MAX) {
                return Err(CirculationOperatorError::NumericalOverflow);
            }
            *target = *value as f32;
        }
        let after =
            extensive_total_cancellable(self.grid, &workspace.output, false, &mut cancelled)?;
        let mass_scale = extensive_total_cancellable(self.grid, scalar, true, &mut cancelled)?
            .max(f64::MIN_POSITIVE);
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
#[derive(Debug, Clone, PartialEq)]
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
    component_root: Vec<u32>,
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
            component_root: vec![0; cell_count],
        }
    }

    /// Capacity fingerprint used by tests and diagnostics to prove reuse.
    pub fn allocation_signature(&self) -> [usize; 12] {
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
            self.component_root.capacity(),
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
            || self.component_root.len() != expected_cells
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
        self.component_root.fill(0);
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

pub(crate) fn interpolate_vector(
    edge: &SphericalEdge,
    first: [f32; 3],
    second: [f32; 3],
) -> [f64; 3] {
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

fn extensive_total_cancellable(
    grid: &CubedSphereGrid,
    values: &[f32],
    absolute: bool,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<f64, CirculationOperatorError> {
    let mut sum = 0.0_f64;
    let mut correction = 0.0_f64;
    for (index, (cell, value)) in grid.cells().iter().zip(values).enumerate() {
        if index % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        let scalar = if absolute { value.abs() } else { *value };
        let term = cell.area_m2() * f64::from(scalar);
        let adjusted = term - correction;
        let next = sum + adjusted;
        correction = (next - sum) - adjusted;
        sum = next;
    }
    Ok(sum)
}

fn label_open_components(
    grid: &CubedSphereGrid,
    edge_permeability: &[f32],
    component_root: &mut [u32],
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), CirculationOperatorError> {
    for (index, root) in component_root.iter_mut().enumerate() {
        if index % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        *root = index as u32;
    }
    for (edge_index, (edge, permeability)) in grid.edges().iter().zip(edge_permeability).enumerate()
    {
        if edge_index % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        if *permeability <= 0.0 {
            continue;
        }
        let [first, second] = *edge.cells();
        let first_root = find_component_root(component_root, first);
        let second_root = find_component_root(component_root, second);
        if first_root != second_root {
            let retained = first_root.min(second_root);
            let replaced = first_root.max(second_root);
            component_root[replaced as usize] = retained;
        }
    }
    for cell in 0..component_root.len() {
        if cell % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        component_root[cell] = find_component_root(component_root, cell as u32);
    }
    Ok(())
}

fn find_component_root(parent: &mut [u32], mut cell: u32) -> u32 {
    while parent[cell as usize] != cell {
        let next = parent[cell as usize];
        let grandparent = parent[next as usize];
        parent[cell as usize] = grandparent;
        cell = grandparent;
    }
    cell
}

fn conservative_bound_redistribution(
    grid: &CubedSphereGrid,
    original: &[f32],
    enforce_nonnegative: bool,
    workspace: &mut SecondOrderTransportWorkspace,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), CirculationOperatorError> {
    // The flux graph, reconstruction stencil, and final bound projection use
    // the same positive-permeability connected components. The three arrays
    // below are no longer live transport scratch at this point, so they are
    // reused as component target, bounded total, and adjustment capacity.
    workspace.extensive_delta.fill(0.0);
    workspace.outgoing_amount.fill(0.0);
    workspace.outgoing_scale.fill(0.0);
    for (index, ((cell, original), bounded)) in grid
        .cells()
        .iter()
        .zip(original)
        .zip(&workspace.bounded_values)
        .enumerate()
    {
        if index % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        let root = workspace.component_root[index] as usize;
        workspace.extensive_delta[root] += cell.area_m2() * f64::from(*original);
        workspace.outgoing_amount[root] += cell.area_m2() * bounded;
    }

    for (index, cell) in grid.cells().iter().enumerate() {
        if index % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        let root = workspace.component_root[index] as usize;
        let correction = workspace.extensive_delta[root] - workspace.outgoing_amount[root];
        if correction == 0.0 {
            continue;
        }
        let lower = if enforce_nonnegative {
            workspace.local_min[index].max(0.0)
        } else {
            workspace.local_min[index]
        };
        let bound = if correction > 0.0 {
            workspace.local_max[index].max(lower)
        } else {
            lower
        };
        workspace.outgoing_scale[root] +=
            cell.area_m2() * (bound - workspace.bounded_values[index]).abs();
    }

    for root in 0..grid.cell_count() {
        if root % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        if workspace.component_root[root] as usize != root {
            continue;
        }
        let target = workspace.extensive_delta[root];
        let bounded = workspace.outgoing_amount[root];
        let correction = target - bounded;
        let roundoff = component_roundoff(grid.cell_count(), target, bounded);
        let capacity = workspace.outgoing_scale[root];
        if !capacity.is_finite() || capacity + roundoff < correction.abs() {
            return Err(CirculationOperatorError::NumericalOverflow);
        }
    }

    for (index, cell) in grid.cells().iter().enumerate() {
        if index % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        let root = workspace.component_root[index] as usize;
        let correction = workspace.extensive_delta[root] - workspace.outgoing_amount[root];
        let capacity = workspace.outgoing_scale[root];
        let roundoff = component_roundoff(
            grid.cell_count(),
            workspace.extensive_delta[root],
            workspace.outgoing_amount[root],
        );
        if correction.abs() <= roundoff || capacity <= roundoff {
            continue;
        }
        let lower = if enforce_nonnegative {
            workspace.local_min[index].max(0.0)
        } else {
            workspace.local_min[index]
        };
        let bound = if correction > 0.0 {
            workspace.local_max[index].max(lower)
        } else {
            lower
        };
        let extensive_capacity = cell.area_m2() * (bound - workspace.bounded_values[index]).abs();
        let fraction = (correction.abs() / capacity).clamp(0.0, 1.0);
        let signed_adjustment = extensive_capacity * fraction * correction.signum();
        workspace.bounded_values[index] += signed_adjustment / cell.area_m2();
    }

    // Recompute component totals and remove the last few summation ulps without
    // crossing a local bound. Residuals are never shared between components.
    workspace.outgoing_amount.fill(0.0);
    for (index, (cell, value)) in grid
        .cells()
        .iter()
        .zip(&workspace.bounded_values)
        .enumerate()
    {
        if index % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        let root = workspace.component_root[index] as usize;
        workspace.outgoing_amount[root] += cell.area_m2() * value;
    }
    for (index, cell) in grid.cells().iter().enumerate() {
        if index % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        let root = workspace.component_root[index] as usize;
        let residual = workspace.extensive_delta[root] - workspace.outgoing_amount[root];
        let roundoff = component_roundoff(
            grid.cell_count(),
            workspace.extensive_delta[root],
            workspace.outgoing_amount[root],
        );
        if residual.abs() <= roundoff {
            continue;
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
        workspace.outgoing_amount[root] += adjustment;
    }

    for root in 0..grid.cell_count() {
        if root % 256 == 0 && cancelled() {
            return Err(CirculationOperatorError::Cancelled);
        }
        if workspace.component_root[root] as usize != root {
            continue;
        }
        let residual = workspace.extensive_delta[root] - workspace.outgoing_amount[root];
        let roundoff = component_roundoff(
            grid.cell_count(),
            workspace.extensive_delta[root],
            workspace.outgoing_amount[root],
        );
        if residual.abs() > roundoff {
            return Err(CirculationOperatorError::NumericalOverflow);
        }
    }
    Ok(())
}

fn component_roundoff(cell_count: usize, first: f64, second: f64) -> f64 {
    128.0 * cell_count as f64 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
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

fn poll_operator_cancelled(
    index: usize,
    cancellation: Option<&BuildCancellation>,
) -> Result<(), CirculationOperatorError> {
    if index % 256 == 0 {
        check_operator_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_operator_cancelled(
    cancellation: Option<&BuildCancellation>,
) -> Result<(), CirculationOperatorError> {
    if cancellation.is_some_and(BuildCancellation::is_cancelled) {
        Err(CirculationOperatorError::Cancelled)
    } else {
        Ok(())
    }
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
    #[error("finite-volume operation was cancelled")]
    Cancelled,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reusable_fast_operator_buffers_are_bitwise_equivalent() {
        let grid = CubedSphereGrid::new(5, 6_371_000.0).unwrap();
        let operators = CirculationOperators::new(&grid);
        let cancellation = BuildCancellation::new();
        let permeability = grid
            .edges()
            .iter()
            .enumerate()
            .map(|(index, _)| if index % 7 == 0 { 0.25 } else { 1.0 })
            .collect::<Vec<_>>();
        let scalar = grid
            .cells()
            .iter()
            .map(|cell| (12.0 * cell.center_unit()[0] - 3.0 * cell.center_unit()[2]) as f32)
            .collect::<Vec<_>>();
        let velocity = grid
            .cells()
            .iter()
            .map(|cell| {
                let radial = cell.center_unit();
                [
                    (-8.0 * radial[1]) as f32,
                    (8.0 * radial[0]) as f32,
                    (2.0 * radial[0] * radial[2]) as f32,
                ]
            })
            .collect::<Vec<_>>();
        let expected_gradient = operators
            .gradient_with_permeability_cancellable(&scalar, &permeability, &cancellation)
            .unwrap();
        let expected_coriolis = operators
            .coriolis_cancellable(&velocity, 7.292_115_9e-5, &cancellation)
            .unwrap();
        let expected_tangent = operators.tangentize(&velocity).unwrap();

        let mut workspace = SecondOrderTransportWorkspace::for_grid(&grid);
        let allocation = workspace.allocation_signature();
        let mut gradient = vec![[f32::NAN; 3]; grid.cell_count()];
        let mut thickness = vec![0.0; grid.cell_count()];
        operators
            .gradient_and_donor_layer_thickness_tendency_into_cancellable_validated(
                &scalar,
                &velocity,
                &permeability,
                6_000.0,
                &mut gradient,
                &mut thickness,
                &mut workspace,
                &cancellation,
            )
            .unwrap();
        let coriolis = velocity
            .iter()
            .enumerate()
            .map(|(cell, value)| operators.coriolis_cell_validated(cell, *value, 7.292_115_9e-5))
            .collect::<Vec<_>>();
        let stage_coriolis = velocity
            .iter()
            .enumerate()
            .map(|(cell, value)| {
                operators.coriolis_cell_projected_validated(cell, *value, 7.292_115_9e-5)
            })
            .collect::<Vec<_>>();
        let tangent = velocity
            .iter()
            .enumerate()
            .map(|(cell, value)| operators.tangentize_cell_validated(cell, *value))
            .collect::<Vec<_>>();
        let stage_tangent = velocity
            .iter()
            .enumerate()
            .map(|(cell, value)| operators.project_tangent_cell_validated(cell, *value))
            .collect::<Vec<_>>();

        assert_eq!(gradient, expected_gradient);
        assert_eq!(coriolis, expected_coriolis);
        assert_eq!(tangent, expected_tangent);
        for (cell, value) in grid.cells().iter().zip(stage_tangent) {
            assert!(
                dot(to_f64_vector(value), cell.center_unit()).abs()
                    <= crate::world::natural::GLOBAL_CIRCULATION_TANGENCY_TOLERANCE_M_S
            );
        }
        for (cell, value) in grid.cells().iter().zip(stage_coriolis) {
            assert!(
                dot(to_f64_vector(value), cell.center_unit()).abs()
                    <= crate::world::natural::GLOBAL_CIRCULATION_TANGENCY_TOLERANCE_M_S
            );
        }
        assert_eq!(workspace.allocation_signature(), allocation);
    }
}
