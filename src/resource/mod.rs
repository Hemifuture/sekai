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
#[allow(dead_code)]
pub type MapSystemResource = resource_impl::Resource<MapSystem>;
pub type CanvasStateResource = resource_impl::Resource<CanvasState>;
#[allow(dead_code)]
pub type PointsRendererResource = resource_impl::Resource<PointsRenderer>;
#[allow(dead_code)]
pub type DelaunayRendererResource = resource_impl::Resource<DelaunayRenderer>;
#[allow(dead_code)]
pub type VoronoiRendererResource = resource_impl::Resource<VoronoiRenderer>;
pub type FieldRendererResource = resource_impl::Resource<CellFieldRenderer>;
pub type FieldDisplayResource = resource_impl::Resource<FieldDisplayResourceState>;
pub type FieldViewerStateResource = resource_impl::Resource<FieldDisplayState>;
