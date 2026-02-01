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

/// 颜色停止点结构
struct ColorStop {
    position: f32,  // 0.0-1.0
    color: (f32, f32, f32),  // RGB
}

/// 在两个颜色之间平滑插值
fn lerp_color(c1: (f32, f32, f32), c2: (f32, f32, f32), t: f32) -> (f32, f32, f32) {
    // 使用平滑的 smoothstep 插值，避免线性过渡的生硬感
    let t_smooth = t * t * (3.0 - 2.0 * t);
    (
        c1.0 + (c2.0 - c1.0) * t_smooth,
        c1.1 + (c2.1 - c1.1) * t_smooth,
        c1.2 + (c2.2 - c1.2) * t_smooth,
    )
}

/// 根据高度值生成颜色 - 改进版平滑渐变
pub fn height_to_color(height: u8) -> Color32 {
    // 海平面
    const SEA_LEVEL: u8 = 20;
    
    let ratio = height as f32 / 255.0;
    let sea_ratio = SEA_LEVEL as f32 / 255.0;
    
    if height < SEA_LEVEL {
        // ========== 海洋渐变 ==========
        // 从深海到浅海：深蓝 → 中蓝 → 浅蓝/青色
        let ocean_stops: [(f32, (f32, f32, f32)); 4] = [
            (0.0,       (8.0, 24.0, 58.0)),      // 深海：非常深的蓝
            (0.3,       (16.0, 48.0, 120.0)),    // 中深海
            (0.7,       (32.0, 80.0, 170.0)),    // 浅海
            (1.0,       (60.0, 120.0, 190.0)),   // 近岸浅水
        ];
        
        let ocean_ratio = ratio / sea_ratio;  // 0.0（最深）到 1.0（海平面）
        
        interpolate_gradient(&ocean_stops, ocean_ratio)
    } else {
        // ========== 陆地渐变 ==========
        // 多色调平滑过渡：沙滩 → 深绿 → 浅绿 → 黄绿 → 黄 → 橙 → 棕 → 灰岩 → 雪白
        let land_stops: [(f32, (f32, f32, f32)); 10] = [
            (0.0,       (210.0, 180.0, 140.0)),  // 沙滩/海岸
            (0.05,      (34.0, 120.0, 50.0)),    // 深绿（低地森林）
            (0.15,      (50.0, 150.0, 50.0)),    // 中绿
            (0.25,      (100.0, 170.0, 60.0)),   // 浅绿
            (0.35,      (160.0, 180.0, 70.0)),   // 黄绿（草地/灌木）
            (0.45,      (200.0, 170.0, 80.0)),   // 黄/卡其（干草/丘陵）
            (0.55,      (180.0, 130.0, 70.0)),   // 橙棕（低山）
            (0.70,      (130.0, 100.0, 70.0)),   // 深棕（山地）
            (0.85,      (150.0, 145.0, 140.0)),  // 灰色（岩石）
            (1.0,       (255.0, 255.0, 255.0)),  // 白色（雪峰）
        ];
        
        let land_ratio = (ratio - sea_ratio) / (1.0 - sea_ratio);  // 归一化到 0.0-1.0
        
        interpolate_gradient(&land_stops, land_ratio)
    }
}

/// 根据渐变停止点数组进行插值
fn interpolate_gradient(stops: &[(f32, (f32, f32, f32))], ratio: f32) -> Color32 {
    let ratio = ratio.clamp(0.0, 1.0);
    
    // 找到 ratio 所在的区间
    for i in 0..stops.len() - 1 {
        let (pos1, color1) = stops[i];
        let (pos2, color2) = stops[i + 1];
        
        if ratio >= pos1 && ratio <= pos2 {
            let t = (ratio - pos1) / (pos2 - pos1);
            let (r, g, b) = lerp_color(color1, color2, t);
            return Color32::from_rgb(r as u8, g as u8, b as u8);
        }
    }
    
    // 默认返回最后一个颜色
    let (_, (r, g, b)) = stops[stops.len() - 1];
    Color32::from_rgb(r as u8, g as u8, b as u8)
}
