use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    build_surface_water_geometry, solve_physical_sea_level, water_volume_at_sea_level_m3,
};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    LandOceanKind, SurfaceWaterGeometry, SurfaceWaterGeometryValidationError, WaterVolumeSolveError,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, SphericalSpaceSpec};

fn surface() -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(1_000.0).unwrap(),
        target_cell_count: 42,
    })
    .unwrap()
}

fn signed_x_elevation(surface: &SphericalSurfaceSnapshot) -> Vec<f32> {
    surface
        .cells()
        .iter()
        .map(|cell| (100.0 * cell.centroid.components()[0]) as f32)
        .collect()
}

#[test]
fn constant_surface_degenerates_to_exact_all_land_and_all_ocean() {
    let surface = surface();
    let elevation = vec![10.0; surface.cells().len()];
    let cancellation = BuildCancellation::new();

    for sea_level_m in [5.0, 10.0] {
        let dry =
            build_surface_water_geometry(&surface, &elevation, sea_level_m, &cancellation).unwrap();
        assert!(dry.ocean_area_fraction().iter().all(|value| *value == 0.0));
        assert!(dry.wet_edge_fraction().iter().all(|value| *value == 0.0));
        assert!(dry.cell_water_volume_m3().iter().all(|value| *value == 0.0));
        assert_eq!(dry.total_water_volume_m3(), 0.0);
        assert!((0..dry.land_ocean().len())
            .all(|index| dry.land_ocean().get(index) == Some(LandOceanKind::Land)));
    }

    let wet = build_surface_water_geometry(&surface, &elevation, 15.0, &cancellation).unwrap();
    assert!(wet.ocean_area_fraction().iter().all(|value| *value == 1.0));
    assert!(wet.wet_edge_fraction().iter().all(|value| *value == 1.0));
    assert!(surface.cells().iter().all(|cell| {
        wet.mean_wet_depth_m(&surface, cell.id)
            .is_some_and(|depth| depth.to_bits() == 5.0_f32.to_bits())
    }));
    let expected = surface.total_cell_area().get() * 5.0;
    assert!((wet.total_water_volume_m3() - expected).abs() <= 1.0e-12 * expected);
    assert!((0..wet.land_ocean().len())
        .all(|index| wet.land_ocean().get(index) == Some(LandOceanKind::Ocean)));
}

#[test]
fn fractional_geometry_is_bounded_complementary_and_shared_by_both_edge_owners() {
    let surface = surface();
    let elevation = signed_x_elevation(&surface);
    let geometry =
        build_surface_water_geometry(&surface, &elevation, 0.0, &BuildCancellation::new()).unwrap();

    assert!(geometry
        .ocean_area_fraction()
        .iter()
        .any(|fraction| *fraction > 0.0 && *fraction < 1.0));
    for (index, ocean) in geometry.ocean_area_fraction().iter().copied().enumerate() {
        assert!((0.0..=1.0).contains(&ocean));
        assert_eq!(geometry.land_area_fraction(index).unwrap() + ocean, 1.0);
    }
    for edge in surface.edges() {
        let first = geometry
            .wet_fraction_for_cell_edge(&surface, edge.cells[0], edge.id)
            .unwrap();
        let second = geometry
            .wet_fraction_for_cell_edge(&surface, edge.cells[1], edge.id)
            .unwrap();
        assert_eq!(first.to_bits(), second.to_bits());
    }
}

#[test]
fn p1_volume_is_continuous_monotone_and_inverts_the_same_geometry_operator() {
    let surface = surface();
    let elevation = signed_x_elevation(&surface);
    let levels = [-25.0_f32, -0.125, 0.0, 0.125, 25.0];
    let volumes = levels
        .iter()
        .map(|level| water_volume_at_sea_level_m3(&surface, &elevation, *level).unwrap())
        .collect::<Vec<_>>();
    assert!(volumes.windows(2).all(|pair| pair[0] < pair[1]));

    let target_level = 12.5_f32;
    let inventory = water_volume_at_sea_level_m3(&surface, &elevation, target_level).unwrap();
    let solution = solve_physical_sea_level(&surface, &elevation, inventory).unwrap();
    assert_eq!(solution.sea_level_m().to_bits(), target_level.to_bits());
    assert_eq!(
        solution.realized_water_volume_m3().to_bits(),
        inventory.to_bits()
    );
    assert!(solution.relative_error() <= 1.0e-6);
    assert_eq!(
        solution.geometry().total_water_volume_m3().to_bits(),
        inventory.to_bits()
    );
}

#[test]
fn geometry_rejects_cancellation_bad_fields_and_invalid_topology() {
    let surface = surface();
    let elevation = vec![0.0; surface.cells().len()];
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    assert_eq!(
        build_surface_water_geometry(&surface, &elevation, 0.0, &cancellation).unwrap_err(),
        WaterVolumeSolveError::Cancelled
    );

    assert!(build_surface_water_geometry(
        &surface,
        &elevation[..elevation.len() - 1],
        0.0,
        &BuildCancellation::new(),
    )
    .is_err());
    let mut non_finite = elevation.clone();
    non_finite[0] = f32::NAN;
    assert!(
        build_surface_water_geometry(&surface, &non_finite, 0.0, &BuildCancellation::new(),)
            .is_err()
    );
    assert!(build_surface_water_geometry(
        &surface,
        &elevation,
        f32::NAN,
        &BuildCancellation::new(),
    )
    .is_err());

    let mut wire = serde_json::to_value(&surface).unwrap();
    wire["cells"][0]["boundary_vertices"][0] = wire["cells"][0]["boundary_vertices"][1].clone();
    let invalid: SphericalSurfaceSnapshot = serde_json::from_value(wire).unwrap();
    assert!(matches!(
        build_surface_water_geometry(&invalid, &elevation, 0.0, &BuildCancellation::new(),),
        Err(WaterVolumeSolveError::InvalidSurface(_))
    ));
}

#[test]
fn geometry_wire_is_strict_fingerprinted_and_bound_to_elevations() {
    let surface = surface();
    let elevation = signed_x_elevation(&surface);
    let geometry =
        build_surface_water_geometry(&surface, &elevation, 0.0, &BuildCancellation::new()).unwrap();

    let encoded = serde_json::to_value(&geometry).unwrap();
    let decoded: SurfaceWaterGeometry = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, geometry);
    decoded.validate_against(&surface, &elevation).unwrap();

    let mut unknown = encoded.clone();
    unknown["surprise"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SurfaceWaterGeometry>(unknown).is_err());

    let mut fingerprint = encoded;
    fingerprint["fingerprint"][0] =
        serde_json::json!(fingerprint["fingerprint"][0].as_u64().unwrap() as u8 ^ 1);
    assert!(serde_json::from_value::<SurfaceWaterGeometry>(fingerprint).is_err());

    let mut stale_elevation = elevation;
    stale_elevation[0] = f32::from_bits(stale_elevation[0].to_bits() + 1);
    assert_eq!(
        geometry
            .validate_against(&surface, &stale_elevation)
            .unwrap_err(),
        SurfaceWaterGeometryValidationError::ElevationFingerprintMismatch
    );
}
