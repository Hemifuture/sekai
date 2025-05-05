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

#[derive(Debug, Clone)]
pub struct MapSystem {
    pub config: MapConfig,

    // 基础几何数据
    pub grid: Grid,
    pub delaunay: Vec<usize>,
    pub voronoi: IndexedVoronoiDiagram,

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
