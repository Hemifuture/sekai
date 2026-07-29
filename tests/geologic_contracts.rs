use sekai::world::natural::{
    BedrockKind, BedrockKindField, CrustKind, CrustKindField, ElevationField, GeologicSnapshot,
    GeologicValidationError, LandOceanField, LandOceanKind, MantleSnapshot, Plate, PlateIdField,
    PlateVelocity, ReliefSnapshot, TectonicSnapshot, GEOLOGIC_SNAPSHOT_SCHEMA_V1,
    MANTLE_SNAPSHOT_SCHEMA_V1, RELIEF_SCHEMA_V2, TECTONIC_SNAPSHOT_SCHEMA_V1,
};
use sekai::world::spatial::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
use sekai::world::{
    BoundaryCondition, CellId, EdgeId, Meters, PlateId, SquareMeters, WorldPoint, WorldRect,
};

const CELL_COUNT: usize = 4;

fn meters(value: f64) -> Meters {
    Meters::new(value).unwrap()
}

fn point(x: f64, y: f64) -> WorldPoint {
    WorldPoint::new(meters(x), meters(y))
}

fn cell(id: u32, site: (f64, f64), polygon: &[(f64, f64)], neighbors: &[u32]) -> SpatialCell {
    SpatialCell {
        id: CellId::from_raw(id),
        site: point(site.0, site.1),
        centroid: point(site.0, site.1),
        area: SquareMeters::new(1.0).unwrap(),
        polygon: polygon.iter().map(|&(x, y)| point(x, y)).collect(),
        neighbors: neighbors.iter().copied().map(CellId::from_raw).collect(),
    }
}

fn edge(id: u32, start: (f64, f64), end: (f64, f64), cells: [Option<u32>; 2]) -> SpatialEdge {
    let start = point(start.0, start.1);
    let end = point(end.0, end.1);
    SpatialEdge {
        id: EdgeId::from_raw(id),
        start,
        end,
        length: meters((end.x().get() - start.x().get()).hypot(end.y().get() - start.y().get())),
        cells: cells.map(|owner| owner.map(CellId::from_raw)),
    }
}

fn spatial_fixture() -> SpatialSnapshot {
    let cells = vec![
        cell(
            0,
            (0.5, 0.5),
            &[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
            &[1, 2],
        ),
        cell(
            1,
            (1.5, 0.5),
            &[(1.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0)],
            &[0, 3],
        ),
        cell(
            2,
            (0.5, 1.5),
            &[(0.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)],
            &[0, 3],
        ),
        cell(
            3,
            (1.5, 1.5),
            &[(1.0, 1.0), (2.0, 1.0), (2.0, 2.0), (1.0, 2.0)],
            &[1, 2],
        ),
    ];
    let edges = vec![
        edge(0, (0.0, 0.0), (1.0, 0.0), [Some(0), None]),
        edge(1, (0.0, 1.0), (0.0, 0.0), [Some(0), None]),
        edge(2, (1.0, 0.0), (2.0, 0.0), [Some(1), None]),
        edge(3, (2.0, 0.0), (2.0, 1.0), [Some(1), None]),
        edge(4, (0.0, 2.0), (0.0, 1.0), [Some(2), None]),
        edge(5, (1.0, 2.0), (0.0, 2.0), [Some(2), None]),
        edge(6, (2.0, 1.0), (2.0, 2.0), [Some(3), None]),
        edge(7, (2.0, 2.0), (1.0, 2.0), [Some(3), None]),
        edge(8, (1.0, 0.0), (1.0, 1.0), [Some(0), Some(1)]),
        edge(9, (1.0, 1.0), (0.0, 1.0), [Some(0), Some(2)]),
        edge(10, (1.0, 1.0), (2.0, 1.0), [Some(1), Some(3)]),
        edge(11, (1.0, 1.0), (1.0, 2.0), [Some(2), Some(3)]),
    ];
    SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        WorldRect::new(point(0.0, 0.0), point(2.0, 2.0)).unwrap(),
        BoundaryCondition::Closed,
        cells,
        edges,
    )
    .unwrap()
}

fn tectonic_fixture(crust: Vec<CrustKind>) -> TectonicSnapshot {
    let count = crust.len();
    TectonicSnapshot::new(
        TECTONIC_SNAPSHOT_SCHEMA_V1,
        count as u32,
        0,
        vec![Plate {
            id: PlateId::from_raw(0),
            seed_cell: CellId::from_raw(0),
            velocity: PlateVelocity::new(0, 0).unwrap(),
        }],
        PlateIdField::from_ids(vec![PlateId::from_raw(0); count]),
        CrustKindField::from_kinds(crust.clone()),
        crust
            .into_iter()
            .map(|kind| match kind {
                CrustKind::Oceanic => 7.0,
                CrustKind::Continental => 35.0,
            })
            .collect(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

fn mantle_fixture(cell_count: usize) -> MantleSnapshot {
    MantleSnapshot::new(
        MANTLE_SNAPSHOT_SCHEMA_V1,
        cell_count as u32,
        Vec::new(),
        vec![65.0; cell_count],
        vec![0.0; cell_count],
    )
    .unwrap()
}

fn relief_fixture(cell_count: usize) -> ReliefSnapshot {
    let zero = || ElevationField::from_values(vec![0.0; cell_count]).unwrap();
    ReliefSnapshot::new(
        RELIEF_SCHEMA_V2,
        cell_count as u32,
        0.0,
        zero(),
        zero(),
        zero(),
        zero(),
        zero(),
        LandOceanField::from_kinds(vec![LandOceanKind::Land; cell_count]),
    )
    .unwrap()
}

fn valid_bedrock() -> BedrockKindField {
    BedrockKindField::new(vec![0, 1, 2, 4]).unwrap()
}

fn snapshot_with_bedrock(bedrock: BedrockKindField) -> GeologicSnapshot {
    GeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V1,
        CELL_COUNT as u32,
        bedrock,
        vec![0.2, 0.4, 0.1, 0.8],
        vec![0.8, 0.7, 0.4, 0.5],
        vec![0.1, 0.2, 0.7, 0.6],
        vec![0.2, 0.3, 0.1, 0.9],
        vec![0.1, 0.2, 0.3, 0.8],
        vec![0.0, 0.1, 0.9, 0.2],
    )
    .unwrap()
}

fn valid_snapshot() -> GeologicSnapshot {
    snapshot_with_bedrock(valid_bedrock())
}

fn upstream_tectonic() -> TectonicSnapshot {
    tectonic_fixture(vec![
        CrustKind::Oceanic,
        CrustKind::Continental,
        CrustKind::Oceanic,
        CrustKind::Continental,
    ])
}

#[test]
fn bedrock_category_codes_are_exact_and_checked() {
    assert_eq!(BedrockKind::OceanicMafic.raw(), 0);
    assert_eq!(BedrockKind::ContinentalCrystalline.raw(), 1);
    assert_eq!(BedrockKind::Sedimentary.raw(), 2);
    assert_eq!(BedrockKind::Metamorphic.raw(), 3);
    assert_eq!(BedrockKind::Volcanic.raw(), 4);
    for raw in 0..=4 {
        assert_eq!(BedrockKind::try_from_raw(raw).unwrap().raw(), raw);
    }
    assert!(matches!(
        BedrockKindField::new(vec![0, 9]),
        Err(GeologicValidationError::InvalidBedrockKind { .. })
    ));
}

#[test]
fn valid_snapshot_exposes_borrowed_dense_fields_and_validates_upstream_counts() {
    let snapshot = valid_snapshot();
    let spatial = spatial_fixture();
    let tectonic = upstream_tectonic();
    let mantle = mantle_fixture(CELL_COUNT);
    let relief = relief_fixture(CELL_COUNT);

    snapshot.validate().unwrap();
    snapshot
        .validate_against(&spatial, &tectonic, &mantle, &relief)
        .unwrap();
    assert_eq!(snapshot.schema_version(), GEOLOGIC_SNAPSHOT_SCHEMA_V1);
    assert_eq!(snapshot.cell_count(), CELL_COUNT as u32);
    assert_eq!(snapshot.bedrock_kinds().raw_values(), &[0, 1, 2, 4]);
    assert_eq!(snapshot.fracture_intensity(), &[0.2, 0.4, 0.1, 0.8]);
    assert_eq!(snapshot.erosion_resistance(), &[0.8, 0.7, 0.4, 0.5]);
    assert_eq!(snapshot.relative_permeability(), &[0.1, 0.2, 0.7, 0.6]);
    assert_eq!(snapshot.metallic_mineral_potential(), &[0.2, 0.3, 0.1, 0.9]);
    assert_eq!(snapshot.geothermal_potential(), &[0.1, 0.2, 0.3, 0.8]);
    assert_eq!(
        snapshot.sedimentary_basin_potential(),
        &[0.0, 0.1, 0.9, 0.2]
    );
}

#[test]
fn every_dense_field_requires_exact_length() {
    let snapshot = valid_snapshot();
    for field in [
        "bedrock_kinds",
        "fracture_intensity",
        "erosion_resistance",
        "relative_permeability",
        "metallic_mineral_potential",
        "geothermal_potential",
        "sedimentary_basin_potential",
    ] {
        let mut wire = serde_json::to_value(&snapshot).unwrap();
        wire[field].as_array_mut().unwrap().pop();
        assert!(
            serde_json::from_value::<GeologicSnapshot>(wire).is_err(),
            "{field} accepted a short dense payload"
        );
    }
}

#[test]
fn continuous_fields_accept_inclusive_boundaries_and_reject_nan() {
    for value in [0.0, 1.0] {
        GeologicSnapshot::new(
            GEOLOGIC_SNAPSHOT_SCHEMA_V1,
            CELL_COUNT as u32,
            valid_bedrock(),
            vec![value; CELL_COUNT],
            vec![value; CELL_COUNT],
            vec![value; CELL_COUNT],
            vec![value; CELL_COUNT],
            vec![value; CELL_COUNT],
            vec![value; CELL_COUNT],
        )
        .unwrap();
    }

    for field_index in 0..6 {
        let mut fields = [
            vec![0.5; CELL_COUNT],
            vec![0.5; CELL_COUNT],
            vec![0.5; CELL_COUNT],
            vec![0.5; CELL_COUNT],
            vec![0.5; CELL_COUNT],
            vec![0.5; CELL_COUNT],
        ];
        fields[field_index][0] = f32::NAN;
        assert!(matches!(
            GeologicSnapshot::new(
                GEOLOGIC_SNAPSHOT_SCHEMA_V1,
                CELL_COUNT as u32,
                valid_bedrock(),
                fields[0].clone(),
                fields[1].clone(),
                fields[2].clone(),
                fields[3].clone(),
                fields[4].clone(),
                fields[5].clone(),
            ),
            Err(GeologicValidationError::FieldValueOutOfRange { .. })
        ));
    }
}

#[test]
fn unsupported_schema_and_each_upstream_count_mismatch_are_rejected() {
    let mut wire = serde_json::to_value(valid_snapshot()).unwrap();
    wire["schema_version"] = serde_json::json!(2);
    assert!(serde_json::from_value::<GeologicSnapshot>(wire).is_err());

    let snapshot = valid_snapshot();
    let spatial = spatial_fixture();
    let tectonic = upstream_tectonic();
    let mantle = mantle_fixture(CELL_COUNT);
    let relief = relief_fixture(CELL_COUNT);
    let three_cell_snapshot = GeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V1,
        3,
        BedrockKindField::new(vec![0, 1, 2]).unwrap(),
        vec![0.5; 3],
        vec![0.5; 3],
        vec![0.5; 3],
        vec![0.5; 3],
        vec![0.5; 3],
        vec![0.5; 3],
    )
    .unwrap();
    assert!(matches!(
        three_cell_snapshot.validate_against(
            &spatial,
            &tectonic_fixture(vec![CrustKind::Oceanic; 3]),
            &mantle_fixture(3),
            &relief_fixture(3),
        ),
        Err(GeologicValidationError::SpatialCellCountMismatch { .. })
    ));
    assert!(matches!(
        snapshot.validate_against(
            &spatial,
            &tectonic_fixture(vec![CrustKind::Oceanic; 3]),
            &mantle,
            &relief,
        ),
        Err(GeologicValidationError::TectonicCellCountMismatch { .. })
    ));
    assert!(matches!(
        snapshot.validate_against(&spatial, &tectonic, &mantle_fixture(3), &relief),
        Err(GeologicValidationError::MantleCellCountMismatch { .. })
    ));
    assert!(matches!(
        snapshot.validate_against(&spatial, &tectonic, &mantle, &relief_fixture(3)),
        Err(GeologicValidationError::ReliefCellCountMismatch { .. })
    ));
}

#[test]
fn crust_restrictions_reject_incompatible_crystalline_categories() {
    let spatial = spatial_fixture();
    let tectonic = upstream_tectonic();
    let mantle = mantle_fixture(CELL_COUNT);
    let relief = relief_fixture(CELL_COUNT);

    for bedrock in [vec![0, 0, 2, 4], vec![1, 1, 2, 4], vec![3, 1, 2, 4]] {
        let snapshot = snapshot_with_bedrock(BedrockKindField::new(bedrock).unwrap());
        assert!(matches!(
            snapshot.validate_against(&spatial, &tectonic, &mantle, &relief),
            Err(GeologicValidationError::BedrockCrustMismatch { .. })
        ));
    }
}

#[test]
fn sedimentary_and_volcanic_are_valid_on_either_crust() {
    let snapshot = snapshot_with_bedrock(
        BedrockKindField::new(vec![
            BedrockKind::Sedimentary.raw(),
            BedrockKind::Volcanic.raw(),
            BedrockKind::Volcanic.raw(),
            BedrockKind::Sedimentary.raw(),
        ])
        .unwrap(),
    );
    snapshot
        .validate_against(
            &spatial_fixture(),
            &upstream_tectonic(),
            &mantle_fixture(CELL_COUNT),
            &relief_fixture(CELL_COUNT),
        )
        .unwrap();
}

#[test]
fn snapshot_round_trip_revalidates_private_json() {
    let snapshot = valid_snapshot();
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: GeologicSnapshot = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, snapshot);

    let mut malformed = serde_json::to_value(snapshot).unwrap();
    malformed["fracture_intensity"][0] = serde_json::json!(1.1);
    assert!(serde_json::from_value::<GeologicSnapshot>(malformed).is_err());
}
