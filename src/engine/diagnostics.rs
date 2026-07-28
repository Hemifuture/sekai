use std::time::Duration;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::world::fields::FieldId;
use crate::world::{AuthorObjectId, CellId};

const MAX_IDENTIFIER_BYTES: usize = 128;

/// Returns whether a stable engine identifier uses the supported V1 grammar.
pub(crate) fn is_valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=MAX_IDENTIFIER_BYTES).contains(&bytes.len())
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
        && bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

/// Errors returned while constructing validated diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DiagnosticError {
    /// A machine-readable diagnostic code violates the V1 identifier grammar.
    #[error("diagnostic code must be 1..={MAX_IDENTIFIER_BYTES} lowercase ASCII bytes, use only a-z, 0-9, '-', '_', or '.', and start and end with an alphanumeric byte")]
    InvalidCode,
}

/// The severity of a build diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    /// The build cannot produce a valid result.
    Error,
    /// The build succeeded but observed a non-fatal condition.
    Warning,
    /// The build recorded an informational condition.
    Info,
}

/// Typed optional locations attached to a diagnostic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticContext {
    /// The stage that emitted the diagnostic.
    pub stage_id: Option<String>,
    /// The field associated with the diagnostic.
    pub field_id: Option<FieldId>,
    /// The spatial cell associated with the diagnostic.
    pub cell_id: Option<CellId>,
    /// The authored object associated with the diagnostic.
    pub author_object_id: Option<AuthorObjectId>,
}

/// A structured, machine-readable build diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    severity: DiagnosticSeverity,
    code: String,
    message: String,
    context: DiagnosticContext,
}

#[derive(Deserialize)]
struct DiagnosticWire {
    severity: DiagnosticSeverity,
    code: String,
    message: String,
    context: DiagnosticContext,
}

impl Diagnostic {
    /// Creates a diagnostic without additional typed context.
    pub fn new(
        severity: DiagnosticSeverity,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, DiagnosticError> {
        Self::with_context(severity, code, message, DiagnosticContext::default())
    }

    /// Creates a diagnostic with typed context for consumers to inspect.
    pub fn with_context(
        severity: DiagnosticSeverity,
        code: impl Into<String>,
        message: impl Into<String>,
        context: DiagnosticContext,
    ) -> Result<Self, DiagnosticError> {
        let code = code.into();
        if !is_valid_identifier(&code) {
            return Err(DiagnosticError::InvalidCode);
        }
        Ok(Self {
            severity,
            code,
            message: message.into(),
            context,
        })
    }

    /// Returns the diagnostic severity.
    pub const fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    /// Returns the stable machine-readable code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the typed context.
    pub fn context(&self) -> &DiagnosticContext {
        &self.context
    }
}

impl<'de> Deserialize<'de> for Diagnostic {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = DiagnosticWire::deserialize(deserializer)?;
        Self::with_context(wire.severity, wire.code, wire.message, wire.context)
            .map_err(D::Error::custom)
    }
}

/// A semantic hash for all successful stage outputs in a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct BuildResultHash([u8; 32]);

impl BuildResultHash {
    /// Creates a result hash from already-computed semantic bytes within the engine.
    #[allow(dead_code)] // Populated by the scheduler introduced in Task 9.
    pub(crate) const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the hash bytes without copying them.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Reporting metadata for one executed stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageReport {
    stage_id: String,
    duration: Duration,
    cache_hit: bool,
}

impl StageReport {
    /// Creates reporting metadata for a stage execution.
    pub fn new(stage_id: impl Into<String>, duration: Duration, cache_hit: bool) -> Self {
        Self {
            stage_id: stage_id.into(),
            duration,
            cache_hit,
        }
    }

    /// Returns the stage identifier.
    pub fn stage_id(&self) -> &str {
        &self.stage_id
    }

    /// Returns the observed stage duration, which is not semantic build data.
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// Returns whether the stage output came from the cache.
    pub const fn cache_hit(&self) -> bool {
        self.cache_hit
    }
}

/// Ordered operational metadata and diagnostics for one build attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BuildReport {
    stages: Vec<StageReport>,
    diagnostics: Vec<Diagnostic>,
    cache_hits: usize,
    cache_misses: usize,
    result_hash: Option<BuildResultHash>,
}

impl BuildReport {
    /// Creates an empty report for a new build attempt.
    pub const fn new() -> Self {
        Self {
            stages: Vec::new(),
            diagnostics: Vec::new(),
            cache_hits: 0,
            cache_misses: 0,
            result_hash: None,
        }
    }

    /// Records one stage in deterministic execution order and updates cache counters.
    pub fn record_stage(&mut self, report: StageReport) {
        if report.cache_hit() {
            self.cache_hits += 1;
        } else {
            self.cache_misses += 1;
        }
        self.stages.push(report);
    }

    /// Appends a structured diagnostic in emission order.
    pub fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        if diagnostic.severity() == DiagnosticSeverity::Error {
            self.result_hash = None;
        }
        self.diagnostics.push(diagnostic);
    }

    /// Sets the semantic result hash after every stage has completed successfully.
    #[allow(dead_code)] // Called by the scheduler introduced in Task 9.
    pub(crate) fn set_result_hash(&mut self, result_hash: BuildResultHash) {
        if !self.has_errors() {
            self.result_hash = Some(result_hash);
        }
    }

    /// Returns reporting metadata in deterministic stage order.
    pub fn stages(&self) -> &[StageReport] {
        &self.stages
    }

    /// Returns diagnostics in emission order.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Returns stage identifiers in deterministic report order.
    pub fn stage_ids(&self) -> Vec<&str> {
        self.stages.iter().map(|stage| stage.stage_id()).collect()
    }

    /// Returns the number of stages restored from cache.
    pub const fn cache_hits(&self) -> usize {
        self.cache_hits
    }

    /// Returns the number of stages that executed without a cache entry.
    pub const fn cache_misses(&self) -> usize {
        self.cache_misses
    }

    /// Returns whether the report contains an error-severity diagnostic.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error)
    }

    /// Returns the semantic result hash, present only for successful builds.
    pub const fn result_hash(&self) -> Option<&BuildResultHash> {
        self.result_hash.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildReport, BuildResultHash, Diagnostic, DiagnosticSeverity};

    fn result_hash() -> BuildResultHash {
        BuildResultHash::new([7; 32])
    }

    #[test]
    fn error_diagnostic_clears_an_existing_result_hash() {
        let mut report = BuildReport::new();
        report.set_result_hash(result_hash());
        report.push_diagnostic(
            Diagnostic::new(DiagnosticSeverity::Error, "test.error", "failed").unwrap(),
        );

        assert!(report.result_hash().is_none());
    }

    #[test]
    fn error_diagnostic_prevents_later_result_hash_population() {
        let mut report = BuildReport::new();
        report.push_diagnostic(
            Diagnostic::new(DiagnosticSeverity::Error, "test.error", "failed").unwrap(),
        );
        report.set_result_hash(result_hash());

        assert!(report.result_hash().is_none());
    }

    #[test]
    fn non_error_diagnostics_preserve_a_result_hash() {
        let mut report = BuildReport::new();
        report.set_result_hash(result_hash());
        report.push_diagnostic(
            Diagnostic::new(DiagnosticSeverity::Warning, "test.warning", "warning").unwrap(),
        );
        report.push_diagnostic(
            Diagnostic::new(DiagnosticSeverity::Info, "test.info", "info").unwrap(),
        );

        assert_eq!(report.result_hash(), Some(&result_hash()));
    }
}
