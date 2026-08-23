use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{LandOceanField, LandOceanKind};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceRef, SurfaceRefError,
};
use crate::world::{CellId, EdgeId, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT};

const MAX_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;

/// Wire version for the unified P1 sub-cell surface-water geometry.
pub const SURFACE_WATER_GEOMETRY_SCHEMA_V1: u16 = 1;

/// One validated surface-bound interpretation of a sea level.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SurfaceWaterGeometry {
    schema_version: u16,
    surface_ref: SurfaceRef,
    elevation_fingerprint: [u8; 32],
    sea_level_m: f32,
    ocean_area_fraction: Vec<f32>,
    wet_edge_fraction: Vec<f32>,
    cell_water_volume_m3: Vec<f64>,
    land_ocean: LandOceanField,
    fingerprint: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceWaterGeometryWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    elevation_fingerprint: [u8; 32],
    sea_level_m: f32,
    #[serde(deserialize_with = "deserialize_cell_f32")]
    ocean_area_fraction: Vec<f32>,
    #[serde(deserialize_with = "deserialize_edge_f32")]
    wet_edge_fraction: Vec<f32>,
    #[serde(deserialize_with = "deserialize_cell_f64")]
    cell_water_volume_m3: Vec<f64>,
    land_ocean: LandOceanField,
    fingerprint: [u8; 32],
}

impl SurfaceWaterGeometry {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_cancellable(
        surface_ref: SurfaceRef,
        elevation_fingerprint: [u8; 32],
        sea_level_m: f32,
        ocean_area_fraction: Vec<f32>,
        wet_edge_fraction: Vec<f32>,
        cell_water_volume_m3: Vec<f64>,
        land_ocean: LandOceanField,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, SurfaceWaterGeometryValidationError> {
        let mut geometry = Self {
            schema_version: SURFACE_WATER_GEOMETRY_SCHEMA_V1,
            surface_ref,
            elevation_fingerprint,
            sea_level_m,
            ocean_area_fraction,
            wet_edge_fraction,
            cell_water_volume_m3,
            land_ocean,
            fingerprint: [0; 32],
        };
        geometry.validate_content(Some(cancelled))?;
        geometry.fingerprint = geometry.calculate_fingerprint(Some(cancelled))?;
        Ok(geometry)
    }

    /// Revalidates the self-contained wire and its content identity.
    pub fn validate(&self) -> Result<(), SurfaceWaterGeometryValidationError> {
        self.validate_impl(None)
    }

    fn validate_impl(
        &self,
        cancellation: Option<&dyn Fn() -> bool>,
    ) -> Result<(), SurfaceWaterGeometryValidationError> {
        self.validate_content(cancellation)?;
        if self.fingerprint != self.calculate_fingerprint(cancellation)? {
            return Err(SurfaceWaterGeometryValidationError::FingerprintMismatch);
        }
        check_cancelled(cancellation)
    }

    /// Binds cached geometry to its authoritative surface and center elevations.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
        elevation_m: &[f32],
    ) -> Result<(), SurfaceWaterGeometryValidationError> {
        self.validate()?;
        surface.validate()?;
        let authoritative = SurfaceRef::for_spherical(surface);
        if self.surface_ref != authoritative {
            return Err(SurfaceWaterGeometryValidationError::SurfaceMismatch {
                geometry: self.surface_ref,
                authoritative,
            });
        }
        validate_elevations(elevation_m, surface.cells().len())?;
        if self.elevation_fingerprint != surface_elevation_fingerprint(elevation_m) {
            return Err(SurfaceWaterGeometryValidationError::ElevationFingerprintMismatch);
        }
        for (index, &elevation) in elevation_m.iter().enumerate() {
            let expected = LandOceanKind::classify(elevation, self.sea_level_m);
            let found = self
                .land_ocean
                .get(index)
                .ok_or(SurfaceWaterGeometryValidationError::InvalidLandOceanKind { index })?;
            if found != expected {
                return Err(SurfaceWaterGeometryValidationError::LandOceanMismatch {
                    cell: CellId::from_raw(index as u32),
                    found,
                    expected,
                });
            }
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    pub const fn elevation_fingerprint(&self) -> &[u8; 32] {
        &self.elevation_fingerprint
    }

    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }

    pub fn ocean_area_fraction(&self) -> &[f32] {
        &self.ocean_area_fraction
    }

    /// Derives the complementary land fraction without storing a second fact.
    pub fn land_area_fraction(&self, index: usize) -> Option<f32> {
        self.ocean_area_fraction.get(index).map(|ocean| 1.0 - ocean)
    }

    pub fn wet_edge_fraction(&self) -> &[f32] {
        &self.wet_edge_fraction
    }

    pub fn cell_water_volume_m3(&self) -> &[f64] {
        &self.cell_water_volume_m3
    }

    pub const fn land_ocean(&self) -> &LandOceanField {
        &self.land_ocean
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    pub fn total_water_volume_m3(&self) -> f64 {
        compensated_sum(self.cell_water_volume_m3.iter().copied())
    }

    /// Derives the continuous area-weighted land share without a second mask.
    pub fn global_land_area_fraction(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<f32, SurfaceWaterGeometryValidationError> {
        self.validate()?;
        surface.validate()?;
        let authoritative = SurfaceRef::for_spherical(surface);
        if authoritative != self.surface_ref {
            return Err(SurfaceWaterGeometryValidationError::SurfaceMismatch {
                geometry: self.surface_ref,
                authoritative,
            });
        }
        let total_area = surface.total_cell_area().get();
        let land_area =
            compensated_sum(surface.cells().iter().zip(&self.ocean_area_fraction).map(
                |(cell, ocean_fraction)| cell.area.get() * (1.0 - f64::from(*ocean_fraction)),
            ));
        Ok((land_area / total_area) as f32)
    }

    /// Derives mean depth over only the wet sub-cell area.
    pub fn mean_wet_depth_m(
        &self,
        surface: &SphericalSurfaceSnapshot,
        cell: CellId,
    ) -> Option<f32> {
        if SurfaceRef::for_spherical(surface) != self.surface_ref {
            return None;
        }
        let index = cell.raw() as usize;
        let fraction = f64::from(*self.ocean_area_fraction.get(index)?);
        let area = surface.cell(cell)?.area.get() * fraction;
        if area == 0.0 {
            return Some(0.0);
        }
        Some((self.cell_water_volume_m3.get(index)? / area) as f32)
    }

    /// Resolves a cell-local edge through the one canonical wet-edge field.
    pub fn wet_fraction_for_cell_edge(
        &self,
        surface: &SphericalSurfaceSnapshot,
        cell: CellId,
        edge: EdgeId,
    ) -> Option<f32> {
        if SurfaceRef::for_spherical(surface) != self.surface_ref
            || !surface.cell_edges(cell)?.contains(&edge)
        {
            return None;
        }
        self.wet_edge_fraction.get(edge.raw() as usize).copied()
    }

    fn validate_content(
        &self,
        cancellation: Option<&dyn Fn() -> bool>,
    ) -> Result<(), SurfaceWaterGeometryValidationError> {
        check_cancelled(cancellation)?;
        if self.schema_version != SURFACE_WATER_GEOMETRY_SCHEMA_V1 {
            return Err(SurfaceWaterGeometryValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: SURFACE_WATER_GEOMETRY_SCHEMA_V1,
            });
        }
        self.surface_ref.validate()?;
        if !self.surface_ref.geometry_kind().is_spherical() {
            return Err(SurfaceWaterGeometryValidationError::NonSphericalSurface);
        }
        if self.elevation_fingerprint == [0; 32] {
            return Err(SurfaceWaterGeometryValidationError::ZeroElevationFingerprint);
        }
        if !self.sea_level_m.is_finite() {
            return Err(SurfaceWaterGeometryValidationError::InvalidSeaLevel {
                found: self.sea_level_m,
            });
        }
        let expected_cells = self.surface_ref.cell_count() as usize;
        let expected_edges = self.surface_ref.edge_count() as usize;
        for (field, expected, found) in [
            (
                "ocean_area_fraction",
                expected_cells,
                self.ocean_area_fraction.len(),
            ),
            (
                "cell_water_volume_m3",
                expected_cells,
                self.cell_water_volume_m3.len(),
            ),
            ("land_ocean", expected_cells, self.land_ocean.len()),
            (
                "wet_edge_fraction",
                expected_edges,
                self.wet_edge_fraction.len(),
            ),
        ] {
            if found != expected {
                return Err(SurfaceWaterGeometryValidationError::FieldLengthMismatch {
                    field,
                    expected,
                    found,
                });
            }
        }
        for (index, &value) in self.ocean_area_fraction.iter().enumerate() {
            poll_cancelled(index, cancellation)?;
            validate_fraction("ocean_area_fraction", index, value)?;
        }
        for (index, &value) in self.wet_edge_fraction.iter().enumerate() {
            poll_cancelled(index, cancellation)?;
            validate_fraction("wet_edge_fraction", index, value)?;
        }
        for (index, &value) in self.cell_water_volume_m3.iter().enumerate() {
            poll_cancelled(index, cancellation)?;
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SurfaceWaterGeometryValidationError::InvalidNonNegativeValue {
                        field: "cell_water_volume_m3",
                        index,
                        found: value,
                    },
                );
            }
            if self.land_ocean.get(index).is_none() {
                return Err(SurfaceWaterGeometryValidationError::InvalidLandOceanKind { index });
            }
        }
        check_cancelled(cancellation)
    }

    fn calculate_fingerprint(
        &self,
        cancellation: Option<&dyn Fn() -> bool>,
    ) -> Result<[u8; 32], SurfaceWaterGeometryValidationError> {
        check_cancelled(cancellation)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.surface-water-geometry.v1\0");
        hasher.update(&self.schema_version.to_le_bytes());
        let geometry_tag = match self.surface_ref.geometry_kind() {
            crate::world::spatial::SurfaceGeometryKind::PlanarV1 => 0_u8,
            crate::world::spatial::SurfaceGeometryKind::SphericalV1 => 1,
            crate::world::spatial::SurfaceGeometryKind::SphericalGeodesicV2 => 2,
        };
        hasher.update(&[geometry_tag]);
        hasher.update(&self.surface_ref.geometry_schema().to_le_bytes());
        hasher.update(&self.surface_ref.cell_count().to_le_bytes());
        hasher.update(&self.surface_ref.edge_count().to_le_bytes());
        hasher.update(&self.surface_ref.fingerprint());
        hasher.update(&self.elevation_fingerprint);
        hasher.update(&self.sea_level_m.to_bits().to_le_bytes());
        hash_f32_slice(&mut hasher, &self.ocean_area_fraction, cancellation)?;
        hash_f32_slice(&mut hasher, &self.wet_edge_fraction, cancellation)?;
        hash_f64_slice(&mut hasher, &self.cell_water_volume_m3, cancellation)?;
        hasher.update(&(self.land_ocean.len() as u64).to_le_bytes());
        for (index, value) in self.land_ocean.raw_values().iter().enumerate() {
            poll_cancelled(index, cancellation)?;
            hasher.update(&value.to_le_bytes());
        }
        check_cancelled(cancellation)?;
        Ok(*hasher.finalize().as_bytes())
    }
}

impl<'de> Deserialize<'de> for SurfaceWaterGeometry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SurfaceWaterGeometryWire::deserialize(deserializer)?;
        let geometry = Self {
            schema_version: wire.schema_version,
            surface_ref: wire.surface_ref,
            elevation_fingerprint: wire.elevation_fingerprint,
            sea_level_m: wire.sea_level_m,
            ocean_area_fraction: wire.ocean_area_fraction,
            wet_edge_fraction: wire.wet_edge_fraction,
            cell_water_volume_m3: wire.cell_water_volume_m3,
            land_ocean: wire.land_ocean,
            fingerprint: wire.fingerprint,
        };
        geometry.validate().map_err(D::Error::custom)?;
        Ok(geometry)
    }
}

pub(crate) fn surface_elevation_fingerprint(elevation_m: &[f32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.surface-elevation.v1\0");
    hasher.update(&(elevation_m.len() as u64).to_le_bytes());
    for value in elevation_m {
        hasher.update(&value.to_bits().to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

fn validate_elevations(
    elevation_m: &[f32],
    expected: usize,
) -> Result<(), SurfaceWaterGeometryValidationError> {
    if elevation_m.len() != expected {
        return Err(
            SurfaceWaterGeometryValidationError::ElevationLengthMismatch {
                expected,
                found: elevation_m.len(),
            },
        );
    }
    for (index, &found) in elevation_m.iter().enumerate() {
        if !found.is_finite() {
            return Err(SurfaceWaterGeometryValidationError::InvalidElevation { index, found });
        }
    }
    Ok(())
}

fn validate_fraction(
    field: &'static str,
    index: usize,
    found: f32,
) -> Result<(), SurfaceWaterGeometryValidationError> {
    if !found.is_finite() || !(0.0..=1.0).contains(&found) {
        return Err(SurfaceWaterGeometryValidationError::InvalidFraction {
            field,
            index,
            found,
        });
    }
    Ok(())
}

fn hash_f32_slice(
    hasher: &mut blake3::Hasher,
    values: &[f32],
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), SurfaceWaterGeometryValidationError> {
    hasher.update(&(values.len() as u64).to_le_bytes());
    for (index, value) in values.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        hasher.update(&value.to_bits().to_le_bytes());
    }
    Ok(())
}

fn hash_f64_slice(
    hasher: &mut blake3::Hasher,
    values: &[f64],
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), SurfaceWaterGeometryValidationError> {
    hasher.update(&(values.len() as u64).to_le_bytes());
    for (index, value) in values.iter().enumerate() {
        poll_cancelled(index, cancellation)?;
        hasher.update(&value.to_bits().to_le_bytes());
    }
    Ok(())
}

fn compensated_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = 0.0_f64;
    let mut correction = 0.0_f64;
    for value in values {
        let next = sum + value;
        if sum.abs() >= value.abs() {
            correction += (sum - next) + value;
        } else {
            correction += (value - next) + sum;
        }
        sum = next;
    }
    sum + correction
}

fn poll_cancelled(
    index: usize,
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), SurfaceWaterGeometryValidationError> {
    if index & 255 == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(
    cancellation: Option<&dyn Fn() -> bool>,
) -> Result<(), SurfaceWaterGeometryValidationError> {
    if cancellation.is_some_and(|cancelled| cancelled()) {
        Err(SurfaceWaterGeometryValidationError::Cancelled)
    } else {
        Ok(())
    }
}

fn deserialize_cell_f32<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<D, f32, MAX_CELLS>(deserializer)
}

fn deserialize_edge_f32<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<D, f32, MAX_EDGES>(deserializer)
}

fn deserialize_cell_f64<'de, D>(deserializer: D) -> Result<Vec<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<D, f64, MAX_CELLS>(deserializer)
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SurfaceWaterGeometryValidationError {
    #[error("surface-water geometry validation cancelled")]
    Cancelled,
    #[error("unsupported surface-water geometry schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("invalid surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    #[error("surface-water geometry requires a spherical surface")]
    NonSphericalSurface,
    #[error("surface-water geometry elevation fingerprint cannot be zero")]
    ZeroElevationFingerprint,
    #[error("invalid surface-water sea level {found}")]
    InvalidSeaLevel { found: f32 },
    #[error("{field} length {found} differs from expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("{field} fraction {found} at index {index} is outside 0..=1")]
    InvalidFraction {
        field: &'static str,
        index: usize,
        found: f32,
    },
    #[error("{field} value {found} at index {index} must be finite and non-negative")]
    InvalidNonNegativeValue {
        field: &'static str,
        index: usize,
        found: f64,
    },
    #[error("invalid land/ocean category at dense index {index}")]
    InvalidLandOceanKind { index: usize },
    #[error("surface-water geometry fingerprint mismatch")]
    FingerprintMismatch,
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("geometry surface {geometry:?} differs from authority {authoritative:?}")]
    SurfaceMismatch {
        geometry: SurfaceRef,
        authoritative: SurfaceRef,
    },
    #[error("elevation length {found} differs from expected {expected}")]
    ElevationLengthMismatch { expected: usize, found: usize },
    #[error("invalid elevation {found} at dense index {index}")]
    InvalidElevation { index: usize, found: f32 },
    #[error("surface-water geometry elevation fingerprint mismatch")]
    ElevationFingerprintMismatch,
    #[error("cell {cell:?} land/ocean kind {found:?} differs from {expected:?}")]
    LandOceanMismatch {
        cell: CellId,
        found: LandOceanKind,
        expected: LandOceanKind,
    },
}
