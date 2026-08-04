use sekai::generators::natural::{
    FluvialErosionGenerator, HydroErosionGenerator, HydrologyGenerator,
};
use sekai::world::natural::{
    BedrockKind, BedrockKindField, ElevationField, GeologicSnapshot, HydroErosionSpec,
    LandOceanField, LandOceanKind, MonthlyScalarField, MonthlyVectorField,
    PreliminaryClimateSnapshot, ReliefSnapshot, CLIMATE_MONTH_COUNT, GEOLOGIC_SNAPSHOT_SCHEMA_V1,
    PRELIMINARY_CLIMATE_SCHEMA_V1, RELIEF_SCHEMA_V2,
};
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

fn linear_spatial(cell_count: usize, cell_size_m: f64) -> SpatialSnapshot {
    let mut cells = Vec::new();
    for index in 0..cell_count {
        let x0 = index as f64 * cell_size_m;
        let x1 = x0 + cell_size_m;
        let mut neighbors = Vec::new();
        if index > 0 {
            neighbors.push(CellId::from_raw((index - 1) as u32));
        }
        if index + 1 < cell_count {
            neighbors.push(CellId::from_raw((index + 1) as u32));
        }
        cells.push(SpatialCell {
            id: CellId::from_raw(index as u32),
            site: point((x0 + x1) * 0.5, cell_size_m * 0.5),
            centroid: point((x0 + x1) * 0.5, cell_size_m * 0.5),
            area: SquareMeters::new(cell_size_m * cell_size_m).unwrap(),
            polygon: vec![
                point(x0, 0.0),
                point(x1, 0.0),
                point(x1, cell_size_m),
                point(x0, cell_size_m),
            ],
            neighbors,
        });
    }

    let mut edges = Vec::new();
    for index in 0..cell_count {
        let x0 = index as f64 * cell_size_m;
        let x1 = x0 + cell_size_m;
        for (start, end) in [
            ((x0, 0.0), (x1, 0.0)),
            ((x1, cell_size_m), (x0, cell_size_m)),
        ] {
            edges.push(SpatialEdge {
                id: EdgeId::from_raw(edges.len() as u32),
                start: point(start.0, start.1),
                end: point(end.0, end.1),
                length: meters(cell_size_m),
                cells: [Some(CellId::from_raw(index as u32)), None],
            });
        }
    }
    for boundary in 0..=cell_count {
        let x = boundary as f64 * cell_size_m;
        let owners = if boundary == 0 {
            [Some(CellId::from_raw(0)), None]
        } else if boundary == cell_count {
            [Some(CellId::from_raw((cell_count - 1) as u32)), None]
        } else {
            [
                Some(CellId::from_raw((boundary - 1) as u32)),
                Some(CellId::from_raw(boundary as u32)),
            ]
        };
        edges.push(SpatialEdge {
            id: EdgeId::from_raw(edges.len() as u32),
            start: point(x, 0.0),
            end: point(x, cell_size_m),
            length: meters(cell_size_m),
            cells: owners,
        });
    }

    SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        WorldRect::new(
            point(0.0, 0.0),
            point(cell_count as f64 * cell_size_m, cell_size_m),
        )
        .unwrap(),
        BoundaryCondition::Closed,
        cells,
        edges,
    )
    .unwrap()
}

fn relief(elevations: &[f32], sea_level_m: f32) -> ReliefSnapshot {
    let count = elevations.len();
    ReliefSnapshot::new(
        RELIEF_SCHEMA_V2,
        count as u32,
        sea_level_m,
        ElevationField::from_values(elevations.to_vec()).unwrap(),
        ElevationField::from_values(vec![0.0; count]).unwrap(),
        ElevationField::from_values(vec![0.0; count]).unwrap(),
        ElevationField::from_values(vec![0.0; count]).unwrap(),
        ElevationField::from_values(elevations.to_vec()).unwrap(),
        LandOceanField::from_kinds(
            elevations
                .iter()
                .map(|&value| LandOceanKind::classify(value, sea_level_m))
                .collect(),
        ),
    )
    .unwrap()
}

fn geology(
    count: usize,
    erosion_resistance: Vec<f32>,
    permeability: Vec<f32>,
    unrelated: f32,
) -> GeologicSnapshot {
    GeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V1,
        count as u32,
        BedrockKindField::from_kinds(vec![BedrockKind::ContinentalCrystalline; count]),
        vec![unrelated; count],
        erosion_resistance,
        permeability,
        vec![unrelated; count],
        vec![unrelated; count],
        vec![unrelated; count],
    )
    .unwrap()
}

fn climate(count: usize, monthly_precipitation_mm: f32) -> PreliminaryClimateSnapshot {
    PreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V1,
        count as u32,
        vec![0.0; count],
        vec![0.0; count],
        MonthlyScalarField::from_values(vec![[20.0; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        MonthlyScalarField::from_values(vec![
            [monthly_precipitation_mm; CLIMATE_MONTH_COUNT];
            count
        ])
        .unwrap(),
        MonthlyVectorField::from_values(vec![[[0.0, 0.0]; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        vec![20.0; count],
        vec![0.0; count],
        vec![monthly_precipitation_mm * CLIMATE_MONTH_COUNT as f32; count],
        vec![[0.0, 0.0]; count],
    )
    .unwrap()
}

fn spec(erosion_strength_permille: u16) -> HydroErosionSpec {
    HydroErosionSpec {
        river_discharge_threshold_deci_m3_s: 1,
        erosion_strength_permille,
        ..HydroErosionSpec::default()
    }
}

#[test]
fn same_inputs_produce_byte_identical_composite_snapshots() {
    let spatial = linear_spatial(4, 10_000.0);
    let relief = relief(&[200.0, 120.0, 60.0, -10.0], 0.0);
    let geology = geology(4, vec![0.2; 4], vec![0.3; 4], 0.0);
    let climate = climate(4, 500.0);

    let first =
        HydroErosionGenerator::generate(&spatial, &relief, &geology, &climate, &spec(1_000))
            .unwrap();
    let second =
        HydroErosionGenerator::generate(&spatial, &relief, &geology, &climate, &spec(1_000))
            .unwrap();

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_eq!(
        blake3::hash(&serde_json::to_vec(&first).unwrap())
            .to_hex()
            .as_str(),
        "59f982f1902fcaa81e601d91d270909526e0d9a5986a043c675f2924710b6b8f"
    );
}

#[test]
fn zero_erosion_strength_preserves_constructional_relief_exactly() {
    let spatial = linear_spatial(4, 10_000.0);
    let relief = relief(&[200.123, 120.456, 60.789, -10.0], 0.0);
    let geology = geology(4, vec![0.0; 4], vec![0.3; 4], 0.0);
    let climate = climate(4, 500.0);

    let output =
        HydroErosionGenerator::generate(&spatial, &relief, &geology, &climate, &spec(0)).unwrap();

    assert_eq!(
        output.surface().surface_elevation_m().values(),
        relief.elevation_m().values()
    );
    assert!(output
        .surface()
        .erosion_depth_m()
        .iter()
        .all(|&value| value == 0.0));
    assert!(output
        .surface()
        .deposition_thickness_m()
        .iter()
        .all(|&value| value == 0.0));
    assert_eq!(output.surface().sediment_export_m3(), 0.0);
}

#[test]
fn ocean_current_surface_preserves_constructional_relief_exactly() {
    let spatial = linear_spatial(4, 10_000.0);
    let relief = relief(&[200.0, 120.0, -60.25, -10.75], 0.0);
    let geology = geology(4, vec![0.0; 4], vec![0.3; 4], 0.0);
    let climate = climate(4, 500.0);
    let output =
        HydroErosionGenerator::generate(&spatial, &relief, &geology, &climate, &spec(1_000))
            .unwrap();

    for index in 2..4 {
        assert_eq!(
            output.surface().surface_elevation_m().values()[index],
            relief.elevation_m().values()[index]
        );
        assert_eq!(output.surface().erosion_depth_m()[index], 0.0);
        assert_eq!(output.surface().deposition_thickness_m()[index], 0.0);
    }
}

#[test]
fn softer_rock_more_water_and_steeper_slope_increase_incision() {
    let spatial = linear_spatial(3, 10_000.0);
    let base_relief = relief(&[200.0, 100.0, -10.0], 0.0);
    let wet = climate(3, 1_000.0);
    let dry = climate(3, 100.0);
    let controls = spec(1_000);

    let hydrology = HydrologyGenerator::generate(
        &spatial,
        base_relief.elevation_m(),
        0.0,
        &[0.0; 3],
        &wet,
        &controls,
    )
    .unwrap();
    let soft =
        FluvialErosionGenerator::generate(&spatial, &base_relief, &[0.0; 3], &hydrology, &controls)
            .unwrap();
    let resistant =
        FluvialErosionGenerator::generate(&spatial, &base_relief, &[0.9; 3], &hydrology, &controls)
            .unwrap();
    assert!(soft.erosion_depth_m()[0] > resistant.erosion_depth_m()[0]);

    let dry_hydrology = HydrologyGenerator::generate(
        &spatial,
        base_relief.elevation_m(),
        0.0,
        &[0.0; 3],
        &dry,
        &controls,
    )
    .unwrap();
    let dry_surface = FluvialErosionGenerator::generate(
        &spatial,
        &base_relief,
        &[0.0; 3],
        &dry_hydrology,
        &controls,
    )
    .unwrap();
    assert!(soft.erosion_depth_m()[0] > dry_surface.erosion_depth_m()[0]);

    let gentle_relief = relief(&[110.0, 100.0, -10.0], 0.0);
    let gentle_hydrology = HydrologyGenerator::generate(
        &spatial,
        gentle_relief.elevation_m(),
        0.0,
        &[0.0; 3],
        &wet,
        &controls,
    )
    .unwrap();
    let gentle = FluvialErosionGenerator::generate(
        &spatial,
        &gentle_relief,
        &[0.0; 3],
        &gentle_hydrology,
        &controls,
    )
    .unwrap();
    assert!(soft.erosion_depth_m()[0] > gentle.erosion_depth_m()[0]);
}

#[test]
fn flat_or_zero_flow_cells_do_not_incise() {
    let spatial = linear_spatial(3, 10_000.0);
    let flat_relief = relief(&[100.0, 100.0, 100.0], 0.0);
    let controls = spec(1_000);
    let wet = climate(3, 500.0);
    let flat_hydrology = HydrologyGenerator::generate(
        &spatial,
        flat_relief.elevation_m(),
        0.0,
        &[0.0; 3],
        &wet,
        &controls,
    )
    .unwrap();
    let flat = FluvialErosionGenerator::generate(
        &spatial,
        &flat_relief,
        &[0.0; 3],
        &flat_hydrology,
        &controls,
    )
    .unwrap();
    assert!(flat.erosion_depth_m().iter().all(|&value| value == 0.0));

    let sloped_relief = relief(&[200.0, 100.0, 0.0], -100.0);
    let no_rain = climate(3, 0.0);
    let no_flow_hydrology = HydrologyGenerator::generate(
        &spatial,
        sloped_relief.elevation_m(),
        -100.0,
        &[0.0; 3],
        &no_rain,
        &controls,
    )
    .unwrap();
    let no_flow = FluvialErosionGenerator::generate(
        &spatial,
        &sloped_relief,
        &[0.0; 3],
        &no_flow_hydrology,
        &controls,
    )
    .unwrap();
    assert!(no_flow.erosion_depth_m().iter().all(|&value| value == 0.0));
}

#[test]
fn low_energy_sources_retain_a_larger_sediment_fraction() {
    let spatial = linear_spatial(3, 10_000.0);
    let climate = climate(3, 1_000.0);
    let controls = spec(1_000);

    let low_relief = relief(&[101.0, 100.0, -10.0], 0.0);
    let low_hydrology = HydrologyGenerator::generate(
        &spatial,
        low_relief.elevation_m(),
        0.0,
        &[0.0; 3],
        &climate,
        &controls,
    )
    .unwrap();
    let low = FluvialErosionGenerator::generate(
        &spatial,
        &low_relief,
        &[0.0; 3],
        &low_hydrology,
        &controls,
    )
    .unwrap();

    let high_relief = relief(&[200.0, 100.0, -10.0], 0.0);
    let high_hydrology = HydrologyGenerator::generate(
        &spatial,
        high_relief.elevation_m(),
        0.0,
        &[0.0; 3],
        &climate,
        &controls,
    )
    .unwrap();
    let high = FluvialErosionGenerator::generate(
        &spatial,
        &high_relief,
        &[0.0; 3],
        &high_hydrology,
        &controls,
    )
    .unwrap();

    let low_fraction = low.deposition_thickness_m()[0] / low.erosion_depth_m()[0];
    let high_fraction = high.deposition_thickness_m()[0] / high.erosion_depth_m()[0];
    assert!(low_fraction > high_fraction);
}

#[test]
fn sediment_and_surface_identities_hold_and_final_hydrology_is_recomputed() {
    let spatial = linear_spatial(4, 10_000.0);
    let relief = relief(&[250.0, 150.0, 50.0, -10.0], 0.0);
    let geology = geology(4, vec![0.0; 4], vec![0.0; 4], 0.0);
    let climate = climate(4, 1_000.0);
    let controls = spec(1_000);
    let initial = HydrologyGenerator::generate(
        &spatial,
        relief.elevation_m(),
        relief.sea_level_m(),
        geology.relative_permeability(),
        &climate,
        &controls,
    )
    .unwrap();

    let output =
        HydroErosionGenerator::generate(&spatial, &relief, &geology, &climate, &controls).unwrap();
    output
        .surface()
        .validate_against(&spatial, &relief)
        .unwrap();
    let surface = output.surface();
    let mut terminal_export_m3 = 0.0;
    for index in 0..spatial.cell_count() {
        let cell = CellId::from_raw(index as u32);
        let area_m2 = spatial.cell(cell).unwrap().area.get();
        let incoming_m3 = initial
            .flow_receiver()
            .iter()
            .enumerate()
            .filter(|(_, receiver)| **receiver == Some(cell))
            .map(|(upstream, _)| surface.sediment_throughput_m3()[upstream])
            .sum::<f64>();
        let eroded_m3 = area_m2 * f64::from(surface.erosion_depth_m()[index]);
        let deposited_m3 = area_m2 * f64::from(surface.deposition_thickness_m()[index]);
        let outgoing_m3 = surface.sediment_throughput_m3()[index];
        let scale = (incoming_m3 + eroded_m3).abs().max(1.0);
        assert!(
            (incoming_m3 + eroded_m3 - deposited_m3 - outgoing_m3).abs() <= scale * 1.0e-9,
            "cell {index} does not conserve sediment"
        );
        if initial.flow_receiver()[index].is_none() {
            terminal_export_m3 += outgoing_m3;
        }
    }
    assert!(
        (terminal_export_m3 - surface.sediment_export_m3()).abs()
            <= terminal_export_m3.abs().max(1.0) * 1.0e-9
    );
    assert_ne!(
        output.surface().surface_elevation_m().values(),
        relief.elevation_m().values()
    );
    assert_ne!(
        output.hydrology().drainage_surface_elevation_m().values(),
        initial.drainage_surface_elevation_m().values()
    );

    let recomputed = HydrologyGenerator::generate(
        &spatial,
        output.surface().surface_elevation_m(),
        relief.sea_level_m(),
        geology.relative_permeability(),
        &climate,
        &controls,
    )
    .unwrap();
    assert_eq!(output.hydrology(), &recomputed);
}

#[test]
fn unrelated_geologic_potentials_do_not_affect_hydro_erosion() {
    let spatial = linear_spatial(4, 10_000.0);
    let relief = relief(&[250.0, 150.0, 50.0, -10.0], 0.0);
    let baseline = geology(4, vec![0.2; 4], vec![0.3; 4], 0.0);
    let unrelated = geology(4, vec![0.2; 4], vec![0.3; 4], 1.0);
    let climate = climate(4, 500.0);
    let controls = spec(1_000);

    let first =
        HydroErosionGenerator::generate(&spatial, &relief, &baseline, &climate, &controls).unwrap();
    let second =
        HydroErosionGenerator::generate(&spatial, &relief, &unrelated, &climate, &controls)
            .unwrap();
    assert_eq!(first, second);
}
