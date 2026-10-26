//! Three-vector helpers of the reduced climate on the unit sphere.

/// Unit eastward vector at a unit position; the x axis at the poles.
pub(super) fn east_unit(center: [f64; 3]) -> [f64; 3] {
    let east = [-center[1], center[0], 0.0];
    let length = norm(east);
    if length <= 1.0e-12 {
        [1.0, 0.0, 0.0]
    } else {
        scale(east, 1.0 / length)
    }
}

/// Part of `v` tangent to the sphere at the unit position `center`.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "consumed by the reduced solver from A10 T4")
)]
pub(super) fn tangent(v: [f64; 3], center: [f64; 3]) -> [f64; 3] {
    add(v, scale(center, -dot(v, center)))
}

pub(super) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(super) fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "consumed by the reduced solver from A10 T4")
)]
pub(super) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(super) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(super) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
