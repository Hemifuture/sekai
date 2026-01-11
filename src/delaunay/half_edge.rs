//! 半边数据结构 (Half-Edge / DCEL)
//!
//! 本模块提供高效的网格拓扑表示，支持：
//! - O(1) 邻接三角形查询
//! - O(k) 顶点周围遍历（k = 相邻边数）
//! - Voronoi 单元格顶点自然有序
//!
//! # 核心概念
//!
//! 半边数据结构将每条边拆分为两个有方向的"半边"：
//!
//! ```text
//!        传统边                    半边表示
//!      
//!     A ←──────→ B           A ─────→ B   (半边 e1)
//!                            A ←───── B   (半边 e2，e1 的 twin)
//! ```
//!
//! # 与 delaunator 的关系
//!
//! 本模块直接利用 `delaunator` 库的输出：
//! - `triangles[i]` = 半边 i 的起点
//! - `halfedges[i]` = 半边 i 的对偶半边（twin）
//! - 三角形 t 的三条半边索引为 `3*t`, `3*t+1`, `3*t+2`

use egui::Pos2;

// ============================================================================
// 常量
// ============================================================================

/// 无效索引标记（对应 delaunator::EMPTY）
pub const EMPTY: u32 = u32::MAX;

// ============================================================================
// 核心数据结构
// ============================================================================

/// Delaunay 网格（半边表示）
///
/// 包含完整的拓扑信息，支持高效的邻接查询和遍历。
#[derive(Debug, Clone)]
pub struct DelaunayMesh {
    /// 所有顶点坐标
    pub points: Vec<Pos2>,

    /// 半边数组：halfedges[i] 存储半边 i 的对偶半边索引
    /// 如果 halfedges[i] == EMPTY，则半边 i 在凸包边界上
    pub halfedges: Vec<u32>,

    /// 三角形顶点索引：triangles[i] 是半边 i 的起点
    /// 每3个连续索引构成一个三角形
    pub triangles: Vec<u32>,

    /// 每个顶点的一条出边索引
    /// vertex_to_halfedge[v] 返回从顶点 v 出发的任意一条半边
    pub vertex_to_halfedge: Vec<u32>,

    /// 凸包顶点索引（逆时针顺序）
    pub hull: Vec<u32>,
}

impl Default for DelaunayMesh {
    fn default() -> Self {
        Self::new()
    }
}

impl DelaunayMesh {
    /// 创建空的网格
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            halfedges: Vec::new(),
            triangles: Vec::new(),
            vertex_to_halfedge: Vec::new(),
            hull: Vec::new(),
        }
    }

    /// 从 delaunator 结果构建半边网格
    ///
    /// # 参数
    /// - `points`: 原始点坐标
    /// - `triangulation`: delaunator 的三角剖分结果
    ///
    /// # 返回值
    /// 构建好的半边网格
    pub fn from_delaunator(points: Vec<Pos2>, triangulation: &delaunator::Triangulation) -> Self {
        let n_points = points.len();

        // 转换索引类型
        let triangles: Vec<u32> = triangulation.triangles.iter().map(|&i| i as u32).collect();

        let halfedges: Vec<u32> = triangulation
            .halfedges
            .iter()
            .map(|&i| {
                if i == delaunator::EMPTY {
                    EMPTY
                } else {
                    i as u32
                }
            })
            .collect();

        let hull: Vec<u32> = triangulation.hull.iter().map(|&i| i as u32).collect();

        // 构建顶点到半边的映射
        let mut vertex_to_halfedge = vec![EMPTY; n_points];
        for (he_idx, &vertex_idx) in triangles.iter().enumerate() {
            if vertex_to_halfedge[vertex_idx as usize] == EMPTY {
                vertex_to_halfedge[vertex_idx as usize] = he_idx as u32;
            }
        }

        Self {
            points,
            halfedges,
            triangles,
            vertex_to_halfedge,
            hull,
        }
    }

    // ========================================================================
    // 基本查询
    // ========================================================================

    /// 获取三角形数量
    #[inline]
    pub fn triangle_count(&self) -> usize {
        self.triangles.len() / 3
    }

    /// 获取半边数量
    #[inline]
    pub fn halfedge_count(&self) -> usize {
        self.triangles.len()
    }

    /// 获取顶点数量
    #[inline]
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// 获取半边的起点索引
    #[inline]
    pub fn halfedge_start(&self, he: u32) -> u32 {
        self.triangles[he as usize]
    }

    /// 获取半边的终点索引
    #[inline]
    pub fn halfedge_end(&self, he: u32) -> u32 {
        self.triangles[Self::next_halfedge(he) as usize]
    }

    /// 获取半边的对偶半边
    #[inline]
    pub fn twin(&self, he: u32) -> u32 {
        self.halfedges[he as usize]
    }

    /// 获取同一三角形内的下一条半边（逆时针）
    #[inline]
    pub fn next_halfedge(he: u32) -> u32 {
        if he % 3 == 2 {
            he - 2
        } else {
            he + 1
        }
    }

    /// 获取同一三角形内的上一条半边（顺时针）
    #[inline]
    pub fn prev_halfedge(he: u32) -> u32 {
        if he % 3 == 0 {
            he + 2
        } else {
            he - 1
        }
    }

    /// 获取半边所属的三角形索引
    #[inline]
    pub fn triangle_of_halfedge(he: u32) -> u32 {
        he / 3
    }

    /// 获取三角形的第一条半边索引
    #[inline]
    pub fn halfedge_of_triangle(tri: u32) -> u32 {
        tri * 3
    }

    /// 检查半边是否在凸包边界上
    #[inline]
    pub fn is_boundary(&self, he: u32) -> bool {
        self.halfedges[he as usize] == EMPTY
    }

    // ========================================================================
    // 三角形操作
    // ========================================================================

    /// 获取三角形的三个顶点索引
    pub fn triangle_vertices(&self, tri: u32) -> [u32; 3] {
        let base = (tri * 3) as usize;
        [
            self.triangles[base],
            self.triangles[base + 1],
            self.triangles[base + 2],
        ]
    }

    /// 获取三角形的三个顶点坐标
    pub fn triangle_points(&self, tri: u32) -> [Pos2; 3] {
        let [i, j, k] = self.triangle_vertices(tri);
        [
            self.points[i as usize],
            self.points[j as usize],
            self.points[k as usize],
        ]
    }

    /// 计算三角形的外心（Voronoi 顶点）
    pub fn circumcenter(&self, tri: u32) -> Pos2 {
        let [a, b, c] = self.triangle_points(tri);
        compute_circumcenter(a, b, c)
    }

    /// 获取三角形的邻接三角形
    ///
    /// 返回与该三角形共享边的三角形索引列表（最多3个）
    pub fn adjacent_triangles(&self, tri: u32) -> Vec<u32> {
        let base = tri * 3;
        let mut neighbors = Vec::with_capacity(3);

        for i in 0..3 {
            let twin = self.halfedges[(base + i) as usize];
            if twin != EMPTY {
                neighbors.push(Self::triangle_of_halfedge(twin));
            }
        }

        neighbors
    }

    // ========================================================================
    // 顶点周围遍历
    // ========================================================================

    /// 遍历顶点周围的所有出边（半边）
    ///
    /// 返回从顶点 v 出发的所有半边索引，按逆时针顺序排列。
    /// 这是实现 Voronoi 单元格有序遍历的关键。
    ///
    /// # 算法
    /// 1. 从 vertex_to_halfedge[v] 开始
    /// 2. 跳到 prev_halfedge，然后跳到 twin
    /// 3. 重复直到回到起点或遇到边界
    ///
    /// # 返回值
    /// - `(halfedges, is_closed)`: 半边列表和是否形成闭环
    pub fn edges_around_vertex(&self, v: u32) -> (Vec<u32>, bool) {
        let start = self.vertex_to_halfedge[v as usize];
        if start == EMPTY {
            return (Vec::new(), true);
        }

        let mut edges = Vec::new();
        let mut current = start;

        loop {
            edges.push(current);

            // 移动到下一条出边：先到前一条半边，再到对偶
            let prev = Self::prev_halfedge(current);
            let twin = self.twin(prev);

            if twin == EMPTY {
                // 遇到边界，需要从另一个方向遍历
                break;
            }

            current = twin;
            if current == start {
                // 回到起点，形成闭环
                return (edges, true);
            }
        }

        // 顶点在边界上，需要从起点向另一个方向遍历
        let twin_start = self.twin(start);
        if twin_start != EMPTY {
            current = Self::next_halfedge(twin_start);

            while current != start {
                let twin = self.twin(current);
                if twin == EMPTY {
                    break;
                }
                current = Self::next_halfedge(twin);
                edges.insert(0, current); // 插入到开头保持顺序
            }
        }

        (edges, false)
    }

    /// 遍历顶点周围的所有三角形
    ///
    /// 返回包含顶点 v 的所有三角形索引，按逆时针顺序排列。
    pub fn triangles_around_vertex(&self, v: u32) -> Vec<u32> {
        let (edges, _) = self.edges_around_vertex(v);
        edges
            .iter()
            .map(|&he| Self::triangle_of_halfedge(he))
            .collect()
    }

    /// 获取顶点对应的 Voronoi 单元格顶点（有序）
    ///
    /// 返回 Voronoi 单元格的顶点坐标，按逆时针顺序排列。
    /// 每个 Voronoi 顶点是相邻三角形的外心。
    ///
    /// # 返回值
    /// - `(vertices, is_closed)`: 顶点坐标列表和是否形成闭合多边形
    pub fn voronoi_cell_vertices(&self, v: u32) -> (Vec<Pos2>, bool) {
        let (edges, is_closed) = self.edges_around_vertex(v);

        let vertices: Vec<Pos2> = edges
            .iter()
            .map(|&he| {
                let tri = Self::triangle_of_halfedge(he);
                self.circumcenter(tri)
            })
            .collect();

        (vertices, is_closed)
    }

    // ========================================================================
    // 用于渲染的数据生成
    // ========================================================================

    /// 生成 Delaunay 三角形的边索引（用于线框渲染）
    ///
    /// 返回边索引列表，每2个索引构成一条边。
    pub fn delaunay_edge_indices(&self) -> Vec<u32> {
        let mut edges = Vec::with_capacity(self.halfedge_count());
        let mut visited = vec![false; self.halfedge_count()];

        for he in 0..self.halfedge_count() as u32 {
            if visited[he as usize] {
                continue;
            }

            let twin = self.twin(he);
            if twin != EMPTY {
                visited[twin as usize] = true;
            }

            edges.push(self.halfedge_start(he));
            edges.push(self.halfedge_end(he));
        }

        edges
    }

    /// 生成 Voronoi 边索引和顶点（用于渲染）
    ///
    /// # 返回值
    /// - `(vertices, indices)`: Voronoi 顶点列表和边索引列表
    pub fn voronoi_render_data(&self) -> (Vec<Pos2>, Vec<u32>) {
        // 预计算所有外心
        let circumcenters: Vec<Pos2> = (0..self.triangle_count() as u32)
            .map(|tri| self.circumcenter(tri))
            .collect();

        let mut edges = Vec::new();

        // 遍历所有半边，只处理 he < twin 的情况避免重复
        for he in 0..self.halfedge_count() as u32 {
            let twin = self.twin(he);

            // 跳过边界边和已处理的边
            if twin == EMPTY || he > twin {
                continue;
            }

            let tri1 = Self::triangle_of_halfedge(he);
            let tri2 = Self::triangle_of_halfedge(twin);

            edges.push(tri1);
            edges.push(tri2);
        }

        (circumcenters, edges)
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 计算三角形的外心
fn compute_circumcenter(a: Pos2, b: Pos2, c: Pos2) -> Pos2 {
    // 边的中点
    let ab_mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let bc_mid = Pos2::new((b.x + c.x) / 2.0, (b.y + c.y) / 2.0);

    // 边的法线方向（垂直于边）
    let ab_normal = Pos2::new(-(b.y - a.y), b.x - a.x);
    let bc_normal = Pos2::new(-(c.y - b.y), c.x - b.x);

    // 检查是否退化（两条法线平行）
    let det = ab_normal.x * bc_normal.y - ab_normal.y * bc_normal.x;
    if det.abs() < 1e-10 {
        // 退化三角形，返回重心
        return Pos2::new((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0);
    }

    // 求解交点
    let t = ((bc_mid.x - ab_mid.x) * bc_normal.y - (bc_mid.y - ab_mid.y) * bc_normal.x) / det;

    Pos2::new(ab_mid.x + t * ab_normal.x, ab_mid.y + t * ab_normal.y)
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_mesh() -> DelaunayMesh {
        // 创建一个简单的正方形点集
        let points = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 0.0),
            Pos2::new(10.0, 10.0),
            Pos2::new(0.0, 10.0),
        ];

        // 使用 delaunator 进行三角剖分
        let delaunay_points: Vec<delaunator::Point> = points
            .iter()
            .map(|p| delaunator::Point {
                x: p.x as f64,
                y: p.y as f64,
            })
            .collect();

        let triangulation = delaunator::triangulate(&delaunay_points);
        DelaunayMesh::from_delaunator(points, &triangulation)
    }

    #[test]
    fn test_mesh_creation() {
        let mesh = create_test_mesh();

        assert_eq!(mesh.point_count(), 4);
        assert_eq!(mesh.triangle_count(), 2); // 正方形分成2个三角形
        assert_eq!(mesh.halfedge_count(), 6); // 2 个三角形 × 3 条边
    }

    #[test]
    fn test_halfedge_navigation() {
        let mesh = create_test_mesh();

        // 测试 next_halfedge
        assert_eq!(DelaunayMesh::next_halfedge(0), 1);
        assert_eq!(DelaunayMesh::next_halfedge(1), 2);
        assert_eq!(DelaunayMesh::next_halfedge(2), 0);
        assert_eq!(DelaunayMesh::next_halfedge(3), 4);

        // 测试 prev_halfedge
        assert_eq!(DelaunayMesh::prev_halfedge(0), 2);
        assert_eq!(DelaunayMesh::prev_halfedge(1), 0);
        assert_eq!(DelaunayMesh::prev_halfedge(2), 1);
    }

    #[test]
    fn test_triangle_of_halfedge() {
        assert_eq!(DelaunayMesh::triangle_of_halfedge(0), 0);
        assert_eq!(DelaunayMesh::triangle_of_halfedge(1), 0);
        assert_eq!(DelaunayMesh::triangle_of_halfedge(2), 0);
        assert_eq!(DelaunayMesh::triangle_of_halfedge(3), 1);
        assert_eq!(DelaunayMesh::triangle_of_halfedge(4), 1);
        assert_eq!(DelaunayMesh::triangle_of_halfedge(5), 1);
    }

    #[test]
    fn test_edges_around_vertex() {
        let mesh = create_test_mesh();

        // 测试每个顶点周围的边
        for v in 0..mesh.point_count() as u32 {
            let (edges, _is_closed) = mesh.edges_around_vertex(v);
            assert!(!edges.is_empty(), "顶点 {} 应该有出边", v);

            // 验证每条边的起点都是 v
            for &he in &edges {
                assert_eq!(
                    mesh.halfedge_start(he),
                    v,
                    "半边 {} 的起点应该是 {}",
                    he,
                    v
                );
            }
        }
    }

    #[test]
    fn test_voronoi_cell_vertices() {
        let mesh = create_test_mesh();

        // 测试每个顶点的 Voronoi 单元格
        for v in 0..mesh.point_count() as u32 {
            let (vertices, _is_closed) = mesh.voronoi_cell_vertices(v);

            // 正方形的情况下，每个顶点应该有 1-2 个 Voronoi 顶点
            assert!(
                !vertices.is_empty(),
                "顶点 {} 的 Voronoi 单元格应该有顶点",
                v
            );
        }
    }

    #[test]
    fn test_voronoi_render_data() {
        let mesh = create_test_mesh();
        let (vertices, indices) = mesh.voronoi_render_data();

        // 应该有2个外心（2个三角形）
        assert_eq!(vertices.len(), 2);

        // 共享边只有1条，所以只有1条 Voronoi 边
        assert_eq!(indices.len(), 2); // 1 条边 × 2 个索引
    }

    #[test]
    fn test_larger_mesh() {
        // 创建更大的测试网格
        let mut points = Vec::new();
        for i in 0..5 {
            for j in 0..5 {
                points.push(Pos2::new(i as f32 * 10.0, j as f32 * 10.0));
            }
        }

        let delaunay_points: Vec<delaunator::Point> = points
            .iter()
            .map(|p| delaunator::Point {
                x: p.x as f64,
                y: p.y as f64,
            })
            .collect();

        let triangulation = delaunator::triangulate(&delaunay_points);
        let mesh = DelaunayMesh::from_delaunator(points, &triangulation);

        assert_eq!(mesh.point_count(), 25);

        // 验证所有顶点都能正确遍历
        for v in 0..mesh.point_count() as u32 {
            let triangles = mesh.triangles_around_vertex(v);
            assert!(!triangles.is_empty(), "顶点 {} 应该有相邻三角形", v);
        }

        // 验证 Voronoi 渲染数据
        let (voronoi_vertices, voronoi_indices) = mesh.voronoi_render_data();
        assert_eq!(voronoi_vertices.len(), mesh.triangle_count());
        assert!(voronoi_indices.len() > 0, "应该有 Voronoi 边");
    }
}
