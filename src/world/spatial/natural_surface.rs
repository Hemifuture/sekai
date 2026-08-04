use std::f64::consts::PI;

use thiserror::Error;

use super::{
    SpatialSnapshot, SpatialValidationError, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRef, SurfaceRefError, Topology,
};
use crate::world::{CellId, EdgeId, Meters, SquareMeters};

/// Copyable natural-process metrics for one authoritative surface cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCellMetrics {
    id: CellId,
    area: SquareMeters,
    shape_position: [f64; 3],
}

impl SurfaceCellMetrics {
    /// Returns the authoritative dense cell identifier.
    pub const fn id(self) -> CellId {
        self.id
    }

    /// Returns the true cell area in square meters.
    pub const fn area(self) -> SquareMeters {
        self.area
    }

    /// Returns a deterministic unitless embedding used only for spatial ranking and noise.
    pub const fn shape_position(self) -> [f64; 3] {
        self.shape_position
    }
}

/// Copyable natural-process metrics for one authoritative surface edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceEdgeMetrics {
    id: EdgeId,
    owners: [Option<CellId>; 2],
    boundary_length: Meters,
    traversal_length: Meters,
    center_distance: Option<Meters>,
}

impl SurfaceEdgeMetrics {
    /// Returns the authoritative dense edge identifier.
    pub const fn id(self) -> EdgeId {
        self.id
    }

    /// Returns the stored cell owners; closed surfaces always have two.
    pub const fn owners(self) -> [Option<CellId>; 2] {
        self.owners
    }

    /// Returns the physical length of the shared polygon side.
    pub const fn boundary_length(self) -> Meters {
        self.boundary_length
    }

    /// Returns the versioned distance used by the current natural graph algorithms.
    pub const fn traversal_length(self) -> Meters {
        self.traversal_length
    }

    /// Returns the physical distance between cell sites when the edge has two owners.
    pub const fn center_distance(self) -> Option<Meters> {
        self.center_distance
    }
}

/// Minimal read-only geometry and metric facts consumed by natural processes.
///
/// Implementations borrow one authoritative snapshot. This contract deliberately
/// excludes projections, rendering geometry, climate work-grid coordinates, and
/// mutable or serialized derived topology.
pub trait NaturalSurface {
    /// Returns the exact identity of the borrowed authoritative surface.
    fn surface_ref(&self) -> SurfaceRef;

    /// Returns whether the surface is a closed two-manifold without boundary edges.
    fn is_closed(&self) -> bool;

    /// Returns the exact dense cell count.
    fn cell_count(&self) -> usize;

    /// Returns the exact dense edge count.
    fn edge_count(&self) -> usize;

    /// Returns the compensated authoritative cell-area sum.
    fn total_area(&self) -> SquareMeters;

    /// Looks up one dense cell's natural-process metrics.
    fn cell(&self, id: CellId) -> Option<SurfaceCellMetrics>;

    /// Looks up one dense edge's natural-process metrics.
    fn edge(&self, id: EdgeId) -> Option<SurfaceEdgeMetrics>;

    /// Returns the short physical scale used by versioned regional-distance rules.
    fn short_length_scale(&self) -> Meters;

    /// Returns the long physical scale used to normalize graph traversal costs.
    fn long_length_scale(&self) -> Meters;
}

/// A borrowed metric view of a validated legacy planar surface.
#[derive(Debug, Clone, Copy)]
pub struct PlanarNaturalSurface<'a> {
    snapshot: &'a SpatialSnapshot,
    surface_ref: Option<SurfaceRef>,
}

impl<'a> PlanarNaturalSurface<'a> {
    /// Validates and borrows a planar snapshot for natural-process queries.
    pub fn new(snapshot: &'a SpatialSnapshot) -> Result<Self, NaturalSurfaceError> {
        snapshot.validate()?;
        Ok(Self {
            snapshot,
            surface_ref: Some(SurfaceRef::try_for_planar(snapshot)?),
        })
    }

    pub(crate) const fn from_validated(snapshot: &'a SpatialSnapshot) -> Self {
        Self {
            snapshot,
            surface_ref: None,
        }
    }
}

impl NaturalSurface for PlanarNaturalSurface<'_> {
    fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
            .unwrap_or_else(|| SurfaceRef::for_planar(self.snapshot))
    }

    fn is_closed(&self) -> bool {
        false
    }

    fn cell_count(&self) -> usize {
        self.snapshot.cell_count()
    }

    fn edge_count(&self) -> usize {
        self.snapshot.edges().len()
    }

    fn total_area(&self) -> SquareMeters {
        let bounds = self.snapshot.bounds();
        SquareMeters::new(bounds.width().get() * bounds.height().get())
            .expect("validated planar bounds have a finite area")
    }

    fn cell(&self, id: CellId) -> Option<SurfaceCellMetrics> {
        let cell = self.snapshot.cell(id)?;
        let bounds = self.snapshot.bounds();
        let scale = bounds.width().get().max(bounds.height().get());
        Some(SurfaceCellMetrics {
            id: cell.id,
            area: cell.area,
            shape_position: [
                (cell.centroid.x().get() - bounds.min().x().get()) / scale,
                (cell.centroid.y().get() - bounds.min().y().get()) / scale,
                0.0,
            ],
        })
    }

    fn edge(&self, id: EdgeId) -> Option<SurfaceEdgeMetrics> {
        let edge = self
            .snapshot
            .edges()
            .get(id.raw() as usize)
            .filter(|edge| edge.id == id)?;
        let center_distance = match edge.cells {
            [Some(first), Some(second)] => self.snapshot.distance_between_sites(first, second),
            _ => None,
        };
        Some(SurfaceEdgeMetrics {
            id: edge.id,
            owners: edge.cells,
            boundary_length: edge.length,
            traversal_length: edge.length,
            center_distance,
        })
    }

    fn short_length_scale(&self) -> Meters {
        let bounds = self.snapshot.bounds();
        Meters::new(bounds.width().get().min(bounds.height().get()))
            .expect("validated planar bounds have a finite short scale")
    }

    fn long_length_scale(&self) -> Meters {
        let bounds = self.snapshot.bounds();
        Meters::new(bounds.width().get().max(bounds.height().get()))
            .expect("validated planar bounds have a finite long scale")
    }
}

/// A borrowed metric view of a validated authoritative spherical surface.
#[derive(Debug, Clone, Copy)]
pub struct SphericalNaturalSurface<'a> {
    snapshot: &'a SphericalSurfaceSnapshot,
    surface_ref: SurfaceRef,
}

impl<'a> SphericalNaturalSurface<'a> {
    /// Validates and borrows a spherical snapshot for natural-process queries.
    pub fn new(snapshot: &'a SphericalSurfaceSnapshot) -> Result<Self, NaturalSurfaceError> {
        snapshot.validate()?;
        Ok(Self {
            snapshot,
            surface_ref: SurfaceRef::try_for_spherical(snapshot)?,
        })
    }
}

impl NaturalSurface for SphericalNaturalSurface<'_> {
    fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    fn is_closed(&self) -> bool {
        true
    }

    fn cell_count(&self) -> usize {
        self.snapshot.cells().len()
    }

    fn edge_count(&self) -> usize {
        self.snapshot.edges().len()
    }

    fn total_area(&self) -> SquareMeters {
        self.snapshot.total_cell_area()
    }

    fn cell(&self, id: CellId) -> Option<SurfaceCellMetrics> {
        let cell = self.snapshot.cell(id)?;
        Some(SurfaceCellMetrics {
            id: cell.id,
            area: cell.area,
            shape_position: cell
                .centroid
                .components()
                .map(|component| (component + 1.0) * 0.5),
        })
    }

    fn edge(&self, id: EdgeId) -> Option<SurfaceEdgeMetrics> {
        let edge = self.snapshot.edge(id)?;
        Some(SurfaceEdgeMetrics {
            id: edge.id,
            owners: edge.cells.map(Some),
            boundary_length: edge.length,
            traversal_length: edge.center_distance,
            center_distance: Some(edge.center_distance),
        })
    }

    fn short_length_scale(&self) -> Meters {
        sphere_maximum_distance(self.snapshot.radius())
    }

    fn long_length_scale(&self) -> Meters {
        sphere_maximum_distance(self.snapshot.radius())
    }
}

fn sphere_maximum_distance(radius: Meters) -> Meters {
    Meters::new(PI * radius.get())
        .expect("validated spherical radius has a finite half-circumference")
}

/// Failures while establishing a trusted borrowed natural-surface view.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum NaturalSurfaceError {
    /// The borrowed planar snapshot failed its authoritative validation.
    #[error("invalid planar natural surface: {0}")]
    InvalidPlanar(#[from] SpatialValidationError),
    /// The borrowed spherical snapshot failed its authoritative validation.
    #[error("invalid spherical natural surface: {0}")]
    InvalidSpherical(#[from] SphericalSurfaceValidationError),
    /// The validated snapshot could not produce a complete stable identity.
    #[error("invalid natural surface identity: {0}")]
    InvalidIdentity(#[from] SurfaceRefError),
}
