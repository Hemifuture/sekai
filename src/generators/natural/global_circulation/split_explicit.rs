use super::rk3::{
    copy_scalars, estimate_cfl, rk3_step_with, rk3_step_with_first, validate_step,
    ClimateDerivative, ClimateIntegratorDiagnostics, ClimateIntegratorError, ClimateStepResult,
};
use super::{
    ClimateConservationInterpretation, FormationProcedureIdentity, GlobalCirculationPhase,
    LayeredClimateState, LayeredClimateTendency, LayeredTendencySystem, LayeredTendencyWorkspace,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::natural::{ClimateCapabilitySet, ClimateModelProfile, PlanetForcing};

const FAST_CFL_TARGET: f64 = 0.20;
const MAXIMUM_SLOW_STEP_SECONDS: f64 = 7_200.0;

/// Slow/fast additive RK3 with one frozen slow tendency per macro step.
#[derive(Debug, Clone, Copy)]
pub struct SplitExplicitRk3Integrator<'grid> {
    grid: &'grid CubedSphereGrid,
    tendency_system: LayeredTendencySystem<'grid>,
    maximum_fast_step_seconds: f64,
}

impl<'grid> SplitExplicitRk3Integrator<'grid> {
    pub fn new(
        grid: &'grid CubedSphereGrid,
        maximum_fast_step_seconds: f64,
    ) -> Result<Self, ClimateIntegratorError> {
        if !maximum_fast_step_seconds.is_finite() || maximum_fast_step_seconds <= 0.0 {
            return Err(ClimateIntegratorError::InvalidFastStep {
                found: maximum_fast_step_seconds,
            });
        }
        Ok(Self {
            grid,
            tendency_system: LayeredTendencySystem::new(grid),
            maximum_fast_step_seconds,
        })
    }

    pub(crate) fn new_with_terrain_gradient(
        grid: &'grid CubedSphereGrid,
        terrain_gradient_m_per_m: &'grid [[f32; 3]],
        maximum_fast_step_seconds: f64,
    ) -> Result<Self, ClimateIntegratorError> {
        let mut integrator = Self::new(grid, maximum_fast_step_seconds)?;
        integrator.tendency_system =
            LayeredTendencySystem::with_terrain_gradient(grid, terrain_gradient_m_per_m);
        Ok(integrator)
    }

    pub(crate) const fn tendency_system(&self) -> LayeredTendencySystem<'grid> {
        self.tendency_system
    }

    /// Declares the scientific capabilities and conservation ledger owned by
    /// the actual selected split-explicit implementation.
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
        macro_step_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<ClimateStepResult, ClimateIntegratorError> {
        self.advance_with_phase_observer(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            macro_step_seconds,
            cancellation,
            &mut |_| {},
        )
    }

    /// Advances only the closed pressure/divergence/Coriolis subsystem.
    ///
    /// This uses the same split-explicit RK3 fast kernel as production while
    /// deliberately excluding every declared external source or sink. It is
    /// the locked analytic conservation path, not a product integration mode.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_closed_no_source(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        macro_step_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<ClimateStepResult, ClimateIntegratorError> {
        validate_step(self.grid, state, macro_step_seconds, cancellation)?;
        let (substeps, fast_step_seconds) =
            self.fast_substep_plan(state, macro_step_seconds, cancellation)?;
        let system = self.tendency_system;
        let mut fast_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        system.validate_fast_inputs(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            &fast_workspace,
        )?;
        let mut advanced = state.clone_cancellable(cancellation)?;
        let mut evaluations = 0_u64;
        for _ in 0..substeps {
            if cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            advanced = rk3_step_with(
                self.grid,
                &advanced,
                fast_step_seconds,
                cancellation,
                |stage| {
                    evaluations += 1;
                    let value = system.evaluate_fast_with_workspace_validated(
                        stage,
                        forcing,
                        ocean_edge_permeability,
                        cancellation,
                        &mut fast_workspace,
                    )?;
                    ClimateDerivative::from_tendency(stage, &value, cancellation)
                },
            )?;
        }
        advanced.validate_against_cancellable(self.grid, cancellation)?;
        Ok(ClimateStepResult::new(
            advanced,
            ClimateIntegratorDiagnostics::split(
                evaluations,
                substeps,
                estimate_cfl(self.grid, state, fast_step_seconds, cancellation)?,
            ),
            vec![0.0; self.grid.cell_count()],
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn advance_with_phase_observer<F>(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        macro_step_seconds: f64,
        cancellation: &BuildCancellation,
        observer: &mut F,
    ) -> Result<ClimateStepResult, ClimateIntegratorError>
    where
        F: FnMut(GlobalCirculationPhase),
    {
        validate_step(self.grid, state, macro_step_seconds, cancellation)?;
        let mut full_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        let mut fast_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        if macro_step_seconds <= MAXIMUM_SLOW_STEP_SECONDS {
            return self.advance_single_slow_step_with_phase_observer(
                state,
                forcing,
                ocean_edge_permeability,
                month,
                macro_step_seconds,
                cancellation,
                observer,
                None,
                &mut full_workspace,
                &mut fast_workspace,
            );
        }

        let slow_step_count = (macro_step_seconds / MAXIMUM_SLOW_STEP_SECONDS).ceil() as u32;
        let slow_step_seconds = macro_step_seconds / f64::from(slow_step_count);
        let mut advanced: Option<LayeredClimateState> = None;
        let mut diagnostics = ClimateIntegratorDiagnostics::default();
        let mut precipitation_integral = vec![0.0_f64; self.grid.cell_count()];
        for _ in 0..slow_step_count {
            if cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            let input = advanced.as_ref().unwrap_or(state);
            let result = self.advance_single_slow_step_with_phase_observer(
                input,
                forcing,
                ocean_edge_permeability,
                month,
                slow_step_seconds,
                cancellation,
                observer,
                None,
                &mut full_workspace,
                &mut fast_workspace,
            )?;
            diagnostics.accumulate(result.diagnostics());
            for (cell, precipitation) in result.mean_precipitation_rate_mm_s().iter().enumerate() {
                if cell % 256 == 0 && cancellation.is_cancelled() {
                    return Err(ClimateIntegratorError::Cancelled);
                }
                precipitation_integral[cell] += f64::from(*precipitation) * slow_step_seconds;
            }
            advanced = Some(result.into_state());
        }
        let mut mean_precipitation_rate_mm_s = Vec::with_capacity(self.grid.cell_count());
        for (cell, integral) in precipitation_integral.into_iter().enumerate() {
            if cell % 256 == 0 && cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            mean_precipitation_rate_mm_s.push((integral / macro_step_seconds) as f32);
        }
        Ok(ClimateStepResult::new(
            advanced.expect("positive slow-step count"),
            diagnostics,
            mean_precipitation_rate_mm_s,
        ))
    }

    /// Advances one production macro step from the exact full tendency that
    /// the generation driver already retained for its conservation ledger.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn advance_with_declared_tendency_and_phase_observer<F>(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        macro_step_seconds: f64,
        declared_full: &LayeredClimateTendency,
        cancellation: &BuildCancellation,
        observer: &mut F,
    ) -> Result<ClimateStepResult, ClimateIntegratorError>
    where
        F: FnMut(GlobalCirculationPhase),
    {
        validate_step(self.grid, state, macro_step_seconds, cancellation)?;
        if macro_step_seconds > MAXIMUM_SLOW_STEP_SECONDS {
            return Err(ClimateIntegratorError::InvalidTimeStep {
                found: macro_step_seconds,
            });
        }
        let mut full_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        let mut fast_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        self.advance_single_slow_step_with_phase_observer(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            macro_step_seconds,
            cancellation,
            observer,
            Some(declared_full),
            &mut full_workspace,
            &mut fast_workspace,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn advance_single_slow_step_with_phase_observer<F>(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        macro_step_seconds: f64,
        cancellation: &BuildCancellation,
        observer: &mut F,
        declared_full: Option<&LayeredClimateTendency>,
        full_workspace: &mut LayeredTendencyWorkspace,
        fast_workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<ClimateStepResult, ClimateIntegratorError>
    where
        F: FnMut(GlobalCirculationPhase),
    {
        validate_step(self.grid, state, macro_step_seconds, cancellation)?;
        let (substeps, fast_step_seconds) =
            self.fast_substep_plan(state, macro_step_seconds, cancellation)?;
        let system = self.tendency_system;

        let evaluated_full = if declared_full.is_none() {
            Some(system.evaluate_with_workspace_for_step(
                state,
                forcing,
                ocean_edge_permeability,
                month,
                macro_step_seconds,
                cancellation,
                full_workspace,
            )?)
        } else {
            None
        };
        let full = declared_full
            .or(evaluated_full.as_ref())
            .expect("full tendency is supplied or evaluated");
        let fast = system.evaluate_fast_with_workspace_validated(
            state,
            forcing,
            ocean_edge_permeability,
            cancellation,
            fast_workspace,
        )?;
        let full_derivative = ClimateDerivative::from_tendency(state, full, cancellation)?;
        let fast_derivative = ClimateDerivative::from_tendency(state, &fast, cancellation)?;
        let mut slow = full_derivative.subtract(&fast_derivative, cancellation)?;
        // Transport and phase-change closures diagnose a conservative scalar
        // endpoint over the declared physical step; they are not autonomous
        // RK stage derivatives. Apply that endpoint once, replace the frozen
        // thermal-pressure contribution with its post-endpoint value, then
        // retain that slow dynamical background through the fast stages.
        // Explicit drops keep the live owner inventory used by the public
        // memory report mechanically true.
        drop(full_derivative);
        drop(fast);
        let mut advanced = state.clone_cancellable(cancellation)?;
        apply_frozen_slow_scalar_endpoint(
            state,
            &slow,
            macro_step_seconds,
            &mut advanced,
            cancellation,
        )?;
        let thermal_pressure_difference = system
            .evaluate_thermal_pressure_endpoint_difference_with_workspace_validated(
                state,
                &advanced,
                ocean_edge_permeability,
                cancellation,
                fast_workspace,
            )?;
        let thermal_pressure_difference = ClimateDerivative::from_tendency(
            &advanced,
            &thermal_pressure_difference,
            cancellation,
        )?;
        slow = slow.add(&thermal_pressure_difference, cancellation)?;
        clear_scalar_components(&mut slow);
        let first_fast_plus_slow = fast_derivative.add(&slow, cancellation)?;
        drop(thermal_pressure_difference);
        drop(fast_derivative);
        let mut evaluations = 3_u64;
        let mut first_fast_plus_slow = Some(first_fast_plus_slow);
        observer(GlobalCirculationPhase::FastSubstepsStarted);
        if cancellation.is_cancelled() {
            return Err(ClimateIntegratorError::Cancelled);
        }
        for _ in 0..substeps {
            if cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            let mut evaluate = |stage: &LayeredClimateState| {
                evaluations += 1;
                let value = system.evaluate_fast_with_workspace_validated(
                    stage,
                    forcing,
                    ocean_edge_permeability,
                    cancellation,
                    fast_workspace,
                )?;
                ClimateDerivative::from_tendency(stage, &value, cancellation)?
                    .add(&slow, cancellation)
            };
            advanced = if let Some(first) = first_fast_plus_slow.take() {
                rk3_step_with_first(
                    self.grid,
                    &advanced,
                    fast_step_seconds,
                    cancellation,
                    first,
                    &mut evaluate,
                )?
            } else {
                rk3_step_with(
                    self.grid,
                    &advanced,
                    fast_step_seconds,
                    cancellation,
                    &mut evaluate,
                )?
            };
            observer(GlobalCirculationPhase::FastSubstepCompleted);
        }
        advanced.validate_against_cancellable(self.grid, cancellation)?;
        Ok(ClimateStepResult::new(
            advanced,
            ClimateIntegratorDiagnostics::split(
                evaluations,
                substeps,
                estimate_cfl(self.grid, state, fast_step_seconds, cancellation)?,
            ),
            copy_scalars(full.precipitation_rate_mm_s(), cancellation)?,
        ))
    }

    fn fast_substep_plan(
        &self,
        state: &LayeredClimateState,
        macro_step_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<(u32, f64), ClimateIntegratorError> {
        let configured_cfl = estimate_cfl(
            self.grid,
            state,
            self.maximum_fast_step_seconds,
            cancellation,
        )?;
        let cfl_limited_step = if configured_cfl > FAST_CFL_TARGET {
            self.maximum_fast_step_seconds * FAST_CFL_TARGET / configured_cfl
        } else {
            self.maximum_fast_step_seconds
        };
        let substeps_f64 = (macro_step_seconds / cfl_limited_step).ceil().max(1.0);
        if substeps_f64 > f64::from(u32::MAX) {
            return Err(ClimateIntegratorError::InvalidTimeStep {
                found: macro_step_seconds,
            });
        }
        let substeps = substeps_f64 as u32;
        Ok((substeps, macro_step_seconds / f64::from(substeps)))
    }
}

fn clear_scalar_components(derivative: &mut ClimateDerivative) {
    for layer in &mut derivative.layers {
        layer.temperature.fill(0.0);
    }
    derivative.humidity.fill(0.0);
    if let Some(upper_humidity) = &mut derivative.upper_humidity {
        upper_humidity.fill(0.0);
    }
    if let Some(deep_temperature) = &mut derivative.deep_temperature {
        deep_temperature.fill(0.0);
    }
}

fn apply_frozen_slow_scalar_endpoint(
    initial: &LayeredClimateState,
    slow: &ClimateDerivative,
    step_seconds: f64,
    advanced: &mut LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<(), ClimateIntegratorError> {
    let quantize = |before: f32, tendency: f32| -> Result<f32, ClimateIntegratorError> {
        let value = f64::from(before) + step_seconds * f64::from(tendency);
        if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
            return Err(ClimateIntegratorError::LinearSolveBreakdown);
        }
        Ok(value as f32)
    };
    for layer in &slow.layers {
        let before = initial.temperature_c(layer.role).expect("active role");
        let after = advanced.temperature_c_mut(layer.role).expect("active role");
        for (cell, target) in after.iter_mut().enumerate() {
            if cell % 256 == 0 && cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            *target = quantize(before[cell], layer.temperature[cell])?;
        }
    }
    for (cell, target) in advanced.specific_humidity_mut().iter_mut().enumerate() {
        if cell % 256 == 0 && cancellation.is_cancelled() {
            return Err(ClimateIntegratorError::Cancelled);
        }
        *target = quantize(initial.specific_humidity()[cell], slow.humidity[cell])?.max(0.0);
    }
    if let (Some(before), Some(tendency), Some(after)) = (
        initial.upper_specific_humidity(),
        slow.upper_humidity.as_ref(),
        advanced.upper_specific_humidity_mut(),
    ) {
        for (cell, target) in after.iter_mut().enumerate() {
            if cell % 256 == 0 && cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            *target = quantize(before[cell], tendency[cell])?.max(0.0);
        }
    }
    if let (Some(before), Some(tendency), Some(after)) = (
        initial.deep_ocean_temperature_c(),
        slow.deep_temperature.as_ref(),
        advanced.deep_ocean_temperature_c_mut(),
    ) {
        for (cell, target) in after.iter_mut().enumerate() {
            if cell % 256 == 0 && cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            *target = quantize(before[cell], tendency[cell])?;
        }
    }
    if cancellation.is_cancelled() {
        return Err(ClimateIntegratorError::Cancelled);
    }
    Ok(())
}
