use eframe::egui_wgpu;
use egui::Widget;

use crate::gpu::field::FieldFillCallback;

use super::canvas::Canvas;

impl Widget for &mut Canvas {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = ui.available_size();
        let (screen_rect, canvas_response) =
            ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

        self.fit_current_mesh_once(screen_rect);
        self.input_state_manager.update(ui);
        if canvas_response.clicked_by(egui::PointerButton::Primary) {
            if let Some(screen_position) = canvas_response.interact_pointer_pos() {
                let local = self
                    .canvas_state_resource
                    .read_resource(|canvas| canvas.to_canvas(screen_position));
                let selected = self.field_display_resource.read_resource(|display| {
                    display
                        .current()
                        .and_then(|packet| packet.mesh().pick_local([local.x, local.y]))
                });
                self.field_viewer_state_resource
                    .with_resource(|state| state.select_cell(selected));
            }
        }

        let field_callback = FieldFillCallback::new(
            self.canvas_state_resource.clone(),
            self.field_display_resource.clone(),
            screen_rect,
        );
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            screen_rect,
            field_callback,
        ));

        canvas_response
    }
}
