use std::num::NonZeroU64;

use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use egui::emath::TSTransform;
use egui::{Pos2, Rect};

use crate::delaunay::voronoi::IndexedVoronoiDiagram;
use crate::gpu::canvas_uniform::CanvasUniforms;
use crate::gpu::map_renderer::MapRenderer;
use crate::resource::CanvasStateResource;
use crate::terrain::HeightColorMap;

const MAX_VERTICES: usize = 300_000; // Each Voronoi cell triangulated

/// Renders Voronoi cells as filled polygons with height-based colors
pub struct HeightMapRenderer {
    canvas_state_resource: CanvasStateResource,
    pub vertices: Vec<Pos2>,
    pub colors: Vec<[f32; 4]>,
    pub uniforms: CanvasUniforms,
    pub vertices_buffer: wgpu::Buffer,
    pub colors_buffer: wgpu::Buffer,
    pub uniform_buffer: wgpu::Buffer,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl HeightMapRenderer {
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        canvas_state_resource: CanvasStateResource,
    ) -> Self {
        let vertices: Vec<Pos2> = Vec::with_capacity(MAX_VERTICES);
        let colors: Vec<[f32; 4]> = Vec::with_capacity(MAX_VERTICES);
        let uniforms = CanvasUniforms::new(Rect::ZERO, TSTransform::IDENTITY);

        // Create vertex buffer
        let vertices_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("height_map_vertices_buffer"),
            size: (std::mem::size_of::<Pos2>() * MAX_VERTICES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create color buffer
        let colors_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("height_map_colors_buffer"),
            size: (std::mem::size_of::<[f32; 4]>() * MAX_VERTICES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create uniform buffer
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("height_map_uniform_buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // Create bind group layout
        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("height_map_bind_group_layout"),
                entries: &[
                    // Vertex buffer
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<Pos2>() as u64
                            ),
                        },
                        count: None,
                    },
                    // Color buffer
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<[f32; 4]>() as u64
                            ),
                        },
                        count: None,
                    },
                    // Uniform buffer
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

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("height_map_bind_group"),
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

        // Load shader
        let shader_source = include_str!("../../../assets/shaders/height_map.wgsl");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("height_map_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("height_map_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("height_map_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
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
            canvas_state_resource,
            vertices,
            colors,
            uniforms,
            vertices_buffer,
            colors_buffer,
            uniform_buffer,
            pipeline,
            bind_group,
        }
    }

    /// Triangulate Voronoi cells and assign colors based on height data
    pub fn update_geometry(
        &mut self,
        voronoi: &IndexedVoronoiDiagram,
        heights: &[u8],
        color_map: &HeightColorMap,
    ) {
        self.vertices.clear();
        self.colors.clear();

        // Triangulate each Voronoi cell using fan triangulation
        for (cell_idx, cell) in voronoi.cells.iter().enumerate() {
            if cell.vertex_indices.len() < 3 {
                continue; // Skip degenerate cells
            }

            // Get height for this cell (use site index)
            let height = if cell.site_idx < heights.len() {
                heights[cell.site_idx]
            } else {
                0
            };

            // Get color for this height
            let color = color_map.interpolate_u8(height);

            // Get the cell's vertices
            let cell_vertices: Vec<Pos2> = cell
                .vertex_indices
                .iter()
                .filter_map(|&idx| {
                    if idx < voronoi.vertices.len() {
                        Some(voronoi.vertices[idx])
                    } else {
                        None
                    }
                })
                .collect();

            if cell_vertices.len() < 3 {
                continue;
            }

            // Fan triangulation: Use first vertex as pivot
            let pivot = cell_vertices[0];

            for i in 1..cell_vertices.len() - 1 {
                // Create triangle: pivot -> vertex[i] -> vertex[i+1]
                self.vertices.push(pivot);
                self.colors.push(color);

                self.vertices.push(cell_vertices[i]);
                self.colors.push(color);

                self.vertices.push(cell_vertices[i + 1]);
                self.colors.push(color);
            }
        }

        log::info!(
            "HeightMapRenderer: Generated {} triangles from {} cells",
            self.vertices.len() / 3,
            voronoi.cells.len()
        );
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
}

impl MapRenderer for HeightMapRenderer {
    fn prepare(&mut self, queue: &wgpu::Queue) {
        // Update canvas state
        self.canvas_state_resource.read_resource(|state| {
            self.uniforms = CanvasUniforms::new(state.canvas_rect, state.transform);
        });

        // Write uniforms to GPU
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[self.uniforms]));

        // Write vertices to GPU
        if !self.vertices.is_empty() {
            queue.write_buffer(
                &self.vertices_buffer,
                0,
                bytemuck::cast_slice(&self.vertices),
            );
        }

        // Write colors to GPU
        if !self.colors.is_empty() {
            queue.write_buffer(&self.colors_buffer, 0, bytemuck::cast_slice(&self.colors));
        }
    }

    fn paint<'rp>(&'rp self, render_pass: &mut wgpu::RenderPass<'rp>) {
        if self.vertices.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..self.vertices.len() as u32, 0..1);
    }
}
