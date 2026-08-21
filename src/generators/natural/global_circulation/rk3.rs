use thiserror::Error;

use super::{
    ClimateConservationInterpretation, FormationProcedureIdentity, LayeredClimateState,
    LayeredClimateTendency, LayeredStateError, LayeredTendencyError, LayeredTendencySystem,
    LayeredTendencyWorkspace,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
use crate::world::natural::{
    ClimateCapabilitySet, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
};

const REFERENCE_WAVE_SPEED_M_S: f64 = 65.0;
const EARTH_ROTATION_RATE_RAD_S: f64 = 7.292_115_9e-5;
pub(super) const FORMATION_TEMPERATURE_SCALE_K: f64 = 30.0;
pub(super) const FORMATION_ATMOSPHERE_SPEED_SCALE_M_S: f64 = 20.0;
pub(super) const FORMATION_OCEAN_SPEED_SCALE_M_S: f64 = 2.0;
pub(super) const FORMATION_SPECIFIC_HUMIDITY_SCALE: f64 = 0.02;

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
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

    pub(crate) fn accumulate(&mut self, other: Self) {
        self.tendency_evaluations = self
            .tendency_evaluations
            .saturating_add(other.tendency_evaluations);
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
        let mut evaluations = 0_u64;
        let mut stage_precipitation = Vec::with_capacity(3);
        let advanced = rk3_step_with(self.grid, state, dt_seconds, cancellation, |stage| {
            evaluations += 1;
            let tendency = system.evaluate_with_workspace_for_step(
                stage,
                forcing,
                ocean_edge_permeability,
                month,
                dt_seconds,
                cancellation,
                &mut workspace,
            )?;
            stage_precipitation.push(copy_scalars(
                tendency.precipitation_rate_mm_s(),
                cancellation,
            )?);
            ClimateDerivative::from_tendency(stage, &tendency, cancellation)
        })?;
        let mut mean_precipitation_rate_mm_s = Vec::with_capacity(self.grid.cell_count());
        let [first, second, third] = stage_precipitation.as_slice() else {
            unreachable!("classic RK3 always evaluates exactly three precipitation stages")
        };
        for (cell, ((first, second), third)) in first.iter().zip(second).zip(third).enumerate() {
            poll_integrator_cancelled(cell, Some(cancellation))?;
            mean_precipitation_rate_mm_s.push(
                (f64::from(*first) / 6.0 + 2.0 * f64::from(*second) / 3.0 + f64::from(*third) / 6.0)
                    as f32,
            );
        }
        Ok(ClimateStepResult::new(
            advanced,
            ClimateIntegratorDiagnostics::explicit(
                evaluations,
                estimate_cfl(self.grid, state, dt_seconds, cancellation)?,
            ),
            mean_precipitation_rate_mm_s,
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
                temperature: copy_scalars(
                    tendency
                        .temperature_tendency_k_s(*role)
                        .expect("active tendency role"),
                    cancellation,
                )?,
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
        let mut velocity = Vec::with_capacity(base.cell_count());
        for (index, original) in base_velocity.iter().copied().enumerate() {
            poll_integrator_cancelled(index, Some(cancellation))?;
            let mut value = [0.0_f32; 3];
            for component in 0..3 {
                value[component] = accumulate_scalar(original[component], terms, |derivative| {
                    derivative.layer(*role).velocity[index][component]
                })?;
            }
            velocity.push(value);
        }
        let tangent = CirculationOperators::new(grid)
            .tangentize_cancellable(&velocity, cancellation)
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
            poll_integrator_cancelled(index, Some(cancellation))?;
            *target = accumulate_scalar(base_temperature[index], terms, |derivative| {
                derivative.layer(*role).temperature[index]
            })?;
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
    values: &[[f32; 3]],
    cancellation: &BuildCancellation,
) -> Result<Vec<[f32; 3]>, ClimateIntegratorError> {
    let mut copy = Vec::with_capacity(values.len());
    for (index, value) in values.iter().copied().enumerate() {
        poll_integrator_cancelled(index, Some(cancellation))?;
        copy.push(value);
    }
    Ok(copy)
}

fn combine_scalars(
    left: &[f32],
    right: &[f32],
    cancellation: &BuildCancellation,
    combine: impl Fn(f32, f32) -> f32,
) -> Result<Vec<f32>, ClimateIntegratorError> {
    debug_assert_eq!(left.len(), right.len());
    let mut result = Vec::with_capacity(left.len());
    for (index, (&left, &right)) in left.iter().zip(right).enumerate() {
        poll_integrator_cancelled(index, Some(cancellation))?;
        result.push(combine(left, right));
    }
    Ok(result)
}

fn combine_vectors(
    left: &[[f32; 3]],
    right: &[[f32; 3]],
    cancellation: &BuildCancellation,
    combine: impl Fn(f32, f32) -> f32,
) -> Result<Vec<[f32; 3]>, ClimateIntegratorError> {
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
    let advective =
        dt_seconds * (REFERENCE_WAVE_SPEED_M_S + maximum_speed) / grid.minimum_center_distance_m();
    let rotational = dt_seconds * 2.0 * EARTH_ROTATION_RATE_RAD_S;
    check_integrator_cancelled(Some(cancellation))?;
    Ok(advective.max(rotational))
}
