//! Immutable planar cells, edges, topology queries, and partition validation.

mod snapshot;
mod sphere_geometry;
mod spherical_snapshot;
mod spherical_validation;
mod topology;
mod validation;

pub use snapshot::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
pub use sphere_geometry::{
    central_angle, project_tangent, spherical_triangle_area_unit, SphereGeometryError, UnitVector3,
};
pub use spherical_snapshot::{
    SphericalSurfaceCell, SphericalSurfaceEdge, SphericalSurfaceSnapshot, SphericalSurfaceVertex,
    SPHERICAL_SURFACE_SCHEMA_V1,
};
pub use spherical_validation::SphericalSurfaceValidationError;
pub use topology::Topology;
pub use validation::SpatialValidationError;
