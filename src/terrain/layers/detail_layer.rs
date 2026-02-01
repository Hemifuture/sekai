//! Detail layer - small-scale surface texture
//!
//! Adds fine terrain details, only on land to keep oceans smooth.

use super::r#trait::{LayerOutput, LegacyTerrainLayer, Pos2, TerrainContext, TerrainLayer};
use crate::terrain::noise::{constrained_noise, smootherstep, NoiseConfig, NoiseGenerator};

/// Detail terrain layer for surface texture
pub struct DetailLayer {
    /// Detail amplitude
    pub amplitude: f64,
    /// Minimum threshold to avoid scattered points
    pub threshold: f64,
    /// Noise configuration
    config: NoiseConfig,
    generator: NoiseGenerator,
}

impl Default for DetailLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl DetailLayer {
    pub fn new() -> Self {
        Self {
            amplitude: 0.08,
            threshold: 0.1,
            config: NoiseConfig {
                base_frequency: 0.02,
                octaves: 4,
                persistence: 0.4,
                lacunarity: 2.2,
                seed: 200,
            },
            generator: NoiseGenerator::new(200),
        }
    }

    pub fn with_seed(mut self, seed: u32) -> Self {
        self.config.seed = seed;
        self.generator = NoiseGenerator::new(seed);
        self
    }

    /// Sample detail at a point (only applies to land)
    fn sample_at(&self, x: f64, y: f64, is_land: bool, coast_distance: f64) -> f64 {
        // No detail in ocean
        if !is_land {
            return 0.0;
        }

        let noise = self.generator.fbm(x, y, &self.config);

        // Apply threshold to avoid scattered points
        let filtered = constrained_noise(noise, self.threshold);

        // Fade out near coast
        let coast_factor = smootherstep(0.0, 30.0, coast_distance);

        filtered * self.amplitude * coast_factor
    }
}

impl LegacyTerrainLayer for DetailLayer {
    fn name(&self) -> &'static str {
        "Detail"
    }

    fn apply(&self, ctx: &mut TerrainContext) {
        let contribution = self.sample_at(ctx.x, ctx.y, ctx.is_land, ctx.coast_distance);
        ctx.elevation += contribution;
    }

    fn sample(&self, ctx: &TerrainContext) -> f64 {
        self.sample_at(ctx.x, ctx.y, ctx.is_land, ctx.coast_distance)
    }
}

impl TerrainLayer for DetailLayer {
    fn name(&self) -> &'static str {
        "Detail"
    }

    fn generate(
        &self,
        cells: &[Pos2],
        _neighbors: &[Vec<u32>],
        previous: &LayerOutput,
    ) -> LayerOutput {
        let mut output = previous.clone();

        for (i, cell) in cells.iter().enumerate() {
            let is_land = previous.heights[i] > 0.0;

            // Only add detail to land
            if is_land {
                let noise = self
                    .generator
                    .fbm(cell.x as f64, cell.y as f64, &self.config);
                let filtered = constrained_noise(noise, self.threshold);
                output.heights[i] += (filtered * self.amplitude) as f32;
            }
        }

        output
    }
}
