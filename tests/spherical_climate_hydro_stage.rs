use sekai::engine::{
    derive_stage_seed, Artifact, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage,
    StageGraph, StageGraphBuilder, StageIdentity, StageRng,
};
use sekai::generators::natural::{
    ClimateGenerator, GeologicGenerator, HydroErosionGenerator, MantleGenerator, ReliefGenerator,
    ResolvedClimateInput, ResolvedClimateInputArtifact, ResolvedHydroErosionInput,
    ResolvedHydroErosionInputArtifact, SphericalGeologicArtifact, SphericalHydroErosionArtifact,
    SphericalHydroErosionStage, SphericalPreliminaryClimateArtifact,
    SphericalPreliminaryClimateStage, SphericalReliefArtifact, TectonicGenerator,
};
use sekai::generators::spatial::{GeodesicVoronoiBuilder, SphericalSurfaceArtifact};
use sekai::rules::{ClimateModel, HydroErosionModel};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, SphericalGeologicSnapshot, SphericalReliefSnapshot, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

struct Upstream {
    surface: sekai::world::spatial::SphericalSurfaceSnapshot,
    relief: SphericalReliefSnapshot,
    geology: SphericalGeologicSnapshot,
}

fn rng(root_seed: RootSeed, stage_id: &'static str) -> StageRng {
    let version = match stage_id {
        "natural.spherical-tectonics" => 3,
        "natural.spherical-relief" => 2,
        _ => 1,
    };
    StageRng::from_seed(derive_stage_seed(
        root_seed,
        StageIdentity::new(stage_id, version, "sekai.core"),
    ))
}

fn surface(radius_m: f64) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
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

fn upstream(root_seed: RootSeed, radius_m: f64) -> Upstream {
    let surface = surface(radius_m);
    let tectonic = TectonicGenerator::generate_spherical(
        &surface,
        &TectonicSpec::default(),
        &formation(),
        &mut rng(root_seed, "natural.spherical-tectonics"),
    )
    .unwrap();
    let mantle = MantleGenerator::generate_spherical(
        &surface,
        &GeologicSpec::default(),
        formation().mantle_bias(),
        &mut rng(root_seed, "natural.spherical-mantle"),
    )
    .unwrap();
    let relief = ReliefGenerator::generate_spherical(
        &surface,
        &tectonic,
        &mantle,
        &ReliefSpec::default(),
        &mut rng(root_seed, "natural.spherical-relief"),
        &mut Vec::new(),
    )
    .unwrap();
    let geology = GeologicGenerator::generate_spherical(
        &surface,
        &tectonic,
        &mantle,
        &relief,
        &GeologicSpec::default(),
        &mut rng(root_seed, "natural.spherical-geology"),
    )
    .unwrap();
    Upstream {
        surface,
        relief,
        geology,
    }
}

fn graph() -> StageGraph {
    StageGraphBuilder::new()
        .external::<SphericalSurfaceArtifact>()
        .external::<SphericalReliefArtifact>()
        .external::<SphericalGeologicArtifact>()
        .external::<ResolvedClimateInputArtifact>()
        .external::<ResolvedHydroErosionInputArtifact>()
        .stage(SphericalPreliminaryClimateStage)
        .stage(SphericalHydroErosionStage)
        .build()
        .unwrap()
}

fn external(upstream: &Upstream) -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(SphericalSurfaceArtifact::new(upstream.surface.clone()))
        .unwrap();
    artifacts
        .insert(SphericalReliefArtifact::new(upstream.relief.clone()))
        .unwrap();
    artifacts
        .insert(SphericalGeologicArtifact::new(upstream.geology.clone()))
        .unwrap();
    artifacts
        .insert(ResolvedClimateInputArtifact::new(
            ResolvedClimateInput::new(
                ClimateModel::SeasonalEnergyMoistureV1,
                ClimateSpec::default(),
            )
            .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedHydroErosionInputArtifact::new(
            ResolvedHydroErosionInput::new(
                HydroErosionModel::PriorityFloodStreamPowerV1,
                HydroErosionSpec::default(),
            )
            .unwrap(),
        ))
        .unwrap();
    artifacts
}

#[test]
fn stages_publish_exact_identities_and_dependencies() {
    assert_eq!(
        SphericalPreliminaryClimateArtifact::KEY.as_str(),
        "world.spherical-preliminary-climate"
    );
    assert_eq!(
        SphericalHydroErosionArtifact::KEY.as_str(),
        "world.spherical-hydro-erosion"
    );
    assert_eq!(
        SphericalPreliminaryClimateStage.id().as_str(),
        "natural.spherical-preliminary-climate"
    );
    assert_eq!(
        SphericalHydroErosionStage.id().as_str(),
        "natural.spherical-hydro-erosion"
    );
    assert_eq!(SphericalPreliminaryClimateStage.version(), 1);
    assert_eq!(SphericalHydroErosionStage.version(), 1);
    assert_eq!(SphericalPreliminaryClimateStage.namespace(), "sekai.core");
    assert_eq!(SphericalHydroErosionStage.namespace(), "sekai.core");

    let graph = graph();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.spherical-preliminary-climate",
            "natural.spherical-hydro-erosion",
        ]
    );
    let climate = &graph.descriptors()[0];
    assert_eq!(
        climate
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.resolved-climate-input",
            "world.spherical-relief",
            "world.spherical-surface",
        ]
    );
    assert_eq!(climate.output(), SphericalPreliminaryClimateArtifact::KEY);
    let hydro = &graph.descriptors()[1];
    assert_eq!(
        hydro
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.resolved-hydro-erosion-input",
            "world.spherical-geology",
            "world.spherical-preliminary-climate",
            "world.spherical-relief",
            "world.spherical-surface",
        ]
    );
    assert_eq!(hydro.output(), SphericalHydroErosionArtifact::KEY);
}

#[test]
fn stages_publish_atomic_strict_outputs_without_using_stage_rng() {
    let upstream = upstream(RootSeed::new(42), 6_371_000.0);
    let first = BuildEngine::new(graph())
        .build(
            RootSeed::new(1),
            external(&upstream),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let second = BuildEngine::new(graph())
        .build(
            RootSeed::new(999),
            external(&upstream),
            &mut MemoryStageCache::new(),
        )
        .unwrap();

    let expected_climate = ClimateGenerator::generate_spherical(
        &upstream.surface,
        &upstream.relief,
        &ClimateSpec::default(),
    )
    .unwrap();
    let expected_hydro = HydroErosionGenerator::generate_spherical(
        &upstream.surface,
        &upstream.relief,
        &upstream.geology,
        &expected_climate,
        &HydroErosionSpec::default(),
    )
    .unwrap();

    let first_climate = first
        .artifacts
        .get::<SphericalPreliminaryClimateArtifact>()
        .unwrap();
    let first_hydro = first
        .artifacts
        .get::<SphericalHydroErosionArtifact>()
        .unwrap();
    assert_eq!(first_climate.snapshot(), &expected_climate);
    assert_eq!(first_hydro.snapshot(), &expected_hydro);
    assert_eq!(
        first_climate.snapshot(),
        second
            .artifacts
            .get::<SphericalPreliminaryClimateArtifact>()
            .unwrap()
            .snapshot()
    );
    assert_eq!(
        first_hydro.snapshot(),
        second
            .artifacts
            .get::<SphericalHydroErosionArtifact>()
            .unwrap()
            .snapshot()
    );
    first_climate
        .snapshot()
        .validate_against(&upstream.surface, &upstream.relief)
        .unwrap();
    first_hydro
        .snapshot()
        .validate_against(
            &upstream.surface,
            &upstream.relief,
            &upstream.geology,
            first_climate.snapshot(),
        )
        .unwrap();

    let mut climate_json = serde_json::to_value(first_climate.as_ref()).unwrap();
    let climate_round_trip: SphericalPreliminaryClimateArtifact =
        serde_json::from_value(climate_json.clone()).unwrap();
    assert_eq!(&climate_round_trip, first_climate.as_ref());
    climate_json["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SphericalPreliminaryClimateArtifact>(climate_json).is_err());
    let mut hydro_json = serde_json::to_value(first_hydro.as_ref()).unwrap();
    let hydro_round_trip: SphericalHydroErosionArtifact =
        serde_json::from_value(hydro_json.clone()).unwrap();
    assert_eq!(&hydro_round_trip, first_hydro.as_ref());
    hydro_json["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SphericalHydroErosionArtifact>(hydro_json).is_err());
}

#[test]
fn downstream_stages_reject_equal_count_upstreams_from_another_surface() {
    let first = upstream(RootSeed::new(8), 6_371_000.0);
    let second = surface(7_000_000.0);
    assert_eq!(first.surface.cells().len(), second.cells().len());

    let climate_graph = StageGraphBuilder::new()
        .external::<SphericalSurfaceArtifact>()
        .external::<SphericalReliefArtifact>()
        .external::<ResolvedClimateInputArtifact>()
        .stage(SphericalPreliminaryClimateStage)
        .build()
        .unwrap();
    let mut climate_external = ExternalArtifacts::new();
    climate_external
        .insert(SphericalSurfaceArtifact::new(second.clone()))
        .unwrap();
    climate_external
        .insert(SphericalReliefArtifact::new(first.relief.clone()))
        .unwrap();
    climate_external
        .insert(ResolvedClimateInputArtifact::new(
            ResolvedClimateInput::new(
                ClimateModel::SeasonalEnergyMoistureV1,
                ClimateSpec::default(),
            )
            .unwrap(),
        ))
        .unwrap();
    let climate_failure = BuildEngine::new(climate_graph)
        .build(
            RootSeed::new(8),
            climate_external,
            &mut MemoryStageCache::new(),
        )
        .unwrap_err();
    assert_eq!(
        climate_failure.report.diagnostics().last().unwrap().code(),
        "spherical-natural.invalid-preliminary-climate-input"
    );

    let climate = ClimateGenerator::generate_spherical(
        &first.surface,
        &first.relief,
        &ClimateSpec::default(),
    )
    .unwrap();
    let hydro_graph = StageGraphBuilder::new()
        .external::<SphericalSurfaceArtifact>()
        .external::<SphericalReliefArtifact>()
        .external::<SphericalGeologicArtifact>()
        .external::<SphericalPreliminaryClimateArtifact>()
        .external::<ResolvedHydroErosionInputArtifact>()
        .stage(SphericalHydroErosionStage)
        .build()
        .unwrap();
    let mut hydro_external = ExternalArtifacts::new();
    hydro_external
        .insert(SphericalSurfaceArtifact::new(second))
        .unwrap();
    hydro_external
        .insert(SphericalReliefArtifact::new(first.relief))
        .unwrap();
    hydro_external
        .insert(SphericalGeologicArtifact::new(first.geology))
        .unwrap();
    hydro_external
        .insert(SphericalPreliminaryClimateArtifact::new(climate))
        .unwrap();
    hydro_external
        .insert(ResolvedHydroErosionInputArtifact::new(
            ResolvedHydroErosionInput::new(
                HydroErosionModel::PriorityFloodStreamPowerV1,
                HydroErosionSpec::default(),
            )
            .unwrap(),
        ))
        .unwrap();
    let hydro_failure = BuildEngine::new(hydro_graph)
        .build(
            RootSeed::new(8),
            hydro_external,
            &mut MemoryStageCache::new(),
        )
        .unwrap_err();
    assert_eq!(
        hydro_failure.report.diagnostics().last().unwrap().code(),
        "spherical-natural.invalid-hydro-erosion-input"
    );
}
