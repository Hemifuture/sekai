//! Delaunay 三角剖分工具函数模块
//!
//! 提供验证和辅助计算功能。

use crate::delaunay::triangle::Triangle;
use egui::Pos2;

// ============================================================================
// 公开 API
// ============================================================================

/// 验证三角剖分结果是否满足 Delaunay 性质
///
/// Delaunay 性质：任意三角形的外接圆内不包含其他点。
///
/// # 参数
/// - `indices`: 三角形索引列表，每3个索引构成一个三角形
/// - `points`: 点坐标列表
///
/// # 返回值
/// - `true`: 满足 Delaunay 性质
/// - `false`: 不满足或输入无效
///
/// # 示例
/// ```ignore
/// let valid = validate_delaunay(&indices, &points);
/// assert!(valid);
/// ```
pub fn validate_delaunay(indices: &[usize], points: &[Pos2]) -> bool {
    // 确保索引列表长度是3的倍数
    if indices.len() % 3 != 0 {
        return false;
    }

    // 去除重复点用于验证
    let unique_points = deduplicate_points(points);

    // 检查每个三角形的外接圆是否不包含任何其他点
    for triangle_idx in 0..(indices.len() / 3) {
        let i1 = indices[triangle_idx * 3];
        let i2 = indices[triangle_idx * 3 + 1];
        let i3 = indices[triangle_idx * 3 + 2];

        // 确保索引在有效范围内
        if i1 >= points.len() || i2 >= points.len() || i3 >= points.len() {
            return false;
        }

        let triangle = Triangle::new([points[i1], points[i2], points[i3]]);

        // 检查是否有任何非顶点的点在外接圆内
        for &point in &unique_points {
            if is_triangle_vertex(&triangle, point) {
                continue;
            }

            if triangle.contains_in_circumcircle(point) {
                return false;
            }
        }
    }

    true
}

/// 计算点集凸包的边界点数量
///
/// 使用 Graham 扫描算法计算凸包。
/// 主要用于调试时验证三角形数量是否符合理论值。
///
/// 理论上，对于 n 个点（其中 k 个在凸包边界上），
/// Delaunay 三角剖分产生的三角形数为 `2n - 2 - k`。
///
/// # 参数
/// - `point_count`: 有效点的数量
/// - `points`: 点坐标列表
///
/// # 返回值
/// 凸包边界上的点数量
#[cfg(debug_assertions)]
pub fn calculate_convex_hull_indices(point_count: usize, points: &[Pos2]) -> i32 {
    if point_count < 3 {
        return point_count as i32;
    }

    // 找到最左下角的点作为参考点
    let mut ref_idx = 0;
    for i in 1..point_count {
        if points[i].y < points[ref_idx].y
            || (points[i].y == points[ref_idx].y && points[i].x < points[ref_idx].x)
        {
            ref_idx = i;
        }
    }

    // 计算其他点相对于参考点的极角并排序
    let mut angles: Vec<(f32, usize)> = (0..point_count)
        .filter(|&i| i != ref_idx)
        .map(|i| {
            let dx = points[i].x - points[ref_idx].x;
            let dy = points[i].y - points[ref_idx].y;
            (dy.atan2(dx), i)
        })
        .collect();

    angles.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // 构建凸包（Graham 扫描）
    let mut hull = vec![ref_idx];
    for (_, idx) in angles {
        while hull.len() >= 2 {
            let n = hull.len();
            let p1 = points[hull[n - 2]];
            let p2 = points[hull[n - 1]];
            let p = points[idx];

            // 检查是否形成左转（叉积为正）
            let cross_product = (p2.x - p1.x) * (p.y - p2.y) - (p2.y - p1.y) * (p.x - p2.x);

            if cross_product > 0.0 {
                break;
            }
            hull.pop();
        }
        hull.push(idx);
    }

    hull.len() as i32
}

// ============================================================================
// 内部辅助函数
// ============================================================================

/// 容差值，用于浮点数比较
const EPSILON: f32 = 1e-6;

/// 去除重复点
fn deduplicate_points(points: &[Pos2]) -> Vec<Pos2> {
    let mut unique_points = Vec::new();

    for &point in points {
        let is_duplicate = unique_points.iter().any(|&p: &Pos2| {
            (p.x - point.x).abs() < EPSILON && (p.y - point.y).abs() < EPSILON
        });

        if !is_duplicate {
            unique_points.push(point);
        }
    }

    unique_points
}

/// 判断点是否是三角形的顶点之一
fn is_triangle_vertex(triangle: &Triangle, point: Pos2) -> bool {
    triangle.points.iter().any(|&p: &Pos2| {
        (p.x - point.x).abs() < EPSILON && (p.y - point.y).abs() < EPSILON
    })
}

// ============================================================================
// 废弃代码 - 保留供参考，但不再使用
// ============================================================================

#[cfg(feature = "deprecated")]
mod deprecated {
    //! 以下代码是自己实现 Delaunay 三角剖分时使用的辅助函数。
    //! 现在已切换到 `delaunator` 库，这些代码不再需要。
    //!
    //! 保留这些代码仅供学习参考，不会被编译。

    use super::*;

    /// 创建包含所有点的超级三角形
    ///
    /// **废弃原因**: 现在使用 delaunator 库，不需要手动创建超级三角形
    #[allow(dead_code)]
    pub fn create_super_triangle(points: &[&Pos2]) -> Triangle {
        let (min_x, min_y, max_x, max_y) = find_bounding_box_ref(points);

        if !is_valid_bounds(min_x, min_y, max_x, max_y) {
            return default_super_triangle();
        }

        if is_single_point(min_x, min_y, max_x, max_y) {
            return create_point_enclosing_triangle(min_x, min_y);
        }

        create_enclosing_triangle(min_x, min_y, max_x, max_y)
    }

    /// 创建超级三角形并返回三个超级顶点的索引
    #[allow(dead_code)]
    pub fn create_super_triangle_indices(point_count: usize) -> [usize; 3] {
        [point_count, point_count + 1, point_count + 2]
    }

    /// 基于点集创建超级三角形顶点
    #[allow(dead_code)]
    pub fn create_super_triangle_points(points: &[Pos2]) -> Triangle {
        let (min_x, min_y, max_x, max_y) = find_bounding_box(points);

        if !is_valid_bounds(min_x, min_y, max_x, max_y) {
            return default_super_triangle();
        }

        if is_single_point(min_x, min_y, max_x, max_y) {
            return create_point_enclosing_triangle(min_x, min_y);
        }

        create_enclosing_triangle(min_x, min_y, max_x, max_y)
    }

    /// 移除重复边
    ///
    /// **废弃原因**: 旧版 Delaunay 算法需要，新版不需要
    #[allow(dead_code)]
    pub fn remove_duplicate_edges(edges: &mut Vec<[Pos2; 2]>) {
        let mut i = 0;
        while i < edges.len() {
            let mut j = i + 1;
            while j < edges.len() {
                if edges_equal(&edges[i], &edges[j]) {
                    edges.swap_remove(j);
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
    }

    /// 计算凸包边界点数量（使用点引用版本）
    #[allow(dead_code)]
    pub fn calculate_convex_hull_points(points: &[&Pos2]) -> i32 {
        if points.len() < 3 {
            return points.len() as i32;
        }

        // Graham 扫描算法实现...
        // 省略具体实现
        points.len() as i32
    }

    // --- 辅助函数 ---

    fn find_bounding_box_ref(points: &[&Pos2]) -> (f32, f32, f32, f32) {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for &point in points {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }

        (min_x, min_y, max_x, max_y)
    }

    fn find_bounding_box(points: &[Pos2]) -> (f32, f32, f32, f32) {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for point in points {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }

        (min_x, min_y, max_x, max_y)
    }

    fn is_valid_bounds(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> bool {
        min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()
    }

    fn is_single_point(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> bool {
        (max_x - min_x).abs() < 1e-6 && (max_y - min_y).abs() < 1e-6
    }

    fn default_super_triangle() -> Triangle {
        Triangle::new([
            Pos2::new(-1000.0, -1000.0),
            Pos2::new(0.0, 1000.0),
            Pos2::new(1000.0, -1000.0),
        ])
    }

    fn create_point_enclosing_triangle(x: f32, y: f32) -> Triangle {
        Triangle::new([
            Pos2::new(x - 10.0, y - 10.0),
            Pos2::new(x, y + 10.0),
            Pos2::new(x + 10.0, y - 10.0),
        ])
    }

    fn create_enclosing_triangle(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Triangle {
        let dx = max_x - min_x;
        let dy = max_y - min_y;
        let dmax = dx.max(dy).max(1.0) * 2.0;

        let mid_x = (min_x + max_x) / 2.0;
        let mid_y = (min_y + max_y) / 2.0;

        Triangle::new([
            Pos2::new(mid_x - 10.0 * dmax, mid_y - dmax),
            Pos2::new(mid_x, mid_y + 10.0 * dmax),
            Pos2::new(mid_x + 10.0 * dmax, mid_y - dmax),
        ])
    }

    fn edges_equal(e1: &[Pos2; 2], e2: &[Pos2; 2]) -> bool {
        (e1[0] == e2[0] && e1[1] == e2[1]) || (e1[0] == e2[1] && e1[1] == e2[0])
    }
}
