//! Causal formation pipeline (P2v5 through P5).

mod causal;
mod climate_work_domain;
mod evolved_tectonics;
mod geologic_substrate;
pub(in crate::generators::natural) mod global_circulation;
mod global_circulation_stage;
mod graph;
mod primary_relief;
mod surface_formation;
mod world;
mod world_stage;

pub(crate) use climate_work_domain::{
    validate_climate_work_domain_maps_against, validate_climate_work_domain_reconstruction,
    validate_climate_work_domain_reconstruction_cancellable,
};
pub use climate_work_domain::{ClimateWorkDomainBuildError, ClimateWorkDomainBuilder};
pub use evolved_tectonics::{EvolvedTectonicGenerationError, EvolvedTectonicGenerator};
pub use geologic_substrate::{
    classify_substrate_bedrock, GeologicSubstrateGenerationError, GeologicSubstrateGenerator,
    METAMORPHIC_SHORTENING_THRESHOLD_MM_PER_YEAR, METAMORPHIC_UPLIFT_THRESHOLD_MM_PER_YEAR,
    SEDIMENTARY_FRACTURE_MAX, SEDIMENTARY_SUBSIDENCE_THRESHOLD_MM_PER_YEAR,
    VOLCANIC_COVER_INFLUENCE_THRESHOLD,
};
pub use global_circulation_stage::{
    ClimateWorkDomainArtifact, ClimateWorkDomainStage, ClimateWorkDomainStageInputs,
};
pub use graph::{
    causal_natural_formation_graph, CausalNaturalFormationStage, CausalNaturalFormationStageInputs,
    NaturalFormationBundleArtifact, NaturalQualityProfileArtifact,
};
pub use primary_relief::{
    continental_airy_elevation_m, gdh1_ocean_depth_m, oceanic_isostatic_elevation_m,
    oceanic_sediment_seafloor_rise_m, PrimaryReliefGenerationError, PrimaryReliefGenerator,
};
pub use surface_formation::{
    implicit_stream_power_n1_height, CoastGenerationError, CoastalExchange, CoastalExchangeStep,
    CoastalInputs, FormationHydrologyGenerationError, FormationHydrologyGenerator,
    HillslopeGenerationError, HillslopeInputs, HillslopeTransportStep, HillslopeWorkspace,
    ImplicitStreamPowerSolver, IsostasyGenerationError, IsostaticAdjustmentStep, LocalAiryIsostasy,
    NonlinearHillslopeTransport, ProvenanceSedimentRouter, SedimentGenerationError, SedimentInputs,
    SedimentTransportStep, StreamPowerGenerationError, StreamPowerInputs, StreamPowerStep,
    SurfaceFormationGenerationError,
};
pub(crate) use surface_formation::{SurfaceFormationGenerator, SurfaceFormationInputs};
pub use world::{WorldFormationGenerationError, WorldFormationGenerator};
pub use world_stage::{
    ResolvedWorldFormationArtifact, WorldFormationSpecArtifact, WorldFormationStage,
    WorldFormationStageInputs,
};

pub use global_circulation::{
    annual_precipitation_total_bias, climate_state_formation_residual,
    climate_state_rms_difference, compare_climate_states, compare_formation_procedure_identities,
    formation_procedure_identity_matches, global_circulation_model_fingerprint,
    paired_heat_exchange, paired_momentum_exchange, project_monthly_extensive_rate,
    project_monthly_intensive_scalar, project_monthly_tangent_vectors,
    run_closed_split_annual_mass_fixture, run_formation_cycle_comparison,
    run_integrator_comparison, AnnualLayerMassConservationReport, CandidateIntegratorComparison,
    ClimateAgreementFailure, ClimateAgreementThresholds, ClimateConservationInterpretation,
    ClimateIntegrationProcedure, ClimateIntegratorDiagnostics, ClimateIntegratorError,
    ClimatePrecipitationAgreement, ClimateProjectionError, ClimateScalarAgreement,
    ClimateStateComparison, ClimateStepResult, ClimateVectorAgreement, ExplicitRk3Integrator,
    FormationCycleComparisonReport, FormationProcedureAgreement, FormationProcedureIdentity,
    FormationRunOutcome, GlobalCirculationGenerationError, GlobalCirculationGenerator,
    GlobalCirculationPhase, GlobalClimateForcing, GlobalClimateForcingBuilder,
    GlobalClimateForcingError, ImexCrankNicolsonIntegrator, IntegratorComparisonReport,
    LayerMassConservationDiagnostic, LayeredClimateState, LayeredClimateTendency,
    LayeredStateError, LayeredTendencyBudget, LayeredTendencyError, LayeredTendencySystem,
    LayeredTendencyWorkspace, PairedHeatExchange, PairedMomentumExchange,
    ProductionCandidateSelection, ProjectedMonthlyScalar, SplitExplicitRk3Integrator,
    CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M, CLOSED_ANNUAL_LAYER_MASS_DRIFT_MAX,
    SELECTED_PRODUCTION_INTEGRATOR,
};
