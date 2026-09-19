//! Deterministic generation of the current natural world slice.

mod climate;
mod climate_rule_input;
mod connectivity;
mod erosion;
mod fractal;
mod geologic_rule_input;
mod geology;
mod hierarchical_derivation;
mod hierarchical_rivers;
mod hydro_erosion;
mod hydro_erosion_rule_input;
mod hydrology;
mod island_relief;
mod land_fraction;
mod mantle;
mod morphology;
mod quality;
mod random;
mod relief;
mod relief_noise;
mod relief_spec;
mod rule_input;
mod stage;
mod surface_water_geometry;
mod tectonics;
mod terrain_amplification;
mod topology;

pub mod circulation;
pub mod formation;
pub mod foundation;

pub use climate::{ClimateGenerationError, ClimateGenerator};
pub use climate_rule_input::{
    ClimateRuleResolutionArtifact, ClimateSpecArtifact, ResolvedClimateInput,
    ResolvedClimateInputArtifact, ResolvedClimateInputStage, ResolvedClimateInputStageInputs,
    RuleClimateResolutionStage, RuleClimateResolutionStageInputs,
};
pub use erosion::{FluvialErosionError, FluvialErosionGenerator};
pub use formation::{
    annual_precipitation_total_bias, causal_natural_formation_graph, classify_substrate_bedrock,
    climate_state_formation_residual, climate_state_rms_difference, compare_climate_states,
    compare_formation_procedure_identities, continental_airy_elevation_m,
    formation_procedure_identity_matches, gdh1_ocean_depth_m, global_circulation_model_fingerprint,
    implicit_stream_power_n1_height, oceanic_isostatic_elevation_m,
    oceanic_sediment_seafloor_rise_m, paired_heat_exchange, paired_momentum_exchange,
    project_monthly_extensive_rate, project_monthly_intensive_scalar,
    project_monthly_tangent_vectors, run_closed_split_annual_mass_fixture,
    run_formation_cycle_comparison, run_integrator_comparison, AnnualLayerMassConservationReport,
    CandidateIntegratorComparison, CausalNaturalFormationStage, CausalNaturalFormationStageInputs,
    ClimateAgreementFailure, ClimateAgreementThresholds, ClimateConservationInterpretation,
    ClimateIntegrationProcedure, ClimateIntegratorDiagnostics, ClimateIntegratorError,
    ClimatePrecipitationAgreement, ClimateProjectionError, ClimateScalarAgreement,
    ClimateStateComparison, ClimateStepResult, ClimateVectorAgreement, ClimateWorkDomainArtifact,
    ClimateWorkDomainBuildError, ClimateWorkDomainBuilder, ClimateWorkDomainStage,
    ClimateWorkDomainStageInputs, CoastGenerationError, CoastalExchange, CoastalExchangeStep,
    CoastalInputs, EvolvedTectonicGenerationError, EvolvedTectonicGenerator, ExplicitRk3Integrator,
    FormationCycleComparisonReport, FormationHydrologyGenerationError, FormationHydrologyGenerator,
    FormationProcedureAgreement, FormationProcedureIdentity, FormationRunOutcome,
    GeologicSubstrateGenerationError, GeologicSubstrateGenerator, GlobalCirculationGenerationError,
    GlobalCirculationGenerator, GlobalCirculationPhase, GlobalClimateForcing,
    GlobalClimateForcingBuilder, GlobalClimateForcingError, HillslopeGenerationError,
    HillslopeInputs, HillslopeTransportStep, HillslopeWorkspace, ImexCrankNicolsonIntegrator,
    ImplicitStreamPowerSolver, IntegratorComparisonReport, IsostasyGenerationError,
    IsostaticAdjustmentStep, LayerMassConservationDiagnostic, LayeredClimateState,
    LayeredClimateTendency, LayeredStateError, LayeredTendencyBudget, LayeredTendencyError,
    LayeredTendencySystem, LayeredTendencyWorkspace, LocalAiryIsostasy,
    NaturalFormationBundleArtifact, NaturalQualityProfileArtifact, NonlinearHillslopeTransport,
    PairedHeatExchange, PairedMomentumExchange, PrimaryReliefGenerationError,
    PrimaryReliefGenerator, ProductionCandidateSelection, ProjectedMonthlyScalar,
    ProvenanceSedimentRouter, ResolvedWorldFormationArtifact, SedimentGenerationError,
    SedimentInputs, SedimentTransportStep, SplitExplicitRk3Integrator, StreamPowerGenerationError,
    StreamPowerInputs, StreamPowerStep, SurfaceFormationGenerationError,
    WorldFormationGenerationError, WorldFormationGenerator, WorldFormationSpecArtifact,
    WorldFormationStage, WorldFormationStageInputs, CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M,
    CLOSED_ANNUAL_LAYER_MASS_DRIFT_MAX, METAMORPHIC_SHORTENING_THRESHOLD_MM_PER_YEAR,
    METAMORPHIC_UPLIFT_THRESHOLD_MM_PER_YEAR, SEDIMENTARY_FRACTURE_MAX,
    SEDIMENTARY_SUBSIDENCE_THRESHOLD_MM_PER_YEAR, SELECTED_PRODUCTION_INTEGRATOR,
    VOLCANIC_COVER_INFLUENCE_THRESHOLD,
};
pub(crate) use formation::{
    validate_climate_work_domain_maps_against, validate_climate_work_domain_reconstruction,
    validate_climate_work_domain_reconstruction_cancellable, SurfaceFormationGenerator,
    SurfaceFormationInputs,
};
pub use foundation::{
    spherical_natural_foundation_graph, NaturalQualityArtifact, SphericalClimateGenerationError,
    SphericalFluvialErosionError, SphericalGeologicArtifact, SphericalGeologicGenerationError,
    SphericalGeologicStage, SphericalGeologicStageInputs, SphericalHydroErosionArtifact,
    SphericalHydroErosionGenerationError, SphericalHydroErosionStage,
    SphericalHydroErosionStageInputs, SphericalHydrologyGenerationError, SphericalMantleArtifact,
    SphericalMantleGenerationError, SphericalMantleStage, SphericalMantleStageInputs,
    SphericalNaturalQualityStage, SphericalNaturalQualityStageInputs,
    SphericalPreliminaryClimateArtifact, SphericalPreliminaryClimateStage,
    SphericalPreliminaryClimateStageInputs, SphericalReliefArtifact,
    SphericalReliefGenerationError, SphericalReliefStage, SphericalReliefStageInputs,
    SphericalTectonicArtifact, SphericalTectonicGenerationError, SphericalTectonicStage,
    SphericalTectonicStageInputs,
};
pub use geologic_rule_input::{
    GeologicRuleResolutionArtifact, GeologicSpecArtifact, ResolvedGeologicInput,
    ResolvedGeologicInputArtifact, ResolvedGeologicInputStage, ResolvedGeologicInputStageInputs,
    RuleGeologicResolutionStage, RuleGeologicResolutionStageInputs,
};
pub use geology::{GeologicGenerationError, GeologicGenerator};
pub use hierarchical_derivation::{
    HierarchicalEvaluator, HierarchicalPath, HierarchicalProbe, LocatedPrimitive, PrimitiveValue,
    HIERARCHICAL_PATH_DEPTH_MAX, HIERARCHICAL_PROBE_COUNT,
};
pub use hydro_erosion::{HydroErosionGenerationError, HydroErosionGenerator};
pub use hydro_erosion_rule_input::{
    HydroErosionRuleResolutionArtifact, HydroErosionSpecArtifact, ResolvedHydroErosionInput,
    ResolvedHydroErosionInputArtifact, ResolvedHydroErosionInputStage,
    ResolvedHydroErosionInputStageInputs, RuleHydroErosionResolutionStage,
    RuleHydroErosionResolutionStageInputs,
};
pub use hydrology::{HydrologyGenerationError, HydrologyGenerator};
pub use mantle::{MantleGenerationError, MantleGenerator};
pub use quality::{
    evaluate_evolved_tectonic_corpus_quality, evaluate_evolved_tectonic_quality,
    evaluate_global_circulation_quality, evaluate_global_circulation_quality_cancellable,
    evaluate_primary_relief_corpus_quality, evaluate_primary_relief_quality,
    evaluate_profile_surface_quality, evaluate_spherical_foundation_quality,
    evaluate_surface_formation_corpus_hypsometry, evaluate_surface_formation_quality,
    evaluate_surface_formation_quality_cancellable, PrimaryReliefQualitySample, QualityBuildError,
};
pub use relief::{ReliefGenerationError, ReliefGenerator};
pub use relief_spec::ReliefSpecArtifact;
pub use rule_input::{
    AuthorConstraintsArtifact, ResolvedTectonicInput, ResolvedTectonicInputArtifact,
    ResolvedTectonicInputStage, ResolvedTectonicInputStageInputs, RulePackSetArtifact,
    RuleTectonicResolutionStage, RuleTectonicResolutionStageInputs, TectonicRuleResolutionArtifact,
};
pub use stage::TectonicSpecArtifact;
pub use surface_water_geometry::{
    build_surface_water_geometry, solve_physical_sea_level, solve_physical_sea_level_cancellable,
    water_volume_at_sea_level_m3,
};
pub use tectonics::{TectonicGenerationError, TectonicGenerator};
pub use terrain_amplification::{
    fibonacci_probe, AmplificationFieldsView, AmplificationLod, AmplifiedSample,
    FormationDerivationInputs, SurfaceRegime, TerrainAmplificationError, TerrainAmplifier,
    PROBE_COUNT,
};
