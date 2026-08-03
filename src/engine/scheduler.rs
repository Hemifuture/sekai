use std::time::Instant;

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
            let started = Instant::now();
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
        report.set_result_hash(result_hash(&output_hashes));
        Ok(BuildOutcome { artifacts, report })
    }
}

/// A complete successful build and its ordered report.
#[derive(Debug)]
pub struct BuildOutcome {
    /// All checked external and produced typed artifacts.
    pub artifacts: BuildArtifacts,
    /// Deterministic stage ordering, diagnostics, and non-semantic reporting metadata.
    pub report: BuildReport,
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

    use super::{erased_stage_error, BuildEngine, ExternalArtifacts};
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

    struct Inputs(#[allow(dead_code)] Arc<External>);

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
            _inputs: Self::Inputs,
            _rng: &mut StageRng,
            _diagnostics: &mut Vec<Diagnostic>,
        ) -> Result<Self::Output, StageError> {
            Ok(Output(1))
        }
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
