use eframe::{
    egui_wgpu::{CallbackResources, CallbackTrait},
    wgpu::RenderPass,
};
use egui::{PaintCallbackInfo, Rect};

use crate::resource::{CanvasStateResource, VoronoiRendererResource};

pub struct VoronoiCallback {
    canvas_state_resource: CanvasStateResource,
    canvas_rect: Rect,
}

impl VoronoiCallback {
    pub fn new(canvas_state_resource: CanvasStateResource, rect: Rect) -> Self {
        Self {
            canvas_state_resource,
            canvas_rect: rect,
        }
    }
}

impl CallbackTrait for VoronoiCallback {
    fn prepare(
        &self,
        _device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        _screen_descriptor: &eframe::egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut eframe::wgpu::CommandEncoder,
        resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        let voronoi_renderer_resource = resources.get::<VoronoiRendererResource>().unwrap();
        voronoi_renderer_resource.with_resource(|voronoi_renderer| {
            self.canvas_state_resource.read_resource(|canvas_state| {
                voronoi_renderer.update_uniforms(self.canvas_rect, canvas_state.transform);
                voronoi_renderer.upload_to_gpu(queue);
            })
        });

        vec![]
    }

    fn paint(
        &self,
        _info: PaintCallbackInfo,
        render_pass: &mut RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let voronoi_renderer_resource = resources.get::<VoronoiRendererResource>().unwrap();
        voronoi_renderer_resource.read_resource(|voronoi_renderer| {
            voronoi_renderer.render(render_pass);
        });
    }
}
