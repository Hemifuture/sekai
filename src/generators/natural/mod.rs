//! Deterministic generation of the current natural world slice.

mod climate;
mod climate_rule_input;
mod climate_stage;
mod erosion;
mod formation;
mod formation_stage;
mod geologic_rule_input;
mod geologic_stage;
mod geology;
mod hydro_erosion;
mod hydro_erosion_rule_input;
mod hydro_erosion_stage;
mod hydrology;
mod island_relief;
mod mantle;
mod random;
mod relief;
mod relief_noise;
mod rule_input;
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
pub use relief::{ReliefGenerationError, ReliefGenerator};
pub use rule_input::{
    AuthorConstraintsArtifact, ResolvedTectonicInput, ResolvedTectonicInputArtifact,
    ResolvedTectonicInputStage, ResolvedTectonicInputStageInputs, RulePackSetArtifact,
    RuleTectonicResolutionStage, RuleTectonicResolutionStageInputs, TectonicRuleResolutionArtifact,
};
pub use stage::{
    natural_foundation_graph, ReliefArtifact, ReliefStage, TectonicArtifact, TectonicSpecArtifact,
    TectonicStage,
};
pub use tectonics::{TectonicGenerationError, TectonicGenerator};
