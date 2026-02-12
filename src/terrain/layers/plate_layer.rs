//! Plate layer - tectonic plate generation
//!
//! Generates the basic plate distribution using Voronoi-style expansion.

use super::r#trait::{LayerOutput, Pos2, TerrainLayer};
use rand::{Rng, SeedableRng};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

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
            continental_base: 50.0,
            oceanic_base: -40.0,
        }
    }
}

/// Plate data
#[derive(Debug, Clone)]
pub struct Plate {
    pub id: u16,
    pub plate_type: PlateType,
    pub direction: f32, // radians
    pub speed: f32,
    pub cells: Vec<usize>,
}

/// Priority queue entry for plate expansion
#[derive(Debug, Clone)]
struct PlateFrontier {
    cell: usize,
    plate_id: u16,
    cost: f32,
}

impl PartialEq for PlateFrontier {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for PlateFrontier {}

impl PartialOrd for PlateFrontier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PlateFrontier {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering: lowest cost has highest priority
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
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

    /// Generate plates using priority-weighted BFS for organic shapes
    ///
    /// Each plate gets a random growth speed. The priority queue ensures
    /// faster-growing plates expand first, creating varied plate sizes.
    /// Noise-based cost adds irregularity to plate boundaries.
    pub fn generate_plates(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (Vec<u16>, Vec<Plate>) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        let n = cells.len();

        // Initialize plate IDs (0 = unassigned)
        let mut plate_ids = vec![0u16; n];

        // Choose random seed points for each plate, spread them apart
        let mut plates = Vec::new();
        let num_continental =
            (self.config.num_plates as f32 * self.config.continental_ratio).ceil() as usize;

        // Use rejection sampling to spread seed points
        let mut seed_cells = Vec::new();
        let _min_dist_sq = if n > 100 {
            // Estimate map dimensions from cell positions
            let (min_x, max_x) = cells.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                (lo.min(p.x), hi.max(p.x))
            });
            let (min_y, max_y) = cells.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                (lo.min(p.y), hi.max(p.y))
            });
            let area = (max_x - min_x) * (max_y - min_y);
            // Target distance: spread evenly, then require at least 30% of that
            let target = (area / self.config.num_plates as f32).sqrt() * 0.3;
            target * target
        } else {
            0.0
        };

        for _ in 0..self.config.num_plates {
            // Try to find a cell far from existing seeds
            let mut best_cell = rng.random_range(0..n);
            let mut best_min_dist = 0.0f32;

            for _ in 0..30 {
                let candidate = rng.random_range(0..n);
                if plate_ids[candidate] != 0 {
                    continue;
                }
                let min_d = seed_cells
                    .iter()
                    .map(|&s: &usize| {
                        let dx = cells[candidate].x - cells[s].x;
                        let dy = cells[candidate].y - cells[s].y;
                        dx * dx + dy * dy
                    })
                    .fold(f32::MAX, f32::min);

                if min_d > best_min_dist {
                    best_min_dist = min_d;
                    best_cell = candidate;
                }
            }

            seed_cells.push(best_cell);
        }

        // Create plates with variable growth speeds
        for (i, &seed_cell) in seed_cells.iter().enumerate() {
            let plate_type = if i < num_continental {
                PlateType::Continental
            } else {
                PlateType::Oceanic
            };

            let plate = Plate {
                id: (i + 1) as u16,
                plate_type,
                direction: rng.random_range(0.0..std::f32::consts::TAU),
                speed: rng.random_range(0.6..1.4),
                cells: vec![seed_cell],
            };

            plate_ids[seed_cell] = plate.id;
            plates.push(plate);
        }

        // Priority-weighted BFS: lower cost = expands first
        // Each plate has a growth speed; cost = base_cost / speed + noise
        let mut heap: BinaryHeap<PlateFrontier> = plates
            .iter()
            .map(|p| PlateFrontier {
                cell: p.cells[0],
                plate_id: p.id,
                cost: 0.0,
            })
            .collect();

        let mut costs = vec![f32::MAX; n];
        for p in &plates {
            costs[p.cells[0]] = 0.0;
        }

        // Simple hash-based noise for cost perturbation
        let noise_seed = self.seed.wrapping_mul(2654435761);

        while let Some(front) = heap.pop() {
            if plate_ids[front.cell] != 0 && plate_ids[front.cell] != front.plate_id {
                continue; // Already claimed by another plate
            }
            if front.cost > costs[front.cell] + 0.001 {
                continue; // Stale entry
            }

            let speed = plates[(front.plate_id - 1) as usize].speed;

            for &neighbor in &neighbors[front.cell] {
                let neighbor = neighbor as usize;
                if plate_ids[neighbor] != 0 {
                    continue;
                }

                // Cost: base step (1.0) / speed + noise perturbation
                let noise = {
                    let h = (neighbor as u64)
                        .wrapping_mul(noise_seed)
                        .wrapping_add(front.plate_id as u64);
                    let h = h.wrapping_mul(0x517cc1b727220a95);
                    (h >> 48) as f32 / 65536.0 * 0.6 // 0..0.6 noise
                };
                let step_cost = (1.0 / speed) + noise;
                let new_cost = front.cost + step_cost;

                if new_cost < costs[neighbor] {
                    costs[neighbor] = new_cost;
                    plate_ids[neighbor] = front.plate_id;
                    plates[(front.plate_id - 1) as usize].cells.push(neighbor);
                    heap.push(PlateFrontier {
                        cell: neighbor,
                        plate_id: front.plate_id,
                        cost: new_cost,
                    });
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
        let heights: Vec<f32> = plate_ids
            .iter()
            .map(|&pid| {
                if pid == 0 {
                    return 0.0;
                }
                let plate = &plates[(pid - 1) as usize];
                match plate.plate_type {
                    PlateType::Continental => self.config.continental_base,
                    PlateType::Oceanic => self.config.oceanic_base,
                }
            })
            .collect();

        LayerOutput {
            heights,
            plate_ids: Some(plate_ids),
            boundary_cells: None,
            metadata: std::collections::HashMap::new(),
        }
    }
}
