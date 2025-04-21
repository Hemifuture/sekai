use std::num::NonZeroU64;

use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use eframe::wgpu::core::device::queue;
use egui::emath::TSTransform;
use egui::Pos2;

use super::canvas_uniform::CanvasUniforms;
use super::map_renderer::MapRenderer;

const MAX_POINTS: usize = 10_000;

pub struct PointsRenderer {
    pub points: Vec<Pos2>,
    pub uniforms: CanvasUniforms,
    pub points_buffer: wgpu::Buffer,
    pub uniform_buffer: wgpu::Buffer,
    pub points_pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl PointsRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let points: Vec<Pos2> = vec![Pos2::new(0.0, 0.0); MAX_POINTS];
        let uniforms = CanvasUniforms::new(egui::Rect::ZERO, TSTransform::IDENTITY);

        let points_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("points_buffer"),
            contents: bytemuck::cast_slice(&points),
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
                            std::mem::size_of::<Pos2>() as u64 * points.len() as u64,
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
                    resource: points_buffer.as_entire_binding(),
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

        let points_pipeline =
            MapRenderer::create_points_pipeline(device, &pipeline_layout, target_format);

        Self {
            points,
            uniforms,
            points_buffer,
            uniform_buffer,
            points_pipeline,
            bind_group,
        }
    }

    pub fn update_points(&mut self, points: Vec<Pos2>) {
        self.points = points;
    }

    pub fn update_uniforms(&mut self, rect: egui::Rect, transform: TSTransform) {
        self.uniforms = CanvasUniforms::new(rect, transform);
    }

    pub fn upload_to_gpu(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.points_buffer, 0, bytemuck::cast_slice(&self.points));
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        render_pass.set_pipeline(&self.points_pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..self.points.len() as u32 * 6, 0..1);
    }
}
