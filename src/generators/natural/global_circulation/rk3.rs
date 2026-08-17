use thiserror::Error;

use super::{
    LayeredClimateState, LayeredClimateTendency, LayeredStateError, LayeredTendencyError,
    LayeredTendencySystem, LayeredTendencyWorkspace,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
use crate::world::natural::{ClimateLayerRole, PlanetForcing};

const REFERENCE_WAVE_SPEED_M_S: f64 = 65.0;
const EARTH_ROTATION_RATE_RAD_S: f64 = 7.292_115_9e-5;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ClimateIntegratorDiagnostics {
    tendency_evaluations: u64,
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

    pub(crate) const fn explicit(tendency_evaluations: u64, cfl: f64) -> Self {
        Self {
            tendency_evaluations,
            fast_substeps: 1,
            linear_iterations: 0,
            initial_linear_relative_residual: 0.0,
            final_linear_relative_residual: 0.0,
            maximum_cfl: cfl,
        }
    }

    pub(crate) const fn split(tendency_evaluations: u64, substeps: u32, cfl: f64) -> Self {
        Self {
            tendency_evaluations,
            fast_substeps: substeps,
            linear_iterations: 0,
            initial_linear_relative_residual: 0.0,
            final_linear_relative_residual: 0.0,
            maximum_cfl: cfl,
        }
    }

    pub(crate) const fn imex(
        tendency_evaluations: u64,
        iterations: u16,
        initial_residual: f64,
        final_residual: f64,
        cfl: f64,
    ) -> Self {
        Self {
            tendency_evaluations,
            fast_substeps: 1,
            linear_iterations: iterations,
            initial_linear_relative_residual: initial_residual,
            final_linear_relative_residual: final_residual,
            maximum_cfl: cfl,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClimateStepResult {
    state: LayeredClimateState,
    diagnostics: ClimateIntegratorDiagnostics,
}

impl ClimateStepResult {
    pub(crate) const fn new(
        state: LayeredClimateState,
        diagnostics: ClimateIntegratorDiagnostics,
    ) -> Self {
        Self { state, diagnostics }
    }

    pub const fn state(&self) -> &LayeredClimateState {
        &self.state
    }

    pub const fn diagnostics(&self) -> ClimateIntegratorDiagnostics {
        self.diagnostics
    }

    pub fn into_state(self) -> LayeredClimateState {
        self.state
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
    State(#[from] LayeredStateError),
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

#[derive(Debug, Clone, Copy)]
pub struct ExplicitRk3Integrator<'grid> {
    grid: &'grid CubedSphereGrid,
}

impl<'grid> ExplicitRk3Integrator<'grid> {
    pub const fn new(grid: &'grid CubedSphereGrid) -> Self {
        Self { grid }
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
        let mut evaluations = 0_u64;
        let advanced = rk3_step_with(self.grid, state, dt_seconds, |stage| {
            evaluations += 1;
            system
                .evaluate_with_workspace(
                    stage,
                    forcing,
                    ocean_edge_permeability,
                    month,
                    cancellation,
                    &mut workspace,
                )
                .map(|value| ClimateDerivative::from_tendency(stage, &value))
                .map_err(ClimateIntegratorError::from)
        })?;
        Ok(ClimateStepResult::new(
            advanced,
            ClimateIntegratorDiagnostics::explicit(
                evaluations,
                estimate_cfl(self.grid, state, dt_seconds),
            ),
        ))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayerDerivative {
    pub(crate) role: ClimateLayerRole,
    pub(crate) height: Vec<f32>,
    pub(crate) velocity: Vec<[f32; 3]>,
    pub(crate) temperature: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClimateDerivative {
    pub(crate) layers: Vec<LayerDerivative>,
    pub(crate) humidity: Vec<f32>,
    pub(crate) deep_temperature: Option<Vec<f32>>,
}

impl ClimateDerivative {
    pub(crate) fn from_tendency(
        state: &LayeredClimateState,
        tendency: &LayeredClimateTendency,
    ) -> Self {
        Self {
            layers: state
                .active_roles()
                .iter()
                .map(|role| LayerDerivative {
                    role: *role,
                    height: tendency
                        .height_tendency_m_s(*role)
                        .expect("active tendency role")
                        .to_vec(),
                    velocity: tendency
                        .velocity_tendency_m_s2(*role)
                        .expect("active tendency role")
                        .to_vec(),
                    temperature: tendency
                        .temperature_tendency_k_s(*role)
                        .expect("active tendency role")
                        .to_vec(),
                })
                .collect(),
            humidity: tendency.specific_humidity_tendency_s_inv().to_vec(),
            deep_temperature: tendency
                .deep_ocean_temperature_tendency_k_s()
                .map(<[f32]>::to_vec),
        }
    }

    pub(crate) fn subtract(&self, other: &Self) -> Self {
        debug_assert_eq!(self.layers.len(), other.layers.len());
        Self {
            layers: self
                .layers
                .iter()
                .zip(&other.layers)
                .map(|(left, right)| LayerDerivative {
                    role: left.role,
                    height: subtract_scalars(&left.height, &right.height),
                    velocity: subtract_vectors(&left.velocity, &right.velocity),
                    temperature: subtract_scalars(&left.temperature, &right.temperature),
                })
                .collect(),
            humidity: subtract_scalars(&self.humidity, &other.humidity),
            deep_temperature: match (&self.deep_temperature, &other.deep_temperature) {
                (Some(left), Some(right)) => Some(subtract_scalars(left, right)),
                (None, None) => None,
                _ => unreachable!("matching profiles have matching deep reservoirs"),
            },
        }
    }

    pub(crate) fn add(&self, other: &Self) -> Self {
        debug_assert_eq!(self.layers.len(), other.layers.len());
        Self {
            layers: self
                .layers
                .iter()
                .zip(&other.layers)
                .map(|(left, right)| LayerDerivative {
                    role: left.role,
                    height: add_scalars(&left.height, &right.height),
                    velocity: add_vectors(&left.velocity, &right.velocity),
                    temperature: add_scalars(&left.temperature, &right.temperature),
                })
                .collect(),
            humidity: add_scalars(&self.humidity, &other.humidity),
            deep_temperature: match (&self.deep_temperature, &other.deep_temperature) {
                (Some(left), Some(right)) => Some(add_scalars(left, right)),
                (None, None) => None,
                _ => unreachable!("matching profiles have matching deep reservoirs"),
            },
        }
    }

    pub(crate) fn layer(&self, role: ClimateLayerRole) -> &LayerDerivative {
        self.layers
            .iter()
            .find(|layer| layer.role == role)
            .expect("derivative contains every active role")
    }
}

pub(crate) fn rk3_step_with<F>(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    dt_seconds: f64,
    mut evaluate: F,
) -> Result<LayeredClimateState, ClimateIntegratorError>
where
    F: FnMut(&LayeredClimateState) -> Result<ClimateDerivative, ClimateIntegratorError>,
{
    let first = evaluate(state)?;
    let stage_two = combine_state(grid, state, &[(0.5 * dt_seconds, &first)])?;
    let second = evaluate(&stage_two)?;
    let stage_three = combine_state(
        grid,
        state,
        &[(-dt_seconds, &first), (2.0 * dt_seconds, &second)],
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
    )
}

pub(crate) fn combine_state(
    grid: &CubedSphereGrid,
    base: &LayeredClimateState,
    terms: &[(f64, &ClimateDerivative)],
) -> Result<LayeredClimateState, ClimateIntegratorError> {
    let mut result = base.clone();
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
            *target = accumulate_scalar(base_height[index], terms, |derivative| {
                derivative.layer(*role).height[index]
            })?;
        }
        let mut velocity = Vec::with_capacity(base.cell_count());
        for (index, original) in base_velocity.iter().copied().enumerate() {
            let mut value = [0.0_f32; 3];
            for component in 0..3 {
                value[component] = accumulate_scalar(original[component], terms, |derivative| {
                    derivative.layer(*role).velocity[index][component]
                })?;
            }
            velocity.push(value);
        }
        let tangent = CirculationOperators::new(grid)
            .tangentize(&velocity)
            .map_err(|error| {
                ClimateIntegratorError::Tendency(LayeredTendencyError::Operator(error))
            })?;
        result
            .velocity_m_s_mut(*role)
            .expect("active role")
            .copy_from_slice(&tangent);
        for (index, target) in result
            .temperature_c_mut(*role)
            .expect("active role")
            .iter_mut()
            .enumerate()
        {
            *target = accumulate_scalar(base_temperature[index], terms, |derivative| {
                derivative.layer(*role).temperature[index]
            })?;
        }
    }
    for (index, target) in result.specific_humidity_mut().iter_mut().enumerate() {
        *target = accumulate_scalar(base.specific_humidity()[index], terms, |derivative| {
            derivative.humidity[index]
        })?
        .max(0.0);
    }
    if let (Some(base_deep), Some(result_deep)) = (
        base.deep_ocean_temperature_c(),
        result.deep_ocean_temperature_c_mut(),
    ) {
        for (index, target) in result_deep.iter_mut().enumerate() {
            *target = accumulate_scalar(base_deep[index], terms, |derivative| {
                derivative.deep_temperature.as_ref().expect("C2 derivative")[index]
            })?;
        }
    }
    result.validate_against(grid)?;
    Ok(result)
}

fn accumulate_scalar<F>(
    base: f32,
    terms: &[(f64, &ClimateDerivative)],
    mut component: F,
) -> Result<f32, ClimateIntegratorError>
where
    F: FnMut(&ClimateDerivative) -> f32,
{
    let mut value = f64::from(base);
    for (coefficient, derivative) in terms {
        value += coefficient * f64::from(component(derivative));
    }
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(ClimateIntegratorError::LinearSolveBreakdown);
    }
    Ok(value as f32)
}

fn subtract_scalars(left: &[f32], right: &[f32]) -> Vec<f32> {
    left.iter()
        .zip(right)
        .map(|(left, right)| left - right)
        .collect()
}

fn add_scalars(left: &[f32], right: &[f32]) -> Vec<f32> {
    left.iter()
        .zip(right)
        .map(|(left, right)| left + right)
        .collect()
}

fn subtract_vectors(left: &[[f32; 3]], right: &[[f32; 3]]) -> Vec<[f32; 3]> {
    left.iter()
        .zip(right)
        .map(|(left, right)| std::array::from_fn(|index| left[index] - right[index]))
        .collect()
}

fn add_vectors(left: &[[f32; 3]], right: &[[f32; 3]]) -> Vec<[f32; 3]> {
    left.iter()
        .zip(right)
        .map(|(left, right)| std::array::from_fn(|index| left[index] + right[index]))
        .collect()
}

pub fn climate_state_rms_difference(
    grid: &CubedSphereGrid,
    left: &LayeredClimateState,
    right: &LayeredClimateState,
) -> Result<f64, ClimateIntegratorError> {
    left.validate_against(grid)?;
    right.validate_against(grid)?;
    if left.profile() != right.profile() {
        return Err(ClimateIntegratorError::StateMismatch);
    }
    let mut sum = 0.0_f64;
    let mut count = 0_usize;
    for role in left.active_roles() {
        for (left, right) in left
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
        {
            sum += f64::from(*left - *right).powi(2);
            count += 1;
        }
        for (left, right) in left
            .velocity_m_s(*role)
            .expect("active role")
            .iter()
            .zip(right.velocity_m_s(*role).expect("active role"))
        {
            for component in 0..3 {
                sum += f64::from(left[component] - right[component]).powi(2);
                count += 1;
            }
        }
    }
    for (left, right) in left
        .specific_humidity()
        .iter()
        .zip(right.specific_humidity())
    {
        sum += f64::from(*left - *right).powi(2);
        count += 1;
    }
    if let (Some(left), Some(right)) = (
        left.deep_ocean_temperature_c(),
        right.deep_ocean_temperature_c(),
    ) {
        for (left, right) in left.iter().zip(right) {
            sum += f64::from(*left - *right).powi(2);
            count += 1;
        }
    }
    Ok((sum / count.max(1) as f64).sqrt())
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
    state.validate_against(grid)?;
    Ok(())
}

pub(crate) fn estimate_cfl(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    dt_seconds: f64,
) -> f64 {
    let maximum_speed = state
        .active_roles()
        .iter()
        .flat_map(|role| state.velocity_m_s(*role).expect("active role"))
        .map(|velocity| {
            velocity
                .iter()
                .map(|value| f64::from(*value).powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .fold(0.0_f64, f64::max);
    let advective =
        dt_seconds * (REFERENCE_WAVE_SPEED_M_S + maximum_speed) / grid.minimum_center_distance_m();
    let rotational = dt_seconds * 2.0 * EARTH_ROTATION_RATE_RAD_S;
    advective.max(rotational)
}
