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
/// - `indices`: 三角形索引列表（`&[u32]`），每3个索引构成一个三角形
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
pub fn validate_delaunay(indices: &[u32], points: &[Pos2]) -> bool {
    // 确保索引列表长度是3的倍数
    if indices.len() % 3 != 0 {
        return false;
    }

    // 去除重复点用于验证
    let unique_points = deduplicate_points(points);

    // 检查每个三角形的外接圆是否不包含任何其他点
    for triangle_idx in 0..(indices.len() / 3) {
        let i1 = indices[triangle_idx * 3] as usize;
        let i2 = indices[triangle_idx * 3 + 1] as usize;
        let i3 = indices[triangle_idx * 3 + 2] as usize;

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
        let is_duplicate = unique_points
            .iter()
            .any(|&p: &Pos2| (p.x - point.x).abs() < EPSILON && (p.y - point.y).abs() < EPSILON);

        if !is_duplicate {
            unique_points.push(point);
        }
    }

    unique_points
}

/// 判断点是否是三角形的顶点之一
fn is_triangle_vertex(triangle: &Triangle, point: Pos2) -> bool {
    triangle
        .points
        .iter()
        .any(|&p: &Pos2| (p.x - point.x).abs() < EPSILON && (p.y - point.y).abs() < EPSILON)
}
