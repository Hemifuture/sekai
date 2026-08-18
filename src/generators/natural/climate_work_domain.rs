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
            .validate_cancellable(&|| cancellation.is_cancelled())
            .map_err(|error| {
                if error == crate::world::spatial::SphericalSurfaceValidationError::Cancelled {
                    ClimateWorkDomainBuildError::Cancelled
                } else {
                    ClimateWorkDomainBuildError::InvalidSource {
                        reason: error.to_string(),
                    }
                }
            })?;

        let resolution = profile.climate_face_resolution();
        let grid = CubedSphereGrid::new_cancellable(resolution, source.radius().get(), &|| {
            cancellation.is_cancelled()
        })
        .map_err(map_grid_error)?;
        let climate_surface = grid
            .to_surface_snapshot_cancellable(&|| cancellation.is_cancelled())
            .map_err(map_grid_error)?;
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

        let snapshot = ClimateWorkDomainSnapshot::new_cancellable(
            CLIMATE_WORK_DOMAIN_SCHEMA_V1,
            profile,
            resolution,
            SurfaceRef::for_spherical(source),
            *grid.fingerprint(),
            climate_surface,
            source_to_climate,
            climate_to_source,
            &|| cancellation.is_cancelled(),
        )
        .map_err(domain_validation_error)?;
        Ok(snapshot)
    }
}

/// Reconstructs the exact locked grid algorithm so artifact/deserialization
/// boundaries cannot accept a merely same-sized V2 surface or arbitrary
/// nonzero grid fingerprint.
pub(crate) fn validate_climate_work_domain_reconstruction(
    snapshot: &ClimateWorkDomainSnapshot,
) -> Result<(), ClimateWorkDomainBuildError> {
    validate_climate_work_domain_reconstruction_impl(snapshot, None)
}

pub(crate) fn validate_climate_work_domain_reconstruction_cancellable(
    snapshot: &ClimateWorkDomainSnapshot,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), ClimateWorkDomainBuildError> {
    validate_climate_work_domain_reconstruction_impl(snapshot, Some(cancelled))
}

/// Rebuilds both directed overlap maps from the supplied endpoint geometry.
///
/// Sparse margin closure alone cannot prove that overlap support or tangent
/// transforms came from those surfaces. This contextual audit is therefore
/// the authoritative rehydration boundary for a portable domain snapshot.
pub(crate) fn validate_climate_work_domain_maps_against(
    snapshot: &ClimateWorkDomainSnapshot,
    source: &SphericalSurfaceSnapshot,
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), ClimateWorkDomainBuildError> {
    let forward = match cancellation {
        Some(cancelled) => ConservativeSurfaceMapBuilder::build_cancellable(
            source,
            snapshot.climate_surface(),
            cancelled,
        ),
        None => ConservativeSurfaceMapBuilder::build(source, snapshot.climate_surface()),
    }
    .map_err(map_error)?;
    let reverse = match cancellation {
        Some(cancelled) => ConservativeSurfaceMapBuilder::build_cancellable(
            snapshot.climate_surface(),
            source,
            cancelled,
        ),
        None => ConservativeSurfaceMapBuilder::build(snapshot.climate_surface(), source),
    }
    .map_err(map_error)?;
    if &forward != snapshot.source_to_climate() || &reverse != snapshot.climate_to_source() {
        return Err(ClimateWorkDomainBuildError::CanonicalMapMismatch);
    }
    Ok(())
}

fn validate_climate_work_domain_reconstruction_impl(
    snapshot: &ClimateWorkDomainSnapshot,
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), ClimateWorkDomainBuildError> {
    let reconstructed = match cancellation {
        Some(cancelled) => CubedSphereGrid::new_cancellable(
            snapshot.face_resolution(),
            snapshot.climate_surface().radius().get(),
            cancelled,
        ),
        None => CubedSphereGrid::new(
            snapshot.face_resolution(),
            snapshot.climate_surface().radius().get(),
        ),
    }
    .map_err(map_grid_error)?;
    let reconstructed_surface = match cancellation {
        Some(cancelled) => reconstructed.to_surface_snapshot_cancellable(cancelled),
        None => reconstructed.to_surface_snapshot(),
    }
    .map_err(map_grid_error)?;
    if snapshot.climate_grid_fingerprint() != reconstructed.fingerprint()
        || snapshot.climate_surface() != &reconstructed_surface
    {
        return Err(ClimateWorkDomainBuildError::ReconstructionMismatch);
    }
    Ok(())
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

fn map_grid_error(error: CubedSphereGridError) -> ClimateWorkDomainBuildError {
    if error == CubedSphereGridError::Cancelled {
        ClimateWorkDomainBuildError::Cancelled
    } else {
        ClimateWorkDomainBuildError::CubedSphere(error)
    }
}

fn domain_validation_error(error: ClimateWorkDomainValidationError) -> ClimateWorkDomainBuildError {
    if error == ClimateWorkDomainValidationError::Cancelled {
        ClimateWorkDomainBuildError::Cancelled
    } else {
        ClimateWorkDomainBuildError::Validation(error)
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
    #[error("climate work-domain conservative maps differ from canonical endpoint geometry")]
    CanonicalMapMismatch,
}
