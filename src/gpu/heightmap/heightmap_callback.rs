// 高度图渲染回调

use eframe::{
    egui::{self, Color32, Rect},
    egui_wgpu,
};
use eframe::egui_wgpu::wgpu;

use crate::resource::{CanvasStateResource, MapSystemResource};

use super::heightmap_renderer::height_to_color;

pub struct HeightmapCallback {
    canvas_state_resource: CanvasStateResource,
    map_system_resource: MapSystemResource,
    canvas_rect: Rect,
}

impl HeightmapCallback {
    pub fn new(
        canvas_state_resource: CanvasStateResource,
        map_system_resource: MapSystemResource,
        canvas_rect: Rect,
    ) -> Self {
        Self {
            canvas_state_resource,
            map_system_resource,
            canvas_rect,
        }
    }
}

impl egui_wgpu::CallbackTrait for HeightmapCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        // 从 CallbackResources 获取 HeightmapRendererResource
        let heightmap_renderer_resource = resources
            .get::<super::HeightmapRendererResource>()
            .unwrap();

        heightmap_renderer_resource.with_resource(|heightmap_renderer| {
            self.canvas_state_resource.read_resource(|canvas_state| {
                self.map_system_resource.read_resource(|map_system| {
                    // 生成颜色
                    let cell_colors: Vec<Color32> = map_system
                        .cells_data
                        .height
                        .iter()
                        .map(|&h| height_to_color(h))
                        .collect();

                    // 更新渲染器数据
                    heightmap_renderer.update_data(
                        &map_system.voronoi.vertices,
                        &map_system.voronoi.cells,
                        &cell_colors,
                    );

                    // 构建 uniforms
                    let uniforms = crate::gpu::canvas_uniform::CanvasUniforms::new(
                        self.canvas_rect,
                        canvas_state.transform,
                    );

                    // 上传数据到 GPU
                    heightmap_renderer.upload_to_gpu(queue);
                    heightmap_renderer.update_uniforms(queue, &uniforms);
                });
            });
        });

        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let heightmap_renderer_resource = resources
            .get::<super::HeightmapRendererResource>()
            .unwrap();

        heightmap_renderer_resource.read_resource(|heightmap_renderer| {
            heightmap_renderer.render(render_pass);
        });
    }
}
