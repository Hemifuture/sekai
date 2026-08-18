//! Coupled P5 surface-formation kernels.

mod hydrology;
mod stream_power;

pub use hydrology::{FormationHydrologyGenerationError, FormationHydrologyGenerator};
pub use stream_power::{
    implicit_stream_power_n1_height, ImplicitStreamPowerSolver, StreamPowerGenerationError,
    StreamPowerInputs, StreamPowerStep,
};
