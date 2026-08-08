//! Renderer-neutral spherical map-projection mathematics.

use std::f64::consts::{FRAC_PI_2, PI};

use thiserror::Error;

use crate::world::spatial::{canonical_east_north_basis, UnitVector3};

const A1: f64 = 1.340_264;
const A2: f64 = -0.081_106;
const A3: f64 = 0.000_893;
const A4: f64 = 0.003_796;
const MAX_NEWTON_ITERATIONS: usize = 12;
const NEWTON_TOLERANCE: f64 = 1.0e-13;
const JACOBIAN_STEP: f64 = 1.0e-7;
const MIN_MAPPED_LENGTH: f64 = 1.0e-12;
const OUTLINE_EDGE_ULPS: usize = 4;

/// The available spherical map projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SphericalProjectionKind {
    /// The Equal Earth equal-area pseudocylindrical projection.
    EqualEarth,
    /// A plate carree projection normalized to the square `[-1, 1] x [-1, 1]`.
    Equirectangular,
}

/// A coordinate in projection-local planar space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectionPoint {
    x: f64,
    y: f64,
}

impl ProjectionPoint {
    /// Creates a projection point. Finite validation occurs at projection boundaries.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Returns the horizontal coordinate.
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns the vertical coordinate.
    pub const fn y(self) -> f64 {
        self.y
    }
}

/// The natural planar extent of a spherical projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectionBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl ProjectionBounds {
    /// Returns the minimum horizontal coordinate.
    pub const fn min_x(self) -> f64 {
        self.min_x
    }

    /// Returns the maximum horizontal coordinate.
    pub const fn max_x(self) -> f64 {
        self.max_x
    }

    /// Returns the minimum vertical coordinate.
    pub const fn min_y(self) -> f64 {
        self.min_y
    }

    /// Returns the maximum vertical coordinate.
    pub const fn max_y(self) -> f64 {
        self.max_y
    }
}

/// A normalized direction in projection-local planar space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedDirection {
    x: f64,
    y: f64,
}

impl ProjectedDirection {
    /// Returns the horizontal direction component.
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns the vertical direction component.
    pub const fn y(self) -> f64 {
        self.y
    }
}

/// Errors returned by spherical-projection operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SphericalProjectionError {
    /// A caller supplied a non-finite coordinate or central meridian.
    #[error("projection inputs must be finite")]
    NonFiniteInput,
    /// A point does not lie within the projection's valid planar outline.
    #[error("projection point lies outside the projection outline")]
    OutsideProjectionOutline,
    /// The bounded Equal Earth inverse solver did not converge.
    #[error("Equal Earth inverse Newton solver did not converge")]
    NewtonDidNotConverge,
    /// The local planar Jacobian does not define a usable direction.
    #[error("projection Jacobian is degenerate at this direction")]
    ProjectionJacobianDegenerate,
}

/// A configured spherical map projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SphericalProjection {
    kind: SphericalProjectionKind,
    central_meridian: f64,
}

impl SphericalProjection {
    /// Creates a projection with a central meridian in radians.
    pub fn new(
        kind: SphericalProjectionKind,
        central_meridian: f64,
    ) -> Result<Self, SphericalProjectionError> {
        if !central_meridian.is_finite() {
            return Err(SphericalProjectionError::NonFiniteInput);
        }
        Ok(Self {
            kind,
            central_meridian: wrap_radians(central_meridian),
        })
    }

    /// Returns this projection's kind.
    pub const fn kind(self) -> SphericalProjectionKind {
        self.kind
    }

    /// Returns the normalized central meridian in `[-pi, pi)` radians.
    pub const fn central_meridian(self) -> f64 {
        self.central_meridian
    }

    /// Maps a unit direction to this projection's planar coordinates.
    pub fn forward(
        self,
        direction: UnitVector3,
    ) -> Result<ProjectionPoint, SphericalProjectionError> {
        let [x, y, z] = direction.components();
        let longitude = y.atan2(x);
        let latitude = z.asin();
        if !longitude.is_finite() || !latitude.is_finite() {
            return Err(SphericalProjectionError::NonFiniteInput);
        }
        let relative_longitude = wrap_radians(longitude - self.central_meridian);
        let point = match self.kind {
            SphericalProjectionKind::EqualEarth => {
                let theta = (m() * latitude.sin()).asin();
                let theta2 = theta * theta;
                let theta6 = theta2 * theta2 * theta2;
                let denominator =
                    m() * (A1 + 3.0 * A2 * theta2 + theta6 * (7.0 * A3 + 9.0 * A4 * theta2));
                ProjectionPoint::new(
                    relative_longitude * theta.cos() / denominator,
                    equal_earth_y(theta),
                )
            }
            SphericalProjectionKind::Equirectangular => {
                ProjectionPoint::new(relative_longitude / PI, latitude / FRAC_PI_2)
            }
        };
        if point.x.is_finite() && point.y.is_finite() {
            Ok(point)
        } else {
            Err(SphericalProjectionError::NonFiniteInput)
        }
    }

    /// Inverts a point in this projection's valid outline to a unit direction.
    pub fn inverse(self, point: ProjectionPoint) -> Result<UnitVector3, SphericalProjectionError> {
        self.inverse_with_newton_iterations(point, MAX_NEWTON_ITERATIONS)
    }

    /// Returns the natural planar bounds of this projection.
    pub fn bounds(self) -> ProjectionBounds {
        match self.kind {
            SphericalProjectionKind::EqualEarth => ProjectionBounds {
                min_x: -equal_earth_max_x(),
                max_x: equal_earth_max_x(),
                min_y: -equal_earth_max_y(),
                max_y: equal_earth_max_y(),
            },
            SphericalProjectionKind::Equirectangular => ProjectionBounds {
                min_x: -1.0,
                max_x: 1.0,
                min_y: -1.0,
                max_y: 1.0,
            },
        }
    }

    /// Returns whether a planar point is finite and lies within the projection outline.
    pub fn outline_contains(self, point: ProjectionPoint) -> bool {
        if !point.x.is_finite() || !point.y.is_finite() {
            return false;
        }
        match self.kind {
            SphericalProjectionKind::Equirectangular => {
                (-1.0..=1.0).contains(&point.x) && (-1.0..=1.0).contains(&point.y)
            }
            SphericalProjectionKind::EqualEarth => {
                if point.y.abs() > equal_earth_max_y() || point.x.abs() > equal_earth_max_x() {
                    return false;
                }
                let theta = match equal_earth_theta_for_y(point.y, MAX_NEWTON_ITERATIONS) {
                    Ok(theta) => theta,
                    Err(_) => return false,
                };
                point.x.abs() <= equal_earth_outline_edge(equal_earth_half_width(theta))
            }
        }
    }

    /// Maps an east/north tangent vector to a normalized planar direction.
    ///
    /// A zero input vector has no direction and returns `Ok(None)`.
    pub fn map_local_vector(
        self,
        radial: UnitVector3,
        east_north: [f64; 2],
    ) -> Result<Option<ProjectedDirection>, SphericalProjectionError> {
        if !east_north.into_iter().all(f64::is_finite) {
            return Err(SphericalProjectionError::NonFiniteInput);
        }
        if east_north == [0.0, 0.0] {
            return Ok(None);
        }
        let [x, y, _] = radial.components();
        if x.hypot(y) <= f64::EPSILON {
            return Err(SphericalProjectionError::ProjectionJacobianDegenerate);
        }

        let (east, north) = canonical_east_north_basis(radial);
        let east_difference = self.centered_projection_difference(radial, east)?;
        let north_difference = self.centered_projection_difference(radial, north)?;
        let mapped_x = east_north[0] * east_difference.0 + east_north[1] * north_difference.0;
        let mapped_y = east_north[0] * east_difference.1 + east_north[1] * north_difference.1;
        let length = mapped_x.hypot(mapped_y);
        if !length.is_finite() || length < MIN_MAPPED_LENGTH {
            return Err(SphericalProjectionError::ProjectionJacobianDegenerate);
        }
        Ok(Some(ProjectedDirection {
            x: mapped_x / length,
            y: mapped_y / length,
        }))
    }

    fn inverse_with_newton_iterations(
        self,
        point: ProjectionPoint,
        max_iterations: usize,
    ) -> Result<UnitVector3, SphericalProjectionError> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(SphericalProjectionError::NonFiniteInput);
        }
        let bounds = self.bounds();
        if point.x < bounds.min_x
            || point.x > bounds.max_x
            || point.y < bounds.min_y
            || point.y > bounds.max_y
        {
            return Err(SphericalProjectionError::OutsideProjectionOutline);
        }

        let (relative_longitude, latitude) = match self.kind {
            SphericalProjectionKind::Equirectangular => (point.x * PI, point.y * FRAC_PI_2),
            SphericalProjectionKind::EqualEarth => {
                let theta = equal_earth_theta_for_y(point.y, max_iterations)?;
                let half_width = equal_earth_half_width(theta);
                if point.x.abs() > equal_earth_outline_edge(half_width) {
                    return Err(SphericalProjectionError::OutsideProjectionOutline);
                }
                let recovered_longitude = point.x / half_width * PI;
                if !recovered_longitude.is_finite()
                    || recovered_longitude.abs() > equal_earth_outline_edge(PI)
                {
                    return Err(SphericalProjectionError::OutsideProjectionOutline);
                }
                // A forward edge point can be a few ulps outside the width
                // recovered from its rounded y. It is the same analytical
                // antimeridian, not an out-of-range longitude to wrap.
                let relative_longitude = recovered_longitude.clamp(-PI, PI);
                let latitude_argument = theta.sin() / m();
                if !latitude_argument.is_finite() || latitude_argument.abs() > 1.0 {
                    return Err(SphericalProjectionError::OutsideProjectionOutline);
                }
                (relative_longitude, latitude_argument.asin())
            }
        };
        let longitude = wrap_radians(self.central_meridian + relative_longitude);
        let direction = UnitVector3::new(
            latitude.cos() * longitude.cos(),
            latitude.cos() * longitude.sin(),
            latitude.sin(),
        )
        .map_err(|_| SphericalProjectionError::NonFiniteInput)?;
        Ok(direction)
    }

    fn centered_projection_difference(
        self,
        radial: UnitVector3,
        tangent: [f64; 3],
    ) -> Result<(f64, f64), SphericalProjectionError> {
        let [rx, ry, rz] = radial.components();
        let positive = UnitVector3::new(
            rx * JACOBIAN_STEP.cos() + tangent[0] * JACOBIAN_STEP.sin(),
            ry * JACOBIAN_STEP.cos() + tangent[1] * JACOBIAN_STEP.sin(),
            rz * JACOBIAN_STEP.cos() + tangent[2] * JACOBIAN_STEP.sin(),
        )
        .map_err(|_| SphericalProjectionError::ProjectionJacobianDegenerate)?;
        let negative = UnitVector3::new(
            rx * JACOBIAN_STEP.cos() - tangent[0] * JACOBIAN_STEP.sin(),
            ry * JACOBIAN_STEP.cos() - tangent[1] * JACOBIAN_STEP.sin(),
            rz * JACOBIAN_STEP.cos() - tangent[2] * JACOBIAN_STEP.sin(),
        )
        .map_err(|_| SphericalProjectionError::ProjectionJacobianDegenerate)?;
        let positive = self.forward(positive)?;
        let negative = self.forward(negative)?;
        let period = self.local_x_period(radial);
        let delta_x = unwrap_delta(positive.x - negative.x, period);
        Ok((
            delta_x / (2.0 * JACOBIAN_STEP),
            (positive.y - negative.y) / (2.0 * JACOBIAN_STEP),
        ))
    }

    fn local_x_period(self, radial: UnitVector3) -> f64 {
        match self.kind {
            SphericalProjectionKind::Equirectangular => 2.0,
            SphericalProjectionKind::EqualEarth => {
                let latitude = radial.components()[2].asin();
                let theta = (m() * latitude.sin()).asin();
                2.0 * equal_earth_half_width(theta)
            }
        }
    }
}

fn equal_earth_y(theta: f64) -> f64 {
    let theta2 = theta * theta;
    let theta6 = theta2 * theta2 * theta2;
    theta * (A1 + A2 * theta2 + theta6 * (A3 + A4 * theta2))
}

fn equal_earth_derivative(theta: f64) -> f64 {
    let theta2 = theta * theta;
    let theta6 = theta2 * theta2 * theta2;
    A1 + 3.0 * A2 * theta2 + theta6 * (7.0 * A3 + 9.0 * A4 * theta2)
}

fn equal_earth_half_width(theta: f64) -> f64 {
    let denominator = m() * equal_earth_derivative(theta);
    PI * theta.cos() / denominator
}

fn equal_earth_outline_edge(half_width: f64) -> f64 {
    let mut edge = half_width;
    for _ in 0..OUTLINE_EDGE_ULPS {
        // This helper only receives finite positive half-widths, so incrementing
        // the IEEE-754 representation is the next larger finite f64 value.
        edge = f64::from_bits(edge.to_bits() + 1);
    }
    edge
}

fn m() -> f64 {
    3.0_f64.sqrt() / 2.0
}

fn equal_earth_max_x() -> f64 {
    PI / (m() * A1)
}

fn equal_earth_max_y() -> f64 {
    equal_earth_y(m().asin())
}

fn equal_earth_theta_for_y(y: f64, max_iterations: usize) -> Result<f64, SphericalProjectionError> {
    if !y.is_finite() {
        return Err(SphericalProjectionError::NonFiniteInput);
    }
    let maximum_y = equal_earth_max_y();
    if y.abs() > maximum_y {
        return Err(SphericalProjectionError::OutsideProjectionOutline);
    }
    if y == maximum_y {
        return Ok(m().asin());
    }
    if y == -maximum_y {
        return Ok(-m().asin());
    }
    let mut theta = y / A1;
    for _ in 0..max_iterations {
        let delta = (equal_earth_y(theta) - y) / equal_earth_derivative(theta);
        theta -= delta;
        if !theta.is_finite() || theta.abs() > m().asin() + 1.0e-12 {
            return Err(SphericalProjectionError::NewtonDidNotConverge);
        }
        if delta.abs() <= NEWTON_TOLERANCE {
            return Ok(theta);
        }
    }
    Err(SphericalProjectionError::NewtonDidNotConverge)
}

fn wrap_radians(angle: f64) -> f64 {
    (angle + PI).rem_euclid(2.0 * PI) - PI
}

fn unwrap_delta(delta: f64, period: f64) -> f64 {
    if delta > period / 2.0 {
        delta - period
    } else if delta < -period / 2.0 {
        delta + period
    } else {
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProjectionPoint, SphericalProjection, SphericalProjectionError, SphericalProjectionKind,
    };

    #[test]
    fn bounded_inverse_reports_non_convergence_without_a_public_solver_knob() {
        let projection =
            SphericalProjection::new(SphericalProjectionKind::EqualEarth, 0.0).unwrap();
        assert_eq!(
            projection.inverse_with_newton_iterations(ProjectionPoint::new(0.1, 0.1), 0),
            Err(SphericalProjectionError::NewtonDidNotConverge)
        );
    }
}
