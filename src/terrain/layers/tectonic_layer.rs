//! Tectonic layer - generates terrain along plate boundaries
//!
//! Key insight: Mountains form ALONG boundaries, not as radial patterns.
//! This layer uses boundary cells as ridge lines and creates elevation
//! that falls off with distance.

use super::plate_layer::{Plate, PlateConfig, PlateLayer, PlateType};
use super::r#trait::{LayerOutput, Pos2, TerrainLayer};
use rand::{Rng, SeedableRng};
use std::collections::{HashMap, VecDeque};

/// Collision type determines terrain features
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionType {
    /// Continental + Continental → Mountain range
    ContinentalCollision,
    /// Oceanic subducts under Continental → Trench + Volcanic arc
    OceanicSubduction,
    /// Oceanic + Oceanic → Island arc
    OceanicCollision,
    /// Divergent on continent → Rift valley
    ContinentalRift,
    /// Divergent in ocean → Mid-ocean ridge
    OceanicRidge,
    /// Transform fault
    Transform,
}

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
            trench_depth: 30.0,
            ridge_height: 20.0,
            rift_depth: 25.0,
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

    /// Compute distance from each cell to nearest boundary
    fn compute_distance_field(
        &self,
        boundary_cells: &[usize],
        neighbors: &[Vec<u32>],
        n: usize,
    ) -> Vec<f32> {
        let mut distances = vec![f32::MAX; n];
        let mut queue = VecDeque::new();

        // Initialize boundary cells with distance 0
        for &cell in boundary_cells {
            distances[cell] = 0.0;
            queue.push_back(cell);
        }

        // BFS to compute distances
        while let Some(current) = queue.pop_front() {
            let current_dist = distances[current];

            for &neighbor in &neighbors[current] {
                let neighbor = neighbor as usize;
                let new_dist = current_dist + 1.0;

                if new_dist < distances[neighbor] {
                    distances[neighbor] = new_dist;
                    queue.push_back(neighbor);
                }
            }
        }

        distances
    }

    /// Classify collision type based on plate types and motion vectors
    fn classify_collision(
        &self,
        plate_a: &Plate,
        plate_b: &Plate,
        cell_a: &Pos2,
        cell_b: &Pos2,
    ) -> CollisionType {
        use PlateType::*;

        // Calculate relative motion along the boundary normal
        let dx = cell_b.x - cell_a.x;
        let dy = cell_b.y - cell_a.y;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let nx = dx / len;
        let ny = dy / len;

        // Velocity of plate A and B along the normal
        let va = plate_a.speed * (plate_a.direction.cos() * nx + plate_a.direction.sin() * ny);
        let vb = plate_b.speed * (plate_b.direction.cos() * nx + plate_b.direction.sin() * ny);

        // Relative velocity: positive = converging, negative = diverging
        let relative = va - vb;

        if relative > 0.15 {
            // Convergent
            match (plate_a.plate_type, plate_b.plate_type) {
                (Continental, Continental) => CollisionType::ContinentalCollision,
                (Continental, Oceanic) | (Oceanic, Continental) => CollisionType::OceanicSubduction,
                (Oceanic, Oceanic) => CollisionType::OceanicCollision,
            }
        } else if relative < -0.15 {
            // Divergent
            match (plate_a.plate_type, plate_b.plate_type) {
                (Continental, Continental) => CollisionType::ContinentalRift,
                _ => CollisionType::OceanicRidge,
            }
        } else {
            // Transform
            CollisionType::Transform
        }
    }

    /// Calculate terrain contribution based on distance and collision type
    fn terrain_contribution(
        &self,
        distance: f32,
        collision_type: CollisionType,
        rng: &mut impl Rng,
    ) -> f32 {
        let width = self.config.mountain_width;

        // Gaussian-like falloff
        let falloff = (-distance * distance / (2.0 * width * width)).exp();

        // Add some noise for natural variation
        let noise = rng.random_range(-0.1..0.1);

        match collision_type {
            CollisionType::ContinentalCollision => {
                // Tall mountains
                self.config.mountain_height * falloff * (1.0 + noise)
            }
            CollisionType::OceanicSubduction => {
                // Volcanic arc (mountains) on continental side
                self.config.mountain_height * 0.7 * falloff * (1.0 + noise)
            }
            CollisionType::OceanicCollision => {
                // Island arc
                self.config.mountain_height * 0.5 * falloff * (1.0 + noise)
            }
            CollisionType::ContinentalRift => {
                // Rift valley (negative)
                -self.config.rift_depth * falloff * (1.0 + noise)
            }
            CollisionType::OceanicRidge => {
                // Mid-ocean ridge
                self.config.ridge_height * falloff * (1.0 + noise)
            }
            CollisionType::Transform => {
                // Minimal terrain
                5.0 * falloff * (1.0 + noise)
            }
        }
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

        // Generate plates first
        let plate_layer = PlateLayer::new(self.config.plate_config.clone()).with_seed(self.seed);
        let (plate_ids, plates) = plate_layer.generate_plates(cells, neighbors);

        // Find boundary cells and their collision types
        let mut boundary_cells = Vec::new();
        let mut boundary_collisions: HashMap<usize, CollisionType> = HashMap::new();

        for (i, &plate_id) in plate_ids.iter().enumerate() {
            if plate_id == 0 {
                continue;
            }

            for &neighbor in &neighbors[i] {
                let ni = neighbor as usize;
                let neighbor_plate = plate_ids[ni];
                if neighbor_plate != 0 && neighbor_plate != plate_id {
                    // This is a boundary cell
                    let plate_a = &plates[(plate_id - 1) as usize];
                    let plate_b = &plates[(neighbor_plate - 1) as usize];
                    let collision_type =
                        self.classify_collision(plate_a, plate_b, &cells[i], &cells[ni]);

                    boundary_cells.push(i);
                    boundary_collisions.insert(i, collision_type);
                    break;
                }
            }
        }

        // Compute distance field from boundaries
        let distances = self.compute_distance_field(&boundary_cells, neighbors, n);

        // Build a map from each cell to its nearest boundary's collision type
        // using BFS from boundary cells outward
        let mut nearest_collision: Vec<Option<CollisionType>> = vec![None; n];
        {
            let mut visited = vec![false; n];
            let mut queue = VecDeque::new();
            for &bc in &boundary_cells {
                if let Some(&ct) = boundary_collisions.get(&bc) {
                    nearest_collision[bc] = Some(ct);
                    visited[bc] = true;
                    queue.push_back(bc);
                }
            }
            while let Some(current) = queue.pop_front() {
                let ct = nearest_collision[current].unwrap();
                for &neighbor in &neighbors[current] {
                    let neighbor = neighbor as usize;
                    if !visited[neighbor] {
                        visited[neighbor] = true;
                        nearest_collision[neighbor] = Some(ct);
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        // Compute distance from each cell to nearest cell of a DIFFERENT plate type
        // This is used for continental shelf gradient
        let mut dist_to_type_boundary = vec![f32::MAX; n];
        {
            let mut queue = VecDeque::new();
            for (i, &pid) in plate_ids.iter().enumerate() {
                if pid == 0 {
                    continue;
                }
                let my_type = plates[(pid - 1) as usize].plate_type;
                for &neighbor in &neighbors[i] {
                    let ni = neighbor as usize;
                    let npid = plate_ids[ni];
                    if npid != 0 && plates[(npid - 1) as usize].plate_type != my_type {
                        dist_to_type_boundary[i] = 0.0;
                        queue.push_back(i);
                        break;
                    }
                }
            }
            while let Some(current) = queue.pop_front() {
                let cd = dist_to_type_boundary[current];
                for &neighbor in &neighbors[current] {
                    let ni = neighbor as usize;
                    let nd = cd + 1.0;
                    if nd < dist_to_type_boundary[ni] {
                        dist_to_type_boundary[ni] = nd;
                        queue.push_back(ni);
                    }
                }
            }
        }

        // Generate heights with continental shelf gradient
        let shelf_width = 6.0; // cells over which the shelf gradient applies
        let mut heights = vec![0.0f32; n];

        for i in 0..n {
            let plate_id = plate_ids[i];
            if plate_id == 0 {
                continue;
            }

            let plate = &plates[(plate_id - 1) as usize];
            let continental_base = self.config.plate_config.continental_base;
            let oceanic_base = self.config.plate_config.oceanic_base;

            // Base height from plate type with shelf gradient at edges
            let dist_tb = dist_to_type_boundary[i];
            let base = match plate.plate_type {
                PlateType::Continental => {
                    if dist_tb < shelf_width {
                        // Near ocean: gradually descend toward oceanic level
                        let t = dist_tb / shelf_width;
                        // Smooth hermite interpolation
                        let t = t * t * (3.0 - 2.0 * t);
                        oceanic_base + (continental_base - oceanic_base) * t
                    } else {
                        continental_base
                    }
                }
                PlateType::Oceanic => {
                    if dist_tb < shelf_width {
                        // Near continent: gradually rise toward continental level
                        let t = dist_tb / shelf_width;
                        let t = t * t * (3.0 - 2.0 * t);
                        continental_base + (oceanic_base - continental_base) * t
                    } else {
                        oceanic_base
                    }
                }
            };

            // Intra-continental noise: gentle variation (±12) so interiors aren't flat
            // but minimum stays well above sea level
            let interior_noise =
                if plate.plate_type == PlateType::Continental && dist_tb >= shelf_width {
                    let noise_val = {
                        let h = (i as u64)
                            .wrapping_mul(self.seed.wrapping_mul(0x9E3779B97F4A7C15))
                            .wrapping_add(plate_id as u64);
                        let h = h.wrapping_mul(0x517cc1b727220a95);
                        ((h >> 32) as f32 / u32::MAX as f32) * 2.0 - 1.0 // -1..1
                    };
                    noise_val * 12.0
                } else {
                    0.0
                };

            // Tectonic contribution based on distance to nearest boundary
            let distance = distances[i];

            let collision_type =
                nearest_collision[i].unwrap_or(CollisionType::ContinentalCollision);

            let tectonic = self.terrain_contribution(distance, collision_type, &mut rng);

            heights[i] = base + tectonic + interior_noise;
        }

        // Post-assignment smoothing passes - EDGE ONLY (near plate boundaries)
        // Only smooth cells within 4 hops of a boundary to preserve continental interiors
        for _ in 0..3 {
            let old = heights.clone();
            for i in 0..n {
                if plate_ids[i] == 0 || neighbors[i].is_empty() {
                    continue;
                }
                let dist_b = distances[i];
                // Only smooth cells near plate boundaries (within 4 hops)
                if dist_b > 4.0 {
                    continue;
                }
                let mut sum = old[i];
                let mut count = 1.0f32;
                for &nb in &neighbors[i] {
                    let ni = nb as usize;
                    if plate_ids[ni] != 0 {
                        sum += old[ni];
                        count += 1.0;
                    }
                }
                // Blend strength decreases with distance from boundary
                let blend = 0.4 * (1.0 - dist_b / 5.0);
                if blend > 0.0 {
                    heights[i] = old[i] * (1.0 - blend) + (sum / count) * blend;
                }
            }
        }

        LayerOutput {
            heights,
            plate_ids: Some(plate_ids),
            boundary_cells: Some(boundary_cells.iter().map(|&i| i as u32).collect()),
            metadata: HashMap::new(),
        }
    }
}
