use sekai::generators::natural::{ClimateGenerator, SphericalClimateGenerationError};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ClimateSpec, ElevationField, LandOceanField, SphericalReliefSnapshot, RELIEF_SCHEMA_V4,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

const EARTH_RADIUS_M: f64 = 6_371_000.0;

fn surface(target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(EARTH_RADIUS_M).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn relief(
    surface: &SphericalSurfaceSnapshot,
    elevation: impl Fn(CellId) -> f32,
) -> SphericalReliefSnapshot {
    let values = surface
        .cells()
        .iter()
        .map(|cell| elevation(cell.id))
        .collect::<Vec<_>>();
    let zero = vec![0.0; values.len()];
    let final_elevation = ElevationField::from_values(values.clone()).unwrap();
    SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::for_spherical(surface),
        0.0,
        ElevationField::from_values(values).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero).unwrap(),
        final_elevation.clone(),
        LandOceanField::classify(&final_elevation, 0.0),
    )
    .unwrap()
}

fn nearest_latitude(surface: &SphericalSurfaceSnapshot, target_degrees: f64) -> usize {
    surface
        .cells()
        .iter()
        .enumerate()
        .min_by(|(_, first), (_, second)| {
            let first_error =
                (first.centroid.components()[2].asin().to_degrees() - target_degrees).abs();
            let second_error =
                (second.centroid.components()[2].asin().to_degrees() - target_degrees).abs();
            first_error.total_cmp(&second_error)
        })
        .unwrap()
        .0
}

fn dot(first: [f64; 3], second: [f32; 3]) -> f64 {
    first[0] * f64::from(second[0])
        + first[1] * f64::from(second[1])
        + first[2] * f64::from(second[2])
}

fn east_component(radial: [f64; 3], wind: [f32; 3]) -> f64 {
    let horizontal = radial[0].hypot(radial[1]);
    if horizontal <= f64::EPSILON {
        0.0
    } else {
        dot([-radial[1] / horizontal, radial[0] / horizontal, 0.0], wind)
    }
}

fn graph_distances(surface: &SphericalSurfaceSnapshot, source: CellId) -> Vec<f64> {
    let count = surface.cells().len();
    let mut adjacency = vec![Vec::new(); count];
    for edge in surface.edges() {
        let [first, second] = edge.cells;
        let distance = edge.center_distance.get();
        adjacency[first.raw() as usize].push((second.raw() as usize, distance));
        adjacency[second.raw() as usize].push((first.raw() as usize, distance));
    }

    let mut distances = vec![f64::INFINITY; count];
    let mut settled = vec![false; count];
    distances[source.raw() as usize] = 0.0;
    for _ in 0..count {
        let Some(current) = (0..count)
            .filter(|&index| !settled[index])
            .min_by(|&first, &second| distances[first].total_cmp(&distances[second]))
        else {
            break;
        };
        settled[current] = true;
        for &(neighbor, length) in &adjacency[current] {
            let candidate = distances[current] + length;
            if candidate < distances[neighbor] {
                distances[neighbor] = candidate;
            }
        }
    }
    distances
}

fn mean(values: impl Iterator<Item = f32>) -> f64 {
    let values = values.map(f64::from).collect::<Vec<_>>();
    values.iter().sum::<f64>() / values.len() as f64
}

#[test]
fn spherical_forcing_is_deterministic_surface_native_seasonal_and_tangent() {
    let sphere = surface(642);
    let land = relief(&sphere, |_| 250.0);
    let spec = ClimateSpec::default();

    let first = ClimateGenerator::generate_spherical(&sphere, &land, &spec).unwrap();
    let repeated = ClimateGenerator::generate_spherical(&sphere, &land, &spec).unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    first.validate_against(&sphere, &land).unwrap();

    let minimum_latitude = first
        .latitude_degrees()
        .iter()
        .copied()
        .fold(f32::INFINITY, f32::min);
    let maximum_latitude = first
        .latitude_degrees()
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(minimum_latitude < -75.0, "minimum={minimum_latitude}");
    assert!(maximum_latitude > 75.0, "maximum={maximum_latitude}");
    for (index, cell) in sphere.cells().iter().enumerate() {
        let expected = cell.centroid.components()[2].asin().to_degrees() as f32;
        assert!((first.latitude_degrees()[index] - expected).abs() <= 1.0e-5);
        for &wind in &first.monthly_wind_m_s().values()[index] {
            assert!(dot(cell.centroid.components(), wind).abs() <= 1.0e-4);
        }
    }

    let north = nearest_latitude(&sphere, 45.0);
    let south = nearest_latitude(&sphere, -45.0);
    let temperatures = first.monthly_air_temperature_c().values();
    assert!(temperatures[north][5] > temperatures[north][11]);
    assert!(temperatures[south][5] < temperatures[south][11]);

    let equator = nearest_latitude(&sphere, 0.0);
    let midlatitude = nearest_latitude(&sphere, 45.0);
    let migration_band = nearest_latitude(&sphere, 25.0);
    let winds = first.monthly_wind_m_s().values();
    let equator_radial = sphere.cells()[equator].centroid.components();
    let midlatitude_radial = sphere.cells()[midlatitude].centroid.components();
    let migration_radial = sphere.cells()[migration_band].centroid.components();
    assert!(east_component(equator_radial, winds[equator][2]) < 0.0);
    assert!(east_component(midlatitude_radial, winds[midlatitude][2]) > 0.0);
    assert!(
        east_component(migration_radial, winds[migration_band][5])
            < east_component(migration_radial, winds[migration_band][11])
    );
}

#[test]
fn spherical_maritime_influence_has_closed_surface_extremes_and_distance_decay() {
    let sphere = surface(162);
    let spec = ClimateSpec::default();
    let source = sphere
        .cells()
        .iter()
        .max_by(|first, second| {
            first.centroid.components()[0].total_cmp(&second.centroid.components()[0])
        })
        .unwrap()
        .id;

    let all_ocean = relief(&sphere, |_| -100.0);
    let ocean_climate = ClimateGenerator::generate_spherical(&sphere, &all_ocean, &spec).unwrap();
    assert!(ocean_climate
        .maritime_influence()
        .iter()
        .all(|&value| value == 1.0));

    let all_land = relief(&sphere, |_| 100.0);
    let land_climate = ClimateGenerator::generate_spherical(&sphere, &all_land, &spec).unwrap();
    assert!(land_climate
        .maritime_influence()
        .iter()
        .all(|&value| value == 0.0));
    let midlatitude = nearest_latitude(&sphere, 45.0);
    assert!(
        ocean_climate.temperature_seasonality_c()[midlatitude]
            < land_climate.temperature_seasonality_c()[midlatitude]
    );

    let one_ocean = relief(&sphere, |cell| if cell == source { -100.0 } else { 100.0 });
    let climate = ClimateGenerator::generate_spherical(&sphere, &one_ocean, &spec).unwrap();
    let maritime = climate.maritime_influence();
    assert_eq!(maritime[source.raw() as usize], 1.0);
    assert!(maritime.iter().all(|value| value.is_finite()));
    let distances = graph_distances(&sphere, source);
    for first in 0..distances.len() {
        for second in 0..distances.len() {
            if distances[first] + 100.0 < distances[second] {
                assert!(
                    maritime[first] + 1.0e-6 >= maritime[second],
                    "distance/maritime inversion: {first} ({}, {}) vs {second} ({}, {})",
                    distances[first],
                    maritime[first],
                    distances[second],
                    maritime[second]
                );
            }
        }
    }
}

#[test]
fn spherical_generation_rejects_wrong_surface_relief_before_work() {
    let sphere = surface(162);
    let other = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(EARTH_RADIUS_M + 1.0).unwrap(),
        target_cell_count: 162,
    })
    .unwrap();
    let wrong_relief = relief(&other, |_| 100.0);

    assert!(matches!(
        ClimateGenerator::generate_spherical(&sphere, &wrong_relief, &ClimateSpec::default()),
        Err(SphericalClimateGenerationError::InvalidRelief(_))
    ));

    let invalid_spec = ClimateSpec {
        schema_version: 0,
        ..ClimateSpec::default()
    };
    assert!(matches!(
        ClimateGenerator::generate_spherical(&sphere, &relief(&sphere, |_| 100.0), &invalid_spec),
        Err(SphericalClimateGenerationError::InvalidSpec(_))
    ));
}

#[test]
fn spherical_forcing_ignores_planar_extent_but_responds_to_temperature_offset() {
    let sphere = surface(162);
    let land = relief(&sphere, |_| 100.0);
    let default_spec = ClimateSpec::default();
    let alternate_planar_extent = ClimateSpec {
        south_latitude_centideg: -9_000,
        north_latitude_centideg: -8_000,
        ..default_spec.clone()
    };
    let default_climate =
        ClimateGenerator::generate_spherical(&sphere, &land, &default_spec).unwrap();
    let alternate_climate =
        ClimateGenerator::generate_spherical(&sphere, &land, &alternate_planar_extent).unwrap();
    assert_eq!(
        serde_json::to_vec(&default_climate).unwrap(),
        serde_json::to_vec(&alternate_climate).unwrap()
    );

    let cold_spec = ClimateSpec {
        temperature_offset_deci_c: -300,
        ..default_spec.clone()
    };
    let warm_spec = ClimateSpec {
        temperature_offset_deci_c: 300,
        ..default_spec
    };
    let cold = ClimateGenerator::generate_spherical(&sphere, &land, &cold_spec).unwrap();
    let warm = ClimateGenerator::generate_spherical(&sphere, &land, &warm_spec).unwrap();
    let equator = nearest_latitude(&sphere, 0.0);
    assert!(
        warm.mean_annual_air_temperature_c()[equator]
            > cold.mean_annual_air_temperature_c()[equator] + 50.0
    );
}

#[test]
fn explicit_ocean_supply_and_moisture_scale_control_spherical_precipitation() {
    let sphere = surface(162);
    let ocean = relief(&sphere, |_| -100.0);
    let land = relief(&sphere, |_| 100.0);
    let dry_spec = ClimateSpec {
        moisture_scale_permille: 250,
        ..ClimateSpec::default()
    };
    let wet_spec = ClimateSpec {
        moisture_scale_permille: 2_500,
        ..ClimateSpec::default()
    };

    let dry_ocean = ClimateGenerator::generate_spherical(&sphere, &ocean, &dry_spec).unwrap();
    let wet_ocean = ClimateGenerator::generate_spherical(&sphere, &ocean, &wet_spec).unwrap();
    let wet_land = ClimateGenerator::generate_spherical(&sphere, &land, &wet_spec).unwrap();
    let dry_mean = mean(dry_ocean.annual_precipitation_mm().iter().copied());
    let wet_mean = mean(wet_ocean.annual_precipitation_mm().iter().copied());
    let land_mean = mean(wet_land.annual_precipitation_mm().iter().copied());

    assert!(dry_mean > 0.0, "dry ocean precipitation={dry_mean}");
    assert!(wet_mean > dry_mean * 5.0, "dry={dry_mean}, wet={wet_mean}");
    assert!(wet_mean > land_mean, "ocean={wet_mean}, land={land_mean}");
}

#[test]
fn spherical_westerlies_create_windward_rain_and_a_downstream_shadow() {
    let sphere = surface(642);
    let terrain = relief(&sphere, |cell| {
        let radial = sphere.cell(cell).unwrap().centroid.components();
        let latitude = radial[2].asin().to_degrees();
        let longitude = radial[1].atan2(radial[0]).to_degrees();
        if (20.0..=60.0).contains(&latitude) && (-80.0..=-20.0).contains(&longitude) {
            -100.0
        } else if (25.0..=55.0).contains(&latitude) && (-8.0..=8.0).contains(&longitude) {
            3_000.0
        } else {
            200.0
        }
    });
    let climate =
        ClimateGenerator::generate_spherical(&sphere, &terrain, &ClimateSpec::default()).unwrap();
    let annual = climate.annual_precipitation_mm();
    let mut ridge = Vec::new();
    let mut leeward = Vec::new();
    for cell in sphere.cells() {
        let radial = cell.centroid.components();
        let latitude = radial[2].asin().to_degrees();
        let longitude = radial[1].atan2(radial[0]).to_degrees();
        if (30.0..=50.0).contains(&latitude) && (-8.0..=8.0).contains(&longitude) {
            ridge.push(annual[cell.id.raw() as usize]);
        } else if (30.0..=50.0).contains(&latitude) && (8.0..=30.0).contains(&longitude) {
            leeward.push(annual[cell.id.raw() as usize]);
        }
    }
    assert!(!ridge.is_empty() && !leeward.is_empty());
    let ridge_mean = mean(ridge.into_iter());
    let leeward_mean = mean(leeward.into_iter());
    assert!(
        ridge_mean > leeward_mean * 1.2,
        "ridge={ridge_mean}, leeward={leeward_mean}"
    );
}
