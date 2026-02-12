//! Tectonic layer — elevation from distance fields (mapgen4-inspired)
//!
//! Uses signed distance from coastline + multi-scale noise + mountain peaks.
//! Based on proven techniques from Red Blob Games / mapgen4 / Brash & Plucky.

use super::plate_layer::{PlateConfig, PlateLayer, PlateType};
use super::r#trait::{LayerOutput, Pos2, TerrainLayer};
use noise::{NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use std::collections::{HashMap, VecDeque};

/// Tectonic configuration
#[derive(Debug, Clone)]
pub struct TectonicConfig {
    pub plate_config: PlateConfig,
    pub mountain_height: f32,
    pub mountain_width: f32,
    pub trench_depth: f32,
    pub ridge_height: f32,
    pub rift_depth: f32,
}

impl Default for TectonicConfig {
    fn default() -> Self {
        Self {
            plate_config: PlateConfig::default(),
            mountain_height: 80.0,
            mountain_width: 20.0,
            trench_depth: 40.0,
            ridge_height: 25.0,
            rift_depth: 30.0,
        }
    }
}

/// Tectonic terrain layer
pub struct TectonicLayer {
    config: TectonicConfig,
    seed: u64,
}

impl Default for TectonicLayer {
    fn default() -> Self {
        Self::new(TectonicConfig::default())
    }
}

impl TectonicLayer {
    pub fn new(config: TectonicConfig) -> Self {
        Self { config, seed: 0 }
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }
}

impl TerrainLayer for TectonicLayer {
    fn name(&self) -> &'static str {
        "Tectonic"
    }

    fn generate(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        _previous: &LayerOutput,
    ) -> LayerOutput {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        let n = cells.len();

        // Step 1: Generate plates (ellipse-based continental mask)
        let plate_layer = PlateLayer::new(self.config.plate_config.clone()).with_seed(self.seed);
        let (plate_ids, plates) = plate_layer.generate_plates(cells, neighbors);

        // Step 2: Compute signed distance from coastline
        // Positive = inland (continental), Negative = seaward (oceanic)
        // Coastline cells are cells that border a different plate type
        let mut signed_dist = vec![0.0f32; n];
        let mut coast_cells = Vec::new();
        {
            let mut queue = VecDeque::new();
            let mut visited = vec![false; n];

            // Find coastline cells (continental cells adjacent to oceanic, or vice versa)
            for i in 0..n {
                if plate_ids[i] == 0 {
                    continue;
                }
                let my_type = plates[(plate_ids[i] - 1) as usize].plate_type;
                let mut is_coast = false;
                for &nb in &neighbors[i] {
                    let ni = nb as usize;
                    if plate_ids[ni] == 0 {
                        continue;
                    }
                    let nb_type = plates[(plate_ids[ni] - 1) as usize].plate_type;
                    if nb_type != my_type {
                        is_coast = true;
                        break;
                    }
                }
                if is_coast {
                    signed_dist[i] = 0.0;
                    visited[i] = true;
                    queue.push_back(i);
                    coast_cells.push(i);
                }
            }

            // BFS outward from coastline with jaggedness (à la mapgen4)
            let noise_seed = self.seed.wrapping_mul(0x9E3779B97F4A7C15);
            while let Some(current) = queue.pop_front() {
                let cd = signed_dist[current];
                let _my_type = if plate_ids[current] == 0 {
                    PlateType::Oceanic
                } else {
                    plates[(plate_ids[current] - 1) as usize].plate_type
                };

                for &nb in &neighbors[current] {
                    let ni = nb as usize;
                    if visited[ni] || plate_ids[ni] == 0 {
                        continue;
                    }

                    // Add jaggedness to distance (triangular distribution)
                    let jag = {
                        let h = (ni as u64)
                            .wrapping_mul(noise_seed)
                            .wrapping_add(current as u64);
                        let h = h.wrapping_mul(0x517cc1b727220a95);
                        let r1 = (h >> 32) as f32 / u32::MAX as f32;
                        let r2 = ((h >> 16) & 0xFFFF) as f32 / 65535.0;
                        (r1 - r2) * 0.3 // Triangular distribution, ±0.3 jag
                    };

                    let step = 1.0 + jag;
                    let nb_type = plates[(plate_ids[ni] - 1) as usize].plate_type;

                    let new_dist = if nb_type == PlateType::Continental {
                        cd + step // Going inland: positive
                    } else {
                        cd - step // Going seaward: negative
                    };

                    // Only update if this gives a more extreme distance
                    let should_update = if nb_type == PlateType::Continental {
                        new_dist > signed_dist[ni] && !visited[ni]
                    } else {
                        new_dist < signed_dist[ni] && !visited[ni]
                    };

                    if should_update || !visited[ni] {
                        signed_dist[ni] = new_dist;
                        visited[ni] = true;
                        queue.push_back(ni);
                    }
                }
            }

            // Handle any unvisited cells (shouldn't happen, but safety)
            for i in 0..n {
                if !visited[i] {
                    signed_dist[i] = if plate_ids[i] != 0
                        && plates[(plate_ids[i] - 1) as usize].plate_type == PlateType::Continental
                    {
                        5.0
                    } else {
                        -5.0
                    };
                }
            }
        }

        // Normalize signed distance
        let max_land_dist = signed_dist.iter().cloned().fold(1.0f32, f32::max);
        let min_ocean_dist = signed_dist.iter().cloned().fold(-1.0f32, f32::min);

        // Step 3: Mountain peaks using BFS distance field (à la mapgen4)
        // Place mountain seeds on continental cells far from coast
        let continental_cells: Vec<usize> = (0..n)
            .filter(|&i| {
                plate_ids[i] != 0
                    && plates[(plate_ids[i] - 1) as usize].plate_type == PlateType::Continental
                    && signed_dist[i] > max_land_dist * 0.3
            })
            .collect();

        let num_peaks = (continental_cells.len() / 40).clamp(3, 20);
        let mut mountain_dist = vec![f32::MAX; n];

        if !continental_cells.is_empty() {
            // Place peaks spread apart
            let mut peak_cells = Vec::new();
            for _ in 0..num_peaks {
                let mut best = continental_cells[rng.random_range(0..continental_cells.len())];
                let mut best_min = 0.0f32;
                for _ in 0..30 {
                    let c = continental_cells[rng.random_range(0..continental_cells.len())];
                    let min_d = peak_cells
                        .iter()
                        .map(|&p: &usize| {
                            let dx = cells[c].x - cells[p].x;
                            let dy = cells[c].y - cells[p].y;
                            (dx * dx + dy * dy).sqrt()
                        })
                        .fold(f32::MAX, f32::min);
                    // Prefer cells that are far from coast AND far from other peaks
                    let score = min_d * signed_dist[c].max(0.0);
                    if score > best_min {
                        best_min = score;
                        best = c;
                    }
                }
                peak_cells.push(best);
            }

            // BFS distance from peaks with jaggedness
            let mut queue = VecDeque::new();
            for &p in &peak_cells {
                mountain_dist[p] = 0.0;
                queue.push_back(p);
            }

            let mtn_noise = self.seed.wrapping_mul(0x6C62272E07BB0142);
            while let Some(current) = queue.pop_front() {
                let cd = mountain_dist[current];
                for &nb in &neighbors[current] {
                    let ni = nb as usize;
                    // Mountain distance only propagates on land
                    if plate_ids[ni] == 0 {
                        continue;
                    }
                    if plates[(plate_ids[ni] - 1) as usize].plate_type != PlateType::Continental {
                        continue;
                    }

                    let jag = {
                        let h = (ni as u64)
                            .wrapping_mul(mtn_noise)
                            .wrapping_add(current as u64);
                        let h = h.wrapping_mul(0x517cc1b727220a95);
                        let r1 = (h >> 32) as f32 / u32::MAX as f32;
                        let r2 = ((h >> 16) & 0xFFFF) as f32 / 65535.0;
                        (r1 - r2) * 0.5 // More jaggedness for mountains
                    };

                    let new_dist = cd + 1.0 + jag;
                    if new_dist < mountain_dist[ni] {
                        mountain_dist[ni] = new_dist;
                        queue.push_back(ni);
                    }
                }
            }
        }

        let max_mtn_dist = mountain_dist
            .iter()
            .filter(|&&d| d < f32::MAX)
            .cloned()
            .fold(1.0f32, f32::max);

        // Step 4: Multi-scale noise layers
        let perlin_n0 = Perlin::new(self.seed as u32);
        let perlin_n1 = Perlin::new(self.seed.wrapping_add(100) as u32);
        let perlin_n2 = Perlin::new(self.seed.wrapping_add(200) as u32);
        let perlin_n4 = Perlin::new(self.seed.wrapping_add(400) as u32);

        // Map bounds for noise coordinates
        let (min_x, max_x) = cells.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
        let (min_y, max_y) = cells.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
        let rx = (max_x - min_x).max(1.0);
        let ry = (max_y - min_y).max(1.0);

        // Step 5: Compute final elevation using mapgen4-style blending
        let continental_base = self.config.plate_config.continental_base;
        let oceanic_base = self.config.plate_config.oceanic_base;

        let mut heights = vec![0.0f32; n];

        for i in 0..n {
            if plate_ids[i] == 0 {
                heights[i] = oceanic_base;
                continue;
            }

            let nx = (cells[i].x - min_x) / rx;
            let ny = (cells[i].y - min_y) / ry;

            // Noise layers at different frequencies
            let n0 = perlin_n0.get([nx as f64 * 2.0, ny as f64 * 2.0]) as f32;
            let n1 = perlin_n1.get([nx as f64 * 4.0, ny as f64 * 4.0]) as f32;
            let n2 = perlin_n2.get([nx as f64 * 8.0, ny as f64 * 8.0]) as f32;
            let n4 = perlin_n4.get([nx as f64 * 16.0, ny as f64 * 16.0]) as f32;

            let sd = signed_dist[i];
            let plate = &plates[(plate_ids[i] - 1) as usize];

            if plate.plate_type == PlateType::Continental {
                // --- LAND ---
                // Normalize inland distance (0 at coast, 1 deep inland)
                let inland_t = (sd / max_land_dist).clamp(0.0, 1.0);

                // Base elevation: rises with distance from coast
                // Concave curve so coast rises quickly then flattens
                let base = continental_base * inland_t.sqrt();

                // Hill component: low-frequency noise modulated by mid-frequency
                let hill_blend = (1.0 + n0) / 2.0; // 0..1
                let hill_noise = n1 * (1.0 - hill_blend) + n2 * hill_blend;
                let hill_height = 15.0 * (1.0 + hill_noise);

                // Mountain component from distance field
                let mtn_contribution = if mountain_dist[i] < f32::MAX {
                    let mtn_t = (mountain_dist[i] / max_mtn_dist).clamp(0.0, 1.0);
                    // Mountains are tallest at peak (mtn_t=0), fall off with distance
                    let mtn_height = self.config.mountain_height * (1.0 - mtn_t.sqrt());
                    mtn_height
                } else {
                    0.0
                };

                // Blend: near coast → hills, deep inland → mix of hills and mountains
                let mtn_blend = inland_t * inland_t; // Quadratic: mountains only deep inland
                let elevation =
                    base + hill_height * (1.0 - mtn_blend) + mtn_contribution * mtn_blend;

                // Add fine detail noise
                let detail = n4 * 5.0 * inland_t; // Detail increases inland

                heights[i] = elevation + detail;
            } else {
                // --- OCEAN ---
                let depth_t = (-sd / -min_ocean_dist).clamp(0.0, 1.0);
                let base = oceanic_base * depth_t.sqrt();

                // Ocean floor variation
                let ocean_noise = n1 * 0.3 + n2 * 0.15;
                let detail = oceanic_base * 0.1 * ocean_noise;

                heights[i] = base + detail;
            }
        }

        // Collect boundary cells for metadata
        let boundary_cells: Vec<u32> = coast_cells.iter().map(|&i| i as u32).collect();

        LayerOutput {
            heights,
            plate_ids: Some(plate_ids),
            boundary_cells: Some(boundary_cells),
            metadata: HashMap::new(),
        }
    }
}
