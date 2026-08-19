use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::wgpu;
use std::sync::Arc;
use thiserror::Error;

use super::overlay::{
    prepare_globe_overlay_instances, prepare_map_overlay_instances, GpuGlobeOverlayInstance,
    GpuMapOverlayInstance, PreparedGlobeOverlayInstances, PreparedMapOverlayInstances,
};
use crate::view::{
    DisplayRevision, GlobeCamera, LinearRgba, MapCamera, OwnedViewDiagnostic, PreparedFieldKind,
    PreparedFieldLayers, PreparedGlobeMesh, PreparedProjectedMap, PreparedSphericalOverlay,
    SphericalLayerVisibility, SphericalPresentationSource, VectorAnimationUniform,
    DIAGNOSTIC_ERROR_COLOR, DIAGNOSTIC_INFO_COLOR, DIAGNOSTIC_WARNING_COLOR,
};

const MIN_BUFFER_BYTES: u64 = 16;
const MAX_PALETTE_ENTRIES: usize = 65_536;

#[cfg(test)]
pub(crate) mod validation_probe {
    use std::cell::Cell;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(crate) struct ScanCounts {
        pub full_validations: u64,
        pub cell_ids: u64,
        pub indices: u64,
        pub positions: u64,
    }

    thread_local! {
        static COUNTS: Cell<ScanCounts> = Cell::new(ScanCounts::default());
    }

    pub(crate) fn reset() {
        COUNTS.set(ScanCounts::default());
    }

    pub(crate) fn snapshot() -> ScanCounts {
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
    direction: [f32; 3],
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
    viewport_pixels: [f32; 2],
    vector_phase: f32,
    globe_silhouette_clip: u32,
    fill_visible: u32,
    overlay_visible: u32,
    amplified_mode: u32,
    _padding: u32,
}

impl SphericalFrameUniform {
    #[cfg(test)]
    pub(super) fn for_map(
        packet: &SphericalGpuPacket,
        camera: MapCamera,
        viewport: [u32; 2],
    ) -> Result<Self, SphericalRenderError> {
        Self::for_map_with_animation(packet, camera, viewport, VectorAnimationUniform::default())
    }

    pub(super) fn for_map_with_animation(
        packet: &SphericalGpuPacket,
        camera: MapCamera,
        viewport: [u32; 2],
        animation: VectorAnimationUniform,
    ) -> Result<Self, SphericalRenderError> {
        Self::for_map_with_animation_and_visibility(
            packet,
            camera,
            viewport,
            animation,
            SphericalLayerVisibility::default(),
        )
    }

    #[cfg(test)]
    pub(super) fn for_map_with_visibility(
        packet: &SphericalGpuPacket,
        camera: MapCamera,
        viewport: [u32; 2],
        visibility: SphericalLayerVisibility,
    ) -> Result<Self, SphericalRenderError> {
        Self::for_map_with_animation_and_visibility(
            packet,
            camera,
            viewport,
            VectorAnimationUniform::default(),
            visibility,
        )
    }

    pub(super) fn for_map_with_animation_and_visibility(
        packet: &SphericalGpuPacket,
        camera: MapCamera,
        viewport: [u32; 2],
        animation: VectorAnimationUniform,
        visibility: SphericalLayerVisibility,
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
        Self::with_transform(
            packet,
            transform,
            [width as f32, height as f32],
            animation,
            false,
            visibility,
        )
    }

    #[cfg(test)]
    pub(super) fn for_globe(
        packet: &SphericalGpuPacket,
        camera: GlobeCamera,
        viewport: [u32; 2],
    ) -> Result<Self, SphericalRenderError> {
        Self::for_globe_with_animation(packet, camera, viewport, VectorAnimationUniform::default())
    }

    pub(super) fn for_globe_with_animation(
        packet: &SphericalGpuPacket,
        camera: GlobeCamera,
        viewport: [u32; 2],
        animation: VectorAnimationUniform,
    ) -> Result<Self, SphericalRenderError> {
        Self::for_globe_with_animation_and_visibility(
            packet,
            camera,
            viewport,
            animation,
            SphericalLayerVisibility::default(),
        )
    }

    #[cfg(test)]
    pub(super) fn for_globe_with_visibility(
        packet: &SphericalGpuPacket,
        camera: GlobeCamera,
        viewport: [u32; 2],
        visibility: SphericalLayerVisibility,
    ) -> Result<Self, SphericalRenderError> {
        Self::for_globe_with_animation_and_visibility(
            packet,
            camera,
            viewport,
            VectorAnimationUniform::default(),
            visibility,
        )
    }

    pub(super) fn for_globe_with_animation_and_visibility(
        packet: &SphericalGpuPacket,
        camera: GlobeCamera,
        viewport: [u32; 2],
        animation: VectorAnimationUniform,
        visibility: SphericalLayerVisibility,
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
        Self::with_transform(
            packet,
            transform,
            [width as f32, height as f32],
            animation,
            true,
            visibility,
        )
    }

    fn with_transform(
        packet: &SphericalGpuPacket,
        transform: [[f32; 4]; 4],
        viewport_pixels: [f32; 2],
        animation: VectorAnimationUniform,
        globe_silhouette_clip: bool,
        visibility: SphericalLayerVisibility,
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
            viewport_pixels,
            vector_phase: animation.phase(),
            globe_silhouette_clip: u32::from(globe_silhouette_clip),
            fill_visible: u32::from(visibility.fill),
            overlay_visible: u32::from(visibility.overlay),
            amplified_mode: u32::from(visibility.amplified),
            _padding: 0,
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

    /// Validates and returns a GPU-neutral packet before any renderer allocation occurs.
    pub(crate) fn try_new(
        map: Arc<PreparedProjectedMap>,
        map_geometry_revision: DisplayRevision,
        globe: Arc<PreparedGlobeMesh>,
        globe_geometry_revision: DisplayRevision,
        layers: Arc<PreparedFieldLayers>,
    ) -> Result<Self, SphericalRenderError> {
        let packet = Self::new(
            map,
            map_geometry_revision,
            globe,
            globe_geometry_revision,
            layers,
        );
        validate_packet(&packet)?;
        Ok(packet)
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
    /// A fixed-size frame uniform named a packet that is not currently installed.
    #[error("spherical frame packet is not the currently installed GPU packet")]
    FramePacketNotInstalled,
    /// An initial publication targeted a renderer that already owns another publication packet.
    #[error("spherical renderer is already initialized by a publication")]
    RendererAlreadyInitialized,
    /// A replacement publication targeted a renderer whose current packet belongs elsewhere.
    #[error("spherical renderer does not contain the publication's expected current packet")]
    RendererCurrentPacketMismatch,
    /// One changed packet component reused or regressed its installed revision.
    #[error(
        "{resource} changed without a newer spherical display revision (installed {installed}, candidate {candidate})"
    )]
    RevisionNotAdvanced {
        /// Stable name of the changed component.
        resource: &'static str,
        /// Revision currently installed for that component.
        installed: u64,
        /// Revision supplied by the rejected candidate.
        candidate: u64,
    },
    /// Prepared geometry violated an indexed GPU layout invariant.
    #[error("{resource} contains invalid spherical GPU geometry")]
    InvalidGeometry {
        /// Stable name of the invalid geometry resource.
        resource: &'static str,
    },
    /// Source-bound overlay instances could not be prepared coherently.
    #[error("spherical overlay instances are invalid")]
    InvalidOverlayInstances,
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
    /// Successful projected-map edge/vector instance upload batches.
    pub map_overlay_instances: u64,
    /// Successful unit-globe edge/vector instance upload batches.
    pub globe_overlay_instances: u64,
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
    overlay: DisplayRevision,
    overlay_palette: DisplayRevision,
    vector_glyphs: DisplayRevision,
}

impl From<&SphericalGpuPacket> for InstalledRevisions {
    fn from(packet: &SphericalGpuPacket) -> Self {
        Self {
            map_geometry: packet.map_geometry_revision(),
            globe_geometry: packet.globe_geometry_revision(),
            fill: packet.layers().revisions().fill,
            diagnostics: packet.layers().revisions().diagnostics,
            palette: packet.layers().revisions().fill_palette,
            overlay: packet.layers().revisions().overlay,
            overlay_palette: packet.layers().revisions().overlay_palette,
            vector_glyphs: packet.layers().revisions().vector_glyphs,
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

    fn validate_revision_progress(
        &self,
        packet: &SphericalGpuPacket,
    ) -> Result<(), SphericalRenderError> {
        if self.source != *packet.source() {
            return Ok(());
        }
        let next = InstalledRevisions::from(packet);
        for (resource, changed, installed, candidate) in [
            (
                "projected map",
                !Arc::ptr_eq(&self.map, &packet.map),
                self.revisions.map_geometry,
                next.map_geometry,
            ),
            (
                "unit globe",
                !Arc::ptr_eq(&self.globe, &packet.globe),
                self.revisions.globe_geometry,
                next.globe_geometry,
            ),
            (
                "fill field",
                !Arc::ptr_eq(self.layers.fill_arc(), packet.layers.fill_arc()),
                self.revisions.fill,
                next.fill,
            ),
            (
                "diagnostics",
                !Arc::ptr_eq(
                    self.layers.diagnostics_arc(),
                    packet.layers.diagnostics_arc(),
                ),
                self.revisions.diagnostics,
                next.diagnostics,
            ),
            (
                "fill palette",
                !Arc::ptr_eq(
                    self.layers.fill_palette_arc(),
                    packet.layers.fill_palette_arc(),
                ),
                self.revisions.palette,
                next.palette,
            ),
            (
                "overlay field",
                !same_overlay_arc(self.layers.overlay(), packet.layers.overlay()),
                self.revisions.overlay,
                next.overlay,
            ),
            (
                "overlay palette",
                !same_optional_arc(
                    self.layers.overlay_palette_arc(),
                    packet.layers.overlay_palette_arc(),
                ),
                self.revisions.overlay_palette,
                next.overlay_palette,
            ),
            (
                "vector glyphs",
                vector_glyph_inputs_changed(&self.layers, packet.layers()),
                self.revisions.vector_glyphs,
                next.vector_glyphs,
            ),
        ] {
            if candidate < installed || (changed && candidate == installed) {
                return Err(SphericalRenderError::RevisionNotAdvanced {
                    resource,
                    installed: installed.get(),
                    candidate: candidate.get(),
                });
            }
        }
        Ok(())
    }
}

fn same_optional_arc<T>(left: Option<&Arc<T>>, right: Option<&Arc<T>>) -> bool
where
    T: ?Sized,
{
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

fn same_overlay_arc(
    left: Option<&PreparedSphericalOverlay>,
    right: Option<&PreparedSphericalOverlay>,
) -> bool {
    match (left, right) {
        (
            Some(PreparedSphericalOverlay::Edge(left)),
            Some(PreparedSphericalOverlay::Edge(right)),
        ) => Arc::ptr_eq(left, right),
        (
            Some(PreparedSphericalOverlay::Vector(left)),
            Some(PreparedSphericalOverlay::Vector(right)),
        ) => Arc::ptr_eq(left, right),
        (None, None) => true,
        (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) => false,
    }
}

fn vector_glyph_inputs_changed(
    installed: &PreparedFieldLayers,
    candidate: &PreparedFieldLayers,
) -> bool {
    let candidate_is_vector =
        candidate.overlay_kind() == Some(crate::view::PreparedOverlayKind::CellVector);
    candidate_is_vector
        && (installed.selected_vector_cell() != candidate.selected_vector_cell()
            || installed.glyph_lod_key() != candidate.glyph_lod_key())
}

#[derive(Debug, Clone, Copy)]
struct UploadPlan {
    map_geometry: bool,
    globe_geometry: bool,
    fill: bool,
    diagnostics: bool,
    palette: bool,
    map_overlay: bool,
    globe_overlay: bool,
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
                map_overlay: true,
                globe_overlay: true,
            };
        }
        let next = InstalledRevisions::from(packet);
        Self {
            map_geometry: installed.is_none_or(|old| old.map_geometry != next.map_geometry),
            globe_geometry: installed.is_none_or(|old| old.globe_geometry != next.globe_geometry),
            fill: installed.is_none_or(|old| old.fill != next.fill),
            diagnostics: installed.is_none_or(|old| old.diagnostics != next.diagnostics),
            palette: installed.is_none_or(|old| old.palette != next.palette),
            map_overlay: installed.is_none_or(|old| {
                old.map_geometry != next.map_geometry
                    || old.overlay != next.overlay
                    || old.overlay_palette != next.overlay_palette
                    || old.vector_glyphs != next.vector_glyphs
            }),
            globe_overlay: installed.is_none_or(|old| {
                old.globe_geometry != next.globe_geometry
                    || old.overlay != next.overlay
                    || old.overlay_palette != next.overlay_palette
                    || old.vector_glyphs != next.vector_glyphs
            }),
        }
    }
}

struct ReplacementBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
}

/// Independent projected-map and unit-globe fill renderer sharing one field packet.
///
/// Immutable packet installation is sealed behind the publication lineage boundary.
///
/// ```compile_fail
/// fn bypass_publication_lineage(
///     renderer: &mut sekai::gpu::spherical::SphericalFieldRenderer,
///     device: &eframe::egui_wgpu::wgpu::Device,
///     queue: &eframe::egui_wgpu::wgpu::Queue,
///     retained: &sekai::gpu::spherical::SphericalGpuPacket,
/// ) {
///     renderer.prepare_packet(device, queue, retained).unwrap();
/// }
/// ```
pub struct SphericalFieldRenderer {
    map_vertex_buffer: wgpu::Buffer,
    map_vertex_capacity: u64,
    map_index_buffer: wgpu::Buffer,
    map_index_capacity: u64,
    globe_vertex_buffer: wgpu::Buffer,
    globe_vertex_capacity: u64,
    globe_index_buffer: wgpu::Buffer,
    globe_index_capacity: u64,
    map_overlay_buffer: wgpu::Buffer,
    map_overlay_capacity: u64,
    globe_overlay_buffer: wgpu::Buffer,
    globe_overlay_capacity: u64,
    fill_buffer: wgpu::Buffer,
    fill_capacity: u64,
    diagnostic_buffer: wgpu::Buffer,
    diagnostic_capacity: u64,
    palette_buffer: wgpu::Buffer,
    palette_capacity: u64,
    map_uniform_buffer: wgpu::Buffer,
    globe_uniform_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    amplified_sampler: wgpu::Sampler,
    amplified_dummy_view: wgpu::TextureView,
    amplified_view: Option<wgpu::TextureView>,
    map_bind_group: wgpu::BindGroup,
    globe_bind_group: wgpu::BindGroup,
    map_pipeline: wgpu::RenderPipeline,
    globe_pipeline: wgpu::RenderPipeline,
    map_overlay_pipeline: wgpu::RenderPipeline,
    globe_overlay_pipeline: wgpu::RenderPipeline,
    installed_source: Option<SphericalPresentationSource>,
    installed_revisions: Option<InstalledRevisions>,
    installed_packet_key: Option<InstalledPacketKey>,
    map_index_count: u32,
    globe_index_count: u32,
    map_overlay_instance_count: u32,
    globe_overlay_instance_count: u32,
    installed_map_overlay: Option<Arc<PreparedMapOverlayInstances>>,
    installed_globe_overlay: Option<Arc<PreparedGlobeOverlayInstances>>,
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
        let map_overlay_buffer = create_buffer(
            device,
            "Spherical Map Overlay Instances",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        let globe_overlay_buffer = create_buffer(
            device,
            "Spherical Globe Overlay Instances",
            MIN_BUFFER_BYTES,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
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
        let amplified_sampler = create_amplified_sampler(device);
        let amplified_dummy_view = create_amplified_texture(device, 1, 1).1;
        let map_bind_group = create_bind_group(
            device,
            &bind_group_layout,
            &fill_buffer,
            &diagnostic_buffer,
            &palette_buffer,
            &map_uniform_buffer,
            &amplified_dummy_view,
            &amplified_sampler,
            "Spherical Map Fill Bind Group",
        );
        let globe_bind_group = create_bind_group(
            device,
            &bind_group_layout,
            &fill_buffer,
            &diagnostic_buffer,
            &palette_buffer,
            &globe_uniform_buffer,
            &amplified_dummy_view,
            &amplified_sampler,
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
        let map_overlay_pipeline = create_overlay_pipeline(
            device,
            target_format,
            &bind_group_layout,
            SphericalRenderMode::Map,
        );
        let globe_overlay_pipeline = create_overlay_pipeline(
            device,
            target_format,
            &bind_group_layout,
            SphericalRenderMode::Globe,
        );
        Self {
            amplified_sampler,
            amplified_dummy_view,
            amplified_view: None,
            map_vertex_buffer,
            map_vertex_capacity: MIN_BUFFER_BYTES,
            map_index_buffer,
            map_index_capacity: MIN_BUFFER_BYTES,
            globe_vertex_buffer,
            globe_vertex_capacity: MIN_BUFFER_BYTES,
            globe_index_buffer,
            globe_index_capacity: MIN_BUFFER_BYTES,
            map_overlay_buffer,
            map_overlay_capacity: MIN_BUFFER_BYTES,
            globe_overlay_buffer,
            globe_overlay_capacity: MIN_BUFFER_BYTES,
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
            map_overlay_pipeline,
            globe_overlay_pipeline,
            installed_source: None,
            installed_revisions: None,
            installed_packet_key: None,
            map_index_count: 0,
            globe_index_count: 0,
            map_overlay_instance_count: 0,
            globe_overlay_instance_count: 0,
            installed_map_overlay: None,
            installed_globe_overlay: None,
            counters: SphericalUploadCounters::default(),
            frame_generation: 0,
        }
    }

    /// Returns whether this renderer can accept one standalone initial publication.
    pub(crate) fn ensure_publication_uninitialized(&self) -> Result<(), SphericalRenderError> {
        if self.installed_packet_key.is_some() {
            return Err(SphericalRenderError::RendererAlreadyInitialized);
        }
        Ok(())
    }

    /// Requires the renderer to contain the exact packet owned by a publication.
    pub(crate) fn ensure_publication_current(
        &self,
        expected: &SphericalGpuPacket,
    ) -> Result<(), SphericalRenderError> {
        if !self.callback_packet_is_current(expected) {
            return Err(SphericalRenderError::RendererCurrentPacketMismatch);
        }
        Ok(())
    }

    /// Validates and atomically installs the first publication packet into an empty renderer.
    pub(crate) fn prepare_initial_publication_packet(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
    ) -> Result<(), SphericalRenderError> {
        self.ensure_publication_uninitialized()?;
        self.prepare_packet_with_limits(device, queue, packet, device.limits())
    }

    /// Atomically replaces the exact packet currently owned by one publication.
    pub(crate) fn prepare_replacement_publication_packet(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        expected: &SphericalGpuPacket,
        packet: &SphericalGpuPacket,
    ) -> Result<(), SphericalRenderError> {
        self.ensure_publication_current(expected)?;
        self.prepare_packet_with_limits(device, queue, packet, device.limits())
    }

    /// Test-only raw installation boundary for renderer unit and sealed crate fixtures.
    #[cfg(test)]
    pub(crate) fn prepare_packet(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
    ) -> Result<(), SphericalRenderError> {
        self.prepare_packet_with_limits(device, queue, packet, device.limits())
    }

    #[cfg(test)]
    pub(crate) fn prepare_map_frame_for_test(
        &mut self,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
        camera: MapCamera,
        viewport: [u32; 2],
    ) -> Result<u64, SphericalRenderError> {
        let uniform = SphericalFrameUniform::for_map(packet, camera, viewport)?;
        self.prepare_frame(queue, SphericalRenderMode::Map, &uniform)
    }

    #[cfg(test)]
    pub(crate) const fn frame_uniform_size_for_test() -> u64 {
        std::mem::size_of::<SphericalFrameUniform>() as u64
    }

    /// Prepares one fixed-size projected-map frame without rebuilding immutable resources.
    pub fn prepare_map_frame(
        &mut self,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
        camera: MapCamera,
        viewport: [u32; 2],
        animation: VectorAnimationUniform,
    ) -> Result<u64, SphericalRenderError> {
        if !self.callback_packet_is_current(packet) {
            return Err(SphericalRenderError::FramePacketNotInstalled);
        }
        let uniform =
            SphericalFrameUniform::for_map_with_animation(packet, camera, viewport, animation)?;
        self.prepare_frame(queue, SphericalRenderMode::Map, &uniform)
    }

    /// Prepares one fixed-size unit-globe frame without rebuilding immutable resources.
    pub fn prepare_globe_frame(
        &mut self,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
        camera: GlobeCamera,
        viewport: [u32; 2],
        animation: VectorAnimationUniform,
    ) -> Result<u64, SphericalRenderError> {
        if !self.callback_packet_is_current(packet) {
            return Err(SphericalRenderError::FramePacketNotInstalled);
        }
        let uniform =
            SphericalFrameUniform::for_globe_with_animation(packet, camera, viewport, animation)?;
        self.prepare_frame(queue, SphericalRenderMode::Globe, &uniform)
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
        if let Some(installed) = &self.installed_packet_key {
            installed.validate_revision_progress(packet)?;
        }
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
                    direction: vertex.direction(),
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
        let candidate_map_overlay = plan
            .map_overlay
            .then(|| prepare_map_overlay_instances(packet.map(), packet.globe(), packet.layers()))
            .transpose()
            .map_err(|_| SphericalRenderError::InvalidOverlayInstances)?
            .map(Arc::new);
        let candidate_globe_overlay = plan
            .globe_overlay
            .then(|| prepare_globe_overlay_instances(packet.globe(), packet.layers()))
            .transpose()
            .map_err(|_| SphericalRenderError::InvalidOverlayInstances)?
            .map(Arc::new);
        let map_overlay_instances = candidate_map_overlay
            .as_ref()
            .map(|prepared| prepared.instances.as_ref());
        let globe_overlay_instances = candidate_globe_overlay
            .as_ref()
            .map(|prepared| prepared.instances.as_ref());
        let sizes = BufferSizes::for_packet(
            packet,
            map_overlay_instances.unwrap_or_default().len(),
            globe_overlay_instances.unwrap_or_default().len(),
            limits.clone(),
        )?;
        let map_index_count = checked_u32(packet.map().indices().len(), "map index count")?;
        let globe_index_count = checked_u32(packet.globe().indices().len(), "globe index count")?;
        let map_overlay_instance_count = match map_overlay_instances {
            Some(instances) => checked_u32(instances.len(), "map overlay instance count")?,
            None => self.map_overlay_instance_count,
        };
        let globe_overlay_instance_count = match globe_overlay_instances {
            Some(instances) => checked_u32(instances.len(), "globe overlay instance count")?,
            None => self.globe_overlay_instance_count,
        };
        let next_counters = preflight_counters(
            self.counters,
            plan,
            sizes,
            candidate_map_overlay.is_some() && packet.layers().overlay().is_some(),
            candidate_globe_overlay.is_some() && packet.layers().overlay().is_some(),
        )?;

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
        let new_map_overlay = replacement_buffer(
            device,
            "Spherical Map Overlay Instances",
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            self.map_overlay_capacity,
            sizes.map_overlay,
            limits.max_buffer_size,
            plan.map_overlay,
        )?;
        let new_globe_overlay = replacement_buffer(
            device,
            "Spherical Globe Overlay Instances",
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            self.globe_overlay_capacity,
            sizes.globe_overlay,
            limits.max_buffer_size,
            plan.globe_overlay,
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
                    self.amplified_view
                        .as_ref()
                        .unwrap_or(&self.amplified_dummy_view),
                    &self.amplified_sampler,
                    "Spherical Map Fill Bind Group",
                ),
                create_bind_group(
                    device,
                    &self.bind_group_layout,
                    fill,
                    diagnostics,
                    palette,
                    &self.globe_uniform_buffer,
                    self.amplified_view
                        .as_ref()
                        .unwrap_or(&self.amplified_dummy_view),
                    &self.amplified_sampler,
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
        if let Some(instances) = map_overlay_instances {
            write_if_nonempty(
                queue,
                new_map_overlay
                    .as_ref()
                    .map_or(&self.map_overlay_buffer, |replacement| &replacement.buffer),
                bytemuck::cast_slice(instances),
            );
        }
        if let Some(instances) = globe_overlay_instances {
            write_if_nonempty(
                queue,
                new_globe_overlay
                    .as_ref()
                    .map_or(&self.globe_overlay_buffer, |replacement| {
                        &replacement.buffer
                    }),
                bytemuck::cast_slice(instances),
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
        apply_replacement(
            &mut self.map_overlay_buffer,
            &mut self.map_overlay_capacity,
            new_map_overlay,
        );
        apply_replacement(
            &mut self.globe_overlay_buffer,
            &mut self.globe_overlay_capacity,
            new_globe_overlay,
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
        self.map_overlay_instance_count = map_overlay_instance_count;
        self.globe_overlay_instance_count = globe_overlay_instance_count;
        if let Some(prepared) = candidate_map_overlay {
            self.installed_map_overlay = Some(prepared);
        }
        if let Some(prepared) = candidate_globe_overlay {
            self.installed_globe_overlay = Some(prepared);
        }
        self.counters = next_counters;
        Ok(())
    }

    /// Writes one fixed-size mode-specific camera/value uniform.
    /// Installs (or replaces) the amplified equirect texture and rebinds.
    pub fn set_amplified_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        rgba8: &[u8],
    ) {
        debug_assert_eq!((width as usize) * (height as usize) * 4, rgba8.len());
        let (texture, view) = create_amplified_texture(device, width, height);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba8,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.amplified_view = Some(view);
        self.rebuild_fill_bind_groups(device);
    }

    /// Drops the amplified texture (legacy worlds render cells only).
    pub fn clear_amplified_image(&mut self, device: &wgpu::Device) {
        if self.amplified_view.take().is_some() {
            self.rebuild_fill_bind_groups(device);
        }
    }

    fn rebuild_fill_bind_groups(&mut self, device: &wgpu::Device) {
        let amplified = self
            .amplified_view
            .as_ref()
            .unwrap_or(&self.amplified_dummy_view);
        self.map_bind_group = create_bind_group(
            device,
            &self.bind_group_layout,
            &self.fill_buffer,
            &self.diagnostic_buffer,
            &self.palette_buffer,
            &self.map_uniform_buffer,
            amplified,
            &self.amplified_sampler,
            "Spherical Map Fill Bind Group",
        );
        self.globe_bind_group = create_bind_group(
            device,
            &self.bind_group_layout,
            &self.fill_buffer,
            &self.diagnostic_buffer,
            &self.palette_buffer,
            &self.globe_uniform_buffer,
            amplified,
            &self.amplified_sampler,
            "Spherical Globe Fill Bind Group",
        );
    }

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
        // The amplified view only renders when its texture actually exists;
        // a stale toggle can never sample the placeholder.
        let mut gated = *uniform;
        if self.amplified_view.is_none() {
            gated.amplified_mode = 0;
        }
        queue.write_buffer(buffer, 0, bytemuck::bytes_of(&gated));
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
                if self.map_overlay_instance_count > 0 {
                    pass.set_pipeline(&self.map_overlay_pipeline);
                    pass.set_bind_group(0, &self.map_bind_group, &[]);
                    pass.set_vertex_buffer(0, self.map_overlay_buffer.slice(..));
                    pass.draw(0..9, 0..self.map_overlay_instance_count);
                }
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
                if self.globe_overlay_instance_count > 0 {
                    pass.set_pipeline(&self.globe_overlay_pipeline);
                    pass.set_bind_group(0, &self.globe_bind_group, &[]);
                    pass.set_vertex_buffer(0, self.globe_overlay_buffer.slice(..));
                    pass.draw(0..9, 0..self.globe_overlay_instance_count);
                }
            }
        }
    }

    /// Returns cumulative successful immutable and uniform upload evidence.
    pub const fn upload_counters(&self) -> SphericalUploadCounters {
        self.counters
    }

    /// Returns map-projection diagnostics emitted while preparing the installed vector glyphs.
    pub fn vector_diagnostics(&self) -> &[OwnedViewDiagnostic] {
        self.installed_map_overlay
            .as_deref()
            .map(PreparedMapOverlayInstances::vector_diagnostics)
            .unwrap_or_default()
    }

    /// Returns the source identity of the last completely installed packet.
    pub const fn installed_source(&self) -> Option<&SphericalPresentationSource> {
        self.installed_source.as_ref()
    }

    /// Returns whether a deferred egui callback still names the synchronously published packet.
    pub(super) fn callback_packet_is_current(&self, packet: &SphericalGpuPacket) -> bool {
        self.installed_packet_key
            .as_ref()
            .is_some_and(|key| key.exactly_matches(packet))
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

#[cfg(test)]
pub(crate) fn installed_overlay_arc_ids(
    renderer: &SphericalFieldRenderer,
) -> Option<(usize, usize)> {
    Some((
        Arc::as_ptr(renderer.installed_map_overlay.as_ref()?) as usize,
        Arc::as_ptr(renderer.installed_globe_overlay.as_ref()?) as usize,
    ))
}

#[derive(Debug, Clone, Copy)]
struct BufferSizes {
    map_vertices: u64,
    map_indices: u64,
    globe_vertices: u64,
    globe_indices: u64,
    map_overlay: u64,
    globe_overlay: u64,
    fill: u64,
    diagnostics: u64,
    palette: u64,
}

impl BufferSizes {
    fn for_packet(
        packet: &SphericalGpuPacket,
        map_overlay_count: usize,
        globe_overlay_count: usize,
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
            map_overlay: checked_buffer_bytes::<GpuMapOverlayInstance>(
                map_overlay_count,
                "map overlay instances",
            )?,
            globe_overlay: checked_buffer_bytes::<GpuGlobeOverlayInstance>(
                globe_overlay_count,
                "globe overlay instances",
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
            (
                "map overlay instances",
                sizes.map_overlay,
                limits.max_buffer_size,
            ),
            (
                "globe overlay instances",
                sizes.globe_overlay,
                limits.max_buffer_size,
            ),
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
    if let Some(overlay) = packet.layers().overlay() {
        match overlay {
            PreparedSphericalOverlay::Edge(field) => validate_cardinality(
                "edge overlay",
                packet.source().surface_ref().edge_count() as usize,
                field.len(),
            )?,
            PreparedSphericalOverlay::Vector(field) => {
                validate_cardinality("vector overlay", cell_count, field.len())?
            }
        }
        if packet
            .layers()
            .overlay_palette()
            .is_none_or(|palette| palette.is_empty())
        {
            return Err(SphericalRenderError::InvalidGeometry {
                resource: "overlay palette",
            });
        }
    }
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
    uploaded_map_overlay: bool,
    uploaded_globe_overlay: bool,
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
    if uploaded_map_overlay {
        next.map_overlay_instances = checked_counter(
            next.map_overlay_instances,
            1,
            "map overlay instance upload counter",
        )?;
    }
    if uploaded_globe_overlay {
        next.globe_overlay_instances = checked_counter(
            next.globe_overlay_instances,
            1,
            "globe overlay instance upload counter",
        )?;
    }
    let submitted = [
        plan.map_geometry.then_some(sizes.map_vertices),
        plan.map_geometry.then_some(sizes.map_indices),
        plan.globe_geometry.then_some(sizes.globe_vertices),
        plan.globe_geometry.then_some(sizes.globe_indices),
        uploaded_map_overlay.then_some(sizes.map_overlay),
        uploaded_globe_overlay.then_some(sizes.globe_overlay),
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
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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

fn create_amplified_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Spherical Amplified Sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    })
}

fn create_amplified_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Spherical Amplified Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[allow(clippy::too_many_arguments)]
fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    fill: &wgpu::Buffer,
    diagnostics: &wgpu::Buffer,
    palette: &wgpu::Buffer,
    uniform: &wgpu::Buffer,
    amplified: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
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
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(amplified),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(sampler),
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
    const MAP_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Uint32, 2 => Float32x3];
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

fn create_overlay_pipeline(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
    mode: SphericalRenderMode,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Spherical Overlay Shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("../../../assets/shaders/spherical_field.wgsl").into(),
        ),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Spherical Overlay Pipeline Layout"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });
    const MAP_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32,
        4 => Uint32
    ];
    const GLOBE_ATTRIBUTES: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32,
        2 => Float32x3,
        3 => Float32,
        4 => Float32x4,
        5 => Uint32
    ];
    let (label, entry_point, array_stride, attributes) = match mode {
        SphericalRenderMode::Map => (
            "Spherical Map Overlay Pipeline",
            "vs_map_overlay",
            std::mem::size_of::<GpuMapOverlayInstance>() as u64,
            &MAP_ATTRIBUTES[..],
        ),
        SphericalRenderMode::Globe => (
            "Spherical Globe Overlay Pipeline",
            "vs_globe_overlay",
            std::mem::size_of::<GpuGlobeOverlayInstance>() as u64,
            &GLOBE_ATTRIBUTES[..],
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
                step_mode: wgpu::VertexStepMode::Instance,
                attributes,
            }],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_overlay"),
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{mpsc, Arc};

    use super::super::overlay::{
        overlay_preparation_counts, reset_overlay_preparation_counts, GpuGlobeOverlayInstance,
        GpuMapOverlayInstance,
    };
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
        SelectedSurfaceEntity, SphericalFieldDisplayState, SphericalLayerVisibility,
        SphericalMeshBudgets, SphericalPresentationSource, SphericalProjection,
        SphericalProjectionKind, VectorAnimationUniform, VectorGlyphLod, ViewDiagnosticSeverity,
        DIAGNOSTIC_ERROR_COLOR,
    };
    use crate::world::fields::{
        DomainSizes, ExtensionFieldSet, FieldData, FieldDisplayMetadata, FieldDomain, FieldId,
        FieldPaletteHint, FieldRegistryBuilder, FieldSchema, FieldUnit, FieldValueType,
        MissingValuePolicy, ValueRange,
    };
    use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    #[test]
    fn spherical_gpu_layouts_match_wgsl_vertex_and_uniform_contracts() {
        assert_eq!(std::mem::size_of::<GpuMapVertex>(), 24);
        assert_eq!(std::mem::size_of::<GpuGlobeVertex>(), 16);
        assert_eq!(std::mem::size_of::<GpuMapOverlayInstance>(), 48);
        assert_eq!(std::mem::size_of::<GpuGlobeOverlayInstance>(), 64);
        assert_eq!(std::mem::size_of::<SphericalFrameUniform>(), 128);
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
    fn publication_ownership_guards_run_before_packet_validation_or_uploads() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let installed = packet_fixture(TestFieldKind::Scalar, 103);
        let elsewhere = packet_fixture(TestFieldKind::Category, 103);
        assert_eq!(installed.packet.source(), elsewhere.packet.source());
        assert_eq!(
            installed.packet.map_geometry_revision(),
            elsewhere.packet.map_geometry_revision()
        );
        assert_eq!(
            installed.packet.globe_geometry_revision(),
            elsewhere.packet.globe_geometry_revision()
        );
        assert_eq!(
            installed.packet.layers().revisions(),
            elsewhere.packet.layers().revisions()
        );
        assert!(!Arc::ptr_eq(
            installed.packet.layers_arc(),
            elsewhere.packet.layers_arc()
        ));
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer
            .prepare_initial_publication_packet(&device, &queue, &installed.packet)
            .unwrap();
        let before = renderer.upload_counters();
        assert!(renderer.callback_packet_is_current(&installed.packet));
        assert!(!renderer.callback_packet_is_current(&elsewhere.packet));

        validation_probe::reset();
        assert_eq!(
            renderer.prepare_initial_publication_packet(&device, &queue, &elsewhere.packet),
            Err(SphericalRenderError::RendererAlreadyInitialized)
        );
        assert_eq!(validation_probe::snapshot(), Default::default());
        assert_eq!(renderer.upload_counters(), before);
        assert!(renderer.callback_packet_is_current(&installed.packet));
        assert!(!renderer.callback_packet_is_current(&elsewhere.packet));

        validation_probe::reset();
        assert_eq!(
            renderer.prepare_replacement_publication_packet(
                &device,
                &queue,
                &elsewhere.packet,
                &elsewhere.packet,
            ),
            Err(SphericalRenderError::RendererCurrentPacketMismatch)
        );
        assert_eq!(validation_probe::snapshot(), Default::default());
        assert_eq!(renderer.upload_counters(), before);
        assert!(renderer.callback_packet_is_current(&installed.packet));
        assert!(!renderer.callback_packet_is_current(&elsewhere.packet));
    }

    #[test]
    fn changed_layers_cannot_reuse_installed_revisions_or_replace_the_cpu_packet_key() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = packet_fixture(TestFieldKind::Scalar, 107);
        let changed_layers = layers_for_count(
            fixture.packet.source().clone(),
            fixture.surface.cells().len(),
            fixture.surface.edges().len(),
            TestFieldKind::Category,
        );
        assert_eq!(
            changed_layers.revisions(),
            fixture.packet.layers().revisions(),
            "independent clocks reproduce the revision collision"
        );
        assert_ne!(
            changed_layers.fill().kind(),
            fixture.packet.layers().fill().kind(),
            "the colliding packet must carry observably different data"
        );
        let changed = SphericalGpuPacket::new(
            Arc::clone(&fixture.packet.map),
            fixture.packet.map_geometry_revision,
            Arc::clone(&fixture.packet.globe),
            fixture.packet.globe_geometry_revision,
            changed_layers,
        );
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let before = renderer.upload_counters();

        let result = renderer.prepare_packet(&device, &queue, &changed);

        assert_eq!(
            result,
            Err(SphericalRenderError::RevisionNotAdvanced {
                resource: "fill field",
                installed: fixture.packet.layers().revisions().fill.get(),
                candidate: changed.layers().revisions().fill.get(),
            })
        );
        assert_eq!(renderer.upload_counters(), before);
        assert!(renderer.callback_packet_is_current(&fixture.packet));
        assert!(!renderer.callback_packet_is_current(&changed));
    }

    #[test]
    fn changed_layers_cannot_regress_installed_revisions() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = packet_fixture(TestFieldKind::Scalar, 108);
        let installed_layers = layers_for_count_after_issues(
            fixture.packet.source().clone(),
            fixture.surface.cells().len(),
            fixture.surface.edges().len(),
            TestFieldKind::Scalar,
            20,
        );
        let installed = SphericalGpuPacket::new(
            Arc::clone(&fixture.packet.map),
            fixture.packet.map_geometry_revision,
            Arc::clone(&fixture.packet.globe),
            fixture.packet.globe_geometry_revision,
            installed_layers,
        );
        let regressed_layers = layers_for_count(
            fixture.packet.source().clone(),
            fixture.surface.cells().len(),
            fixture.surface.edges().len(),
            TestFieldKind::Category,
        );
        let regressed = SphericalGpuPacket::new(
            Arc::clone(&fixture.packet.map),
            fixture.packet.map_geometry_revision,
            Arc::clone(&fixture.packet.globe),
            fixture.packet.globe_geometry_revision,
            regressed_layers,
        );
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer
            .prepare_packet(&device, &queue, &installed)
            .unwrap();
        let before = renderer.upload_counters();

        assert_eq!(
            renderer.prepare_packet(&device, &queue, &regressed),
            Err(SphericalRenderError::RevisionNotAdvanced {
                resource: "fill field",
                installed: installed.layers().revisions().fill.get(),
                candidate: regressed.layers().revisions().fill.get(),
            })
        );
        assert_eq!(renderer.upload_counters(), before);
        assert!(renderer.callback_packet_is_current(&installed));
        assert!(!renderer.callback_packet_is_current(&regressed));
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
    fn vector_phase_and_camera_frames_preserve_instance_uploads_and_validation_scans() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = vector_packet_fixture(71, true);
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        validation_probe::reset();
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let after_upload = renderer.upload_counters();
        let after_validation = validation_probe::snapshot();
        assert_eq!(after_upload.map_overlay_instances, 1);
        assert_eq!(after_upload.globe_overlay_instances, 1);

        let viewport = [256, 128];
        let paused = VectorAnimationUniform::new(0.2);
        let first_uniform = SphericalFrameUniform::for_map_with_animation(
            &fixture.packet,
            MapCamera::default(),
            viewport,
            paused,
        )
        .unwrap();
        renderer
            .prepare_frame(&queue, SphericalRenderMode::Map, &first_uniform)
            .unwrap();
        let first = readback_renderer(
            &device,
            &queue,
            &renderer,
            SphericalRenderMode::Map,
            viewport,
        );
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        renderer
            .prepare_frame(&queue, SphericalRenderMode::Map, &first_uniform)
            .unwrap();
        let repeated = readback_renderer(
            &device,
            &queue,
            &renderer,
            SphericalRenderMode::Map,
            viewport,
        );
        assert_eq!(
            first, repeated,
            "paused uniform must render byte-identically"
        );

        let advanced = SphericalFrameUniform::for_map_with_animation(
            &fixture.packet,
            MapCamera::default(),
            viewport,
            VectorAnimationUniform::new(0.65),
        )
        .unwrap();
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        renderer
            .prepare_frame(&queue, SphericalRenderMode::Map, &advanced)
            .unwrap();
        let animated = readback_renderer(
            &device,
            &queue,
            &renderer,
            SphericalRenderMode::Map,
            viewport,
        );
        assert_ne!(first, animated, "phase must move only the bright segment");

        let mut zoomed_camera = MapCamera::default();
        assert!(zoomed_camera.zoom_by(SphericalProjectionKind::Equirectangular, 1.25));
        let zoomed = SphericalFrameUniform::for_map_with_animation(
            &fixture.packet,
            zoomed_camera,
            viewport,
            VectorAnimationUniform::new(0.65),
        )
        .unwrap();
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        renderer
            .prepare_frame(&queue, SphericalRenderMode::Map, &zoomed)
            .unwrap();

        let after_frames = renderer.upload_counters();
        assert_eq!(
            after_frames.map_overlay_instances,
            after_upload.map_overlay_instances
        );
        assert_eq!(
            after_frames.globe_overlay_instances,
            after_upload.globe_overlay_instances
        );
        assert_eq!(after_frames.uniforms, after_upload.uniforms + 4);
        assert_eq!(validation_probe::snapshot(), after_validation);
    }

    #[test]
    fn projection_change_rebuilds_only_map_geometry_and_map_overlay_instances() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = vector_packet_fixture(72, true);
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let before = renderer.upload_counters();
        let projection =
            SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.3).unwrap();
        let changed_map = Arc::new(
            PreparedProjectedMap::build(
                fixture.packet.source().clone(),
                &fixture.surface,
                projection,
                SphericalMeshBudgets::DEFAULT,
            )
            .unwrap(),
        );
        let changed = SphericalGpuPacket::new(
            changed_map,
            DisplayRevision::new(103).unwrap(),
            Arc::clone(&fixture.packet.globe),
            fixture.packet.globe_geometry_revision,
            Arc::clone(&fixture.layers),
        );

        renderer.prepare_packet(&device, &queue, &changed).unwrap();

        let after = renderer.upload_counters();
        assert_eq!(after.map_geometry, before.map_geometry + 1);
        assert_eq!(after.globe_geometry, before.globe_geometry);
        assert_eq!(
            after.map_overlay_instances,
            before.map_overlay_instances + 1
        );
        assert_eq!(
            after.globe_overlay_instances,
            before.globe_overlay_instances
        );
        assert_eq!(after.fill_field, before.fill_field);
        assert_eq!(after.diagnostics, before.diagnostics);
        assert_eq!(after.palettes, before.palettes);
    }

    #[test]
    fn projection_change_replaces_only_map_cpu_overlay_arc_for_vectors_and_edges() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        for fixture in [
            vector_packet_fixture(84, true),
            overlay_packet_fixture(OverlayTestKind::EdgeScalar, 85),
        ] {
            let mut renderer =
                SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            reset_overlay_preparation_counts();
            renderer
                .prepare_packet(&device, &queue, &fixture.packet)
                .unwrap();
            assert_eq!(overlay_preparation_counts().map, 1);
            assert_eq!(overlay_preparation_counts().globe, 1);
            let installed_map = Arc::clone(renderer.installed_map_overlay.as_ref().unwrap());
            let installed_globe = Arc::clone(renderer.installed_globe_overlay.as_ref().unwrap());
            let before = renderer.upload_counters();
            let changed_map = Arc::new(
                PreparedProjectedMap::build(
                    fixture.packet.source().clone(),
                    &fixture.surface,
                    SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.3).unwrap(),
                    SphericalMeshBudgets::DEFAULT,
                )
                .unwrap(),
            );
            let changed = SphericalGpuPacket::new(
                changed_map,
                DisplayRevision::new(103).unwrap(),
                Arc::clone(&fixture.packet.globe),
                fixture.packet.globe_geometry_revision,
                Arc::clone(&fixture.layers),
            );

            renderer.prepare_packet(&device, &queue, &changed).unwrap();

            assert_eq!(overlay_preparation_counts().map, 2);
            assert_eq!(overlay_preparation_counts().globe, 1);
            assert!(!Arc::ptr_eq(
                &installed_map,
                renderer.installed_map_overlay.as_ref().unwrap()
            ));
            assert!(Arc::ptr_eq(
                &installed_globe,
                renderer.installed_globe_overlay.as_ref().unwrap()
            ));
            let after = renderer.upload_counters();
            assert_eq!(
                after.map_overlay_instances,
                before.map_overlay_instances + 1
            );
            assert_eq!(
                after.globe_overlay_instances,
                before.globe_overlay_instances
            );
        }
    }

    #[test]
    fn offscreen_scalar_and_category_edges_render_as_triangle_instances_in_both_modes() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        for kind in [OverlayTestKind::EdgeScalar, OverlayTestKind::EdgeCategory] {
            let seed = match kind {
                OverlayTestKind::EdgeScalar => 73,
                OverlayTestKind::EdgeCategory => 74,
            };
            let baseline = packet_fixture(TestFieldKind::Scalar, seed);
            let overlay = overlay_packet_fixture(kind, seed);
            for (mode, viewport) in [
                (SphericalRenderMode::Map, [256, 128]),
                (SphericalRenderMode::Globe, [128, 128]),
            ] {
                let (baseline_pixels, _) =
                    render_offscreen(&device, &queue, &baseline.packet, mode, viewport);
                let (overlay_pixels, _) =
                    render_offscreen(&device, &queue, &overlay.packet, mode, viewport);
                assert_ne!(
                    overlay_pixels, baseline_pixels,
                    "{kind:?} must add bounded triangle annotations in {mode:?}"
                );
            }
        }
    }

    #[test]
    fn layer_visibility_is_independent_for_fill_overlay_and_diagnostics_in_map_and_globe() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = overlay_diagnostic_packet_fixture(211);
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();
        let installed = renderer.upload_counters();

        for (mode, viewport) in [
            (SphericalRenderMode::Map, [256, 128]),
            (SphericalRenderMode::Globe, [128, 128]),
        ] {
            let make_uniform = |visibility| match mode {
                SphericalRenderMode::Map => SphericalFrameUniform::for_map_with_visibility(
                    &fixture.packet,
                    MapCamera::default(),
                    viewport,
                    visibility,
                ),
                SphericalRenderMode::Globe => SphericalFrameUniform::for_globe_with_visibility(
                    &fixture.packet,
                    GlobeCamera::default(),
                    viewport,
                    visibility,
                ),
            };
            let render = |renderer: &mut SphericalFieldRenderer,
                          uniform: &SphericalFrameUniform| {
                renderer.prepare_frame(&queue, mode, uniform).unwrap();
                readback_renderer(&device, &queue, renderer, mode, viewport)
            };

            let all = render(
                &mut renderer,
                &make_uniform(SphericalLayerVisibility::default()).unwrap(),
            );

            let mut overlay_only = make_uniform(SphericalLayerVisibility {
                fill: false,
                overlay: true,
                amplified: false,
            })
            .unwrap();
            overlay_only.diagnostics_enabled = 0;
            let overlay_only = render(&mut renderer, &overlay_only);

            let mut transparent = make_uniform(SphericalLayerVisibility {
                fill: false,
                overlay: false,
                amplified: false,
            })
            .unwrap();
            transparent.diagnostics_enabled = 0;
            let transparent = render(&mut renderer, &transparent);

            let diagnostic_only = render(
                &mut renderer,
                &make_uniform(SphericalLayerVisibility {
                    fill: false,
                    overlay: false,
                    amplified: false,
                })
                .unwrap(),
            );

            let mut fill_only = make_uniform(SphericalLayerVisibility {
                fill: true,
                overlay: false,
                amplified: false,
            })
            .unwrap();
            fill_only.diagnostics_enabled = 0;
            let fill_only = render(&mut renderer, &fill_only);

            let opaque =
                |pixels: &[u8]| pixels.chunks_exact(4).filter(|pixel| pixel[3] != 0).count();
            assert_eq!(opaque(&transparent), 0, "{mode:?}: both layers hidden");
            assert!(
                opaque(&overlay_only) > 0,
                "{mode:?}: overlay remains visible"
            );
            assert!(
                opaque(&diagnostic_only) > 0,
                "{mode:?}: diagnostic remains visible"
            );
            assert!(
                opaque(&diagnostic_only) < opaque(&fill_only),
                "{mode:?}: diagnostic-only output must not restore the hidden fill"
            );
            assert_ne!(all, fill_only, "{mode:?}: hiding overlay changes pixels");
            let cell = fixture.diagnostic_cell.unwrap();
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
            let uniform = make_uniform(SphericalLayerVisibility {
                fill: false,
                overlay: false,
                amplified: false,
            })
            .unwrap();
            assert_pixel_near(
                &diagnostic_only,
                viewport,
                transformed_pixel(uniform.transform, position, viewport),
                linear_to_srgba8(DIAGNOSTIC_ERROR_COLOR),
                2,
            );
        }

        let after = renderer.upload_counters();
        assert_eq!(after.map_geometry, installed.map_geometry);
        assert_eq!(after.globe_geometry, installed.globe_geometry);
        assert_eq!(after.fill_field, installed.fill_field);
        assert_eq!(after.diagnostics, installed.diagnostics);
        assert_eq!(after.palettes, installed.palettes);
        assert_eq!(after.map_overlay_instances, installed.map_overlay_instances);
        assert_eq!(
            after.globe_overlay_instances,
            installed.globe_overlay_instances
        );
        assert_eq!(after.uniforms, installed.uniforms + 10);
        assert!(renderer.callback_packet_is_current(&fixture.packet));
    }

    #[test]
    fn zero_and_no_event_edges_filter_only_display_instances_not_prepared_payloads() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        for (kind, seed) in [
            (OverlayTestKind::EdgeScalar, 75),
            (OverlayTestKind::EdgeCategory, 76),
        ] {
            let fixture = overlay_packet_fixture(kind, seed);
            let field = match fixture.layers.overlay().unwrap() {
                crate::view::PreparedSphericalOverlay::Edge(field) => field,
                crate::view::PreparedSphericalOverlay::Vector(_) => unreachable!(),
            };
            assert_eq!(field.len(), fixture.surface.edges().len());
            let expected_visible = match field.kind() {
                PreparedFieldKind::Scalar => field
                    .raw_values()
                    .iter()
                    .filter(|&&raw| f32::from_bits(raw) != 0.0)
                    .count(),
                PreparedFieldKind::Category => field
                    .raw_values()
                    .iter()
                    .filter(|&&raw| field.category_keys()[raw as usize] != 0)
                    .count(),
            };
            let mut renderer =
                SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            renderer
                .prepare_packet(&device, &queue, &fixture.packet)
                .unwrap();
            assert_eq!(
                renderer.globe_overlay_instance_count as usize,
                expected_visible
            );
            assert!(renderer.map_overlay_instance_count as usize >= expected_visible);
        }
    }

    #[test]
    fn offscreen_globe_vector_glyphs_cull_back_hemisphere_and_keep_front_direction() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let seed = 79;
        let baseline = packet_fixture(TestFieldKind::Scalar, seed);
        let back = vector_packet_fixture(seed, false);
        let front = vector_packet_fixture(seed, true);
        let viewport = [192, 192];
        let (baseline_pixels, _) = render_offscreen(
            &device,
            &queue,
            &baseline.packet,
            SphericalRenderMode::Globe,
            viewport,
        );
        let (back_pixels, _) = render_offscreen(
            &device,
            &queue,
            &back.packet,
            SphericalRenderMode::Globe,
            viewport,
        );
        let (front_pixels, _) = render_offscreen(
            &device,
            &queue,
            &front.packet,
            SphericalRenderMode::Globe,
            viewport,
        );

        assert_eq!(back_pixels, baseline_pixels);
        assert_ne!(front_pixels, baseline_pixels);
    }

    #[test]
    fn offscreen_globe_edge_annotations_cull_back_hemisphere_segments() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let seed = 83;
        let baseline = packet_fixture(TestFieldKind::Scalar, seed);
        let back = edge_hemisphere_packet_fixture(seed, false);
        let front = edge_hemisphere_packet_fixture(seed, true);
        let viewport = [192, 192];
        let (baseline_pixels, _) = render_offscreen(
            &device,
            &queue,
            &baseline.packet,
            SphericalRenderMode::Globe,
            viewport,
        );
        let (back_pixels, _) = render_offscreen(
            &device,
            &queue,
            &back.packet,
            SphericalRenderMode::Globe,
            viewport,
        );
        let (front_pixels, _) = render_offscreen(
            &device,
            &queue,
            &front.packet,
            SphericalRenderMode::Globe,
            viewport,
        );

        assert_eq!(back_pixels, baseline_pixels);
        assert_ne!(front_pixels, baseline_pixels);
    }

    #[test]
    fn globe_horizon_edges_and_arrows_stay_inside_the_orthographic_silhouette() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let viewport = [64, 64];
        let mut rotated_edge_camera = GlobeCamera::default();
        assert!(rotated_edge_camera.trackball_drag(
            [32.0, 32.0],
            [40.0, 32.0],
            viewport.map(f64::from),
        ));
        for (seed, overlay, globe_camera) in [
            (86, edge_horizon_packet_fixture(86), rotated_edge_camera),
            (
                87,
                vector_horizon_packet_fixture(87),
                GlobeCamera::default(),
            ),
        ] {
            let baseline = packet_fixture(TestFieldKind::Scalar, seed);
            let (baseline_pixels, _) = render_offscreen_with_globe_camera(
                &device,
                &queue,
                &baseline.packet,
                SphericalRenderMode::Globe,
                viewport,
                globe_camera,
            );
            let (overlay_pixels, uniform) = render_offscreen_with_globe_camera(
                &device,
                &queue,
                &overlay.packet,
                SphericalRenderMode::Globe,
                viewport,
                globe_camera,
            );

            assert_overlay_changes_inside_globe_silhouette(
                &baseline_pixels,
                &overlay_pixels,
                uniform.transform,
                viewport,
                seed,
            );
        }
    }

    #[test]
    fn globe_overlay_silhouette_is_invariant_to_nonzero_viewport_origin() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let viewport = [192, 192];
        let target = [640, 320];
        let mut rotated_edge_camera = GlobeCamera::default();
        assert!(rotated_edge_camera.trackball_drag(
            [96.0, 96.0],
            [120.0, 96.0],
            viewport.map(f64::from),
        ));
        for (seed, fixture, camera) in [
            (86, edge_horizon_packet_fixture(86), rotated_edge_camera),
            (
                87,
                vector_horizon_packet_fixture(87),
                GlobeCamera::default(),
            ),
        ] {
            let baseline = packet_fixture(TestFieldKind::Scalar, seed);
            let baseline_zero = render_globe_viewport_crop(
                &device,
                &queue,
                &baseline.packet,
                viewport,
                target,
                [0, 0],
                camera,
            );
            let baseline_offset = render_globe_viewport_crop(
                &device,
                &queue,
                &baseline.packet,
                viewport,
                target,
                [320, 40],
                camera,
            );
            assert_eq!(
                baseline_zero, baseline_offset,
                "the globe fill establishes a viewport-local crop oracle for seed {seed}"
            );

            let overlay_zero = render_globe_viewport_crop(
                &device,
                &queue,
                &fixture.packet,
                viewport,
                target,
                [0, 0],
                camera,
            );
            let overlay_offset = render_globe_viewport_crop(
                &device,
                &queue,
                &fixture.packet,
                viewport,
                target,
                [320, 40],
                camera,
            );
            let uniform =
                SphericalFrameUniform::for_globe(&fixture.packet, camera, viewport).unwrap();
            assert_overlay_changes_inside_globe_silhouette(
                &baseline_zero,
                &overlay_zero,
                uniform.transform,
                viewport,
                seed,
            );
            assert_overlay_changes_inside_globe_silhouette(
                &baseline_offset,
                &overlay_offset,
                uniform.transform,
                viewport,
                seed,
            );
            assert_eq!(
                overlay_zero, overlay_offset,
                "globe edge/vector silhouette clipping must use callback-local coordinates for seed {seed}"
            );
        }
    }

    #[test]
    fn horizon_edge_fixture_keeps_authoritative_prepared_segments() {
        let fixture = edge_horizon_packet_fixture(86);
        let authoritative = PreparedGlobeMesh::build(
            fixture.packet.source().clone(),
            &fixture.surface,
            SphericalMeshBudgets::DEFAULT,
        )
        .unwrap();

        assert_eq!(
            fixture.packet.globe().edge_segments(),
            authoritative.edge_segments()
        );
    }

    #[test]
    fn prepared_renderer_exposes_map_jacobian_diagnostics_from_the_real_packet_path() {
        let Some((device, queue)) = request_test_device() else {
            return;
        };
        let fixture = vector_jacobian_packet_fixture(89);
        let mut renderer =
            SphericalFieldRenderer::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);

        renderer
            .prepare_packet(&device, &queue, &fixture.packet)
            .unwrap();

        assert_eq!(renderer.globe_overlay_instance_count, 1);
        assert_eq!(renderer.map_overlay_instance_count, 0);
        let diagnostics = renderer.vector_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            "display.vector_projection_jacobian_degenerate"
        );
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
    fn offscreen_diagnostic_overlay_is_source_bound_semantic_and_portable() {
        let Some((adapter, device, queue)) = request_test_device_with_info() else {
            return;
        };
        let fixture = packet_fixture_with_diagnostic(TestFieldKind::Scalar, 11, true);
        let cell = fixture
            .diagnostic_cell
            .expect("diagnostic fixture selects a front-facing cell");
        assert_eq!(fixture.packet.source(), fixture.layers.source());
        assert_eq!(fixture.packet.map().source(), fixture.packet.source());
        assert_eq!(fixture.packet.globe().source(), fixture.packet.source());
        assert_eq!(fixture.layers.fill().len(), fixture.surface.cells().len());
        assert_eq!(
            fixture.layers.diagnostics().len(),
            fixture.surface.cells().len()
        );
        assert_eq!(fixture.diagnostics.len(), 1);
        assert_eq!(
            fixture.diagnostics[0].severity,
            ViewDiagnosticSeverity::Error
        );
        assert_eq!(
            fixture.diagnostics[0].cell_id,
            Some(CellId::from_raw(cell as u32))
        );
        assert_eq!(
            fixture.diagnostics[0].field_id.as_ref(),
            Some(fixture.layers.fill().field_id())
        );
        let mut expected_mask = vec![0_u32; fixture.surface.cells().len()];
        expected_mask[cell] = 3;
        assert_eq!(fixture.layers.diagnostics().cells(), expected_mask);
        let expected = linear_to_srgba8(DIAGNOSTIC_ERROR_COLOR);
        let mut rendered = Vec::new();
        for mode in [SphericalRenderMode::Map, SphericalRenderMode::Globe] {
            let viewport = [192, 96];
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
            rendered.push((mode, rgba8));
        }

        let audited = matches!(
            (adapter.name.as_str(), adapter.backend),
            ("NVIDIA GeForce RTX 4080 SUPER", wgpu::Backend::Vulkan)
                | ("NVIDIA GeForce RTX 4080 SUPER/PCIe/SSE2", wgpu::Backend::Gl)
        );
        for ((mode, pixels), expected_hash) in rendered.into_iter().zip([
            "dff1205540d94745dbb36fd43f64f882afdfec43385f1c4dd6d53fe10791662c",
            "55136898b70af3a770a973686f0a7bc500cd57bc1f10c97af4916f43d07e6574",
        ]) {
            let hash = blake3::hash(&pixels).to_hex().to_string();
            eprintln!(
                "diagnostic golden {mode:?}: adapter={:?}/{:?} blake3={hash} policy={}",
                adapter.name,
                adapter.backend,
                if audited {
                    "audited-exact"
                } else {
                    "semantic-only-unaudited"
                }
            );
            if audited {
                assert_eq!(hash, expected_hash);
            }
        }
    }

    struct PacketFixture {
        packet: SphericalGpuPacket,
        layers: Arc<crate::view::PreparedFieldLayers>,
        surface: SphericalSurfaceSnapshot,
        diagnostic_cell: Option<usize>,
        diagnostics: Vec<OwnedViewDiagnostic>,
    }

    #[derive(Debug, Clone, Copy)]
    enum TestFieldKind {
        Scalar,
        Category,
    }

    #[derive(Debug, Clone, Copy)]
    enum OverlayTestKind {
        EdgeScalar,
        EdgeCategory,
    }

    #[derive(Debug, Clone, Copy)]
    enum VectorFixturePlacement {
        Hemisphere(bool),
        ProjectionPole,
        HorizonCrossing,
    }

    #[derive(Debug, Clone, Copy)]
    enum EdgeFixturePlacement {
        Hemisphere(bool),
        HorizonCrossing,
    }

    fn request_test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        request_test_device_with_info().map(|(_, device, queue)| (device, queue))
    }

    fn request_test_device_with_info() -> Option<(wgpu::AdapterInfo, wgpu::Device, wgpu::Queue)> {
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
            let info = adapter.get_info();
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
                Ok((device, queue)) => Some((info, device, queue)),
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
            diagnostics,
        }
    }

    fn overlay_packet_fixture(kind: OverlayTestKind, seed: u64) -> PacketFixture {
        overlay_packet_fixture_inner(kind, seed, None, None, false)
    }

    fn overlay_diagnostic_packet_fixture(seed: u64) -> PacketFixture {
        overlay_packet_fixture_inner(OverlayTestKind::EdgeScalar, seed, None, None, true)
    }

    fn vector_packet_fixture(seed: u64, front: bool) -> PacketFixture {
        overlay_packet_fixture_inner(
            OverlayTestKind::EdgeScalar,
            seed,
            Some(VectorFixturePlacement::Hemisphere(front)),
            None,
            false,
        )
    }

    fn vector_jacobian_packet_fixture(seed: u64) -> PacketFixture {
        overlay_packet_fixture_inner(
            OverlayTestKind::EdgeScalar,
            seed,
            Some(VectorFixturePlacement::ProjectionPole),
            None,
            false,
        )
    }

    fn edge_hemisphere_packet_fixture(seed: u64, front: bool) -> PacketFixture {
        overlay_packet_fixture_inner(
            OverlayTestKind::EdgeScalar,
            seed,
            None,
            Some(EdgeFixturePlacement::Hemisphere(front)),
            false,
        )
    }

    fn vector_horizon_packet_fixture(seed: u64) -> PacketFixture {
        overlay_packet_fixture_inner(
            OverlayTestKind::EdgeScalar,
            seed,
            Some(VectorFixturePlacement::HorizonCrossing),
            None,
            false,
        )
    }

    fn edge_horizon_packet_fixture(seed: u64) -> PacketFixture {
        overlay_packet_fixture_inner(
            OverlayTestKind::EdgeScalar,
            seed,
            None,
            Some(EdgeFixturePlacement::HorizonCrossing),
            false,
        )
    }

    fn overlay_packet_fixture_inner(
        kind: OverlayTestKind,
        seed: u64,
        vector_placement: Option<VectorFixturePlacement>,
        edge_placement: Option<EdgeFixturePlacement>,
        with_diagnostic: bool,
    ) -> PacketFixture {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let pole_cell = matches!(
            vector_placement,
            Some(VectorFixturePlacement::ProjectionPole)
        )
        .then(|| surface.cells()[0].id);
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
        let mut globe =
            PreparedGlobeMesh::build(source.clone(), &surface, SphericalMeshBudgets::DEFAULT)
                .unwrap();
        if let Some(cell) = pole_cell {
            globe.set_cell_centroid_for_test(cell, UnitVector3::new(0.0, 0.0, 1.0).unwrap());
        }
        let horizon_edge = matches!(edge_placement, Some(EdgeFixturePlacement::HorizonCrossing))
            .then(|| {
                globe
                    .edge_segments()
                    .iter()
                    .filter(|segment| segment.start()[2] * segment.end()[2] < 0.0)
                    .max_by(|left, right| {
                        horizon_crossing_radius_squared(left.start(), left.end())
                            .total_cmp(&horizon_crossing_radius_squared(right.start(), right.end()))
                    })
                    .expect("authoritative fixture contains a horizon-crossing edge")
                    .edge()
            });
        let globe = Arc::new(globe);
        let cell_count = surface.cells().len();
        let edge_count = surface.edges().len();
        let (fill_schema, fill_data, _) = test_field(TestFieldKind::Scalar, cell_count);
        let fill_id = fill_schema.id.clone();
        let (overlay_schema, overlay_data, selected_cell) =
            if let Some(placement) = vector_placement {
                let selected = match placement {
                    VectorFixturePlacement::ProjectionPole => pole_cell.unwrap(),
                    VectorFixturePlacement::Hemisphere(front) => {
                        surface
                            .cells()
                            .iter()
                            .find(|cell| {
                                let z = cell.centroid.components()[2];
                                if front {
                                    z > 0.55
                                } else {
                                    z < -0.55
                                }
                            })
                            .expect("fixture contains the requested hemisphere cell")
                            .id
                    }
                    VectorFixturePlacement::HorizonCrossing => {
                        surface
                            .cells()
                            .iter()
                            .filter(|cell| {
                                let z = cell.centroid.components()[2];
                                z > 0.4 && z < 0.75
                            })
                            .min_by(|left, right| {
                                left.centroid.components()[2]
                                    .total_cmp(&right.centroid.components()[2])
                            })
                            .expect("fixture contains a near-horizon front cell")
                            .id
                    }
                };
                let mut vectors = vec![[0.0, 0.0]; cell_count];
                vectors[selected.raw() as usize] = match placement {
                    VectorFixturePlacement::HorizonCrossing => [1.0, -1.0],
                    VectorFixturePlacement::Hemisphere(_)
                    | VectorFixturePlacement::ProjectionPole => [1.0, 0.0],
                };
                (
                    FieldSchema {
                        id: FieldId::new("test.spherical.gpu", "vector_overlay", 1).unwrap(),
                        domain: FieldDomain::Cells,
                        value_type: FieldValueType::Vector2F32,
                        unit: FieldUnit::Unitless,
                        valid_range: None,
                        missing: MissingValuePolicy::Forbidden,
                        dependencies: Vec::new(),
                        category_labels: BTreeMap::new(),
                        display: FieldDisplayMetadata::new(
                            "field.test.spherical.gpu.vector_overlay",
                            FieldPaletteHint::Vector,
                            4,
                        )
                        .unwrap(),
                    },
                    FieldData::Vector2F32(vectors),
                    Some(selected),
                )
            } else {
                match kind {
                    OverlayTestKind::EdgeScalar => (
                        FieldSchema {
                            id: FieldId::new("test.spherical.gpu", "edge_scalar", 1).unwrap(),
                            domain: FieldDomain::Edges,
                            value_type: FieldValueType::ScalarF32,
                            unit: FieldUnit::Unitless,
                            valid_range: Some(ValueRange::new(0.0, 1.0).unwrap()),
                            missing: MissingValuePolicy::Forbidden,
                            dependencies: Vec::new(),
                            category_labels: BTreeMap::new(),
                            display: FieldDisplayMetadata::new(
                                "field.test.spherical.gpu.edge_scalar",
                                FieldPaletteHint::Sequential,
                                4,
                            )
                            .unwrap(),
                        },
                        FieldData::ScalarF32(if let Some(placement) = edge_placement {
                            let selected = match placement {
                                EdgeFixturePlacement::Hemisphere(front) => {
                                    surface
                                        .edges()
                                        .iter()
                                        .find(|edge| {
                                            let z = edge.midpoint.components()[2];
                                            if front {
                                                z > 0.65
                                            } else {
                                                z < -0.65
                                            }
                                        })
                                        .expect("fixture contains the requested hemisphere edge")
                                        .id
                                }
                                EdgeFixturePlacement::HorizonCrossing => horizon_edge.unwrap(),
                            };
                            let mut values = vec![0.0; edge_count];
                            values[selected.raw() as usize] = 1.0;
                            values
                        } else {
                            (0..edge_count)
                                .map(|index| if index % 3 == 0 { 0.0 } else { 1.0 })
                                .collect()
                        }),
                        None,
                    ),
                    OverlayTestKind::EdgeCategory => (
                        FieldSchema {
                            id: FieldId::new("test.spherical.gpu", "edge_category", 1).unwrap(),
                            domain: FieldDomain::Edges,
                            value_type: FieldValueType::CategoryU32,
                            unit: FieldUnit::Unitless,
                            valid_range: None,
                            missing: MissingValuePolicy::Forbidden,
                            dependencies: Vec::new(),
                            category_labels: BTreeMap::from([
                                (0, "field.test.edge.none".into()),
                                (1, "field.test.edge.event".into()),
                            ]),
                            display: FieldDisplayMetadata::new(
                                "field.test.spherical.gpu.edge_category",
                                FieldPaletteHint::Categorical,
                                0,
                            )
                            .unwrap(),
                        },
                        FieldData::CategoryU32(
                            (0..edge_count).map(|index| (index % 2) as u32).collect(),
                        ),
                        None,
                    ),
                }
            };
        let overlay_id = overlay_schema.id.clone();
        let mut registry = FieldRegistryBuilder::new();
        registry.register(fill_schema).unwrap();
        registry.register(overlay_schema).unwrap();
        let registry = registry.build().unwrap();
        let mut fields = ExtensionFieldSet::new();
        fields
            .insert(
                &registry,
                fill_id.clone(),
                fill_data,
                &DomainSizes::new(cell_count, edge_count),
            )
            .unwrap();
        fields
            .insert(
                &registry,
                overlay_id.clone(),
                overlay_data,
                &DomainSizes::new(cell_count, edge_count),
            )
            .unwrap();
        let catalog = FieldCatalog::from_extension_fields(&registry, &fields).unwrap();
        let mut state = SphericalFieldDisplayState::default();
        state.select_overlay(Some(overlay_id));
        state.set_vector_lod(VectorGlyphLod::High);
        state.select_entity(selected_cell.map(SelectedSurfaceEntity::Cell));
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
                code: "test.spherical.visibility".into(),
                field_id: Some(fill_id.clone()),
                cell_id: Some(CellId::from_raw(cell as u32)),
                message: "visibility diagnostic".into(),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let mut clock = DisplayRevisionClock::default();
        let layers = Arc::new(
            prepare_spherical_field_layers(
                source,
                &catalog,
                cell_count,
                edge_count,
                &diagnostics,
                Some(fill_id),
                |_| Some(DisplayRangeMode::Data),
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
            diagnostics,
        }
    }

    fn horizon_crossing_radius_squared(start: [f32; 3], end: [f32; 3]) -> f32 {
        let crossing = start[2] / (start[2] - end[2]);
        let x = start[0] + (end[0] - start[0]) * crossing;
        let y = start[1] + (end[1] - start[1]) * crossing;
        x.mul_add(x, y * y)
    }

    fn layers_for_count(
        source: SphericalPresentationSource,
        cell_count: usize,
        edge_count: usize,
        kind: TestFieldKind,
    ) -> Arc<crate::view::PreparedFieldLayers> {
        layers_for_count_after_issues(source, cell_count, edge_count, kind, 0)
    }

    fn layers_for_count_after_issues(
        source: SphericalPresentationSource,
        cell_count: usize,
        edge_count: usize,
        kind: TestFieldKind,
        issues_before_preparation: usize,
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
        for _ in 0..issues_before_preparation {
            clock.issue().unwrap();
        }
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
        render_offscreen_with_globe_camera(
            device,
            queue,
            packet,
            mode,
            viewport,
            GlobeCamera::default(),
        )
    }

    fn render_offscreen_with_globe_camera(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
        mode: SphericalRenderMode,
        viewport: [u32; 2],
        globe_camera: GlobeCamera,
    ) -> (Vec<u8>, SphericalFrameUniform) {
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut renderer = SphericalFieldRenderer::new(device, format);
        renderer.prepare_packet(device, queue, packet).unwrap();
        let uniform = match mode {
            SphericalRenderMode::Map => {
                SphericalFrameUniform::for_map(packet, MapCamera::default(), viewport).unwrap()
            }
            SphericalRenderMode::Globe => {
                SphericalFrameUniform::for_globe(packet, globe_camera, viewport).unwrap()
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

    fn render_globe_viewport_crop(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &SphericalGpuPacket,
        viewport: [u32; 2],
        target: [u32; 2],
        origin: [u32; 2],
        camera: GlobeCamera,
    ) -> Vec<u8> {
        assert!(origin[0] + viewport[0] <= target[0]);
        assert!(origin[1] + viewport[1] <= target[1]);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut renderer = SphericalFieldRenderer::new(device, format);
        renderer.prepare_packet(device, queue, packet).unwrap();
        let uniform = SphericalFrameUniform::for_globe(packet, camera, viewport).unwrap();
        renderer
            .prepare_frame(queue, SphericalRenderMode::Globe, &uniform)
            .unwrap();

        let extent = wgpu::Extent3d {
            width: target[0],
            height: target[1],
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Spherical Nonzero Viewport Test Target"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let unpadded_bytes_per_row = target[0] * 4;
        let padded_bytes_per_row = unpadded_bytes_per_row
            .div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Spherical Nonzero Viewport Test Readback"),
            size: u64::from(padded_bytes_per_row) * u64::from(target[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Spherical Nonzero Viewport Test Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Spherical Nonzero Viewport Test Pass"),
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
            pass.set_viewport(
                origin[0] as f32,
                origin[1] as f32,
                viewport[0] as f32,
                viewport[1] as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(origin[0], origin[1], viewport[0], viewport[1]);
            renderer.paint(SphericalRenderMode::Globe, &mut pass.forget_lifetime());
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
                    rows_per_image: Some(target[1]),
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
        let mut crop = vec![0; (viewport[0] * viewport[1] * 4) as usize];
        for row in 0..viewport[1] as usize {
            let source_start =
                (origin[1] as usize + row) * padded_bytes_per_row as usize + origin[0] as usize * 4;
            let target_start = row * viewport[0] as usize * 4;
            crop[target_start..target_start + viewport[0] as usize * 4]
                .copy_from_slice(&mapped[source_start..source_start + viewport[0] as usize * 4]);
        }
        drop(mapped);
        readback.unmap();
        crop
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

    fn assert_overlay_changes_inside_globe_silhouette(
        baseline: &[u8],
        overlay: &[u8],
        transform: [[f32; 4]; 4],
        viewport: [u32; 2],
        seed: u64,
    ) {
        let radius_x = (0..3)
            .map(|column| transform[column][0] * transform[column][0])
            .sum::<f32>()
            .sqrt();
        let radius_y = (0..3)
            .map(|column| transform[column][1] * transform[column][1])
            .sum::<f32>()
            .sqrt();
        let mut changed = 0;
        for (pixel, (before, after)) in baseline
            .chunks_exact(4)
            .zip(overlay.chunks_exact(4))
            .enumerate()
        {
            if before == after {
                continue;
            }
            changed += 1;
            let x = pixel as u32 % viewport[0];
            let y = pixel as u32 / viewport[0];
            let ndc_x = (x as f32 + 0.5) * 2.0 / viewport[0] as f32 - 1.0;
            let ndc_y = 1.0 - (y as f32 + 0.5) * 2.0 / viewport[1] as f32;
            let silhouette = (ndc_x / radius_x).powi(2) + (ndc_y / radius_y).powi(2);
            assert!(
                silhouette <= 1.0 + 1.0e-5,
                "overlay pixel ({x}, {y}) escaped globe silhouette: {silhouette}"
            );
        }
        assert!(
            changed > 0,
            "front part of horizon annotation for seed {seed} must remain visible"
        );
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
