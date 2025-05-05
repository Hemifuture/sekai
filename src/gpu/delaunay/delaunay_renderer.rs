use std::num::NonZeroU64;

use crate::gpu::canvas_uniform::CanvasUniforms;
use crate::gpu::helpers;
use crate::gpu::map_renderer::MapRenderer;
use crate::resource::CanvasStateResource;
use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use egui::emath::TSTransform;
use egui::Pos2;

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

pub struct DelaunayRenderer {
    canvas_state_resource: CanvasStateResource,
    pub points: Vec<Pos2>,
    pub triangle_indices: Vec<u32>,
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
    }

    pub fn update_indices(&mut self, indices: Vec<u32>) {
        self.triangle_indices = self.make_line_list_indices(indices);
    }

    pub fn update_uniforms(&mut self, rect: egui::Rect, transform: TSTransform) {
        self.uniforms = CanvasUniforms::new(rect, transform);
    }

    pub fn upload_to_gpu(&self, queue: &wgpu::Queue) {
        let visible_triangle_indices = helpers::get_visible_indices(
            &self.points,
            self.uniforms,
            self.triangle_indices.clone(),
            self.canvas_state_resource.clone(),
        );
        // println!("{:#?}", self.uniforms);
        // let visible_indices = self.triangle_indices.clone();
        // println!("[delaunay]update_points");
        // println!(
        //     "points size: {:?}",
        //     self.points.len() * std::mem::size_of::<Pos2>()
        // );
        queue.write_buffer(&self.points_buffer, 0, bytemuck::cast_slice(&self.points));
        println!("[delaunay]update_triangle_indices");
        queue.write_buffer(
            &self.triangle_indices_buffer,
            0,
            bytemuck::cast_slice(&visible_triangle_indices),
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
        render_pass.set_index_buffer(
            self.triangle_indices_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );

        // 使用索引缓冲区绘制三角形边
        if !self.triangle_indices.is_empty() {
            render_pass.draw_indexed(0..self.triangle_indices.len() as u32, 0, 0..1);
        }
    }

    /// 将三角形索引转换为LineList需要的索引格式
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
