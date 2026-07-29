pub mod canvas_uniform;
// Retained for the compiled legacy map path; the formal app composes only `field`.
#[allow(dead_code)]
pub mod delaunay;
pub mod field;
mod helpers;
#[allow(dead_code)]
pub mod map_renderer;
mod pipelines;
#[allow(dead_code)]
pub mod points_callback;
#[allow(dead_code)]
pub mod points_renderer;
#[allow(dead_code)]
pub mod voronoi;
