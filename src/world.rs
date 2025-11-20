use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_SIZE: usize = 256;
pub const TALUS_SLOPE: f32 = 0.03;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LayerKind {
    Elevation,
    Erosion,
    Moisture,
    Temperature,
    Rivers,
    Tectonics,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub kind: LayerKind,
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
    pub min: f32,
    pub max: f32,
}

impl Layer {
    pub fn new(kind: LayerKind, width: usize, height: usize) -> Self {
        Self {
            kind,
            width,
            height,
            data: vec![0.0; width * height],
            min: f32::MAX,
            max: f32::MIN,
        }
    }

    pub fn normalize_mut(&mut self) {
        self.min = self.data.iter().copied().fold(f32::INFINITY, f32::min);
        self.max = self.data.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let range = (self.max - self.min).max(1e-5);
        for value in &mut self.data {
            *value = (*value - self.min) / range;
        }
        self.min = 0.0;
        self.max = 1.0;
    }

    pub fn sample(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationParameters {
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub erosion_strength: f32,
    pub rainfall: f32,
    pub plate_count: usize,
    pub iterations: usize,
}

impl Default for GenerationParameters {
    fn default() -> Self {
        Self {
            width: DEFAULT_SIZE,
            height: DEFAULT_SIZE,
            sea_level: 0.35,
            erosion_strength: 0.9,
            rainfall: 0.6,
            plate_count: 9,
            iterations: 48,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Plate {
    pub origin: (f32, f32),
    pub velocity: (f32, f32),
    pub elevation: f32,
}

#[derive(Debug, Clone)]
pub struct World {
    pub width: usize,
    pub height: usize,
    layers: HashMap<LayerKind, Layer>,
}

impl World {
    pub fn generate(seed: u64, params: &GenerationParameters) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut elevation = Layer::new(LayerKind::Elevation, params.width, params.height);
        let plates = build_plates(&mut rng, params);
        let tectonics = synthesize_elevation_from_plates(&plates, &mut elevation, params);

        let mut eroded = elevation.clone();
        let erosion_iterations = ((params.iterations as f32) * params.erosion_strength)
            .ceil()
            .max(1.0) as usize;
        thermal_erosion(
            &mut eroded.data,
            params.width,
            params.height,
            erosion_iterations,
        );

        let mut erosion = Layer::new(LayerKind::Erosion, params.width, params.height);
        for (idx, (&base, &final_height)) in
            elevation.data.iter().zip(eroded.data.iter()).enumerate()
        {
            erosion.data[idx] = (base - final_height).max(0.0);
        }
        erosion.normalize_mut();

        elevation = eroded;
        elevation.normalize_mut();

        let river_flow = accumulate_flow(
            &elevation.data,
            params.width,
            params.height,
            params.rainfall,
            params.sea_level,
        );
        let mut rivers = Layer::new(LayerKind::Rivers, params.width, params.height);
        rivers.data = river_flow;
        rivers.normalize_mut();

        let mut moisture = build_moisture(
            &elevation.data,
            &rivers.data,
            params.width,
            params.height,
            params.sea_level,
        );
        moisture.normalize_mut();

        let mut temperature = build_temperature(
            params.width,
            params.height,
            &elevation.data,
            params.sea_level,
        );
        temperature.normalize_mut();

        let mut layers = HashMap::new();
        layers.insert(LayerKind::Elevation, elevation);
        layers.insert(LayerKind::Erosion, erosion);
        layers.insert(LayerKind::Moisture, moisture);
        layers.insert(LayerKind::Temperature, temperature);
        layers.insert(LayerKind::Rivers, rivers);
        layers.insert(LayerKind::Tectonics, tectonics);

        Self {
            width: params.width,
            height: params.height,
            layers,
        }
    }

    pub fn layer(&self, kind: LayerKind) -> Option<&Layer> {
        self.layers.get(&kind)
    }

    pub fn layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.values()
    }
}

fn build_plates(rng: &mut ChaCha8Rng, params: &GenerationParameters) -> Vec<Plate> {
    (0..params.plate_count)
        .map(|_| Plate {
            origin: (rng.gen::<f32>(), rng.gen::<f32>()),
            velocity: (rng.gen_range(-1.0..1.0), rng.gen_range(-1.0..1.0)),
            elevation: rng.gen_range(-0.5..1.5),
        })
        .collect()
}

fn synthesize_elevation_from_plates(
    plates: &[Plate],
    elevation: &mut Layer,
    params: &GenerationParameters,
) -> Layer {
    let mut tectonics = Layer::new(LayerKind::Tectonics, params.width, params.height);
    for y in 0..params.height {
        for x in 0..params.width {
            let xf = x as f32 / params.width as f32;
            let yf = y as f32 / params.height as f32;

            let mut nearest = (f32::MAX, 0usize);
            let mut second = (f32::MAX, 0usize);
            for (idx, plate) in plates.iter().enumerate() {
                let dx = xf - plate.origin.0;
                let dy = yf - plate.origin.1;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < nearest.0 {
                    second = nearest;
                    nearest = (dist, idx);
                } else if dist < second.0 {
                    second = (dist, idx);
                }
            }
            let primary = &plates[nearest.1];
            let secondary = &plates[second.1];
            let base_height = primary.elevation - nearest.0 * 0.8 + secondary.elevation * 0.25;
            let collision = (primary.velocity.0 - secondary.velocity.0)
                .hypot(primary.velocity.1 - secondary.velocity.1);
            tectonics.data[y * params.width + x] = collision;
            elevation.data[y * params.width + x] = base_height;
        }
    }
    tectonics.normalize_mut();
    tectonics
}

fn thermal_erosion(height: &mut [f32], width: usize, height_count: usize, iterations: usize) {
    let mut buffer = vec![0.0; width * height_count];
    for _ in 0..iterations {
        buffer.fill(0.0);
        for y in 0..height_count {
            for x in 0..width {
                let idx = y * width + x;
                let current = height[idx];
                let mut transfers = [(0usize, 0.0f32); 8];
                let mut transfer_count = 0;
                let mut total_transfer = 0.0;

                for (nx, ny) in neighbors(x, y, width, height_count) {
                    let n_idx = ny * width + nx;
                    let delta = current - height[n_idx];
                    if delta > TALUS_SLOPE {
                        let transfer = (delta - TALUS_SLOPE) * 0.25;
                        transfers[transfer_count] = (n_idx, transfer);
                        transfer_count += 1;
                        total_transfer += transfer;
                    }
                }

                if total_transfer > 0.0 {
                    let scale = (current * 0.5 / total_transfer).min(1.0);
                    for (n_idx, transfer) in transfers.into_iter().take(transfer_count) {
                        buffer[n_idx] += transfer * scale;
                    }
                    buffer[idx] -= total_transfer * scale;
                }
            }
        }
        for (h, b) in height.iter_mut().zip(buffer.iter()) {
            *h += *b;
        }
    }
}

const NEIGHBOR_OFFSETS: &[(isize, isize)] = &[
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

fn neighbors(
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> impl Iterator<Item = (usize, usize)> {
    NEIGHBOR_OFFSETS.iter().filter_map(move |(dx, dy)| {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        if (0..width as isize).contains(&nx) && (0..height as isize).contains(&ny) {
            Some((nx as usize, ny as usize))
        } else {
            None
        }
    })
}

fn accumulate_flow(
    elevation: &[f32],
    width: usize,
    height: usize,
    rainfall: f32,
    sea_level: f32,
) -> Vec<f32> {
    let mut flow = vec![rainfall; elevation.len()];
    let mut indices: Vec<usize> = (0..elevation.len()).collect();
    indices.sort_by(|&a, &b| elevation[b].total_cmp(&elevation[a]));

    for idx in indices {
        let x = idx % width;
        let y = idx / width;
        let current_height = elevation[idx];
        let mut lowest = current_height;
        let mut target = None;
        for (nx, ny) in neighbors(x, y, width, height) {
            let n_idx = ny * width + nx;
            if elevation[n_idx] < lowest {
                lowest = elevation[n_idx];
                target = Some(n_idx);
            }
        }
        if let Some(target_idx) = target {
            flow[target_idx] += flow[idx];
        } else if current_height < sea_level {
            // ocean sink retains its own rainfall to keep coasts moist
            flow[idx] += rainfall;
        }
    }

    flow
}

fn build_moisture(
    elevation: &[f32],
    rivers: &[f32],
    width: usize,
    height: usize,
    sea_level: f32,
) -> Layer {
    let mut layer = Layer::new(LayerKind::Moisture, width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let altitude = elevation[idx];
            let river_bonus = (rivers[idx] * 0.6).min(0.6);
            let sea_bonus = if altitude < sea_level { 0.4 } else { 0.0 };
            let latitude = 1.0 - ((y as f32 / height as f32) - 0.5).abs() * 2.0; // equator wetter
            layer.data[idx] = latitude * 0.4 + sea_bonus + river_bonus - altitude * 0.2;
        }
    }
    layer
}

fn build_temperature(width: usize, height: usize, elevation: &[f32], sea_level: f32) -> Layer {
    let mut layer = Layer::new(LayerKind::Temperature, width, height);
    for y in 0..height {
        let latitude_factor = 1.0 - ((y as f32 / height as f32) - 0.5).abs() * 2.0;
        for x in 0..width {
            let idx = y * width + x;
            let lapse_rate = if elevation[idx] > sea_level {
                elevation[idx] * 0.6
            } else {
                0.0
            };
            layer.data[idx] = latitude_factor - lapse_rate;
        }
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erosion_reduces_peaks() {
        let mut grid = vec![0.0; 9];
        grid[4] = 1.0;
        thermal_erosion(&mut grid, 3, 3, 10);
        assert!(grid[4] < 1.0);
        assert!(grid.iter().all(|v| *v >= 0.0));
    }

    #[test]
    fn rivers_flow_downhill() {
        // simple gradient: height decreases to the right
        let width = 4;
        let height = 1;
        let mut elevation = vec![0.9, 0.6, 0.3, 0.0];
        let flow = accumulate_flow(&elevation, width, height, 1.0, 0.2);
        assert!(flow[3] > flow[0]);
        elevation[0] = 0.9; // unchanged check
    }

    #[test]
    fn world_generation_is_deterministic() {
        let params = GenerationParameters::default();
        let a = World::generate(42, &params);
        let b = World::generate(42, &params);
        let layer_a = a.layer(LayerKind::Elevation).unwrap();
        let layer_b = b.layer(LayerKind::Elevation).unwrap();
        assert_eq!(layer_a.data, layer_b.data);
    }

    #[test]
    fn generated_layers_are_normalized_and_sized() {
        let params = GenerationParameters::default();
        let world = World::generate(7, &params);
        for layer in world.layers() {
            assert_eq!(layer.data.len(), params.width * params.height);
            let within_bounds = layer
                .data
                .iter()
                .all(|v| *v >= 0.0 - f32::EPSILON && *v <= 1.0 + f32::EPSILON);
            assert!(within_bounds, "layer {:?} outside [0,1] range", layer.kind);
        }
    }
}
