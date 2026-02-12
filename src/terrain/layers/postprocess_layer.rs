//! Postprocess layer - terrain cleanup and smoothing
//!
//! Removes small islands, fills small lakes, smooths coastlines.

use super::r#trait::{LayerOutput, Pos2, TerrainLayer};
use std::collections::VecDeque;

/// Postprocessing configuration
#[derive(Debug, Clone)]
pub struct PostprocessConfig {
    /// Minimum island size (smaller islands are removed)
    pub min_island_size: usize,
    /// Minimum lake size (smaller lakes are filled)
    pub min_lake_size: usize,
    /// Coastline smoothing iterations
    pub smoothing_iterations: u32,
    /// Target ocean ratio (0.0-1.0), e.g., 0.7 means 70% ocean
    pub ocean_ratio: f32,
}

impl Default for PostprocessConfig {
    fn default() -> Self {
        Self {
            min_island_size: 15,
            min_lake_size: 10,
            smoothing_iterations: 5,
            ocean_ratio: 0.65,
        }
    }
}

/// Postprocessing layer
pub struct PostprocessLayer {
    config: PostprocessConfig,
}

impl Default for PostprocessLayer {
    fn default() -> Self {
        Self::new(PostprocessConfig::default())
    }
}

impl PostprocessLayer {
    pub fn new(config: PostprocessConfig) -> Self {
        Self { config }
    }

    /// Find connected components of land or water
    fn find_components(
        heights: &[f32],
        neighbors: &[Vec<u32>],
        is_target: impl Fn(f32) -> bool,
    ) -> Vec<Vec<usize>> {
        let mut visited = vec![false; heights.len()];
        let mut components = Vec::new();

        for start in 0..heights.len() {
            if visited[start] || !is_target(heights[start]) {
                continue;
            }

            // BFS to find connected component
            let mut component = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back(start);
            visited[start] = true;

            while let Some(current) = queue.pop_front() {
                component.push(current);

                for &neighbor in &neighbors[current] {
                    let neighbor = neighbor as usize;
                    if !visited[neighbor] && is_target(heights[neighbor]) {
                        visited[neighbor] = true;
                        queue.push_back(neighbor);
                    }
                }
            }

            components.push(component);
        }

        components
    }

    /// Remove small islands (set to water level)
    fn remove_small_islands(heights: &mut [f32], neighbors: &[Vec<u32>], min_size: usize) {
        let land_components = Self::find_components(heights, neighbors, |h| h > 0.0);

        for component in land_components {
            if component.len() < min_size {
                for idx in component {
                    heights[idx] = -10.0; // Set to water
                }
            }
        }
    }

    /// Fill small lakes (set to land level)
    fn fill_small_lakes(heights: &mut [f32], neighbors: &[Vec<u32>], min_size: usize) {
        let water_components = Self::find_components(heights, neighbors, |h| h <= 0.0);

        for component in water_components {
            if component.len() < min_size {
                // Find average height of surrounding land
                let mut sum = 0.0f32;
                let mut count = 0;

                for &idx in &component {
                    for &neighbor in &neighbors[idx] {
                        let neighbor = neighbor as usize;
                        if heights[neighbor] > 0.0 {
                            sum += heights[neighbor];
                            count += 1;
                        }
                    }
                }

                let fill_height = if count > 0 { sum / count as f32 } else { 5.0 };

                for idx in component {
                    heights[idx] = fill_height;
                }
            }
        }
    }

    /// Smooth coastline by averaging boundary cells
    fn smooth_coastline(heights: &mut [f32], neighbors: &[Vec<u32>], iterations: u32) {
        for _ in 0..iterations {
            let mut new_heights = heights.to_vec();

            for (i, h) in heights.iter().enumerate() {
                // Check if this is a coastline cell
                let _is_land = *h > 0.0;
                let has_water_neighbor = neighbors[i].iter().any(|&n| heights[n as usize] <= 0.0);
                let has_land_neighbor = neighbors[i].iter().any(|&n| heights[n as usize] > 0.0);

                if has_water_neighbor && has_land_neighbor {
                    // This is a coastline cell - average with neighbors
                    let mut sum = *h;
                    let mut count = 1;

                    for &neighbor in &neighbors[i] {
                        sum += heights[neighbor as usize];
                        count += 1;
                    }

                    new_heights[i] = sum / count as f32;
                }
            }

            heights.copy_from_slice(&new_heights);
        }
    }

    /// Continental shelf pass: smooth cells near land-sea boundaries
    /// Gradually transitions ocean depth near coast and land height near shore
    fn continental_shelf_pass(heights: &mut [f32], neighbors: &[Vec<u32>], max_hops: usize) {
        // Find land-sea boundary cells
        let n = heights.len();
        let mut dist_to_coast = vec![u32::MAX; n];
        let mut queue = VecDeque::new();

        for i in 0..n {
            let is_land = heights[i] > 0.0;
            for &nb in &neighbors[i] {
                if (heights[nb as usize] > 0.0) != is_land {
                    dist_to_coast[i] = 0;
                    queue.push_back(i);
                    break;
                }
            }
        }

        // BFS to compute distance to coast
        while let Some(current) = queue.pop_front() {
            let cd = dist_to_coast[current];
            if cd as usize >= max_hops {
                continue;
            }
            for &nb in &neighbors[current] {
                let ni = nb as usize;
                if cd + 1 < dist_to_coast[ni] {
                    dist_to_coast[ni] = cd + 1;
                    queue.push_back(ni);
                }
            }
        }

        // Smooth cells within max_hops of coast, with strength decreasing with distance
        for _ in 0..3 {
            let old = heights.to_vec();
            for i in 0..n {
                let d = dist_to_coast[i];
                if d == u32::MAX || d as usize > max_hops || neighbors[i].is_empty() {
                    continue;
                }
                let blend = 0.5 * (1.0 - d as f32 / (max_hops as f32 + 1.0));
                if blend <= 0.0 {
                    continue;
                }
                let avg: f32 = neighbors[i].iter().map(|&nb| old[nb as usize]).sum::<f32>()
                    / neighbors[i].len() as f32;
                heights[i] = old[i] * (1.0 - blend) + avg * blend;
            }
        }
    }

    /// Adjust heights to achieve target ocean ratio
    /// This finds the height threshold that gives the desired water percentage
    fn adjust_sea_ratio(heights: &mut [f32], ocean_ratio: f32) {
        if heights.is_empty() || ocean_ratio <= 0.0 || ocean_ratio >= 1.0 {
            return;
        }

        // Sort heights to find the percentile
        let mut sorted: Vec<f32> = heights.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Find the height at the ocean_ratio percentile
        let percentile_idx = ((sorted.len() as f32) * ocean_ratio.clamp(0.0, 0.99)) as usize;
        let threshold = sorted[percentile_idx];

        #[cfg(debug_assertions)]
        println!(
            "调整海平面: ocean_ratio={}, threshold={:.2}",
            ocean_ratio, threshold
        );

        // Shift all heights so that 'threshold' becomes the sea level (0.0)
        // Heights below threshold become negative (water), above become positive (land)
        for h in heights.iter_mut() {
            *h -= threshold;
        }

        #[cfg(debug_assertions)]
        {
            let water_count = heights.iter().filter(|&&h| h <= 0.0).count();
            let actual_ratio = water_count as f32 / heights.len() as f32;
            println!("调整后海洋比例: {:.1}%", actual_ratio * 100.0);
        }
    }
}

impl TerrainLayer for PostprocessLayer {
    fn name(&self) -> &'static str {
        "Postprocess"
    }

    fn generate(
        &self,
        _cells: &[Pos2],
        neighbors: &[Vec<u32>],
        previous: &LayerOutput,
    ) -> LayerOutput {
        let mut output = previous.clone();

        // First: Adjust sea level to achieve target ocean ratio
        Self::adjust_sea_ratio(&mut output.heights, self.config.ocean_ratio);

        // Remove small islands
        Self::remove_small_islands(&mut output.heights, neighbors, self.config.min_island_size);

        // Fill small lakes
        Self::fill_small_lakes(&mut output.heights, neighbors, self.config.min_lake_size);

        // Smooth coastlines
        Self::smooth_coastline(
            &mut output.heights,
            neighbors,
            self.config.smoothing_iterations,
        );

        // Continental shelf pass: gradual transitions near coast
        Self::continental_shelf_pass(&mut output.heights, neighbors, 8);

        output
    }
}
