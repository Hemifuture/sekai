use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::geology::{
    validate_bedrock_crust_compatibility, validate_geologic_fields, GeologicFields,
    GEOLOGIC_SNAPSHOT_SCHEMA_V2,
};
use super::{
    BedrockKind, BedrockKindField, GeologicValidationError, SphericalMantleSnapshot,
    SphericalMantleValidationError, SphericalReliefSnapshot, SphericalReliefValidationError,
    SphericalTectonicSnapshot, SphericalTectonicValidationError,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceGeometryKind, SurfaceRef,
    SurfaceRefError,
};
use crate::world::{CellId, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT};

const MAX_SPHERICAL_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_SPHERICAL_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;

/// Immutable present-day geologic material fields bound to one spherical surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalGeologicSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    bedrock_kinds: BedrockKindField,
    fracture_intensity: Vec<f32>,
    erosion_resistance: Vec<f32>,
    relative_permeability: Vec<f32>,
    metallic_mineral_potential: Vec<f32>,
    geothermal_potential: Vec<f32>,
    sedimentary_basin_potential: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalGeologicSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    #[serde(deserialize_with = "deserialize_spherical_geologic_kind_values")]
    bedrock_kinds: Vec<u32>,
    #[serde(deserialize_with = "deserialize_spherical_geologic_float_values")]
    fracture_intensity: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_geologic_float_values")]
    erosion_resistance: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_geologic_float_values")]
    relative_permeability: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_geologic_float_values")]
    metallic_mineral_potential: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_geologic_float_values")]
    geothermal_potential: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_geologic_float_values")]
    sedimentary_basin_potential: Vec<f32>,
}

fn deserialize_spherical_geologic_float_values<'de, D>(
    deserializer: D,
) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_geologic_kind_values<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

impl SphericalGeologicSnapshot {
    /// Constructs a snapshot only when every surface-bound geology invariant holds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        bedrock_kinds: BedrockKindField,
        fracture_intensity: Vec<f32>,
        erosion_resistance: Vec<f32>,
        relative_permeability: Vec<f32>,
        metallic_mineral_potential: Vec<f32>,
        geothermal_potential: Vec<f32>,
        sedimentary_basin_potential: Vec<f32>,
    ) -> Result<Self, SphericalGeologicValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            bedrock_kinds,
            fracture_intensity,
            erosion_resistance,
            relative_permeability,
            metallic_mineral_potential,
            geothermal_potential,
            sedimentary_basin_potential,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks every invariant that does not need authoritative upstream records.
    pub fn validate(&self) -> Result<(), SphericalGeologicValidationError> {
        if self.schema_version != GEOLOGIC_SNAPSHOT_SCHEMA_V2 {
            return Err(SphericalGeologicValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: GEOLOGIC_SNAPSHOT_SCHEMA_V2,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(SphericalGeologicValidationError::InvalidSurfaceKind {
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
        validate_geologic_fields(
            self.surface_ref.cell_count(),
            GeologicFields {
                bedrock_kinds: &self.bedrock_kinds,
                fracture_intensity: &self.fracture_intensity,
                erosion_resistance: &self.erosion_resistance,
                relative_permeability: &self.relative_permeability,
                metallic_mineral_potential: &self.metallic_mineral_potential,
                geothermal_potential: &self.geothermal_potential,
                sedimentary_basin_potential: &self.sedimentary_basin_potential,
            },
        )?;
        Ok(())
    }

    /// Rechecks exact surface identity, upstream contracts, and crust compatibility.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
        tectonic: &SphericalTectonicSnapshot,
        mantle: &SphericalMantleSnapshot,
        relief: &SphericalReliefSnapshot,
    ) -> Result<(), SphericalGeologicValidationError> {
        surface.validate()?;
        tectonic.validate_against_validated_surface(surface)?;
        mantle.validate_against_validated_surface(surface)?;
        relief.validate_against_validated_surface(surface)?;
        self.validate_against_validated_surface(surface, tectonic)
    }

    pub(crate) fn validate_against_validated_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
        tectonic: &SphericalTectonicSnapshot,
    ) -> Result<(), SphericalGeologicValidationError> {
        self.validate()?;
        let authoritative = SurfaceRef::from_validated_spherical(surface)?;
        if self.surface_ref != authoritative {
            return Err(SphericalGeologicValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        validate_bedrock_crust_compatibility(
            self.surface_ref.cell_count(),
            &self.bedrock_kinds,
            tectonic.crust_kinds(),
        )?;
        Ok(())
    }

    /// Returns the V2 schema version.
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

    /// Returns stable raw and typed bedrock categories.
    pub const fn bedrock_kinds(&self) -> &BedrockKindField {
        &self.bedrock_kinds
    }

    /// Returns normalized current fracture intensity.
    pub fn fracture_intensity(&self) -> &[f32] {
        &self.fracture_intensity
    }

    /// Returns normalized resistance to erosion.
    pub fn erosion_resistance(&self) -> &[f32] {
        &self.erosion_resistance
    }

    /// Returns normalized relative permeability.
    pub fn relative_permeability(&self) -> &[f32] {
        &self.relative_permeability
    }

    /// Returns relative metallic-mineral formation potential.
    pub fn metallic_mineral_potential(&self) -> &[f32] {
        &self.metallic_mineral_potential
    }

    /// Returns relative geothermal potential.
    pub fn geothermal_potential(&self) -> &[f32] {
        &self.geothermal_potential
    }

    /// Returns relative sedimentary-basin formation potential.
    pub fn sedimentary_basin_potential(&self) -> &[f32] {
        &self.sedimentary_basin_potential
    }

    /// Returns the bedrock category for one cell.
    pub fn bedrock_kind(&self, cell: CellId) -> Option<BedrockKind> {
        self.bedrock_kinds.get(cell.raw() as usize)
    }
}

impl<'de> Deserialize<'de> for SphericalGeologicSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalGeologicSnapshotWire::deserialize(deserializer)?;
        let bedrock_kinds = BedrockKindField::new(wire.bedrock_kinds).map_err(D::Error::custom)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            bedrock_kinds,
            wire.fracture_intensity,
            wire.erosion_resistance,
            wire.relative_permeability,
            wire.metallic_mineral_potential,
            wire.geothermal_potential,
            wire.sedimentary_basin_potential,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_allocation_limit(
    field: &'static str,
    found: usize,
    max: usize,
) -> Result<(), SphericalGeologicValidationError> {
    if found > max {
        return Err(SphericalGeologicValidationError::AllocationExceedsLimit { field, found, max });
    }
    Ok(())
}

/// Failures in the surface-bound spherical geology contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalGeologicValidationError {
    /// The snapshot uses an unsupported schema version.
    #[error("unsupported spherical geology schema {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
        /// The supported schema version.
        supported: u16,
    },
    /// The stored surface identity is malformed.
    #[error("invalid surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    /// The stored identity does not describe spherical V1 geometry.
    #[error("spherical geology requires spherical_v1 geometry, found {found:?}")]
    InvalidSurfaceKind {
        /// The rejected geometry kind.
        found: SurfaceGeometryKind,
    },
    /// A surface identity exceeds the spherical allocation budget.
    #[error("{field} allocation {found} exceeds spherical limit {max}")]
    AllocationExceedsLimit {
        /// The bounded identity field.
        field: &'static str,
        /// The rejected allocation.
        found: usize,
        /// The inclusive maximum.
        max: usize,
    },
    /// A reused geology-field or bedrock/crust invariant failed.
    #[error("invalid geologic fields: {0}")]
    InvalidGeologicFields(#[from] GeologicValidationError),
    /// The authoritative spherical surface is invalid.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The exact surface identity differs from the authoritative snapshot.
    #[error("geology surface identity {snapshot:?} does not match {authoritative:?}")]
    SurfaceMismatch {
        /// Identity stored by this snapshot.
        snapshot: SurfaceRef,
        /// Identity derived from the authoritative surface.
        authoritative: SurfaceRef,
    },
    /// The tectonic upstream failed its authoritative spherical contract.
    #[error("invalid spherical tectonic upstream: {0}")]
    InvalidTectonic(#[from] SphericalTectonicValidationError),
    /// The mantle upstream failed its authoritative spherical contract.
    #[error("invalid spherical mantle upstream: {0}")]
    InvalidMantle(#[from] SphericalMantleValidationError),
    /// The relief upstream failed its authoritative spherical contract.
    #[error("invalid spherical relief upstream: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
}
