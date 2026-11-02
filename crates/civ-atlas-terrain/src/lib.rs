//! Terrain generation ported from Civ Atlas; see NOTICE.md for source and licenses.

// These internal helpers gain their production consumers in the following port tasks.
#[allow(dead_code)]
pub(crate) mod noise;
#[allow(dead_code)]
pub(crate) mod random;

#[cfg(test)]
mod tests;
