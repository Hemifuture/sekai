use eframe::{
    egui_wgpu::{CallbackResources, CallbackTrait},
    wgpu::RenderPass,
};
use egui::{PaintCallbackInfo, Rect};

use crate::resource::{CanvasStateResource, PointsRendererResource};

pub struct PointsCallback {
    canvas_state_resource: CanvasStateResource,
    canvas_rect: Rect,
}

impl PointsCallback {
    pub fn new(canvas_state_resource: CanvasStateResource, rect: Rect) -> Self {
        Self {
            canvas_state_resource,
            canvas_rect: rect,
        }
    }
}

impl CallbackTrait for PointsCallback {
    fn prepare(
        &self,
        _device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        _screen_descriptor: &eframe::egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut eframe::wgpu::CommandEncoder,
        resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        let points_renderer_resource = resources.get::<PointsRendererResource>().unwrap();
        points_renderer_resource.with_resource(|points_renderer| {
            // println!("points count: {}", points_renderer.points.len());
            self.canvas_state_resource.read_resource(|canvas_state| {
                points_renderer.update_uniforms(self.canvas_rect, canvas_state.transform);
                points_renderer.upload_to_gpu(queue);
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
        let points_renderer_resource = resources.get::<PointsRendererResource>().unwrap();
        points_renderer_resource.read_resource(|points_renderer| {
            points_renderer.render(render_pass);
        });
    }
}
