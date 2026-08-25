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
    SurfaceFormationGenerationError, SurfaceFormationGenerator, SurfaceFormationInputs,
    SurfaceFormationStage,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, SphericalSurfaceArtifact};
use sekai::rules::{ClimateModel, GeologicModel, HydroErosionModel, TectonicModel};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, NaturalQualityProfile, ReliefSpec,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    ELEVATION_MAX_M, ELEVATION_MIN_M, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
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

/// On-demand per-seed convergence check through the exact app externals
/// (`SEKAI_P5_SEED`, `SEKAI_P5_PROFILE`; `SEKAI_P5_TRACE=1` prints the
/// per-iteration residual vector). Fails with the report diagnostics
/// when the seed does not build.
#[test]
#[ignore = "on-demand single-seed P5 convergence probe"]
fn probe_formation_fixed_point_seed() {
    let seed: u64 = std::env::var("SEKAI_P5_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3_945_477_593_443_907_072);
    let profile = match std::env::var("SEKAI_P5_PROFILE").as_deref() {
        Ok("standard") => NaturalQualityProfile::Standard,
        Ok("high") => NaturalQualityProfile::High,
        _ => NaturalQualityProfile::Draft,
    };
    let built;
    let probe_surface = if profile == NaturalQualityProfile::Draft {
        surface()
    } else {
        built = ProfileSurfaceBuilder::build(
            profile,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap()
        .authoritative_surface()
        .clone();
        &built
    };
    let root_seed = RootSeed::new(seed);
    let external = sekai::app::build_spherical_formation_external_artifacts(
        root_seed,
        profile,
        probe_surface,
        &sekai::world::natural::WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
    )
    .unwrap();
    let result = BuildEngine::new(surface_formation_graph().unwrap()).build(
        root_seed,
        external,
        &mut MemoryStageCache::new(),
    );
    match &result {
        Ok(_) => println!("seed {seed}: CONVERGED"),
        Err(failure) => {
            for diagnostic in failure.report.diagnostics() {
                println!(
                    "  [{:?}] {}: {}",
                    diagnostic.severity(),
                    diagnostic.code(),
                    diagnostic.message()
                );
            }
            panic!("seed {seed}: {failure}");
        }
    }
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
    assert_eq!(SurfaceFormationStage.version(), 2);
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
fn default_absolute_steady_state_fails_atomically_with_f64_domain_evidence() {
    let root_seed = RootSeed::new(42);
    let climate_spec = ClimateSpec::default();
    let formation_spec = HydroErosionSpec::default();
    let mut cache = MemoryStageCache::new();
    let p4 = BuildEngine::new(global_circulation_graph().unwrap())
        .build(root_seed, p4_external(climate_spec.clone()), &mut cache)
        .unwrap();
    assert_eq!(cache.len(), 5, "P4 must publish all five upstream stages");

    let tectonics = p4.artifacts.get::<EvolvedTectonicArtifact>().unwrap();
    let substrate = p4.artifacts.get::<GeologicSubstrateArtifact>().unwrap();
    let relief = p4.artifacts.get::<PrimaryReliefArtifact>().unwrap();
    let domain = p4.artifacts.get::<ClimateWorkDomainArtifact>().unwrap();
    let climate = p4.artifacts.get::<GlobalCirculationArtifact>().unwrap();
    let direct_error = SurfaceFormationGenerator::generate(
        SurfaceFormationInputs {
            surface: surface(),
            quality_profile: NaturalQualityProfile::Draft,
            tectonics: tectonics.snapshot(),
            substrate: substrate.snapshot(),
            relief: relief.snapshot(),
            domain: domain.snapshot(),
            climate_spec: &climate_spec,
            initial_climate: climate.snapshot(),
            formation_spec: &formation_spec,
        },
        &BuildCancellation::new(),
    )
    .unwrap_err();
    let found = match &direct_error {
        SurfaceFormationGenerationError::ElevationOutOfRange { found, .. } => *found,
        other => panic!("default absolute steady state returned the wrong typed failure: {other}"),
    };
    assert!(
        found < f64::from(ELEVATION_MIN_M) || found > f64::from(ELEVATION_MAX_M),
        "the full-f64 candidate must truly lie outside the publishable domain"
    );
    assert!(
        (ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&(found as f32)),
        "the legacy f32 projection must demonstrate its false-green rounding"
    );

    let failure = BuildEngine::new(surface_formation_graph().unwrap())
        .build(
            root_seed,
            p5_external(climate_spec, formation_spec),
            &mut cache,
        )
        .unwrap_err();
    assert_eq!(failure.report.cache_hits(), 5);
    assert_eq!(failure.report.cache_misses(), 1);
    assert_eq!(cache.len(), 5, "the failed P5 product must not enter cache");
    assert!(failure.report.result_hash().is_none());
    let diagnostic = failure
        .report
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == "surface-formation.build-failed")
        .expect("the graph must retain the typed P5 failure at its stage boundary");
    assert_eq!(diagnostic.message(), direct_error.to_string());
}

// Task 0 intentionally has no successful default/target P5 artifact to hash.
// The real field payload and T1/T1v2 product fingerprints return with the
// Task 11 bundle/UI restoration; Task 9 first restores default P5 success.
