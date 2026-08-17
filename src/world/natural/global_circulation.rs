use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::NaturalQualityProfile;
use crate::world::spatial::{
    ConservativeSurfaceMap, SphericalSurfaceSnapshot, SurfaceGeometryKind, SurfaceRef,
};

/// The first strict schema for the reconstructable climate work domain.
pub const CLIMATE_WORK_DOMAIN_SCHEMA_V1: u16 = 1;

/// A cubed-sphere climate grid and the exact conservative bridges to one
/// authoritative geodesic surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateWorkDomainSnapshot {
    schema_version: u16,
    profile: NaturalQualityProfile,
    face_resolution: u16,
    source_ref: SurfaceRef,
    climate_grid_fingerprint: [u8; 32],
    climate_surface: SphericalSurfaceSnapshot,
    source_to_climate: ConservativeSurfaceMap,
    climate_to_source: ConservativeSurfaceMap,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateWorkDomainSnapshotWire {
    schema_version: u16,
    profile: NaturalQualityProfile,
    face_resolution: u16,
    source_ref: SurfaceRef,
    climate_grid_fingerprint: [u8; 32],
    climate_surface: SphericalSurfaceSnapshot,
    source_to_climate: ConservativeSurfaceMap,
    climate_to_source: ConservativeSurfaceMap,
}

impl ClimateWorkDomainSnapshot {
    /// Constructs the domain only after all cross-object identities close.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        profile: NaturalQualityProfile,
        face_resolution: u16,
        source_ref: SurfaceRef,
        climate_grid_fingerprint: [u8; 32],
        climate_surface: SphericalSurfaceSnapshot,
        source_to_climate: ConservativeSurfaceMap,
        climate_to_source: ConservativeSurfaceMap,
    ) -> Result<Self, ClimateWorkDomainValidationError> {
        let snapshot = Self {
            schema_version,
            profile,
            face_resolution,
            source_ref,
            climate_grid_fingerprint,
            climate_surface,
            source_to_climate,
            climate_to_source,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks the self-contained schema, topology counts, and map identities.
    pub fn validate(&self) -> Result<(), ClimateWorkDomainValidationError> {
        if self.schema_version != CLIMATE_WORK_DOMAIN_SCHEMA_V1 {
            return Err(ClimateWorkDomainValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: CLIMATE_WORK_DOMAIN_SCHEMA_V1,
            });
        }
        let expected_resolution = self.profile.climate_face_resolution();
        if self.face_resolution != expected_resolution {
            return Err(ClimateWorkDomainValidationError::FaceResolutionMismatch {
                profile: self.profile,
                found: self.face_resolution,
                expected: expected_resolution,
            });
        }
        self.source_ref.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidSourceRef {
                reason: error.to_string(),
            }
        })?;
        if self.source_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(ClimateWorkDomainValidationError::NonSphericalSource);
        }
        if self.climate_grid_fingerprint == [0; 32] {
            return Err(ClimateWorkDomainValidationError::ZeroGridFingerprint);
        }
        self.climate_surface.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidClimateSurface {
                reason: error.to_string(),
            }
        })?;

        let resolution = u32::from(self.face_resolution);
        let expected_cells = 6_u32 * resolution * resolution;
        let expected_edges = 2 * expected_cells;
        let expected_vertices = expected_cells + 2;
        let found_cells = self.climate_surface.cells().len() as u32;
        let found_edges = self.climate_surface.edges().len() as u32;
        let found_vertices = self.climate_surface.vertices().len() as u32;
        if (found_cells, found_edges, found_vertices)
            != (expected_cells, expected_edges, expected_vertices)
        {
            return Err(ClimateWorkDomainValidationError::CubedSphereCountMismatch {
                found_cells,
                found_edges,
                found_vertices,
                expected_cells,
                expected_edges,
                expected_vertices,
            });
        }

        self.source_to_climate.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidMap {
                role: "source_to_climate",
                reason: error.to_string(),
            }
        })?;
        self.climate_to_source.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidMap {
                role: "climate_to_source",
                reason: error.to_string(),
            }
        })?;
        let climate_ref = SurfaceRef::for_spherical(&self.climate_surface);
        validate_map_identity(
            "source_to_climate",
            &self.source_to_climate,
            self.source_ref,
            climate_ref,
        )?;
        validate_map_identity(
            "climate_to_source",
            &self.climate_to_source,
            climate_ref,
            self.source_ref,
        )?;
        Ok(())
    }

    /// Binds the serialized source identity and radius to the supplied surface.
    pub fn validate_against(
        &self,
        source: &SphericalSurfaceSnapshot,
    ) -> Result<(), ClimateWorkDomainValidationError> {
        self.validate()?;
        source.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidAuthoritativeSurface {
                reason: error.to_string(),
            }
        })?;
        let found_ref = SurfaceRef::for_spherical(source);
        if found_ref != self.source_ref {
            return Err(ClimateWorkDomainValidationError::SourceMismatch {
                stored: self.source_ref,
                found: found_ref,
            });
        }
        if source.radius().get().to_bits() != self.climate_surface.radius().get().to_bits() {
            return Err(ClimateWorkDomainValidationError::RadiusMismatch {
                source_m: source.radius().get(),
                climate_m: self.climate_surface.radius().get(),
            });
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn profile(&self) -> NaturalQualityProfile {
        self.profile
    }

    pub const fn face_resolution(&self) -> u16 {
        self.face_resolution
    }

    pub const fn source_ref(&self) -> SurfaceRef {
        self.source_ref
    }

    pub const fn climate_grid_fingerprint(&self) -> &[u8; 32] {
        &self.climate_grid_fingerprint
    }

    pub const fn climate_surface(&self) -> &SphericalSurfaceSnapshot {
        &self.climate_surface
    }

    pub const fn source_to_climate(&self) -> &ConservativeSurfaceMap {
        &self.source_to_climate
    }

    pub const fn climate_to_source(&self) -> &ConservativeSurfaceMap {
        &self.climate_to_source
    }
}

impl<'de> Deserialize<'de> for ClimateWorkDomainSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateWorkDomainSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.profile,
            wire.face_resolution,
            wire.source_ref,
            wire.climate_grid_fingerprint,
            wire.climate_surface,
            wire.source_to_climate,
            wire.climate_to_source,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_map_identity(
    role: &'static str,
    map: &ConservativeSurfaceMap,
    expected_source: SurfaceRef,
    expected_target: SurfaceRef,
) -> Result<(), ClimateWorkDomainValidationError> {
    if map.source_ref() != expected_source || map.target_ref() != expected_target {
        return Err(ClimateWorkDomainValidationError::MapIdentityMismatch { role });
    }
    Ok(())
}

/// Invalid serialized or cross-linked climate work-domain data.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateWorkDomainValidationError {
    #[error("unsupported climate work-domain schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("{profile:?} climate face resolution is {found}, expected {expected}")]
    FaceResolutionMismatch {
        profile: NaturalQualityProfile,
        found: u16,
        expected: u16,
    },
    #[error("invalid authoritative source identity: {reason}")]
    InvalidSourceRef { reason: String },
    #[error("the climate work-domain source must be spherical")]
    NonSphericalSource,
    #[error("the climate work-grid fingerprint cannot be zero")]
    ZeroGridFingerprint,
    #[error("invalid climate surface: {reason}")]
    InvalidClimateSurface { reason: String },
    #[error("cubed-sphere counts are cells={found_cells}, edges={found_edges}, vertices={found_vertices}; expected cells={expected_cells}, edges={expected_edges}, vertices={expected_vertices}")]
    CubedSphereCountMismatch {
        found_cells: u32,
        found_edges: u32,
        found_vertices: u32,
        expected_cells: u32,
        expected_edges: u32,
        expected_vertices: u32,
    },
    #[error("invalid {role} conservative map: {reason}")]
    InvalidMap { role: &'static str, reason: String },
    #[error("{role} map source/target identity does not match the work domain")]
    MapIdentityMismatch { role: &'static str },
    #[error("invalid supplied authoritative surface: {reason}")]
    InvalidAuthoritativeSurface { reason: String },
    #[error("work-domain source identity {stored:?} does not match supplied surface {found:?}")]
    SourceMismatch {
        stored: SurfaceRef,
        found: SurfaceRef,
    },
    #[error("authoritative radius {source_m} m differs from climate radius {climate_m} m")]
    RadiusMismatch { source_m: f64, climate_m: f64 },
}
