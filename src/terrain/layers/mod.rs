//! Terrain layers for composable terrain generation
//!
//! Each layer adds a specific aspect to the terrain.

mod detail_layer;
mod plate_layer;
mod postprocess_layer;
mod regional_layer;
mod tectonic_layer;

// Core trait definition
#[allow(clippy::module_inception)]
pub mod r#trait;

pub use detail_layer::DetailLayer;
pub use plate_layer::{BoundaryType, Plate, PlateConfig, PlateLayer, PlateType};
pub use postprocess_layer::{PostprocessConfig, PostprocessLayer};
pub use regional_layer::RegionalLayer;
pub use tectonic_layer::{CollisionType, TectonicConfig, TectonicLayer};

// Re-export the trait and types
pub use r#trait::{LayerOutput, LegacyTerrainLayer, Pos2, TerrainContext, TerrainLayer};
