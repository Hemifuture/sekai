use sekai::generators::natural::HydrologyGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BasinOutletKind, BedrockKind, BedrockKindField, ElevationField, HydroErosionSpec,
    LandOceanField, LandOceanKind, MonthlyScalarField, MonthlyVector3Field,
    SphericalGeologicSnapshot, SphericalHydrologySnapshot, SphericalPreliminaryClimateSnapshot,
    SphericalReliefSnapshot, SurfaceWaterKind, CLIMATE_MONTH_COUNT, GEOLOGIC_SNAPSHOT_SCHEMA_V2,
    PRELIMINARY_CLIMATE_SCHEMA_V2, RELIEF_SCHEMA_V4,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

fn surface(radius_m: f64, target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(radius_m).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn relief(surface: &SphericalSurfaceSnapshot, elevations: Vec<f32>) -> SphericalReliefSnapshot {
    let count = surface.cells().len();
    assert_eq!(elevations.len(), count);
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

fn geology(surface: &SphericalSurfaceSnapshot, permeability: f32) -> SphericalGeologicSnapshot {
    let count = surface.cells().len();
    SphericalGeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        BedrockKindField::from_kinds(vec![BedrockKind::ContinentalCrystalline; count]),
        vec![0.0; count],
        vec![0.5; count],
        vec![permeability; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

fn climate(
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
    let maritime = (0..count)
        .map(|index| {
            if relief.land_ocean().get(index) == Some(LandOceanKind::Ocean) {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    let snapshot = SphericalPreliminaryClimateSnapshot::new(
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
    .unwrap();
    snapshot.validate_against(surface, relief).unwrap();
    snapshot
}

fn low_threshold_spec() -> HydroErosionSpec {
    HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: 1,
        ..HydroErosionSpec::default()
    }
}

fn generate(
    surface: &SphericalSurfaceSnapshot,
    relief: &SphericalReliefSnapshot,
) -> SphericalHydrologySnapshot {
    HydrologyGenerator::generate_spherical(
        surface,
        relief,
        &geology(surface, 0.25),
        &climate(surface, relief, 100.0),
        &low_threshold_spec(),
    )
    .unwrap()
}

#[test]
fn spherical_hydrology_is_deterministic_adjacent_surface_bound_and_geodesic() {
    let surface = surface(6_371_000.0, 162);
    let elevations = surface
        .cells()
        .iter()
        .map(|cell| {
            let [x, y, z] = cell.centroid.components();
            (900.0 * x + 500.0 * y - 300.0 * z - 150.0) as f32
        })
        .collect();
    let relief = relief(&surface, elevations);

    let first = generate(&surface, &relief);
    let second = generate(&surface, &relief);
    first.validate_against(&surface).unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );

    for (index, receiver) in first.flow_receiver().iter().enumerate() {
        let Some(receiver) = receiver else {
            continue;
        };
        let cell = CellId::from_raw(index as u32);
        assert!(surface
            .cell_edges(cell)
            .unwrap()
            .iter()
            .any(|&edge| { surface.opposite_cell(cell, edge) == Some(*receiver) }));
    }
    assert_eq!(
        first.river_segments().len(),
        first.river_segment_length_m().len()
    );
    for (segment, &stored_length) in first
        .river_segments()
        .iter()
        .zip(first.river_segment_length_m())
    {
        let edge = surface
            .cell_edges(segment.from())
            .unwrap()
            .iter()
            .copied()
            .find(|&edge| surface.opposite_cell(segment.from(), edge) == Some(segment.to()))
            .unwrap();
        assert_eq!(
            stored_length,
            surface.edge(edge).unwrap().center_distance.get()
        );
    }
}

#[test]
fn all_land_sphere_publishes_each_local_minimum_as_an_endorheic_basin() {
    let surface = surface(6_371_000.0, 162);
    let first = CellId::from_raw(0);
    let first_radial = surface.cell(first).unwrap().centroid.components();
    let second = surface
        .cells()
        .iter()
        .min_by(|left, right| {
            dot(left.centroid.components(), first_radial)
                .total_cmp(&dot(right.centroid.components(), first_radial))
        })
        .unwrap()
        .id;
    let mut elevations = vec![1_000.0; surface.cells().len()];
    elevations[first.raw() as usize] = 100.0;
    elevations[second.raw() as usize] = 200.0;
    let relief = relief(&surface, elevations);
    let hydrology = generate(&surface, &relief);

    assert_eq!(hydrology.basins().len(), 2);
    assert!(hydrology
        .basins()
        .iter()
        .all(|basin| basin.outlet_kind() == BasinOutletKind::ClosedSink));
    assert_eq!(
        hydrology
            .basins()
            .iter()
            .map(|basin| basin.outlet_cell())
            .collect::<Vec<_>>(),
        vec![first.min(second), first.max(second)]
    );
    assert!(hydrology
        .surface_water()
        .raw_values()
        .iter()
        .all(|&kind| kind == SurfaceWaterKind::DryLand.raw()));
}

#[test]
fn flat_all_land_and_all_ocean_spheres_have_explicit_terminal_semantics() {
    let surface = surface(6_371_000.0, 42);
    let land_relief = relief(&surface, vec![100.0; surface.cells().len()]);
    let land = generate(&surface, &land_relief);
    assert_eq!(land.basins().len(), 1);
    assert_eq!(land.basins()[0].outlet_kind(), BasinOutletKind::ClosedSink);
    assert_eq!(land.basins()[0].outlet_cell(), CellId::from_raw(0));

    let ocean_relief = relief(&surface, vec![-100.0; surface.cells().len()]);
    let ocean = generate(&surface, &ocean_relief);
    assert!(ocean.basins().is_empty());
    assert!(ocean.flow_receiver().iter().all(Option::is_none));
    assert!(ocean
        .surface_water()
        .raw_values()
        .iter()
        .all(|&kind| kind == SurfaceWaterKind::Ocean.raw()));
}

#[test]
fn ocean_seeded_priority_flood_forms_a_real_bowl_lake_and_spill() {
    let surface = surface(6_371_000.0, 42);
    let center = CellId::from_raw(0);
    let mut elevations = vec![-100.0; surface.cells().len()];
    elevations[center.raw() as usize] = 1.0;
    for &edge in surface.cell_edges(center).unwrap() {
        let neighbor = surface.opposite_cell(center, edge).unwrap();
        elevations[neighbor.raw() as usize] = 5.0;
    }
    let relief = relief(&surface, elevations);
    let hydrology = generate(&surface, &relief);

    assert_eq!(hydrology.lakes().len(), 1);
    assert_eq!(hydrology.lakes()[0].cells(), &[center]);
    assert_eq!(hydrology.lakes()[0].surface_elevation_m(), 5.0);
    assert_eq!(hydrology.lake_depth_m()[center.raw() as usize], 4.0);
    assert!(hydrology.lakes()[0].outlet_cell().is_some());
    assert!(hydrology.lakes()[0].downstream_cell().is_some());
}

#[test]
fn spherical_area_and_river_length_follow_radius_squared_and_radius() {
    let small_surface = surface(6_371_000.0, 162);
    let large_surface = surface(12_742_000.0, 162);
    let elevations = small_surface
        .cells()
        .iter()
        .map(|cell| {
            let [x, y, z] = cell.centroid.components();
            (500.0 * x + 300.0 * y + 100.0 * z + 1_000.0) as f32
        })
        .collect::<Vec<_>>();
    let small = generate(&small_surface, &relief(&small_surface, elevations.clone()));
    let large = generate(&large_surface, &relief(&large_surface, elevations));

    assert_eq!(small.flow_receiver(), large.flow_receiver());
    assert_eq!(small.river_segments().len(), large.river_segments().len());
    assert!(!small.river_segments().is_empty());
    for (&small_length, &large_length) in small
        .river_segment_length_m()
        .iter()
        .zip(large.river_segment_length_m())
    {
        assert!((large_length / small_length - 2.0).abs() < 1.0e-12);
    }
    for (&small_area, &large_area) in small
        .drainage_area_km2()
        .iter()
        .zip(large.drainage_area_km2())
    {
        assert!((large_area / small_area - 4.0).abs() < 1.0e-5);
    }
}

#[test]
fn spherical_generation_rejects_same_count_upstreams_from_another_surface() {
    let authoritative = surface(6_371_000.0, 42);
    let other = surface(6_372_000.0, 42);
    let relief = relief(&authoritative, vec![100.0; authoritative.cells().len()]);
    let climate = climate(&authoritative, &relief, 100.0);
    let error = HydrologyGenerator::generate_spherical(
        &authoritative,
        &relief,
        &geology(&other, 0.25),
        &climate,
        &low_threshold_spec(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("geology surface"));
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}
