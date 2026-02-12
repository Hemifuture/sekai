//! Plate layer - tectonic plate generation
//!
//! Uses elliptical continent placement with noise-perturbed edges for naturally
//! broad landmasses, then partitions into tectonic plates via BFS.

use super::r#trait::{LayerOutput, Pos2, TerrainLayer};
use noise::{NoiseFn, Perlin};
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
            continental_base: 80.0,
            oceanic_base: -50.0,
        }
    }
}

/// Plate data
#[derive(Debug, Clone)]
pub struct Plate {
    pub id: u16,
    pub plate_type: PlateType,
    pub direction: f32,
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
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

/// Continent ellipse for placement
struct ContinentEllipse {
    cx: f32,
    cy: f32,
    a: f32,
    b: f32,
    angle: f32,
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

    /// Generate plates using elliptical continent placement.
    ///
    /// 1. Place large ellipses as continents (inherently broad shapes)
    /// 2. Add Perlin noise to edges for organic coastlines
    /// 3. Seed plates in continental/oceanic regions
    /// 4. BFS expand with heavy cross-boundary penalty
    pub fn generate_plates(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (Vec<u16>, Vec<Plate>) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        let n = cells.len();

        // Map bounds
        let (min_x, max_x) = cells.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
        let (min_y, max_y) = cells.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
        let range_x = (max_x - min_x).max(1.0);
        let range_y = (max_y - min_y).max(1.0);

        // Step 1: Place continent ellipses
        let perlin_edge = Perlin::new(self.seed as u32);

        let num_continents = if self.config.continental_ratio >= 0.5 {
            1 // Pangea-like: one massive continent
        } else if self.config.continental_ratio > 0.35 {
            2 // Continental/earth-like: 2 big continents
        } else if self.config.continental_ratio > 0.15 {
            3 // Multiple continents
        } else {
            4 // Archipelago: many smaller ones
        };

        // Target area per continent ellipse
        // Ellipse area = π * a * b. We want total ellipse coverage ≈ continental_ratio * 1.1
        // (slightly more since noise will eat some edges)
        let total_target = self.config.continental_ratio * 1.3; // Overshoot: noise shrinks edges
        let area_per = total_target / num_continents as f32;
        let ab_product = area_per / std::f32::consts::PI;
        // Aspect ratio varies: 1.2 to 1.8
        let mut ellipses = Vec::new();

        for _ in 0..num_continents {
            let aspect = rng.random_range(1.2..1.8);
            let b = (ab_product / aspect).sqrt();
            let a = aspect * b;
            // Clamp to reasonable sizes
            let a = a.clamp(0.08, 0.45);
            let b = b.clamp(0.06, 0.35);

            // Spread centers — ensure no partial overlaps
            // Minimum distance: sum of semi-major axes + gap
            // This ensures continents are clearly separated (no thin land bridges)
            let _min_separation = a + 0.08; // At least 8% gap beyond this ellipse's radius
            let mut best_cx = rng.random_range(0.15..0.85);
            let mut best_cy = rng.random_range(0.15..0.85);
            let mut best_min_dist = 0.0f32;

            for _ in 0..80 {
                let cx = rng.random_range(0.1..0.9);
                let cy = rng.random_range(0.1..0.9);
                let min_d = ellipses
                    .iter()
                    .map(|e: &ContinentEllipse| {
                        let dx = cx - e.cx;
                        let dy = cy - e.cy;
                        let dist = (dx * dx + dy * dy).sqrt();
                        // Effective distance considering ellipse sizes
                        dist - e.a // Subtract existing ellipse's semi-major axis
                    })
                    .fold(f32::MAX, f32::min);
                if min_d > best_min_dist {
                    best_min_dist = min_d;
                    best_cx = cx;
                    best_cy = cy;
                }
            }

            let angle = rng.random_range(0.0..std::f32::consts::PI);
            ellipses.push(ContinentEllipse {
                cx: best_cx,
                cy: best_cy,
                a,
                b,
                angle,
            });
        }

        // Create continent mask
        let is_continental: Vec<bool> = cells
            .iter()
            .map(|p| {
                let nx = (p.x - min_x) / range_x;
                let ny = (p.y - min_y) / range_y;

                for ell in &ellipses {
                    let dx = nx - ell.cx;
                    let dy = ny - ell.cy;
                    let cos_a = ell.angle.cos();
                    let sin_a = ell.angle.sin();
                    let lx = dx * cos_a + dy * sin_a;
                    let ly = -dx * sin_a + dy * cos_a;

                    let dist_sq = (lx / ell.a).powi(2) + (ly / ell.b).powi(2);

                    // Low-freq noise only — prevents thin tendrils at coast
                    let edge_angle = ly.atan2(lx);
                    let noise1 = perlin_edge.get([
                        nx as f64 * 3.0,
                        ny as f64 * 3.0,
                        edge_angle as f64 * 0.3,
                    ]) as f32;
                    // Small perturbation for organic coast, capped to prevent thin tendrils
                    let edge_perturbation = noise1 * 0.12;

                    if dist_sq < (1.0 + edge_perturbation) {
                        return true;
                    }
                }
                false
            })
            .collect();

        // Step 2: Seed plates
        let num_continental_plates = ((self.config.num_plates as f32) * 0.45).ceil() as usize;
        let num_oceanic_plates = self
            .config
            .num_plates
            .saturating_sub(num_continental_plates);

        let continental_cells: Vec<usize> = (0..n).filter(|&i| is_continental[i]).collect();
        let oceanic_cells: Vec<usize> = (0..n).filter(|&i| !is_continental[i]).collect();

        let mut plates = Vec::new();
        let mut plate_ids = vec![0u16; n];

        // Seed continental plates
        let cont_seeds =
            Self::spread_seeds(&continental_cells, cells, num_continental_plates, &mut rng);
        for seed_cell in cont_seeds {
            let plate = Plate {
                id: (plates.len() + 1) as u16,
                plate_type: PlateType::Continental,
                direction: rng.random_range(0.0..std::f32::consts::TAU),
                speed: rng.random_range(0.6..1.4),
                cells: vec![seed_cell],
            };
            plate_ids[seed_cell] = plate.id;
            plates.push(plate);
        }

        // Seed oceanic plates
        let ocean_seeds = Self::spread_seeds(&oceanic_cells, cells, num_oceanic_plates, &mut rng);
        for seed_cell in ocean_seeds {
            let plate = Plate {
                id: (plates.len() + 1) as u16,
                plate_type: PlateType::Oceanic,
                direction: rng.random_range(0.0..std::f32::consts::TAU),
                speed: rng.random_range(0.6..1.4),
                cells: vec![seed_cell],
            };
            plate_ids[seed_cell] = plate.id;
            plates.push(plate);
        }

        // Step 3: BFS expand — plates strongly prefer staying in their region
        let noise_seed = self.seed.wrapping_mul(2654435761);
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

        while let Some(front) = heap.pop() {
            if plate_ids[front.cell] != 0 && plate_ids[front.cell] != front.plate_id {
                continue;
            }
            if front.cost > costs[front.cell] + 0.001 {
                continue;
            }

            let plate = &plates[(front.plate_id - 1) as usize];
            let speed = plate.speed;
            let plate_is_continental = plate.plate_type == PlateType::Continental;

            for &neighbor in &neighbors[front.cell] {
                let neighbor = neighbor as usize;
                if plate_ids[neighbor] != 0 {
                    continue;
                }

                let noise = {
                    let h = (neighbor as u64)
                        .wrapping_mul(noise_seed)
                        .wrapping_add(front.plate_id as u64);
                    let h = h.wrapping_mul(0x517cc1b727220a95);
                    (h >> 48) as f32 / 65536.0 * 0.4
                };

                // Very heavy penalty for crossing continent/ocean boundary
                let cross_penalty = if plate_is_continental != is_continental[neighbor] {
                    15.0
                } else {
                    0.0
                };

                let step_cost = (1.0 / speed) + noise + cross_penalty;
                let new_cost = front.cost + step_cost;

                if new_cost < costs[neighbor] {
                    costs[neighbor] = new_cost;
                    plate_ids[neighbor] = front.plate_id;
                    heap.push(PlateFrontier {
                        cell: neighbor,
                        plate_id: front.plate_id,
                        cost: new_cost,
                    });
                }
            }
        }

        // Rebuild plate cell lists
        for plate in &mut plates {
            plate.cells.clear();
        }
        for (i, &pid) in plate_ids.iter().enumerate() {
            if pid > 0 {
                plates[(pid - 1) as usize].cells.push(i);
            }
        }

        (plate_ids, plates)
    }

    /// Pick `count` seed cells spread apart using rejection sampling
    fn spread_seeds(
        candidates: &[usize],
        cells: &[Pos2],
        count: usize,
        rng: &mut rand::rngs::StdRng,
    ) -> Vec<usize> {
        if candidates.is_empty() || count == 0 {
            return Vec::new();
        }
        let count = count.min(candidates.len());
        let mut seeds = Vec::with_capacity(count);

        for _ in 0..count {
            let mut best = candidates[rng.random_range(0..candidates.len())];
            let mut best_min_dist = 0.0f32;

            for _ in 0..60 {
                let idx = rng.random_range(0..candidates.len());
                let candidate = candidates[idx];
                let min_d = seeds
                    .iter()
                    .map(|&s: &usize| {
                        let dx = cells[candidate].x - cells[s].x;
                        let dy = cells[candidate].y - cells[s].y;
                        dx * dx + dy * dy
                    })
                    .fold(f32::MAX, f32::min);
                if min_d > best_min_dist {
                    best_min_dist = min_d;
                    best = candidate;
                }
            }
            seeds.push(best);
        }
        seeds
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
