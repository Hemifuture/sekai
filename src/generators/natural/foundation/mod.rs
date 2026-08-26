//! Spherical current-slice natural foundation pipeline.

pub(in crate::generators::natural) mod climate;
mod climate_stage;
pub(in crate::generators::natural) mod crust_physics;
pub(in crate::generators::natural) mod erosion;
mod geologic_stage;
pub(in crate::generators::natural) mod geology;
mod graph;
pub(in crate::generators::natural) mod hydro_erosion;
mod hydro_erosion_stage;
pub(in crate::generators::natural) mod hydrology;
pub(in crate::generators::natural) mod island_relief;
pub(in crate::generators::natural) mod mantle;
pub(in crate::generators::natural) mod moisture;
mod quality_stage;
pub(in crate::generators::natural) mod relief;
pub(in crate::generators::natural) mod tectonics;

pub use climate::SphericalClimateGenerationError;
pub use climate_stage::{
    SphericalPreliminaryClimateArtifact, SphericalPreliminaryClimateStage,
    SphericalPreliminaryClimateStageInputs,
};
pub use erosion::SphericalFluvialErosionError;
pub use geologic_stage::{
    SphericalGeologicArtifact, SphericalGeologicStage, SphericalGeologicStageInputs,
    SphericalMantleArtifact, SphericalMantleStage, SphericalMantleStageInputs,
};
pub use geology::SphericalGeologicGenerationError;
pub use graph::{
    spherical_natural_foundation_graph, SphericalReliefArtifact, SphericalReliefStage,
    SphericalReliefStageInputs, SphericalTectonicArtifact, SphericalTectonicStage,
    SphericalTectonicStageInputs,
};
pub use hydro_erosion::SphericalHydroErosionGenerationError;
pub use hydro_erosion_stage::{
    SphericalHydroErosionArtifact, SphericalHydroErosionStage, SphericalHydroErosionStageInputs,
};
pub use hydrology::SphericalHydrologyGenerationError;
pub use mantle::SphericalMantleGenerationError;
pub use quality_stage::{
    NaturalQualityArtifact, SphericalNaturalQualityStage, SphericalNaturalQualityStageInputs,
};
pub use relief::SphericalReliefGenerationError;
pub use tectonics::SphericalTectonicGenerationError;
