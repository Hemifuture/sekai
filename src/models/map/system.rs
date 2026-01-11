use crate::delaunay::{
    self,
    voronoi::{self, IndexedVoronoiDiagram},
};

use super::{cells_data::CellsData, grid::Grid};

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
            spacing: 10,
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
}

impl Default for MapSystem {
    fn default() -> Self {
        let config = MapConfig::default();
        let mut grid = Grid::new(config.width, config.height, config.spacing);
        grid.generate_points();
        let delaunay = delaunay::triangulate(&grid.get_all_points());
        let voronoi = voronoi::compute_indexed_voronoi(&delaunay, &grid.get_all_points());
        let cells_data = CellsData::new(grid.get_all_points().len());
        Self {
            config,
            grid,
            delaunay,
            voronoi,
            cells_data,
        }
    }
}

impl MapSystem {}
