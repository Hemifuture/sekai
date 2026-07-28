use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use sekai::engine::{
    derive_stage_seed, Artifact, ArtifactError, ArtifactKey, ArtifactValidationError,
    BuildArtifacts, BuildEngine, Diagnostic, DiagnosticSeverity, ExternalArtifacts,
    MemoryStageCache, Stage, StageCacheError, StageCacheKey, StageError, StageGraphBuilder,
    StageId, StageIdentity, StageInputs, StageRng,
};
use sekai::world::RootSeed;
use serde::ser::Error as _;
use serde::{Serialize, Serializer};

macro_rules! scalar_artifact {
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

scalar_artifact!(SpecArtifact, "test.spec");
scalar_artifact!(RightSpecArtifact, "test.right-spec");
scalar_artifact!(ExtraArtifact, "test.extra");
scalar_artifact!(AArtifact, "test.a");
scalar_artifact!(BArtifact, "test.b");
scalar_artifact!(LeftArtifact, "test.left");
scalar_artifact!(RightArtifact, "test.right");
scalar_artifact!(JoinArtifact, "test.join");
scalar_artifact!(ZFirstArtifact, "test.z-first");
scalar_artifact!(ASecondArtifact, "test.a-second");

#[derive(Debug, Serialize)]
struct WrongSpecArtifact(i32);

impl Artifact for WrongSpecArtifact {
    const KEY: ArtifactKey = SpecArtifact::KEY;

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct InvalidExternalArtifact;

impl Artifact for InvalidExternalArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("test.invalid-external");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Err(ArtifactValidationError::new(
            "test.invalid-external",
            "external value is invalid",
        ))
    }
}

#[derive(Debug, Serialize)]
struct InvalidOutputArtifact;

impl Artifact for InvalidOutputArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("test.invalid-output");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Err(ArtifactValidationError::new(
            "test.invalid-output",
            "stage output is invalid",
        ))
    }
}

#[derive(Debug, Serialize)]
struct InvalidCodeOutputArtifact;

impl Artifact for InvalidCodeOutputArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("test.invalid-code-output");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Err(ArtifactValidationError::new(
            "Bad Code",
            "stage output uses an invalid validation code",
        ))
    }
}

#[derive(Debug)]
struct SerializationFailureArtifact;

impl Serialize for SerializationFailureArtifact {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(S::Error::custom("intentional serialization failure"))
    }
}

impl Artifact for SerializationFailureArtifact {
    const KEY: ArtifactKey = ArtifactKey::new("test.serialization-failure");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Ok(())
    }
}

struct SpecInput(Arc<SpecArtifact>);

impl StageInputs for SpecInput {
    fn dependencies() -> &'static [ArtifactKey] {
        &[SpecArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        artifacts.get::<SpecArtifact>().map(Self)
    }
}

struct RightSpecInput(Arc<RightSpecArtifact>);

impl StageInputs for RightSpecInput {
    fn dependencies() -> &'static [ArtifactKey] {
        &[RightSpecArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        artifacts.get::<RightSpecArtifact>().map(Self)
    }
}

struct AInput(Arc<AArtifact>);

impl StageInputs for AInput {
    fn dependencies() -> &'static [ArtifactKey] {
        &[AArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        artifacts.get::<AArtifact>().map(Self)
    }
}

struct JoinInput {
    left: Arc<LeftArtifact>,
    right: Arc<RightArtifact>,
}

struct ZFirstInput(Arc<ZFirstArtifact>);

impl StageInputs for ZFirstInput {
    fn dependencies() -> &'static [ArtifactKey] {
        &[ZFirstArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        artifacts.get::<ZFirstArtifact>().map(Self)
    }
}

impl StageInputs for JoinInput {
    fn dependencies() -> &'static [ArtifactKey] {
        &[RightArtifact::KEY, LeftArtifact::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        Ok(Self {
            left: artifacts.get::<LeftArtifact>()?,
            right: artifacts.get::<RightArtifact>()?,
        })
    }
}

struct AStage {
    version: u32,
    runs: Option<Arc<AtomicUsize>>,
}

impl AStage {
    fn new(version: u32) -> Self {
        Self {
            version,
            runs: None,
        }
    }

    fn counted(version: u32, runs: Arc<AtomicUsize>) -> Self {
        Self {
            version,
            runs: Some(runs),
        }
    }
}

impl Stage for AStage {
    type Inputs = SpecInput;
    type Output = AArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.a")
    }

    fn version(&self) -> u32 {
        self.version
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
        if let Some(runs) = &self.runs {
            runs.fetch_add(1, Ordering::SeqCst);
        }
        Ok(AArtifact(inputs.0 .0 + 1))
    }
}

struct BStage;

impl Stage for BStage {
    type Inputs = AInput;
    type Output = BArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.b")
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
        Ok(BArtifact(inputs.0 .0 * 2))
    }
}

struct LeftStage;

impl Stage for LeftStage {
    type Inputs = SpecInput;
    type Output = LeftArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.left")
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
        Ok(LeftArtifact(inputs.0 .0 + 10))
    }
}

struct RightStage;

impl Stage for RightStage {
    type Inputs = RightSpecInput;
    type Output = RightArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.right")
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
        Ok(RightArtifact(inputs.0 .0 + 20))
    }
}

struct JoinStage;

impl Stage for JoinStage {
    type Inputs = JoinInput;
    type Output = JoinArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.join")
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
        Ok(JoinArtifact(inputs.left.0 + inputs.right.0))
    }
}

struct ZFirstStage;

impl Stage for ZFirstStage {
    type Inputs = SpecInput;
    type Output = ZFirstArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.first")
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
        Ok(ZFirstArtifact(inputs.0 .0 + 10))
    }
}

struct ASecondStage;

impl Stage for ASecondStage {
    type Inputs = ZFirstInput;
    type Output = ASecondArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.second")
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
        Ok(ASecondArtifact(inputs.0 .0 * 2))
    }
}

struct RecoverableBStage {
    fail: Arc<AtomicBool>,
}

impl Stage for RecoverableBStage {
    type Inputs = AInput;
    type Output = BArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.b")
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
        if self.fail.load(Ordering::SeqCst) {
            Err(StageError::new("test.stage-failure", "stage failed"))
        } else {
            Ok(BArtifact(inputs.0 .0 * 2))
        }
    }
}

struct StageFailure;

impl Stage for StageFailure {
    type Inputs = SpecInput;
    type Output = AArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.stage-failure")
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
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticSeverity::Warning,
                "test.before-failure",
                "warning emitted before failure",
            )
            .unwrap(),
        );
        Err(StageError::new("test.stage-failure", "stage failed"))
    }
}

struct InvalidOutputStage;

impl Stage for InvalidOutputStage {
    type Inputs = SpecInput;
    type Output = InvalidOutputArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.invalid-output-stage")
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
        Ok(InvalidOutputArtifact)
    }
}

struct SerializationFailureStage;

impl Stage for SerializationFailureStage {
    type Inputs = SpecInput;
    type Output = SerializationFailureArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.serialization-failure-stage")
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
        Ok(SerializationFailureArtifact)
    }
}

struct InvalidCodeOutputStage;

impl Stage for InvalidCodeOutputStage {
    type Inputs = SpecInput;
    type Output = InvalidCodeOutputArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.invalid-code-output-stage")
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
        Ok(InvalidCodeOutputArtifact)
    }
}

struct DiagnosticStage {
    severity: DiagnosticSeverity,
    runs: Arc<AtomicUsize>,
}

impl Stage for DiagnosticStage {
    type Inputs = SpecInput;
    type Output = AArtifact;

    fn id(&self) -> StageId {
        StageId::new("test.diagnostic")
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
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        diagnostics.push(
            Diagnostic::new(self.severity, "test.stage-diagnostic", "stage diagnostic").unwrap(),
        );
        Ok(AArtifact(inputs.0 .0 + 1))
    }
}

fn spec(value: i32) -> ExternalArtifacts {
    let mut external = ExternalArtifacts::new();
    external.insert(SpecArtifact(value)).unwrap();
    external
}

fn branched_inputs(left: i32, right: i32) -> ExternalArtifacts {
    let mut external = spec(left);
    external.insert(RightSpecArtifact(right)).unwrap();
    external
}

fn two_stage_engine(version: u32) -> BuildEngine {
    BuildEngine::new(
        StageGraphBuilder::new()
            .external::<SpecArtifact>()
            .stage(AStage::new(version))
            .stage(BStage)
            .build()
            .unwrap(),
    )
}

fn one_stage_engine(version: u32) -> BuildEngine {
    BuildEngine::new(
        StageGraphBuilder::new()
            .external::<SpecArtifact>()
            .stage(AStage::new(version))
            .build()
            .unwrap(),
    )
}

fn branched_engine() -> BuildEngine {
    BuildEngine::new(
        StageGraphBuilder::new()
            .external::<SpecArtifact>()
            .external::<RightSpecArtifact>()
            .stage(JoinStage)
            .stage(RightStage)
            .stage(LeftStage)
            .build()
            .unwrap(),
    )
}

fn single_stage_engine<S: Stage>(stage: S) -> BuildEngine {
    BuildEngine::new(
        StageGraphBuilder::new()
            .external::<SpecArtifact>()
            .stage(stage)
            .build()
            .unwrap(),
    )
}

fn a_cache_key(
    root_seed: RootSeed,
    version: u32,
    spec_hash: sekai::engine::ContentHash,
) -> StageCacheKey {
    let identity = StageIdentity::new("test.a", version, "test");
    StageCacheKey::new(
        identity,
        AArtifact::KEY,
        derive_stage_seed(root_seed, identity),
        &[(SpecArtifact::KEY, spec_hash)],
    )
    .unwrap()
}

#[test]
fn executes_each_stage_once_in_topological_order() {
    let engine = two_stage_engine(1);
    let mut cache = MemoryStageCache::new();

    let outcome = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();

    assert_eq!(outcome.report.stage_ids(), vec!["test.a", "test.b"]);
    assert_eq!(outcome.report.cache_hits(), 0);
    assert_eq!(outcome.report.cache_misses(), 2);
}

#[test]
fn second_identical_build_hits_cache_and_shares_artifact_arcs() {
    let engine = two_stage_engine(1);
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();
    let first_a = first.artifacts.get::<AArtifact>().unwrap();

    let second = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();
    let second_a = second.artifacts.get::<AArtifact>().unwrap();

    assert_eq!(second.report.cache_hits(), 2);
    assert_eq!(second.report.cache_misses(), 0);
    assert!(Arc::ptr_eq(&first_a, &second_a));
}

#[test]
fn changed_external_hash_invalidates_downstream_stages() {
    let engine = two_stage_engine(1);
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();

    let outcome = engine
        .build(RootSeed::new(42), spec(2), &mut cache)
        .unwrap();

    assert_eq!(outcome.report.cache_hits(), 0);
    assert_eq!(outcome.report.cache_misses(), 2);
}

#[test]
fn changed_external_hash_keeps_unrelated_branch_cached() {
    let engine = branched_engine();
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), branched_inputs(1, 10), &mut cache)
        .unwrap();

    let outcome = engine
        .build(RootSeed::new(42), branched_inputs(2, 10), &mut cache)
        .unwrap();

    assert_eq!(outcome.report.cache_hits(), 1);
    assert_eq!(outcome.report.cache_misses(), 2);
}

#[test]
fn changed_root_seed_invalidates_all_stages() {
    let engine = two_stage_engine(1);
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();

    let outcome = engine
        .build(RootSeed::new(43), spec(1), &mut cache)
        .unwrap();

    assert_eq!(outcome.report.cache_hits(), 0);
    assert_eq!(outcome.report.cache_misses(), 2);
}

#[test]
fn changed_stage_version_misses_the_prior_cache_entry() {
    let mut cache = MemoryStageCache::new();
    one_stage_engine(1)
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();

    let outcome = one_stage_engine(2)
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();

    assert_eq!(outcome.report.cache_hits(), 0);
    assert_eq!(outcome.report.cache_misses(), 1);
}

#[test]
fn cache_is_bounded_fifo_and_gets_do_not_refresh_age() {
    let hash_source = spec(1);
    let spec_hash = hash_source.hash::<SpecArtifact>().unwrap();
    let mut cache = MemoryStageCache::with_max_entries(2).unwrap();
    let engine = one_stage_engine(1);

    engine
        .build(RootSeed::new(1), hash_source, &mut cache)
        .unwrap();
    engine.build(RootSeed::new(2), spec(1), &mut cache).unwrap();
    let hit = engine.build(RootSeed::new(1), spec(1), &mut cache).unwrap();
    assert_eq!(hit.report.cache_hits(), 1);
    engine.build(RootSeed::new(3), spec(1), &mut cache).unwrap();

    let first = a_cache_key(RootSeed::new(1), 1, spec_hash);
    let second = a_cache_key(RootSeed::new(2), 1, spec_hash);
    let third = a_cache_key(RootSeed::new(3), 1, spec_hash);
    assert_eq!(cache.len(), 2);
    assert!(!cache.contains(&first));
    assert!(cache.contains(&second));
    assert!(cache.contains(&third));
}

#[test]
fn cache_rejects_zero_capacity() {
    assert!(matches!(
        MemoryStageCache::with_max_entries(0),
        Err(StageCacheError::ZeroCapacity)
    ));
}

#[test]
fn cache_defaults_to_thirty_two_empty_entries() {
    let cache = MemoryStageCache::new();

    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);
    assert_eq!(cache.max_entries(), 32);
}

#[test]
fn cache_key_uses_the_exact_v1_byte_frame() {
    let mut external = ExternalArtifacts::new();
    external.insert(SpecArtifact(3)).unwrap();
    external.insert(RightSpecArtifact(5)).unwrap();
    let spec_hash = external.hash::<SpecArtifact>().unwrap();
    let right_hash = external.hash::<RightSpecArtifact>().unwrap();
    let identity = StageIdentity::new("test.cache", 12, "example.mod");
    let stage_seed = derive_stage_seed(RootSeed::new(42), identity);

    let actual = StageCacheKey::new(
        identity,
        AArtifact::KEY,
        stage_seed,
        &[
            (SpecArtifact::KEY, spec_hash),
            (RightSpecArtifact::KEY, right_hash),
        ],
    )
    .unwrap();

    let mut expected_frame = Vec::new();
    expected_frame.extend_from_slice(b"sekai-stage-cache-v1\0");
    expected_frame.extend_from_slice(&11_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"example.mod");
    expected_frame.extend_from_slice(&10_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"test.cache");
    expected_frame.extend_from_slice(&12_u32.to_le_bytes());
    expected_frame.extend_from_slice(&6_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"test.a");
    expected_frame.extend_from_slice(&stage_seed.into_bytes());
    expected_frame.extend_from_slice(&2_u32.to_le_bytes());
    expected_frame.extend_from_slice(&15_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"test.right-spec");
    expected_frame.extend_from_slice(right_hash.as_bytes());
    expected_frame.extend_from_slice(&9_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"test.spec");
    expected_frame.extend_from_slice(spec_hash.as_bytes());
    let expected = blake3::hash(&expected_frame);

    assert_eq!(actual.as_bytes(), expected.as_bytes());
}

#[test]
fn external_artifacts_validate_hash_and_reject_duplicates_on_insert() {
    let mut external = ExternalArtifacts::new();
    external.insert(SpecArtifact(7)).unwrap();
    let expected = blake3::hash(b"7");

    assert_eq!(
        external.hash::<SpecArtifact>().unwrap().as_bytes(),
        expected.as_bytes()
    );
    assert!(matches!(
        external.insert(SpecArtifact(8)),
        Err(ArtifactError::Duplicate { artifact_key })
            if artifact_key == SpecArtifact::KEY
    ));
    assert!(matches!(
        external.insert(InvalidExternalArtifact),
        Err(ArtifactError::Validation { artifact_key, .. })
            if artifact_key == InvalidExternalArtifact::KEY
    ));
    assert!(matches!(
        external.insert(SerializationFailureArtifact),
        Err(ArtifactError::Serialization { artifact_key, .. })
            if artifact_key == SerializationFailureArtifact::KEY
    ));
}

#[test]
fn exact_external_set_is_enforced_before_a_possible_cache_hit() {
    let engine = one_stage_engine(1);
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();
    let mut extra = spec(1);
    extra.insert(ExtraArtifact(9)).unwrap();

    let failure = engine
        .build(RootSeed::new(42), extra, &mut cache)
        .unwrap_err();

    assert!(failure.report.stage_ids().is_empty());
    assert_eq!(failure.report.cache_hits(), 0);
    assert_eq!(failure.report.cache_misses(), 0);
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "engine.external-artifact-set"
    );
}

#[test]
fn external_type_is_checked_before_a_possible_cache_hit() {
    let engine = one_stage_engine(1);
    let mut cache = MemoryStageCache::new();
    engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();
    let mut wrong = ExternalArtifacts::new();
    wrong.insert(WrongSpecArtifact(1)).unwrap();

    let failure = engine
        .build(RootSeed::new(42), wrong, &mut cache)
        .unwrap_err();

    assert!(failure.report.stage_ids().is_empty());
    assert_eq!(failure.report.cache_hits(), 0);
    assert_eq!(failure.report.cache_misses(), 0);
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "engine.external-artifact"
    );
}

#[test]
fn a_later_failure_returns_only_a_report_but_keeps_validated_earlier_cache_entries() {
    let fail = Arc::new(AtomicBool::new(true));
    let graph = StageGraphBuilder::new()
        .external::<SpecArtifact>()
        .stage(AStage::new(1))
        .stage(RecoverableBStage {
            fail: Arc::clone(&fail),
        })
        .build()
        .unwrap();
    let engine = BuildEngine::new(graph);
    let mut cache = MemoryStageCache::new();

    let failure = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap_err();

    assert!(failure.report.has_errors());
    assert_eq!(cache.len(), 1);

    fail.store(false, Ordering::SeqCst);
    let recovered = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();
    assert_eq!(recovered.report.cache_hits(), 1);
    assert_eq!(recovered.report.cache_misses(), 1);
}

#[test]
fn validation_failure_is_atomic_and_not_cached() {
    let engine = single_stage_engine(InvalidOutputStage);
    let mut cache = MemoryStageCache::new();

    let failure = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap_err();

    assert!(failure.report.has_errors());
    assert_eq!(cache.len(), 0);
    let diagnostic = failure.report.diagnostics().last().unwrap();
    assert_eq!(diagnostic.code(), "test.invalid-output");
    assert_eq!(diagnostic.message(), "stage output is invalid");
    assert_eq!(
        diagnostic.context().stage_id.as_deref(),
        Some("test.invalid-output-stage")
    );
}

#[test]
fn invalid_output_validation_code_keeps_the_normalized_fallback() {
    let engine = single_stage_engine(InvalidCodeOutputStage);
    let mut cache = MemoryStageCache::new();

    let failure = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap_err();

    let diagnostic = failure.report.diagnostics().last().unwrap();
    assert_eq!(diagnostic.code(), "engine.invalid-artifact-validation-code");
    assert!(diagnostic.message().contains("Bad Code"));
    assert_eq!(
        diagnostic.context().stage_id.as_deref(),
        Some("test.invalid-code-output-stage")
    );
}

#[test]
fn serialization_failure_is_atomic_and_not_cached() {
    let engine = single_stage_engine(SerializationFailureStage);
    let mut cache = MemoryStageCache::new();

    let failure = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap_err();

    assert!(failure.report.has_errors());
    assert_eq!(cache.len(), 0);
    let diagnostic = failure.report.diagnostics().last().unwrap();
    assert_eq!(diagnostic.code(), "engine.stage-output");
    assert_eq!(
        diagnostic.context().stage_id.as_deref(),
        Some("test.serialization-failure-stage")
    );
}

#[test]
fn stage_failure_preserves_emitted_diagnostics_and_is_not_cached() {
    let engine = single_stage_engine(StageFailure);
    let mut cache = MemoryStageCache::new();

    let failure = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap_err();

    assert_eq!(cache.len(), 0);
    assert_eq!(failure.report.diagnostics().len(), 2);
    assert_eq!(
        failure.report.diagnostics()[0].code(),
        "test.before-failure"
    );
    let mapped = &failure.report.diagnostics()[1];
    assert_eq!(mapped.code(), "test.stage-failure");
    assert_eq!(mapped.message(), "stage failed");
    assert_eq!(
        mapped.context().stage_id.as_deref(),
        Some("test.stage-failure")
    );
}

#[test]
fn error_diagnostic_with_ok_output_fails_and_is_not_cached() {
    let runs = Arc::new(AtomicUsize::new(0));
    let engine = single_stage_engine(DiagnosticStage {
        severity: DiagnosticSeverity::Error,
        runs: Arc::clone(&runs),
    });
    let mut cache = MemoryStageCache::new();

    let first = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap_err();
    let second = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap_err();

    assert!(first.report.has_errors());
    assert!(second.report.has_errors());
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    assert_eq!(cache.len(), 0);
    assert_eq!(
        first.report.diagnostics()[0].code(),
        "test.stage-diagnostic"
    );
}

#[test]
fn warning_diagnostic_succeeds_and_is_cached() {
    let runs = Arc::new(AtomicUsize::new(0));
    let engine = single_stage_engine(DiagnosticStage {
        severity: DiagnosticSeverity::Warning,
        runs: Arc::clone(&runs),
    });
    let mut cache = MemoryStageCache::new();

    let first = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();
    let second = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();

    assert!(!first.report.has_errors());
    assert_eq!(first.report.diagnostics().len(), 1);
    assert_eq!(first.report.cache_misses(), 1);
    assert_eq!(second.report.cache_hits(), 1);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[test]
fn info_diagnostic_succeeds() {
    let runs = Arc::new(AtomicUsize::new(0));
    let engine = single_stage_engine(DiagnosticStage {
        severity: DiagnosticSeverity::Info,
        runs,
    });
    let mut cache = MemoryStageCache::new();

    let outcome = engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();

    assert!(!outcome.report.has_errors());
    assert_eq!(outcome.report.diagnostics().len(), 1);
    assert!(outcome.report.result_hash().is_some());
}

#[test]
fn result_hash_is_identical_across_cache_misses_and_hits() {
    let engine = two_stage_engine(1);
    let mut cache = MemoryStageCache::new();

    let first = engine
        .build(RootSeed::new(42), spec(3), &mut cache)
        .unwrap();
    let second = engine
        .build(RootSeed::new(42), spec(3), &mut cache)
        .unwrap();

    assert_eq!(first.report.cache_misses(), 2);
    assert_eq!(second.report.cache_hits(), 2);
    assert_eq!(first.report.result_hash(), second.report.result_hash());
}

#[test]
fn result_hash_uses_the_exact_v1_output_only_byte_frame() {
    let engine = two_stage_engine(1);
    let mut cache = MemoryStageCache::new();

    let outcome = engine
        .build(RootSeed::new(42), spec(3), &mut cache)
        .unwrap();

    let a_hash = blake3::hash(b"4");
    let b_hash = blake3::hash(b"8");
    let mut expected_frame = Vec::new();
    expected_frame.extend_from_slice(b"sekai-build-result-v1\0");
    expected_frame.extend_from_slice(&6_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"test.a");
    expected_frame.extend_from_slice(a_hash.as_bytes());
    expected_frame.extend_from_slice(&6_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"test.b");
    expected_frame.extend_from_slice(b_hash.as_bytes());
    let expected = blake3::hash(&expected_frame);

    assert_eq!(
        outcome.report.result_hash().unwrap().as_bytes(),
        expected.as_bytes()
    );
}

#[test]
fn result_hash_frames_non_lexical_output_keys_in_graph_order() {
    let engine = BuildEngine::new(
        StageGraphBuilder::new()
            .external::<SpecArtifact>()
            .stage(ASecondStage)
            .stage(ZFirstStage)
            .build()
            .unwrap(),
    );
    let mut cache = MemoryStageCache::new();

    let outcome = engine
        .build(RootSeed::new(42), spec(3), &mut cache)
        .unwrap();

    assert_eq!(
        outcome.report.stage_ids(),
        vec!["test.first", "test.second"]
    );
    let first_hash = blake3::hash(b"13");
    let second_hash = blake3::hash(b"26");
    let mut expected_frame = Vec::new();
    expected_frame.extend_from_slice(b"sekai-build-result-v1\0");
    expected_frame.extend_from_slice(&12_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"test.z-first");
    expected_frame.extend_from_slice(first_hash.as_bytes());
    expected_frame.extend_from_slice(&13_u32.to_le_bytes());
    expected_frame.extend_from_slice(b"test.a-second");
    expected_frame.extend_from_slice(second_hash.as_bytes());
    let expected = blake3::hash(&expected_frame);

    assert_eq!(
        outcome.report.result_hash().unwrap().as_bytes(),
        expected.as_bytes()
    );
}

#[test]
fn stage_execution_count_stays_one_on_a_cache_hit() {
    let runs = Arc::new(AtomicUsize::new(0));
    let engine = BuildEngine::new(
        StageGraphBuilder::new()
            .external::<SpecArtifact>()
            .stage(AStage::counted(1, Arc::clone(&runs)))
            .build()
            .unwrap(),
    );
    let mut cache = MemoryStageCache::new();

    engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();
    engine
        .build(RootSeed::new(42), spec(1), &mut cache)
        .unwrap();

    assert_eq!(runs.load(Ordering::SeqCst), 1);
}
