use std::collections::VecDeque;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    ElevationField, ELEVATION_MAX_M, ELEVATION_MIN_M, MAX_LAKE_DEPTH_CM,
    MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S, MIN_LAKE_DEPTH_CM,
    MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S, MONTHLY_PRECIPITATION_MAX_MM,
};
use crate::world::spatial::{SpatialSnapshot, SpatialValidationError, Topology};
use crate::world::{CellId, DrainageBasinId, LakeId, RiverSegmentId};

/// The supported version of the serialized hydrology schema.
pub const HYDROLOGY_SCHEMA_V1: u16 = 1;
/// The surface-bound hydrology schema used by authoritative spherical worlds.
pub const HYDROLOGY_SCHEMA_V2: u16 = 2;
/// The fixed number of climatological months.
const MONTH_COUNT: usize = 12;
/// Mean Gregorian-year duration used by the current-slice water-volume conversion.
pub const CLIMATOLOGICAL_YEAR_SECONDS: f64 = 31_556_952.0;
/// Uniform mean month duration used for every climatological month.
pub const SECONDS_PER_CLIMATOLOGICAL_MONTH: f64 = CLIMATOLOGICAL_YEAR_SECONDS / MONTH_COUNT as f64;
/// Largest representable V1 Strahler stream order.
pub const MAX_STRAHLER_ORDER: u8 = u8::MAX;
/// Largest geometrically possible lake depth inside the supported elevation range.
pub const MAX_LAKE_DEPTH_M: f32 = ELEVATION_MAX_M - ELEVATION_MIN_M;
/// Absolute component used when checking stored monthly and annual summaries.
pub const HYDROLOGY_SUMMARY_ABSOLUTE_TOLERANCE: f64 = 0.05;
/// Relative component used for all hydrologic numerical identities.
pub const HYDROLOGY_SUMMARY_RELATIVE_TOLERANCE: f64 = 1.0e-5;
/// Absolute drainage-area tolerance, in square kilometers.
pub const DRAINAGE_AREA_ABSOLUTE_TOLERANCE_KM2: f64 = 1.0e-8;
/// Absolute discharge-accumulation tolerance, in cubic meters per second.
pub const DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S: f64 = 1.0e-9;

const MIN_RIVER_DISCHARGE_THRESHOLD_M3_S: f32 =
    MIN_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S as f32 / 10.0;
const MAX_RIVER_DISCHARGE_THRESHOLD_M3_S: f32 =
    MAX_RIVER_DISCHARGE_THRESHOLD_DECI_M3_S as f32 / 10.0;
const MINIMUM_LAKE_DEPTH_MIN_M: f32 = MIN_LAKE_DEPTH_CM as f32 / 100.0;
const MINIMUM_LAKE_DEPTH_MAX_M: f32 = MAX_LAKE_DEPTH_CM as f32 / 100.0;

/// Stable V1 classification of current surface water.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SurfaceWaterKind {
    /// Land without a published lake.
    DryLand,
    /// A cell below formal sea level.
    Ocean,
    /// A published inland-water cell.
    Lake,
}

impl SurfaceWaterKind {
    /// Decodes a stable V1 category value.
    pub fn try_from_raw(raw: u32) -> Result<Self, HydrologyValidationError> {
        match raw {
            0 => Ok(Self::DryLand),
            1 => Ok(Self::Ocean),
            2 => Ok(Self::Lake),
            found => Err(HydrologyValidationError::InvalidSurfaceWaterKind { cell: None, found }),
        }
    }

    /// Returns the stable V1 category value.
    pub const fn raw(self) -> u32 {
        match self {
            Self::DryLand => 0,
            Self::Ocean => 1,
            Self::Lake => 2,
        }
    }
}

/// Dense, display-borrowable raw surface-water categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SurfaceWaterField(Vec<u32>);

impl SurfaceWaterField {
    /// Encodes typed categories into stable raw storage.
    pub fn from_kinds(values: Vec<SurfaceWaterKind>) -> Self {
        Self(values.into_iter().map(SurfaceWaterKind::raw).collect())
    }

    /// Validates and constructs stable raw category storage.
    pub fn from_raw(values: Vec<u32>) -> Result<Self, HydrologyValidationError> {
        for (index, &found) in values.iter().enumerate() {
            SurfaceWaterKind::try_from_raw(found).map_err(|_| {
                HydrologyValidationError::InvalidSurfaceWaterKind {
                    cell: Some(CellId::from_raw(index as u32)),
                    found,
                }
            })?;
        }
        Ok(Self(values))
    }

    /// Returns one typed category by dense index.
    pub fn get(&self, index: usize) -> Option<SurfaceWaterKind> {
        self.0
            .get(index)
            .and_then(|&raw| SurfaceWaterKind::try_from_raw(raw).ok())
    }

    /// Returns stable raw categories without copying.
    pub fn raw_values(&self) -> &[u32] {
        &self.0
    }

    /// Returns the dense field length.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the field is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for SurfaceWaterField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_raw(Vec::<u32>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Dense, display-borrowable raw Strahler orders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct StrahlerOrderField(Vec<u32>);

impl StrahlerOrderField {
    /// Validates and constructs raw V1 Strahler orders.
    pub fn from_raw(values: Vec<u32>) -> Result<Self, HydrologyValidationError> {
        for (index, &found) in values.iter().enumerate() {
            if found > u32::from(MAX_STRAHLER_ORDER) {
                return Err(HydrologyValidationError::InvalidStrahlerOrder {
                    cell: CellId::from_raw(index as u32),
                    found,
                    max: MAX_STRAHLER_ORDER,
                });
            }
        }
        Ok(Self(values))
    }

    /// Returns one typed V1 order by dense index.
    pub fn get(&self, index: usize) -> Option<u8> {
        self.0.get(index).map(|&raw| raw as u8)
    }

    /// Returns raw orders without copying.
    pub fn raw_values(&self) -> &[u32] {
        &self.0
    }

    /// Returns the dense field length.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the field is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for StrahlerOrderField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_raw(Vec::<u32>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Stable V1 terminal class for a drainage basin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BasinOutletKind {
    /// Drainage terminates at a formal ocean cell.
    Ocean,
    /// Drainage terminates in an endorheic lake.
    Lake,
    /// An all-land drainage basin terminates at a stable closed sink.
    ClosedSink,
}

/// Stable V1 class of a published directed river segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiverSegmentKind {
    /// An ordinary dry-land channel.
    Channel,
    /// The single real outflow from a lake.
    LakeOutlet,
}

/// Aggregate record for one stable terminal drainage basin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DrainageBasin {
    id: DrainageBasinId,
    outlet_cell: CellId,
    outlet_kind: BasinOutletKind,
    area_km2: f64,
    mean_discharge_m3_s: f32,
}

#[derive(Deserialize)]
struct DrainageBasinWire {
    id: DrainageBasinId,
    outlet_cell: CellId,
    outlet_kind: BasinOutletKind,
    area_km2: f64,
    mean_discharge_m3_s: f32,
}

impl DrainageBasin {
    /// Constructs a locally valid basin aggregate.
    pub fn new(
        id: DrainageBasinId,
        outlet_cell: CellId,
        outlet_kind: BasinOutletKind,
        area_km2: f64,
        mean_discharge_m3_s: f32,
    ) -> Result<Self, HydrologyValidationError> {
        validate_positive_record_value("basin.area_km2", id.raw(), area_km2)?;
        validate_nonnegative_record_value(
            "basin.mean_discharge_m3_s",
            id.raw(),
            f64::from(mean_discharge_m3_s),
        )?;
        Ok(Self {
            id,
            outlet_cell,
            outlet_kind,
            area_km2,
            mean_discharge_m3_s,
        })
    }

    /// Returns the stable basin ID.
    pub const fn id(&self) -> DrainageBasinId {
        self.id
    }

    /// Returns the stable terminal cell.
    pub const fn outlet_cell(&self) -> CellId {
        self.outlet_cell
    }

    /// Returns the terminal classification.
    pub const fn outlet_kind(&self) -> BasinOutletKind {
        self.outlet_kind
    }

    /// Returns aggregate basin area in square kilometers.
    pub const fn area_km2(&self) -> f64 {
        self.area_km2
    }

    /// Returns mean discharge at the terminal.
    pub const fn mean_discharge_m3_s(&self) -> f32 {
        self.mean_discharge_m3_s
    }
}

impl<'de> Deserialize<'de> for DrainageBasin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = DrainageBasinWire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.outlet_cell,
            wire.outlet_kind,
            wire.area_km2,
            wire.mean_discharge_m3_s,
        )
        .map_err(D::Error::custom)
    }
}

/// Aggregate record for one published lake.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lake {
    id: LakeId,
    cells: Vec<CellId>,
    surface_elevation_m: f32,
    area_km2: f64,
    volume_m3: f64,
    outlet_cell: Option<CellId>,
    downstream_cell: Option<CellId>,
}

#[derive(Deserialize)]
struct LakeWire {
    id: LakeId,
    cells: Vec<CellId>,
    surface_elevation_m: f32,
    area_km2: f64,
    volume_m3: f64,
    outlet_cell: Option<CellId>,
    downstream_cell: Option<CellId>,
}

impl Lake {
    /// Constructs a locally valid canonical lake record.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: LakeId,
        cells: Vec<CellId>,
        surface_elevation_m: f32,
        area_km2: f64,
        volume_m3: f64,
        outlet_cell: Option<CellId>,
        downstream_cell: Option<CellId>,
    ) -> Result<Self, HydrologyValidationError> {
        if cells.is_empty() {
            return Err(HydrologyValidationError::EmptyLake { lake: id });
        }
        for pair in cells.windows(2) {
            if pair[0] == pair[1] {
                return Err(HydrologyValidationError::DuplicateLakeCell {
                    lake: id,
                    cell: pair[0],
                });
            }
            if pair[0] > pair[1] {
                return Err(HydrologyValidationError::UnsortedLakeCells { lake: id });
            }
        }
        if !surface_elevation_m.is_finite()
            || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&surface_elevation_m)
        {
            return Err(HydrologyValidationError::InvalidRecordValue {
                record: "lake.surface_elevation_m",
                id: id.raw(),
                found: f64::from(surface_elevation_m),
            });
        }
        validate_positive_record_value("lake.area_km2", id.raw(), area_km2)?;
        validate_positive_record_value("lake.volume_m3", id.raw(), volume_m3)?;
        if outlet_cell.is_some() != downstream_cell.is_some() {
            return Err(HydrologyValidationError::IncompleteLakeOutlet { lake: id });
        }
        if let (Some(outlet), Some(downstream)) = (outlet_cell, downstream_cell) {
            if cells.binary_search(&outlet).is_err() {
                return Err(HydrologyValidationError::LakeOutletNotContained { lake: id, outlet });
            }
            if cells.binary_search(&downstream).is_ok() {
                return Err(HydrologyValidationError::LakeOutletRemainsInside {
                    lake: id,
                    downstream,
                });
            }
        }
        Ok(Self {
            id,
            cells,
            surface_elevation_m,
            area_km2,
            volume_m3,
            outlet_cell,
            downstream_cell,
        })
    }

    /// Returns the stable lake ID.
    pub const fn id(&self) -> LakeId {
        self.id
    }

    /// Returns sorted member cells.
    pub fn cells(&self) -> &[CellId] {
        &self.cells
    }

    /// Returns the filled lake-surface elevation.
    pub const fn surface_elevation_m(&self) -> f32 {
        self.surface_elevation_m
    }

    /// Returns aggregate lake area.
    pub const fn area_km2(&self) -> f64 {
        self.area_km2
    }

    /// Returns aggregate lake volume.
    pub const fn volume_m3(&self) -> f64 {
        self.volume_m3
    }

    /// Returns the real outlet cell for an open lake.
    pub const fn outlet_cell(&self) -> Option<CellId> {
        self.outlet_cell
    }

    /// Returns the first downstream cell outside an open lake.
    pub const fn downstream_cell(&self) -> Option<CellId> {
        self.downstream_cell
    }
}

impl<'de> Deserialize<'de> for Lake {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = LakeWire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.cells,
            wire.surface_elevation_m,
            wire.area_km2,
            wire.volume_m3,
            wire.outlet_cell,
            wire.downstream_cell,
        )
        .map_err(D::Error::custom)
    }
}

/// One stable directed reach in the published river network.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RiverSegment {
    id: RiverSegmentId,
    from: CellId,
    to: CellId,
    kind: RiverSegmentKind,
    strahler_order: u8,
    mean_discharge_m3_s: f32,
}

#[derive(Deserialize)]
struct RiverSegmentWire {
    id: RiverSegmentId,
    from: CellId,
    to: CellId,
    kind: RiverSegmentKind,
    strahler_order: u8,
    mean_discharge_m3_s: f32,
}

impl RiverSegment {
    /// Constructs a locally valid directed river segment.
    pub fn new(
        id: RiverSegmentId,
        from: CellId,
        to: CellId,
        kind: RiverSegmentKind,
        strahler_order: u8,
        mean_discharge_m3_s: f32,
    ) -> Result<Self, HydrologyValidationError> {
        if from == to {
            return Err(HydrologyValidationError::SelfRiverSegment {
                segment: id,
                cell: from,
            });
        }
        if strahler_order == 0 {
            return Err(HydrologyValidationError::InvalidSegmentStrahlerOrder {
                segment: id,
                found: strahler_order,
            });
        }
        validate_nonnegative_record_value(
            "river_segment.mean_discharge_m3_s",
            id.raw(),
            f64::from(mean_discharge_m3_s),
        )?;
        Ok(Self {
            id,
            from,
            to,
            kind,
            strahler_order,
            mean_discharge_m3_s,
        })
    }

    /// Returns the stable segment ID.
    pub const fn id(&self) -> RiverSegmentId {
        self.id
    }

    /// Returns the upstream cell.
    pub const fn from(&self) -> CellId {
        self.from
    }

    /// Returns the direct receiver cell.
    pub const fn to(&self) -> CellId {
        self.to
    }

    /// Returns the reach class.
    pub const fn kind(&self) -> RiverSegmentKind {
        self.kind
    }

    /// Returns the unnormalized Strahler order.
    pub const fn strahler_order(&self) -> u8 {
        self.strahler_order
    }

    /// Returns mean discharge at the reach origin.
    pub const fn mean_discharge_m3_s(&self) -> f32 {
        self.mean_discharge_m3_s
    }
}

impl<'de> Deserialize<'de> for RiverSegment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RiverSegmentWire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.from,
            wire.to,
            wire.kind,
            wire.strahler_order,
            wire.mean_discharge_m3_s,
        )
        .map_err(D::Error::custom)
    }
}

/// Immutable current-slice runoff, drainage, water-body, basin, and river contracts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HydrologySnapshot {
    schema_version: u16,
    cell_count: u32,
    river_discharge_threshold_m3_s: f32,
    minimum_lake_depth_m: f32,
    monthly_local_runoff_mm: Vec<[f32; MONTH_COUNT]>,
    monthly_discharge_m3_s: Vec<[f32; MONTH_COUNT]>,
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

#[derive(Deserialize)]
struct HydrologySnapshotWire {
    schema_version: u16,
    cell_count: u32,
    river_discharge_threshold_m3_s: f32,
    minimum_lake_depth_m: f32,
    monthly_local_runoff_mm: Vec<[f32; MONTH_COUNT]>,
    monthly_discharge_m3_s: Vec<[f32; MONTH_COUNT]>,
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

impl HydrologySnapshot {
    /// Constructs a snapshot only when every self-contained V1 invariant holds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        cell_count: u32,
        river_discharge_threshold_m3_s: f32,
        minimum_lake_depth_m: f32,
        monthly_local_runoff_mm: Vec<[f32; MONTH_COUNT]>,
        monthly_discharge_m3_s: Vec<[f32; MONTH_COUNT]>,
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
    ) -> Result<Self, HydrologyValidationError> {
        let snapshot = Self {
            schema_version,
            cell_count,
            river_discharge_threshold_m3_s,
            minimum_lake_depth_m,
            monthly_local_runoff_mm,
            monthly_discharge_m3_s,
            annual_local_runoff_mm,
            mean_annual_discharge_m3_s,
            drainage_area_km2,
            drainage_surface_elevation_m,
            lake_depth_m,
            surface_water_kind,
            flow_receiver,
            basin_id,
            strahler_order,
            basins,
            lakes,
            river_segments,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks every self-contained V1 hydrology invariant.
    pub fn validate(&self) -> Result<(), HydrologyValidationError> {
        self.validate_header_and_fields()?;
        self.validate_receivers()?;
        let roots = self.receiver_roots();
        self.validate_basins(&roots)?;
        let lake_owner = self.validate_lakes()?;
        self.validate_rivers(&lake_owner)?;
        Ok(())
    }

    /// Adds topology, exact area, water-volume, and lake-aggregate validation.
    pub fn validate_against_spatial(
        &self,
        spatial: &SpatialSnapshot,
    ) -> Result<(), HydrologyValidationError> {
        self.validate()?;
        spatial.validate()?;
        self.validate_spatial_relations(spatial)
    }

    /// Rechecks topology-dependent invariants when the spatial artifact is already validated.
    pub(crate) fn validate_against_validated_spatial(
        &self,
        spatial: &SpatialSnapshot,
    ) -> Result<(), HydrologyValidationError> {
        self.validate()?;
        self.validate_spatial_relations(spatial)
    }

    fn validate_spatial_relations(
        &self,
        spatial: &SpatialSnapshot,
    ) -> Result<(), HydrologyValidationError> {
        self.validate_metric_relations(
            spatial.cell_count(),
            |cell| {
                spatial
                    .cell(cell)
                    .expect("validated dense spatial input contains every cell")
                    .area
                    .get()
            },
            |cell, receiver| {
                spatial
                    .neighbors(cell)
                    .is_some_and(|neighbors| neighbors.contains(&receiver))
            },
        )
    }

    pub(crate) fn validate_metric_relations<Area, Adjacent>(
        &self,
        metric_cell_count: usize,
        mut cell_area_m2: Area,
        mut adjacent: Adjacent,
    ) -> Result<(), HydrologyValidationError>
    where
        Area: FnMut(CellId) -> f64,
        Adjacent: FnMut(CellId, CellId) -> bool,
    {
        if self.cell_count as usize != metric_cell_count {
            return Err(HydrologyValidationError::SpatialCellCountMismatch {
                hydrology: self.cell_count,
                spatial: metric_cell_count,
            });
        }

        for (index, receiver) in self.flow_receiver.iter().enumerate() {
            if let Some(receiver) = receiver {
                let cell = CellId::from_raw(index as u32);
                if !adjacent(cell, *receiver) {
                    return Err(HydrologyValidationError::ReceiverNotAdjacent {
                        cell,
                        receiver: *receiver,
                    });
                }
            }
        }

        let order = self.upstream_to_downstream_order();
        let mut expected_area_km2 = vec![0.0_f64; self.cell_count as usize];
        let mut expected_discharge = vec![[0.0_f64; MONTH_COUNT]; self.cell_count as usize];
        for (index, (expected_area, expected_months)) in expected_area_km2
            .iter_mut()
            .zip(&mut expected_discharge)
            .enumerate()
        {
            let area_m2 = cell_area_m2(CellId::from_raw(index as u32));
            *expected_area = area_m2 / 1_000_000.0;
            for (expected, &runoff_mm) in expected_months
                .iter_mut()
                .zip(&self.monthly_local_runoff_mm[index])
            {
                *expected =
                    f64::from(runoff_mm) / 1_000.0 * area_m2 / SECONDS_PER_CLIMATOLOGICAL_MONTH;
            }
        }
        for &cell in &order {
            let index = cell.raw() as usize;
            if let Some(receiver) = self.flow_receiver[index] {
                let receiver_index = receiver.raw() as usize;
                let upstream_area = expected_area_km2[index];
                expected_area_km2[receiver_index] += upstream_area;
                let upstream_discharge = expected_discharge[index];
                for (downstream, upstream) in expected_discharge[receiver_index]
                    .iter_mut()
                    .zip(upstream_discharge)
                {
                    *downstream += upstream;
                }
            }
        }

        for index in 0..self.cell_count as usize {
            let cell = CellId::from_raw(index as u32);
            let stored_area = f64::from(self.drainage_area_km2[index]);
            if !nearly_equal(
                stored_area,
                expected_area_km2[index],
                DRAINAGE_AREA_ABSOLUTE_TOLERANCE_KM2,
            ) {
                return Err(HydrologyValidationError::DrainageAreaAccumulationMismatch {
                    cell,
                    stored: stored_area,
                    calculated: expected_area_km2[index],
                });
            }
            for (month, (&stored, &calculated)) in self.monthly_discharge_m3_s[index]
                .iter()
                .zip(&expected_discharge[index])
                .enumerate()
            {
                let stored = f64::from(stored);
                if !nearly_equal(
                    stored,
                    calculated,
                    DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S,
                ) {
                    return Err(HydrologyValidationError::DischargeAccumulationMismatch {
                        cell,
                        month,
                        stored,
                        calculated,
                    });
                }
            }
        }

        for lake in &self.lakes {
            let mut area_m2 = 0.0;
            let mut volume_m3 = 0.0;
            for &cell in &lake.cells {
                let index = cell.raw() as usize;
                let cell_area = cell_area_m2(cell);
                area_m2 += cell_area;
                volume_m3 += cell_area * f64::from(self.lake_depth_m[index]);
            }
            let area_km2 = area_m2 / 1_000_000.0;
            if !nearly_equal(
                lake.area_km2,
                area_km2,
                DRAINAGE_AREA_ABSOLUTE_TOLERANCE_KM2,
            ) {
                return Err(HydrologyValidationError::LakeAggregateMismatch {
                    lake: lake.id,
                    field: "area_km2",
                    stored: lake.area_km2,
                    calculated: area_km2,
                });
            }
            if !nearly_equal(
                lake.volume_m3,
                volume_m3,
                HYDROLOGY_SUMMARY_ABSOLUTE_TOLERANCE,
            ) {
                return Err(HydrologyValidationError::LakeAggregateMismatch {
                    lake: lake.id,
                    field: "volume_m3",
                    stored: lake.volume_m3,
                    calculated: volume_m3,
                });
            }
        }
        Ok(())
    }

    fn validate_header_and_fields(&self) -> Result<(), HydrologyValidationError> {
        if self.schema_version != HYDROLOGY_SCHEMA_V1 {
            return Err(HydrologyValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: HYDROLOGY_SCHEMA_V1,
            });
        }
        validate_setting(
            "river_discharge_threshold_m3_s",
            self.river_discharge_threshold_m3_s,
            MIN_RIVER_DISCHARGE_THRESHOLD_M3_S,
            MAX_RIVER_DISCHARGE_THRESHOLD_M3_S,
        )?;
        validate_setting(
            "minimum_lake_depth_m",
            self.minimum_lake_depth_m,
            MINIMUM_LAKE_DEPTH_MIN_M,
            MINIMUM_LAKE_DEPTH_MAX_M,
        )?;

        for (field, found) in [
            (
                "monthly_local_runoff_mm",
                self.monthly_local_runoff_mm.len(),
            ),
            ("monthly_discharge_m3_s", self.monthly_discharge_m3_s.len()),
            ("annual_local_runoff_mm", self.annual_local_runoff_mm.len()),
            (
                "mean_annual_discharge_m3_s",
                self.mean_annual_discharge_m3_s.len(),
            ),
            ("drainage_area_km2", self.drainage_area_km2.len()),
            (
                "drainage_surface_elevation_m",
                self.drainage_surface_elevation_m.len(),
            ),
            ("lake_depth_m", self.lake_depth_m.len()),
            ("surface_water_kind", self.surface_water_kind.len()),
            ("flow_receiver", self.flow_receiver.len()),
            ("basin_id", self.basin_id.len()),
            ("strahler_order", self.strahler_order.len()),
        ] {
            validate_length(field, found, self.cell_count)?;
        }

        validate_monthly_values(
            "monthly_local_runoff_mm",
            &self.monthly_local_runoff_mm,
            0.0,
            MONTHLY_PRECIPITATION_MAX_MM,
        )?;
        validate_monthly_values(
            "monthly_discharge_m3_s",
            &self.monthly_discharge_m3_s,
            0.0,
            f32::MAX,
        )?;
        validate_scalar_values(
            "annual_local_runoff_mm",
            &self.annual_local_runoff_mm,
            0.0,
            super::ANNUAL_PRECIPITATION_MAX_MM,
            false,
        )?;
        validate_scalar_values(
            "mean_annual_discharge_m3_s",
            &self.mean_annual_discharge_m3_s,
            0.0,
            f32::MAX,
            false,
        )?;
        validate_scalar_values(
            "drainage_area_km2",
            &self.drainage_area_km2,
            0.0,
            f32::MAX,
            true,
        )?;
        validate_scalar_values(
            "drainage_surface_elevation_m",
            self.drainage_surface_elevation_m.values(),
            ELEVATION_MIN_M,
            ELEVATION_MAX_M,
            false,
        )?;
        validate_scalar_values(
            "lake_depth_m",
            &self.lake_depth_m,
            0.0,
            MAX_LAKE_DEPTH_M,
            false,
        )?;

        for index in 0..self.cell_count as usize {
            let cell = CellId::from_raw(index as u32);
            let annual = self.monthly_local_runoff_mm[index]
                .iter()
                .map(|&value| f64::from(value))
                .sum::<f64>();
            if !nearly_equal(
                f64::from(self.annual_local_runoff_mm[index]),
                annual,
                HYDROLOGY_SUMMARY_ABSOLUTE_TOLERANCE,
            ) {
                return Err(HydrologyValidationError::SummaryIdentityMismatch {
                    field: "annual_local_runoff_mm",
                    cell,
                    stored: f64::from(self.annual_local_runoff_mm[index]),
                    calculated: annual,
                });
            }
            let mean = self.monthly_discharge_m3_s[index]
                .iter()
                .map(|&value| f64::from(value))
                .sum::<f64>()
                / MONTH_COUNT as f64;
            if !nearly_equal(
                f64::from(self.mean_annual_discharge_m3_s[index]),
                mean,
                HYDROLOGY_SUMMARY_ABSOLUTE_TOLERANCE,
            ) {
                return Err(HydrologyValidationError::SummaryIdentityMismatch {
                    field: "mean_annual_discharge_m3_s",
                    cell,
                    stored: f64::from(self.mean_annual_discharge_m3_s[index]),
                    calculated: mean,
                });
            }

            let water = self
                .surface_water_kind
                .get(index)
                .expect("validated surface-water field decodes");
            let lake_depth = self.lake_depth_m[index];
            match water {
                SurfaceWaterKind::Lake if lake_depth < self.minimum_lake_depth_m => {
                    return Err(HydrologyValidationError::LakeDepthKindMismatch {
                        cell,
                        kind: water,
                        depth: lake_depth,
                        minimum_lake_depth: self.minimum_lake_depth_m,
                    });
                }
                SurfaceWaterKind::DryLand | SurfaceWaterKind::Ocean if lake_depth != 0.0 => {
                    return Err(HydrologyValidationError::LakeDepthKindMismatch {
                        cell,
                        kind: water,
                        depth: lake_depth,
                        minimum_lake_depth: self.minimum_lake_depth_m,
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_receivers(&self) -> Result<(), HydrologyValidationError> {
        for (index, receiver) in self.flow_receiver.iter().enumerate() {
            let cell = CellId::from_raw(index as u32);
            if let Some(receiver) = receiver {
                if receiver.raw() >= self.cell_count {
                    return Err(HydrologyValidationError::ReceiverOutOfRange {
                        cell,
                        receiver: *receiver,
                        cell_count: self.cell_count,
                    });
                }
                if *receiver == cell {
                    return Err(HydrologyValidationError::SelfReceiver { cell });
                }
            }
            if self.surface_water_kind.get(index) == Some(SurfaceWaterKind::Ocean)
                && receiver.is_some()
            {
                return Err(HydrologyValidationError::OceanHasReceiver { cell });
            }
        }

        let mut state = vec![0_u8; self.cell_count as usize];
        for start in 0..self.cell_count as usize {
            if state[start] != 0 {
                continue;
            }
            let mut path = Vec::new();
            let mut current = start;
            while state[current] == 0 {
                state[current] = 1;
                path.push(current);
                match self.flow_receiver[current] {
                    Some(receiver) => current = receiver.raw() as usize,
                    None => break,
                }
            }
            if state[current] == 1
                && self.flow_receiver[*path.last().expect("path is nonempty")].is_some()
            {
                return Err(HydrologyValidationError::ReceiverCycle {
                    cell: CellId::from_raw(current as u32),
                });
            }
            for index in path {
                state[index] = 2;
            }
        }

        for (index, receiver) in self.flow_receiver.iter().enumerate() {
            let Some(receiver) = receiver else {
                continue;
            };
            let receiver_index = receiver.raw() as usize;
            let cell = CellId::from_raw(index as u32);
            if self.drainage_surface_elevation_m.values()[receiver_index]
                > self.drainage_surface_elevation_m.values()[index] + 0.01
            {
                return Err(HydrologyValidationError::ReceiverFlowsUphill {
                    cell,
                    receiver: *receiver,
                });
            }
            check_downstream_value(
                "drainage_area_km2",
                cell,
                *receiver,
                f64::from(self.drainage_area_km2[index]),
                f64::from(self.drainage_area_km2[receiver_index]),
                DRAINAGE_AREA_ABSOLUTE_TOLERANCE_KM2,
            )?;
            check_downstream_value(
                "mean_annual_discharge_m3_s",
                cell,
                *receiver,
                f64::from(self.mean_annual_discharge_m3_s[index]),
                f64::from(self.mean_annual_discharge_m3_s[receiver_index]),
                DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S,
            )?;
            for month in 0..MONTH_COUNT {
                check_downstream_value(
                    "monthly_discharge_m3_s",
                    cell,
                    *receiver,
                    f64::from(self.monthly_discharge_m3_s[index][month]),
                    f64::from(self.monthly_discharge_m3_s[receiver_index][month]),
                    DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S,
                )?;
            }
        }
        Ok(())
    }

    fn receiver_roots(&self) -> Vec<CellId> {
        let mut roots = vec![None; self.cell_count as usize];
        for start in 0..self.cell_count as usize {
            if roots[start].is_some() {
                continue;
            }
            let mut path = Vec::new();
            let mut current = start;
            while roots[current].is_none() {
                path.push(current);
                match self.flow_receiver[current] {
                    Some(receiver) => current = receiver.raw() as usize,
                    None => {
                        roots[current] = Some(CellId::from_raw(current as u32));
                        break;
                    }
                }
            }
            let root = roots[current].expect("acyclic receiver traversal reaches a root");
            for index in path {
                roots[index] = Some(root);
            }
        }
        roots
            .into_iter()
            .map(|root| root.expect("every receiver component has a root"))
            .collect()
    }

    fn validate_basins(&self, roots: &[CellId]) -> Result<(), HydrologyValidationError> {
        for (index, basin) in self.basins.iter().enumerate() {
            if basin.id.raw() as usize != index {
                return Err(HydrologyValidationError::NonContiguousRecordId {
                    record: "basin",
                    position: index,
                    found: basin.id.raw(),
                });
            }
            if basin.outlet_cell.raw() >= self.cell_count {
                return Err(HydrologyValidationError::RecordCellOutOfRange {
                    record: "basin.outlet_cell",
                    id: basin.id.raw(),
                    cell: basin.outlet_cell,
                    cell_count: self.cell_count,
                });
            }
            let outlet_index = basin.outlet_cell.raw() as usize;
            if self.flow_receiver[outlet_index].is_some() {
                return Err(HydrologyValidationError::BasinOutletHasReceiver {
                    basin: basin.id,
                    outlet: basin.outlet_cell,
                });
            }
            let water = self
                .surface_water_kind
                .get(outlet_index)
                .expect("validated water kind");
            let expected = match water {
                SurfaceWaterKind::Ocean => BasinOutletKind::Ocean,
                SurfaceWaterKind::Lake => BasinOutletKind::Lake,
                SurfaceWaterKind::DryLand => BasinOutletKind::ClosedSink,
            };
            if basin.outlet_kind != expected {
                return Err(HydrologyValidationError::BasinOutletKindMismatch {
                    basin: basin.id,
                    stored: basin.outlet_kind,
                    expected,
                });
            }
            if !nearly_equal(
                basin.area_km2,
                f64::from(self.drainage_area_km2[outlet_index]),
                DRAINAGE_AREA_ABSOLUTE_TOLERANCE_KM2,
            ) {
                return Err(HydrologyValidationError::BasinAggregateMismatch {
                    basin: basin.id,
                    field: "area_km2",
                });
            }
            if !nearly_equal(
                f64::from(basin.mean_discharge_m3_s),
                f64::from(self.mean_annual_discharge_m3_s[outlet_index]),
                DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S,
            ) {
                return Err(HydrologyValidationError::BasinAggregateMismatch {
                    basin: basin.id,
                    field: "mean_discharge_m3_s",
                });
            }
        }

        let mut used = vec![false; self.basins.len()];
        for (index, &terminal) in roots.iter().enumerate() {
            let cell = CellId::from_raw(index as u32);
            let water = self
                .surface_water_kind
                .get(index)
                .expect("validated water kind");
            match (water, self.basin_id[index]) {
                (SurfaceWaterKind::Ocean, None) => {}
                (SurfaceWaterKind::Ocean, Some(found)) => {
                    return Err(HydrologyValidationError::OceanHasBasin { cell, found });
                }
                (_, None) => return Err(HydrologyValidationError::MissingBasin { cell }),
                (_, Some(id)) => {
                    let Some(basin) = self.basins.get(id.raw() as usize) else {
                        return Err(HydrologyValidationError::BasinIdOutOfRange {
                            cell,
                            found: id,
                            basin_count: self.basins.len(),
                        });
                    };
                    if terminal != basin.outlet_cell {
                        return Err(HydrologyValidationError::BasinTerminalMismatch {
                            cell,
                            basin: id,
                            terminal,
                            outlet: basin.outlet_cell,
                        });
                    }
                    used[id.raw() as usize] = true;
                }
            }
        }
        for (index, used) in used.into_iter().enumerate() {
            if !used {
                return Err(HydrologyValidationError::UnusedBasin {
                    basin: DrainageBasinId::from_raw(index as u32),
                });
            }
        }
        Ok(())
    }

    fn validate_lakes(&self) -> Result<Vec<Option<LakeId>>, HydrologyValidationError> {
        let mut owner = vec![None; self.cell_count as usize];
        for (index, lake) in self.lakes.iter().enumerate() {
            if lake.id.raw() as usize != index {
                return Err(HydrologyValidationError::NonContiguousRecordId {
                    record: "lake",
                    position: index,
                    found: lake.id.raw(),
                });
            }
            for &cell in &lake.cells {
                if cell.raw() >= self.cell_count {
                    return Err(HydrologyValidationError::RecordCellOutOfRange {
                        record: "lake.cells",
                        id: lake.id.raw(),
                        cell,
                        cell_count: self.cell_count,
                    });
                }
                let slot = &mut owner[cell.raw() as usize];
                if let Some(first) = slot {
                    return Err(HydrologyValidationError::LakeCellOverlap {
                        cell,
                        first: *first,
                        second: lake.id,
                    });
                }
                *slot = Some(lake.id);
                let stored_surface =
                    self.drainage_surface_elevation_m.values()[cell.raw() as usize];
                if !nearly_equal(
                    f64::from(stored_surface),
                    f64::from(lake.surface_elevation_m),
                    0.01,
                ) {
                    return Err(HydrologyValidationError::LakeSurfaceMismatch {
                        lake: lake.id,
                        cell,
                        stored: stored_surface,
                        expected: lake.surface_elevation_m,
                    });
                }
            }
            match (lake.outlet_cell, lake.downstream_cell) {
                (Some(outlet), Some(downstream)) => {
                    if downstream.raw() >= self.cell_count {
                        return Err(HydrologyValidationError::RecordCellOutOfRange {
                            record: "lake.downstream_cell",
                            id: lake.id.raw(),
                            cell: downstream,
                            cell_count: self.cell_count,
                        });
                    }
                    if self.flow_receiver[outlet.raw() as usize] != Some(downstream) {
                        return Err(HydrologyValidationError::LakeOutletDirectionMismatch {
                            lake: lake.id,
                            outlet,
                            downstream,
                        });
                    }
                }
                (None, None) => {
                    if !lake
                        .cells
                        .iter()
                        .any(|cell| self.flow_receiver[cell.raw() as usize].is_none())
                    {
                        return Err(HydrologyValidationError::ClosedLakeHasNoTerminal {
                            lake: lake.id,
                        });
                    }
                }
                _ => unreachable!("lake constructor enforces complete outlet pairs"),
            }
        }
        for (index, &lake) in owner.iter().enumerate() {
            let cell = CellId::from_raw(index as u32);
            let kind = self
                .surface_water_kind
                .get(index)
                .expect("validated water kind");
            if (kind == SurfaceWaterKind::Lake) != lake.is_some() {
                return Err(HydrologyValidationError::LakeCoverageMismatch { cell, kind, lake });
            }
        }
        Ok(owner)
    }

    fn validate_rivers(
        &self,
        lake_owner: &[Option<LakeId>],
    ) -> Result<(), HydrologyValidationError> {
        let mut by_origin = vec![None; self.cell_count as usize];
        let mut incoming: Vec<Vec<&RiverSegment>> = vec![Vec::new(); self.cell_count as usize];
        for (index, segment) in self.river_segments.iter().enumerate() {
            if segment.id.raw() as usize != index {
                return Err(HydrologyValidationError::NonContiguousRecordId {
                    record: "river_segment",
                    position: index,
                    found: segment.id.raw(),
                });
            }
            if segment.from.raw() >= self.cell_count {
                return Err(HydrologyValidationError::RecordCellOutOfRange {
                    record: "river_segment.from",
                    id: segment.id.raw(),
                    cell: segment.from,
                    cell_count: self.cell_count,
                });
            }
            if segment.to.raw() >= self.cell_count {
                return Err(HydrologyValidationError::RecordCellOutOfRange {
                    record: "river_segment.to",
                    id: segment.id.raw(),
                    cell: segment.to,
                    cell_count: self.cell_count,
                });
            }
            let from_index = segment.from.raw() as usize;
            if self.flow_receiver[from_index] != Some(segment.to) {
                return Err(HydrologyValidationError::SegmentDirectionMismatch {
                    segment: segment.id,
                    from: segment.from,
                    to: segment.to,
                    receiver: self.flow_receiver[from_index],
                });
            }
            if let Some(first) = by_origin[from_index] {
                return Err(HydrologyValidationError::DuplicateRiverOrigin {
                    cell: segment.from,
                    first,
                    second: segment.id,
                });
            }
            by_origin[from_index] = Some(segment.id);
            incoming[segment.to.raw() as usize].push(segment);

            let water = self
                .surface_water_kind
                .get(from_index)
                .expect("validated water kind");
            match segment.kind {
                RiverSegmentKind::Channel if water != SurfaceWaterKind::DryLand => {
                    return Err(HydrologyValidationError::SegmentKindMismatch {
                        segment: segment.id,
                        kind: segment.kind,
                        water,
                    });
                }
                RiverSegmentKind::LakeOutlet => {
                    let Some(lake_id) = lake_owner[from_index] else {
                        return Err(HydrologyValidationError::SegmentKindMismatch {
                            segment: segment.id,
                            kind: segment.kind,
                            water,
                        });
                    };
                    let lake = &self.lakes[lake_id.raw() as usize];
                    if lake.outlet_cell != Some(segment.from)
                        || lake.downstream_cell != Some(segment.to)
                    {
                        return Err(HydrologyValidationError::SegmentKindMismatch {
                            segment: segment.id,
                            kind: segment.kind,
                            water,
                        });
                    }
                }
                _ => {}
            }
            let mean = self.mean_annual_discharge_m3_s[from_index];
            if mean + (DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S as f32)
                < self.river_discharge_threshold_m3_s
            {
                return Err(HydrologyValidationError::RiverBelowThreshold {
                    segment: segment.id,
                    discharge: mean,
                    threshold: self.river_discharge_threshold_m3_s,
                });
            }
            if !nearly_equal(
                f64::from(segment.mean_discharge_m3_s),
                f64::from(mean),
                DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S,
            ) {
                return Err(HydrologyValidationError::SegmentDischargeMismatch {
                    segment: segment.id,
                });
            }
            if self.strahler_order.get(from_index) != Some(segment.strahler_order) {
                return Err(HydrologyValidationError::SegmentStrahlerMismatch {
                    segment: segment.id,
                    stored: segment.strahler_order,
                    field: self.strahler_order.get(from_index).unwrap_or(0),
                });
            }
        }

        for index in 0..self.cell_count as usize {
            let cell = CellId::from_raw(index as u32);
            let order = self.strahler_order.get(index).expect("validated order");
            let segment = by_origin[index];
            if segment.is_none() && order != 0 {
                return Err(HydrologyValidationError::NonRiverStrahlerOrder { cell, order });
            }
            let water = self
                .surface_water_kind
                .get(index)
                .expect("validated water kind");
            let is_open_lake_outlet = lake_owner[index].is_some_and(|lake_id| {
                self.lakes[lake_id.raw() as usize].outlet_cell == Some(cell)
            });
            let eligible = self.flow_receiver[index].is_some()
                && self.mean_annual_discharge_m3_s[index]
                    + (DISCHARGE_ACCUMULATION_ABSOLUTE_TOLERANCE_M3_S as f32)
                    >= self.river_discharge_threshold_m3_s
                && (water == SurfaceWaterKind::DryLand || is_open_lake_outlet);
            if eligible != segment.is_some() {
                return Err(HydrologyValidationError::RiverCoverageMismatch {
                    cell,
                    eligible,
                    segment,
                });
            }
        }

        for segment in &self.river_segments {
            if segment.kind != RiverSegmentKind::Channel {
                continue;
            }
            let upstream = &incoming[segment.from.raw() as usize];
            let expected = if upstream.is_empty() {
                1
            } else {
                let max = upstream
                    .iter()
                    .map(|segment| segment.strahler_order)
                    .max()
                    .expect("nonempty incoming list has a maximum");
                let repeated = upstream
                    .iter()
                    .filter(|segment| segment.strahler_order == max)
                    .count()
                    >= 2;
                if repeated {
                    max.saturating_add(1)
                } else {
                    max
                }
            };
            if segment.strahler_order != expected {
                return Err(HydrologyValidationError::InvalidStrahlerTopology {
                    segment: segment.id,
                    stored: segment.strahler_order,
                    calculated: expected,
                });
            }
        }
        Ok(())
    }

    fn upstream_to_downstream_order(&self) -> Vec<CellId> {
        let mut indegree = vec![0_usize; self.cell_count as usize];
        for receiver in self.flow_receiver.iter().flatten() {
            indegree[receiver.raw() as usize] += 1;
        }
        let mut queue = VecDeque::new();
        for (index, &degree) in indegree.iter().enumerate() {
            if degree == 0 {
                queue.push_back(CellId::from_raw(index as u32));
            }
        }
        let mut order = Vec::with_capacity(self.cell_count as usize);
        while let Some(cell) = queue.pop_front() {
            order.push(cell);
            if let Some(receiver) = self.flow_receiver[cell.raw() as usize] {
                let degree = &mut indegree[receiver.raw() as usize];
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(receiver);
                }
            }
        }
        order
    }

    /// Returns the schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact dense cell count.
    pub const fn cell_count(&self) -> u32 {
        self.cell_count
    }

    /// Returns the river publication threshold.
    pub const fn river_discharge_threshold_m3_s(&self) -> f32 {
        self.river_discharge_threshold_m3_s
    }

    /// Returns the minimum published lake depth.
    pub const fn minimum_lake_depth_m(&self) -> f32 {
        self.minimum_lake_depth_m
    }

    /// Returns monthly local runoff without copying.
    pub fn monthly_local_runoff_mm(&self) -> &[[f32; MONTH_COUNT]] {
        &self.monthly_local_runoff_mm
    }

    /// Returns monthly accumulated discharge without copying.
    pub fn monthly_discharge_m3_s(&self) -> &[[f32; MONTH_COUNT]] {
        &self.monthly_discharge_m3_s
    }

    /// Returns annual local runoff summaries without copying.
    pub fn annual_local_runoff_mm(&self) -> &[f32] {
        &self.annual_local_runoff_mm
    }

    /// Returns mean annual discharge summaries without copying.
    pub fn mean_annual_discharge_m3_s(&self) -> &[f32] {
        &self.mean_annual_discharge_m3_s
    }

    /// Returns accumulated drainage area without copying.
    pub fn drainage_area_km2(&self) -> &[f32] {
        &self.drainage_area_km2
    }

    /// Returns the Priority-Flood drainage surface.
    pub const fn drainage_surface_elevation_m(&self) -> &ElevationField {
        &self.drainage_surface_elevation_m
    }

    /// Returns published lake depths without copying.
    pub fn lake_depth_m(&self) -> &[f32] {
        &self.lake_depth_m
    }

    /// Returns stable surface-water categories.
    pub const fn surface_water(&self) -> &SurfaceWaterField {
        &self.surface_water_kind
    }

    /// Returns direct flow receivers without copying.
    pub fn flow_receiver(&self) -> &[Option<CellId>] {
        &self.flow_receiver
    }

    /// Returns per-cell basin assignments without copying.
    pub fn basin_id(&self) -> &[Option<DrainageBasinId>] {
        &self.basin_id
    }

    /// Returns stable Strahler orders.
    pub const fn strahler_order(&self) -> &StrahlerOrderField {
        &self.strahler_order
    }

    /// Returns canonical basin records.
    pub fn basins(&self) -> &[DrainageBasin] {
        &self.basins
    }

    /// Returns canonical lake records.
    pub fn lakes(&self) -> &[Lake] {
        &self.lakes
    }

    /// Returns canonical river-segment records.
    pub fn river_segments(&self) -> &[RiverSegment] {
        &self.river_segments
    }
}

impl<'de> Deserialize<'de> for HydrologySnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = HydrologySnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.cell_count,
            wire.river_discharge_threshold_m3_s,
            wire.minimum_lake_depth_m,
            wire.monthly_local_runoff_mm,
            wire.monthly_discharge_m3_s,
            wire.annual_local_runoff_mm,
            wire.mean_annual_discharge_m3_s,
            wire.drainage_area_km2,
            wire.drainage_surface_elevation_m,
            wire.lake_depth_m,
            wire.surface_water_kind,
            wire.flow_receiver,
            wire.basin_id,
            wire.strahler_order,
            wire.basins,
            wire.lakes,
            wire.river_segments,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_length(
    field: &'static str,
    found: usize,
    cell_count: u32,
) -> Result<(), HydrologyValidationError> {
    let expected = cell_count as usize;
    if found != expected {
        return Err(HydrologyValidationError::FieldLengthMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

fn validate_setting(
    field: &'static str,
    found: f32,
    min: f32,
    max: f32,
) -> Result<(), HydrologyValidationError> {
    if !found.is_finite() || !(min..=max).contains(&found) {
        return Err(HydrologyValidationError::SettingOutOfRange {
            field,
            found,
            min,
            max,
        });
    }
    Ok(())
}

fn validate_monthly_values(
    field: &'static str,
    values: &[[f32; MONTH_COUNT]],
    min: f32,
    max: f32,
) -> Result<(), HydrologyValidationError> {
    for (index, months) in values.iter().enumerate() {
        for (month, &found) in months.iter().enumerate() {
            if !found.is_finite() || !(min..=max).contains(&found) {
                return Err(HydrologyValidationError::ScalarValueOutOfRange {
                    field,
                    cell: CellId::from_raw(index as u32),
                    month: Some(month),
                    found,
                    min,
                    max,
                });
            }
        }
    }
    Ok(())
}

fn validate_scalar_values(
    field: &'static str,
    values: &[f32],
    min: f32,
    max: f32,
    strictly_positive: bool,
) -> Result<(), HydrologyValidationError> {
    for (index, &found) in values.iter().enumerate() {
        let in_range = if strictly_positive {
            found > min && found <= max
        } else {
            (min..=max).contains(&found)
        };
        if !found.is_finite() || !in_range {
            return Err(HydrologyValidationError::ScalarValueOutOfRange {
                field,
                cell: CellId::from_raw(index as u32),
                month: None,
                found,
                min,
                max,
            });
        }
    }
    Ok(())
}

fn validate_positive_record_value(
    record: &'static str,
    id: u32,
    found: f64,
) -> Result<(), HydrologyValidationError> {
    if !found.is_finite() || found <= 0.0 {
        return Err(HydrologyValidationError::InvalidRecordValue { record, id, found });
    }
    Ok(())
}

fn validate_nonnegative_record_value(
    record: &'static str,
    id: u32,
    found: f64,
) -> Result<(), HydrologyValidationError> {
    if !found.is_finite() || found < 0.0 {
        return Err(HydrologyValidationError::InvalidRecordValue { record, id, found });
    }
    Ok(())
}

fn nearly_equal(stored: f64, calculated: f64, absolute_tolerance: f64) -> bool {
    let tolerance = absolute_tolerance
        .max(stored.abs().max(calculated.abs()) * HYDROLOGY_SUMMARY_RELATIVE_TOLERANCE);
    (stored - calculated).abs() <= tolerance
}

fn check_downstream_value(
    field: &'static str,
    cell: CellId,
    receiver: CellId,
    upstream: f64,
    downstream: f64,
    absolute_tolerance: f64,
) -> Result<(), HydrologyValidationError> {
    let tolerance = absolute_tolerance
        .max(upstream.abs().max(downstream.abs()) * HYDROLOGY_SUMMARY_RELATIVE_TOLERANCE);
    if downstream + tolerance < upstream {
        return Err(HydrologyValidationError::DownstreamValueDecreases {
            field,
            cell,
            receiver,
            upstream,
            downstream,
        });
    }
    Ok(())
}

/// Errors returned when hydrology records violate the V1 contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum HydrologyValidationError {
    #[error("unsupported hydrology schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("setting {field} value {found} is outside finite {min}..={max}")]
    SettingOutOfRange {
        field: &'static str,
        found: f32,
        min: f32,
        max: f32,
    },
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("field {field} value {found} at {cell:?}, month {month:?}, is outside {min}..={max}")]
    ScalarValueOutOfRange {
        field: &'static str,
        cell: CellId,
        month: Option<usize>,
        found: f32,
        min: f32,
        max: f32,
    },
    #[error("invalid surface-water category {found} at {cell:?}")]
    InvalidSurfaceWaterKind { cell: Option<CellId>, found: u32 },
    #[error("Strahler order {found} at {cell:?} exceeds {max}")]
    InvalidStrahlerOrder { cell: CellId, found: u32, max: u8 },
    #[error("record {record} {id} has invalid value {found}")]
    InvalidRecordValue {
        record: &'static str,
        id: u32,
        found: f64,
    },
    #[error("lake {lake:?} has no member cells")]
    EmptyLake { lake: LakeId },
    #[error("lake {lake:?} repeats cell {cell:?}")]
    DuplicateLakeCell { lake: LakeId, cell: CellId },
    #[error("lake {lake:?} cells are not in stable ascending order")]
    UnsortedLakeCells { lake: LakeId },
    #[error("lake {lake:?} must provide both outlet and downstream cells or neither")]
    IncompleteLakeOutlet { lake: LakeId },
    #[error("lake {lake:?} outlet {outlet:?} is not a member cell")]
    LakeOutletNotContained { lake: LakeId, outlet: CellId },
    #[error("lake {lake:?} downstream cell {downstream:?} remains inside the lake")]
    LakeOutletRemainsInside { lake: LakeId, downstream: CellId },
    #[error("river segment {segment:?} is a self edge at {cell:?}")]
    SelfRiverSegment {
        segment: RiverSegmentId,
        cell: CellId,
    },
    #[error("river segment {segment:?} has invalid Strahler order {found}")]
    InvalidSegmentStrahlerOrder { segment: RiverSegmentId, found: u8 },
    #[error("summary {field} at {cell:?} stores {stored}; calculated {calculated}")]
    SummaryIdentityMismatch {
        field: &'static str,
        cell: CellId,
        stored: f64,
        calculated: f64,
    },
    #[error(
        "lake depth {depth} at {cell:?} disagrees with {kind:?} and minimum {minimum_lake_depth}"
    )]
    LakeDepthKindMismatch {
        cell: CellId,
        kind: SurfaceWaterKind,
        depth: f32,
        minimum_lake_depth: f32,
    },
    #[error("receiver {receiver:?} for {cell:?} is outside cell count {cell_count}")]
    ReceiverOutOfRange {
        cell: CellId,
        receiver: CellId,
        cell_count: u32,
    },
    #[error("cell {cell:?} receives itself")]
    SelfReceiver { cell: CellId },
    #[error("receiver graph contains a cycle through {cell:?}")]
    ReceiverCycle { cell: CellId },
    #[error("ocean cell {cell:?} has a receiver")]
    OceanHasReceiver { cell: CellId },
    #[error("cell {cell:?} receiver {receiver:?} is uphill on the drainage surface")]
    ReceiverFlowsUphill { cell: CellId, receiver: CellId },
    #[error("{field} decreases from {upstream} at {cell:?} to {downstream} at {receiver:?}")]
    DownstreamValueDecreases {
        field: &'static str,
        cell: CellId,
        receiver: CellId,
        upstream: f64,
        downstream: f64,
    },
    #[error("{record} at position {position} has non-contiguous ID {found}")]
    NonContiguousRecordId {
        record: &'static str,
        position: usize,
        found: u32,
    },
    #[error("{record} record {id} references cell {cell:?} outside count {cell_count}")]
    RecordCellOutOfRange {
        record: &'static str,
        id: u32,
        cell: CellId,
        cell_count: u32,
    },
    #[error("basin {basin:?} outlet {outlet:?} has a receiver")]
    BasinOutletHasReceiver {
        basin: DrainageBasinId,
        outlet: CellId,
    },
    #[error("basin {basin:?} stores {stored:?}; expected outlet kind {expected:?}")]
    BasinOutletKindMismatch {
        basin: DrainageBasinId,
        stored: BasinOutletKind,
        expected: BasinOutletKind,
    },
    #[error("basin {basin:?} aggregate {field} disagrees with its outlet field")]
    BasinAggregateMismatch {
        basin: DrainageBasinId,
        field: &'static str,
    },
    #[error("ocean cell {cell:?} unexpectedly belongs to basin {found:?}")]
    OceanHasBasin {
        cell: CellId,
        found: DrainageBasinId,
    },
    #[error("non-ocean cell {cell:?} has no basin")]
    MissingBasin { cell: CellId },
    #[error("cell {cell:?} basin {found:?} is outside basin count {basin_count}")]
    BasinIdOutOfRange {
        cell: CellId,
        found: DrainageBasinId,
        basin_count: usize,
    },
    #[error("cell {cell:?} terminates at {terminal:?}, not basin {basin:?} outlet {outlet:?}")]
    BasinTerminalMismatch {
        cell: CellId,
        basin: DrainageBasinId,
        terminal: CellId,
        outlet: CellId,
    },
    #[error("basin {basin:?} is not referenced by any non-ocean cell")]
    UnusedBasin { basin: DrainageBasinId },
    #[error("lake cell {cell:?} belongs to both {first:?} and {second:?}")]
    LakeCellOverlap {
        cell: CellId,
        first: LakeId,
        second: LakeId,
    },
    #[error("lake {lake:?} cell {cell:?} surface {stored} differs from lake surface {expected}")]
    LakeSurfaceMismatch {
        lake: LakeId,
        cell: CellId,
        stored: f32,
        expected: f32,
    },
    #[error("lake {lake:?} outlet {outlet:?} does not receive into {downstream:?}")]
    LakeOutletDirectionMismatch {
        lake: LakeId,
        outlet: CellId,
        downstream: CellId,
    },
    #[error("closed lake {lake:?} has no terminal member cell")]
    ClosedLakeHasNoTerminal { lake: LakeId },
    #[error("cell {cell:?} water kind {kind:?} disagrees with lake membership {lake:?}")]
    LakeCoverageMismatch {
        cell: CellId,
        kind: SurfaceWaterKind,
        lake: Option<LakeId>,
    },
    #[error("segment {segment:?} {from:?}->{to:?} disagrees with receiver {receiver:?}")]
    SegmentDirectionMismatch {
        segment: RiverSegmentId,
        from: CellId,
        to: CellId,
        receiver: Option<CellId>,
    },
    #[error("cell {cell:?} has duplicate segments {first:?} and {second:?}")]
    DuplicateRiverOrigin {
        cell: CellId,
        first: RiverSegmentId,
        second: RiverSegmentId,
    },
    #[error("segment {segment:?} kind {kind:?} disagrees with origin water {water:?}")]
    SegmentKindMismatch {
        segment: RiverSegmentId,
        kind: RiverSegmentKind,
        water: SurfaceWaterKind,
    },
    #[error("segment {segment:?} discharge {discharge} is below threshold {threshold}")]
    RiverBelowThreshold {
        segment: RiverSegmentId,
        discharge: f32,
        threshold: f32,
    },
    #[error("segment {segment:?} discharge disagrees with its origin field")]
    SegmentDischargeMismatch { segment: RiverSegmentId },
    #[error("segment {segment:?} order {stored} disagrees with origin field {field}")]
    SegmentStrahlerMismatch {
        segment: RiverSegmentId,
        stored: u8,
        field: u8,
    },
    #[error("nonriver cell {cell:?} has Strahler order {order}")]
    NonRiverStrahlerOrder { cell: CellId, order: u8 },
    #[error("cell {cell:?} eligibility {eligible} disagrees with segment {segment:?}")]
    RiverCoverageMismatch {
        cell: CellId,
        eligible: bool,
        segment: Option<RiverSegmentId>,
    },
    #[error("segment {segment:?} order {stored} does not match topology result {calculated}")]
    InvalidStrahlerTopology {
        segment: RiverSegmentId,
        stored: u8,
        calculated: u8,
    },
    #[error("hydrology cell count {hydrology} does not match spatial count {spatial}")]
    SpatialCellCountMismatch { hydrology: u32, spatial: usize },
    #[error("receiver {receiver:?} for {cell:?} is not a spatial neighbor")]
    ReceiverNotAdjacent { cell: CellId, receiver: CellId },
    #[error("cell {cell:?} drainage area {stored} differs from accumulation {calculated}")]
    DrainageAreaAccumulationMismatch {
        cell: CellId,
        stored: f64,
        calculated: f64,
    },
    #[error(
        "cell {cell:?} month {month} discharge {stored} differs from accumulation {calculated}"
    )]
    DischargeAccumulationMismatch {
        cell: CellId,
        month: usize,
        stored: f64,
        calculated: f64,
    },
    #[error("lake {lake:?} {field} {stored} differs from spatial aggregate {calculated}")]
    LakeAggregateMismatch {
        lake: LakeId,
        field: &'static str,
        stored: f64,
        calculated: f64,
    },
    #[error("invalid spatial input: {0}")]
    Spatial(#[from] SpatialValidationError),
}
