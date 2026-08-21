mod callback;
mod overlay;
mod renderer;

pub use callback::SphericalPaintCallback;
pub use renderer::{
    RiverGlobeSegment, RiverMapSegment, SphericalFieldRenderer, SphericalGpuPacket,
    SphericalRenderError, SphericalRenderMode, SphericalUploadCounters,
};

#[cfg(test)]
pub(crate) use renderer::{installed_overlay_arc_ids, validation_probe};
