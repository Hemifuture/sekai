use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::wgpu;
use eframe::egui_wgpu::wgpu::util::DeviceExt;
use thiserror::Error;

use super::UploadPlan;
use crate::gpu::canvas_uniform::CanvasUniforms;
use crate::view::{
    DisplayRevisions, LinearRgba, PreparedFieldDisplay, PreparedFieldKind, DIAGNOSTIC_ERROR_COLOR,
    DIAGNOSTIC_INFO_COLOR, DIAGNOSTIC_WARNING_COLOR, MAX_DISPLAY_CELLS, MAX_DISPLAY_INDICES,
    MAX_DISPLAY_VERTICES,
};

const MIN_BUFFER_BYTES: u64 = 16;
const MAX_PALETTE_ENTRIES: usize = 65_536;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct GpuCellVertex {
    position: [f32; 2],
    cell: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct FieldUniforms {
    canvas_x: f32,
    canvas_y: f32,
    canvas_width: f32,
    canvas_height: f32,
    translation_x: f32,
    translation_y: f32,
    scale: f32,
    canvas_padding1: f32,
    canvas_padding2: f32,
    canvas_padding3: f32,
    local_extent: [f32; 2],
    display_min: f32,
    display_max: f32,
    field_kind: u32,
    palette_len: u32,
    diagnostics_enabled: u32,
    diagnostic_info_index: u32,
    diagnostic_warning_index: u32,
    diagnostic_error_index: u32,
    padding: [u32; 4],
}

impl FieldUniforms {
    fn from_packet(
        packet: &PreparedFieldDisplay,
        canvas: &CanvasUniforms,
    ) -> Result<Self, FieldRenderError> {
        let palette_len = u32::try_from(packet.palette().len()).map_err(|_| {
            FieldRenderError::IntegerOverflow {
                context: "palette length",
            }
        })?;
        let diagnostic_info_index = palette_len;
        let diagnostic_warning_index =
            palette_len
                .checked_add(1)
                .ok_or(FieldRenderError::IntegerOverflow {
                    context: "diagnostic palette index",
                })?;
        let diagnostic_error_index =
            palette_len
                .checked_add(2)
                .ok_or(FieldRenderError::IntegerOverflow {
                    context: "diagnostic palette index",
                })?;
        let (display_min, display_max, field_kind) = match packet.field().kind() {
            PreparedFieldKind::Scalar => {
                let (min, max) = packet
                    .display_range()
                    .ok_or(FieldRenderError::MissingDisplayRange)?
                    .bounds();
                (min, max, 0)
            }
            PreparedFieldKind::Category => (0.0, 1.0, 1),
        };

        Ok(Self {
            canvas_x: canvas.canvas_x,
            canvas_y: canvas.canvas_y,
            canvas_width: canvas.canvas_width,
            canvas_height: canvas.canvas_height,
            translation_x: canvas.translation_x,
            translation_y: canvas.translation_y,
            scale: canvas.scale,
            canvas_padding1: canvas.padding1,
            canvas_padding2: canvas.padding2,
            canvas_padding3: canvas.padding3,
            local_extent: packet.mesh().local_extent(),
            display_min,
            display_max,
            field_kind,
            palette_len,
            diagnostics_enabled: u32::from(packet.diagnostics_enabled()),
            diagnostic_info_index,
            diagnostic_warning_index,
            diagnostic_error_index,
            padding: [0; 4],
        })
    }
}

/// Errors returned before a GPU upload mutates renderer state.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FieldRenderError {
    /// An element count could not be converted to bytes.
    #[error("GPU buffer size overflow")]
    BufferSizeOverflow,
    /// Required bytes exceeded a renderer or device limit.
    #[error("GPU buffer requires {required} bytes, limit is {max}")]
    BufferLimitExceeded {
        /// Required buffer bytes.
        required: u64,
        /// Maximum allowed buffer bytes.
        max: u64,
    },
    /// Checked renderer arithmetic overflowed.
    #[error("integer overflow while computing {context}")]
    IntegerOverflow {
        /// The checked operation's stable context.
        context: &'static str,
    },
    /// A scalar packet unexpectedly lacked a display range.
    #[error("scalar packet has no display range")]
    MissingDisplayRange,
}

/// Cumulative evidence of immutable uploads and per-frame uniform updates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RendererUploadStats {
    /// Number of vertex/index upload batches.
    pub geometry_uploads: u64,
    /// Number of raw field upload batches.
    pub field_uploads: u64,
    /// Number of diagnostic mask upload batches.
    pub diagnostic_uploads: u64,
    /// Number of combined palette upload batches.
    pub palette_uploads: u64,
    /// Number of uniform writes.
    pub uniform_updates: u64,
    /// Total bytes submitted through queue writes.
    pub uploaded_bytes: u64,
}

/// Reusable indexed renderer for scalar and category cell fields.
pub struct CellFieldRenderer {
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: u64,
    index_buffer: wgpu::Buffer,
    index_capacity: u64,
    field_buffer: wgpu::Buffer,
    field_capacity: u64,
    diagnostic_buffer: wgpu::Buffer,
    diagnostic_capacity: u64,
    palette_buffer: wgpu::Buffer,
    palette_capacity: u64,
    uniform_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    last_revisions: Option<DisplayRevisions>,
    index_count: u32,
    stats: RendererUploadStats,
}

impl CellFieldRenderer {
    /// Creates an empty renderer for one target texture format.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let vertex_buffer = create_buffer(
            device,
            "Field Vertices",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let index_buffer = create_buffer(
            device,
            "Field Indices",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );
        let field_buffer = create_buffer(
            device,
            "Field Values",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let diagnostic_buffer = create_buffer(
            device,
            "Field Diagnostics",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let palette_buffer = create_buffer(
            device,
            "Field Palette",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Field Uniforms"),
            contents: bytemuck::bytes_of(&FieldUniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = create_bind_group_layout(device);
        let bind_group = create_bind_group(
            device,
            &bind_group_layout,
            &vertex_buffer,
            &field_buffer,
            &diagnostic_buffer,
            &palette_buffer,
            &uniform_buffer,
        );
        let pipeline = create_pipeline(device, target_format, &bind_group_layout);

        Self {
            vertex_buffer,
            vertex_capacity: MIN_BUFFER_BYTES,
            index_buffer,
            index_capacity: MIN_BUFFER_BYTES,
            field_buffer,
            field_capacity: MIN_BUFFER_BYTES,
            diagnostic_buffer,
            diagnostic_capacity: MIN_BUFFER_BYTES,
            palette_buffer,
            palette_capacity: MIN_BUFFER_BYTES,
            uniform_buffer,
            bind_group_layout,
            bind_group,
            pipeline,
            last_revisions: None,
            index_count: 0,
            stats: RendererUploadStats::default(),
        }
    }

    /// Uploads only revision-changed immutable inputs and always refreshes uniforms.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &PreparedFieldDisplay,
        canvas: &CanvasUniforms,
    ) -> Result<(), FieldRenderError> {
        let plan = UploadPlan::between(self.last_revisions, packet.revisions());
        let vertices = plan.mesh.then(|| {
            packet
                .mesh()
                .vertices()
                .iter()
                .map(|vertex| GpuCellVertex {
                    position: vertex.position,
                    cell: vertex.cell,
                    padding: 0,
                })
                .collect::<Vec<_>>()
        });
        let palette = if plan.palette {
            Some(combined_palette(packet.palette())?)
        } else {
            None
        };
        let uniforms = FieldUniforms::from_packet(packet, canvas)?;
        let index_count = u32::try_from(packet.mesh().indices().len()).map_err(|_| {
            FieldRenderError::IntegerOverflow {
                context: "draw index count",
            }
        })?;

        let vertex_bytes =
            checked_buffer_bytes::<GpuCellVertex>(vertices.as_ref().map_or(0, Vec::len))?;
        let index_bytes = if plan.mesh {
            checked_buffer_bytes::<u32>(packet.mesh().indices().len())?
        } else {
            0
        };
        let field_bytes = if plan.field {
            checked_buffer_bytes::<u32>(packet.field().raw_values().len())?
        } else {
            0
        };
        let diagnostic_bytes = if plan.diagnostics {
            checked_buffer_bytes::<u32>(packet.diagnostics().cells().len())?
        } else {
            0
        };
        let palette_bytes = checked_buffer_bytes::<[f32; 4]>(palette.as_ref().map_or(0, Vec::len))?;

        let limits = device.limits();
        let storage_limit =
            u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        let vertex_limit =
            checked_buffer_bytes::<GpuCellVertex>(MAX_DISPLAY_VERTICES)?.min(storage_limit);
        let index_limit =
            checked_buffer_bytes::<u32>(MAX_DISPLAY_INDICES)?.min(limits.max_buffer_size);
        let cell_limit = checked_buffer_bytes::<u32>(MAX_DISPLAY_CELLS)?.min(storage_limit);
        let palette_limit =
            checked_buffer_bytes::<[f32; 4]>(MAX_PALETTE_ENTRIES)?.min(storage_limit);

        let new_vertex = if plan.mesh {
            replacement_buffer(
                device,
                "Field Vertices",
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                self.vertex_capacity,
                vertex_bytes,
                vertex_limit,
            )?
        } else {
            None
        };
        let new_index = if plan.mesh {
            replacement_buffer(
                device,
                "Field Indices",
                wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                self.index_capacity,
                index_bytes,
                index_limit,
            )?
        } else {
            None
        };
        let new_field = if plan.field {
            replacement_buffer(
                device,
                "Field Values",
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                self.field_capacity,
                field_bytes,
                cell_limit,
            )?
        } else {
            None
        };
        let new_diagnostic = if plan.diagnostics {
            replacement_buffer(
                device,
                "Field Diagnostics",
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                self.diagnostic_capacity,
                diagnostic_bytes,
                cell_limit,
            )?
        } else {
            None
        };
        let new_palette = if plan.palette {
            replacement_buffer(
                device,
                "Field Palette",
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                self.palette_capacity,
                palette_bytes,
                palette_limit,
            )?
        } else {
            None
        };

        let bind_group_changed = new_vertex.is_some()
            || new_field.is_some()
            || new_diagnostic.is_some()
            || new_palette.is_some();
        let next_bind_group = bind_group_changed.then(|| {
            create_bind_group(
                device,
                &self.bind_group_layout,
                new_vertex
                    .as_ref()
                    .map_or(&self.vertex_buffer, |replacement| &replacement.buffer),
                new_field
                    .as_ref()
                    .map_or(&self.field_buffer, |replacement| &replacement.buffer),
                new_diagnostic
                    .as_ref()
                    .map_or(&self.diagnostic_buffer, |replacement| &replacement.buffer),
                new_palette
                    .as_ref()
                    .map_or(&self.palette_buffer, |replacement| &replacement.buffer),
                &self.uniform_buffer,
            )
        });

        let next_stats = self.preflight_stats(
            plan,
            vertex_bytes,
            index_bytes,
            field_bytes,
            diagnostic_bytes,
            palette_bytes,
        )?;

        if let Some(vertices) = &vertices {
            write_if_nonempty(
                queue,
                new_vertex
                    .as_ref()
                    .map_or(&self.vertex_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(vertices),
            );
            write_if_nonempty(
                queue,
                new_index
                    .as_ref()
                    .map_or(&self.index_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(packet.mesh().indices()),
            );
        }
        if plan.field {
            write_if_nonempty(
                queue,
                new_field
                    .as_ref()
                    .map_or(&self.field_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(packet.field().raw_values()),
            );
        }
        if plan.diagnostics {
            write_if_nonempty(
                queue,
                new_diagnostic
                    .as_ref()
                    .map_or(&self.diagnostic_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(packet.diagnostics().cells()),
            );
        }
        if let Some(palette) = &palette {
            write_if_nonempty(
                queue,
                new_palette
                    .as_ref()
                    .map_or(&self.palette_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(palette),
            );
        }
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        apply_replacement(
            &mut self.vertex_buffer,
            &mut self.vertex_capacity,
            new_vertex,
        );
        apply_replacement(&mut self.index_buffer, &mut self.index_capacity, new_index);
        apply_replacement(&mut self.field_buffer, &mut self.field_capacity, new_field);
        apply_replacement(
            &mut self.diagnostic_buffer,
            &mut self.diagnostic_capacity,
            new_diagnostic,
        );
        apply_replacement(
            &mut self.palette_buffer,
            &mut self.palette_capacity,
            new_palette,
        );
        if let Some(bind_group) = next_bind_group {
            self.bind_group = bind_group;
        }
        self.last_revisions = Some(packet.revisions());
        self.index_count = index_count;
        self.stats = next_stats;
        Ok(())
    }

    /// Draws the last successfully prepared packet.
    pub fn render(&self, pass: &mut wgpu::RenderPass<'static>) {
        if self.index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    /// Returns cumulative upload evidence.
    pub const fn stats(&self) -> &RendererUploadStats {
        &self.stats
    }

    fn preflight_stats(
        &self,
        plan: UploadPlan,
        vertex_bytes: u64,
        index_bytes: u64,
        field_bytes: u64,
        diagnostic_bytes: u64,
        palette_bytes: u64,
    ) -> Result<RendererUploadStats, FieldRenderError> {
        let mut next = self.stats;
        if plan.mesh {
            next.geometry_uploads =
                checked_counter(next.geometry_uploads, 1, "geometry upload counter")?;
        }
        if plan.field {
            next.field_uploads = checked_counter(next.field_uploads, 1, "field upload counter")?;
        }
        if plan.diagnostics {
            next.diagnostic_uploads =
                checked_counter(next.diagnostic_uploads, 1, "diagnostic upload counter")?;
        }
        if plan.palette {
            next.palette_uploads =
                checked_counter(next.palette_uploads, 1, "palette upload counter")?;
        }
        next.uniform_updates = checked_counter(next.uniform_updates, 1, "uniform update counter")?;

        let immutable_bytes = [
            vertex_bytes,
            index_bytes,
            field_bytes,
            diagnostic_bytes,
            palette_bytes,
        ]
        .into_iter()
        .try_fold(0_u64, |total, bytes| {
            total
                .checked_add(bytes)
                .ok_or(FieldRenderError::IntegerOverflow {
                    context: "uploaded byte count",
                })
        })?;
        let submitted = immutable_bytes
            .checked_add(std::mem::size_of::<FieldUniforms>() as u64)
            .ok_or(FieldRenderError::IntegerOverflow {
                context: "uploaded byte count",
            })?;
        next.uploaded_bytes =
            checked_counter(next.uploaded_bytes, submitted, "uploaded byte counter")?;
        Ok(next)
    }
}

struct ReplacementBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
}

fn checked_buffer_bytes<T>(len: usize) -> Result<u64, FieldRenderError> {
    len.checked_mul(std::mem::size_of::<T>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(FieldRenderError::BufferSizeOverflow)
}

fn next_buffer_capacity(required: u64, current: u64, max: u64) -> Result<u64, FieldRenderError> {
    let required = required.max(MIN_BUFFER_BYTES);
    if required > max {
        return Err(FieldRenderError::BufferLimitExceeded { required, max });
    }
    if current >= required {
        return Ok(current);
    }
    let grown = required.checked_next_power_of_two().unwrap_or(max).min(max);
    if grown < required {
        return Err(FieldRenderError::BufferLimitExceeded { required, max });
    }
    Ok(grown)
}

fn combined_palette(base: &[LinearRgba]) -> Result<Vec<[f32; 4]>, FieldRenderError> {
    let capacity = base
        .len()
        .checked_add(3)
        .ok_or(FieldRenderError::IntegerOverflow {
            context: "combined palette length",
        })?;
    let mut combined = Vec::with_capacity(capacity);
    combined.extend(base.iter().map(|color| color.components()));
    combined.push(DIAGNOSTIC_INFO_COLOR.components());
    combined.push(DIAGNOSTIC_WARNING_COLOR.components());
    combined.push(DIAGNOSTIC_ERROR_COLOR.components());
    Ok(combined)
}

fn replacement_buffer(
    device: &wgpu::Device,
    label: &'static str,
    usage: wgpu::BufferUsages,
    current: u64,
    required: u64,
    max: u64,
) -> Result<Option<ReplacementBuffer>, FieldRenderError> {
    let capacity = next_buffer_capacity(required, current, max)?;
    Ok((capacity > current).then(|| ReplacementBuffer {
        buffer: create_buffer(device, label, capacity, usage),
        capacity,
    }))
}

fn apply_replacement(
    buffer: &mut wgpu::Buffer,
    capacity: &mut u64,
    replacement: Option<ReplacementBuffer>,
) {
    if let Some(replacement) = replacement {
        *buffer = replacement.buffer;
        *capacity = replacement.capacity;
    }
}

fn create_buffer(
    device: &wgpu::Device,
    label: &'static str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}

fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Field Fill Bind Group Layout"),
        entries: &[
            storage_layout_entry(0),
            storage_layout_entry(1),
            storage_layout_entry(2),
            storage_layout_entry(3),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn storage_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    vertices: &wgpu::Buffer,
    field: &wgpu::Buffer,
    diagnostics: &wgpu::Buffer,
    palette: &wgpu::Buffer,
    uniforms: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Field Fill Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: vertices.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: field.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: diagnostics.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: palette.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: uniforms.as_entire_binding(),
            },
        ],
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Field Fill Shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../../../assets/shaders/field_fill.wgsl").into(),
        ),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Field Fill Pipeline Layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Field Fill Pipeline"),
        layout: Some(&layout),
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
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn write_if_nonempty(queue: &wgpu::Queue, buffer: &wgpu::Buffer, bytes: &[u8]) {
    if !bytes.is_empty() {
        queue.write_buffer(buffer, 0, bytes);
    }
}

fn checked_counter(
    current: u64,
    amount: u64,
    context: &'static str,
) -> Result<u64, FieldRenderError> {
    current
        .checked_add(amount)
        .ok_or(FieldRenderError::IntegerOverflow { context })
}

#[cfg(test)]
mod tests {
    use super::{
        checked_buffer_bytes, combined_palette, next_buffer_capacity, FieldRenderError,
        FieldUniforms, GpuCellVertex,
    };
    use crate::view::{
        LinearRgba, DIAGNOSTIC_ERROR_COLOR, DIAGNOSTIC_INFO_COLOR, DIAGNOSTIC_WARNING_COLOR,
    };

    #[test]
    fn gpu_struct_layouts_match_storage_and_uniform_contracts() {
        assert_eq!(std::mem::size_of::<GpuCellVertex>(), 16);
        assert_eq!(std::mem::size_of::<FieldUniforms>(), 96);
    }

    #[test]
    fn checked_buffer_growth_is_power_of_two_and_never_crosses_limits() {
        assert_eq!(next_buffer_capacity(0, 0, 128).unwrap(), 16);
        assert_eq!(next_buffer_capacity(17, 16, 128).unwrap(), 32);
        assert_eq!(next_buffer_capacity(64, 128, 128).unwrap(), 128);
        assert!(matches!(
            next_buffer_capacity(129, 16, 128),
            Err(FieldRenderError::BufferLimitExceeded { .. })
        ));
        assert_eq!(
            checked_buffer_bytes::<GpuCellVertex>(usize::MAX),
            Err(FieldRenderError::BufferSizeOverflow)
        );
    }

    #[test]
    fn combined_palette_appends_shared_diagnostic_colors_in_severity_order() {
        let base = [
            LinearRgba::new(0.0, 0.1, 0.2, 1.0),
            LinearRgba::new(0.3, 0.4, 0.5, 1.0),
        ];
        let combined = combined_palette(&base).unwrap();

        assert_eq!(combined.len(), 5);
        assert_eq!(combined[0], base[0].components());
        assert_eq!(combined[1], base[1].components());
        assert_eq!(combined[2], DIAGNOSTIC_INFO_COLOR.components());
        assert_eq!(combined[3], DIAGNOSTIC_WARNING_COLOR.components());
        assert_eq!(combined[4], DIAGNOSTIC_ERROR_COLOR.components());
    }
}
