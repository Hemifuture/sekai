use eframe::{egui_wgpu::wgpu, wgpu::PipelineCompilationOptions};

use super::map_renderer::MapRenderer;

impl MapRenderer {
    pub fn create_points_pipeline(
        device: &wgpu::Device,
        pipeline_layout: &wgpu::PipelineLayout,
        target_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let points_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("points_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../assets/shaders/points.wgsl"
            ))),
        });

        let points_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("points_pipeline"),
            cache: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &points_shader_module,
                compilation_options: PipelineCompilationOptions::default(),
                entry_point: Some("vs_main"), // 对应 WGSL 中的入口函数
                buffers: &[],                 // 我们用 StorageBuffer，而不是传统的 VertexBuffer
            },
            fragment: Some(wgpu::FragmentState {
                module: &points_shader_module,
                compilation_options: PipelineCompilationOptions::default(),
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        points_pipeline
    }

    pub fn create_delaunay_pipeline(
        device: &wgpu::Device,
        pipeline_layout: &wgpu::PipelineLayout,
        target_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let delaunay_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("delaunay_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../assets/shaders/delaunay.wgsl"
            ))),
        });

        let delaunay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("delaunay_pipeline"),
            cache: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &delaunay_shader_module,
                compilation_options: PipelineCompilationOptions::default(),
                entry_point: Some("vs_main"), // 对应 WGSL 中的入口函数
                buffers: &[],                 // 我们用 StorageBuffer，而不是传统的 VertexBuffer
            },
            fragment: Some(wgpu::FragmentState {
                module: &delaunay_shader_module,
                compilation_options: PipelineCompilationOptions::default(),
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        delaunay_pipeline
    }

    pub fn create_voronoi_pipeline(
        device: &wgpu::Device,
        pipeline_layout: &wgpu::PipelineLayout,
        target_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let voronoi_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("voronoi_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../../assets/shaders/voronoi.wgsl"
            ))),
        });

        let voronoi_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("voronoi_pipeline"),
            cache: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &voronoi_shader_module,
                compilation_options: PipelineCompilationOptions::default(),
                entry_point: Some("vs_main"), // 对应 WGSL 中的入口函数
                buffers: &[],                 // 我们用 StorageBuffer，而不是传统的 VertexBuffer
            },
            fragment: Some(wgpu::FragmentState {
                module: &voronoi_shader_module,
                compilation_options: PipelineCompilationOptions::default(),
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        voronoi_pipeline
    }
}
