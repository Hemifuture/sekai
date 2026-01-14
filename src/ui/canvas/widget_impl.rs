use eframe::egui_wgpu;
use egui::Widget;

use crate::gpu::{
    delaunay::delaunay_callback::DelaunayCallback,
    heightmap::heightmap_callback::HeightmapCallback,
    points_callback::PointsCallback,
    voronoi::voronoi_callback::VoronoiCallback,
};

use super::{canvas::Canvas, helpers::draw_grid};

impl Widget for &mut Canvas {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = ui.available_size();
        let (screen_rect, canvas_response) =
            ui.allocate_exact_size(desired_size, egui::Sense::drag());

        self.input_state_manager.update(ui);

        self.canvas_state_resource.read_resource(|canvas_state| {
            draw_grid(ui, canvas_state, screen_rect);
            // println!("canvas rect: {}", screen_rect);
            // println!("transform: {:?}", canvas_state.transform);
        });

        // 获取图层可见性设置
        let layer_visibility = self
            .map_system_resource
            .read_resource(|map_system| map_system.layer_visibility);

        // 图层按照从底到顶的顺序渲染
        // 1. 高度图图层（填充的Voronoi单元格）- 底层
        if layer_visibility.heightmap {
            let heightmap_callback = HeightmapCallback::new(
                self.canvas_state_resource.clone(),
                self.map_system_resource.clone(),
                screen_rect,
            );
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                screen_rect,
                heightmap_callback,
            ));
        }

        // 2. Delaunay三角剖分图层 - 中层
        if layer_visibility.delaunay {
            let delaunay_callback =
                DelaunayCallback::new(self.canvas_state_resource.clone(), screen_rect);
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                screen_rect,
                delaunay_callback,
            ));
        }

        // 3. Voronoi边线图层 - 上层
        if layer_visibility.voronoi_edges {
            let voronoi_callback =
                VoronoiCallback::new(self.canvas_state_resource.clone(), screen_rect);
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                screen_rect,
                voronoi_callback,
            ));
        }

        // 4. 点图层 - 最上层
        if layer_visibility.points {
            let points_callback =
                PointsCallback::new(self.canvas_state_resource.clone(), screen_rect);
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                screen_rect,
                points_callback,
            ));
        }

        canvas_response
    }
}
