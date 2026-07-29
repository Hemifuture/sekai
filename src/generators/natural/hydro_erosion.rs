use thiserror::Error;

use super::{
    FluvialErosionError, FluvialErosionGenerator, HydrologyGenerationError, HydrologyGenerator,
};
use crate::world::natural::{
    ClimateValidationError, GeologicSnapshot, GeologicValidationError, HydroErosionSnapshot,
    HydroErosionSpec, HydroErosionSpecError, HydroErosionValidationError,
    PreliminaryClimateSnapshot, ReliefSnapshot, ReliefValidationError,
    HYDRO_EROSION_SNAPSHOT_SCHEMA_V1,
};
use crate::world::spatial::{SpatialSnapshot, SpatialValidationError, Topology};

/// Fixed two-pass current-slice hydro-erosion orchestration.
#[derive(Debug, Clone, Copy, Default)]
pub struct HydroErosionGenerator;

impl HydroErosionGenerator {
    /// Runs initial hydrology, bounded formation, and final hydrology exactly once each.
    pub fn generate(
        spatial: &SpatialSnapshot,
        relief: &ReliefSnapshot,
        geology: &GeologicSnapshot,
        climate: &PreliminaryClimateSnapshot,
        spec: &HydroErosionSpec,
    ) -> Result<HydroErosionSnapshot, HydroErosionGenerationError> {
        spatial.validate()?;
        relief.validate_against(spatial)?;
        geology.validate()?;
        climate.validate_against(spatial, relief)?;
        spec.validate()?;
        if geology.cell_count() as usize != spatial.cell_count() {
            return Err(HydroErosionGenerationError::CellCountMismatch {
                input: "geology",
                expected: spatial.cell_count(),
                found: geology.cell_count() as usize,
            });
        }

        let initial_hydrology = HydrologyGenerator::generate(
            spatial,
            relief.elevation_m(),
            relief.sea_level_m(),
            geology.relative_permeability(),
            climate,
            spec,
        )
        .map_err(HydroErosionGenerationError::InitialHydrology)?;
        let surface = FluvialErosionGenerator::generate(
            spatial,
            relief,
            geology.erosion_resistance(),
            &initial_hydrology,
            spec,
        )?;
        let final_hydrology = HydrologyGenerator::generate(
            spatial,
            surface.surface_elevation_m(),
            relief.sea_level_m(),
            geology.relative_permeability(),
            climate,
            spec,
        )
        .map_err(HydroErosionGenerationError::FinalHydrology)?;
        let snapshot =
            HydroErosionSnapshot::new(HYDRO_EROSION_SNAPSHOT_SCHEMA_V1, surface, final_hydrology)?;
        snapshot.validate_against(spatial, relief, geology, climate)?;
        Ok(snapshot)
    }
}

/// Errors returned by fixed two-pass hydro-erosion generation.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum HydroErosionGenerationError {
    /// Spatial topology is invalid.
    #[error("invalid spatial input: {0}")]
    Spatial(#[from] SpatialValidationError),
    /// Constructional relief is invalid.
    #[error("invalid relief input: {0}")]
    Relief(#[from] ReliefValidationError),
    /// Geologic substrate is invalid.
    #[error("invalid geologic input: {0}")]
    Geology(#[from] GeologicValidationError),
    /// Preliminary climate is invalid.
    #[error("invalid preliminary climate input: {0}")]
    Climate(#[from] ClimateValidationError),
    /// Hydro-erosion controls are invalid.
    #[error("invalid hydro-erosion specification: {0}")]
    Spec(#[from] HydroErosionSpecError),
    /// Initial hydrology failed.
    #[error("initial hydrology failed: {0}")]
    InitialHydrology(HydrologyGenerationError),
    /// Bounded erosion or sediment routing failed.
    #[error("fluvial formation failed: {0}")]
    Erosion(#[from] FluvialErosionError),
    /// Final hydrology failed.
    #[error("final hydrology failed: {0}")]
    FinalHydrology(HydrologyGenerationError),
    /// Atomic output validation failed.
    #[error("invalid atomic hydro-erosion output: {0}")]
    Composite(#[from] HydroErosionValidationError),
    /// One upstream snapshot has a different cardinality.
    #[error("input {input} has count {found}; expected {expected}")]
    CellCountMismatch {
        /// The stable input name.
        input: &'static str,
        /// The spatial count.
        expected: usize,
        /// The supplied count.
        found: usize,
    },
}
