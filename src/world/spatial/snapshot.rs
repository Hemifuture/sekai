use serde::{Deserialize, Serialize};

use super::SpatialValidationError;
use crate::world::{
    BoundaryCondition, CellId, EdgeId, Meters, SquareMeters, WorldPoint, WorldRect,
};

/// The supported version of the serialized spatial snapshot schema.
pub const SPATIAL_SCHEMA_V1: u16 = 1;

/// A validated polygonal cell in planar world space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialCell {
    /// The contiguous stable identifier of this cell.
    pub id: CellId,
    /// The generating site associated with this cell.
    pub site: WorldPoint,
    /// The centroid calculated from this cell's polygon.
    pub centroid: WorldPoint,
    /// The area calculated from this cell's polygon.
    pub area: SquareMeters,
    /// The counter-clockwise boundary vertices of this cell.
    pub polygon: Vec<WorldPoint>,
    /// The sorted stable identifiers of adjacent cells.
    pub neighbors: Vec<CellId>,
}

/// A validated polygon segment shared by one or two spatial cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialEdge {
    /// The contiguous stable identifier of this edge.
    pub id: EdgeId,
    /// The first world-space endpoint of this edge.
    pub start: WorldPoint,
    /// The second world-space endpoint of this edge.
    pub end: WorldPoint,
    /// The distance between this edge's endpoints.
    pub length: Meters,
    /// The owning cell for a boundary edge or two owners for an internal edge.
    pub cells: [Option<CellId>; 2],
}

/// An immutable, versioned, validated snapshot of planar spatial topology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialSnapshot {
    /// The schema version used to interpret this snapshot.
    pub schema_version: u16,
    /// The rectangular extent covered by the cells.
    pub bounds: WorldRect,
    /// The condition applied at the outer edge of the snapshot.
    pub boundary: BoundaryCondition,
    pub(super) cells: Vec<SpatialCell>,
    pub(super) edges: Vec<SpatialEdge>,
}

impl SpatialSnapshot {
    /// Sorts records by stable ID and constructs a snapshot only when every invariant holds.
    pub fn new(
        schema_version: u16,
        bounds: WorldRect,
        boundary: BoundaryCondition,
        mut cells: Vec<SpatialCell>,
        mut edges: Vec<SpatialEdge>,
    ) -> Result<Self, SpatialValidationError> {
        cells.sort_by_key(|cell| cell.id);
        edges.sort_by_key(|edge| edge.id);

        let snapshot = Self {
            schema_version,
            bounds,
            boundary,
            cells,
            edges,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Returns the compensated sum of the stored, validated cell areas.
    pub fn total_cell_area(&self) -> SquareMeters {
        let mut sum = 0.0;
        let mut compensation = 0.0;
        for cell in &self.cells {
            let adjusted = cell.area.get() - compensation;
            let next = sum + adjusted;
            compensation = (next - sum) - adjusted;
            sum = next;
        }
        SquareMeters::new(sum).expect("validated spatial cell areas have a finite sum")
    }
}
