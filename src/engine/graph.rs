use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use crate::engine::artifact::{
    Artifact, ArtifactError, ArtifactKey, ArtifactType, BuildArtifacts, ContentHash,
};
use crate::engine::diagnostics::is_valid_identifier;
use crate::engine::stage::{
    erase_stage, ErasedStage, Stage, StageDescriptor, StageId, StageInputs,
};

/// Errors returned while validating a generation-stage graph.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphError {
    /// A stage identifier violates the supported stable identifier grammar.
    #[error("stage identifier `{stage_id:?}` is invalid")]
    InvalidStageId {
        /// The rejected stage identifier.
        stage_id: StageId,
    },
    /// An artifact key violates the supported stable identifier grammar.
    #[error("artifact key `{artifact_key:?}` is invalid")]
    InvalidArtifactKey {
        /// The rejected artifact key.
        artifact_key: ArtifactKey,
    },
    /// More than one stage registration uses the same stage identifier.
    #[error("stage identifier `{stage_id:?}` was registered more than once")]
    DuplicateStage {
        /// The duplicated stage identifier.
        stage_id: StageId,
    },
    /// More than one external or stage output provides an artifact key.
    #[error("artifact key `{artifact_key:?}` has more than one provider")]
    DuplicateProvider {
        /// The artifact key with multiple providers.
        artifact_key: ArtifactKey,
    },
    /// One stage declares the same dependency key more than once.
    #[error("stage `{stage_id:?}` declares dependency `{artifact_key:?}` more than once")]
    DuplicateDependency {
        /// The stage containing the duplicate declaration.
        stage_id: StageId,
        /// The duplicated dependency key.
        artifact_key: ArtifactKey,
    },
    /// No external or stage output provides a declared dependency.
    #[error("stage `{stage_id:?}` has no provider for `{artifact_key:?}`")]
    MissingProvider {
        /// The stage with the unresolved dependency.
        stage_id: StageId,
        /// The dependency key without a provider.
        artifact_key: ArtifactKey,
    },
    /// A stage directly depends on the same artifact key it publishes.
    #[error("stage `{stage_id:?}` directly depends on its output `{artifact_key:?}`")]
    SelfDependency {
        /// The self-dependent stage.
        stage_id: StageId,
        /// The stage's dependency and output key.
        artifact_key: ArtifactKey,
    },
    /// The graph contains a dependency cycle.
    #[error("stage graph contains a cycle involving {remaining_stage_ids:?}")]
    Cycle {
        /// All stages left by Kahn's algorithm, sorted by stage identifier.
        remaining_stage_ids: Vec<StageId>,
    },
}

struct RegisteredStage {
    descriptor: StageDescriptor,
    output_type: ArtifactType,
    stage: Box<dyn ErasedStage>,
}

/// A fluent collector for external artifacts and typed stages.
#[derive(Default)]
pub struct StageGraphBuilder {
    external_artifacts: Vec<ArtifactType>,
    stages: Vec<RegisteredStage>,
}

impl StageGraphBuilder {
    /// Creates an empty stage graph builder.
    pub const fn new() -> Self {
        Self {
            external_artifacts: Vec::new(),
            stages: Vec::new(),
        }
    }

    /// Registers an artifact type supplied externally to each build.
    pub fn external<T: Artifact>(mut self) -> Self {
        self.external_artifacts.push(ArtifactType::of::<T>());
        self
    }

    /// Registers one typed generation stage without normalizing declarations.
    pub fn stage<S: Stage>(mut self, stage: S) -> Self {
        let dependencies = S::Inputs::dependencies().to_vec();
        let output_type = ArtifactType::of::<S::Output>();
        let descriptor = StageDescriptor::new(
            stage.id(),
            stage.version(),
            stage.namespace(),
            dependencies.clone(),
            output_type.key(),
        );
        self.stages.push(RegisteredStage {
            descriptor,
            output_type,
            stage: erase_stage(stage, dependencies),
        });
        self
    }

    /// Validates registrations and returns a deterministic topological graph.
    pub fn build(mut self) -> Result<StageGraph, GraphError> {
        validate_identifiers(&self.external_artifacts, &self.stages)?;
        validate_unique_stage_ids(&self.stages)?;
        validate_unique_providers(&self.external_artifacts, &self.stages)?;

        self.external_artifacts
            .sort_unstable_by_key(|artifact_type| artifact_type.key());
        self.stages
            .sort_by_key(|registration| registration.descriptor.id());
        normalize_dependencies(&mut self.stages)?;

        let providers = providers(&self.external_artifacts, &self.stages);
        validate_dependencies(&providers, &self.stages)?;
        let stage_order = topological_order(&providers, &self.stages)?;
        let provider_types = provider_types(&self.external_artifacts, &self.stages);

        let mut registrations = self
            .stages
            .into_iter()
            .map(|registration| (registration.descriptor.id(), registration))
            .collect::<BTreeMap<_, _>>();
        let mut descriptors = Vec::with_capacity(stage_order.len());
        let mut stages = Vec::with_capacity(stage_order.len());
        for stage_id in stage_order {
            let registration = registrations
                .remove(&stage_id)
                .expect("validated stage order must reference every registration");
            descriptors.push(registration.descriptor);
            stages.push(registration.stage);
        }

        Ok(StageGraph {
            external_artifact_types: self.external_artifacts,
            provider_types,
            descriptors,
            stages,
        })
    }
}

/// A validated generation graph in deterministic execution order.
pub struct StageGraph {
    external_artifact_types: Vec<ArtifactType>,
    provider_types: BTreeMap<ArtifactKey, ArtifactType>,
    descriptors: Vec<StageDescriptor>,
    stages: Vec<Box<dyn ErasedStage>>,
}

impl fmt::Debug for StageGraph {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let external_artifact_keys = self
            .external_artifact_types
            .iter()
            .map(|artifact_type| artifact_type.key())
            .collect::<Vec<_>>();
        formatter
            .debug_struct("StageGraph")
            .field("external_artifacts", &external_artifact_keys)
            .field("descriptors", &self.descriptors)
            .finish_non_exhaustive()
    }
}

impl StageGraph {
    /// Returns freshly collected stage identifiers in execution order.
    pub fn stage_ids(&self) -> Vec<&str> {
        self.descriptors
            .iter()
            .map(|descriptor| descriptor.id().as_str())
            .collect()
    }

    /// Returns immutable descriptors in deterministic execution order.
    pub fn descriptors(&self) -> &[StageDescriptor] {
        &self.descriptors
    }

    #[allow(dead_code)] // Used to seed checked external state in Task 9.
    pub(crate) fn external_hashes(
        &self,
        artifacts: &BuildArtifacts,
    ) -> Result<Vec<(ArtifactKey, ContentHash)>, ArtifactError> {
        let mut hashes = Vec::with_capacity(self.external_artifact_types.len());
        for artifact_type in &self.external_artifact_types {
            hashes.push((artifact_type.key(), artifact_type.hash_in(artifacts)?));
        }
        Ok(hashes)
    }

    #[allow(dead_code)] // Used to frame one stage cache key in Task 9.
    pub(crate) fn dependency_hashes(
        &self,
        descriptor: &StageDescriptor,
        artifacts: &BuildArtifacts,
    ) -> Result<Vec<(ArtifactKey, ContentHash)>, ArtifactError> {
        let mut hashes = Vec::with_capacity(descriptor.dependencies().len());
        for artifact_key in descriptor.dependencies() {
            let artifact_type = self
                .provider_types
                .get(artifact_key)
                .expect("validated dependency must retain its provider type");
            hashes.push((*artifact_key, artifact_type.hash_in(artifacts)?));
        }
        Ok(hashes)
    }

    #[allow(dead_code)] // Used to frame the successful build result in Task 9.
    pub(crate) fn output_hashes(
        &self,
        artifacts: &BuildArtifacts,
    ) -> Result<Vec<(ArtifactKey, ContentHash)>, ArtifactError> {
        let mut hashes = Vec::with_capacity(self.descriptors.len());
        for descriptor in &self.descriptors {
            let artifact_key = descriptor.output();
            let artifact_type = self
                .provider_types
                .get(&artifact_key)
                .expect("validated output must retain its provider type");
            hashes.push((artifact_key, artifact_type.hash_in(artifacts)?));
        }
        Ok(hashes)
    }

    #[allow(dead_code)] // Read by the scheduler introduced in Task 9.
    pub(crate) fn execution_stages(
        &self,
    ) -> impl ExactSizeIterator<Item = (&StageDescriptor, &dyn ErasedStage)> {
        self.descriptors
            .iter()
            .zip(self.stages.iter().map(|stage| stage.as_ref()))
    }
}

fn validate_identifiers(
    external_artifacts: &[ArtifactType],
    stages: &[RegisteredStage],
) -> Result<(), GraphError> {
    let invalid_stage_id = stages
        .iter()
        .map(|registration| registration.descriptor.id())
        .filter(|stage_id| !is_valid_identifier(stage_id.as_str()))
        .min();
    if let Some(stage_id) = invalid_stage_id {
        return Err(GraphError::InvalidStageId { stage_id });
    }

    let invalid_artifact_key = external_artifacts
        .iter()
        .map(|artifact_type| artifact_type.key())
        .chain(stages.iter().flat_map(|registration| {
            registration
                .descriptor
                .dependencies()
                .iter()
                .copied()
                .chain(std::iter::once(registration.descriptor.output()))
        }))
        .filter(|artifact_key| !is_valid_identifier(artifact_key.as_str()))
        .min();
    if let Some(artifact_key) = invalid_artifact_key {
        return Err(GraphError::InvalidArtifactKey { artifact_key });
    }

    Ok(())
}

fn validate_unique_stage_ids(stages: &[RegisteredStage]) -> Result<(), GraphError> {
    let mut counts = BTreeMap::<StageId, usize>::new();
    for registration in stages {
        *counts.entry(registration.descriptor.id()).or_default() += 1;
    }
    if let Some((stage_id, _)) = counts.into_iter().find(|(_, count)| *count > 1) {
        return Err(GraphError::DuplicateStage { stage_id });
    }
    Ok(())
}

fn validate_unique_providers(
    external_artifacts: &[ArtifactType],
    stages: &[RegisteredStage],
) -> Result<(), GraphError> {
    let mut counts = BTreeMap::<ArtifactKey, usize>::new();
    for artifact_key in external_artifacts
        .iter()
        .map(|artifact_type| artifact_type.key())
        .chain(
            stages
                .iter()
                .map(|registration| registration.descriptor.output()),
        )
    {
        *counts.entry(artifact_key).or_default() += 1;
    }
    if let Some((artifact_key, _)) = counts.into_iter().find(|(_, count)| *count > 1) {
        return Err(GraphError::DuplicateProvider { artifact_key });
    }
    Ok(())
}

fn normalize_dependencies(stages: &mut [RegisteredStage]) -> Result<(), GraphError> {
    for registration in stages {
        let descriptor = &registration.descriptor;
        let mut dependencies = descriptor.dependencies().to_vec();
        dependencies.sort_unstable();
        if let Some(artifact_key) = dependencies
            .windows(2)
            .find_map(|pair| (pair[0] == pair[1]).then_some(pair[0]))
        {
            return Err(GraphError::DuplicateDependency {
                stage_id: descriptor.id(),
                artifact_key,
            });
        }
        registration.descriptor = StageDescriptor::new(
            descriptor.id(),
            descriptor.version(),
            descriptor.namespace(),
            dependencies,
            descriptor.output(),
        );
    }
    Ok(())
}

fn providers(
    external_artifacts: &[ArtifactType],
    stages: &[RegisteredStage],
) -> BTreeMap<ArtifactKey, Option<StageId>> {
    let mut providers = external_artifacts
        .iter()
        .map(|artifact_type| (artifact_type.key(), None))
        .collect::<BTreeMap<_, _>>();
    for registration in stages {
        providers.insert(
            registration.descriptor.output(),
            Some(registration.descriptor.id()),
        );
    }
    providers
}

fn provider_types(
    external_artifacts: &[ArtifactType],
    stages: &[RegisteredStage],
) -> BTreeMap<ArtifactKey, ArtifactType> {
    external_artifacts
        .iter()
        .copied()
        .chain(stages.iter().map(|registration| registration.output_type))
        .map(|artifact_type| (artifact_type.key(), artifact_type))
        .collect()
}

fn validate_dependencies(
    providers: &BTreeMap<ArtifactKey, Option<StageId>>,
    stages: &[RegisteredStage],
) -> Result<(), GraphError> {
    for registration in stages {
        let descriptor = &registration.descriptor;
        if descriptor.dependencies().contains(&descriptor.output()) {
            return Err(GraphError::SelfDependency {
                stage_id: descriptor.id(),
                artifact_key: descriptor.output(),
            });
        }
        if let Some(artifact_key) = descriptor
            .dependencies()
            .iter()
            .copied()
            .find(|artifact_key| !providers.contains_key(artifact_key))
        {
            return Err(GraphError::MissingProvider {
                stage_id: descriptor.id(),
                artifact_key,
            });
        }
    }
    Ok(())
}

fn topological_order(
    providers: &BTreeMap<ArtifactKey, Option<StageId>>,
    stages: &[RegisteredStage],
) -> Result<Vec<StageId>, GraphError> {
    let mut indegrees = stages
        .iter()
        .map(|registration| (registration.descriptor.id(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut dependents = stages
        .iter()
        .map(|registration| (registration.descriptor.id(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();

    for registration in stages {
        let stage_id = registration.descriptor.id();
        for dependency in registration.descriptor.dependencies() {
            let Some(Some(provider_id)) = providers.get(dependency) else {
                continue;
            };
            if dependents
                .get_mut(provider_id)
                .expect("validated provider must be a registered stage")
                .insert(stage_id)
            {
                *indegrees
                    .get_mut(&stage_id)
                    .expect("registered stage must have an indegree") += 1;
            }
        }
    }

    let mut ready = indegrees
        .iter()
        .filter_map(|(stage_id, indegree)| (*indegree == 0).then_some(*stage_id))
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(stages.len());

    while let Some(stage_id) = ready.pop_first() {
        ordered.push(stage_id);
        for dependent_id in dependents
            .get(&stage_id)
            .expect("registered stage must have a dependent set")
        {
            let indegree = indegrees
                .get_mut(dependent_id)
                .expect("dependent stage must have an indegree");
            *indegree -= 1;
            if *indegree == 0 {
                ready.insert(*dependent_id);
            }
        }
    }

    if ordered.len() != stages.len() {
        let remaining_stage_ids = indegrees
            .into_iter()
            .filter_map(|(stage_id, indegree)| (indegree > 0).then_some(stage_id))
            .collect();
        return Err(GraphError::Cycle {
            remaining_stage_ids,
        });
    }

    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde::Serialize;

    use super::{StageGraph, StageGraphBuilder};
    use crate::engine::artifact::{
        Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts,
    };
    use crate::engine::diagnostics::Diagnostic;
    use crate::engine::random::StageRng;
    use crate::engine::stage::{Stage, StageError, StageId, StageInputs};

    #[derive(Debug, Serialize)]
    struct ExpectedExternal(u32);

    impl Artifact for ExpectedExternal {
        const KEY: ArtifactKey = ArtifactKey::new("test.external");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize)]
    struct WrongExternal(u32);

    impl Artifact for WrongExternal {
        const KEY: ArtifactKey = ExpectedExternal::KEY;

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize)]
    struct AnotherExternal(u32);

    impl Artifact for AnotherExternal {
        const KEY: ArtifactKey = ArtifactKey::new("test.another-external");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize)]
    struct DependencyOutput;

    impl Artifact for DependencyOutput {
        const KEY: ArtifactKey = ArtifactKey::new("test.dependency-output");

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    #[derive(Debug, Serialize)]
    struct WrongDependencyOutput;

    impl Artifact for WrongDependencyOutput {
        const KEY: ArtifactKey = DependencyOutput::KEY;

        fn validate(&self) -> Result<(), ArtifactValidationError> {
            Ok(())
        }
    }

    struct DependencyInputs {
        #[allow(dead_code)]
        expected: Arc<ExpectedExternal>,
        #[allow(dead_code)]
        another: Arc<AnotherExternal>,
    }

    impl StageInputs for DependencyInputs {
        fn dependencies() -> &'static [ArtifactKey] {
            &[ExpectedExternal::KEY, AnotherExternal::KEY]
        }

        fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
            Ok(Self {
                expected: artifacts.get::<ExpectedExternal>()?,
                another: artifacts.get::<AnotherExternal>()?,
            })
        }
    }

    struct DependencyStage;

    impl Stage for DependencyStage {
        type Inputs = DependencyInputs;
        type Output = DependencyOutput;

        fn id(&self) -> StageId {
            StageId::new("test.dependencies")
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
            Ok(DependencyOutput)
        }
    }

    fn dependency_graph() -> StageGraph {
        StageGraphBuilder::new()
            .external::<ExpectedExternal>()
            .external::<AnotherExternal>()
            .stage(DependencyStage)
            .build()
            .unwrap()
    }

    #[test]
    fn external_validation_distinguishes_missing_from_same_key_type_mismatch() {
        let graph = StageGraphBuilder::new()
            .external::<ExpectedExternal>()
            .build()
            .unwrap();
        let missing = BuildArtifacts::default();

        assert!(matches!(
            graph.external_hashes(&missing),
            Err(ArtifactError::Missing { artifact_key })
                if artifact_key == ExpectedExternal::KEY
        ));

        let mut mismatched = BuildArtifacts::default();
        mismatched.insert(WrongExternal(1)).unwrap();
        assert!(matches!(
            graph.external_hashes(&mismatched),
            Err(ArtifactError::TypeMismatch { artifact_key })
                if artifact_key == ExpectedExternal::KEY
        ));
    }

    #[test]
    fn external_hashes_are_type_checked_and_sorted_by_artifact_key() {
        let graph = StageGraphBuilder::new()
            .external::<ExpectedExternal>()
            .external::<AnotherExternal>()
            .build()
            .unwrap();
        let mut artifacts = BuildArtifacts::default();
        artifacts.insert(ExpectedExternal(1)).unwrap();
        artifacts.insert(AnotherExternal(2)).unwrap();

        let hashes = graph.external_hashes(&artifacts).unwrap();

        assert_eq!(
            hashes,
            vec![
                (
                    AnotherExternal::KEY,
                    artifacts.hash::<AnotherExternal>().unwrap()
                ),
                (
                    ExpectedExternal::KEY,
                    artifacts.hash::<ExpectedExternal>().unwrap()
                ),
            ]
        );
    }

    #[test]
    fn dependency_hashing_rejects_same_key_wrong_type() {
        let graph = dependency_graph();
        let mut artifacts = BuildArtifacts::default();
        artifacts.insert(WrongExternal(1)).unwrap();
        artifacts.insert(AnotherExternal(2)).unwrap();

        let error = graph
            .dependency_hashes(&graph.descriptors()[0], &artifacts)
            .unwrap_err();

        assert!(matches!(
            error,
            ArtifactError::TypeMismatch { artifact_key }
                if artifact_key == ExpectedExternal::KEY
        ));
    }

    #[test]
    fn dependency_hashes_follow_sorted_descriptor_order() {
        let graph = dependency_graph();
        let mut artifacts = BuildArtifacts::default();
        artifacts.insert(ExpectedExternal(1)).unwrap();
        artifacts.insert(AnotherExternal(2)).unwrap();

        let hashes = graph
            .dependency_hashes(&graph.descriptors()[0], &artifacts)
            .unwrap();

        assert_eq!(
            hashes,
            vec![
                (
                    AnotherExternal::KEY,
                    artifacts.hash::<AnotherExternal>().unwrap()
                ),
                (
                    ExpectedExternal::KEY,
                    artifacts.hash::<ExpectedExternal>().unwrap()
                ),
            ]
        );
    }

    #[test]
    fn output_hashes_retain_stage_output_type_identity() {
        let graph = dependency_graph();
        let mut mismatched = BuildArtifacts::default();
        mismatched.insert(WrongDependencyOutput).unwrap();

        assert!(matches!(
            graph.output_hashes(&mismatched),
            Err(ArtifactError::TypeMismatch { artifact_key })
                if artifact_key == DependencyOutput::KEY
        ));

        let mut valid = BuildArtifacts::default();
        valid.insert(DependencyOutput).unwrap();
        assert_eq!(
            graph.output_hashes(&valid).unwrap(),
            vec![(
                DependencyOutput::KEY,
                valid.hash::<DependencyOutput>().unwrap()
            )]
        );
    }
}
