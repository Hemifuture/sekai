use crate::delaunay::triangle::Triangle;
use egui::Pos2;

/// 创建包含所有点的超级三角形
pub fn create_super_triangle(points: &[&Pos2]) -> Triangle {
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

    // 处理边缘情况
    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        // 使用默认值创建一个大三角形
        return Triangle::new([
            Pos2::new(-1000.0, -1000.0),
            Pos2::new(0.0, 1000.0),
            Pos2::new(1000.0, -1000.0),
        ]);
    }

    // 处理所有点都在同一位置的情况
    if (max_x - min_x).abs() < 1e-6 && (max_y - min_y).abs() < 1e-6 {
        // 创建一个包围单点的三角形
        return Triangle::new([
            Pos2::new(min_x - 10.0, min_y - 10.0),
            Pos2::new(min_x, min_y + 10.0),
            Pos2::new(min_x + 10.0, min_y - 10.0),
        ]);
    }

    let dx = max_x - min_x;
    let dy = max_y - min_y;
    let dmax = dx.max(dy).max(1.0) * 2.0; // 确保至少有一定大小

    let mid_x = (min_x + max_x) / 2.0;
    let mid_y = (min_y + max_y) / 2.0;

    // 创建一个足够大的三角形
    Triangle::new([
        Pos2::new(mid_x - 10.0 * dmax, mid_y - dmax),
        Pos2::new(mid_x, mid_y + 10.0 * dmax),
        Pos2::new(mid_x + 10.0 * dmax, mid_y - dmax),
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

/// 计算给定点集的凸包边界点数量
pub fn calculate_convex_hull_points(points: &[&Pos2]) -> i32 {
    if points.len() < 3 {
        return points.len() as i32;
    }

    // 使用Graham扫描算法计算凸包
    // 首先找到最左下角的点作为参考点
    let mut ref_point = points[0];
    for &p in points.iter().skip(1) {
        if p.y < ref_point.y || (p.y == ref_point.y && p.x < ref_point.x) {
            ref_point = p;
        }
    }

    // 计算其他点相对于参考点的极角
    let mut angles: Vec<(f32, &Pos2)> = points
        .iter()
        .filter(|&&p| p != ref_point)
        .map(|&p| {
            let dx = p.x - ref_point.x;
            let dy = p.y - ref_point.y;
            (dy.atan2(dx), p)
        })
        .collect();

    // 按极角排序
    angles.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // 构建凸包
    let mut hull = vec![ref_point];
    for (_, point) in angles {
        while hull.len() >= 2 {
            let n = hull.len();
            let p1 = hull[n - 2];
            let p2 = hull[n - 1];

            // 检查是否形成左转（叉积为正）
            let cross_product = (p2.x - p1.x) * (point.y - p2.y) - (p2.y - p1.y) * (point.x - p2.x);

            if cross_product > 0.0 {
                break;
            }

            // 当前形成右转或共线，移除栈顶元素
            hull.pop();
        }

        hull.push(point);
    }

    // 返回凸包边界点的数量
    hull.len() as i32
}
