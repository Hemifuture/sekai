mod comparison;
mod forcing;
mod generation;
mod imex;
mod project;
mod rk3;
mod split_explicit;
mod state;
mod tendency;

use crate::world::natural::{
    ClimateModelProfile, CLIMATE_MONTH_COUNT, GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
};

pub use comparison::{
    annual_precipitation_total_bias, compare_climate_states,
    compare_formation_procedure_identities, formation_procedure_identity_matches,
    run_closed_split_annual_mass_fixture, run_formation_cycle_comparison,
    run_integrator_comparison, AnnualLayerMassConservationReport, CandidateIntegratorComparison,
    ClimateAgreementFailure, ClimateAgreementThresholds, ClimateConservationInterpretation,
    ClimatePrecipitationAgreement, ClimateScalarAgreement, ClimateStateComparison,
    ClimateVectorAgreement, FormationCycleComparisonReport, FormationProcedureAgreement,
    FormationProcedureIdentity, FormationRunOutcome, IntegratorComparisonReport,
    LayerMassConservationDiagnostic, ProductionCandidateSelection,
    CLOSED_ANNUAL_LAYER_MASS_DRIFT_MAX, SELECTED_PRODUCTION_INTEGRATOR,
};
pub use forcing::{
    GlobalClimateForcing, GlobalClimateForcingBuilder, GlobalClimateForcingError,
    CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M,
};
pub use generation::{
    GlobalCirculationGenerationError, GlobalCirculationGenerator, GlobalCirculationPhase,
};
pub use imex::ImexCrankNicolsonIntegrator;
pub use project::{
    project_monthly_extensive_rate, project_monthly_intensive_scalar,
    project_monthly_tangent_vectors, ClimateProjectionError, ProjectedMonthlyScalar,
};
pub(crate) use rk3::climate_state_formation_residual_cancellable;
pub use rk3::{
    climate_state_formation_residual, climate_state_rms_difference, ClimateIntegratorDiagnostics,
    ClimateIntegratorError, ClimateStepResult, ExplicitRk3Integrator,
};
pub use split_explicit::SplitExplicitRk3Integrator;
pub use state::{LayeredClimateState, LayeredStateError};
pub use tendency::{
    paired_heat_exchange, paired_momentum_exchange, LayeredClimateTendency, LayeredTendencyBudget,
    LayeredTendencyError, LayeredTendencySystem, LayeredTendencyWorkspace, PairedHeatExchange,
    PairedMomentumExchange,
};

/// Canonical identity of the locked shared equations and formation procedure.
pub fn global_circulation_model_fingerprint(profile: ClimateModelProfile) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.global-circulation-equations.v2\0");
    hasher.update(&tendency::layered_equation_model_fingerprint(profile));
    hasher.update(&(CLIMATE_MONTH_COUNT as u64).to_le_bytes());
    hasher.update(&GLOBAL_CIRCULATION_MACRO_STEP_SECONDS.to_le_bytes());
    *hasher.finalize().as_bytes()
}
