//! Deterministic, domain-neutral engine services.

mod diagnostics;
mod provenance;
mod random;

pub use diagnostics::{
    BuildReport, BuildResultHash, Diagnostic, DiagnosticContext, DiagnosticError,
    DiagnosticSeverity, StageReport,
};
pub use provenance::{EntityRef, FactorContribution, ProvenanceError, ProvenanceIndex, SourceRef};
pub use random::{derive_entity_seed, derive_stage_seed, StageIdentity, StageRng, StageSeed};
