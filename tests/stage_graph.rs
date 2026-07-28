use std::marker::PhantomData;
use std::sync::Arc;

use sekai::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts, Diagnostic,
    GraphError, Stage, StageError, StageGraph, StageGraphBuilder, StageId, StageInputs, StageRng,
};
use serde::Serialize;

macro_rules! test_artifact {
    ($name:ident, $key:literal) => {
        #[derive(Debug, Serialize)]
        struct $name(i32);

        impl Artifact for $name {
            const KEY: ArtifactKey = ArtifactKey::new($key);

            fn validate(&self) -> Result<(), ArtifactValidationError> {
                Ok(())
            }
        }
    };
}

test_artifact!(SpecArtifact, "test.spec");
test_artifact!(AArtifact, "test.a");
test_artifact!(BArtifact, "test.b");
test_artifact!(CArtifact, "test.c");
test_artifact!(ZArtifact, "test.z");
test_artifact!(InvalidKeyArtifact, "Bad.Key");

struct NoInputs;

impl StageInputs for NoInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[]
    }

    fn load(_artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self)
    }
}

struct OneInput<T: Artifact>(#[allow(dead_code)] Arc<T>);

impl<T: Artifact> StageInputs for OneInput<T> {
    fn dependencies() -> &'static [ArtifactKey] {
        &[T::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        artifacts.get::<T>().map(Self)
    }
}

struct ReversePairInputs {
    #[allow(dead_code)]
    spec: Arc<SpecArtifact>,
    #[allow(dead_code)]
    a: Arc<AArtifact>,
}

impl StageInputs for ReversePairInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[SpecArtifact::KEY, AArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            spec: artifacts.get::<SpecArtifact>()?,
            a: artifacts.get::<AArtifact>()?,
        })
    }
}

struct DuplicateSpecInputs;

impl StageInputs for DuplicateSpecInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[SpecArtifact::KEY, SpecArtifact::KEY]
    }

    fn load(_artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self)
    }
}

struct TestStage<I, O> {
    id: StageId,
    marker: PhantomData<fn() -> (I, O)>,
}

impl<I, O> TestStage<I, O> {
    const fn new(id: &'static str) -> Self {
        Self {
            id: StageId::new(id),
            marker: PhantomData,
        }
    }
}

impl<I, O> Stage for TestStage<I, O>
where
    I: StageInputs + Sync,
    O: Artifact,
{
    type Inputs = I;
    type Output = O;

    fn id(&self) -> StageId {
        self.id
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
        unreachable!("graph validation never executes stages")
    }
}

fn stage<I, O>(id: &'static str) -> TestStage<I, O> {
    TestStage::new(id)
}

fn graph_with_external_spec_and_two_stages() -> Result<StageGraph, GraphError> {
    StageGraphBuilder::new()
        .external::<SpecArtifact>()
        .stage(stage::<OneInput<AArtifact>, BArtifact>("test.b"))
        .stage(stage::<OneInput<SpecArtifact>, AArtifact>("test.a"))
        .build()
}

#[test]
fn sorts_stages_by_declared_artifact_dependencies() {
    let graph = graph_with_external_spec_and_two_stages().unwrap();

    assert_eq!(graph.stage_ids(), vec!["test.a", "test.b"]);
}

#[test]
fn rejects_missing_artifact_provider() {
    let error = StageGraphBuilder::new()
        .stage(stage::<OneInput<SpecArtifact>, AArtifact>("test.a"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::MissingProvider {
            stage_id,
            artifact_key
        } if stage_id == StageId::new("test.a") && artifact_key == SpecArtifact::KEY
    ));
}

#[test]
fn rejects_duplicate_output_provider() {
    let error = StageGraphBuilder::new()
        .stage(stage::<NoInputs, AArtifact>("test.first"))
        .stage(stage::<NoInputs, AArtifact>("test.second"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::DuplicateProvider { artifact_key }
            if artifact_key == AArtifact::KEY
    ));
}

#[test]
fn rejects_duplicate_external_provider() {
    let error = StageGraphBuilder::new()
        .external::<SpecArtifact>()
        .external::<SpecArtifact>()
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::DuplicateProvider { artifact_key }
            if artifact_key == SpecArtifact::KEY
    ));
}

#[test]
fn rejects_external_and_stage_provider_collision() {
    let error = StageGraphBuilder::new()
        .external::<AArtifact>()
        .stage(stage::<NoInputs, AArtifact>("test.a"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::DuplicateProvider { artifact_key }
            if artifact_key == AArtifact::KEY
    ));
}

#[test]
fn rejects_dependency_cycles_with_sorted_remaining_stage_ids() {
    let error = StageGraphBuilder::new()
        .stage(stage::<OneInput<AArtifact>, CArtifact>("test.c"))
        .stage(stage::<OneInput<BArtifact>, AArtifact>("test.a"))
        .stage(stage::<OneInput<AArtifact>, BArtifact>("test.b"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::Cycle {
            remaining_stage_ids
        } if remaining_stage_ids
            == vec![
                StageId::new("test.a"),
                StageId::new("test.b"),
                StageId::new("test.c")
            ]
    ));
}

#[test]
fn sorts_independent_stages_by_stage_id() {
    let graph = StageGraphBuilder::new()
        .stage(stage::<NoInputs, ZArtifact>("test.z"))
        .stage(stage::<NoInputs, AArtifact>("test.a"))
        .build()
        .unwrap();

    assert_eq!(graph.stage_ids(), vec!["test.a", "test.z"]);
}

#[test]
fn rejects_invalid_stage_identifier() {
    let error = StageGraphBuilder::new()
        .stage(stage::<NoInputs, AArtifact>("Test.Bad"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::InvalidStageId { stage_id }
            if stage_id == StageId::new("Test.Bad")
    ));
}

#[test]
fn rejects_invalid_external_artifact_identifier() {
    let error = StageGraphBuilder::new()
        .external::<InvalidKeyArtifact>()
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::InvalidArtifactKey { artifact_key }
            if artifact_key == InvalidKeyArtifact::KEY
    ));
}

#[test]
fn rejects_invalid_output_artifact_identifier() {
    let error = StageGraphBuilder::new()
        .stage(stage::<NoInputs, InvalidKeyArtifact>("test.invalid-output"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::InvalidArtifactKey { artifact_key }
            if artifact_key == InvalidKeyArtifact::KEY
    ));
}

#[test]
fn rejects_invalid_dependency_artifact_identifier() {
    let error = StageGraphBuilder::new()
        .stage(stage::<OneInput<InvalidKeyArtifact>, AArtifact>(
            "test.invalid-dependency",
        ))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::InvalidArtifactKey { artifact_key }
            if artifact_key == InvalidKeyArtifact::KEY
    ));
}

#[test]
fn rejects_duplicate_stage_ids() {
    let error = StageGraphBuilder::new()
        .stage(stage::<NoInputs, AArtifact>("test.same"))
        .stage(stage::<NoInputs, BArtifact>("test.same"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::DuplicateStage { stage_id }
            if stage_id == StageId::new("test.same")
    ));
}

#[test]
fn rejects_duplicate_dependency_keys() {
    let error = StageGraphBuilder::new()
        .external::<SpecArtifact>()
        .stage(stage::<DuplicateSpecInputs, AArtifact>("test.a"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::DuplicateDependency {
            stage_id,
            artifact_key
        } if stage_id == StageId::new("test.a") && artifact_key == SpecArtifact::KEY
    ));
}

#[test]
fn rejects_direct_self_dependency() {
    let error = StageGraphBuilder::new()
        .stage(stage::<OneInput<AArtifact>, AArtifact>("test.a"))
        .build()
        .unwrap_err();

    assert!(matches!(
        error,
        GraphError::SelfDependency {
            stage_id,
            artifact_key
        } if stage_id == StageId::new("test.a") && artifact_key == AArtifact::KEY
    ));
}

#[test]
fn descriptors_keep_sorted_dependency_keys_and_stage_metadata() {
    let graph = StageGraphBuilder::new()
        .external::<SpecArtifact>()
        .external::<AArtifact>()
        .stage(stage::<ReversePairInputs, BArtifact>("test.b"))
        .build()
        .unwrap();

    let descriptor = &graph.descriptors()[0];
    assert_eq!(descriptor.id(), StageId::new("test.b"));
    assert_eq!(descriptor.version(), 1);
    assert_eq!(descriptor.namespace(), "test");
    assert_eq!(descriptor.output(), BArtifact::KEY);
    assert_eq!(
        descriptor.dependencies(),
        &[AArtifact::KEY, SpecArtifact::KEY]
    );
}
