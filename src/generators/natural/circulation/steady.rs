use crate::world::natural::{
    CirculationSnapshot, CirculationSolveStats, CirculationSolverId, CirculationSpec,
    PlanetForcing, CLIMATE_MONTH_COUNT,
};

use super::{
    dynamics::{balanced_iteration, dense_state_bytes, initial_state, CirculationState},
    solver::{validate_solver_inputs, MonthlySnapshotBuilder},
    CirculationEdgePermeability, CirculationOperators, CirculationSolveError, CirculationSolver,
    CubedSphereGrid,
};

/// Fast diagnostic balance solver using the shared spherical operators and scalar physics.
#[derive(Debug, Clone, Copy, Default)]
pub struct BalancedSteadySolver;

impl CirculationSolver for BalancedSteadySolver {
    fn id(&self) -> CirculationSolverId {
        CirculationSolverId::BalancedSteadyV1
    }

    fn solve(
        &self,
        grid: &CubedSphereGrid,
        forcing: &PlanetForcing,
        spec: &CirculationSpec,
    ) -> Result<CirculationSnapshot, CirculationSolveError> {
        let identity = validate_solver_inputs(grid, forcing, spec)?;
        let operators = CirculationOperators::new(grid);
        let permeability = CirculationEdgePermeability::from_forcing(grid, forcing)?;
        let mut output = MonthlySnapshotBuilder::new(grid.cell_count());
        let mut preceding_month: Option<CirculationState> = None;
        let mut total_iterations = 0_u64;
        let mut maximum_final_residual = 0.0_f64;
        let mut maximum_relative_mass_error = 0.0_f64;

        for month in 0..CLIMATE_MONTH_COUNT {
            let mut state = match preceding_month.take() {
                Some(state) => state,
                None => initial_state(grid, forcing, spec, month)?,
            };
            let mut final_residual = f64::INFINITY;
            let mut precipitation_mm_day = vec![0.0; grid.cell_count()];
            let mut month_iterations = 0_u64;

            for _ in 0..spec.max_steady_iterations {
                let iteration =
                    balanced_iteration(&state, &operators, forcing, spec, &permeability, month)?;
                state = iteration.state;
                precipitation_mm_day = iteration.precipitation_mm_day;
                final_residual = iteration.residual;
                maximum_relative_mass_error =
                    maximum_relative_mass_error.max(iteration.relative_mass_error);
                month_iterations += 1;
                total_iterations += 1;
                if final_residual <= f64::from(spec.convergence_tolerance) {
                    break;
                }
            }

            if final_residual > f64::from(spec.convergence_tolerance) {
                return Err(CirculationSolveError::NotConverged {
                    solver_id: self.id(),
                    month,
                    iterations: month_iterations,
                    residual: final_residual,
                    tolerance: f64::from(spec.convergence_tolerance),
                });
            }
            maximum_final_residual = maximum_final_residual.max(final_residual);
            output.record(month, &state, &precipitation_mm_day)?;
            preceding_month = Some(state);
        }

        output.finish(
            identity,
            self.id(),
            CirculationSolveStats {
                iterations_or_steps: total_iterations,
                formation_years: 0,
                final_residual: maximum_final_residual,
                relative_mass_error: maximum_relative_mass_error,
                dense_state_bytes: dense_state_bytes(grid.cell_count())?,
            },
        )
    }
}
