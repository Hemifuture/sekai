use eframe::{
    egui_wgpu::{CallbackResources, CallbackTrait},
    wgpu::RenderPass,
};
use egui::{accesskit::Point, PaintCallbackInfo};

pub struct PointsCallback {
    points: Vec<Point>,
}

impl PointsCallback {
    pub fn new(points: Vec<Point>) -> Self {
        Self { points }
    }
}

impl CallbackTrait for PointsCallback {
    fn prepare(
        &self,
        _device: &eframe::wgpu::Device,
        _queue: &eframe::wgpu::Queue,
        _screen_descriptor: &eframe::egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut eframe::wgpu::CommandEncoder,
        _callback_resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        vec![]
    }

    fn paint(
        &self,
        _info: PaintCallbackInfo,
        render_pass: &mut RenderPass<'static>,
        resources: &CallbackResources,
    ) {
    }
}
