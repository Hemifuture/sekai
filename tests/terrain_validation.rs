// 手动验证地形生成系统的核心功能
use eframe_template::terrain::{NoiseConfig, NoiseGenerator, HeightGenerator, HeightColorMap};
use eframe_template::models::map::grid::Grid;

#[test]
fn test_noise_generator_basic() {
    println!("\n=== Testing NoiseGenerator ===");

    let config = NoiseConfig::new(12345, 0.01, 4, 0.5, 2.0);
    let generator = NoiseGenerator::new(config);

    // Test at origin
    let v1 = generator.generate(0.0, 0.0);
    println!("Noise at (0, 0): {}", v1);
    assert!(v1 >= 0.0 && v1 <= 1.0, "Value {} out of range [0, 1]", v1);

    // Test at different positions
    let v2 = generator.generate(100.0, 100.0);
    let v3 = generator.generate(200.0, 200.0);
    println!("Noise at (100, 100): {}", v2);
    println!("Noise at (200, 200): {}", v3);

    assert!(v2 >= 0.0 && v2 <= 1.0);
    assert!(v3 >= 0.0 && v3 <= 1.0);

    // Verify determinism
    let v1_repeat = generator.generate(0.0, 0.0);
    assert_eq!(v1, v1_repeat, "Noise generation not deterministic!");

    println!("✓ NoiseGenerator basic test passed");
}

#[test]
fn test_noise_generator_statistics() {
    println!("\n=== Testing NoiseGenerator Statistics ===");

    let config = NoiseConfig::default();
    let generator = NoiseGenerator::new(config);

    let mut values = Vec::new();
    for x in 0..100 {
        for y in 0..100 {
            let v = generator.generate(x as f32, y as f32);
            values.push(v);
        }
    }

    // Calculate statistics
    let mean: f64 = values.iter().sum::<f64>() / values.len() as f64;
    let variance: f64 = values.iter()
        .map(|v| (v - mean).powi(2))
        .sum::<f64>() / values.len() as f64;
    let std_dev = variance.sqrt();

    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    println!("Statistics for {} samples:", values.len());
    println!("  Mean: {:.4}", mean);
    println!("  Std Dev: {:.4}", std_dev);
    println!("  Min: {:.4}", min);
    println!("  Max: {:.4}", max);

    assert!(min >= 0.0 && max <= 1.0, "Values outside [0, 1] range");
    assert!(std_dev > 0.05, "Standard deviation too low, noise too uniform");
    assert!(mean > 0.2 && mean < 0.8, "Mean too far from 0.5");

    println!("✓ NoiseGenerator statistics test passed");
}

#[test]
fn test_height_generator_basic() {
    println!("\n=== Testing HeightGenerator ===");

    let mut grid = Grid::new(200, 200, 10);
    grid.generate_points();
    println!("Generated grid with {} points", grid.points.len());

    let config = NoiseConfig::new(42, 0.01, 4, 0.5, 2.0);
    let height_gen = HeightGenerator::new(config);

    let heights = height_gen.generate_for_grid(&grid);

    println!("Generated {} height values", heights.len());
    assert_eq!(heights.len(), grid.points.len(), "Height count mismatch");

    // Check all heights in valid range
    for (i, &h) in heights.iter().enumerate() {
        assert!(h <= 255, "Height {} at index {} exceeds 255", h, i);
    }

    // Calculate statistics
    let mean = heights.iter().map(|&h| h as f64).sum::<f64>() / heights.len() as f64;
    let variance = heights.iter()
        .map(|&h| (h as f64 - mean).powi(2))
        .sum::<f64>() / heights.len() as f64;
    let std_dev = variance.sqrt();

    let min = *heights.iter().min().unwrap();
    let max = *heights.iter().max().unwrap();

    println!("Height statistics:");
    println!("  Mean: {:.2}", mean);
    println!("  Std Dev: {:.2}", std_dev);
    println!("  Min: {}", min);
    println!("  Max: {}", max);

    assert!(std_dev > 10.0, "Heights too uniform");
    assert!(max > min + 50, "Range too small");

    println!("✓ HeightGenerator basic test passed");
}

#[test]
fn test_height_color_map() {
    println!("\n=== Testing HeightColorMap ===");

    let color_map = HeightColorMap::earth_style();

    // Test boundary values
    let color_min = color_map.interpolate(0.0);
    let color_max = color_map.interpolate(1.0);

    println!("Color at height 0.0: {:?}", color_min);
    println!("Color at height 1.0: {:?}", color_max);

    // Verify valid RGBA values
    for c in color_min.iter().chain(color_max.iter()) {
        assert!(*c >= 0.0 && *c <= 1.0, "Color component {} out of range", c);
    }

    // Test interpolation at various heights
    let test_heights = [0.0, 0.25, 0.5, 0.75, 1.0];
    println!("\nColor gradient:");
    for &h in &test_heights {
        let color = color_map.interpolate(h);
        println!("  {:.2}: RGB({:.2}, {:.2}, {:.2})",
                 h, color[0], color[1], color[2]);
    }

    // Test u8 conversion
    let color_128 = color_map.interpolate_u8(128);
    println!("\nColor for height 128/255: {:?}", color_128);

    // Test smoothness (no huge jumps)
    let steps = 100;
    let mut prev_color = color_map.interpolate(0.0);
    for i in 1..=steps {
        let h = i as f32 / steps as f32;
        let color = color_map.interpolate(h);

        let diff: f32 = (0..3)
            .map(|j| (color[j] - prev_color[j]).abs())
            .sum();

        assert!(diff < 0.3, "Color jump too large at height {}: diff = {}", h, diff);
        prev_color = color;
    }

    println!("✓ HeightColorMap test passed");
}

#[test]
fn test_noise_seed_consistency() {
    println!("\n=== Testing Noise Seed Consistency ===");

    let config1 = NoiseConfig::new(999, 0.01, 4, 0.5, 2.0);
    let config2 = NoiseConfig::new(999, 0.01, 4, 0.5, 2.0);

    let gen1 = NoiseGenerator::new(config1);
    let gen2 = NoiseGenerator::new(config2);

    // Test multiple points
    for x in 0..50 {
        for y in 0..50 {
            let v1 = gen1.generate(x as f32 * 10.0, y as f32 * 10.0);
            let v2 = gen2.generate(x as f32 * 10.0, y as f32 * 10.0);

            assert!((v1 - v2).abs() < 1e-10,
                    "Same seed produced different values at ({}, {}): {} vs {}",
                    x, y, v1, v2);
        }
    }

    println!("✓ Seed consistency test passed");
}

#[test]
fn test_different_seeds_produce_different_results() {
    println!("\n=== Testing Different Seeds ===");

    let config1 = NoiseConfig::new(111, 0.01, 4, 0.5, 2.0);
    let config2 = NoiseConfig::new(222, 0.01, 4, 0.5, 2.0);

    let gen1 = NoiseGenerator::new(config1);
    let gen2 = NoiseGenerator::new(config2);

    let mut differences = 0;
    let total = 2500;

    for x in 0..50 {
        for y in 0..50 {
            let v1 = gen1.generate(x as f32 * 10.0, y as f32 * 10.0);
            let v2 = gen2.generate(x as f32 * 10.0, y as f32 * 10.0);

            if (v1 - v2).abs() > 0.01 {
                differences += 1;
            }
        }
    }

    let percentage = (differences as f32 / total as f32) * 100.0;
    println!("Different values: {}/{} ({:.1}%)", differences, total, percentage);

    assert!(percentage > 80.0, "Seeds too similar, only {:.1}% different", percentage);

    println!("✓ Different seeds test passed");
}

#[test]
fn test_integration_full_pipeline() {
    println!("\n=== Testing Full Integration Pipeline ===");

    // Create a small grid
    let mut grid = Grid::new(100, 100, 10);
    grid.generate_points();
    println!("Grid: {} points", grid.points.len());

    // Generate heights
    let noise_config = NoiseConfig::terrain();
    let height_gen = HeightGenerator::new(noise_config);
    let heights = height_gen.generate_for_grid(&grid);
    println!("Heights: {} values", heights.len());

    // Map to colors
    let color_map = HeightColorMap::earth_style();
    let colors: Vec<_> = heights.iter()
        .map(|&h| color_map.interpolate_u8(h))
        .collect();
    println!("Colors: {} values", colors.len());

    assert_eq!(heights.len(), grid.points.len());
    assert_eq!(colors.len(), grid.points.len());

    // Verify diversity
    let unique_heights: std::collections::HashSet<u8> = heights.iter().cloned().collect();
    println!("Unique heights: {}/{}", unique_heights.len(), heights.len());

    assert!(unique_heights.len() > 50, "Too few unique heights");

    println!("✓ Full pipeline integration test passed");
}
