use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::hydro_erosion::validate_hydro_erosion_semantics;
use super::{
    HydroErosionValidationError, SphericalClimateValidationError, SphericalGeologicSnapshot,
    SphericalGeologicValidationError, SphericalHydrologySnapshot,
    SphericalHydrologyValidationError, SphericalPreliminaryClimateSnapshot,
    SphericalReliefSnapshot, SphericalReliefValidationError, SphericalSurfaceProcessSnapshot,
    SphericalSurfaceProcessValidationError, HYDRO_EROSION_SNAPSHOT_SCHEMA_V2,
};
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceRef, SurfaceRefError,
};

/// Atomic current-surface and final-hydrology output on one exact closed sphere.
///
/// The constructional hydrology used to drive erosion is deliberately transient. Only the
/// post-erosion surface and the hydrology recomputed from that surface can be published together.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalHydroErosionSnapshot {
    schema_version: u16,
    surface: SphericalSurfaceProcessSnapshot,
    hydrology: SphericalHydrologySnapshot,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalHydroErosionSnapshotWire {
    schema_version: u16,
    surface: SphericalSurfaceProcessSnapshot,
    hydrology: SphericalHydrologySnapshot,
}

impl SphericalHydroErosionSnapshot {
    /// Constructs an atomic V2 snapshot only when both outputs share one exact surface identity.
    pub fn new(
        schema_version: u16,
        surface: SphericalSurfaceProcessSnapshot,
        hydrology: SphericalHydrologySnapshot,
    ) -> Result<Self, SphericalHydroErosionValidationError> {
        let snapshot = Self {
            schema_version,
            surface,
            hydrology,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks the strict envelope and both geometry-specific subcontracts.
    pub fn validate(&self) -> Result<(), SphericalHydroErosionValidationError> {
        if self.schema_version != HYDRO_EROSION_SNAPSHOT_SCHEMA_V2 {
            return Err(SphericalHydroErosionValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: HYDRO_EROSION_SNAPSHOT_SCHEMA_V2,
            });
        }
        self.surface.validate()?;
        self.hydrology.validate()?;
        if self.surface.cell_count() != self.hydrology.cell_count() {
            return Err(SphericalHydroErosionValidationError::CellCountMismatch {
                surface: self.surface.cell_count(),
                hydrology: self.hydrology.cell_count(),
            });
        }
        if self.surface.surface_ref() != self.hydrology.surface_ref() {
            return Err(
                SphericalHydroErosionValidationError::SubsnapshotSurfaceMismatch {
                    surface: self.surface.surface_ref(),
                    hydrology: self.hydrology.surface_ref(),
                },
            );
        }
        Ok(())
    }

    /// Validates exact upstream identity and all current-surface water/runoff identities.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
        geology: &SphericalGeologicSnapshot,
        climate: &SphericalPreliminaryClimateSnapshot,
    ) -> Result<(), SphericalHydroErosionValidationError> {
        self.validate()?;
        surface.validate()?;
        relief.validate_against_validated_surface(surface)?;
        self.validate_against_validated_surface(surface, relief, geology, climate)
    }

    /// Rechecks cross-domain identities when the authoritative surface and relief are validated.
    pub(crate) fn validate_against_validated_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
        geology: &SphericalGeologicSnapshot,
        climate: &SphericalPreliminaryClimateSnapshot,
    ) -> Result<(), SphericalHydroErosionValidationError> {
        self.validate()?;
        geology.validate()?;
        climate.validate_against_validated_surface(surface, relief)?;
        self.surface
            .validate_against_validated_surface(surface, relief)?;
        self.hydrology.validate_against_validated_surface(surface)?;

        let authoritative = SurfaceRef::from_validated_spherical(surface)?;
        if geology.surface_ref() != authoritative {
            return Err(
                SphericalHydroErosionValidationError::GeologySurfaceMismatch {
                    geology: geology.surface_ref(),
                    authoritative,
                },
            );
        }

        validate_hydro_erosion_semantics(
            self.cell_count(),
            self.surface.surface_elevation_m(),
            self.hydrology.semantic_payload(),
            relief.sea_level_m(),
            geology.relative_permeability(),
            climate.monthly_precipitation_mm().values(),
        )?;
        Ok(())
    }

    /// Returns the strict spherical envelope schema.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the one authoritative surface identity shared by both outputs.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface.surface_ref()
    }

    /// Returns the shared dense cell allocation.
    pub const fn cell_count(&self) -> u32 {
        self.surface.cell_count()
    }

    /// Returns the bounded post-erosion current surface.
    pub const fn surface(&self) -> &SphericalSurfaceProcessSnapshot {
        &self.surface
    }

    /// Returns hydrology recomputed from the published current surface.
    pub const fn hydrology(&self) -> &SphericalHydrologySnapshot {
        &self.hydrology
    }
}

impl<'de> Deserialize<'de> for SphericalHydroErosionSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalHydroErosionSnapshotWire::deserialize(deserializer)?;
        Self::new(wire.schema_version, wire.surface, wire.hydrology).map_err(D::Error::custom)
    }
}

/// Failures in the atomic closed-sphere hydro-erosion contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalHydroErosionValidationError {
    /// The outer envelope schema is unsupported.
    #[error(
        "unsupported spherical hydro-erosion schema {found}; supported version is {supported}"
    )]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The sub-snapshots declare different dense cardinalities.
    #[error("surface cell count {surface} does not match hydrology count {hydrology}")]
    CellCountMismatch { surface: u32, hydrology: u32 },
    /// The sub-snapshots belong to different exact closed spheres.
    #[error(
        "surface-process identity {surface:?} does not match hydrology identity {hydrology:?}"
    )]
    SubsnapshotSurfaceMismatch {
        surface: SurfaceRef,
        hydrology: SurfaceRef,
    },
    /// The geology upstream belongs to another exact closed sphere.
    #[error("geology identity {geology:?} does not match authoritative surface {authoritative:?}")]
    GeologySurfaceMismatch {
        geology: SurfaceRef,
        authoritative: SurfaceRef,
    },
    /// The authoritative surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The authoritative identity could not be derived.
    #[error("invalid spherical surface identity: {0}")]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The constructional relief is invalid or incompatible.
    #[error("invalid spherical relief: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
    /// The geologic substrate is invalid.
    #[error("invalid spherical geology: {0}")]
    InvalidGeology(#[from] SphericalGeologicValidationError),
    /// The preliminary climate is invalid or incompatible.
    #[error("invalid spherical climate: {0}")]
    InvalidClimate(#[from] SphericalClimateValidationError),
    /// The current-surface sub-snapshot is invalid or incompatible.
    #[error("invalid spherical surface-process snapshot: {0}")]
    InvalidSurfaceProcess(#[from] SphericalSurfaceProcessValidationError),
    /// The final hydrology sub-snapshot is invalid or incompatible.
    #[error("invalid spherical hydrology snapshot: {0}")]
    InvalidHydrology(#[from] SphericalHydrologyValidationError),
    /// A shared current-surface, lake, ocean, or runoff identity failed.
    #[error("invalid hydro-erosion semantics: {0}")]
    InvalidSemantics(#[from] HydroErosionValidationError),
}
