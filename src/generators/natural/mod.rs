//! Deterministic generation of the current natural world slice.

mod random;
mod relief;
mod rule_input;
mod stage;
mod tectonics;
mod topology;

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
