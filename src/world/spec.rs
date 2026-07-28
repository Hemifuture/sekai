use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{Meters, RootSeed};

/// The supported version of the serialized world specification schema.
pub const WORLD_SPEC_SCHEMA_V1: u16 = 1;
/// The smallest cell count allowed by the V1 numerical-safety budget.
pub const MIN_CELL_COUNT: u32 = 16;
/// The largest cell count allowed by the V1 numerical-safety budget.
pub const MAX_CELL_COUNT: u32 = 200_000;
/// The smallest planar dimension allowed by the V1 numerical-safety budget, in meters.
pub const MIN_DIMENSION_METERS: f64 = 1.0;
/// The largest planar dimension allowed by the V1 numerical-safety budget, in meters.
pub const MAX_DIMENSION_METERS: f64 = 100_000_000.0;

const MIN_ASPECT_RATIO: f64 = 1.0 / 16.0;
const MAX_ASPECT_RATIO: f64 = 16.0;

/// The condition applied at the outer boundary of planar world space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundaryCondition {
    /// Stops world-space traversal at the outer boundary.
    Closed,
}

/// The technology level used as the baseline for world generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TechnologyBaseline {
    /// Represents a pre-industrial medieval technology baseline.
    PreIndustrialMedieval,
}

/// The validated planar extent and allocation budget for a world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanarSpaceSpec {
    /// The horizontal extent of the world space.
    pub width: Meters,
    /// The vertical extent of the world space.
    pub height: Meters,
    /// The requested number of generated spatial cells.
    pub target_cell_count: u32,
    /// The condition applied at the world's outer boundary.
    pub boundary: BoundaryCondition,
}

impl PlanarSpaceSpec {
    /// Validates the V1 spatial numerical-safety and allocation budgets.
    pub fn validate(&self) -> Result<(), SpecError> {
        for dimension in [self.width.get(), self.height.get()] {
            if !(MIN_DIMENSION_METERS..=MAX_DIMENSION_METERS).contains(&dimension) {
                return Err(SpecError::DimensionOutOfRange {
                    found: dimension,
                    min: MIN_DIMENSION_METERS,
                    max: MAX_DIMENSION_METERS,
                });
            }
        }

        let aspect_ratio = self.width.get() / self.height.get();
        if !(MIN_ASPECT_RATIO..=MAX_ASPECT_RATIO).contains(&aspect_ratio) {
            return Err(SpecError::AspectRatioOutOfRange {
                found: aspect_ratio,
                min: MIN_ASPECT_RATIO,
                max: MAX_ASPECT_RATIO,
            });
        }

        if !(MIN_CELL_COUNT..=MAX_CELL_COUNT).contains(&self.target_cell_count) {
            return Err(SpecError::CellCountOutOfRange {
                found: self.target_cell_count,
                min: MIN_CELL_COUNT,
                max: MAX_CELL_COUNT,
            });
        }

        Ok(())
    }
}

/// A versioned, deterministic description of a world to generate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldSpec {
    /// The schema version used to interpret this specification.
    pub schema_version: u16,
    /// The root seed from which deterministic generation begins.
    pub root_seed: RootSeed,
    /// The planar spatial extent and generation budget.
    pub space: PlanarSpaceSpec,
    /// The baseline technology level used by generation rules.
    pub technology: TechnologyBaseline,
}

impl WorldSpec {
    /// Validates this V1 world specification.
    pub fn validate(&self) -> Result<(), SpecError> {
        if self.schema_version != WORLD_SPEC_SCHEMA_V1 {
            return Err(SpecError::UnsupportedSchema {
                found: self.schema_version,
                supported: WORLD_SPEC_SCHEMA_V1,
            });
        }

        self.space.validate()
    }
}

/// Errors returned when a world specification exceeds a V1 safety budget.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SpecError {
    /// The specification uses a schema version that this engine does not support.
    #[error("unsupported schema version {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The schema version found in the specification.
        found: u16,
        /// The schema version supported by this engine.
        supported: u16,
    },
    /// A planar dimension lies outside the V1 numerical-safety budget.
    #[error("dimension {found} is outside {min}..={max} meters")]
    DimensionOutOfRange {
        /// The dimension that failed validation, in meters.
        found: f64,
        /// The inclusive lower limit, in meters.
        min: f64,
        /// The inclusive upper limit, in meters.
        max: f64,
    },
    /// The planar width-to-height ratio lies outside the V1 numerical-safety budget.
    #[error("aspect ratio {found} is outside {min}..={max}")]
    AspectRatioOutOfRange {
        /// The ratio that failed validation.
        found: f64,
        /// The inclusive lower ratio limit.
        min: f64,
        /// The inclusive upper ratio limit.
        max: f64,
    },
    /// The requested cell count lies outside the V1 allocation-safety budget.
    #[error("cell count {found} is outside {min}..={max}")]
    CellCountOutOfRange {
        /// The cell count that failed validation.
        found: u32,
        /// The inclusive lower cell-count limit.
        min: u32,
        /// The inclusive upper cell-count limit.
        max: u32,
    },
}
