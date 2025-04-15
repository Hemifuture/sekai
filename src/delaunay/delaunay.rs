use crate::delaunay::triangle::Triangle;
use crate::delaunay::utils::{create_super_triangle, remove_duplicate_edges};
use egui::Pos2;

/// 执行Delaunay三角剖分，根据输入点集合返回三角形列表
pub fn triangulate(points: &[Pos2]) -> Vec<Triangle> {
    // 至少需要3个点才能形成三角形
    if points.len() < 3 {
        return Vec::new();
    }

    // 去除重复点
    let mut unique_points = Vec::new();
    for &point in points {
        if !unique_points.contains(&point) {
            unique_points.push(point);
        }
    }

    // 如果去重后点数量不足，返回空
    if unique_points.len() < 3 {
        return Vec::new();
    }

    // 找到能包含所有点的超级三角形
    let super_triangle = create_super_triangle(&unique_points);
    let super_points = [
        super_triangle.points[0],
        super_triangle.points[1],
        super_triangle.points[2],
    ];

    let mut triangles = vec![super_triangle];

    // 逐点插入
    for &point in &unique_points {
        let mut edges = Vec::new();

        // 移除包含当前点的三角形
        let mut i = 0;
        while i < triangles.len() {
            if triangles[i].contains_in_circumcircle(point) {
                // 收集边
                edges.push([triangles[i].points[0], triangles[i].points[1]]);
                edges.push([triangles[i].points[1], triangles[i].points[2]]);
                edges.push([triangles[i].points[2], triangles[i].points[0]]);

                // 移除三角形
                triangles.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // 移除重复边
        remove_duplicate_edges(&mut edges);

        // 使用当前点和保留的边创建新三角形
        for edge in edges {
            triangles.push(Triangle::new([edge[0], edge[1], point]));
        }
    }

    // 移除与超级三角形相关的三角形
    triangles.retain(|t| {
        !t.points.contains(&super_points[0])
            && !t.points.contains(&super_points[1])
            && !t.points.contains(&super_points[2])
    });

    triangles
}
