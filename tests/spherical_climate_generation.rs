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
}
