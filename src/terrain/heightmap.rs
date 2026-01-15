// 高度图生成

use super::noise::{NoiseConfig, NoiseGenerator};
use super::plate::{
    BoundaryType, PlateBoundary, PlateGenerator, PlateType, TectonicConfig, TectonicPlate,
};
use super::template::{get_template_by_name, TerrainTemplate};
use super::template_executor::TemplateExecutor;
use eframe::egui::Pos2;
use rayon::prelude::*;

/// 海平面高度阈值
pub const SEA_LEVEL: u8 = 20;

/// 地形生成模式
#[derive(Debug, Clone, PartialEq)]
pub enum TerrainGenerationMode {
    /// 板块构造模拟（物理模拟）
    TectonicSimulation,
    /// 模板生成（使用预设模板）
    Template(String),
}

/// 地形生成配置
#[derive(Debug, Clone)]
pub struct TerrainConfig {
    /// 生成模式
    pub mode: TerrainGenerationMode,
    /// 板块构造配置（仅在 TectonicSimulation 模式下使用）
    pub tectonic: TectonicConfig,
    pub medium_noise_strength: f32,
    pub detail_noise_strength: f32,
    pub continental_noise_mult: f32,
    pub oceanic_noise_mult: f32,
    pub enable_erosion: bool,
    pub erosion_iterations: u32,
    pub smoothing: u32,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            mode: TerrainGenerationMode::Template("earth-like".to_string()),
            tectonic: TectonicConfig::default(),
            medium_noise_strength: 0.2,
            detail_noise_strength: 0.1,
            continental_noise_mult: 1.5,
            oceanic_noise_mult: 0.5,
            enable_erosion: false,
            erosion_iterations: 50,
            smoothing: 0,
        }
    }
}

impl TerrainConfig {
    /// 使用模板生成
    pub fn with_template(template_name: impl Into<String>) -> Self {
        Self {
            mode: TerrainGenerationMode::Template(template_name.into()),
            ..Default::default()
        }
    }

    /// 使用板块构造模拟
    pub fn with_tectonic_simulation(tectonic_config: TectonicConfig) -> Self {
        Self {
            mode: TerrainGenerationMode::TectonicSimulation,
            tectonic: tectonic_config,
            ..Default::default()
        }
    }
}

/// 地形生成器
pub struct TerrainGenerator {
    config: TerrainConfig,
}

impl TerrainGenerator {
    pub fn new(config: TerrainConfig) -> Self {
        Self { config }
    }

    /// 生成完整地形
    /// 返回: (heights, plates, plate_id)
    pub fn generate(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (Vec<u8>, Vec<TectonicPlate>, Vec<u16>) {
        match &self.config.mode {
            TerrainGenerationMode::TectonicSimulation => self.generate_tectonic(cells, neighbors),
            TerrainGenerationMode::Template(template_name) => {
                self.generate_from_template(cells, neighbors, template_name)
            }
        }
    }

    /// 使用模板生成地形
    fn generate_from_template(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        template_name: &str,
    ) -> (Vec<u8>, Vec<TectonicPlate>, Vec<u16>) {
        #[cfg(debug_assertions)]
        println!("使用模板生成地形: {}", template_name);

        // 获取模板
        let template = get_template_by_name(template_name).unwrap_or_else(|| {
            eprintln!(
                "警告: 未找到模板 '{}', 使用默认的 'earth-like' 模板",
                template_name
            );
            TerrainTemplate::earth_like()
        });

        // 计算地图尺寸
        let (min_x, max_x, min_y, max_y) = cells.iter().fold(
            (
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ),
            |(min_x, max_x, min_y, max_y), pos| {
                (
                    min_x.min(pos.x),
                    max_x.max(pos.x),
                    min_y.min(pos.y),
                    max_y.max(pos.y),
                )
            },
        );
        let width = (max_x - min_x) as u32;
        let height = (max_y - min_y) as u32;

        // 执行模板
        let executor = TemplateExecutor::new(width, height, self.config.tectonic.seed);
        let mut heights = executor.execute(&template, cells, neighbors);

        // 可选：添加细节噪声
        if self.config.detail_noise_strength > 0.0 {
            let detail_noise_config = NoiseConfig {
                octaves: 4,
                base_frequency: 0.08,
                persistence: 0.4,
                lacunarity: 2.2,
                seed: (self.config.tectonic.seed + 1) as u32,
            };

            let generator = NoiseGenerator::new(detail_noise_config.seed);
            let strengths = vec![self.config.detail_noise_strength; cells.len()];
            let noise_values =
                generator.generate_constrained_noise(cells, &detail_noise_config, &strengths);

            for (i, &noise) in noise_values.iter().enumerate() {
                heights[i] += noise * 30.0; // 添加噪声细节
            }
        }

        // 可选：侵蚀
        if self.config.enable_erosion {
            self.thermal_erosion(&mut heights, neighbors, self.config.erosion_iterations);
        }

        // 可选：额外平滑
        if self.config.smoothing > 0 {
            self.smooth_heights(&mut heights, neighbors, self.config.smoothing);
        }

        // 确保归一化
        self.normalize_heights(&mut heights);

        // 转换为 u8
        let heights_u8: Vec<u8> = heights.iter().map(|&h| h.clamp(0.0, 255.0) as u8).collect();

        // 模板模式下不生成板块数据
        let plates = Vec::new();
        let plate_id = vec![0; cells.len()];

        (heights_u8, plates, plate_id)
    }

    /// 使用板块构造模拟生成地形
    fn generate_tectonic(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (Vec<u8>, Vec<TectonicPlate>, Vec<u16>) {
        // ====== 阶段 1: 板块构造模拟 ======
        let (mut heights, plates, plate_id) = self.simulate_plate_tectonics(cells, neighbors);

        // ====== 阶段 2: 中尺度噪声（大地貌） ======
        let medium_noise_config = NoiseConfig {
            octaves: 3,
            base_frequency: 0.01,
            persistence: 0.5,
            lacunarity: 2.0,
            seed: self.config.tectonic.seed as u32,
        };

        self.apply_detail_noise(
            &mut heights,
            &plates,
            &plate_id,
            cells,
            neighbors,
            &medium_noise_config,
            self.config.medium_noise_strength,
        );

        // ====== 阶段 3: 侵蚀模拟（可选） ======
        if self.config.enable_erosion {
            self.thermal_erosion(&mut heights, neighbors, self.config.erosion_iterations);
        }

        // ====== 阶段 4: 小尺度噪声（细节） ======
        let detail_noise_config = NoiseConfig {
            octaves: 5,
            base_frequency: 0.05,
            persistence: 0.4,
            lacunarity: 2.2,
            seed: (self.config.tectonic.seed + 1) as u32,
        };

        self.apply_detail_noise(
            &mut heights,
            &plates,
            &plate_id,
            cells,
            neighbors,
            &detail_noise_config,
            self.config.detail_noise_strength,
        );

        // ====== 阶段 5: 归一化与后处理 ======
        self.normalize_heights(&mut heights);

        if self.config.smoothing > 0 {
            self.smooth_heights(&mut heights, neighbors, self.config.smoothing);
        }

        // 转换为 u8
        let heights_u8: Vec<u8> = heights.iter().map(|&h| h.clamp(0.0, 255.0) as u8).collect();

        (heights_u8, plates, plate_id)
    }

    /// 板块构造模拟
    fn simulate_plate_tectonics(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (Vec<f32>, Vec<TectonicPlate>, Vec<u16>) {
        let generator = PlateGenerator::new(self.config.tectonic.clone());

        // 1. 生成板块
        let (plates, plate_id) = generator.generate_plates(cells, neighbors);

        // 2. 初始化高度（基于板块类型）
        let mut heights: Vec<f32> = plate_id
            .iter()
            .map(|&pid| {
                if pid == 0 {
                    0.0
                } else {
                    plates[(pid - 1) as usize].plate_type.base_height()
                }
            })
            .collect();

        // 3. 分析边界
        let boundaries = generator.analyze_boundaries(&plates, &plate_id, cells, neighbors);

        // 4. 迭代模拟
        for _ in 0..self.config.tectonic.iterations {
            // 应用边界效应
            self.apply_boundary_effects(&mut heights, &boundaries, &plate_id, neighbors);

            // 地壳均衡调整
            self.apply_isostasy(&mut heights, neighbors);
        }

        (heights, plates, plate_id)
    }

    /// 应用边界效应
    fn apply_boundary_effects(
        &self,
        heights: &mut [f32],
        boundaries: &[PlateBoundary],
        plate_id: &[u16],
        neighbors: &[Vec<u32>],
    ) {
        for boundary in boundaries {
            match boundary.boundary_type {
                BoundaryType::Convergent {
                    intensity,
                    subducting_plate,
                } => {
                    self.apply_convergent_effects(
                        heights,
                        boundary,
                        plate_id,
                        neighbors,
                        intensity,
                        subducting_plate,
                    );
                }
                BoundaryType::Divergent { intensity } => {
                    self.apply_divergent_effects(heights, boundary, plate_id, neighbors, intensity);
                }
                BoundaryType::Transform { .. } => {
                    // 转换边界对高度影响较小，暂不处理
                }
            }
        }
    }

    /// 应用汇聚边界效应
    fn apply_convergent_effects(
        &self,
        heights: &mut [f32],
        boundary: &PlateBoundary,
        plate_id: &[u16],
        neighbors: &[Vec<u32>],
        intensity: f32,
        subducting_plate: Option<u16>,
    ) {
        let boundary_width = self.config.tectonic.boundary_width as usize;

        for &cell_idx in &boundary.cells {
            let cell_idx = cell_idx as usize;

            // 使用 BFS 扩展影响范围
            let mut visited = vec![false; heights.len()];
            let mut queue = std::collections::VecDeque::new();
            queue.push_back((cell_idx, 0));
            visited[cell_idx] = true;

            while let Some((current, distance)) = queue.pop_front() {
                if distance >= boundary_width {
                    continue;
                }

                let falloff = 1.0 - (distance as f32 / boundary_width as f32);

                match subducting_plate {
                    Some(subducting_id) => {
                        // 俯冲带
                        if plate_id[current] == subducting_id {
                            // 俯冲板块下沉（海沟）
                            heights[current] -= self.config.tectonic.subduction_depth_rate
                                * intensity
                                * falloff
                                * 0.1;
                        } else {
                            // 上覆板块隆起（火山弧）
                            heights[current] += self.config.tectonic.collision_uplift_rate
                                * intensity
                                * falloff
                                * 0.1;
                        }
                    }
                    None => {
                        // 大陆-大陆碰撞：两侧都隆起
                        heights[current] +=
                            self.config.tectonic.collision_uplift_rate * intensity * falloff * 0.15;
                    }
                }

                // 扩展到邻居
                for &neighbor_idx in &neighbors[current] {
                    let neighbor_idx = neighbor_idx as usize;
                    if !visited[neighbor_idx] {
                        visited[neighbor_idx] = true;
                        queue.push_back((neighbor_idx, distance + 1));
                    }
                }
            }
        }
    }

    /// 应用分离边界效应
    fn apply_divergent_effects(
        &self,
        heights: &mut [f32],
        boundary: &PlateBoundary,
        _plate_id: &[u16],
        neighbors: &[Vec<u32>],
        intensity: f32,
    ) {
        let boundary_width = self.config.tectonic.boundary_width as usize;

        for &cell_idx in &boundary.cells {
            let cell_idx = cell_idx as usize;

            let mut visited = vec![false; heights.len()];
            let mut queue = std::collections::VecDeque::new();
            queue.push_back((cell_idx, 0));
            visited[cell_idx] = true;

            while let Some((current, distance)) = queue.pop_front() {
                if distance >= boundary_width {
                    continue;
                }

                let falloff = 1.0 - (distance as f32 / boundary_width as f32);

                // 裂谷：下沉
                heights[current] -=
                    self.config.tectonic.rift_depth_rate * intensity * falloff * 0.1;

                for &neighbor_idx in &neighbors[current] {
                    let neighbor_idx = neighbor_idx as usize;
                    if !visited[neighbor_idx] {
                        visited[neighbor_idx] = true;
                        queue.push_back((neighbor_idx, distance + 1));
                    }
                }
            }
        }
    }

    /// 地壳均衡调整
    fn apply_isostasy(&self, heights: &mut [f32], neighbors: &[Vec<u32>]) {
        let original = heights.to_vec();

        for i in 0..heights.len() {
            if neighbors[i].is_empty() {
                continue;
            }

            let neighbor_avg: f32 = neighbors[i]
                .iter()
                .map(|&n| original[n as usize])
                .sum::<f32>()
                / neighbors[i].len() as f32;

            heights[i] += (neighbor_avg - heights[i]) * self.config.tectonic.isostasy_rate;
        }
    }

    /// 应用噪声细节
    fn apply_detail_noise(
        &self,
        heights: &mut [f32],
        plates: &[TectonicPlate],
        plate_id: &[u16],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        noise_config: &NoiseConfig,
        base_strength: f32,
    ) {
        let generator = NoiseGenerator::new(noise_config.seed);

        // 计算每个单元格的噪声强度
        let strengths: Vec<f32> = (0..heights.len())
            .into_par_iter()
            .map(|i| {
                let pid = plate_id[i];
                if pid == 0 {
                    return 0.0;
                }

                let plate = &plates[(pid - 1) as usize];

                // 基础强度（受板块类型影响）
                let type_strength = match plate.plate_type {
                    PlateType::Continental => base_strength * self.config.continental_noise_mult,
                    PlateType::Oceanic => base_strength * self.config.oceanic_noise_mult,
                };

                // 边界抑制
                let boundary_dist = self.calculate_boundary_distance(i, plate, neighbors);
                let boundary_suppression = 1.0 - (-boundary_dist * 5.0).exp();

                // 高度调制
                let h = heights[i];
                let erosion_factor = if h > SEA_LEVEL as f32 {
                    1.0 + (h - SEA_LEVEL as f32) / 255.0 * 0.5
                } else {
                    0.5
                };

                type_strength * boundary_suppression * erosion_factor
            })
            .collect();

        // 生成并应用噪声
        let noise_values = generator.generate_constrained_noise(cells, noise_config, &strengths);

        for (i, &noise) in noise_values.iter().enumerate() {
            heights[i] += noise * 255.0;
        }
    }

    /// 计算到板块边界的距离
    fn calculate_boundary_distance(
        &self,
        cell_idx: usize,
        plate: &TectonicPlate,
        neighbors: &[Vec<u32>],
    ) -> f32 {
        // 如果是边界单元格，距离为 0
        if plate.boundary_cells.contains(&(cell_idx as u32)) {
            return 0.0;
        }

        // BFS 查找最近边界
        let mut visited = vec![false; neighbors.len()];
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((cell_idx, 0));
        visited[cell_idx] = true;

        while let Some((current, distance)) = queue.pop_front() {
            if plate.boundary_cells.contains(&(current as u32)) {
                return distance as f32;
            }

            if distance > 10 {
                // 限制搜索深度
                break;
            }

            for &neighbor_idx in &neighbors[current] {
                let neighbor_idx = neighbor_idx as usize;
                if !visited[neighbor_idx] {
                    visited[neighbor_idx] = true;
                    queue.push_back((neighbor_idx, distance + 1));
                }
            }
        }

        10.0 // 默认距离
    }

    /// 热力侵蚀
    fn thermal_erosion(&self, heights: &mut [f32], neighbors: &[Vec<u32>], iterations: u32) {
        let talus = 5.0; // 安息角阈值

        for _ in 0..iterations {
            let original = heights.to_vec();

            for i in 0..heights.len() {
                for &n in &neighbors[i] {
                    let n = n as usize;
                    let diff = original[i] - original[n];
                    if diff > talus {
                        let transfer = (diff - talus) * 0.5;
                        heights[i] -= transfer;
                        heights[n] += transfer;
                    }
                }
            }
        }
    }

    /// 归一化高度值
    fn normalize_heights(&self, heights: &mut [f32]) {
        if heights.is_empty() {
            return;
        }

        let min = heights.iter().copied().fold(f32::INFINITY, f32::min);
        let max = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);

        if (max - min).abs() < 0.001 {
            return;
        }

        for h in heights.iter_mut() {
            *h = (*h - min) / (max - min) * 255.0;
        }
    }

    /// 平滑处理
    fn smooth_heights(&self, heights: &mut [f32], neighbors: &[Vec<u32>], iterations: u32) {
        for _ in 0..iterations {
            let original = heights.to_vec();

            for i in 0..heights.len() {
                if neighbors[i].is_empty() {
                    continue;
                }

                let neighbor_avg: f32 = neighbors[i]
                    .iter()
                    .map(|&n| original[n as usize])
                    .sum::<f32>()
                    / neighbors[i].len() as f32;

                heights[i] = heights[i] * 0.7 + neighbor_avg * 0.3;
            }
        }
    }
}
