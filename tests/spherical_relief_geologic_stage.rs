use sekai::engine::{
    derive_stage_seed, Artifact, BuildEngine, Diagnostic, ExternalArtifacts, MemoryStageCache,
    Stage, StageGraph, StageGraphBuilder, StageIdentity, StageRng,
};
use sekai::generators::natural::{
    GeologicGenerator, MantleGenerator, ReliefGenerator, ReliefSpecArtifact, ResolvedGeologicInput,
    ResolvedGeologicInputArtifact, ResolvedTectonicInput, ResolvedTectonicInputArtifact,
    ResolvedWorldFormationArtifact, SphericalGeologicArtifact, SphericalGeologicStage,
    SphericalMantleArtifact, SphericalMantleStage, SphericalReliefArtifact, SphericalReliefStage,
    SphericalTectonicArtifact, SphericalTectonicStage, TectonicGenerator,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceArtifact};
use sekai::rules::{GeologicModel, TectonicModel};
use sekai::world::natural::{
    GeologicSpec, MantleActivity, ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    TectonicActivity, TectonicSpec, WorldFormationPreset, MAX_HOTSPOT_COUNT,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

fn surface(radius_m: f64) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count: 162,
    })
    .unwrap()
}

fn tectonic_spec() -> TectonicSpec {
    TectonicSpec {
        continental_crust_fraction: 0.16,
        activity: TectonicActivity::Active,
        ..TectonicSpec::default()
    }
}

fn geologic_spec() -> GeologicSpec {
    GeologicSpec {
        hotspot_count: MAX_HOTSPOT_COUNT,
        mantle_activity: MantleActivity::Active,
        ..GeologicSpec::default()
    }
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::VolcanicIslands,
        ResolvedWorldFormationPreset::VolcanicIslands,
    )
    .unwrap()
}

fn rng(root_seed: RootSeed, stage_id: &'static str) -> StageRng {
    let version = match stage_id {
        "natural.spherical-tectonics" => 9,
        "natural.spherical-relief" => 3,
        "natural.spherical-mantle" | "natural.spherical-geology" => 1,
        _ => panic!("unexpected spherical stage {stage_id}"),
    };
    StageRng::from_seed(derive_stage_seed(
        root_seed,
        StageIdentity::new(stage_id, version, "sekai.core"),
    ))
}

fn graph() -> StageGraph {
    StageGraphBuilder::new()
        .external::<SphericalSurfaceArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<ReliefSpecArtifact>()
        .stage(SphericalTectonicStage)
        .stage(SphericalMantleStage)
        .stage(SphericalReliefStage)
        .stage(SphericalGeologicStage)
        .build()
        .unwrap()
}

fn external(surface: &sekai::world::spatial::SphericalSurfaceSnapshot) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(SphericalSurfaceArtifact::new(surface.clone()))
        .unwrap();
    artifacts
        .insert(ResolvedTectonicInputArtifact::new(
            ResolvedTectonicInput::new(TectonicModel::CurrentSliceV1, tectonic_spec()).unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedGeologicInputArtifact::new(
            ResolvedGeologicInput::new(GeologicModel::CurrentSliceV1, geologic_spec()).unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedWorldFormationArtifact::new(formation()))
        .unwrap();
    artifacts
        .insert(ReliefSpecArtifact::new(ReliefSpec::default()))
        .unwrap();
    artifacts
}

#[test]
fn stages_publish_exact_identities_and_dependencies() {
    assert_eq!(
        SphericalReliefArtifact::KEY.as_str(),
        "world.spherical-relief"
    );
    assert_eq!(
        SphericalGeologicArtifact::KEY.as_str(),
        "world.spherical-geology"
    );
    assert_eq!(
        SphericalReliefStage.id().as_str(),
        "natural.spherical-relief"
    );
    assert_eq!(
        SphericalGeologicStage.id().as_str(),
        "natural.spherical-geology"
    );
    assert_eq!(SphericalReliefStage.version(), 3);
    assert_eq!(SphericalGeologicStage.version(), 1);
    assert_eq!(SphericalReliefStage.namespace(), "sekai.core");
    assert_eq!(SphericalGeologicStage.namespace(), "sekai.core");

    let graph = graph();
    let relief = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.spherical-relief")
        .unwrap();
    assert_eq!(
        relief
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.relief-spec",
            "world.spherical-mantle",
            "world.spherical-surface",
            "world.spherical-tectonics",
        ]
    );
    assert_eq!(relief.output(), SphericalReliefArtifact::KEY);
    let geology = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.spherical-geology")
        .unwrap();
    assert_eq!(
        geology
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.resolved-geologic-input",
            "world.spherical-mantle",
            "world.spherical-relief",
            "world.spherical-surface",
            "world.spherical-tectonics",
        ]
    );
    assert_eq!(geology.output(), SphericalGeologicArtifact::KEY);
}

#[test]
fn stages_forward_science_diagnostics_and_strict_surface_bound_wires() {
    // This fixed seed intentionally exercises bounded-elevation reconciliation so the stage
    // contract proves that non-empty scientific diagnostics are forwarded unchanged.
    let root_seed = RootSeed::new(0);
    let surface = surface(6_371_000.0);
    let outcome = BuildEngine::new(graph())
        .build(root_seed, external(&surface), &mut MemoryStageCache::new())
        .unwrap();

    let tectonic = TectonicGenerator::generate_spherical(
        &surface,
        &tectonic_spec(),
        &formation(),
        &mut rng(root_seed, "natural.spherical-tectonics"),
    )
    .unwrap();
    let mantle = MantleGenerator::generate_spherical(
        &surface,
        &geologic_spec(),
        formation().mantle_bias(),
        &mut rng(root_seed, "natural.spherical-mantle"),
    )
    .unwrap();
    let mut expected_diagnostics = Vec::<Diagnostic>::new();
    let relief = ReliefGenerator::generate_spherical(
        &surface,
        &tectonic,
        &mantle,
        &ReliefSpec::default(),
        &mut rng(root_seed, "natural.spherical-relief"),
        &mut expected_diagnostics,
    )
    .unwrap();
    let geology = GeologicGenerator::generate_spherical(
        &surface,
        &tectonic,
        &mantle,
        &relief,
        &geologic_spec(),
        &mut rng(root_seed, "natural.spherical-geology"),
    )
    .unwrap();

    let staged_relief = outcome.artifacts.get::<SphericalReliefArtifact>().unwrap();
    let staged_geology = outcome
        .artifacts
        .get::<SphericalGeologicArtifact>()
        .unwrap();
    assert_eq!(staged_relief.snapshot(), &relief);
    assert_eq!(staged_geology.snapshot(), &geology);
    assert!(!expected_diagnostics.is_empty());
    assert_eq!(outcome.report.diagnostics(), expected_diagnostics);
    staged_relief
        .snapshot()
        .validate_against(
            &surface,
            outcome
                .artifacts
                .get::<SphericalTectonicArtifact>()
                .unwrap()
                .snapshot(),
            outcome
                .artifacts
                .get::<SphericalMantleArtifact>()
                .unwrap()
                .snapshot(),
        )
        .unwrap();
    staged_geology
        .snapshot()
        .validate_against(
            &surface,
            outcome
                .artifacts
                .get::<SphericalTectonicArtifact>()
                .unwrap()
                .snapshot(),
            outcome
                .artifacts
                .get::<SphericalMantleArtifact>()
                .unwrap()
                .snapshot(),
            staged_relief.snapshot(),
        )
        .unwrap();

    for mut value in [
        serde_json::to_value(staged_relief.as_ref()).unwrap(),
        serde_json::to_value(staged_geology.as_ref()).unwrap(),
    ] {
        value["unknown"] = serde_json::json!(true);
        assert!(serde_json::from_value::<SphericalReliefArtifact>(value.clone()).is_err());
        assert!(serde_json::from_value::<SphericalGeologicArtifact>(value).is_err());
    }
    let relief_round_trip: SphericalReliefArtifact =
        serde_json::from_value(serde_json::to_value(staged_relief.as_ref()).unwrap()).unwrap();
    let geology_round_trip: SphericalGeologicArtifact =
        serde_json::from_value(serde_json::to_value(staged_geology.as_ref()).unwrap()).unwrap();
    assert_eq!(&relief_round_trip, staged_relief.as_ref());
    assert_eq!(&geology_round_trip, staged_geology.as_ref());
}

#[test]
fn relief_rejects_equal_count_upstreams_from_another_surface() {
    let root_seed = RootSeed::new(23);
    let first = surface(6_371_000.0);
    let second = surface(7_000_000.0);
    assert_eq!(first.cells().len(), second.cells().len());
    let tectonic = TectonicGenerator::generate_spherical(
        &first,
        &tectonic_spec(),
        &formation(),
        &mut rng(root_seed, "natural.spherical-tectonics"),
    )
    .unwrap();
    let mantle = MantleGenerator::generate_spherical(
        &first,
        &geologic_spec(),
        formation().mantle_bias(),
        &mut rng(root_seed, "natural.spherical-mantle"),
    )
    .unwrap();
    let graph = StageGraphBuilder::new()
        .external::<SphericalSurfaceArtifact>()
        .external::<SphericalTectonicArtifact>()
        .external::<SphericalMantleArtifact>()
        .external::<ReliefSpecArtifact>()
        .stage(SphericalReliefStage)
        .build()
        .unwrap();
    let mut external = ExternalArtifacts::new();
    external
        .insert(SphericalSurfaceArtifact::new(second))
        .unwrap();
    external
        .insert(SphericalTectonicArtifact::new(tectonic))
        .unwrap();
    external
        .insert(SphericalMantleArtifact::new(mantle))
        .unwrap();
    external
        .insert(ReliefSpecArtifact::new(ReliefSpec::default()))
        .unwrap();

    let failure = BuildEngine::new(graph)
        .build(root_seed, external, &mut MemoryStageCache::new())
        .unwrap_err();
    assert_eq!(
        failure.report.diagnostics().last().unwrap().code(),
        "spherical-natural.invalid-relief-input"
    );
}
