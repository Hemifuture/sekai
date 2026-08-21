pub(crate) mod canvas_uniform;
// Retained for the compiled legacy map path; the formal app composes only `field`.
#[allow(dead_code)]
pub(crate) mod delaunay;
pub(crate) mod field;
mod helpers;
#[allow(dead_code)]
pub(crate) mod map_renderer;
mod pipelines;
#[allow(dead_code)]
pub(crate) mod points_callback;
#[allow(dead_code)]
pub(crate) mod points_renderer;
pub mod spherical;
#[allow(dead_code)]
pub(crate) mod voronoi;
