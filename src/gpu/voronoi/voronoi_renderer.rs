use std::num::NonZeroU64;

use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use egui::emath::TSTransform;
use egui::{Pos2, Rect};

use crate::gpu::canvas_uniform::CanvasUniforms;
use crate::gpu::map_renderer::MapRenderer;
use crate::resource::CanvasStateResource;
use crate::spatial::EdgeIndex;

const INITIAL_MAX_VORONOI_VERTICES: usize = 100_000;
const INITIAL_MAX_VORONOI_INDICES: usize = 200_000;

/// Voronoi 图渲染器
///
/// 使用 `u32` 类型的索引，与 GPU 索引缓冲区兼容。
/// 内部使用空间索引加速视口裁剪。
pub struct VoronoiRenderer {
    canvas_state_resource: CanvasStateResource,
    pub vertices: Vec<Pos2>,
    /// 边的索引（u32），每2个索引构成一条边
    pub indices: Vec<u32>,
    /// 边的空间索引，用于快速视口裁剪
    edge_index: Option<EdgeIndex>,
    visible_indices_count: usize,
    vertices_capacity: usize,
    indices_capacity: usize,
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
        let vertices: Vec<Pos2> = Vec::new();
        let indices: Vec<u32> = Vec::new();
        let vertices_capacity = INITIAL_MAX_VORONOI_VERTICES;
        let indices_capacity = INITIAL_MAX_VORONOI_INDICES;
        let uniforms = CanvasUniforms::new(Rect::ZERO, TSTransform::IDENTITY);

        // 创建顶点缓冲区
        let vertices_buffer = Self::create_vertices_buffer(device, vertices_capacity);

        // 创建索引缓冲区
        let indices_buffer = Self::create_indices_buffer(device, indices_capacity);

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
            edge_index: None,
            visible_indices_count: 0,
            vertices_capacity,
            indices_capacity,
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
        // 顶点更新后需要重建空间索引
        self.edge_index = None;
    }

    /// 更新索引数据
    ///
    /// 输入为边索引（每2个索引构成一条边）。
    pub fn update_indices(&mut self, indices: Vec<u32>) {
        self.indices = indices;
        self.visible_indices_count = self.indices.len();
        // 索引更新后需要重建空间索引
        self.edge_index = None;
    }

    /// 设置预构建的边空间索引
    ///
    /// 如果 MapSystem 已经构建了空间索引，可以直接使用避免重复构建。
    #[allow(dead_code)]
    pub fn set_edge_index(&mut self, edge_index: EdgeIndex) {
        self.edge_index = Some(edge_index);
    }

    /// 确保空间索引已构建
    fn ensure_edge_index(&mut self) {
        if self.edge_index.is_none() && !self.vertices.is_empty() && !self.indices.is_empty() {
            // 计算边界框
            let bounds = self.compute_bounds();
            self.edge_index = Some(EdgeIndex::build_auto(&self.vertices, &self.indices, bounds));
        }
    }

    /// 计算顶点的边界框
    fn compute_bounds(&self) -> Rect {
        if self.vertices.is_empty() {
            return Rect::ZERO;
        }

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for v in &self.vertices {
            min_x = min_x.min(v.x);
            min_y = min_y.min(v.y);
            max_x = max_x.max(v.x);
            max_y = max_y.max(v.y);
        }

        Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
    }

    pub fn update_uniforms(&mut self, rect: Rect, transform: TSTransform) {
        self.uniforms = CanvasUniforms::new(rect, transform);
    }

    fn create_vertices_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voronoi_vertices_buffer"),
            size: (std::mem::size_of::<Pos2>() * capacity.max(1)) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn create_indices_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voronoi_indices_buffer"),
            size: (std::mem::size_of::<u32>() * capacity.max(1)) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn required_capacity(current: usize, required: usize) -> usize {
        if required <= current {
            return current.max(1);
        }

        required.next_power_of_two()
    }

    fn recreate_bind_group(&mut self, device: &wgpu::Device) {
        let bind_group_layout = self.voronoi_pipeline.get_bind_group_layout(0);
        self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("voronoi_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.vertices_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.indices_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
            ],
        });
    }

    fn ensure_gpu_capacity(
        &mut self,
        device: &wgpu::Device,
        required_vertices: usize,
        required_indices: usize,
    ) {
        let new_vertices_capacity =
            Self::required_capacity(self.vertices_capacity, required_vertices);
        let new_indices_capacity = Self::required_capacity(self.indices_capacity, required_indices);

        let mut resized = false;

        if new_vertices_capacity != self.vertices_capacity {
            self.vertices_buffer = Self::create_vertices_buffer(device, new_vertices_capacity);
            self.vertices_capacity = new_vertices_capacity;
            resized = true;
        }

        if new_indices_capacity != self.indices_capacity {
            self.indices_buffer = Self::create_indices_buffer(device, new_indices_capacity);
            self.indices_capacity = new_indices_capacity;
            resized = true;
        }

        if resized {
            self.recreate_bind_group(device);
        }
    }

    pub fn upload_to_gpu(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut visible_indices: Vec<u32> = Vec::new();

        if !self.indices.is_empty() {
            // 确保空间索引已构建
            self.ensure_edge_index();

            // 使用空间索引进行快速视口裁剪
            let view_rect = self.canvas_state_resource.read_resource(|canvas_state| {
                canvas_state.to_canvas_rect(egui::Rect::from_min_max(
                    egui::Pos2::new(self.uniforms.canvas_x, self.uniforms.canvas_y),
                    egui::Pos2::new(
                        self.uniforms.canvas_x + self.uniforms.canvas_width,
                        self.uniforms.canvas_y + self.uniforms.canvas_height,
                    ),
                ))
            });

            visible_indices = if let Some(ref edge_index) = self.edge_index {
                edge_index.get_visible_indices(&self.vertices, &self.indices, view_rect)
            } else {
                // 后备：如果没有空间索引，返回所有索引
                self.indices.clone()
            };
        }

        self.ensure_gpu_capacity(device, self.vertices.len(), visible_indices.len());

        if !self.vertices.is_empty() {
            queue.write_buffer(
                &self.vertices_buffer,
                0,
                bytemuck::cast_slice(&self.vertices),
            );
        }

        self.visible_indices_count = visible_indices.len();

        if self.visible_indices_count > 0 {
            queue.write_buffer(
                &self.indices_buffer,
                0,
                bytemuck::cast_slice(&visible_indices),
            );
        }

        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[self.uniforms]),
        );
    }

    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        if self.vertices.is_empty() || self.visible_indices_count == 0 {
            return;
        }

        render_pass.set_pipeline(&self.voronoi_pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..self.visible_indices_count as u32, 0..1);
    }
}
