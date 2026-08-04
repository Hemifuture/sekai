use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::relief::{validate_relief_fields, ReliefFields, RELIEF_SCHEMA_V4};
use super::{
    ElevationField, LandOceanField, LandOceanKind, ReliefValidationError, SphericalMantleSnapshot,
    SphericalMantleValidationError, SphericalTectonicSnapshot, SphericalTectonicValidationError,
    VOLCANIC_OFFSET_MAX_M,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceGeometryKind, SurfaceRef,
    SurfaceRefError,
};
use crate::world::{CellId, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT};

const MAX_SPHERICAL_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_SPHERICAL_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;

/// Immutable explainable relief fields bound to one authoritative spherical surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalReliefSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    sea_level_m: f32,
    crust_base_elevation_m: ElevationField,
    tectonic_offset_m: ElevationField,
    volcanic_offset_m: ElevationField,
    regional_offset_m: ElevationField,
    elevation_m: ElevationField,
    land_ocean_kind: LandOceanField,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalReliefSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    sea_level_m: f32,
    #[serde(deserialize_with = "deserialize_spherical_relief_float_values")]
    crust_base_elevation_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_relief_float_values")]
    tectonic_offset_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_relief_float_values")]
    volcanic_offset_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_relief_float_values")]
    regional_offset_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_relief_float_values")]
    elevation_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_relief_kind_values")]
    land_ocean_kind: Vec<u32>,
}

fn deserialize_spherical_relief_float_values<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_relief_kind_values<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

impl SphericalReliefSnapshot {
    /// Constructs a snapshot only when every surface-bound relief invariant holds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        sea_level_m: f32,
        crust_base_elevation_m: ElevationField,
        tectonic_offset_m: ElevationField,
        volcanic_offset_m: ElevationField,
        regional_offset_m: ElevationField,
        elevation_m: ElevationField,
        land_ocean_kind: LandOceanField,
    ) -> Result<Self, SphericalReliefValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            sea_level_m,
            crust_base_elevation_m,
            tectonic_offset_m,
            volcanic_offset_m,
            regional_offset_m,
            elevation_m,
            land_ocean_kind,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks every invariant that does not need authoritative surface records.
    pub fn validate(&self) -> Result<(), SphericalReliefValidationError> {
        if self.schema_version != RELIEF_SCHEMA_V4 {
            return Err(SphericalReliefValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: RELIEF_SCHEMA_V4,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(SphericalReliefValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        validate_allocation_limit(
            "surface_ref.cell_count",
            self.surface_ref.cell_count() as usize,
            MAX_SPHERICAL_CELLS,
        )?;
        validate_allocation_limit(
            "surface_ref.edge_count",
            self.surface_ref.edge_count() as usize,
            MAX_SPHERICAL_EDGES,
        )?;
        validate_relief_fields(
            self.surface_ref.cell_count(),
            self.sea_level_m,
            VOLCANIC_OFFSET_MAX_M,
            ReliefFields {
                crust_base_elevation_m: &self.crust_base_elevation_m,
                tectonic_offset_m: &self.tectonic_offset_m,
                volcanic_offset_m: &self.volcanic_offset_m,
                regional_offset_m: &self.regional_offset_m,
                elevation_m: &self.elevation_m,
                land_ocean_kind: &self.land_ocean_kind,
            },
        )?;
        Ok(())
    }

    /// Rechecks exact surface identity and both authoritative upstream snapshots.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
        tectonic: &SphericalTectonicSnapshot,
        mantle: &SphericalMantleSnapshot,
    ) -> Result<(), SphericalReliefValidationError> {
        self.validate()?;
        surface.validate()?;
        let authoritative = SurfaceRef::from_validated_spherical(surface)?;
        if self.surface_ref != authoritative {
            return Err(SphericalReliefValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        tectonic.validate_against(surface)?;
        mantle.validate_against(surface)?;
        Ok(())
    }

    /// Returns the V4 schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact authoritative surface identity.
    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    /// Returns the dense cell allocation encoded by the authoritative surface.
    pub const fn cell_count(&self) -> u32 {
        self.surface_ref.cell_count()
    }

    /// Returns the global sea level in meters.
    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }

    /// Returns the crust and continental-margin baseline field.
    pub const fn crust_base_elevation_m(&self) -> &ElevationField {
        &self.crust_base_elevation_m
    }

    /// Returns the current tectonic-boundary contribution.
    pub const fn tectonic_offset_m(&self) -> &ElevationField {
        &self.tectonic_offset_m
    }

    /// Returns the current mantle-driven volcanic contribution.
    pub const fn volcanic_offset_m(&self) -> &ElevationField {
        &self.volcanic_offset_m
    }

    /// Returns the seamless regional-relief contribution.
    pub const fn regional_offset_m(&self) -> &ElevationField {
        &self.regional_offset_m
    }

    /// Returns final elevation in meters.
    pub const fn elevation_m(&self) -> &ElevationField {
        &self.elevation_m
    }

    /// Returns raw and typed land/ocean categories.
    pub const fn land_ocean(&self) -> &LandOceanField {
        &self.land_ocean_kind
    }

    /// Returns final elevation for one cell.
    pub fn elevation_for_cell(&self, cell: CellId) -> Option<f32> {
        self.elevation_m.get(cell.raw() as usize)
    }

    /// Returns the land/ocean category for one cell.
    pub fn land_ocean_kind(&self, cell: CellId) -> Option<LandOceanKind> {
        self.land_ocean_kind.get(cell.raw() as usize)
    }
}

impl<'de> Deserialize<'de> for SphericalReliefSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalReliefSnapshotWire::deserialize(deserializer)?;
        let crust_base_elevation_m =
            ElevationField::from_values(wire.crust_base_elevation_m).map_err(D::Error::custom)?;
        let tectonic_offset_m =
            ElevationField::from_values(wire.tectonic_offset_m).map_err(D::Error::custom)?;
        let volcanic_offset_m =
            ElevationField::from_values(wire.volcanic_offset_m).map_err(D::Error::custom)?;
        let regional_offset_m =
            ElevationField::from_values(wire.regional_offset_m).map_err(D::Error::custom)?;
        let elevation_m =
            ElevationField::from_values(wire.elevation_m).map_err(D::Error::custom)?;
        let land_ocean_kind =
            LandOceanField::from_raw(wire.land_ocean_kind).map_err(D::Error::custom)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.sea_level_m,
            crust_base_elevation_m,
            tectonic_offset_m,
            volcanic_offset_m,
            regional_offset_m,
            elevation_m,
            land_ocean_kind,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_allocation_limit(
    field: &'static str,
    found: usize,
    max: usize,
) -> Result<(), SphericalReliefValidationError> {
    if found > max {
        return Err(SphericalReliefValidationError::AllocationExceedsLimit { field, found, max });
    }
    Ok(())
}

/// Failures in the surface-bound spherical relief contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalReliefValidationError {
    /// The snapshot uses an unsupported schema version.
    #[error("unsupported spherical relief schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The stored surface identity is malformed.
    #[error("invalid surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    /// The stored identity does not describe spherical V1 geometry.
    #[error("spherical relief requires spherical_v1 geometry, found {found:?}")]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    /// A surface identity exceeds the spherical allocation budget.
    #[error("{field} allocation {found} exceeds spherical limit {max}")]
    AllocationExceedsLimit {
        field: &'static str,
        found: usize,
        max: usize,
    },
    /// A reused relief-field invariant failed.
    #[error("invalid relief fields: {0}")]
    InvalidReliefFields(#[from] ReliefValidationError),
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The exact surface identity differs from the authoritative snapshot.
    #[error("relief surface identity {snapshot:?} does not match {authoritative:?}")]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    /// The tectonic upstream failed its authoritative spherical contract.
    #[error("invalid spherical tectonic upstream: {0}")]
    InvalidTectonic(#[from] SphericalTectonicValidationError),
    /// The mantle upstream failed its authoritative spherical contract.
    #[error("invalid spherical mantle upstream: {0}")]
    InvalidMantle(#[from] SphericalMantleValidationError),
}
