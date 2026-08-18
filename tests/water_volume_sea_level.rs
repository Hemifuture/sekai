use sekai::world::natural::{
    scaled_earth_ocean_inventory_m3, solve_physical_sea_level,
    solve_physical_sea_level_cancellable, water_volume_at_sea_level_m3, WaterVolumeSolveError,
    EARTH_OCEAN_VOLUME_M3,
};

#[test]
fn piecewise_linear_solver_closes_below_the_second_cell() {
    let solution = solve_physical_sea_level(&[0.0, 10.0], &[2.0, 1.0], 10.0).unwrap();

    assert_eq!(solution.sea_level_m(), 5.0);
    assert_eq!(solution.realized_water_volume_m3(), 10.0);
    assert_eq!(solution.relative_error(), 0.0);
}

#[test]
fn piecewise_linear_solver_crosses_breakpoints_without_percentiles() {
    let solution = solve_physical_sea_level(&[0.0, 10.0], &[2.0, 1.0], 30.0).unwrap();

    assert!((solution.sea_level_m() - 13.333_333).abs() < 1.0e-5);
    assert!(solution.relative_error() <= 1.0e-6);
    assert_eq!(
        solution.realized_water_volume_m3(),
        water_volume_at_sea_level_m3(&[0.0, 10.0], &[2.0, 1.0], solution.sea_level_m()).unwrap()
    );
}

#[test]
fn stable_cell_ties_and_zero_inventory_have_a_defined_solution() {
    let solution = solve_physical_sea_level(&[-20.0, -20.0, 5.0], &[3.0, 2.0, 1.0], 0.0).unwrap();
    assert_eq!(solution.sea_level_m(), -20.0);
    assert_eq!(solution.realized_water_volume_m3(), 0.0);
}

#[test]
fn cancellable_solver_is_bit_identical_to_the_frozen_water_operator() {
    let elevations = [12.0, -20.0, 3.0, -20.0, 80.0, 3.0];
    let areas = [4.0, 3.0, 7.0, 2.0, 9.0, 5.0];
    for inventory in [0.0, 10.0, 100.0, 10_000.0] {
        let frozen = solve_physical_sea_level(&elevations, &areas, inventory).unwrap();
        let cancellable =
            solve_physical_sea_level_cancellable(&elevations, &areas, inventory, &|| false)
                .unwrap();
        assert_eq!(
            cancellable.sea_level_m().to_bits(),
            frozen.sea_level_m().to_bits()
        );
        assert_eq!(
            cancellable.realized_water_volume_m3().to_bits(),
            frozen.realized_water_volume_m3().to_bits()
        );
        assert_eq!(
            cancellable.relative_error().to_bits(),
            frozen.relative_error().to_bits()
        );
    }
    assert_eq!(
        solve_physical_sea_level_cancellable(&elevations, &areas, 10.0, &|| true).unwrap_err(),
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
fn solver_rejects_malformed_or_impossible_inputs() {
    assert_eq!(
        solve_physical_sea_level(&[], &[], 1.0).unwrap_err(),
        WaterVolumeSolveError::EmptySurface
    );
    assert!(solve_physical_sea_level(&[0.0], &[], 1.0).is_err());
    assert!(solve_physical_sea_level(&[f32::NAN], &[1.0], 1.0).is_err());
    assert!(solve_physical_sea_level(&[0.0], &[0.0], 1.0).is_err());
    assert!(solve_physical_sea_level(&[0.0], &[1.0], -1.0).is_err());
}
