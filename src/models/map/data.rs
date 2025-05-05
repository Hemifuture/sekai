use crate::delaunay::{
    self,
    voronoi::{self, IndexedVoronoiDiagram},
};

use super::grid::Grid;

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
pub struct MapData {
    pub config: MapConfig,
    pub grid: Grid,
    pub delaunay: Vec<u32>,
    pub voronoi: IndexedVoronoiDiagram,
}

impl Default for MapData {
    fn default() -> Self {
        let config = MapConfig::default();
        let mut grid = Grid::new(config.width, config.height, config.spacing);
        grid.generate_points();
        let delaunay = delaunay::triangulate(&grid.get_all_points());
        let voronoi = voronoi::compute_indexed_voronoi(&delaunay, &grid.get_all_points());
        Self {
            config,
            grid,
            delaunay,
            voronoi,
        }
    }
}
