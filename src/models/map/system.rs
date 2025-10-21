use crate::delaunay::{
    self,
    voronoi::{self, IndexedVoronoiDiagram},
};
use crate::terrain::{HeightGenerator, NoiseConfig};

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

    // 地形数据
    pub heights: Vec<u8>,
    pub noise_config: NoiseConfig,
}

impl Default for MapSystem {
    fn default() -> Self {
        let config = MapConfig::default();
        let mut grid = Grid::new(config.width, config.height, config.spacing);
        grid.generate_points();
        let delaunay = delaunay::triangulate(&grid.get_all_points());
        let voronoi = voronoi::compute_indexed_voronoi(&delaunay, &grid.get_all_points());
        let cells_data = CellsData::new(grid.get_all_points().len());

        // Generate terrain heights
        let noise_config = NoiseConfig::terrain();
        let height_generator = HeightGenerator::new(noise_config.clone());
        let heights = height_generator.generate_for_grid(&grid);

        log::info!(
            "MapSystem: Generated {} height values for {} grid points",
            heights.len(),
            grid.points.len()
        );

        Self {
            config,
            grid,
            delaunay,
            voronoi,
            cells_data,
            heights,
            noise_config,
        }
    }
}

impl MapSystem {
    /// Regenerate terrain heights with a new random seed
    pub fn regenerate_heights(&mut self) {
        self.noise_config.seed = rand::random();
        let height_generator = HeightGenerator::new(self.noise_config.clone());
        self.heights = height_generator.generate_for_grid(&self.grid);

        log::info!(
            "MapSystem: Regenerated {} height values with seed {}",
            self.heights.len(),
            self.noise_config.seed
        );
    }

    /// Regenerate terrain heights with a specific noise configuration
    pub fn regenerate_heights_with_config(&mut self, config: NoiseConfig) {
        self.noise_config = config;
        let height_generator = HeightGenerator::new(self.noise_config.clone());
        self.heights = height_generator.generate_for_grid(&self.grid);

        log::info!(
            "MapSystem: Regenerated {} height values with custom config",
            self.heights.len()
        );
    }
}
