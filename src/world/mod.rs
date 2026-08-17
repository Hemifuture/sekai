pub mod fields;
pub mod natural;
pub mod spatial;

mod ids;
mod serde_bounded;
mod spec;
mod units;

pub use ids::{
    AuthorObjectId, BoundarySegmentId, CellId, CultureId, DrainageBasinId, EdgeId, HotspotId,
    LakeId, PlateId, PolityId, RiverSegmentId, RootSeed, SettlementId, SpeciesId, SurfaceVertexId,
};
pub use spec::{
    BoundaryCondition, PlanarSpaceSpec, SpecError, SphericalSpaceSpec, SphericalSpecError,
    TechnologyBaseline, WorldSpec, MAX_CELL_COUNT, MAX_DIMENSION_METERS, MAX_GEODESIC_FREQUENCY,
    MAX_SPHERICAL_CELL_BOUNDARY_DEGREE, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT,
    MAX_SPHERICAL_TARGET_CELL_COUNT, MAX_SPHERICAL_VERTEX_COUNT, MIN_CELL_COUNT,
    MIN_DIMENSION_METERS, MIN_GEODESIC_FREQUENCY, MIN_SPHERICAL_CELL_COUNT, WORLD_SPEC_SCHEMA_V1,
};
pub use units::{Meters, SquareMeters, UnitError, WorldPoint, WorldRect};
