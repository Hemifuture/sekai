//! Shared spherical coherent-noise primitives.
//!
//! The fractal core is ordinary 3D OpenSimplex sampled without longitude or a
//! privileged pole. The oriented detail kernel follows sparse convolution
//! Gabor noise (Lagae et al.): deterministic impulses carry a Gaussian
//! envelope and cosine carrier in the local tangent plane. This module owns
//! both implementations so tectonics and relief cannot drift apart.

use std::array;
use std::f64::consts::{PI, TAU};

use noise::{NoiseFn, OpenSimplex};

use crate::generators::natural::fractal::{FractalProfile, MAX_FRACTAL_OCTAVES};
use crate::world::spatial::UnitVector3;

const OCTAVE_SEED_STEP: u32 = 0x9E37_79B9;
const OCTAVE_ROTATION_3D: [[f64; 3]; 3] = [[0.36, 0.48, -0.8], [-0.8, 0.6, 0.0], [0.48, 0.64, 0.6]];
const GABOR_SEED_XOR: u32 = 0xA511_E9B3;
const SPLITMIX_INCREMENT: u64 = 0x9E37_79B9_7F4A_7C15;
const SPLITMIX_MULTIPLIER_1: u64 = 0xBF58_476D_1CE4_E5B9;
const SPLITMIX_MULTIPLIER_2: u64 = 0x94D0_49BB_1331_11EB;
const TANGENT_EPSILON: f64 = 1.0e-12;

/// One bounded sparse-convolution Gabor kernel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::generators::natural) struct GaborKernel {
    /// Gaussian envelope scale in radians.
    pub(in crate::generators::natural) envelope_scale_rad: f64,
    /// Cosine carrier frequency in cycles per radian.
    pub(in crate::generators::natural) carrier_frequency: f64,
    /// Fixed number of deterministic global impulses.
    pub(in crate::generators::natural) impulse_count: u8,
}

/// The single deterministic 3D noise source used by spherical natural generation.
pub(in crate::generators::natural) struct SphericalNoise3d {
    octaves: [OpenSimplex; MAX_FRACTAL_OCTAVES],
    gabor_seed: u32,
}

impl SphericalNoise3d {
    pub(in crate::generators::natural) fn new(seed: u32) -> Self {
        Self {
            octaves: array::from_fn(|index| {
                OpenSimplex::new(seed.wrapping_add(OCTAVE_SEED_STEP.wrapping_mul(index as u32 + 1)))
            }),
            gabor_seed: seed ^ GABOR_SEED_XOR,
        }
    }

    pub(in crate::generators::natural) fn fbm(
        &self,
        direction: UnitVector3,
        profile: FractalProfile,
    ) -> f64 {
        self.fbm_coordinate(direction.components(), profile)
    }

    pub(in crate::generators::natural) fn ridged(
        &self,
        direction: UnitVector3,
        profile: FractalProfile,
    ) -> f64 {
        self.ridged_coordinate(direction.components(), profile)
    }

    pub(in crate::generators::natural) fn sparse_gabor(
        &self,
        direction: UnitVector3,
        tangent: [f64; 3],
        kernel: GaborKernel,
    ) -> f64 {
        debug_assert!(
            kernel.envelope_scale_rad.is_finite()
                && kernel.envelope_scale_rad > 0.0
                && kernel.envelope_scale_rad <= PI
                && kernel.carrier_frequency.is_finite()
                && kernel.carrier_frequency > 0.0
                && kernel.impulse_count > 0
        );
        if !kernel.envelope_scale_rad.is_finite()
            || kernel.envelope_scale_rad <= 0.0
            || kernel.envelope_scale_rad > PI
            || !kernel.carrier_frequency.is_finite()
            || kernel.carrier_frequency <= 0.0
            || kernel.impulse_count == 0
        {
            return 0.0;
        }

        let radial = direction.components();
        let Some(ridge_tangent) = normalize(project_tangent(tangent, radial)) else {
            return 0.0;
        };
        let across_tangent = cross(radial, ridge_tangent);
        let mut sum = 0.0;

        for impulse in 0..kernel.impulse_count {
            let (center, phase) = gabor_impulse(self.gabor_seed, impulse);
            let cosine = dot(radial, center).clamp(-1.0, 1.0);
            let angular_distance = cosine.acos();
            let tangent_delta = project_tangent(center, radial);
            let tangent_norm = norm(tangent_delta);
            let across = if tangent_norm <= TANGENT_EPSILON {
                0.0
            } else {
                angular_distance * dot(scale(tangent_delta, tangent_norm.recip()), across_tangent)
            };
            let normalized_distance = angular_distance / kernel.envelope_scale_rad;
            let envelope = (-PI * normalized_distance * normalized_distance).exp();
            let carrier = (TAU * kernel.carrier_frequency * across + phase).cos();
            sum += envelope * carrier;
        }

        (sum / f64::from(kernel.impulse_count)).clamp(-1.0, 1.0)
    }

    /// Coordinate-space bridge used by existing spherical relief and field recipes.
    /// New tectonic code should prefer the unit-direction API above.
    pub(in crate::generators::natural) fn fbm_coordinate(
        &self,
        point: [f64; 3],
        profile: FractalProfile,
    ) -> f64 {
        self.fractal_sum(point, profile, |signal| signal)
            .clamp(-1.0, 1.0)
    }

    /// Coordinate-space bridge used by existing spherical relief and field recipes.
    pub(in crate::generators::natural) fn ridged_coordinate(
        &self,
        point: [f64; 3],
        profile: FractalProfile,
    ) -> f64 {
        self.fractal_sum(point, profile, |signal| {
            let ridge = 1.0 - signal.abs().clamp(0.0, 1.0);
            ridge * ridge
        })
        .clamp(0.0, 1.0)
    }

    fn fractal_sum(
        &self,
        point: [f64; 3],
        profile: FractalProfile,
        shape: impl Fn(f64) -> f64,
    ) -> f64 {
        profile.assert_valid();
        let mut coordinate = point.map(|component| component * profile.frequency);
        let mut amplitude = 1.0;
        let mut amplitude_sum = 0.0;
        let mut result = 0.0;
        for source in self.octaves.iter().take(profile.octaves) {
            result += shape(source.get(coordinate)) * amplitude;
            amplitude_sum += amplitude;
            amplitude *= profile.persistence;
            let rotated = OCTAVE_ROTATION_3D.map(|row| {
                row[0] * coordinate[0] + row[1] * coordinate[1] + row[2] * coordinate[2]
            });
            coordinate = rotated.map(|component| component * profile.lacunarity);
        }
        result / amplitude_sum
    }
}

fn gabor_impulse(seed: u32, impulse: u8) -> ([f64; 3], f64) {
    let state = u64::from(seed) ^ (u64::from(impulse) + 1).wrapping_mul(SPLITMIX_INCREMENT);
    let vertical = unit_interval(splitmix64(state)).mul_add(2.0, -1.0);
    let azimuth = TAU * unit_interval(splitmix64(state.wrapping_add(SPLITMIX_INCREMENT)));
    let phase = TAU
        * unit_interval(splitmix64(
            state.wrapping_add(SPLITMIX_INCREMENT.wrapping_mul(2)),
        ));
    let horizontal = (1.0 - vertical * vertical).max(0.0).sqrt();
    (
        [
            horizontal * azimuth.cos(),
            horizontal * azimuth.sin(),
            vertical,
        ],
        phase,
    )
}

fn splitmix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(SPLITMIX_MULTIPLIER_1);
    value = (value ^ (value >> 27)).wrapping_mul(SPLITMIX_MULTIPLIER_2);
    value ^ (value >> 31)
}

fn unit_interval(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}

fn project_tangent(vector: [f64; 3], radial: [f64; 3]) -> [f64; 3] {
    let radial_component = dot(vector, radial);
    [
        vector[0] - radial_component * radial[0],
        vector[1] - radial_component * radial[1],
        vector[2] - radial_component * radial[2],
    ]
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = norm(vector);
    (length.is_finite() && length > TANGENT_EPSILON).then(|| scale(vector, length.recip()))
}

fn scale(vector: [f64; 3], scalar: f64) -> [f64; 3] {
    vector.map(|component| component * scalar)
}
