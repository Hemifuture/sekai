//! Terrain generation ported from Civ Atlas; see NOTICE.md for source and licenses.
// These crate-private modules gain default-world consumers in the following port task.
// Keep their visibility private; remove dead-code allowances as those consumers arrive.
#[allow(dead_code)]
pub(crate) mod mesh;
#[allow(dead_code)]
pub(crate) mod noise;
#[allow(dead_code)]
pub(crate) mod random;
/// Invalid parameters or interruption while building terrain.
#[derive(Debug, thiserror::Error, PartialEq)]
pub(crate) enum TerrainError {
    /// The requested mesh exceeds the supported domain.
    #[error("invalid mesh dimensions or spacing")]
    InvalidMesh,
    /// The caller requested cancellation.
    #[error("terrain generation cancelled")]
    Cancelled,
}
#[cfg(test)]
mod tests;

mod triangulation;

#[allow(dead_code)]
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
