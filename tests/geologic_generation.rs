use std::path::Path;

use sekai::engine::{derive_stage_seed, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{GeologicGenerator, ReliefGenerator};
use sekai::world::natural::{
    BedrockKind, BoundaryKind, BoundaryRecord, BoundarySegment, CrustKind, CrustKindField,
    GeologicSnapshot, GeologicSpec, Hotspot, MantleActivity, MantleSnapshot, Plate, PlateIdField,
    PlateVelocity, ReliefSnapshot, TectonicSnapshot, MANTLE_SNAPSHOT_SCHEMA_V1,
    TECTONIC_SNAPSHOT_SCHEMA_V1,
};
use sekai::world::spatial::{
    SpatialCell, SpatialEdge, SpatialSnapshot, Topology, SPATIAL_SCHEMA_V1,
};
use sekai::world::{
    BoundaryCondition, BoundarySegmentId, CellId, EdgeId, HotspotId, Meters, PlateId, RootSeed,
    SquareMeters, WorldPoint, WorldRect,
};

const GRID_COLUMNS: usize = 8;
const GRID_ROWS: usize = 3;

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(meters(x), meters(y))
}

fn cell_at(row: usize, column: usize) -> CellId {
    CellId::from_raw((row * GRID_COLUMNS + column) as u32)
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
    for row_edge in 0..=GRID_ROWS {
        for column in 0..GRID_COLUMNS {
            let owners = if row_edge == 0 {
                [Some(column), None]
            } else if row_edge == GRID_ROWS {
                [Some((GRID_ROWS - 1) * GRID_COLUMNS + column), None]
            } else {
                [
                    Some((row_edge - 1) * GRID_COLUMNS + column),
                    Some(row_edge * GRID_COLUMNS + column),
                ]
            };
            edges.push(spatial_edge(
                edges.len(),
                (column as f64, row_edge as f64),
                (column as f64 + 1.0, row_edge as f64),
                owners,
            ));
        }
    }
    for column_edge in 0..=GRID_COLUMNS {
        for row in 0..GRID_ROWS {
            let owners = if column_edge == 0 {
                [Some(row * GRID_COLUMNS), None]
            } else if column_edge == GRID_COLUMNS {
                [Some(row * GRID_COLUMNS + GRID_COLUMNS - 1), None]
            } else {
                [
                    Some(row * GRID_COLUMNS + column_edge - 1),
                    Some(row * GRID_COLUMNS + column_edge),
                ]
            };
            edges.push(spatial_edge(
                edges.len(),
                (column_edge as f64, row as f64),
                (column_edge as f64, row as f64 + 1.0),
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
        cells: owners.map(|owner| owner.map(|index| CellId::from_raw(index as u32))),
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
            BoundaryKind::Subduction if plate == 0 => CrustKind::Oceanic,
            BoundaryKind::OceanicRidge => CrustKind::Oceanic,
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
        vec![BoundarySegment {
            id: segment_id,
            plates: [PlateId::from_raw(0), PlateId::from_raw(1)],
            kind,
            member_edges,
            mean_strength: 1.0,
            subducting_plate,
            direction: [0.0, 1.0],
        }],
    )
    .unwrap();
    snapshot.validate_against(spatial).unwrap();
    snapshot
}

fn zero_mantle(spatial: &SpatialSnapshot) -> MantleSnapshot {
    MantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V1,
        spatial.cell_count() as u32,
        Vec::new(),
        vec![65.0; spatial.cell_count()],
        vec![0.0; spatial.cell_count()],
    )
    .unwrap()
}

fn hotspot_mantle(spatial: &SpatialSnapshot, source: CellId) -> MantleSnapshot {
    let mut heat = vec![65.0; spatial.cell_count()];
    let mut influence = vec![0.0; spatial.cell_count()];
    heat[source.raw() as usize] = 285.0;
    influence[source.raw() as usize] = 1.0;
    MantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V1,
        spatial.cell_count() as u32,
        vec![Hotspot::new(HotspotId::from_raw(0), source, 900, meters(2.0)).unwrap()],
        heat,
        influence,
    )
    .unwrap()
}

fn relief_rng(seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.relief", 6, "sekai.core"),
    ))
}

fn geology_rng(seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.geology", 1, "sekai.core"),
    ))
}

fn generate_relief(
    spatial: &SpatialSnapshot,
    tectonic: &TectonicSnapshot,
    mantle: &MantleSnapshot,
) -> ReliefSnapshot {
    ReliefGenerator::generate(
        spatial,
        tectonic,
        mantle,
        &mut relief_rng(17),
        &mut Vec::<Diagnostic>::new(),
    )
    .unwrap()
}

fn generate_geology(
    spatial: &SpatialSnapshot,
    tectonic: &TectonicSnapshot,
    mantle: &MantleSnapshot,
    relief: &ReliefSnapshot,
    seed: u64,
) -> GeologicSnapshot {
    let spec = GeologicSpec {
        hotspot_count: mantle.hotspots().len() as u16,
        mantle_activity: MantleActivity::Moderate,
        ..GeologicSpec::default()
    };
    GeologicGenerator::generate(
        spatial,
        tectonic,
        mantle,
        relief,
        &spec,
        &mut geology_rng(seed),
    )
    .unwrap()
}

#[test]
fn generation_is_repeatable_seed_sensitive_complete_and_does_not_mutate_inputs() {
    let spatial = regular_grid();
    let tectonic = custom_tectonics(&spatial, BoundaryKind::Subduction);
    let mantle = zero_mantle(&spatial);
    let relief = generate_relief(&spatial, &tectonic, &mantle);
    let upstream_before = serde_json::to_vec(&(&spatial, &tectonic, &mantle, &relief)).unwrap();

    let first = generate_geology(&spatial, &tectonic, &mantle, &relief, 41);
    let repeated = generate_geology(&spatial, &tectonic, &mantle, &relief, 41);
    let different = generate_geology(&spatial, &tectonic, &mantle, &relief, 42);

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    assert_ne!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&different).unwrap()
    );
    assert_eq!(
        upstream_before,
        serde_json::to_vec(&(&spatial, &tectonic, &mantle, &relief)).unwrap()
    );
    assert_eq!(
        first.bedrock_kinds().len(),
        spatial.cell_count(),
        "every spatial cell must be classified"
    );
    assert!((0..spatial.cell_count())
        .all(|index| first.bedrock_kind(CellId::from_raw(index as u32)).is_some()));
    assert!((0..spatial.cell_count())
        .any(|index| first.bedrock_kind(CellId::from_raw(index as u32))
            == Some(BedrockKind::OceanicMafic)));
    assert!((0..spatial.cell_count())
        .any(|index| first.bedrock_kind(CellId::from_raw(index as u32))
            == Some(BedrockKind::ContinentalCrystalline)));
    first
        .validate_against(&spatial, &tectonic, &mantle, &relief)
        .unwrap();
}

#[test]
fn collision_creates_metamorphic_fractured_metallic_margin() {
    let spatial = regular_grid();
    let tectonic = custom_tectonics(&spatial, BoundaryKind::ContinentalCollision);
    let mantle = zero_mantle(&spatial);
    let relief = generate_relief(&spatial, &tectonic, &mantle);
    let geology = generate_geology(&spatial, &tectonic, &mantle, &relief, 7);
    let margin = cell_at(1, 3).raw() as usize;
    let interior = cell_at(1, 1).raw() as usize;

    assert_eq!(
        geology.bedrock_kind(CellId::from_raw(margin as u32)),
        Some(BedrockKind::Metamorphic)
    );
    assert!(geology.fracture_intensity()[margin] > geology.fracture_intensity()[interior]);
    assert!(geology.erosion_resistance()[margin] < geology.erosion_resistance()[interior]);
    assert!(geology.relative_permeability()[margin] > geology.relative_permeability()[interior]);
    assert!(
        geology.metallic_mineral_potential()[margin]
            > geology.metallic_mineral_potential()[interior]
    );
}

#[test]
fn hotspot_has_priority_and_heat_is_monotonic_at_equal_fracture() {
    let spatial = regular_grid();
    let tectonic = custom_tectonics(&spatial, BoundaryKind::ContinentalCollision);
    let source = cell_at(1, 0);
    let mantle = hotspot_mantle(&spatial, source);
    let relief = generate_relief(&spatial, &tectonic, &mantle);
    let geology = generate_geology(&spatial, &tectonic, &mantle, &relief, 7);
    assert_eq!(geology.bedrock_kind(source), Some(BedrockKind::Volcanic));

    let low = cell_at(0, 7);
    let high = cell_at(2, 7);
    let mut json = serde_json::to_value(zero_mantle(&spatial)).unwrap();
    json["heat_flow_mw_m2"][high.raw() as usize] = serde_json::json!(300.0);
    let heat_contrast: MantleSnapshot = serde_json::from_value(json).unwrap();
    let contrast_relief = generate_relief(&spatial, &tectonic, &heat_contrast);
    let contrast = generate_geology(&spatial, &tectonic, &heat_contrast, &contrast_relief, 7);
    assert_eq!(
        contrast.fracture_intensity()[low.raw() as usize],
        contrast.fracture_intensity()[high.raw() as usize]
    );
    assert!(
        contrast.geothermal_potential()[high.raw() as usize]
            > contrast.geothermal_potential()[low.raw() as usize]
    );
}

#[test]
fn broad_rift_subsidence_creates_sedimentary_cells_and_independent_potentials() {
    let spatial = regular_grid();
    let tectonic = custom_tectonics(&spatial, BoundaryKind::ContinentalRift);
    let mantle = zero_mantle(&spatial);
    let relief = generate_relief(&spatial, &tectonic, &mantle);
    let geology = generate_geology(&spatial, &tectonic, &mantle, &relief, 99);

    assert!((0..spatial.cell_count()).any(|index| {
        geology.bedrock_kind(CellId::from_raw(index as u32)) == Some(BedrockKind::Sedimentary)
    }));
    assert_ne!(
        geology.metallic_mineral_potential(),
        geology.geothermal_potential()
    );
    assert_ne!(
        geology.geothermal_potential(),
        geology.sedimentary_basin_potential()
    );
    assert!(geology
        .sedimentary_basin_potential()
        .windows(2)
        .any(|pair| pair[0] != pair[1]));
}

#[test]
fn property_formulas_are_bounded_and_match_category_bases() {
    let spatial = regular_grid();
    let tectonic = custom_tectonics(&spatial, BoundaryKind::Subduction);
    let mantle = zero_mantle(&spatial);
    let relief = generate_relief(&spatial, &tectonic, &mantle);
    let geology = generate_geology(&spatial, &tectonic, &mantle, &relief, 41);

    for index in 0..spatial.cell_count() {
        let cell = CellId::from_raw(index as u32);
        let fracture = geology.fracture_intensity()[index];
        let (base_resistance, base_permeability) = match geology.bedrock_kind(cell).unwrap() {
            BedrockKind::OceanicMafic => (0.78, 0.18),
            BedrockKind::ContinentalCrystalline => (0.86, 0.12),
            BedrockKind::Sedimentary => (0.42, 0.58),
            BedrockKind::Metamorphic => (0.82, 0.10),
            BedrockKind::Volcanic => (0.68, 0.24),
        };
        let expected_resistance = (base_resistance - 0.30 * fracture).clamp(0.0, 1.0);
        let expected_permeability =
            (base_permeability + 0.55 * fracture * (1.0 - base_permeability)).clamp(0.0, 1.0);
        assert!((geology.erosion_resistance()[index] - expected_resistance).abs() < 1.0e-6);
        assert!((geology.relative_permeability()[index] - expected_permeability).abs() < 1.0e-6);
    }
    for values in [
        geology.fracture_intensity(),
        geology.erosion_resistance(),
        geology.relative_permeability(),
        geology.metallic_mineral_potential(),
        geology.geothermal_potential(),
        geology.sedimentary_basin_potential(),
    ] {
        assert!(values
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
    }
}

#[test]
fn generator_keeps_domain_and_presentation_dependencies_orthogonal() {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/generators/natural/geology.rs"),
    )
    .unwrap();
    for forbidden in [
        "crate::terrain",
        "crate::app",
        "crate::view",
        "egui",
        "wgpu",
    ] {
        assert!(
            !source.contains(forbidden),
            "geologic generator must not depend on {forbidden}"
        );
    }
}
