use eframe::egui_wgpu::{self, wgpu};

use crate::gpu::canvas_uniform::CanvasUniforms;
use crate::resource::{CanvasStateResource, FieldDisplayResource, FieldRendererResource};

/// egui-wgpu callback that consumes only an immutable prepared display packet.
pub struct FieldFillCallback {
    canvas_state_resource: CanvasStateResource,
    field_display_resource: FieldDisplayResource,
    canvas_rect: egui::Rect,
}

impl FieldFillCallback {
    /// Creates a field callback for one canvas rectangle.
    pub fn new(
        canvas_state_resource: CanvasStateResource,
        field_display_resource: FieldDisplayResource,
        canvas_rect: egui::Rect,
    ) -> Self {
        Self {
            canvas_state_resource,
            field_display_resource,
            canvas_rect,
        }
    }
}

impl egui_wgpu::CallbackTrait for FieldFillCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(packet) = self
            .field_display_resource
            .read_resource(|state| state.current_cloned())
        else {
            return Vec::new();
        };
        let uniforms = self
            .canvas_state_resource
            .read_resource(|canvas| CanvasUniforms::new(self.canvas_rect, canvas.transform));
        let Some(renderer) = resources.get::<FieldRendererResource>() else {
            publish_runtime_error(
                &self.field_display_resource,
                "display.resource",
                "field renderer resource is not registered",
            );
            return Vec::new();
        };
        let result =
            renderer.with_resource(|renderer| renderer.prepare(device, queue, &packet, &uniforms));
        if let Err(error) = result {
            publish_runtime_error(
                &self.field_display_resource,
                "display.gpu",
                error.to_string(),
            );
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let has_packet = self
            .field_display_resource
            .read_resource(|state| state.current().is_some());
        if !has_packet {
            return;
        }
        if let Some(renderer) = resources.get::<FieldRendererResource>() {
            renderer.read_resource(|renderer| renderer.render(render_pass));
        }
    }
}

fn publish_runtime_error(
    display: &FieldDisplayResource,
    code: &'static str,
    message: impl Into<String>,
) {
    display.with_resource(|state| {
        if let Err(error) = state.reject_runtime(code, message) {
            log::error!("failed to publish field display status: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::FieldFillCallback;
    use crate::resource::{CanvasStateResource, FieldDisplayResource};

    #[test]
    fn callback_accepts_an_explicitly_empty_display_resource() {
        let _callback = FieldFillCallback::new(
            CanvasStateResource::default(),
            FieldDisplayResource::default(),
            egui::Rect::ZERO,
        );
    }
}
