// 板块构造模拟

use eframe::egui::Pos2;
use noise::{NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use std::collections::{HashMap, VecDeque};

/// 板块类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlateType {
    /// 大陆板块（密度 2.7 g/cm³）
    Continental,
    /// 海洋板块（密度 3.0 g/cm³）
    Oceanic,
}

impl PlateType {
    pub fn density(self) -> f32 {
        match self {
            PlateType::Continental => 2.7,
            PlateType::Oceanic => 3.0,
        }
    }

    pub fn base_height(self) -> f32 {
        match self {
            PlateType::Continental => 128.0, // 大陆基准高度
            PlateType::Oceanic => 64.0,      // 海洋基准高度
        }
    }
}

/// 边界类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundaryType {
    /// 汇聚边界（碰撞）
    Convergent {
        intensity: f32,
        subducting_plate: Option<u16>,
    },
    /// 分离边界（张裂）
    Divergent { intensity: f32 },
    /// 转换边界（错动）
    Transform { intensity: f32 },
}

/// 板块边界信息
#[derive(Debug, Clone)]
pub struct PlateBoundary {
    pub plate_a: u16,
    pub plate_b: u16,
    pub boundary_type: BoundaryType,
    pub cells: Vec<u32>, // 边界单元格
}

/// 板块
#[derive(Debug, Clone)]
pub struct TectonicPlate {
    pub id: u16,
    pub plate_type: PlateType,
    pub direction: f32,           // 运动方向（弧度）
    pub speed: f32,               // 运动速度
    pub cells: Vec<u32>,          // 板块包含的单元格
    pub boundary_cells: Vec<u32>, // 边界单元格
    pub centroid: Pos2,           // 质心
    pub density: f32,             // 密度
}

impl TectonicPlate {
    pub fn new(id: u16, plate_type: PlateType) -> Self {
        Self {
            id,
            plate_type,
            direction: 0.0,
            speed: 1.0,
            cells: Vec::new(),
            boundary_cells: Vec::new(),
            centroid: Pos2::ZERO,
            density: plate_type.density(),
        }
    }

    /// 计算板块的质心
    pub fn calculate_centroid(&mut self, cell_positions: &[Pos2]) {
        if self.cells.is_empty() {
            return;
        }

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;

        for &cell_idx in &self.cells {
            let pos = cell_positions[cell_idx as usize];
            sum_x += pos.x;
            sum_y += pos.y;
        }

        let count = self.cells.len() as f32;
        self.centroid = Pos2::new(sum_x / count, sum_y / count);
    }

    /// 获取运动向量
    pub fn velocity_vector(&self) -> Pos2 {
        let dx = self.direction.cos() * self.speed;
        let dy = self.direction.sin() * self.speed;
        Pos2::new(dx, dy)
    }
}

/// 板块构造配置
#[derive(Debug, Clone)]
pub struct TectonicConfig {
    /// 板块数量
    pub plate_count: u32,
    /// 大陆板块比例 (0.0 - 1.0)
    pub continental_ratio: f32,
    /// 模拟迭代次数
    pub iterations: u32,
    /// 碰撞隆起速率
    pub collision_uplift_rate: f32,
    /// 俯冲下沉速率
    pub subduction_depth_rate: f32,
    /// 裂谷下沉速率
    pub rift_depth_rate: f32,
    /// 边界影响宽度（单元格数）
    pub boundary_width: u32,
    /// 地壳均衡调整速率
    pub isostasy_rate: f32,
    /// 随机种子
    pub seed: u64,
}

impl Default for TectonicConfig {
    fn default() -> Self {
        Self {
            plate_count: 12,
            continental_ratio: 0.4,
            iterations: 100,
            collision_uplift_rate: 0.5,
            subduction_depth_rate: 0.3,
            rift_depth_rate: 0.2,
            boundary_width: 5,
            isostasy_rate: 0.05,
            seed: 0,
        }
    }
}

impl TectonicConfig {
    /// 类地球配置
    pub fn earth_like() -> Self {
        Self {
            plate_count: 15,
            continental_ratio: 0.3,
            iterations: 200,
            collision_uplift_rate: 0.6,
            subduction_depth_rate: 0.4,
            ..Default::default()
        }
    }

    /// 多山地配置
    pub fn mountainous() -> Self {
        Self {
            plate_count: 20,
            continental_ratio: 0.5,
            iterations: 300,
            collision_uplift_rate: 0.8,
            ..Default::default()
        }
    }

    /// 群岛配置
    pub fn archipelago() -> Self {
        Self {
            plate_count: 25,
            continental_ratio: 0.2,
            iterations: 150,
            rift_depth_rate: 0.3,
            ..Default::default()
        }
    }

    /// 超级大陆配置
    pub fn supercontinent() -> Self {
        Self {
            plate_count: 8,
            continental_ratio: 0.6,
            iterations: 250,
            collision_uplift_rate: 0.7,
            ..Default::default()
        }
    }
}

/// 板块生成器
pub struct PlateGenerator {
    config: TectonicConfig,
}

impl PlateGenerator {
    pub fn new(config: TectonicConfig) -> Self {
        Self { config }
    }

    /// 生成板块
    pub fn generate_plates(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> (Vec<TectonicPlate>, Vec<u16>) {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.config.seed);
        let n = cells.len();

        // 1. 随机选择板块种子点
        let seed_indices: Vec<usize> = (0..n).collect::<Vec<_>>().into_iter().collect::<Vec<_>>();
        let seed_indices = {
            let mut indices = seed_indices;
            use rand::seq::SliceRandom;
            indices.shuffle(&mut rng);
            indices
                .into_iter()
                .take(self.config.plate_count as usize)
                .collect::<Vec<_>>()
        };

        // 2. 创建板块对象
        let mut plates = Vec::new();
        let continental_count =
            (self.config.plate_count as f32 * self.config.continental_ratio) as usize;

        for (i, &_seed_idx) in seed_indices.iter().enumerate() {
            let plate_type = if i < continental_count {
                PlateType::Continental
            } else {
                PlateType::Oceanic
            };

            let mut plate = TectonicPlate::new((i + 1) as u16, plate_type);

            // 分配随机运动方向和速度
            plate.direction = rng.random_range(0.0..std::f32::consts::TAU);
            plate.speed = rng.random_range(0.5..1.5);

            plates.push(plate);
        }

        // 3. 使用加权 BFS 扩张分配单元格到板块
        //    每个板块有随机的偏好方向和不同的生长速率
        let mut plate_id = vec![0u16; n];

        // 为每个板块生成偏好方向和生长速率
        let plate_bias_angles: Vec<f32> = (0..plates.len())
            .map(|_| rng.random_range(0.0..std::f32::consts::TAU))
            .collect();
        let plate_growth_rates: Vec<f32> = (0..plates.len())
            .map(|_| rng.random_range(0.7..1.3))
            .collect();

        // 使用优先级队列模拟：BFS with variable growth
        // 每个条目: (priority, cell_idx, plate_id)
        // 较低的 priority 先扩展
        let mut queue: VecDeque<(f32, usize, u16)> = VecDeque::new();

        // 初始化种子点
        for (i, &seed_idx) in seed_indices.iter().enumerate() {
            plate_id[seed_idx] = (i + 1) as u16;
            queue.push_back((0.0, seed_idx, (i + 1) as u16));
        }

        // 加权 BFS 扩张
        while let Some((priority, cell, pid)) = queue.pop_front() {
            let plate_idx = (pid - 1) as usize;
            let seed_pos = cells[seed_indices[plate_idx]];
            let bias_angle = plate_bias_angles[plate_idx];
            let growth_rate = plate_growth_rates[plate_idx];
            let bias_dx = bias_angle.cos();
            let bias_dy = bias_angle.sin();

            for &neighbor_idx in &neighbors[cell] {
                let neighbor_idx = neighbor_idx as usize;
                if plate_id[neighbor_idx] == 0 {
                    plate_id[neighbor_idx] = pid;

                    // 计算方向偏置：沿偏好方向扩展更快（更低的 priority）
                    let dx = cells[neighbor_idx].x - seed_pos.x;
                    let dy = cells[neighbor_idx].y - seed_pos.y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let alignment = if dist > 0.001 {
                        (dx * bias_dx + dy * bias_dy) / dist
                    } else {
                        0.0
                    };

                    // 偏好方向上 priority 更低（扩展更快）
                    let dir_weight = 1.0 - alignment * 0.3;
                    let next_priority =
                        priority + dir_weight / growth_rate + rng.random::<f32>() * 0.2;

                    // 插入到合适位置（简单排序插入）
                    let insert_pos = queue
                        .iter()
                        .position(|(p, _, _)| *p > next_priority)
                        .unwrap_or(queue.len());
                    queue.insert(insert_pos, (next_priority, neighbor_idx, pid));
                }
            }
        }

        // 4. 噪声扰动边界：随机重新分配部分边界单元格
        let boundary_perlin = Perlin::new(self.config.seed as u32);
        let noise_freq = 0.01;
        for cell_idx in 0..n {
            let pid = plate_id[cell_idx];
            if pid == 0 {
                continue;
            }
            // 检查是否为边界
            let is_boundary = neighbors[cell_idx]
                .iter()
                .any(|&nb| plate_id[nb as usize] != pid && plate_id[nb as usize] != 0);

            if is_boundary {
                let noise_val = boundary_perlin.get([
                    cells[cell_idx].x as f64 * noise_freq,
                    cells[cell_idx].y as f64 * noise_freq,
                ]);
                // ~15% chance to reassign based on noise
                if noise_val > 0.4 {
                    // Find a neighboring plate to reassign to
                    for &nb in &neighbors[cell_idx] {
                        let nb_pid = plate_id[nb as usize];
                        if nb_pid != 0 && nb_pid != pid {
                            plate_id[cell_idx] = nb_pid;
                            break;
                        }
                    }
                }
            }
        }

        // 4. 填充板块的单元格列表
        for (cell_idx, &pid) in plate_id.iter().enumerate() {
            if pid > 0 {
                plates[(pid - 1) as usize].cells.push(cell_idx as u32);
            }
        }

        // 5. 计算板块质心
        for plate in plates.iter_mut() {
            plate.calculate_centroid(cells);
        }

        // 6. 识别边界单元格
        for (cell_idx, &pid) in plate_id.iter().enumerate() {
            if pid == 0 {
                continue;
            }

            // 检查是否为边界（有相邻不同板块的单元格）
            let is_boundary = neighbors[cell_idx]
                .iter()
                .any(|&n| plate_id[n as usize] != pid);

            if is_boundary {
                plates[(pid - 1) as usize]
                    .boundary_cells
                    .push(cell_idx as u32);
            }
        }

        (plates, plate_id)
    }

    /// 分析板块边界类型
    pub fn analyze_boundaries(
        &self,
        plates: &[TectonicPlate],
        plate_id: &[u16],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
    ) -> Vec<PlateBoundary> {
        let mut boundaries = Vec::new();
        let mut processed_pairs: HashMap<(u16, u16), ()> = HashMap::new();

        // 遍历所有边界单元格
        for plate in plates {
            for &cell_idx in &plate.boundary_cells {
                let cell_idx = cell_idx as usize;
                let pid_a = plate_id[cell_idx];

                for &neighbor_idx in &neighbors[cell_idx] {
                    let neighbor_idx = neighbor_idx as usize;
                    let pid_b = plate_id[neighbor_idx];

                    if pid_b == 0 || pid_b == pid_a {
                        continue;
                    }

                    // 确保只处理一次每对板块
                    let pair = if pid_a < pid_b {
                        (pid_a, pid_b)
                    } else {
                        (pid_b, pid_a)
                    };

                    if processed_pairs.contains_key(&pair) {
                        continue;
                    }
                    processed_pairs.insert(pair, ());

                    // 收集边界单元格
                    let mut boundary_cells = Vec::new();
                    for (i, &pid) in plate_id.iter().enumerate() {
                        if pid == pid_a {
                            let has_neighbor_b =
                                neighbors[i].iter().any(|&n| plate_id[n as usize] == pid_b);
                            if has_neighbor_b {
                                boundary_cells.push(i as u32);
                            }
                        }
                    }

                    // 分析边界类型
                    let plate_a = &plates[(pid_a - 1) as usize];
                    let plate_b = &plates[(pid_b - 1) as usize];
                    let boundary_type =
                        self.classify_boundary(plate_a, plate_b, &boundary_cells, cells);

                    boundaries.push(PlateBoundary {
                        plate_a: pid_a,
                        plate_b: pid_b,
                        boundary_type,
                        cells: boundary_cells,
                    });
                }
            }
        }

        boundaries
    }

    /// 判断边界类型
    fn classify_boundary(
        &self,
        plate_a: &TectonicPlate,
        plate_b: &TectonicPlate,
        _boundary_cells: &[u32],
        _cells: &[Pos2],
    ) -> BoundaryType {
        // 运动向量
        let vel_a = plate_a.velocity_vector();
        let vel_b = plate_b.velocity_vector();

        // 边界法向量（从 A 指向 B）
        let dx = plate_b.centroid.x - plate_a.centroid.x;
        let dy = plate_b.centroid.y - plate_a.centroid.y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 0.001 {
            return BoundaryType::Transform { intensity: 0.0 };
        }
        let normal_x = dx / dist;
        let normal_y = dy / dist;

        // 相对运动在法向的投影
        let approach_a = vel_a.x * normal_x + vel_a.y * normal_y;
        let approach_b = vel_b.x * (-normal_x) + vel_b.y * (-normal_y);
        let relative_approach = approach_a + approach_b;

        // 判断边界类型
        let threshold = 0.3;
        if relative_approach > threshold {
            // 汇聚边界
            let subducting = if plate_a.density > plate_b.density {
                Some(plate_a.id)
            } else if plate_b.density > plate_a.density {
                Some(plate_b.id)
            } else {
                None
            };

            BoundaryType::Convergent {
                intensity: relative_approach,
                subducting_plate: subducting,
            }
        } else if relative_approach < -threshold {
            // 分离边界
            BoundaryType::Divergent {
                intensity: -relative_approach,
            }
        } else {
            // 转换边界
            BoundaryType::Transform {
                intensity: relative_approach.abs(),
            }
        }
    }
}
