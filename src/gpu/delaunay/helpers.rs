use crate::delaunay::Triangle;

use super::delaunay_renderer::GPUTriangle;

pub fn to_gpu_triangles(triangles: Vec<Triangle>) -> Vec<GPUTriangle> {
    triangles
        .iter()
        .map(|t| GPUTriangle {
            points: [t.points[0].into(), t.points[1].into(), t.points[2].into()],
        })
        .collect()
}
