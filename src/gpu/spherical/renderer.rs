use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::wgpu;
use std::sync::Arc;
use thiserror::Error;

use crate::view::{
    DisplayRevision, GlobeCamera, LinearRgba, MapCamera, PreparedFieldKind, PreparedFieldLayers,
    PreparedGlobeMesh, PreparedProjectedMap, SphericalPresentationSource, DIAGNOSTIC_ERROR_COLOR,
    DIAGNOSTIC_INFO_COLOR, DIAGNOSTIC_WARNING_COLOR,
};

const MIN_BUFFER_BYTES: u64 = 16;
const MAX_PALETTE_ENTRIES: usize = 65_536;

#[cfg(test)]
mod validation_probe {
    use std::cell::Cell;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(super) struct ScanCounts {
        pub full_validations: u64,
        pub cell_ids: u64,
        pub indices: u64,
        pub positions: u64,
    }

    thread_local! {
        static COUNTS: Cell<ScanCounts> = Cell::new(ScanCounts::default());
    }

    pub(super) fn reset() {
        COUNTS.set(ScanCounts::default());
    }

    pub(super) fn snapshot() -> ScanCounts {
        COUNTS.get()
    }

    pub(super) fn full_validation() {
        COUNTS.with(|slot| {
            let mut counts = slot.get();
            counts.full_validations += 1;
            slot.set(counts);
        });
    }

    pub(super) fn cell_id() {
        COUNTS.with(|slot| {
            let mut counts = slot.get();
            counts.cell_ids += 1;
            slot.set(counts);
        });
    }

    pub(super) fn index() {
        COUNTS.with(|slot| {
            let mut counts = slot.get();
            counts.indices += 1;
            slot.set(counts);
        });
    }

    pub(super) fn position() {
        COUNTS.with(|slot| {
            let mut counts = slot.get();
            counts.positions += 1;
            slot.set(counts);
        });
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct GpuMapVertex {
    position: [f32; 2],
    cell: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct GpuGlobeVertex {
    position: [f32; 3],
    cell: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(super) struct SphericalFrameUniform {
    transform: [[f32; 4]; 4],
    display_min: f32,
    display_max: f32,
    field_kind: u32,
    palette_len: u32,
    diagnostics_enabled: u32,
    diagnostic_info_index: u32,
    diagnostic_warning_index: u32,
    diagnostic_error_index: u32,
}

impl SphericalFrameUniform {
    pub(super) fn for_map(
        packet: &SphericalGpuPacket,
        camera: MapCamera,
        viewport: [u32; 2],
    ) -> Result<Self, SphericalRenderError> {
        let [width, height] = validated_viewport(viewport)?;
        let bounds = packet.map().bounds();
        let bounds_width = bounds.max_x() - bounds.min_x();
        let bounds_height = bounds.max_y() - bounds.min_y();
        if !bounds_width.is_finite()
            || !bounds_height.is_finite()
            || bounds_width <= 0.0
            || bounds_height <= 0.0
        {
            return Err(SphericalRenderError::InvalidGeometry {
                resource: "projected map bounds",
            });
        }
        let aspect = width / height;
        let map_aspect = bounds_width / bounds_height;
        let (fit_x, fit_y) = if aspect >= map_aspect {
            (2.0 / (bounds_height * aspect), 2.0 / bounds_height)
        } else {
            (2.0 / bounds_width, 2.0 * aspect / bounds_width)
        };
        let zoom = camera.zoom(packet.map().projection().kind());
        let pan = camera.pan(packet.map().projection().kind());
        let scale_x = fit_x * zoom;
        let scale_y = fit_y * zoom;
        let center_x = (bounds.min_x() + bounds.max_x()) * 0.5;
        let center_y = (bounds.min_y() + bounds.max_y()) * 0.5;
        let translate_x = -center_x * scale_x + pan[0] * 2.0;
        let translate_y = -center_y * scale_y + pan[1] * 2.0;
        let transform = f64_matrix_to_f32([
            [scale_x, 0.0, 0.0, 0.0],
            [0.0, scale_y, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [translate_x, translate_y, 0.0, 1.0],
        ])?;
        Self::with_transform(packet, transform)
    }

    pub(super) fn for_globe(
        packet: &SphericalGpuPacket,
        camera: GlobeCamera,
        viewport: [u32; 2],
    ) -> Result<Self, SphericalRenderError> {
        let [width, height] = validated_viewport(viewport)?;
        let aspect = width / height;
        let diameter_scale = camera.orthographic_scale();
        let (scale_x, scale_y) = if aspect >= 1.0 {
            (diameter_scale / aspect, diameter_scale)
        } else {
            (diameter_scale, diameter_scale * aspect)
        };
        let [x, y, z, w] = camera.orientation_xyzw();
        let rows = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - z * w),
                2.0 * (x * z + y * w),
            ],
            [
                2.0 * (x * y + z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - x * w),
            ],
            [
                2.0 * (x * z - y * w),
                2.0 * (y * z + x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ];
        let transform = f64_matrix_to_f32([
            [
                scale_x * rows[0][0],
                scale_y * rows[1][0],
                0.5 * rows[2][0],
                0.0,
            ],
            [
                scale_x * rows[0][1],
                scale_y * rows[1][1],
                0.5 * rows[2][1],
                0.0,
            ],
            [
                scale_x * rows[0][2],
                scale_y * rows[1][2],
                0.5 * rows[2][2],
                0.0,
            ],
            [0.0, 0.0, 0.5, 1.0],
        ])?;
        Self::with_transform(packet, transform)
    }

    fn with_transform(
        packet: &SphericalGpuPacket,
        transform: [[f32; 4]; 4],
    ) -> Result<Self, SphericalRenderError> {
        let palette_len = u32::try_from(packet.layers().fill_palette().len()).map_err(|_| {
            SphericalRenderError::IntegerOverflow {
                context: "fill palette length",
            }
        })?;
        let diagnostic_info_index = palette_len;
        let diagnostic_warning_index =
            checked_u32_add(palette_len, 1, "diagnostic warning palette index")?;
        let diagnostic_error_index =
            checked_u32_add(palette_len, 2, "diagnostic error palette index")?;
        let (display_min, display_max, field_kind) = match packet.layers().fill().kind() {
            PreparedFieldKind::Scalar => {
                let (min, max) = packet
                    .layers()
                    .fill()
                    .display_range()
                    .ok_or(SphericalRenderError::MissingDisplayRange)?
                    .bounds();
                (min, max, 0)
            }
            PreparedFieldKind::Category => (0.0, 1.0, 1),
        };
        Ok(Self {
            transform,
            display_min,
            display_max,
            field_kind,
            palette_len,
            diagnostics_enabled: u32::from(packet.layers().diagnostics_enabled()),
            diagnostic_info_index,
            diagnostic_warning_index,
            diagnostic_error_index,
        })
    }
}

fn validated_viewport(viewport: [u32; 2]) -> Result<[f64; 2], SphericalRenderError> {
    if viewport[0] == 0 || viewport[1] == 0 {
        return Err(SphericalRenderError::InvalidViewport);
    }
    Ok([f64::from(viewport[0]), f64::from(viewport[1])])
}

fn f64_matrix_to_f32(matrix: [[f64; 4]; 4]) -> Result<[[f32; 4]; 4], SphericalRenderError> {
    let mut converted = [[0.0_f32; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            let value = matrix[column][row] as f32;
            if !value.is_finite() {
                return Err(SphericalRenderError::InvalidCamera);
            }
            converted[column][row] = value;
        }
    }
    Ok(converted)
}

/// One GPU-neutral source-bound packet for both spherical fill pipelines.
#[derive(Debug, Clone)]
pub struct SphericalGpuPacket {
    map: Arc<PreparedProjectedMap>,
    map_geometry_revision: DisplayRevision,
    globe: Arc<PreparedGlobeMesh>,
    globe_geometry_revision: DisplayRevision,
    layers: Arc<PreparedFieldLayers>,
}

impl SphericalGpuPacket {
    /// Binds independently revised map/globe geometry to one shared field packet.
    #[allow(dead_code)]
    pub(crate) const fn new(
        map: Arc<PreparedProjectedMap>,
        map_geometry_revision: DisplayRevision,
        globe: Arc<PreparedGlobeMesh>,
        globe_geometry_revision: DisplayRevision,
        layers: Arc<PreparedFieldLayers>,
    ) -> Self {
        Self {
            map,
            map_geometry_revision,
            globe,
            globe_geometry_revision,
            layers,
        }
    }

    /// Returns the source identity shared by every valid packet component.
    pub fn source(&self) -> &SphericalPresentationSource {
        self.layers.source()
    }

    /// Returns the projected map geometry.
    pub fn map(&self) -> &PreparedProjectedMap {
        &self.map
    }

    /// Returns the projected map geometry revision.
    pub const fn map_geometry_revision(&self) -> DisplayRevision {
        self.map_geometry_revision
    }

    /// Returns the undeformed globe geometry.
    pub fn globe(&self) -> &PreparedGlobeMesh {
        &self.globe
    }

    /// Returns the undeformed globe geometry revision.
    pub const fn globe_geometry_revision(&self) -> DisplayRevision {
        self.globe_geometry_revision
    }

    /// Returns the shared prepared fill/diagnostic/palette packet.
    pub fn layers(&self) -> &PreparedFieldLayers {
        &self.layers
    }

    /// Returns the shared prepared field allocation.
    pub const fn layers_arc(&self) -> &Arc<PreparedFieldLayers> {
        &self.layers
    }
}

/// The independent geometry pipeline selected for one spherical paint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SphericalRenderMode {
    /// Paint the seam-safe projected map.
    #[default]
    Map,
    /// Paint the undeformed unit globe.
    Globe,
}

/// Errors detected before an installed spherical GPU packet is replaced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SphericalRenderError {
    /// One packet component was derived from a different authoritative source.
    #[error("{resource} has a different spherical presentation source")]
    SourceMismatch {
        /// Stable name of the mismatched component.
        resource: &'static str,
    },
    /// A source-bound resource has a different cell cardinality.
    #[error(
        "{resource} cardinality {actual} does not match spherical geometry cardinality {expected}"
    )]
    CardinalityMismatch {
        /// Stable name of the rejected resource.
        resource: &'static str,
        /// Required authoritative cell cardinality.
        expected: usize,
        /// Rejected cardinality.
        actual: usize,
    },
    /// A GPU buffer would exceed a renderer or device byte limit.
    #[error("{resource} require {required} GPU buffer bytes, limit is {max}")]
    BufferLimitExceeded {
        /// Stable name of the rejected resource.
        resource: &'static str,
        /// Required byte length.
        required: u64,
        /// Maximum permitted byte length.
        max: u64,
    },
    /// An element count could not be represented as GPU bytes.
    #[error("{resource} GPU buffer size overflow")]
    BufferSizeOverflow {
        /// Stable name of the rejected resource.
        resource: &'static str,
    },
    /// Checked renderer arithmetic overflowed.
    #[error("integer overflow while computing {context}")]
    IntegerOverflow {
        /// Stable context for the failed calculation.
        context: &'static str,
    },
    /// A scalar fill unexpectedly lacked an active display range.
    #[error("scalar spherical fill has no display range")]
    MissingDisplayRange,
    /// A viewport dimension was zero.
    #[error("spherical GPU viewport dimensions must be non-zero")]
    InvalidViewport,
    /// A camera transform was non-finite after checked conversion.
    #[error("spherical GPU camera transform must be finite")]
    InvalidCamera,
    /// Prepared geometry violated an indexed GPU layout invariant.
    #[error("{resource} contains invalid spherical GPU geometry")]
    InvalidGeometry {
        /// Stable name of the invalid geometry resource.
        resource: &'static str,
    },
}

/// Cumulative evidence of immutable spherical uploads and fixed uniform writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SphericalUploadCounters {
    /// Successful projected-map geometry upload batches.
    pub map_geometry: u64,
    /// Successful unit-globe geometry upload batches.
    pub globe_geometry: u64,
    /// Successful packed cell-fill upload batches.
    pub fill_field: u64,
    /// Successful diagnostic-mask upload batches.
    pub diagnostics: u64,
    /// Successful combined fill/diagnostic palette upload batches.
    pub palettes: u64,
    /// Fixed-size camera/mode uniform writes.
    pub uniforms: u64,
    /// Total bytes submitted by successful uploads and uniform writes.
    pub uploaded_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InstalledRevisions {
    map_geometry: DisplayRevision,
    globe_geometry: DisplayRevision,
    fill: DisplayRevision,
    diagnostics: DisplayRevision,
    palette: DisplayRevision,
}

impl From<&SphericalGpuPacket> for InstalledRevisions {
    fn from(packet: &SphericalGpuPacket) -> Self {
        Self {
            map_geometry: packet.map_geometry_revision(),
            globe_geometry: packet.globe_geometry_revision(),
            fill: packet.layers().revisions().fill,
            diagnostics: packet.layers().revisions().diagnostics,
            palette: packet.layers().revisions().fill_palette,
        }
    }
}

#[derive(Debug, Clone)]
struct InstalledPacketKey {
    source: SphericalPresentationSource,
    revisions: InstalledRevisions,
    map: Arc<PreparedProjectedMap>,
    globe: Arc<PreparedGlobeMesh>,
    layers: Arc<PreparedFieldLayers>,
}

impl InstalledPacketKey {
    fn for_packet(packet: &SphericalGpuPacket) -> Self {
        Self {
            source: packet.source().clone(),
            revisions: InstalledRevisions::from(packet),
            map: Arc::clone(&packet.map),
            globe: Arc::clone(&packet.globe),
            layers: Arc::clone(&packet.layers),
        }
    }

    fn exactly_matches(&self, packet: &SphericalGpuPacket) -> bool {
        self.source == *packet.source()
            && self.revisions == InstalledRevisions::from(packet)
            && Arc::ptr_eq(&self.map, &packet.map)
            && Arc::ptr_eq(&self.globe, &packet.globe)
            && Arc::ptr_eq(&self.layers, &packet.layers)
    }
}

#[derive(Debug, Clone, Copy)]
struct UploadPlan {
    map_geometry: bool,
    globe_geometry: bool,
    fill: bool,
    diagnostics: bool,
    palette: bool,
}

impl UploadPlan {
    fn between(
        installed_source: Option<&SphericalPresentationSource>,
        installed: Option<InstalledRevisions>,
        packet: &SphericalGpuPacket,
    ) -> Self {
        if installed_source != Some(packet.source()) {
            return Self {
                map_geometry: true,
                globe_geometry: true,
                fill: true,
                diagnostics: true,
                palette: true,
            };
        }
        let next = InstalledRevisions::from(packet);
        Self {
            map_geometry: installed.is_none_or(|old| old.map_geometry != next.map_geometry),
            globe_geometry: installed.is_none_or(|old| old.globe_geometry != next.globe_geometry),
            fill: installed.is_none_or(|old| old.fill != next.fill),
            diagnostics: installed.is_none_or(|old| old.diagnostics != next.diagnostics),
            palette: installed.is_none_or(|old| old.palette != next.palette),
        }
    }
}

struct ReplacementBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
}

/// Independent projected-map and unit-globe fill renderer sharing one field packet.
pub struct SphericalFieldRenderer {
    map_vertex_buffer: wgpu::Buffer,
    map_vertex_capacity: u64,
    map_index_buffer: wgpu::Buffer,
    map_index_capacity: u64,
    globe_vertex_buffer: wgpu::Buffer,
    globe_vertex_capacity: u64,
    globe_index_buffer: wgpu::Buffer,
    globe_index_capacity: u64,
    fill_buffer: wgpu::Buffer,
    fill_capacity: u64,
    diagnostic_buffer: wgpu::Buffer,
    diagnostic_capacity: u64,
    palette_buffer: wgpu::Buffer,
    palette_capacity: u64,
    map_uniform_buffer: wgpu::Buffer,
    globe_uniform_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    map_bind_group: wgpu::BindGroup,
    globe_bind_group: wgpu::BindGroup,
    map_pipeline: wgpu::RenderPipeline,
    globe_pipeline: wgpu::RenderPipeline,
    installed_source: Option<SphericalPresentationSource>,
    installed_revisions: Option<InstalledRevisions>,
    installed_packet_key: Option<InstalledPacketKey>,
    map_index_count: u32,
    globe_index_count: u32,
    counters: SphericalUploadCounters,
    frame_generation: u64,
}

impl SphericalFieldRenderer {
    /// Creates an empty renderer for one color target format.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let map_vertex_buffer = create_buffer(
            device,
            "Spherical Map Vertices",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        let map_index_buffer = create_buffer(
            device,
            "Spherical Map Indices",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );
        let globe_vertex_buffer = create_buffer(
            device,
            "Spherical Globe Vertices",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        let globe_index_buffer = create_buffer(
            device,
            "Spherical Globe Indices",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        );
        let fill_buffer = create_buffer(
            device,
            "Spherical Fill Values",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let diagnostic_buffer = create_buffer(
            device,
            "Spherical Diagnostics",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let palette_buffer = create_buffer(
            device,
            "Spherical Fill Palette",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let map_uniform_buffer = create_buffer(
            device,
            "Spherical Map Uniform",
            std::mem::size_of::<SphericalFrameUniform>() as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let globe_uniform_buffer = create_buffer(
            device,
            "Spherical Globe Uniform",
            std::mem::size_of::<SphericalFrameUniform>() as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let bind_group_layout = create_bind_group_layout(device);
        let map_bind_group = create_bind_group(
            device,
            &bind_group_layout,
            &fill_buffer,
            &diagnostic_buffer,
            &palette_buffer,
            &map_uniform_buffer,
            "Spherical Map Fill Bind Group",
        );
        let globe_bind_group = create_bind_group(
            device,
            &bind_group_layout,
            &fill_buffer,
            &diagnostic_buffer,
            &palette_buffer,
            &globe_uniform_buffer,
            "Spherical Globe Fill Bind Group",
        );
        let map_pipeline = create_pipeline(
            device,
            target_format,
            &bind_group_layout,
            SphericalRenderMode::Map,
        );
        let globe_pipeline = create_pipeline(
            device,
            target_format,
            &bind_group_layout,
            SphericalRenderMode::Globe,
        );
        Self {
            map_vertex_buffer,
            map_vertex_capacity: MIN_BUFFER_BYTES,
            map_index_buffer,
            map_index_capacity: MIN_BUFFER_BYTES,
            globe_vertex_buffer,
            globe_vertex_capacity: MIN_BUFFER_BYTES,
            globe_index_buffer,
            globe_index_capacity: MIN_BUFFER_BYTES,
            fill_buffer,
            fill_capacity: MIN_BUFFER_BYTES,
            diagnostic_buffer,
            diagnostic_capacity: MIN_BUFFER_BYTES,
            palette_buffer,
            palette_capacity: MIN_BUFFER_BYTES,
            map_uniform_buffer,
            globe_uniform_buffer,
            bind_group_layout,
            map_bind_group,
            globe_bind_group,
            map_pipeline,
            globe_pipeline,
            installed_source: None,
            installed_revisions: None,
            installed_packet_key: None,
            map_index_count: 0,
            globe_index_count: 0,
            counters: SphericalUploadCounters::default(),
            frame_generation: 0,
        }
    }

    /// Validates and atomically installs every revision-changed immutable resource.
    pub fn prepare_packet(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
    ) -> Result<(), SphericalRenderError> {
        self.prepare_packet_with_limits(device, queue, packet, device.limits())
    }

    fn prepare_packet_with_limits(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
        limits: wgpu::Limits,
    ) -> Result<(), SphericalRenderError> {
        if self
            .installed_packet_key
            .as_ref()
            .is_some_and(|key| key.exactly_matches(packet))
        {
            return Ok(());
        }
        validate_packet(packet)?;
        let plan = UploadPlan::between(
            self.installed_source.as_ref(),
            self.installed_revisions,
            packet,
        );
        let map_vertices = plan.map_geometry.then(|| {
            packet
                .map()
                .vertices()
                .iter()
                .map(|vertex| GpuMapVertex {
                    position: [vertex.position().x() as f32, vertex.position().y() as f32],
                    cell: vertex.cell().raw(),
                })
                .collect::<Vec<_>>()
        });
        let globe_vertices = plan.globe_geometry.then(|| {
            packet
                .globe()
                .vertices()
                .iter()
                .map(|vertex| GpuGlobeVertex {
                    position: vertex.position(),
                    cell: vertex.cell().raw(),
                })
                .collect::<Vec<_>>()
        });
        let palette = plan
            .palette
            .then(|| combined_palette(packet.layers().fill_palette()))
            .transpose()?;
        let sizes = BufferSizes::for_packet(packet, limits.clone())?;
        let map_index_count = checked_u32(packet.map().indices().len(), "map index count")?;
        let globe_index_count = checked_u32(packet.globe().indices().len(), "globe index count")?;
        let next_counters = preflight_counters(self.counters, plan, sizes)?;

        let new_map_vertex = replacement_buffer(
            device,
            "Spherical Map Vertices",
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            self.map_vertex_capacity,
            sizes.map_vertices,
            limits.max_buffer_size,
            plan.map_geometry,
        )?;
        let new_map_index = replacement_buffer(
            device,
            "Spherical Map Indices",
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            self.map_index_capacity,
            sizes.map_indices,
            limits.max_buffer_size,
            plan.map_geometry,
        )?;
        let new_globe_vertex = replacement_buffer(
            device,
            "Spherical Globe Vertices",
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            self.globe_vertex_capacity,
            sizes.globe_vertices,
            limits.max_buffer_size,
            plan.globe_geometry,
        )?;
        let new_globe_index = replacement_buffer(
            device,
            "Spherical Globe Indices",
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            self.globe_index_capacity,
            sizes.globe_indices,
            limits.max_buffer_size,
            plan.globe_geometry,
        )?;
        let storage_limit =
            u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        let new_fill = replacement_buffer(
            device,
            "Spherical Fill Values",
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            self.fill_capacity,
            sizes.fill,
            storage_limit,
            plan.fill,
        )?;
        let new_diagnostic = replacement_buffer(
            device,
            "Spherical Diagnostics",
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            self.diagnostic_capacity,
            sizes.diagnostics,
            storage_limit,
            plan.diagnostics,
        )?;
        let new_palette = replacement_buffer(
            device,
            "Spherical Fill Palette",
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            self.palette_capacity,
            sizes.palette,
            storage_limit,
            plan.palette,
        )?;
        let shared_binding_changed =
            new_fill.is_some() || new_diagnostic.is_some() || new_palette.is_some();
        let next_bind_groups = shared_binding_changed.then(|| {
            let fill = new_fill
                .as_ref()
                .map_or(&self.fill_buffer, |replacement| &replacement.buffer);
            let diagnostics = new_diagnostic
                .as_ref()
                .map_or(&self.diagnostic_buffer, |replacement| &replacement.buffer);
            let palette = new_palette
                .as_ref()
                .map_or(&self.palette_buffer, |replacement| &replacement.buffer);
            (
                create_bind_group(
                    device,
                    &self.bind_group_layout,
                    fill,
                    diagnostics,
                    palette,
                    &self.map_uniform_buffer,
                    "Spherical Map Fill Bind Group",
                ),
                create_bind_group(
                    device,
                    &self.bind_group_layout,
                    fill,
                    diagnostics,
                    palette,
                    &self.globe_uniform_buffer,
                    "Spherical Globe Fill Bind Group",
                ),
            )
        });

        if let Some(vertices) = &map_vertices {
            write_if_nonempty(
                queue,
                new_map_vertex
                    .as_ref()
                    .map_or(&self.map_vertex_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(vertices),
            );
            write_if_nonempty(
                queue,
                new_map_index
                    .as_ref()
                    .map_or(&self.map_index_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(packet.map().indices()),
            );
        }
        if let Some(vertices) = &globe_vertices {
            write_if_nonempty(
                queue,
                new_globe_vertex
                    .as_ref()
                    .map_or(&self.globe_vertex_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(vertices),
            );
            write_if_nonempty(
                queue,
                new_globe_index
                    .as_ref()
                    .map_or(&self.globe_index_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(packet.globe().indices()),
            );
        }
        if plan.fill {
            write_if_nonempty(
                queue,
                new_fill
                    .as_ref()
                    .map_or(&self.fill_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(packet.layers().fill().raw_values()),
            );
        }
        if plan.diagnostics {
            write_if_nonempty(
                queue,
                new_diagnostic
                    .as_ref()
                    .map_or(&self.diagnostic_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(packet.layers().diagnostics().cells()),
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

        apply_replacement(
            &mut self.map_vertex_buffer,
            &mut self.map_vertex_capacity,
            new_map_vertex,
        );
        apply_replacement(
            &mut self.map_index_buffer,
            &mut self.map_index_capacity,
            new_map_index,
        );
        apply_replacement(
            &mut self.globe_vertex_buffer,
            &mut self.globe_vertex_capacity,
            new_globe_vertex,
        );
        apply_replacement(
            &mut self.globe_index_buffer,
            &mut self.globe_index_capacity,
            new_globe_index,
        );
        apply_replacement(&mut self.fill_buffer, &mut self.fill_capacity, new_fill);
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
        if let Some((map, globe)) = next_bind_groups {
            self.map_bind_group = map;
            self.globe_bind_group = globe;
        }
        self.installed_source = Some(packet.source().clone());
        self.installed_revisions = Some(InstalledRevisions::from(packet));
        self.installed_packet_key = Some(InstalledPacketKey::for_packet(packet));
        self.map_index_count = map_index_count;
        self.globe_index_count = globe_index_count;
        self.counters = next_counters;
        Ok(())
    }

    /// Writes one fixed-size mode-specific camera/value uniform.
    pub(super) fn prepare_frame(
        &mut self,
        queue: &wgpu::Queue,
        mode: SphericalRenderMode,
        uniform: &SphericalFrameUniform,
    ) -> Result<u64, SphericalRenderError> {
        let mut next = self.counters;
        next.uniforms = checked_counter(next.uniforms, 1, "uniform upload counter")?;
        next.uploaded_bytes = checked_counter(
            next.uploaded_bytes,
            std::mem::size_of::<SphericalFrameUniform>() as u64,
            "uploaded byte counter",
        )?;
        let next_generation =
            checked_counter(self.frame_generation, 1, "frame generation counter")?;
        let buffer = match mode {
            SphericalRenderMode::Map => &self.map_uniform_buffer,
            SphericalRenderMode::Globe => &self.globe_uniform_buffer,
        };
        queue.write_buffer(buffer, 0, bytemuck::bytes_of(uniform));
        self.counters = next;
        self.frame_generation = next_generation;
        Ok(next_generation)
    }

    /// Draws the last successfully installed packet through the selected pipeline.
    pub fn paint(&self, mode: SphericalRenderMode, pass: &mut wgpu::RenderPass<'static>) {
        match mode {
            SphericalRenderMode::Map => {
                if self.map_index_count == 0 {
                    return;
                }
                pass.set_pipeline(&self.map_pipeline);
                pass.set_bind_group(0, &self.map_bind_group, &[]);
                pass.set_vertex_buffer(0, self.map_vertex_buffer.slice(..));
                pass.set_index_buffer(self.map_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.map_index_count, 0, 0..1);
            }
            SphericalRenderMode::Globe => {
                if self.globe_index_count == 0 {
                    return;
                }
                pass.set_pipeline(&self.globe_pipeline);
                pass.set_bind_group(0, &self.globe_bind_group, &[]);
                pass.set_vertex_buffer(0, self.globe_vertex_buffer.slice(..));
                pass.set_index_buffer(self.globe_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.globe_index_count, 0, 0..1);
            }
        }
    }

    /// Returns cumulative successful immutable and uniform upload evidence.
    pub const fn upload_counters(&self) -> SphericalUploadCounters {
        self.counters
    }

    /// Returns the source identity of the last completely installed packet.
    pub const fn installed_source(&self) -> Option<&SphericalPresentationSource> {
        self.installed_source.as_ref()
    }

    pub(super) const fn is_frame_current(&self, generation: u64) -> bool {
        generation != 0 && self.frame_generation == generation
    }

    pub(super) fn paint_if_current(
        &self,
        generation: u64,
        mode: SphericalRenderMode,
        pass: &mut wgpu::RenderPass<'static>,
    ) -> bool {
        if !self.is_frame_current(generation) {
            return false;
        }
        self.paint(mode, pass);
        true
    }

    #[cfg(test)]
    fn paint_for_test(
        &mut self,
        queue: &wgpu::Queue,
        mode: SphericalRenderMode,
        uniform: &SphericalFrameUniform,
    ) -> u64 {
        self.prepare_frame(queue, mode, uniform).unwrap()
    }
}

#[derive(Debug, Clone, Copy)]
struct BufferSizes {
    map_vertices: u64,
    map_indices: u64,
    globe_vertices: u64,
    globe_indices: u64,
    fill: u64,
    diagnostics: u64,
    palette: u64,
}

impl BufferSizes {
    fn for_packet(
        packet: &SphericalGpuPacket,
        limits: wgpu::Limits,
    ) -> Result<Self, SphericalRenderError> {
        let sizes = Self {
            map_vertices: checked_buffer_bytes::<GpuMapVertex>(
                packet.map().vertices().len(),
                "map vertices",
            )?,
            map_indices: checked_buffer_bytes::<u32>(packet.map().indices().len(), "map indices")?,
            globe_vertices: checked_buffer_bytes::<GpuGlobeVertex>(
                packet.globe().vertices().len(),
                "globe vertices",
            )?,
            globe_indices: checked_buffer_bytes::<u32>(
                packet.globe().indices().len(),
                "globe indices",
            )?,
            fill: checked_buffer_bytes::<u32>(packet.layers().fill().len(), "fill field")?,
            diagnostics: checked_buffer_bytes::<u32>(
                packet.layers().diagnostics().len(),
                "diagnostics",
            )?,
            palette: checked_buffer_bytes::<[f32; 4]>(
                packet.layers().fill_palette().len().checked_add(3).ok_or(
                    SphericalRenderError::IntegerOverflow {
                        context: "combined palette length",
                    },
                )?,
                "fill palette",
            )?,
        };
        let storage_limit =
            u64::from(limits.max_storage_buffer_binding_size).min(limits.max_buffer_size);
        for (resource, required, max) in [
            ("map vertices", sizes.map_vertices, limits.max_buffer_size),
            ("map indices", sizes.map_indices, limits.max_buffer_size),
            (
                "globe vertices",
                sizes.globe_vertices,
                limits.max_buffer_size,
            ),
            ("globe indices", sizes.globe_indices, limits.max_buffer_size),
            ("fill field", sizes.fill, storage_limit),
            ("diagnostics", sizes.diagnostics, storage_limit),
            ("fill palette", sizes.palette, storage_limit),
        ] {
            if required.max(MIN_BUFFER_BYTES) > max {
                return Err(SphericalRenderError::BufferLimitExceeded {
                    resource,
                    required: required.max(MIN_BUFFER_BYTES),
                    max,
                });
            }
        }
        Ok(sizes)
    }
}

fn validate_packet(packet: &SphericalGpuPacket) -> Result<(), SphericalRenderError> {
    #[cfg(test)]
    validation_probe::full_validation();
    if packet.map().source() != packet.source() {
        return Err(SphericalRenderError::SourceMismatch {
            resource: "projected map",
        });
    }
    if packet.globe().source() != packet.source() {
        return Err(SphericalRenderError::SourceMismatch {
            resource: "unit globe",
        });
    }
    let cell_count = packet.map().cell_count();
    validate_cardinality("unit globe", cell_count, packet.globe().cell_count())?;
    validate_cardinality("fill field", cell_count, packet.layers().fill().len())?;
    validate_cardinality(
        "diagnostics",
        cell_count,
        packet.layers().diagnostics().len(),
    )?;
    validate_geometry(
        "projected map",
        packet.map().vertices().len(),
        packet
            .map()
            .vertices()
            .iter()
            .map(|vertex| vertex.cell().raw()),
        packet.map().indices(),
        cell_count,
    )?;
    validate_geometry(
        "unit globe",
        packet.globe().vertices().len(),
        packet
            .globe()
            .vertices()
            .iter()
            .map(|vertex| vertex.cell().raw()),
        packet.globe().indices(),
        cell_count,
    )?;
    if packet.layers().fill_palette().is_empty()
        || packet
            .layers()
            .fill_palette()
            .len()
            .checked_add(3)
            .is_none_or(|combined| combined > MAX_PALETTE_ENTRIES)
    {
        return Err(SphericalRenderError::InvalidGeometry {
            resource: "fill palette",
        });
    }
    if packet.map().vertices().iter().any(|vertex| {
        #[cfg(test)]
        validation_probe::position();
        let point = vertex.position();
        !point.x().is_finite() || !point.y().is_finite()
    }) || packet.globe().vertices().iter().any(|vertex| {
        #[cfg(test)]
        validation_probe::position();
        vertex
            .position()
            .into_iter()
            .any(|component| !component.is_finite())
    }) {
        return Err(SphericalRenderError::InvalidGeometry {
            resource: "vertex positions",
        });
    }
    Ok(())
}

fn validate_cardinality(
    resource: &'static str,
    expected: usize,
    actual: usize,
) -> Result<(), SphericalRenderError> {
    if actual != expected {
        return Err(SphericalRenderError::CardinalityMismatch {
            resource,
            expected,
            actual,
        });
    }
    Ok(())
}

fn validate_geometry(
    resource: &'static str,
    vertex_count: usize,
    mut cells: impl Iterator<Item = u32>,
    indices: &[u32],
    cell_count: usize,
) -> Result<(), SphericalRenderError> {
    if cells.any(|cell| {
        #[cfg(test)]
        validation_probe::cell_id();
        cell as usize >= cell_count
    }) || indices.iter().any(|&index| {
        #[cfg(test)]
        validation_probe::index();
        index as usize >= vertex_count
    }) {
        return Err(SphericalRenderError::InvalidGeometry { resource });
    }
    checked_u32(vertex_count, "vertex count")?;
    checked_u32(indices.len(), "index count")?;
    Ok(())
}

fn combined_palette(base: &[LinearRgba]) -> Result<Vec<[f32; 4]>, SphericalRenderError> {
    let capacity = base
        .len()
        .checked_add(3)
        .ok_or(SphericalRenderError::IntegerOverflow {
            context: "combined palette length",
        })?;
    let mut combined = Vec::with_capacity(capacity);
    combined.extend(base.iter().map(|color| color.components()));
    combined.push(DIAGNOSTIC_INFO_COLOR.components());
    combined.push(DIAGNOSTIC_WARNING_COLOR.components());
    combined.push(DIAGNOSTIC_ERROR_COLOR.components());
    Ok(combined)
}

fn checked_buffer_bytes<T>(
    len: usize,
    resource: &'static str,
) -> Result<u64, SphericalRenderError> {
    len.checked_mul(std::mem::size_of::<T>())
        .and_then(|bytes| u64::try_from(bytes).ok())
        .ok_or(SphericalRenderError::BufferSizeOverflow { resource })
}

fn checked_u32(value: usize, context: &'static str) -> Result<u32, SphericalRenderError> {
    u32::try_from(value).map_err(|_| SphericalRenderError::IntegerOverflow { context })
}

fn checked_u32_add(
    left: u32,
    right: u32,
    context: &'static str,
) -> Result<u32, SphericalRenderError> {
    left.checked_add(right)
        .ok_or(SphericalRenderError::IntegerOverflow { context })
}

fn checked_counter(
    current: u64,
    amount: u64,
    context: &'static str,
) -> Result<u64, SphericalRenderError> {
    current
        .checked_add(amount)
        .ok_or(SphericalRenderError::IntegerOverflow { context })
}

fn preflight_counters(
    current: SphericalUploadCounters,
    plan: UploadPlan,
    sizes: BufferSizes,
) -> Result<SphericalUploadCounters, SphericalRenderError> {
    let mut next = current;
    if plan.map_geometry {
        next.map_geometry = checked_counter(next.map_geometry, 1, "map geometry upload counter")?;
    }
    if plan.globe_geometry {
        next.globe_geometry =
            checked_counter(next.globe_geometry, 1, "globe geometry upload counter")?;
    }
    if plan.fill {
        next.fill_field = checked_counter(next.fill_field, 1, "fill upload counter")?;
    }
    if plan.diagnostics {
        next.diagnostics = checked_counter(next.diagnostics, 1, "diagnostic upload counter")?;
    }
    if plan.palette {
        next.palettes = checked_counter(next.palettes, 1, "palette upload counter")?;
    }
    let submitted = [
        plan.map_geometry.then_some(sizes.map_vertices),
        plan.map_geometry.then_some(sizes.map_indices),
        plan.globe_geometry.then_some(sizes.globe_vertices),
        plan.globe_geometry.then_some(sizes.globe_indices),
        plan.fill.then_some(sizes.fill),
        plan.diagnostics.then_some(sizes.diagnostics),
        plan.palette.then_some(sizes.palette),
    ]
    .into_iter()
    .flatten()
    .try_fold(0_u64, |total, bytes| {
        total
            .checked_add(bytes)
            .ok_or(SphericalRenderError::IntegerOverflow {
                context: "uploaded byte count",
            })
    })?;
    next.uploaded_bytes = checked_counter(next.uploaded_bytes, submitted, "uploaded byte counter")?;
    Ok(next)
}

fn next_buffer_capacity(
    required: u64,
    current: u64,
    max: u64,
    resource: &'static str,
) -> Result<u64, SphericalRenderError> {
    let required = required.max(MIN_BUFFER_BYTES);
    if required > max {
        return Err(SphericalRenderError::BufferLimitExceeded {
            resource,
            required,
            max,
        });
    }
    if current >= required {
        return Ok(current);
    }
    let grown = required.checked_next_power_of_two().unwrap_or(max).min(max);
    if grown < required {
        return Err(SphericalRenderError::BufferLimitExceeded {
            resource,
            required,
            max,
        });
    }
    Ok(grown)
}

fn replacement_buffer(
    device: &wgpu::Device,
    label: &'static str,
    usage: wgpu::BufferUsages,
    current: u64,
    required: u64,
    max: u64,
    changed: bool,
) -> Result<Option<ReplacementBuffer>, SphericalRenderError> {
    if !changed {
        return Ok(None);
    }
    let capacity = next_buffer_capacity(required, current, max, label)?;
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
        label: Some("Spherical Fill Bind Group Layout"),
        entries: &[
            storage_layout_entry(0),
            storage_layout_entry(1),
            storage_layout_entry(2),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
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
    fill: &wgpu::Buffer,
    diagnostics: &wgpu::Buffer,
    palette: &wgpu::Buffer,
    uniform: &wgpu::Buffer,
    label: &'static str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: fill.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: diagnostics.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: palette.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: uniform.as_entire_binding(),
            },
        ],
    })
}

fn create_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
    mode: SphericalRenderMode,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Spherical Field Shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../../../assets/shaders/spherical_field.wgsl").into(),
        ),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Spherical Field Pipeline Layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });
    const MAP_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Uint32];
    const GLOBE_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32];
    let (label, entry_point, array_stride, attributes, cull_mode) = match mode {
        SphericalRenderMode::Map => (
            "Spherical Map Fill Pipeline",
            "vs_map",
            std::mem::size_of::<GpuMapVertex>() as u64,
            &MAP_ATTRIBUTES[..],
            None,
        ),
        SphericalRenderMode::Globe => (
            "Spherical Globe Fill Pipeline",
            "vs_globe",
            std::mem::size_of::<GpuGlobeVertex>() as u64,
            &GLOBE_ATTRIBUTES[..],
            Some(wgpu::Face::Back),
        ),
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some(entry_point),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes,
            }],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_fill"),
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
            cull_mode,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{mpsc, Arc};

    use super::{
        validation_probe, wgpu, GpuGlobeVertex, GpuMapVertex, SphericalFieldRenderer,
        SphericalFrameUniform, SphericalGpuPacket, SphericalRenderError, SphericalRenderMode,
    };
    use crate::engine::BuildResultHash;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::view::{
        category_color, prepare_spherical_field_layers, scalar_color, DisplayRangeMode,
        DisplayRevision, DisplayRevisionClock, FieldCatalog, GlobeCamera, LinearRgba, MapCamera,
        OwnedViewDiagnostic, PaletteId, PreparedFieldKind, PreparedGlobeMesh, PreparedProjectedMap,
        SphericalFieldDisplayState, SphericalMeshBudgets, SphericalPresentationSource,
        SphericalProjection, SphericalProjectionKind, ViewDiagnosticSeverity,
        DIAGNOSTIC_ERROR_COLOR,
    };
    use crate::world::fields::{
        DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain, FieldId,
        FieldPaletteHint, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
        MissingValuePolicy, ValueRange,
    };
    use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    #[test]
    fn spherical_gpu_layouts_match_wgsl_vertex_and_uniform_contracts() {
        assert_eq!(std::mem::size_of::<GpuMapVertex>(), 12);
        assert_eq!(std::mem::size_of::<GpuGlobeVertex>(), 16);
        assert_eq!(std::mem::size_of::<SphericalFrameUniform>(), 96);
    }

    #[test]
    fn packet_owns_independent_geometry_and_one_shared_field_layers_arc() {
        let fixture = packet_fixture(TestFieldKind::Scalar, 7);

        assert!(Arc::ptr_eq(fixture.packet.layers_arc(), &fixture.layers));
        assert_eq!(fixture.packet.map().source(), fixture.packet.source());
        assert_eq!(fixture.packet.globe().source(), fixture.packet.source());
        assert_eq!(fixture.packet.layers().source(), fixture.packet.source());
        assert_eq!(fixture.packet.map_geometry_revision().get(), 101);
        assert_eq!(fixture.packet.globe_geometry_revision().get(), 102);
        assert_eq!(
            fixture.packet.layers().fill().kind(),
            PreparedFieldKind::Scalar
        );
    }

    #[test]
    fn static_frames_and_camera_or_mode_changes_upload_only_fixed_uniforms() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = packet_fixture(TestFieldKind::Scalar, 7);
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);

        validation_probe::reset();
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let after_first_validation = validation_probe::snapshot();
        assert_eq!(after_first_validation.full_validations, 1);
        assert!(after_first_validation.cell_ids > 0);
        assert!(after_first_validation.indices > 0);
        assert!(after_first_validation.positions > 0);
        let after_upload = renderer.upload_counters();
        assert_eq!(after_upload.map_geometry, 1);
        assert_eq!(after_upload.globe_geometry, 1);
        assert_eq!(after_upload.fill_field, 1);
        assert_eq!(after_upload.diagnostics, 1);
        assert_eq!(after_upload.palettes, 1);
        assert_eq!(after_upload.uniforms, 0);
        let map_uniform =
            SphericalFrameUniform::for_map(&fixture.packet, MapCamera::default(), [256, 128])
                .unwrap();
        let globe_uniform =
            SphericalFrameUniform::for_globe(&fixture.packet, GlobeCamera::default(), [128, 128])
                .unwrap();
        let mut rotated = GlobeCamera::default();
        assert!(rotated.trackball_drag([32.0, 64.0], [96.0, 64.0], [128.0, 128.0]));
        assert!(rotated.zoom_by(1.25));
        let rotated_uniform =
            SphericalFrameUniform::for_globe(&fixture.packet, rotated, [128, 128]).unwrap();

        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let map_generation =
            renderer.paint_for_test(&queue, SphericalRenderMode::Map, &map_uniform);
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let globe_generation =
            renderer.paint_for_test(&queue, SphericalRenderMode::Globe, &globe_uniform);
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let rotated_generation =
            renderer.paint_for_test(&queue, SphericalRenderMode::Globe, &rotated_uniform);

        let after_frames = renderer.upload_counters();
        let after_static_frames = validation_probe::snapshot();
        assert_eq!(after_static_frames, after_first_validation);
        assert_eq!(after_frames.map_geometry, after_upload.map_geometry);
        assert_eq!(after_frames.globe_geometry, after_upload.globe_geometry);
        assert_eq!(after_frames.fill_field, after_upload.fill_field);
        assert_eq!(after_frames.diagnostics, after_upload.diagnostics);
        assert_eq!(after_frames.palettes, after_upload.palettes);
        assert_eq!(after_frames.uniforms, after_upload.uniforms + 3);
        assert!(!renderer.is_frame_current(map_generation));
        assert!(!renderer.is_frame_current(globe_generation));
        assert!(renderer.is_frame_current(rotated_generation));
        eprintln!("validation work after first install: {after_first_validation:?}");
        eprintln!("validation work after static frames: {after_static_frames:?}");
        eprintln!("after immutable upload: {after_upload:?}");
        eprintln!("after camera/mode frames: {after_frames:?}");
    }

    #[test]
    fn rejected_source_candidate_preserves_installed_packet_and_all_counters() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let installed = packet_fixture(TestFieldKind::Scalar, 7);
        let foreign = packet_fixture(TestFieldKind::Category, 8);
        let rejected = SphericalGpuPacket::new(
            Arc::clone(&foreign.packet.map),
            foreign.packet.map_geometry_revision,
            Arc::clone(&foreign.packet.globe),
            foreign.packet.globe_geometry_revision,
            Arc::clone(&installed.layers),
        );
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        validation_probe::reset();
        renderer
            .prepare_packet(&device, &queue, &installed.packet)
            .unwrap();
        let baseline = prepare_and_read_map(&device, &queue, &mut renderer, &installed.packet);
        let installed_source = renderer.installed_source().cloned();
        let before = renderer.upload_counters();
        let scans_before = validation_probe::snapshot();
        assert_eq!(
            rejected.map_geometry_revision(),
            installed.packet.map_geometry_revision()
        );
        assert_eq!(
            rejected.globe_geometry_revision(),
            installed.packet.globe_geometry_revision()
        );
        assert!(!Arc::ptr_eq(&rejected.map, &installed.packet.map));
        assert!(!Arc::ptr_eq(&rejected.globe, &installed.packet.globe));

        let error = renderer
            .prepare_packet(&device, &queue, &rejected)
            .unwrap_err();

        assert_eq!(
            error,
            SphericalRenderError::SourceMismatch {
                resource: "projected map"
            }
        );
        assert_eq!(renderer.installed_source(), installed_source.as_ref());
        assert_eq!(renderer.upload_counters(), before);
        let scans_after = validation_probe::snapshot();
        assert_eq!(
            scans_after.full_validations,
            scans_before.full_validations + 1
        );
        assert_eq!(scans_after.cell_ids, scans_before.cell_ids);
        assert_eq!(scans_after.indices, scans_before.indices);
        assert_eq!(scans_after.positions, scans_before.positions);
        assert_eq!(
            readback_renderer(
                &device,
                &queue,
                &renderer,
                SphericalRenderMode::Map,
                [256, 128],
            ),
            baseline
        );
    }

    #[test]
    fn rejected_cardinality_candidate_preserves_installed_packet_and_all_counters() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let installed = packet_fixture(TestFieldKind::Scalar, 12);
        let short_layers = layers_for_count(
            installed.packet.source().clone(),
            installed.packet.map().cell_count() - 1,
            installed.surface.edges().len(),
            TestFieldKind::Scalar,
        );
        let rejected = SphericalGpuPacket::new(
            Arc::clone(&installed.packet.map),
            installed.packet.map_geometry_revision,
            Arc::clone(&installed.packet.globe),
            installed.packet.globe_geometry_revision,
            short_layers,
        );
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer
            .prepare_packet(&device, &queue, &installed.packet)
            .unwrap();
        let baseline = prepare_and_read_map(&device, &queue, &mut renderer, &installed.packet);
        let installed_source = renderer.installed_source().cloned();
        let before = renderer.upload_counters();

        let error = renderer
            .prepare_packet(&device, &queue, &rejected)
            .unwrap_err();

        assert_eq!(
            error,
            SphericalRenderError::CardinalityMismatch {
                resource: "fill field",
                expected: installed.packet.map().cell_count(),
                actual: installed.packet.map().cell_count() - 1,
            }
        );
        assert_eq!(renderer.installed_source(), installed_source.as_ref());
        assert_eq!(renderer.upload_counters(), before);
        assert_eq!(
            readback_renderer(
                &device,
                &queue,
                &renderer,
                SphericalRenderMode::Map,
                [256, 128],
            ),
            baseline
        );
    }

    #[test]
    fn rejected_byte_limit_candidate_preserves_installed_packet_and_all_counters() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = packet_fixture(TestFieldKind::Scalar, 13);
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let baseline = prepare_and_read_map(&device, &queue, &mut renderer, &fixture.packet);
        let installed_source = renderer.installed_source().cloned();
        let before = renderer.upload_counters();
        let mut rejected = fixture.packet.clone();
        rejected.map_geometry_revision = DisplayRevision::new(103).unwrap();
        let mut rejected_limits = device.limits();
        rejected_limits.max_buffer_size = 64;
        rejected_limits.max_storage_buffer_binding_size = 64;

        let error = renderer
            .prepare_packet_with_limits(&device, &queue, &rejected, rejected_limits)
            .unwrap_err();

        assert!(matches!(
            error,
            SphericalRenderError::BufferLimitExceeded { .. }
        ));
        assert_eq!(renderer.installed_source(), installed_source.as_ref());
        assert_eq!(renderer.upload_counters(), before);
        assert_eq!(
            readback_renderer(
                &device,
                &queue,
                &renderer,
                SphericalRenderMode::Map,
                [256, 128],
            ),
            baseline
        );
    }

    #[test]
    fn offscreen_map_and_unlit_globe_scalar_and_category_match_cpu_colors() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        for kind in [TestFieldKind::Scalar, TestFieldKind::Category] {
            let fixture = packet_fixture(kind, 9);
            for (mode, viewport) in [
                (SphericalRenderMode::Map, [256, 128]),
                (SphericalRenderMode::Globe, [128, 128]),
            ] {
                let (rgba8, uniform) =
                    render_offscreen(&device, &queue, &fixture.packet, mode, viewport);
                let samples = sample_cells(&fixture, mode);
                assert_eq!(
                    samples.len(),
                    4,
                    "one front-facing sample per fixture color"
                );
                for cell in samples {
                    let position = match mode {
                        SphericalRenderMode::Map => {
                            let point = fixture
                                .packet
                                .map()
                                .projection()
                                .forward(fixture.surface.cells()[cell].centroid)
                                .unwrap();
                            [point.x() as f32, point.y() as f32, 0.0, 1.0]
                        }
                        SphericalRenderMode::Globe => {
                            let [x, y, z] = fixture.surface.cells()[cell]
                                .centroid
                                .components()
                                .map(|value| value as f32);
                            [x, y, z, 1.0]
                        }
                    };
                    let pixel = transformed_pixel(uniform.transform, position, viewport);
                    let expected = expected_cell_rgba8(&fixture.packet, cell);
                    assert_pixel_near(&rgba8, viewport, pixel, expected, 2);
                }
            }
        }
    }

    #[test]
    fn offscreen_globe_culls_back_facing_cell_triangles() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = packet_fixture(TestFieldKind::Category, 10);
        let viewport = [128, 128];
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let uniform =
            SphericalFrameUniform::for_globe(&fixture.packet, GlobeCamera::default(), viewport)
                .unwrap();
        renderer
            .prepare_frame(&queue, SphericalRenderMode::Globe, &uniform)
            .unwrap();
        let back_triangle = [
            GpuGlobeVertex {
                position: [-0.5, -0.5, -0.5],
                cell: 0,
            },
            GpuGlobeVertex {
                position: [0.0, 0.5, -0.5],
                cell: 0,
            },
            GpuGlobeVertex {
                position: [0.5, -0.5, -0.5],
                cell: 0,
            },
        ];
        queue.write_buffer(
            &renderer.globe_vertex_buffer,
            0,
            bytemuck::cast_slice(&back_triangle),
        );
        queue.write_buffer(
            &renderer.globe_index_buffer,
            0,
            bytemuck::cast_slice(&[0_u32, 1, 2]),
        );
        renderer.globe_index_count = 3;

        let rgba8 = readback_renderer(
            &device,
            &queue,
            &renderer,
            SphericalRenderMode::Globe,
            viewport,
        );
        assert!(
            rgba8.chunks_exact(4).all(|pixel| pixel[3] == 0),
            "a back-facing triangle must not write any pixels"
        );
    }

    #[test]
    fn offscreen_diagnostic_overlay_replaces_fill_color_in_both_modes() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = packet_fixture_with_diagnostic(TestFieldKind::Scalar, 11, true);
        let cell = fixture
            .diagnostic_cell
            .expect("diagnostic fixture selects a front-facing cell");
        let expected = linear_to_srgba8(DIAGNOSTIC_ERROR_COLOR);
        for (mode, viewport) in [
            (SphericalRenderMode::Map, [256, 128]),
            (SphericalRenderMode::Globe, [128, 128]),
        ] {
            let (rgba8, uniform) =
                render_offscreen(&device, &queue, &fixture.packet, mode, viewport);
            let position = match mode {
                SphericalRenderMode::Map => {
                    let point = fixture
                        .packet
                        .map()
                        .projection()
                        .forward(fixture.surface.cells()[cell].centroid)
                        .unwrap();
                    [point.x() as f32, point.y() as f32, 0.0, 1.0]
                }
                SphericalRenderMode::Globe => {
                    let [x, y, z] = fixture.surface.cells()[cell]
                        .centroid
                        .components()
                        .map(|value| value as f32);
                    [x, y, z, 1.0]
                }
            };
            let pixel = transformed_pixel(uniform.transform, position, viewport);
            assert_pixel_near(&rgba8, viewport, pixel, expected, 2);
        }
    }

    struct PacketFixture {
        packet: SphericalGpuPacket,
        layers: Arc<crate::view::PreparedFieldLayers>,
        surface: SphericalSurfaceSnapshot,
        diagnostic_cell: Option<usize>,
    }

    #[derive(Clone, Copy)]
    enum TestFieldKind {
        Scalar,
        Category,
    }

    fn request_test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        pollster::block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    force_fallback_adapter: true,
                    compatible_surface: None,
                })
                .await;
            let adapter = match adapter {
                Some(adapter) => adapter,
                None => {
                    let adapter = instance
                        .request_adapter(&wgpu::RequestAdapterOptions {
                            power_preference: wgpu::PowerPreference::LowPower,
                            force_fallback_adapter: false,
                            compatible_surface: None,
                        })
                        .await;
                    let Some(adapter) = adapter else {
                        return gpu_unavailable("no fallback or hardware adapter is available");
                    };
                    adapter
                }
            };
            match adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("Spherical Field Test Device"),
                        required_limits: wgpu::Limits::downlevel_defaults(),
                        ..Default::default()
                    },
                    None,
                )
                .await
            {
                Ok(device) => Some(device),
                Err(error) => gpu_unavailable(&format!("fallback device request failed: {error}")),
            }
        })
    }

    fn gpu_unavailable<T>(reason: &str) -> Option<T> {
        if std::env::var("SEKAI_REQUIRE_SPHERICAL_GPU").as_deref() == Ok("1") {
            panic!("spherical GPU evidence is required: {reason}");
        }
        eprintln!("skipping optional spherical GPU test: {reason}");
        None
    }

    fn packet_fixture(kind: TestFieldKind, seed: u64) -> PacketFixture {
        packet_fixture_with_diagnostic(kind, seed, false)
    }

    fn packet_fixture_with_diagnostic(
        kind: TestFieldKind,
        seed: u64,
        with_diagnostic: bool,
    ) -> PacketFixture {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let source = SphericalPresentationSource::new(
            RootSeed::new(seed),
            SurfaceRef::for_spherical(&surface),
            BuildResultHash::new([seed as u8; 32]),
            1,
        );
        let projection =
            SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.0).unwrap();
        let map = Arc::new(
            PreparedProjectedMap::build(
                source.clone(),
                &surface,
                projection,
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap(),
        );
        let globe = Arc::new(
            PreparedGlobeMesh::build(source.clone(), &surface, SphericalMeshBudgets::DEFAULT)
                .unwrap(),
        );
        let cell_count = surface.cells().len();
        let (schema, data, _palette) = test_field(kind, cell_count);
        let field_id = schema.id.clone();
        let mut registry = FieldRegistryBuilder::new();
        registry.register(schema).unwrap();
        let registry = registry.build().unwrap();
        let mut fields = ExtensionFieldSet::new();
        fields
            .insert(
                &registry,
                field_id.clone(),
                data,
                &DomainSizes::new(cell_count, surface.edges().len()),
            )
            .unwrap();
        let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
        let mut state = SphericalFieldDisplayState::default();
        let mut clock = DisplayRevisionClock::default();
        let diagnostic_cell = with_diagnostic.then(|| {
            surface
                .cells()
                .iter()
                .position(|cell| cell.centroid.components()[2] > 0.5)
                .expect("fixture has a front-facing cell")
        });
        let diagnostics = diagnostic_cell
            .map(|cell| OwnedViewDiagnostic {
                severity: ViewDiagnosticSeverity::Error,
                code: "test.spherical.gpu".into(),
                field_id: Some(field_id.clone()),
                cell_id: Some(CellId::from_raw(cell as u32)),
                message: "test diagnostic".into(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let layers = Arc::new(
            prepare_spherical_field_layers(
                source,
                &catalog,
                cell_count,
                surface.edges().len(),
                &diagnostics,
                Some(field_id),
                |_| Some(DisplayRangeMode::Schema),
                &mut state,
                &mut clock,
            )
            .unwrap(),
        );
        let packet = SphericalGpuPacket::new(
            map,
            DisplayRevision::new(101).unwrap(),
            globe,
            DisplayRevision::new(102).unwrap(),
            Arc::clone(&layers),
        );
        PacketFixture {
            packet,
            layers,
            surface,
            diagnostic_cell,
        }
    }

    fn layers_for_count(
        source: SphericalPresentationSource,
        cell_count: usize,
        edge_count: usize,
        kind: TestFieldKind,
    ) -> Arc<crate::view::PreparedFieldLayers> {
        let (schema, data, _palette) = test_field(kind, cell_count);
        let field_id = schema.id.clone();
        let mut registry = FieldRegistryBuilder::new();
        registry.register(schema).unwrap();
        let registry = registry.build().unwrap();
        let mut fields = ExtensionFieldSet::new();
        fields
            .insert(
                &registry,
                field_id.clone(),
                data,
                &DomainSizes::new(cell_count, edge_count),
            )
            .unwrap();
        let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
        let mut state = SphericalFieldDisplayState::default();
        let mut clock = DisplayRevisionClock::default();
        Arc::new(
            prepare_spherical_field_layers(
                source,
                &catalog,
                cell_count,
                edge_count,
                &[],
                Some(field_id),
                |_| Some(DisplayRangeMode::Schema),
                &mut state,
                &mut clock,
            )
            .unwrap(),
        )
    }

    fn render_offscreen(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
        mode: SphericalRenderMode,
        viewport: [u32; 2],
    ) -> (Vec<u8>, SphericalFrameUniform) {
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut renderer = SphericalFieldRenderer::new(device, format);
        renderer.prepare_packet(device, queue, packet).unwrap();
        let uniform = match mode {
            SphericalRenderMode::Map => {
                SphericalFrameUniform::for_map(packet, MapCamera::default(), viewport).unwrap()
            }
            SphericalRenderMode::Globe => {
                SphericalFrameUniform::for_globe(packet, GlobeCamera::default(), viewport).unwrap()
            }
        };
        renderer.prepare_frame(queue, mode, &uniform).unwrap();
        let rgba8 = readback_renderer(device, queue, &renderer, mode, viewport);
        (rgba8, uniform)
    }

    fn prepare_and_read_map(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut SphericalFieldRenderer,
        packet: &SphericalGpuPacket,
    ) -> Vec<u8> {
        let viewport = [256, 128];
        let uniform =
            SphericalFrameUniform::for_map(packet, MapCamera::default(), viewport).unwrap();
        renderer
            .prepare_frame(queue, SphericalRenderMode::Map, &uniform)
            .unwrap();
        readback_renderer(device, queue, renderer, SphericalRenderMode::Map, viewport)
    }

    fn readback_renderer(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &SphericalFieldRenderer,
        mode: SphericalRenderMode,
        viewport: [u32; 2],
    ) -> Vec<u8> {
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let extent = wgpu::Extent3d {
            width: viewport[0],
            height: viewport[1],
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Spherical Field Test Target"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let unpadded_bytes_per_row = viewport[0] * 4;
        let padded_bytes_per_row = unpadded_bytes_per_row
            .div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Spherical Field Test Readback"),
            size: u64::from(padded_bytes_per_row) * u64::from(viewport[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Spherical Field Test Encoder"),
        });
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Spherical Field Test Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            renderer.paint(mode, &mut pass.forget_lifetime());
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(viewport[1]),
                },
            },
            extent,
        );
        queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).expect("mapping receiver is alive");
        });
        let _ = device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .expect("mapping callback runs")
            .expect("readback maps");
        let mapped = slice.get_mapped_range();
        let mut rgba8 = vec![0; unpadded_bytes_per_row as usize * viewport[1] as usize];
        for row in 0..viewport[1] as usize {
            let source_start = row * padded_bytes_per_row as usize;
            let target_start = row * unpadded_bytes_per_row as usize;
            rgba8[target_start..target_start + unpadded_bytes_per_row as usize].copy_from_slice(
                &mapped[source_start..source_start + unpadded_bytes_per_row as usize],
            );
        }
        drop(mapped);
        readback.unmap();
        rgba8
    }

    fn sample_cells(fixture: &PacketFixture, mode: SphericalRenderMode) -> Vec<usize> {
        let mut samples = Vec::new();
        for color in 0..4 {
            let candidate = fixture
                .surface
                .cells()
                .iter()
                .enumerate()
                .filter(|(index, _)| index % 4 == color)
                .filter(|(_, cell)| match mode {
                    SphericalRenderMode::Map => {
                        let point = fixture
                            .packet
                            .map()
                            .projection()
                            .forward(cell.centroid)
                            .unwrap();
                        let bounds = fixture.packet.map().bounds();
                        point.x() > bounds.min_x() + 0.05 * (bounds.max_x() - bounds.min_x())
                            && point.x() < bounds.max_x() - 0.05 * (bounds.max_x() - bounds.min_x())
                    }
                    SphericalRenderMode::Globe => cell.centroid.components()[2] > 0.35,
                })
                .map(|(index, _)| index)
                .next();
            if let Some(candidate) = candidate {
                samples.push(candidate);
            }
        }
        samples
    }

    fn expected_cell_rgba8(packet: &SphericalGpuPacket, cell: usize) -> [u8; 4] {
        let raw = packet.layers().fill().raw_values()[cell];
        let color = match packet.layers().fill().kind() {
            PreparedFieldKind::Scalar => scalar_color(
                f32::from_bits(raw),
                packet.layers().fill().display_range().unwrap(),
                packet.layers().fill_palette(),
            ),
            PreparedFieldKind::Category => category_color(raw, packet.layers().fill_palette()),
        };
        linear_to_srgba8(color)
    }

    fn transformed_pixel(
        matrix: [[f32; 4]; 4],
        position: [f32; 4],
        viewport: [u32; 2],
    ) -> [i32; 2] {
        let mut clip = [0.0_f32; 4];
        for row in 0..4 {
            clip[row] = (0..4)
                .map(|column| matrix[column][row] * position[column])
                .sum();
        }
        let ndc_x = clip[0] / clip[3];
        let ndc_y = clip[1] / clip[3];
        [
            ((ndc_x + 1.0) * 0.5 * viewport[0] as f32).floor() as i32,
            ((1.0 - ndc_y) * 0.5 * viewport[1] as f32).floor() as i32,
        ]
    }

    fn assert_pixel_near(
        rgba8: &[u8],
        viewport: [u32; 2],
        pixel: [i32; 2],
        expected: [u8; 4],
        tolerance: u8,
    ) {
        let found = (-2..=2).any(|dy| {
            (-2..=2).any(|dx| {
                let x = pixel[0] + dx;
                let y = pixel[1] + dy;
                if x < 0 || y < 0 || x >= viewport[0] as i32 || y >= viewport[1] as i32 {
                    return false;
                }
                let offset = (y as usize * viewport[0] as usize + x as usize) * 4;
                (0..4)
                    .all(|channel| rgba8[offset + channel].abs_diff(expected[channel]) <= tolerance)
            })
        });
        assert!(found, "no pixel near {pixel:?} matched {expected:?}");
    }

    fn linear_to_srgba8(color: LinearRgba) -> [u8; 4] {
        let [red, green, blue, alpha] = color.components();
        [
            linear_channel_to_srgb8(red),
            linear_channel_to_srgb8(green),
            linear_channel_to_srgb8(blue),
            unit_to_u8(alpha),
        ]
    }

    fn linear_channel_to_srgb8(linear: f32) -> u8 {
        let linear = linear.clamp(0.0, 1.0);
        let srgb = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        unit_to_u8(srgb)
    }

    fn unit_to_u8(value: f32) -> u8 {
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    fn test_field(kind: TestFieldKind, cell_count: usize) -> (FieldSchema, FieldData, PaletteId) {
        match kind {
            TestFieldKind::Scalar => (
                FieldSchema {
                    id: FieldId::new("test.spherical.gpu", "scalar", 1).unwrap(),
                    domain: FieldDomain::Cells,
                    value_type: FieldValueType::ScalarF32,
                    unit: FieldUnit::Unitless,
                    valid_range: Some(ValueRange::new(0.0, 1.0).unwrap()),
                    missing: MissingValuePolicy::Forbidden,
                    dependencies: Vec::new(),
                    category_labels: BTreeMap::new(),
                    display: FieldDisplayMetadata::new(
                        "field.test.spherical.gpu.scalar",
                        FieldPaletteHint::Sequential,
                        4,
                    )
                    .unwrap(),
                },
                FieldData::ScalarF32(
                    (0..cell_count)
                        .map(|index| [0.0, 0.35, 0.7, 1.0][index % 4])
                        .collect(),
                ),
                PaletteId::Sequential,
            ),
            TestFieldKind::Category => (
                FieldSchema {
                    id: FieldId::new("test.spherical.gpu", "category", 1).unwrap(),
                    domain: FieldDomain::Cells,
                    value_type: FieldValueType::CategoryU32,
                    unit: FieldUnit::Unitless,
                    valid_range: None,
                    missing: MissingValuePolicy::Forbidden,
                    dependencies: Vec::new(),
                    category_labels: BTreeMap::from([
                        (10, "field.test.category.ten".into()),
                        (20, "field.test.category.twenty".into()),
                        (30, "field.test.category.thirty".into()),
                        (40, "field.test.category.forty".into()),
                    ]),
                    display: FieldDisplayMetadata::new(
                        "field.test.spherical.gpu.category",
                        FieldPaletteHint::Categorical,
                        0,
                    )
                    .unwrap(),
                },
                FieldData::CategoryU32(
                    (0..cell_count)
                        .map(|index| [10, 20, 30, 40][index % 4])
                        .collect(),
                ),
                PaletteId::Categorical,
            ),
        }
    }
}
