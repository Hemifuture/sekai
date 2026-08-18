use std::sync::OnceLock;

use sekai::engine::{
    Artifact, BuildCancellation, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage,
};
use sekai::generators::natural::{
    global_circulation_graph, surface_formation_graph, ClimateWorkDomainArtifact,
    EvolvedTectonicArtifact, GeologicSubstrateArtifact, GlobalCirculationArtifact,
    NaturalQualityProfileArtifact, NaturalSurfaceFormationArtifact, PrimaryReliefArtifact,
    ReliefSpecArtifact, ResolvedClimateInput, ResolvedClimateInputArtifact, ResolvedGeologicInput,
    ResolvedGeologicInputArtifact, ResolvedHydroErosionInput, ResolvedHydroErosionInputArtifact,
    ResolvedTectonicInput, ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
    SurfaceFormationStage,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, SphericalSurfaceArtifact};
use sekai::rules::{ClimateModel, GeologicModel, HydroErosionModel, TectonicModel};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, NaturalQualityProfile, ReliefSpec,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

fn surface() -> &'static sekai::world::spatial::SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<sekai::world::spatial::SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap()
        .authoritative_surface()
        .clone()
    })
}

fn p5_external(climate_spec: ClimateSpec, formation_spec: HydroErosionSpec) -> ExternalArtifacts {
    let mut artifacts = p4_external(climate_spec);
    artifacts
        .insert(ResolvedHydroErosionInputArtifact::new(
            ResolvedHydroErosionInput::new(
                HydroErosionModel::PriorityFloodStreamPowerV1,
                formation_spec,
            )
            .unwrap(),
        ))
        .unwrap();
    artifacts
}

fn p4_external(climate_spec: ClimateSpec) -> ExternalArtifacts {
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
        .insert(ResolvedWorldFormationArtifact::new(
            ResolvedWorldFormation::new(
                RESOLVED_WORLD_FORMATION_SCHEMA_V1,
                WorldFormationPreset::Continents,
                ResolvedWorldFormationPreset::Continents,
            )
            .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ResolvedGeologicInputArtifact::new(
            ResolvedGeologicInput::new(GeologicModel::CurrentSliceV1, GeologicSpec::default())
                .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(ReliefSpecArtifact::new(ReliefSpec::default()))
        .unwrap();
    artifacts
        .insert(ResolvedClimateInputArtifact::new(
            ResolvedClimateInput::new(ClimateModel::SeasonalEnergyMoistureV1, climate_spec)
                .unwrap(),
        ))
        .unwrap();
    artifacts
        .insert(SphericalSurfaceArtifact::new(surface().clone()))
        .unwrap();
    artifacts
}

#[test]
fn the_p5_stage_publishes_a_locked_key_identity_and_exact_dependency_boundary() {
    assert_eq!(
        NaturalSurfaceFormationArtifact::KEY.as_str(),
        "world.natural-surface-formation"
    );
    assert_eq!(
        SurfaceFormationStage.id().as_str(),
        "natural.surface-formation"
    );
    assert_eq!(SurfaceFormationStage.version(), 1);
    assert_eq!(SurfaceFormationStage.namespace(), "sekai.core");

    let graph = surface_formation_graph().unwrap();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.climate-work-domain",
            "natural.evolved-tectonics",
            "natural.geologic-substrate",
            "natural.primary-relief",
            "natural.global-circulation",
            "natural.surface-formation",
        ]
    );
    let formation = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.surface-formation")
        .unwrap();
    assert_eq!(
        formation
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.quality-profile",
            "natural.resolved-climate-input",
            "natural.resolved-hydro-erosion-input",
            "world.climate-work-domain",
            "world.evolved-tectonics",
            "world.geologic-substrate",
            "world.global-circulation",
            "world.primary-relief",
            "world.spherical-surface",
        ]
    );

    // The frozen P0-P4 graph keeps its exact stage set.
    assert_eq!(
        global_circulation_graph().unwrap().stage_ids(),
        vec![
            "natural.climate-work-domain",
            "natural.evolved-tectonics",
            "natural.geologic-substrate",
            "natural.primary-relief",
            "natural.global-circulation",
        ]
    );
}

#[test]
fn the_p5_graph_reuses_p4_hashes_and_republishes_only_on_formation_input_changes() {
    let mut cache = MemoryStageCache::new();
    let p4 = BuildEngine::new(global_circulation_graph().unwrap())
        .build(
            RootSeed::new(42),
            p4_external(ClimateSpec::default()),
            &mut cache,
        )
        .unwrap();
    let engine = BuildEngine::new(surface_formation_graph().unwrap());
    let first = engine
        .build(
            RootSeed::new(42),
            p5_external(ClimateSpec::default(), HydroErosionSpec::default()),
            &mut cache,
        )
        .unwrap();
    assert_eq!(first.report.cache_hits(), 5);
    for unchanged in [
        (
            p4.artifacts
                .hash::<EvolvedTectonicArtifact>()
                .unwrap()
                .as_bytes(),
            first
                .artifacts
                .hash::<EvolvedTectonicArtifact>()
                .unwrap()
                .as_bytes(),
        ),
        (
            p4.artifacts
                .hash::<GeologicSubstrateArtifact>()
                .unwrap()
                .as_bytes(),
            first
                .artifacts
                .hash::<GeologicSubstrateArtifact>()
                .unwrap()
                .as_bytes(),
        ),
        (
            p4.artifacts
                .hash::<PrimaryReliefArtifact>()
                .unwrap()
                .as_bytes(),
            first
                .artifacts
                .hash::<PrimaryReliefArtifact>()
                .unwrap()
                .as_bytes(),
        ),
        (
            p4.artifacts
                .hash::<ClimateWorkDomainArtifact>()
                .unwrap()
                .as_bytes(),
            first
                .artifacts
                .hash::<ClimateWorkDomainArtifact>()
                .unwrap()
                .as_bytes(),
        ),
        (
            p4.artifacts
                .hash::<GlobalCirculationArtifact>()
                .unwrap()
                .as_bytes(),
            first
                .artifacts
                .hash::<GlobalCirculationArtifact>()
                .unwrap()
                .as_bytes(),
        ),
    ] {
        assert_eq!(unchanged.0, unchanged.1);
    }

    let formation = first
        .artifacts
        .get::<NaturalSurfaceFormationArtifact>()
        .unwrap();
    formation.validate().unwrap();
    formation.snapshot().validate_against(surface()).unwrap();
    assert!(formation.snapshot().solve_report().converged());

    let repeated = engine
        .build(
            RootSeed::new(42),
            p5_external(ClimateSpec::default(), HydroErosionSpec::default()),
            &mut cache,
        )
        .unwrap();
    assert_eq!(repeated.report.cache_hits(), 6);
    assert_eq!(
        repeated
            .artifacts
            .hash::<NaturalSurfaceFormationArtifact>()
            .unwrap()
            .as_bytes(),
        first
            .artifacts
            .hash::<NaturalSurfaceFormationArtifact>()
            .unwrap()
            .as_bytes()
    );

    let changed_spec = HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: HydroErosionSpec::default()
            .river_discharge_threshold_deci_m3_s
            / 2,
        ..HydroErosionSpec::default()
    };
    let changed = engine
        .build(
            RootSeed::new(42),
            p5_external(ClimateSpec::default(), changed_spec),
            &mut cache,
        )
        .unwrap();
    assert_eq!(changed.report.cache_hits(), 5);
    assert_ne!(
        changed
            .artifacts
            .hash::<NaturalSurfaceFormationArtifact>()
            .unwrap()
            .as_bytes(),
        first
            .artifacts
            .hash::<NaturalSurfaceFormationArtifact>()
            .unwrap()
            .as_bytes()
    );
    assert_eq!(
        changed
            .artifacts
            .hash::<GlobalCirculationArtifact>()
            .unwrap()
            .as_bytes(),
        first
            .artifacts
            .hash::<GlobalCirculationArtifact>()
            .unwrap()
            .as_bytes(),
        "changing only the formation spec must not disturb the P4 product"
    );
}
