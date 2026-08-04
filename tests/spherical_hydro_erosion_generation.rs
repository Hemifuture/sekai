use std::collections::VecDeque;

use sekai::generators::natural::{FluvialErosionGenerator, HydrologyGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BedrockKind, BedrockKindField, ElevationField, HydroErosionSpec, LandOceanField, LandOceanKind,
    MonthlyScalarField, MonthlyVector3Field, SphericalGeologicSnapshot, SphericalHydrologySnapshot,
    SphericalPreliminaryClimateSnapshot, SphericalReliefSnapshot, SphericalSurfaceProcessSnapshot,
    CLIMATE_MONTH_COUNT, GEOLOGIC_SNAPSHOT_SCHEMA_V2, PRELIMINARY_CLIMATE_SCHEMA_V2,
    RELIEF_SCHEMA_V4,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

fn surface() -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 162,
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
                .map(|height| LandOceanKind::classify(height, 0.0))
                .collect(),
        ),
    )
    .unwrap()
}

fn geology(surface: &SphericalSurfaceSnapshot, resistance: f32) -> SphericalGeologicSnapshot {
    let count = surface.cells().len();
    SphericalGeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        BedrockKindField::from_kinds(vec![BedrockKind::ContinentalCrystalline; count]),
        vec![0.0; count],
        vec![resistance; count],
        vec![0.25; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

fn climate(
    surface: &SphericalSurfaceSnapshot,
    relief: &SphericalReliefSnapshot,
) -> SphericalPreliminaryClimateSnapshot {
    let count = surface.cells().len();
    let latitude = surface
        .cells()
        .iter()
        .map(|cell| cell.centroid.components()[2].asin().to_degrees() as f32)
        .collect();
    let maritime = (0..count)
        .map(|index| {
            if relief.land_ocean().get(index) == Some(LandOceanKind::Ocean) {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    let precipitation_mm = 500.0;
    SphericalPreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V2,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        latitude,
        maritime,
        MonthlyScalarField::from_values(vec![[20.0; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        MonthlyScalarField::from_values(vec![[precipitation_mm; CLIMATE_MONTH_COUNT]; count])
            .unwrap(),
        MonthlyVector3Field::from_values(vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        vec![20.0; count],
        vec![0.0; count],
        vec![precipitation_mm * CLIMATE_MONTH_COUNT as f32; count],
        vec![[0.0; 3]; count],
    )
    .unwrap()
}

fn spec() -> HydroErosionSpec {
    HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: 1,
        erosion_strength_permille: 1_000,
        ..HydroErosionSpec::default()
    }
}

fn hydrology(
    surface: &SphericalSurfaceSnapshot,
    relief: &SphericalReliefSnapshot,
    geology: &SphericalGeologicSnapshot,
) -> SphericalHydrologySnapshot {
    HydrologyGenerator::generate_spherical(
        surface,
        relief,
        geology,
        &climate(surface, relief),
        &spec(),
    )
    .unwrap()
}

fn erode(
    surface: &SphericalSurfaceSnapshot,
    relief: &SphericalReliefSnapshot,
    geology: &SphericalGeologicSnapshot,
    hydrology: &SphericalHydrologySnapshot,
) -> SphericalSurfaceProcessSnapshot {
    FluvialErosionGenerator::generate_spherical(surface, relief, geology, hydrology, &spec())
        .unwrap()
}

#[test]
fn spherical_erosion_is_deterministic_bounded_causal_and_surface_native() {
    let surface = surface();
    let elevations = ocean_basin_elevations(&surface);
    let relief = relief(&surface, elevations);
    let soft = geology(&surface, 0.1);
    let hard = geology(&surface, 0.9);
    let hydrology = hydrology(&surface, &relief, &soft);

    let soft_output = erode(&surface, &relief, &soft, &hydrology);
    let repeated = erode(&surface, &relief, &soft, &hydrology);
    let hard_output = erode(&surface, &relief, &hard, &hydrology);
    soft_output.validate_against(&surface, &relief).unwrap();
    assert_eq!(
        serde_json::to_vec(&soft_output).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );

    let soft_total = soft_output.erosion_depth_m().iter().sum::<f32>();
    let hard_total = hard_output.erosion_depth_m().iter().sum::<f32>();
    assert!(soft_total > hard_total);
    assert!(soft_total > 0.0);
    for index in 0..surface.cells().len() {
        let constructional = relief.elevation_m().values()[index];
        let expected = constructional - soft_output.erosion_depth_m()[index]
            + soft_output.deposition_thickness_m()[index];
        assert!((soft_output.surface_elevation_m().values()[index] - expected).abs() <= 0.05);
        if relief.land_ocean().get(index) == Some(LandOceanKind::Ocean) {
            assert_eq!(soft_output.erosion_depth_m()[index], 0.0);
            assert_eq!(soft_output.deposition_thickness_m()[index], 0.0);
        }
    }
}

#[test]
fn ocean_and_endorheic_terminals_use_distinct_conservative_ledgers() {
    let surface = surface();

    let ocean_relief = relief(&surface, ocean_basin_elevations(&surface));
    let substrate = geology(&surface, 0.1);
    let ocean_hydrology = hydrology(&surface, &ocean_relief, &substrate);
    let ocean_process = erode(&surface, &ocean_relief, &substrate, &ocean_hydrology);
    assert!(ocean_process.sediment_ocean_delivery_m3() > 0.0);
    assert_eq!(ocean_process.sediment_endorheic_storage_m3(), 0.0);

    let minima = [CellId::from_raw(0), antipode(&surface, CellId::from_raw(0))];
    let distances = hop_distance(&surface, &minima);
    let land_relief = relief(
        &surface,
        distances
            .into_iter()
            .map(|distance| 100.0 + distance as f32 * 250.0)
            .collect(),
    );
    let land_hydrology = hydrology(&surface, &land_relief, &substrate);
    let land_process = erode(&surface, &land_relief, &substrate, &land_hydrology);
    assert_eq!(land_process.sediment_ocean_delivery_m3(), 0.0);
    assert!(land_process.sediment_endorheic_storage_m3() > 0.0);
    land_process
        .validate_against(&surface, &land_relief)
        .unwrap();
}

#[test]
fn zero_strength_preserves_the_constructional_spherical_surface_exactly() {
    let surface = surface();
    let relief = relief(&surface, ocean_basin_elevations(&surface));
    let substrate = geology(&surface, 0.1);
    let hydrology = hydrology(&surface, &relief, &substrate);
    let zero = HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: 1,
        erosion_strength_permille: 0,
        ..HydroErosionSpec::default()
    };
    let output = FluvialErosionGenerator::generate_spherical(
        &surface, &relief, &substrate, &hydrology, &zero,
    )
    .unwrap();

    assert_eq!(output.surface_elevation_m(), relief.elevation_m());
    assert!(output.erosion_depth_m().iter().all(|&value| value == 0.0));
    assert!(output
        .deposition_thickness_m()
        .iter()
        .all(|&value| value == 0.0));
    assert_eq!(output.sediment_terminal_transfer_m3(), 0.0);
}

fn ocean_basin_elevations(surface: &SphericalSurfaceSnapshot) -> Vec<f32> {
    let ocean = CellId::from_raw(0);
    hop_distance(surface, &[ocean])
        .into_iter()
        .map(|distance| {
            if distance == 0 {
                -100.0
            } else {
                distance as f32 * 250.0
            }
        })
        .collect()
}

fn hop_distance(surface: &SphericalSurfaceSnapshot, sources: &[CellId]) -> Vec<u32> {
    let mut distance = vec![u32::MAX; surface.cells().len()];
    let mut queue = VecDeque::new();
    for &source in sources {
        distance[source.raw() as usize] = 0;
        queue.push_back(source);
    }
    while let Some(cell) = queue.pop_front() {
        let next_distance = distance[cell.raw() as usize] + 1;
        for &edge in surface.cell_edges(cell).unwrap() {
            let neighbor = surface.opposite_cell(cell, edge).unwrap();
            let index = neighbor.raw() as usize;
            if distance[index] == u32::MAX {
                distance[index] = next_distance;
                queue.push_back(neighbor);
            }
        }
    }
    distance
}

fn antipode(surface: &SphericalSurfaceSnapshot, cell: CellId) -> CellId {
    let radial = surface.cell(cell).unwrap().centroid.components();
    surface
        .cells()
        .iter()
        .min_by(|left, right| {
            dot(left.centroid.components(), radial)
                .total_cmp(&dot(right.centroid.components(), radial))
        })
        .unwrap()
        .id
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}
