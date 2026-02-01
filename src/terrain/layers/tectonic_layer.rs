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
    
    /// Classify collision type based on plate types
    fn classify_collision(
        &self,
        plate_a: &Plate,
        plate_b: &Plate,
    ) -> CollisionType {
        use PlateType::*;
        
        match (plate_a.plate_type, plate_b.plate_type) {
            (Continental, Continental) => CollisionType::ContinentalCollision,
            (Continental, Oceanic) | (Oceanic, Continental) => CollisionType::OceanicSubduction,
            (Oceanic, Oceanic) => CollisionType::OceanicCollision,
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
        let plate_layer = PlateLayer::new(self.config.plate_config.clone())
            .with_seed(self.seed);
        let (plate_ids, plates) = plate_layer.generate_plates(cells, neighbors);
        
        // Find boundary cells and their collision types
        let mut boundary_cells = Vec::new();
        let mut boundary_collisions: HashMap<usize, CollisionType> = HashMap::new();
        
        for (i, &plate_id) in plate_ids.iter().enumerate() {
            if plate_id == 0 {
                continue;
            }
            
            for &neighbor in &neighbors[i] {
                let neighbor_plate = plate_ids[neighbor as usize];
                if neighbor_plate != 0 && neighbor_plate != plate_id {
                    // This is a boundary cell
                    let plate_a = &plates[(plate_id - 1) as usize];
                    let plate_b = &plates[(neighbor_plate - 1) as usize];
                    let collision_type = self.classify_collision(plate_a, plate_b);
                    
                    boundary_cells.push(i);
                    boundary_collisions.insert(i, collision_type);
                    break;
                }
            }
        }
        
        // Compute distance field from boundaries
        let distances = self.compute_distance_field(&boundary_cells, neighbors, n);
        
        // Generate heights
        let mut heights = vec![0.0f32; n];
        
        for i in 0..n {
            let plate_id = plate_ids[i];
            if plate_id == 0 {
                continue;
            }
            
            let plate = &plates[(plate_id - 1) as usize];
            
            // Base height from plate type
            let base = match plate.plate_type {
                PlateType::Continental => self.config.plate_config.continental_base,
                PlateType::Oceanic => self.config.plate_config.oceanic_base,
            };
            
            // Tectonic contribution based on distance to nearest boundary
            let distance = distances[i];
            
            // Find nearest boundary's collision type
            let collision_type = if distance < self.config.mountain_width * 2.0 {
                // Find the collision type of the nearest boundary
                let mut nearest_boundary = None;
                let mut nearest_dist = f32::MAX;
                
                for &bc in &boundary_cells {
                    let d = distances[bc];
                    if d < nearest_dist {
                        nearest_dist = d;
                        nearest_boundary = Some(bc);
                    }
                }
                
                nearest_boundary
                    .and_then(|bc| boundary_collisions.get(&bc))
                    .copied()
                    .unwrap_or(CollisionType::ContinentalCollision)
            } else {
                CollisionType::ContinentalCollision
            };
            
            let tectonic = self.terrain_contribution(distance, collision_type, &mut rng);
            
            heights[i] = base + tectonic;
        }
        
        LayerOutput {
            heights,
            plate_ids: Some(plate_ids),
            boundary_cells: Some(boundary_cells.iter().map(|&i| i as u32).collect()),
            metadata: HashMap::new(),
        }
    }
}
