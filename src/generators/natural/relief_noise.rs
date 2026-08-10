use std::array;

use noise::{NoiseFn, Perlin};

use super::fractal::{FractalProfile, MAX_FRACTAL_OCTAVES};

const OCTAVE_ROTATION_COS: f64 = 0.819_152_044_288_991_8;
const OCTAVE_ROTATION_SIN: f64 = 0.573_576_436_351_046;
const OCTAVE_SEED_STEP: u32 = 0x9E37_79B9;

/// Deterministic continuous noise used only for Relief morphology.
pub(super) struct ReliefNoise2d {
    octaves: [Perlin; MAX_FRACTAL_OCTAVES],
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

#[cfg(test)]
mod tests {
    use super::{FractalProfile, ReliefNoise2d};

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
}
