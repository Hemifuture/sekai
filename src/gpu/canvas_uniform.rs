use egui::emath::TSTransform;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CanvasUniforms {
    pub canvas_x: f32,
    pub canvas_y: f32,
    pub canvas_width: f32,
    pub canvas_height: f32,
    pub translation_x: f32,
    pub translation_y: f32,
    pub scale: f32,
    pub padding1: f32,
    pub padding2: f32,
    pub padding3: f32,
}

impl CanvasUniforms {
    pub fn new(canvas_rect: egui::Rect, transform: TSTransform) -> Self {
        Self {
            canvas_x: canvas_rect.min.x,
            canvas_y: canvas_rect.min.y,
            canvas_width: canvas_rect.width(),
            canvas_height: canvas_rect.height(),
            translation_x: transform.translation.x,
            translation_y: transform.translation.y,
            scale: transform.scaling,
            padding1: 0.0,
            padding2: 0.0,
            padding3: 0.0,
        }
    }
}
