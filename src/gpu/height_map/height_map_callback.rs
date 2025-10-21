use eframe::{
    egui_wgpu::{CallbackResources, CallbackTrait},
    wgpu::RenderPass,
};
use egui::{PaintCallbackInfo, Rect};

use crate::gpu::map_renderer::MapRenderer;
use crate::resource::HeightMapRendererResource;

pub struct HeightMapCallback {
    canvas_rect: Rect,
}

impl HeightMapCallback {
    pub fn new(rect: Rect) -> Self {
        Self { canvas_rect: rect }
    }
}

impl CallbackTrait for HeightMapCallback {
    fn prepare(
        &self,
        _device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        _screen_descriptor: &eframe::egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut eframe::wgpu::CommandEncoder,
        resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        let height_map_renderer_resource = resources.get::<HeightMapRendererResource>().unwrap();
        height_map_renderer_resource.with_resource(|renderer| {
            renderer.prepare(queue);
        });

        vec![]
    }

    fn paint(
        &self,
        _info: PaintCallbackInfo,
        render_pass: &mut RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let height_map_renderer_resource = resources.get::<HeightMapRendererResource>().unwrap();
        height_map_renderer_resource.read_resource(|renderer| {
            renderer.paint(render_pass);
        });
    }
}
