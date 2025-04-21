use std::num::NonZeroU64;

use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use egui::emath::TSTransform;
use egui::Pos2;

use crate::delaunay::Triangle;
use crate::gpu::canvas_uniform::CanvasUniforms;
use crate::gpu::map_renderer::MapRenderer;

const MAX_TRIANGLES: usize = 10_000;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GPUTriangle {
    pub points: [Pos2; 3],
}

impl Default for GPUTriangle {
    fn default() -> Self {
        Self {
            points: [
                Pos2::new(0.0, 0.0),
                Pos2::new(0.0, 0.0),
                Pos2::new(0.0, 0.0),
            ],
        }
    }
}

pub struct DelaunayRenderer {
    pub triangles: Vec<GPUTriangle>,
    pub uniforms: CanvasUniforms,
    pub delaunay_buffer: wgpu::Buffer,
    pub uniform_buffer: wgpu::Buffer,
    pub delaunay_pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl DelaunayRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let triangles: Vec<GPUTriangle> = vec![GPUTriangle::default(); MAX_TRIANGLES];
        let uniforms = CanvasUniforms::new(egui::Rect::ZERO, TSTransform::IDENTITY);

        let delaunay_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("delaunay_buffer"),
            contents: bytemuck::cast_slice(&triangles),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map_uniform_buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map_bind_group_layout"),
            entries: &[
                // 绑定 storage_buffer
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<Triangle>() as u64 * triangles.len() as u64,
                        ),
                    },
                    count: None,
                },
                // 绑定 uniform_buffer
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        // 16字节
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<CanvasUniforms>() as u64
                        ),
                    },
                    count: None,
                },
            ],
        });

        // 创建 BindGroup
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: delaunay_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let delaunay_pipeline =
            MapRenderer::create_delaunay_pipeline(device, &pipeline_layout, target_format);

        Self {
            triangles,
            uniforms,
            delaunay_buffer,
            uniform_buffer,
            delaunay_pipeline,
            bind_group,
        }
    }

    pub fn update_triangles(&mut self, triangles: Vec<GPUTriangle>) {
        self.triangles = triangles;
    }

    pub fn update_uniforms(&mut self, rect: egui::Rect, transform: TSTransform) {
        self.uniforms = CanvasUniforms::new(rect, transform);
    }

    pub fn upload_to_gpu(&self, queue: &wgpu::Queue) {
        queue.write_buffer(
            &self.delaunay_buffer,
            0,
            bytemuck::cast_slice(&self.triangles),
        );
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        render_pass.set_pipeline(&self.delaunay_pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..self.triangles.len() as u32 * 6, 0..1);
    }
}
