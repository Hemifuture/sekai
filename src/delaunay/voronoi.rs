use egui::Pos2;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Voronoi边缘表示
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoronoiEdge {
    /// 边的起点
    pub start: Pos2,
    /// 边的终点
    pub end: Pos2,
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
    /// 围绕细胞的边（有序）
    pub edges: Vec<VoronoiEdge>,
    /// 细胞顶点（有序）
    pub vertices: Vec<Pos2>,
}

/// Voronoi图结构，包含所有Voronoi单元格
#[derive(Debug, Clone)]
pub struct VoronoiDiagram {
    /// 所有Voronoi单元格，按原始点索引排序
    pub cells: Vec<VoronoiCell>,
    /// 所有Voronoi边
    pub edges: Vec<VoronoiEdge>,
}

impl VoronoiDiagram {
    /// 创建一个空的Voronoi图
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// 获取所有Voronoi顶点
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

/// 从Delaunay三角剖分计算Voronoi图
pub fn compute_voronoi(triangle_indices: &[u32], points: &[Pos2]) -> VoronoiDiagram {
    let start_time = Instant::now();
    println!(
        "开始生成Voronoi图，基于 {} 个三角形",
        triangle_indices.len() / 3
    );

    if triangle_indices.len() < 3 || points.len() < 3 {
        return VoronoiDiagram::new();
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

    // 创建Voronoi边
    let mut voronoi_edges = Vec::new();

    for ((p1_idx, p2_idx), triangle_indices) in edge_to_triangles.iter() {
        if triangle_indices.len() == 2 {
            // 内部边: 两个三角形共享这条边
            let t1_idx = triangle_indices[0];
            let t2_idx = triangle_indices[1];

            let edge = VoronoiEdge {
                start: circumcenters[t1_idx],
                end: circumcenters[t2_idx],
                site1: *p1_idx,
                site2: *p2_idx,
            };

            voronoi_edges.push(edge);
        } else if triangle_indices.len() == 1 {
            // 边界边: 只有一个三角形使用这条边
            // 这里需要创建无限延伸的Voronoi边，或者截断在一个足够大的边界框内
            // 在实际应用中，我们可以选择不返回这些边，或者将它们延伸到一个预设的边界
            // TODO: 处理边界边
            // 由于我们已经添加了外扩的边界点，这里的边界边应该很少，可以简单忽略
        }
    }

    // 为每个原始点构建Voronoi单元
    let mut cells = vec![
        VoronoiCell {
            site: Pos2::new(0.0, 0.0),
            edges: Vec::new(),
            vertices: Vec::new(),
        };
        points.len()
    ];

    // 设置每个单元格的site
    for (i, &point) in points.iter().enumerate() {
        cells[i].site = point;
    }

    // 将Voronoi边分配给相应的单元格
    for edge in &voronoi_edges {
        let p1_idx = edge.site1;
        let p2_idx = edge.site2;

        // 每条边被两个相邻单元格共享
        cells[p1_idx].edges.push(*edge);

        // 对于第二个单元格，需要交换起点和终点
        let reversed_edge = VoronoiEdge {
            start: edge.end,
            end: edge.start,
            site1: edge.site2,
            site2: edge.site1,
        };
        cells[p2_idx].edges.push(reversed_edge);
    }

    // 对每个单元格的边进行排序，使它们形成连续的多边形
    for cell in &mut cells {
        if cell.edges.is_empty() {
            continue;
        }

        let mut sorted_edges = Vec::new();
        let mut vertices = Vec::new();

        // 从第一条边开始
        let mut current_edge = cell.edges[0];
        sorted_edges.push(current_edge);
        vertices.push(current_edge.start);

        let mut remaining_edges: HashSet<usize> = (1..cell.edges.len()).collect();

        // 找到形成闭合多边形的边序列
        while !remaining_edges.is_empty() {
            let current_end = current_edge.end;
            let mut found = false;

            for &edge_idx in remaining_edges.iter() {
                let next_edge = cell.edges[edge_idx];

                // 检查是否能连接当前边的终点
                if (next_edge.start - current_end).length() < 1e-4 {
                    sorted_edges.push(next_edge);
                    vertices.push(next_edge.start);
                    current_edge = next_edge;
                    remaining_edges.remove(&edge_idx);
                    found = true;
                    break;
                }
            }

            if !found {
                // 如果找不到下一条边，说明多边形可能不闭合
                // 在实际应用中，这可能发生在边界附近的单元格
                break;
            }
        }

        cell.edges = sorted_edges;
        cell.vertices = vertices;
    }

    let duration = start_time.elapsed();
    println!(
        "Voronoi图生成完成，包含 {} 个单元格和 {} 条边，耗时 {:.2?}",
        cells.len(),
        voronoi_edges.len(),
        duration
    );

    VoronoiDiagram {
        cells,
        edges: voronoi_edges,
    }
}

/// 生成Voronoi图的边界表示
/// 返回一个包含所有Voronoi边的列表，适合用于渲染
pub fn generate_voronoi_edges(indices: &[u32], points: &[Pos2]) -> Vec<[Pos2; 2]> {
    let voronoi = compute_voronoi(indices, points);

    voronoi
        .edges
        .iter()
        .map(|edge| [edge.start, edge.end])
        .collect()
}
