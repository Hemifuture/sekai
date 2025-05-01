use egui::Pos2;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Voronoi边缘表示
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoronoiEdge {
    /// 边的起点索引
    pub start_idx: u32,
    /// 边的终点索引
    pub end_idx: u32,
    /// 相邻的Delaunay顶点索引（左侧）
    pub site1: usize,
    /// 相邻的Delaunay顶点索引（右侧）
    pub site2: usize,
}

/// Voronoi单元结构，表示一个点的Voronoi多边形
#[derive(Debug, Clone)]
pub struct VoronoiCell {
    /// 细胞中心（原始点）
    pub site: Pos2,
    /// 该单元格顶点的索引（有序）
    pub vertex_indices: Vec<u32>,
}

/// 索引化的Voronoi图结构
#[derive(Debug, Clone)]
pub struct IndexedVoronoiDiagram {
    /// 所有唯一的顶点坐标
    pub vertices: Vec<Pos2>,
    /// 边的索引，每两个索引表示一条边
    pub indices: Vec<u32>,
    /// 所有Voronoi单元格，按原始点索引排序
    pub cells: Vec<VoronoiCell>,
}

impl IndexedVoronoiDiagram {
    /// 创建一个空的Voronoi图
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            cells: Vec::new(),
        }
    }

    /// 获取用于渲染的顶点和索引
    pub fn get_render_data(&self) -> (Vec<Pos2>, Vec<u32>) {
        (self.vertices.clone(), self.indices.clone())
    }
}

/// 计算三角形的外心
fn compute_circumcenter(points: &[Pos2], indices: &[usize; 3]) -> Pos2 {
    let a = points[indices[0]];
    let b = points[indices[1]];
    let c = points[indices[2]];

    // 三角形的三条边的中点
    let ab_mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let bc_mid = Pos2::new((b.x + c.x) / 2.0, (b.y + c.y) / 2.0);

    // 三角形边的法线方向
    let ab_normal = Pos2::new(-(b.y - a.y), b.x - a.x);
    let bc_normal = Pos2::new(-(c.y - b.y), c.x - b.x);

    // 计算外心
    // 解方程:
    // ab_mid + t1 * ab_normal = bc_mid + t2 * bc_normal

    // 避免平行线导致的除零问题
    if (ab_normal.x * bc_normal.y - ab_normal.y * bc_normal.x).abs() < 1e-10 {
        // 如果三角形接近退化，返回重心
        return Pos2::new((a.x + b.x + c.x) / 3.0, (a.y + b.y + c.y) / 3.0);
    }

    // 计算参数t1
    let t1 = ((bc_mid.x - ab_mid.x) * bc_normal.y - (bc_mid.y - ab_mid.y) * bc_normal.x)
        / (ab_normal.x * bc_normal.y - ab_normal.y * bc_normal.x);

    // 计算外心坐标
    Pos2::new(ab_mid.x + t1 * ab_normal.x, ab_mid.y + t1 * ab_normal.y)
}

/// 从Delaunay三角剖分计算索引化的Voronoi图
pub fn compute_indexed_voronoi(triangle_indices: &[u32], points: &[Pos2]) -> IndexedVoronoiDiagram {
    let start_time = Instant::now();
    println!(
        "开始生成索引化Voronoi图，基于 {} 个三角形",
        triangle_indices.len() / 3
    );

    if triangle_indices.len() < 3 || points.len() < 3 {
        return IndexedVoronoiDiagram::new();
    }

    // 构建三角形顶点索引列表
    let triangles_idx: Vec<[usize; 3]> = (0..triangle_indices.len() / 3)
        .map(|i| {
            [
                triangle_indices[i * 3] as usize,
                triangle_indices[i * 3 + 1] as usize,
                triangle_indices[i * 3 + 2] as usize,
            ]
        })
        .collect();

    // 计算每个三角形的外心
    let circumcenters: Vec<Pos2> = triangles_idx
        .par_iter()
        .map(|indices| compute_circumcenter(points, indices))
        .collect();

    // 查找共享边的三角形
    // 为每条边创建一个键，映射到使用该边的三角形索引
    let mut edge_to_triangles: HashMap<(usize, usize), Vec<usize>> = HashMap::new();

    for (t_idx, indices) in triangles_idx.iter().enumerate() {
        for i in 0..3 {
            let j = (i + 1) % 3;
            // 确保边的索引是有序的(小的在前)
            let edge = if indices[i] < indices[j] {
                (indices[i], indices[j])
            } else {
                (indices[j], indices[i])
            };

            edge_to_triangles.entry(edge).or_default().push(t_idx);
        }
    }

    // 收集所有唯一的Voronoi顶点
    let mut vertex_map: HashMap<(i64, i64), u32> = HashMap::new();
    let mut vertices: Vec<Pos2> = Vec::new();
    let mut edges: Vec<VoronoiEdge> = Vec::new();

    // 用于记录每个原始点的Voronoi顶点
    let mut site_to_voronoi_vertices: HashMap<usize, HashSet<u32>> = HashMap::new();

    for ((p1_idx, p2_idx), triangle_indices) in edge_to_triangles.iter() {
        if triangle_indices.len() == 2 {
            // 内部边: 两个三角形共享这条边
            let t1_idx = triangle_indices[0];
            let t2_idx = triangle_indices[1];

            let start = circumcenters[t1_idx];
            let end = circumcenters[t2_idx];

            // 量化顶点坐标以确定唯一顶点
            let start_key = (
                (start.x * 10000.0).round() as i64,
                (start.y * 10000.0).round() as i64,
            );
            let end_key = (
                (end.x * 10000.0).round() as i64,
                (end.y * 10000.0).round() as i64,
            );

            // 获取或添加顶点索引
            let start_idx = match vertex_map.get(&start_key) {
                Some(&idx) => idx,
                None => {
                    let idx = vertices.len() as u32;
                    vertices.push(start);
                    vertex_map.insert(start_key, idx);
                    idx
                }
            };

            let end_idx = match vertex_map.get(&end_key) {
                Some(&idx) => idx,
                None => {
                    let idx = vertices.len() as u32;
                    vertices.push(end);
                    vertex_map.insert(end_key, idx);
                    idx
                }
            };

            // 添加边
            let edge = VoronoiEdge {
                start_idx,
                end_idx,
                site1: *p1_idx,
                site2: *p2_idx,
            };
            edges.push(edge);

            // 记录每个原始点的相关Voronoi顶点
            site_to_voronoi_vertices
                .entry(*p1_idx)
                .or_default()
                .insert(start_idx);
            site_to_voronoi_vertices
                .entry(*p1_idx)
                .or_default()
                .insert(end_idx);
            site_to_voronoi_vertices
                .entry(*p2_idx)
                .or_default()
                .insert(start_idx);
            site_to_voronoi_vertices
                .entry(*p2_idx)
                .or_default()
                .insert(end_idx);
        } else if triangle_indices.len() == 1 {
            // 边界边: 只有一个三角形使用这条边
            // 对于完整实现，这里需要处理边界情况
            // 当前版本简单忽略边界边
        }
    }

    // 构建索引数组，每两个索引表示一条边
    let mut indices = Vec::with_capacity(edges.len() * 2);
    for edge in &edges {
        indices.push(edge.start_idx);
        indices.push(edge.end_idx);
    }

    // 构建Voronoi单元格
    let mut cells = vec![
        VoronoiCell {
            site: Pos2::new(0.0, 0.0),
            vertex_indices: Vec::new(),
        };
        points.len()
    ];

    // 设置每个单元格的site点
    for (i, &point) in points.iter().enumerate() {
        cells[i].site = point;

        // 添加该单元格的顶点索引
        if let Some(vertex_indices) = site_to_voronoi_vertices.get(&i) {
            cells[i].vertex_indices = vertex_indices.iter().copied().collect();
        }
    }

    // 尝试为每个单元格排序顶点，使其形成闭合多边形
    for cell in &mut cells {
        if cell.vertex_indices.len() <= 2 {
            continue; // 至少需要3个点才能形成多边形
        }

        // 这里可以添加更复杂的算法来排序顶点，使其形成闭合多边形
        // 当前实现简单地收集顶点，但不保证它们组成有效的多边形
    }

    let duration = start_time.elapsed();
    println!(
        "索引化Voronoi图生成完成，包含 {} 个顶点、{} 条边和 {} 个单元格，耗时 {:.2?}",
        vertices.len(),
        edges.len(),
        cells.len(),
        duration
    );

    IndexedVoronoiDiagram {
        vertices,
        indices,
        cells,
    }
}

/// 生成Voronoi图的边界表示，返回顶点数组和索引数组
/// 适合直接用于GPU渲染
pub fn generate_voronoi_render_data(indices: &[u32], points: &[Pos2]) -> (Vec<Pos2>, Vec<u32>) {
    compute_indexed_voronoi(indices, points).get_render_data()
}

/// 为兼容性保留原有函数，但内部使用新的索引化实现
pub fn generate_voronoi_edges(indices: &[u32], points: &[Pos2]) -> Vec<[Pos2; 2]> {
    let voronoi = compute_indexed_voronoi(indices, points);

    let mut edges = Vec::new();
    for i in 0..voronoi.indices.len() / 2 {
        let start_idx = voronoi.indices[i * 2] as usize;
        let end_idx = voronoi.indices[i * 2 + 1] as usize;

        if start_idx < voronoi.vertices.len() && end_idx < voronoi.vertices.len() {
            edges.push([voronoi.vertices[start_idx], voronoi.vertices[end_idx]]);
        }
    }

    edges
}

/// 维持旧的接口以保持向后兼容性
pub fn compute_voronoi(triangle_indices: &[u32], points: &[Pos2]) -> VoronoiDiagram {
    let indexed_voronoi = compute_indexed_voronoi(triangle_indices, points);

    // 将新的索引化结构转换为旧的非索引化结构
    let mut old_edges = Vec::new();
    for i in 0..indexed_voronoi.indices.len() / 2 {
        let start_idx = indexed_voronoi.indices[i * 2] as usize;
        let end_idx = indexed_voronoi.indices[i * 2 + 1] as usize;

        if start_idx < indexed_voronoi.vertices.len() && end_idx < indexed_voronoi.vertices.len() {
            // 找到这条边对应的原始点索引
            // 注意：这是一个简化的转换，可能不完全准确
            let site1 = i % points.len();
            let site2 = (i + 1) % points.len();

            old_edges.push(OldVoronoiEdge {
                start: indexed_voronoi.vertices[start_idx],
                end: indexed_voronoi.vertices[end_idx],
                site1,
                site2,
            });
        }
    }

    // 构建旧格式的单元格
    let mut old_cells = vec![
        OldVoronoiCell {
            site: Pos2::new(0.0, 0.0),
            edges: Vec::new(),
            vertices: Vec::new(),
        };
        points.len()
    ];

    for (i, old_cell) in old_cells.iter_mut().enumerate() {
        if i < indexed_voronoi.cells.len() {
            old_cell.site = indexed_voronoi.cells[i].site;

            // 从顶点索引构建旧格式的顶点列表
            old_cell.vertices = indexed_voronoi.cells[i]
                .vertex_indices
                .iter()
                .filter_map(|&idx| {
                    if (idx as usize) < indexed_voronoi.vertices.len() {
                        Some(indexed_voronoi.vertices[idx as usize])
                    } else {
                        None
                    }
                })
                .collect();
        }
    }

    VoronoiDiagram {
        cells: old_cells,
        edges: old_edges,
    }
}

// 为了支持旧的API，保留旧的结构体定义
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OldVoronoiEdge {
    pub start: Pos2,
    pub end: Pos2,
    pub site1: usize,
    pub site2: usize,
}

#[derive(Debug, Clone)]
pub struct OldVoronoiCell {
    pub site: Pos2,
    pub edges: Vec<OldVoronoiEdge>,
    pub vertices: Vec<Pos2>,
}

#[derive(Debug, Clone)]
pub struct VoronoiDiagram {
    pub cells: Vec<OldVoronoiCell>,
    pub edges: Vec<OldVoronoiEdge>,
}

impl VoronoiDiagram {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn get_vertices(&self) -> Vec<Pos2> {
        let mut vertices = HashSet::new();

        for edge in &self.edges {
            // 使用精确的表示方法来避免重复点
            let start_key = (
                (edge.start.x * 10000.0).round() as i64,
                (edge.start.y * 10000.0).round() as i64,
            );
            let end_key = (
                (edge.end.x * 10000.0).round() as i64,
                (edge.end.y * 10000.0).round() as i64,
            );

            vertices.insert(start_key);
            vertices.insert(end_key);
        }

        vertices
            .iter()
            .map(|&(x, y)| Pos2::new(x as f32 / 10000.0, y as f32 / 10000.0))
            .collect()
    }
}
