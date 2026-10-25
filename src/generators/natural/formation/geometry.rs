//! Seed-independent geometry of one formation quality profile and planet radius.

use thiserror::Error;

use super::{ClimateWorkDomainArtifact, ClimateWorkDomainBuildError};
use crate::engine::{ArtifactError, BuildCancellation, PreparedArtifact};
use crate::generators::spatial::{
    ProfileSurfaceBuildError, ProfileSurfaceBuilder, ProfileSurfaceBundle, SphericalSurfaceArtifact,
};
use crate::world::natural::NaturalQualityProfile;
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use crate::world::Meters;

/// Every input of [`FormationGeometry`]: the quality profile and the exact planet radius.
///
/// Radii compare by their bits, so a geometry is reused only for the very radius it was built
/// from.
#[derive(Debug, Clone, Copy)]
pub struct FormationGeometryKey {
    profile: NaturalQualityProfile,
    radius: Meters,
}

impl FormationGeometryKey {
    /// Names the geometry of `profile` on a planet of `radius`.
    pub const fn new(profile: NaturalQualityProfile, radius: Meters) -> Self {
        Self { profile, radius }
    }
}

impl PartialEq for FormationGeometryKey {
    fn eq(&self, other: &Self) -> bool {
        self.profile == other.profile && self.radius.get().to_bits() == other.radius.get().to_bits()
    }
}

impl Eq for FormationGeometryKey {}

/// The formation geometry that depends only on the quality profile and the planet radius.
///
/// It is the authoritative surface, the P1 control grid with its conservative map and quality
/// report, and the P4 climate work domain, each built and validated once and the engine
/// artifacts also content-hashed once. A build that receives this value publishes exactly what
/// its own stages would have built from the same profile and radius; nothing here depends on
/// the root seed or on any authored world parameter.
#[derive(Debug)]
pub struct FormationGeometry {
    key: FormationGeometryKey,
    surface_ref: SurfaceRef,
    surface: PreparedArtifact<SphericalSurfaceArtifact>,
    profile_bundle: ProfileSurfaceBundle,
    climate_domain: PreparedArtifact<ClimateWorkDomainArtifact>,
}

impl FormationGeometry {
    /// Builds every seed-independent formation geometry named by `key`.
    ///
    /// # Errors
    ///
    /// Returns the P1 surface, climate work-domain or artifact failure (each carrying its own
    /// cancellation variant) that the formation stages would report for the same inputs.
    pub fn build(
        key: FormationGeometryKey,
        cancellation: &BuildCancellation,
    ) -> Result<Self, FormationGeometryError> {
        let profile_bundle = ProfileSurfaceBuilder::build(key.profile, key.radius, cancellation)?;
        let authoritative = profile_bundle.authoritative_surface();
        let climate_domain = PreparedArtifact::new_cancellable(
            ClimateWorkDomainArtifact::build(authoritative, key.profile, cancellation)?,
            cancellation,
        )?;
        let surface = PreparedArtifact::new_cancellable(
            SphericalSurfaceArtifact::new(authoritative.clone()),
            cancellation,
        )?;
        Ok(Self {
            key,
            surface_ref: SurfaceRef::for_spherical(authoritative),
            surface,
            profile_bundle,
            climate_domain,
        })
    }

    /// Returns the inputs the geometry was built from.
    pub const fn key(&self) -> FormationGeometryKey {
        self.key
    }

    /// Returns the quality profile the geometry was built for.
    pub const fn profile(&self) -> NaturalQualityProfile {
        self.key.profile
    }

    /// Borrows the authoritative surface.
    pub fn authoritative_surface(&self) -> &SphericalSurfaceSnapshot {
        self.profile_bundle.authoritative_surface()
    }

    /// Returns the prepared engine artifact of the authoritative surface.
    pub const fn surface_artifact(&self) -> &PreparedArtifact<SphericalSurfaceArtifact> {
        &self.surface
    }

    /// Returns the P1 bundle when `surface` is this geometry's authority under `profile`.
    pub(in crate::generators::natural) fn profile_bundle_for(
        &self,
        profile: NaturalQualityProfile,
        surface: &SphericalSurfaceSnapshot,
    ) -> Option<&ProfileSurfaceBundle> {
        self.binds(profile, surface).then_some(&self.profile_bundle)
    }

    /// Returns the climate work domain when `surface` is this geometry's authority under
    /// `profile`.
    pub(in crate::generators::natural) fn climate_domain_for(
        &self,
        profile: NaturalQualityProfile,
        surface: &SphericalSurfaceSnapshot,
    ) -> Option<&PreparedArtifact<ClimateWorkDomainArtifact>> {
        self.binds(profile, surface).then_some(&self.climate_domain)
    }

    /// The P1 bundle and the climate domain are functions of the authoritative surface and the
    /// profile alone, and the surface identity carries the full geometry fingerprint.
    fn binds(&self, profile: NaturalQualityProfile, surface: &SphericalSurfaceSnapshot) -> bool {
        self.key.profile == profile && SurfaceRef::for_spherical(surface) == self.surface_ref
    }
}

/// Geometry failures, each the one the formation stages report for the same inputs.
#[derive(Debug, Error)]
pub enum FormationGeometryError {
    /// The P1 profile surfaces could not be built.
    #[error(transparent)]
    Surface(#[from] ProfileSurfaceBuildError),
    /// The P4 climate work domain could not be built.
    #[error(transparent)]
    ClimateWorkDomain(#[from] ClimateWorkDomainBuildError),
    /// A prepared engine artifact could not be validated or hashed.
    #[error(transparent)]
    Artifact(#[from] ArtifactError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ExternalArtifacts;

    /// The prewarm contract: every prepared part equals what the formation stages build from
    /// the same profile and surface, hashes as the engine would, and binds to nothing else.
    #[test]
    fn prepared_geometry_matches_the_stage_builds_and_binds_only_to_its_key() {
        let cancellation = BuildCancellation::new();
        let radius = Meters::new(6_371_000.0).expect("Earth radius is positive");
        let key = FormationGeometryKey::new(NaturalQualityProfile::Draft, radius);
        let geometry = FormationGeometry::build(key, &cancellation).expect("Draft geometry builds");
        let surface = geometry.authoritative_surface();

        let completed =
            ProfileSurfaceBuilder::complete(NaturalQualityProfile::Draft, surface, &cancellation)
                .expect("the stage completes the same surface");
        let domain =
            ClimateWorkDomainArtifact::build(surface, NaturalQualityProfile::Draft, &cancellation)
                .expect("the stage builds the same domain");
        let mut fresh = ExternalArtifacts::new();
        fresh
            .insert(SphericalSurfaceArtifact::new(surface.clone()))
            .expect("the surface publishes");
        let mut prepared = ExternalArtifacts::new();
        prepared
            .insert_prepared(geometry.surface_artifact())
            .expect("the prepared surface publishes");

        assert_eq!(
            geometry.profile_bundle_for(NaturalQualityProfile::Draft, surface),
            Some(&completed)
        );
        assert_eq!(
            geometry
                .climate_domain_for(NaturalQualityProfile::Draft, surface)
                .map(PreparedArtifact::get),
            Some(&domain)
        );
        assert_eq!(
            prepared.hash::<SphericalSurfaceArtifact>().unwrap(),
            fresh.hash::<SphericalSurfaceArtifact>().unwrap()
        );
        assert!(geometry
            .profile_bundle_for(NaturalQualityProfile::Standard, surface)
            .is_none());
        assert!(geometry
            .climate_domain_for(
                NaturalQualityProfile::Draft,
                completed.tectonic_control_surface()
            )
            .is_none());
        assert_eq!(geometry.key(), key);
        assert_ne!(
            FormationGeometryKey::new(NaturalQualityProfile::Standard, radius),
            key
        );
        assert_ne!(
            FormationGeometryKey::new(
                NaturalQualityProfile::Draft,
                Meters::new(3_000_000.0).expect("positive radius")
            ),
            key
        );
    }
}
