//! 边的空间索引
//!
//! 用于快速查询与视口相交的边，加速视口裁剪。

use egui::{Pos2, Rect};

/// 边的空间索引
///
/// 将边按其端点分配到网格格子中，用于快速筛选与视口相交的边。
/// 主要用于视口裁剪优化。
///
/// # 设计说明
/// 每条边被分配到其端点所在的所有格子中。虽然这会导致一些边被重复存储，
/// 但可以确保不会遗漏任何可能与视口相交的边。
#[derive(Debug, Clone)]
pub struct EdgeIndex {
    /// 每个格子的尺寸
    cell_size: f32,
    /// 网格列数
    grid_width: usize,
    /// 网格行数
    grid_height: usize,
    /// 边界框
    bounds: Rect,
    /// 每个格子包含的边索引
    /// cells[y * grid_width + x] = 该格子内的边索引列表
    /// 边索引 i 对应 indices[i*2] 和 indices[i*2+1]
    cells: Vec<Vec<u32>>,
}

impl EdgeIndex {
    /// 构建边索引
    ///
    /// # 参数
    /// - `vertices`: 顶点坐标数组
    /// - `indices`: 边索引数组，每 2 个索引构成一条边
    /// - `bounds`: 边界框
    /// - `cell_size`: 网格格子尺寸
    pub fn build(vertices: &[Pos2], indices: &[u32], bounds: Rect, cell_size: f32) -> Self {
        let cell_size = cell_size.max(1.0);

        let grid_width = ((bounds.width() / cell_size).ceil() as usize).max(1);
        let grid_height = ((bounds.height() / cell_size).ceil() as usize).max(1);

        let mut cells = vec![Vec::new(); grid_width * grid_height];

        // 遍历每条边
        for (edge_idx, chunk) in indices.chunks(2).enumerate() {
            if chunk.len() != 2 {
                continue;
            }

            let p1 = vertices[chunk[0] as usize];
            let p2 = vertices[chunk[1] as usize];

            // 获取边的包围盒覆盖的所有格子
            let edge_rect = Rect::from_two_pos(p1, p2);
            let cell_indices =
                Self::get_covered_cells(edge_rect, bounds, cell_size, grid_width, grid_height);

            // 将边索引添加到所有覆盖的格子
            for cell_idx in cell_indices {
                cells[cell_idx].push(edge_idx as u32);
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
    pub fn build_auto(vertices: &[Pos2], indices: &[u32], bounds: Rect) -> Self {
        // 估算平均边长
        let edge_count = indices.len() / 2;
        if edge_count == 0 {
            return Self {
                cell_size: 1.0,
                grid_width: 1,
                grid_height: 1,
                bounds,
                cells: vec![Vec::new()],
            };
        }

        // 采样一些边来估算平均长度
        let sample_count = edge_count.min(100);
        let step = edge_count / sample_count;
        let mut total_length = 0.0;
        let mut count = 0;

        for i in (0..edge_count).step_by(step.max(1)) {
            let p1 = vertices[indices[i * 2] as usize];
            let p2 = vertices[indices[i * 2 + 1] as usize];
            total_length += (p2 - p1).length();
            count += 1;
        }

        let avg_length = if count > 0 {
            total_length / count as f32
        } else {
            10.0
        };

        // 使用平均边长的 5-10 倍作为格子尺寸
        let cell_size = avg_length * 7.0;

        Self::build(vertices, indices, bounds, cell_size)
    }

    /// 获取与矩形视口相交的所有边索引
    ///
    /// # 参数
    /// - `vertices`: 顶点坐标数组
    /// - `indices`: 边索引数组
    /// - `view_rect`: 视口矩形
    ///
    /// # 返回值
    /// 与视口相交的边的起始索引（在 indices 数组中的位置 / 2）
    pub fn query_visible_edges(
        &self,
        vertices: &[Pos2],
        indices: &[u32],
        view_rect: Rect,
    ) -> Vec<u32> {
        // 获取视口覆盖的所有格子
        let cell_indices = Self::get_covered_cells(
            view_rect,
            self.bounds,
            self.cell_size,
            self.grid_width,
            self.grid_height,
        );

        // 收集所有候选边（去重）
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for cell_idx in cell_indices {
            for &edge_idx in &self.cells[cell_idx] {
                if seen.insert(edge_idx) {
                    // 进一步验证边是否确实与视口相交
                    let i = edge_idx as usize * 2;
                    if i + 1 < indices.len() {
                        let p1 = vertices[indices[i] as usize];
                        let p2 = vertices[indices[i + 1] as usize];

                        if Self::edge_intersects_rect(p1, p2, view_rect) {
                            result.push(edge_idx);
                        }
                    }
                }
            }
        }

        result
    }

    /// 获取与矩形视口相交的边索引（返回原始 indices 数组格式）
    ///
    /// 直接返回用于渲染的索引数组片段。
    pub fn get_visible_indices(
        &self,
        vertices: &[Pos2],
        indices: &[u32],
        view_rect: Rect,
    ) -> Vec<u32> {
        let visible_edges = self.query_visible_edges(vertices, indices, view_rect);

        let mut result = Vec::with_capacity(visible_edges.len() * 2);
        for edge_idx in visible_edges {
            let i = edge_idx as usize * 2;
            if i + 1 < indices.len() {
                result.push(indices[i]);
                result.push(indices[i + 1]);
            }
        }

        result
    }

    /// 获取格子尺寸
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// 获取网格尺寸
    pub fn grid_dimensions(&self) -> (usize, usize) {
        (self.grid_width, self.grid_height)
    }

    // ========================================================================
    // 内部方法
    // ========================================================================

    /// 获取矩形覆盖的所有格子索引
    fn get_covered_cells(
        rect: Rect,
        bounds: Rect,
        cell_size: f32,
        grid_width: usize,
        grid_height: usize,
    ) -> Vec<usize> {
        let min_gx = ((rect.min.x - bounds.min.x) / cell_size).floor().max(0.0) as usize;
        let min_gy = ((rect.min.y - bounds.min.y) / cell_size).floor().max(0.0) as usize;
        let max_gx = ((rect.max.x - bounds.min.x) / cell_size)
            .floor()
            .min((grid_width - 1) as f32) as usize;
        let max_gy = ((rect.max.y - bounds.min.y) / cell_size)
            .floor()
            .min((grid_height - 1) as f32) as usize;

        let mut result = Vec::new();
        for gy in min_gy..=max_gy {
            for gx in min_gx..=max_gx {
                result.push(gy * grid_width + gx);
            }
        }

        result
    }

    /// 检查边是否与矩形相交
    fn edge_intersects_rect(p1: Pos2, p2: Pos2, rect: Rect) -> bool {
        // 快速接受：任一端点在矩形内
        if rect.contains(p1) || rect.contains(p2) {
            return true;
        }

        // 快速拒绝：线段完全在矩形的一侧
        if (p1.x < rect.min.x && p2.x < rect.min.x)
            || (p1.x > rect.max.x && p2.x > rect.max.x)
            || (p1.y < rect.min.y && p2.y < rect.min.y)
            || (p1.y > rect.max.y && p2.y > rect.max.y)
        {
            return false;
        }

        // Cohen-Sutherland 线段裁剪算法
        Self::line_intersects_rect_precise(p1, p2, rect)
    }

    /// 精确的线段-矩形相交测试
    fn line_intersects_rect_precise(p1: Pos2, p2: Pos2, rect: Rect) -> bool {
        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;

        // 检查与水平边界的交点
        if dy.abs() > 1e-6 {
            // 上边界
            let t = (rect.min.y - p1.y) / dy;
            if (0.0..=1.0).contains(&t) {
                let x = p1.x + t * dx;
                if x >= rect.min.x && x <= rect.max.x {
                    return true;
                }
            }

            // 下边界
            let t = (rect.max.y - p1.y) / dy;
            if (0.0..=1.0).contains(&t) {
                let x = p1.x + t * dx;
                if x >= rect.min.x && x <= rect.max.x {
                    return true;
                }
            }
        }

        // 检查与垂直边界的交点
        if dx.abs() > 1e-6 {
            // 左边界
            let t = (rect.min.x - p1.x) / dx;
            if (0.0..=1.0).contains(&t) {
                let y = p1.y + t * dy;
                if y >= rect.min.y && y <= rect.max.y {
                    return true;
                }
            }

            // 右边界
            let t = (rect.max.x - p1.x) / dx;
            if (0.0..=1.0).contains(&t) {
                let y = p1.y + t * dy;
                if y >= rect.min.y && y <= rect.max.y {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_and_query() {
        let vertices = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(100.0, 0.0),
            Pos2::new(100.0, 100.0),
            Pos2::new(0.0, 100.0),
        ];

        // 四条边: 0-1, 1-2, 2-3, 3-0
        let indices = vec![0, 1, 1, 2, 2, 3, 3, 0];

        let bounds = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
        let index = EdgeIndex::build(&vertices, &indices, bounds, 50.0);

        // 查询左半部分视口
        let view_rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(50.0, 100.0));
        let visible = index.query_visible_edges(&vertices, &indices, view_rect);

        // 应该包含边 0 (0-1, 上边), 边 3 (3-0, 左边)
        // 可能还包含边 2 (2-3, 下边的左半部分)
        assert!(!visible.is_empty());
    }

    #[test]
    fn test_edge_intersects_rect() {
        let rect = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(90.0, 90.0));

        // 完全在矩形内的边
        assert!(EdgeIndex::edge_intersects_rect(
            Pos2::new(20.0, 20.0),
            Pos2::new(80.0, 80.0),
            rect
        ));

        // 穿过矩形的边
        assert!(EdgeIndex::edge_intersects_rect(
            Pos2::new(0.0, 50.0),
            Pos2::new(100.0, 50.0),
            rect
        ));

        // 完全在矩形外的边
        assert!(!EdgeIndex::edge_intersects_rect(
            Pos2::new(0.0, 0.0),
            Pos2::new(5.0, 5.0),
            rect
        ));
    }
}
