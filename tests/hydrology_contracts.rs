use sekai::world::natural::{
    BasinOutletKind, DrainageBasin, ElevationField, HydrologySnapshot, HydrologyValidationError,
    Lake, RiverSegment, RiverSegmentKind, StrahlerOrderField, SurfaceWaterField, SurfaceWaterKind,
    CLIMATE_MONTH_COUNT, HYDROLOGY_SCHEMA_V1, MAX_STRAHLER_ORDER, SECONDS_PER_CLIMATOLOGICAL_MONTH,
};
use sekai::world::spatial::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
use sekai::world::{
    BoundaryCondition, CellId, DrainageBasinId, EdgeId, LakeId, Meters, RiverSegmentId,
    SquareMeters, WorldPoint, WorldRect,
};

const CELL_COUNT: usize = 4;
const CELL_AREA_M2: f64 = 1_000_000.0;
const LOCAL_RUNOFF_MM: f32 = 1_000.0;

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
        area: SquareMeters::new(CELL_AREA_M2).unwrap(),
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
            (500.0, 500.0),
            &[
                (0.0, 0.0),
                (1_000.0, 0.0),
                (1_000.0, 1_000.0),
                (0.0, 1_000.0),
            ],
            &[1, 2],
        ),
        cell(
            1,
            (1_500.0, 500.0),
            &[
                (1_000.0, 0.0),
                (2_000.0, 0.0),
                (2_000.0, 1_000.0),
                (1_000.0, 1_000.0),
            ],
            &[0, 3],
        ),
        cell(
            2,
            (500.0, 1_500.0),
            &[
                (0.0, 1_000.0),
                (1_000.0, 1_000.0),
                (1_000.0, 2_000.0),
                (0.0, 2_000.0),
            ],
            &[0, 3],
        ),
        cell(
            3,
            (1_500.0, 1_500.0),
            &[
                (1_000.0, 1_000.0),
                (2_000.0, 1_000.0),
                (2_000.0, 2_000.0),
                (1_000.0, 2_000.0),
            ],
            &[1, 2],
        ),
    ];
    let edges = vec![
        edge(0, (0.0, 0.0), (1_000.0, 0.0), [Some(0), None]),
        edge(1, (0.0, 1_000.0), (0.0, 0.0), [Some(0), None]),
        edge(2, (1_000.0, 0.0), (2_000.0, 0.0), [Some(1), None]),
        edge(3, (2_000.0, 0.0), (2_000.0, 1_000.0), [Some(1), None]),
        edge(4, (0.0, 2_000.0), (0.0, 1_000.0), [Some(2), None]),
        edge(5, (1_000.0, 2_000.0), (0.0, 2_000.0), [Some(2), None]),
        edge(6, (2_000.0, 1_000.0), (2_000.0, 2_000.0), [Some(3), None]),
        edge(7, (2_000.0, 2_000.0), (1_000.0, 2_000.0), [Some(3), None]),
        edge(8, (1_000.0, 0.0), (1_000.0, 1_000.0), [Some(0), Some(1)]),
        edge(9, (1_000.0, 1_000.0), (0.0, 1_000.0), [Some(0), Some(2)]),
        edge(
            10,
            (1_000.0, 1_000.0),
            (2_000.0, 1_000.0),
            [Some(1), Some(3)],
        ),
        edge(
            11,
            (1_000.0, 1_000.0),
            (1_000.0, 2_000.0),
            [Some(2), Some(3)],
        ),
    ];

    SpatialSnapshot::new(
        SPATIAL_SCHEMA_V1,
        WorldRect::new(point(0.0, 0.0), point(2_000.0, 2_000.0)).unwrap(),
        BoundaryCondition::Closed,
        cells,
        edges,
    )
    .unwrap()
}

#[derive(Clone)]
struct SnapshotArgs {
    schema_version: u16,
    cell_count: u32,
    river_discharge_threshold_m3_s: f32,
    minimum_lake_depth_m: f32,
    monthly_local_runoff_mm: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    monthly_discharge_m3_s: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    annual_local_runoff_mm: Vec<f32>,
    mean_annual_discharge_m3_s: Vec<f32>,
    drainage_area_km2: Vec<f32>,
    drainage_surface_elevation_m: ElevationField,
    lake_depth_m: Vec<f32>,
    surface_water_kind: SurfaceWaterField,
    flow_receiver: Vec<Option<CellId>>,
    basin_id: Vec<Option<DrainageBasinId>>,
    strahler_order: StrahlerOrderField,
    basins: Vec<DrainageBasin>,
    lakes: Vec<Lake>,
    river_segments: Vec<RiverSegment>,
}

impl SnapshotArgs {
    fn build(self) -> Result<HydrologySnapshot, HydrologyValidationError> {
        HydrologySnapshot::new(
            self.schema_version,
            self.cell_count,
            self.river_discharge_threshold_m3_s,
            self.minimum_lake_depth_m,
            self.monthly_local_runoff_mm,
            self.monthly_discharge_m3_s,
            self.annual_local_runoff_mm,
            self.mean_annual_discharge_m3_s,
            self.drainage_area_km2,
            self.drainage_surface_elevation_m,
            self.lake_depth_m,
            self.surface_water_kind,
            self.flow_receiver,
            self.basin_id,
            self.strahler_order,
            self.basins,
            self.lakes,
            self.river_segments,
        )
    }
}

fn valid_args() -> SnapshotArgs {
    let local_discharge = (f64::from(LOCAL_RUNOFF_MM) / 1_000.0 * CELL_AREA_M2
        / SECONDS_PER_CLIMATOLOGICAL_MONTH) as f32;
    let discharge = [
        local_discharge,
        local_discharge * 2.0,
        local_discharge,
        local_discharge * 4.0,
    ];

    SnapshotArgs {
        schema_version: HYDROLOGY_SCHEMA_V1,
        cell_count: CELL_COUNT as u32,
        river_discharge_threshold_m3_s: 0.1,
        minimum_lake_depth_m: 1.0,
        monthly_local_runoff_mm: vec![[LOCAL_RUNOFF_MM; CLIMATE_MONTH_COUNT]; CELL_COUNT],
        monthly_discharge_m3_s: discharge
            .iter()
            .map(|&value| [value; CLIMATE_MONTH_COUNT])
            .collect(),
        annual_local_runoff_mm: vec![LOCAL_RUNOFF_MM * CLIMATE_MONTH_COUNT as f32; CELL_COUNT],
        mean_annual_discharge_m3_s: discharge.to_vec(),
        drainage_area_km2: vec![1.0, 2.0, 1.0, 4.0],
        drainage_surface_elevation_m: ElevationField::from_values(vec![100.0, 90.0, 95.0, 50.0])
            .unwrap(),
        lake_depth_m: vec![0.0, 0.0, 0.0, 5.0],
        surface_water_kind: SurfaceWaterField::from_kinds(vec![
            SurfaceWaterKind::DryLand,
            SurfaceWaterKind::DryLand,
            SurfaceWaterKind::DryLand,
            SurfaceWaterKind::Lake,
        ]),
        flow_receiver: vec![
            Some(CellId::from_raw(1)),
            Some(CellId::from_raw(3)),
            Some(CellId::from_raw(3)),
            None,
        ],
        basin_id: vec![Some(DrainageBasinId::from_raw(0)); CELL_COUNT],
        strahler_order: StrahlerOrderField::from_raw(vec![1, 1, 1, 0]).unwrap(),
        basins: vec![DrainageBasin::new(
            DrainageBasinId::from_raw(0),
            CellId::from_raw(3),
            BasinOutletKind::Lake,
            4.0,
            discharge[3],
        )
        .unwrap()],
        lakes: vec![Lake::new(
            LakeId::from_raw(0),
            vec![CellId::from_raw(3)],
            50.0,
            1.0,
            5_000_000.0,
            None,
            None,
        )
        .unwrap()],
        river_segments: vec![
            RiverSegment::new(
                RiverSegmentId::from_raw(0),
                CellId::from_raw(0),
                CellId::from_raw(1),
                RiverSegmentKind::Channel,
                1,
                discharge[0],
            )
            .unwrap(),
            RiverSegment::new(
                RiverSegmentId::from_raw(1),
                CellId::from_raw(1),
                CellId::from_raw(3),
                RiverSegmentKind::Channel,
                1,
                discharge[1],
            )
            .unwrap(),
            RiverSegment::new(
                RiverSegmentId::from_raw(2),
                CellId::from_raw(2),
                CellId::from_raw(3),
                RiverSegmentKind::Channel,
                1,
                discharge[2],
            )
            .unwrap(),
        ],
    }
}

fn valid_snapshot() -> HydrologySnapshot {
    valid_args().build().unwrap()
}

#[test]
fn typed_display_fields_validate_and_borrow_raw_values() {
    let water = SurfaceWaterField::from_kinds(vec![
        SurfaceWaterKind::DryLand,
        SurfaceWaterKind::Ocean,
        SurfaceWaterKind::Lake,
    ]);
    assert_eq!(water.raw_values(), &[0, 1, 2]);
    assert_eq!(water.get(2), Some(SurfaceWaterKind::Lake));
    assert_eq!(SurfaceWaterKind::Ocean.raw(), 1);
    assert!(SurfaceWaterField::from_raw(vec![0, 3]).is_err());
    assert!(serde_json::from_str::<SurfaceWaterField>("[0,9]").is_err());

    let orders = StrahlerOrderField::from_raw(vec![0, 1, u32::from(MAX_STRAHLER_ORDER)]).unwrap();
    assert_eq!(orders.raw_values(), &[0, 1, u32::from(MAX_STRAHLER_ORDER)]);
    assert_eq!(orders.get(1), Some(1));
    assert!(StrahlerOrderField::from_raw(vec![u32::from(MAX_STRAHLER_ORDER) + 1]).is_err());
    assert!(serde_json::from_str::<StrahlerOrderField>("[999]").is_err());
}

#[test]
fn valid_hydrology_constructs_round_trips_and_validates_against_spatial() {
    let spatial = spatial_fixture();
    let snapshot = valid_snapshot();

    snapshot.validate().unwrap();
    snapshot.validate_against_spatial(&spatial).unwrap();
    assert_eq!(snapshot.schema_version(), HYDROLOGY_SCHEMA_V1);
    assert_eq!(snapshot.cell_count(), CELL_COUNT as u32);
    assert_eq!(snapshot.monthly_local_runoff_mm()[0][0], LOCAL_RUNOFF_MM);
    assert_eq!(snapshot.flow_receiver()[0], Some(CellId::from_raw(1)));
    assert_eq!(snapshot.basins()[0].outlet_kind(), BasinOutletKind::Lake);
    assert_eq!(snapshot.lakes()[0].cells(), &[CellId::from_raw(3)]);
    assert_eq!(snapshot.river_segments()[1].to(), CellId::from_raw(3));

    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let decoded: HydrologySnapshot = serde_json::from_slice(&encoded).unwrap();
    decoded.validate_against_spatial(&spatial).unwrap();
    assert_eq!(decoded, snapshot);
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
}

#[test]
fn dense_fields_reject_bad_lengths_nonfinite_values_and_ranges() {
    let mut args = valid_args();
    args.monthly_local_runoff_mm.pop();
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::FieldLengthMismatch { .. })
    ));

    let mut args = valid_args();
    args.monthly_local_runoff_mm[0][0] = f32::NAN;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::ScalarValueOutOfRange { .. })
    ));

    let mut args = valid_args();
    args.monthly_discharge_m3_s[0][0] = -1.0;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::ScalarValueOutOfRange { .. })
    ));

    let mut args = valid_args();
    args.drainage_area_km2[0] = 0.0;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::ScalarValueOutOfRange { .. })
    ));

    let mut args = valid_args();
    args.lake_depth_m[0] = -1.0;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::ScalarValueOutOfRange { .. })
    ));

    let mut args = valid_args();
    args.schema_version += 1;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::UnsupportedSchema { .. })
    ));
}

#[test]
fn receiver_graph_rejects_out_of_range_self_and_cycles() {
    let mut args = valid_args();
    args.flow_receiver[0] = Some(CellId::from_raw(99));
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::ReceiverOutOfRange { .. })
    ));

    let mut args = valid_args();
    args.flow_receiver[0] = Some(CellId::from_raw(0));
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::SelfReceiver { .. })
    ));

    let mut args = valid_args();
    args.flow_receiver[1] = Some(CellId::from_raw(0));
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::ReceiverCycle { .. })
    ));
}

#[test]
fn basin_lake_and_river_records_enforce_canonical_cross_references() {
    assert!(matches!(
        Lake::new(
            LakeId::from_raw(0),
            vec![CellId::from_raw(3), CellId::from_raw(3)],
            50.0,
            1.0,
            1.0,
            None,
            None,
        ),
        Err(HydrologyValidationError::DuplicateLakeCell { .. })
    ));

    let mut args = valid_args();
    args.basins[0] = DrainageBasin::new(
        DrainageBasinId::from_raw(1),
        CellId::from_raw(3),
        BasinOutletKind::Lake,
        4.0,
        args.mean_annual_discharge_m3_s[3],
    )
    .unwrap();
    args.basin_id.fill(Some(DrainageBasinId::from_raw(1)));
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::NonContiguousRecordId { .. })
    ));

    let mut args = valid_args();
    args.surface_water_kind = SurfaceWaterField::from_kinds(vec![
        SurfaceWaterKind::DryLand,
        SurfaceWaterKind::DryLand,
        SurfaceWaterKind::Lake,
        SurfaceWaterKind::Lake,
    ]);
    args.lake_depth_m[2] = 5.0;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::LakeCoverageMismatch { .. })
    ));

    let mut args = valid_args();
    args.river_segments[0] = RiverSegment::new(
        RiverSegmentId::from_raw(0),
        CellId::from_raw(0),
        CellId::from_raw(3),
        RiverSegmentKind::Channel,
        1,
        args.mean_annual_discharge_m3_s[0],
    )
    .unwrap();
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::SegmentDirectionMismatch { .. })
    ));

    let mut args = valid_args();
    args.strahler_order = StrahlerOrderField::from_raw(vec![1, 1, 1, 1]).unwrap();
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::NonRiverStrahlerOrder { .. })
    ));
}

#[test]
fn monthly_summaries_and_downstream_monotonicity_are_checked() {
    let mut args = valid_args();
    args.annual_local_runoff_mm[0] += 10.0;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::SummaryIdentityMismatch { .. })
    ));

    let mut args = valid_args();
    args.mean_annual_discharge_m3_s[0] += 1.0;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::SummaryIdentityMismatch { .. })
    ));

    let mut args = valid_args();
    args.drainage_area_km2[1] = 0.5;
    assert!(matches!(
        args.build(),
        Err(HydrologyValidationError::DownstreamValueDecreases { .. })
    ));
}

#[test]
fn spatial_validation_checks_adjacency_and_exact_accumulation() {
    let spatial = spatial_fixture();

    let mut args = valid_args();
    args.flow_receiver[0] = Some(CellId::from_raw(3));
    args.river_segments[0] = RiverSegment::new(
        RiverSegmentId::from_raw(0),
        CellId::from_raw(0),
        CellId::from_raw(3),
        RiverSegmentKind::Channel,
        1,
        args.mean_annual_discharge_m3_s[0],
    )
    .unwrap();
    let snapshot = args.build().unwrap();
    assert!(matches!(
        snapshot.validate_against_spatial(&spatial),
        Err(HydrologyValidationError::ReceiverNotAdjacent { .. })
    ));

    let mut args = valid_args();
    args.drainage_area_km2[0] += 0.1;
    let snapshot = args.build().unwrap();
    assert!(matches!(
        snapshot.validate_against_spatial(&spatial),
        Err(HydrologyValidationError::DrainageAreaAccumulationMismatch { .. })
    ));

    let mut args = valid_args();
    for month in 0..CLIMATE_MONTH_COUNT {
        args.monthly_discharge_m3_s[0][month] *= 0.9;
    }
    args.mean_annual_discharge_m3_s[0] *= 0.9;
    args.river_segments[0] = RiverSegment::new(
        RiverSegmentId::from_raw(0),
        CellId::from_raw(0),
        CellId::from_raw(1),
        RiverSegmentKind::Channel,
        1,
        args.mean_annual_discharge_m3_s[0],
    )
    .unwrap();
    let snapshot = args.build().unwrap();
    assert!(matches!(
        snapshot.validate_against_spatial(&spatial),
        Err(HydrologyValidationError::DischargeAccumulationMismatch { .. })
    ));
}

#[test]
fn invalid_json_cannot_bypass_hydrology_validation() {
    let valid = valid_snapshot();

    let mut bad_water = serde_json::to_value(&valid).unwrap();
    bad_water["surface_water_kind"][0] = serde_json::json!(9);
    assert!(serde_json::from_value::<HydrologySnapshot>(bad_water).is_err());

    let mut self_receiver = serde_json::to_value(&valid).unwrap();
    self_receiver["flow_receiver"][0] = serde_json::json!(0);
    assert!(serde_json::from_value::<HydrologySnapshot>(self_receiver).is_err());

    let mut non_contiguous = serde_json::to_value(&valid).unwrap();
    non_contiguous["river_segments"][0]["id"] = serde_json::json!(7);
    assert!(serde_json::from_value::<HydrologySnapshot>(non_contiguous).is_err());
}
