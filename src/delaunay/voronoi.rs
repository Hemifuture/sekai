//! Voronoi 图生成模块
//!
//! 基于 Delaunay 三角剖分生成 Voronoi 图。
//! Voronoi 图是 Delaunay 图的对偶图，每个 Delaunay 三角形的外心
//! 成为 Voronoi 图的顶点，共享边的三角形外心之间形成 Voronoi 边。
//!
//! # 主要类型
//! - [`IndexedVoronoiDiagram`]: 索引化的 Voronoi 图，用于高效存储和渲染
//! - [`VoronoiCell`]: Voronoi 单元格，对应原始点集中的一个点
//!
//! # 索引类型
//! 所有索引均使用 `u32` 类型，原因：
//! - 内存占用减少 50%（64位系统）
//! - GPU 索引缓冲区原生支持
//! - 支持最多 40 亿个点，远超实际需求
//!
//! # 使用示例
//! ```ignore
//! use crate::delaunay::{triangulate, voronoi::compute_indexed_voronoi};
//!
//! let points = vec![...];
//! let triangle_indices = triangulate(&points);
//! let voronoi = compute_indexed_voronoi(&triangle_indices, &points);
//!
//! // 用于渲染
//! let (vertices, indices) = voronoi.get_render_data();
//! ```

use egui::Pos2;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ============================================================================
// 公开类型定义
// ============================================================================

/// Voronoi 边的内部表示
///
/// 存储边的几何信息和拓扑关系。所有索引使用 `u32` 类型。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoronoiEdge {
    /// 边起点在 vertices 数组中的索引
    pub start_idx: u32,
    /// 边终点在 vertices 数组中的索引
    pub end_idx: u32,
    /// 相邻的原始点索引（边左侧的单元格）
    pub site1: u32,
    /// 相邻的原始点索引（边右侧的单元格）
    pub site2: u32,
}

/// Voronoi 单元格
///
/// 表示原始点集中一个点对应的 Voronoi 多边形区域。
/// 单元格内的所有位置到该点的距离都比到其他点更近。
/// 所有索引使用 `u32` 类型。
#[derive(Debug, Clone)]
pub struct VoronoiCell {
    /// 单元格对应的原始点索引
    pub site_idx: u32,
    /// 单元格顶点的索引列表
    ///
    /// **注意**: 顶点已按照相对于质心的角度逆时针排序，
    /// 形成闭合多边形，可直接用于填充渲染。
    pub vertex_indices: Vec<u32>,
}

/// 索引化的 Voronoi 图
///
/// 使用索引而非直接存储坐标，减少内存占用并便于 GPU 渲染。
/// 这是推荐使用的 Voronoi 图表示方式。
/// 所有索引使用 `u32` 类型。
///
/// # 内存布局
/// - `vertices`: 所有唯一顶点坐标，约 N 个（N 为三角形数量）
/// - `indices`: 边索引（u32），每2个索引构成一条边
/// - `cells`: 单元格信息，与原始点一一对应
#[derive(Debug, Clone)]
pub struct IndexedVoronoiDiagram {
    /// 所有唯一的 Voronoi 顶点坐标（三角形外心）
    pub vertices: Vec<Pos2>,
    /// 边的索引数组（u32），每两个连续索引表示一条边 [start, end, start, end, ...]
    pub indices: Vec<u32>,
    /// 所有 Voronoi 单元格，索引与原始点对应
    pub cells: Vec<VoronoiCell>,
}

impl Default for IndexedVoronoiDiagram {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexedVoronoiDiagram {
    /// 创建一个空的 Voronoi 图
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            cells: Vec::new(),
        }
    }

    /// 获取用于 GPU 渲染的数据
    ///
    /// 返回顶点坐标和边索引，可直接用于 wgpu 线段渲染。
    /// 索引类型为 `u32`，与 GPU 索引缓冲区兼容。
    ///
    /// # 返回值
    /// - `(vertices, indices)`: 顶点数组和索引数组的克隆
    pub fn get_render_data(&self) -> (Vec<Pos2>, Vec<u32>) {
        (self.vertices.clone(), self.indices.clone())
    }

    /// 获取边的数量
    pub fn edge_count(&self) -> usize {
        self.indices.len() / 2
    }

    /// 获取顶点数量
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// 获取单元格数量
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

// ============================================================================
// 公开 API
// ============================================================================

/// 从 Delaunay 三角剖分计算索引化的 Voronoi 图
///
/// 这是生成 Voronoi 图的主要函数。
///
/// # 算法流程
/// 1. 计算每个三角形的外心（作为 Voronoi 顶点）
/// 2. 找出共享边的三角形对
/// 3. 连接共享边对应的两个外心形成 Voronoi 边
/// 4. 收集每个原始点关联的 Voronoi 顶点
///
/// # 参数
/// - `triangle_indices`: Delaunay 三角形索引（`&[u32]`），每3个索引构成一个三角形
/// - `points`: 原始点坐标
///
/// # 返回值
/// 索引化的 Voronoi 图结构，所有索引使用 `u32` 类型
///
/// # 性能
/// - 使用 rayon 并行计算外心
/// - 10000 点约需 100-200ms
pub fn compute_indexed_voronoi(triangle_indices: &[u32], points: &[Pos2]) -> IndexedVoronoiDiagram {
    #[cfg(debug_assertions)]
    let start_time = std::time::Instant::now();

    #[cfg(debug_assertions)]
    println!(
        "开始生成索引化 Voronoi 图，基于 {} 个三角形",
        triangle_indices.len() / 3
    );

    // 输入验证
    if triangle_indices.len() < 3 || points.len() < 3 {
        return IndexedVoronoiDiagram::new();
    }

    // Step 1: 构建三角形索引列表
    let triangles: Vec<[u32; 3]> = build_triangle_list(triangle_indices);

    // Step 2: 并行计算每个三角形的外心
    let circumcenters: Vec<Pos2> = triangles
        .par_iter()
        .map(|indices| compute_circumcenter(points, indices))
        .collect();

    // Step 3: 构建边到三角形的映射
    let edge_to_triangles = build_edge_triangle_map(&triangles);

    // Step 4: 生成 Voronoi 边和顶点
    let (vertices, edges, site_vertices) =
        generate_voronoi_geometry(&circumcenters, &edge_to_triangles);

    // Step 5: 构建索引数组
    let indices = edges_to_indices(&edges);

    // Step 6: 构建单元格
    let mut cells = build_cells(points.len(), &site_vertices);

    // Step 7: 对每个单元格的顶点进行排序，使其形成闭合多边形
    for cell in cells.iter_mut() {
        let site_pos = points[cell.site_idx as usize];
        sort_cell_vertices(cell, &vertices, site_pos);
    }

    #[cfg(debug_assertions)]
    {
        let duration = start_time.elapsed();
        println!(
            "Voronoi 图生成完成: {} 顶点, {} 边, {} 单元格, 耗时 {:.2?}",
            vertices.len(),
            edges.len(),
            cells.len(),
            duration
        );
    }

    IndexedVoronoiDiagram {
        vertices,
        indices,
        cells,
    }
}

/// 生成用于渲染的 Voronoi 数据
///
/// 这是一个便捷函数，直接返回渲染所需的顶点和索引。
///
/// # 参数
/// - `triangle_indices`: Delaunay 三角形索引（`&[u32]`）
/// - `points`: 原始点坐标
///
/// # 返回值
/// `(vertices, indices)` 元组，可直接用于 GPU 渲染
/// 索引类型为 `u32`
pub fn generate_voronoi_render_data(
    triangle_indices: &[u32],
    points: &[Pos2],
) -> (Vec<Pos2>, Vec<u32>) {
    compute_indexed_voronoi(triangle_indices, points).get_render_data()
}

// ============================================================================
// 内部实现
// ============================================================================

/// 顶点坐标量化精度（用于去重）
const VERTEX_QUANTIZATION: f64 = 10000.0;

/// 构建三角形索引列表
fn build_triangle_list(triangle_indices: &[u32]) -> Vec<[u32; 3]> {
    (0..triangle_indices.len() / 3)
        .map(|i| {
            [
                triangle_indices[i * 3],
                triangle_indices[i * 3 + 1],
                triangle_indices[i * 3 + 2],
            ]
        })
        .collect()
}

/// 构建边到三角形的映射表
///
/// 返回的 HashMap 中，每个键是有序的边 (min_idx, max_idx)，
/// 值是使用该边的三角形索引列表。
fn build_edge_triangle_map(triangles: &[[u32; 3]]) -> HashMap<(u32, u32), Vec<u32>> {
    let mut edge_to_triangles: HashMap<(u32, u32), Vec<u32>> = HashMap::new();

    for (t_idx, indices) in triangles.iter().enumerate() {
        for i in 0..3 {
            let j = (i + 1) % 3;
            // 确保边的索引有序（小的在前）
            let edge = if indices[i] < indices[j] {
                (indices[i], indices[j])
            } else {
                (indices[j], indices[i])
            };
            edge_to_triangles
                .entry(edge)
                .or_default()
                .push(t_idx as u32);
        }
    }

    edge_to_triangles
}

/// 生成 Voronoi 几何数据
///
/// 返回：(顶点列表, 边列表, 每个原始点对应的 Voronoi 顶点集合)
fn generate_voronoi_geometry(
    circumcenters: &[Pos2],
    edge_to_triangles: &HashMap<(u32, u32), Vec<u32>>,
) -> (Vec<Pos2>, Vec<VoronoiEdge>, HashMap<u32, HashSet<u32>>) {
    let mut vertex_map: HashMap<(i64, i64), u32> = HashMap::new();
    let mut vertices: Vec<Pos2> = Vec::new();
    let mut edges: Vec<VoronoiEdge> = Vec::new();
    let mut site_to_vertices: HashMap<u32, HashSet<u32>> = HashMap::new();

    for ((p1_idx, p2_idx), tri_indices) in edge_to_triangles.iter() {
        // 只处理内部边（被两个三角形共享的边）
        if tri_indices.len() != 2 {
            // 边界边（只有一个三角形使用）暂不处理
            // TODO: 可以考虑延伸到无穷远或裁剪到边界
            continue;
        }

        let t1_idx = tri_indices[0] as usize;
        let t2_idx = tri_indices[1] as usize;

        let start = circumcenters[t1_idx];
        let end = circumcenters[t2_idx];

        // 获取或创建顶点索引
        let start_idx = get_or_create_vertex(&mut vertex_map, &mut vertices, start);
        let end_idx = get_or_create_vertex(&mut vertex_map, &mut vertices, end);

        // 创建边
        edges.push(VoronoiEdge {
            start_idx,
            end_idx,
            site1: *p1_idx,
            site2: *p2_idx,
        });

        // 记录每个原始点关联的 Voronoi 顶点
        site_to_vertices
            .entry(*p1_idx)
            .or_default()
            .insert(start_idx);
        site_to_vertices.entry(*p1_idx).or_default().insert(end_idx);
        site_to_vertices
            .entry(*p2_idx)
            .or_default()
            .insert(start_idx);
        site_to_vertices.entry(*p2_idx).or_default().insert(end_idx);
    }

    (vertices, edges, site_to_vertices)
}

/// 获取或创建顶点索引
///
/// 使用量化坐标作为键来去除重复顶点。
fn get_or_create_vertex(
    vertex_map: &mut HashMap<(i64, i64), u32>,
    vertices: &mut Vec<Pos2>,
    pos: Pos2,
) -> u32 {
    let key = (
        (pos.x as f64 * VERTEX_QUANTIZATION).round() as i64,
        (pos.y as f64 * VERTEX_QUANTIZATION).round() as i64,
    );

    *vertex_map.entry(key).or_insert_with(|| {
        let idx = vertices.len() as u32;
        vertices.push(pos);
        idx
    })
}

/// 将边列表转换为索引数组
fn edges_to_indices(edges: &[VoronoiEdge]) -> Vec<u32> {
    let mut indices = Vec::with_capacity(edges.len() * 2);
    for edge in edges {
        indices.push(edge.start_idx);
        indices.push(edge.end_idx);
    }
    indices
}

/// 构建 Voronoi 单元格
fn build_cells(
    point_count: usize,
    site_to_vertices: &HashMap<u32, HashSet<u32>>,
) -> Vec<VoronoiCell> {
    (0..point_count as u32)
        .map(|i| VoronoiCell {
            site_idx: i,
            vertex_indices: site_to_vertices
                .get(&i)
                .map(|set| set.iter().copied().collect())
                .unwrap_or_default(),
        })
        .collect()
}

/// 对 Voronoi 单元格的顶点进行排序，使其形成闭合多边形
///
/// 顶点按照相对于质心的角度逆时针排序
fn sort_cell_vertices(cell: &mut VoronoiCell, vertices: &[Pos2], _site_pos: Pos2) {
    if cell.vertex_indices.len() < 3 {
        return;
    }

    // 计算质心
    let mut center = Pos2::ZERO;
    for &idx in &cell.vertex_indices {
        if (idx as usize) < vertices.len() {
            center += vertices[idx as usize].to_vec2();
        }
    }
    center = Pos2::new(
        center.x / cell.vertex_indices.len() as f32,
        center.y / cell.vertex_indices.len() as f32,
    );

    // 按照相对于质心的角度排序
    cell.vertex_indices.sort_by(|&a, &b| {
        if (a as usize) >= vertices.len() || (b as usize) >= vertices.len() {
            return std::cmp::Ordering::Equal;
        }

        let va = vertices[a as usize];
        let vb = vertices[b as usize];

        let angle_a = (va.y - center.y).atan2(va.x - center.x);
        let angle_b = (vb.y - center.y).atan2(vb.x - center.x);

        angle_a
            .partial_cmp(&angle_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// 计算三角形的外心
///
/// 外心是三角形外接圆的圆心，到三个顶点的距离相等。
fn compute_circumcenter(points: &[Pos2], indices: &[u32; 3]) -> Pos2 {
    let a = points[indices[0] as usize];
    let b = points[indices[1] as usize];
    let c = points[indices[2] as usize];

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

    // 求解 ab_mid + t * ab_normal = bc_mid + s * bc_normal
    let t = ((bc_mid.x - ab_mid.x) * bc_normal.y - (bc_mid.y - ab_mid.y) * bc_normal.x) / det;

    Pos2::new(ab_mid.x + t * ab_normal.x, ab_mid.y + t * ab_normal.y)
}

// ============================================================================
// 测试兼容性 API（仅用于测试，不推荐在生产代码中使用）
// ============================================================================

/// 生成 Voronoi 边的坐标对表示
///
/// **注意**: 此函数主要用于测试兼容性，生产代码应使用 `compute_indexed_voronoi`
/// 或 `generate_voronoi_render_data`。
#[cfg(test)]
pub fn generate_voronoi_edges(indices: &[u32], points: &[Pos2]) -> Vec<[Pos2; 2]> {
    let voronoi = compute_indexed_voronoi(indices, points);

    (0..voronoi.indices.len() / 2)
        .filter_map(|i| {
            let start_idx = voronoi.indices[i * 2] as usize;
            let end_idx = voronoi.indices[i * 2 + 1] as usize;

            if start_idx < voronoi.vertices.len() && end_idx < voronoi.vertices.len() {
                Some([voronoi.vertices[start_idx], voronoi.vertices[end_idx]])
            } else {
                None
            }
        })
        .collect()
}

/// 计算 Voronoi 图（旧版兼容 API）
///
/// **注意**: 此函数已废弃，仅用于测试兼容性。
/// 生产代码应使用 `compute_indexed_voronoi`。
#[cfg(test)]
pub fn compute_voronoi(triangle_indices: &[u32], points: &[Pos2]) -> VoronoiDiagram {
    let indexed = compute_indexed_voronoi(triangle_indices, points);

    let edges: Vec<OldVoronoiEdge> = (0..indexed.indices.len() / 2)
        .filter_map(|i| {
            let start_idx = indexed.indices[i * 2] as usize;
            let end_idx = indexed.indices[i * 2 + 1] as usize;

            if start_idx < indexed.vertices.len() && end_idx < indexed.vertices.len() {
                Some(OldVoronoiEdge {
                    start: indexed.vertices[start_idx],
                    end: indexed.vertices[end_idx],
                    site1: i % points.len(),
                    site2: (i + 1) % points.len(),
                })
            } else {
                None
            }
        })
        .collect();

    let cells: Vec<OldVoronoiCell> = indexed
        .cells
        .iter()
        .enumerate()
        .map(|(i, cell)| OldVoronoiCell {
            site: if i < points.len() {
                points[cell.site_idx as usize]
            } else {
                Pos2::ZERO
            },
            edges: Vec::new(),
            vertices: cell
                .vertex_indices
                .iter()
                .filter_map(|&idx| indexed.vertices.get(idx as usize).copied())
                .collect(),
        })
        .collect();

    VoronoiDiagram { cells, edges }
}

// ============================================================================
// 旧版类型定义（仅用于测试兼容性）
// ============================================================================

/// 旧版 Voronoi 边（仅用于测试）
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OldVoronoiEdge {
    pub start: Pos2,
    pub end: Pos2,
    pub site1: usize,
    pub site2: usize,
}

/// 旧版 Voronoi 单元格（仅用于测试）
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct OldVoronoiCell {
    pub site: Pos2,
    pub edges: Vec<OldVoronoiEdge>,
    pub vertices: Vec<Pos2>,
}

/// 旧版 Voronoi 图（仅用于测试）
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct VoronoiDiagram {
    pub cells: Vec<OldVoronoiCell>,
    pub edges: Vec<OldVoronoiEdge>,
}

#[cfg(test)]
impl VoronoiDiagram {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            edges: Vec::new(),
        }
    }
}
