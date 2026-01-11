//! 三角形数据结构和几何计算
//!
//! 提供三角形的基本表示和 Delaunay 验证所需的外接圆测试。

use egui::Pos2;

/// 三角形结构，存储三个顶点坐标
///
/// # 用途
/// - Delaunay 三角剖分的基本单元
/// - 验证三角剖分是否满足 Delaunay 性质
#[derive(Debug, Clone, Copy)]
pub struct Triangle {
    /// 三角形的三个顶点，按逆时针或顺时针顺序存储
    pub points: [Pos2; 3],
}

impl Triangle {
    /// 创建新的三角形
    ///
    /// # 参数
    /// - `points`: 三个顶点坐标
    ///
    /// # 示例
    /// ```ignore
    /// let triangle = Triangle::new([
    ///     Pos2::new(0.0, 0.0),
    ///     Pos2::new(1.0, 0.0),
    ///     Pos2::new(0.5, 1.0),
    /// ]);
    /// ```
    pub fn new(points: [Pos2; 3]) -> Self {
        Self { points }
    }

    /// 判断一个点是否在三角形的外接圆内
    ///
    /// 这是 Delaunay 三角剖分的核心验证条件：
    /// 任意三角形的外接圆内不应包含其他点。
    ///
    /// # 算法
    /// 使用行列式方法判断点相对于外接圆的位置。
    /// 通过计算 4x4 行列式的符号来确定点是否在圆内。
    ///
    /// # 参数
    /// - `point`: 待测试的点
    ///
    /// # 返回值
    /// - `true`: 点在外接圆内（严格内部）
    /// - `false`: 点在外接圆上或外部，或三角形退化
    ///
    /// # 数值稳定性
    /// - 使用相对坐标减少浮点误差
    /// - 设置容差阈值处理边界情况
    pub fn contains_in_circumcircle(&self, point: Pos2) -> bool {
        let a = self.points[0];
        let b = self.points[1];
        let c = self.points[2];

        const EPSILON: f32 = 1e-10;

        // 计算三角形有向面积（判断顶点顺序）
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);

        // 退化三角形（面积接近零）
        if area.abs() < EPSILON {
            return false;
        }

        // 点与三角形顶点重合
        if (point - a).length_sq() < EPSILON
            || (point - b).length_sq() < EPSILON
            || (point - c).length_sq() < EPSILON
        {
            return false;
        }

        // 使用相对坐标提高数值稳定性
        let ax = a.x - point.x;
        let ay = a.y - point.y;
        let bx = b.x - point.x;
        let by = b.y - point.y;
        let cx = c.x - point.x;
        let cy = c.y - point.y;

        let a_sq = ax * ax + ay * ay;
        let b_sq = bx * bx + by * by;
        let c_sq = cx * cx + cy * cy;

        // 行列式计算：
        // | ax  ay  ax²+ay² |
        // | bx  by  bx²+by² |
        // | cx  cy  cx²+cy² |
        let det = ax * (by * c_sq - cy * b_sq)
            + bx * (cy * a_sq - ay * c_sq)
            + cx * (ay * b_sq - by * a_sq);

        // 根据三角形方向判断
        if area > 0.0 {
            det > EPSILON
        } else {
            det < -EPSILON
        }
    }

    /// 获取三角形的有向面积
    ///
    /// 正值表示顶点按逆时针排列，负值表示顺时针排列。
    /// 绝对值的一半是三角形的实际面积。
    #[inline]
    pub fn signed_area(&self) -> f32 {
        let a = self.points[0];
        let b = self.points[1];
        let c = self.points[2];
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }

    /// 判断三角形是否退化（三点共线或重合）
    #[inline]
    pub fn is_degenerate(&self) -> bool {
        self.signed_area().abs() < 1e-10
    }
}

// ============================================================================
// 废弃代码 - 保留供参考
// ============================================================================

#[cfg(feature = "deprecated")]
impl Triangle {
    /// 检查点是否在三角形内部
    ///
    /// **注意**: 此方法当前未被使用，保留供将来可能的需求。
    ///
    /// # 算法
    /// 使用重心坐标判断：如果点的三个重心坐标都非负，则点在三角形内部。
    ///
    /// # 参数
    /// - `point`: 待测试的点
    ///
    /// # 返回值
    /// - `true`: 点在三角形内部或边上
    /// - `false`: 点在三角形外部
    pub fn contains_point(&self, point: Pos2) -> bool {
        let a = self.points[0];
        let b = self.points[1];
        let c = self.points[2];

        // 计算三角形面积
        let area = 0.5 * ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs();

        // 退化三角形
        if area < 1e-10 {
            return false;
        }

        // 计算重心坐标
        let inv_2area = 1.0 / (2.0 * area);
        let s = inv_2area
            * (a.y * c.x - a.x * c.y + (c.y - a.y) * point.x + (a.x - c.x) * point.y);
        let t = inv_2area
            * (a.x * b.y - a.y * b.x + (a.y - b.y) * point.x + (b.x - a.x) * point.y);
        let u = 1.0 - s - t;

        s >= 0.0 && t >= 0.0 && u >= 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circumcircle_inside() {
        let triangle = Triangle::new([
            Pos2::new(0.0, 0.0),
            Pos2::new(2.0, 0.0),
            Pos2::new(1.0, 2.0),
        ]);

        // 外接圆圆心附近的点应该在圆内
        let center = Pos2::new(1.0, 0.75);
        assert!(triangle.contains_in_circumcircle(center));
    }

    #[test]
    fn test_circumcircle_outside() {
        let triangle = Triangle::new([
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(0.5, 1.0),
        ]);

        // 远离三角形的点应该在外接圆外
        let far_point = Pos2::new(10.0, 10.0);
        assert!(!triangle.contains_in_circumcircle(far_point));
    }

    #[test]
    fn test_degenerate_triangle() {
        // 共线的三个点
        let degenerate = Triangle::new([
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(2.0, 0.0),
        ]);

        assert!(degenerate.is_degenerate());
        assert!(!degenerate.contains_in_circumcircle(Pos2::new(1.0, 1.0)));
    }

    #[test]
    fn test_signed_area() {
        // 逆时针三角形
        let ccw = Triangle::new([
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(0.0, 1.0),
        ]);
        assert!(ccw.signed_area() > 0.0);

        // 顺时针三角形
        let cw = Triangle::new([
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 1.0),
            Pos2::new(1.0, 0.0),
        ]);
        assert!(cw.signed_area() < 0.0);
    }
}
