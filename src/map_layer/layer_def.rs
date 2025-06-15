use std::any::Any;

use eframe::wgpu;
use egui::Rect;

use crate::ui::canvas::state::CanvasState;

pub trait MapLayer: Any + Send + Sync {
    // 层的唯一标识符
    fn id(&self) -> &'static str;

    // 层是否可见
    fn is_visible(&self) -> bool;

    // 层的透明度
    fn opacity(&self) -> f32 {
        1.0
    }

    // 层的渲染顺序（数值越小越先渲染）
    fn z_order(&self) -> i32;

    // 创建或更新 GPU 资源
    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, canvas_state: &CanvasState);

    // 渲染层
    fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>);

    // // 处理交互事件（返回是否消费了事件）
    // fn handle_event(&mut self, event: &LayerEvent) -> bool {
    //     false
    // }

    // 获取层的配置界面（可选）
    fn view_controls(&self, ui: &mut egui::Ui) -> egui::Response {
        ui.label("No controls available")
    }
}
