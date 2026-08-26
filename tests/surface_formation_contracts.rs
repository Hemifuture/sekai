use sekai::engine::BuildCancellation;
use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::{
    build_surface_water_geometry, global_circulation_model_fingerprint,
};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    expected_global_circulation_dense_state_bytes, surface_formation_model_fingerprint,
    surface_formation_state_fingerprint, ClimateBudgetReport, ClimateCapabilitySet,
    ClimateCheckpoint, ClimateLayerLayout, ClimateModelProfile, ClimateQuantizationId,
    ClimateRemapReport, ClimateSolveReport, FormationElevationComponents, FormationEvolutionReport,
    FormationProcessRates, FormationResiduals, FormationSedimentFields, FormationTerrainFields,
    GlobalCirculationFields, GlobalCirculationSnapshot, HydrologySnapshot, MonthlyScalarField,
    MonthlyVector3Field, NaturalQualityProfile, NaturalSurfaceFormationSnapshot,
    ProductionIntegratorId, SedimentBudgetReport, SphericalHydrologySnapshot, StrahlerOrderField,
    SurfaceFormationCapabilityAvailability, SurfaceFormationCapabilityId,
    SurfaceFormationCapabilitySet, SurfaceFormationCheckpoint,
    SurfaceFormationUpstreamFingerprints, SurfaceWaterField, SurfaceWaterKind,
    FORMATION_TERRAIN_FIELDS_SCHEMA_V4, GLOBAL_CIRCULATION_SCHEMA_V2, HYDROLOGY_SCHEMA_V1,
    HYDROLOGY_SCHEMA_V2, NATURAL_SURFACE_FORMATION_SCHEMA_V5, SEDIMENT_BUDGET_RELATIVE_ERROR_MAX,
    SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX, SURFACE_FORMATION_HORIZON_YEARS,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{Meters, SphericalSpaceSpec, MAX_SPHERICAL_CELL_COUNT};

fn surface() -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 42,
    })
    .unwrap()
}

fn upstreams() -> SurfaceFormationUpstreamFingerprints {
    SurfaceFormationUpstreamFingerprints::new(
        [1; 32], [2; 32], [3; 32], [4; 32], [5; 32], [6; 32], [7; 32],
    )
    .unwrap()
}

fn upstreams_for_climate(
    climate: &GlobalCirculationSnapshot,
) -> SurfaceFormationUpstreamFingerprints {
    SurfaceFormationUpstreamFingerprints::new(
        [1; 32],
        [2; 32],
        [3; 32],
        [4; 32],
        [5; 32],
        *climate.checkpoint().fingerprint(),
        [7; 32],
    )
    .unwrap()
}

fn zero_sediment(count: usize) -> FormationSedimentFields {
    FormationSedimentFields::new(
        vec![0.0; count],
        vec![[0.0; 5]; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

fn zero_process_rates(count: usize) -> FormationProcessRates {
    FormationProcessRates::new(
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

fn scalar(cell_count: usize, value: f32) -> MonthlyScalarField {
    MonthlyScalarField::from_values(vec![[value; 12]; cell_count]).unwrap()
}

fn vectors(cell_count: usize) -> MonthlyVector3Field {
    MonthlyVector3Field::from_values(vec![[[0.0; 3]; 12]; cell_count]).unwrap()
}

fn climate(surface: &SphericalSurfaceSnapshot) -> GlobalCirculationSnapshot {
    let count = surface.cells().len();
    let fields = GlobalCirculationFields::new_c2(
        vectors(count),
        vectors(count),
        vectors(count),
        vectors(count),
        scalar(count, 12.0),
        scalar(count, 15.0),
        vec![0.1; count],
        scalar(count, 240.0),
        scalar(count, 240.0),
        scalar(count, 8.0),
        scalar(count, 900.0),
        scalar(count, 0.008),
        scalar(count, 0.0),
        scalar(count, 2.0),
        scalar(count, 0.5),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 0.0),
        scalar(count, 4.0),
    )
    .unwrap();
    let checkpoint = ClimateCheckpoint::new(
        NaturalQualityProfile::Draft,
        ClimateModelProfile::C2LayeredV1,
        ProductionIntegratorId::SplitExplicitRk3V1,
        *CubedSphereGrid::new(
            NaturalQualityProfile::Draft.climate_face_resolution(),
            surface.radius().get(),
        )
        .unwrap()
        .fingerprint(),
        [21; 32],
        global_circulation_model_fingerprint(ClimateModelProfile::C2LayeredV1),
        [22; 32],
        ClimateQuantizationId::DeterministicF64V1,
        24,
        fields.fingerprint(),
    )
    .unwrap();
    GlobalCirculationSnapshot::new(
        GLOBAL_CIRCULATION_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1),
        ProductionIntegratorId::SplitExplicitRk3V1,
        ClimateCapabilitySet::for_profile(ClimateModelProfile::C2LayeredV1),
        checkpoint,
        ClimateSolveReport::new(
            2,
            24,
            144,
            0,
            1.0,
            0.1,
            0.5,
            expected_global_circulation_dense_state_bytes(
                NaturalQualityProfile::Draft,
                ClimateModelProfile::C2LayeredV1,
                count as u32,
            )
            .unwrap(),
        )
        .unwrap(),
        ClimateBudgetReport::new(0.0, 0.0, 0.0, 0.0, 0.0).unwrap(),
        ClimateRemapReport::new(0.0, 0.0, 0.0, 0.0, 0.0, 1, 1).unwrap(),
        fields,
    )
    .unwrap()
}

fn ocean_hydrology(surface: &SphericalSurfaceSnapshot) -> SphericalHydrologySnapshot {
    let count = surface.cells().len();
    let drainage_area_km2 = surface
        .cells()
        .iter()
        .map(|cell| (cell.area.get() / 1_000_000.0) as f32)
        .collect();
    let hydrology = HydrologySnapshot::new(
        HYDROLOGY_SCHEMA_V1,
        count as u32,
        10.0,
        1.0,
        vec![[0.0; 12]; count],
        vec![[0.0; 12]; count],
        vec![0.0; count],
        vec![0.0; count],
        drainage_area_km2,
        sekai::world::natural::ElevationField::from_values(vec![-1_000.0; count]).unwrap(),
        vec![0.0; count],
        SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::Ocean; count]),
        vec![None; count],
        vec![None; count],
        StrahlerOrderField::from_raw(vec![0; count]).unwrap(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    SphericalHydrologySnapshot::new(
        HYDROLOGY_SCHEMA_V2,
        SurfaceRef::for_spherical(surface),
        hydrology,
        Vec::new(),
    )
    .unwrap()
}

fn terrain_for_surface(surface: &SphericalSurfaceSnapshot) -> FormationTerrainFields {
    let count = surface.cells().len();
    let components = FormationElevationComponents::new(
        vec![-1_000.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![-1_000.0; count],
    )
    .unwrap();
    let geometry = build_surface_water_geometry(
        surface,
        components.final_elevation_m(),
        0.0,
        &BuildCancellation::new(),
    )
    .unwrap();
    let realized = geometry.total_water_volume_m3();
    FormationTerrainFields::new(
        FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
        components,
        geometry,
        realized,
        zero_sediment(count),
    )
    .unwrap()
}

#[test]
fn serialized_p5_contract_retains_current_state_without_work_history() {
    let source = surface();
    let terrain = terrain_for_surface(&source);
    let terrain_wire = serde_json::to_value(&terrain).unwrap();
    let elevation = terrain_wire["elevation_components"].as_object().unwrap();
    for current_field in [
        "primary_elevation_m",
        "tectonic_displacement_m",
        "fluvial_erosion_m",
        "hillslope_erosion_m",
        "hillslope_deposition_m",
        "routed_sediment_deposition_m",
        "coastal_erosion_m",
        "coastal_deposition_m",
        "isostatic_response_m",
        "final_elevation_m",
    ] {
        assert!(elevation.contains_key(current_field));
    }
    for historical_field in [
        "primary_relief_m",
        "equilibrium_adjustment_m",
        "current_elevation_m",
    ] {
        assert!(!elevation.contains_key(historical_field));
    }

    let sediment = terrain_wire["sediment"].as_object().unwrap();
    for current_field in [
        "sediment_thickness_m",
        "provenance_fraction",
        "sediment_throughput_kg_per_year",
        "shelf_deposition_kg_per_year",
        "deep_ocean_export_kg_per_year",
        "endorheic_deposition_kg_per_year",
        "delta_potential",
    ] {
        assert!(sediment.contains_key(current_field));
    }
    for historical_field in [
        "sediment_throughput_kg",
        "shelf_delivery_kg",
        "deep_ocean_delivery_kg",
        "endorheic_storage_kg",
    ] {
        assert!(!sediment.contains_key(historical_field));
    }

    let checkpoint = SurfaceFormationCheckpoint::new(
        SurfaceRef::for_spherical(&source),
        NaturalQualityProfile::Draft,
        upstreams(),
        [31; 32],
    )
    .unwrap();
    let checkpoint_wire = serde_json::to_value(checkpoint).unwrap();
    assert!(checkpoint_wire.get("outer_iterations").is_none());

    let report = FormationEvolutionReport::new(
        1,
        SURFACE_FORMATION_HORIZON_YEARS,
        FormationResiduals::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0).unwrap(),
        8_192,
    )
    .unwrap();
    let report_wire = serde_json::to_value(report).unwrap();
    assert!(report_wire.get("current_rates").is_some());
    assert!(report_wire.get("accepted_surface_substeps").is_some());
    assert!(report_wire.get("integrated_duration_years").is_some());
    assert!(report_wire.get("terminal_residual").is_none());
    assert!(report_wire.get("residuals").is_none());
    assert!(report_wire.get("geomorphic_macro_steps").is_none());
    let residual = report_wire["current_rates"].as_object().unwrap();
    for current_field in [
        "net_surface_rate_rms_m_per_year",
        "gross_surface_rate_rms_m_per_year",
        "mean_elevation_rate_m_per_year",
        "rms_relief_rate_m_per_year",
        "sediment_stock_change_kg_per_year",
        "sediment_stock_change_ratio",
    ] {
        assert!(residual.contains_key(current_field));
    }
    for historical_field in [
        "elevation_rms_m",
        "receiver_changed_fraction",
        "log_discharge_rms",
        "sediment_thickness_rms_m",
        "coastline_area_changed_fraction",
    ] {
        assert!(!residual.contains_key(historical_field));
    }
}

#[test]
fn model_and_checkpoint_fingerprints_cover_exact_upstream_identity() {
    let source = surface();
    let upstream = upstreams();
    let first = SurfaceFormationCheckpoint::new(
        SurfaceRef::for_spherical(&source),
        NaturalQualityProfile::Draft,
        upstream.clone(),
        [31; 32],
    )
    .unwrap();
    let second = SurfaceFormationCheckpoint::new(
        SurfaceRef::for_spherical(&source),
        NaturalQualityProfile::Draft,
        upstream.clone(),
        [31; 32],
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.model_fingerprint(),
        &surface_formation_model_fingerprint()
    );
    assert_ne!(first.fingerprint(), &[0; 32]);
    assert_eq!(
        serde_json::from_slice::<SurfaceFormationCheckpoint>(&serde_json::to_vec(&first).unwrap())
            .unwrap(),
        first
    );

    let mut changed = upstream;
    let mut value = serde_json::to_value(&changed).unwrap();
    value["primary_relief_fingerprint"] = serde_json::to_value([9_u8; 32]).unwrap();
    changed = serde_json::from_value(value).unwrap();
    assert!(first
        .validate_against(
            SurfaceRef::for_spherical(&source),
            NaturalQualityProfile::Draft,
            &changed,
        )
        .is_err());

    let mut tampered = serde_json::to_value(&first).unwrap();
    tampered["outer_iterations"] = serde_json::json!(3);
    assert!(serde_json::from_value::<SurfaceFormationCheckpoint>(tampered).is_err());
    assert!(SurfaceFormationUpstreamFingerprints::new(
        [0; 32], [2; 32], [3; 32], [4; 32], [5; 32], [6; 32], [7; 32]
    )
    .is_err());
}

#[test]
fn terrain_fields_enforce_component_identity_provenance_and_dense_bounds() {
    let sediment = FormationSedimentFields::new(
        vec![2.0, 0.0, 4.0],
        vec![
            [1.0, 0.0, 0.0, 0.0, 0.0],
            [0.0; 5],
            [0.2, 0.3, 0.1, 0.15, 0.25],
        ],
        vec![100.0, 0.0, 200.0],
        vec![20.0, 0.0, 40.0],
        vec![10.0, 0.0, 20.0],
        vec![5.0, 0.0, 0.0],
        vec![0.5, 0.0, 1.0],
    )
    .unwrap();
    assert_eq!(
        sediment.dominant_source(0),
        Some(sekai::world::natural::SedimentSourceKind::Felsic)
    );
    assert_eq!(sediment.dominant_source(1), None);
    assert_eq!(
        sediment.dominant_source(2),
        Some(sekai::world::natural::SedimentSourceKind::Mafic)
    );
    let source = surface();
    let terrain = terrain_for_surface(&source);
    terrain.validate_against_surface(&source).unwrap();
    assert_ne!(terrain.fingerprint(), [0; 32]);
    assert_eq!(
        serde_json::from_slice::<FormationTerrainFields>(&serde_json::to_vec(&terrain).unwrap())
            .unwrap(),
        terrain
    );

    let mut drift = serde_json::to_value(&terrain).unwrap();
    drift["elevation_components"]["final_elevation_m"][0] = serde_json::json!(999.0);
    assert!(serde_json::from_value::<FormationTerrainFields>(drift).is_err());

    let mut invalid_provenance = serde_json::to_value(&terrain).unwrap();
    invalid_provenance["sediment"]["provenance_fraction"][0] =
        serde_json::json!([0.8, 0.8, 0.0, 0.0, 0.0]);
    assert!(serde_json::from_value::<FormationTerrainFields>(invalid_provenance).is_err());

    let wire = serde_json::to_value(&terrain).unwrap();
    assert!(wire.get("surface_water_geometry").is_some());
    assert!(wire.get("sea_level_m").is_none());
    assert!(wire.get("realized_water_volume_m3").is_none());
    assert!(wire.get("land_ocean").is_none());
    let mut invalid_water = wire;
    invalid_water["water_inventory_m3"] = serde_json::json!(1.0);
    assert!(serde_json::from_value::<FormationTerrainFields>(invalid_water).is_err());

    let mut oversized_wire = serde_json::to_value(&terrain).unwrap();
    oversized_wire["elevation_components"]["primary_elevation_m"] =
        serde_json::to_value(vec![0.0_f32; MAX_SPHERICAL_CELL_COUNT as usize + 1]).unwrap();
    let error = serde_json::from_value::<FormationTerrainFields>(oversized_wire).unwrap_err();
    assert!(error.to_string().contains("at most"));

    let oversized = vec![0.0; MAX_SPHERICAL_CELL_COUNT as usize + 1];
    assert!(FormationElevationComponents::new(
        oversized.clone(),
        oversized.clone(),
        oversized.clone(),
        oversized.clone(),
        oversized.clone(),
        oversized.clone(),
        oversized.clone(),
        oversized.clone(),
        oversized.clone(),
        oversized,
    )
    .is_err());
}

#[test]
fn evolution_budget_and_capability_reports_are_strict() {
    let terminal = FormationResiduals::new(1.0e-9, 1.0e-3, 2.0e-10, -3.0e-10, 1.0, 1.0e-6).unwrap();
    let evolution =
        FormationEvolutionReport::new(16, SURFACE_FORMATION_HORIZON_YEARS, terminal, 8_192)
            .unwrap();
    assert_eq!(evolution.accepted_surface_substeps(), 16);
    assert_eq!(
        evolution.integrated_duration_years().to_bits(),
        SURFACE_FORMATION_HORIZON_YEARS.to_bits()
    );
    assert_eq!(evolution.current_rates(), &terminal);

    let produced = [50.0, 20.0, 10.0, 15.0, 5.0];
    let accounted = [50.0, 20.0, 10.0, 15.0, 5.0];
    let budget =
        SedimentBudgetReport::new(100.0, 30.0, 20.0, 40.0, 10.0, produced, accounted).unwrap();
    assert!(budget.global_relative_error() <= SEDIMENT_BUDGET_RELATIVE_ERROR_MAX);
    assert!(budget
        .provenance_relative_errors()
        .iter()
        .all(|error| *error <= SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX));

    let capabilities = SurfaceFormationCapabilitySet::p5();
    assert_eq!(
        capabilities.availability(SurfaceFormationCapabilityId::ImplicitStreamPowerV1),
        SurfaceFormationCapabilityAvailability::Available
    );
    assert_eq!(
        capabilities.availability(SurfaceFormationCapabilityId::GlacialErosionV1),
        SurfaceFormationCapabilityAvailability::Unavailable
    );
    assert_eq!(
        serde_json::from_slice::<SurfaceFormationCapabilitySet>(
            &serde_json::to_vec(&capabilities).unwrap()
        )
        .unwrap(),
        capabilities
    );

    let mut forged_capabilities = serde_json::to_value(&capabilities).unwrap();
    forged_capabilities["statuses"][0]["availability"] = serde_json::json!("unavailable");
    assert!(serde_json::from_value::<SurfaceFormationCapabilitySet>(forged_capabilities).is_err());

    let mut forged_budget = serde_json::to_value(budget).unwrap();
    forged_budget["global_relative_error"] = serde_json::json!(1.0e-12);
    forged_budget["deep_ocean_export_kg_per_year"] = serde_json::json!(60.0);
    assert!(serde_json::from_value::<SedimentBudgetReport>(forged_budget).is_err());

    let mut invalid_evolution = serde_json::to_value(&evolution).unwrap();
    invalid_evolution["accepted_surface_substeps"] = serde_json::json!(0);
    assert!(serde_json::from_value::<FormationEvolutionReport>(invalid_evolution).is_err());
}

#[test]
fn atomic_snapshot_binds_terrain_hydrology_and_endpoint_checkpoint() {
    let source = surface();
    let terrain = terrain_for_surface(&source);
    let process_rates = zero_process_rates(source.cells().len());
    let hydrology = ocean_hydrology(&source);
    let climate = climate(&source);
    let state_fingerprint =
        surface_formation_state_fingerprint(&terrain, &process_rates, &hydrology);
    let upstream = upstreams_for_climate(&climate);
    let checkpoint = SurfaceFormationCheckpoint::new(
        SurfaceRef::for_spherical(&source),
        NaturalQualityProfile::Draft,
        upstream.clone(),
        state_fingerprint,
    )
    .unwrap();
    let snapshot = NaturalSurfaceFormationSnapshot::new(
        NATURAL_SURFACE_FORMATION_SCHEMA_V5,
        SurfaceRef::for_spherical(&source),
        checkpoint,
        terrain,
        process_rates,
        hydrology,
        FormationEvolutionReport::new(
            1,
            SURFACE_FORMATION_HORIZON_YEARS,
            FormationResiduals::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0).unwrap(),
            8_192,
        )
        .unwrap(),
        SedimentBudgetReport::new(0.0, 0.0, 0.0, 0.0, 0.0, [0.0; 5], [0.0; 5]).unwrap(),
        SurfaceFormationCapabilitySet::p5(),
    )
    .unwrap();
    snapshot.validate_against(&source).unwrap();
    snapshot
        .validate_against_inputs(&source, NaturalQualityProfile::Draft, &upstream)
        .unwrap();

    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let decoded: NaturalSurfaceFormationSnapshot = serde_json::from_slice(&bytes).unwrap();
    decoded.validate_against(&source).unwrap();
    assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());
    assert!(serde_json::to_value(&decoded).unwrap()["formation_climate"].is_null());

    let mut unknown = serde_json::to_value(&snapshot).unwrap();
    unknown["surprise"] = serde_json::json!(true);
    assert!(serde_json::from_value::<NaturalSurfaceFormationSnapshot>(unknown).is_err());

    let mut wrong_state = serde_json::to_value(&snapshot).unwrap();
    wrong_state["terrain_fields"]["sea_level_m"] = serde_json::json!(10.0);
    assert!(serde_json::from_value::<NaturalSurfaceFormationSnapshot>(wrong_state).is_err());
}
