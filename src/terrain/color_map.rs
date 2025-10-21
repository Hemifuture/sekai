use serde::{Deserialize, Serialize};

/// A color stop in a gradient, mapping a height value to a color
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorStop {
    /// Normalized height value [0.0, 1.0]
    pub height: f32,

    /// RGBA color (each component in range [0.0, 1.0])
    pub color: [f32; 4],
}

impl ColorStop {
    pub fn new(height: f32, color: [f32; 4]) -> Self {
        Self { height, color }
    }
}

/// Maps height values to colors using gradient interpolation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeightColorMap {
    stops: Vec<ColorStop>,
}

impl HeightColorMap {
    /// Create a new color map from a list of color stops
    /// Stops should be ordered by height (lowest to highest)
    pub fn new(mut stops: Vec<ColorStop>) -> Self {
        // Sort stops by height to ensure correct interpolation
        stops.sort_by(|a, b| a.height.partial_cmp(&b.height).unwrap());
        Self { stops }
    }

    /// Create a color map with Earth-like terrain colors
    /// Deep ocean -> Shallow water -> Beach -> Plains -> Hills -> Mountains -> Snow peaks
    pub fn earth_style() -> Self {
        Self::new(vec![
            // Deep ocean (dark blue)
            ColorStop::new(0.0, [0.0, 0.0, 0.5, 1.0]),
            // Shallow ocean (blue)
            ColorStop::new(0.35, [0.1, 0.3, 0.8, 1.0]),
            // Beach/coast (light sand)
            ColorStop::new(0.4, [0.8, 0.8, 0.5, 1.0]),
            // Low plains (green)
            ColorStop::new(0.45, [0.2, 0.6, 0.2, 1.0]),
            // Plains (bright green)
            ColorStop::new(0.55, [0.3, 0.7, 0.3, 1.0]),
            // Hills (dark green)
            ColorStop::new(0.65, [0.4, 0.5, 0.2, 1.0]),
            // Mountains (brown)
            ColorStop::new(0.75, [0.5, 0.4, 0.3, 1.0]),
            // High mountains (gray)
            ColorStop::new(0.85, [0.6, 0.6, 0.6, 1.0]),
            // Snow peaks (white)
            ColorStop::new(1.0, [0.95, 0.95, 0.95, 1.0]),
        ])
    }

    /// Create a simple grayscale color map
    pub fn grayscale() -> Self {
        Self::new(vec![
            ColorStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
            ColorStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
        ])
    }

    /// Create a color map with vibrant, fantasy-style colors
    pub fn fantasy_style() -> Self {
        Self::new(vec![
            // Deep void (dark purple)
            ColorStop::new(0.0, [0.1, 0.0, 0.2, 1.0]),
            // Dark water (deep blue)
            ColorStop::new(0.3, [0.0, 0.2, 0.6, 1.0]),
            // Shallow water (cyan)
            ColorStop::new(0.4, [0.0, 0.6, 0.8, 1.0]),
            // Grassland (bright green)
            ColorStop::new(0.5, [0.3, 0.8, 0.2, 1.0]),
            // Forest (dark green)
            ColorStop::new(0.6, [0.2, 0.5, 0.2, 1.0]),
            // Hills (yellow-brown)
            ColorStop::new(0.7, [0.7, 0.6, 0.3, 1.0]),
            // Mountains (orange-brown)
            ColorStop::new(0.8, [0.7, 0.4, 0.2, 1.0]),
            // Peaks (red-brown)
            ColorStop::new(0.9, [0.6, 0.3, 0.2, 1.0]),
            // Snow (white with blue tint)
            ColorStop::new(1.0, [0.9, 0.95, 1.0, 1.0]),
        ])
    }

    /// Interpolate color for a given height value [0.0, 1.0]
    pub fn interpolate(&self, height: f32) -> [f32; 4] {
        let height = height.clamp(0.0, 1.0);

        if self.stops.is_empty() {
            return [1.0, 0.0, 1.0, 1.0]; // Magenta for error
        }

        if self.stops.len() == 1 {
            return self.stops[0].color;
        }

        // Find the two stops to interpolate between
        let mut lower_idx = 0;
        let mut upper_idx = 0;

        for (i, stop) in self.stops.iter().enumerate() {
            if stop.height <= height {
                lower_idx = i;
            }
            if stop.height >= height && upper_idx == 0 {
                upper_idx = i;
            }
        }

        // If height is below first stop
        if height <= self.stops[0].height {
            return self.stops[0].color;
        }

        // If height is above last stop
        if height >= self.stops[self.stops.len() - 1].height {
            return self.stops[self.stops.len() - 1].color;
        }

        // Interpolate between lower and upper stops
        let lower = &self.stops[lower_idx];
        let upper = &self.stops[upper_idx];

        if lower_idx == upper_idx {
            return lower.color;
        }

        let range = upper.height - lower.height;
        let t = if range > 0.0 {
            (height - lower.height) / range
        } else {
            0.0
        };

        // Linear interpolation for each color component
        [
            lerp(lower.color[0], upper.color[0], t),
            lerp(lower.color[1], upper.color[1], t),
            lerp(lower.color[2], upper.color[2], t),
            lerp(lower.color[3], upper.color[3], t),
        ]
    }

    /// Convert a u8 height value [0, 255] to color
    pub fn interpolate_u8(&self, height: u8) -> [f32; 4] {
        let normalized = height as f32 / 255.0;
        self.interpolate(normalized)
    }

    /// Get the color stops
    pub fn stops(&self) -> &[ColorStop] {
        &self.stops
    }
}

/// Linear interpolation between two values
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate_boundary_values() {
        let color_map = HeightColorMap::new(vec![
            ColorStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
            ColorStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
        ]);

        // Test lower boundary
        let color_min = color_map.interpolate(0.0);
        assert_eq!(color_min, [0.0, 0.0, 0.0, 1.0]);

        // Test upper boundary
        let color_max = color_map.interpolate(1.0);
        assert_eq!(color_max, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_interpolate_middle_value() {
        let color_map = HeightColorMap::new(vec![
            ColorStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
            ColorStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
        ]);

        let color_mid = color_map.interpolate(0.5);

        // Should be approximately halfway
        for i in 0..3 {
            assert!(
                (color_mid[i] - 0.5).abs() < 0.01,
                "Component {} should be ~0.5, got {}",
                i,
                color_mid[i]
            );
        }
    }

    #[test]
    fn test_interpolate_clamping() {
        let color_map = HeightColorMap::new(vec![
            ColorStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
            ColorStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
        ]);

        // Test values outside [0, 1] are clamped
        let color_below = color_map.interpolate(-0.5);
        assert_eq!(color_below, [0.0, 0.0, 0.0, 1.0]);

        let color_above = color_map.interpolate(1.5);
        assert_eq!(color_above, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_multiple_stops_interpolation() {
        let color_map = HeightColorMap::new(vec![
            ColorStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),   // Black
            ColorStop::new(0.5, [1.0, 0.0, 0.0, 1.0]),   // Red
            ColorStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),   // White
        ]);

        // At 0.25 should interpolate between black and red
        let color_quarter = color_map.interpolate(0.25);
        assert!(color_quarter[0] > 0.4 && color_quarter[0] < 0.6); // Red component ~0.5
        assert!(color_quarter[1] < 0.1); // Green component ~0
        assert!(color_quarter[2] < 0.1); // Blue component ~0

        // At 0.75 should interpolate between red and white
        let color_three_quarter = color_map.interpolate(0.75);
        assert!(color_three_quarter[0] > 0.9); // Red component ~1
        assert!(color_three_quarter[1] > 0.4 && color_three_quarter[1] < 0.6); // Green ~0.5
        assert!(color_three_quarter[2] > 0.4 && color_three_quarter[2] < 0.6); // Blue ~0.5
    }

    #[test]
    fn test_single_stop() {
        let color_map = HeightColorMap::new(vec![
            ColorStop::new(0.5, [0.5, 0.5, 0.5, 1.0]),
        ]);

        // Any height should return the single color
        assert_eq!(color_map.interpolate(0.0), [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(color_map.interpolate(0.5), [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(color_map.interpolate(1.0), [0.5, 0.5, 0.5, 1.0]);
    }

    #[test]
    fn test_empty_color_map() {
        let color_map = HeightColorMap::new(vec![]);

        // Should return magenta error color
        let color = color_map.interpolate(0.5);
        assert_eq!(color, [1.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn test_unordered_stops_are_sorted() {
        let color_map = HeightColorMap::new(vec![
            ColorStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
            ColorStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
            ColorStop::new(0.5, [0.5, 0.0, 0.0, 1.0]),
        ]);

        // Should still interpolate correctly after sorting
        let color_low = color_map.interpolate(0.0);
        assert_eq!(color_low, [0.0, 0.0, 0.0, 1.0]);

        let color_high = color_map.interpolate(1.0);
        assert_eq!(color_high, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn test_interpolate_u8() {
        let color_map = HeightColorMap::new(vec![
            ColorStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
            ColorStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
        ]);

        let color_0 = color_map.interpolate_u8(0);
        assert_eq!(color_0, [0.0, 0.0, 0.0, 1.0]);

        let color_255 = color_map.interpolate_u8(255);
        assert_eq!(color_255, [1.0, 1.0, 1.0, 1.0]);

        let color_128 = color_map.interpolate_u8(128);
        // Should be approximately middle gray
        for i in 0..3 {
            assert!(
                (color_128[i] - 0.5).abs() < 0.01,
                "Component {} should be ~0.5, got {}",
                i,
                color_128[i]
            );
        }
    }

    #[test]
    fn test_earth_style_preset() {
        let color_map = HeightColorMap::earth_style();

        // Test that it has reasonable number of stops
        assert!(color_map.stops().len() >= 5);

        // Test that interpolation works
        let deep_ocean = color_map.interpolate(0.0);
        let land = color_map.interpolate(0.5);
        let mountain = color_map.interpolate(0.8);
        let peak = color_map.interpolate(1.0);

        // Deep ocean should be dark and blue
        assert!(deep_ocean[2] > deep_ocean[0]); // More blue than red
        assert!(deep_ocean[2] > deep_ocean[1]); // More blue than green

        // Peak should be light (high values)
        assert!(peak[0] > 0.8 && peak[1] > 0.8 && peak[2] > 0.8);

        // All should be valid
        for color in [deep_ocean, land, mountain, peak] {
            for component in color {
                assert!(component >= 0.0 && component <= 1.0);
            }
        }
    }

    #[test]
    fn test_grayscale_preset() {
        let color_map = HeightColorMap::grayscale();

        // Test various heights
        for h in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let color = color_map.interpolate(h);
            // All RGB components should be equal (grayscale)
            assert!(
                (color[0] - color[1]).abs() < 0.01,
                "R and G should be equal for grayscale"
            );
            assert!(
                (color[1] - color[2]).abs() < 0.01,
                "G and B should be equal for grayscale"
            );
        }
    }

    #[test]
    fn test_fantasy_style_preset() {
        let color_map = HeightColorMap::fantasy_style();

        // Test that it has stops and produces valid colors
        assert!(color_map.stops().len() >= 5);

        for h in [0.0, 0.2, 0.4, 0.6, 0.8, 1.0] {
            let color = color_map.interpolate(h);
            for component in color {
                assert!(
                    component >= 0.0 && component <= 1.0,
                    "Invalid color component {} at height {}",
                    component,
                    h
                );
            }
        }
    }

    #[test]
    fn test_color_smoothness() {
        let color_map = HeightColorMap::earth_style();

        // Test that colors change gradually (no huge jumps)
        let steps = 100;
        let mut prev_color = color_map.interpolate(0.0);

        for i in 1..=steps {
            let h = i as f32 / steps as f32;
            let color = color_map.interpolate(h);

            // Calculate color difference
            let diff: f32 = (0..3)
                .map(|j| (color[j] - prev_color[j]).abs())
                .sum();

            // Each step should change by a reasonable amount (not too sudden)
            assert!(
                diff < 0.3,
                "Color change too abrupt at height {}: diff = {}",
                h,
                diff
            );

            prev_color = color;
        }
    }
}
