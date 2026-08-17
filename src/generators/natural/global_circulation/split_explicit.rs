use super::rk3::{
    estimate_cfl, rk3_step_with, validate_step, ClimateDerivative, ClimateIntegratorDiagnostics,
    ClimateIntegratorError, ClimateStepResult,
};
use super::{LayeredClimateState, LayeredTendencySystem, LayeredTendencyWorkspace};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::CubedSphereGrid;
use crate::world::natural::PlanetForcing;

const FAST_CFL_TARGET: f64 = 0.35;

/// Slow/fast additive RK3 with one frozen slow tendency per macro step.
#[derive(Debug, Clone, Copy)]
pub struct SplitExplicitRk3Integrator<'grid> {
    grid: &'grid CubedSphereGrid,
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
            maximum_fast_step_seconds,
        })
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
        validate_step(self.grid, state, macro_step_seconds, cancellation)?;
        let configured_cfl = estimate_cfl(self.grid, state, self.maximum_fast_step_seconds);
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
        let fast_step_seconds = macro_step_seconds / f64::from(substeps);
        let system = LayeredTendencySystem::new(self.grid);
        let mut full_workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        let mut fast_workspace = LayeredTendencyWorkspace::for_grid(self.grid);

        let full = system.evaluate_with_workspace(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            &mut full_workspace,
        )?;
        let fast = system.evaluate_fast_with_workspace(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            &mut fast_workspace,
        )?;
        let slow = ClimateDerivative::from_tendency(state, &full)
            .subtract(&ClimateDerivative::from_tendency(state, &fast));

        let mut advanced = state.clone();
        let mut evaluations = 2_u64;
        for _ in 0..substeps {
            if cancellation.is_cancelled() {
                return Err(ClimateIntegratorError::Cancelled);
            }
            advanced = rk3_step_with(self.grid, &advanced, fast_step_seconds, |stage| {
                evaluations += 1;
                system
                    .evaluate_fast_with_workspace(
                        stage,
                        forcing,
                        ocean_edge_permeability,
                        month,
                        cancellation,
                        &mut fast_workspace,
                    )
                    .map(|value| ClimateDerivative::from_tendency(stage, &value).add(&slow))
                    .map_err(ClimateIntegratorError::from)
            })?;
        }
        Ok(ClimateStepResult::new(
            advanced,
            ClimateIntegratorDiagnostics::split(
                evaluations,
                substeps,
                estimate_cfl(self.grid, state, fast_step_seconds),
            ),
        ))
    }
}
