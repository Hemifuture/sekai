use egui::Pos2;

/// 三角形结构，存储三个顶点坐标
#[derive(Debug, Clone, Copy)]
pub struct Triangle {
    pub points: [Pos2; 3],
}

impl Triangle {
    /// 创建新的三角形
    pub fn new(points: [Pos2; 3]) -> Self {
        Self { points }
    }

    /// 判断一个点是否在三角形的外接圆内
    pub fn contains_in_circumcircle(&self, point: Pos2) -> bool {
        let a = self.points[0];
        let b = self.points[1];
        let c = self.points[2];

        // 使用精确的行列式算法判断点是否在外接圆内
        // 为了数值稳定性，将点移到一个更合适的坐标系
        let epsilon = 1e-10;

        // 首先检查三角形是否有效（非退化）
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area.abs() < epsilon {
            return false; // 退化三角形
        }

        // 为了数值稳定性，如果点非常接近三角形的顶点之一，认为它不在外接圆内
        if (point - a).length_sq() < epsilon
            || (point - b).length_sq() < epsilon
            || (point - c).length_sq() < epsilon
        {
            return false;
        }

        // 应用更稳定的外接圆测试
        // 使用相对坐标减少数值误差
        let ax = a.x - point.x;
        let ay = a.y - point.y;
        let bx = b.x - point.x;
        let by = b.y - point.y;
        let cx = c.x - point.x;
        let cy = c.y - point.y;

        let a_squared = ax * ax + ay * ay;
        let b_squared = bx * bx + by * by;
        let c_squared = cx * cx + cy * cy;

        // 行列式计算
        let det = ax * (by * c_squared - cy * b_squared)
            + bx * (cy * a_squared - ay * c_squared)
            + cx * (ay * b_squared - by * a_squared);

        // 判断三角形方向
        if area > 0.0 {
            det > epsilon
        } else {
            det < -epsilon
        }
    }

    /// 检查点是否在三角形内部
    pub fn contains_point(&self, point: Pos2) -> bool {
        let a = self.points[0];
        let b = self.points[1];
        let c = self.points[2];

        // 使用重心坐标判断
        let area = 0.5 * ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs();

        // 处理面积接近零的情况（即三角形退化的情况）
        if area < 1e-10 {
            return false;
        }

        let s = 1.0 / (2.0 * area)
            * (a.y * c.x - a.x * c.y + (c.y - a.y) * point.x + (a.x - c.x) * point.y);
        let t = 1.0 / (2.0 * area)
            * (a.x * b.y - a.y * b.x + (a.y - b.y) * point.x + (b.x - a.x) * point.y);

        let u = 1.0 - s - t;

        s >= 0.0 && t >= 0.0 && u >= 0.0
    }
}
