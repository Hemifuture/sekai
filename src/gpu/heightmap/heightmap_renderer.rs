// 高度图渲染器 - 渲染填充的 Voronoi 单元格

use bytemuck::{Pod, Zeroable};
use eframe::egui::{Color32, Pos2};
use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;

use crate::delaunay::voronoi::VoronoiCell;
use crate::gpu::canvas_uniform::CanvasUniforms;

const MAX_VERTICES: usize = 1_000_000; // 最多100万个顶点（对于复杂的填充多边形）

/// Pos2 的 wgpu 兼容表示
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Pos2Repr {
    x: f32,
    y: f32,
}

impl From<Pos2> for Pos2Repr {
    fn from(p: Pos2) -> Self {
        Self { x: p.x, y: p.y }
    }
}

/// 颜色的 wgpu 兼容表示
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct ColorRepr {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl From<Color32> for ColorRepr {
    fn from(c: Color32) -> Self {
        Self {
            r: c.r() as f32 / 255.0,
            g: c.g() as f32 / 255.0,
            b: c.b() as f32 / 255.0,
            a: c.a() as f32 / 255.0,
        }
    }
}

/// 高度图渲染器
pub struct HeightmapRenderer {
    vertices_buffer: wgpu::Buffer,
    colors_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,

    // CPU 端数据
    vertices: Vec<Pos2Repr>,
    colors: Vec<ColorRepr>,
    vertex_count: usize,
}

impl HeightmapRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        // 创建着色器模块
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Heightmap Shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../assets/shaders/heightmap.wgsl").into(),
            ),
        });

        // 创建缓冲区
        let vertices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Heightmap Vertices Buffer"),
            size: (MAX_VERTICES * std::mem::size_of::<Pos2Repr>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let colors_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Heightmap Colors Buffer"),
            size: (MAX_VERTICES * std::mem::size_of::<ColorRepr>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniforms = CanvasUniforms::new(
            eframe::egui::Rect::ZERO,
            eframe::egui::emath::TSTransform::IDENTITY,
        );
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Heightmap Uniforms Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // 创建绑定组布局
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Heightmap Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // 创建绑定组
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Heightmap Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: vertices_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: colors_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        // 创建管线布局
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Heightmap Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // 创建渲染管线
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Heightmap Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        Self {
            vertices_buffer,
            colors_buffer,
            uniform_buffer,
            bind_group,
            pipeline,
            vertices: Vec::new(),
            colors: Vec::new(),
            vertex_count: 0,
        }
    }

    /// 更新数据：为每个 Voronoi 单元格生成三角形
    pub fn update_data(
        &mut self,
        voronoi_vertices: &[Pos2],
        cells: &[VoronoiCell],
        cell_colors: &[Color32],
    ) {
        self.vertices.clear();
        self.colors.clear();

        for (cell_idx, cell) in cells.iter().enumerate() {
            if cell.vertex_indices.len() < 3 {
                continue; // 至少需要3个顶点才能形成三角形
            }

            let color = if cell_idx < cell_colors.len() {
                cell_colors[cell_idx]
            } else {
                Color32::GRAY
            };
            let color_repr = ColorRepr::from(color);

            // 使用扇形三角剖分（fan triangulation）
            // 选择第一个顶点作为中心点
            let center_idx = cell.vertex_indices[0] as usize;
            if center_idx >= voronoi_vertices.len() {
                continue;
            }
            let center = voronoi_vertices[center_idx];

            for i in 1..cell.vertex_indices.len() - 1 {
                let idx1 = cell.vertex_indices[i] as usize;
                let idx2 = cell.vertex_indices[i + 1] as usize;

                if idx1 >= voronoi_vertices.len() || idx2 >= voronoi_vertices.len() {
                    continue;
                }

                let v1 = voronoi_vertices[idx1];
                let v2 = voronoi_vertices[idx2];

                // 添加三角形的三个顶点
                self.vertices.push(Pos2Repr::from(center));
                self.vertices.push(Pos2Repr::from(v1));
                self.vertices.push(Pos2Repr::from(v2));

                self.colors.push(color_repr);
                self.colors.push(color_repr);
                self.colors.push(color_repr);
            }
        }

        self.vertex_count = self.vertices.len();
    }

    /// 上传数据到 GPU
    pub fn upload_to_gpu(&self, queue: &wgpu::Queue) {
        if self.vertex_count == 0 {
            return;
        }

        queue.write_buffer(
            &self.vertices_buffer,
            0,
            bytemuck::cast_slice(&self.vertices[..self.vertex_count]),
        );

        queue.write_buffer(
            &self.colors_buffer,
            0,
            bytemuck::cast_slice(&self.colors[..self.vertex_count]),
        );
    }

    /// 更新 uniforms
    pub fn update_uniforms(&self, queue: &wgpu::Queue, uniforms: &CanvasUniforms) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[*uniforms]));
    }

    /// 渲染
    pub fn render(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        if self.vertex_count == 0 {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..self.vertex_count as u32, 0..1);
    }
}

/// 根据高度值生成颜色
pub fn height_to_color(height: u8) -> Color32 {
    // 海平面
    const SEA_LEVEL: u8 = 20;

    if height < SEA_LEVEL {
        // 海洋：深蓝到浅蓝
        let depth = SEA_LEVEL - height;
        let ratio = depth as f32 / SEA_LEVEL as f32;
        let r = (10.0 + ratio * 20.0) as u8;
        let g = (50.0 + ratio * 50.0) as u8;
        let b = (150.0 + ratio * 105.0) as u8;
        Color32::from_rgb(r, g, b)
    } else {
        // 陆地：绿色到棕色到白色
        let elevation = height - SEA_LEVEL;
        let max_elevation = (255 - SEA_LEVEL) as u32;  // 转换为 u32 避免溢出

        let threshold_low = max_elevation / 3;
        let threshold_high = max_elevation * 2 / 3;  // 现在可以安全地乘以2

        if (elevation as u32) < threshold_low {
            // 低地：绿色
            let ratio = elevation as f32 / threshold_low as f32;
            let r = (34.0 + ratio * 100.0) as u8;
            let g = (139.0 + ratio * 50.0) as u8;
            let b = (34.0 - ratio * 20.0) as u8;
            Color32::from_rgb(r, g, b)
        } else if (elevation as u32) < threshold_high {
            // 中地：棕色
            let ratio = (elevation as u32 - threshold_low) as f32 / (threshold_high - threshold_low) as f32;
            let r = (134.0 + ratio * 40.0) as u8;
            let g = (89.0 + ratio * 30.0) as u8;
            let b = (14.0 + ratio * 10.0) as u8;
            Color32::from_rgb(r, g, b)
        } else {
            // 高地：白色（雪山）
            let ratio = (elevation as u32 - threshold_high) as f32 / (max_elevation - threshold_high) as f32;
            let r = (174.0 + ratio * 81.0) as u8;
            let g = (119.0 + ratio * 136.0) as u8;
            let b = (24.0 + ratio * 231.0) as u8;
            Color32::from_rgb(r, g, b)
        }
    }
}
