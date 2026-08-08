use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use eframe::egui_wgpu::{self, wgpu};

use super::renderer::{SphericalFrameUniform, SphericalGpuPacket};
use super::{SphericalFieldRenderer, SphericalRenderMode};
use crate::view::{GlobeCamera, MapCamera};

/// Egui-wgpu callback for one source-bound spherical fill packet and active camera.
pub struct SphericalPaintCallback {
    packet: Arc<SphericalGpuPacket>,
    mode: SphericalRenderMode,
    map_camera: MapCamera,
    globe_camera: GlobeCamera,
    viewport_pixels: [u32; 2],
    prepared_generation: AtomicU64,
}

impl SphericalPaintCallback {
    /// Captures the immutable packet and current per-mode camera state for one paint.
    pub fn new(
        packet: Arc<SphericalGpuPacket>,
        mode: SphericalRenderMode,
        map_camera: MapCamera,
        globe_camera: GlobeCamera,
        viewport_pixels: [u32; 2],
    ) -> Self {
        Self {
            packet,
            mode,
            map_camera,
            globe_camera,
            viewport_pixels,
            prepared_generation: AtomicU64::new(0),
        }
    }
}

impl egui_wgpu::CallbackTrait for SphericalPaintCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(renderer) = resources.get_mut::<SphericalFieldRenderer>() else {
            self.prepared_generation.store(0, Ordering::Release);
            log::error!("spherical field renderer is not registered in callback resources");
            return Vec::new();
        };
        let uniform = match self.mode {
            SphericalRenderMode::Map => {
                SphericalFrameUniform::for_map(&self.packet, self.map_camera, self.viewport_pixels)
            }
            SphericalRenderMode::Globe => SphericalFrameUniform::for_globe(
                &self.packet,
                self.globe_camera,
                self.viewport_pixels,
            ),
        };
        let result = uniform.and_then(|uniform| {
            renderer.prepare_packet(device, queue, &self.packet)?;
            renderer.prepare_frame(queue, self.mode, &uniform)
        });
        match result {
            Ok(generation) => self
                .prepared_generation
                .store(generation, Ordering::Release),
            Err(error) => {
                self.prepared_generation.store(0, Ordering::Release);
                log::error!("spherical field GPU preparation failed: {error}");
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let generation = self.prepared_generation.load(Ordering::Acquire);
        if generation == 0 {
            return;
        }
        if let Some(renderer) = resources.get::<SphericalFieldRenderer>() {
            renderer.paint_if_current(generation, self.mode, render_pass);
        }
    }
}
