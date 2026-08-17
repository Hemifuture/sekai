//! Experimental closed-sphere circulation geometry and solvers.

mod comparison;
mod dynamics;
mod fixtures;
mod grid;
mod linear;
mod math;
mod operators;
mod solver;
mod steady;
mod thermodynamics;
mod transient;

pub use comparison::{
    compare_snapshots, run_comparison_suite, ComparisonCaseReport, ComparisonError,
    ComparisonReport, ComparisonSuiteReport, ComparisonTimings, DenseByteSummary,
    EligibilityFailure, EligibilityRule, FixtureComparison, MonthlyAgreement, ScalarAgreement,
    TimingSummary, VectorAgreement, WysiwygEligibility,
};
pub use fixtures::{build_fixture, CirculationFixture, FixtureBuildError};
pub use grid::{CubedSphereGrid, CubedSphereGridError, SphericalCell, SphericalEdge};
pub use operators::{
    CirculationOperatorError, CirculationOperators, ConservativeTransport, SecondOrderTransport,
    SecondOrderTransportWorkspace, SteadyTransportSolve, UpwindTracerTransport,
};
pub use solver::{CirculationSolveError, CirculationSolver};
pub use steady::BalancedSteadySolver;
pub use thermodynamics::{
    advance_thermodynamics, saturation_specific_humidity, thermodynamic_tendencies,
    CirculationEdgePermeability, ThermodynamicError, ThermodynamicState, ThermodynamicTendencies,
};
pub use transient::TransientShallowWaterSolver;
