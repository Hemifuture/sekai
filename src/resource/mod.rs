use crate::{
    gpu::{
        delaunay::delaunay_renderer::DelaunayRenderer, map_renderer::MapRenderer,
        points_renderer::PointsRenderer,
    },
    models::map::grid::Grid,
    ui::canvas::state::CanvasState,
};

mod resource_impl;

// pub type GraphResource = resource_impl::Resource<Graph>;
// pub type CanvasStateResource = resource_impl::Resource<CanvasState>;
// pub type ParticleSystemResource = resource_impl::Resource<ParticleSystem>;
pub type MapRendererResource = resource_impl::Resource<MapRenderer>;
pub type CanvasStateResource = resource_impl::Resource<CanvasState>;
pub type PointsRendererResource = resource_impl::Resource<PointsRenderer>;
pub type DelaunayRendererResource = resource_impl::Resource<DelaunayRenderer>;
pub type GridResource = resource_impl::Resource<Grid>;
