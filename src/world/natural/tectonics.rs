use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::world::spatial::{SpatialEdge, SpatialSnapshot, Topology};
use crate::world::{BoundarySegmentId, CellId, EdgeId, PlateId, WorldPoint};

/// The supported version of the serialized tectonic snapshot schema.
pub const TECTONIC_SNAPSHOT_SCHEMA_V1: u16 = 1;
/// The largest absolute plate-velocity component supported by V1, in millimeters per year.
pub const MAX_PLATE_VELOCITY_MM_PER_YEAR: i16 = 120;
/// The thinnest supported oceanic crust, in kilometers.
pub const OCEANIC_CRUST_MIN_THICKNESS_KM: f32 = 3.0;
/// The thickest supported oceanic crust, in kilometers.
pub const OCEANIC_CRUST_MAX_THICKNESS_KM: f32 = 15.0;
/// The thinnest supported continental crust, in kilometers.
pub const CONTINENTAL_CRUST_MIN_THICKNESS_KM: f32 = 20.0;
/// The thickest supported continental crust, in kilometers.
pub const CONTINENTAL_CRUST_MAX_THICKNESS_KM: f32 = 80.0;

const STRENGTH_TOLERANCE: f32 = 1.0e-5;

/// A two-dimensional fixed-point plate velocity in millimeters per year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlateVelocity {
    x_mm_per_year: i16,
    y_mm_per_year: i16,
}

impl PlateVelocity {
    /// Creates a velocity when both components are inside the V1 physical bound.
    pub fn new(x_mm_per_year: i16, y_mm_per_year: i16) -> Result<Self, TectonicValidationError> {
        let velocity = Self {
            x_mm_per_year,
            y_mm_per_year,
        };
        velocity.validate()?;
        Ok(velocity)
    }

    /// Returns the horizontal and vertical components in millimeters per year.
    pub const fn components_mm_per_year(self) -> [i16; 2] {
        [self.x_mm_per_year, self.y_mm_per_year]
    }

    fn validate(self) -> Result<(), TectonicValidationError> {
        for component in self.components_mm_per_year() {
            if !(-MAX_PLATE_VELOCITY_MM_PER_YEAR..=MAX_PLATE_VELOCITY_MM_PER_YEAR)
                .contains(&component)
            {
                return Err(TectonicValidationError::PlateVelocityOutOfRange {
                    found: component,
                    min: -MAX_PLATE_VELOCITY_MM_PER_YEAR,
                    max: MAX_PLATE_VELOCITY_MM_PER_YEAR,
                });
            }
        }
        Ok(())
    }
}

/// A tectonic plate in the current world slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plate {
    /// The contiguous stable identifier of this plate.
    pub id: PlateId,
    /// A spatial cell guaranteed to belong to this plate.
    pub seed_cell: CellId,
    /// The plate's fixed-point planar velocity.
    pub velocity: PlateVelocity,
}

/// The broad material class of a cell's crust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrustKind {
    /// Thin, dense oceanic crust.
    Oceanic,
    /// Thick, buoyant continental crust.
    Continental,
}

impl CrustKind {
    /// Decodes the stable V1 category value.
    pub fn try_from_raw(raw: u32) -> Result<Self, TectonicValidationError> {
        match raw {
            0 => Ok(Self::Oceanic),
            1 => Ok(Self::Continental),
            found => Err(TectonicValidationError::InvalidCrustKind { cell: None, found }),
        }
    }

    /// Returns the stable V1 category value.
    pub const fn raw(self) -> u32 {
        match self {
            Self::Oceanic => 0,
            Self::Continental => 1,
        }
    }

    fn thickness_range(self) -> (f32, f32) {
        match self {
            Self::Oceanic => (
                OCEANIC_CRUST_MIN_THICKNESS_KM,
                OCEANIC_CRUST_MAX_THICKNESS_KM,
            ),
            Self::Continental => (
                CONTINENTAL_CRUST_MIN_THICKNESS_KM,
                CONTINENTAL_CRUST_MAX_THICKNESS_KM,
            ),
        }
    }
}

/// A dense, display-borrowable field of raw plate identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlateIdField(Vec<u32>);

impl PlateIdField {
    /// Encodes typed plate identifiers into stable raw category storage.
    pub fn from_ids(values: Vec<PlateId>) -> Self {
        Self(values.into_iter().map(PlateId::raw).collect())
    }

    /// Constructs a field from already encoded V1 values.
    pub fn from_raw(values: Vec<u32>) -> Self {
        Self(values)
    }

    /// Returns a typed plate identifier at the requested dense index.
    pub fn get(&self, index: usize) -> Option<PlateId> {
        self.0.get(index).copied().map(PlateId::from_raw)
    }

    /// Returns the encoded values without copying them.
    pub fn raw_values(&self) -> &[u32] {
        &self.0
    }

    /// Returns the number of dense values.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether this field contains no values.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A dense, display-borrowable field of raw crust categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CrustKindField(Vec<u32>);

impl CrustKindField {
    /// Encodes typed crust categories into stable raw category storage.
    pub fn from_kinds(values: Vec<CrustKind>) -> Self {
        Self(values.into_iter().map(CrustKind::raw).collect())
    }

    /// Validates and constructs a field from encoded V1 values.
    pub fn from_raw(values: Vec<u32>) -> Result<Self, TectonicValidationError> {
        for (index, &value) in values.iter().enumerate() {
            CrustKind::try_from_raw(value).map_err(|_| {
                TectonicValidationError::InvalidCrustKind {
                    cell: Some(CellId::from_raw(index as u32)),
                    found: value,
                }
            })?;
        }
        Ok(Self(values))
    }

    /// Returns a typed crust category at the requested dense index.
    pub fn get(&self, index: usize) -> Option<CrustKind> {
        self.0
            .get(index)
            .and_then(|&raw| CrustKind::try_from_raw(raw).ok())
    }

    /// Returns the encoded values without copying them.
    pub fn raw_values(&self) -> &[u32] {
        &self.0
    }

    /// Returns the number of dense values.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether this field contains no values.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The present-day tectonic interpretation of a spatial edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundaryKind {
    /// No tectonic event occurs on this edge.
    None,
    /// Relative motion is too weak for a stronger classification.
    Weak,
    /// Two continental crust regions converge.
    ContinentalCollision,
    /// One plate descends beneath the other.
    Subduction,
    /// Continental crust diverges.
    ContinentalRift,
    /// Oceanic crust diverges and forms a ridge.
    OceanicRidge,
    /// Plates move primarily parallel to their shared boundary.
    Transform,
}

/// An edge-aligned current tectonic event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundaryRecord {
    /// The classified event, or [`BoundaryKind::None`] away from plate boundaries.
    pub kind: BoundaryKind,
    /// The normalized event strength.
    pub strength: f32,
    /// The boundary segment containing this edge.
    pub segment_id: Option<BoundarySegmentId>,
    /// The descending plate for a subduction event.
    pub subducting_plate: Option<PlateId>,
}

impl BoundaryRecord {
    /// Creates an edge-aligned boundary record.
    pub const fn new(
        kind: BoundaryKind,
        strength: f32,
        segment_id: Option<BoundarySegmentId>,
        subducting_plate: Option<PlateId>,
    ) -> Self {
        Self {
            kind,
            strength,
            segment_id,
            subducting_plate,
        }
    }

    /// Returns the canonical record for an edge without a tectonic event.
    pub const fn none() -> Self {
        Self::new(BoundaryKind::None, 0.0, None, None)
    }
}

/// A connected, same-kind portion of a current plate boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundarySegment {
    /// The contiguous stable identifier of this segment.
    pub id: BoundarySegmentId,
    /// The two involved plates in ascending identifier order.
    pub plates: [PlateId; 2],
    /// The common event classification of all member edges.
    pub kind: BoundaryKind,
    /// Sorted, unique edge identifiers in this segment.
    pub member_edges: Vec<EdgeId>,
    /// The arithmetic mean of member-edge strengths.
    pub mean_strength: f32,
    /// The descending plate when this is a subduction segment.
    pub subducting_plate: Option<PlateId>,
    /// A finite aggregate tangent direction for presentation and relief rules.
    pub direction: [f32; 2],
}

/// An immutable, versioned snapshot of plates, crust, and current boundary events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TectonicSnapshot {
    schema_version: u16,
    cell_count: u32,
    edge_count: u32,
    plates: Vec<Plate>,
    cell_plates: PlateIdField,
    crust_kinds: CrustKindField,
    crust_thickness_km: Vec<f32>,
    boundaries: Vec<BoundaryRecord>,
    boundary_segments: Vec<BoundarySegment>,
}

impl TectonicSnapshot {
    /// Sorts stable-ID tables and constructs a snapshot only when all local invariants hold.
    pub fn new(
        schema_version: u16,
        cell_count: u32,
        edge_count: u32,
        mut plates: Vec<Plate>,
        cell_plates: PlateIdField,
        crust_kinds: CrustKindField,
        crust_thickness_km: Vec<f32>,
        boundaries: Vec<BoundaryRecord>,
        mut boundary_segments: Vec<BoundarySegment>,
    ) -> Result<Self, TectonicValidationError> {
        plates.sort_by_key(|plate| plate.id);
        boundary_segments.sort_by_key(|segment| segment.id);
        let snapshot = Self {
            schema_version,
            cell_count,
            edge_count,
            plates,
            cell_plates,
            crust_kinds,
            crust_thickness_km,
            boundaries,
            boundary_segments,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks all invariants that do not require an external spatial artifact.
    pub fn validate(&self) -> Result<(), TectonicValidationError> {
        if self.schema_version != TECTONIC_SNAPSHOT_SCHEMA_V1 {
            return Err(TectonicValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: TECTONIC_SNAPSHOT_SCHEMA_V1,
            });
        }
        if self.plates.is_empty() || self.plates.len() > self.cell_count as usize {
            return Err(TectonicValidationError::InvalidPlateCount {
                found: self.plates.len(),
                cell_count: self.cell_count,
            });
        }

        for (expected, plate) in self.plates.iter().enumerate() {
            if plate.id.raw() as usize != expected {
                return Err(TectonicValidationError::NonContiguousPlateId {
                    expected: PlateId::from_raw(expected as u32),
                    found: plate.id,
                });
            }
            if plate.seed_cell.raw() >= self.cell_count {
                return Err(TectonicValidationError::InvalidPlateSeed {
                    plate: plate.id,
                    seed: plate.seed_cell,
                    cell_count: self.cell_count,
                });
            }
            plate.velocity.validate()?;
        }

        validate_length("cell_plates", self.cell_plates.len(), self.cell_count)?;
        validate_length("crust_kinds", self.crust_kinds.len(), self.cell_count)?;
        validate_length(
            "crust_thickness_km",
            self.crust_thickness_km.len(),
            self.cell_count,
        )?;
        validate_length("boundaries", self.boundaries.len(), self.edge_count)?;

        for index in 0..self.cell_count as usize {
            let cell = CellId::from_raw(index as u32);
            let plate = self
                .cell_plates
                .get(index)
                .expect("dense length was validated");
            if plate.raw() as usize >= self.plates.len() {
                return Err(TectonicValidationError::InvalidCellPlate { cell, plate });
            }
            let raw_kind = self.crust_kinds.raw_values()[index];
            let kind = CrustKind::try_from_raw(raw_kind).map_err(|_| {
                TectonicValidationError::InvalidCrustKind {
                    cell: Some(cell),
                    found: raw_kind,
                }
            })?;
            let thickness = self.crust_thickness_km[index];
            let (min, max) = kind.thickness_range();
            if !thickness.is_finite() || !(min..=max).contains(&thickness) {
                return Err(TectonicValidationError::CrustThicknessOutOfRange {
                    cell,
                    kind,
                    found: thickness,
                    min,
                    max,
                });
            }
        }

        self.validate_segments_and_boundaries()
    }

    /// Validates topology-dependent plate ownership and boundary connectivity.
    pub fn validate_against(
        &self,
        spatial: &SpatialSnapshot,
    ) -> Result<(), TectonicValidationError> {
        self.validate()?;
        if spatial.cell_count() != self.cell_count as usize {
            return Err(TectonicValidationError::SpatialCellCountMismatch {
                tectonic: self.cell_count,
                spatial: spatial.cell_count(),
            });
        }
        if spatial.edges().len() != self.edge_count as usize {
            return Err(TectonicValidationError::SpatialEdgeCountMismatch {
                tectonic: self.edge_count,
                spatial: spatial.edges().len(),
            });
        }

        for plate in &self.plates {
            if self.plate_for_cell(plate.seed_cell) != Some(plate.id) {
                return Err(TectonicValidationError::PlateSeedOwnership {
                    plate: plate.id,
                    seed: plate.seed_cell,
                    owner: self.plate_for_cell(plate.seed_cell),
                });
            }
            self.validate_plate_connectivity(plate.id, spatial)?;
        }

        for edge in spatial.edges() {
            self.validate_edge_topology(edge)?;
        }
        for segment in &self.boundary_segments {
            self.validate_segment_topology(segment, spatial)?;
        }
        Ok(())
    }

    /// Returns the snapshot schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the number of cell-aligned values.
    pub const fn cell_count(&self) -> u32 {
        self.cell_count
    }

    /// Returns the number of edge-aligned values.
    pub const fn edge_count(&self) -> u32 {
        self.edge_count
    }

    /// Returns plates in stable identifier order.
    pub fn plates(&self) -> &[Plate] {
        &self.plates
    }

    /// Returns the dense raw plate-identifier field.
    pub const fn cell_plates(&self) -> &PlateIdField {
        &self.cell_plates
    }

    /// Returns the dense raw crust-category field.
    pub const fn crust_kinds(&self) -> &CrustKindField {
        &self.crust_kinds
    }

    /// Returns crust thickness values in kilometers.
    pub fn crust_thickness_km(&self) -> &[f32] {
        &self.crust_thickness_km
    }

    /// Returns edge-aligned boundary records.
    pub fn boundaries(&self) -> &[BoundaryRecord] {
        &self.boundaries
    }

    /// Returns boundary segments in stable identifier order.
    pub fn boundary_segments(&self) -> &[BoundarySegment] {
        &self.boundary_segments
    }

    /// Returns the owning plate for a cell identifier.
    pub fn plate_for_cell(&self, cell: CellId) -> Option<PlateId> {
        self.cell_plates.get(cell.raw() as usize)
    }

    /// Returns the crust category for a cell identifier.
    pub fn crust_kind(&self, cell: CellId) -> Option<CrustKind> {
        self.crust_kinds.get(cell.raw() as usize)
    }

    /// Returns the crust thickness for a cell identifier, in kilometers.
    pub fn crust_thickness_for_cell(&self, cell: CellId) -> Option<f32> {
        self.crust_thickness_km.get(cell.raw() as usize).copied()
    }

    /// Returns the boundary record for an edge identifier.
    pub fn boundary_for_edge(&self, edge: EdgeId) -> Option<&BoundaryRecord> {
        self.boundaries.get(edge.raw() as usize)
    }

    fn validate_segments_and_boundaries(&self) -> Result<(), TectonicValidationError> {
        for (index, boundary) in self.boundaries.iter().enumerate() {
            validate_strength(EdgeId::from_raw(index as u32), boundary.strength)?;
        }

        let mut membership = vec![None; self.edge_count as usize];
        for (expected, segment) in self.boundary_segments.iter().enumerate() {
            if segment.id.raw() as usize != expected {
                return Err(TectonicValidationError::NonContiguousBoundarySegmentId {
                    expected: BoundarySegmentId::from_raw(expected as u32),
                    found: segment.id,
                });
            }
            self.validate_segment(segment, &mut membership)?;
        }

        for (index, boundary) in self.boundaries.iter().enumerate() {
            let edge = EdgeId::from_raw(index as u32);
            match boundary.kind {
                BoundaryKind::None => {
                    if boundary.strength != 0.0
                        || boundary.segment_id.is_some()
                        || boundary.subducting_plate.is_some()
                        || membership[index].is_some()
                    {
                        return Err(TectonicValidationError::InvalidBoundaryRecord { edge });
                    }
                }
                _ => {
                    let segment_id = boundary
                        .segment_id
                        .ok_or(TectonicValidationError::BoundarySegmentMismatch { edge })?;
                    if membership[index] != Some(segment_id) {
                        return Err(TectonicValidationError::BoundarySegmentMismatch { edge });
                    }
                    let segment = self
                        .boundary_segments
                        .get(segment_id.raw() as usize)
                        .ok_or(TectonicValidationError::BoundarySegmentMismatch { edge })?;
                    if segment.kind != boundary.kind
                        || segment.subducting_plate != boundary.subducting_plate
                    {
                        return Err(TectonicValidationError::BoundarySegmentMismatch { edge });
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_segment(
        &self,
        segment: &BoundarySegment,
        membership: &mut [Option<BoundarySegmentId>],
    ) -> Result<(), TectonicValidationError> {
        let [first, second] = segment.plates;
        if first >= second
            || second.raw() as usize >= self.plates.len()
            || segment.kind == BoundaryKind::None
        {
            return Err(TectonicValidationError::InvalidBoundarySegment {
                segment: segment.id,
            });
        }
        if segment.member_edges.is_empty() {
            return Err(TectonicValidationError::EmptyBoundarySegment {
                segment: segment.id,
            });
        }
        if !segment.mean_strength.is_finite()
            || !(0.0..=1.0).contains(&segment.mean_strength)
            || !segment
                .direction
                .iter()
                .all(|component| component.is_finite())
        {
            return Err(TectonicValidationError::InvalidBoundarySegment {
                segment: segment.id,
            });
        }
        match segment.kind {
            BoundaryKind::Subduction => {
                if !segment
                    .subducting_plate
                    .is_some_and(|plate| segment.plates.contains(&plate))
                {
                    return Err(TectonicValidationError::InvalidBoundarySegment {
                        segment: segment.id,
                    });
                }
            }
            _ if segment.subducting_plate.is_some() => {
                return Err(TectonicValidationError::InvalidBoundarySegment {
                    segment: segment.id,
                });
            }
            _ => {}
        }

        let mut previous = None;
        let mut total_strength = 0.0_f32;
        for &edge in &segment.member_edges {
            if previous.is_some_and(|prior| edge <= prior) {
                return Err(TectonicValidationError::UnsortedBoundarySegmentEdges {
                    segment: segment.id,
                    previous: previous.expect("checked as some"),
                    found: edge,
                });
            }
            previous = Some(edge);
            let index = edge.raw() as usize;
            if index >= membership.len() {
                return Err(TectonicValidationError::InvalidBoundarySegmentEdge {
                    segment: segment.id,
                    edge,
                });
            }
            if membership[index].replace(segment.id).is_some() {
                return Err(TectonicValidationError::DuplicateBoundaryMembership { edge });
            }
            let boundary = &self.boundaries[index];
            if boundary.segment_id != Some(segment.id)
                || boundary.kind != segment.kind
                || boundary.subducting_plate != segment.subducting_plate
            {
                return Err(TectonicValidationError::BoundarySegmentMismatch { edge });
            }
            total_strength += boundary.strength;
        }
        let calculated = total_strength / segment.member_edges.len() as f32;
        if (calculated - segment.mean_strength).abs() > STRENGTH_TOLERANCE {
            return Err(TectonicValidationError::BoundarySegmentStrengthMismatch {
                segment: segment.id,
                stored: segment.mean_strength,
                calculated,
            });
        }
        Ok(())
    }

    fn validate_plate_connectivity(
        &self,
        plate: PlateId,
        spatial: &SpatialSnapshot,
    ) -> Result<(), TectonicValidationError> {
        let start = self
            .cell_plates
            .raw_values()
            .iter()
            .position(|&raw| raw == plate.raw())
            .expect("every plate owns its validated seed");
        let mut visited = vec![false; self.cell_count as usize];
        let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
        visited[start] = true;
        while let Some(cell) = queue.pop_front() {
            for &neighbor in spatial
                .neighbors(cell)
                .expect("spatial cardinality was validated")
            {
                let index = neighbor.raw() as usize;
                if !visited[index] && self.plate_for_cell(neighbor) == Some(plate) {
                    visited[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        let owned = self
            .cell_plates
            .raw_values()
            .iter()
            .filter(|&&raw| raw == plate.raw())
            .count();
        let reached = visited.into_iter().filter(|value| *value).count();
        if reached != owned {
            return Err(TectonicValidationError::DisconnectedPlate {
                plate,
                reached,
                owned,
            });
        }
        Ok(())
    }

    fn validate_edge_topology(&self, edge: &SpatialEdge) -> Result<(), TectonicValidationError> {
        let record = &self.boundaries[edge.id.raw() as usize];
        match edge.cells {
            [Some(_), None] | [None, Some(_)] => {
                if record.kind != BoundaryKind::None {
                    return Err(TectonicValidationError::BoundaryTopologyMismatch {
                        edge: edge.id,
                    });
                }
            }
            [Some(first), Some(second)] => {
                let first_plate = self
                    .plate_for_cell(first)
                    .expect("cell cardinality was validated");
                let second_plate = self
                    .plate_for_cell(second)
                    .expect("cell cardinality was validated");
                let crosses_plate = first_plate != second_plate;
                if crosses_plate == (record.kind == BoundaryKind::None) {
                    return Err(TectonicValidationError::BoundaryTopologyMismatch {
                        edge: edge.id,
                    });
                }
                if crosses_plate {
                    let segment = &self.boundary_segments
                        [record.segment_id.expect("validated boundary segment").raw() as usize];
                    if segment.plates != normalized_plate_pair(first_plate, second_plate) {
                        return Err(TectonicValidationError::BoundarySegmentPlatePairMismatch {
                            segment: segment.id,
                            edge: edge.id,
                        });
                    }
                }
            }
            [None, None] => {
                return Err(TectonicValidationError::BoundaryTopologyMismatch { edge: edge.id });
            }
        }
        Ok(())
    }

    fn validate_segment_topology(
        &self,
        segment: &BoundarySegment,
        spatial: &SpatialSnapshot,
    ) -> Result<(), TectonicValidationError> {
        if segment.member_edges.len() < 2 {
            return Ok(());
        }
        let member_edges: Vec<&SpatialEdge> = segment
            .member_edges
            .iter()
            .map(|edge| &spatial.edges()[edge.raw() as usize])
            .collect();
        let mut visited = vec![false; member_edges.len()];
        let mut queue = VecDeque::from([0_usize]);
        visited[0] = true;
        while let Some(index) = queue.pop_front() {
            for candidate in 0..member_edges.len() {
                if !visited[candidate]
                    && edges_share_endpoint(member_edges[index], member_edges[candidate])
                {
                    visited[candidate] = true;
                    queue.push_back(candidate);
                }
            }
        }
        if visited.iter().any(|reached| !reached) {
            return Err(TectonicValidationError::DisconnectedBoundarySegment {
                segment: segment.id,
            });
        }
        Ok(())
    }
}

fn validate_length(
    field: &'static str,
    found: usize,
    expected: u32,
) -> Result<(), TectonicValidationError> {
    if found != expected as usize {
        return Err(TectonicValidationError::FieldLengthMismatch {
            field,
            expected: expected as usize,
            found,
        });
    }
    Ok(())
}

fn validate_strength(edge: EdgeId, strength: f32) -> Result<(), TectonicValidationError> {
    if !strength.is_finite() || !(0.0..=1.0).contains(&strength) {
        return Err(TectonicValidationError::BoundaryStrengthOutOfRange {
            edge,
            found: strength,
        });
    }
    Ok(())
}

fn normalized_plate_pair(first: PlateId, second: PlateId) -> [PlateId; 2] {
    if first < second {
        [first, second]
    } else {
        [second, first]
    }
}

fn edges_share_endpoint(first: &SpatialEdge, second: &SpatialEdge) -> bool {
    [first.start, first.end]
        .into_iter()
        .any(|point| point_matches(point, second.start) || point_matches(point, second.end))
}

fn point_matches(first: WorldPoint, second: WorldPoint) -> bool {
    first == second
}

/// Errors returned when tectonic data violates V1 local or topology-aware invariants.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TectonicValidationError {
    /// The snapshot uses an unsupported schema version.
    #[error("unsupported tectonic snapshot schema {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The schema version found in the snapshot.
        found: u16,
        /// The schema version supported by this engine.
        supported: u16,
    },
    /// A velocity component lies outside the supported physical bound.
    #[error("plate velocity component {found} is outside {min}..={max} mm/year")]
    PlateVelocityOutOfRange {
        /// The component that failed validation.
        found: i16,
        /// The inclusive lower bound.
        min: i16,
        /// The inclusive upper bound.
        max: i16,
    },
    /// The plate table is empty or larger than the cell set.
    #[error("plate count {found} is invalid for {cell_count} cells")]
    InvalidPlateCount {
        /// The plate count that failed validation.
        found: usize,
        /// The number of cells in the snapshot.
        cell_count: u32,
    },
    /// A plate table entry is not at its stable contiguous identifier.
    #[error("expected plate {expected:?}, found {found:?}")]
    NonContiguousPlateId {
        /// The expected identifier.
        expected: PlateId,
        /// The stored identifier.
        found: PlateId,
    },
    /// A plate seed lies outside the dense cell range.
    #[error("plate {plate:?} has invalid seed {seed:?} for {cell_count} cells")]
    InvalidPlateSeed {
        /// The affected plate.
        plate: PlateId,
        /// The invalid seed.
        seed: CellId,
        /// The stored cell count.
        cell_count: u32,
    },
    /// A dense field length differs from its declared alignment count.
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        /// The stable field name.
        field: &'static str,
        /// The required length.
        expected: usize,
        /// The stored length.
        found: usize,
    },
    /// A cell references a plate that is not present.
    #[error("cell {cell:?} references invalid plate {plate:?}")]
    InvalidCellPlate {
        /// The affected cell.
        cell: CellId,
        /// The invalid plate identifier.
        plate: PlateId,
    },
    /// A raw crust category does not decode under V1.
    #[error("invalid crust category {found} at {cell:?}")]
    InvalidCrustKind {
        /// The affected cell when known.
        cell: Option<CellId>,
        /// The invalid raw category.
        found: u32,
    },
    /// A crust thickness is non-finite or invalid for its material class.
    #[error("cell {cell:?} {kind:?} crust thickness {found} is outside {min}..={max} km")]
    CrustThicknessOutOfRange {
        /// The affected cell.
        cell: CellId,
        /// The cell's crust class.
        kind: CrustKind,
        /// The invalid thickness.
        found: f32,
        /// The inclusive lower physical bound.
        min: f32,
        /// The inclusive upper physical bound.
        max: f32,
    },
    /// An edge-aligned strength is non-finite or outside the normalized range.
    #[error("edge {edge:?} has invalid boundary strength {found}")]
    BoundaryStrengthOutOfRange {
        /// The affected edge.
        edge: EdgeId,
        /// The invalid strength.
        found: f32,
    },
    /// A no-event record contains event-only data.
    #[error("edge {edge:?} has an inconsistent boundary record")]
    InvalidBoundaryRecord {
        /// The affected edge.
        edge: EdgeId,
    },
    /// A boundary segment table entry is not at its stable contiguous identifier.
    #[error("expected boundary segment {expected:?}, found {found:?}")]
    NonContiguousBoundarySegmentId {
        /// The expected identifier.
        expected: BoundarySegmentId,
        /// The stored identifier.
        found: BoundarySegmentId,
    },
    /// A boundary segment has an invalid plate pair, kind, strength, direction, or subduction side.
    #[error("boundary segment {segment:?} has invalid metadata")]
    InvalidBoundarySegment {
        /// The affected segment.
        segment: BoundarySegmentId,
    },
    /// A boundary segment contains no member edges.
    #[error("boundary segment {segment:?} has no member edges")]
    EmptyBoundarySegment {
        /// The affected segment.
        segment: BoundarySegmentId,
    },
    /// Boundary segment members are not strictly ordered.
    #[error(
        "boundary segment {segment:?} edge {found:?} does not follow edge {previous:?} strictly"
    )]
    UnsortedBoundarySegmentEdges {
        /// The affected segment.
        segment: BoundarySegmentId,
        /// The preceding edge.
        previous: EdgeId,
        /// The out-of-order or duplicate edge.
        found: EdgeId,
    },
    /// A segment references an edge outside the dense edge range.
    #[error("boundary segment {segment:?} references invalid edge {edge:?}")]
    InvalidBoundarySegmentEdge {
        /// The affected segment.
        segment: BoundarySegmentId,
        /// The invalid edge.
        edge: EdgeId,
    },
    /// An edge appears in more than one boundary segment.
    #[error("edge {edge:?} appears in more than one boundary segment")]
    DuplicateBoundaryMembership {
        /// The duplicated edge.
        edge: EdgeId,
    },
    /// An edge record and its declared segment disagree.
    #[error("edge {edge:?} does not agree with its boundary segment")]
    BoundarySegmentMismatch {
        /// The affected edge.
        edge: EdgeId,
    },
    /// A segment's stored mean does not equal its member-edge mean.
    #[error(
        "boundary segment {segment:?} mean strength {stored} does not match calculated {calculated}"
    )]
    BoundarySegmentStrengthMismatch {
        /// The affected segment.
        segment: BoundarySegmentId,
        /// The stored mean.
        stored: f32,
        /// The calculated mean.
        calculated: f32,
    },
    /// Spatial and tectonic cell cardinalities differ.
    #[error("tectonic cell count {tectonic} does not match spatial count {spatial}")]
    SpatialCellCountMismatch {
        /// The tectonic count.
        tectonic: u32,
        /// The spatial count.
        spatial: usize,
    },
    /// Spatial and tectonic edge cardinalities differ.
    #[error("tectonic edge count {tectonic} does not match spatial count {spatial}")]
    SpatialEdgeCountMismatch {
        /// The tectonic count.
        tectonic: u32,
        /// The spatial count.
        spatial: usize,
    },
    /// A plate does not own its declared seed.
    #[error("plate {plate:?} does not own seed {seed:?}; owner is {owner:?}")]
    PlateSeedOwnership {
        /// The affected plate.
        plate: PlateId,
        /// The declared seed.
        seed: CellId,
        /// The actual owner, if any.
        owner: Option<PlateId>,
    },
    /// A plate's cells do not form one connected region.
    #[error("plate {plate:?} reaches {reached} of its {owned} cells")]
    DisconnectedPlate {
        /// The affected plate.
        plate: PlateId,
        /// The number of connected cells reached.
        reached: usize,
        /// The number of owned cells.
        owned: usize,
    },
    /// An edge event disagrees with spatial ownership and cell plate assignments.
    #[error("edge {edge:?} boundary classification disagrees with spatial topology")]
    BoundaryTopologyMismatch {
        /// The affected edge.
        edge: EdgeId,
    },
    /// A segment plate pair disagrees with one of its member edges.
    #[error("boundary segment {segment:?} plate pair disagrees with edge {edge:?}")]
    BoundarySegmentPlatePairMismatch {
        /// The affected segment.
        segment: BoundarySegmentId,
        /// The affected edge.
        edge: EdgeId,
    },
    /// A segment's member edges do not form one connected chain or network.
    #[error("boundary segment {segment:?} is disconnected")]
    DisconnectedBoundarySegment {
        /// The affected segment.
        segment: BoundarySegmentId,
    },
}
