//! Deterministic generation of the current natural world slice.

mod random;
mod tectonics;
mod topology;

pub use tectonics::{TectonicGenerationError, TectonicGenerator};
