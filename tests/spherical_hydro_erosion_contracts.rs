use sekai::generators::natural::{FluvialErosionGenerator, HydrologyGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BedrockKind, BedrockKindField, ElevationField, HydroErosionSpec, LandOceanField, LandOceanKind,
    MonthlyScalarField, MonthlyVector3Field, SphericalGeologicSnapshot,
    SphericalHydroErosionSnapshot, SphericalPreliminaryClimateSnapshot, SphericalReliefSnapshot,
    SphericalSurfaceProcessSnapshot, CLIMATE_MONTH_COUNT, GEOLOGIC_SNAPSHOT_SCHEMA_V2,
    HYDRO_EROSION_SNAPSHOT_SCHEMA_V2, PRELIMINARY_CLIMATE_SCHEMA_V2, RELIEF_SCHEMA_V4,
    SURFACE_PROCESS_SCHEMA_V2,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{Meters, SphericalSpaceSpec, MAX_SPHERICAL_CELL_COUNT};

fn surface(radius_m: f64) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count: 42,
    })
    .unwrap()
}

fn relief(surface: &SphericalSurfaceSnapshot, elevations: Vec<f32>) -> SphericalReliefSnapshot {
    let count = surface.cells().len();
    let zero = vec![0.0; count];
    SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        0.0,
        ElevationField::from_values(elevations.clone()).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero).unwrap(),
        ElevationField::from_values(elevations.clone()).unwrap(),
        LandOceanField::from_kinds(
            elevations
                .into_iter()
                .map(|value| LandOceanKind::classify(value, 0.0))
                .collect(),
        ),
    )
    .unwrap()
}

fn valid_surface_process(
    surface: &SphericalSurfaceSnapshot,
    relief: &SphericalReliefSnapshot,
) -> SphericalSurfaceProcessSnapshot {
    let count = surface.cells().len();
    let erosion_depth_m = vec![1.0; count];
    let deposition_thickness_m = vec![0.25; count];
    let surface_elevation_m = relief
        .elevation_m()
        .values()
        .iter()
        .map(|&value| value - 0.75)
        .collect();
    let eroded_m3 = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .sum::<f64>();
    let deposited_m3 = eroded_m3 * 0.25;
    SphericalSurfaceProcessSnapshot::new(
        SURFACE_PROCESS_SCHEMA_V2,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        erosion_depth_m,
        deposition_thickness_m,
        ElevationField::from_values(surface_elevation_m).unwrap(),
        surface.cells().iter().map(|cell| cell.area.get()).collect(),
        0.0,
        eroded_m3 - deposited_m3,
    )
    .unwrap()
}

#[test]
fn spherical_surface_process_round_trips_and_closes_the_terminal_ledger() {
    let surface = surface(6_371_000.0);
    let relief = relief(&surface, vec![1_000.0; surface.cells().len()]);
    let snapshot = valid_surface_process(&surface, &relief);

    snapshot.validate_against(&surface, &relief).unwrap();
    assert_eq!(snapshot.schema_version(), SURFACE_PROCESS_SCHEMA_V2);
    assert_eq!(
        snapshot.surface_ref(),
        SurfaceRef::try_for_spherical(&surface).unwrap()
    );
    assert_eq!(snapshot.cell_count() as usize, surface.cells().len());
    assert_eq!(snapshot.sediment_ocean_delivery_m3(), 0.0);
    assert!(snapshot.sediment_endorheic_storage_m3() > 0.0);
    assert_eq!(
        snapshot.sediment_terminal_transfer_m3(),
        snapshot.sediment_ocean_delivery_m3() + snapshot.sediment_endorheic_storage_m3()
    );

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: SphericalSurfaceProcessSnapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, snapshot);
    decoded.validate_against(&surface, &relief).unwrap();
}

#[test]
fn spherical_surface_process_rejects_wrong_identity_mass_and_ocean_processes() {
    let authoritative = surface(6_371_000.0);
    let other = surface(6_372_000.0);
    let base_relief = relief(&authoritative, vec![1_000.0; authoritative.cells().len()]);
    let snapshot = valid_surface_process(&authoritative, &base_relief);
    assert!(snapshot.validate_against(&other, &base_relief).is_err());

    let mut mass_drift = serde_json::to_value(&snapshot).unwrap();
    mass_drift["sediment_endorheic_storage_m3"] = serde_json::json!(0.0);
    let mass_drift: SphericalSurfaceProcessSnapshot = serde_json::from_value(mass_drift).unwrap();
    assert!(mass_drift
        .validate_against(&authoritative, &base_relief)
        .is_err());

    let mut elevations = vec![1_000.0; authoritative.cells().len()];
    elevations[0] = -100.0;
    let ocean_relief = relief(&authoritative, elevations);
    let ocean_process = valid_surface_process(&authoritative, &ocean_relief);
    assert!(matches!(
        ocean_process.validate_against(&authoritative, &ocean_relief),
        Err(error) if error.to_string().contains("ocean cell")
    ));
}

#[test]
fn spherical_surface_process_wire_is_strict_bounded_and_finite() {
    let surface = surface(6_371_000.0);
    let relief = relief(&surface, vec![1_000.0; surface.cells().len()]);
    let snapshot = valid_surface_process(&surface, &relief);
    let value = serde_json::to_value(snapshot).unwrap();

    let mut unknown = value.clone();
    unknown["elapsed_years"] = serde_json::json!(1_000_000);
    assert!(serde_json::from_value::<SphericalSurfaceProcessSnapshot>(unknown).is_err());

    let mut oversized = value.clone();
    oversized["surface_ref"]["cell_count"] = serde_json::json!(MAX_SPHERICAL_CELL_COUNT + 1);
    assert!(serde_json::from_value::<SphericalSurfaceProcessSnapshot>(oversized).is_err());

    let mut non_finite = serde_json::to_string(&value).unwrap();
    let start = non_finite.find("\"sediment_ocean_delivery_m3\":").unwrap()
        + "\"sediment_ocean_delivery_m3\":".len();
    let end = non_finite[start..]
        .find(',')
        .map(|offset| start + offset)
        .unwrap();
    non_finite.replace_range(start..end, "1e999");
    assert!(serde_json::from_str::<SphericalSurfaceProcessSnapshot>(&non_finite).is_err());
}

#[test]
fn spherical_surface_process_constructor_rejects_dense_length_mismatch() {
    let surface = surface(6_371_000.0);
    let count = surface.cells().len();
    let error = SphericalSurfaceProcessSnapshot::new(
        SURFACE_PROCESS_SCHEMA_V2,
        SurfaceRef::try_for_spherical(&surface).unwrap(),
        vec![0.0; count - 1],
        vec![0.0; count],
        ElevationField::from_values(vec![100.0; count]).unwrap(),
        vec![0.0; count],
        0.0,
        0.0,
    )
    .unwrap_err();
    assert!(error.to_string().contains("erosion_depth_m"));
}

fn composite_inputs(
    surface: &SphericalSurfaceSnapshot,
) -> (
    SphericalReliefSnapshot,
    SphericalGeologicSnapshot,
    SphericalPreliminaryClimateSnapshot,
    SphericalHydroErosionSnapshot,
) {
    let count = surface.cells().len();
    let relief = relief(surface, vec![1_000.0; count]);
    let surface_ref = SurfaceRef::try_for_spherical(surface).unwrap();
    let geology = SphericalGeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V2,
        surface_ref,
        BedrockKindField::from_kinds(vec![BedrockKind::ContinentalCrystalline; count]),
        vec![0.0; count],
        vec![0.5; count],
        vec![0.25; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap();
    let latitude = surface
        .cells()
        .iter()
        .map(|cell| cell.centroid.components()[2].asin().to_degrees() as f32)
        .collect();
    let precipitation = 100.0;
    let climate = SphericalPreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V2,
        surface_ref,
        latitude,
        vec![0.0; count],
        MonthlyScalarField::from_values(vec![[20.0; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        MonthlyScalarField::from_values(vec![[precipitation; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        MonthlyVector3Field::from_values(vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        vec![20.0; count],
        vec![0.0; count],
        vec![precipitation * CLIMATE_MONTH_COUNT as f32; count],
        vec![[0.0; 3]; count],
    )
    .unwrap();
    let spec = HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: 1,
        erosion_strength_permille: 0,
        ..HydroErosionSpec::default()
    };
    let hydrology =
        HydrologyGenerator::generate_spherical(surface, &relief, &geology, &climate, &spec)
            .unwrap();
    let process =
        FluvialErosionGenerator::generate_spherical(surface, &relief, &geology, &hydrology, &spec)
            .unwrap();
    let composite =
        SphericalHydroErosionSnapshot::new(HYDRO_EROSION_SNAPSHOT_SCHEMA_V2, process, hydrology)
            .unwrap();
    (relief, geology, climate, composite)
}

#[test]
fn spherical_hydro_erosion_composite_round_trips_and_cross_validates() {
    let surface = surface(6_371_000.0);
    let (relief, geology, climate, snapshot) = composite_inputs(&surface);

    snapshot
        .validate_against(&surface, &relief, &geology, &climate)
        .unwrap();
    assert_eq!(snapshot.schema_version(), HYDRO_EROSION_SNAPSHOT_SCHEMA_V2);
    assert_eq!(
        snapshot.surface_ref(),
        SurfaceRef::try_for_spherical(&surface).unwrap()
    );
    assert_eq!(snapshot.cell_count() as usize, surface.cells().len());
    assert_eq!(
        snapshot.surface().surface_ref(),
        snapshot.hydrology().surface_ref()
    );

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: SphericalHydroErosionSnapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, snapshot);
    decoded
        .validate_against(&surface, &relief, &geology, &climate)
        .unwrap();
}

#[test]
fn spherical_hydro_erosion_composite_rejects_mixed_surfaces_and_unknown_fields() {
    let first_surface = surface(6_371_000.0);
    let second_surface = surface(6_372_000.0);
    let (_, _, _, first) = composite_inputs(&first_surface);
    let (_, _, _, second) = composite_inputs(&second_surface);

    assert!(SphericalHydroErosionSnapshot::new(
        HYDRO_EROSION_SNAPSHOT_SCHEMA_V2,
        first.surface().clone(),
        second.hydrology().clone(),
    )
    .is_err());

    let mut unknown = serde_json::to_value(first).unwrap();
    unknown["initial_hydrology"] = unknown["hydrology"].clone();
    assert!(serde_json::from_value::<SphericalHydroErosionSnapshot>(unknown).is_err());
}
