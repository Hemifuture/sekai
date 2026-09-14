use super::rk3::{
    apply_scalar_endpoint, conservative_ocean_layer, copy_scalars, estimate_cfl,
    ocean_mass_source_heat_j, rk3_step_with, rk3_step_with_first, validate_step, ClimateDerivative,
    ClimateIntegratorDiagnostics, ClimateIntegratorError, ClimateStepResult,
};
use super::{
    ClimateConservationInterpretation, ClimateIntegrationProcedure, FormationProcedureIdentity,
    GlobalCirculationPhase, LayeredClimateState, LayeredClimateTendency, LayeredTendencySystem,
    LayeredTendencyWorkspace,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::natural::{
    ClimateCapabilitySet, ClimateModelProfile, PlanetForcing, GLOBAL_CIRCULATION_FAST_CFL_TARGET,
    GLOBAL_CIRCULATION_MAXIMUM_SLOW_STEP_SECONDS,
};

/// Slow/fast additive RK3 with one frozen slow tendency per macro step.
#[derive(Debug, Clone, Copy)]
pub struct SplitExplicitRk3Integrator<'grid> {
    grid: &'grid CubedSphereGrid,
    tendency_system: LayeredTendencySystem<'grid>,
    maximum_fast_step_seconds: f64,
}

impl<'grid> SplitExplicitRk3Integrator<'grid> {
    /// Splits a validated forcing-phase duration into equal slow steps.
    /// Generation uses this same plan to account for every retained endpoint.
    pub(crate) fn slow_step_plan(macro_step_seconds: f64) -> (u32, f64) {
        let count =
            (macro_step_seconds / GLOBAL_CIRCULATION_MAXIMUM_SLOW_STEP_SECONDS).ceil() as u32;
        (count, macro_step_seconds / f64::from(count))
    }

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

    pub(crate) fn new_with_terrain(
        grid: &'grid CubedSphereGrid,
        forcing: &'grid super::forcing::GlobalClimateForcing,
        terrain_floor_m: &'grid [f32],
        maximum_fast_step_seconds: f64,
    ) -> Result<Self, ClimateIntegratorError> {
        Self::with_tendency_system(
            grid,
            LayeredTendencySystem::with_terrain(
                grid,
                forcing.terrain_gradient_m_per_m(),
                terrain_floor_m,
                forcing.land_evapotranspiration_fraction(),
                forcing.sea_level_m(),
            ),
            maximum_fast_step_seconds,
        )
    }

    /// Uses a tendency system constructed for this same grid and validated
    /// terrain fields. Returns the existing invalid-fast-step error when the
    /// supplied fast-step bound is not finite and positive.
    pub(super) fn with_tendency_system(
        grid: &'grid CubedSphereGrid,
        tendency_system: LayeredTendencySystem<'grid>,
        maximum_fast_step_seconds: f64,
    ) -> Result<Self, ClimateIntegratorError> {
        let mut integrator = Self::new(grid, maximum_fast_step_seconds)?;
        integrator.tendency_system = tendency_system;
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
            ClimateIntegrationProcedure::SplitExplicitRk3V1,
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
        let initial_fast = system.evaluate_fast_with_workspace_validated(
            state,
            forcing,
            ocean_edge_permeability,
            cancellation,
            &mut fast_workspace,
        )?;
        let (substeps, fast_step_seconds) = self.fast_substep_plan(
            state,
            macro_step_seconds,
            initial_fast.momentum_transport_rate_s_inv(),
            cancellation,
        )?;
        drop(initial_fast);
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
                0,
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
        let (slow_step_count, slow_step_seconds) = Self::slow_step_plan(macro_step_seconds);
        if slow_step_count == 1 {
            validate_step(self.grid, state, macro_step_seconds, cancellation)?;
            let full = self.tendency_system.evaluate_with_workspace_for_step(
                state,
                forcing,
                ocean_edge_permeability,
                month,
                macro_step_seconds,
                cancellation,
                &mut full_workspace,
            )?;
            return self.advance_single_slow_step_with_phase_observer(
                state,
                forcing,
                ocean_edge_permeability,
                macro_step_seconds,
                cancellation,
                observer,
                &full,
                &mut fast_workspace,
            );
        }

        let mut advanced: Option<LayeredClimateState> = None;
        let mut diagnostics = ClimateIntegratorDiagnostics::default();
        let mut precipitation_integral = vec![0.0_f64; self.grid.cell_count()];
        let mut source_heat = 0.0;
        for _ in 0..slow_step_count {
            if cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            let input = advanced.as_ref().unwrap_or(state);
            validate_step(self.grid, input, slow_step_seconds, cancellation)?;
            let full = self.tendency_system.evaluate_with_workspace_for_step(
                input,
                forcing,
                ocean_edge_permeability,
                month,
                slow_step_seconds,
                cancellation,
                &mut full_workspace,
            )?;
            let result = self.advance_single_slow_step_with_phase_observer(
                input,
                forcing,
                ocean_edge_permeability,
                slow_step_seconds,
                cancellation,
                observer,
                &full,
                &mut fast_workspace,
            )?;
            drop(full);
            diagnostics.accumulate(result.diagnostics());
            source_heat += result.ocean_mass_source_heat_j();
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
        )
        .with_ocean_mass_source_heat_j(source_heat))
    }

    /// Advances one production macro step from the exact full tendency that
    /// the generation driver already retained for its conservation ledger.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn advance_with_declared_tendency_and_phase_observer<F>(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        macro_step_seconds: f64,
        declared_full: &LayeredClimateTendency,
        cancellation: &BuildCancellation,
        observer: &mut F,
    ) -> Result<ClimateStepResult, ClimateIntegratorError>
    where
        F: FnMut(GlobalCirculationPhase),
    {
        validate_step(self.grid, state, macro_step_seconds, cancellation)?;
        if Self::slow_step_plan(macro_step_seconds).0 != 1 {
            return Err(ClimateIntegratorError::InvalidTimeStep {
                found: macro_step_seconds,
            });
        }
        let mut fast_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        self.advance_single_slow_step_with_phase_observer(
            state,
            forcing,
            ocean_edge_permeability,
            macro_step_seconds,
            cancellation,
            observer,
            declared_full,
            &mut fast_workspace,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn advance_single_slow_step_with_phase_observer<F>(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        macro_step_seconds: f64,
        cancellation: &BuildCancellation,
        observer: &mut F,
        full: &LayeredClimateTendency,
        fast_workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<ClimateStepResult, ClimateIntegratorError>
    where
        F: FnMut(GlobalCirculationPhase),
    {
        let system = self.tendency_system;

        let (mut substeps, mut fast_step_seconds) = self.fast_substep_plan(
            state,
            macro_step_seconds,
            full.momentum_transport_rate_s_inv(),
            cancellation,
        )?;
        let mut endpoint_cfl = 0.0_f64;
        let mut fast = system.evaluate_fast_with_workspace_validated(
            state,
            forcing,
            ocean_edge_permeability,
            cancellation,
            fast_workspace,
        )?;
        if let Some(exchange) = full.overturning_exchange_m_s() {
            system.apply_declared_overturning_momentum(state, exchange, cancellation, &mut fast)?;
        }
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
        apply_scalar_endpoint(
            state,
            &slow,
            macro_step_seconds,
            &mut advanced,
            cancellation,
        )?;
        let source_heat = ocean_mass_source_heat_j(self.grid, state, &advanced, cancellation)?;
        if state.profile() == ClimateModelProfile::C1SingleLayerV1 {
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
        }
        clear_scalar_components(&mut slow, state.profile());
        // Atmospheric T is fixed from this scalar endpoint through the fast
        // stages. These two arrays live only for this slow step; H and ocean T
        // remain stage-dependent, and no cache survives into the next endpoint.
        let endpoint_temperature = if state.profile() == ClimateModelProfile::C2LayeredV1 {
            Some(system.atmospheric_temperature_gradients(
                &advanced,
                Some(forcing),
                cancellation,
            )?)
        } else {
            None
        };
        let first_fast_plus_slow = if state.profile() == ClimateModelProfile::C2LayeredV1 {
            let mut endpoint_fast = system.evaluate_fast_with_temperature_gradients_validated(
                &advanced,
                forcing,
                ocean_edge_permeability,
                cancellation,
                (&mut *fast_workspace, endpoint_temperature.as_ref()),
            )?;
            if let Some(exchange) = full.overturning_exchange_m_s() {
                system.apply_declared_overturning_momentum(
                    &advanced,
                    exchange,
                    cancellation,
                    &mut endpoint_fast,
                )?;
            }
            // The scalar endpoint can change thermal wave speed and actual
            // ocean depth. Retain the stricter of entry and endpoint plans.
            let endpoint_plan = self.fast_substep_plan(
                &advanced,
                macro_step_seconds,
                endpoint_fast.momentum_transport_rate_s_inv(),
                cancellation,
            )?;
            if endpoint_plan.0 > substeps {
                (substeps, fast_step_seconds) = endpoint_plan;
            }
            endpoint_cfl = estimate_cfl(self.grid, &advanced, fast_step_seconds, cancellation)?;
            ClimateDerivative::from_tendency(&advanced, &endpoint_fast, cancellation)?
                .add(&slow, cancellation)?
        } else {
            fast_derivative.add(&slow, cancellation)?
        };
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
                let mut value = system.evaluate_fast_with_temperature_gradients_validated(
                    stage,
                    forcing,
                    ocean_edge_permeability,
                    cancellation,
                    (&mut *fast_workspace, endpoint_temperature.as_ref()),
                )?;
                if let Some(exchange) = full.overturning_exchange_m_s() {
                    system.apply_declared_overturning_momentum(
                        stage,
                        exchange,
                        cancellation,
                        &mut value,
                    )?;
                }
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
                1,
                substeps,
                estimate_cfl(self.grid, state, fast_step_seconds, cancellation)?.max(endpoint_cfl),
            ),
            copy_scalars(full.precipitation_rate_mm_s(), cancellation)?,
        )
        .with_ocean_mass_source_heat_j(source_heat))
    }

    fn fast_substep_plan(
        &self,
        state: &LayeredClimateState,
        macro_step_seconds: f64,
        momentum_exchange_rate_s_inv: f64,
        cancellation: &BuildCancellation,
    ) -> Result<(u32, f64), ClimateIntegratorError> {
        let configured_cfl = estimate_cfl(
            self.grid,
            state,
            self.maximum_fast_step_seconds,
            cancellation,
        )?
        .max(self.maximum_fast_step_seconds * momentum_exchange_rate_s_inv);
        let cfl_limited_step = if configured_cfl > GLOBAL_CIRCULATION_FAST_CFL_TARGET {
            self.maximum_fast_step_seconds * GLOBAL_CIRCULATION_FAST_CFL_TARGET / configured_cfl
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

fn clear_scalar_components(derivative: &mut ClimateDerivative, profile: ClimateModelProfile) {
    for layer in &mut derivative.layers {
        layer.temperature.fill(0.0);
        if conservative_ocean_layer(profile, layer.role) {
            layer.height.fill(0.0);
        }
    }
    derivative.humidity.fill(0.0);
    if let Some(upper_humidity) = &mut derivative.upper_humidity {
        upper_humidity.fill(0.0);
    }
    if let Some(deep_temperature) = &mut derivative.deep_temperature {
        deep_temperature.fill(0.0);
    }
}
