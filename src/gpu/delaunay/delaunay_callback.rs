use eframe::{
    egui_wgpu::{CallbackResources, CallbackTrait},
    wgpu::RenderPass,
};
use egui::{accesskit::Point, PaintCallbackInfo, Rect};

use crate::resource::{CanvasStateResource, DelaunayRendererResource};

pub struct DelaunayCallback {
    canvas_state_resource: CanvasStateResource,
    canvas_rect: Rect,
}

impl DelaunayCallback {
    pub fn new(canvas_state_resource: CanvasStateResource, rect: Rect) -> Self {
        Self {
            canvas_state_resource,
            canvas_rect: rect,
        }
    }
}

impl CallbackTrait for DelaunayCallback {
    fn prepare(
        &self,
        _device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        _screen_descriptor: &eframe::egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut eframe::wgpu::CommandEncoder,
        resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        let delaunay_renderer_resource = resources.get::<DelaunayRendererResource>().unwrap();
        delaunay_renderer_resource.with_resource(|delaunay_renderer| {
            // println!("points count: {}", points_renderer.points.len());
            self.canvas_state_resource.read_resource(|canvas_state| {
                delaunay_renderer.update_uniforms(self.canvas_rect, canvas_state.transform);
                delaunay_renderer.upload_to_gpu(queue);
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
        let delaunay_renderer_resource = resources.get::<DelaunayRendererResource>().unwrap();
        delaunay_renderer_resource.read_resource(|delaunay_renderer| {
            delaunay_renderer.render(render_pass);
        });
    }
}
