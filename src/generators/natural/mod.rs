//! Deterministic generation of the current natural world slice.

mod climate;
mod climate_rule_input;
mod climate_stage;
mod climate_work_domain;
mod connectivity;
mod erosion;
mod evolved_tectonic_stage;
mod evolved_tectonics;
mod formation;
mod formation_stage;
mod fractal;
mod geologic_rule_input;
mod geologic_stage;
mod geologic_substrate;
mod geology;
mod global_circulation;
mod global_circulation_stage;
mod hydro_erosion;
mod hydro_erosion_rule_input;
mod hydro_erosion_stage;
mod hydrology;
mod island_relief;
mod land_fraction;
mod mantle;
mod morphology;
mod primary_relief;
mod primary_relief_stage;
mod quality;
mod random;
mod relief;
mod relief_noise;
mod relief_spec;
mod rule_input;
mod spherical_climate;
mod spherical_climate_stage;
mod spherical_crust_physics;
mod spherical_erosion;
mod spherical_geologic_stage;
mod spherical_geology;
mod spherical_hydro_erosion;
mod spherical_hydro_erosion_stage;
mod spherical_hydrology;
mod spherical_island_relief;
mod spherical_mantle;
mod spherical_moisture;
mod spherical_quality_stage;
mod spherical_relief;
mod spherical_stage;
mod spherical_tectonics;
mod stage;
mod surface_formation;
mod surface_formation_stage;
mod tectonics;
mod topology;

pub mod circulation;

pub use climate::{ClimateGenerationError, ClimateGenerator};
pub use climate_rule_input::{
    ClimateRuleResolutionArtifact, ClimateSpecArtifact, ResolvedClimateInput,
    ResolvedClimateInputArtifact, ResolvedClimateInputStage, ResolvedClimateInputStageInputs,
    RuleClimateResolutionStage, RuleClimateResolutionStageInputs,
};
pub use climate_stage::{
    PreliminaryClimateArtifact, PreliminaryClimateStage, PreliminaryClimateStageInputs,
};
pub(crate) use climate_work_domain::{
    validate_climate_work_domain_maps_against, validate_climate_work_domain_reconstruction,
    validate_climate_work_domain_reconstruction_cancellable,
};
pub use climate_work_domain::{ClimateWorkDomainBuildError, ClimateWorkDomainBuilder};
pub use erosion::{FluvialErosionError, FluvialErosionGenerator};
pub use evolved_tectonic_stage::{
    evolved_tectonic_graph, EvolvedTectonicArtifact, EvolvedTectonicStage,
    EvolvedTectonicStageInputs, NaturalQualityProfileArtifact,
};
pub use evolved_tectonics::{EvolvedTectonicGenerationError, EvolvedTectonicGenerator};
pub use formation::{WorldFormationGenerationError, WorldFormationGenerator};
pub use formation_stage::{
    ResolvedWorldFormationArtifact, WorldFormationSpecArtifact, WorldFormationStage,
    WorldFormationStageInputs,
};
pub use geologic_rule_input::{
    GeologicRuleResolutionArtifact, GeologicSpecArtifact, ResolvedGeologicInput,
    ResolvedGeologicInputArtifact, ResolvedGeologicInputStage, ResolvedGeologicInputStageInputs,
    RuleGeologicResolutionStage, RuleGeologicResolutionStageInputs,
};
pub use geologic_stage::{
    GeologicArtifact, GeologicStage, GeologicStageInputs, MantleArtifact, MantleStage,
    MantleStageInputs,
};
pub use geologic_substrate::{
    classify_substrate_bedrock, GeologicSubstrateGenerationError, GeologicSubstrateGenerator,
    METAMORPHIC_SHORTENING_THRESHOLD_MM_PER_YEAR, METAMORPHIC_UPLIFT_THRESHOLD_MM_PER_YEAR,
    SEDIMENTARY_FRACTURE_MAX, SEDIMENTARY_SUBSIDENCE_THRESHOLD_MM_PER_YEAR,
    VOLCANIC_COVER_INFLUENCE_THRESHOLD,
};
pub use geology::{GeologicGenerationError, GeologicGenerator};
pub use global_circulation::{
    annual_precipitation_total_bias, climate_state_formation_residual,
    climate_state_rms_difference, compare_climate_states, compare_formation_procedure_identities,
    formation_procedure_identity_matches, global_circulation_model_fingerprint,
    paired_heat_exchange, paired_momentum_exchange, project_monthly_extensive_rate,
    project_monthly_intensive_scalar, project_monthly_tangent_vectors,
    run_closed_split_annual_mass_fixture, run_formation_cycle_comparison,
    run_integrator_comparison, AnnualLayerMassConservationReport, CandidateIntegratorComparison,
    ClimateAgreementFailure, ClimateAgreementThresholds, ClimateConservationInterpretation,
    ClimateIntegratorDiagnostics, ClimateIntegratorError, ClimatePrecipitationAgreement,
    ClimateProjectionError, ClimateScalarAgreement, ClimateStateComparison, ClimateStepResult,
    ClimateVectorAgreement, ExplicitRk3Integrator, FormationCycleComparisonReport,
    FormationProcedureAgreement, FormationProcedureIdentity, FormationRunOutcome,
    GlobalCirculationGenerationError, GlobalCirculationGenerator, GlobalCirculationPhase,
    GlobalClimateForcing, GlobalClimateForcingBuilder, GlobalClimateForcingError,
    ImexCrankNicolsonIntegrator, IntegratorComparisonReport, LayerMassConservationDiagnostic,
    LayeredClimateState, LayeredClimateTendency, LayeredStateError, LayeredTendencyBudget,
    LayeredTendencyError, LayeredTendencySystem, LayeredTendencyWorkspace, PairedHeatExchange,
    PairedMomentumExchange, ProductionCandidateSelection, ProjectedMonthlyScalar,
    SplitExplicitRk3Integrator, CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M,
    CLOSED_ANNUAL_LAYER_MASS_DRIFT_MAX, SELECTED_PRODUCTION_INTEGRATOR,
};
pub use global_circulation_stage::{
    global_circulation_graph, ClimateWorkDomainArtifact, ClimateWorkDomainStage,
    ClimateWorkDomainStageInputs, GlobalCirculationArtifact, GlobalCirculationProductError,
    GlobalCirculationStage, GlobalCirculationStageInputs,
};
pub use hydro_erosion::{HydroErosionGenerationError, HydroErosionGenerator};
pub use hydro_erosion_rule_input::{
    HydroErosionRuleResolutionArtifact, HydroErosionSpecArtifact, ResolvedHydroErosionInput,
    ResolvedHydroErosionInputArtifact, ResolvedHydroErosionInputStage,
    ResolvedHydroErosionInputStageInputs, RuleHydroErosionResolutionStage,
    RuleHydroErosionResolutionStageInputs,
};
pub use hydro_erosion_stage::{HydroErosionArtifact, HydroErosionStage, HydroErosionStageInputs};
pub use hydrology::{HydrologyGenerationError, HydrologyGenerator};
pub use mantle::{MantleGenerationError, MantleGenerator};
pub use primary_relief::{
    causal_accumulated_response_m, continental_airy_elevation_m, dynamic_tectonic_response_m,
    oceanic_isostatic_elevation_m, parsons_sclater_ocean_depth_m, PrimaryReliefGenerationError,
    PrimaryReliefGenerator,
};
pub use primary_relief_stage::{
    primary_relief_graph, GeologicSubstrateArtifact, GeologicSubstrateStage,
    GeologicSubstrateStageInputs, PrimaryReliefArtifact, PrimaryReliefStage,
    PrimaryReliefStageInputs,
};
pub use quality::{
    evaluate_evolved_tectonic_corpus_quality, evaluate_evolved_tectonic_quality,
    evaluate_global_circulation_quality, evaluate_global_circulation_quality_cancellable,
    evaluate_primary_relief_corpus_quality, evaluate_primary_relief_quality,
    evaluate_profile_surface_quality, evaluate_spherical_foundation_quality,
    evaluate_surface_formation_quality, evaluate_surface_formation_quality_cancellable,
    PrimaryReliefQualitySample, QualityBuildError,
};
pub use relief::{ReliefGenerationError, ReliefGenerator};
pub use relief_spec::ReliefSpecArtifact;
pub use rule_input::{
    AuthorConstraintsArtifact, ResolvedTectonicInput, ResolvedTectonicInputArtifact,
    ResolvedTectonicInputStage, ResolvedTectonicInputStageInputs, RulePackSetArtifact,
    RuleTectonicResolutionStage, RuleTectonicResolutionStageInputs, TectonicRuleResolutionArtifact,
};
pub use spherical_climate::SphericalClimateGenerationError;
pub use spherical_climate_stage::{
    SphericalPreliminaryClimateArtifact, SphericalPreliminaryClimateStage,
    SphericalPreliminaryClimateStageInputs,
};
pub use spherical_erosion::SphericalFluvialErosionError;
pub use spherical_geologic_stage::{
    SphericalGeologicArtifact, SphericalGeologicStage, SphericalGeologicStageInputs,
    SphericalMantleArtifact, SphericalMantleStage, SphericalMantleStageInputs,
};
pub use spherical_geology::SphericalGeologicGenerationError;
pub use spherical_hydro_erosion::SphericalHydroErosionGenerationError;
pub use spherical_hydro_erosion_stage::{
    SphericalHydroErosionArtifact, SphericalHydroErosionStage, SphericalHydroErosionStageInputs,
};
pub use spherical_hydrology::SphericalHydrologyGenerationError;
pub use spherical_mantle::SphericalMantleGenerationError;
pub use spherical_quality_stage::{
    NaturalQualityArtifact, SphericalNaturalQualityStage, SphericalNaturalQualityStageInputs,
};
pub use spherical_relief::SphericalReliefGenerationError;
pub use spherical_stage::{
    spherical_natural_foundation_graph, SphericalReliefArtifact, SphericalReliefStage,
    SphericalReliefStageInputs, SphericalTectonicArtifact, SphericalTectonicStage,
    SphericalTectonicStageInputs,
};
pub use spherical_tectonics::SphericalTectonicGenerationError;
pub use stage::{
    legacy_planar_natural_foundation_graph, natural_foundation_graph, ReliefArtifact, ReliefStage,
    TectonicArtifact, TectonicSpecArtifact, TectonicStage,
};
pub use surface_formation::{
    implicit_stream_power_n1_height, CoastGenerationError, CoastalExchange, CoastalExchangeStep,
    CoastalInputs, FormationHydrologyGenerationError, FormationHydrologyGenerator,
    FormationSeaLevelSolver, FormationWaterState, HillslopeGenerationError, HillslopeInputs,
    HillslopeTransportStep, HillslopeWorkspace, ImplicitStreamPowerSolver, IsostasyGenerationError,
    IsostaticAdjustmentStep, LocalAiryIsostasy, NonlinearHillslopeTransport,
    ProvenanceSedimentRouter, SedimentGenerationError, SedimentInputs, SedimentTransportStep,
    StreamPowerGenerationError, StreamPowerInputs, StreamPowerStep,
    SurfaceFormationGenerationError, SurfaceFormationGenerator, SurfaceFormationInputs,
};
pub use surface_formation_stage::{
    surface_formation_graph, NaturalSurfaceFormationArtifact, SurfaceFormationProductError,
    SurfaceFormationStage, SurfaceFormationStageInputs,
};
pub use tectonics::{TectonicGenerationError, TectonicGenerator};
