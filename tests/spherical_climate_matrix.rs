use sekai::generators::natural::ClimateGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ClimateSpec, ElevationField, LandOceanField, LandOceanKind, SphericalReliefSnapshot,
    RELIEF_SCHEMA_V4,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{Meters, SphericalSpaceSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerrainPattern {
    AllLand,
    AllOcean,
    Continents,
    MountainArc,
}

#[derive(Clone, Copy)]
struct MatrixCase {
    name: &'static str,
    radius_m: f64,
    target_cells: u32,
    terrain: TerrainPattern,
    axial_tilt_centideg: u16,
    temperature_offset_deci_c: i16,
    moisture_scale_permille: u16,
    expected_hash: &'static str,
}

const CASES: [MatrixCase; 4] = [
    MatrixCase {
        name: "minimum-radius-cold-land",
        radius_m: 1.0,
        target_cells: 42,
        terrain: TerrainPattern::AllLand,
        axial_tilt_centideg: 0,
        temperature_offset_deci_c: -300,
        moisture_scale_permille: 250,
        expected_hash: "f00d5b7e0768597c4f0112c69940ddd6378945a9a0acb5d7640cbf0a5bc3a6b6",
    },
    MatrixCase {
        name: "regional-high-tilt-ocean",
        radius_m: 1_000_000.0,
        target_cells: 92,
        terrain: TerrainPattern::AllOcean,
        axial_tilt_centideg: 6_000,
        temperature_offset_deci_c: 300,
        moisture_scale_permille: 2_500,
        expected_hash: "7835c9d0a6107c12220e3878b26a059afdf8ea7ab04ab955de856b2d6075b185",
    },
    MatrixCase {
        name: "earth-mixed-continents",
        radius_m: 6_371_000.0,
        target_cells: 162,
        terrain: TerrainPattern::Continents,
        axial_tilt_centideg: 2_340,
        temperature_offset_deci_c: 0,
        moisture_scale_permille: 1_000,
        expected_hash: "43f5ad23c89ee39cbe7142e6aab0f6b4d6003308f467e9055e0cdc37ea8fc532",
    },
    MatrixCase {
        name: "maximum-radius-mountain-arc",
        radius_m: 100_000_000.0,
        target_cells: 642,
        terrain: TerrainPattern::MountainArc,
        axial_tilt_centideg: 4_500,
        temperature_offset_deci_c: -100,
        moisture_scale_permille: 1_500,
        expected_hash: "aa8efb1b8d4ba180bb8faf451d013f732c7c86afb178fa1e90530dd1ef873821",
    },
];

fn surface(case: MatrixCase) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(case.radius_m).unwrap(),
        target_cell_count: case.target_cells,
    })
    .unwrap()
}

fn elevation(pattern: TerrainPattern, radial: [f64; 3]) -> f32 {
    match pattern {
        TerrainPattern::AllLand => 0.0,
        TerrainPattern::AllOcean => -100.0,
        TerrainPattern::Continents => {
            if radial[0] + radial[2] * 0.35 > 0.05 {
                (180.0 + radial[2].abs() as f32 * 900.0).min(1_100.0)
            } else {
                -2_800.0
            }
        }
        TerrainPattern::MountainArc => {
            if radial[0] + radial[2] * 0.35 <= -0.15 {
                -3_200.0
            } else if radial[0] > 0.82 && radial[2].abs() < 0.72 {
                3_200.0
            } else {
                240.0
            }
        }
    }
}

fn relief(surface: &SphericalSurfaceSnapshot, pattern: TerrainPattern) -> SphericalReliefSnapshot {
    let values = surface
        .cells()
        .iter()
        .map(|cell| elevation(pattern, cell.centroid.components()))
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

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

#[test]
fn spherical_climate_scientific_and_deterministic_matrix() {
    let mut actual_hashes = Vec::new();
    for case in CASES {
        let surface = surface(case);
        let relief = relief(&surface, case.terrain);
        let spec = ClimateSpec {
            axial_tilt_centideg: case.axial_tilt_centideg,
            temperature_offset_deci_c: case.temperature_offset_deci_c,
            moisture_scale_permille: case.moisture_scale_permille,
            ..ClimateSpec::default()
        };
        let climate = ClimateGenerator::generate_spherical(&surface, &relief, &spec).unwrap();
        let repeated = ClimateGenerator::generate_spherical(&surface, &relief, &spec).unwrap();
        assert_eq!(climate, repeated, "{}", case.name);
        climate.validate_against(&surface, &relief).unwrap();

        let north = nearest_latitude(&surface, 45.0);
        let south = nearest_latitude(&surface, -45.0);
        if case.axial_tilt_centideg > 0 {
            let temperatures = climate.monthly_air_temperature_c().values();
            assert!(
                temperatures[north][5] > temperatures[north][11],
                "{}",
                case.name
            );
            assert!(
                temperatures[south][5] < temperatures[south][11],
                "{}",
                case.name
            );
        }
        let equator = nearest_latitude(&surface, 0.0);
        let midlatitude = nearest_latitude(&surface, 45.0);
        assert!(
            east_component(
                surface.cells()[equator].centroid.components(),
                climate.monthly_wind_m_s().values()[equator][2]
            ) < 0.0,
            "{}",
            case.name
        );
        assert!(
            east_component(
                surface.cells()[midlatitude].centroid.components(),
                climate.monthly_wind_m_s().values()[midlatitude][2]
            ) > 0.0,
            "{}",
            case.name
        );

        for (index, cell) in surface.cells().iter().enumerate() {
            let expected_latitude = cell.centroid.components()[2].asin().to_degrees() as f32;
            assert!(
                (climate.latitude_degrees()[index] - expected_latitude).abs() <= 1.0e-5,
                "{}",
                case.name
            );
            for &wind in &climate.monthly_wind_m_s().values()[index] {
                assert!(
                    dot(cell.centroid.components(), wind).abs() <= 1.0e-4,
                    "{}",
                    case.name
                );
            }
            if relief.land_ocean_kind(cell.id) == Some(LandOceanKind::Ocean) {
                assert_eq!(climate.maritime_influence()[index], 1.0, "{}", case.name);
            }
        }
        match case.terrain {
            TerrainPattern::AllLand => assert!(climate
                .maritime_influence()
                .iter()
                .all(|&value| value == 0.0)),
            TerrainPattern::AllOcean => assert!(climate
                .maritime_influence()
                .iter()
                .all(|&value| value == 1.0)),
            TerrainPattern::Continents | TerrainPattern::MountainArc => {}
        }
        assert!(
            climate
                .annual_precipitation_mm()
                .iter()
                .copied()
                .sum::<f32>()
                > 0.0,
            "{}",
            case.name
        );

        let mut all_jumps = Vec::new();
        let mut cut_jumps = Vec::new();
        let mut polar_jumps = Vec::new();
        for edge in surface.edges() {
            let [first, second] = edge.cells.map(|cell| cell.raw() as usize);
            let temperature_jump = (climate.mean_annual_air_temperature_c()[first]
                - climate.mean_annual_air_temperature_c()[second])
                .abs();
            let precipitation_jump = (climate.annual_precipitation_mm()[first]
                - climate.annual_precipitation_mm()[second])
                .abs();
            let jump = f64::from(temperature_jump) + f64::from(precipitation_jump) * 0.01;
            all_jumps.push(jump);
            let first_radial = surface.cells()[first].centroid.components();
            let second_radial = surface.cells()[second].centroid.components();
            if first_radial[1].is_sign_positive() != second_radial[1].is_sign_positive()
                && first_radial[0] < 0.0
                && second_radial[0] < 0.0
            {
                cut_jumps.push(jump);
            }
            if first_radial[2].abs().max(second_radial[2].abs()) > 0.80 {
                polar_jumps.push(jump);
            }
        }
        assert!(
            !cut_jumps.is_empty() && !polar_jumps.is_empty(),
            "{}",
            case.name
        );
        assert!(
            mean(&cut_jumps) <= mean(&all_jumps) * 4.0 + 5.0,
            "{} cut={} all={}",
            case.name,
            mean(&cut_jumps),
            mean(&all_jumps)
        );
        assert!(
            mean(&polar_jumps) <= mean(&all_jumps) * 4.0 + 5.0,
            "{} polar={} all={}",
            case.name,
            mean(&polar_jumps),
            mean(&all_jumps)
        );

        let hash = blake3::hash(&serde_json::to_vec(&climate).unwrap())
            .to_hex()
            .to_string();
        eprintln!(
            "matrix_case={} cells={} climate_hash={}",
            case.name,
            surface.cells().len(),
            hash
        );
        actual_hashes.push((case, hash));
    }

    for (case, hash) in actual_hashes {
        assert_eq!(hash, case.expected_hash, "{}", case.name);
    }
}
