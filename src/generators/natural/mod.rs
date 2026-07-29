//! Deterministic generation of the current natural world slice.

mod random;
mod relief;
mod stage;
mod tectonics;
mod topology;

pub use relief::{ReliefGenerationError, ReliefGenerator};
pub use stage::{
    natural_foundation_graph, ReliefArtifact, ReliefStage, TectonicArtifact, TectonicSpecArtifact,
    TectonicStage,
};
pub use tectonics::{TectonicGenerationError, TectonicGenerator};
