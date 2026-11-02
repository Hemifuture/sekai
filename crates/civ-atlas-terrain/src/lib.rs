//! Terrain generation ported from Civ Atlas; see NOTICE.md for source and licenses.
/// Pinned upstream source revision; it is part of the terrain source identity.
pub const UPSTREAM_REVISION: &str = "176cc9168a69be30d1e09bfd85db84a5f3fb99da";
/// Package version declared in the pinned Civ Atlas package.json.
pub const UPSTREAM_PACKAGE_VERSION: &str = "1.5.0";
/// Source GENERATOR_VERSION from the pinned src/gen/edits.ts.
pub const UPSTREAM_GENERATOR_VERSION: u32 = 7;

mod erosion;
mod initial_climate;
mod initial_currents;
mod inland_sea;
mod params;
mod tectonics;
mod util;
pub use params::TerrainParams;
mod terrain;
pub use terrain::{generate_terrain, Terrain};
#[cfg(test)]
#[allow(dead_code)]
mod terrain_reference {
    include!("../reference/terrain.rs");
}
pub(crate) mod mesh;
pub(crate) mod noise;
pub(crate) mod random;
/// Invalid parameters or interruption while building terrain.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TerrainError {
    /// A named author control is outside the supported source UI domain.
    #[error("invalid terrain parameter: {name}")]
    InvalidParameter {
        /// Name of the rejected control.
        name: &'static str,
    },
    /// The requested mesh exceeds the supported domain.
    #[error("invalid mesh dimensions or spacing")]
    InvalidMesh,
    /// The caller requested cancellation.
    #[error("terrain generation cancelled")]
    Cancelled,
    /// A generated field was non-finite or inconsistent with its mesh.
    #[error("invalid generated terrain field: {field}")]
    InvalidOutput {
        /// Name of the invalid field.
        field: &'static str,
    },
}
#[cfg(test)]
mod tests;

mod triangulation;

pub(crate) mod geometry;

#[cfg(test)]
mod geometry_reference {
    include!("../reference/geometry.rs");
    pub(crate) fn trig_matches() -> bool {
        TRIG.iter().all(|&(x, expected)| {
            let x = std::hint::black_box(x);
            [x.sin(), x.cos(), x.asin(), x.atan2(-0.3)]
                .map(crate::random::round24)
                .map(f64::to_bits)
                == expected
        })
    }
}
