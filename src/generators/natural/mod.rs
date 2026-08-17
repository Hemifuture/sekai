//! Deterministic generation of the current natural world slice.

mod climate;
mod climate_rule_input;
mod climate_stage;
mod connectivity;
mod erosion;
mod formation;
mod formation_stage;
mod fractal;
mod geologic_rule_input;
mod geologic_stage;
mod geology;
mod hydro_erosion;
mod hydro_erosion_rule_input;
mod hydro_erosion_stage;
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
pub use erosion::{FluvialErosionError, FluvialErosionGenerator};
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
pub use geology::{GeologicGenerationError, GeologicGenerator};
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
pub(crate) use quality::evaluate_profile_surface_quality;
pub use quality::{evaluate_spherical_foundation_quality, QualityBuildError};
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
pub use tectonics::{TectonicGenerationError, TectonicGenerator};
