use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// A finite, canonical vector on the unit sphere.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UnitVector3([f64; 3]);

impl UnitVector3 {
    /// Normalizes finite nonzero components into a unit vector.
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, SphereGeometryError> {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(SphereGeometryError::NonFiniteComponent);
        }

        let largest_component = x.abs().max(y.abs()).max(z.abs());
        if largest_component == 0.0 {
            return Err(SphereGeometryError::ZeroLengthVector);
        }

        let scaled = [
            x / largest_component,
            y / largest_component,
            z / largest_component,
        ];
        let length = scaled[0].hypot(scaled[1]).hypot(scaled[2]);
        Ok(Self([
            scaled[0] / length,
            scaled[1] / length,
            scaled[2] / length,
        ]))
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
        self.0[0].hypot(self.0[1]).hypot(self.0[2])
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
    let a_components = a.components();
    let cross = cross(a_components, subtract(b.components(), a_components));
    let sine = cross[0].hypot(cross[1]).hypot(cross[2]);
    sine.atan2(a.dot(b).clamp(-1.0, 1.0))
}

/// Orthogonally projects a vector onto the tangent plane at a radial direction.
pub fn project_tangent(vector: [f64; 3], radial: UnitVector3) -> [f64; 3] {
    subtract(
        vector,
        scale(radial.components(), dot(vector, radial.components())),
    )
}

/// Returns the area of a spherical triangle on the unit sphere in steradians.
pub fn spherical_triangle_area_unit(a: UnitVector3, b: UnitVector3, c: UnitVector3) -> f64 {
    let a = a.components();
    let b = b.components();
    let c = c.components();
    let numerator = dot(a, cross(subtract(b, a), subtract(c, a))).abs();
    let denominator = 1.0 + dot(a, b) + dot(b, c) + dot(c, a);
    2.0 * numerator.atan2(denominator)
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

#[allow(dead_code)]
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
