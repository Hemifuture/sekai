use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::surface_process::{
    validate_f32_range, validate_length, validate_sediment_volume,
    validate_surface_process_relations,
};
use super::{
    ElevationField, SphericalReliefSnapshot, SphericalReliefValidationError,
    SurfaceProcessValidationError, ELEVATION_MAX_M, ELEVATION_MIN_M, MAX_DEPOSITION_THICKNESS_M,
    MAX_EROSION_DEPTH_M, SURFACE_PROCESS_SCHEMA_V2,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceGeometryKind, SurfaceRef,
    SurfaceRefError,
};
use crate::world::{CellId, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT};

const MAX_SPHERICAL_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_SPHERICAL_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;

/// Bounded current-state fluvial formation fields on one exact closed sphere.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphericalSurfaceProcessSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    erosion_depth_m: Vec<f32>,
    deposition_thickness_m: Vec<f32>,
    surface_elevation_m: ElevationField,
    sediment_throughput_m3: Vec<f64>,
    sediment_ocean_delivery_m3: f64,
    sediment_endorheic_storage_m3: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphericalSurfaceProcessSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    #[serde(deserialize_with = "deserialize_spherical_f32_values")]
    erosion_depth_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_f32_values")]
    deposition_thickness_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_f32_values")]
    surface_elevation_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_spherical_f64_values")]
    sediment_throughput_m3: Vec<f64>,
    sediment_ocean_delivery_m3: f64,
    sediment_endorheic_storage_m3: f64,
}

fn deserialize_spherical_f32_values<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_spherical_f64_values<'de, D>(deserializer: D) -> Result<Vec<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

impl SphericalSurfaceProcessSnapshot {
    /// Constructs a V2 snapshot only when all local surface-process invariants hold.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        erosion_depth_m: Vec<f32>,
        deposition_thickness_m: Vec<f32>,
        surface_elevation_m: ElevationField,
        sediment_throughput_m3: Vec<f64>,
        sediment_ocean_delivery_m3: f64,
        sediment_endorheic_storage_m3: f64,
    ) -> Result<Self, SphericalSurfaceProcessValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            erosion_depth_m,
            deposition_thickness_m,
            surface_elevation_m,
            sediment_throughput_m3,
            sediment_ocean_delivery_m3,
            sediment_endorheic_storage_m3,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks all V2 invariants that do not need authoritative surface records.
    pub fn validate(&self) -> Result<(), SphericalSurfaceProcessValidationError> {
        if self.schema_version != SURFACE_PROCESS_SCHEMA_V2 {
            return Err(SphericalSurfaceProcessValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: SURFACE_PROCESS_SCHEMA_V2,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(SphericalSurfaceProcessValidationError::InvalidSurfaceKind {
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

        let count = self.surface_ref.cell_count();
        validate_length("erosion_depth_m", self.erosion_depth_m.len(), count)?;
        validate_length(
            "deposition_thickness_m",
            self.deposition_thickness_m.len(),
            count,
        )?;
        validate_length("surface_elevation_m", self.surface_elevation_m.len(), count)?;
        validate_length(
            "sediment_throughput_m3",
            self.sediment_throughput_m3.len(),
            count,
        )?;
        validate_f32_range(
            "erosion_depth_m",
            &self.erosion_depth_m,
            0.0,
            MAX_EROSION_DEPTH_M,
        )?;
        validate_f32_range(
            "deposition_thickness_m",
            &self.deposition_thickness_m,
            0.0,
            MAX_DEPOSITION_THICKNESS_M,
        )?;
        validate_f32_range(
            "surface_elevation_m",
            self.surface_elevation_m.values(),
            ELEVATION_MIN_M,
            ELEVATION_MAX_M,
        )?;
        for (index, &found) in self.sediment_throughput_m3.iter().enumerate() {
            validate_sediment_volume(
                "sediment_throughput_m3",
                Some(CellId::from_raw(index as u32)),
                found,
            )?;
        }
        validate_sediment_volume(
            "sediment_ocean_delivery_m3",
            None,
            self.sediment_ocean_delivery_m3,
        )?;
        validate_sediment_volume(
            "sediment_endorheic_storage_m3",
            None,
            self.sediment_endorheic_storage_m3,
        )?;
        if !self.sediment_terminal_transfer_m3().is_finite() {
            return Err(
                SphericalSurfaceProcessValidationError::TerminalTransferOverflow {
                    ocean_delivery_m3: self.sediment_ocean_delivery_m3,
                    endorheic_storage_m3: self.sediment_endorheic_storage_m3,
                },
            );
        }
        Ok(())
    }

    /// Validates exact surface identity, component identity, ocean behavior, and mass closure.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
    ) -> Result<(), SphericalSurfaceProcessValidationError> {
        surface.validate()?;
        relief.validate_against_validated_surface(surface)?;
        self.validate_against_validated_surface(surface, relief)
    }

    pub(crate) fn validate_against_validated_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief: &SphericalReliefSnapshot,
    ) -> Result<(), SphericalSurfaceProcessValidationError> {
        self.validate()?;
        let authoritative = SurfaceRef::from_validated_spherical(surface)?;
        if self.surface_ref != authoritative {
            return Err(SphericalSurfaceProcessValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        if relief.surface_ref() != authoritative {
            return Err(
                SphericalSurfaceProcessValidationError::ReliefSurfaceMismatch {
                    relief: relief.surface_ref(),
                    authoritative,
                },
            );
        }
        validate_surface_process_relations(
            self.cell_count(),
            &self.erosion_depth_m,
            &self.deposition_thickness_m,
            &self.surface_elevation_m,
            self.sediment_terminal_transfer_m3(),
            surface.cells().len(),
            relief.cell_count(),
            relief.elevation_m(),
            |cell| relief.land_ocean_kind(cell),
            |cell| {
                surface
                    .cell(cell)
                    .expect("validated spherical cells are dense")
                    .area
                    .get()
            },
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

    /// Returns the dense cell allocation encoded in the surface identity.
    pub const fn cell_count(&self) -> u32 {
        self.surface_ref.cell_count()
    }

    /// Returns bounded fluvial incision depths.
    pub fn erosion_depth_m(&self) -> &[f32] {
        &self.erosion_depth_m
    }

    /// Returns bounded local sediment deposition thicknesses.
    pub fn deposition_thickness_m(&self) -> &[f32] {
        &self.deposition_thickness_m
    }

    /// Returns the current post-process surface elevation.
    pub const fn surface_elevation_m(&self) -> &ElevationField {
        &self.surface_elevation_m
    }

    /// Returns sediment leaving each cell's local transport balance.
    pub fn sediment_throughput_m3(&self) -> &[f64] {
        &self.sediment_throughput_m3
    }

    /// Returns sediment delivered from the fluvial domain into the ocean reservoir.
    pub const fn sediment_ocean_delivery_m3(&self) -> f64 {
        self.sediment_ocean_delivery_m3
    }

    /// Returns sediment retained by terminal endorheic reservoirs.
    pub const fn sediment_endorheic_storage_m3(&self) -> f64 {
        self.sediment_endorheic_storage_m3
    }

    /// Returns all sediment transferred out of the routed fluvial surface domain.
    pub const fn sediment_terminal_transfer_m3(&self) -> f64 {
        self.sediment_ocean_delivery_m3 + self.sediment_endorheic_storage_m3
    }
}

impl<'de> Deserialize<'de> for SphericalSurfaceProcessSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SphericalSurfaceProcessSnapshotWire::deserialize(deserializer)?;
        let surface_elevation_m =
            ElevationField::from_values(wire.surface_elevation_m).map_err(D::Error::custom)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.erosion_depth_m,
            wire.deposition_thickness_m,
            surface_elevation_m,
            wire.sediment_throughput_m3,
            wire.sediment_ocean_delivery_m3,
            wire.sediment_endorheic_storage_m3,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_allocation_limit(
    field: &'static str,
    found: usize,
    max: usize,
) -> Result<(), SphericalSurfaceProcessValidationError> {
    if found > max {
        return Err(
            SphericalSurfaceProcessValidationError::AllocationLimitExceeded { field, found, max },
        );
    }
    Ok(())
}

/// Errors returned when spherical current-surface fields violate their V2 contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalSurfaceProcessValidationError {
    /// The outer V2 schema is unsupported.
    #[error(
        "unsupported spherical surface-process schema {found}; supported version is {supported}"
    )]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The exact surface identity is malformed.
    #[error("invalid spherical surface-process identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    /// The identity does not refer to spherical geometry.
    #[error("spherical surface process requires SphericalV1 geometry, found {found:?}")]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    /// A declared allocation exceeds the supported spherical schema budget.
    #[error("field {field} declares {found} records; maximum is {max}")]
    AllocationLimitExceeded {
        field: &'static str,
        found: usize,
        max: usize,
    },
    /// The two terminal totals overflow when combined.
    #[error(
        "sediment terminal transfer overflows: ocean {ocean_delivery_m3}, endorheic {endorheic_storage_m3}"
    )]
    TerminalTransferOverflow {
        ocean_delivery_m3: f64,
        endorheic_storage_m3: f64,
    },
    /// The snapshot references another authoritative surface.
    #[error("surface-process identity {snapshot:?} does not match {authoritative:?}")]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    /// The relief upstream references another authoritative surface.
    #[error("relief identity {relief:?} does not match {authoritative:?}")]
    ReliefSurfaceMismatch {
        relief: SurfaceRef,
        authoritative: SurfaceRef,
    },
    /// The authoritative surface is invalid.
    #[error("invalid spherical surface input: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    /// The constructional relief is invalid or incompatible.
    #[error("invalid spherical relief input: {0}")]
    InvalidRelief(#[from] SphericalReliefValidationError),
    /// Shared component, range, ocean, or mass semantics are invalid.
    #[error("invalid surface-process semantics: {0}")]
    InvalidProcess(#[from] SurfaceProcessValidationError),
}
