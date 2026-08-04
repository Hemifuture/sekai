use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ElevationField, HydroErosionSpec, HydrologySnapshot, SphericalHydrologySnapshot,
    StrahlerOrderField, SurfaceWaterField, SurfaceWaterKind, CLIMATE_MONTH_COUNT,
    HYDROLOGY_SCHEMA_V1, HYDROLOGY_SCHEMA_V2,
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

fn all_ocean_snapshot(surface: &SphericalSurfaceSnapshot) -> SphericalHydrologySnapshot {
    let count = surface.cells().len();
    let spec = HydroErosionSpec::default();
    let hydrology = HydrologySnapshot::new(
        HYDROLOGY_SCHEMA_V1,
        count as u32,
        spec.river_discharge_threshold_m3_s(),
        spec.minimum_lake_depth_m(),
        vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        vec![0.0; count],
        vec![0.0; count],
        surface
            .cells()
            .iter()
            .map(|cell| (cell.area.get() / 1_000_000.0) as f32)
            .collect(),
        ElevationField::from_values(vec![-100.0; count]).unwrap(),
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
        SurfaceRef::try_for_spherical(surface).unwrap(),
        hydrology,
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn spherical_hydrology_round_trips_with_exact_identity_and_borrowed_semantics() {
    let surface = surface(6_371_000.0);
    let snapshot = all_ocean_snapshot(&surface);

    snapshot.validate_against(&surface).unwrap();
    assert_eq!(snapshot.schema_version(), HYDROLOGY_SCHEMA_V2);
    assert_eq!(
        snapshot.surface_ref(),
        SurfaceRef::try_for_spherical(&surface).unwrap()
    );
    assert_eq!(snapshot.cell_count() as usize, surface.cells().len());
    assert_eq!(
        snapshot.surface_water().raw_values().as_ptr(),
        snapshot.surface_water().raw_values().as_ptr()
    );
    assert!(snapshot.river_segment_length_m().is_empty());

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: SphericalHydrologySnapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, snapshot);
    decoded.validate_against(&surface).unwrap();
}

#[test]
fn spherical_hydrology_rejects_wrong_surface_and_metric_cardinality() {
    let authoritative = surface(6_371_000.0);
    let other = surface(6_372_000.0);
    let snapshot = all_ocean_snapshot(&authoritative);

    assert!(snapshot.validate_against(&other).is_err());
    assert!(SphericalHydrologySnapshot::new(
        HYDROLOGY_SCHEMA_V2,
        snapshot.surface_ref(),
        HydrologySnapshot::new(
            HYDROLOGY_SCHEMA_V1,
            snapshot.cell_count(),
            snapshot.river_discharge_threshold_m3_s(),
            snapshot.minimum_lake_depth_m(),
            snapshot.monthly_local_runoff_mm().to_vec(),
            snapshot.monthly_discharge_m3_s().to_vec(),
            snapshot.annual_local_runoff_mm().to_vec(),
            snapshot.mean_annual_discharge_m3_s().to_vec(),
            snapshot.drainage_area_km2().to_vec(),
            snapshot.drainage_surface_elevation_m().clone(),
            snapshot.lake_depth_m().to_vec(),
            snapshot.surface_water().clone(),
            snapshot.flow_receiver().to_vec(),
            snapshot.basin_id().to_vec(),
            snapshot.strahler_order().clone(),
            snapshot.basins().to_vec(),
            snapshot.lakes().to_vec(),
            snapshot.river_segments().to_vec(),
        )
        .unwrap(),
        vec![1.0],
    )
    .is_err());
}

#[test]
fn spherical_hydrology_wire_is_strict_and_surface_budgeted() {
    let surface = surface(6_371_000.0);
    let snapshot = all_ocean_snapshot(&surface);
    let value = serde_json::to_value(snapshot).unwrap();

    let mut unknown_top = value.clone();
    unknown_top["projection"] = serde_json::json!("equirectangular");
    assert!(serde_json::from_value::<SphericalHydrologySnapshot>(unknown_top).is_err());

    let mut unknown_payload = value.clone();
    unknown_payload["hydrology"]["external_boundary"] = serde_json::json!(0);
    assert!(serde_json::from_value::<SphericalHydrologySnapshot>(unknown_payload).is_err());

    let mut unknown_record = value.clone();
    unknown_record["hydrology"]["basins"] = serde_json::json!([{
        "id": 0,
        "outlet_cell": 0,
        "outlet_kind": "ClosedSink",
        "area_km2": 1.0,
        "mean_discharge_m3_s": 0.0,
        "external_boundary": true
    }]);
    assert!(serde_json::from_value::<SphericalHydrologySnapshot>(unknown_record).is_err());

    let mut oversized_identity = value;
    oversized_identity["surface_ref"]["cell_count"] =
        serde_json::json!(MAX_SPHERICAL_CELL_COUNT + 1);
    assert!(serde_json::from_value::<SphericalHydrologySnapshot>(oversized_identity).is_err());
}
