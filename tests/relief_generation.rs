use std::sync::OnceLock;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::engine::{derive_stage_seed, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{ReliefGenerator, TectonicGenerator};
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    BoundaryKind, BoundaryRecord, BoundarySegment, CrustKind, CrustKindField, LandOceanKind, Plate,
    PlateIdField, PlateVelocity, ReliefSnapshot, TectonicSnapshot, TectonicSpec,
    REGIONAL_OFFSET_MAX_M, REGIONAL_OFFSET_MIN_M, TECTONIC_SNAPSHOT_SCHEMA_V1,
};
use sekai::world::spatial::{
    SpatialCell, SpatialEdge, SpatialSnapshot, Topology, SPATIAL_SCHEMA_V1,
};
use sekai::world::{
    BoundaryCondition, BoundarySegmentId, CellId, EdgeId, Meters, PlanarSpaceSpec, PlateId,
    RootSeed, SquareMeters, WorldPoint, WorldRect,
};

const GRID_COLUMNS: usize = 8;
const GRID_ROWS: usize = 3;

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(meters(x), meters(y))
}

fn regular_grid() -> SpatialSnapshot {
    let mut cells = Vec::new();
    for row in 0..GRID_ROWS {
        for column in 0..GRID_COLUMNS {
            let id = row * GRID_COLUMNS + column;
            let mut neighbors = Vec::new();
            if column > 0 {
                neighbors.push(CellId::from_raw((id - 1) as u32));
            }
            if column + 1 < GRID_COLUMNS {
                neighbors.push(CellId::from_raw((id + 1) as u32));
            }
            if row > 0 {
                neighbors.push(CellId::from_raw((id - GRID_COLUMNS) as u32));
            }
            if row + 1 < GRID_ROWS {
                neighbors.push(CellId::from_raw((id + GRID_COLUMNS) as u32));
            }
            neighbors.sort_unstable();
            cells.push(SpatialCell {
                id: CellId::from_raw(id as u32),
                site: point(column as f64 + 0.5, row as f64 + 0.5),
                centroid: point(column as f64 + 0.5, row as f64 + 0.5),
                area: SquareMeters::new(1.0).unwrap(),
                polygon: vec![
                    point(column as f64, row as f64),
                    point(column as f64 + 1.0, row as f64),
                    point(column as f64 + 1.0, row as f64 + 1.0),
                    point(column as f64, row as f64 + 1.0),
                ],
                neighbors,
            });
        }
    }

    let mut edges = Vec::new();
    for y in 0..=GRID_ROWS {
        for x in 0..GRID_COLUMNS {
            let owners = if y == 0 {
                [Some(x), None]
            } else if y == GRID_ROWS {
                [Some((GRID_ROWS - 1) * GRID_COLUMNS + x), None]
            } else {
                [Some((y - 1) * GRID_COLUMNS + x), Some(y * GRID_COLUMNS + x)]
            };
            edges.push(spatial_edge(
                edges.len(),
                (x as f64, y as f64),
                (x as f64 + 1.0, y as f64),
                owners,
            ));
        }
    }
    for x in 0..=GRID_COLUMNS {
        for y in 0..GRID_ROWS {
            let owners = if x == 0 {
                [Some(y * GRID_COLUMNS), None]
            } else if x == GRID_COLUMNS {
                [Some(y * GRID_COLUMNS + GRID_COLUMNS - 1), None]
            } else {
                [Some(y * GRID_COLUMNS + x - 1), Some(y * GRID_COLUMNS + x)]
            };
            edges.push(spatial_edge(
                edges.len(),
                (x as f64, y as f64),
                (x as f64, y as f64 + 1.0),
                owners,
            ));
        }
    }

    SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        WorldRect::new(
            point(0.0, 0.0),
            point(GRID_COLUMNS as f64, GRID_ROWS as f64),
        )
        .unwrap(),
        BoundaryCondition::Closed,
        cells,
        edges,
    )
    .unwrap()
}

fn spatial_edge(
    id: usize,
    start: (f64, f64),
    end: (f64, f64),
    owners: [Option<usize>; 2],
) -> SpatialEdge {
    let start = point(start.0, start.1);
    let end = point(end.0, end.1);
    SpatialEdge {
        id: EdgeId::from_raw(id as u32),
        start,
        end,
        length: meters((end.x().get() - start.x().get()).hypot(end.y().get() - start.y().get())),
        cells: owners.map(|owner| owner.map(|id| CellId::from_raw(id as u32))),
    }
}

fn custom_tectonics(spatial: &SpatialSnapshot, kind: BoundaryKind) -> TectonicSnapshot {
    let split = GRID_COLUMNS / 2;
    let mut plate_ids = Vec::new();
    let mut crust = Vec::new();
    let mut thickness = Vec::new();
    for index in 0..spatial.cell_count() {
        let column = index % GRID_COLUMNS;
        let plate = usize::from(column >= split);
        plate_ids.push(PlateId::from_raw(plate as u32));
        let crust_kind = match kind {
            BoundaryKind::OceanicRidge => CrustKind::Oceanic,
            BoundaryKind::Subduction if plate == 0 => CrustKind::Oceanic,
            _ => CrustKind::Continental,
        };
        crust.push(crust_kind);
        thickness.push(match crust_kind {
            CrustKind::Oceanic => 7.0,
            CrustKind::Continental => 35.0,
        });
    }
    let cell_plates = PlateIdField::from_ids(plate_ids);
    let mut boundaries = vec![BoundaryRecord::none(); spatial.edges().len()];
    let mut member_edges = Vec::new();
    for edge in spatial.edges() {
        let [Some(first), Some(second)] = edge.cells else {
            continue;
        };
        if cell_plates.get(first.raw() as usize) != cell_plates.get(second.raw() as usize) {
            member_edges.push(edge.id);
        }
    }
    member_edges.sort_unstable();
    let segment_id = BoundarySegmentId::from_raw(0);
    let subducting_plate = (kind == BoundaryKind::Subduction).then_some(PlateId::from_raw(0));
    for &edge in &member_edges {
        boundaries[edge.raw() as usize] =
            BoundaryRecord::new(kind, 1.0, Some(segment_id), subducting_plate);
    }
    let segments = vec![BoundarySegment {
        id: segment_id,
        plates: [PlateId::from_raw(0), PlateId::from_raw(1)],
        kind,
        member_edges,
        mean_strength: 1.0,
        subducting_plate,
        direction: [0.0, 1.0],
    }];
    let snapshot = TectonicSnapshot::new(
        TECTONIC_SNAPSHOT_SCHEMA_V1,
        spatial.cell_count() as u32,
        spatial.edges().len() as u32,
        vec![
            Plate {
                id: PlateId::from_raw(0),
                seed_cell: CellId::from_raw(0),
                velocity: PlateVelocity::new(30, 0).unwrap(),
            },
            Plate {
                id: PlateId::from_raw(1),
                seed_cell: CellId::from_raw(split as u32),
                velocity: PlateVelocity::new(-30, 0).unwrap(),
            },
        ],
        cell_plates,
        CrustKindField::from_kinds(crust),
        thickness,
        boundaries,
        segments,
    )
    .unwrap();
    snapshot.validate_against(spatial).unwrap();
    snapshot
}

fn relief_rng(seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.relief", 1, "sekai.core"),
    ))
}

fn generate_relief(
    spatial: &SpatialSnapshot,
    tectonic: &TectonicSnapshot,
    seed: u64,
) -> ReliefSnapshot {
    ReliefGenerator::generate(
        spatial,
        tectonic,
        &mut relief_rng(seed),
        &mut Vec::<Diagnostic>::new(),
    )
    .unwrap()
}

fn generated_fixture() -> (&'static SpatialSnapshot, TectonicSnapshot) {
    static SPATIAL: OnceLock<SpatialSnapshot> = OnceLock::new();
    let spatial = SPATIAL.get_or_init(|| {
        PlanarVoronoiBuilder::build(
            &PlanarSpaceSpec {
                width: Meters::new(2_000_000.0).unwrap(),
                height: Meters::new(1_200_000.0).unwrap(),
                target_cell_count: 576,
                boundary: BoundaryCondition::Closed,
            },
            &mut ChaCha8Rng::seed_from_u64(9001),
        )
        .unwrap()
    });
    let mut tectonic_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new("natural.tectonics", 1, "sekai.core"),
    ));
    let tectonic =
        TectonicGenerator::generate(spatial, &TectonicSpec::default(), &mut tectonic_rng).unwrap();
    (spatial, tectonic)
}

fn cell_at(row: usize, column: usize) -> CellId {
    CellId::from_raw((row * GRID_COLUMNS + column) as u32)
}

#[test]
fn crust_base_separates_interiors_and_softens_both_margins() {
    let spatial = regular_grid();
    let tectonic = custom_tectonics(&spatial, BoundaryKind::Subduction);
    let relief = generate_relief(&spatial, &tectonic, 7);
    let base = relief.crust_base_elevation_m();
    let ocean_interior = base.get(cell_at(1, 0).raw() as usize).unwrap();
    let ocean_margin = base.get(cell_at(1, 3).raw() as usize).unwrap();
    let continental_margin = base.get(cell_at(1, 4).raw() as usize).unwrap();
    let continental_interior = base.get(cell_at(1, 7).raw() as usize).unwrap();

    assert!(continental_interior > ocean_interior);
    assert!(ocean_margin.abs() < ocean_interior.abs());
    assert!(continental_margin.abs() < continental_interior.abs());
}

#[test]
fn targeted_boundary_events_have_the_expected_signed_relief() {
    let spatial = regular_grid();

    let collision = generate_relief(
        &spatial,
        &custom_tectonics(&spatial, BoundaryKind::ContinentalCollision),
        7,
    );
    assert!(
        collision
            .tectonic_offset_m()
            .values()
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max)
            > 0.0
    );

    let subduction = generate_relief(
        &spatial,
        &custom_tectonics(&spatial, BoundaryKind::Subduction),
        7,
    );
    assert!(
        subduction
            .tectonic_offset_m()
            .get(cell_at(1, 3).raw() as usize)
            .unwrap()
            < 0.0
    );
    assert!(
        subduction
            .tectonic_offset_m()
            .get(cell_at(1, 4).raw() as usize)
            .unwrap()
            > 0.0
    );

    let ridge = generate_relief(
        &spatial,
        &custom_tectonics(&spatial, BoundaryKind::OceanicRidge),
        7,
    );
    assert!(
        ridge
            .tectonic_offset_m()
            .get(cell_at(1, 3).raw() as usize)
            .unwrap()
            > 0.0
    );

    let rift = generate_relief(
        &spatial,
        &custom_tectonics(&spatial, BoundaryKind::ContinentalRift),
        7,
    );
    assert!(
        rift.tectonic_offset_m()
            .get(cell_at(1, 3).raw() as usize)
            .unwrap()
            < 0.0
    );
}

#[test]
fn transform_is_weaker_than_collision_and_event_support_is_compact() {
    let spatial = regular_grid();
    let collision = generate_relief(
        &spatial,
        &custom_tectonics(&spatial, BoundaryKind::ContinentalCollision),
        7,
    );
    let transform = generate_relief(
        &spatial,
        &custom_tectonics(&spatial, BoundaryKind::Transform),
        7,
    );
    let collision_amplitude = collision
        .tectonic_offset_m()
        .values()
        .iter()
        .map(|value| value.abs())
        .fold(0.0_f32, f32::max);
    let transform_amplitude = transform
        .tectonic_offset_m()
        .values()
        .iter()
        .map(|value| value.abs())
        .fold(0.0_f32, f32::max);

    assert!(transform_amplitude < collision_amplitude);
    assert_eq!(
        collision
            .tectonic_offset_m()
            .get(cell_at(1, 0).raw() as usize),
        Some(0.0)
    );
    assert_eq!(
        collision
            .tectonic_offset_m()
            .get(cell_at(1, 7).raw() as usize),
        Some(0.0)
    );
}

#[test]
fn regional_relief_is_repeatable_bounded_and_near_zero_mean() {
    let (spatial, tectonic) = generated_fixture();
    let first = generate_relief(spatial, &tectonic, 42);
    let second = generate_relief(spatial, &tectonic, 42);
    let values = first.regional_offset_m().values();
    let mean = values.iter().map(|&value| f64::from(value)).sum::<f64>() / values.len() as f64;

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert!(values.iter().all(|value| {
        value.is_finite() && (REGIONAL_OFFSET_MIN_M..=REGIONAL_OFFSET_MAX_M).contains(value)
    }));
    assert!(mean.abs() < 10.0, "regional mean was {mean}");
}

#[test]
fn final_relief_is_explainable_and_default_has_land_and_ocean() {
    let (spatial, tectonic) = generated_fixture();
    let relief = generate_relief(spatial, &tectonic, 42);
    let mut counts = [0_usize; 2];
    for index in 0..spatial.cell_count() {
        let expected = relief.crust_base_elevation_m().values()[index]
            + relief.tectonic_offset_m().values()[index]
            + relief.regional_offset_m().values()[index];
        assert!((relief.elevation_m().values()[index] - expected).abs() <= 0.01);
        match relief
            .land_ocean_kind(CellId::from_raw(index as u32))
            .unwrap()
        {
            LandOceanKind::Ocean => counts[0] += 1,
            LandOceanKind::Land => counts[1] += 1,
        }
    }
    assert!(counts.iter().all(|&count| count > 0));
    println!("ocean={} land={}", counts[0], counts[1]);
}
