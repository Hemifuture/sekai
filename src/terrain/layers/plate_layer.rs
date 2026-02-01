//! Plate layer - tectonic plate generation
//!
//! Generates the basic plate distribution using Voronoi-style expansion.

use super::r#trait::{LayerOutput, Pos2, TerrainLayer};
use rand::{Rng, SeedableRng};
use std::collections::VecDeque;

/// Plate type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlateType {
    Continental,
    Oceanic,
}

/// Boundary type between plates
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryType {
    Convergent { intensity: f32 },
    Divergent { intensity: f32 },
    Transform { intensity: f32 },
}

/// Plate configuration
#[derive(Debug, Clone)]
pub struct PlateConfig {
    pub num_plates: usize,
    pub continental_ratio: f32,
    pub continental_base: f32,
    pub oceanic_base: f32,
}

impl Default for PlateConfig {
    fn default() -> Self {
        Self {
            num_plates: 12,
            continental_ratio: 0.35,
            continental_base: 30.0,
            oceanic_base: -40.0,
        }
    }
}

/// Plate data
#[derive(Debug, Clone)]
pub struct Plate {
    pub id: u16,
    pub plate_type: PlateType,
    pub direction: f32,  // radians
    pub speed: f32,
    pub cells: Vec<usize>,
}

/// Plate generation layer
pub struct PlateLayer {
    config: PlateConfig,
    seed: u64,
}

impl Default for PlateLayer {
    fn default() -> Self {
        Self::new(PlateConfig::default())
    }
}

impl PlateLayer {
    pub fn new(config: PlateConfig) -> Self {
        Self { config, seed: 0 }
    }
    
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }
    
    /// Generate plates using random flood fill
    pub fn generate_plates(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (Vec<u16>, Vec<Plate>) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        let n = cells.len();
        
        // Initialize plate IDs (0 = unassigned)
        let mut plate_ids = vec![0u16; n];
        
        // Choose random seed points for each plate
        let mut available: Vec<usize> = (0..n).collect();
        let mut plates = Vec::new();
        
        let num_continental = (self.config.num_plates as f32 * self.config.continental_ratio) as usize;
        
        for i in 0..self.config.num_plates {
            if available.is_empty() {
                break;
            }
            
            // Pick random seed point
            let seed_idx = rng.gen_range(0..available.len());
            let seed_cell = available.swap_remove(seed_idx);
            
            let plate_type = if i < num_continental {
                PlateType::Continental
            } else {
                PlateType::Oceanic
            };
            
            let plate = Plate {
                id: (i + 1) as u16,
                plate_type,
                direction: rng.gen_range(0.0..std::f32::consts::TAU),
                speed: rng.gen_range(0.5..1.5),
                cells: vec![seed_cell],
            };
            
            plate_ids[seed_cell] = plate.id;
            plates.push(plate);
        }
        
        // Random flood fill to assign remaining cells
        let mut queue: VecDeque<usize> = plates.iter()
            .flat_map(|p| p.cells.iter().copied())
            .collect();
        
        while let Some(current) = queue.pop_front() {
            let current_plate = plate_ids[current];
            
            // Shuffle neighbors for randomness
            let mut neighbor_list: Vec<u32> = neighbors[current].clone();
            for i in (1..neighbor_list.len()).rev() {
                let j = rng.gen_range(0..=i);
                neighbor_list.swap(i, j);
            }
            
            for &neighbor in &neighbor_list {
                let neighbor = neighbor as usize;
                if plate_ids[neighbor] == 0 {
                    plate_ids[neighbor] = current_plate;
                    plates[(current_plate - 1) as usize].cells.push(neighbor);
                    queue.push_back(neighbor);
                }
            }
        }
        
        (plate_ids, plates)
    }
    
    /// Detect boundary cells between plates
    pub fn detect_boundaries(
        &self,
        plate_ids: &[u16],
        neighbors: &[Vec<u32>],
    ) -> Vec<(usize, BoundaryType)> {
        let mut boundaries = Vec::new();
        
        for (i, &plate_id) in plate_ids.iter().enumerate() {
            for &neighbor in &neighbors[i] {
                let neighbor_plate = plate_ids[neighbor as usize];
                if neighbor_plate != plate_id {
                    // This is a boundary cell
                    let boundary_type = BoundaryType::Convergent { intensity: 1.0 };
                    boundaries.push((i, boundary_type));
                    break;
                }
            }
        }
        
        boundaries
    }
}

impl TerrainLayer for PlateLayer {
    fn name(&self) -> &'static str {
        "Plates"
    }
    
    fn generate(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        _previous: &LayerOutput,
    ) -> LayerOutput {
        let (plate_ids, plates) = self.generate_plates(cells, neighbors);
        
        // Generate base heights based on plate type
        let heights: Vec<f32> = plate_ids.iter().map(|&pid| {
            if pid == 0 {
                return 0.0;
            }
            let plate = &plates[(pid - 1) as usize];
            match plate.plate_type {
                PlateType::Continental => self.config.continental_base,
                PlateType::Oceanic => self.config.oceanic_base,
            }
        }).collect();
        
        LayerOutput {
            heights,
            plate_ids: Some(plate_ids),
            boundary_cells: None,
            metadata: std::collections::HashMap::new(),
        }
    }
}
