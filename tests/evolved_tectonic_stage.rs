use std::sync::{Arc, Barrier, OnceLock};
use std::time::Duration;

use sekai::engine::{
    Artifact, BuildCancellation, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage,
};
use sekai::generators::natural::{
    evolved_tectonic_graph, spherical_natural_foundation_graph, EvolvedTectonicArtifact,
    EvolvedTectonicStage, NaturalQualityProfileArtifact, ResolvedTectonicInput,
    ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceArtifact};
use sekai::rules::TectonicModel;
use sekai::world::natural::{
    NaturalQualityProfile, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

fn draft_surface() -> &'static SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: NaturalQualityProfile::Draft.authoritative_target_cell_count(),
        })
        .unwrap()
    })
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn external(surface: &SphericalSurfaceSnapshot, spec: TectonicSpec) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(NaturalQualityProfileArtifact::new(
            NaturalQualityProfile::Draft,
        ))
        .unwrap();
    artifacts
        .insert(ResolvedTectonicInputArtifact::new(
            ResolvedTectonicInput::new(TectonicModel::CurrentSliceV1, spec).unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedWorldFormationArtifact::new(formation()))
        .unwrap();
    artifacts
        .insert(SphericalSurfaceArtifact::new(surface.clone()))
        .unwrap();
    artifacts
}

#[test]
fn stage_declares_the_locked_v5_identity_and_exact_dependency_boundary() {
    assert_eq!(
        NaturalQualityProfileArtifact::KEY.as_str(),
        "natural.quality-profile"
    );
    assert_eq!(
        EvolvedTectonicArtifact::KEY.as_str(),
        "world.evolved-tectonics"
    );
    assert_eq!(
        EvolvedTectonicStage.id().as_str(),
        "natural.evolved-tectonics"
    );
    assert_eq!(EvolvedTectonicStage.version(), 5);
    assert_eq!(EvolvedTectonicStage.namespace(), "sekai.core");

    let graph = evolved_tectonic_graph().unwrap();
    assert_eq!(graph.stage_ids(), vec!["natural.evolved-tectonics"]);
    let descriptor = &graph.descriptors()[0];
    assert_eq!(
        descriptor
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.quality-profile",
            "natural.resolved-tectonic-input",
            "natural.resolved-world-formation",
            "world.spherical-surface",
        ]
    );
    assert_eq!(descriptor.output(), EvolvedTectonicArtifact::KEY);

    let legacy = spherical_natural_foundation_graph().unwrap();
    assert!(legacy.stage_ids().contains(&"natural.spherical-tectonics"));
    assert!(!legacy.stage_ids().contains(&"natural.evolved-tectonics"));
}

#[test]
fn stage_reuses_the_supplied_authority_and_is_deterministic_and_cache_sensitive() {
    let engine = BuildEngine::new(evolved_tectonic_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(
            RootSeed::new(42),
            external(draft_surface(), TectonicSpec::default()),
            &mut cache,
        )
        .unwrap();
    let first_artifact = first.artifacts.get::<EvolvedTectonicArtifact>().unwrap();
    first_artifact
        .snapshot()
        .validate_against(draft_surface())
        .unwrap();
    assert_eq!(
        first_artifact.snapshot().surface_ref().fingerprint(),
        draft_surface().fingerprint()
    );
    assert_eq!(
        first_artifact.quality_report().surface_ref(),
        first_artifact.snapshot().surface_ref()
    );
    let encoded = serde_json::to_value(first_artifact.as_ref()).unwrap();
    let decoded: EvolvedTectonicArtifact = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(&decoded, first_artifact.as_ref());
    let mut unknown = encoded;
    unknown["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<EvolvedTectonicArtifact>(unknown).is_err());
    assert_eq!(first.report.cache_hits(), 0);

    let repeated = engine
        .build(
            RootSeed::new(42),
            external(draft_surface(), TectonicSpec::default()),
            &mut cache,
        )
        .unwrap();
    assert_eq!(repeated.report.cache_hits(), 1);
    assert_eq!(
        serde_json::to_vec(first_artifact.as_ref()).unwrap(),
        serde_json::to_vec(
            repeated
                .artifacts
                .get::<EvolvedTectonicArtifact>()
                .unwrap()
                .as_ref()
        )
        .unwrap()
    );

    let changed_spec = TectonicSpec {
        plate_count: TectonicSpec::default().plate_count + 1,
        ..TectonicSpec::default()
    };
    let changed = engine
        .build(
            RootSeed::new(42),
            external(draft_surface(), changed_spec),
            &mut cache,
        )
        .unwrap();
    assert_eq!(changed.report.cache_hits(), 0);
    assert_ne!(
        changed.artifacts.hash::<EvolvedTectonicArtifact>().unwrap(),
        first.artifacts.hash::<EvolvedTectonicArtifact>().unwrap()
    );
}

#[test]
fn profile_surface_mismatch_and_cancellation_publish_no_partial_artifact() {
    let mismatched = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 642,
    })
    .unwrap();
    let failure = BuildEngine::new(evolved_tectonic_graph().unwrap())
        .build(
            RootSeed::new(7),
            external(&mismatched, TectonicSpec::default()),
            &mut MemoryStageCache::new(),
        )
        .unwrap_err();
    assert!(failure.report.has_errors());
    assert!(failure
        .report
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code() == "evolved-tectonics.invalid-input"));

    let cancellation = BuildCancellation::new();
    let worker_cancellation = cancellation.clone();
    let barrier = Arc::new(Barrier::new(2));
    let worker_barrier = Arc::clone(&barrier);
    let worker = std::thread::spawn(move || {
        worker_barrier.wait();
        BuildEngine::new(evolved_tectonic_graph().unwrap()).build_with_cancellation(
            RootSeed::new(31),
            external(draft_surface(), TectonicSpec::default()),
            &mut MemoryStageCache::new(),
            &worker_cancellation,
        )
    });
    barrier.wait();
    std::thread::sleep(Duration::from_millis(2));
    cancellation.cancel();
    let failure = worker.join().unwrap().unwrap_err();
    assert!(failure
        .report
        .diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.code() == "engine.cancelled"));
}

#[test]
fn profile_and_evolved_artifact_wires_are_strict() {
    let profile = NaturalQualityProfileArtifact::new(NaturalQualityProfile::Draft);
    let value = serde_json::to_value(profile).unwrap();
    assert_eq!(
        serde_json::from_value::<NaturalQualityProfileArtifact>(value.clone())
            .unwrap()
            .profile(),
        NaturalQualityProfile::Draft
    );
    let mut unknown = value;
    unknown["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<NaturalQualityProfileArtifact>(unknown).is_err());
}
