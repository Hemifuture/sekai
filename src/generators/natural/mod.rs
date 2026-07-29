//! Deterministic generation of the current natural world slice.

mod geologic_rule_input;
mod random;
mod relief;
mod rule_input;
mod stage;
mod tectonics;
mod topology;

pub use geologic_rule_input::{
    GeologicRuleResolutionArtifact, GeologicSpecArtifact, ResolvedGeologicInput,
    ResolvedGeologicInputArtifact, ResolvedGeologicInputStage, ResolvedGeologicInputStageInputs,
    RuleGeologicResolutionStage, RuleGeologicResolutionStageInputs,
};
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
