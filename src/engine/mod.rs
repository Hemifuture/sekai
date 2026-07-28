//! Deterministic, domain-neutral engine services.

mod artifact;
mod cache;
mod diagnostics;
mod graph;
mod provenance;
mod random;
mod scheduler;
mod stage;

pub use artifact::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, ContentHash,
};
pub use cache::{MemoryStageCache, StageCacheError, StageCacheKey};
pub use diagnostics::{
    BuildReport, BuildResultHash, Diagnostic, DiagnosticContext, DiagnosticError,
    DiagnosticSeverity, StageReport,
};
pub use graph::{GraphError, StageGraph, StageGraphBuilder};
pub use provenance::{EntityRef, FactorContribution, ProvenanceError, ProvenanceIndex, SourceRef};
pub use random::{derive_entity_seed, derive_stage_seed, StageIdentity, StageRng, StageSeed};
pub use scheduler::{BuildEngine, BuildFailure, BuildOutcome, ExternalArtifacts};
pub use stage::{Stage, StageDescriptor, StageError, StageId, StageInputs};
