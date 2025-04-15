use crate::gpu::map_renderer::MapRenderer;

mod resource_impl;

// pub type GraphResource = resource_impl::Resource<Graph>;
// pub type CanvasStateResource = resource_impl::Resource<CanvasState>;
// pub type ParticleSystemResource = resource_impl::Resource<ParticleSystem>;
pub type MapRendererResource = resource_impl::Resource<MapRenderer>;
