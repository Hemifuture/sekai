use egui::Pos2;

/// 三角形结构，存储三个顶点坐标
#[derive(Debug)]
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

        // 使用行列式判断点是否在外接圆内
        // 计算公式:
        // | (ax-px)² + (ay-py)² | ax-px | ay-py | 1 |
        // | (bx-px)² + (by-py)² | bx-px | by-py | 1 |
        // | (cx-px)² + (cy-py)² | cx-px | cy-py | 1 |

        // 为了数值稳定性，如果点非常接近三角形的顶点之一，认为它不在外接圆内
        let epsilon = 1e-6;

        let px = point.x;
        let py = point.y;

        let ax = a.x - px;
        let ay = a.y - py;
        let bx = b.x - px;
        let by = b.y - py;
        let cx = c.x - px;
        let cy = c.y - py;

        // 检查点是否与三角形的任何顶点重合或非常接近
        if (ax.abs() < epsilon && ay.abs() < epsilon)
            || (bx.abs() < epsilon && by.abs() < epsilon)
            || (cx.abs() < epsilon && cy.abs() < epsilon)
        {
            return false;
        }

        let a_squared = ax * ax + ay * ay;
        let b_squared = bx * bx + by * by;
        let c_squared = cx * cx + cy * cy;

        let det = ax * (by * c_squared - b_squared * cy) - ay * (bx * c_squared - b_squared * cx)
            + a_squared * (bx * cy - by * cx)
            - bx * (ax * c_squared - a_squared * cx)
            + by * (ax * b_squared - a_squared * bx)
            - b_squared * (ax * cy - ay * cx);

        // 判断三角形方向：如果三角形为顺时针方向，则结果要取反
        // 计算三角形面积的两倍（使用叉积）
        let area2 = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);

        // 添加数值容差
        if area2.abs() < epsilon {
            // 三角形退化，面积接近零
            return false;
        }

        if area2 > 0.0 {
            det > epsilon // 为正值添加容差
        } else {
            det < -epsilon // 为负值添加容差
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
