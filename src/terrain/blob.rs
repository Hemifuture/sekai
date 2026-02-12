// BFS 扩散式地形生成
//
// 参考 Azgaar Fantasy Map Generator 的算法
// 使用 BFS 从中心向外传播高度值，创造自然不规则的地形

use eframe::egui::Pos2;
use noise::{NoiseFn, Perlin};
use rand::Rng;
use std::collections::VecDeque;

/// Bounds info: (points, (min_x, max_x, min_y, max_y), (width, height))
type BoundsInfo<'a> = (&'a [Pos2], (f32, f32, f32, f32), (f32, f32));

/// Blob 生成器配置
#[derive(Debug, Clone)]
pub struct BlobConfig {
    /// 衰减因子 (0.93 ~ 0.997)
    /// 值越高，blob 越大
    pub blob_power: f32,
    /// 线性衰减因子（用于山脉）
    pub line_power: f32,
    /// 随机扰动范围 (例如 0.45 表示 0.55 ~ 1.45)
    pub jitter: f32,
    /// 噪声权重强度 (0.0 = 无噪声, 1.0 = 强噪声影响)
    /// 使用 Perlin 噪声场来偏置 BFS 传播方向
    pub noise_weight: f32,
    /// 噪声频率（控制噪声场的尺度）
    pub noise_frequency: f64,
    /// 方向偏置强度 (0.0 = 无偏置, 1.0 = 强烈拉伸)
    /// 给每个 blob 随机方向，使其沿该方向拉伸
    pub directional_bias: f32,
    /// 概率跳过率 (0.0 ~ 0.15)
    /// BFS 中随机跳过邻居的概率，创造凹陷和不规则边缘
    pub skip_probability: f32,
}

impl Default for BlobConfig {
    fn default() -> Self {
        Self {
            blob_power: 0.97,
            line_power: 0.82,
            jitter: 0.45,
            noise_weight: 0.3,
            noise_frequency: 0.02,
            directional_bias: 0.25,
            skip_probability: 0.07,
        }
    }
}

impl BlobConfig {
    /// 根据单元格数量计算最佳 blob_power
    /// 参考 Azgaar 的映射表
    pub fn from_cell_count(cells: usize) -> Self {
        let blob_power = match cells {
            0..=1000 => 0.93,
            1001..=2000 => 0.95,
            2001..=5000 => 0.97,
            5001..=10000 => 0.98,
            10001..=20000 => 0.99,
            20001..=30000 => 0.991,
            30001..=40000 => 0.993,
            40001..=50000 => 0.994,
            50001..=60000 => 0.995,
            60001..=70000 => 0.9955,
            70001..=80000 => 0.996,
            80001..=90000 => 0.9964,
            _ => 0.9973,
        };

        let line_power = match cells {
            0..=1000 => 0.75,
            1001..=2000 => 0.77,
            2001..=5000 => 0.79,
            5001..=10000 => 0.81,
            10001..=20000 => 0.82,
            20001..=30000 => 0.83,
            30001..=40000 => 0.84,
            40001..=50000 => 0.86,
            50001..=60000 => 0.87,
            60001..=70000 => 0.88,
            70001..=80000 => 0.91,
            80001..=90000 => 0.92,
            _ => 0.93,
        };

        Self {
            blob_power,
            line_power,
            jitter: 0.45,
            noise_weight: 0.3,
            noise_frequency: 0.02,
            directional_bias: 0.25,
            skip_probability: 0.07,
        }
    }
}

/// BFS 扩散式地形生成器
pub struct BlobGenerator {
    config: BlobConfig,
}

impl BlobGenerator {
    pub fn new(config: BlobConfig) -> Self {
        Self { config }
    }

    /// 从单元格数量自动配置
    pub fn from_cell_count(cells: usize) -> Self {
        Self::new(BlobConfig::from_cell_count(cells))
    }

    /// 添加 BFS 扩散式丘陵
    ///
    /// 从指定中心开始，通过 BFS 向邻居传播高度值。
    /// 每次传播时高度按指数衰减，并加入随机扰动。
    pub fn add_hill(
        &self,
        heights: &mut [f32],
        neighbors: &[Vec<u32>],
        start_idx: usize,
        height: f32,
        rng: &mut impl Rng,
    ) {
        self.add_hill_internal(heights, neighbors, start_idx, height, None, None, rng);
    }

    /// 添加带边界限制的 BFS 扩散式丘陵
    ///
    /// 扩散不会超出指定的边界范围，用于生成相互独立的大陆
    #[allow(clippy::too_many_arguments)]
    pub fn add_hill_bounded(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        start_idx: usize,
        height: f32,
        bounds: (f32, f32, f32, f32), // (min_x, max_x, min_y, max_y) 归一化坐标
        map_size: (f32, f32),         // (width, height)
        rng: &mut impl Rng,
    ) {
        self.add_hill_internal(
            heights,
            neighbors,
            start_idx,
            height,
            Some((cells, bounds, map_size)),
            None,
            rng,
        );
    }

    /// 内部实现：BFS 扩散式丘陵
    ///
    /// 使用噪声权重、方向偏置和概率跳过来打破圆形对称性
    #[allow(clippy::too_many_arguments)]
    fn add_hill_internal(
        &self,
        heights: &mut [f32],
        neighbors: &[Vec<u32>],
        start_idx: usize,
        height: f32,
        bounds_info: Option<BoundsInfo<'_>>,
        _extra: Option<()>, // 预留扩展
        rng: &mut impl Rng,
    ) {
        if start_idx >= heights.len() {
            return;
        }

        // 初始化噪声场用于权重 BFS 传播
        let noise_seed = rng.random::<u32>();
        let perlin = Perlin::new(noise_seed);

        // 随机方向偏置：blob 沿此方向拉伸
        let bias_angle: f32 = rng.random_range(0.0..std::f32::consts::TAU);
        let bias_dx = bias_angle.cos();
        let bias_dy = bias_angle.sin();

        // 获取起始点坐标（用于方向偏置计算）
        let start_pos = bounds_info.map(|(cells, _, _)| {
            if start_idx < cells.len() {
                cells[start_idx]
            } else {
                Pos2::ZERO
            }
        });

        let mut change = vec![0.0f32; heights.len()];
        change[start_idx] = height;

        let mut queue = VecDeque::new();
        queue.push_back(start_idx);

        while let Some(current) = queue.pop_front() {
            if current >= neighbors.len() {
                continue;
            }
            for &neighbor in &neighbors[current] {
                let n = neighbor as usize;
                if n >= change.len() || change[n] > 0.0 {
                    continue;
                }

                // 概率跳过：随机跳过邻居以创造凹陷
                if self.config.skip_probability > 0.0
                    && rng.random::<f32>() < self.config.skip_probability
                {
                    continue;
                }

                // 边界检查：如果有边界限制，检查邻居是否在边界内
                if let Some((cells, bounds, map_size)) = bounds_info {
                    if n < cells.len() {
                        let cell = &cells[n];
                        let norm_x = cell.x / map_size.0;
                        let norm_y = cell.y / map_size.1;
                        if norm_x < bounds.0
                            || norm_x > bounds.1
                            || norm_y < bounds.2
                            || norm_y > bounds.3
                        {
                            continue;
                        }
                    }
                }

                // 核心算法：指数衰减 + 随机扰动
                let jitter =
                    1.0 - self.config.jitter + rng.random::<f32>() * self.config.jitter * 2.0;

                // 噪声权重：使用 Perlin 噪声偏置传播
                let noise_mult = if self.config.noise_weight > 0.0 {
                    if let Some((cells, _, _)) = bounds_info {
                        if n < cells.len() {
                            let pos = cells[n];
                            let noise_val = perlin.get([
                                pos.x as f64 * self.config.noise_frequency,
                                pos.y as f64 * self.config.noise_frequency,
                            ]) as f32;
                            // Map noise [-1,1] to a multiplier centered at 1.0
                            1.0 + noise_val * self.config.noise_weight
                        } else {
                            1.0
                        }
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };

                // 方向偏置：沿偏好方向传播更强
                let dir_mult = if self.config.directional_bias > 0.0 {
                    if let Some((cells, _, _)) = bounds_info {
                        if let Some(sp) = start_pos {
                            if n < cells.len() {
                                let pos = cells[n];
                                let dx = pos.x - sp.x;
                                let dy = pos.y - sp.y;
                                let dist = (dx * dx + dy * dy).sqrt();
                                if dist > 0.001 {
                                    // dot product with bias direction, normalized
                                    let alignment = (dx * bias_dx + dy * bias_dy) / dist;
                                    // alignment in [-1, 1], scale to multiplier
                                    1.0 + alignment * self.config.directional_bias
                                } else {
                                    1.0
                                }
                            } else {
                                1.0
                            }
                        } else {
                            1.0
                        }
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };

                change[n] =
                    change[current].powf(self.config.blob_power) * jitter * noise_mult * dir_mult;

                // 只有足够高的值才继续传播
                if change[n] > 1.0 {
                    queue.push_back(n);
                }
            }
        }

        // 应用变化
        for (i, c) in change.iter().enumerate() {
            heights[i] += c;
        }
    }

    /// 添加 BFS 扩散式坑洞（与丘陵相反）
    pub fn add_pit(
        &self,
        heights: &mut [f32],
        neighbors: &[Vec<u32>],
        start_idx: usize,
        depth: f32,
        rng: &mut impl Rng,
    ) {
        if start_idx >= heights.len() {
            return;
        }

        let mut used = vec![false; heights.len()];
        used[start_idx] = true;

        let mut queue = VecDeque::new();
        queue.push_back((start_idx, depth));

        while let Some((current, h)) = queue.pop_front() {
            if current >= heights.len() {
                continue;
            }
            // 应用高度变化
            let jitter = 1.0 - self.config.jitter + rng.random::<f32>() * self.config.jitter * 2.0;
            heights[current] -= h * jitter;

            // 计算下一层的高度
            let next_h = h.powf(self.config.blob_power);
            if next_h < 1.0 {
                continue;
            }

            if current >= neighbors.len() {
                continue;
            }
            for &neighbor in &neighbors[current] {
                let n = neighbor as usize;
                if n >= used.len() || used[n] {
                    continue;
                }
                // 概率跳过
                if self.config.skip_probability > 0.0
                    && rng.random::<f32>() < self.config.skip_probability
                {
                    continue;
                }
                used[n] = true;
                queue.push_back((n, next_h));
            }
        }
    }

    /// 添加 BFS 扩散式山脉
    ///
    /// 首先找到从起点到终点的路径，然后从路径向两侧扩散。
    #[allow(clippy::too_many_arguments)]
    pub fn add_range(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        start_idx: usize,
        end_idx: usize,
        height: f32,
        rng: &mut impl Rng,
    ) {
        if start_idx >= heights.len() || end_idx >= heights.len() {
            return;
        }

        // 1. 找到从起点到终点的路径
        let range = self.find_path(cells, neighbors, start_idx, end_idx, rng);
        if range.is_empty() {
            return;
        }

        // 2. 从路径向两侧扩散
        let mut used = vec![false; heights.len()];
        for &idx in &range {
            used[idx] = true;
        }

        let mut queue: Vec<usize> = range.clone();
        let mut h = height;
        let mut iteration = 0;

        while !queue.is_empty() {
            let frontier = queue.clone();
            queue.clear();

            // 对当前层的所有单元格应用高度
            for &idx in &frontier {
                let jitter = 0.85 + rng.random::<f32>() * 0.3;
                heights[idx] += h * jitter;
            }

            // 高度衰减
            h = h.powf(self.config.line_power) - 1.0;
            if h < 2.0 {
                break;
            }

            // 扩展到邻居
            for &f in &frontier {
                if f >= neighbors.len() {
                    continue;
                }
                for &neighbor in &neighbors[f] {
                    let n = neighbor as usize;
                    if n >= used.len() {
                        continue;
                    }
                    if !used[n] {
                        queue.push(n);
                        used[n] = true;
                    }
                }
            }

            iteration += 1;
            if iteration > 20 {
                break; // 防止无限循环
            }
        }

        // 3. 生成山脊突出点（每隔几个点向下延伸）
        for (d, &cur) in range.iter().enumerate() {
            if d % 6 != 0 {
                continue;
            }

            let mut current = cur;
            for _ in 0..iteration {
                if current >= neighbors.len() {
                    break;
                }
                // 找到高度最低的邻居
                if let Some(&min_neighbor) = neighbors[current]
                    .iter()
                    .filter(|&&n| (n as usize) < heights.len())
                    .min_by(|&&a, &&b| {
                        heights[a as usize]
                            .partial_cmp(&heights[b as usize])
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                {
                    let min_idx = min_neighbor as usize;
                    if min_idx >= heights.len() {
                        break;
                    }
                    // 平滑过渡
                    heights[min_idx] = (heights[current] * 2.0 + heights[min_idx]) / 3.0;
                    current = min_idx;
                }
            }
        }
    }

    /// 添加 BFS 扩散式海沟（与山脉相反）
    #[allow(clippy::too_many_arguments)]
    pub fn add_trough(
        &self,
        heights: &mut [f32],
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        start_idx: usize,
        end_idx: usize,
        depth: f32,
        rng: &mut impl Rng,
    ) {
        if start_idx >= heights.len() || end_idx >= heights.len() {
            return;
        }

        // 找到路径
        let range = self.find_path(cells, neighbors, start_idx, end_idx, rng);
        if range.is_empty() {
            return;
        }

        // 从路径向两侧扩散（降低高度）
        let mut used = vec![false; heights.len()];
        for &idx in &range {
            used[idx] = true;
        }

        let mut queue: Vec<usize> = range.clone();
        let mut d = depth;

        while !queue.is_empty() {
            let frontier = queue.clone();
            queue.clear();

            for &idx in &frontier {
                let jitter = 0.85 + rng.random::<f32>() * 0.3;
                heights[idx] -= d * jitter;
            }

            d = d.powf(self.config.line_power) - 1.0;
            if d < 2.0 {
                break;
            }

            for &f in &frontier {
                if f >= neighbors.len() {
                    continue;
                }
                for &neighbor in &neighbors[f] {
                    let n = neighbor as usize;
                    if n >= used.len() {
                        continue;
                    }
                    if !used[n] {
                        queue.push(n);
                        used[n] = true;
                    }
                }
            }
        }
    }

    /// 找到从起点到终点的路径
    ///
    /// 使用贪心算法，每一步选择距离终点最近的邻居。
    /// 加入随机性使路径不完全直线。
    fn find_path(
        &self,
        cells: &[Pos2],
        neighbors: &[Vec<u32>],
        start: usize,
        end: usize,
        rng: &mut impl Rng,
    ) -> Vec<usize> {
        let mut path = vec![start];
        let mut used = vec![false; cells.len()];
        used[start] = true;

        let mut current = start;
        let end_pos = cells[end];

        while current != end {
            let mut best_neighbor = None;
            let mut best_dist = f32::INFINITY;

            if current >= neighbors.len() {
                break;
            }
            for &neighbor in &neighbors[current] {
                let n = neighbor as usize;
                if n >= used.len() || used[n] {
                    continue;
                }
                if n >= cells.len() {
                    continue;
                }

                let pos = cells[n];
                let mut dist = (pos.x - end_pos.x).powi(2) + (pos.y - end_pos.y).powi(2);

                // 15% 的概率将距离减半（增加路径随机性）
                if rng.random::<f32>() > 0.85 {
                    dist /= 2.0;
                }

                if dist < best_dist {
                    best_dist = dist;
                    best_neighbor = Some(n);
                }
            }

            match best_neighbor {
                Some(next) => {
                    path.push(next);
                    used[next] = true;
                    current = next;
                }
                None => break, // 死路
            }
        }

        path
    }

    /// 找到最接近指定坐标的单元格
    pub fn find_nearest_cell(cells: &[Pos2], x: f32, y: f32) -> usize {
        let target = Pos2::new(x, y);
        cells
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = a.distance(target);
                let db = b.distance(target);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// 找到指定范围内的随机单元格
    pub fn find_random_cell_in_range(
        cells: &[Pos2],
        width: f32,
        height: f32,
        x_range: (f32, f32),
        y_range: (f32, f32),
        rng: &mut impl Rng,
    ) -> usize {
        let x = rng.random_range(x_range.0..=x_range.1) * width;
        let y = rng.random_range(y_range.0..=y_range.1) * height;
        Self::find_nearest_cell(cells, x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blob_config_from_cell_count() {
        let config = BlobConfig::from_cell_count(10000);
        assert!((config.blob_power - 0.98).abs() < 0.01);
        assert!((config.line_power - 0.81).abs() < 0.01);
    }

    #[test]
    fn test_find_nearest_cell() {
        let cells = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 10.0),
            Pos2::new(5.0, 5.0),
        ];
        let nearest = BlobGenerator::find_nearest_cell(&cells, 4.0, 4.0);
        assert_eq!(nearest, 2); // 最接近 (5, 5)
    }
}
