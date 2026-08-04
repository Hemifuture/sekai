//! Immutable planar and spherical cells, edges, topology queries, and validation.

mod snapshot;
mod sphere_geometry;
mod spherical_snapshot;
mod spherical_validation;
mod surface_ref;
mod topology;
mod validation;

pub use snapshot::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
pub(crate) use sphere_geometry::{
    add, central_angle_raw, cross, dot, normalize_legacy_compatible, oriented_arc_normal,
    project_tangent_raw, scale, spherical_triangle_area_unit_raw, subtract,
};
pub use sphere_geometry::{
    central_angle, project_tangent, spherical_triangle_area_unit, SphereGeometryError, UnitVector3,
};
pub use spherical_snapshot::{
    SphericalSurfaceCell, SphericalSurfaceEdge, SphericalSurfaceSnapshot, SphericalSurfaceVertex,
    SPHERICAL_SURFACE_SCHEMA_V1,
};
pub(crate) use spherical_validation::spherical_polygon_metrics;
pub use spherical_validation::SphericalSurfaceValidationError;
pub use surface_ref::{SurfaceGeometryKind, SurfaceRef, SurfaceRefError};
pub use topology::Topology;
pub use validation::SpatialValidationError;
