use std::fmt;

use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    BasinOutletKind, DrainageBasin, ElevationField, HydrologySnapshot, HydrologyValidationError,
    Lake, RiverSegment, RiverSegmentKind, StrahlerOrderField, SurfaceWaterField,
    CLIMATE_MONTH_COUNT, HYDROLOGY_SCHEMA_V2,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceGeometryKind, SurfaceRef,
    SurfaceRefError,
};
use crate::world::{
    CellId, DrainageBasinId, LakeId, RiverSegmentId, MAX_SPHERICAL_CELL_COUNT,
    MAX_SPHERICAL_EDGE_COUNT,
};

const MAX_SPHERICAL_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_SPHERICAL_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;
const RIVER_LENGTH_ABSOLUTE_TOLERANCE_M: f64 = 1.0e-6;
const RIVER_LENGTH_RELATIVE_TOLERANCE: f64 = 1.0e-12;

/// Surface-bound V2 hydrology semantics for one authoritative closed sphere.
///
/// The nested V1 payload is the sole definition of common water, basin, lake,
/// and river semantics. This envelope adds exact surface identity and the one
/// physical reach measure that the planar V1 wire did not publish.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalHydrologySnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    hydrology: HydrologySnapshot,
    river_segment_length_m: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalHydrologySnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    hydrology: StrictHydrologySnapshotWire,
    #[serde(deserialize_with = "deserialize_spherical_f64_values")]
    river_segment_length_m: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictHydrologySnapshotWire {
    schema_version: u16,
    cell_count: u32,
    river_discharge_threshold_m3_s: f32,
    minimum_lake_depth_m: f32,
    #[serde(deserialize_with = "deserialize_spherical_monthly_values")]
    monthly_local_runoff_mm: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    #[serde(deserialize_with = "deserialize_spherical_monthly_values")]
    monthly_discharge_m3_s: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    #[serde(deserialize_with = "deserialize_spherical_f32_values")]
    annual_local_runoff_mm: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_f32_values")]
    mean_annual_discharge_m3_s: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_f32_values")]
    drainage_area_km2: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_f32_values")]
    drainage_surface_elevation_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_f32_values")]
    lake_depth_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_u32_values")]
    surface_water_kind: Vec<u32>,
    #[serde(deserialize_with = "deserialize_spherical_optional_cells")]
    flow_receiver: Vec<Option<CellId>>,
    #[serde(deserialize_with = "deserialize_spherical_optional_basins")]
    basin_id: Vec<Option<DrainageBasinId>>,
    #[serde(deserialize_with = "deserialize_spherical_u32_values")]
    strahler_order: Vec<u32>,
    #[serde(deserialize_with = "deserialize_strict_basins")]
    basins: Vec<StrictDrainageBasinWire>,
    #[serde(deserialize_with = "deserialize_strict_lakes")]
    lakes: Vec<StrictLakeWire>,
    #[serde(deserialize_with = "deserialize_strict_rivers")]
    river_segments: Vec<StrictRiverSegmentWire>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictDrainageBasinWire {
    id: DrainageBasinId,
    outlet_cell: CellId,
    outlet_kind: BasinOutletKind,
    area_km2: f64,
    mean_discharge_m3_s: f32,
}

impl StrictDrainageBasinWire {
    fn into_record(self) -> Result<DrainageBasin, HydrologyValidationError> {
        DrainageBasin::new(
            self.id,
            self.outlet_cell,
            self.outlet_kind,
            self.area_km2,
            self.mean_discharge_m3_s,
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictLakeWire {
    id: LakeId,
    #[serde(deserialize_with = "deserialize_spherical_cells")]
    cells: Vec<CellId>,
    surface_elevation_m: f32,
    area_km2: f64,
    volume_m3: f64,
    outlet_cell: Option<CellId>,
    downstream_cell: Option<CellId>,
}

impl StrictLakeWire {
    fn into_record(self) -> Result<Lake, HydrologyValidationError> {
        Lake::new(
            self.id,
            self.cells,
            self.surface_elevation_m,
            self.area_km2,
            self.volume_m3,
            self.outlet_cell,
            self.downstream_cell,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictRiverSegmentWire {
    id: RiverSegmentId,
    from: CellId,
    to: CellId,
    kind: RiverSegmentKind,
    strahler_order: u8,
    mean_discharge_m3_s: f32,
}

impl StrictRiverSegmentWire {
    fn into_record(self) -> Result<RiverSegment, HydrologyValidationError> {
        RiverSegment::new(
            self.id,
            self.from,
            self.to,
            self.kind,
            self.strahler_order,
            self.mean_discharge_m3_s,
        )
    }
}

fn deserialize_spherical_monthly_values<'de, D>(
    deserializer: D,
) -> Result<Vec<[f32; CLIMATE_MONTH_COUNT]>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_f32_values<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_f64_values<'de, D>(deserializer: D) -> Result<Vec<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_u32_values<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_cells<'de, D>(deserializer: D) -> Result<Vec<CellId>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_optional_cells<'de, D>(
    deserializer: D,
) -> Result<Vec<Option<CellId>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_optional_basins<'de, D>(
    deserializer: D,
) -> Result<Vec<Option<DrainageBasinId>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_strict_basins<'de, D>(
    deserializer: D,
) -> Result<Vec<StrictDrainageBasinWire>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_strict_rivers<'de, D>(
    deserializer: D,
) -> Result<Vec<StrictRiverSegmentWire>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_strict_lakes<'de, D>(deserializer: D) -> Result<Vec<StrictLakeWire>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_strict_lakes_with_limit::<_, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_strict_lakes_with_limit<'de, D, const MAX: usize>(
    deserializer: D,
) -> Result<Vec<StrictLakeWire>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StrictLakesVisitor<const MAX: usize>;

    impl<'de, const MAX: usize> Visitor<'de> for StrictLakesVisitor<MAX> {
        type Value = Vec<StrictLakeWire>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "at most {MAX} lakes containing at most {MAX} total member cells"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if let Some(length) = sequence.size_hint() {
                if length > MAX {
                    return Err(A::Error::invalid_length(length, &self));
                }
            }
            let mut lakes = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX));
            let mut member_count = 0_usize;
            while lakes.len() < MAX {
                let Some(lake) = sequence.next_element::<StrictLakeWire>()? else {
                    return Ok(lakes);
                };
                member_count = member_count
                    .checked_add(lake.cells.len())
                    .ok_or_else(|| A::Error::custom("lake member count overflow"))?;
                if member_count > MAX {
                    return Err(A::Error::custom(format_args!(
                        "lakes contain {member_count} member cells; at most {MAX} are allowed"
                    )));
                }
                lakes.push(lake);
            }
            if sequence.next_element::<IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(MAX + 1, &self));
            }
            Ok(lakes)
        }
    }

    deserializer.deserialize_seq(StrictLakesVisitor::<MAX>)
}

impl StrictHydrologySnapshotWire {
    fn into_snapshot(self) -> Result<HydrologySnapshot, String> {
        let drainage_surface_elevation_m =
            ElevationField::from_values(self.drainage_surface_elevation_m)
                .map_err(|error| error.to_string())?;
        let surface_water_kind = SurfaceWaterField::from_raw(self.surface_water_kind)
            .map_err(|error| error.to_string())?;
        let strahler_order =
            StrahlerOrderField::from_raw(self.strahler_order).map_err(|error| error.to_string())?;
        let basins = self
            .basins
            .into_iter()
            .map(StrictDrainageBasinWire::into_record)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let lakes = self
            .lakes
            .into_iter()
            .map(StrictLakeWire::into_record)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let river_segments = self
            .river_segments
            .into_iter()
            .map(StrictRiverSegmentWire::into_record)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
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
            drainage_surface_elevation_m,
            self.lake_depth_m,
            surface_water_kind,
            self.flow_receiver,
            self.basin_id,
            strahler_order,
            basins,
            lakes,
            river_segments,
        )
        .map_err(|error| error.to_string())
    }
}

impl SphericalHydrologySnapshot {
    /// Constructs a surface-bound snapshot after validating all V2-local invariants.
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        hydrology: HydrologySnapshot,
        river_segment_length_m: Vec<f64>,
    ) -> Result<Self, SphericalHydrologyValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            hydrology,
            river_segment_length_m,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks every invariant that does not require authoritative surface records.
    pub fn validate(&self) -> Result<(), SphericalHydrologyValidationError> {
        if self.schema_version != HYDROLOGY_SCHEMA_V2 {
            return Err(SphericalHydrologyValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: HYDROLOGY_SCHEMA_V2,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(SphericalHydrologyValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        validate_allocation_limit(
            "surface_ref.cell_count",
            self.surface_ref.cell_count() as usize,
            MAX_SPHERICAL_CELLS,
        )?;
        validate_allocation_limit(
            "surface_ref.edge_count",
            self.surface_ref.edge_count() as usize,
            MAX_SPHERICAL_EDGES,
        )?;
        self.hydrology.validate()?;
        if self.hydrology.cell_count() != self.surface_ref.cell_count() {
            return Err(SphericalHydrologyValidationError::CellCountMismatch {
                hydrology: self.hydrology.cell_count(),
                surface: self.surface_ref.cell_count(),
            });
        }
        if self.river_segment_length_m.len() != self.hydrology.river_segments().len() {
            return Err(
                SphericalHydrologyValidationError::RiverLengthCountMismatch {
                    lengths: self.river_segment_length_m.len(),
                    segments: self.hydrology.river_segments().len(),
                },
            );
        }
        for (index, &found) in self.river_segment_length_m.iter().enumerate() {
            if !found.is_finite() || found <= 0.0 {
                return Err(SphericalHydrologyValidationError::InvalidRiverLength {
                    segment: RiverSegmentId::from_raw(index as u32),
                    found,
                });
            }
        }
        Ok(())
    }

    /// Validates exact identity, spherical adjacency, area budgets, and reach lengths.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SphericalHydrologyValidationError> {
        surface.validate()?;
        self.validate_against_validated_surface(surface)
    }

    pub(crate) fn validate_against_validated_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SphericalHydrologyValidationError> {
        self.validate()?;
        let authoritative = SurfaceRef::from_validated_spherical(surface)?;
        if self.surface_ref != authoritative {
            return Err(SphericalHydrologyValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        validate_metric_relations(&self.hydrology, &self.river_segment_length_m, surface)
    }

    /// Returns the V2 envelope schema.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact authoritative surface identity.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the dense cell allocation encoded by the surface identity.
    pub const fn cell_count(&self) -> u32 {
        self.surface_ref.cell_count()
    }

    /// Returns the published river threshold.
    pub const fn river_discharge_threshold_m3_s(&self) -> f32 {
        self.hydrology.river_discharge_threshold_m3_s()
    }

    /// Returns the published minimum lake depth.
    pub const fn minimum_lake_depth_m(&self) -> f32 {
        self.hydrology.minimum_lake_depth_m()
    }

    /// Returns monthly local effective runoff.
    pub fn monthly_local_runoff_mm(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        self.hydrology.monthly_local_runoff_mm()
    }

    /// Returns monthly accumulated discharge.
    pub fn monthly_discharge_m3_s(&self) -> &[[f32; CLIMATE_MONTH_COUNT]] {
        self.hydrology.monthly_discharge_m3_s()
    }

    /// Returns annual local effective runoff.
    pub fn annual_local_runoff_mm(&self) -> &[f32] {
        self.hydrology.annual_local_runoff_mm()
    }

    /// Returns mean annual accumulated discharge.
    pub fn mean_annual_discharge_m3_s(&self) -> &[f32] {
        self.hydrology.mean_annual_discharge_m3_s()
    }

    /// Returns accumulated drainage areas.
    pub fn drainage_area_km2(&self) -> &[f32] {
        self.hydrology.drainage_area_km2()
    }

    /// Returns the Priority-Flood drainage surface.
    pub const fn drainage_surface_elevation_m(&self) -> &ElevationField {
        self.hydrology.drainage_surface_elevation_m()
    }

    /// Returns lake depths aligned to cells.
    pub fn lake_depth_m(&self) -> &[f32] {
        self.hydrology.lake_depth_m()
    }

    /// Returns current surface-water categories.
    pub const fn surface_water(&self) -> &SurfaceWaterField {
        self.hydrology.surface_water()
    }

    /// Returns the direct adjacent receiver of every nonterminal cell.
    pub fn flow_receiver(&self) -> &[Option<CellId>] {
        self.hydrology.flow_receiver()
    }

    /// Returns terminal-basin membership.
    pub fn basin_id(&self) -> &[Option<DrainageBasinId>] {
        self.hydrology.basin_id()
    }

    /// Returns raw Strahler orders aligned to cells.
    pub const fn strahler_order(&self) -> &StrahlerOrderField {
        self.hydrology.strahler_order()
    }

    /// Returns canonical basin records.
    pub fn basins(&self) -> &[DrainageBasin] {
        self.hydrology.basins()
    }

    /// Returns canonical lake records.
    pub fn lakes(&self) -> &[Lake] {
        self.hydrology.lakes()
    }

    /// Returns canonical directed river reaches.
    pub fn river_segments(&self) -> &[RiverSegment] {
        self.hydrology.river_segments()
    }

    /// Returns great-circle center-to-center lengths aligned to river IDs.
    pub fn river_segment_length_m(&self) -> &[f64] {
        &self.river_segment_length_m
    }

    pub(crate) const fn semantic_payload(&self) -> &HydrologySnapshot {
        &self.hydrology
    }
}

impl<'de> Deserialize<'de> for SphericalHydrologySnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalHydrologySnapshotWire::deserialize(deserializer)?;
        let hydrology = wire.hydrology.into_snapshot().map_err(D::Error::custom)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            hydrology,
            wire.river_segment_length_m,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_metric_relations(
    hydrology: &HydrologySnapshot,
    river_lengths_m: &[f64],
    surface: &SphericalSurfaceSnapshot,
) -> Result<(), SphericalHydrologyValidationError> {
    if hydrology.cell_count() as usize != surface.cells().len() {
        return Err(SphericalHydrologyValidationError::CellCountMismatch {
            hydrology: hydrology.cell_count(),
            surface: surface.cells().len() as u32,
        });
    }

    hydrology.validate_metric_relations(
        surface.cells().len(),
        |cell| {
            surface
                .cell(cell)
                .expect("validated spherical cells are dense")
                .area
                .get()
        },
        |cell, receiver| {
            surface.cell_edges(cell).is_some_and(|edges| {
                edges
                    .iter()
                    .any(|&edge| surface.opposite_cell(cell, edge) == Some(receiver))
            })
        },
    )?;

    for (index, segment) in hydrology.river_segments().iter().enumerate() {
        let edge = surface
            .cell_edges(segment.from())
            .and_then(|edges| {
                edges.iter().find_map(|&edge| {
                    (surface.opposite_cell(segment.from(), edge) == Some(segment.to()))
                        .then_some(edge)
                })
            })
            .ok_or(SphericalHydrologyValidationError::RiverSegmentNotAdjacent {
                segment: segment.id(),
                from: segment.from(),
                to: segment.to(),
            })?;
        let expected = surface
            .edge(edge)
            .expect("validated spherical cell edge exists")
            .center_distance
            .get();
        let stored = river_lengths_m[index];
        let tolerance = RIVER_LENGTH_ABSOLUTE_TOLERANCE_M
            .max(expected.abs().max(stored.abs()) * RIVER_LENGTH_RELATIVE_TOLERANCE);
        if (stored - expected).abs() > tolerance {
            return Err(SphericalHydrologyValidationError::RiverLengthMismatch {
                segment: segment.id(),
                stored,
                expected,
            });
        }
    }
    Ok(())
}

fn validate_allocation_limit(
    field: &'static str,
    found: usize,
    max: usize,
) -> Result<(), SphericalHydrologyValidationError> {
    if found > max {
        return Err(SphericalHydrologyValidationError::AllocationLimitExceeded {
            field,
            found,
            max,
        });
    }
    Ok(())
}

/// Errors returned when surface-bound spherical hydrology violates its V2 contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalHydrologyValidationError {
    /// The outer V2 schema is unsupported.
    #[error("unsupported spherical hydrology schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The exact surface identity is malformed.
    #[error("invalid spherical hydrology surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    /// The identity does not refer to a spherical V1 geometry.
    #[error("spherical hydrology requires SphericalV1 geometry, found {found:?}")]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    /// A declared allocation exceeds the supported spherical schema budget.
    #[error("field {field} declares {found} records; maximum is {max}")]
    AllocationLimitExceeded {
        field: &'static str,
        found: usize,
        max: usize,
    },
    /// Common hydrology semantics and the surface identity disagree on cardinality.
    #[error("hydrology cell count {hydrology} does not match spherical surface count {surface}")]
    CellCountMismatch { hydrology: u32, surface: u32 },
    /// Reach lengths are not aligned one-for-one with river IDs.
    #[error("river length count {lengths} does not match segment count {segments}")]
    RiverLengthCountMismatch { lengths: usize, segments: usize },
    /// A published reach length is not finite and positive.
    #[error("river segment {segment:?} has invalid length {found} m")]
    InvalidRiverLength { segment: RiverSegmentId, found: f64 },
    /// The snapshot references a different authoritative spherical surface.
    #[error("spherical hydrology surface {snapshot:?} does not match {authoritative:?}")]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    /// A river record does not correspond to a real authoritative shared edge.
    #[error("river segment {segment:?} {from:?}->{to:?} is not a spherical neighbor edge")]
    RiverSegmentNotAdjacent {
        segment: RiverSegmentId,
        from: CellId,
        to: CellId,
    },
    /// A stored reach length differs from authoritative great-circle center distance.
    #[error("river segment {segment:?} length {stored} differs from geodesic {expected}")]
    RiverLengthMismatch {
        segment: RiverSegmentId,
        stored: f64,
        expected: f64,
    },
    /// The authoritative surface itself is invalid.
    #[error("invalid spherical surface input: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The shared hydrology semantics are invalid.
    #[error("invalid hydrology semantics: {0}")]
    InvalidHydrology(#[from] HydrologyValidationError),
}

#[cfg(test)]
mod tests {
    use super::deserialize_strict_lakes_with_limit;

    #[test]
    fn strict_lakes_share_one_aggregate_member_budget() {
        let json = r#"[
            {"id":0,"cells":[0,1],"surface_elevation_m":1.0,"area_km2":1.0,
             "volume_m3":1.0,"outlet_cell":null,"downstream_cell":null},
            {"id":1,"cells":[2],"surface_elevation_m":1.0,"area_km2":1.0,
             "volume_m3":1.0,"outlet_cell":null,"downstream_cell":null}
        ]"#;
        let mut deserializer = serde_json::Deserializer::from_str(json);
        let error = deserialize_strict_lakes_with_limit::<_, 2>(&mut deserializer)
            .unwrap_err()
            .to_string();
        assert!(error.contains("at most 2 are allowed"), "{error}");
    }
}
