//! Immutable planar cells, edges, topology queries, and partition validation.

mod snapshot;
mod topology;
mod validation;

pub use snapshot::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
pub use topology::Topology;
pub use validation::SpatialValidationError;
