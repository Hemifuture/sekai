// BFS 扩散式地形生成
//
// 参考 Azgaar Fantasy Map Generator 的算法
// 使用 BFS 从中心向外传播高度值，创造自然不规则的地形

use std::collections::VecDeque;
use eframe::egui::Pos2;
use rand::Rng;

/// Blob 生成器配置
#[derive(Debug, Clone)]
pub struct BlobConfig {
    /// 衰减因子 (0.93 ~ 0.997)
    /// 值越高，blob 越大
    pub blob_power: f32,
    /// 线性衰减因子（用于山脉）
    pub line_power: f32,
    /// 随机扰动范围 (例如 0.2 表示 0.9 ~ 1.1)
    pub jitter: f32,
}

impl Default for BlobConfig {
    fn default() -> Self {
        Self {
            blob_power: 0.97,
            line_power: 0.82,
            jitter: 0.2,
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
            jitter: 0.2,
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
        if start_idx >= heights.len() {
            return;
        }

        let mut change = vec![0.0f32; heights.len()];
        change[start_idx] = height;

        let mut queue = VecDeque::new();
        queue.push_back(start_idx);

        while let Some(current) = queue.pop_front() {
            for &neighbor in &neighbors[current] {
                let n = neighbor as usize;
                if change[n] > 0.0 {
                    continue;
                }

                // 核心算法：指数衰减 + 随机扰动
                let jitter = 1.0 - self.config.jitter + rng.gen::<f32>() * self.config.jitter * 2.0;
                change[n] = change[current].powf(self.config.blob_power) * jitter;

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
            // 应用高度变化
            let jitter = 1.0 - self.config.jitter + rng.gen::<f32>() * self.config.jitter * 2.0;
            heights[current] -= h * jitter;

            // 计算下一层的高度
            let next_h = h.powf(self.config.blob_power);
            if next_h < 1.0 {
                continue;
            }

            for &neighbor in &neighbors[current] {
                let n = neighbor as usize;
                if used[n] {
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
                let jitter = 0.85 + rng.gen::<f32>() * 0.3;
                heights[idx] += h * jitter;
            }

            // 高度衰减
            h = h.powf(self.config.line_power) - 1.0;
            if h < 2.0 {
                break;
            }

            // 扩展到邻居
            for &f in &frontier {
                for &neighbor in &neighbors[f] {
                    let n = neighbor as usize;
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
                // 找到高度最低的邻居
                if let Some(&min_neighbor) = neighbors[current]
                    .iter()
                    .min_by(|&&a, &&b| {
                        heights[a as usize]
                            .partial_cmp(&heights[b as usize])
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                {
                    let min_idx = min_neighbor as usize;
                    // 平滑过渡
                    heights[min_idx] = (heights[current] * 2.0 + heights[min_idx]) / 3.0;
                    current = min_idx;
                }
            }
        }
    }

    /// 添加 BFS 扩散式海沟（与山脉相反）
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
                let jitter = 0.85 + rng.gen::<f32>() * 0.3;
                heights[idx] -= d * jitter;
            }

            d = d.powf(self.config.line_power) - 1.0;
            if d < 2.0 {
                break;
            }

            for &f in &frontier {
                for &neighbor in &neighbors[f] {
                    let n = neighbor as usize;
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

            for &neighbor in &neighbors[current] {
                let n = neighbor as usize;
                if used[n] {
                    continue;
                }

                let pos = cells[n];
                let mut dist = (pos.x - end_pos.x).powi(2) + (pos.y - end_pos.y).powi(2);

                // 15% 的概率将距离减半（增加路径随机性）
                if rng.gen::<f32>() > 0.85 {
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
