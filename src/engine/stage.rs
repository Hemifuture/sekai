use thiserror::Error;

use crate::engine::artifact::{
    Artifact, ArtifactError, ArtifactKey, BuildArtifacts, StoredArtifact,
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
}

struct ErasedStageAdapter<S>(S);

impl<S: Stage> ErasedStage for ErasedStageAdapter<S> {
    fn run(
        &self,
        artifacts: &BuildArtifacts,
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<StoredArtifact, ErasedStageError> {
        let inputs = S::Inputs::load(artifacts).map_err(ErasedStageError::Inputs)?;
        let output = self
            .0
            .run(inputs, rng, diagnostics)
            .map_err(ErasedStageError::Stage)?;
        StoredArtifact::new(output).map_err(ErasedStageError::Output)
    }
}

pub(crate) fn erase_stage<S: Stage>(stage: S) -> Box<dyn ErasedStage> {
    Box::new(ErasedStageAdapter(stage))
}
