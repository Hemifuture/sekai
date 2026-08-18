use sekai::generators::natural::circulation::CubedSphereGrid;
use sekai::generators::natural::global_circulation_model_fingerprint;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    expected_global_circulation_dense_state_bytes, formation_elevation_from_components,
    surface_formation_model_fingerprint, surface_formation_state_fingerprint,
    water_volume_at_sea_level_m3, ClimateBudgetReport, ClimateCapabilitySet, ClimateCheckpoint,
    ClimateLayerLayout, ClimateModelProfile, ClimateQuantizationId, ClimateRemapReport,
    ClimateSolveReport, FormationElevationComponents, FormationResiduals, FormationSedimentFields,
    FormationSolveReport, FormationTerrainFields, GlobalCirculationFields,
    GlobalCirculationSnapshot, HydrologySnapshot, LandOceanField, MonthlyScalarField,
    MonthlyVector3Field, NaturalQualityProfile, NaturalSurfaceFormationSnapshot,
    ProductionIntegratorId, SedimentBudgetReport, SphericalHydrologySnapshot, StrahlerOrderField,
    SurfaceFormationCapabilityAvailability, SurfaceFormationCapabilityId,
    SurfaceFormationCapabilitySet, SurfaceFormationCheckpoint,
    SurfaceFormationUpstreamFingerprints, SurfaceWaterField, SurfaceWaterKind,
    FORMATION_HILLSLOPE_CRITICAL_SLOPE, FORMATION_HILLSLOPE_DENOMINATOR_MIN,
    FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR, FORMATION_MINIMUM_LAKE_DEPTH_M,
    FORMATION_RUNOFF_MIN_FRACTION, FORMATION_RUNOFF_PERMEABILITY_RANGE,
    FORMATION_STREAM_POWER_AREA_EXPONENT, FORMATION_STREAM_POWER_ERODIBILITY_BASE,
    FORMATION_STREAM_POWER_ERODIBILITY_RANGE, FORMATION_STREAM_POWER_RUNOFF_FACTOR_MAX,
    FORMATION_STREAM_POWER_RUNOFF_FACTOR_MIN, FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM,
    FORMATION_STREAM_POWER_SLOPE_EXPONENT, FORMATION_TERRAIN_FIELDS_SCHEMA_V1,
    GLOBAL_CIRCULATION_SCHEMA_V1, HYDROLOGY_SCHEMA_V1, HYDROLOGY_SCHEMA_V2,
    NATURAL_SURFACE_FORMATION_SCHEMA_V1, SEDIMENT_BUDGET_RELATIVE_ERROR_MAX,
    SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX, SURFACE_FORMATION_MACRO_STEPS,
    SURFACE_FORMATION_MAX_OUTER_ITERATIONS,
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

fn elevation_components(primary: Vec<f32>) -> FormationElevationComponents {
    let count = primary.len();
    let tectonic = (0..count)
        .map(|index| 10.0 + index as f32)
        .collect::<Vec<_>>();
    let fluvial = vec![2.0; count];
    let hillslope_erosion = vec![3.0; count];
    let hillslope_deposition = vec![1.0; count];
    let routed_deposition = vec![0.5; count];
    let coastal_erosion = vec![0.25; count];
    let coastal_deposition = vec![0.125; count];
    let isostatic = vec![0.75; count];
    let final_elevation = (0..count)
        .map(|index| {
            formation_elevation_from_components(
                primary[index],
                tectonic[index],
                fluvial[index],
                hillslope_erosion[index],
                hillslope_deposition[index],
                routed_deposition[index],
                coastal_erosion[index],
                coastal_deposition[index],
                isostatic[index],
            )
        })
        .collect();
    FormationElevationComponents::new(
        primary,
        tectonic,
        fluvial,
        hillslope_erosion,
        hillslope_deposition,
        routed_deposition,
        coastal_erosion,
        coastal_deposition,
        isostatic,
        final_elevation,
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
        scalar(count, 8.0),
        scalar(count, 900.0),
        scalar(count, 0.008),
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
        GLOBAL_CIRCULATION_SCHEMA_V1,
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
    let areas = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .collect::<Vec<_>>();
    let realized =
        water_volume_at_sea_level_m3(components.final_elevation_m(), &areas, 0.0).unwrap();
    FormationTerrainFields::new(
        FORMATION_TERRAIN_FIELDS_SCHEMA_V1,
        components,
        0.0,
        realized,
        realized,
        LandOceanField::from_kinds(vec![sekai::world::natural::LandOceanKind::Ocean; count]),
        zero_sediment(count),
    )
    .unwrap()
}

#[test]
fn model_and_checkpoint_fingerprints_cover_exact_upstream_identity() {
    assert_eq!(FORMATION_RUNOFF_MIN_FRACTION, 0.15);
    assert_eq!(FORMATION_RUNOFF_PERMEABILITY_RANGE, 0.70);
    assert_eq!(FORMATION_MINIMUM_LAKE_DEPTH_M, 1.0);
    assert_eq!(FORMATION_STREAM_POWER_AREA_EXPONENT, 0.5);
    assert_eq!(FORMATION_STREAM_POWER_SLOPE_EXPONENT, 1.0);
    assert_eq!(FORMATION_STREAM_POWER_ERODIBILITY_BASE, 0.25);
    assert_eq!(FORMATION_STREAM_POWER_ERODIBILITY_RANGE, 1.50);
    assert_eq!(FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM, 1_000.0);
    assert_eq!(FORMATION_STREAM_POWER_RUNOFF_FACTOR_MIN, 0.10);
    assert_eq!(FORMATION_STREAM_POWER_RUNOFF_FACTOR_MAX, 4.0);
    assert_eq!(FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR, 5_000.0);
    assert_eq!(FORMATION_HILLSLOPE_DENOMINATOR_MIN, 0.10);
    assert!((FORMATION_HILLSLOPE_CRITICAL_SLOPE - 32.0_f64.to_radians().tan()).abs() < 1.0e-15);
    let source = surface();
    let upstream = upstreams();
    let first = SurfaceFormationCheckpoint::new(
        SurfaceRef::for_spherical(&source),
        NaturalQualityProfile::Draft,
        upstream.clone(),
        2,
        [31; 32],
    )
    .unwrap();
    let second = SurfaceFormationCheckpoint::new(
        SurfaceRef::for_spherical(&source),
        NaturalQualityProfile::Draft,
        upstream.clone(),
        2,
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
    let components = elevation_components(vec![100.0, -200.0, 50.0]);
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
    let final_elevation = components.final_elevation_m().to_vec();
    let terrain = FormationTerrainFields::new(
        FORMATION_TERRAIN_FIELDS_SCHEMA_V1,
        components,
        0.0,
        1_000.0,
        1_000.0,
        LandOceanField::classify(
            &sekai::world::natural::ElevationField::from_values(final_elevation).unwrap(),
            0.0,
        ),
        sediment,
    )
    .unwrap();
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

    let mut invalid_water = serde_json::to_value(&terrain).unwrap();
    invalid_water["realized_water_volume_m3"] = serde_json::json!(2_000.0);
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
fn solve_budget_and_capability_reports_are_derived_and_strict() {
    let residuals = vec![
        FormationResiduals::new(180.0, 0.08, 0.30, 20.0, 0.01).unwrap(),
        FormationResiduals::new(40.0, 0.01, 0.02, 2.0, 0.001).unwrap(),
    ];
    let solve = FormationSolveReport::new(residuals, 8_192).unwrap();
    assert_eq!(solve.outer_iterations(), 2);
    assert_eq!(
        solve.geomorphic_macro_steps(),
        2 * SURFACE_FORMATION_MACRO_STEPS
    );
    assert!(solve.converged());
    assert!(solve.final_residual().normalized_max() <= 1.0);

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
    forged_budget["deep_ocean_delivery_mass_kg"] = serde_json::json!(60.0);
    assert!(serde_json::from_value::<SedimentBudgetReport>(forged_budget).is_err());

    let mut oversized_solve = serde_json::to_value(&solve).unwrap();
    let extra = oversized_solve["residuals"][0].clone();
    while oversized_solve["residuals"].as_array().unwrap().len()
        <= SURFACE_FORMATION_MAX_OUTER_ITERATIONS as usize
    {
        oversized_solve["residuals"]
            .as_array_mut()
            .unwrap()
            .push(extra.clone());
    }
    assert!(serde_json::from_value::<FormationSolveReport>(oversized_solve).is_err());
}

#[test]
fn atomic_snapshot_binds_terrain_hydrology_climate_and_upstreams() {
    let source = surface();
    let terrain = terrain_for_surface(&source);
    let hydrology = ocean_hydrology(&source);
    let climate = climate(&source);
    let state_fingerprint = surface_formation_state_fingerprint(&terrain, &hydrology, &climate);
    let checkpoint = SurfaceFormationCheckpoint::new(
        SurfaceRef::for_spherical(&source),
        NaturalQualityProfile::Draft,
        upstreams(),
        1,
        state_fingerprint,
    )
    .unwrap();
    let snapshot = NaturalSurfaceFormationSnapshot::new(
        NATURAL_SURFACE_FORMATION_SCHEMA_V1,
        SurfaceRef::for_spherical(&source),
        checkpoint,
        terrain,
        hydrology,
        climate,
        FormationSolveReport::new(
            vec![FormationResiduals::new(10.0, 0.01, 0.01, 1.0, 0.001).unwrap()],
            8_192,
        )
        .unwrap(),
        SedimentBudgetReport::new(0.0, 0.0, 0.0, 0.0, 0.0, [0.0; 5], [0.0; 5]).unwrap(),
        SurfaceFormationCapabilitySet::p5(),
    )
    .unwrap();
    snapshot.validate_against(&source).unwrap();
    snapshot
        .validate_against_inputs(&source, NaturalQualityProfile::Draft, &upstreams())
        .unwrap();

    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let decoded: NaturalSurfaceFormationSnapshot = serde_json::from_slice(&bytes).unwrap();
    decoded.validate_against(&source).unwrap();
    assert_eq!(bytes, serde_json::to_vec(&decoded).unwrap());

    let mut unknown = serde_json::to_value(&snapshot).unwrap();
    unknown["surprise"] = serde_json::json!(true);
    assert!(serde_json::from_value::<NaturalSurfaceFormationSnapshot>(unknown).is_err());

    let mut wrong_state = serde_json::to_value(&snapshot).unwrap();
    wrong_state["terrain_fields"]["sea_level_m"] = serde_json::json!(10.0);
    assert!(serde_json::from_value::<NaturalSurfaceFormationSnapshot>(wrong_state).is_err());
}
