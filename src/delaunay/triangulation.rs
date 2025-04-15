use egui::Pos2;

pub struct Triangle {
    pub points: [Pos2; 3],
}

impl Triangle {
    pub fn new(points: [Pos2; 3]) -> Self {
        Self { points }
    }

    // 判断一个点是否在三角形的外接圆内
    fn contains_in_circumcircle(&self, point: Pos2) -> bool {
        let a = self.points[0];
        let b = self.points[1];
        let c = self.points[2];

        let ab = a.x * a.x + a.y * a.y;
        let cd = b.x * b.x + b.y * b.y;
        let ef = c.x * c.x + c.y * c.y;

        let ax = a.x;
        let ay = a.y;
        let bx = b.x;
        let by = b.y;
        let cx = c.x;
        let cy = c.y;

        let det = ax * (by * ef - cd * cy) - ay * (bx * ef - cd * cx) + ab * (bx * cy - by * cx)
            - bx * (ax * ef - ay * cx)
            + by * (ax * cd - ay * bx)
            - cd * (ax * cy - ay * cx);

        let px = point.x;
        let py = point.y;
        let p_squared = px * px + py * py;

        let test = ax * (by * p_squared - cd * py) - ay * (bx * p_squared - cd * px)
            + ab * (bx * py - by * px)
            - px * (ax * cd - ay * bx)
            + py * (ax * by - ay * bx)
            - p_squared * (ax * by - ay * bx);

        if det > 0.0 {
            test < 0.0
        } else {
            test > 0.0
        }
    }
}

pub fn triangulate(points: &[Pos2]) -> Vec<Triangle> {
    if points.len() < 3 {
        return Vec::new();
    }

    // 找到能包含所有点的超级三角形
    let super_triangle = create_super_triangle(points);
    let super_points = [
        super_triangle.points[0],
        super_triangle.points[1],
        super_triangle.points[2],
    ];

    let mut triangles = vec![super_triangle];

    // 逐点插入
    for &point in points {
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

// 创建包含所有点的超级三角形
fn create_super_triangle(points: &[Pos2]) -> Triangle {
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

// 移除重复边
fn remove_duplicate_edges(edges: &mut Vec<[Pos2; 2]>) {
    let mut i = 0;
    while i < edges.len() {
        let mut j = i + 1;
        while j < edges.len() {
            // 检查是否为相同的边（忽略顺序）
            if (edges[i][0] == edges[j][0] && edges[i][1] == edges[j][1])
                || (edges[i][0] == edges[j][1] && edges[i][1] == edges[j][0])
            {
                // 找到重复边，移除两条
                edges.swap_remove(j);
                edges.swap_remove(i);
                i -= 1;
                break;
            }
            j += 1;
        }
        i += 1;
    }
}
