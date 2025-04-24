use eframe::egui_wgpu;
use egui::Widget;

use crate::gpu::{delaunay::delaunay_callback::DelaunayCallback, points_callback::PointsCallback};

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

        let delaunay_callback =
            DelaunayCallback::new(self.canvas_state_resource.clone(), screen_rect);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            screen_rect,
            delaunay_callback,
        ));

        let points_callback = PointsCallback::new(self.canvas_state_resource.clone(), screen_rect);
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            screen_rect,
            points_callback,
        ));

        canvas_response
    }
}
