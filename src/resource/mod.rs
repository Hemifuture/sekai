use crate::{
    gpu::{
        delaunay::delaunay_renderer::DelaunayRenderer, field::CellFieldRenderer,
        points_renderer::PointsRenderer, voronoi::voronoi_renderer::VoronoiRenderer,
    },
    models::map::system::MapSystem,
    ui::canvas::state::CanvasState,
    view::{FieldDisplayResourceState, FieldDisplayState},
};

mod resource_impl;

// pub type GraphResource = resource_impl::Resource<Graph>;
// pub type CanvasStateResource = resource_impl::Resource<CanvasState>;
// pub type ParticleSystemResource = resource_impl::Resource<ParticleSystem>;
pub type MapSystemResource = resource_impl::Resource<MapSystem>;
pub type CanvasStateResource = resource_impl::Resource<CanvasState>;
pub type PointsRendererResource = resource_impl::Resource<PointsRenderer>;
pub type DelaunayRendererResource = resource_impl::Resource<DelaunayRenderer>;
pub type VoronoiRendererResource = resource_impl::Resource<VoronoiRenderer>;
pub type FieldRendererResource = resource_impl::Resource<CellFieldRenderer>;
pub type FieldDisplayResource = resource_impl::Resource<FieldDisplayResourceState>;
pub type FieldViewerStateResource = resource_impl::Resource<FieldDisplayState>;
