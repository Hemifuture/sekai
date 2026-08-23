use std::sync::OnceLock;

use sekai::engine::{
    Artifact, BuildCancellation, BuildEngine, ExternalArtifacts, MemoryStageCache, Stage,
};
use sekai::generators::natural::{
    global_circulation_graph, primary_relief_graph, ClimateWorkDomainArtifact,
    ClimateWorkDomainStage, EvolvedTectonicArtifact, GeologicSubstrateArtifact,
    GlobalCirculationArtifact, GlobalCirculationStage, NaturalQualityProfileArtifact,
    PrimaryReliefArtifact, ReliefSpecArtifact, ResolvedClimateInput, ResolvedClimateInputArtifact,
    ResolvedGeologicInput, ResolvedGeologicInputArtifact, ResolvedTectonicInput,
    ResolvedTectonicInputArtifact, ResolvedWorldFormationArtifact,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, SphericalSurfaceArtifact};
use sekai::rules::{ClimateModel, GeologicModel, TectonicModel};
use sekai::world::natural::{
    ClimateModelProfile, ClimateSpec, GeologicSpec, NaturalQualityProfile, ReliefSpec,
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

fn base_external(relief_spec: ReliefSpec) -> ExternalArtifacts {
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
        .insert(ReliefSpecArtifact::new(relief_spec))
        .unwrap();
    artifacts
        .insert(SphericalSurfaceArtifact::new(surface().clone()))
        .unwrap();
    artifacts
}

fn p4_external(relief_spec: ReliefSpec, climate_spec: ClimateSpec) -> ExternalArtifacts {
    let mut artifacts = base_external(relief_spec);
    artifacts
        .insert(ResolvedClimateInputArtifact::new(
            ResolvedClimateInput::new(ClimateModel::SeasonalEnergyMoistureV1, climate_spec)
                .unwrap(),
        ))
        .unwrap();
    artifacts
}

#[test]
fn p4_stages_publish_locked_keys_identities_and_exact_dependency_boundaries() {
    assert_eq!(
        ClimateWorkDomainArtifact::KEY.as_str(),
        "world.climate-work-domain"
    );
    assert_eq!(
        GlobalCirculationArtifact::KEY.as_str(),
        "world.global-circulation"
    );
    assert_eq!(
        ClimateWorkDomainStage.id().as_str(),
        "natural.climate-work-domain"
    );
    assert_eq!(ClimateWorkDomainStage.version(), 1);
    assert_eq!(ClimateWorkDomainStage.namespace(), "sekai.core");
    assert_eq!(
        GlobalCirculationStage.id().as_str(),
        "natural.global-circulation"
    );
    assert_eq!(GlobalCirculationStage.version(), 4);
    assert_eq!(GlobalCirculationStage.namespace(), "sekai.core");

    let graph = global_circulation_graph().unwrap();
    assert_eq!(
        graph.stage_ids(),
        vec![
            "natural.climate-work-domain",
            "natural.evolved-tectonics",
            "natural.geologic-substrate",
            "natural.primary-relief",
            "natural.global-circulation",
        ]
    );
    let domain = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.climate-work-domain")
        .unwrap();
    assert_eq!(
        domain
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec!["natural.quality-profile", "world.spherical-surface"]
    );
    let circulation = graph
        .descriptors()
        .iter()
        .find(|descriptor| descriptor.id().as_str() == "natural.global-circulation")
        .unwrap();
    assert_eq!(
        circulation
            .dependencies()
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        vec![
            "natural.resolved-climate-input",
            "world.climate-work-domain",
            "world.primary-relief",
            "world.spherical-surface",
        ]
    );
    assert_eq!(
        primary_relief_graph().unwrap().stage_ids(),
        vec![
            "natural.evolved-tectonics",
            "natural.geologic-substrate",
            "natural.primary-relief",
        ]
    );
}

#[test]
fn p4_graph_restores_p3_hashes_and_selectively_invalidates_only_climate() {
    let mut cache = MemoryStageCache::new();
    let p3 = BuildEngine::new(primary_relief_graph().unwrap())
        .build(
            RootSeed::new(42),
            base_external(ReliefSpec::default()),
            &mut cache,
        )
        .unwrap();
    let p4_engine = BuildEngine::new(global_circulation_graph().unwrap());
    let first = p4_engine
        .build(
            RootSeed::new(42),
            p4_external(ReliefSpec::default(), ClimateSpec::default()),
            &mut cache,
        )
        .unwrap();
    assert_eq!(first.report.cache_hits(), 3);
    for unchanged in [
        (
            p3.artifacts
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
            p3.artifacts
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
            p3.artifacts
                .hash::<PrimaryReliefArtifact>()
                .unwrap()
                .as_bytes(),
            first
                .artifacts
                .hash::<PrimaryReliefArtifact>()
                .unwrap()
                .as_bytes(),
        ),
    ] {
        assert_eq!(unchanged.0, unchanged.1);
    }
    let domain = first.artifacts.get::<ClimateWorkDomainArtifact>().unwrap();
    let circulation = first.artifacts.get::<GlobalCirculationArtifact>().unwrap();
    domain.snapshot().validate_against(surface()).unwrap();
    circulation.snapshot().validate_against(surface()).unwrap();
    circulation.validate().unwrap();
    assert_eq!(
        circulation.snapshot().profile(),
        ClimateModelProfile::C2LayeredV1
    );

    let repeated = p4_engine
        .build(
            RootSeed::new(42),
            p4_external(ReliefSpec::default(), ClimateSpec::default()),
            &mut cache,
        )
        .unwrap();
    assert_eq!(repeated.report.cache_hits(), 5);
    assert_eq!(
        repeated
            .artifacts
            .hash::<GlobalCirculationArtifact>()
            .unwrap(),
        first.artifacts.hash::<GlobalCirculationArtifact>().unwrap()
    );

    let changed_climate = ClimateSpec {
        temperature_offset_deci_c: 10,
        ..ClimateSpec::default()
    };
    let changed = p4_engine
        .build(
            RootSeed::new(42),
            p4_external(ReliefSpec::default(), changed_climate),
            &mut cache,
        )
        .unwrap();
    assert_eq!(changed.report.cache_hits(), 4);
    assert_eq!(
        changed
            .artifacts
            .hash::<ClimateWorkDomainArtifact>()
            .unwrap(),
        first.artifacts.hash::<ClimateWorkDomainArtifact>().unwrap()
    );
    assert_ne!(
        changed
            .artifacts
            .hash::<GlobalCirculationArtifact>()
            .unwrap(),
        first.artifacts.hash::<GlobalCirculationArtifact>().unwrap()
    );
}

#[test]
fn p4_artifact_wires_are_strict_and_cancellation_is_atomic() {
    let engine = BuildEngine::new(global_circulation_graph().unwrap());
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    let failure = engine
        .build_with_cancellation(
            RootSeed::new(83),
            p4_external(ReliefSpec::default(), ClimateSpec::default()),
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

    let successful = BuildEngine::new(global_circulation_graph().unwrap())
        .build(
            RootSeed::new(43),
            p4_external(ReliefSpec::default(), ClimateSpec::default()),
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let domain = successful
        .artifacts
        .get::<ClimateWorkDomainArtifact>()
        .unwrap();
    let circulation = successful
        .artifacts
        .get::<GlobalCirculationArtifact>()
        .unwrap();
    let domain_wire = serde_json::to_value(domain.as_ref()).unwrap();
    assert_eq!(domain_wire.as_object().unwrap().len(), 1);
    let mut tampered_grid = serde_json::to_value(domain.as_ref()).unwrap();
    let first = tampered_grid["snapshot"]["climate_grid_fingerprint"][0]
        .as_u64()
        .unwrap();
    tampered_grid["snapshot"]["climate_grid_fingerprint"][0] = serde_json::json!((first + 1) % 256);
    assert!(
        serde_json::from_value::<sekai::world::natural::ClimateWorkDomainSnapshot>(
            tampered_grid["snapshot"].clone()
        )
        .is_err()
    );
    let circulation_wire = serde_json::to_value(circulation.as_ref()).unwrap();
    assert_eq!(circulation_wire.as_object().unwrap().len(), 2);
}
