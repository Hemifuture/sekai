use thiserror::Error;

use super::{
    ClimateConservationInterpretation, ClimateIntegrationProcedure, FormationProcedureIdentity,
    LayeredClimateState, LayeredClimateTendency, LayeredStateError, LayeredTendencyError,
    LayeredTendencySystem, LayeredTendencyWorkspace,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
use crate::world::natural::{
    ClimateCapabilitySet, ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
    EARTH_ROTATION_RATE_RAD_S, GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S,
};

pub(super) const FORMATION_TEMPERATURE_SCALE_K: f64 = 30.0;
pub(super) const FORMATION_ATMOSPHERE_SPEED_SCALE_M_S: f64 = 20.0;
pub(super) const FORMATION_OCEAN_SPEED_SCALE_M_S: f64 = 2.0;
pub(super) const FORMATION_SPECIFIC_HUMIDITY_SCALE: f64 = 0.02;

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct ClimateIntegratorDiagnostics {
    tendency_evaluations: u64,
    endpoint_evaluations: u64,
    fast_substeps: u32,
    linear_iterations: u16,
    initial_linear_relative_residual: f64,
    final_linear_relative_residual: f64,
    maximum_cfl: f64,
}

impl ClimateIntegratorDiagnostics {
    pub const fn tendency_evaluations(self) -> u64 {
        self.tendency_evaluations
    }

    pub const fn endpoint_evaluations(self) -> u64 {
        self.endpoint_evaluations
    }

    pub const fn fast_substeps(self) -> u32 {
        self.fast_substeps
    }

    pub const fn linear_iterations(self) -> u16 {
        self.linear_iterations
    }

    pub const fn initial_linear_relative_residual(self) -> f64 {
        self.initial_linear_relative_residual
    }

    pub const fn final_linear_relative_residual(self) -> f64 {
        self.final_linear_relative_residual
    }

    pub const fn maximum_cfl(self) -> f64 {
        self.maximum_cfl
    }

    pub(crate) const fn explicit(
        tendency_evaluations: u64,
        endpoint_evaluations: u64,
        cfl: f64,
    ) -> Self {
        Self {
            tendency_evaluations,
            endpoint_evaluations,
            fast_substeps: 1,
            linear_iterations: 0,
            initial_linear_relative_residual: 0.0,
            final_linear_relative_residual: 0.0,
            maximum_cfl: cfl,
        }
    }

    pub(crate) const fn split(
        tendency_evaluations: u64,
        endpoint_evaluations: u64,
        substeps: u32,
        cfl: f64,
    ) -> Self {
        Self {
            tendency_evaluations,
            endpoint_evaluations,
            fast_substeps: substeps,
            linear_iterations: 0,
            initial_linear_relative_residual: 0.0,
            final_linear_relative_residual: 0.0,
            maximum_cfl: cfl,
        }
    }

    pub(crate) const fn imex(
        tendency_evaluations: u64,
        endpoint_evaluations: u64,
        iterations: u16,
        initial_residual: f64,
        final_residual: f64,
        cfl: f64,
    ) -> Self {
        Self {
            tendency_evaluations,
            endpoint_evaluations,
            fast_substeps: 1,
            linear_iterations: iterations,
            initial_linear_relative_residual: initial_residual,
            final_linear_relative_residual: final_residual,
            maximum_cfl: cfl,
        }
    }

    pub(crate) fn accumulate(&mut self, other: Self) {
        self.tendency_evaluations = self
            .tendency_evaluations
            .saturating_add(other.tendency_evaluations);
        self.endpoint_evaluations = self
            .endpoint_evaluations
            .saturating_add(other.endpoint_evaluations);
        self.fast_substeps = self.fast_substeps.saturating_add(other.fast_substeps);
        self.linear_iterations = self
            .linear_iterations
            .saturating_add(other.linear_iterations);
        self.initial_linear_relative_residual = self
            .initial_linear_relative_residual
            .max(other.initial_linear_relative_residual);
        self.final_linear_relative_residual = self
            .final_linear_relative_residual
            .max(other.final_linear_relative_residual);
        self.maximum_cfl = self.maximum_cfl.max(other.maximum_cfl);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClimateStepResult {
    state: LayeredClimateState,
    diagnostics: ClimateIntegratorDiagnostics,
    mean_precipitation_rate_mm_s: Vec<f32>,
    ocean_mass_source_heat_j: f64,
}

impl ClimateStepResult {
    pub(crate) const fn new(
        state: LayeredClimateState,
        diagnostics: ClimateIntegratorDiagnostics,
        mean_precipitation_rate_mm_s: Vec<f32>,
    ) -> Self {
        Self {
            state,
            diagnostics,
            mean_precipitation_rate_mm_s,
            ocean_mass_source_heat_j: 0.0,
        }
    }

    pub const fn state(&self) -> &LayeredClimateState {
        &self.state
    }

    pub const fn diagnostics(&self) -> ClimateIntegratorDiagnostics {
        self.diagnostics
    }

    /// Time-mean precipitation actually diagnosed by this numerical step.
    pub fn mean_precipitation_rate_mm_s(&self) -> &[f32] {
        &self.mean_precipitation_rate_mm_s
    }

    pub fn into_state(self) -> LayeredClimateState {
        self.state
    }

    /// Returns signed heat carried by local ocean mass sources, in joules;
    /// it excludes internal transport and ordinary local heating.
    pub(crate) const fn ocean_mass_source_heat_j(&self) -> f64 {
        self.ocean_mass_source_heat_j
    }

    /// Attaches the integrated source heat computed from validated endpoints.
    pub(crate) const fn with_ocean_mass_source_heat_j(mut self, heat_j: f64) -> Self {
        self.ocean_mass_source_heat_j = heat_j;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateIntegratorError {
    #[error("climate integration was cancelled")]
    Cancelled,
    #[error("climate integration time step {found} must be finite and positive")]
    InvalidTimeStep { found: f64 },
    #[error("maximum fast step {found} must be finite and positive")]
    InvalidFastStep { found: f64 },
    #[error("linear iteration budget must be nonzero")]
    InvalidLinearIterationBudget,
    #[error("linear relative tolerance {found} must be finite and positive")]
    InvalidLinearTolerance { found: f64 },
    #[error("matrix-free climate solve broke down numerically")]
    LinearSolveBreakdown,
    #[error(
        "matrix-free climate solve did not converge after {iterations} iterations: {residual} > {tolerance}"
    )]
    LinearSolveNotConverged {
        iterations: u16,
        residual: f64,
        tolerance: f64,
    },
    #[error("climate states do not share one profile and work grid")]
    StateMismatch,
    #[error(transparent)]
    State(LayeredStateError),
    #[error(transparent)]
    Tendency(LayeredTendencyError),
}

impl From<LayeredTendencyError> for ClimateIntegratorError {
    fn from(error: LayeredTendencyError) -> Self {
        if error == LayeredTendencyError::Cancelled {
            Self::Cancelled
        } else {
            Self::Tendency(error)
        }
    }
}

impl From<LayeredStateError> for ClimateIntegratorError {
    fn from(error: LayeredStateError) -> Self {
        if error == LayeredStateError::Cancelled {
            Self::Cancelled
        } else {
            Self::State(error)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ExplicitRk3Integrator<'grid> {
    grid: &'grid CubedSphereGrid,
}

impl<'grid> ExplicitRk3Integrator<'grid> {
    pub const fn new(grid: &'grid CubedSphereGrid) -> Self {
        Self { grid }
    }

    /// Declares the scientific capabilities and conservation ledger owned by
    /// the actual explicit reference implementation.
    pub fn formation_procedure_identity(
        &self,
        profile: ClimateModelProfile,
    ) -> FormationProcedureIdentity {
        FormationProcedureIdentity::new(
            ClimateIntegrationProcedure::ExplicitEndpointThenClassicRk3V1,
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
        let mut endpoint_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        let mut dynamics_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        let endpoint = system.evaluate_with_workspace_for_step(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            dt_seconds,
            cancellation,
            &mut endpoint_workspace,
        )?;
        let mean_precipitation_rate_mm_s =
            copy_scalars(endpoint.precipitation_rate_mm_s(), cancellation)?;
        let mut endpoint_derivative =
            ClimateDerivative::from_tendency(state, &endpoint, cancellation)?;
        if state.profile() == ClimateModelProfile::C2LayeredV1 {
            let fast = system.evaluate_fast_with_workspace_validated(
                state,
                forcing,
                ocean_edge_permeability,
                cancellation,
                &mut dynamics_workspace,
            )?;
            let fast = ClimateDerivative::from_tendency(state, &fast, cancellation)?;
            endpoint_derivative = endpoint_derivative.subtract(&fast, cancellation)?;
        }
        let mut endpoint_state = state.clone_cancellable(cancellation)?;
        apply_scalar_endpoint(
            state,
            &endpoint_derivative,
            dt_seconds,
            &mut endpoint_state,
            cancellation,
        )?;
        let source_heat =
            ocean_mass_source_heat_j(self.grid, state, &endpoint_state, cancellation)?;
        let mut evaluations =
            1_u64 + u64::from(state.profile() == ClimateModelProfile::C2LayeredV1);
        let advanced = rk3_step_with(
            self.grid,
            &endpoint_state,
            dt_seconds,
            cancellation,
            |stage| {
                evaluations += 1;
                let mut tendency = system.evaluate_smooth_dynamics_with_workspace(
                    stage,
                    forcing,
                    ocean_edge_permeability,
                    month,
                    cancellation,
                    &mut dynamics_workspace,
                )?;
                // Moisture selects Q once at this physical endpoint. The
                // smooth operator owns no independent Q; retain the declared
                // mass source and recompute its donor momentum at each stage.
                if let Some(exchange) = endpoint.overturning_exchange_m_s() {
                    system.apply_declared_overturning_tendency(
                        stage,
                        exchange,
                        cancellation,
                        &mut tendency,
                    )?;
                }
                ClimateDerivative::from_tendency(stage, &tendency, cancellation)
            },
        )?;
        Ok(ClimateStepResult::new(
            advanced,
            ClimateIntegratorDiagnostics::explicit(
                evaluations,
                1,
                estimate_cfl(self.grid, state, dt_seconds, cancellation)?,
            ),
            mean_precipitation_rate_mm_s,
        )
        .with_ocean_mass_source_heat_j(source_heat))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayerDerivative {
    pub(crate) role: ClimateLayerRole,
    pub(crate) height: Vec<f32>,
    pub(crate) velocity: Vec<[f64; 3]>,
    // C2 ocean layers store d(H*T)/dt; other roles retain dT/dt.
    pub(crate) temperature: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClimateDerivative {
    pub(crate) layers: Vec<LayerDerivative>,
    pub(crate) humidity: Vec<f32>,
    pub(crate) upper_humidity: Option<Vec<f32>>,
    pub(crate) deep_temperature: Option<Vec<f32>>,
}

impl ClimateDerivative {
    pub(crate) fn from_tendency(
        state: &LayeredClimateState,
        tendency: &LayeredClimateTendency,
        cancellation: &BuildCancellation,
    ) -> Result<Self, ClimateIntegratorError> {
        let mut layers = Vec::with_capacity(state.active_roles().len());
        for role in state.active_roles() {
            let mut temperature = Vec::with_capacity(state.cell_count());
            let temperature_rate = tendency
                .temperature_tendency_k_s(*role)
                .expect("active temperature tendency");
            let height_rate = tendency
                .height_tendency_m_s(*role)
                .expect("active height tendency");
            let values = state.temperature_c(*role).expect("active temperature");
            for cell in 0..state.cell_count() {
                poll_integrator_cancelled(cell, Some(cancellation))?;
                let rate = if conservative_ocean_layer(state.profile(), *role) {
                    ocean_layer_thickness_m(state, *role, cell)? * f64::from(temperature_rate[cell])
                        + f64::from(values[cell]) * f64::from(height_rate[cell])
                } else {
                    f64::from(temperature_rate[cell])
                };
                temperature.push(rate);
            }
            layers.push(LayerDerivative {
                role: *role,
                height: copy_scalars(
                    tendency
                        .height_tendency_m_s(*role)
                        .expect("active tendency role"),
                    cancellation,
                )?,
                velocity: copy_vectors(
                    tendency
                        .velocity_tendency_m_s2(*role)
                        .expect("active tendency role"),
                    cancellation,
                )?,
                temperature,
            });
        }
        Ok(Self {
            layers,
            humidity: copy_scalars(tendency.specific_humidity_tendency_s_inv(), cancellation)?,
            upper_humidity: tendency
                .upper_specific_humidity_tendency_s_inv()
                .map(|values| copy_scalars(values, cancellation))
                .transpose()?,
            deep_temperature: tendency
                .deep_ocean_temperature_tendency_k_s()
                .map(|values| copy_scalars(values, cancellation))
                .transpose()?,
        })
    }

    pub(crate) fn subtract(
        &self,
        other: &Self,
        cancellation: &BuildCancellation,
    ) -> Result<Self, ClimateIntegratorError> {
        debug_assert_eq!(self.layers.len(), other.layers.len());
        let mut layers = Vec::with_capacity(self.layers.len());
        for (left, right) in self.layers.iter().zip(&other.layers) {
            layers.push(LayerDerivative {
                role: left.role,
                height: combine_scalars(&left.height, &right.height, cancellation, |a, b| a - b)?,
                velocity: combine_vectors(
                    &left.velocity,
                    &right.velocity,
                    cancellation,
                    |a, b| a - b,
                )?,
                temperature: combine_scalars(
                    &left.temperature,
                    &right.temperature,
                    cancellation,
                    |a, b| a - b,
                )?,
            });
        }
        Ok(Self {
            layers,
            humidity: combine_scalars(&self.humidity, &other.humidity, cancellation, |a, b| a - b)?,
            upper_humidity: match (&self.upper_humidity, &other.upper_humidity) {
                (Some(left), Some(right)) => {
                    Some(combine_scalars(left, right, cancellation, |a, b| a - b)?)
                }
                (None, None) => None,
                _ => unreachable!("matching profiles have matching upper moisture"),
            },
            deep_temperature: match (&self.deep_temperature, &other.deep_temperature) {
                (Some(left), Some(right)) => {
                    Some(combine_scalars(left, right, cancellation, |a, b| a - b)?)
                }
                (None, None) => None,
                _ => unreachable!("matching profiles have matching deep reservoirs"),
            },
        })
    }

    pub(crate) fn add(
        &self,
        other: &Self,
        cancellation: &BuildCancellation,
    ) -> Result<Self, ClimateIntegratorError> {
        debug_assert_eq!(self.layers.len(), other.layers.len());
        let mut layers = Vec::with_capacity(self.layers.len());
        for (left, right) in self.layers.iter().zip(&other.layers) {
            layers.push(LayerDerivative {
                role: left.role,
                height: combine_scalars(&left.height, &right.height, cancellation, |a, b| a + b)?,
                velocity: combine_vectors(
                    &left.velocity,
                    &right.velocity,
                    cancellation,
                    |a, b| a + b,
                )?,
                temperature: combine_scalars(
                    &left.temperature,
                    &right.temperature,
                    cancellation,
                    |a, b| a + b,
                )?,
            });
        }
        Ok(Self {
            layers,
            humidity: combine_scalars(&self.humidity, &other.humidity, cancellation, |a, b| a + b)?,
            upper_humidity: match (&self.upper_humidity, &other.upper_humidity) {
                (Some(left), Some(right)) => {
                    Some(combine_scalars(left, right, cancellation, |a, b| a + b)?)
                }
                (None, None) => None,
                _ => unreachable!("matching profiles have matching upper moisture"),
            },
            deep_temperature: match (&self.deep_temperature, &other.deep_temperature) {
                (Some(left), Some(right)) => {
                    Some(combine_scalars(left, right, cancellation, |a, b| a + b)?)
                }
                (None, None) => None,
                _ => unreachable!("matching profiles have matching deep reservoirs"),
            },
        })
    }

    pub(crate) fn layer(&self, role: ClimateLayerRole) -> &LayerDerivative {
        self.layers
            .iter()
            .find(|layer| layer.role == role)
            .expect("derivative contains every active role")
    }
}

/// Identifies active C2 ocean roles whose RK tracer variable is H*T.
pub(super) fn conservative_ocean_layer(
    profile: ClimateModelProfile,
    role: ClimateLayerRole,
) -> bool {
    profile == ClimateModelProfile::C2LayeredV1
        && matches!(
            role,
            ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline
        )
}

/// Returns actual ocean H for an active role and valid cell index.
/// Rejects non-finite or non-positive thickness with `InvalidFluidThickness`.
pub(super) fn ocean_layer_thickness_m(
    state: &LayeredClimateState,
    role: ClimateLayerRole,
    cell: usize,
) -> Result<f64, ClimateIntegratorError> {
    Ok(LayeredTendencySystem::validated_fluid_layer_thickness_m(
        f64::from(
            state
                .reference_thickness_m(role)
                .expect("active ocean layer"),
        ),
        state.height_anomaly_m(role).expect("active ocean layer")[cell],
        0.0,
        role,
        cell,
    )?)
}

/// Integrates local ocean mass-source heat between matching validated states.
/// The endpoint must precede transport; returns signed joules and rejects
/// cancellation or invalid ocean thickness/capacity through the shared helpers.
pub(super) fn ocean_mass_source_heat_j(
    grid: &CubedSphereGrid,
    before: &LayeredClimateState,
    endpoint: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<f64, ClimateIntegratorError> {
    let layout = ClimateLayerLayout::for_profile(before.profile());
    let mut heat_j = 0.0;
    for layer in layout
        .layers()
        .iter()
        .filter(|layer| conservative_ocean_layer(before.profile(), layer.role()))
    {
        let role = layer.role();
        for cell in 0..grid.cell_count() {
            poll_integrator_cancelled(cell, Some(cancellation))?;
            let old_depth = ocean_layer_thickness_m(before, role, cell)?;
            let new_depth = ocean_layer_thickness_m(endpoint, role, cell)?;
            let capacity = super::tendency::cell_heat_capacity_per_area(before, layer, cell)?;
            heat_j += grid.cells()[cell].area_m2() * capacity / old_depth
                * (f64::from(endpoint.temperature_c(role).expect("ocean temperature")[cell])
                    + 273.15)
                * (new_depth - old_depth);
        }
    }
    Ok(heat_j)
}

pub(crate) fn apply_scalar_endpoint(
    initial: &LayeredClimateState,
    endpoint: &ClimateDerivative,
    step_seconds: f64,
    advanced: &mut LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<(), ClimateIntegratorError> {
    let quantize = |before: f32, tendency: f64| -> Result<f32, ClimateIntegratorError> {
        let value = f64::from(before) + step_seconds * tendency;
        if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
            return Err(ClimateIntegratorError::LinearSolveBreakdown);
        }
        Ok(value as f32)
    };
    for layer in &endpoint.layers {
        let before = initial.temperature_c(layer.role).expect("active role");
        let after = advanced.temperature_c_mut(layer.role).expect("active role");
        for (cell, target) in after.iter_mut().enumerate() {
            poll_integrator_cancelled(cell, Some(cancellation))?;
            let rate = if conservative_ocean_layer(initial.profile(), layer.role) {
                (layer.temperature[cell] - f64::from(before[cell]) * f64::from(layer.height[cell]))
                    / ocean_layer_thickness_m(initial, layer.role, cell)?
            } else {
                layer.temperature[cell]
            };
            *target = quantize(before[cell], rate)?;
        }
        if conservative_ocean_layer(initial.profile(), layer.role) {
            let initial_height = initial
                .height_anomaly_m(layer.role)
                .expect("active ocean layer");
            for (cell, target) in advanced
                .height_anomaly_m_mut(layer.role)
                .expect("active ocean layer")
                .iter_mut()
                .enumerate()
            {
                poll_integrator_cancelled(cell, Some(cancellation))?;
                *target = quantize(initial_height[cell], f64::from(layer.height[cell]))?;
                LayeredTendencySystem::validated_fluid_layer_thickness_m(
                    f64::from(
                        initial
                            .reference_thickness_m(layer.role)
                            .expect("ocean reference"),
                    ),
                    *target,
                    0.0,
                    layer.role,
                    cell,
                )?;
            }
        }
    }
    for (cell, target) in advanced.specific_humidity_mut().iter_mut().enumerate() {
        poll_integrator_cancelled(cell, Some(cancellation))?;
        *target = quantize(
            initial.specific_humidity()[cell],
            f64::from(endpoint.humidity[cell]),
        )?
        .max(0.0);
    }
    if let (Some(before), Some(tendency), Some(after)) = (
        initial.upper_specific_humidity(),
        endpoint.upper_humidity.as_ref(),
        advanced.upper_specific_humidity_mut(),
    ) {
        for (cell, target) in after.iter_mut().enumerate() {
            poll_integrator_cancelled(cell, Some(cancellation))?;
            *target = quantize(before[cell], f64::from(tendency[cell]))?.max(0.0);
        }
    }
    if let (Some(before), Some(tendency), Some(after)) = (
        initial.deep_ocean_temperature_c(),
        endpoint.deep_temperature.as_ref(),
        advanced.deep_ocean_temperature_c_mut(),
    ) {
        for (cell, target) in after.iter_mut().enumerate() {
            poll_integrator_cancelled(cell, Some(cancellation))?;
            *target = quantize(before[cell], f64::from(tendency[cell]))?;
        }
    }
    check_integrator_cancelled(Some(cancellation))
}

pub(crate) fn rk3_step_with<F>(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    dt_seconds: f64,
    cancellation: &BuildCancellation,
    mut evaluate: F,
) -> Result<LayeredClimateState, ClimateIntegratorError>
where
    F: FnMut(&LayeredClimateState) -> Result<ClimateDerivative, ClimateIntegratorError>,
{
    check_integrator_cancelled(Some(cancellation))?;
    let first = evaluate(state)?;
    rk3_step_with_first(grid, state, dt_seconds, cancellation, first, evaluate)
}

pub(crate) fn rk3_step_with_first<F>(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    dt_seconds: f64,
    cancellation: &BuildCancellation,
    first: ClimateDerivative,
    mut evaluate: F,
) -> Result<LayeredClimateState, ClimateIntegratorError>
where
    F: FnMut(&LayeredClimateState) -> Result<ClimateDerivative, ClimateIntegratorError>,
{
    check_integrator_cancelled(Some(cancellation))?;
    let stage_two = combine_state(grid, state, &[(0.5 * dt_seconds, &first)], cancellation)?;
    let second = evaluate(&stage_two)?;
    let stage_three = combine_state(
        grid,
        state,
        &[(-dt_seconds, &first), (2.0 * dt_seconds, &second)],
        cancellation,
    )?;
    let third = evaluate(&stage_three)?;
    combine_state(
        grid,
        state,
        &[
            (dt_seconds / 6.0, &first),
            (2.0 * dt_seconds / 3.0, &second),
            (dt_seconds / 6.0, &third),
        ],
        cancellation,
    )
}

pub(crate) fn combine_state(
    grid: &CubedSphereGrid,
    base: &LayeredClimateState,
    terms: &[(f64, &ClimateDerivative)],
    cancellation: &BuildCancellation,
) -> Result<LayeredClimateState, ClimateIntegratorError> {
    let mut result = base.clone_cancellable(cancellation)?;
    let operators = CirculationOperators::new(grid);
    for role in base.active_roles() {
        let base_height = base.height_anomaly_m(*role).expect("active role");
        let base_velocity = base.velocity_m_s(*role).expect("active role");
        let base_temperature = base.temperature_c(*role).expect("active role");
        for (index, target) in result
            .height_anomaly_m_mut(*role)
            .expect("active role")
            .iter_mut()
            .enumerate()
        {
            poll_integrator_cancelled(index, Some(cancellation))?;
            *target = accumulate_scalar(base_height[index], terms, |derivative| {
                derivative.layer(*role).height[index]
            })?;
        }
        for (index, (target, original)) in result
            .velocity_m_s_mut(*role)
            .expect("active role")
            .iter_mut()
            .zip(base_velocity)
            .enumerate()
        {
            poll_integrator_cancelled(index, Some(cancellation))?;
            let mut value = [0.0_f32; 3];
            for component in 0..3 {
                value[component] = accumulate_scalar(original[component], terms, |derivative| {
                    derivative.layer(*role).velocity[index][component]
                })?;
            }
            *target = operators.project_tangent_cell_validated(index, value);
        }
        for (index, &base_temperature) in base_temperature.iter().enumerate() {
            poll_integrator_cancelled(index, Some(cancellation))?;
            let temperature = if conservative_ocean_layer(base.profile(), *role) {
                let mut depth = ocean_layer_thickness_m(base, *role, index)?;
                let mut heat_content = depth * f64::from(base_temperature);
                for (coefficient, derivative) in terms {
                    depth += coefficient * f64::from(derivative.layer(*role).height[index]);
                    heat_content += coefficient * derivative.layer(*role).temperature[index];
                }
                if !depth.is_finite() || depth <= 0.0 {
                    return Err(LayeredTendencyError::InvalidFluidThickness {
                        role: *role,
                        cell: index,
                        found: depth,
                    }
                    .into());
                }
                // Form both conservative variables before storage rounding.
                // Dividing by the already-quantized H would turn its f32
                // rounding into a spurious change of a constant tracer.
                let value = heat_content / depth;
                if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX)
                {
                    return Err(ClimateIntegratorError::LinearSolveBreakdown);
                }
                value as f32
            } else {
                accumulate_scalar(base_temperature, terms, |derivative| {
                    derivative.layer(*role).temperature[index]
                })?
            };
            result.temperature_c_mut(*role).expect("active role")[index] = temperature;
        }
    }
    for (index, target) in result.specific_humidity_mut().iter_mut().enumerate() {
        poll_integrator_cancelled(index, Some(cancellation))?;
        *target = accumulate_scalar(base.specific_humidity()[index], terms, |derivative| {
            derivative.humidity[index]
        })?
        .max(0.0);
    }
    if let (Some(base_upper), Some(result_upper)) = (
        base.upper_specific_humidity(),
        result.upper_specific_humidity_mut(),
    ) {
        for (index, target) in result_upper.iter_mut().enumerate() {
            poll_integrator_cancelled(index, Some(cancellation))?;
            *target = accumulate_scalar(base_upper[index], terms, |derivative| {
                derivative
                    .upper_humidity
                    .as_ref()
                    .expect("C2 upper moisture derivative")[index]
            })?
            .max(0.0);
        }
    }
    if let (Some(base_deep), Some(result_deep)) = (
        base.deep_ocean_temperature_c(),
        result.deep_ocean_temperature_c_mut(),
    ) {
        for (index, target) in result_deep.iter_mut().enumerate() {
            poll_integrator_cancelled(index, Some(cancellation))?;
            *target = accumulate_scalar(base_deep[index], terms, |derivative| {
                derivative.deep_temperature.as_ref().expect("C2 derivative")[index]
            })?;
        }
    }
    result.validate_against_cancellable(grid, cancellation)?;
    Ok(result)
}

fn accumulate_scalar<F, T>(
    base: f32,
    terms: &[(f64, &ClimateDerivative)],
    mut component: F,
) -> Result<f32, ClimateIntegratorError>
where
    F: FnMut(&ClimateDerivative) -> T,
    T: Into<f64>,
{
    let mut value = f64::from(base);
    for (coefficient, derivative) in terms {
        value += coefficient * component(derivative).into();
    }
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(ClimateIntegratorError::LinearSolveBreakdown);
    }
    Ok(value as f32)
}

pub(crate) fn copy_scalars(
    values: &[f32],
    cancellation: &BuildCancellation,
) -> Result<Vec<f32>, ClimateIntegratorError> {
    let mut copy = Vec::with_capacity(values.len());
    for (index, value) in values.iter().copied().enumerate() {
        poll_integrator_cancelled(index, Some(cancellation))?;
        copy.push(value);
    }
    Ok(copy)
}

fn copy_vectors(
    values: &[[f64; 3]],
    cancellation: &BuildCancellation,
) -> Result<Vec<[f64; 3]>, ClimateIntegratorError> {
    let mut copy = Vec::with_capacity(values.len());
    for (index, value) in values.iter().copied().enumerate() {
        poll_integrator_cancelled(index, Some(cancellation))?;
        copy.push(value);
    }
    Ok(copy)
}

fn combine_scalars<T: Copy>(
    left: &[T],
    right: &[T],
    cancellation: &BuildCancellation,
    combine: impl Fn(T, T) -> T,
) -> Result<Vec<T>, ClimateIntegratorError> {
    debug_assert_eq!(left.len(), right.len());
    let mut result = Vec::with_capacity(left.len());
    for (index, (&left, &right)) in left.iter().zip(right).enumerate() {
        poll_integrator_cancelled(index, Some(cancellation))?;
        result.push(combine(left, right));
    }
    Ok(result)
}

fn combine_vectors(
    left: &[[f64; 3]],
    right: &[[f64; 3]],
    cancellation: &BuildCancellation,
    combine: impl Fn(f64, f64) -> f64,
) -> Result<Vec<[f64; 3]>, ClimateIntegratorError> {
    debug_assert_eq!(left.len(), right.len());
    let mut result = Vec::with_capacity(left.len());
    for (index, (left, right)) in left.iter().zip(right).enumerate() {
        poll_integrator_cancelled(index, Some(cancellation))?;
        result.push(std::array::from_fn(|component| {
            combine(left[component], right[component])
        }));
    }
    Ok(result)
}

pub fn climate_state_rms_difference(
    grid: &CubedSphereGrid,
    left: &LayeredClimateState,
    right: &LayeredClimateState,
) -> Result<f64, ClimateIntegratorError> {
    climate_state_rms_difference_impl(grid, left, right, None)
}

/// Returns the worst annual-cycle change across the named prognostic fields.
///
/// Each field is first reduced with a spherical-area-weighted RMS and then
/// nondimensionalized with its declared physical scale. Taking the maximum
/// prevents a large-valued height field from hiding unconverged humidity,
/// temperature, or momentum.
pub fn climate_state_formation_residual(
    grid: &CubedSphereGrid,
    previous: &LayeredClimateState,
    current: &LayeredClimateState,
) -> Result<f64, ClimateIntegratorError> {
    climate_state_formation_residual_impl(grid, previous, current, None)
}

pub(crate) fn climate_state_formation_residual_cancellable(
    grid: &CubedSphereGrid,
    previous: &LayeredClimateState,
    current: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<f64, ClimateIntegratorError> {
    climate_state_formation_residual_impl(grid, previous, current, Some(cancellation))
}

fn climate_state_formation_residual_impl(
    grid: &CubedSphereGrid,
    previous: &LayeredClimateState,
    current: &LayeredClimateState,
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, ClimateIntegratorError> {
    let validate = |state: &LayeredClimateState| {
        match cancellation {
            Some(cancellation) => state.validate_against_cancellable(grid, cancellation),
            None => state.validate_against(grid),
        }
        .map_err(|error| {
            if error == LayeredStateError::Cancelled {
                ClimateIntegratorError::Cancelled
            } else {
                ClimateIntegratorError::State(error)
            }
        })
    };
    validate(previous)?;
    validate(current)?;
    if previous.profile() != current.profile() {
        return Err(ClimateIntegratorError::StateMismatch);
    }

    let total_area_m2 = grid.cells().iter().map(|cell| cell.area_m2()).sum::<f64>();
    let scalar_residual = |left: &[f32], right: &[f32], scale: f64| {
        area_weighted_scalar_rms(grid, left, right, total_area_m2, cancellation)
            .map(|rms| rms / scale)
    };
    let vector_residual = |left: &[[f32; 3]], right: &[[f32; 3]], scale: f64| {
        area_weighted_vector_rms(grid, left, right, total_area_m2, cancellation)
            .map(|rms| rms / scale)
    };

    let mut maximum = 0.0_f64;
    for role in previous.active_roles() {
        let height_scale = f64::from(
            previous
                .reference_thickness_m(*role)
                .expect("active role has a reference thickness"),
        );
        maximum = maximum.max(scalar_residual(
            previous.height_anomaly_m(*role).expect("active role"),
            current.height_anomaly_m(*role).expect("active role"),
            height_scale,
        )?);
        maximum = maximum.max(scalar_residual(
            previous.temperature_c(*role).expect("active role"),
            current.temperature_c(*role).expect("active role"),
            FORMATION_TEMPERATURE_SCALE_K,
        )?);
        let velocity_scale = match role {
            ClimateLayerRole::LowerAtmosphere | ClimateLayerRole::UpperAtmosphere => {
                FORMATION_ATMOSPHERE_SPEED_SCALE_M_S
            }
            ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline => {
                FORMATION_OCEAN_SPEED_SCALE_M_S
            }
            ClimateLayerRole::DeepOceanReservoir => unreachable!("inactive reservoir"),
        };
        maximum = maximum.max(vector_residual(
            previous.velocity_m_s(*role).expect("active role"),
            current.velocity_m_s(*role).expect("active role"),
            velocity_scale,
        )?);
    }
    maximum = maximum.max(scalar_residual(
        previous.specific_humidity(),
        current.specific_humidity(),
        FORMATION_SPECIFIC_HUMIDITY_SCALE,
    )?);
    if let (Some(previous), Some(current)) = (
        previous.upper_specific_humidity(),
        current.upper_specific_humidity(),
    ) {
        maximum = maximum.max(scalar_residual(
            previous,
            current,
            FORMATION_SPECIFIC_HUMIDITY_SCALE,
        )?);
    }
    if let (Some(previous), Some(current)) = (
        previous.deep_ocean_temperature_c(),
        current.deep_ocean_temperature_c(),
    ) {
        maximum = maximum.max(scalar_residual(
            previous,
            current,
            FORMATION_TEMPERATURE_SCALE_K,
        )?);
    }
    check_integrator_cancelled(cancellation)?;
    Ok(maximum)
}

fn area_weighted_scalar_rms(
    grid: &CubedSphereGrid,
    left: &[f32],
    right: &[f32],
    total_area_m2: f64,
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, ClimateIntegratorError> {
    debug_assert_eq!(left.len(), grid.cell_count());
    debug_assert_eq!(right.len(), grid.cell_count());
    let mut squared = 0.0_f64;
    for (index, ((left, right), cell)) in left.iter().zip(right).zip(grid.cells()).enumerate() {
        poll_integrator_cancelled(index, cancellation)?;
        squared += cell.area_m2() * (f64::from(*left) - f64::from(*right)).powi(2);
    }
    Ok((squared / total_area_m2).sqrt())
}

fn area_weighted_vector_rms(
    grid: &CubedSphereGrid,
    left: &[[f32; 3]],
    right: &[[f32; 3]],
    total_area_m2: f64,
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, ClimateIntegratorError> {
    debug_assert_eq!(left.len(), grid.cell_count());
    debug_assert_eq!(right.len(), grid.cell_count());
    let mut squared = 0.0_f64;
    for (index, ((left, right), cell)) in left.iter().zip(right).zip(grid.cells()).enumerate() {
        poll_integrator_cancelled(index, cancellation)?;
        let vector_error = (0..3)
            .map(|component| (f64::from(left[component]) - f64::from(right[component])).powi(2))
            .sum::<f64>();
        squared += cell.area_m2() * vector_error;
    }
    Ok((squared / total_area_m2).sqrt())
}

fn climate_state_rms_difference_impl(
    grid: &CubedSphereGrid,
    left: &LayeredClimateState,
    right: &LayeredClimateState,
    cancellation: Option<&BuildCancellation>,
) -> Result<f64, ClimateIntegratorError> {
    let validate = |state: &LayeredClimateState| {
        match cancellation {
            Some(cancellation) => state.validate_against_cancellable(grid, cancellation),
            None => state.validate_against(grid),
        }
        .map_err(|error| {
            if error == LayeredStateError::Cancelled {
                ClimateIntegratorError::Cancelled
            } else {
                ClimateIntegratorError::State(error)
            }
        })
    };
    validate(left)?;
    validate(right)?;
    if left.profile() != right.profile() {
        return Err(ClimateIntegratorError::StateMismatch);
    }
    let mut sum = 0.0_f64;
    let mut count = 0_usize;
    for role in left.active_roles() {
        for (index, (left, right)) in left
            .height_anomaly_m(*role)
            .expect("active role")
            .iter()
            .zip(right.height_anomaly_m(*role).expect("active role"))
            .chain(
                left.temperature_c(*role)
                    .expect("active role")
                    .iter()
                    .zip(right.temperature_c(*role).expect("active role")),
            )
            .enumerate()
        {
            poll_integrator_cancelled(index, cancellation)?;
            sum += (f64::from(*left) - f64::from(*right)).powi(2);
            count += 1;
        }
        for (index, (left, right)) in left
            .velocity_m_s(*role)
            .expect("active role")
            .iter()
            .zip(right.velocity_m_s(*role).expect("active role"))
            .enumerate()
        {
            poll_integrator_cancelled(index, cancellation)?;
            for component in 0..3 {
                sum += (f64::from(left[component]) - f64::from(right[component])).powi(2);
                count += 1;
            }
        }
    }
    for (index, (left, right)) in left
        .specific_humidity()
        .iter()
        .zip(right.specific_humidity())
        .enumerate()
    {
        poll_integrator_cancelled(index, cancellation)?;
        sum += (f64::from(*left) - f64::from(*right)).powi(2);
        count += 1;
    }
    if let (Some(left), Some(right)) = (
        left.upper_specific_humidity(),
        right.upper_specific_humidity(),
    ) {
        for (index, (left, right)) in left.iter().zip(right).enumerate() {
            poll_integrator_cancelled(index, cancellation)?;
            sum += (f64::from(*left) - f64::from(*right)).powi(2);
            count += 1;
        }
    }
    if let (Some(left), Some(right)) = (
        left.deep_ocean_temperature_c(),
        right.deep_ocean_temperature_c(),
    ) {
        for (index, (left, right)) in left.iter().zip(right).enumerate() {
            poll_integrator_cancelled(index, cancellation)?;
            sum += (f64::from(*left) - f64::from(*right)).powi(2);
            count += 1;
        }
    }
    check_integrator_cancelled(cancellation)?;
    Ok((sum / count.max(1) as f64).sqrt())
}

fn poll_integrator_cancelled(
    index: usize,
    cancellation: Option<&BuildCancellation>,
) -> Result<(), ClimateIntegratorError> {
    if index % 256 == 0 {
        check_integrator_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_integrator_cancelled(
    cancellation: Option<&BuildCancellation>,
) -> Result<(), ClimateIntegratorError> {
    if cancellation.is_some_and(BuildCancellation::is_cancelled) {
        Err(ClimateIntegratorError::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_step(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    dt_seconds: f64,
    cancellation: &BuildCancellation,
) -> Result<(), ClimateIntegratorError> {
    if cancellation.is_cancelled() {
        return Err(ClimateIntegratorError::Cancelled);
    }
    if !dt_seconds.is_finite() || dt_seconds <= 0.0 {
        return Err(ClimateIntegratorError::InvalidTimeStep { found: dt_seconds });
    }
    state.validate_against_cancellable(grid, cancellation)?;
    Ok(())
}

pub(crate) fn estimate_cfl(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    dt_seconds: f64,
    cancellation: &BuildCancellation,
) -> Result<f64, ClimateIntegratorError> {
    let mut maximum_speed = 0.0_f64;
    for role in state.active_roles() {
        for (index, velocity) in state
            .velocity_m_s(*role)
            .expect("active role")
            .iter()
            .enumerate()
        {
            poll_integrator_cancelled(index, Some(cancellation))?;
            let speed = velocity
                .iter()
                .map(|value| f64::from(*value).powi(2))
                .sum::<f64>()
                .sqrt();
            maximum_speed = maximum_speed.max(speed);
        }
    }
    let reference_speed =
        if state.profile() == crate::world::natural::ClimateModelProfile::C2LayeredV1 {
            let lower = f64::from(
                state
                    .reference_thickness_m(crate::world::natural::ClimateLayerRole::LowerAtmosphere)
                    .expect("C2"),
            );
            let upper = f64::from(
                state
                    .reference_thickness_m(crate::world::natural::ClimateLayerRole::UpperAtmosphere)
                    .expect("C2"),
            );
            let mut speed = GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S;
            for (cell, (&lower_height, &upper_height)) in state
                .height_anomaly_m(crate::world::natural::ClimateLayerRole::LowerAtmosphere)
                .expect("C2")
                .iter()
                .zip(
                    state
                        .height_anomaly_m(crate::world::natural::ClimateLayerRole::UpperAtmosphere)
                        .expect("C2"),
                )
                .enumerate()
            {
                poll_integrator_cancelled(cell, Some(cancellation))?;
                // Omitting the nonnegative bottom floor overestimates the fluid
                // depth, keeping this wave-speed bound conservative over terrain.
                speed = speed.max(super::tendency::atmospheric_fast_mode_speed_m_s(
                    lower + f64::from(lower_height),
                    upper + f64::from(upper_height),
                    super::tendency::atmospheric_thermal_buoyancy_difference_m_s2(state, cell),
                    super::tendency::atmospheric_unlapsed_upper_buoyancy_m_s2(state, cell),
                    cell,
                )?);
            }
            speed
        } else {
            GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S
        };
    let advective =
        dt_seconds * (reference_speed + maximum_speed) / grid.minimum_center_distance_m();
    let rotational = dt_seconds * 2.0 * EARTH_ROTATION_RATE_RAD_S;
    check_integrator_cancelled(Some(cancellation))?;
    Ok(advective.max(rotational))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (
        CubedSphereGrid,
        PlanetForcing,
        LayeredClimateState,
        ClimateDerivative,
    ) {
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; 12]; count],
            vec![[15.0; 12]; count],
            vec![[15.0; 12]; count],
            vec![[0.001; 12]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let derivative = ClimateDerivative {
            layers: state
                .active_roles()
                .iter()
                .map(|role| LayerDerivative {
                    role: *role,
                    height: vec![0.0; count],
                    velocity: vec![[0.0; 3]; count],
                    temperature: vec![0.0; count],
                })
                .collect(),
            humidity: vec![0.0; count],
            upper_humidity: Some(vec![0.0; count]),
            deep_temperature: Some(vec![0.0; count]),
        };
        (grid, forcing, state, derivative)
    }

    #[test]
    fn explicit_reference_consumes_the_full_endpoint_declared_venting_mass() {
        // Uniform interface displacement and zero velocity isolate the
        // frozen Q source. Global layer sums also cancel horizontal fluxes.
        let (grid, forcing, mut initial, _) = fixture();
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        initial.height_anomaly_m_mut(lower).unwrap().fill(12.0);
        initial.height_anomaly_m_mut(upper).unwrap().fill(-12.0);
        let cancellation = BuildCancellation::new();
        let permeability = vec![1.0; grid.edges().len()];
        let step = 60.0;
        let declared = LayeredTendencySystem::new(&grid)
            .evaluate_for_step(&initial, &forcing, &permeability, 0, step, &cancellation)
            .unwrap();
        let transfer = step
            * grid
                .cells()
                .iter()
                .zip(declared.overturning_exchange_m_s().unwrap())
                .map(|(cell, rate)| cell.area_m2() * rate)
                .sum::<f64>();
        assert!(
            transfer > 0.0,
            "positive interface must declare upward venting"
        );
        let result = ExplicitRk3Integrator::new(&grid)
            .advance(&initial, &forcing, &permeability, 0, step, &cancellation)
            .unwrap();
        for (role, sign) in [(lower, -1.0), (upper, 1.0)] {
            let before = initial.height_anomaly_m(role).unwrap();
            let after = result.state().height_anomaly_m(role).unwrap();
            let change = grid
                .cells()
                .iter()
                .zip(before.iter().zip(after))
                .map(|(cell, (before, after))| {
                    cell.area_m2() * (f64::from(*after) - f64::from(*before))
                })
                .sum::<f64>();
            let stored_scale = grid
                .cells()
                .iter()
                .zip(before)
                .map(|(cell, value)| cell.area_m2() * f64::from(value.abs()))
                .sum::<f64>();
            let tolerance = 8.0 * f64::from(f32::EPSILON) * stored_scale;
            assert!(
                (change - sign * transfer).abs() <= tolerance,
                "{role:?}: retained={change}, declared={}, rounding={tolerance}",
                sign * transfer
            );
        }
    }

    #[test]
    fn ocean_rk_combination_preserves_constant_temperature_and_extensive_heat() {
        // A single conservative face exchange isolates the RK representation
        // from radiation and circulation; unequal H exposes a T-only update.
        let (grid, _, initial, zero) = fixture();
        let role = ClimateLayerRole::OceanMixedLayer;
        let cancellation = BuildCancellation::new();
        for (donor_temperature, receiver_temperature) in [(-5.0, -5.0), (10.0, 10.0), (10.0, 20.0)]
        {
            let mut state = initial.clone();
            state
                .temperature_c_mut(role)
                .unwrap()
                .fill(donor_temperature);
            state.temperature_c_mut(role).unwrap()[1] = receiver_temperature;
            state.height_anomaly_m_mut(role).unwrap()[0] = 20.0;
            state.height_anomaly_m_mut(role).unwrap()[1] = -20.0;
            let mut first = zero.clone();
            let layer = first
                .layers
                .iter_mut()
                .find(|layer| layer.role == role)
                .unwrap();
            // This rate makes the final H round to a different f32 value;
            // include the ocean temperature floor to catch false violations.
            layer.height[0] = -1.2345;
            layer.height[1] = 1.2345;
            layer.temperature[0] = f64::from(layer.height[0]) * f64::from(donor_temperature);
            layer.temperature[1] = f64::from(layer.height[1]) * f64::from(donor_temperature);
            let mut second = first.clone();
            for value in &mut second
                .layers
                .iter_mut()
                .find(|layer| layer.role == role)
                .unwrap()
                .height
            {
                *value *= 0.5;
            }
            for value in &mut second
                .layers
                .iter_mut()
                .find(|layer| layer.role == role)
                .unwrap()
                .temperature
            {
                *value *= 0.5;
            }
            let advanced = combine_state(
                &grid,
                &state,
                &[(0.25, &first), (0.75, &second)],
                &cancellation,
            )
            .unwrap();
            let total = |value: &LayeredClimateState| {
                grid.cells()
                    .iter()
                    .enumerate()
                    .map(|(cell, record)| {
                        record.area_m2()
                            * ocean_layer_thickness_m(value, role, cell).unwrap()
                            * f64::from(value.temperature_c(role).unwrap()[cell])
                    })
                    .sum::<f64>()
            };
            let before = total(&state);
            let after = total(&advanced);
            assert!((after - before).abs() <= f64::from(f32::EPSILON) * before.abs());
            if receiver_temperature == donor_temperature {
                assert!(advanced
                    .temperature_c(role)
                    .unwrap()
                    .iter()
                    .all(|value| *value == donor_temperature));
            }
        }
    }
}
