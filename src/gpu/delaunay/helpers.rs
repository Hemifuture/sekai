use egui::Pos2;

use super::delaunay_renderer::GPUTriangle;

#[allow(dead_code)]
pub fn to_gpu_triangles(indices: Vec<u32>, points: &[Pos2]) -> Vec<GPUTriangle> {
    let mut triangles = Vec::with_capacity(indices.len() / 3);

    // 每三个索引构造一个三角形
    for i in (0..indices.len()).step_by(3) {
        if i + 2 < indices.len() {
            let i1 = indices[i] as usize;
            let i2 = indices[i + 1] as usize;
            let i3 = indices[i + 2] as usize;

            // 确保索引在有效范围内
            if i1 < points.len() && i2 < points.len() && i3 < points.len() {
                triangles.push(GPUTriangle {
                    points: [points[i1].into(), points[i2].into(), points[i3].into()],
                });
            }
        }
    }

    triangles
}
