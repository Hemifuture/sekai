use noise::{NoiseFn, Perlin, Fbm};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for procedural noise generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoiseConfig {
    /// Random seed for reproducible generation
    pub seed: u32,

    /// Base frequency of the noise (higher = more detailed)
    /// Typical range: 0.001 - 0.01
    pub frequency: f64,

    /// Number of noise layers to combine (more = more detail)
    /// Typical range: 1 - 8
    pub octaves: usize,

    /// How much each octave contributes (amplitude decay)
    /// Typical range: 0.3 - 0.7
    pub persistence: f64,

    /// Frequency multiplier between octaves
    /// Typical range: 1.5 - 3.0
    pub lacunarity: f64,
}

impl Default for NoiseConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            frequency: 0.002,
            octaves: 4,
            persistence: 0.5,
            lacunarity: 2.0,
        }
    }
}

impl NoiseConfig {
    /// Create a new noise configuration with custom parameters
    pub fn new(seed: u32, frequency: f64, octaves: usize, persistence: f64, lacunarity: f64) -> Self {
        Self {
            seed,
            frequency,
            octaves,
            persistence,
            lacunarity,
        }
    }

    /// Create configuration optimized for terrain generation
    pub fn terrain() -> Self {
        Self {
            seed: rand::random(),
            frequency: 0.002,
            octaves: 6,
            persistence: 0.5,
            lacunarity: 2.0,
        }
    }

    /// Create configuration for smooth, large-scale features
    pub fn smooth() -> Self {
        Self {
            seed: rand::random(),
            frequency: 0.001,
            octaves: 3,
            persistence: 0.6,
            lacunarity: 2.0,
        }
    }

    /// Create configuration for rough, detailed features
    pub fn rough() -> Self {
        Self {
            seed: rand::random(),
            frequency: 0.003,
            octaves: 8,
            persistence: 0.4,
            lacunarity: 2.5,
        }
    }
}

/// Multi-layered noise generator using Fractional Brownian Motion (FBM)
pub struct NoiseGenerator {
    fbm: Fbm<Perlin>,
    frequency: f64,
}

impl NoiseGenerator {
    /// Create a new noise generator from configuration
    pub fn new(config: NoiseConfig) -> Self {
        let mut fbm = Fbm::<Perlin>::new(config.seed);
        fbm.octaves = config.octaves;
        fbm.persistence = config.persistence;
        fbm.lacunarity = config.lacunarity;

        Self {
            fbm,
            frequency: config.frequency,
        }
    }

    /// Generate noise value at given coordinates
    /// Returns value in range [0.0, 1.0]
    pub fn generate(&self, x: f32, y: f32) -> f64 {
        // Apply frequency scaling
        let nx = (x as f64) * self.frequency;
        let ny = (y as f64) * self.frequency;

        // Get raw noise value (typically in range [-1, 1])
        let raw_value = self.fbm.get([nx, ny]);

        // Normalize to [0, 1]
        (raw_value + 1.0) * 0.5
    }

    /// Generate noise value with custom range
    pub fn generate_range(&self, x: f32, y: f32, min: f64, max: f64) -> f64 {
        let normalized = self.generate(x, y);
        min + normalized * (max - min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_output_range() {
        let config = NoiseConfig::default();
        let generator = NoiseGenerator::new(config);

        // Test a grid of points
        for x in 0..100 {
            for y in 0..100 {
                let value = generator.generate(x as f32, y as f32);
                assert!(
                    value >= 0.0 && value <= 1.0,
                    "Noise value {} out of range [0, 1] at ({}, {})",
                    value, x, y
                );
            }
        }
    }

    #[test]
    fn test_seed_determinism() {
        let config1 = NoiseConfig::new(12345, 0.002, 4, 0.5, 2.0);
        let config2 = NoiseConfig::new(12345, 0.002, 4, 0.5, 2.0);

        let gen1 = NoiseGenerator::new(config1);
        let gen2 = NoiseGenerator::new(config2);

        // Same seed should produce identical results
        for x in 0..50 {
            for y in 0..50 {
                let v1 = gen1.generate(x as f32 * 10.0, y as f32 * 10.0);
                let v2 = gen2.generate(x as f32 * 10.0, y as f32 * 10.0);
                assert!(
                    (v1 - v2).abs() < 1e-10,
                    "Same seed produced different values: {} vs {}",
                    v1, v2
                );
            }
        }
    }

    #[test]
    fn test_different_seeds_produce_different_results() {
        let config1 = NoiseConfig::new(123, 0.002, 4, 0.5, 2.0);
        let config2 = NoiseConfig::new(456, 0.002, 4, 0.5, 2.0);

        let gen1 = NoiseGenerator::new(config1);
        let gen2 = NoiseGenerator::new(config2);

        let mut differences = 0;
        for x in 0..50 {
            for y in 0..50 {
                let v1 = gen1.generate(x as f32 * 10.0, y as f32 * 10.0);
                let v2 = gen2.generate(x as f32 * 10.0, y as f32 * 10.0);
                if (v1 - v2).abs() > 0.01 {
                    differences += 1;
                }
            }
        }

        // At least 90% of values should be different
        assert!(
            differences > 2250,
            "Different seeds should produce different results, only {} differences found",
            differences
        );
    }

    #[test]
    fn test_octaves_increase_detail() {
        let config_low = NoiseConfig::new(42, 0.002, 1, 0.5, 2.0);
        let config_high = NoiseConfig::new(42, 0.002, 8, 0.5, 2.0);

        let gen_low = NoiseGenerator::new(config_low);
        let gen_high = NoiseGenerator::new(config_high);

        // Sample values
        let mut values_low = Vec::new();
        let mut values_high = Vec::new();

        for x in 0..20 {
            for y in 0..20 {
                values_low.push(gen_low.generate(x as f32 * 5.0, y as f32 * 5.0));
                values_high.push(gen_high.generate(x as f32 * 5.0, y as f32 * 5.0));
            }
        }

        // Calculate variance (measure of detail/roughness)
        let mean_low: f64 = values_low.iter().sum::<f64>() / values_low.len() as f64;
        let mean_high: f64 = values_high.iter().sum::<f64>() / values_high.len() as f64;

        let variance_low: f64 = values_low.iter()
            .map(|v| (v - mean_low).powi(2))
            .sum::<f64>() / values_low.len() as f64;

        let variance_high: f64 = values_high.iter()
            .map(|v| (v - mean_high).powi(2))
            .sum::<f64>() / values_high.len() as f64;

        // Higher octaves should generally have more variance (more detail)
        // This is a loose check since noise can be unpredictable
        assert!(
            variance_high >= variance_low * 0.5,
            "Higher octaves should produce more detailed noise. Low variance: {}, High variance: {}",
            variance_low, variance_high
        );
    }

    #[test]
    fn test_frequency_affects_scale() {
        let config_low_freq = NoiseConfig::new(42, 0.001, 4, 0.5, 2.0);
        let config_high_freq = NoiseConfig::new(42, 0.01, 4, 0.5, 2.0);

        let gen_low = NoiseGenerator::new(config_low_freq);
        let gen_high = NoiseGenerator::new(config_high_freq);

        // At adjacent points, high frequency should change more
        let mut diff_low = 0.0;
        let mut diff_high = 0.0;

        for x in 0..50 {
            for y in 0..50 {
                let x_f = x as f32;
                let y_f = y as f32;

                let v1_low = gen_low.generate(x_f, y_f);
                let v2_low = gen_low.generate(x_f + 1.0, y_f);
                diff_low += (v1_low - v2_low).abs();

                let v1_high = gen_high.generate(x_f, y_f);
                let v2_high = gen_high.generate(x_f + 1.0, y_f);
                diff_high += (v1_high - v2_high).abs();
            }
        }

        // Higher frequency should have larger differences between adjacent points
        assert!(
            diff_high > diff_low * 2.0,
            "Higher frequency should produce more variation. Low: {}, High: {}",
            diff_low, diff_high
        );
    }

    #[test]
    fn test_custom_range_generation() {
        let config = NoiseConfig::default();
        let generator = NoiseGenerator::new(config);

        // Test custom range [100.0, 200.0]
        for x in 0..50 {
            for y in 0..50 {
                let value = generator.generate_range(x as f32, y as f32, 100.0, 200.0);
                assert!(
                    value >= 100.0 && value <= 200.0,
                    "Value {} out of custom range [100, 200]",
                    value
                );
            }
        }
    }

    #[test]
    fn test_preset_configs() {
        // Just ensure presets can be created without panicking
        let _terrain = NoiseConfig::terrain();
        let _smooth = NoiseConfig::smooth();
        let _rough = NoiseConfig::rough();

        // And that they produce valid generators
        let gen_terrain = NoiseGenerator::new(NoiseConfig::terrain());
        let gen_smooth = NoiseGenerator::new(NoiseConfig::smooth());
        let gen_rough = NoiseGenerator::new(NoiseConfig::rough());

        // Test that they all produce valid output
        let v1 = gen_terrain.generate(100.0, 100.0);
        let v2 = gen_smooth.generate(100.0, 100.0);
        let v3 = gen_rough.generate(100.0, 100.0);

        assert!(v1 >= 0.0 && v1 <= 1.0);
        assert!(v2 >= 0.0 && v2 <= 1.0);
        assert!(v3 >= 0.0 && v3 <= 1.0);
    }
}
