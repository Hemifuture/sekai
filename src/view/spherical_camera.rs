//! Independent renderer-neutral camera state for spherical map and globe views.
//!
//! Globe orientation is a normalized world-to-camera quaternion. At reset the
//! camera looks from `+Z` toward the origin, so the canonical front direction is
//! `+Z`. Screen x points right and screen y points down; conversion to camera
//! space flips y so camera x/y both use the usual right/up convention. Rays and
//! visibility use this same orientation, and rays transform back to world space
//! through its inverse.

use super::{ProjectionPoint, SphericalProjection, SphericalProjectionKind, UnitRay};
use crate::world::spatial::UnitVector3;

const IDENTITY_ORIENTATION: Quaternion = Quaternion {
    x: 0.0,
    y: 0.0,
    z: 0.0,
    w: 1.0,
};

/// The active presentation family shown by the single spherical canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SphericalViewMode {
    /// A two-dimensional projected map.
    #[default]
    Map,
    /// The undeformed three-dimensional unit globe.
    Globe,
}

/// One validated renderer-neutral snapshot of the complete spherical presentation view.
///
/// Whole-world candidates bind this value together with their projected geometry and prepared
/// field LOD so persisted cameras cannot drift from the publication they drive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SphericalPresentationViewState {
    mode: SphericalViewMode,
    projection: SphericalProjection,
    map_camera: MapCamera,
    globe_camera: GlobeCamera,
}

impl Default for SphericalPresentationViewState {
    fn default() -> Self {
        Self {
            mode: SphericalViewMode::Map,
            projection: SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0)
                .expect("the canonical Equal Earth projection is valid"),
            map_camera: MapCamera::default(),
            globe_camera: GlobeCamera::default(),
        }
    }
}

impl SphericalPresentationViewState {
    /// Binds an active mode, projection, both retained map cameras, and the globe camera.
    pub const fn new(
        mode: SphericalViewMode,
        projection: SphericalProjection,
        map_camera: MapCamera,
        globe_camera: GlobeCamera,
    ) -> Self {
        Self {
            mode,
            projection,
            map_camera,
            globe_camera,
        }
    }

    /// Returns the active presenter family.
    pub const fn mode(self) -> SphericalViewMode {
        self.mode
    }

    /// Returns the exact active map projection.
    pub const fn projection(self) -> SphericalProjection {
        self.projection
    }

    /// Returns both retained projection-specific map cameras.
    pub const fn map_camera(self) -> MapCamera {
        self.map_camera
    }

    /// Returns the retained undeformed-globe camera.
    pub const fn globe_camera(self) -> GlobeCamera {
        self.globe_camera
    }

    /// Returns the active camera zoom that selects the discrete vector-glyph LOD.
    pub const fn active_zoom(self) -> f64 {
        match self.mode {
            SphericalViewMode::Map => self.map_camera.zoom(self.projection.kind()),
            SphericalViewMode::Globe => self.globe_camera.orthographic_scale(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MapCameraState {
    pan: [f64; 2],
    zoom: f64,
}

impl Default for MapCameraState {
    fn default() -> Self {
        Self {
            pan: [0.0, 0.0],
            zoom: 1.0,
        }
    }
}

/// Independent pan and zoom state retained for every supported map projection.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MapCamera {
    equal_earth: MapCameraState,
    equirectangular: MapCameraState,
}

impl MapCamera {
    /// Largest retained normalized pan magnitude at zoom 1; the retained
    /// bound scales with zoom so anchored deep zooms stay reachable.
    pub const MAX_ABS_PAN: f64 = 4.0;
    /// Smallest supported map zoom.
    pub const MIN_ZOOM: f64 = 0.125;
    /// Largest supported map zoom: deep enough to fill the screen with the
    /// finest amplified display unit of every quality tier (plan Task 4R),
    /// with headroom for the M2 distance-adaptive levels.
    pub const MAX_ZOOM: f64 = 4096.0;

    /// Returns the retained normalized pan for `projection`.
    pub const fn pan(self, projection: SphericalProjectionKind) -> [f64; 2] {
        self.state(projection).pan
    }

    /// Returns the retained positive zoom for `projection`.
    pub const fn zoom(self, projection: SphericalProjectionKind) -> f64 {
        self.state(projection).zoom
    }

    /// Adds a finite pan delta without modifying the other projection camera.
    pub fn pan_by(&mut self, projection: SphericalProjectionKind, delta: [f64; 2]) -> bool {
        if delta.into_iter().any(|component| !component.is_finite()) {
            return false;
        }
        let state = self.state_mut(projection);
        let bound = Self::MAX_ABS_PAN * state.zoom.max(1.0);
        let next = [state.pan[0] + delta[0], state.pan[1] + delta[1]];
        if next
            .into_iter()
            .any(|component| !component.is_finite() || component.abs() > bound)
        {
            return false;
        }
        state.pan = next;
        true
    }

    /// Multiplies one projection's zoom about an NDC anchor point.
    ///
    /// The pan is adjusted so the world point under the anchor stays exactly
    /// under it: with the presenter mapping `ndc = (w - c)·fit·zoom + 2·pan`,
    /// the invariant gives `pan' = pan·factor + anchor·(1 - factor)/2`.
    pub fn zoom_about(
        &mut self,
        projection: SphericalProjectionKind,
        factor: f64,
        anchor_ndc: [f64; 2],
    ) -> bool {
        if !factor.is_finite()
            || factor <= 0.0
            || anchor_ndc
                .into_iter()
                .any(|component| !component.is_finite())
        {
            return false;
        }
        let state = self.state_mut(projection);
        let next = state.zoom * factor;
        if !next.is_finite() || !(Self::MIN_ZOOM..=Self::MAX_ZOOM).contains(&next) {
            return false;
        }
        let bound = Self::MAX_ABS_PAN * next.max(1.0);
        let pan = [
            (state.pan[0] * factor + anchor_ndc[0] * (1.0 - factor) * 0.5).clamp(-bound, bound),
            (state.pan[1] * factor + anchor_ndc[1] * (1.0 - factor) * 0.5).clamp(-bound, bound),
        ];
        if pan.into_iter().any(|component| !component.is_finite()) {
            return false;
        }
        state.zoom = next;
        state.pan = pan;
        true
    }

    /// Multiplies one projection's zoom by a finite positive factor.
    pub fn zoom_by(&mut self, projection: SphericalProjectionKind, factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let state = self.state_mut(projection);
        let next = state.zoom * factor;
        if !next.is_finite() || !(Self::MIN_ZOOM..=Self::MAX_ZOOM).contains(&next) {
            return false;
        }
        state.zoom = next;
        true
    }

    /// Resets only `projection`, preserving the other map camera.
    pub fn reset(&mut self, projection: SphericalProjectionKind) {
        *self.state_mut(projection) = MapCameraState::default();
    }

    const fn state(self, projection: SphericalProjectionKind) -> MapCameraState {
        match projection {
            SphericalProjectionKind::EqualEarth => self.equal_earth,
            SphericalProjectionKind::Equirectangular => self.equirectangular,
        }
    }

    fn state_mut(&mut self, projection: SphericalProjectionKind) -> &mut MapCameraState {
        match projection {
            SphericalProjectionKind::EqualEarth => &mut self.equal_earth,
            SphericalProjectionKind::Equirectangular => &mut self.equirectangular,
        }
    }
}

/// The single projection-plane ↔ logical-screen mapping of the map view.
///
/// This is the presenter transform (`ndc = (point − center)·fit·zoom +
/// 2·pan`) bound to one canvas size, shared by picking, screen-space
/// measurement, and the detail scheduler so the mapping exists once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapScreenTransform {
    fit: [f64; 2],
    zoom: f64,
    pan: [f64; 2],
    center: [f64; 2],
    canvas_size: [f64; 2],
    bounds_width: f64,
}

impl MapScreenTransform {
    /// Binds the projection outline, one retained camera, and a canvas.
    ///
    /// Returns `None` for a degenerate (non-finite or non-positive)
    /// canvas size.
    pub fn new(
        projection: SphericalProjection,
        camera: MapCamera,
        canvas_size: [f64; 2],
    ) -> Option<Self> {
        if canvas_size
            .into_iter()
            .any(|component| !component.is_finite() || component <= 0.0)
        {
            return None;
        }
        let bounds = projection.bounds();
        let bounds_width = bounds.max_x() - bounds.min_x();
        let bounds_height = bounds.max_y() - bounds.min_y();
        let aspect = canvas_size[0] / canvas_size[1];
        let map_aspect = bounds_width / bounds_height;
        let fit = if aspect >= map_aspect {
            [2.0 / (bounds_height * aspect), 2.0 / bounds_height]
        } else {
            [2.0 / bounds_width, 2.0 * aspect / bounds_width]
        };
        Some(Self {
            fit,
            zoom: camera.zoom(projection.kind()),
            pan: camera.pan(projection.kind()),
            center: [
                (bounds.min_x() + bounds.max_x()) * 0.5,
                (bounds.min_y() + bounds.max_y()) * 0.5,
            ],
            canvas_size,
            bounds_width,
        })
    }

    /// Maps one projection-plane point to logical screen pixels.
    pub fn to_screen(&self, point: ProjectionPoint) -> [f64; 2] {
        let ndc = [
            (point.x() - self.center[0]) * self.fit[0] * self.zoom + self.pan[0] * 2.0,
            (point.y() - self.center[1]) * self.fit[1] * self.zoom + self.pan[1] * 2.0,
        ];
        [
            (ndc[0] + 1.0) * self.canvas_size[0] * 0.5,
            (1.0 - ndc[1]) * self.canvas_size[1] * 0.5,
        ]
    }

    /// Maps one logical screen pixel back onto the projection plane.
    pub fn to_projection(&self, screen: [f64; 2]) -> ProjectionPoint {
        let ndc_x = 2.0 * screen[0] / self.canvas_size[0] - 1.0;
        let ndc_y = 1.0 - 2.0 * screen[1] / self.canvas_size[1];
        ProjectionPoint::new(
            (ndc_x - 2.0 * self.pan[0]) / (self.fit[0] * self.zoom) + self.center[0],
            (ndc_y - 2.0 * self.pan[1]) / (self.fit[1] * self.zoom) + self.center[1],
        )
    }

    /// The bound logical canvas size.
    pub const fn canvas_size(&self) -> [f64; 2] {
        self.canvas_size
    }

    /// The screen-pixel width of one full outline wrap — the seam period
    /// for wrap-aware visibility tests.
    pub fn wrap_width_px(&self) -> f64 {
        self.bounds_width * self.fit[0] * self.zoom * self.canvas_size[0] * 0.5
    }
}

/// Orthographic trackball state for the undeformed unit globe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlobeCamera {
    orientation: Quaternion,
    orthographic_scale: f64,
}

impl Default for GlobeCamera {
    fn default() -> Self {
        Self {
            orientation: IDENTITY_ORIENTATION,
            orthographic_scale: 1.0,
        }
    }
}

impl GlobeCamera {
    /// Smallest supported visible-globe scale.
    pub const MIN_SCALE: f64 = 0.55;
    /// Largest supported visible-globe scale (regional close-ups; the map
    /// presenter carries the deep zoom).
    pub const MAX_SCALE: f64 = 64.0;

    /// Returns the normalized world-to-camera quaternion as `[x, y, z, w]`.
    pub const fn orientation_xyzw(self) -> [f64; 4] {
        self.orientation.components()
    }

    /// Reconstructs a validated persisted orientation and orthographic scale.
    ///
    /// Finite non-zero quaternions are normalized before they enter runtime state.
    pub fn from_orientation_xyzw(orientation: [f64; 4], scale: f64) -> Option<Self> {
        let orientation = Quaternion {
            x: orientation[0],
            y: orientation[1],
            z: orientation[2],
            w: orientation[3],
        }
        .normalized()?;
        let mut camera = Self {
            orientation,
            orthographic_scale: 1.0,
        };
        camera.set_orthographic_scale(scale).then_some(camera)
    }

    /// Returns the finite bounded orthographic display scale.
    pub const fn orthographic_scale(self) -> f64 {
        self.orthographic_scale
    }

    /// Restores the canonical `+Z` front direction and full-globe scale.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Applies a deterministic shortest-arc trackball rotation.
    ///
    /// Coordinates are logical screen pixels within `canvas_size`. Inputs
    /// outside the canvas or with non-finite components are ignored.
    pub fn trackball_drag(
        &mut self,
        start: [f64; 2],
        end: [f64; 2],
        canvas_size: [f64; 2],
    ) -> bool {
        let Some(start) = trackball_vector(start, canvas_size) else {
            return false;
        };
        let Some(end) = trackball_vector(end, canvas_size) else {
            return false;
        };
        let Some(delta) = Quaternion::shortest_rotation(start, end) else {
            return false;
        };
        if delta == IDENTITY_ORIENTATION {
            return false;
        }
        let Some(next) = delta.multiply(self.orientation).normalized() else {
            return false;
        };
        self.orientation = next;
        true
    }

    /// Multiplies scale by a finite positive factor and clamps to the contract.
    pub fn zoom_by(&mut self, factor: f64) -> bool {
        if !factor.is_finite() || factor <= 0.0 {
            return false;
        }
        let requested = if self.orthographic_scale > Self::MAX_SCALE / factor {
            Self::MAX_SCALE
        } else {
            self.orthographic_scale * factor
        };
        self.orthographic_scale = requested.clamp(Self::MIN_SCALE, Self::MAX_SCALE);
        true
    }

    /// Sets a finite positive scale and clamps it to the supported interval.
    pub fn set_orthographic_scale(&mut self, scale: f64) -> bool {
        if !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        self.orthographic_scale = scale.clamp(Self::MIN_SCALE, Self::MAX_SCALE);
        true
    }

    /// Returns whether a unit world direction lies on the camera-facing hemisphere.
    pub fn is_front_facing(self, direction: UnitVector3) -> bool {
        self.orientation.rotate(direction.components())[2] >= 0.0
    }

    /// Rotates one unit direction into camera space and projects it into
    /// logical screen pixels; the returned depth is the camera-space z
    /// (front hemisphere at z ≥ 0). The caller validates the canvas once.
    pub(crate) fn project_point_with_depth(
        self,
        direction: [f64; 3],
        canvas_size: [f64; 2],
    ) -> ([f64; 2], f64) {
        let rotated = self.orientation.rotate(direction);
        let diameter = canvas_size[0].min(canvas_size[1]);
        (
            [
                canvas_size[0] * 0.5 + rotated[0] * self.orthographic_scale * diameter * 0.5,
                canvas_size[1] * 0.5 - rotated[1] * self.orthographic_scale * diameter * 0.5,
            ],
            rotated[2],
        )
    }

    /// Produces an orthographic world-space ray for one screen point.
    ///
    /// Points outside the canvas or the visible unit-globe disc return `None`
    /// before ray/sphere intersection. The inverse world-to-camera orientation
    /// transforms both the camera-space ray origin and direction.
    pub fn screen_to_ray(self, screen: [f64; 2], canvas_size: [f64; 2]) -> Option<UnitRay> {
        let [x, y] = normalized_screen_point(screen, canvas_size)?;
        let camera_x = x / self.orthographic_scale;
        let camera_y = y / self.orthographic_scale;
        if camera_x.mul_add(camera_x, camera_y * camera_y) > 1.0 {
            return None;
        }
        let inverse = self.orientation.conjugate();
        let origin = inverse.rotate([camera_x, camera_y, 2.0]);
        let direction = inverse.rotate([0.0, 0.0, -1.0]);
        UnitRay::new(origin, direction).ok()
    }

    /// Projects and horizon-clips one renderer unit-sphere segment into logical screen pixels.
    ///
    /// This mirrors the fixed globe uniform and overlay horizon clipping used by the GPU, so
    /// discrete picking can measure its circular logical-pixel tolerance in display space.
    pub(crate) fn project_visible_segment_to_screen(
        self,
        start: [f32; 3],
        end: [f32; 3],
        canvas_size: [f64; 2],
    ) -> Option<[[f64; 2]; 2]> {
        if canvas_size
            .into_iter()
            .any(|component| !component.is_finite() || component <= 0.0)
        {
            return None;
        }
        let mut start = self.orientation.rotate(start.map(f64::from));
        let mut end = self.orientation.rotate(end.map(f64::from));
        if start
            .into_iter()
            .chain(end)
            .any(|component| !component.is_finite())
        {
            return None;
        }
        if start[2] < 0.0 && end[2] < 0.0 {
            return None;
        }
        if start[2] < 0.0 {
            std::mem::swap(&mut start, &mut end);
        }
        if end[2] < 0.0 {
            let crossing = start[2] / (start[2] - end[2]);
            end = std::array::from_fn(|axis| start[axis] + (end[axis] - start[axis]) * crossing);
        }
        let diameter = canvas_size[0].min(canvas_size[1]);
        let project = |point: [f64; 3]| {
            [
                canvas_size[0] * 0.5 + point[0] * self.orthographic_scale * diameter * 0.5,
                canvas_size[1] * 0.5 - point[1] * self.orthographic_scale * diameter * 0.5,
            ]
        };
        Some([project(start), project(end)])
    }
}

fn normalized_screen_point(screen: [f64; 2], canvas_size: [f64; 2]) -> Option<[f64; 2]> {
    if screen.into_iter().any(|component| !component.is_finite())
        || canvas_size
            .into_iter()
            .any(|component| !component.is_finite() || component <= 0.0)
        || screen[0] < 0.0
        || screen[0] > canvas_size[0]
        || screen[1] < 0.0
        || screen[1] > canvas_size[1]
    {
        return None;
    }
    let diameter = canvas_size[0].min(canvas_size[1]);
    Some([
        (2.0 * screen[0] - canvas_size[0]) / diameter,
        (canvas_size[1] - 2.0 * screen[1]) / diameter,
    ])
}

fn trackball_vector(screen: [f64; 2], canvas_size: [f64; 2]) -> Option<[f64; 3]> {
    let [mut x, mut y] = normalized_screen_point(screen, canvas_size)?;
    let radius_squared = x.mul_add(x, y * y);
    let z = if radius_squared <= 1.0 {
        (1.0 - radius_squared).sqrt()
    } else {
        let inverse_radius = radius_squared.sqrt().recip();
        x *= inverse_radius;
        y *= inverse_radius;
        0.0
    };
    Some([x, y, z])
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Quaternion {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

impl Quaternion {
    const fn components(self) -> [f64; 4] {
        [self.x, self.y, self.z, self.w]
    }

    fn shortest_rotation(start: [f64; 3], end: [f64; 3]) -> Option<Self> {
        let direction_dot = dot(start, end).clamp(-1.0, 1.0);
        let rotation_axis = cross(start, end);
        let cross_norm_squared = dot(rotation_axis, rotation_axis);
        if cross_norm_squared <= 64.0 * f64::EPSILON * f64::EPSILON {
            if direction_dot >= 0.0 {
                return Some(IDENTITY_ORIENTATION);
            }
            let basis = if start[0].abs() <= start[1].abs() && start[0].abs() <= start[2].abs() {
                [1.0, 0.0, 0.0]
            } else if start[1].abs() <= start[2].abs() {
                [0.0, 1.0, 0.0]
            } else {
                [0.0, 0.0, 1.0]
            };
            let axis = normalize(cross(start, basis))?;
            return Self {
                x: axis[0],
                y: axis[1],
                z: axis[2],
                w: 0.0,
            }
            .normalized();
        }
        Self {
            x: rotation_axis[0],
            y: rotation_axis[1],
            z: rotation_axis[2],
            w: 1.0 + direction_dot,
        }
        .normalized()
    }

    fn multiply(self, right: Self) -> Self {
        Self {
            x: self.w * right.x + self.x * right.w + self.y * right.z - self.z * right.y,
            y: self.w * right.y - self.x * right.z + self.y * right.w + self.z * right.x,
            z: self.w * right.z + self.x * right.y - self.y * right.x + self.z * right.w,
            w: self.w * right.w - self.x * right.x - self.y * right.y - self.z * right.z,
        }
    }

    const fn conjugate(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }

    fn normalized(self) -> Option<Self> {
        let components = self.components();
        if components
            .into_iter()
            .any(|component| !component.is_finite())
        {
            return None;
        }
        let largest = components.into_iter().map(f64::abs).fold(0.0_f64, f64::max);
        if largest == 0.0 {
            return None;
        }
        let scaled = components.map(|component| component / largest);
        let inverse_norm = dot4(scaled, scaled).sqrt().recip();
        let mut normalized = scaled.map(|component| component * inverse_norm);
        if normalized[3] < 0.0
            || (normalized[3] == 0.0
                && normalized[..3]
                    .iter()
                    .copied()
                    .find(|component| *component != 0.0)
                    .is_some_and(|component| component < 0.0))
        {
            normalized = normalized.map(|component| -component);
        }
        Some(Self {
            x: normalized[0],
            y: normalized[1],
            z: normalized[2],
            w: normalized[3],
        })
    }

    fn rotate(self, vector: [f64; 3]) -> [f64; 3] {
        let quaternion_vector = [self.x, self.y, self.z];
        let first_cross = cross(quaternion_vector, vector);
        let second_cross = cross(quaternion_vector, first_cross);
        [
            vector[0] + 2.0 * (self.w * first_cross[0] + second_cross[0]),
            vector[1] + 2.0 * (self.w * first_cross[1] + second_cross[1]),
            vector[2] + 2.0 * (self.w * first_cross[2] + second_cross[2]),
        ]
    }
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn dot4(left: [f64; 4], right: [f64; 4]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2] + left[3] * right[3]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    if vector.into_iter().any(|component| !component.is_finite()) {
        return None;
    }
    let largest = vector.into_iter().map(f64::abs).fold(0.0_f64, f64::max);
    if largest == 0.0 {
        return None;
    }
    let scaled = vector.map(|component| component / largest);
    let inverse_norm = dot(scaled, scaled).sqrt().recip();
    Some(scaled.map(|component| component * inverse_norm))
}
