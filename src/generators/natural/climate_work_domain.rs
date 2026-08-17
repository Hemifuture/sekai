use thiserror::Error;

use super::circulation::{CubedSphereGrid, CubedSphereGridError};
use crate::engine::BuildCancellation;
use crate::generators::spatial::{ConservativeRemapError, ConservativeSurfaceMapBuilder};
use crate::world::natural::{
    ClimateWorkDomainSnapshot, ClimateWorkDomainValidationError, NaturalQualityProfile,
    CLIMATE_WORK_DOMAIN_SCHEMA_V1,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

/// Builds the reconstructable climate grid and both conservative bridges.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClimateWorkDomainBuilder;

impl ClimateWorkDomainBuilder {
    pub fn build(
        source: &SphericalSurfaceSnapshot,
        profile: NaturalQualityProfile,
        cancellation: &BuildCancellation,
    ) -> Result<ClimateWorkDomainSnapshot, ClimateWorkDomainBuildError> {
        check_cancelled(cancellation)?;
        source
            .validate()
            .map_err(|error| ClimateWorkDomainBuildError::InvalidSource {
                reason: error.to_string(),
            })?;

        let resolution = profile.climate_face_resolution();
        let grid = CubedSphereGrid::new(resolution, source.radius().get())?;
        let climate_surface = grid.to_surface_snapshot()?;
        check_cancelled(cancellation)?;

        let source_to_climate =
            ConservativeSurfaceMapBuilder::build_cancellable(source, &climate_surface, || {
                cancellation.is_cancelled()
            })
            .map_err(map_error)?;
        let climate_to_source =
            ConservativeSurfaceMapBuilder::build_cancellable(&climate_surface, source, || {
                cancellation.is_cancelled()
            })
            .map_err(map_error)?;
        check_cancelled(cancellation)?;

        let snapshot = ClimateWorkDomainSnapshot::new(
            CLIMATE_WORK_DOMAIN_SCHEMA_V1,
            profile,
            resolution,
            SurfaceRef::for_spherical(source),
            *grid.fingerprint(),
            climate_surface,
            source_to_climate,
            climate_to_source,
        )?;
        snapshot.validate_against(source)?;

        // The serialized work surface is required to be a lossless semantic
        // reconstruction of this exact cubed-sphere algorithm and version.
        let reconstructed = CubedSphereGrid::new(resolution, source.radius().get())?;
        let reconstructed_surface = reconstructed.to_surface_snapshot()?;
        if snapshot.climate_grid_fingerprint() != reconstructed.fingerprint()
            || snapshot.climate_surface() != &reconstructed_surface
        {
            return Err(ClimateWorkDomainBuildError::ReconstructionMismatch);
        }
        Ok(snapshot)
    }
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), ClimateWorkDomainBuildError> {
    if cancellation.is_cancelled() {
        Err(ClimateWorkDomainBuildError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_error(error: ConservativeRemapError) -> ClimateWorkDomainBuildError {
    if error == ConservativeRemapError::Cancelled {
        ClimateWorkDomainBuildError::Cancelled
    } else {
        ClimateWorkDomainBuildError::ConservativeRemap {
            reason: error.to_string(),
        }
    }
}

/// Failures that prevent atomic publication of a climate work domain.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateWorkDomainBuildError {
    #[error("climate work-domain build was cancelled")]
    Cancelled,
    #[error("invalid authoritative source surface: {reason}")]
    InvalidSource { reason: String },
    #[error(transparent)]
    CubedSphere(#[from] CubedSphereGridError),
    #[error("conservative climate remap failed: {reason}")]
    ConservativeRemap { reason: String },
    #[error(transparent)]
    Validation(#[from] ClimateWorkDomainValidationError),
    #[error("climate work-domain reconstruction changed semantic content")]
    ReconstructionMismatch,
}
