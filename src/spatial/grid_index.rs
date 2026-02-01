//! 网格空间索引
//!
//! 将空间划分为均匀的格子，快速查询点的空间关系。

use egui::{Pos2, Rect};

/// 网格空间索引
///
/// 将二维空间划分为均匀的格子，每个格子记录其中包含的点索引。
/// 用于加速：
/// - 点击测试：O(1) 查找鼠标位置对应的 Voronoi 单元格
/// - 邻居查询：O(1) 查找某点附近的其他点
/// - 范围查询：快速获取矩形/圆形范围内的点
///
/// # 示例
/// ```ignore
/// let index = GridIndex::build(&points, bounds, cell_size);
///
/// // 查找包含指定点的 Voronoi 单元格
/// if let Some(cell_idx) = index.find_nearest(&points, query_pos) {
///     println!("点击了单元格 {}", cell_idx);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct GridIndex {
    /// 每个格子的尺寸
    cell_size: f32,
    /// 网格列数
    grid_width: usize,
    /// 网格行数
    grid_height: usize,
    /// 边界框
    bounds: Rect,
    /// 每个格子包含的点索引（扁平化存储）
    /// cells[y * grid_width + x] = 该格子内的点索引列表
    cells: Vec<Vec<u32>>,
}

impl GridIndex {
    /// 构建网格索引
    ///
    /// # 参数
    /// - `points`: 需要索引的点集
    /// - `bounds`: 点集的边界框
    /// - `cell_size`: 每个网格格子的尺寸（推荐使用点的平均间距的 2-4 倍）
    ///
    /// # 性能
    /// - 构建时间: O(n)
    /// - 空间复杂度: O(n + grid_cells)
    pub fn build(points: &[Pos2], bounds: Rect, cell_size: f32) -> Self {
        let cell_size = cell_size.max(1.0);

        let grid_width = ((bounds.width() / cell_size).ceil() as usize).max(1);
        let grid_height = ((bounds.height() / cell_size).ceil() as usize).max(1);

        let mut cells = vec![Vec::new(); grid_width * grid_height];

        // 将每个点分配到对应的格子
        for (idx, &point) in points.iter().enumerate() {
            if let Some(cell_idx) =
                Self::point_to_cell_index_static(point, bounds, cell_size, grid_width, grid_height)
            {
                cells[cell_idx].push(idx as u32);
            }
        }

        Self {
            cell_size,
            grid_width,
            grid_height,
            bounds,
            cells,
        }
    }

    /// 使用默认格子尺寸构建索引
    ///
    /// 格子尺寸根据点密度自动计算。
    pub fn build_auto(points: &[Pos2], bounds: Rect) -> Self {
        // 估算平均点间距
        let area = bounds.width() * bounds.height();
        let avg_spacing = (area / points.len() as f32).sqrt();

        // 使用平均间距的 3 倍作为格子尺寸
        // 这样每个格子平均包含约 9 个点
        let cell_size = avg_spacing * 3.0;

        Self::build(points, bounds, cell_size)
    }

    /// 查找包含指定位置的 Voronoi 单元格
    ///
    /// 通过查找最近的点来确定 Voronoi 单元格。
    ///
    /// # 参数
    /// - `points`: 原始点集
    /// - `pos`: 查询位置
    ///
    /// # 返回值
    /// 最近点的索引（即 Voronoi 单元格索引），如果点集为空则返回 None
    ///
    /// # 性能
    /// - 平均: O(1)（只检查当前格子及邻居中的点）
    /// - 最坏: O(k) 其中 k 是邻近格子中的点数
    pub fn find_nearest(&self, points: &[Pos2], pos: Pos2) -> Option<u32> {
        if points.is_empty() {
            return None;
        }

        // 获取当前格子及其邻居中的候选点
        let candidates = self.get_nearby_points(pos);

        if candidates.is_empty() {
            // 如果附近没有点，扩大搜索范围
            return self.find_nearest_fallback(points, pos);
        }

        // 在候选点中找最近的
        candidates
            .iter()
            .min_by(|&&a, &&b| {
                let da = (points[a as usize] - pos).length_sq();
                let db = (points[b as usize] - pos).length_sq();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
    }

    /// 获取指定位置附近的所有点（当前格子及 8 个邻居）
    pub fn get_nearby_points(&self, pos: Pos2) -> Vec<u32> {
        let (gx, gy) = self.point_to_grid_coords(pos);

        let mut result = Vec::new();

        // 遍历 3x3 邻域
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let nx = gx as i32 + dx;
                let ny = gy as i32 + dy;

                if nx >= 0 && nx < self.grid_width as i32 && ny >= 0 && ny < self.grid_height as i32
                {
                    let cell_idx = ny as usize * self.grid_width + nx as usize;
                    result.extend_from_slice(&self.cells[cell_idx]);
                }
            }
        }

        result
    }

    /// 查询矩形范围内的所有点
    ///
    /// # 参数
    /// - `rect`: 查询矩形（画布坐标）
    ///
    /// # 返回值
    /// 矩形范围内所有格子包含的点索引
    pub fn query_rect(&self, rect: Rect) -> Vec<u32> {
        let (min_gx, min_gy) = self.point_to_grid_coords(rect.min);
        let (max_gx, max_gy) = self.point_to_grid_coords(rect.max);

        let mut result = Vec::new();

        for gy in min_gy..=max_gy {
            for gx in min_gx..=max_gx {
                if gx < self.grid_width && gy < self.grid_height {
                    let cell_idx = gy * self.grid_width + gx;
                    result.extend_from_slice(&self.cells[cell_idx]);
                }
            }
        }

        result
    }

    /// 查询圆形范围内的所有点
    ///
    /// # 参数
    /// - `points`: 原始点集
    /// - `center`: 圆心
    /// - `radius`: 半径
    ///
    /// # 返回值
    /// 圆形范围内的点索引
    pub fn query_radius(&self, points: &[Pos2], center: Pos2, radius: f32) -> Vec<u32> {
        // 先用矩形粗筛
        let rect = Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
        let candidates = self.query_rect(rect);

        // 精确筛选
        let radius_sq = radius * radius;
        candidates
            .into_iter()
            .filter(|&idx| {
                let p = points[idx as usize];
                (p - center).length_sq() <= radius_sq
            })
            .collect()
    }

    /// 获取格子尺寸
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// 获取网格尺寸
    pub fn grid_dimensions(&self) -> (usize, usize) {
        (self.grid_width, self.grid_height)
    }

    /// 获取边界框
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    // ========================================================================
    // 内部方法
    // ========================================================================

    /// 将点坐标转换为网格坐标
    fn point_to_grid_coords(&self, pos: Pos2) -> (usize, usize) {
        let x = ((pos.x - self.bounds.min.x) / self.cell_size)
            .floor()
            .max(0.0)
            .min((self.grid_width - 1) as f32) as usize;
        let y = ((pos.y - self.bounds.min.y) / self.cell_size)
            .floor()
            .max(0.0)
            .min((self.grid_height - 1) as f32) as usize;
        (x, y)
    }

    /// 静态版本的坐标转换（用于构建时）
    fn point_to_cell_index_static(
        pos: Pos2,
        bounds: Rect,
        cell_size: f32,
        grid_width: usize,
        grid_height: usize,
    ) -> Option<usize> {
        if !bounds.contains(pos) {
            // 将超出边界的点放入最近的格子
            let clamped = Pos2::new(
                pos.x.clamp(bounds.min.x, bounds.max.x - 0.001),
                pos.y.clamp(bounds.min.y, bounds.max.y - 0.001),
            );
            let x = ((clamped.x - bounds.min.x) / cell_size).floor() as usize;
            let y = ((clamped.y - bounds.min.y) / cell_size).floor() as usize;
            let x = x.min(grid_width - 1);
            let y = y.min(grid_height - 1);
            return Some(y * grid_width + x);
        }

        let x = ((pos.x - bounds.min.x) / cell_size).floor() as usize;
        let y = ((pos.y - bounds.min.y) / cell_size).floor() as usize;

        if x < grid_width && y < grid_height {
            Some(y * grid_width + x)
        } else {
            None
        }
    }

    /// 后备搜索：当附近格子为空时，遍历所有点
    fn find_nearest_fallback(&self, points: &[Pos2], pos: Pos2) -> Option<u32> {
        points
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (**a - pos).length_sq();
                let db = (**b - pos).length_sq();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_and_query() {
        let points = vec![
            Pos2::new(10.0, 10.0),
            Pos2::new(20.0, 10.0),
            Pos2::new(10.0, 20.0),
            Pos2::new(90.0, 90.0),
        ];

        let bounds = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
        let index = GridIndex::build(&points, bounds, 30.0);

        // 测试 find_nearest
        let nearest = index.find_nearest(&points, Pos2::new(15.0, 15.0));
        assert!(nearest.is_some());
        // 应该找到点 0, 1, 2 中的一个（都在左上角）
        assert!(nearest.unwrap() < 3);

        // 测试右下角的点
        let nearest = index.find_nearest(&points, Pos2::new(85.0, 85.0));
        assert_eq!(nearest, Some(3));
    }

    #[test]
    fn test_query_rect() {
        let points = vec![
            Pos2::new(10.0, 10.0),
            Pos2::new(50.0, 50.0),
            Pos2::new(90.0, 90.0),
        ];

        let bounds = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
        let index = GridIndex::build(&points, bounds, 30.0);

        // 查询左上角
        let result = index.query_rect(Rect::from_min_max(Pos2::ZERO, Pos2::new(40.0, 40.0)));
        assert!(result.contains(&0));
    }

    #[test]
    fn test_query_radius() {
        let points = vec![
            Pos2::new(50.0, 50.0),
            Pos2::new(55.0, 50.0),  // 距离 5
            Pos2::new(100.0, 50.0), // 距离 50
        ];

        let bounds = Rect::from_min_max(Pos2::ZERO, Pos2::new(150.0, 100.0));
        let index = GridIndex::build(&points, bounds, 30.0);

        // 查询半径 10 内的点
        let result = index.query_radius(&points, Pos2::new(50.0, 50.0), 10.0);
        assert!(result.contains(&0));
        assert!(result.contains(&1));
        assert!(!result.contains(&2));
    }
}
