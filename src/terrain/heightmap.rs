// 高度图生成

use super::layered_generator::LayeredGenerator;
use super::layers::{
    DetailLayer, PlateConfig, PostprocessConfig, PostprocessLayer, RegionalLayer,
    TectonicConfig as LayeredTectonicConfig, TectonicLayer,
};
use super::noise::{NoiseConfig, NoiseGenerator};
use super::plate::{
    BoundaryType, PlateBoundary, PlateGenerator, PlateType, TectonicConfig, TectonicPlate,
};
use super::template::{
    get_suggested_ocean_ratio, get_suggested_plate_count, get_template_by_name,
    should_use_layered_generation, TerrainTemplate,
};
use super::template_executor::TemplateExecutor;
use eframe::egui::Pos2;
#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

/// 海平面高度阈值
pub const SEA_LEVEL: u8 = 20;

/// 地形生成模式
#[derive(Debug, Clone)]
pub enum TerrainGenerationMode {
    /// 板块构造模拟（物理模拟）
    TectonicSimulation,
    /// 模板生成（使用预设模板名称）
    Template(String),
    /// 模板生成（使用模板对象和指定种子）
    TemplateWithSeed(TerrainTemplate, u64),
    /// 新分层生成（板块→构造→区域→细节→后处理）
    Layered { seed: u64, num_plates: usize },
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
    /// 是否启用特征清理（移除孤立的小岛和小湖）
    pub enable_feature_cleanup: bool,
    /// 最小岛屿大小（小于此值的岛屿会被淹没）
    pub min_island_size: usize,
    /// 最小湖泊大小（小于此值的湖泊会被填充）
    pub min_lake_size: usize,
    /// 海岸线平滑迭代次数
    pub coastline_smoothing: u32,
    /// 是否使用约束噪声（防止噪声产生散点）
    pub use_constrained_noise: bool,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            mode: TerrainGenerationMode::Template("earth-like".to_string()),
            tectonic: TectonicConfig::default(),
            medium_noise_strength: 0.0, // 暂时关闭噪声测试
            detail_noise_strength: 0.0, // 暂时关闭噪声测试
            continental_noise_mult: 1.5,
            oceanic_noise_mult: 0.5,
            enable_erosion: false,
            erosion_iterations: 50,
            smoothing: 0,
            // 新增：特征清理和海岸线优化
            enable_feature_cleanup: true, // 默认启用
            min_island_size: 15,          // 大幅增加最小岛屿大小
            min_lake_size: 10,            // 大幅增加最小湖泊大小
            coastline_smoothing: 1,
            use_constrained_noise: true, // 默认启用约束噪声
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

    /// 使用模板和指定种子生成
    pub fn with_template_and_seed(template: TerrainTemplate, seed: u64) -> Self {
        Self {
            mode: TerrainGenerationMode::TemplateWithSeed(template, seed),
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

    /// 使用新的分层生成系统
    pub fn with_layered(seed: u64, num_plates: usize) -> Self {
        Self {
            mode: TerrainGenerationMode::Layered { seed, num_plates },
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
            TerrainGenerationMode::TemplateWithSeed(template, seed) => {
                self.generate_from_template_with_seed(cells, neighbors, template.clone(), *seed)
            }
            TerrainGenerationMode::Layered { seed, num_plates } => {
                // Default ocean ratio for direct Layered mode
                self.generate_layered(cells, neighbors, *seed, *num_plates, 0.65)
            }
        }
    }

    /// 使用新的分层系统生成地形
    fn generate_layered(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        seed: u64,
        num_plates: usize,
        ocean_ratio: f32,
    ) -> (Vec<u8>, Vec<TectonicPlate>, Vec<u16>) {
        #[cfg(debug_assertions)]
        println!("使用分层系统生成地形: seed={}, plates={}", seed, num_plates);

        // Continental ratio derived from ocean ratio:
        // more ocean → fewer continental plates
        let continental_ratio = (1.0 - ocean_ratio).clamp(0.2, 0.5);

        // 配置板块层
        let plate_config = PlateConfig {
            num_plates,
            continental_ratio,
            continental_base: 80.0,
            oceanic_base: -50.0,
        };

        // 配置构造层
        let tectonic_config = LayeredTectonicConfig {
            plate_config: plate_config.clone(),
            mountain_height: 100.0,
            mountain_width: 15.0,
            trench_depth: 40.0,
            ridge_height: 25.0,
            rift_depth: 30.0,
        };

        // 配置后处理层
        let postprocess_config = PostprocessConfig {
            min_island_size: self.config.min_island_size,
            min_lake_size: self.config.min_lake_size,
            smoothing_iterations: self.config.coastline_smoothing,
            ocean_ratio,
        };

        // 构建分层生成器
        let generator = LayeredGenerator::new()
            .with_seed(seed)
            .add_layer(TectonicLayer::new(tectonic_config).with_seed(seed))
            .add_layer(RegionalLayer::new().with_seed((seed + 100) as u32))
            .add_layer(DetailLayer::new().with_seed((seed + 200) as u32))
            .add_layer(PostprocessLayer::new(postprocess_config));

        // 生成地形
        let output = generator.generate(cells, neighbors);

        // 转换高度值到 u8 范围
        // 保持海平面在固定位置 (SEA_LEVEL = 20)
        // 海平面 (0.0) 映射到 20，负值映射到 0-20，正值映射到 20-255
        let min_h = output.heights.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_h = output
            .heights
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);

        let heights_u8: Vec<u8> = output
            .heights
            .iter()
            .map(|&h| {
                if h <= 0.0 {
                    // 海洋：映射到 0-20
                    // min_h (最深) -> 0, 0 (海平面) -> 20
                    if min_h >= 0.0 {
                        SEA_LEVEL
                    } else {
                        let t = (h - min_h) / (0.0 - min_h);
                        (t * SEA_LEVEL as f32).clamp(0.0, SEA_LEVEL as f32) as u8
                    }
                } else {
                    // 陆地：映射到 20-255
                    // 0 (海平面) -> 20, max_h (最高) -> 255
                    if max_h <= 0.0 {
                        SEA_LEVEL
                    } else {
                        let t = h / max_h;
                        (SEA_LEVEL as f32 + t * (255.0 - SEA_LEVEL as f32))
                            .clamp(SEA_LEVEL as f32, 255.0) as u8
                    }
                }
            })
            .collect();

        // 提取板块信息
        let plate_ids = output.plate_ids.unwrap_or_else(|| vec![0; cells.len()]);

        // 暂时不返回详细的板块对象
        let plates = Vec::new();

        (heights_u8, plates, plate_ids)
    }

    /// 使用模板生成地形
    fn generate_from_template(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        template_name: &str,
    ) -> (Vec<u8>, Vec<TectonicPlate>, Vec<u16>) {
        // 检查是否应该使用新的分层系统
        if should_use_layered_generation(template_name) {
            let num_plates = get_suggested_plate_count(template_name);
            #[cfg(debug_assertions)]
            println!(
                "模板 '{}' 使用分层系统 (plates={})",
                template_name, num_plates
            );
            let ocean_ratio = get_suggested_ocean_ratio(template_name);
            let (mut heights_u8, plates, plate_ids) = self.generate_layered(
                cells,
                neighbors,
                self.config.tectonic.seed,
                num_plates,
                ocean_ratio,
            );

            // Apply template-specific modifiers as subtle adjustments
            if let Some(template) = get_template_by_name(template_name) {
                self.apply_template_modifiers(&mut heights_u8, &template, cells, neighbors);
            }

            // Post-process
            self.post_process(&mut heights_u8, neighbors);

            return (heights_u8, plates, plate_ids);
        }

        #[cfg(debug_assertions)]
        println!("使用传统模板生成地形: {}", template_name);

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

        // 可选：添加细节噪声（简化版，避免产生太多碎片）
        if self.config.detail_noise_strength > 0.0 {
            // 中等尺度噪声 - 增加地形变化但不产生碎片
            let medium_noise_config = NoiseConfig {
                octaves: 4,
                base_frequency: 0.002, // 低频率，大尺度变化
                persistence: 0.5,
                lacunarity: 2.0,
                seed: (self.config.tectonic.seed + 1) as u32,
            };

            let generator = NoiseGenerator::new(medium_noise_config.seed);
            let strengths = vec![self.config.detail_noise_strength; cells.len()];
            let noise_values =
                generator.generate_constrained_noise(cells, &medium_noise_config, &strengths);

            for (i, &noise) in noise_values.iter().enumerate() {
                heights[i] += noise * 20.0;
            }

            // 细节噪声 - 中等尺度，给陆地添加变化
            let detail_noise_config = NoiseConfig {
                octaves: 3,
                base_frequency: 0.005,
                persistence: 0.4,
                lacunarity: 2.0,
                seed: (self.config.tectonic.seed + 2) as u32,
            };

            let generator2 = NoiseGenerator::new(detail_noise_config.seed);
            let strengths2 = vec![self.config.detail_noise_strength * 0.5; cells.len()];
            let noise_values2 =
                generator2.generate_constrained_noise(cells, &detail_noise_config, &strengths2);

            for (i, &noise) in noise_values2.iter().enumerate() {
                heights[i] += noise * 12.0;
            }
        }

        // 后生成噪声叠加：打破残余的放射状图案
        self.apply_post_generation_noise(&mut heights, cells, self.config.tectonic.seed);

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
        let mut heights_u8: Vec<u8> = heights.iter().map(|&h| h.clamp(0.0, 255.0) as u8).collect();

        // 后处理：特征清理和海岸线优化
        self.post_process(&mut heights_u8, neighbors);

        // 模板模式下不生成板块数据
        let plates = Vec::new();
        let plate_id = vec![0; cells.len()];

        (heights_u8, plates, plate_id)
    }

    /// 使用模板和指定种子生成地形
    fn generate_from_template_with_seed(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        template: TerrainTemplate,
        seed: u64,
    ) -> (Vec<u8>, Vec<TectonicPlate>, Vec<u16>) {
        #[cfg(debug_assertions)]
        println!("使用模板 '{}' 和种子 {} 生成地形", template.name, seed);

        // Use the layered pipeline (same as generate_from_template)
        // This ensures the sekai app and generate_screenshots use identical generation
        if should_use_layered_generation(&template.name) {
            let num_plates = get_suggested_plate_count(&template.name);
            let ocean_ratio = get_suggested_ocean_ratio(&template.name);
            let (mut heights_u8, plates, plate_ids) =
                self.generate_layered(cells, neighbors, seed, num_plates, ocean_ratio);

            self.apply_template_modifiers(&mut heights_u8, &template, cells, neighbors);
            self.post_process(&mut heights_u8, neighbors);

            return (heights_u8, plates, plate_ids);
        }

        // Fallback: legacy template executor
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

        // 使用指定种子执行模板
        let executor = TemplateExecutor::new(width, height, seed);
        let mut heights = executor.execute(&template, cells, neighbors);

        // 可选：添加细节噪声
        if self.config.detail_noise_strength > 0.0 {
            let detail_noise_config = NoiseConfig {
                octaves: 4,
                base_frequency: 0.08,
                persistence: 0.4,
                lacunarity: 2.2,
                seed: (seed + 1) as u32,
            };

            let generator = NoiseGenerator::new(detail_noise_config.seed);
            let strengths = vec![self.config.detail_noise_strength; cells.len()];
            let noise_values =
                generator.generate_constrained_noise(cells, &detail_noise_config, &strengths);

            for (i, &noise) in noise_values.iter().enumerate() {
                heights[i] += noise * 30.0;
            }
        }

        // 后生成噪声叠加：打破残余的放射状图案
        self.apply_post_generation_noise(&mut heights, cells, seed);

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
        let mut heights_u8: Vec<u8> = heights.iter().map(|&h| h.clamp(0.0, 255.0) as u8).collect();

        // 后处理：特征清理和海岸线优化
        self.post_process(&mut heights_u8, neighbors);

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

        // 根据板块类型和到边界距离加入浮力偏移，形成更稳定的海陆双峰分布
        self.apply_plate_buoyancy(&mut heights, &plates, &plate_id, neighbors);

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

        // ====== 阶段 5: 地貌整形与后处理 ======
        if self.config.smoothing > 0 {
            self.smooth_heights(&mut heights, neighbors, self.config.smoothing);
        }

        // 使用分位数控制海陆比例 + 非线性映射，得到更拟真的高程分布
        let mut heights_u8 = self.remap_tectonic_heights(&heights);

        // 后处理：特征清理和海岸线优化
        self.post_process(&mut heights_u8, neighbors);

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
    #[allow(clippy::too_many_arguments)]
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
        #[cfg(not(target_arch = "wasm32"))]
        let iter = (0..heights.len()).into_par_iter();
        #[cfg(target_arch = "wasm32")]
        let iter = 0..heights.len();
        let strengths: Vec<f32> = iter
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

    /// 板块浮力偏移：大陆内部抬升、海洋板块压低，强化海陆双峰结构
    fn apply_plate_buoyancy(
        &self,
        heights: &mut [f32],
        plates: &[TectonicPlate],
        plate_id: &[u16],
        neighbors: &[Vec<u32>],
    ) {
        if heights.is_empty() {
            return;
        }

        for i in 0..heights.len() {
            let pid = plate_id[i];
            if pid == 0 {
                continue;
            }

            let plate = &plates[(pid - 1) as usize];
            let dist = self
                .calculate_boundary_distance(i, plate, neighbors)
                .min(12.0);

            match plate.plate_type {
                PlateType::Continental => {
                    // 大陆内部更厚、更高；边界保留一定起伏给造山带
                    heights[i] += 6.0 + dist * 1.1;
                }
                PlateType::Oceanic => {
                    // 海洋板块总体更低，内部更深，形成深海平原
                    heights[i] -= 10.0 + dist * 0.9;
                }
            }
        }
    }

    /// 将板块模拟的原始高度重映射到 0..255，控制海陆比例并塑造拟真高程分布
    fn remap_tectonic_heights(&self, heights: &[f32]) -> Vec<u8> {
        if heights.is_empty() {
            return Vec::new();
        }

        let mut sorted = heights.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let target_ocean_ratio =
            (0.85 - self.config.tectonic.continental_ratio * 0.55).clamp(0.45, 0.80);
        let idx = ((sorted.len() as f32) * target_ocean_ratio) as usize;
        let idx = idx.min(sorted.len() - 1);
        let sea_threshold = sorted[idx];

        let min_h = sorted[0];
        let max_h = sorted[sorted.len() - 1];
        let sea = SEA_LEVEL as f32;

        heights
            .iter()
            .map(|&h| {
                if h <= sea_threshold {
                    // 海洋：加深深海，保留大陆架浅海
                    let t = if (sea_threshold - min_h).abs() < 0.0001 {
                        0.5
                    } else {
                        ((h - min_h) / (sea_threshold - min_h)).clamp(0.0, 1.0)
                    };
                    (t.powf(1.55) * sea).clamp(0.0, sea) as u8
                } else {
                    // 陆地：压缩低地、拉开高山区间，突出造山带
                    let t = if (max_h - sea_threshold).abs() < 0.0001 {
                        0.5
                    } else {
                        ((h - sea_threshold) / (max_h - sea_threshold)).clamp(0.0, 1.0)
                    };
                    let land = sea + t.powf(0.82) * (255.0 - sea);
                    land.clamp(sea, 255.0) as u8
                }
            })
            .collect()
    }

    /// Apply template commands as subtle modifiers on top of plate-driven terrain
    ///
    /// Only Range and Strait commands are applied (as mountain chains and water channels).
    /// Hill/Mountain commands are skipped since the plate system already handles landmasses.
    /// The modifier strength is reduced to 30% to keep plate structure dominant.
    fn apply_template_modifiers(
        &self,
        heights: &mut [u8],
        template: &TerrainTemplate,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) {
        use super::template::TerrainCommand;

        // Compute map bounds
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
        let w = max_x - min_x;
        let h = max_y - min_y;

        let modifier_strength = 0.3; // Only 30% of template effect

        for cmd in &template.commands {
            match cmd {
                TerrainCommand::Range {
                    count: _,
                    height,
                    x,
                    y,
                    length: _,
                    width: _,
                    angle: _,
                } => {
                    // Add subtle mountain ridges in the specified area
                    // This helps templates like Mediterranean get their characteristic features
                    let (hmin, hmax) = *height;
                    let boost = (hmin + hmax) / 2.0 * modifier_strength;
                    let (xmin, xmax) = *x;
                    let (ymin, ymax) = *y;

                    for (i, pos) in cells.iter().enumerate() {
                        let nx = (pos.x - min_x) / w;
                        let ny = (pos.y - min_y) / h;
                        if nx >= xmin && nx <= xmax && ny >= ymin && ny <= ymax {
                            let new_val = (heights[i] as f32 + boost * 0.3).clamp(0.0, 255.0);
                            heights[i] = new_val as u8;
                        }
                    }
                }
                TerrainCommand::Strait {
                    width: sw,
                    direction,
                    position,
                    depth,
                } => {
                    // Carve strait through terrain
                    use super::template::StraitDirection;
                    let carve = *depth * modifier_strength;

                    for (i, pos) in cells.iter().enumerate() {
                        let nx = (pos.x - min_x) / w;
                        let ny = (pos.y - min_y) / h;
                        let in_strait = match direction {
                            StraitDirection::Vertical => (nx - position).abs() < *sw / 2.0,
                            StraitDirection::Horizontal => (ny - position).abs() < *sw / 2.0,
                        };
                        if in_strait {
                            let new_val = (heights[i] as f32 - carve).clamp(0.0, 255.0);
                            heights[i] = new_val as u8;
                        }
                    }
                }
                TerrainCommand::Trough {
                    count: _,
                    depth,
                    x,
                    y,
                    length: _,
                    width: _,
                    angle: _,
                } => {
                    // Subtle deepening in specified areas
                    let (dmin, dmax) = *depth;
                    let carve = (dmin + dmax) / 2.0 * modifier_strength * 0.2;
                    let (xmin, xmax) = *x;
                    let (ymin, ymax) = *y;

                    for (i, pos) in cells.iter().enumerate() {
                        let nx = (pos.x - min_x) / w;
                        let ny = (pos.y - min_y) / h;
                        if nx >= xmin && nx <= xmax && ny >= ymin && ny <= ymax {
                            let new_val = (heights[i] as f32 - carve).clamp(0.0, 255.0);
                            heights[i] = new_val as u8;
                        }
                    }
                }
                _ => {
                    // Skip Hill, Mountain, Pit, etc. - plate system handles these
                }
            }
        }

        let _ = neighbors; // suppress unused warning
    }

    /// 后生成噪声叠加：在模板生成之后叠加多频段噪声，打破放射状图案
    ///
    /// 使用两层噪声：
    /// - 低频层：大尺度形变，使整体地形不对称
    /// - 中频层：中等尺度扰动，打破局部的圆形等高线
    fn apply_post_generation_noise(&self, heights: &mut [f32], cells: &[Pos2], seed: u64) {
        if heights.is_empty() {
            return;
        }

        // 计算当前高度范围，用于按比例叠加噪声
        let max_h = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let amplitude = max_h.abs().max(50.0); // 至少有一些影响

        // 低频噪声：大尺度形变
        let low_freq_config = NoiseConfig {
            octaves: 2,
            base_frequency: 0.003,
            persistence: 0.6,
            lacunarity: 2.0,
            seed: (seed + 42) as u32,
        };
        let gen_low = NoiseGenerator::new(low_freq_config.seed);

        // 中频噪声：打破局部圆形
        let mid_freq_config = NoiseConfig {
            octaves: 3,
            base_frequency: 0.008,
            persistence: 0.45,
            lacunarity: 2.2,
            seed: (seed + 99) as u32,
        };
        let gen_mid = NoiseGenerator::new(mid_freq_config.seed);

        for (i, pos) in cells.iter().enumerate() {
            let low_noise = gen_low.fbm(pos.x as f64, pos.y as f64, &low_freq_config) as f32;
            let mid_noise = gen_mid.fbm(pos.x as f64, pos.y as f64, &mid_freq_config) as f32;

            // Scale noise relative to terrain amplitude
            // Low freq: ~8% of amplitude, mid freq: ~5%
            heights[i] += low_noise * amplitude * 0.08 + mid_noise * amplitude * 0.05;
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

    /// 后处理：特征清理和海岸线优化
    ///
    /// 使用 Azgaar 风格的算法清理孤立的小岛和小湖，
    /// 并平滑海岸线以消除噪点。
    fn post_process(&self, heights: &mut [u8], neighbors: &[Vec<u32>]) {
        use super::features::FeatureDetector;

        // 计算边界单元格（简化版：假设边缘索引的单元格是边界）
        // 实际应用中，这应该从 Voronoi 网格获取
        let n = heights.len();
        let border_cells: Vec<bool> = (0..n)
            .map(|i| {
                // 如果一个单元格的邻居数量少于平均值，可能是边界
                neighbors[i].len() < 4
            })
            .collect();

        let detector = FeatureDetector::new(self.config.min_island_size, self.config.min_lake_size);

        // 1. 检测所有连通区域（特征）
        let (features, _feature_ids) = detector.detect_features(heights, neighbors, &border_cells);

        // 2. 清理太小的特征
        if self.config.enable_feature_cleanup {
            let _cleaned = detector.cleanup_small_features(heights, &features);
            #[cfg(debug_assertions)]
            if _cleaned > 0 {
                println!("清理了 {} 个孤立单元格", _cleaned);
            }
        }

        // 3. 平滑海岸线
        if self.config.coastline_smoothing > 0 {
            let _smoothed =
                detector.smooth_coastline(heights, neighbors, self.config.coastline_smoothing);
            #[cfg(debug_assertions)]
            if _smoothed > 0 {
                println!("平滑了 {} 个海岸线单元格", _smoothed);
            }
        }
    }
}
