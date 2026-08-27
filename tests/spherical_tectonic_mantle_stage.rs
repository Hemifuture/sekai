use sekai::engine::{
    derive_stage_seed, Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage,
    StageGraph, StageGraphBuilder, StageIdentity, StageRng,
};
use sekai::generators::natural::{
    MantleGenerator, ResolvedGeologicInput, ResolvedGeologicInputArtifact, ResolvedTectonicInput,
    ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact, SphericalMantleArtifact,
    SphericalMantleStage, SphericalTectonicArtifact, SphericalTectonicStage, TectonicGenerator,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceArtifact};
use sekai::rules::{GeologicModel, TectonicModel};
use sekai::world::natural::{
    GeologicSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

fn surface() -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 162,
    })
    .unwrap()
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn graph() -> StageGraph {
    StageGraphBuilder::new()
        .external::<SphericalSurfaceArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .stage(SphericalTectonicStage)
        .stage(SphericalMantleStage)
        .build()
        .unwrap()
}

fn external(
    surface: &sekai::world::spatial::SphericalSurfaceSnapshot,
    tectonic_spec: &TectonicSpec,
    geologic_spec: &GeologicSpec,
    formation: &ResolvedWorldFormation,
) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(SphericalSurfaceArtifact::new(surface.clone()))
        .unwrap();
    artifacts
        .insert(ResolvedTectonicInputArtifact::new(
            ResolvedTectonicInput::new(TectonicModel::CurrentSliceV1, tectonic_spec.clone())
                .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedGeologicInputArtifact::new(
            ResolvedGeologicInput::new(GeologicModel::CurrentSliceV1, geologic_spec.clone())
                .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedWorldFormationArtifact::new(formation.clone()))
        .unwrap();
    artifacts
}

fn rng(root_seed: RootSeed, stage_id: &'static str) -> StageRng {
    let version = match stage_id {
        "natural.spherical-tectonics" => 8,
        "natural.spherical-mantle" => 1,
        _ => panic!("unexpected spherical stage {stage_id}"),
    };
    StageRng::from_seed(derive_stage_seed(
        root_seed,
        StageIdentity::new(stage_id, version, "sekai.core"),
    ))
}

#[test]
fn stages_publish_exact_identities_and_surface_bound_dependencies() {
    assert_eq!(
        SphericalTectonicArtifact::KEY.as_str(),
        "world.spherical-tectonics"
    );
    assert_eq!(
        SphericalMantleArtifact::KEY.as_str(),
        "world.spherical-mantle"
    );
    assert_eq!(
        SphericalTectonicStage.id().as_str(),
        "natural.spherical-tectonics"
    );
    assert_eq!(
        SphericalMantleStage.id().as_str(),
        "natural.spherical-mantle"
    );
    assert_eq!(SphericalTectonicStage.version(), 8);
    assert_eq!(SphericalMantleStage.version(), 1);
    assert_eq!(SphericalTectonicStage.namespace(), "sekai.core");
    assert_eq!(SphericalMantleStage.namespace(), "sekai.core");

    let graph = graph();
    assert_eq!(
        graph.stage_ids(),
        vec!["natural.spherical-mantle", "natural.spherical-tectonics"]
    );
    let mantle = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.spherical-mantle")
        .unwrap();
    assert_eq!(
        mantle
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.resolved-geologic-input",
            "natural.resolved-world-formation",
            "world.spherical-surface",
        ]
    );
    assert_eq!(mantle.output(), SphericalMantleArtifact::KEY);
    let tectonic = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.spherical-tectonics")
        .unwrap();
    assert_eq!(
        tectonic
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.resolved-tectonic-input",
            "natural.resolved-world-formation",
            "world.spherical-surface",
        ]
    );
    assert_eq!(tectonic.output(), SphericalTectonicArtifact::KEY);
}

#[test]
fn stages_forward_the_frozen_scientific_streams_and_use_strict_wires() {
    let root_seed = RootSeed::new(42);
    let surface = surface();
    let tectonic_spec = TectonicSpec::default();
    let geologic_spec = GeologicSpec::default();
    let formation = formation();
    let outcome = BuildEngine::new(graph())
        .build(
            root_seed,
            external(&surface, &tectonic_spec, &geologic_spec, &formation),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    let expected_tectonic = TectonicGenerator::generate_spherical(
        &surface,
        &tectonic_spec,
        &formation,
        &mut rng(root_seed, "natural.spherical-tectonics"),
    )
    .unwrap();
    let expected_mantle = MantleGenerator::generate_spherical(
        &surface,
        &geologic_spec,
        formation.mantle_bias(),
        &mut rng(root_seed, "natural.spherical-mantle"),
    )
    .unwrap();

    let tectonic = outcome
        .artifacts
        .get::<SphericalTectonicArtifact>()
        .unwrap();
    let mantle = outcome.artifacts.get::<SphericalMantleArtifact>().unwrap();
    assert_eq!(tectonic.snapshot(), &expected_tectonic);
    assert_eq!(mantle.snapshot(), &expected_mantle);
    tectonic.snapshot().validate_against(&surface).unwrap();
    mantle.snapshot().validate_against(&surface).unwrap();

    let tectonic_json = serde_json::to_value(tectonic.as_ref()).unwrap();
    let decoded_tectonic: SphericalTectonicArtifact =
        serde_json::from_value(tectonic_json.clone()).unwrap();
    assert_eq!(
        decoded_tectonic.snapshot().plates(),
        tectonic.snapshot().plates()
    );
    assert_eq!(
        decoded_tectonic.snapshot().cell_plates(),
        tectonic.snapshot().cell_plates()
    );
    assert_eq!(
        decoded_tectonic.snapshot().crust_state(),
        tectonic.snapshot().crust_state()
    );
    assert_eq!(
        decoded_tectonic.snapshot().boundaries(),
        tectonic.snapshot().boundaries()
    );
    assert_eq!(
        decoded_tectonic.snapshot().boundary_segments(),
        tectonic.snapshot().boundary_segments()
    );
    assert_eq!(&decoded_tectonic, tectonic.as_ref());
    let mut unknown_tectonic = tectonic_json;
    unknown_tectonic["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SphericalTectonicArtifact>(unknown_tectonic).is_err());

    let mantle_json = serde_json::to_value(mantle.as_ref()).unwrap();
    let decoded_mantle: SphericalMantleArtifact =
        serde_json::from_value(mantle_json.clone()).unwrap();
    assert_eq!(&decoded_mantle, mantle.as_ref());
    let mut unknown_mantle = mantle_json;
    unknown_mantle["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SphericalMantleArtifact>(unknown_mantle).is_err());
}
