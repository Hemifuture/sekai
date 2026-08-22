use std::sync::OnceLock;

use sekai::engine::{
    Artifact, BuildCancellation, BuildEngine, BuildOutcome, ExternalArtifacts, MemoryStageCache,
    Stage,
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
    ResolvedWorldFormation, ResolvedWorldFormationPreset, SeaLevelPolicy, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
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

/// Builds the full draft-tier formation product the presentation-layer
/// engines consume (shared by the frozen T1 fingerprint gates).
fn draft_formation_outcome(root_seed: RootSeed) -> BuildOutcome {
    draft_formation_outcome_with_relief(root_seed, &ReliefSpec::default())
}

fn draft_formation_outcome_with_relief(
    root_seed: RootSeed,
    relief_spec: &ReliefSpec,
) -> BuildOutcome {
    let external = sekai::app::build_spherical_formation_external_artifacts(
        root_seed,
        NaturalQualityProfile::Draft,
        surface(),
        &sekai::world::natural::WorldFormationSpec::default(),
        &TectonicSpec::default(),
        relief_spec,
        &GeologicSpec::default(),
    )
    .unwrap();
    BuildEngine::new(surface_formation_graph().unwrap())
        .build(root_seed, external, &mut MemoryStageCache::new())
        .unwrap()
}

#[test]
fn target_land_fraction_inventory_reaches_p5_without_changing_the_default_product() {
    let default = draft_formation_outcome(RootSeed::new(42));
    let default_formation = default
        .artifacts
        .get::<NaturalSurfaceFormationArtifact>()
        .unwrap();
    assert_eq!(
        blake3::hash(&serde_json::to_vec(default_formation.as_ref()).unwrap())
            .to_hex()
            .to_string(),
        "83a67fc6688db690f0a0e691cce280593febbc5b737b26afcb261479717a7f90"
    );

    let target = draft_formation_outcome_with_relief(
        RootSeed::new(42),
        &ReliefSpec {
            target_land_fraction: 0.38,
            sea_level_policy: SeaLevelPolicy::TargetLandFraction,
            ..ReliefSpec::default()
        },
    );
    let primary = target.artifacts.get::<PrimaryReliefArtifact>().unwrap();
    let formation = target
        .artifacts
        .get::<NaturalSurfaceFormationArtifact>()
        .unwrap();
    let implicit_ratio = primary
        .quality_report()
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == "water-inventory-ratio")
        .and_then(|metric| metric.value())
        .expect("P3 publishes the implicit target-mode water ratio");
    let p3_artifact_hash = blake3::hash(&serde_json::to_vec(primary.as_ref()).unwrap())
        .to_hex()
        .to_string();
    let p5_artifact_hash = blake3::hash(&serde_json::to_vec(formation.as_ref()).unwrap())
        .to_hex()
        .to_string();
    assert_eq!(
        p3_artifact_hash,
        "8c0ed4313edb4d136c5c41adad879d320ca0f52d87e182ac14cf49fd4021bd27"
    );
    assert_eq!(
        p5_artifact_hash,
        "95738e6773494eddf765dfccd7117bb259bc5268fd78200ec0cf6c5a1cdc76f8"
    );
    println!(
        "target_driver_seed42 p3_artifact={} p5_artifact={} implicit_water_ratio={implicit_ratio:.12} p3_sea_level_m={:.6} p3_land_fraction={:.9} p5_sea_level_m={:.6} p5_land_fraction={:.9}",
        p3_artifact_hash,
        p5_artifact_hash,
        primary.snapshot().sea_level_m(),
        primary.snapshot().physical_land_fraction(),
        formation.snapshot().terrain_fields().sea_level_m(),
        sekai::world::natural::physical_land_fraction(
            surface(),
            formation.snapshot().terrain_fields().land_ocean(),
        )
        .unwrap(),
    );
    let document =
        sekai::app::SphericalFormationFieldDocument::from_build_outcome(&target).unwrap();
    assert_eq!(
        document.area_summary().sea_level_policy(),
        SeaLevelPolicy::TargetLandFraction
    );
    assert_eq!(
        document.area_summary().water_inventory_ratio().to_bits(),
        implicit_ratio.to_bits()
    );
    assert_eq!(
        formation
            .snapshot()
            .terrain_fields()
            .water_inventory_m3()
            .to_bits(),
        primary.snapshot().water_inventory_m3().to_bits()
    );
    let drift = formation
        .quality_report()
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == "final-land-fraction-absolute-change")
        .and_then(|metric| metric.value())
        .expect("P5 reports its production land-fraction drift gate");
    assert!(drift <= 0.01, "target-mode P5 land-fraction drift {drift}");
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

#[test]
fn the_formation_document_materializes_every_field_from_the_app_build_path() {
    use sekai::world::natural::{
        annual_local_runoff_mm_field_id, circulation_annual_precipitation_mm_field_id,
        circulation_mean_air_temperature_c_field_id, circulation_prevailing_wind_m_s_field_id,
        coastal_deposition_m_field_id, coastal_erosion_m_field_id, crust_kind_field_id,
        crust_thickness_field_id, drainage_area_km2_field_id, fluvial_erosion_depth_m_field_id,
        hillslope_deposition_m_field_id, hillslope_erosion_m_field_id,
        isostatic_response_m_field_id, lake_depth_m_field_id, land_ocean_field_id,
        mean_annual_discharge_m3_s_field_id, plate_id_field_id, primary_elevation_m_field_id,
        routed_sediment_deposition_m_field_id, sediment_deposition_thickness_m_field_id,
        strahler_stream_order_field_id, surface_elevation_m_field_id, surface_water_kind_field_id,
        tectonic_displacement_m_field_id, WorldFormationSpec,
    };

    let root_seed = RootSeed::new(42);
    let external = sekai::app::build_spherical_formation_external_artifacts(
        root_seed,
        NaturalQualityProfile::Draft,
        surface(),
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
    )
    .unwrap();
    let outcome = BuildEngine::new(surface_formation_graph().unwrap())
        .build(root_seed, external, &mut MemoryStageCache::new())
        .unwrap();
    let document =
        sekai::app::SphericalFormationFieldDocument::from_build_outcome(&outcome).unwrap();

    let cell_count = document.surface().cells().len();
    assert_eq!(cell_count, surface().cells().len());
    let catalog = document.catalog().unwrap();
    let expected_fields = [
        plate_id_field_id(),
        crust_kind_field_id(),
        crust_thickness_field_id(),
        primary_elevation_m_field_id(),
        tectonic_displacement_m_field_id(),
        fluvial_erosion_depth_m_field_id(),
        hillslope_erosion_m_field_id(),
        hillslope_deposition_m_field_id(),
        routed_sediment_deposition_m_field_id(),
        coastal_erosion_m_field_id(),
        coastal_deposition_m_field_id(),
        isostatic_response_m_field_id(),
        sediment_deposition_thickness_m_field_id(),
        surface_elevation_m_field_id(),
        land_ocean_field_id(),
        circulation_annual_precipitation_mm_field_id(),
        circulation_mean_air_temperature_c_field_id(),
        circulation_prevailing_wind_m_s_field_id(),
        annual_local_runoff_mm_field_id(),
        lake_depth_m_field_id(),
        surface_water_kind_field_id(),
        mean_annual_discharge_m3_s_field_id(),
        drainage_area_km2_field_id(),
        strahler_stream_order_field_id(),
    ];
    assert_eq!(catalog.entries().len(), expected_fields.len());
    for field in &expected_fields {
        let view = catalog
            .get(field)
            .unwrap_or_else(|| panic!("field {field:?} is missing from the formation catalog"))
            .view()
            .unwrap_or_else(|| panic!("field {field:?} did not materialize a payload"));
        assert_eq!(view.len(), cell_count, "cardinality of {field:?}");
    }

    assert_eq!(
        document.preferred_field(),
        Some(surface_elevation_m_field_id())
    );
    let Some(sekai::view::DisplayRangeMode::Manual(range)) =
        document.preferred_range(&surface_elevation_m_field_id())
    else {
        panic!("the formation surface elevation must use a sea-anchored manual range");
    };
    let summary = document.area_summary();
    let sea = summary.sea_level_m();
    assert!(((range.min() + range.max()) * 0.5 - sea).abs() < 0.5);
    assert_eq!(summary.sea_level_policy(), SeaLevelPolicy::WaterInventory);
    assert_eq!(
        summary.target_land_fraction().to_bits(),
        ReliefSpec::default().target_land_fraction.to_bits()
    );
    assert_eq!(summary.water_inventory_ratio().to_bits(), 1.0_f64.to_bits());
    assert!(
        summary.evolved_continental_fraction() > 0.2,
        "v5 conserves continental area, got {}",
        summary.evolved_continental_fraction()
    );
    assert!(
        (0.05..0.95).contains(&summary.actual_land_fraction()),
        "land fraction {}",
        summary.actual_land_fraction()
    );

    let source = document.presentation_source();
    assert_eq!(source.root_seed(), root_seed);
}

#[test]
fn the_t1_amplifier_matches_its_frozen_product_fingerprint() {
    use sekai::generators::natural::{fibonacci_probe, AmplificationLod, TerrainAmplifier};

    let root_seed = RootSeed::new(42);
    let outcome = draft_formation_outcome(root_seed);
    let evolved = outcome.artifacts.get::<EvolvedTectonicArtifact>().unwrap();
    let substrate = outcome
        .artifacts
        .get::<GeologicSubstrateArtifact>()
        .unwrap();
    let formation = outcome
        .artifacts
        .get::<NaturalSurfaceFormationArtifact>()
        .unwrap();

    let amplifier = TerrainAmplifier::from_formation_product(
        surface(),
        evolved.snapshot().compatibility(),
        substrate.snapshot(),
        formation.snapshot(),
        root_seed,
    )
    .unwrap();

    // M1 bake LOD from the spec §6 Nyquist rule (4096-wide equirect).
    let bake_footprint_m = 40_075_000.0 / 4_096.0;
    let lod =
        AmplificationLod::for_sampling_footprint(amplifier.base_wavelength_m(), bake_footprint_m);
    let fingerprint = amplifier.probe_fingerprint(lod);
    let hex: String = fingerprint
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    eprintln!(
        "t1 probe fingerprint (draft, seed 42, lod {}): {hex}",
        lod.levels()
    );
    assert_eq!(
        hex, "20fb2405f60ea634b2153474a06f2103fc059073479ba8414ac297c164e36ea5",
        "the frozen T1 probe fingerprint changed; record an amendment in the T1 spec"
    );

    // Spec §8 invariant 4 on the real product: amplified classification must
    // keep the land fraction within one percentage point of T0's.
    let terrain = formation.snapshot().terrain_fields();
    let sea = terrain.sea_level_m();
    let total = 16_384_usize;
    let mut t0_land = 0_u32;
    let mut amplified_land = 0_u32;
    for index in 0..total {
        let probe = fibonacci_probe(index, total);
        let sample = amplifier.sample(probe, lod);
        if sample.elevation_m >= sea {
            amplified_land += 1;
        }
        let baseline = amplifier.sample(probe, AmplificationLod::new(0));
        if baseline.elevation_m >= sea {
            t0_land += 1;
        }
    }
    let drift = (f64::from(t0_land) - f64::from(amplified_land)).abs() / total as f64;
    assert!(drift <= 0.01, "product land-fraction drift {drift}");
}

#[test]
fn the_t1v2_hierarchical_engine_matches_its_frozen_product_fingerprint() {
    use sekai::generators::natural::{fibonacci_probe, HierarchicalEvaluator};
    use sekai::world::CellId;

    let root_seed = RootSeed::new(42);
    let outcome = draft_formation_outcome(root_seed);
    let evolved = outcome.artifacts.get::<EvolvedTectonicArtifact>().unwrap();
    let substrate = outcome
        .artifacts
        .get::<GeologicSubstrateArtifact>()
        .unwrap();
    let formation = outcome
        .artifacts
        .get::<NaturalSurfaceFormationArtifact>()
        .unwrap();

    let evaluator = HierarchicalEvaluator::from_formation_product(
        surface(),
        evolved.snapshot().compatibility(),
        substrate.snapshot(),
        formation.snapshot(),
        root_seed,
    )
    .unwrap();

    // Spec §7 invariant 4 on the real product: every L0 cell primitive
    // is the published T0 elevation, bit for bit.
    let terrain = formation.snapshot().terrain_fields();
    for (index, &elevation) in terrain.final_elevation_m().iter().enumerate() {
        assert_eq!(
            evaluator
                .cell_value(CellId::from_raw(index as u32))
                .elevation_m
                .to_bits(),
            elevation.to_bits()
        );
    }

    // Spec §6: the frozen hierarchical probe fingerprint (value history
    // in the spec amendments — A4 froze it; A6.7, A7, and A8 refreshed
    // it).
    let fingerprint = evaluator.probe_fingerprint();
    let hex: String = fingerprint
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    eprintln!("t1 v2 hierarchical probe fingerprint (draft, seed 42): {hex}");
    assert_eq!(
        hex, "cab6c758fe2ce2dac477e6d8fb674f73a3863ac66a96a7d411d6a86bc339c7b7",
        "the frozen T1 v2 hierarchical probe fingerprint changed; record an amendment in the spec"
    );

    // Spec §7 invariant 6 on the real product: the deep-level land
    // fraction stays within one percentage point of the L0 stair field.
    let sea = terrain.sea_level_m();
    let total = 16_384_usize;
    let mut l0_land = 0_u32;
    let mut deep_land = 0_u32;
    for index in 0..total {
        let probe = fibonacci_probe(index, total);
        if evaluator.sample(probe, 0).elevation_m >= sea {
            l0_land += 1;
        }
        if evaluator.sample(probe, 6).elevation_m >= sea {
            deep_land += 1;
        }
    }
    let drift = (f64::from(l0_land) - f64::from(deep_land)).abs() / total as f64;
    assert!(drift <= 0.01, "product land-fraction drift {drift}");
}
