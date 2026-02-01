//! Regional layer - large-scale terrain variation
//!
//! Adds continental features like highlands, basins, and plains.

use super::r#trait::{LayerOutput, LegacyTerrainLayer, Pos2, TerrainContext, TerrainLayer};
use crate::terrain::noise::{smootherstep, NoiseConfig, NoiseGenerator};

/// Regional terrain layer for large-scale features
pub struct RegionalLayer {
    /// Land amplitude (how much terrain varies on land)
    pub land_amplitude: f64,
    /// Ocean amplitude
    pub ocean_amplitude: f64,
    /// Noise configuration
    config: NoiseConfig,
    generator: NoiseGenerator,
}

impl Default for RegionalLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl RegionalLayer {
    pub fn new() -> Self {
        Self {
            land_amplitude: 0.4,
            ocean_amplitude: 0.3,
            config: NoiseConfig {
                base_frequency: 0.002,
                octaves: 3,
                persistence: 0.5,
                lacunarity: 2.0,
                seed: 100,
            },
            generator: NoiseGenerator::new(100),
        }
    }
    
    pub fn with_seed(mut self, seed: u32) -> Self {
        self.config.seed = seed;
        self.generator = NoiseGenerator::new(seed);
        self
    }
    
    /// Sample the regional contribution at a point
    fn sample_at(&self, x: f64, y: f64, is_land: bool, coast_distance: f64) -> f64 {
        let noise = self.generator.fbm(x, y, &self.config);
        
        // Reduce amplitude near coasts
        let coast_factor = smootherstep(0.0, 50.0, coast_distance.abs());
        
        let amplitude = if is_land {
            self.land_amplitude
        } else {
            self.ocean_amplitude
        };
        
        noise * amplitude * coast_factor
    }
}

impl LegacyTerrainLayer for RegionalLayer {
    fn name(&self) -> &'static str {
        "Regional"
    }
    
    fn apply(&self, ctx: &mut TerrainContext) {
        let contribution = self.sample_at(ctx.x, ctx.y, ctx.is_land, ctx.coast_distance);
        ctx.elevation += contribution;
    }
    
    fn sample(&self, ctx: &TerrainContext) -> f64 {
        self.sample_at(ctx.x, ctx.y, ctx.is_land, ctx.coast_distance)
    }
}

impl TerrainLayer for RegionalLayer {
    fn name(&self) -> &'static str {
        "Regional"
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
            let noise = self.generator.fbm(cell.x as f64, cell.y as f64, &self.config);
            
            let amplitude = if is_land {
                self.land_amplitude
            } else {
                self.ocean_amplitude
            };
            
            output.heights[i] += (noise * amplitude) as f32;
        }
        
        output
    }
}
