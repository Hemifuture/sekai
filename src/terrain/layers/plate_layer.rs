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
            continental_base: 60.0,
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

    /// Generate plates using LOD multi-resolution approach for broader shapes.
    ///
    /// 1. Cluster fine cells into ~100-150 super-cells (coarse graph)
    /// 2. Run priority-weighted BFS on the coarse graph
    /// 3. Project back to fine cells with boundary noise
    pub fn generate_plates(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (Vec<u16>, Vec<Plate>) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        let n = cells.len();

        // Step 1: Build coarse graph
        let target_clusters = (n / 20).clamp(50, 200);
        let (cell_to_cluster, cluster_cells, cluster_neighbors, cluster_centers) =
            Self::build_coarse_graph(cells, neighbors, target_clusters, &mut rng);

        // Step 2: Assign plates on coarse graph
        let (cluster_plate_ids, mut plates) =
            self.assign_plates_coarse(&cluster_centers, &cluster_neighbors, &mut rng);

        // Step 3: Project to fine cells with boundary noise
        let plate_ids = Self::project_to_fine(
            neighbors,
            &cell_to_cluster,
            &cluster_cells,
            &cluster_plate_ids,
            &cluster_neighbors,
            &mut rng,
        );

        // Rebuild plate cell lists
        for plate in &mut plates {
            plate.cells.clear();
        }
        for (i, &pid) in plate_ids.iter().enumerate() {
            if pid > 0 {
                plates[(pid - 1) as usize].cells.push(i);
            }
        }

        // Handle any unassigned cells (shouldn't happen, but safety net)
        let _ = n;

        (plate_ids, plates)
    }

    /// Step 1: Cluster fine cells into super-cells via greedy BFS.
    fn build_coarse_graph(
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        target_clusters: usize,
        rng: &mut rand::rngs::StdRng,
    ) -> (Vec<u32>, Vec<Vec<usize>>, Vec<Vec<u32>>, Vec<Pos2>) {
        #![allow(clippy::type_complexity)]
        let n = cells.len();
        let cluster_size = (n / target_clusters).max(1);
        let mut cell_to_cluster = vec![u32::MAX; n];
        let mut cluster_cells: Vec<Vec<usize>> = Vec::new();

        // Collect all cell indices, shuffle for random start order
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = rng.random_range(0..=i);
            order.swap(i, j);
        }

        for &start in &order {
            if cell_to_cluster[start] != u32::MAX {
                continue;
            }
            let cluster_id = cluster_cells.len() as u32;
            let mut members = Vec::new();
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(start);
            cell_to_cluster[start] = cluster_id;
            members.push(start);

            while let Some(curr) = queue.pop_front() {
                if members.len() >= cluster_size {
                    break;
                }
                for &nb in &neighbors[curr] {
                    let nb = nb as usize;
                    if cell_to_cluster[nb] == u32::MAX && members.len() < cluster_size {
                        cell_to_cluster[nb] = cluster_id;
                        members.push(nb);
                        queue.push_back(nb);
                    }
                }
            }
            cluster_cells.push(members);
        }

        let num_clusters = cluster_cells.len();

        // Compute cluster centers (centroids)
        let cluster_centers: Vec<Pos2> = cluster_cells
            .iter()
            .map(|members| {
                let (sx, sy) = members.iter().fold((0.0f32, 0.0f32), |(ax, ay), &c| {
                    (ax + cells[c].x, ay + cells[c].y)
                });
                let len = members.len() as f32;
                Pos2 {
                    x: sx / len,
                    y: sy / len,
                }
            })
            .collect();

        // Build cluster neighbor graph
        let mut cluster_neighbor_set: Vec<std::collections::BTreeSet<u32>> =
            vec![std::collections::BTreeSet::new(); num_clusters];
        for (cell_idx, &cid) in cell_to_cluster.iter().enumerate() {
            if cid == u32::MAX {
                continue;
            }
            for &nb in &neighbors[cell_idx] {
                let nb_cid = cell_to_cluster[nb as usize];
                if nb_cid != u32::MAX && nb_cid != cid {
                    cluster_neighbor_set[cid as usize].insert(nb_cid);
                }
            }
        }
        let cluster_neighbors: Vec<Vec<u32>> = cluster_neighbor_set
            .into_iter()
            .map(|s| s.into_iter().collect())
            .collect();

        (
            cell_to_cluster,
            cluster_cells,
            cluster_neighbors,
            cluster_centers,
        )
    }

    /// Step 2: Assign plates on the coarse graph using priority-weighted BFS.
    fn assign_plates_coarse(
        &self,
        cluster_centers: &[Pos2],
        cluster_neighbors: &[Vec<u32>],
        rng: &mut rand::rngs::StdRng,
    ) -> (Vec<u16>, Vec<Plate>) {
        let nc = cluster_centers.len();
        let mut cluster_plate_ids = vec![0u16; nc];
        let num_continental =
            (self.config.num_plates as f32 * self.config.continental_ratio).ceil() as usize;

        // Select seed clusters spread apart
        let mut seed_clusters = Vec::new();
        for _ in 0..self.config.num_plates {
            let mut best = rng.random_range(0..nc);
            let mut best_min_dist = 0.0f32;
            for _ in 0..50 {
                let candidate = rng.random_range(0..nc);
                if cluster_plate_ids[candidate] != 0 {
                    continue;
                }
                let min_d = seed_clusters
                    .iter()
                    .map(|&s: &usize| {
                        let dx = cluster_centers[candidate].x - cluster_centers[s].x;
                        let dy = cluster_centers[candidate].y - cluster_centers[s].y;
                        dx * dx + dy * dy
                    })
                    .fold(f32::MAX, f32::min);
                if min_d > best_min_dist {
                    best_min_dist = min_d;
                    best = candidate;
                }
            }
            seed_clusters.push(best);
        }

        let mut plates = Vec::new();
        for (i, &seed) in seed_clusters.iter().enumerate() {
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
                cells: Vec::new(),
            };
            cluster_plate_ids[seed] = plate.id;
            plates.push(plate);
        }

        // Priority-weighted BFS on coarse graph
        let mut heap: BinaryHeap<PlateFrontier> = seed_clusters
            .iter()
            .enumerate()
            .map(|(i, &s)| PlateFrontier {
                cell: s,
                plate_id: (i + 1) as u16,
                cost: 0.0,
            })
            .collect();

        let mut costs = vec![f32::MAX; nc];
        for (i, &s) in seed_clusters.iter().enumerate() {
            costs[s] = 0.0;
            let _ = i;
        }

        let noise_seed = self.seed.wrapping_mul(2654435761);

        while let Some(front) = heap.pop() {
            if cluster_plate_ids[front.cell] != 0 && cluster_plate_ids[front.cell] != front.plate_id
            {
                continue;
            }
            if front.cost > costs[front.cell] + 0.001 {
                continue;
            }

            let speed = plates[(front.plate_id - 1) as usize].speed;

            for &nb in &cluster_neighbors[front.cell] {
                let nb = nb as usize;
                if cluster_plate_ids[nb] != 0 {
                    continue;
                }
                let noise = {
                    let h = (nb as u64)
                        .wrapping_mul(noise_seed)
                        .wrapping_add(front.plate_id as u64);
                    let h = h.wrapping_mul(0x517cc1b727220a95);
                    (h >> 48) as f32 / 65536.0 * 0.6
                };
                let step_cost = (1.0 / speed) + noise;
                let new_cost = front.cost + step_cost;

                if new_cost < costs[nb] {
                    costs[nb] = new_cost;
                    cluster_plate_ids[nb] = front.plate_id;
                    heap.push(PlateFrontier {
                        cell: nb,
                        plate_id: front.plate_id,
                        cost: new_cost,
                    });
                }
            }
        }

        (cluster_plate_ids, plates)
    }

    /// Step 3: Project coarse plate assignments to fine cells with boundary noise.
    fn project_to_fine(
        neighbors: &[Vec<u32>],
        cell_to_cluster: &[u32],
        cluster_cells: &[Vec<usize>],
        cluster_plate_ids: &[u16],
        cluster_neighbors: &[Vec<u32>],
        rng: &mut rand::rngs::StdRng,
    ) -> Vec<u16> {
        let n = cell_to_cluster.len();
        let mut plate_ids = vec![0u16; n];

        // Direct projection: each cell gets its cluster's plate
        for (cid, members) in cluster_cells.iter().enumerate() {
            let pid = cluster_plate_ids[cid];
            for &cell in members {
                plate_ids[cell] = pid;
            }
        }

        // Find boundary clusters (clusters with neighbors in different plates)
        let mut boundary_clusters = std::collections::HashSet::new();
        for (cid, nbs) in cluster_neighbors.iter().enumerate() {
            let my_plate = cluster_plate_ids[cid];
            for &nb_cid in nbs {
                if cluster_plate_ids[nb_cid as usize] != my_plate {
                    boundary_clusters.insert(cid);
                    break;
                }
            }
        }

        // For boundary clusters, randomly reassign ~25% of edge fine cells
        for &cid in &boundary_clusters {
            let my_plate = cluster_plate_ids[cid];
            for &cell in &cluster_cells[cid] {
                // Check if this fine cell is on the edge (has neighbor in different plate)
                let mut neighbor_plate = None;
                for &nb in &neighbors[cell] {
                    let nb_pid = plate_ids[nb as usize];
                    if nb_pid != 0 && nb_pid != my_plate {
                        neighbor_plate = Some(nb_pid);
                        break;
                    }
                }
                if let Some(other_plate) = neighbor_plate {
                    if rng.random_range(0.0..1.0f32) < 0.25 {
                        plate_ids[cell] = other_plate;
                    }
                }
            }
        }

        plate_ids
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
