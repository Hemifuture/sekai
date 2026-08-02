use sekai::generators::natural::HydrologyGenerator;
use sekai::world::natural::{
    BasinOutletKind, ElevationField, HydroErosionSpec, PreliminaryClimateSnapshot,
    RiverSegmentKind, SurfaceWaterKind, CLIMATE_MONTH_COUNT, PRELIMINARY_CLIMATE_SCHEMA_V1,
};
use sekai::world::natural::{MonthlyScalarField, MonthlyVectorField};
use sekai::world::spatial::{
    SpatialCell, SpatialEdge, SpatialSnapshot, Topology, SPATIAL_SCHEMA_V1,
};
use sekai::world::{
    BoundaryCondition, CellId, EdgeId, Meters, SquareMeters, WorldPoint, WorldRect,
};

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(meters(x), meters(y))
}

fn grid_spatial(width: usize, height: usize, cell_size_m: f64) -> SpatialSnapshot {
    let mut cells = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let x0 = x as f64 * cell_size_m;
            let y0 = y as f64 * cell_size_m;
            let x1 = x0 + cell_size_m;
            let y1 = y0 + cell_size_m;
            let mut neighbors = Vec::new();
            if x > 0 {
                neighbors.push(CellId::from_raw((index - 1) as u32));
            }
            if x + 1 < width {
                neighbors.push(CellId::from_raw((index + 1) as u32));
            }
            if y > 0 {
                neighbors.push(CellId::from_raw((index - width) as u32));
            }
            if y + 1 < height {
                neighbors.push(CellId::from_raw((index + width) as u32));
            }
            neighbors.sort();
            cells.push(SpatialCell {
                id: CellId::from_raw(index as u32),
                site: point((x0 + x1) * 0.5, (y0 + y1) * 0.5),
                centroid: point((x0 + x1) * 0.5, (y0 + y1) * 0.5),
                area: SquareMeters::new(cell_size_m * cell_size_m).unwrap(),
                polygon: vec![point(x0, y0), point(x1, y0), point(x1, y1), point(x0, y1)],
                neighbors,
            });
        }
    }

    let mut edges = Vec::new();
    for y in 0..=height {
        for x in 0..width {
            let owners = if y == 0 {
                [Some((x) as u32), None]
            } else if y == height {
                [Some(((height - 1) * width + x) as u32), None]
            } else {
                [
                    Some(((y - 1) * width + x) as u32),
                    Some((y * width + x) as u32),
                ]
            };
            edges.push(SpatialEdge {
                id: EdgeId::from_raw(edges.len() as u32),
                start: point(x as f64 * cell_size_m, y as f64 * cell_size_m),
                end: point((x + 1) as f64 * cell_size_m, y as f64 * cell_size_m),
                length: meters(cell_size_m),
                cells: owners.map(|owner| owner.map(CellId::from_raw)),
            });
        }
    }
    for x in 0..=width {
        for y in 0..height {
            let owners = if x == 0 {
                [Some((y * width) as u32), None]
            } else if x == width {
                [Some((y * width + width - 1) as u32), None]
            } else {
                [
                    Some((y * width + x - 1) as u32),
                    Some((y * width + x) as u32),
                ]
            };
            edges.push(SpatialEdge {
                id: EdgeId::from_raw(edges.len() as u32),
                start: point(x as f64 * cell_size_m, y as f64 * cell_size_m),
                end: point(x as f64 * cell_size_m, (y + 1) as f64 * cell_size_m),
                length: meters(cell_size_m),
                cells: owners.map(|owner| owner.map(CellId::from_raw)),
            });
        }
    }

    SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        WorldRect::new(
            point(0.0, 0.0),
            point(width as f64 * cell_size_m, height as f64 * cell_size_m),
        )
        .unwrap(),
        BoundaryCondition::Closed,
        cells,
        edges,
    )
    .unwrap()
}

fn climate(
    cell_count: usize,
    precipitation: impl Fn(usize, usize) -> f32,
) -> PreliminaryClimateSnapshot {
    let monthly_precipitation = (0..cell_count)
        .map(|cell| std::array::from_fn(|month| precipitation(cell, month)))
        .collect::<Vec<_>>();
    let annual_precipitation = monthly_precipitation
        .iter()
        .map(|months| months.iter().sum())
        .collect();
    PreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V1,
        cell_count as u32,
        vec![0.0; cell_count],
        vec![0.0; cell_count],
        MonthlyScalarField::from_values(vec![[20.0; CLIMATE_MONTH_COUNT]; cell_count]).unwrap(),
        MonthlyScalarField::from_values(monthly_precipitation).unwrap(),
        MonthlyVectorField::from_values(vec![[[0.0, 0.0]; CLIMATE_MONTH_COUNT]; cell_count])
            .unwrap(),
        vec![20.0; cell_count],
        vec![0.0; cell_count],
        annual_precipitation,
        vec![[0.0, 0.0]; cell_count],
    )
    .unwrap()
}

fn low_threshold_spec() -> HydroErosionSpec {
    HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: 1,
        ..HydroErosionSpec::default()
    }
}

fn generate(
    spatial: &SpatialSnapshot,
    elevations: &[f32],
    sea_level_m: f32,
    permeability: &[f32],
    climate: &PreliminaryClimateSnapshot,
    spec: &HydroErosionSpec,
) -> sekai::world::natural::HydrologySnapshot {
    HydrologyGenerator::generate(
        spatial,
        &ElevationField::from_values(elevations.to_vec()).unwrap(),
        sea_level_m,
        permeability,
        climate,
        spec,
    )
    .unwrap()
}

#[test]
fn same_inputs_produce_byte_identical_hydrology() {
    let spatial = grid_spatial(5, 3, 10_000.0);
    let elevations = vec![
        -10.0, 10.0, 10.0, 10.0, 10.0, -10.0, 5.0, 1.0, 1.0, 10.0, -10.0, 10.0, 10.0, 10.0, 10.0,
    ];
    let climate = climate(spatial.cell_count(), |_, _| 100.0);
    let permeability = vec![0.25; spatial.cell_count()];
    let spec = low_threshold_spec();

    let first = generate(&spatial, &elevations, 0.0, &permeability, &climate, &spec);
    let second = generate(&spatial, &elevations, 0.0, &permeability, &climate, &spec);

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}

#[test]
fn synthetic_bowl_produces_one_stable_lake_and_real_outlet() {
    let spatial = grid_spatial(5, 3, 10_000.0);
    let elevations = vec![
        -10.0, 10.0, 10.0, 10.0, 10.0, -10.0, 5.0, 1.0, 1.0, 10.0, -10.0, 10.0, 10.0, 10.0, 10.0,
    ];
    let climate = climate(spatial.cell_count(), |_, _| 100.0);
    let snapshot = generate(
        &spatial,
        &elevations,
        0.0,
        &vec![0.0; spatial.cell_count()],
        &climate,
        &low_threshold_spec(),
    );

    assert_eq!(snapshot.lakes().len(), 1);
    let lake = &snapshot.lakes()[0];
    assert_eq!(lake.cells(), &[CellId::from_raw(7), CellId::from_raw(8)]);
    assert_eq!(lake.surface_elevation_m(), 5.0);
    assert_eq!(lake.outlet_cell(), Some(CellId::from_raw(7)));
    assert_eq!(lake.downstream_cell(), Some(CellId::from_raw(6)));
    assert_eq!(snapshot.lake_depth_m()[7], 4.0);
    assert_eq!(snapshot.lake_depth_m()[8], 4.0);
    assert!(snapshot.river_segments().iter().any(|segment| {
        segment.from() == CellId::from_raw(7)
            && segment.to() == CellId::from_raw(6)
            && segment.kind() == RiverSegmentKind::LakeOutlet
    }));
    assert!(!snapshot
        .river_segments()
        .iter()
        .any(|segment| segment.from() == CellId::from_raw(8)));
}

#[test]
fn ridge_forms_stable_separate_terminal_basins() {
    let spatial = grid_spatial(5, 1, 10_000.0);
    let climate = climate(spatial.cell_count(), |_, _| 50.0);
    let snapshot = generate(
        &spatial,
        &[-10.0, 10.0, 100.0, 10.0, -10.0],
        0.0,
        &vec![0.5; spatial.cell_count()],
        &climate,
        &HydroErosionSpec::default(),
    );

    assert_eq!(snapshot.basins().len(), 2);
    assert_eq!(snapshot.basins()[0].outlet_cell(), CellId::from_raw(0));
    assert_eq!(snapshot.basins()[1].outlet_cell(), CellId::from_raw(4));
    assert_eq!(snapshot.basin_id()[1], snapshot.basin_id()[2]);
    assert_ne!(snapshot.basin_id()[2], snapshot.basin_id()[3]);
}

#[test]
fn flat_all_land_and_all_ocean_worlds_are_defined_and_acyclic() {
    let spatial = grid_spatial(3, 3, 1_000.0);
    let climate = climate(spatial.cell_count(), |_, _| 10.0);
    let permeability = vec![0.5; spatial.cell_count()];

    let land = generate(
        &spatial,
        &vec![100.0; spatial.cell_count()],
        0.0,
        &permeability,
        &climate,
        &HydroErosionSpec::default(),
    );
    assert_eq!(land.basins().len(), 1);
    assert_eq!(land.basins()[0].outlet_kind(), BasinOutletKind::ClosedSink);
    assert_eq!(land.basins()[0].outlet_cell(), CellId::from_raw(0));
    assert_receiver_graph_is_adjacent_and_acyclic(&spatial, &land);

    let ocean = generate(
        &spatial,
        &vec![-100.0; spatial.cell_count()],
        0.0,
        &permeability,
        &climate,
        &HydroErosionSpec::default(),
    );
    assert!(ocean.basins().is_empty());
    assert!(ocean
        .surface_water()
        .raw_values()
        .iter()
        .all(|&raw| raw == SurfaceWaterKind::Ocean.raw()));
    assert!(ocean.flow_receiver().iter().all(Option::is_none));
}

#[test]
fn permeability_and_monthly_precipitation_causally_control_flow() {
    let spatial = grid_spatial(3, 3, 10_000.0);
    let elevations = vec![100.0; spatial.cell_count()];
    let baseline_climate = climate(spatial.cell_count(), |_, _| 100.0);
    let wetter_climate = climate(
        spatial.cell_count(),
        |_, month| {
            if month == 4 {
                200.0
            } else {
                100.0
            }
        },
    );
    let spec = HydroErosionSpec::default();

    let impermeable = generate(
        &spatial,
        &elevations,
        0.0,
        &vec![0.0; spatial.cell_count()],
        &baseline_climate,
        &spec,
    );
    let permeable = generate(
        &spatial,
        &elevations,
        0.0,
        &vec![1.0; spatial.cell_count()],
        &baseline_climate,
        &spec,
    );
    assert!(
        impermeable.monthly_local_runoff_mm()[0][0] > permeable.monthly_local_runoff_mm()[0][0]
    );
    assert!(impermeable.monthly_discharge_m3_s()[0][0] > permeable.monthly_discharge_m3_s()[0][0]);

    let wetter = generate(
        &spatial,
        &elevations,
        0.0,
        &vec![0.0; spatial.cell_count()],
        &wetter_climate,
        &spec,
    );
    assert!(wetter.monthly_local_runoff_mm()[0][4] > impermeable.monthly_local_runoff_mm()[0][4]);
    assert!(wetter.monthly_discharge_m3_s()[0][4] > impermeable.monthly_discharge_m3_s()[0][4]);
}

#[test]
fn accumulated_water_equals_local_plus_direct_upstreams() {
    let spatial = grid_spatial(4, 4, 10_000.0);
    let climate = climate(spatial.cell_count(), |_, month| 25.0 + month as f32);
    let snapshot = generate(
        &spatial,
        &vec![100.0; spatial.cell_count()],
        0.0,
        &vec![0.3; spatial.cell_count()],
        &climate,
        &HydroErosionSpec::default(),
    );

    for index in 0..spatial.cell_count() {
        let cell = CellId::from_raw(index as u32);
        let area_m2 = spatial.cell(cell).unwrap().area.get();
        for month in 0..CLIMATE_MONTH_COUNT {
            let local = f64::from(snapshot.monthly_local_runoff_mm()[index][month]) / 1_000.0
                * area_m2
                / sekai::world::natural::SECONDS_PER_CLIMATOLOGICAL_MONTH;
            let upstream = snapshot
                .flow_receiver()
                .iter()
                .enumerate()
                .filter(|(_, receiver)| **receiver == Some(cell))
                .map(|(upstream, _)| f64::from(snapshot.monthly_discharge_m3_s()[upstream][month]))
                .sum::<f64>();
            let stored = f64::from(snapshot.monthly_discharge_m3_s()[index][month]);
            let tolerance = 1.0e-6_f64.max(stored.abs() * 1.0e-5);
            assert!(
                (stored - (local + upstream)).abs() <= tolerance,
                "cell {index} month {month}"
            );
        }
    }
}

#[test]
fn strahler_order_follows_thresholded_branching_dag() {
    let spatial = grid_spatial(3, 2, 10_000.0);
    let climate = climate(spatial.cell_count(), |_, _| 100.0);
    let snapshot = generate(
        &spatial,
        &[30.0, 20.0, 30.0, 40.0, -10.0, 40.0],
        0.0,
        &vec![0.0; spatial.cell_count()],
        &climate,
        &low_threshold_spec(),
    );

    let junction = snapshot
        .river_segments()
        .iter()
        .find(|segment| segment.from() == CellId::from_raw(1))
        .unwrap();
    assert_eq!(junction.to(), CellId::from_raw(4));
    assert_eq!(junction.strahler_order(), 2);
    assert_eq!(snapshot.strahler_order().get(1), Some(2));
}

fn assert_receiver_graph_is_adjacent_and_acyclic(
    spatial: &SpatialSnapshot,
    snapshot: &sekai::world::natural::HydrologySnapshot,
) {
    for (index, &receiver) in snapshot.flow_receiver().iter().enumerate() {
        let cell = CellId::from_raw(index as u32);
        if let Some(receiver) = receiver {
            assert!(spatial.neighbors(cell).unwrap().contains(&receiver));
        }

        let mut seen = vec![false; spatial.cell_count()];
        let mut current = Some(cell);
        while let Some(cell) = current {
            let index = cell.raw() as usize;
            assert!(!seen[index], "receiver cycle through {cell:?}");
            seen[index] = true;
            current = snapshot.flow_receiver()[index];
        }
    }
}
