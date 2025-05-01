use crate::delaunay::voronoi::IndexedVoronoiDiagram;

use super::grid::Grid;

pub struct MapData {
    pub grid: Grid,
    pub delaunay: Vec<u32>,
    pub voronoi: IndexedVoronoiDiagram,
}
