use thiserror::Error;

use super::{
    ConservativeRemapError, ConservativeSurfaceMapBuilder, GeodesicVoronoiBuilder,
    SphericalSurfaceBuildError,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::{evaluate_profile_surface_quality, QualityBuildError};
use crate::world::natural::{
    NaturalProfileError, NaturalQualityProfile, NaturalQualityReport, NaturalResolutionPlan,
    QualityMetricStatus,
};
use crate::world::spatial::{
    ConservativeSurfaceMap, ConservativeSurfaceMapError, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SurfaceRef,
};
use crate::world::{Meters, SphericalSpaceSpec};

/// An atomically constructed set of product and transient P1 spatial surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileSurfaceBundle {
    resolution_plan: NaturalResolutionPlan,
    authoritative_surface: SphericalSurfaceSnapshot,
    tectonic_control_surface: SphericalSurfaceSnapshot,
    control_to_authoritative_map: ConservativeSurfaceMap,
    quality_report: NaturalQualityReport,
}

impl ProfileSurfaceBundle {
    /// Returns the exact quality-profile resolution choices.
    pub const fn resolution_plan(&self) -> &NaturalResolutionPlan {
        &self.resolution_plan
    }

    /// Returns the sole authoritative product surface.
    pub const fn authoritative_surface(&self) -> &SphericalSurfaceSnapshot {
        &self.authoritative_surface
    }

    /// Returns the transient tectonic-control work surface.
    pub const fn tectonic_control_surface(&self) -> &SphericalSurfaceSnapshot {
        &self.tectonic_control_surface
    }

    /// Returns the conservative control-to-authoritative field map.
    pub const fn control_to_authoritative_map(&self) -> &ConservativeSurfaceMap {
        &self.control_to_authoritative_map
    }

    /// Returns the eight-metric P1 spatial quality report.
    pub const fn quality_report(&self) -> &NaturalQualityReport {
        &self.quality_report
    }
}

/// Builds a complete profile-surface bundle without publishing partial results.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProfileSurfaceBuilder;

impl ProfileSurfaceBuilder {
    /// Builds both surfaces, their conservative map, and P1 quality evidence atomically.
    pub fn build(
        profile: NaturalQualityProfile,
        radius: Meters,
        cancellation: &BuildCancellation,
    ) -> Result<ProfileSurfaceBundle, ProfileSurfaceBuildError> {
        check_cancelled(cancellation)?;
        let authoritative_space = SphericalSpaceSpec {
            radius,
            target_cell_count: profile.authoritative_target_cell_count(),
        };
        let resolution_plan = profile.resolve(&authoritative_space)?;
        check_cancelled(cancellation)?;

        let authoritative_surface = GeodesicVoronoiBuilder::build_cancellable(
            &resolution_plan.authoritative_space_spec(),
            || cancellation.is_cancelled(),
        )
        .map_err(map_authoritative_error)?;
        check_cancelled(cancellation)?;
        let tectonic_control_surface = GeodesicVoronoiBuilder::build_cancellable(
            &resolution_plan.tectonic_control_space_spec(),
            || cancellation.is_cancelled(),
        )
        .map_err(map_control_error)?;
        check_cancelled(cancellation)?;
        let control_to_authoritative_map = ConservativeSurfaceMapBuilder::build_cancellable(
            &tectonic_control_surface,
            &authoritative_surface,
            || cancellation.is_cancelled(),
        )
        .map_err(map_remap_error)?;
        check_cancelled(cancellation)?;
        let quality_report = evaluate_profile_surface_quality(
            &authoritative_surface,
            &tectonic_control_surface,
            &control_to_authoritative_map,
        )?;
        check_cancelled(cancellation)?;

        validate_bundle(
            &resolution_plan,
            &authoritative_surface,
            &tectonic_control_surface,
            &control_to_authoritative_map,
            &quality_report,
        )?;
        check_cancelled(cancellation)?;
        Ok(ProfileSurfaceBundle {
            resolution_plan,
            authoritative_surface,
            tectonic_control_surface,
            control_to_authoritative_map,
            quality_report,
        })
    }
}

fn validate_bundle(
    plan: &NaturalResolutionPlan,
    authoritative: &SphericalSurfaceSnapshot,
    control: &SphericalSurfaceSnapshot,
    map: &ConservativeSurfaceMap,
    quality: &NaturalQualityReport,
) -> Result<(), ProfileSurfaceBuildError> {
    plan.validate()?;
    authoritative
        .validate()
        .map_err(ProfileSurfaceBuildError::InvalidAuthoritativeSurface)?;
    control
        .validate()
        .map_err(ProfileSurfaceBuildError::InvalidControlSurface)?;
    map.validate()
        .map_err(ProfileSurfaceBuildError::InvalidConservativeMap)?;
    quality
        .validate()
        .map_err(|error| ProfileSurfaceBuildError::InvalidQualityReport(error.to_string()))?;

    validate_count(
        "authoritative",
        authoritative.cells().len(),
        plan.authoritative_resolved_cell_count(),
    )?;
    validate_count(
        "tectonic-control",
        control.cells().len(),
        plan.tectonic_control_resolved_cell_count(),
    )?;
    for (role, found) in [
        ("authoritative", authoritative.radius()),
        ("tectonic-control", control.radius()),
    ] {
        if found != plan.radius() {
            return Err(ProfileSurfaceBuildError::RadiusMismatch {
                role,
                found: found.get(),
                expected: plan.radius().get(),
            });
        }
    }

    let authoritative_ref = SurfaceRef::try_for_spherical(authoritative).map_err(|error| {
        ProfileSurfaceBuildError::InvalidSurfaceIdentity {
            role: "authoritative",
            reason: error.to_string(),
        }
    })?;
    let control_ref = SurfaceRef::try_for_spherical(control).map_err(|error| {
        ProfileSurfaceBuildError::InvalidSurfaceIdentity {
            role: "tectonic-control",
            reason: error.to_string(),
        }
    })?;
    validate_binding("map source", map.source_ref(), control_ref)?;
    validate_binding("map target", map.target_ref(), authoritative_ref)?;
    validate_binding("quality report", quality.surface_ref(), authoritative_ref)?;
    if let Some(metric) = quality
        .metrics()
        .iter()
        .find(|metric| metric.status() != QualityMetricStatus::Pass)
    {
        return Err(ProfileSurfaceBuildError::QualityGateFailed {
            metric: format!(
                "{}.{}.v{}",
                metric.id().namespace(),
                metric.id().name(),
                metric.id().version()
            ),
            status: metric.status(),
        });
    }
    Ok(())
}

fn validate_count(
    role: &'static str,
    found: usize,
    expected: u32,
) -> Result<(), ProfileSurfaceBuildError> {
    if found != expected as usize {
        return Err(ProfileSurfaceBuildError::ResolvedCellCountMismatch {
            role,
            found,
            expected,
        });
    }
    Ok(())
}

fn validate_binding(
    role: &'static str,
    found: SurfaceRef,
    expected: SurfaceRef,
) -> Result<(), ProfileSurfaceBuildError> {
    if found != expected {
        return Err(ProfileSurfaceBuildError::SurfaceBindingMismatch {
            role,
            found,
            expected,
        });
    }
    Ok(())
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), ProfileSurfaceBuildError> {
    if cancellation.is_cancelled() {
        Err(ProfileSurfaceBuildError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_authoritative_error(error: SphericalSurfaceBuildError) -> ProfileSurfaceBuildError {
    match error {
        SphericalSurfaceBuildError::Cancelled => ProfileSurfaceBuildError::Cancelled,
        error => ProfileSurfaceBuildError::AuthoritativeSurface(error),
    }
}

fn map_control_error(error: SphericalSurfaceBuildError) -> ProfileSurfaceBuildError {
    match error {
        SphericalSurfaceBuildError::Cancelled => ProfileSurfaceBuildError::Cancelled,
        error => ProfileSurfaceBuildError::ControlSurface(error),
    }
}

fn map_remap_error(error: ConservativeRemapError) -> ProfileSurfaceBuildError {
    match error {
        ConservativeRemapError::Cancelled => ProfileSurfaceBuildError::Cancelled,
        error => ProfileSurfaceBuildError::ConservativeRemap(error),
    }
}

/// Failures returned before an atomic profile-surface bundle can be published.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ProfileSurfaceBuildError {
    #[error("profile-surface construction was cancelled")]
    Cancelled,
    #[error("natural quality profile could not be resolved: {0}")]
    Profile(#[from] NaturalProfileError),
    #[error("authoritative surface construction failed: {0}")]
    AuthoritativeSurface(SphericalSurfaceBuildError),
    #[error("tectonic-control surface construction failed: {0}")]
    ControlSurface(SphericalSurfaceBuildError),
    #[error("control-to-authoritative map construction failed: {0}")]
    ConservativeRemap(ConservativeRemapError),
    #[error("P1 spatial quality evaluation failed: {0}")]
    Quality(#[from] QualityBuildError),
    #[error("constructed authoritative surface is invalid: {0}")]
    InvalidAuthoritativeSurface(SphericalSurfaceValidationError),
    #[error("constructed tectonic-control surface is invalid: {0}")]
    InvalidControlSurface(SphericalSurfaceValidationError),
    #[error("constructed conservative map is invalid: {0}")]
    InvalidConservativeMap(ConservativeSurfaceMapError),
    #[error("constructed P1 quality report is invalid: {0}")]
    InvalidQualityReport(String),
    #[error("{role} surface has {found} cells; expected resolved count {expected}")]
    ResolvedCellCountMismatch {
        role: &'static str,
        found: usize,
        expected: u32,
    },
    #[error("{role} radius {found} differs from profile radius {expected}")]
    RadiusMismatch {
        role: &'static str,
        found: f64,
        expected: f64,
    },
    #[error("{role} surface identity could not be constructed: {reason}")]
    InvalidSurfaceIdentity { role: &'static str, reason: String },
    #[error("{role} references {found:?}; expected {expected:?}")]
    SurfaceBindingMismatch {
        role: &'static str,
        found: SurfaceRef,
        expected: SurfaceRef,
    },
    #[error("hard P1 metric {metric} returned {status:?}")]
    QualityGateFailed {
        metric: String,
        status: QualityMetricStatus,
    },
}
