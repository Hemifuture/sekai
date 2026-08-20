//! Single-canvas controls and renderer-neutral inspection for spherical worlds.

use std::sync::{Arc, Weak};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::app::{PublishedSphericalPresentation, SphericalPresentationError};
use crate::gpu::spherical::{SphericalFieldRenderer, SphericalPaintCallback, SphericalRenderMode};
use crate::view::{
    classify_spherical_channel, format_field_value, DiagnosticScope, DisplayRangeMode, FieldValue,
    GlobeCamera, MapCamera, OwnedViewDiagnostic, PaletteId, PreparedFieldLayers,
    SelectedSurfaceEntity, SphericalFieldChannel, SphericalFieldDisplayState,
    SphericalPresentationViewState, SphericalProjection, SphericalProjectionError,
    SphericalProjectionKind, SphericalViewMode, VectorAnimationUniform, VectorGlyphLod,
    ViewDiagnosticSeverity,
};
use crate::world::fields::{FieldDomain, FieldId, FieldSchema, FieldValueType};
use crate::world::CellId;

use super::field::localization::localized_field_key;

/// Product copy for the display-only animation speed control.
pub const VECTOR_DISPLAY_SPEED_LABEL: &str = "显示速度（非物理时间）";
/// Stable labels for the three nested vector-glyph density levels.
pub const GLYPH_DENSITY_LABELS: [&str; 3] = ["Low", "Medium", "High"];
/// Edge picking tolerance expressed in egui logical pixels.
pub const EDGE_PICK_TOLERANCE_LOGICAL_PIXELS: f64 = 8.0;

/// One declarative user intent emitted by the spherical controls or canvas.
#[derive(Debug, Clone, PartialEq)]
pub enum SphericalCanvasAction {
    /// Selects the active presenter in the single canvas.
    SetViewMode(SphericalViewMode),
    /// Selects one of the two supported map projections.
    SetProjectionKind(SphericalProjectionKind),
    /// Rebuilds map geometry for a normalized central meridian in radians.
    SetCentralMeridianRadians(f64),
    /// Pans only the active projection camera in normalized canvas units.
    PanMap { delta: [f64; 2] },
    /// Multiplies only the active projection camera zoom about an NDC
    /// anchor, so the point under the cursor stays under the cursor.
    ZoomMap { factor: f64, anchor: [f64; 2] },
    /// Resets only the active projection camera.
    ResetMap,
    /// Applies a deterministic trackball drag to the globe camera.
    TrackballGlobe {
        start: [f64; 2],
        end: [f64; 2],
        canvas_size: [f64; 2],
    },
    /// Multiplies the globe orthographic scale.
    ZoomGlobe { factor: f64 },
    /// Resets only the globe camera.
    ResetGlobe,
    /// Selects the sole cell-fill field.
    SelectFill(FieldId),
    /// Selects or clears the sole edge/vector overlay.
    SelectOverlay(Option<FieldId>),
    /// Shows or hides the selected cell-fill layer.
    SetFillVisible(bool),
    /// Shows or hides the selected edge/vector overlay layer.
    SetOverlayVisible(bool),
    /// Shows or hides diagnostics.
    SetDiagnosticsEnabled(bool),
    /// Selects one authoritative surface entity.
    SelectEntity(Option<SelectedSurfaceEntity>),
    /// Pauses or resumes display-only vector animation.
    SetVectorPaused(bool),
    /// Sets the bounded, explicitly non-physical display speed.
    SetVectorDisplaySpeed(f32),
    /// Selects the minimum nested glyph density.
    SetVectorLod(VectorGlyphLod),
    /// Advances only the fixed-size phase uniform.
    AdvanceVectorPhase { frame_delta_seconds: f32 },
    /// Requests the explicit one-way migration from a legacy planar world.
    RegenerateAsSpherical,
}

/// Exact downstream work requested by one declarative canvas action.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SphericalCanvasInvalidation {
    map_geometry: bool,
    field_layers: bool,
    vector_glyphs: bool,
    presenter_uniform: bool,
    phase_uniform: bool,
    world_regeneration: bool,
}

impl SphericalCanvasInvalidation {
    /// No renderer or presentation work is required.
    pub const NONE: Self = Self::new(false, false, false, false, false, false);
    /// Switches the active pass or changes camera values through fixed uniforms only.
    pub const ACTIVE_PRESENTER_UNIFORM: Self = Self::new(false, false, false, true, false, false);
    /// Replaces only projected map geometry and its associated uniforms.
    pub const MAP_GEOMETRY: Self = Self::new(true, false, true, true, false, false);
    const MAP_GEOMETRY_AND_FIELD_LAYERS: Self = Self::new(true, true, true, true, false, false);
    /// Reconciles the shared field packet and any dependent glyph instances.
    pub const FIELD_LAYERS: Self = Self::new(false, true, true, true, false, false);
    /// Updates only the display animation phase uniform.
    pub const PHASE_UNIFORM: Self = Self::new(false, false, false, false, true, false);
    /// Requests a complete source-bound world candidate.
    pub const WORLD_REGENERATION: Self = Self::new(false, false, false, false, false, true);

    const fn new(
        map_geometry: bool,
        field_layers: bool,
        vector_glyphs: bool,
        presenter_uniform: bool,
        phase_uniform: bool,
        world_regeneration: bool,
    ) -> Self {
        Self {
            map_geometry,
            field_layers,
            vector_glyphs,
            presenter_uniform,
            phase_uniform,
            world_regeneration,
        }
    }

    /// Returns whether projected-map geometry needs a candidate replacement.
    pub const fn map_geometry(self) -> bool {
        self.map_geometry
    }

    /// Returns whether shared field layers need reconciliation.
    pub const fn field_layers(self) -> bool {
        self.field_layers
    }

    /// Returns whether vector glyph instances may need replacement.
    pub const fn vector_glyphs(self) -> bool {
        self.vector_glyphs
    }

    /// Returns whether the active presenter uniform changed.
    pub const fn presenter_uniform(self) -> bool {
        self.presenter_uniform
    }

    /// Returns whether only the vector phase uniform changed.
    pub const fn phase_uniform(self) -> bool {
        self.phase_uniform
    }

    /// Returns whether a complete world regeneration was explicitly requested.
    pub const fn world_regeneration(self) -> bool {
        self.world_regeneration
    }
}

/// Validated author/UI state retained independently for map and globe presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct SphericalCanvasState {
    view_mode: SphericalViewMode,
    projection: SphericalProjection,
    map_camera: MapCamera,
    globe_camera: GlobeCamera,
    field_state: SphericalFieldDisplayState,
    vector_animation: VectorAnimationUniform,
}

impl Default for SphericalCanvasState {
    fn default() -> Self {
        Self {
            view_mode: SphericalViewMode::Map,
            projection: SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0)
                .expect("the default projection is finite"),
            map_camera: MapCamera::default(),
            globe_camera: GlobeCamera::default(),
            field_state: SphericalFieldDisplayState::default(),
            vector_animation: VectorAnimationUniform::default(),
        }
    }
}

impl SphericalCanvasState {
    /// Applies one action and returns its exact presentation invalidation.
    pub fn apply(
        &mut self,
        action: SphericalCanvasAction,
    ) -> Result<SphericalCanvasInvalidation, SphericalUiError> {
        match action {
            SphericalCanvasAction::SetViewMode(mode) => {
                if self.view_mode == mode {
                    return Ok(SphericalCanvasInvalidation::NONE);
                }
                self.view_mode = mode;
                Ok(self.camera_invalidation())
            }
            SphericalCanvasAction::SetProjectionKind(kind) => {
                if self.projection.kind() == kind {
                    return Ok(SphericalCanvasInvalidation::NONE);
                }
                self.projection =
                    SphericalProjection::new(kind, self.projection.central_meridian())?;
                Ok(if self.camera_invalidation().field_layers() {
                    SphericalCanvasInvalidation::MAP_GEOMETRY_AND_FIELD_LAYERS
                } else {
                    SphericalCanvasInvalidation::MAP_GEOMETRY
                })
            }
            SphericalCanvasAction::SetCentralMeridianRadians(central_meridian) => {
                let next = SphericalProjection::new(self.projection.kind(), central_meridian)?;
                if next == self.projection {
                    return Ok(SphericalCanvasInvalidation::NONE);
                }
                self.projection = next;
                Ok(SphericalCanvasInvalidation::MAP_GEOMETRY)
            }
            SphericalCanvasAction::PanMap { delta } => {
                if !self.map_camera.pan_by(self.projection.kind(), delta) {
                    return Err(SphericalUiError::InvalidCameraInput);
                }
                Ok(SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM)
            }
            SphericalCanvasAction::ZoomMap { factor, anchor } => {
                if !self
                    .map_camera
                    .zoom_about(self.projection.kind(), factor, anchor)
                {
                    return Err(SphericalUiError::InvalidCameraInput);
                }
                Ok(self.camera_invalidation())
            }
            SphericalCanvasAction::ResetMap => {
                self.map_camera.reset(self.projection.kind());
                Ok(self.camera_invalidation())
            }
            SphericalCanvasAction::TrackballGlobe {
                start,
                end,
                canvas_size,
            } => {
                if !self.globe_camera.trackball_drag(start, end, canvas_size) {
                    return Ok(SphericalCanvasInvalidation::NONE);
                }
                Ok(SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM)
            }
            SphericalCanvasAction::ZoomGlobe { factor } => {
                if !self.globe_camera.zoom_by(factor) {
                    return Err(SphericalUiError::InvalidCameraInput);
                }
                Ok(self.camera_invalidation())
            }
            SphericalCanvasAction::ResetGlobe => {
                self.globe_camera.reset();
                Ok(self.camera_invalidation())
            }
            SphericalCanvasAction::SelectFill(field) => {
                self.field_state.select_fill(field);
                Ok(SphericalCanvasInvalidation::FIELD_LAYERS)
            }
            SphericalCanvasAction::SelectOverlay(field) => {
                self.field_state.select_overlay(field);
                Ok(SphericalCanvasInvalidation::FIELD_LAYERS)
            }
            SphericalCanvasAction::SetFillVisible(visible) => {
                if self.field_state.fill_visible() == visible {
                    return Ok(SphericalCanvasInvalidation::NONE);
                }
                self.field_state.set_fill_visible(visible);
                Ok(SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM)
            }
            SphericalCanvasAction::SetOverlayVisible(visible) => {
                if self.field_state.overlay_visible() == visible {
                    return Ok(SphericalCanvasInvalidation::NONE);
                }
                self.field_state.set_overlay_visible(visible);
                Ok(SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM)
            }
            SphericalCanvasAction::SetDiagnosticsEnabled(enabled) => {
                self.field_state.set_diagnostics_enabled(enabled);
                Ok(SphericalCanvasInvalidation::FIELD_LAYERS)
            }
            SphericalCanvasAction::SelectEntity(entity) => {
                let previous = self.field_state.selected_entity();
                if previous == entity {
                    return Ok(SphericalCanvasInvalidation::NONE);
                }
                self.field_state.select_entity(entity);
                let cell_bound = matches!(previous, Some(SelectedSurfaceEntity::Cell(_)))
                    || matches!(entity, Some(SelectedSurfaceEntity::Cell(_)));
                Ok(if cell_bound {
                    SphericalCanvasInvalidation::FIELD_LAYERS
                } else {
                    SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM
                })
            }
            SphericalCanvasAction::SetVectorPaused(paused) => {
                self.field_state.set_vector_paused(paused);
                Ok(SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM)
            }
            SphericalCanvasAction::SetVectorDisplaySpeed(speed) => {
                self.field_state.set_vector_display_speed(speed)?;
                Ok(SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM)
            }
            SphericalCanvasAction::SetVectorLod(lod) => {
                self.field_state.set_vector_lod(lod);
                Ok(SphericalCanvasInvalidation::FIELD_LAYERS)
            }
            SphericalCanvasAction::AdvanceVectorPhase {
                frame_delta_seconds,
            } => {
                let before = self.vector_animation.phase();
                self.vector_animation.advance(
                    frame_delta_seconds,
                    self.field_state.vector_display_speed(),
                    self.field_state.vector_paused(),
                );
                Ok(if self.vector_animation.phase() == before {
                    SphericalCanvasInvalidation::NONE
                } else {
                    SphericalCanvasInvalidation::PHASE_UNIFORM
                })
            }
            SphericalCanvasAction::RegenerateAsSpherical => {
                Ok(SphericalCanvasInvalidation::WORLD_REGENERATION)
            }
        }
    }

    fn camera_invalidation(&mut self) -> SphericalCanvasInvalidation {
        let before = crate::view::GlyphLodKey::for_zoom(
            self.field_state.vector_lod(),
            self.field_state.vector_view_zoom(),
        );
        self.field_state.sync_vector_view_zoom_from_cameras(
            self.view_mode,
            self.projection.kind(),
            self.map_camera,
            self.globe_camera,
        );
        let after = crate::view::GlyphLodKey::for_zoom(
            self.field_state.vector_lod(),
            self.field_state.vector_view_zoom(),
        );
        if before == after {
            SphericalCanvasInvalidation::ACTIVE_PRESENTER_UNIFORM
        } else {
            SphericalCanvasInvalidation::FIELD_LAYERS
        }
    }

    /// Returns the active map/globe presenter family.
    pub const fn view_mode(&self) -> SphericalViewMode {
        self.view_mode
    }

    /// Returns the validated map projection configuration.
    pub const fn projection(&self) -> SphericalProjection {
        self.projection
    }

    /// Returns both retained projection camera states.
    pub const fn map_camera(&self) -> MapCamera {
        self.map_camera
    }

    /// Returns the retained trackball globe camera.
    pub const fn globe_camera(&self) -> GlobeCamera {
        self.globe_camera
    }

    /// Returns shared fill/overlay/diagnostic/selection/vector preferences.
    pub const fn field_state(&self) -> &SphericalFieldDisplayState {
        &self.field_state
    }

    /// Returns the complete renderer-neutral view bound to a whole-world publication.
    pub const fn presentation_view_state(&self) -> SphericalPresentationViewState {
        SphericalPresentationViewState::new(
            self.view_mode,
            self.projection,
            self.map_camera,
            self.globe_camera,
        )
    }

    /// Returns the fixed-size display-only vector animation state.
    pub const fn vector_animation(&self) -> VectorAnimationUniform {
        self.vector_animation
    }

    /// Converts one active-view screen click into an authoritative cell or incident edge.
    pub fn pick_screen(
        &self,
        presentation: &PublishedSphericalPresentation,
        screen: [f64; 2],
        canvas_size: [f64; 2],
        pixels_per_point: f64,
    ) -> Option<SelectedSurfaceEntity> {
        if !pixels_per_point.is_finite()
            || pixels_per_point <= 0.0
            || screen.into_iter().any(|component| !component.is_finite())
            || canvas_size
                .into_iter()
                .any(|component| !component.is_finite() || component <= 0.0)
        {
            return None;
        }
        let direction = self.screen_direction(screen, canvas_size)?;
        let cell = presentation.locator().locate_cell(direction)?;
        if self.active_overlay_is_edge(presentation) {
            if let Some(edge) =
                self.pick_incident_edge_in_screen_space(presentation, cell, screen, canvas_size)
            {
                return Some(SelectedSurfaceEntity::Edge(edge));
            }
        }
        Some(SelectedSurfaceEntity::Cell(cell))
    }

    fn screen_direction(
        &self,
        screen: [f64; 2],
        canvas_size: [f64; 2],
    ) -> Option<crate::world::spatial::UnitVector3> {
        match self.view_mode {
            SphericalViewMode::Map => self
                .projection
                .inverse(self.map_projection_point(screen, canvas_size)?)
                .ok(),
            SphericalViewMode::Globe => self
                .globe_camera
                .screen_to_ray(screen, canvas_size)
                .and_then(crate::view::intersect_unit_sphere)
                .map(|hit| hit.direction()),
        }
    }

    fn map_projection_point(
        &self,
        screen: [f64; 2],
        canvas_size: [f64; 2],
    ) -> Option<crate::view::ProjectionPoint> {
        if screen[0] < 0.0
            || screen[0] > canvas_size[0]
            || screen[1] < 0.0
            || screen[1] > canvas_size[1]
        {
            return None;
        }
        let bounds = self.projection.bounds();
        let bounds_width = bounds.max_x() - bounds.min_x();
        let bounds_height = bounds.max_y() - bounds.min_y();
        let aspect = canvas_size[0] / canvas_size[1];
        let map_aspect = bounds_width / bounds_height;
        let (fit_x, fit_y) = if aspect >= map_aspect {
            (2.0 / (bounds_height * aspect), 2.0 / bounds_height)
        } else {
            (2.0 / bounds_width, 2.0 * aspect / bounds_width)
        };
        let zoom = self.map_camera.zoom(self.projection.kind());
        let pan = self.map_camera.pan(self.projection.kind());
        let ndc_x = 2.0 * screen[0] / canvas_size[0] - 1.0;
        let ndc_y = 1.0 - 2.0 * screen[1] / canvas_size[1];
        let center_x = (bounds.min_x() + bounds.max_x()) * 0.5;
        let center_y = (bounds.min_y() + bounds.max_y()) * 0.5;
        let point = crate::view::ProjectionPoint::new(
            (ndc_x - 2.0 * pan[0]) / (fit_x * zoom) + center_x,
            (ndc_y - 2.0 * pan[1]) / (fit_y * zoom) + center_y,
        );
        self.projection.outline_contains(point).then_some(point)
    }

    fn active_overlay_is_edge(&self, presentation: &PublishedSphericalPresentation) -> bool {
        let Some(field) = self.field_state.overlay_field() else {
            return false;
        };
        presentation
            .document()
            .catalog_for_ui()
            .ok()
            .and_then(|catalog| {
                catalog.get(field).and_then(|entry| {
                    entry.view().and_then(|view| {
                        classify_spherical_channel(view.schema().domain, view.schema().value_type)
                    })
                })
            })
            == Some(SphericalFieldChannel::EdgeOverlay)
    }

    fn pick_incident_edge_in_screen_space(
        &self,
        presentation: &PublishedSphericalPresentation,
        cell: CellId,
        screen: [f64; 2],
        canvas_size: [f64; 2],
    ) -> Option<crate::world::EdgeId> {
        let incident = presentation.document().surface_for_ui().cell_edges(cell)?;
        let mut closest: Option<(crate::world::EdgeId, f64)> = None;
        match self.view_mode {
            SphericalViewMode::Map => {
                for segment in presentation
                    .map()
                    .edge_segments()
                    .iter()
                    .filter(|segment| incident.contains(&segment.edge()))
                {
                    let start = self.map_projection_point_to_screen(segment.start(), canvas_size);
                    let end = self.map_projection_point_to_screen(segment.end(), canvas_size);
                    retain_closest_screen_edge(
                        &mut closest,
                        segment.edge(),
                        point_segment_distance(screen, start, end),
                    );
                }
            }
            SphericalViewMode::Globe => {
                for segment in presentation
                    .globe()
                    .edge_segments()
                    .iter()
                    .filter(|segment| incident.contains(&segment.edge()))
                {
                    let Some([start, end]) = self.globe_camera.project_visible_segment_to_screen(
                        segment.start(),
                        segment.end(),
                        canvas_size,
                    ) else {
                        continue;
                    };
                    retain_closest_screen_edge(
                        &mut closest,
                        segment.edge(),
                        point_segment_distance(screen, start, end),
                    );
                }
            }
        }
        closest
            .filter(|(_, distance)| *distance <= EDGE_PICK_TOLERANCE_LOGICAL_PIXELS)
            .map(|(edge, _)| edge)
    }

    fn map_projection_point_to_screen(
        &self,
        point: crate::view::ProjectionPoint,
        canvas_size: [f64; 2],
    ) -> [f64; 2] {
        let bounds = self.projection.bounds();
        let bounds_width = bounds.max_x() - bounds.min_x();
        let bounds_height = bounds.max_y() - bounds.min_y();
        let aspect = canvas_size[0] / canvas_size[1];
        let map_aspect = bounds_width / bounds_height;
        let (fit_x, fit_y) = if aspect >= map_aspect {
            (2.0 / (bounds_height * aspect), 2.0 / bounds_height)
        } else {
            (2.0 / bounds_width, 2.0 * aspect / bounds_width)
        };
        let zoom = self.map_camera.zoom(self.projection.kind());
        let pan = self.map_camera.pan(self.projection.kind());
        let center_x = (bounds.min_x() + bounds.max_x()) * 0.5;
        let center_y = (bounds.min_y() + bounds.max_y()) * 0.5;
        let ndc = [
            (point.x() - center_x) * fit_x * zoom + pan[0] * 2.0,
            (point.y() - center_y) * fit_y * zoom + pan[1] * 2.0,
        ];
        [
            (ndc[0] + 1.0) * canvas_size[0] * 0.5,
            (1.0 - ndc[1]) * canvas_size[1] * 0.5,
        ]
    }

    pub(crate) fn replace_field_state(&mut self, state: SphericalFieldDisplayState) {
        self.field_state = state;
    }
}

fn point_segment_distance(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_squared = delta[0].mul_add(delta[0], delta[1] * delta[1]);
    if !length_squared.is_finite() || length_squared <= 0.0 {
        return (point[0] - start[0]).hypot(point[1] - start[1]);
    }
    let from_start = [point[0] - start[0], point[1] - start[1]];
    let along =
        ((from_start[0] * delta[0] + from_start[1] * delta[1]) / length_squared).clamp(0.0, 1.0);
    let closest = [start[0] + along * delta[0], start[1] + along * delta[1]];
    (point[0] - closest[0]).hypot(point[1] - closest[1])
}

fn retain_closest_screen_edge(
    closest: &mut Option<(crate::world::EdgeId, f64)>,
    edge: crate::world::EdgeId,
    distance: f64,
) {
    if !distance.is_finite() {
        return;
    }
    let replace = closest.is_none_or(|(current_edge, current_distance)| {
        distance < current_distance || (distance == current_distance && edge < current_edge)
    });
    if replace {
        *closest = Some((edge, distance));
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct SphericalCanvasStateWire {
    view_mode: SphericalViewMode,
    projection_kind: SphericalProjectionKind,
    central_meridian_radians: f64,
    map_cameras: MapCamerasWire,
    globe_camera: GlobeCameraWire,
    field_state: SphericalFieldStateWire,
    vector_phase: f32,
}

impl Default for SphericalCanvasStateWire {
    fn default() -> Self {
        Self::from_state(&SphericalCanvasState::default())
    }
}

impl SphericalCanvasStateWire {
    fn from_state(state: &SphericalCanvasState) -> Self {
        Self {
            view_mode: state.view_mode,
            projection_kind: state.projection.kind(),
            central_meridian_radians: state.projection.central_meridian(),
            map_cameras: MapCamerasWire::from_camera(state.map_camera),
            globe_camera: GlobeCameraWire::from_camera(state.globe_camera),
            field_state: SphericalFieldStateWire::from_state(&state.field_state),
            vector_phase: state.vector_animation.phase(),
        }
    }

    fn try_into_state(self) -> Result<SphericalCanvasState, SphericalUiError> {
        Ok(SphericalCanvasState {
            view_mode: self.view_mode,
            projection: SphericalProjection::new(
                self.projection_kind,
                self.central_meridian_radians,
            )?,
            map_camera: self.map_cameras.try_into_camera()?,
            globe_camera: self.globe_camera.try_into_camera()?,
            field_state: self.field_state.try_into_state()?,
            vector_animation: VectorAnimationUniform::new(self.vector_phase),
        })
    }
}

impl Serialize for SphericalCanvasState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SphericalCanvasStateWire::from_state(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SphericalCanvasState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SphericalCanvasStateWire::deserialize(deserializer)?
            .try_into_state()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct MapCamerasWire {
    equal_earth_pan: [f64; 2],
    equal_earth_zoom: f64,
    equirectangular_pan: [f64; 2],
    equirectangular_zoom: f64,
}

impl Default for MapCamerasWire {
    fn default() -> Self {
        Self::from_camera(MapCamera::default())
    }
}

impl MapCamerasWire {
    fn from_camera(camera: MapCamera) -> Self {
        Self {
            equal_earth_pan: camera.pan(SphericalProjectionKind::EqualEarth),
            equal_earth_zoom: camera.zoom(SphericalProjectionKind::EqualEarth),
            equirectangular_pan: camera.pan(SphericalProjectionKind::Equirectangular),
            equirectangular_zoom: camera.zoom(SphericalProjectionKind::Equirectangular),
        }
    }

    fn try_into_camera(self) -> Result<MapCamera, SphericalUiError> {
        let mut camera = MapCamera::default();
        for (kind, pan, zoom) in [
            (
                SphericalProjectionKind::EqualEarth,
                self.equal_earth_pan,
                self.equal_earth_zoom,
            ),
            (
                SphericalProjectionKind::Equirectangular,
                self.equirectangular_pan,
                self.equirectangular_zoom,
            ),
        ] {
            if !camera.zoom_by(kind, zoom) || !camera.pan_by(kind, pan) {
                return Err(SphericalUiError::InvalidPersistedCamera);
            }
        }
        Ok(camera)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct GlobeCameraWire {
    orientation_xyzw: [f64; 4],
    scale: f64,
}

impl Default for GlobeCameraWire {
    fn default() -> Self {
        Self::from_camera(GlobeCamera::default())
    }
}

impl GlobeCameraWire {
    fn from_camera(camera: GlobeCamera) -> Self {
        Self {
            orientation_xyzw: camera.orientation_xyzw(),
            scale: camera.orthographic_scale(),
        }
    }

    fn try_into_camera(self) -> Result<GlobeCamera, SphericalUiError> {
        GlobeCamera::from_orientation_xyzw(self.orientation_xyzw, self.scale)
            .ok_or(SphericalUiError::InvalidPersistedCamera)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct SphericalFieldStateWire {
    fill_field: Option<FieldId>,
    overlay_field: Option<FieldId>,
    fill_visible: bool,
    overlay_visible: bool,
    range_mode: DisplayRangeMode,
    palette_override: Option<PaletteId>,
    diagnostics_enabled: bool,
    diagnostic_scope: DiagnosticScope,
    selected_entity: Option<SelectedSurfaceEntity>,
    vector_lod: VectorGlyphLod,
    vector_view_zoom: f64,
    vector_paused: bool,
    vector_display_speed: f32,
}

impl Default for SphericalFieldStateWire {
    fn default() -> Self {
        Self::from_state(&SphericalFieldDisplayState::default())
    }
}

impl SphericalFieldStateWire {
    fn from_state(state: &SphericalFieldDisplayState) -> Self {
        Self {
            fill_field: state.fill_field().cloned(),
            overlay_field: state.overlay_field().cloned(),
            fill_visible: state.fill_visible(),
            overlay_visible: state.overlay_visible(),
            range_mode: state.range_mode(),
            palette_override: state.palette_override(),
            diagnostics_enabled: state.diagnostics_enabled(),
            diagnostic_scope: state.diagnostic_scope(),
            selected_entity: state.selected_entity(),
            vector_lod: state.vector_lod(),
            vector_view_zoom: state.vector_view_zoom(),
            vector_paused: state.vector_paused(),
            vector_display_speed: state.vector_display_speed(),
        }
    }

    fn try_into_state(self) -> Result<SphericalFieldDisplayState, SphericalUiError> {
        let mut state = SphericalFieldDisplayState::default();
        if let Some(field) = self.fill_field {
            state.select_fill(field);
        }
        state.select_overlay(self.overlay_field);
        state.set_fill_visible(self.fill_visible);
        state.set_overlay_visible(self.overlay_visible);
        state.set_range_mode(self.range_mode);
        state.set_palette_override(self.palette_override);
        state.set_diagnostics_enabled(self.diagnostics_enabled);
        state.set_diagnostic_scope(self.diagnostic_scope);
        state.select_entity(self.selected_entity);
        state.set_vector_lod(self.vector_lod);
        state.set_vector_view_zoom(self.vector_view_zoom)?;
        state.set_vector_paused(self.vector_paused);
        state.set_vector_display_speed(self.vector_display_speed)?;
        Ok(state)
    }
}

/// One fill field or edge/vector overlay presented in a product control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphericalFieldControl {
    field: FieldId,
    label: String,
}

impl SphericalFieldControl {
    /// Returns the stable field identity.
    pub const fn field(&self) -> &FieldId {
        &self.field
    }

    /// Returns the localized product label.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// The semantic channel selected by one overlay control row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SphericalOverlayControlKind {
    /// No overlay.
    None,
    /// An authoritative edge scalar/category annotation.
    Edge,
    /// An authoritative cell vector encoded as dynamic arrows.
    Vector,
}

/// One optional overlay choice, including the explicit no-overlay entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphericalOverlayControl {
    field: Option<FieldId>,
    label: String,
    kind: SphericalOverlayControlKind,
}

impl SphericalOverlayControl {
    /// Returns the optional stable field identity.
    pub const fn field(&self) -> Option<&FieldId> {
        self.field.as_ref()
    }

    /// Returns the localized product label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the overlay channel.
    pub const fn kind(&self) -> SphericalOverlayControlKind {
        self.kind
    }
}

/// Stable control rows derived from the authoritative document catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphericalControlCatalog {
    fill_fields: Vec<SphericalFieldControl>,
    overlay_fields: Vec<SphericalOverlayControl>,
}

impl SphericalControlCatalog {
    /// Returns exactly the supported cell-fill fields.
    pub fn fill_fields(&self) -> &[SphericalFieldControl] {
        &self.fill_fields
    }

    /// Returns none followed by every supported edge/vector overlay.
    pub fn overlay_fields(&self) -> &[SphericalOverlayControl] {
        &self.overlay_fields
    }

    /// Returns whether the catalog contains an overlay field.
    pub fn contains_overlay(&self, field: &FieldId) -> bool {
        self.overlay_fields
            .iter()
            .any(|control| control.field.as_ref() == Some(field))
    }
}

/// Builds the fill/overlay controls from the published document, never from GPU buffers.
pub fn build_spherical_control_catalog(
    presentation: &PublishedSphericalPresentation,
) -> Result<SphericalControlCatalog, SphericalUiError> {
    let catalog = presentation.document().catalog_for_ui()?;
    let mut fill_fields = Vec::new();
    let mut overlay_fields = vec![SphericalOverlayControl {
        field: None,
        label: "无".to_owned(),
        kind: SphericalOverlayControlKind::None,
    }];
    for entry in catalog.entries() {
        let Some(view) = entry.view() else {
            continue;
        };
        let Some(channel) =
            classify_spherical_channel(view.schema().domain, view.schema().value_type)
        else {
            continue;
        };
        let field = view.schema().id.clone();
        let label = localized_field_key(view.schema().display.label_key()).into_owned();
        match channel {
            SphericalFieldChannel::CellFill => {
                fill_fields.push(SphericalFieldControl { field, label });
            }
            SphericalFieldChannel::EdgeOverlay => {
                overlay_fields.push(SphericalOverlayControl {
                    field: Some(field),
                    label,
                    kind: SphericalOverlayControlKind::Edge,
                });
            }
            SphericalFieldChannel::VectorOverlay => {
                overlay_fields.push(SphericalOverlayControl {
                    field: Some(field),
                    label,
                    kind: SphericalOverlayControlKind::Vector,
                });
            }
        }
    }
    Ok(SphericalControlCatalog {
        fill_fields,
        overlay_fields,
    })
}

/// Applies one validated UI action through Task 9 candidate/publication boundaries.
///
/// Camera/view/phase actions retain the exact published packet. Projection actions replace only
/// map geometry. Field-bearing actions use the shared field candidate and concrete renderer
/// adapter, so CPU state changes only after GPU preparation succeeds.
pub fn apply_spherical_canvas_action(
    presentation: &mut PublishedSphericalPresentation,
    renderer: &mut SphericalFieldRenderer,
    device: &eframe::egui_wgpu::wgpu::Device,
    queue: &eframe::egui_wgpu::wgpu::Queue,
    state: &mut SphericalCanvasState,
    action: SphericalCanvasAction,
) -> Result<SphericalCanvasInvalidation, SphericalUiError> {
    if let SphericalCanvasAction::SelectEntity(Some(SelectedSurfaceEntity::Edge(edge))) = &action {
        if presentation
            .document()
            .surface_for_ui()
            .edge(*edge)
            .is_none()
        {
            return Err(SphericalUiError::EntityValueMissing);
        }
        if !inspector_overlay_field_is_edge(presentation, state.field_state().overlay_field())? {
            return Err(SphericalUiError::UnsupportedInspectorEntity);
        }
    }

    let mut candidate_state = state.clone();
    if let SphericalCanvasAction::SelectOverlay(field) = &action {
        if matches!(
            candidate_state.field_state().selected_entity(),
            Some(SelectedSurfaceEntity::Edge(_))
        ) && !inspector_overlay_field_is_edge(presentation, field.as_ref())?
        {
            candidate_state.apply(SphericalCanvasAction::SelectEntity(None))?;
        }
    }
    let invalidation = candidate_state.apply(action)?;
    if invalidation.map_geometry() {
        let candidate = presentation.prepare_projection_candidate_for_view(
            candidate_state.presentation_view_state(),
            crate::view::SphericalMeshBudgets::DEFAULT,
        )?;
        let mut gpu = crate::app::SphericalRendererPreparer::new(renderer, device, queue);
        presentation.try_replace_projection_candidate(candidate, &mut gpu)?;
        candidate_state.replace_field_state(presentation.state().clone());
    } else if invalidation.field_layers() {
        let candidate = presentation.prepare_field_candidate_for_view(
            candidate_state.field_state().clone(),
            candidate_state.presentation_view_state(),
        )?;
        let mut gpu = crate::app::SphericalRendererPreparer::new(renderer, device, queue);
        presentation.try_replace_field_candidate(candidate, &mut gpu)?;
        candidate_state.replace_field_state(presentation.state().clone());
    } else {
        presentation.reconcile_uniform_only_state(
            candidate_state.field_state().clone(),
            candidate_state.presentation_view_state(),
        )?;
    }
    *state = candidate_state;
    Ok(invalidation)
}

fn inspector_overlay_field_is_edge(
    presentation: &PublishedSphericalPresentation,
    field: Option<&FieldId>,
) -> Result<bool, SphericalUiError> {
    let Some(field) = field else {
        return Ok(false);
    };
    let catalog = presentation.document().catalog_for_ui()?;
    Ok(catalog
        .get(field)
        .is_some_and(|entry| entry.schema().domain == FieldDomain::Edges))
}

/// The response and deferred product actions produced by one canvas frame.
///
/// Canvas input and callback queuing are deliberately a two-stage public protocol. The former
/// one-shot entry point is unavailable because it necessarily captured a packet/camera before the
/// caller could publish the actions it returned.
///
/// ```compile_fail
/// use sekai::ui::spherical::show_spherical_canvas;
/// ```
pub struct SphericalCanvasOutput {
    response: egui::Response,
    actions: Vec<SphericalCanvasAction>,
}

impl SphericalCanvasOutput {
    /// Returns the egui response for the canvas allocation.
    pub const fn response(&self) -> &egui::Response {
        &self.response
    }

    /// Drains actions that require publication orchestration by the app.
    pub fn into_actions(self) -> Vec<SphericalCanvasAction> {
        self.actions
    }
}

/// Allocates the canvas and collects input without capturing a deferred GPU packet yet.
///
/// The caller must publish [`SphericalCanvasOutput::into_actions`] before calling
/// [`queue_spherical_canvas_callback`] with the re-read current publication.
pub fn interact_spherical_canvas(
    ui: &mut egui::Ui,
    presentation: &PublishedSphericalPresentation,
    state: &mut SphericalCanvasState,
) -> SphericalCanvasOutput {
    let mut actions = Vec::new();
    ui.horizontal(|ui| {
        for (mode, label) in [
            (SphericalViewMode::Map, "二维地图"),
            (SphericalViewMode::Globe, "三维球体"),
        ] {
            if ui
                .selectable_label(state.view_mode() == mode, label)
                .clicked()
            {
                actions.push(SphericalCanvasAction::SetViewMode(mode));
            }
        }
    });
    let desired_size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());
    let canvas_size = [f64::from(rect.width()), f64::from(rect.height())];
    let pixels_per_point = f64::from(ui.ctx().pixels_per_point());

    if response.clicked_by(egui::PointerButton::Primary) {
        if let Some(position) = response.interact_pointer_pos() {
            let local = [
                f64::from(position.x - rect.min.x),
                f64::from(position.y - rect.min.y),
            ];
            let selected = state.pick_screen(presentation, local, canvas_size, pixels_per_point);
            actions.push(SphericalCanvasAction::SelectEntity(selected));
        }
    }
    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = ui.input(|input| input.pointer.delta());
        match state.view_mode() {
            SphericalViewMode::Map => actions.push(SphericalCanvasAction::PanMap {
                delta: [
                    f64::from(delta.x / rect.width()),
                    -f64::from(delta.y / rect.height()),
                ],
            }),
            SphericalViewMode::Globe => {
                if let Some(current) = response.interact_pointer_pos() {
                    let end = [
                        f64::from(current.x - rect.min.x),
                        f64::from(current.y - rect.min.y),
                    ];
                    let start = [end[0] - f64::from(delta.x), end[1] - f64::from(delta.y)];
                    actions.push(SphericalCanvasAction::TrackballGlobe {
                        start,
                        end,
                        canvas_size,
                    });
                }
            }
        }
    }
    if response.hovered() {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let factor = f64::from((scroll * 0.002).exp());
            let anchor = response
                .hover_pos()
                .map(|position| {
                    [
                        2.0 * f64::from(position.x - rect.min.x) / canvas_size[0] - 1.0,
                        1.0 - 2.0 * f64::from(position.y - rect.min.y) / canvas_size[1],
                    ]
                })
                .unwrap_or([0.0, 0.0]);
            actions.push(match state.view_mode() {
                SphericalViewMode::Map => SphericalCanvasAction::ZoomMap { factor, anchor },
                SphericalViewMode::Globe => SphericalCanvasAction::ZoomGlobe { factor },
            });
        }
    }

    if presentation.layers().overlay_kind() == Some(crate::view::PreparedOverlayKind::CellVector)
        && !state.field_state().vector_paused()
    {
        let delta = ui.input(|input| input.stable_dt);
        let _ = state.apply(SphericalCanvasAction::AdvanceVectorPhase {
            frame_delta_seconds: delta,
        });
        ui.ctx().request_repaint();
    }

    SphericalCanvasOutput { response, actions }
}

/// Queues exactly one callback after the app has published all actions from this frame.
pub fn queue_spherical_canvas_callback(
    ui: &mut egui::Ui,
    presentation: &PublishedSphericalPresentation,
    state: &SphericalCanvasState,
    rect: egui::Rect,
) {
    let pixels_per_point = f64::from(ui.ctx().pixels_per_point());
    let viewport = [
        (f64::from(rect.width()) * pixels_per_point)
            .round()
            .clamp(1.0, f64::from(u32::MAX)) as u32,
        (f64::from(rect.height()) * pixels_per_point)
            .round()
            .clamp(1.0, f64::from(u32::MAX)) as u32,
    ];
    let mode = match state.view_mode() {
        SphericalViewMode::Map => SphericalRenderMode::Map,
        SphericalViewMode::Globe => SphericalRenderMode::Globe,
    };
    let callback = SphericalPaintCallback::new(
        std::sync::Arc::clone(presentation.gpu_packet_arc()),
        mode,
        state.map_camera(),
        state.globe_camera(),
        viewport,
    )
    .with_vector_animation(state.vector_animation())
    .with_layer_visibility(state.field_state().layer_visibility());
    ui.painter()
        .add(eframe::egui_wgpu::Callback::new_paint_callback(
            rect, callback,
        ));
}

/// Draws projection, field, overlay, diagnostic, and vector controls for the single canvas.
pub fn show_spherical_controls(
    ui: &mut egui::Ui,
    presentation: &PublishedSphericalPresentation,
    state: &SphericalCanvasState,
) -> Result<Vec<SphericalCanvasAction>, SphericalUiError> {
    let controls = build_spherical_control_catalog(presentation)?;
    let mut actions = Vec::new();
    match state.view_mode() {
        SphericalViewMode::Map => {
            let mut kind = state.projection().kind();
            egui::ComboBox::from_label("地图投影")
                .selected_text(match kind {
                    SphericalProjectionKind::EqualEarth => "Equal Earth",
                    SphericalProjectionKind::Equirectangular => "Equirectangular（诊断）",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut kind,
                        SphericalProjectionKind::EqualEarth,
                        "Equal Earth",
                    );
                    ui.selectable_value(
                        &mut kind,
                        SphericalProjectionKind::Equirectangular,
                        "Equirectangular（诊断）",
                    );
                });
            if kind != state.projection().kind() {
                actions.push(SphericalCanvasAction::SetProjectionKind(kind));
            }
            let mut degrees = state.projection().central_meridian().to_degrees();
            if ui
                .add(egui::Slider::new(&mut degrees, -180.0..=180.0).text("中央经线"))
                .changed()
            {
                actions.push(SphericalCanvasAction::SetCentralMeridianRadians(
                    degrees.to_radians(),
                ));
            }
            if ui.button("重置地图").clicked() {
                actions.push(SphericalCanvasAction::ResetMap);
            }
        }
        SphericalViewMode::Globe => {
            if ui.button("重置球体").clicked() {
                actions.push(SphericalCanvasAction::ResetGlobe);
            }
        }
    }

    let selected_fill = state.field_state().fill_field();
    egui::ComboBox::from_label("填色")
        .selected_text(
            controls
                .fill_fields()
                .iter()
                .find(|control| Some(control.field()) == selected_fill)
                .map_or("未选择", SphericalFieldControl::label),
        )
        .show_ui(ui, |ui| {
            for control in controls.fill_fields() {
                if ui
                    .selectable_label(Some(control.field()) == selected_fill, control.label())
                    .clicked()
                {
                    actions.push(SphericalCanvasAction::SelectFill(control.field().clone()));
                }
            }
        });

    let selected_overlay = state.field_state().overlay_field();
    egui::ComboBox::from_label("叠加")
        .selected_text(
            controls
                .overlay_fields()
                .iter()
                .find(|control| control.field() == selected_overlay)
                .map_or("无", SphericalOverlayControl::label),
        )
        .show_ui(ui, |ui| {
            for control in controls.overlay_fields() {
                if ui
                    .selectable_label(control.field() == selected_overlay, control.label())
                    .clicked()
                {
                    actions.push(SphericalCanvasAction::SelectOverlay(
                        control.field().cloned(),
                    ));
                }
            }
        });

    ui.group(|ui| {
        ui.label("显示图层");

        let mut fill_visible = state.field_state().fill_visible();
        if ui.checkbox(&mut fill_visible, "显示填色").changed() {
            actions.push(SphericalCanvasAction::SetFillVisible(fill_visible));
        }

        let mut overlay_visible = state.field_state().overlay_visible();
        if ui
            .add_enabled(
                selected_overlay.is_some(),
                egui::Checkbox::new(&mut overlay_visible, "显示叠加"),
            )
            .changed()
        {
            actions.push(SphericalCanvasAction::SetOverlayVisible(overlay_visible));
        }

        let mut diagnostics_enabled = state.field_state().diagnostics_enabled();
        if ui.checkbox(&mut diagnostics_enabled, "显示诊断").changed() {
            actions.push(SphericalCanvasAction::SetDiagnosticsEnabled(
                diagnostics_enabled,
            ));
        }
    });

    let vector_active = controls.overlay_fields().iter().any(|control| {
        control.field() == selected_overlay && control.kind() == SphericalOverlayControlKind::Vector
    });
    if vector_active {
        ui.separator();
        let paused = state.field_state().vector_paused();
        if ui.button(if paused { "播放" } else { "暂停" }).clicked() {
            actions.push(SphericalCanvasAction::SetVectorPaused(!paused));
        }
        let mut speed = state.field_state().vector_display_speed();
        if ui
            .add(egui::Slider::new(&mut speed, 0.0..=4.0).text(VECTOR_DISPLAY_SPEED_LABEL))
            .changed()
        {
            actions.push(SphericalCanvasAction::SetVectorDisplaySpeed(speed));
        }
        ui.label("Glyph 密度");
        ui.horizontal(|ui| {
            for (lod, label) in [
                (VectorGlyphLod::Low, GLYPH_DENSITY_LABELS[0]),
                (VectorGlyphLod::Medium, GLYPH_DENSITY_LABELS[1]),
                (VectorGlyphLod::High, GLYPH_DENSITY_LABELS[2]),
            ] {
                if ui
                    .selectable_label(state.field_state().vector_lod() == lod, label)
                    .clicked()
                {
                    actions.push(SphericalCanvasAction::SetVectorLod(lod));
                }
            }
        });
    }
    Ok(actions)
}

/// Draws the already formatted, renderer-independent spherical inspector model.
pub fn show_spherical_inspector(ui: &mut egui::Ui, model: &SphericalInspectorModel) {
    ui.heading("实体检查");
    match model.entity() {
        Some(SelectedSurfaceEntity::Cell(cell)) => {
            ui.label(format!("Cell {}", cell.raw()));
        }
        Some(SelectedSurfaceEntity::Edge(edge)) => {
            ui.label(format!("Edge {}", edge.raw()));
        }
        None => {
            ui.label("点击地图或球体以检查实体");
        }
    }
    for row in model.rows() {
        ui.label(format!("{}: {}", row.label(), row.value()));
    }
    for diagnostic in model.diagnostics() {
        ui.label(format!(
            "{:?} · {} · {}",
            diagnostic.severity(),
            diagnostic.code(),
            diagnostic.message()
        ));
    }
}

/// Compatibility UI copy for an explicitly legacy world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyCompatibilityUi {
    notice: &'static str,
    action_label: &'static str,
}

impl LegacyCompatibilityUi {
    /// Returns the non-silent legacy-origin notice.
    pub const fn notice(self) -> &'static str {
        self.notice
    }

    /// Returns the sole explicit one-way regeneration action label.
    pub const fn action_label(self) -> &'static str {
        self.action_label
    }
}

/// Returns compatibility UI only for an actual legacy origin.
pub fn legacy_compatibility_ui(app: &crate::TemplateApp) -> Option<LegacyCompatibilityUi> {
    app.legacy_compatibility_notice()
        .map(|notice| LegacyCompatibilityUi {
            notice,
            action_label: "用当前作者参数重新生成球面世界",
        })
}

/// One formatted inspector row with a stable product label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphericalInspectorRow {
    label: &'static str,
    value: String,
}

impl SphericalInspectorRow {
    /// Returns the row label.
    pub const fn label(&self) -> &'static str {
        self.label
    }

    /// Returns the formatted authoritative value.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// One document diagnostic visible for the inspected entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphericalInspectorDiagnostic {
    severity: ViewDiagnosticSeverity,
    code: String,
    message: String,
    cell: Option<CellId>,
}

impl SphericalInspectorDiagnostic {
    /// Returns an optional authoritative cell context.
    pub const fn cell(&self) -> Option<CellId> {
        self.cell
    }

    /// Returns diagnostic severity.
    pub const fn severity(&self) -> ViewDiagnosticSeverity {
        self.severity
    }

    /// Returns the stable diagnostic code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns diagnostic copy.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Renderer-independent inspector data derived from catalog payloads and authoritative topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SphericalInspectorModel {
    entity: Option<SelectedSurfaceEntity>,
    rows: Vec<SphericalInspectorRow>,
    diagnostics: Vec<SphericalInspectorDiagnostic>,
}

impl SphericalInspectorModel {
    /// Returns the selected authoritative entity.
    pub const fn entity(&self) -> Option<SelectedSurfaceEntity> {
        self.entity
    }

    /// Returns formatted rows in stable display order.
    pub fn rows(&self) -> &[SphericalInspectorRow] {
        &self.rows
    }

    /// Returns whether a row is present.
    pub fn has_row(&self, label: &str) -> bool {
        self.rows.iter().any(|row| row.label == label)
    }

    /// Returns diagnostics filtered according to cell/edge contracts.
    pub fn diagnostics(&self) -> &[SphericalInspectorDiagnostic] {
        &self.diagnostics
    }
}

/// Per-application inspector cache keyed only by authoritative model inputs.
///
/// Camera, presenter mode, and animation phase do not participate. Source allocations are held
/// weakly so replacing a whole publication cannot retain obsolete documents or field layers.
#[derive(Default)]
pub(crate) struct SphericalInspectorCache {
    entry: Option<SphericalInspectorCacheEntry>,
    model_rebuilds: usize,
    diagnostic_values_scanned: usize,
}

struct SphericalInspectorCacheEntry {
    source: crate::view::SphericalPresentationSource,
    document: Weak<crate::app::SphericalWorldFieldDocument>,
    layers: Weak<PreparedFieldLayers>,
    fill_field: Option<FieldId>,
    overlay_field: Option<FieldId>,
    selected_entity: Option<SelectedSurfaceEntity>,
    model: SphericalInspectorModel,
}

impl SphericalInspectorCacheEntry {
    fn matches(
        &self,
        presentation: &PublishedSphericalPresentation,
        state: &SphericalFieldDisplayState,
    ) -> bool {
        self.source == *presentation.source()
            && self.document.as_ptr() == Arc::as_ptr(presentation.document_arc())
            && self.layers.as_ptr() == Arc::as_ptr(presentation.layers_arc())
            && self.fill_field.as_ref() == state.fill_field()
            && self.overlay_field.as_ref() == state.overlay_field()
            && self.selected_entity == state.selected_entity()
    }
}

impl SphericalInspectorCache {
    /// Returns a borrowed stable model, rebuilding only when authoritative inspector inputs change.
    pub(crate) fn model(
        &mut self,
        presentation: &PublishedSphericalPresentation,
        state: &SphericalFieldDisplayState,
        mode: SphericalViewMode,
    ) -> Result<&SphericalInspectorModel, SphericalUiError> {
        self.model_with_diagnostics(
            presentation,
            state,
            mode,
            presentation.document().diagnostics_for_ui(),
        )
    }

    fn model_with_diagnostics(
        &mut self,
        presentation: &PublishedSphericalPresentation,
        state: &SphericalFieldDisplayState,
        mode: SphericalViewMode,
        diagnostics: &[OwnedViewDiagnostic],
    ) -> Result<&SphericalInspectorModel, SphericalUiError> {
        if self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.matches(presentation, state))
        {
            return Ok(&self.entry.as_ref().expect("cache hit").model);
        }

        self.diagnostic_values_scanned += diagnostics.len();
        let model = build_spherical_inspector_model_with_diagnostics(
            presentation,
            state,
            mode,
            diagnostics,
        )?;
        self.model_rebuilds += 1;
        self.entry = Some(SphericalInspectorCacheEntry {
            source: presentation.source().clone(),
            document: Arc::downgrade(presentation.document_arc()),
            layers: Arc::downgrade(presentation.layers_arc()),
            fill_field: state.fill_field().cloned(),
            overlay_field: state.overlay_field().cloned(),
            selected_entity: state.selected_entity(),
            model,
        });
        Ok(&self.entry.as_ref().expect("cache was installed").model)
    }

    #[cfg(test)]
    pub(crate) fn model_with_diagnostics_for_test(
        &mut self,
        presentation: &PublishedSphericalPresentation,
        state: &SphericalFieldDisplayState,
        mode: SphericalViewMode,
        diagnostics: &[OwnedViewDiagnostic],
    ) -> Result<&SphericalInspectorModel, SphericalUiError> {
        self.model_with_diagnostics(presentation, state, mode, diagnostics)
    }

    #[cfg(test)]
    pub(crate) const fn probe_for_test(&self) -> (usize, usize) {
        (self.model_rebuilds, self.diagnostic_values_scanned)
    }
}

/// Formats the same authoritative inspector model for map and globe presentation.
pub fn build_spherical_inspector_model(
    presentation: &PublishedSphericalPresentation,
    state: &SphericalFieldDisplayState,
    _mode: SphericalViewMode,
) -> Result<SphericalInspectorModel, SphericalUiError> {
    let diagnostics = presentation.document().diagnostics_for_ui();
    build_spherical_inspector_model_with_diagnostics(presentation, state, _mode, diagnostics)
}

fn build_spherical_inspector_model_with_diagnostics(
    presentation: &PublishedSphericalPresentation,
    state: &SphericalFieldDisplayState,
    _mode: SphericalViewMode,
    diagnostics: &[OwnedViewDiagnostic],
) -> Result<SphericalInspectorModel, SphericalUiError> {
    let catalog = presentation.document().catalog_for_ui()?;
    let surface = presentation.document().surface_for_ui();
    let entity = state.selected_entity();
    let mut rows = Vec::new();

    match entity {
        Some(SelectedSurfaceEntity::Cell(cell)) => {
            let index = cell.raw() as usize;
            if let Some(fill) = state
                .fill_field()
                .and_then(|field| catalog.get(field))
                .and_then(|entry| entry.view())
            {
                let value =
                    format_field_value(fill, index).ok_or(SphericalUiError::EntityValueMissing)?;
                rows.push(SphericalInspectorRow {
                    label: "填色值",
                    value: value.text,
                });
                rows.push(SphericalInspectorRow {
                    label: "填色单位",
                    value: value.unit,
                });
                rows.push(SphericalInspectorRow {
                    label: "填色字段来源",
                    value: format_field_id(&fill.schema().id),
                });
            }
            if let Some(vector) = state
                .overlay_field()
                .and_then(|field| catalog.get(field))
                .and_then(|entry| entry.view())
                .filter(|view| view.schema().domain == FieldDomain::Cells)
                .and_then(|view| view.value(index).map(|value| (view.schema(), value)))
                .and_then(|(schema, value)| match value {
                    FieldValue::Vector2(components) => Some((schema, components)),
                    _ => None,
                })
            {
                let (schema, [east, north]) = vector;
                let magnitude = east.hypot(north);
                let angle = f64::from(east).atan2(f64::from(north)).to_degrees();
                rows.push(row("东向分量", east));
                rows.push(row("北向分量", north));
                rows.push(row("模长", magnitude));
                rows.push(SphericalInspectorRow {
                    label: "方向角",
                    value: format!("{:.3}°", angle.rem_euclid(360.0)),
                });
                rows.push(SphericalInspectorRow {
                    label: "向量单位",
                    value: schema.unit.symbol().to_owned(),
                });
                rows.push(SphericalInspectorRow {
                    label: "向量字段来源",
                    value: format_field_id(&schema.id),
                });
            }
        }
        Some(SelectedSurfaceEntity::Edge(edge)) => {
            let index = edge.raw() as usize;
            let view = state
                .overlay_field()
                .and_then(|field| catalog.get(field))
                .and_then(|entry| entry.view())
                .filter(|view| view.schema().domain == FieldDomain::Edges)
                .ok_or(SphericalUiError::UnsupportedInspectorEntity)?;
            let value =
                format_field_value(view, index).ok_or(SphericalUiError::EntityValueMissing)?;
            rows.push(SphericalInspectorRow {
                label: "边值",
                value: value.text,
            });
            let owners = surface
                .edge(edge)
                .ok_or(SphericalUiError::EntityValueMissing)?
                .cells;
            rows.push(SphericalInspectorRow {
                label: "Owners",
                value: format!("{}, {}", owners[0].raw(), owners[1].raw()),
            });
            rows.push(SphericalInspectorRow {
                label: "单位",
                value: value.unit,
            });
        }
        None => {
            if let Some(fill_id) = state.fill_field() {
                let schema = catalog
                    .get(fill_id)
                    .ok_or(SphericalUiError::EntityValueMissing)?
                    .schema();
                let fill = presentation.layers().fill();
                push_unselected_field_summary(
                    &mut rows,
                    InspectorSummaryLabels::FILL,
                    schema,
                    fill.display_range(),
                    state
                        .palette_override()
                        .map(|palette| format!("{palette:?}"))
                        .unwrap_or_else(|| format!("{:?}", schema.display.palette())),
                    presentation.layers().fill_palette().len(),
                    fill.category_keys(),
                );
            }
            if let Some(overlay_id) = state.overlay_field() {
                let schema = catalog
                    .get(overlay_id)
                    .ok_or(SphericalUiError::EntityValueMissing)?
                    .schema();
                let (range, category_keys) = match presentation.layers().overlay() {
                    Some(crate::view::PreparedSphericalOverlay::Edge(field))
                        if field.field_id() == overlay_id =>
                    {
                        (field.display_range(), field.category_keys())
                    }
                    Some(crate::view::PreparedSphericalOverlay::Vector(field))
                        if field.field_id() == overlay_id =>
                    {
                        (Some(field.display_range()), &[][..])
                    }
                    _ => (None, &[][..]),
                };
                push_unselected_field_summary(
                    &mut rows,
                    InspectorSummaryLabels::OVERLAY,
                    schema,
                    range,
                    format!("{:?}", schema.display.palette()),
                    presentation
                        .layers()
                        .overlay_palette()
                        .map_or(0, <[crate::view::LinearRgba]>::len),
                    category_keys,
                );
            }
        }
    }

    let active_fields = [state.fill_field(), state.overlay_field()];
    let diagnostics = diagnostics
        .iter()
        .filter(|diagnostic| match entity {
            Some(SelectedSurfaceEntity::Cell(cell)) => {
                diagnostic.cell_id.is_none_or(|candidate| candidate == cell)
                    && diagnostic
                        .field_id
                        .as_ref()
                        .is_none_or(|field| active_fields.contains(&Some(field)))
            }
            Some(SelectedSurfaceEntity::Edge(_)) | None => {
                diagnostic.cell_id.is_none()
                    && diagnostic
                        .field_id
                        .as_ref()
                        .is_none_or(|field| active_fields.contains(&Some(field)))
            }
        })
        .map(|diagnostic| SphericalInspectorDiagnostic {
            severity: diagnostic.severity,
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
            cell: diagnostic.cell_id,
        })
        .collect();

    Ok(SphericalInspectorModel {
        entity,
        rows,
        diagnostics,
    })
}

#[derive(Clone, Copy)]
struct InspectorSummaryLabels {
    description: &'static str,
    unit: &'static str,
    range: &'static str,
    palette: &'static str,
    categories: &'static str,
}

impl InspectorSummaryLabels {
    const FILL: Self = Self {
        description: "填色说明",
        unit: "填色单位",
        range: "填色范围",
        palette: "填色图例",
        categories: "填色类别图例",
    };
    const OVERLAY: Self = Self {
        description: "叠加说明",
        unit: "叠加单位",
        range: "叠加范围",
        palette: "叠加图例",
        categories: "叠加类别图例",
    };
}

fn push_unselected_field_summary(
    rows: &mut Vec<SphericalInspectorRow>,
    labels: InspectorSummaryLabels,
    schema: &FieldSchema,
    range: Option<crate::view::ResolvedDisplayRange>,
    palette: String,
    palette_entries: usize,
    prepared_category_keys: &[u32],
) {
    rows.push(SphericalInspectorRow {
        label: labels.description,
        value: format!(
            "{} · {} · {:?}/{:?}",
            localized_field_key(schema.display.label_key()),
            format_field_id(&schema.id),
            schema.domain,
            schema.value_type,
        ),
    });
    rows.push(SphericalInspectorRow {
        label: labels.unit,
        value: match schema.unit.symbol() {
            "" => "无量纲".to_owned(),
            unit => unit.to_owned(),
        },
    });
    rows.push(SphericalInspectorRow {
        label: labels.range,
        value: format_inspector_range(schema, range),
    });
    rows.push(SphericalInspectorRow {
        label: labels.palette,
        value: format!("{palette} · {palette_entries} 色"),
    });
    rows.push(SphericalInspectorRow {
        label: labels.categories,
        value: format_category_legend(schema, prepared_category_keys),
    });
}

fn format_inspector_range(
    schema: &FieldSchema,
    range: Option<crate::view::ResolvedDisplayRange>,
) -> String {
    let Some(range) = range else {
        return match schema.value_type {
            FieldValueType::CategoryU32 => "不适用（离散类别）".to_owned(),
            _ => "未准备".to_owned(),
        };
    };
    let (min, max) = range.bounds();
    let precision = usize::from(schema.display.decimal_places());
    format!("{min:.precision$}…{max:.precision$}")
}

fn format_category_legend(schema: &FieldSchema, prepared_category_keys: &[u32]) -> String {
    if schema.value_type != FieldValueType::CategoryU32 {
        return "不适用".to_owned();
    }
    schema
        .category_labels
        .iter()
        .filter(|(key, _)| prepared_category_keys.binary_search(key).is_ok())
        .map(|(key, label)| format!("{key}={}", localized_field_key(label)))
        .collect::<Vec<_>>()
        .join("；")
}

fn row(label: &'static str, value: f32) -> SphericalInspectorRow {
    SphericalInspectorRow {
        label,
        value: format!("{value:.6}"),
    }
}

fn format_field_id(field: &FieldId) -> String {
    format!("{}.{}@{}", field.namespace(), field.name(), field.version())
}

/// UI state and persistence validation failures.
#[derive(Debug, Error)]
pub enum SphericalUiError {
    /// A projection parameter was not finite or supported.
    #[error(transparent)]
    Projection(#[from] SphericalProjectionError),
    /// A field state input violated its bounded display contract.
    #[error(transparent)]
    FieldLayer(#[from] crate::view::FieldLayerError),
    /// A persisted camera could not be reconstructed through validated camera APIs.
    #[error("invalid persisted spherical camera")]
    InvalidPersistedCamera,
    /// A live camera action contained invalid values.
    #[error("invalid spherical camera input")]
    InvalidCameraInput,
    /// The published document could not expose its validated field catalog.
    #[error(transparent)]
    FieldView(#[from] crate::view::FieldViewError),
    /// The selected entity is incompatible with the active field channel.
    #[error("selected entity is incompatible with the active spherical field channel")]
    UnsupportedInspectorEntity,
    /// The selected stable ID is not present in the authoritative payload/topology.
    #[error("selected spherical entity has no authoritative value")]
    EntityValueMissing,
    /// A publication update failed and must retain the prior complete state.
    #[error(transparent)]
    Presentation(#[from] SphericalPresentationError),
}
