use std::num::NonZeroU64;

use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use egui::emath::TSTransform;
use egui::{Pos2, Rect};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::gpu::canvas_uniform::CanvasUniforms;
use crate::gpu::helpers;
use crate::gpu::map_renderer::MapRenderer;
use crate::resource::CanvasStateResource;

const MAX_VORONOI_VERTICES: usize = 100_000;
const MAX_VORONOI_INDICES: usize = 200_000;

// 删除Matrix4x4相关代码，直接使用CanvasUniforms
pub struct VoronoiRenderer {
    canvas_state_resource: CanvasStateResource,
    pub vertices: Vec<Pos2>,
    pub indices: Vec<usize>,
    pub uniforms: CanvasUniforms,
    pub vertices_buffer: wgpu::Buffer,
    pub indices_buffer: wgpu::Buffer,
    pub uniform_buffer: wgpu::Buffer,
    pub voronoi_pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl VoronoiRenderer {
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        canvas_state_resource: CanvasStateResource,
    ) -> Self {
        let vertices: Vec<Pos2> = Vec::with_capacity(MAX_VORONOI_VERTICES);
        let indices: Vec<usize> = Vec::with_capacity(MAX_VORONOI_INDICES);
        let uniforms = CanvasUniforms::new(Rect::ZERO, TSTransform::IDENTITY);

        // 创建顶点缓冲区
        let vertices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voronoi_vertices_buffer"),
            size: (std::mem::size_of::<Pos2>() * MAX_VORONOI_VERTICES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 创建索引缓冲区
        let indices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voronoi_indices_buffer"),
            size: (std::mem::size_of::<u32>() * MAX_VORONOI_INDICES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 创建Uniform缓冲区，直接使用CanvasUniforms
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("voronoi_uniform_buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // 创建绑定组布局
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("voronoi_bind_group_layout"),
            entries: &[
                // 绑定顶点缓冲区
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(std::mem::size_of::<Pos2>() as u64),
                    },
                    count: None,
                },
                // 绑定索引缓冲区
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(std::mem::size_of::<u32>() as u64),
                    },
                    count: None,
                },
                // 绑定Uniform缓冲区
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<CanvasUniforms>() as u64
                        ),
                    },
                    count: None,
                },
            ],
        });

        // 创建绑定组
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("voronoi_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: vertices_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: indices_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        // 创建管线布局
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("voronoi_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // 创建管线
        let voronoi_pipeline =
            MapRenderer::create_voronoi_pipeline(device, &pipeline_layout, target_format);

        Self {
            canvas_state_resource,
            vertices,
            indices,
            uniforms,
            vertices_buffer,
            indices_buffer,
            uniform_buffer,
            voronoi_pipeline,
            bind_group,
        }
    }

    pub fn update_vertices(&mut self, vertices: Vec<Pos2>) {
        self.vertices = vertices;
    }

    pub fn update_indices(&mut self, indices: Vec<usize>) {
        self.indices = indices;
    }

    pub fn update_uniforms(&mut self, rect: Rect, transform: TSTransform) {
        self.uniforms = CanvasUniforms::new(rect, transform);
    }

    pub fn upload_to_gpu(&self, queue: &wgpu::Queue) {
        if !self.vertices.is_empty() {
            println!("[voronoi]update_vertices");
            queue.write_buffer(
                &self.vertices_buffer,
                0,
                bytemuck::cast_slice(&self.vertices),
            );
        }

        if !self.indices.is_empty() {
            let visible_indices = helpers::get_visible_indices(
                &self.vertices,
                self.uniforms,
                self.indices.clone(),
                self.canvas_state_resource.clone(),
            );
            println!("[voronoi]update_indices");
            queue.write_buffer(
                &self.indices_buffer,
                0,
                bytemuck::cast_slice(
                    &visible_indices
                        .par_iter()
                        .map(|i| *i as u32)
                        .collect::<Vec<_>>(),
                ),
            );
        }

        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        if self.vertices.is_empty() || self.indices.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.voronoi_pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..self.indices.len() as u32, 0..1);
    }
}
