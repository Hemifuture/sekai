//! Deterministic planar and spherical site and topology generation.

mod conservative_remap;
mod geodesic_voronoi;
mod jittered_grid;
mod planar_voronoi;
mod profile_surface;
mod remap_fields;
mod spherical_stage;
mod stage;

pub use conservative_remap::{ConservativeRemapError, ConservativeSurfaceMapBuilder};
pub use geodesic_voronoi::{GeodesicVoronoiBuilder, SphericalSurfaceBuildError};
pub use jittered_grid::JitteredGridSites;
pub use planar_voronoi::{PlanarVoronoiBuilder, SpatialBuildError};
pub use profile_surface::{ProfileSurfaceBuildError, ProfileSurfaceBuilder, ProfileSurfaceBundle};
pub use remap_fields::{
    remap_categories_u16, remap_extensive_f64, remap_extensive_f64_cancellable,
    remap_intensive_f32, remap_intensive_f32_cancellable, remap_intensive_f64,
    remap_intensive_f64_cancellable, remap_tangent_components_f64,
    remap_tangent_components_f64_cancellable, CategoricalRemap, ExtensiveRemap,
};
pub use spherical_stage::{
    spherical_foundation_graph, SphericalSpaceArtifact, SphericalSurfaceArtifact,
    SphericalSurfaceStage, SphericalSurfaceStageInputs,
};
pub use stage::{
    foundation_graph, PlanarSpaceArtifact, SpatialArtifact, SpatialStage, SpatialStageInputs,
};
