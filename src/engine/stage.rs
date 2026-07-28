use thiserror::Error;

use crate::engine::artifact::{
    Artifact, ArtifactError, ArtifactKey, ArtifactType, BuildArtifacts, StoredArtifact,
};
use crate::engine::diagnostics::{is_valid_identifier, Diagnostic};
use crate::engine::random::StageRng;

const INVALID_STAGE_ERROR_CODE: &str = "engine.invalid-stage-error-code";

/// A stable identifier for one generation stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StageId(&'static str);

impl StageId {
    /// Creates a stage identifier for later graph validation.
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    /// Returns the identifier's static string value.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Immutable scheduling metadata declared by a generation stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageDescriptor {
    id: StageId,
    version: u32,
    namespace: &'static str,
    dependencies: Vec<ArtifactKey>,
    output: ArtifactKey,
}

impl StageDescriptor {
    pub(crate) fn new(
        id: StageId,
        version: u32,
        namespace: &'static str,
        dependencies: Vec<ArtifactKey>,
        output: ArtifactKey,
    ) -> Self {
        Self {
            id,
            version,
            namespace,
            dependencies,
            output,
        }
    }

    /// Returns the stable stage identifier.
    pub const fn id(&self) -> StageId {
        self.id
    }

    /// Returns the version included in random seeds and cache keys.
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Returns the owning namespace included in random seeds and cache keys.
    pub const fn namespace(&self) -> &'static str {
        self.namespace
    }

    /// Returns dependency keys in stable artifact-key order.
    pub fn dependencies(&self) -> &[ArtifactKey] {
        &self.dependencies
    }

    /// Returns the artifact key published by this stage.
    pub const fn output(&self) -> ArtifactKey {
        self.output
    }
}

/// A structured failure returned by a concrete generation stage.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct StageError {
    code: &'static str,
    message: String,
}

impl StageError {
    /// Creates a stage failure with a stable code and readable message.
    ///
    /// Invalid developer-supplied codes are replaced with an engine-owned stable
    /// code while the rejected code is retained in the message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        if is_valid_identifier(code) {
            Self { code, message }
        } else {
            Self {
                code: INVALID_STAGE_ERROR_CODE,
                message: format!("invalid stage error code `{code}`: {message}"),
            }
        }
    }

    /// Returns the stable machine-readable failure code.
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the human-readable failure message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A typed dependency bundle loaded for one concrete stage invocation.
pub trait StageInputs: Sized + Send + 'static {
    /// Declares every artifact key required by this input bundle.
    fn dependencies() -> &'static [ArtifactKey];

    /// Loads the typed dependency bundle from validated build artifacts.
    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError>;
}

/// A typed deterministic generation stage.
pub trait Stage: Send + Sync + 'static {
    /// The typed dependency bundle presented to the concrete stage.
    type Inputs: StageInputs;
    /// The single typed artifact published after a successful invocation.
    type Output: Artifact;

    /// Returns the stable stage identifier.
    fn id(&self) -> StageId;

    /// Returns the stage implementation version.
    fn version(&self) -> u32;

    /// Returns the namespace owning the stage's deterministic streams.
    fn namespace(&self) -> &'static str;

    /// Runs the stage using typed inputs, an isolated RNG, and diagnostics.
    fn run(
        &self,
        inputs: Self::Inputs,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError>;
}

#[derive(Debug, Error)]
pub(crate) enum ErasedStageError {
    #[error("stage inputs could not be loaded: {0}")]
    Inputs(#[source] ArtifactError),
    #[error("stage execution failed: {0}")]
    Stage(#[source] StageError),
    #[error("stage output could not be prepared: {0}")]
    Output(#[source] ArtifactError),
}

#[allow(dead_code)] // Executed by the scheduler introduced in Task 9.
pub(crate) trait ErasedStage: Send + Sync {
    fn run(
        &self,
        artifacts: &BuildArtifacts,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<StoredArtifact, ErasedStageError>;

    fn validate_cached_output(&self, stored: &StoredArtifact) -> Result<(), ArtifactError>;
}

struct ErasedStageAdapter<S> {
    stage: S,
    dependencies: Vec<ArtifactKey>,
    output_type: ArtifactType,
}

impl<S: Stage> ErasedStage for ErasedStageAdapter<S> {
    fn run(
        &self,
        artifacts: &BuildArtifacts,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<StoredArtifact, ErasedStageError> {
        let dependency_view = artifacts
            .dependency_view(&self.dependencies)
            .map_err(ErasedStageError::Inputs)?;
        let inputs = S::Inputs::load(&dependency_view).map_err(ErasedStageError::Inputs)?;
        let output = self
            .stage
            .run(inputs, rng, diagnostics)
            .map_err(ErasedStageError::Stage)?;
        StoredArtifact::new(output).map_err(ErasedStageError::Output)
    }

    fn validate_cached_output(&self, stored: &StoredArtifact) -> Result<(), ArtifactError> {
        self.output_type.validate_stored(stored)
    }
}

pub(crate) fn erase_stage<S: Stage>(
    stage: S,
    mut dependencies: Vec<ArtifactKey>,
) -> Box<dyn ErasedStage> {
    dependencies.sort_unstable();
    Box::new(ErasedStageAdapter {
        stage,
        dependencies,
        output_type: ArtifactType::of::<S::Output>(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde::Serialize;

    use super::{ErasedStageError, Stage, StageError, StageId, StageInputs};
    use crate::engine::artifact::{
        Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts,
        StoredArtifact,
    };
    use crate::engine::diagnostics::Diagnostic;
    use crate::engine::graph::{StageGraph, StageGraphBuilder};
    use crate::engine::random::{derive_stage_seed, StageIdentity, StageRng};
    use crate::world::RootSeed;

    macro_rules! artifact {
        ($name:ident, $key:literal) => {
            #[derive(Debug, Serialize)]
            struct $name(u32);

            impl Artifact for $name {
                const KEY: ArtifactKey = ArtifactKey::new($key);

                fn validate(&self) -> Result<(), ArtifactValidationError> {
                    Ok(())
                }
            }
        };
    }

    artifact!(DeclaredArtifact, "test.declared");
    artifact!(UndeclaredArtifact, "test.undeclared");
    artifact!(OutputArtifact, "test.output");

    #[derive(Debug, Serialize)]
    struct WrongOutputArtifact(u32);

    impl Artifact for WrongOutputArtifact {
        const KEY: ArtifactKey = OutputArtifact::KEY;

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    struct AdversarialInputs {
        #[allow(dead_code)]
        declared: Arc<DeclaredArtifact>,
    }

    impl StageInputs for AdversarialInputs {
        fn dependencies() -> &'static [ArtifactKey] {
            &[DeclaredArtifact::KEY]
        }

        fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
            let declared = artifacts.get::<DeclaredArtifact>()?;
            artifacts.get::<UndeclaredArtifact>()?;
            Ok(Self { declared })
        }
    }

    struct BoundaryStage;

    impl Stage for BoundaryStage {
        type Inputs = AdversarialInputs;
        type Output = OutputArtifact;

        fn id(&self) -> StageId {
            StageId::new("test.boundary")
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
            Ok(OutputArtifact(1))
        }
    }

    fn rng() -> StageRng {
        StageRng::from_seed(derive_stage_seed(
            RootSeed::new(7),
            StageIdentity::new("test.boundary", 1, "test"),
        ))
    }

    fn graph() -> StageGraph {
        StageGraphBuilder::new()
            .external::<DeclaredArtifact>()
            .external::<UndeclaredArtifact>()
            .stage(BoundaryStage)
            .build()
            .unwrap()
    }

    #[test]
    fn erased_stage_hides_undeclared_artifacts_from_input_loader() {
        let mut artifacts = BuildArtifacts::default();
        artifacts.insert(DeclaredArtifact(1)).unwrap();
        artifacts.insert(UndeclaredArtifact(2)).unwrap();
        let graph = graph();
        let (_, stage) = graph.execution_stages().next().unwrap();

        let error = match stage.run(&artifacts, &mut rng(), &mut Vec::new()) {
            Ok(_) => panic!("undeclared access unexpectedly succeeded"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            ErasedStageError::Inputs(ArtifactError::Missing { artifact_key })
                if artifact_key == UndeclaredArtifact::KEY
        ));
    }

    #[test]
    fn erased_stage_rejects_same_key_cached_output_of_another_type() {
        let graph = graph();
        let (_, stage) = graph.execution_stages().next().unwrap();
        let cached = StoredArtifact::new(WrongOutputArtifact(9)).unwrap();

        let error = stage.validate_cached_output(&cached).unwrap_err();

        assert!(matches!(
            error,
            ArtifactError::TypeMismatch { artifact_key }
                if artifact_key == OutputArtifact::KEY
        ));
    }

    #[test]
    fn erased_stage_accepts_cached_output_of_declared_type() {
        let graph = graph();
        let (_, stage) = graph.execution_stages().next().unwrap();
        let cached = StoredArtifact::new(OutputArtifact(9)).unwrap();

        assert!(stage.validate_cached_output(&cached).is_ok());
    }
}
