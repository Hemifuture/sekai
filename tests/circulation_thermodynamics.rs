mod support;

use std::collections::BTreeMap;

use sekai::generators::natural::circulation::{
    advance_thermodynamics, build_fixture, saturation_specific_humidity, thermodynamic_tendencies,
    CirculationEdgePermeability, CirculationFixture, CirculationOperators, CubedSphereGrid,
    ThermodynamicState,
};
use support::circulation::uniform_fixture;

fn zero_velocity(grid: &CubedSphereGrid) -> Vec<[f32; 3]> {
    vec![[0.0; 3]; grid.cell_count()]
}

fn solid_rotation(grid: &CubedSphereGrid, speed_scale: f32) -> Vec<[f32; 3]> {
    grid.cells()
        .iter()
        .map(|cell| {
            let r = cell.center_unit();
            [-r[1] as f32 * speed_scale, r[0] as f32 * speed_scale, 0.0]
        })
        .collect()
}

fn total_moisture(grid: &CubedSphereGrid, state: &ThermodynamicState) -> f64 {
    grid.cells()
        .iter()
        .zip(state.specific_humidity())
        .map(|(cell, humidity)| cell.area_m2() * f64::from(*humidity))
        .sum()
}

#[test]
fn aqua_forcing_is_oceanic_axisymmetric_and_seasonally_reverses_hemispheres() {
    let grid = CubedSphereGrid::new(12, 6_371_000.0).unwrap();
    let forcing = build_fixture(&grid, CirculationFixture::AquaPlanet).unwrap();

    assert!(forcing.land_fraction().iter().all(|value| *value == 0.0));
    let mut latitude_bands = BTreeMap::<i64, Vec<f32>>::new();
    for (cell, months) in grid
        .cells()
        .iter()
        .zip(forcing.equilibrium_air_temperature_c())
    {
        latitude_bands
            .entry((cell.center_unit()[2] * 1.0e10).round() as i64)
            .or_default()
            .push(months[2]);
    }
    for values in latitude_bands.values() {
        let minimum = values.iter().copied().fold(f32::INFINITY, f32::min);
        let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(maximum - minimum < 1.0e-4);
    }

    let north = grid
        .cells()
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.center_unit()[2].total_cmp(&b.center_unit()[2]))
        .unwrap()
        .0;
    let south = grid
        .cells()
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.center_unit()[2].total_cmp(&b.center_unit()[2]))
        .unwrap()
        .0;
    let temperature = forcing.equilibrium_air_temperature_c();
    assert!(temperature[north][5] > temperature[north][11]);
    assert!(temperature[south][5] < temperature[south][11]);
}

#[test]
fn fixture_kinds_are_deterministic_distinct_and_have_closed_land_coasts() {
    let grid = CubedSphereGrid::new(10, 6_371_000.0).unwrap();
    let aqua = build_fixture(&grid, CirculationFixture::AquaPlanet).unwrap();
    let basins = build_fixture(&grid, CirculationFixture::TwoBasins).unwrap();
    let harmonics = build_fixture(&grid, CirculationFixture::EarthLikeHarmonics).unwrap();
    assert_eq!(
        basins.fingerprint(),
        build_fixture(&grid, CirculationFixture::TwoBasins)
            .unwrap()
            .fingerprint()
    );
    assert_ne!(aqua.fingerprint(), basins.fingerprint());
    assert_ne!(basins.fingerprint(), harmonics.fingerprint());
    assert!(harmonics.land_fraction().contains(&0.0));
    assert!(harmonics.land_fraction().contains(&1.0));

    let permeability = CirculationEdgePermeability::from_forcing(&grid, &basins).unwrap();
    assert!(permeability.atmosphere().iter().all(|value| *value == 1.0));
    for (edge, value) in grid.edges().iter().zip(permeability.ocean()) {
        let [first, second] = edge.cells();
        if basins.land_fraction()[*first as usize] == 1.0
            || basins.land_fraction()[*second as usize] == 1.0
        {
            assert_eq!(*value, 0.0);
        }
    }
}

#[test]
fn warmer_air_has_greater_tetens_saturation_humidity() {
    let cold = saturation_specific_humidity(-10.0).unwrap();
    let mild = saturation_specific_humidity(15.0).unwrap();
    let warm = saturation_specific_humidity(30.0).unwrap();
    assert!(cold > 0.0);
    assert!(cold < mild && mild < warm);
}

#[test]
fn equilibrium_with_zero_velocity_has_zero_thermodynamic_tendencies() {
    let (grid, forcing, spec) = uniform_fixture(8);
    let operators = CirculationOperators::new(&grid);
    let permeability = CirculationEdgePermeability::from_forcing(&grid, &forcing).unwrap();
    let state = ThermodynamicState::from_forcing(&grid, &forcing, 0).unwrap();
    let zero = zero_velocity(&grid);
    let tendencies = thermodynamic_tendencies(
        &operators,
        &forcing,
        &spec,
        &state,
        &zero,
        &zero,
        &permeability,
        0,
        3_600.0,
    )
    .unwrap();

    assert!(tendencies
        .air_temperature_c_per_s()
        .iter()
        .all(|value| value.abs() < 1.0e-12));
    assert!(tendencies
        .surface_temperature_c_per_s()
        .iter()
        .all(|value| value.abs() < 1.0e-12));
    assert!(tendencies
        .specific_humidity_per_s()
        .iter()
        .all(|value| value.abs() < 1.0e-12));
    assert!(tendencies
        .precipitation_mm_day()
        .iter()
        .all(|value| *value == 0.0));
}

#[test]
fn ocean_evaporation_source_exceeds_dry_land_source() {
    let spec = sekai::world::natural::CirculationSpec {
        face_resolution: 10,
        ..sekai::world::natural::CirculationSpec::default()
    };
    let grid = CubedSphereGrid::new(10, spec.planet_radius_m).unwrap();
    let forcing = build_fixture(&grid, CirculationFixture::TwoBasins).unwrap();
    let equilibrium = ThermodynamicState::from_forcing(&grid, &forcing, 0).unwrap();
    let state = ThermodynamicState::new(
        equilibrium.air_temperature_c().to_vec(),
        equilibrium.surface_temperature_c().to_vec(),
        vec![0.0; grid.cell_count()],
    )
    .unwrap();
    let operators = CirculationOperators::new(&grid);
    let permeability = CirculationEdgePermeability::from_forcing(&grid, &forcing).unwrap();
    let zero = zero_velocity(&grid);
    let tendencies = thermodynamic_tendencies(
        &operators,
        &forcing,
        &spec,
        &state,
        &zero,
        &zero,
        &permeability,
        0,
        3_600.0,
    )
    .unwrap();
    let ocean = forcing
        .land_fraction()
        .iter()
        .position(|value| *value == 0.0)
        .unwrap();
    let land = forcing
        .land_fraction()
        .iter()
        .position(|value| *value == 1.0)
        .unwrap();
    assert!(
        tendencies.specific_humidity_per_s()[ocean] > tendencies.specific_humidity_per_s()[land]
    );
}

#[test]
fn unsaturated_excess_humidity_relaxes_toward_the_surface_target() {
    let (grid, forcing, spec) = uniform_fixture(8);
    let equilibrium = ThermodynamicState::from_forcing(&grid, &forcing, 0).unwrap();
    let state = ThermodynamicState::new(
        equilibrium.air_temperature_c().to_vec(),
        equilibrium.surface_temperature_c().to_vec(),
        vec![0.006; grid.cell_count()],
    )
    .unwrap();
    assert!(0.006 < saturation_specific_humidity(15.0).unwrap());
    let operators = CirculationOperators::new(&grid);
    let permeability = CirculationEdgePermeability::from_forcing(&grid, &forcing).unwrap();
    let zero = zero_velocity(&grid);
    let tendencies = thermodynamic_tendencies(
        &operators,
        &forcing,
        &spec,
        &state,
        &zero,
        &zero,
        &permeability,
        0,
        3_600.0,
    )
    .unwrap();

    assert!(tendencies
        .specific_humidity_per_s()
        .iter()
        .all(|value| *value < 0.0));
    assert!(tendencies
        .precipitation_mm_day()
        .iter()
        .all(|value| *value == 0.0));
}

#[test]
fn noncondensing_transport_preserves_global_moisture() {
    let spec = sekai::world::natural::CirculationSpec {
        face_resolution: 10,
        ..sekai::world::natural::CirculationSpec::default()
    };
    let grid = CubedSphereGrid::new(10, spec.planet_radius_m).unwrap();
    let forcing = build_fixture(&grid, CirculationFixture::AquaPlanet).unwrap();
    let state = ThermodynamicState::from_forcing(&grid, &forcing, 2).unwrap();
    let operators = CirculationOperators::new(&grid);
    let permeability = CirculationEdgePermeability::from_forcing(&grid, &forcing).unwrap();
    let wind = solid_rotation(&grid, 10.0);
    let current = zero_velocity(&grid);
    let tendencies = thermodynamic_tendencies(
        &operators,
        &forcing,
        &spec,
        &state,
        &wind,
        &current,
        &permeability,
        2,
        3_600.0,
    )
    .unwrap();
    assert!(tendencies.relative_moisture_transport_error() < 1.0e-6);
    assert!(tendencies
        .precipitation_mm_day()
        .iter()
        .all(|value| *value == 0.0));

    let advanced = advance_thermodynamics(&state, &tendencies, 3_600.0).unwrap();
    let before = total_moisture(&grid, &state);
    let after = total_moisture(&grid, &advanced);
    assert!((after - before).abs() / before.abs() < 1.0e-6);
}
