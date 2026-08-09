#[cfg(target_arch = "wasm32")]
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant as StageTimer;

use thiserror::Error;

use crate::engine::artifact::{Artifact, ArtifactError, ArtifactKey, BuildArtifacts, ContentHash};
use crate::engine::cache::{MemoryStageCache, StageCacheKey};
use crate::engine::diagnostics::{
    BuildReport, BuildResultHash, Diagnostic, DiagnosticContext, DiagnosticSeverity, StageReport,
};
use crate::engine::graph::StageGraph;
use crate::engine::random::{derive_stage_seed, StageIdentity, StageRng};
use crate::engine::stage::{ErasedStageError, StageDescriptor};
use crate::world::RootSeed;

#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Copy)]
struct StageTimer(f64);

#[cfg(target_arch = "wasm32")]
impl StageTimer {
    fn now() -> Self {
        Self(browser_milliseconds())
    }

    fn elapsed(self) -> Duration {
        let elapsed_milliseconds = (browser_milliseconds() - self.0).max(0.0);
        Duration::from_secs_f64(elapsed_milliseconds / 1_000.0)
    }
}

#[cfg(target_arch = "wasm32")]
fn browser_milliseconds() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |performance| performance.now())
}

/// Typed external artifacts supplied to one build attempt.
#[derive(Debug, Default)]
pub struct ExternalArtifacts {
    artifacts: BuildArtifacts,
}

impl ExternalArtifacts {
    /// Creates an empty external-artifact set.
    pub fn new() -> Self {
        Self {
            artifacts: BuildArtifacts::default(),
        }
    }

    /// Validates, stream-hashes, and inserts one typed external artifact.
    pub fn insert<T: Artifact>(&mut self, value: T) -> Result<(), ArtifactError> {
        self.artifacts.insert(value)
    }

    /// Returns the checked semantic hash of a typed external artifact.
    pub fn hash<T: Artifact>(&self) -> Result<ContentHash, ArtifactError> {
        self.artifacts.hash::<T>()
    }

    /// Returns the number of external artifact keys supplied.
    pub fn len(&self) -> usize {
        self.artifacts.keys().len()
    }

    /// Returns whether no external artifacts have been supplied.
    pub fn is_empty(&self) -> bool {
        self.artifacts.keys().len() == 0
    }
}

/// Executes a validated stage graph with deterministic seeds and cache keys.
#[derive(Debug)]
pub struct BuildEngine {
    graph: StageGraph,
}

impl BuildEngine {
    /// Creates an engine for an already validated deterministic stage graph.
    pub const fn new(graph: StageGraph) -> Self {
        Self { graph }
    }

    /// Executes the graph atomically for one root seed and exact external input set.
    pub fn build(
        &self,
        root_seed: RootSeed,
        external: ExternalArtifacts,
        cache: &mut MemoryStageCache,
    ) -> Result<BuildOutcome, BuildFailure> {
        let mut report = BuildReport::new();
        let external_hashes = match self.graph.external_hashes(&external.artifacts) {
            Ok(hashes) => hashes,
            Err(error) => {
                push_engine_error(
                    &mut report,
                    "engine.external-artifact",
                    error.to_string(),
                    None,
                );
                return Err(BuildFailure { report });
            }
        };
        if !same_keys(
            external.artifacts.keys(),
            external_hashes.iter().map(|(key, _)| *key),
        ) {
            push_engine_error(
                &mut report,
                "engine.external-artifact-set",
                "supplied external artifact keys do not exactly match the graph registration",
                None,
            );
            return Err(BuildFailure { report });
        }

        let mut artifacts = external.artifacts;
        for (descriptor, stage) in self.graph.execution_stages() {
            let started = StageTimer::now();
            let dependency_hashes = match self.graph.dependency_hashes(descriptor, &artifacts) {
                Ok(hashes) => hashes,
                Err(error) => {
                    push_engine_error(
                        &mut report,
                        "engine.dependency-artifact",
                        error.to_string(),
                        Some(descriptor),
                    );
                    return Err(BuildFailure { report });
                }
            };
            let identity = StageIdentity::new(
                descriptor.id().as_str(),
                descriptor.version(),
                descriptor.namespace(),
            );
            let stage_seed = derive_stage_seed(root_seed, identity);
            let cache_key = match StageCacheKey::new(
                identity,
                descriptor.output(),
                stage_seed,
                &dependency_hashes,
            ) {
                Ok(cache_key) => cache_key,
                Err(error) => {
                    push_engine_error(
                        &mut report,
                        "engine.stage-cache-key",
                        error.to_string(),
                        Some(descriptor),
                    );
                    return Err(BuildFailure { report });
                }
            };

            if let Some((stored, cached_diagnostics)) = cache.get(&cache_key) {
                if let Err(error) = stage.restore_cached_output(stored, &mut artifacts) {
                    push_engine_error(
                        &mut report,
                        "engine.cache-restore",
                        error.to_string(),
                        Some(descriptor),
                    );
                    return Err(BuildFailure { report });
                }
                for diagnostic in cached_diagnostics {
                    report.push_diagnostic(diagnostic);
                }
                report.record_stage(StageReport::new(
                    descriptor.id().as_str(),
                    started.elapsed(),
                    true,
                ));
                continue;
            }

            let mut emitted = Vec::new();
            let run_result = stage.run(
                &mut artifacts,
                &mut StageRng::from_seed(stage_seed),
                &mut emitted,
            );
            let emitted_error = emitted
                .iter()
                .any(|diagnostic| diagnostic.severity() == DiagnosticSeverity::Error);
            for diagnostic in &emitted {
                report.push_diagnostic(diagnostic.clone());
            }
            report.record_stage(StageReport::new(
                descriptor.id().as_str(),
                started.elapsed(),
                false,
            ));

            let stored = match run_result {
                Ok(stored) => stored,
                Err(error) => {
                    let (code, message) = erased_stage_error(error);
                    push_engine_error(&mut report, code, message, Some(descriptor));
                    return Err(BuildFailure { report });
                }
            };
            if emitted_error {
                return Err(BuildFailure { report });
            }
            cache.insert(cache_key, stored, emitted);
        }

        let output_hashes = match self.graph.output_hashes(&artifacts) {
            Ok(hashes) => hashes,
            Err(error) => {
                push_engine_error(
                    &mut report,
                    "engine.result-artifact",
                    error.to_string(),
                    None,
                );
                return Err(BuildFailure { report });
            }
        };
        let result_hash = result_hash(&output_hashes);
        report.set_result_hash(result_hash);
        let provenance = BuildProvenance {
            root_seed,
            result_hash,
            artifact_set_hash: artifacts.semantic_binding_hash(),
            report_hash: report_binding_hash(&report),
        };
        Ok(BuildOutcome {
            artifacts,
            report,
            provenance,
        })
    }
}

/// Immutable semantic provenance retained by one successful engine build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildProvenance {
    root_seed: RootSeed,
    result_hash: BuildResultHash,
    artifact_set_hash: [u8; 32],
    report_hash: [u8; 32],
}

impl BuildProvenance {
    /// Returns the root seed used by the engine for every stage seed.
    pub const fn root_seed(&self) -> RootSeed {
        self.root_seed
    }

    /// Returns the semantic hash of the successful graph outputs.
    pub const fn result_hash(&self) -> &BuildResultHash {
        &self.result_hash
    }
}

/// A complete successful build and its ordered report.
#[derive(Debug)]
pub struct BuildOutcome {
    /// All checked external and produced typed artifacts.
    pub artifacts: BuildArtifacts,
    /// Deterministic stage ordering, diagnostics, and non-semantic reporting metadata.
    pub report: BuildReport,
    provenance: BuildProvenance,
}

impl BuildOutcome {
    /// Verifies that the report and artifact store still belong to this build.
    ///
    /// The public compatibility fields may be inspected or moved by callers;
    /// consumers that publish audited identity must call this method before
    /// trusting either component.
    pub fn verified_provenance(&self) -> Result<&BuildProvenance, BuildOutcomeIntegrityError> {
        let report_result_hash = self
            .report
            .result_hash()
            .ok_or(BuildOutcomeIntegrityError::MissingReportResultHash)?;
        if report_result_hash != self.provenance.result_hash() {
            return Err(BuildOutcomeIntegrityError::ReportResultHashMismatch {
                expected: self.provenance.result_hash,
                found: *report_result_hash,
            });
        }
        let report_hash = report_binding_hash(&self.report);
        if report_hash != self.provenance.report_hash {
            return Err(BuildOutcomeIntegrityError::ReportMetadataMismatch {
                expected: self.provenance.report_hash,
                found: report_hash,
            });
        }
        let artifact_set_hash = self.artifacts.semantic_binding_hash();
        if artifact_set_hash != self.provenance.artifact_set_hash {
            return Err(BuildOutcomeIntegrityError::ArtifactSetMismatch {
                expected: self.provenance.artifact_set_hash,
                found: artifact_set_hash,
            });
        }
        Ok(&self.provenance)
    }
}

/// Integrity failures detected after successful build components were changed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildOutcomeIntegrityError {
    /// The report no longer carries its successful semantic result hash.
    #[error("successful build report is missing its result hash")]
    MissingReportResultHash,
    /// The report was replaced with metadata from another successful build.
    #[error("build report result hash does not match immutable build provenance")]
    ReportResultHashMismatch {
        /// The result hash retained by the successful build.
        expected: BuildResultHash,
        /// The result hash found in the current report.
        found: BuildResultHash,
    },
    /// The successful report's stages or diagnostics were changed in place.
    #[error("build report metadata does not match immutable build provenance")]
    ReportMetadataMismatch {
        /// The serialized report hash retained by the successful build.
        expected: [u8; 32],
        /// The hash of the current report.
        found: [u8; 32],
    },
    /// The artifact store was replaced with data from another build.
    #[error("build artifact set does not match immutable build provenance")]
    ArtifactSetMismatch {
        /// The artifact-set hash retained by the successful build.
        expected: [u8; 32],
        /// The hash of the current artifact set.
        found: [u8; 32],
    },
}

/// A failed build attempt that intentionally exposes no partial artifact store.
#[derive(Debug, Error)]
#[error("build failed; inspect the report diagnostics")]
pub struct BuildFailure {
    /// The ordered stage metadata and diagnostics recorded before failure.
    pub report: BuildReport,
}

fn same_keys(
    left: impl ExactSizeIterator<Item = ArtifactKey>,
    right: impl ExactSizeIterator<Item = ArtifactKey>,
) -> bool {
    left.len() == right.len() && left.eq(right)
}

fn erased_stage_error(error: ErasedStageError) -> (&'static str, String) {
    match error {
        ErasedStageError::Inputs(error) => artifact_error_diagnostic(error, "engine.stage-inputs"),
        ErasedStageError::Stage(error) => (error.code(), error.message().to_owned()),
        ErasedStageError::Output(error) => artifact_error_diagnostic(error, "engine.stage-output"),
        ErasedStageError::Publication(error) => {
            artifact_error_diagnostic(error, "engine.stage-publication")
        }
    }
}

fn artifact_error_diagnostic(
    error: ArtifactError,
    generic_code: &'static str,
) -> (&'static str, String) {
    match error {
        ArtifactError::Validation { source, .. } => (source.code(), source.message().to_owned()),
        error => (generic_code, error.to_string()),
    }
}

fn push_engine_error(
    report: &mut BuildReport,
    code: &'static str,
    message: impl Into<String>,
    descriptor: Option<&StageDescriptor>,
) {
    let context = DiagnosticContext {
        stage_id: descriptor.map(|descriptor| descriptor.id().as_str().to_owned()),
        ..DiagnosticContext::default()
    };
    let diagnostic = Diagnostic::with_context(DiagnosticSeverity::Error, code, message, context)
        .expect("engine-owned diagnostic codes must satisfy the identifier grammar");
    report.push_diagnostic(diagnostic);
}

fn result_hash(outputs: &[(ArtifactKey, ContentHash)]) -> BuildResultHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai-build-result-v1\0");
    for (artifact_key, content_hash) in outputs {
        update_length_prefixed(&mut hasher, artifact_key.as_str());
        hasher.update(content_hash.as_bytes());
    }
    BuildResultHash::new(*hasher.finalize().as_bytes())
}

fn report_binding_hash(report: &BuildReport) -> [u8; 32] {
    let bytes = serde_json::to_vec(report).expect("engine-owned build reports always serialize");
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai-build-report-v1\0");
    hasher.update(&bytes);
    *hasher.finalize().as_bytes()
}

fn update_length_prefixed(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&length_u32(value.len()).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn length_u32(length: usize) -> u32 {
    u32::try_from(length).expect("validated engine metadata must fit in a u32 frame length")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde::Serialize;

    use super::{erased_stage_error, BuildEngine, BuildOutcomeIntegrityError, ExternalArtifacts};
    use crate::engine::artifact::{
        Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts,
        StoredArtifact,
    };
    use crate::engine::cache::{MemoryStageCache, StageCacheKey};
    use crate::engine::diagnostics::{Diagnostic, DiagnosticSeverity};
    use crate::engine::graph::StageGraphBuilder;
    use crate::engine::random::{derive_stage_seed, StageIdentity, StageRng};
    use crate::engine::stage::{Stage, StageError, StageId, StageInputs};
    use crate::world::RootSeed;

    #[derive(Debug, Serialize)]
    struct External(u32);

    impl Artifact for External {
        const KEY: ArtifactKey = ArtifactKey::new("test.external");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize)]
    struct Output(u32);

    impl Artifact for Output {
        const KEY: ArtifactKey = ArtifactKey::new("test.output");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize)]
    struct PoisonedOutput(u32);

    impl Artifact for PoisonedOutput {
        const KEY: ArtifactKey = Output::KEY;

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    struct Inputs(Arc<External>);

    impl StageInputs for Inputs {
        fn dependencies() -> &'static [ArtifactKey] {
            &[External::KEY]
        }

        fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
            artifacts.get::<External>().map(Self)
        }
    }

    struct OutputStage;

    impl Stage for OutputStage {
        type Inputs = Inputs;
        type Output = Output;

        fn id(&self) -> StageId {
            StageId::new("test.output-stage")
        }

        fn version(&self) -> u32 {
            1
        }

        fn namespace(&self) -> &'static str {
            "test"
        }

        fn run(
            &self,
            inputs: Self::Inputs,
            _rng: &mut StageRng,
            _diagnostics: &mut Vec<Diagnostic>,
        ) -> Result<Self::Output, StageError> {
            let external_value = inputs.0.as_ref().0;
            Ok(Output(external_value))
        }
    }

    fn successful_outcome(root_seed: RootSeed, external_value: u32) -> super::BuildOutcome {
        let engine = BuildEngine::new(
            StageGraphBuilder::new()
                .external::<External>()
                .stage(OutputStage)
                .build()
                .unwrap(),
        );
        let mut external = ExternalArtifacts::new();
        external.insert(External(external_value)).unwrap();
        engine
            .build(root_seed, external, &mut MemoryStageCache::new())
            .unwrap()
    }

    #[test]
    fn successful_outcome_provenance_binds_seed_report_and_artifact_store() {
        let first_seed = RootSeed::new(42);
        let mut first = successful_outcome(first_seed, 7);
        let second = successful_outcome(RootSeed::new(43), 8);

        let provenance = first.verified_provenance().unwrap();
        assert_eq!(provenance.root_seed(), first_seed);
        assert_eq!(
            provenance.result_hash(),
            first.report.result_hash().unwrap()
        );

        first.report.push_diagnostic(
            Diagnostic::new(
                DiagnosticSeverity::Warning,
                "test.foreign-warning",
                "must invalidate successful outcome provenance",
            )
            .unwrap(),
        );
        assert!(matches!(
            first.verified_provenance(),
            Err(BuildOutcomeIntegrityError::ReportMetadataMismatch { .. })
        ));

        let mut first = successful_outcome(first_seed, 7);
        first.report = second.report.clone();
        assert!(matches!(
            first.verified_provenance(),
            Err(BuildOutcomeIntegrityError::ReportResultHashMismatch { .. })
        ));

        let mut first = successful_outcome(first_seed, 7);
        first.artifacts = second.artifacts;
        assert!(matches!(
            first.verified_provenance(),
            Err(BuildOutcomeIntegrityError::ArtifactSetMismatch { .. })
        ));
    }

    #[test]
    fn poisoned_cache_output_type_fails_checked_restore_without_reporting_a_hit() {
        let identity = StageIdentity::new("test.output-stage", 1, "test");
        let root_seed = RootSeed::new(42);
        let mut external = ExternalArtifacts::new();
        external.insert(External(7)).unwrap();
        let external_hash = external.hash::<External>().unwrap();
        let key = StageCacheKey::new(
            identity,
            Output::KEY,
            derive_stage_seed(root_seed, identity),
            &[(External::KEY, external_hash)],
        )
        .unwrap();
        let mut cache = MemoryStageCache::new();
        cache.insert(
            key,
            StoredArtifact::new(PoisonedOutput(9)).unwrap(),
            vec![Diagnostic::new(
                DiagnosticSeverity::Warning,
                "test.stale-cache-diagnostic",
                "must not replay after restore failure",
            )
            .unwrap()],
        );
        let engine = BuildEngine::new(
            StageGraphBuilder::new()
                .external::<External>()
                .stage(OutputStage)
                .build()
                .unwrap(),
        );

        let failure = engine.build(root_seed, external, &mut cache).unwrap_err();

        assert_eq!(failure.report.cache_hits(), 0);
        assert!(failure.report.stage_ids().is_empty());
        let diagnostic = &failure.report.diagnostics()[0];
        assert_eq!(diagnostic.code(), "engine.cache-restore");
        assert_eq!(
            diagnostic.context().stage_id.as_deref(),
            Some("test.output-stage")
        );
        assert_eq!(failure.report.diagnostics().len(), 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn validation_codes_survive_every_erased_artifact_error_boundary() {
        for error in [
            crate::engine::stage::ErasedStageError::Inputs(validation_error()),
            crate::engine::stage::ErasedStageError::Output(validation_error()),
            crate::engine::stage::ErasedStageError::Publication(validation_error()),
        ] {
            let (code, message) = erased_stage_error(error);

            assert_eq!(code, "test.boundary-validation");
            assert_eq!(message, "invalid at erased boundary");
        }
    }

    fn validation_error() -> ArtifactError {
        ArtifactError::Validation {
            artifact_key: Output::KEY,
            source: ArtifactValidationError::new(
                "test.boundary-validation",
                "invalid at erased boundary",
            ),
        }
    }
}
