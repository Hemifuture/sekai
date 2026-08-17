use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rand::RngCore;
use sekai::engine::{
    Artifact, ArtifactError, ArtifactKey, ArtifactValidationError, BuildArtifacts,
    BuildCancellation, BuildEngine, Diagnostic, DiagnosticSeverity, ExternalArtifacts,
    MemoryStageCache, Stage, StageError, StageGraphBuilder, StageId, StageInputs, StageRng,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceBuildError};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};
use serde::Serialize;

const RADIUS_M: f64 = 6_371_000.0;

#[derive(Debug, Serialize)]
struct Spec(u64);

impl Artifact for Spec {
    const KEY: ArtifactKey = ArtifactKey::new("test.cancellation-spec");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct First(u64);

impl Artifact for First {
    const KEY: ArtifactKey = ArtifactKey::new("test.cancellation-first");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct Final(u64);

impl Artifact for Final {
    const KEY: ArtifactKey = ArtifactKey::new("test.cancellation-final");

    fn validate(&self) -> Result<(), ArtifactValidationError> {
        Ok(())
    }
}

struct SpecInputs(Arc<Spec>);

impl StageInputs for SpecInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[Spec::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        artifacts.get::<Spec>().map(Self)
    }
}

struct FirstInputs(Arc<First>);

impl StageInputs for FirstInputs {
    fn dependencies() -> &'static [ArtifactKey] {
        &[First::KEY]
    }

    fn load(artifacts: &BuildArtifacts) -> Result<Self, ArtifactError> {
        artifacts.get::<First>().map(Self)
    }
}

struct RandomStage {
    runs: Arc<AtomicUsize>,
}

impl Stage for RandomStage {
    type Inputs = SpecInputs;
    type Output = First;

    fn id(&self) -> StageId {
        StageId::new("test.cancellation-random")
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
        rng: &mut StageRng,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        diagnostics.push(
            Diagnostic::new(
                DiagnosticSeverity::Info,
                "test.cancellation-random",
                "deterministic random stage",
            )
            .unwrap(),
        );
        Ok(First(inputs.0 .0 ^ rng.next_u64()))
    }
}

struct FinalStage;

impl Stage for FinalStage {
    type Inputs = FirstInputs;
    type Output = Final;

    fn id(&self) -> StageId {
        StageId::new("test.cancellation-final")
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
        Ok(Final(inputs.0 .0.rotate_left(7)))
    }
}

struct CancellingStage {
    cancellation: BuildCancellation,
    cooperative_runs: Arc<AtomicUsize>,
}

impl Stage for CancellingStage {
    type Inputs = SpecInputs;
    type Output = First;

    fn id(&self) -> StageId {
        StageId::new("test.cancellation-cooperative")
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
        rng: &mut StageRng,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        assert!(!rng.is_cancelled());
        self.cancellation.cancel();
        assert!(rng.is_cancelled());
        self.cooperative_runs.fetch_add(1, Ordering::SeqCst);
        Ok(First(inputs.0 .0))
    }
}

fn external(value: u64) -> ExternalArtifacts {
    let mut external = ExternalArtifacts::new();
    external.insert(Spec(value)).unwrap();
    external
}

fn deterministic_engine(runs: Arc<AtomicUsize>) -> BuildEngine {
    BuildEngine::new(
        StageGraphBuilder::new()
            .external::<Spec>()
            .stage(FinalStage)
            .stage(RandomStage { runs })
            .build()
            .unwrap(),
    )
}

#[test]
fn cancellation_before_build_returns_only_a_stable_error_report() {
    let runs = Arc::new(AtomicUsize::new(0));
    let engine = deterministic_engine(Arc::clone(&runs));
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    let mut cache = MemoryStageCache::new();

    let failure = engine
        .build_with_cancellation(RootSeed::new(42), external(7), &mut cache, &cancellation)
        .unwrap_err();

    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert!(cache.is_empty());
    assert!(failure.report.stage_ids().is_empty());
    assert_eq!(failure.report.diagnostics().len(), 1);
    let diagnostic = &failure.report.diagnostics()[0];
    assert_eq!(diagnostic.code(), "engine.cancelled");
    assert_eq!(diagnostic.severity(), DiagnosticSeverity::Error);
    assert!(diagnostic.context().stage_id.is_none());
}

#[test]
fn cancellation_observed_inside_a_stage_stops_before_cache_and_downstream_work() {
    let cancellation = BuildCancellation::new();
    let stage_token = cancellation.clone();
    let cooperative_runs = Arc::new(AtomicUsize::new(0));
    let final_runs = Arc::new(AtomicUsize::new(0));
    let engine = BuildEngine::new(
        StageGraphBuilder::new()
            .external::<Spec>()
            .stage(CountingFinalStage {
                runs: Arc::clone(&final_runs),
            })
            .stage(CancellingStage {
                cancellation: stage_token,
                cooperative_runs: Arc::clone(&cooperative_runs),
            })
            .build()
            .unwrap(),
    );
    let mut cache = MemoryStageCache::new();

    let failure = engine
        .build_with_cancellation(RootSeed::new(42), external(7), &mut cache, &cancellation)
        .unwrap_err();

    assert_eq!(cooperative_runs.load(Ordering::SeqCst), 1);
    assert_eq!(final_runs.load(Ordering::SeqCst), 0);
    assert!(cache.is_empty());
    let diagnostic = failure.report.diagnostics().last().unwrap();
    assert_eq!(diagnostic.code(), "engine.cancelled");
    assert_eq!(
        diagnostic.context().stage_id.as_deref(),
        Some("test.cancellation-cooperative")
    );
}

struct CountingFinalStage {
    runs: Arc<AtomicUsize>,
}

impl Stage for CountingFinalStage {
    type Inputs = FirstInputs;
    type Output = Final;

    fn id(&self) -> StageId {
        StageId::new("test.cancellation-counting-final")
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
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(Final(inputs.0 .0))
    }
}

#[test]
fn never_cancelled_build_is_semantically_identical_to_the_legacy_entry_point() {
    let engine = deterministic_engine(Arc::new(AtomicUsize::new(0)));
    let seed = RootSeed::new(4_242);
    let mut legacy_cache = MemoryStageCache::new();
    let legacy = engine.build(seed, external(91), &mut legacy_cache).unwrap();
    let mut cancellable_cache = MemoryStageCache::new();
    let cancellable = engine
        .build_with_cancellation(
            seed,
            external(91),
            &mut cancellable_cache,
            &BuildCancellation::new(),
        )
        .unwrap();

    assert_eq!(
        serde_json::to_vec(legacy.artifacts.get::<First>().unwrap().as_ref()).unwrap(),
        serde_json::to_vec(cancellable.artifacts.get::<First>().unwrap().as_ref()).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(legacy.artifacts.get::<Final>().unwrap().as_ref()).unwrap(),
        serde_json::to_vec(cancellable.artifacts.get::<Final>().unwrap().as_ref()).unwrap()
    );
    assert_eq!(legacy.report.stage_ids(), cancellable.report.stage_ids());
    assert_eq!(
        legacy.report.diagnostics(),
        cancellable.report.diagnostics()
    );
    assert_eq!(
        legacy.report.result_hash(),
        cancellable.report.result_hash()
    );
    assert_eq!(
        legacy.verified_provenance().unwrap().result_hash(),
        cancellable.verified_provenance().unwrap().result_hash()
    );
}

#[test]
fn cancelled_cache_restore_attempt_does_not_mutate_cache_or_prior_outcome() {
    let engine = deterministic_engine(Arc::new(AtomicUsize::new(0)));
    let mut cache = MemoryStageCache::new();
    let outcome = engine
        .build(RootSeed::new(99), external(5), &mut cache)
        .unwrap();
    let prior_result_hash = *outcome.verified_provenance().unwrap().result_hash();
    let cache_len = cache.len();
    let cancellation = BuildCancellation::new();
    cancellation.cancel();

    let failure = engine
        .build_with_cancellation(RootSeed::new(99), external(5), &mut cache, &cancellation)
        .unwrap_err();

    assert_eq!(failure.report.diagnostics()[0].code(), "engine.cancelled");
    assert_eq!(cache.len(), cache_len);
    assert_eq!(
        outcome.verified_provenance().unwrap().result_hash(),
        &prior_result_hash
    );
}

#[test]
fn geodesic_builder_polls_cooperatively_and_never_publishes_a_partial_surface() {
    let mut polls = 0_usize;
    let error = GeodesicVoronoiBuilder::build_cancellable(&space(20_000), || {
        polls += 1;
        polls >= 5
    })
    .unwrap_err();

    assert_eq!(error, SphericalSurfaceBuildError::Cancelled);
    assert_eq!(polls, 5);
}

#[test]
fn never_cancelled_geodesic_builder_preserves_exact_surface_bytes() {
    let specification = space(162);
    let legacy = GeodesicVoronoiBuilder::build(&specification).unwrap();
    let cancellable = GeodesicVoronoiBuilder::build_cancellable(&specification, || false).unwrap();

    assert_eq!(
        serde_json::to_vec(&legacy).unwrap(),
        serde_json::to_vec(&cancellable).unwrap()
    );
}

fn space(target_cell_count: u32) -> SphericalSpaceSpec {
    SphericalSpaceSpec {
        radius: Meters::new(RADIUS_M).unwrap(),
        target_cell_count,
    }
}
