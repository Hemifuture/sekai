use sekai::generators::natural::{HydroErosionGenerator, HydrologyGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BasinOutletKind, BedrockKind, BedrockKindField, ElevationField, HydroErosionSpec,
    LandOceanField, LandOceanKind, MonthlyScalarField, MonthlyVector3Field,
    SphericalGeologicSnapshot, SphericalPreliminaryClimateSnapshot, SphericalReliefSnapshot,
    SurfaceWaterKind, CLIMATE_MONTH_COUNT, GEOLOGIC_SNAPSHOT_SCHEMA_V2,
    HYDRO_EROSION_SPEC_SCHEMA_V1, PRELIMINARY_CLIMATE_SCHEMA_V2, RELIEF_SCHEMA_V4,
};
use sekai::world::spatial::audited_float_platform;
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

#[derive(Clone, Copy)]
enum ReliefShape {
    AllLand,
    AllOcean,
    Mixed,
}

#[derive(Clone, Copy)]
struct MatrixCase {
    name: &'static str,
    radius_m: f64,
    target_cells: u32,
    shape: ReliefShape,
    precipitation_mm: f32,
    erosion_resistance: f32,
    permeability: f32,
    river_threshold_deci_m3_s: u32,
    erosion_strength_permille: u16,
    minimum_lake_depth_cm: u16,
    expected_hash: &'static str,
}

const CASES: [MatrixCase; 5] = [
    MatrixCase {
        name: "minimum-dry-hard-all-land",
        radius_m: 1.0,
        target_cells: 42,
        shape: ReliefShape::AllLand,
        precipitation_mm: 0.0,
        erosion_resistance: 1.0,
        permeability: 1.0,
        river_threshold_deci_m3_s: 1_000_000,
        erosion_strength_permille: 0,
        minimum_lake_depth_cm: 10_000,
        expected_hash: "e43ff5de8494499e18e06246c6e00ef56cfa6ed39693404feff093a9cb00fd0b",
    },
    MatrixCase {
        name: "earth-wet-soft-all-ocean",
        radius_m: 6_371_000.0,
        target_cells: 42,
        shape: ReliefShape::AllOcean,
        precipitation_mm: 1_500.0,
        erosion_resistance: 0.0,
        permeability: 0.0,
        river_threshold_deci_m3_s: 1,
        erosion_strength_permille: 2_000,
        minimum_lake_depth_cm: 1,
        expected_hash: "291855a73f860fb82b7096fc2c3211bae8dcd772153312b8d11c690533e4b300",
    },
    MatrixCase {
        name: "earth-moderate-mixed",
        radius_m: 6_371_000.0,
        target_cells: 162,
        shape: ReliefShape::Mixed,
        precipitation_mm: 500.0,
        erosion_resistance: 0.5,
        permeability: 0.25,
        river_threshold_deci_m3_s: 2_500,
        erosion_strength_permille: 1_000,
        minimum_lake_depth_cm: 100,
        expected_hash: "d23c5afaf6228c7e0745b49909a9723212f6e38d59bf57b8d1ac1c2db54b8eec",
    },
    MatrixCase {
        name: "earth-wet-soft-mixed-medium",
        radius_m: 6_371_000.0,
        target_cells: 642,
        shape: ReliefShape::Mixed,
        precipitation_mm: 1_500.0,
        erosion_resistance: 0.0,
        permeability: 0.0,
        river_threshold_deci_m3_s: 1,
        erosion_strength_permille: 2_000,
        minimum_lake_depth_cm: 1,
        expected_hash: "04560c868114798c95574583c2dea8c232130ea4d9c3db3169899e1c394ff930",
    },
    MatrixCase {
        name: "maximum-wet-hard-all-land",
        radius_m: 100_000_000.0,
        target_cells: 162,
        shape: ReliefShape::AllLand,
        precipitation_mm: 1_500.0,
        erosion_resistance: 1.0,
        permeability: 1.0,
        river_threshold_deci_m3_s: 1_000_000,
        erosion_strength_permille: 2_000,
        minimum_lake_depth_cm: 10_000,
        expected_hash: "2e1758ef9cd99f2f2c8bbac2017e607af7393ea7676021491fc3f2e8ebf486f0",
    },
];

#[test]
fn spherical_hydro_erosion_scientific_and_deterministic_matrix() {
    for case in CASES {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(case.radius_m).unwrap(),
            target_cell_count: case.target_cells,
        })
        .unwrap();
        let relief = build_relief(&surface, elevations(&surface, case.shape));
        let geology = geology(&surface, case.erosion_resistance, case.permeability);
        let climate = build_climate(&surface, &relief, case.precipitation_mm);
        let spec = HydroErosionSpec {
            schema_version: HYDRO_EROSION_SPEC_SCHEMA_V1,
            river_discharge_threshold_deci_m3_s: case.river_threshold_deci_m3_s,
            erosion_strength_permille: case.erosion_strength_permille,
            minimum_lake_depth_cm: case.minimum_lake_depth_cm,
        };
        let upstream_before = (
            serde_json::to_vec(&relief).unwrap(),
            serde_json::to_vec(&geology).unwrap(),
            serde_json::to_vec(&climate).unwrap(),
        );

        let first =
            HydroErosionGenerator::generate_spherical(&surface, &relief, &geology, &climate, &spec)
                .unwrap();
        let repeated =
            HydroErosionGenerator::generate_spherical(&surface, &relief, &geology, &climate, &spec)
                .unwrap();
        first
            .validate_against(&surface, &relief, &geology, &climate)
            .unwrap();
        let encoded = serde_json::to_vec(&first).unwrap();
        assert_eq!(
            encoded,
            serde_json::to_vec(&repeated).unwrap(),
            "{}",
            case.name
        );
        assert_eq!(
            upstream_before,
            (
                serde_json::to_vec(&relief).unwrap(),
                serde_json::to_vec(&geology).unwrap(),
                serde_json::to_vec(&climate).unwrap(),
            ),
            "{}",
            case.name
        );

        assert_receiver_graph_is_spherical_and_acyclic(&surface, first.hydrology(), case.name);
        assert_terminal_semantics(first.hydrology(), case.shape, case.name);

        let current_relief = build_relief(
            &surface,
            first.surface().surface_elevation_m().values().to_vec(),
        );
        let current_climate = build_climate(&surface, &current_relief, case.precipitation_mm);
        let independently_recomputed = HydrologyGenerator::generate_spherical(
            &surface,
            &current_relief,
            &geology,
            &current_climate,
            &spec,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_vec(first.hydrology()).unwrap(),
            serde_json::to_vec(&independently_recomputed).unwrap(),
            "{}",
            case.name
        );

        let hash = blake3::hash(&encoded).to_hex().to_string();
        eprintln!(
            "matrix_case={} cells={} basins={} lakes={} rivers={} ocean_delivery_m3={} endorheic_storage_m3={} hash={}",
            case.name,
            surface.cells().len(),
            first.hydrology().basins().len(),
            first.hydrology().lakes().len(),
            first.hydrology().river_segments().len(),
            first.surface().sediment_ocean_delivery_m3(),
            first.surface().sediment_endorheic_storage_m3(),
            hash,
        );
        if audited_float_platform() {
            assert_eq!(hash, case.expected_hash, "{}", case.name);
        } else {
            eprintln!("exact identity checks skipped: unaudited float platform");
        }
    }
}

fn elevations(surface: &SphericalSurfaceSnapshot, shape: ReliefShape) -> Vec<f32> {
    surface
        .cells()
        .iter()
        .map(|cell| {
            let [x, y, z] = cell.centroid.components();
            match shape {
                ReliefShape::AllLand => (800.0 + 220.0 * x + 130.0 * y + 70.0 * z) as f32,
                ReliefShape::AllOcean => (-800.0 + 120.0 * x - 80.0 * z) as f32,
                ReliefShape::Mixed => (1_100.0 * x + 450.0 * z - 100.0) as f32,
            }
        })
        .collect()
}

fn build_relief(
    surface: &SphericalSurfaceSnapshot,
    elevations: Vec<f32>,
) -> SphericalReliefSnapshot {
    let count = surface.cells().len();
    let zero = vec![0.0; count];
    let elevation = ElevationField::from_values(elevations).unwrap();
    SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        0.0,
        elevation.clone(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero).unwrap(),
        elevation.clone(),
        LandOceanField::classify(&elevation, 0.0),
    )
    .unwrap()
}

fn geology(
    surface: &SphericalSurfaceSnapshot,
    erosion_resistance: f32,
    permeability: f32,
) -> SphericalGeologicSnapshot {
    let count = surface.cells().len();
    SphericalGeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        BedrockKindField::from_kinds(vec![BedrockKind::ContinentalCrystalline; count]),
        vec![0.0; count],
        vec![erosion_resistance; count],
        vec![permeability; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

fn build_climate(
    surface: &SphericalSurfaceSnapshot,
    relief: &SphericalReliefSnapshot,
    precipitation_mm: f32,
) -> SphericalPreliminaryClimateSnapshot {
    let count = surface.cells().len();
    let latitude = surface
        .cells()
        .iter()
        .map(|cell| cell.centroid.components()[2].asin().to_degrees() as f32)
        .collect();
    let maritime = relief
        .land_ocean()
        .raw_values()
        .iter()
        .map(|&kind| {
            if kind == LandOceanKind::Ocean.raw() {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    SphericalPreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V2,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        latitude,
        maritime,
        MonthlyScalarField::from_values(vec![[15.0; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        MonthlyScalarField::from_values(vec![[precipitation_mm; CLIMATE_MONTH_COUNT]; count])
            .unwrap(),
        MonthlyVector3Field::from_values(vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        vec![15.0; count],
        vec![0.0; count],
        vec![precipitation_mm * CLIMATE_MONTH_COUNT as f32; count],
        vec![[0.0; 3]; count],
    )
    .unwrap()
}

fn assert_receiver_graph_is_spherical_and_acyclic(
    surface: &SphericalSurfaceSnapshot,
    hydrology: &sekai::world::natural::SphericalHydrologySnapshot,
    case: &str,
) {
    for (index, &receiver) in hydrology.flow_receiver().iter().enumerate() {
        if let Some(receiver) = receiver {
            let cell = CellId::from_raw(index as u32);
            assert!(
                surface
                    .cell_edges(cell)
                    .unwrap()
                    .iter()
                    .any(|&edge| surface.opposite_cell(cell, edge) == Some(receiver)),
                "{case}"
            );
        }
        let mut cursor = Some(CellId::from_raw(index as u32));
        for _ in 0..=surface.cells().len() {
            let Some(cell) = cursor else {
                break;
            };
            cursor = hydrology.flow_receiver()[cell.raw() as usize];
        }
        assert!(cursor.is_none(), "receiver cycle in {case} at cell {index}");
    }
    for (segment, &length) in hydrology
        .river_segments()
        .iter()
        .zip(hydrology.river_segment_length_m())
    {
        let edge = surface
            .cell_edges(segment.from())
            .unwrap()
            .iter()
            .copied()
            .find(|&edge| surface.opposite_cell(segment.from(), edge) == Some(segment.to()))
            .unwrap();
        assert_eq!(
            length,
            surface.edge(edge).unwrap().center_distance.get(),
            "{case}"
        );
    }
}

fn assert_terminal_semantics(
    hydrology: &sekai::world::natural::SphericalHydrologySnapshot,
    shape: ReliefShape,
    case: &str,
) {
    match shape {
        ReliefShape::AllLand => {
            assert!(!hydrology.basins().is_empty(), "{case}");
            assert!(
                hydrology
                    .basins()
                    .iter()
                    .all(|basin| basin.outlet_kind() == BasinOutletKind::ClosedSink),
                "{case}"
            );
            assert!(
                hydrology
                    .surface_water()
                    .raw_values()
                    .iter()
                    .all(|&kind| kind != SurfaceWaterKind::Ocean.raw()),
                "{case}"
            );
        }
        ReliefShape::AllOcean => {
            assert!(hydrology.basins().is_empty(), "{case}");
            assert!(
                hydrology.flow_receiver().iter().all(Option::is_none),
                "{case}"
            );
            assert!(
                hydrology
                    .surface_water()
                    .raw_values()
                    .iter()
                    .all(|&kind| kind == SurfaceWaterKind::Ocean.raw()),
                "{case}"
            );
        }
        ReliefShape::Mixed => {
            assert!(
                hydrology
                    .surface_water()
                    .raw_values()
                    .iter()
                    .any(|&kind| kind == SurfaceWaterKind::Ocean.raw()),
                "{case}"
            );
            assert!(
                hydrology
                    .basins()
                    .iter()
                    .all(|basin| basin.outlet_kind() == BasinOutletKind::Ocean),
                "{case}"
            );
        }
    }
}
