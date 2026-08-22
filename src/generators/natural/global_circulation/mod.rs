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
    ClimateModelProfile, CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2,
    CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2, CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2,
    CERES_EBAF_SURFACE_UP_LONGWAVE_GLOBAL_MEAN_W_M2, CLIMATE_MONTH_COUNT,
    EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE, EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN,
    EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN, EARTH_GRAY_GREENHOUSE_OFFSET_K,
    EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2, GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
    P4_HIGHLAND_ALBEDO_RAMP_ONSET_M, P4_HIGHLAND_ALBEDO_RAMP_SPAN_M,
    P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT, P4_OPEN_OCEAN_SURFACE_ALBEDO,
    P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT, STEFAN_BOLTZMANN_CONSTANT_W_M2_K4,
};

pub use crate::world::natural::CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M;

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
pub use forcing::{GlobalClimateForcing, GlobalClimateForcingBuilder, GlobalClimateForcingError};
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
    hasher.update(b"sekai.global-circulation-equations.v3\0");
    hasher.update(&tendency::layered_equation_model_fingerprint(profile));
    hasher.update(&(CLIMATE_MONTH_COUNT as u64).to_le_bytes());
    hasher.update(&GLOBAL_CIRCULATION_MACRO_STEP_SECONDS.to_le_bytes());
    for value in [
        CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M,
        P4_OPEN_OCEAN_SURFACE_ALBEDO,
        P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT,
        P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT,
        P4_HIGHLAND_ALBEDO_RAMP_ONSET_M,
        P4_HIGHLAND_ALBEDO_RAMP_SPAN_M,
        EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2,
        CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2,
        CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2,
        CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2,
        CERES_EBAF_SURFACE_UP_LONGWAVE_GLOBAL_MEAN_W_M2,
        EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN,
        EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN,
        EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE,
        STEFAN_BOLTZMANN_CONSTANT_W_M2_K4,
        EARTH_GRAY_GREENHOUSE_OFFSET_K,
    ] {
        hasher.update(&value.to_bits().to_le_bytes());
    }
    for semantic_id in [
        b"toa-gray-radiation-ledger.v1".as_slice(),
        b"annual-mean-climatology-initial-state.v1".as_slice(),
        b"surface-albedo-asr-olr-fields.v1".as_slice(),
    ] {
        hasher.update(&(semantic_id.len() as u32).to_le_bytes());
        hasher.update(semantic_id);
    }
    *hasher.finalize().as_bytes()
}
