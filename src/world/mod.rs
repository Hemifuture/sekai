pub mod fields;
pub mod natural;
pub mod spatial;

mod ids;
mod spec;
mod units;

pub use ids::{
    AuthorObjectId, BoundarySegmentId, CellId, CultureId, EdgeId, HotspotId, PlateId, PolityId,
    RootSeed, SettlementId, SpeciesId,
};
pub use spec::{
    BoundaryCondition, PlanarSpaceSpec, SpecError, TechnologyBaseline, WorldSpec, MAX_CELL_COUNT,
    MAX_DIMENSION_METERS, MIN_CELL_COUNT, MIN_DIMENSION_METERS, WORLD_SPEC_SCHEMA_V1,
};
pub use units::{Meters, SquareMeters, UnitError, WorldPoint, WorldRect};
