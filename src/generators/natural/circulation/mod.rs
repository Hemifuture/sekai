//! Experimental closed-sphere circulation geometry and solvers.

mod fixtures;
mod grid;
mod math;
mod operators;
mod thermodynamics;

pub use fixtures::{build_fixture, CirculationFixture, FixtureBuildError};
pub use grid::{CubedSphereGrid, CubedSphereGridError, SphericalCell, SphericalEdge};
pub use operators::{CirculationOperatorError, CirculationOperators, ConservativeTransport};
pub use thermodynamics::{
    advance_thermodynamics, saturation_specific_humidity, thermodynamic_tendencies,
    CirculationEdgePermeability, ThermodynamicError, ThermodynamicState, ThermodynamicTendencies,
};
