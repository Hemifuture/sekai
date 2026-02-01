use egui::{Pos2, Rect};

use crate::delaunay::{
    self,
    voronoi::{self, IndexedVoronoiDiagram},
};
use crate::spatial::{EdgeIndex, GridIndex};

use super::{cells_data::CellsData, grid::Grid};

/// 图层可见性设置
#[derive(Debug, Clone, Copy)]
pub struct LayerVisibility {
    /// 高度图图层（填充的Voronoi单元格）
    pub heightmap: bool,
    /// Voronoi边线图层
    pub voronoi_edges: bool,
    /// Delaunay三角剖分图层
    pub delaunay: bool,
    /// 点图层
    pub points: bool,
}

impl Default for LayerVisibility {
    fn default() -> Self {
        Self {
            heightmap: true,
            voronoi_edges: false,
            delaunay: false,
            points: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MapConfig {
    pub width: u32,
    pub height: u32,
    pub spacing: u32,
}

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            width: 2000,
            height: 1000,
            spacing: 5,  // 减小间距以获得更高分辨率 (约 80,000 点)
        }
    }
}

/// 地图系统
/// 
/// 包含地图的所有几何数据和属性数据。
#[derive(Debug, Clone)]
pub struct MapSystem {
    /// 地图配置
    pub config: MapConfig,

    // 基础几何数据
    /// 网格点生成器
    pub grid: Grid,
    /// Delaunay 三角剖分索引（u32）
    /// 每3个连续索引构成一个三角形
    pub delaunay: Vec<u32>,
    /// Voronoi 图
    pub voronoi: IndexedVoronoiDiagram,

    /// 单元格属性数据
    pub cells_data: CellsData,

    // 空间索引
    /// 点的空间索引（用于点击测试、邻居查询）
    pub point_index: GridIndex,
    /// Voronoi 边的空间索引（用于视口裁剪）
    pub voronoi_edge_index: EdgeIndex,
    /// Delaunay 边的空间索引（用于视口裁剪）
    pub delaunay_edge_index: EdgeIndex,

    /// 图层可见性设置
    pub layer_visibility: LayerVisibility,
}

impl Default for MapSystem {
    fn default() -> Self {
        let config = MapConfig::default();
        let mut grid = Grid::new(config.width, config.height, config.spacing);
        grid.generate_points();
        let points = grid.get_all_points();
        let delaunay = delaunay::triangulate(&points);
        let voronoi = voronoi::compute_indexed_voronoi(&delaunay, &points);
        let cells_data = CellsData::new(points.len());
        
        // 计算边界框
        let bounds = Rect::from_min_max(
            Pos2::ZERO,
            Pos2::new(config.width as f32, config.height as f32),
        );
        
        // 构建空间索引
        let point_index = GridIndex::build_auto(&points, bounds);
        let voronoi_edge_index = EdgeIndex::build_auto(
            &voronoi.vertices,
            &voronoi.indices,
            bounds,
        );
        
        // 构建 Delaunay 边索引
        // Delaunay 的顶点是原始点，需要从三角形索引提取边
        let delaunay_edges = Self::extract_delaunay_edges(&delaunay);
        let delaunay_edge_index = EdgeIndex::build_auto(
            &points,
            &delaunay_edges,
            bounds,
        );
        
        Self {
            config,
            grid,
            delaunay,
            voronoi,
            cells_data,
            point_index,
            voronoi_edge_index,
            delaunay_edge_index,
            layer_visibility: LayerVisibility::default(),
        }
    }
}

impl MapSystem {
    /// 从三角形索引提取边索引
    ///
    /// 每个三角形有 3 条边，但相邻三角形共享边，所以需要去重。
    fn extract_delaunay_edges(triangle_indices: &[u32]) -> Vec<u32> {
        use std::collections::HashSet;
        
        let mut edge_set: HashSet<(u32, u32)> = HashSet::new();
        
        for chunk in triangle_indices.chunks(3) {
            if chunk.len() == 3 {
                let (a, b, c) = (chunk[0], chunk[1], chunk[2]);
                
                // 确保边的索引有序（小的在前）以便去重
                for (p1, p2) in [(a, b), (b, c), (c, a)] {
                    let edge = if p1 < p2 { (p1, p2) } else { (p2, p1) };
                    edge_set.insert(edge);
                }
            }
        }
        
        // 转换为扁平数组
        let mut edges = Vec::with_capacity(edge_set.len() * 2);
        for (p1, p2) in edge_set {
            edges.push(p1);
            edges.push(p2);
        }
        
        edges
    }
    
    /// 查找包含指定位置的 Voronoi 单元格
    ///
    /// # 参数
    /// - `pos`: 查询位置（画布坐标）
    ///
    /// # 返回值
    /// 单元格索引，如果位置超出地图范围则返回 None
    pub fn find_cell_at(&self, pos: Pos2) -> Option<u32> {
        let points = self.grid.get_all_points();
        self.point_index.find_nearest(&points, pos)
    }
    
    /// 查询指定位置附近的单元格
    ///
    /// # 参数
    /// - `pos`: 中心位置
    /// - `radius`: 搜索半径
    ///
    /// # 返回值
    /// 半径范围内的所有单元格索引
    pub fn find_cells_in_radius(&self, pos: Pos2, radius: f32) -> Vec<u32> {
        let points = self.grid.get_all_points();
        self.point_index.query_radius(&points, pos, radius)
    }
    
    /// 获取视口内可见的 Voronoi 边索引
    ///
    /// # 参数
    /// - `view_rect`: 视口矩形（画布坐标）
    ///
    /// # 返回值
    /// 可见边的索引数组，可直接用于 GPU 渲染
    pub fn get_visible_voronoi_edges(&self, view_rect: Rect) -> Vec<u32> {
        self.voronoi_edge_index.get_visible_indices(
            &self.voronoi.vertices,
            &self.voronoi.indices,
            view_rect,
        )
    }
    
    /// 获取视口内可见的 Delaunay 边索引
    ///
    /// # 参数
    /// - `view_rect`: 视口矩形（画布坐标）
    ///
    /// # 返回值
    /// 可见边的索引数组
    pub fn get_visible_delaunay_edges(&self, view_rect: Rect) -> Vec<u32> {
        let points = self.grid.get_all_points();
        let delaunay_edges = Self::extract_delaunay_edges(&self.delaunay);
        self.delaunay_edge_index.get_visible_indices(
            &points,
            &delaunay_edges,
            view_rect,
        )
    }
    
    /// 获取地图边界框
    pub fn bounds(&self) -> Rect {
        Rect::from_min_max(
            Pos2::ZERO,
            Pos2::new(self.config.width as f32, self.config.height as f32),
        )
    }
}
