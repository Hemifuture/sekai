//! Deterministic planar and spherical site and topology generation.

mod geodesic_voronoi;
mod jittered_grid;
mod planar_voronoi;
mod spherical_stage;
mod stage;

pub use geodesic_voronoi::{GeodesicVoronoiBuilder, SphericalSurfaceBuildError};
pub use jittered_grid::JitteredGridSites;
pub use planar_voronoi::{PlanarVoronoiBuilder, SpatialBuildError};
pub use spherical_stage::{
    spherical_foundation_graph, SphericalSpaceArtifact, SphericalSurfaceArtifact,
    SphericalSurfaceStage, SphericalSurfaceStageInputs,
};
pub use stage::{
    foundation_graph, PlanarSpaceArtifact, SpatialArtifact, SpatialStage, SpatialStageInputs,
};
