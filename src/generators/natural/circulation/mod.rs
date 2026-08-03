//! Experimental closed-sphere circulation geometry and solvers.

mod grid;
mod math;
mod operators;

pub use grid::{CubedSphereGrid, CubedSphereGridError, SphericalCell, SphericalEdge};
pub use operators::{CirculationOperatorError, CirculationOperators, ConservativeTransport};
