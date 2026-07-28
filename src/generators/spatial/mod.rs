//! Deterministic planar site and topology generation.

mod jittered_grid;
mod planar_voronoi;
mod stage;

pub use jittered_grid::JitteredGridSites;
pub use planar_voronoi::{PlanarVoronoiBuilder, SpatialBuildError};
pub use stage::{
    foundation_graph, PlanarSpaceArtifact, SpatialArtifact, SpatialStage, SpatialStageInputs,
};
