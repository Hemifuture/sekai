use std::collections::VecDeque;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    BasinOutletKind, BedrockKind, BedrockKindField, DrainageBasin, ElevationField,
    GeologicSnapshot, HydroErosionSnapshot, HydroErosionValidationError, HydrologySnapshot,
    LandOceanField, LandOceanKind, MonthlyScalarField, MonthlyVectorField,
    PreliminaryClimateSnapshot, ReliefSnapshot, StrahlerOrderField, SurfaceProcessSnapshot,
    SurfaceWaterField, SurfaceWaterKind, CLIMATE_MONTH_COUNT, GEOLOGIC_SNAPSHOT_SCHEMA_V1,
    HYDROLOGY_SCHEMA_V1, HYDRO_EROSION_SNAPSHOT_SCHEMA_V1, PRELIMINARY_CLIMATE_SCHEMA_V1,
    RELIEF_SCHEMA_V2, SECONDS_PER_CLIMATOLOGICAL_MONTH, SURFACE_PROCESS_SCHEMA_V1,
};
use sekai::world::spatial::{SpatialSnapshot, Topology};
use sekai::world::{BoundaryCondition, CellId, DrainageBasinId, Meters, PlanarSpaceSpec};

fn spatial_fixture(target_cell_count: u32) -> SpatialSnapshot {
    PlanarVoronoiBuilder::build(
        &PlanarSpaceSpec {
            width: Meters::new(1_000.0).unwrap(),
            height: Meters::new(500.0).unwrap(),
            target_cell_count,
            boundary: BoundaryCondition::Closed,
        },
        &mut ChaCha8Rng::seed_from_u64(29),
    )
    .unwrap()
}

fn relief_fixture(cell_count: usize, sea_level_m: f32) -> ReliefSnapshot {
    let elevations = vec![-100.0; cell_count];
    ReliefSnapshot::new(
        RELIEF_SCHEMA_V2,
        cell_count as u32,
        sea_level_m,
        ElevationField::from_values(elevations.clone()).unwrap(),
        ElevationField::from_values(vec![0.0; cell_count]).unwrap(),
        ElevationField::from_values(vec![0.0; cell_count]).unwrap(),
        ElevationField::from_values(vec![0.0; cell_count]).unwrap(),
        ElevationField::from_values(elevations.clone()).unwrap(),
        LandOceanField::from_kinds(
            elevations
                .into_iter()
                .map(|elevation| LandOceanKind::classify(elevation, sea_level_m))
                .collect(),
        ),
    )
    .unwrap()
}

fn surface_fixture(cell_count: usize) -> SurfaceProcessSnapshot {
    SurfaceProcessSnapshot::new(
        SURFACE_PROCESS_SCHEMA_V1,
        cell_count as u32,
        vec![0.0; cell_count],
        vec![0.0; cell_count],
        ElevationField::from_values(vec![-100.0; cell_count]).unwrap(),
        vec![0.0; cell_count],
        0.0,
    )
    .unwrap()
}

fn geology_fixture(cell_count: usize) -> GeologicSnapshot {
    GeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V1,
        cell_count as u32,
        BedrockKindField::from_kinds(vec![BedrockKind::OceanicMafic; cell_count]),
        vec![0.0; cell_count],
        vec![0.7; cell_count],
        vec![0.4; cell_count],
        vec![0.0; cell_count],
        vec![0.0; cell_count],
        vec![0.0; cell_count],
    )
    .unwrap()
}

fn climate_fixture(cell_count: usize) -> PreliminaryClimateSnapshot {
    PreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V1,
        cell_count as u32,
        vec![0.0; cell_count],
        vec![1.0; cell_count],
        MonthlyScalarField::from_values(vec![[20.0; CLIMATE_MONTH_COUNT]; cell_count]).unwrap(),
        MonthlyScalarField::from_values(vec![[100.0; CLIMATE_MONTH_COUNT]; cell_count]).unwrap(),
        MonthlyVectorField::from_values(vec![[[0.0, 0.0]; CLIMATE_MONTH_COUNT]; cell_count])
            .unwrap(),
        vec![20.0; cell_count],
        vec![0.0; cell_count],
        vec![1_200.0; cell_count],
        vec![[0.0, 0.0]; cell_count],
    )
    .unwrap()
}

fn ocean_hydrology_fixture(spatial: &SpatialSnapshot, monthly_runoff_mm: f32) -> HydrologySnapshot {
    let cell_count = spatial.cell_count();
    let monthly_discharge_m3_s: Vec<[f32; CLIMATE_MONTH_COUNT]> = (0..cell_count)
        .map(|index| {
            let area_m2 = spatial
                .cell(CellId::from_raw(index as u32))
                .unwrap()
                .area
                .get();
            let discharge = (f64::from(monthly_runoff_mm) / 1_000.0 * area_m2
                / SECONDS_PER_CLIMATOLOGICAL_MONTH) as f32;
            [discharge; CLIMATE_MONTH_COUNT]
        })
        .collect();
    let mean_annual_discharge_m3_s = monthly_discharge_m3_s
        .iter()
        .map(|months| months[0])
        .collect();

    HydrologySnapshot::new(
        HYDROLOGY_SCHEMA_V1,
        cell_count as u32,
        0.1,
        1.0,
        vec![[monthly_runoff_mm; CLIMATE_MONTH_COUNT]; cell_count],
        monthly_discharge_m3_s,
        vec![monthly_runoff_mm * CLIMATE_MONTH_COUNT as f32; cell_count],
        mean_annual_discharge_m3_s,
        (0..cell_count)
            .map(|index| {
                (spatial
                    .cell(CellId::from_raw(index as u32))
                    .unwrap()
                    .area
                    .get()
                    / 1_000_000.0) as f32
            })
            .collect(),
        ElevationField::from_values(vec![-100.0; cell_count]).unwrap(),
        vec![0.0; cell_count],
        SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::Ocean; cell_count]),
        vec![None; cell_count],
        vec![None; cell_count],
        StrahlerOrderField::from_raw(vec![0; cell_count]).unwrap(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

fn land_hydrology_fixture(spatial: &SpatialSnapshot, monthly_runoff_mm: f32) -> HydrologySnapshot {
    let cell_count = spatial.cell_count();
    let root = CellId::from_raw(0);
    let mut visited = vec![false; cell_count];
    let mut receiver = vec![None; cell_count];
    let mut root_to_leaves = Vec::with_capacity(cell_count);
    let mut queue = VecDeque::from([root]);
    visited[0] = true;
    while let Some(cell) = queue.pop_front() {
        root_to_leaves.push(cell);
        for &neighbor in spatial.neighbors(cell).unwrap() {
            let index = neighbor.raw() as usize;
            if !visited[index] {
                visited[index] = true;
                receiver[index] = Some(cell);
                queue.push_back(neighbor);
            }
        }
    }
    assert!(visited.into_iter().all(|value| value));

    let mut drainage_area_km2 = vec![0.0_f64; cell_count];
    let mut monthly_discharge_m3_s = vec![[0.0_f64; CLIMATE_MONTH_COUNT]; cell_count];
    for index in 0..cell_count {
        let area_m2 = spatial
            .cell(CellId::from_raw(index as u32))
            .unwrap()
            .area
            .get();
        drainage_area_km2[index] = area_m2 / 1_000_000.0;
        let local_discharge =
            f64::from(monthly_runoff_mm) / 1_000.0 * area_m2 / SECONDS_PER_CLIMATOLOGICAL_MONTH;
        monthly_discharge_m3_s[index].fill(local_discharge);
    }
    for &cell in root_to_leaves.iter().rev() {
        let index = cell.raw() as usize;
        if let Some(parent) = receiver[index] {
            let parent_index = parent.raw() as usize;
            drainage_area_km2[parent_index] += drainage_area_km2[index];
            let upstream = monthly_discharge_m3_s[index];
            for (downstream, upstream) in monthly_discharge_m3_s[parent_index]
                .iter_mut()
                .zip(upstream)
            {
                *downstream += upstream;
            }
        }
    }
    let monthly_discharge_m3_s: Vec<[f32; CLIMATE_MONTH_COUNT]> = monthly_discharge_m3_s
        .into_iter()
        .map(|months| months.map(|value| value as f32))
        .collect();
    let mean_annual_discharge_m3_s = monthly_discharge_m3_s
        .iter()
        .map(|months| months[0])
        .collect::<Vec<_>>();
    let basin_area_km2 = drainage_area_km2[0];
    let basin_discharge_m3_s = mean_annual_discharge_m3_s[0];

    HydrologySnapshot::new(
        HYDROLOGY_SCHEMA_V1,
        cell_count as u32,
        0.1,
        1.0,
        vec![[monthly_runoff_mm; CLIMATE_MONTH_COUNT]; cell_count],
        monthly_discharge_m3_s,
        vec![monthly_runoff_mm * CLIMATE_MONTH_COUNT as f32; cell_count],
        mean_annual_discharge_m3_s,
        drainage_area_km2
            .into_iter()
            .map(|area| area as f32)
            .collect(),
        ElevationField::from_values(vec![-100.0; cell_count]).unwrap(),
        vec![0.0; cell_count],
        SurfaceWaterField::from_kinds(vec![SurfaceWaterKind::DryLand; cell_count]),
        receiver,
        vec![Some(DrainageBasinId::from_raw(0)); cell_count],
        StrahlerOrderField::from_raw(vec![0; cell_count]).unwrap(),
        vec![DrainageBasin::new(
            DrainageBasinId::from_raw(0),
            root,
            BasinOutletKind::ClosedSink,
            basin_area_km2,
            basin_discharge_m3_s,
        )
        .unwrap()],
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

fn valid_fixture() -> (
    SpatialSnapshot,
    ReliefSnapshot,
    GeologicSnapshot,
    PreliminaryClimateSnapshot,
    HydroErosionSnapshot,
) {
    let spatial = spatial_fixture(16);
    let cell_count = spatial.cell_count();
    let relief = relief_fixture(cell_count, 0.0);
    let geology = geology_fixture(cell_count);
    let climate = climate_fixture(cell_count);
    let snapshot = HydroErosionSnapshot::new(
        HYDRO_EROSION_SNAPSHOT_SCHEMA_V1,
        surface_fixture(cell_count),
        ocean_hydrology_fixture(&spatial, 0.0),
    )
    .unwrap();
    (spatial, relief, geology, climate, snapshot)
}

#[test]
fn valid_composite_constructs_round_trips_and_cross_validates() {
    let (spatial, relief, geology, climate, snapshot) = valid_fixture();

    snapshot.validate().unwrap();
    snapshot
        .validate_against(&spatial, &relief, &geology, &climate)
        .unwrap();
    assert_eq!(snapshot.schema_version(), HYDRO_EROSION_SNAPSHOT_SCHEMA_V1);
    assert_eq!(snapshot.surface().cell_count(), snapshot.cell_count());
    assert_eq!(snapshot.hydrology().cell_count(), snapshot.cell_count());

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: HydroErosionSnapshot = serde_json::from_slice(&encoded).unwrap();
    decoded
        .validate_against(&spatial, &relief, &geology, &climate)
        .unwrap();
    assert_eq!(decoded, snapshot);
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn schema_and_subsnapshot_cell_counts_must_match() {
    let spatial = spatial_fixture(16);
    let count = spatial.cell_count();
    assert!(matches!(
        HydroErosionSnapshot::new(
            HYDRO_EROSION_SNAPSHOT_SCHEMA_V1 + 1,
            surface_fixture(count),
            ocean_hydrology_fixture(&spatial, 0.0),
        ),
        Err(HydroErosionValidationError::UnsupportedSchema { .. })
    ));

    let other_spatial = spatial_fixture(25);
    assert!(matches!(
        HydroErosionSnapshot::new(
            HYDRO_EROSION_SNAPSHOT_SCHEMA_V1,
            surface_fixture(count),
            ocean_hydrology_fixture(&other_spatial, 0.0),
        ),
        Err(HydroErosionValidationError::CellCountMismatch { .. })
    ));
}

#[test]
fn cross_validation_rejects_external_cardinality_mismatches() {
    let (spatial, relief, geology, climate, snapshot) = valid_fixture();

    assert!(matches!(
        snapshot.validate_against(&spatial_fixture(25), &relief, &geology, &climate),
        Err(HydroErosionValidationError::Surface(_))
            | Err(HydroErosionValidationError::Hydrology(_))
            | Err(HydroErosionValidationError::SpatialCellCountMismatch { .. })
    ));
    assert!(matches!(
        snapshot.validate_against(
            &spatial,
            &relief,
            &geology_fixture(spatial.cell_count() - 1),
            &climate,
        ),
        Err(HydroErosionValidationError::GeologyCellCountMismatch { .. })
    ));
    assert!(matches!(
        snapshot.validate_against(
            &spatial,
            &relief,
            &geology,
            &climate_fixture(spatial.cell_count() - 1),
        ),
        Err(HydroErosionValidationError::ClimateCellCountMismatch { .. })
    ));
}

#[test]
fn formal_sea_level_and_current_surface_define_ocean_cells() {
    let (spatial, _relief, geology, climate, snapshot) = valid_fixture();
    let land_relief = relief_fixture(spatial.cell_count(), -200.0);

    assert!(matches!(
        snapshot.validate_against(&spatial, &land_relief, &geology, &climate),
        Err(HydroErosionValidationError::OceanClassificationMismatch { .. })
    ));
}

#[test]
fn ocean_runoff_must_be_exactly_zero() {
    let (spatial, relief, geology, climate, _) = valid_fixture();
    let snapshot = HydroErosionSnapshot::new(
        HYDRO_EROSION_SNAPSHOT_SCHEMA_V1,
        surface_fixture(spatial.cell_count()),
        ocean_hydrology_fixture(&spatial, 1.0),
    )
    .unwrap();

    assert!(matches!(
        snapshot.validate_against(&spatial, &relief, &geology, &climate),
        Err(HydroErosionValidationError::OceanRunoffNonZero { .. })
    ));
}

#[test]
fn land_runoff_must_match_precipitation_and_permeability() {
    let spatial = spatial_fixture(16);
    let count = spatial.cell_count();
    let relief = relief_fixture(count, -200.0);
    let geology = geology_fixture(count);
    let climate = climate_fixture(count);

    let valid = HydroErosionSnapshot::new(
        HYDRO_EROSION_SNAPSHOT_SCHEMA_V1,
        surface_fixture(count),
        land_hydrology_fixture(&spatial, 59.0),
    )
    .unwrap();
    valid
        .validate_against(&spatial, &relief, &geology, &climate)
        .unwrap();

    let invalid = HydroErosionSnapshot::new(
        HYDRO_EROSION_SNAPSHOT_SCHEMA_V1,
        surface_fixture(count),
        land_hydrology_fixture(&spatial, 58.0),
    )
    .unwrap();
    assert!(matches!(
        invalid.validate_against(&spatial, &relief, &geology, &climate),
        Err(HydroErosionValidationError::RunoffIdentityMismatch { .. })
    ));
}

#[test]
fn invalid_nested_json_cannot_hide_broken_surface_or_receiver() {
    let valid = valid_fixture().4;

    let mut bad_surface = serde_json::to_value(&valid).unwrap();
    bad_surface["surface"]["erosion_depth_m"][0] = serde_json::json!(-1.0);
    assert!(serde_json::from_value::<HydroErosionSnapshot>(bad_surface).is_err());

    let mut bad_receiver = serde_json::to_value(&valid).unwrap();
    bad_receiver["hydrology"]["flow_receiver"][0] = serde_json::json!(0);
    assert!(serde_json::from_value::<HydroErosionSnapshot>(bad_receiver).is_err());
}
