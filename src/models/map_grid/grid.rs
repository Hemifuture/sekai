use egui::accesskit::Point;

pub struct Grid {
    pub width: u32,
    pub height: u32,
    pub spacing: u32,
    pub points: Vec<Point>,
}

impl Grid {
    pub fn new(width: u32, height: u32, spacing: u32) -> Self {
        Self {
            width,
            height,
            spacing,
            points: vec![],
        }
    }

    pub fn generate_points(&mut self) {}

    fn generate_jittered_grid(&mut self) {}
}
