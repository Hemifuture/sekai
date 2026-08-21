pub(super) const MAX_FRACTAL_OCTAVES: usize = 6;
const MIN_SAMPLES_PER_WAVELENGTH: f64 = 2.0;

/// Compile-time-owned parameters for one bounded fractal signal.
#[derive(Debug, Clone, Copy)]
pub(super) struct FractalProfile {
    pub(super) octaves: usize,
    pub(super) frequency: f64,
    pub(super) lacunarity: f64,
    pub(super) persistence: f64,
}

impl FractalProfile {
    pub(super) fn assert_valid(self) {
        debug_assert!((1..=MAX_FRACTAL_OCTAVES).contains(&self.octaves));
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
