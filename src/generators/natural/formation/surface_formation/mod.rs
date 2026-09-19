//! Coupled P5 surface-formation kernels.

mod coast;
pub(super) mod generation;
mod hillslope;
mod hydrology;
mod isostasy;
mod sediment;
mod state;
mod stream_power;

pub use coast::{CoastGenerationError, CoastalExchange, CoastalExchangeStep, CoastalInputs};
pub use generation::{
    SurfaceFormationGenerationError, SurfaceFormationGenerator, SurfaceFormationInputs,
};
pub use hillslope::{
    HillslopeGenerationError, HillslopeInputs, HillslopeTransportStep, HillslopeWorkspace,
    NonlinearHillslopeTransport,
};
pub use hydrology::{FormationHydrologyGenerationError, FormationHydrologyGenerator};
pub use isostasy::{IsostasyGenerationError, IsostaticAdjustmentStep, LocalAiryIsostasy};
pub use sediment::{
    ProvenanceSedimentRouter, SedimentGenerationError, SedimentInputs, SedimentTransportStep,
};
pub(in crate::generators::natural) use state::{FormationState, FormationStateError};
pub use stream_power::{
    implicit_stream_power_n1_height, ImplicitStreamPowerSolver, StreamPowerGenerationError,
    StreamPowerInputs, StreamPowerStep,
};
