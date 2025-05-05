use crate::{
    gpu::{
        delaunay::delaunay_renderer::DelaunayRenderer, map_renderer::MapRenderer,
        points_renderer::PointsRenderer, voronoi::voronoi_renderer::VoronoiRenderer,
    },
    models::map::{data::MapData, grid::Grid},
    ui::canvas::state::CanvasState,
};

mod resource_impl;

// pub type GraphResource = resource_impl::Resource<Graph>;
// pub type CanvasStateResource = resource_impl::Resource<CanvasState>;
// pub type ParticleSystemResource = resource_impl::Resource<ParticleSystem>;
pub type MapDataResource = resource_impl::Resource<MapData>;
pub type CanvasStateResource = resource_impl::Resource<CanvasState>;
pub type PointsRendererResource = resource_impl::Resource<PointsRenderer>;
pub type DelaunayRendererResource = resource_impl::Resource<DelaunayRenderer>;
pub type VoronoiRendererResource = resource_impl::Resource<VoronoiRenderer>;
pub type GridResource = resource_impl::Resource<Grid>;
