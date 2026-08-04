use std::array;

use noise::{NoiseFn, Perlin};

const MAX_OCTAVES: usize = 6;
const MIN_SAMPLES_PER_WAVELENGTH: f64 = 2.0;
const OCTAVE_ROTATION_COS: f64 = 0.819_152_044_288_991_8;
const OCTAVE_ROTATION_SIN: f64 = 0.573_576_436_351_046;
const OCTAVE_SEED_STEP: u32 = 0x9E37_79B9;
const OCTAVE_ROTATION_3D: [[f64; 3]; 3] = [[0.36, 0.48, -0.8], [-0.8, 0.6, 0.0], [0.48, 0.64, 0.6]];

/// Compile-time-owned parameters for one bounded fractal noise signal.
#[derive(Debug, Clone, Copy)]
pub(super) struct FractalProfile {
    pub(super) octaves: usize,
    pub(super) frequency: f64,
    pub(super) lacunarity: f64,
    pub(super) persistence: f64,
}

impl FractalProfile {
    fn assert_valid(self) {
        debug_assert!((1..=MAX_OCTAVES).contains(&self.octaves));
        debug_assert!(self.frequency.is_finite() && self.frequency > 0.0);
        debug_assert!(self.lacunarity.is_finite() && self.lacunarity > 1.0);
        debug_assert!(
            self.persistence.is_finite() && self.persistence > 0.0 && self.persistence < 1.0
        );
    }

    /// Drops detail octaves whose physical wavelength is below the sampling
    /// grid's Nyquist limit. The base octave remains so a causal morphology
    /// does not disappear entirely on very coarse meshes.
    pub(super) fn limited_to_resolution(
        self,
        coordinate_scale_m: f64,
        sample_spacing_m: f64,
    ) -> Self {
        self.assert_valid();
        debug_assert!(coordinate_scale_m.is_finite() && coordinate_scale_m > 0.0);
        debug_assert!(sample_spacing_m.is_finite() && sample_spacing_m > 0.0);

        let maximum_frequency =
            coordinate_scale_m / (MIN_SAMPLES_PER_WAVELENGTH * sample_spacing_m);
        let mut frequency = self.frequency;
        let mut octaves = 1;
        for octave in 1..self.octaves {
            frequency *= self.lacunarity;
            if frequency > maximum_frequency {
                break;
            }
            octaves = octave + 1;
        }
        Self { octaves, ..self }
    }
}

/// Deterministic continuous noise used only for Relief morphology.
pub(super) struct ReliefNoise2d {
    octaves: [Perlin; MAX_OCTAVES],
    warp_x: Perlin,
    warp_y: Perlin,
}

impl ReliefNoise2d {
    pub(super) fn new(seed: u32) -> Self {
        let octaves = array::from_fn(|index| {
            Perlin::new(seed.wrapping_add(OCTAVE_SEED_STEP.wrapping_mul(index as u32 + 1)))
        });
        Self {
            octaves,
            warp_x: Perlin::new(seed ^ 0xA511_E9B3),
            warp_y: Perlin::new(seed ^ 0x63D8_3595),
        }
    }

    pub(super) fn fbm(&self, point: [f64; 2], profile: FractalProfile) -> f64 {
        self.fractal_sum(point, profile, |signal| signal)
            .clamp(-1.0, 1.0)
    }

    pub(super) fn ridged(&self, point: [f64; 2], profile: FractalProfile) -> f64 {
        self.fractal_sum(point, profile, |signal| {
            let ridge = 1.0 - signal.abs().clamp(0.0, 1.0);
            ridge * ridge
        })
        .clamp(0.0, 1.0)
    }

    pub(super) fn warp(&self, point: [f64; 2], frequency: f64, strength: f64) -> [f64; 2] {
        debug_assert!(frequency.is_finite() && frequency > 0.0);
        debug_assert!(strength.is_finite() && strength >= 0.0);
        let sample = [point[0] * frequency, point[1] * frequency];
        let dx = self.warp_x.get(sample).clamp(-1.0, 1.0) * strength;
        let dy = self
            .warp_y
            .get([sample[0] + 19.19, sample[1] - 7.73])
            .clamp(-1.0, 1.0)
            * strength;
        [point[0] + dx, point[1] + dy]
    }

    fn fractal_sum(
        &self,
        point: [f64; 2],
        profile: FractalProfile,
        shape: impl Fn(f64) -> f64,
    ) -> f64 {
        profile.assert_valid();
        let mut coordinate = [point[0] * profile.frequency, point[1] * profile.frequency];
        let mut amplitude = 1.0;
        let mut amplitude_sum = 0.0;
        let mut result = 0.0;
        for source in self.octaves.iter().take(profile.octaves) {
            result += shape(source.get(coordinate)) * amplitude;
            amplitude_sum += amplitude;
            amplitude *= profile.persistence;
            let rotated = [
                coordinate[0] * OCTAVE_ROTATION_COS - coordinate[1] * OCTAVE_ROTATION_SIN,
                coordinate[0] * OCTAVE_ROTATION_SIN + coordinate[1] * OCTAVE_ROTATION_COS,
            ];
            coordinate = [
                rotated[0] * profile.lacunarity,
                rotated[1] * profile.lacunarity,
            ];
        }
        result / amplitude_sum
    }
}

/// Deterministic coherent noise sampled directly in three-dimensional space.
///
/// Unit radial vectors can be sampled without choosing longitude, a map seam,
/// or a privileged pole. The type is deliberately separate from the frozen 2D
/// implementation so spherical work cannot perturb planar morphology.
pub(super) struct ReliefNoise3d {
    octaves: [Perlin; MAX_OCTAVES],
}

impl ReliefNoise3d {
    pub(super) fn new(seed: u32) -> Self {
        Self {
            octaves: array::from_fn(|index| {
                Perlin::new(seed.wrapping_add(OCTAVE_SEED_STEP.wrapping_mul(index as u32 + 1)))
            }),
        }
    }

    pub(super) fn fbm(&self, point: [f64; 3], profile: FractalProfile) -> f64 {
        self.fractal_sum(point, profile, |signal| signal)
            .clamp(-1.0, 1.0)
    }

    pub(super) fn ridged(&self, point: [f64; 3], profile: FractalProfile) -> f64 {
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

#[cfg(test)]
mod tests {
    use super::{FractalProfile, ReliefNoise2d, ReliefNoise3d};

    const PROFILE: FractalProfile = FractalProfile {
        octaves: 5,
        frequency: 1.25,
        lacunarity: 2.03,
        persistence: 0.5,
    };

    #[test]
    fn multiscale_noise_is_seeded_bounded_and_nonconstant() {
        let first = ReliefNoise2d::new(41);
        let repeated = ReliefNoise2d::new(41);
        let changed = ReliefNoise2d::new(42);
        let points = [[0.125, 0.25], [0.5, 0.75], [1.25, -0.5], [3.0, 2.0]];
        let actual = points.map(|point| first.fbm(point, PROFILE));

        assert_eq!(actual, points.map(|point| repeated.fbm(point, PROFILE)));
        assert_ne!(actual, points.map(|point| changed.fbm(point, PROFILE)));
        assert!(actual.iter().all(|value| (-1.0..=1.0).contains(value)));
        assert!(actual.windows(2).any(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn ridged_and_domain_warp_keep_their_bounded_contracts() {
        let noise = ReliefNoise2d::new(99);
        for point in [[0.0, 0.0], [0.25, 0.75], [2.0, -1.0]] {
            let ridge = noise.ridged(point, PROFILE);
            let warped = noise.warp(point, 0.8, 0.12);
            assert!((0.0..=1.0).contains(&ridge));
            assert!((warped[0] - point[0]).abs() <= 0.12);
            assert!((warped[1] - point[1]).abs() <= 0.12);
        }
    }

    #[test]
    fn octave_limit_stops_before_frequencies_the_cell_spacing_cannot_resolve() {
        let coarse = PROFILE.limited_to_resolution(40.0, 10.0);
        let medium = PROFILE.limited_to_resolution(100.0, 5.0);

        assert_eq!(coarse.octaves, 1);
        assert_eq!(medium.octaves, 3);
        assert_eq!(PROFILE.octaves, 5);
    }

    #[test]
    fn octave_limit_preserves_the_profile_when_all_scales_are_resolvable() {
        assert_eq!(PROFILE.limited_to_resolution(100.0, 1.0).octaves, 5);
    }

    #[test]
    fn spherical_noise_is_seeded_finite_bounded_and_coordinate_seam_free() {
        let first = ReliefNoise3d::new(71);
        let repeated = ReliefNoise3d::new(71);
        let changed = ReliefNoise3d::new(72);
        let points = [
            [1.0, 0.0, 0.0],
            [-1.0, 1.0e-12, 0.0],
            [-1.0, -1.0e-12, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        let actual = points.map(|point| first.fbm(point, PROFILE));

        assert_eq!(actual, points.map(|point| repeated.fbm(point, PROFILE)));
        assert_ne!(actual, points.map(|point| changed.fbm(point, PROFILE)));
        assert!(actual
            .iter()
            .all(|value| value.is_finite() && (-1.0..=1.0).contains(value)));
        assert!((actual[1] - actual[2]).abs() < 1.0e-9);
        assert!(points
            .map(|point| first.ridged(point, PROFILE))
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
    }
}
