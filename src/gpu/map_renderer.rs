use std::num::NonZeroU64;

use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use egui::Pos2;

use crate::delaunay::Triangle;

#[allow(dead_code)]
const MAX_POINTS: usize = 10_000;
#[allow(dead_code)]
const MAX_VORONOI_CELLS: usize = 10_000;
#[allow(dead_code)]
const MAX_TRIANGLES: usize = 10_000;

#[allow(dead_code)]
pub struct MapRenderer {
    pub points: Vec<Pos2>,
    pub triangles: Vec<Triangle>,

    pub points_buffer: wgpu::Buffer,
    pub voronoi_buffer: wgpu::Buffer,
    pub uniform_buffer: wgpu::Buffer,

    pub points_pipeline: Option<wgpu::RenderPipeline>,
    pub voronoi_pipeline: Option<wgpu::RenderPipeline>,

    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
}

#[allow(dead_code)]
impl MapRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let points: Vec<Pos2> = vec![Pos2::new(0.0, 0.0); MAX_POINTS];
        let voronoi_cells: Vec<Pos2> = vec![Pos2::new(0.0, 0.0); MAX_VORONOI_CELLS];
        let triangles: Vec<Triangle> = vec![Triangle::new([
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 0.0),
        ])];

        let points_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map_points_buffer"),
            contents: bytemuck::cast_slice(&points),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let voronoi_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map_voronoi_buffer"),
            contents: bytemuck::cast_slice(&voronoi_cells),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_data = [0.0f32; 4];
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("map_uniform_buffer"),
            contents: bytemuck::cast_slice(&uniform_data),
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
                // 绑定 voronoi_buffer
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<Pos2>() as u64 * voronoi_cells.len() as u64,
                        ),
                    },
                    count: None,
                },
                // 绑定 uniform_buffer
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        // 16字节
                        min_binding_size: NonZeroU64::new(16),
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
                    resource: voronoi_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
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

        let voronoi_pipeline =
            MapRenderer::create_voronoi_pipeline(device, &pipeline_layout, target_format);

        Self {
            points,
            triangles,
            points_pipeline: Some(points_pipeline),
            voronoi_pipeline: Some(voronoi_pipeline),
            points_buffer,
            voronoi_buffer,
            uniform_buffer,
            bind_group_layout,
            bind_group,
        }
    }
}
