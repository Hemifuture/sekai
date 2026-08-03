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

    let squared_components = vector.map(|component| component * component);
    let squared_length = dot(vector, vector);
    let length = norm(vector);
    let reciprocal_length = length.recip();
    let direct_normalized = scale(vector, reciprocal_length);
    // Retain the legacy multiply-by-reciprocal order exactly whenever none of
    // its nonzero squared, root, reciprocal, or output intermediates is non-normal.
    let direct_intermediates_are_safe = squared_components.into_iter().all(zero_or_normal)
        && squared_length.is_normal()
        && length.is_normal()
        && reciprocal_length.is_normal()
        && direct_normalized.into_iter().all(zero_or_normal);
    let scaled = [
        vector[0] / largest_component,
        vector[1] / largest_component,
        vector[2] / largest_component,
    ];
    let scaled_length = norm(scaled);
    let scaled_normalized = [
        scaled[0] / scaled_length,
        scaled[1] / scaled_length,
        scaled[2] / scaled_length,
    ];
    if direct_intermediates_are_safe {
        let direct_unit_error = (dot(direct_normalized, direct_normalized) - 1.0).abs();
        let scaled_unit_error = (dot(scaled_normalized, scaled_normalized) - 1.0).abs();
        // Prefer the more nearly unit candidate; an exact tie retains the
        // established direct evaluation order used by ordinary grid inputs.
        return Some(if direct_unit_error <= scaled_unit_error {
            direct_normalized
        } else {
            scaled_normalized
        });
    }

    Some(scaled_normalized)
}

fn zero_or_normal(value: f64) -> bool {
    value == 0.0 || value.is_normal()
}

pub(crate) fn project_tangent_raw(vector: [f64; 3], radial_unit: [f64; 3]) -> [f64; 3] {
    subtract(vector, scale(radial_unit, dot(vector, radial_unit)))
}

pub(crate) fn central_angle_raw(a: [f64; 3], b: [f64; 3]) -> f64 {
    let direct_sine = norm(cross(a, b));
    let cosine = dot(a, b);
    // Below sqrt(epsilon), the cosine cannot resolve the squared angular
    // complement; form the small same/opposite-direction delta explicitly.
    if direct_sine > f64::EPSILON.sqrt() {
        return direct_sine.atan2(cosine);
    }

    let stable_delta = if cosine >= 0.0 {
        subtract(b, a)
    } else {
        add(b, a)
    };
    let robust_sine = norm(cross(a, stable_delta));
    robust_sine.atan2(cosine.clamp(-1.0, 1.0))
}

pub(crate) fn spherical_triangle_area_unit_raw(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    let direct_numerator = dot(a, cross(b, c)).abs();
    let ab = dot(a, b);
    let bc = dot(b, c);
    let ca = dot(c, a);
    let direct_denominator = 1.0 + ab + bc + ca;
    let relative_error_limit = f64::EPSILON.sqrt();
    let numerator_scale = determinant_roundoff_scale(a, b, c);
    let denominator_scale = 1.0 + ab.abs() + bc.abs() + ca.abs();
    // Keep the established direct order while both sums retain at least half
    // the available precision. Otherwise use cancellation-resistant identities.
    let direct_path_is_well_conditioned = direct_numerator > relative_error_limit * numerator_scale
        && direct_denominator.abs() > relative_error_limit * denominator_scale;
    if direct_path_is_well_conditioned {
        return 2.0 * direct_numerator.atan2(direct_denominator);
    }

    let robust_numerator = stable_triangle_numerator(a, b, c, ab, bc, ca);
    let robust_denominator = stable_triangle_denominator(a, b, c, ab, bc, ca);
    2.0 * robust_numerator.atan2(robust_denominator)
}

fn determinant_roundoff_scale(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    (a[0] * b[1] * c[2]).abs()
        + (a[0] * b[2] * c[1]).abs()
        + (a[1] * b[2] * c[0]).abs()
        + (a[1] * b[0] * c[2]).abs()
        + (a[2] * b[0] * c[1]).abs()
        + (a[2] * b[1] * c[0]).abs()
}

fn stable_triangle_numerator(
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    ab: f64,
    bc: f64,
    ca: f64,
) -> f64 {
    let (first, second, other, pair_dot) = if ab.abs() >= bc.abs() && ab.abs() >= ca.abs() {
        (a, b, c, ab)
    } else if bc.abs() >= ca.abs() {
        (b, c, a, bc)
    } else {
        (c, a, b, ca)
    };
    let stable_delta = if pair_dot >= 0.0 {
        subtract(second, first)
    } else {
        add(second, first)
    };
    dot(first, cross(stable_delta, other)).abs()
}

fn stable_triangle_denominator(
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    ab: f64,
    bc: f64,
    ca: f64,
) -> f64 {
    let (first, second, other) = if ab <= bc && ab <= ca {
        (a, b, c)
    } else if bc <= ca {
        (b, c, a)
    } else {
        (c, a, b)
    };
    let pair_sum = add(first, second);
    0.5 * dot(pair_sum, pair_sum) + dot(pair_sum, other)
}
