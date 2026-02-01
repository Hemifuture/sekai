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
}

impl Default for PostprocessConfig {
    fn default() -> Self {
        Self {
            min_island_size: 15,
            min_lake_size: 10,
            smoothing_iterations: 2,
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
        
        // Remove small islands
        Self::remove_small_islands(&mut output.heights, neighbors, self.config.min_island_size);
        
        // Fill small lakes
        Self::fill_small_lakes(&mut output.heights, neighbors, self.config.min_lake_size);
        
        // Smooth coastlines
        Self::smooth_coastline(&mut output.heights, neighbors, self.config.smoothing_iterations);
        
        output
    }
}
