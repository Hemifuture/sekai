use std::num::NonZeroU64;

use crate::gpu::canvas_uniform::CanvasUniforms;
use crate::gpu::map_renderer::MapRenderer;
use crate::resource::CanvasStateResource;
use crate::spatial::EdgeIndex;
use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use egui::emath::TSTransform;
use egui::{Pos2, Rect};

const MAX_POINTS: usize = 100_000;

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

/// Delaunay 三角剖分渲染器
///
/// 使用 `u32` 类型的索引，与 GPU 索引缓冲区兼容。
/// 内部使用空间索引加速视口裁剪。
pub struct DelaunayRenderer {
    canvas_state_resource: CanvasStateResource,
    pub points: Vec<Pos2>,
    /// 三角形边的索引（u32），每2个索引构成一条边
    pub triangle_indices: Vec<u32>,
    /// 边的空间索引，用于快速视口裁剪
    edge_index: Option<EdgeIndex>,
    pub uniforms: CanvasUniforms,
    pub points_buffer: wgpu::Buffer,
    pub triangle_indices_buffer: wgpu::Buffer,
    pub uniform_buffer: wgpu::Buffer,
    pub delaunay_pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl DelaunayRenderer {
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        canvas_state_resource: CanvasStateResource,
    ) -> Self {
        let points: Vec<Pos2> = vec![Pos2::ZERO; MAX_POINTS];
        let triangle_indices: Vec<u32> = vec![0; MAX_POINTS * 3];
        let uniforms = CanvasUniforms::new(egui::Rect::ZERO, TSTransform::IDENTITY);

        let points_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("points_buffer"),
            contents: bytemuck::cast_slice(&points),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let triangle_indices_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("triangle_indices_buffer"),
                contents: bytemuck::cast_slice(&triangle_indices),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map_uniform_buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map_bind_group_layout"),
            entries: &[
                // 绑定点数据
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<Pos2>() as u64 * MAX_POINTS as u64,
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

        let delaunay_pipeline =
            MapRenderer::create_delaunay_pipeline(device, &pipeline_layout, target_format);

        Self {
            canvas_state_resource,
            points,
            triangle_indices,
            edge_index: None,
            uniforms,
            points_buffer,
            triangle_indices_buffer,
            uniform_buffer,
            delaunay_pipeline,
            bind_group,
        }
    }

    pub fn update_points(&mut self, points: Vec<Pos2>) {
        self.points = points;
        // 点更新后需要重建空间索引
        self.edge_index = None;
    }

    /// 更新索引数据
    ///
    /// 输入为三角形索引（每3个索引构成一个三角形），
    /// 内部转换为线段列表格式（每2个索引构成一条边）。
    pub fn update_indices(&mut self, indices: Vec<u32>) {
        self.triangle_indices = self.make_line_list_indices(indices);
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
        if self.edge_index.is_none() && !self.points.is_empty() && !self.triangle_indices.is_empty()
        {
            // 计算边界框
            let bounds = self.compute_bounds();
            self.edge_index = Some(EdgeIndex::build_auto(
                &self.points,
                &self.triangle_indices,
                bounds,
            ));
        }
    }

    /// 计算点的边界框
    fn compute_bounds(&self) -> Rect {
        if self.points.is_empty() {
            return Rect::ZERO;
        }

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for p in &self.points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }

        Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
    }

    pub fn update_uniforms(&mut self, rect: egui::Rect, transform: TSTransform) {
        self.uniforms = CanvasUniforms::new(rect, transform);
    }

    pub fn upload_to_gpu(&mut self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.points_buffer, 0, bytemuck::cast_slice(&self.points));

        if !self.triangle_indices.is_empty() {
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

            let visible_indices = if let Some(ref edge_index) = self.edge_index {
                edge_index.get_visible_indices(&self.points, &self.triangle_indices, view_rect)
            } else {
                // 后备：如果没有空间索引，返回所有索引
                self.triangle_indices.clone()
            };

            #[cfg(debug_assertions)]
            // println!(
            //     "[delaunay] 可见边: {}/{} ({:.1}%)",
            //     visible_indices.len() / 2,
            //     self.triangle_indices.len() / 2,
            //     visible_indices.len() as f32 / self.triangle_indices.len() as f32 * 100.0
            // );
            queue.write_buffer(
                &self.triangle_indices_buffer,
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
        render_pass.set_pipeline(&self.delaunay_pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_index_buffer(
            self.triangle_indices_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );

        // 使用索引缓冲区绘制三角形边
        if !self.triangle_indices.is_empty() {
            render_pass.draw_indexed(0..self.triangle_indices.len() as u32, 0, 0..1);
        }
    }

    /// 将三角形索引转换为 LineList 需要的索引格式
    /// 每个三角形需要3条边，每条边2个顶点，共6个顶点
    fn make_line_list_indices(&self, triangle_indices: Vec<u32>) -> Vec<u32> {
        let mut line_indices = Vec::with_capacity(triangle_indices.len() * 2);

        for chunk in triangle_indices.chunks(3) {
            if chunk.len() == 3 {
                // 三角形的第一条边：顶点0->1
                line_indices.push(chunk[0]);
                line_indices.push(chunk[1]);

                // 三角形的第二条边：顶点1->2
                line_indices.push(chunk[1]);
                line_indices.push(chunk[2]);

                // 三角形的第三条边：顶点2->0
                line_indices.push(chunk[2]);
                line_indices.push(chunk[0]);
            }
        }

        line_indices
    }
}
