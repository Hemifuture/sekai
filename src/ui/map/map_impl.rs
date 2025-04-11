use egui::{Color32, FontId, Pos2, Response, Ui, Widget};

pub struct MapWidget {}

impl Default for MapWidget {
    fn default() -> Self {
        Self {}
    }
}

impl Widget for &mut MapWidget {
    fn ui(self, ui: &mut Ui) -> Response {
        let painter = ui.painter();
        painter.text(
            Pos2::new(10.0, 10.0),
            egui::Align2::CENTER_CENTER,
            "Map",
            FontId::monospace(13.0),
            Color32::RED,
        );
        let response = ui.label("Map");
        response
    }
}
