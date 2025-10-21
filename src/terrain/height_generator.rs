use crate::models::map::grid::Grid;
use crate::terrain::noise::{NoiseConfig, NoiseGenerator};
use rayon::prelude::*;

/// Generates height values for a grid using procedural noise
pub struct HeightGenerator {
    noise_gen: NoiseGenerator,
}

impl HeightGenerator {
    /// Create a new height generator from noise configuration
    pub fn new(config: NoiseConfig) -> Self {
        Self {
            noise_gen: NoiseGenerator::new(config),
        }
    }

    /// Generate height values for all points in the grid
    /// Returns a Vec of height values in range [0, 255], one per grid point
    pub fn generate_for_grid(&self, grid: &Grid) -> Vec<u8> {
        // Use parallel processing for better performance
        grid.points
            .par_iter()
            .map(|point| {
                let noise_value = self.noise_gen.generate(point.x, point.y);
                self.normalize_to_u8(noise_value)
            })
            .collect()
    }

    /// Generate height value for a single point
    pub fn generate_at(&self, x: f32, y: f32) -> u8 {
        let noise_value = self.noise_gen.generate(x, y);
        self.normalize_to_u8(noise_value)
    }

    /// Generate height value as float in range [0.0, 1.0]
    pub fn generate_at_normalized(&self, x: f32, y: f32) -> f32 {
        self.noise_gen.generate(x, y) as f32
    }

    /// Convert noise value [0.0, 1.0] to height byte [0, 255]
    fn normalize_to_u8(&self, value: f64) -> u8 {
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::noise::NoiseConfig;

    fn create_test_grid() -> Grid {
        let mut grid = Grid::new(100, 100, 10);
        grid.generate_points();
        grid
    }

    #[test]
    fn test_height_count_matches_grid_points() {
        let config = NoiseConfig::default();
        let generator = HeightGenerator::new(config);
        let grid = create_test_grid();

        let heights = generator.generate_for_grid(&grid);

        assert_eq!(
            heights.len(),
            grid.points.len(),
            "Height count should match grid point count"
        );
    }

    #[test]
    fn test_height_values_in_valid_range() {
        let config = NoiseConfig::default();
        let generator = HeightGenerator::new(config);
        let grid = create_test_grid();

        let heights = generator.generate_for_grid(&grid);

        for (i, &height) in heights.iter().enumerate() {
            assert!(
                height <= 255,
                "Height {} at index {} exceeds maximum value 255",
                height, i
            );
            // All u8 values are >= 0 by definition
        }
    }

    #[test]
    fn test_heights_are_non_uniform() {
        let config = NoiseConfig::default();
        let generator = HeightGenerator::new(config);
        let grid = create_test_grid();

        let heights = generator.generate_for_grid(&grid);

        // Calculate standard deviation to ensure variation
        let mean: f64 = heights.iter().map(|&h| h as f64).sum::<f64>() / heights.len() as f64;
        let variance: f64 = heights
            .iter()
            .map(|&h| (h as f64 - mean).powi(2))
            .sum::<f64>()
            / heights.len() as f64;
        let std_dev = variance.sqrt();

        // Standard deviation should be significant (at least 10)
        assert!(
            std_dev > 10.0,
            "Heights should vary significantly. Std dev: {}, mean: {}",
            std_dev,
            mean
        );
    }

    #[test]
    fn test_same_config_produces_same_heights() {
        let config = NoiseConfig::new(12345, 0.01, 4, 0.5, 2.0);
        let gen1 = HeightGenerator::new(config.clone());
        let gen2 = HeightGenerator::new(config);

        let grid = create_test_grid();

        let heights1 = gen1.generate_for_grid(&grid);
        let heights2 = gen2.generate_for_grid(&grid);

        assert_eq!(
            heights1, heights2,
            "Same configuration should produce identical heights"
        );
    }

    #[test]
    fn test_different_seeds_produce_different_heights() {
        let config1 = NoiseConfig::new(123, 0.01, 4, 0.5, 2.0);
        let config2 = NoiseConfig::new(456, 0.01, 4, 0.5, 2.0);

        let gen1 = HeightGenerator::new(config1);
        let gen2 = HeightGenerator::new(config2);

        let grid = create_test_grid();

        let heights1 = gen1.generate_for_grid(&grid);
        let heights2 = gen2.generate_for_grid(&grid);

        let differences: usize = heights1
            .iter()
            .zip(heights2.iter())
            .filter(|(h1, h2)| h1 != h2)
            .count();

        // At least 80% should be different
        let threshold = (heights1.len() as f64 * 0.8) as usize;
        assert!(
            differences > threshold,
            "Different seeds should produce different heights. Only {} out of {} are different",
            differences,
            heights1.len()
        );
    }

    #[test]
    fn test_generate_at_single_point() {
        let config = NoiseConfig::default();
        let generator = HeightGenerator::new(config);

        let height = generator.generate_at(100.0, 100.0);
        assert!(height <= 255);

        // Same point should produce same height
        let height2 = generator.generate_at(100.0, 100.0);
        assert_eq!(height, height2);
    }

    #[test]
    fn test_generate_at_normalized() {
        let config = NoiseConfig::default();
        let generator = HeightGenerator::new(config);

        let height = generator.generate_at_normalized(100.0, 100.0);
        assert!(
            height >= 0.0 && height <= 1.0,
            "Normalized height {} should be in [0, 1]",
            height
        );
    }

    #[test]
    fn test_adjacent_points_vary() {
        let config = NoiseConfig::new(42, 0.01, 4, 0.5, 2.0);
        let generator = HeightGenerator::new(config);

        // Test that adjacent points have different heights
        let mut total_diff = 0.0;
        let samples = 50;

        for i in 0..samples {
            let x = i as f32 * 10.0;
            let y = i as f32 * 10.0;

            let h1 = generator.generate_at(x, y);
            let h2 = generator.generate_at(x + 1.0, y);
            total_diff += (h1 as i32 - h2 as i32).abs() as f32;
        }

        let avg_diff = total_diff / samples as f32;

        // Adjacent points should have some variation on average
        assert!(
            avg_diff > 0.1,
            "Adjacent points should vary. Average difference: {}",
            avg_diff
        );
    }

    #[test]
    fn test_empty_grid() {
        let config = NoiseConfig::default();
        let generator = HeightGenerator::new(config);
        let grid = Grid::new(100, 100, 10); // Empty grid (no points generated)

        let heights = generator.generate_for_grid(&grid);

        assert_eq!(heights.len(), 0, "Empty grid should produce no heights");
    }

    #[test]
    fn test_large_grid_performance() {
        let config = NoiseConfig::default();
        let generator = HeightGenerator::new(config);
        let mut grid = Grid::new(2000, 1000, 10);
        grid.generate_points();

        // This should complete without timeout (tests parallel processing)
        let start = std::time::Instant::now();
        let heights = generator.generate_for_grid(&grid);
        let duration = start.elapsed();

        assert_eq!(heights.len(), grid.points.len());
        println!(
            "Generated {} heights in {:?} ({} points/sec)",
            heights.len(),
            duration,
            (heights.len() as f64 / duration.as_secs_f64()) as u64
        );

        // Should be reasonably fast with rayon parallelization
        // This is a soft check - exact timing depends on hardware
        assert!(
            duration.as_secs() < 5,
            "Large grid generation took too long: {:?}",
            duration
        );
    }
}
