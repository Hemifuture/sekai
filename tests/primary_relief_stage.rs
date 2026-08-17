use std::sync::OnceLock;

use sekai::engine::{
    Artifact, BuildCancellation, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage,
};
use sekai::generators::natural::{
    primary_relief_graph, spherical_natural_foundation_graph, EvolvedTectonicArtifact,
    GeologicSubstrateArtifact, GeologicSubstrateStage, NaturalQualityProfileArtifact,
    PrimaryReliefArtifact, PrimaryReliefStage, ReliefSpecArtifact, ResolvedGeologicInput,
    ResolvedGeologicInputArtifact, ResolvedTectonicInput, ResolvedTectonicInputArtifact,
    ResolvedWorldFormationArtifact,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceArtifact};
use sekai::rules::{GeologicModel, TectonicModel};
use sekai::world::natural::{
    GeologicSpec, NaturalQualityProfile, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
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

fn external(relief_spec: ReliefSpec) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(NaturalQualityProfileArtifact::new(
            NaturalQualityProfile::Draft,
        ))
        .unwrap();
    artifacts
        .insert(ResolvedTectonicInputArtifact::new(
            ResolvedTectonicInput::new(TectonicModel::CurrentSliceV1, TectonicSpec::default())
                .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedWorldFormationArtifact::new(formation()))
        .unwrap();
    artifacts
        .insert(ResolvedGeologicInputArtifact::new(
            ResolvedGeologicInput::new(GeologicModel::CurrentSliceV1, GeologicSpec::default())
                .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ReliefSpecArtifact::new(relief_spec))
        .unwrap();
    artifacts
        .insert(SphericalSurfaceArtifact::new(draft_surface().clone()))
        .unwrap();
    artifacts
}

#[test]
fn stages_publish_locked_keys_identities_and_exact_dependency_boundaries() {
    assert_eq!(
        GeologicSubstrateArtifact::KEY.as_str(),
        "world.geologic-substrate"
    );
    assert_eq!(PrimaryReliefArtifact::KEY.as_str(), "world.primary-relief");
    assert_eq!(
        GeologicSubstrateStage.id().as_str(),
        "natural.geologic-substrate"
    );
    assert_eq!(GeologicSubstrateStage.version(), 1);
    assert_eq!(GeologicSubstrateStage.namespace(), "sekai.core");
    assert_eq!(PrimaryReliefStage.id().as_str(), "natural.primary-relief");
    assert_eq!(PrimaryReliefStage.version(), 1);
    assert_eq!(PrimaryReliefStage.namespace(), "sekai.core");

    let graph = primary_relief_graph().unwrap();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.evolved-tectonics",
            "natural.geologic-substrate",
            "natural.primary-relief",
        ]
    );
    assert_eq!(
        graph.descriptors()[0].output(),
        EvolvedTectonicArtifact::KEY
    );
    assert_eq!(
        graph.descriptors()[1].output(),
        GeologicSubstrateArtifact::KEY
    );
    assert_eq!(graph.descriptors()[2].output(), PrimaryReliefArtifact::KEY);
    assert_eq!(
        graph.descriptors()[1]
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.resolved-geologic-input",
            "natural.resolved-world-formation",
            "world.evolved-tectonics",
            "world.spherical-surface",
        ]
    );
    assert_eq!(
        graph.descriptors()[2]
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.relief-spec",
            "world.evolved-tectonics",
            "world.geologic-substrate",
            "world.spherical-surface",
        ]
    );

    let legacy = spherical_natural_foundation_graph().unwrap();
    assert!(!legacy.stage_ids().contains(&"natural.evolved-tectonics"));
    assert!(!legacy.stage_ids().contains(&"natural.geologic-substrate"));
    assert!(!legacy.stage_ids().contains(&"natural.primary-relief"));
}

#[test]
fn graph_builds_strict_artifacts_and_restores_all_three_stages_from_cache() {
    let engine = BuildEngine::new(primary_relief_graph().unwrap());
    let mut cache = MemoryStageCache::new();
    let first = engine
        .build(
            RootSeed::new(42),
            external(ReliefSpec::default()),
            &mut cache,
        )
        .unwrap();
    assert_eq!(first.report.cache_hits(), 0);
    let evolved = first.artifacts.get::<EvolvedTectonicArtifact>().unwrap();
    let substrate = first.artifacts.get::<GeologicSubstrateArtifact>().unwrap();
    let relief = first.artifacts.get::<PrimaryReliefArtifact>().unwrap();
    substrate
        .snapshot()
        .validate_against(draft_surface(), evolved.snapshot())
        .unwrap();
    relief
        .snapshot()
        .validate_against(
            draft_surface(),
            substrate.snapshot(),
            &ReliefSpec::default(),
        )
        .unwrap();
    relief.validate().unwrap();

    for value in [
        serde_json::to_value(substrate.as_ref()).unwrap(),
        serde_json::to_value(relief.as_ref()).unwrap(),
    ] {
        assert!(value.is_object());
    }
    let decoded_substrate: GeologicSubstrateArtifact =
        serde_json::from_value(serde_json::to_value(substrate.as_ref()).unwrap()).unwrap();
    let decoded_relief: PrimaryReliefArtifact =
        serde_json::from_value(serde_json::to_value(relief.as_ref()).unwrap()).unwrap();
    assert_eq!(&decoded_substrate, substrate.as_ref());
    assert_eq!(&decoded_relief, relief.as_ref());

    let repeated = engine
        .build(
            RootSeed::new(42),
            external(ReliefSpec::default()),
            &mut cache,
        )
        .unwrap();
    assert_eq!(repeated.report.cache_hits(), 3);
    assert_eq!(
        repeated.artifacts.hash::<PrimaryReliefArtifact>().unwrap(),
        first.artifacts.hash::<PrimaryReliefArtifact>().unwrap()
    );

    let changed_spec = ReliefSpec {
        target_land_fraction: 0.45,
        ..ReliefSpec::default()
    };
    let changed = engine
        .build(RootSeed::new(42), external(changed_spec), &mut cache)
        .unwrap();
    assert_eq!(changed.report.cache_hits(), 2);
    assert_ne!(
        changed.artifacts.hash::<PrimaryReliefArtifact>().unwrap(),
        first.artifacts.hash::<PrimaryReliefArtifact>().unwrap()
    );
    assert_eq!(
        changed
            .artifacts
            .hash::<GeologicSubstrateArtifact>()
            .unwrap(),
        first.artifacts.hash::<GeologicSubstrateArtifact>().unwrap()
    );
}

#[test]
fn artifact_wires_are_strict_and_cancelled_graph_publishes_no_result() {
    let engine = BuildEngine::new(primary_relief_graph().unwrap());
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    let failure = engine
        .build_with_cancellation(
            RootSeed::new(83),
            external(ReliefSpec::default()),
            &mut MemoryStageCache::new(),
            &cancellation,
        )
        .unwrap_err();
    assert!(failure.report.has_errors());
    assert!(failure
        .report
        .diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.code() == "engine.cancelled"));

    let successful = BuildEngine::new(primary_relief_graph().unwrap())
        .build(
            RootSeed::new(43),
            external(ReliefSpec::default()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let substrate = successful
        .artifacts
        .get::<GeologicSubstrateArtifact>()
        .unwrap();
    let relief = successful.artifacts.get::<PrimaryReliefArtifact>().unwrap();
    let mut substrate_wire = serde_json::to_value(substrate.as_ref()).unwrap();
    substrate_wire["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<GeologicSubstrateArtifact>(substrate_wire).is_err());
    let mut relief_wire = serde_json::to_value(relief.as_ref()).unwrap();
    relief_wire["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PrimaryReliefArtifact>(relief_wire).is_err());
}
