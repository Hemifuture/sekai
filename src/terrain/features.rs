// 地形特征检测与清理
//
// 参考 Azgaar Fantasy Map Generator 的 features.ts
// 用于识别连通区域（海洋、湖泊、岛屿）并清理孤立的小区域

use super::heightmap::SEA_LEVEL;
use std::collections::VecDeque;

/// 地形特征类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureType {
    Ocean,  // 连接地图边缘的海洋
    Lake,   // 内陆水体
    Island, // 陆地
}

/// 地形特征
#[derive(Debug, Clone)]
pub struct Feature {
    pub id: u16,
    pub feature_type: FeatureType,
    pub cells: Vec<usize>,
    pub is_border: bool, // 是否接触地图边缘
}

impl Feature {
    pub fn size(&self) -> usize {
        self.cells.len()
    }

    pub fn is_land(&self) -> bool {
        self.feature_type == FeatureType::Island
    }

    pub fn is_water(&self) -> bool {
        matches!(self.feature_type, FeatureType::Ocean | FeatureType::Lake)
    }
}

/// 特征检测器
pub struct FeatureDetector {
    /// 最小岛屿大小（小于此值的将被清理）
    pub min_island_size: usize,
    /// 最小湖泊大小（小于此值的将被填充）
    pub min_lake_size: usize,
}

impl Default for FeatureDetector {
    fn default() -> Self {
        Self {
            min_island_size: 3,
            min_lake_size: 2,
        }
    }
}

impl FeatureDetector {
    pub fn new(min_island_size: usize, min_lake_size: usize) -> Self {
        Self {
            min_island_size,
            min_lake_size,
        }
    }

    /// 检测所有地形特征
    ///
    /// 返回 (特征列表, 每个单元格对应的特征 ID)
    pub fn detect_features(
        &self,
        heights: &[u8],
        neighbors: &[Vec<u32>],
        border_cells: &[bool],
    ) -> (Vec<Feature>, Vec<u16>) {
        let n = heights.len();
        let mut feature_ids = vec![0u16; n];
        let mut features = Vec::new();
        let mut current_id = 0u16;

        // 找到第一个未标记的单元格
        let mut search_start = 0usize;

        while let Some(start) = feature_ids[search_start..]
            .iter()
            .position(|&id| id == 0)
            .map(|p| p + search_start)
        {
            search_start = start + 1;
            current_id += 1;
            let is_land = heights[start] >= SEA_LEVEL;
            let mut is_border = border_cells.get(start).copied().unwrap_or(false);
            let mut cells = Vec::new();

            // BFS 填充
            let mut queue = VecDeque::new();
            queue.push_back(start);
            feature_ids[start] = current_id;

            while let Some(current) = queue.pop_front() {
                cells.push(current);

                if !is_border && border_cells.get(current).copied().unwrap_or(false) {
                    is_border = true;
                }

                for &neighbor in &neighbors[current] {
                    let n_idx = neighbor as usize;
                    if feature_ids[n_idx] != 0 {
                        continue;
                    }

                    let neighbor_is_land = heights[n_idx] >= SEA_LEVEL;
                    if neighbor_is_land == is_land {
                        feature_ids[n_idx] = current_id;
                        queue.push_back(n_idx);
                    }
                }
            }

            let feature_type = if is_land {
                FeatureType::Island
            } else if is_border {
                FeatureType::Ocean
            } else {
                FeatureType::Lake
            };

            features.push(Feature {
                id: current_id,
                feature_type,
                cells,
                is_border,
            });
        }

        (features, feature_ids)
    }

    /// 清理孤立的小特征
    ///
    /// - 太小的岛屿会被淹没（变成海洋）
    /// - 太小的湖泊会被填充（变成陆地）
    pub fn cleanup_small_features(&self, heights: &mut [u8], features: &[Feature]) -> usize {
        let mut cleaned = 0;

        for feature in features {
            match feature.feature_type {
                FeatureType::Island => {
                    if feature.size() < self.min_island_size {
                        // 淹没小岛
                        for &cell in &feature.cells {
                            heights[cell] = SEA_LEVEL - 1;
                        }
                        cleaned += feature.size();
                    }
                }
                FeatureType::Lake => {
                    if feature.size() < self.min_lake_size {
                        // 填充小湖
                        for &cell in &feature.cells {
                            heights[cell] = SEA_LEVEL;
                        }
                        cleaned += feature.size();
                    }
                }
                FeatureType::Ocean => {
                    // 海洋不清理
                }
            }
        }

        cleaned
    }

    /// 获取海岸线单元格
    ///
    /// 返回所有与海洋相邻的陆地单元格
    pub fn get_coastline_cells(&self, heights: &[u8], neighbors: &[Vec<u32>]) -> Vec<usize> {
        let mut coastline = Vec::new();

        for (i, &h) in heights.iter().enumerate() {
            if h < SEA_LEVEL {
                continue; // 跳过海洋
            }

            // 检查是否有海洋邻居
            let has_ocean_neighbor = neighbors[i]
                .iter()
                .any(|&n| heights[n as usize] < SEA_LEVEL);

            if has_ocean_neighbor {
                coastline.push(i);
            }
        }

        coastline
    }

    /// 计算到海岸线的距离场
    ///
    /// 陆地为正值，海洋为负值
    /// 返回每个单元格到最近海岸线的距离
    pub fn calculate_distance_field(&self, heights: &[u8], neighbors: &[Vec<u32>]) -> Vec<i8> {
        let n = heights.len();
        let mut distance = vec![0i8; n];

        // 标记海岸线
        for (i, &h) in heights.iter().enumerate() {
            let is_land = h >= SEA_LEVEL;
            let has_opposite_neighbor = neighbors[i].iter().any(|&n| {
                let neighbor_is_land = heights[n as usize] >= SEA_LEVEL;
                neighbor_is_land != is_land
            });

            if has_opposite_neighbor {
                distance[i] = if is_land { 1 } else { -1 };
            }
        }

        // 向内陆扩展
        self.markup_distance(&mut distance, neighbors, 2, 1, 127);
        // 向深海扩展
        self.markup_distance(&mut distance, neighbors, -2, -1, -127);

        distance
    }

    /// 辅助函数：从起始值向外扩展距离标记
    fn markup_distance(
        &self,
        distance: &mut [i8],
        neighbors: &[Vec<u32>],
        start: i8,
        increment: i8,
        limit: i8,
    ) {
        let mut current = start;
        let prev = start - increment;

        loop {
            let mut marked = 0;

            for i in 0..distance.len() {
                if distance[i] != prev {
                    continue;
                }

                for &neighbor in &neighbors[i] {
                    let n = neighbor as usize;
                    if distance[n] == 0 {
                        distance[n] = current;
                        marked += 1;
                    }
                }
            }

            if marked == 0 || current == limit {
                break;
            }

            current += increment;
        }
    }

    /// 平滑海岸线
    ///
    /// 移除单独突出的点和单独凹陷的点
    pub fn smooth_coastline(
        &self,
        heights: &mut [u8],
        neighbors: &[Vec<u32>],
        iterations: u32,
    ) -> usize {
        let mut changed = 0;

        for _ in 0..iterations {
            let original = heights.to_vec();

            for (i, &h) in original.iter().enumerate() {
                let is_land = h >= SEA_LEVEL;

                // 统计同类型邻居数量
                let same_type_count = neighbors[i]
                    .iter()
                    .filter(|&&n| (original[n as usize] >= SEA_LEVEL) == is_land)
                    .count();

                let total_neighbors = neighbors[i].len();

                // 如果大多数邻居是不同类型，则转换
                if total_neighbors > 0 && same_type_count <= total_neighbors / 4 {
                    if is_land {
                        heights[i] = SEA_LEVEL - 1;
                    } else {
                        heights[i] = SEA_LEVEL;
                    }
                    changed += 1;
                }
            }
        }

        changed
    }

    /// 约束噪声，防止在海岸线附近产生散点
    ///
    /// 返回每个单元格允许的最大噪声幅度
    pub fn calculate_noise_constraints(&self, heights: &[u8], distance_field: &[i8]) -> Vec<f32> {
        heights
            .iter()
            .zip(distance_field.iter())
            .map(|(&_h, &d)| {
                let dist = d.abs() as f32;

                // 海岸线附近（距离 1-2）几乎不允许噪声
                // 内陆/深海区域允许更多噪声
                if dist <= 1.0 {
                    0.1 // 海岸线：几乎无噪声
                } else if dist <= 2.0 {
                    0.3 // 近海岸：少量噪声
                } else if dist <= 4.0 {
                    0.6 // 过渡区：中等噪声
                } else {
                    1.0 // 内陆/深海：完全噪声
                }
            })
            .collect()
    }
}

/// 应用约束噪声
///
/// 确保噪声不会改变海陆类型
pub fn apply_constrained_noise(heights: &mut [f32], noise: &[f32], constraints: &[f32]) {
    let sea_level = SEA_LEVEL as f32;

    for i in 0..heights.len() {
        let original = heights[i];
        let is_land = original >= sea_level;

        // 应用约束后的噪声
        let constrained_noise = noise[i] * constraints[i];
        let new_height = original + constrained_noise;

        // 检查是否改变了海陆类型
        let new_is_land = new_height >= sea_level;

        if is_land != new_is_land {
            // 不允许改变海陆类型，限制到边界
            if is_land {
                heights[i] = sea_level; // 保持为陆地最低点
            } else {
                heights[i] = sea_level - 0.1; // 保持为海洋最高点
            }
        } else {
            heights[i] = new_height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_grid() -> (Vec<u8>, Vec<Vec<u32>>, Vec<bool>) {
        // 3x3 网格，中心是岛屿
        // 0 1 2
        // 3 4 5
        // 6 7 8
        let heights = vec![
            10, 10, 10, // 海洋
            10, 25, 10, // 中心是陆地
            10, 10, 10, // 海洋
        ];

        let neighbors = vec![
            vec![1, 3],       // 0
            vec![0, 2, 4],    // 1
            vec![1, 5],       // 2
            vec![0, 4, 6],    // 3
            vec![1, 3, 5, 7], // 4
            vec![2, 4, 8],    // 5
            vec![3, 7],       // 6
            vec![4, 6, 8],    // 7
            vec![5, 7],       // 8
        ];

        let borders = vec![true, true, true, true, false, true, true, true, true];

        (heights, neighbors, borders)
    }

    #[test]
    fn test_detect_features() {
        let (heights, neighbors, borders) = create_test_grid();
        let detector = FeatureDetector::default();

        let (features, ids) = detector.detect_features(&heights, &neighbors, &borders);

        // 应该有 2 个特征：1 个海洋，1 个岛屿
        assert_eq!(features.len(), 2);

        // 中心单元格应该是岛屿
        let island_id = ids[4];
        let island = features.iter().find(|f| f.id == island_id).unwrap();
        assert_eq!(island.feature_type, FeatureType::Island);
        assert_eq!(island.size(), 1);
    }

    #[test]
    fn test_cleanup_small_islands() {
        let (mut heights, neighbors, borders) = create_test_grid();
        let detector = FeatureDetector::new(2, 1); // 最小岛屿大小 = 2

        let (features, _) = detector.detect_features(&heights, &neighbors, &borders);
        detector.cleanup_small_features(&mut heights, &features);

        // 小岛应该被淹没
        assert!(heights[4] < SEA_LEVEL);
    }

    #[test]
    fn test_coastline_detection() {
        let heights = vec![
            10, 10, 10, 10, 10, 25, 25, 10, 10, 25, 25, 10, 10, 10, 10, 10,
        ];

        let neighbors: Vec<Vec<u32>> = (0..16)
            .map(|i| {
                let row = i / 4;
                let col = i % 4;
                let mut n = Vec::new();
                if row > 0 {
                    n.push((i - 4) as u32);
                }
                if row < 3 {
                    n.push((i + 4) as u32);
                }
                if col > 0 {
                    n.push((i - 1) as u32);
                }
                if col < 3 {
                    n.push((i + 1) as u32);
                }
                n
            })
            .collect();

        let detector = FeatureDetector::default();
        let coastline = detector.get_coastline_cells(&heights, &neighbors);

        // 中心 2x2 的所有单元格都应该在海岸线上
        assert_eq!(coastline.len(), 4);
        assert!(coastline.contains(&5));
        assert!(coastline.contains(&6));
        assert!(coastline.contains(&9));
        assert!(coastline.contains(&10));
    }
}
