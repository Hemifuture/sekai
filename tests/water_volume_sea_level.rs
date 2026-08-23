use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    solve_physical_sea_level, solve_physical_sea_level_cancellable, water_volume_at_sea_level_m3,
};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    scaled_earth_ocean_inventory_m3, WaterVolumeSolveError, EARTH_OCEAN_VOLUME_M3,
    WATER_VOLUME_RELATIVE_TOLERANCE,
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
fn constant_surface_has_an_exact_sea_level_and_closes_volume() {
    let surface = surface();
    let elevation = vec![10.0; surface.cells().len()];
    let inventory = surface.total_cell_area().get() * 5.0;
    let solution = solve_physical_sea_level(&surface, &elevation, inventory).unwrap();

    assert_eq!(solution.sea_level_m(), 15.0);
    assert!(solution.relative_error() <= WATER_VOLUME_RELATIVE_TOLERANCE);
    assert_eq!(
        solution.realized_water_volume_m3().to_bits(),
        water_volume_at_sea_level_m3(&surface, &elevation, solution.sea_level_m())
            .unwrap()
            .to_bits()
    );
}

#[test]
fn zero_inventory_publishes_the_lowest_center_level_without_water() {
    let surface = surface();
    let elevation = signed_x_elevation(&surface);
    let minimum = elevation.iter().copied().min_by(f32::total_cmp).unwrap();
    let solution = solve_physical_sea_level(&surface, &elevation, 0.0).unwrap();

    assert_eq!(solution.sea_level_m().to_bits(), minimum.to_bits());
    assert_eq!(solution.realized_water_volume_m3(), 0.0);
}

#[test]
fn cancellable_solver_is_bit_identical_and_observes_cancellation() {
    let surface = surface();
    let elevation = signed_x_elevation(&surface);
    let area = surface.total_cell_area().get();
    for inventory in [0.0, area * 10.0, area * 100.0] {
        let direct = solve_physical_sea_level(&surface, &elevation, inventory).unwrap();
        let cancellable = solve_physical_sea_level_cancellable(
            &surface,
            &elevation,
            inventory,
            &BuildCancellation::new(),
        )
        .unwrap();
        assert_eq!(
            cancellable.sea_level_m().to_bits(),
            direct.sea_level_m().to_bits()
        );
        assert_eq!(
            cancellable.realized_water_volume_m3().to_bits(),
            direct.realized_water_volume_m3().to_bits()
        );
        assert_eq!(
            cancellable.relative_error().to_bits(),
            direct.relative_error().to_bits()
        );
    }

    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    assert_eq!(
        solve_physical_sea_level_cancellable(&surface, &elevation, area, &cancellation)
            .unwrap_err(),
        WaterVolumeSolveError::Cancelled
    );
}

#[test]
fn earth_area_scaling_preserves_the_locked_reference_inventory() {
    let reference_area = 4.0 * std::f64::consts::PI * 6_371_000.0_f64.powi(2);
    assert_eq!(
        scaled_earth_ocean_inventory_m3(reference_area).unwrap(),
        EARTH_OCEAN_VOLUME_M3
    );
    assert_eq!(
        scaled_earth_ocean_inventory_m3(reference_area * 0.25).unwrap(),
        EARTH_OCEAN_VOLUME_M3 * 0.25
    );
}

#[test]
fn solver_rejects_malformed_inputs() {
    let surface = surface();
    let elevation = vec![0.0; surface.cells().len()];
    assert!(solve_physical_sea_level(&surface, &elevation[..elevation.len() - 1], 1.0).is_err());
    let mut non_finite = elevation.clone();
    non_finite[0] = f32::NAN;
    assert!(solve_physical_sea_level(&surface, &non_finite, 1.0).is_err());
    assert!(solve_physical_sea_level(&surface, &elevation, -1.0).is_err());
}
