use crate::delaunay::triangle::Triangle;
use egui::Pos2;

/// 创建包含所有点的超级三角形
pub fn create_super_triangle(points: &[Pos2]) -> Triangle {
    // 找到包围所有点的矩形
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

    let dx = max_x - min_x;
    let dy = max_y - min_y;
    let dmax = dx.max(dy) * 2.0;

    let mid_x = (min_x + max_x) / 2.0;
    let mid_y = (min_y + max_y) / 2.0;

    // 创建一个足够大的三角形
    Triangle::new([
        Pos2::new(mid_x - 20.0 * dmax, mid_y - dmax),
        Pos2::new(mid_x, mid_y + 20.0 * dmax),
        Pos2::new(mid_x + 20.0 * dmax, mid_y - dmax),
    ])
}

/// 移除重复边
pub fn remove_duplicate_edges(edges: &mut Vec<[Pos2; 2]>) {
    let mut i = 0;
    while i < edges.len() {
        let mut j = i + 1;
        while j < edges.len() {
            // 检查是否为相同的边（忽略顺序）
            if (edges[i][0] == edges[j][0] && edges[i][1] == edges[j][1])
                || (edges[i][0] == edges[j][1] && edges[i][1] == edges[j][0])
            {
                // 找到重复边，只移除一条
                edges.swap_remove(j);
            } else {
                j += 1;
            }
        }
        i += 1;
    }
}

/// 验证三角剖分结果是否满足Delaunay性质
pub fn validate_delaunay(triangles: &[Triangle], points: &[Pos2]) -> bool {
    // 去除重复点
    let mut unique_points = Vec::new();
    let epsilon = 1e-6;

    for &point in points {
        let is_duplicate = unique_points.iter().any(|&p: &Pos2| {
            // 使用容差判断是否是重复点
            (p.x - point.x).abs() < epsilon && (p.y - point.y).abs() < epsilon
        });

        if !is_duplicate {
            unique_points.push(point);
        }
    }

    // 检查每个三角形的外接圆是否不包含任何其他点
    for triangle in triangles {
        for &point in &unique_points {
            // 使用容差判断点是否是三角形的顶点之一
            let is_vertex = triangle.points.iter().any(|&p: &Pos2| {
                (p.x - point.x).abs() < epsilon && (p.y - point.y).abs() < epsilon
            });

            if is_vertex {
                continue;
            }

            // 如果有任何点在外接圆内，则不满足Delaunay性质
            if triangle.contains_in_circumcircle(point) {
                return false;
            }
        }
    }

    true
}
