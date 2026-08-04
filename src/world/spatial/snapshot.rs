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

    /// Returns the compensated sum of the stored cell areas.
    ///
    /// Call [`Self::validate`] first when this snapshot came from deserialization.
    ///
    /// # Panics
    ///
    /// Panics only when an unvalidated deserialized snapshot contains individually
    /// finite areas whose sum is non-finite. Snapshots returned by [`Self::new`],
    /// or deserialized snapshots that pass [`Self::validate`], cannot panic here.
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

    /// Returns a deterministic semantic fingerprint without changing the V1 wire format.
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.planar-surface.v1\0");
        hash_u16(&mut hasher, self.schema_version);
        hash_point(&mut hasher, self.bounds.min());
        hash_point(&mut hasher, self.bounds.max());
        hash_u8(
            &mut hasher,
            match self.boundary {
                BoundaryCondition::Closed => 0,
            },
        );

        hash_len(&mut hasher, self.cells.len());
        for cell in &self.cells {
            hash_u32(&mut hasher, cell.id.raw());
            hash_point(&mut hasher, cell.site);
            hash_point(&mut hasher, cell.centroid);
            hash_f64(&mut hasher, cell.area.get());
            hash_len(&mut hasher, cell.polygon.len());
            for &vertex in &cell.polygon {
                hash_point(&mut hasher, vertex);
            }
            hash_len(&mut hasher, cell.neighbors.len());
            for &neighbor in &cell.neighbors {
                hash_u32(&mut hasher, neighbor.raw());
            }
        }

        hash_len(&mut hasher, self.edges.len());
        for edge in &self.edges {
            hash_u32(&mut hasher, edge.id.raw());
            hash_point(&mut hasher, edge.start);
            hash_point(&mut hasher, edge.end);
            hash_f64(&mut hasher, edge.length.get());
            for owner in edge.cells {
                match owner {
                    Some(cell) => {
                        hash_u8(&mut hasher, 1);
                        hash_u32(&mut hasher, cell.raw());
                    }
                    None => hash_u8(&mut hasher, 0),
                }
            }
        }

        *hasher.finalize().as_bytes()
    }
}

fn hash_point(hasher: &mut blake3::Hasher, point: WorldPoint) {
    hash_f64(hasher, point.x().get());
    hash_f64(hasher, point.y().get());
}

fn hash_len(hasher: &mut blake3::Hasher, value: usize) {
    let value = u64::try_from(value).expect("supported planar allocations fit in u64");
    hasher.update(&value.to_le_bytes());
}

fn hash_f64(hasher: &mut blake3::Hasher, value: f64) {
    hasher.update(&value.to_bits().to_le_bytes());
}

fn hash_u32(hasher: &mut blake3::Hasher, value: u32) {
    hasher.update(&value.to_le_bytes());
}

fn hash_u16(hasher: &mut blake3::Hasher, value: u16) {
    hasher.update(&value.to_le_bytes());
}

fn hash_u8(hasher: &mut blake3::Hasher, value: u8) {
    hasher.update(&[value]);
}
