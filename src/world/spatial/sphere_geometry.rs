use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

// Bounded unit-sphere construction vectors keep the historical direct
// evaluation order; larger or underflowed inputs use scale-safe normalization.
const DIRECT_NORMALIZATION_MAX_LENGTH: f64 = 2.0;
// Subtraction-based products avoid cancellation once direct products are tiny.
const CENTRAL_ANGLE_FALLBACK_SINE: f64 = 1.0e-8;
const TRIANGLE_AREA_FALLBACK_NUMERATOR: f64 = 1.0e-8;

/// A finite, canonical vector on the unit sphere.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UnitVector3([f64; 3]);

impl UnitVector3 {
    /// Normalizes finite nonzero components into a unit vector.
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, SphereGeometryError> {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(SphereGeometryError::NonFiniteComponent);
        }

        normalize([x, y, z])
            .map(Self)
            .ok_or(SphereGeometryError::ZeroLengthVector)
    }

    /// Stores components already checked as finite unit-vector values without
    /// renormalizing their semantic floating-point representation.
    pub(crate) const fn from_verified_unit_components(components: [f64; 3]) -> Self {
        Self(components)
    }

    /// Returns the canonical vector components by value.
    pub const fn components(self) -> [f64; 3] {
        self.0
    }

    /// Returns the scalar product with another unit vector.
    pub fn dot(self, other: Self) -> f64 {
        dot(self.0, other.0)
    }

    /// Returns the Euclidean norm of this vector.
    pub fn norm(self) -> f64 {
        norm(self.0)
    }
}

impl<'de> Deserialize<'de> for UnitVector3 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let [x, y, z] = <[f64; 3]>::deserialize(deserializer)?;
        Self::new(x, y, z).map_err(serde::de::Error::custom)
    }
}

/// Errors returned when a vector cannot represent a point on the unit sphere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SphereGeometryError {
    /// At least one vector component was not finite.
    #[error("unit-vector components must be finite")]
    NonFiniteComponent,
    /// The vector did not have a direction to normalize.
    #[error("a unit vector cannot have zero length")]
    ZeroLengthVector,
}

/// Returns the shortest angular separation between two unit vectors in radians.
pub fn central_angle(a: UnitVector3, b: UnitVector3) -> f64 {
    central_angle_raw(a.components(), b.components())
}

/// Orthogonally projects a vector onto the tangent plane at a radial direction.
pub fn project_tangent(vector: [f64; 3], radial: UnitVector3) -> [f64; 3] {
    project_tangent_raw(vector, radial.components())
}

/// Returns the area of a spherical triangle on the unit sphere in steradians.
pub fn spherical_triangle_area_unit(a: UnitVector3, b: UnitVector3, c: UnitVector3) -> f64 {
    spherical_triangle_area_unit_raw(a.components(), b.components(), c.components())
}

pub(crate) fn oriented_arc_normal(
    first_endpoint: UnitVector3,
    second_endpoint: UnitVector3,
    first_owner: UnitVector3,
    second_owner: UnitVector3,
) -> Option<UnitVector3> {
    let first_endpoint = first_endpoint.components();
    let endpoint_delta = subtract(second_endpoint.components(), first_endpoint);
    let components = cross(first_endpoint, endpoint_delta);
    let normal = UnitVector3::new(components[0], components[1], components[2]).ok()?;
    let owner_delta = subtract(second_owner.components(), first_owner.components());
    let orientation = dot(normal.components(), owner_delta);
    if !orientation.is_finite() || orientation == 0.0 {
        return None;
    }
    if orientation > 0.0 {
        Some(normal)
    } else {
        let [x, y, z] = normal.components();
        UnitVector3::new(-x, -y, -z).ok()
    }
}

pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale(vector: [f64; 3], factor: f64) -> [f64; 3] {
    [vector[0] * factor, vector[1] * factor, vector[2] * factor]
}

pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

pub(crate) fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    if vector.iter().any(|component| !component.is_finite()) {
        return None;
    }
    let largest_component = vector
        .iter()
        .map(|component| component.abs())
        .fold(0.0, f64::max);
    if largest_component == 0.0 {
        return None;
    }

    let length = norm(vector);
    let direct_square_is_normal = (largest_component * largest_component).is_normal();
    if direct_square_is_normal
        && length.is_finite()
        && length > 0.0
        && length <= DIRECT_NORMALIZATION_MAX_LENGTH
    {
        return Some(scale(vector, length.recip()));
    }

    let scaled = [
        vector[0] / largest_component,
        vector[1] / largest_component,
        vector[2] / largest_component,
    ];
    let scaled_length = norm(scaled);
    Some([
        scaled[0] / scaled_length,
        scaled[1] / scaled_length,
        scaled[2] / scaled_length,
    ])
}

pub(crate) fn project_tangent_raw(vector: [f64; 3], radial_unit: [f64; 3]) -> [f64; 3] {
    subtract(vector, scale(radial_unit, dot(vector, radial_unit)))
}

pub(crate) fn central_angle_raw(a: [f64; 3], b: [f64; 3]) -> f64 {
    let direct_sine = norm(cross(a, b));
    let cosine = dot(a, b);
    if direct_sine > CENTRAL_ANGLE_FALLBACK_SINE {
        return direct_sine.atan2(cosine);
    }

    let robust_sine = norm(cross(a, subtract(b, a)));
    robust_sine.atan2(cosine.clamp(-1.0, 1.0))
}

pub(crate) fn spherical_triangle_area_unit_raw(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let direct_numerator = dot(a, cross(b, c)).abs();
    let denominator = 1.0 + dot(a, b) + dot(b, c) + dot(c, a);
    if direct_numerator > TRIANGLE_AREA_FALLBACK_NUMERATOR {
        return 2.0 * direct_numerator.atan2(denominator);
    }

    let robust_numerator = dot(a, cross(subtract(b, a), subtract(c, a))).abs();
    2.0 * robust_numerator.atan2(denominator)
}
