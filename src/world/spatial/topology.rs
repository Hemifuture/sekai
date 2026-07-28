use super::{SpatialCell, SpatialEdge, SpatialSnapshot};
use crate::world::{CellId, Meters, WorldRect};

/// Read-only access to validated planar cell topology.
pub trait Topology {
    /// Returns the rectangular world-space extent.
    fn bounds(&self) -> WorldRect;

    /// Returns the number of spatial cells.
    fn cell_count(&self) -> usize;

    /// Returns a cell by its stable identifier.
    fn cell(&self, id: CellId) -> Option<&SpatialCell>;

    /// Returns the sorted neighbors of a cell by stable identifier.
    fn neighbors(&self, id: CellId) -> Option<&[CellId]>;

    /// Returns all edges in stable-identifier order.
    fn edges(&self) -> &[SpatialEdge];

    /// Returns the Euclidean distance between two cell sites.
    fn distance_between_sites(&self, a: CellId, b: CellId) -> Option<Meters>;
}

impl Topology for SpatialSnapshot {
    fn bounds(&self) -> WorldRect {
        self.bounds
    }

    fn cell_count(&self) -> usize {
        self.cells.len()
    }

    fn cell(&self, id: CellId) -> Option<&SpatialCell> {
        self.cells
            .get(id.raw() as usize)
            .filter(|cell| cell.id == id)
    }

    fn neighbors(&self, id: CellId) -> Option<&[CellId]> {
        self.cell(id).map(|cell| cell.neighbors.as_slice())
    }

    fn edges(&self) -> &[SpatialEdge] {
        &self.edges
    }

    fn distance_between_sites(&self, a: CellId, b: CellId) -> Option<Meters> {
        let a = self.cell(a)?.site;
        let b = self.cell(b)?.site;
        let dx = b.x().get() - a.x().get();
        let dy = b.y().get() - a.y().get();
        Meters::new(dx.hypot(dy)).ok()
    }
}
