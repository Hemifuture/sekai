use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    ElevationField, LandOceanKind, ReliefSnapshot, ReliefValidationError, ELEVATION_MAX_M,
    ELEVATION_MIN_M,
};
use crate::world::spatial::{SpatialSnapshot, SpatialValidationError, Topology};
use crate::world::CellId;

/// The supported version of the serialized surface-process schema.
pub const SURFACE_PROCESS_SCHEMA_V1: u16 = 1;
/// The hard safety bound for fluvial incision depth in one current-slice solve.
pub const MAX_EROSION_DEPTH_M: f32 = 5_000.0;
/// The hard safety bound for sediment deposition thickness in one current-slice solve.
pub const MAX_DEPOSITION_THICKNESS_M: f32 = 5_000.0;
/// The allowed rounding difference in the current-surface component identity.
pub const SURFACE_IDENTITY_TOLERANCE_M: f32 = 0.05;
/// Relative tolerance used when comparing world sediment volumes.
pub const SEDIMENT_VOLUME_RELATIVE_TOLERANCE: f64 = 1.0e-6;
/// Absolute floor for sediment-volume comparisons, in cubic meters.
pub const SEDIMENT_VOLUME_ABSOLUTE_TOLERANCE_M3: f64 = 1.0e-6;

/// Immutable fluvial erosion, deposition, and current-surface fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SurfaceProcessSnapshot {
    schema_version: u16,
    cell_count: u32,
    erosion_depth_m: Vec<f32>,
    deposition_thickness_m: Vec<f32>,
    surface_elevation_m: ElevationField,
    sediment_throughput_m3: Vec<f64>,
    sediment_export_m3: f64,
}

#[derive(Deserialize)]
struct SurfaceProcessSnapshotWire {
    schema_version: u16,
    cell_count: u32,
    erosion_depth_m: Vec<f32>,
    deposition_thickness_m: Vec<f32>,
    surface_elevation_m: ElevationField,
    sediment_throughput_m3: Vec<f64>,
    sediment_export_m3: f64,
}

impl SurfaceProcessSnapshot {
    /// Constructs a snapshot only when every self-contained V1 invariant holds.
    pub fn new(
        schema_version: u16,
        cell_count: u32,
        erosion_depth_m: Vec<f32>,
        deposition_thickness_m: Vec<f32>,
        surface_elevation_m: ElevationField,
        sediment_throughput_m3: Vec<f64>,
        sediment_export_m3: f64,
    ) -> Result<Self, SurfaceProcessValidationError> {
        let snapshot = Self {
            schema_version,
            cell_count,
            erosion_depth_m,
            deposition_thickness_m,
            surface_elevation_m,
            sediment_throughput_m3,
            sediment_export_m3,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks all self-contained V1 surface-process invariants.
    pub fn validate(&self) -> Result<(), SurfaceProcessValidationError> {
        if self.schema_version != SURFACE_PROCESS_SCHEMA_V1 {
            return Err(SurfaceProcessValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: SURFACE_PROCESS_SCHEMA_V1,
            });
        }

        validate_length(
            "erosion_depth_m",
            self.erosion_depth_m.len(),
            self.cell_count,
        )?;
        validate_length(
            "deposition_thickness_m",
            self.deposition_thickness_m.len(),
            self.cell_count,
        )?;
        validate_length(
            "surface_elevation_m",
            self.surface_elevation_m.len(),
            self.cell_count,
        )?;
        validate_length(
            "sediment_throughput_m3",
            self.sediment_throughput_m3.len(),
            self.cell_count,
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
        validate_sediment_volume("sediment_export_m3", None, self.sediment_export_m3)?;
        Ok(())
    }

    /// Validates alignment, component identity, ocean behavior, and sediment conservation.
    pub fn validate_against(
        &self,
        spatial: &SpatialSnapshot,
        relief: &ReliefSnapshot,
    ) -> Result<(), SurfaceProcessValidationError> {
        self.validate()?;
        spatial.validate()?;
        if self.cell_count as usize != spatial.cell_count() {
            return Err(SurfaceProcessValidationError::SpatialCellCountMismatch {
                surface: self.cell_count,
                spatial: spatial.cell_count(),
            });
        }
        if self.cell_count != relief.cell_count() {
            return Err(SurfaceProcessValidationError::ReliefCellCountMismatch {
                surface: self.cell_count,
                relief: relief.cell_count(),
            });
        }
        relief.validate_against(spatial)?;

        let mut eroded_volume_m3 = CompensatedSum::default();
        let mut deposited_volume_m3 = CompensatedSum::default();

        for index in 0..self.cell_count as usize {
            let cell = CellId::from_raw(index as u32);
            let constructional = relief.elevation_m().values()[index];
            let erosion = self.erosion_depth_m[index];
            let deposition = self.deposition_thickness_m[index];
            let surface = self.surface_elevation_m.values()[index];
            let calculated = constructional - erosion + deposition;
            if (surface - calculated).abs() > SURFACE_IDENTITY_TOLERANCE_M {
                return Err(SurfaceProcessValidationError::SurfaceIdentityMismatch {
                    cell,
                    surface,
                    calculated,
                });
            }

            if relief.land_ocean_kind(cell) == Some(LandOceanKind::Ocean)
                && (erosion != 0.0 || deposition != 0.0)
            {
                return Err(SurfaceProcessValidationError::OceanSurfaceProcess {
                    cell,
                    erosion,
                    deposition,
                });
            }

            let area_m2 = spatial
                .cell(cell)
                .expect("validated dense spatial snapshot contains every cell")
                .area
                .get();
            eroded_volume_m3.add_checked(area_m2 * f64::from(erosion), cell)?;
            deposited_volume_m3.add_checked(area_m2 * f64::from(deposition), cell)?;
        }

        let eroded_volume_m3 = eroded_volume_m3.total();
        let deposited_volume_m3 = deposited_volume_m3.total();
        let accounted_volume_m3 = deposited_volume_m3 + self.sediment_export_m3;
        if !accounted_volume_m3.is_finite() {
            return Err(
                SurfaceProcessValidationError::NonFiniteDerivedSedimentVolume {
                    cell: None,
                    found: accounted_volume_m3,
                },
            );
        }
        let difference_m3 = (eroded_volume_m3 - accounted_volume_m3).abs();
        let tolerance_m3 = SEDIMENT_VOLUME_ABSOLUTE_TOLERANCE_M3.max(
            eroded_volume_m3.abs().max(accounted_volume_m3.abs())
                * SEDIMENT_VOLUME_RELATIVE_TOLERANCE,
        );
        if difference_m3 > tolerance_m3 {
            return Err(SurfaceProcessValidationError::SedimentMassMismatch {
                eroded_volume_m3,
                deposited_volume_m3,
                exported_volume_m3: self.sediment_export_m3,
                difference_m3,
                tolerance_m3,
            });
        }
        Ok(())
    }

    /// Returns the serialized snapshot schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact dense spatial-cell cardinality.
    pub const fn cell_count(&self) -> u32 {
        self.cell_count
    }

    /// Returns bounded fluvial incision depths without copying.
    pub fn erosion_depth_m(&self) -> &[f32] {
        &self.erosion_depth_m
    }

    /// Returns bounded sediment-deposition thicknesses without copying.
    pub fn deposition_thickness_m(&self) -> &[f32] {
        &self.deposition_thickness_m
    }

    /// Returns current post-erosion and post-deposition surface elevation.
    pub const fn surface_elevation_m(&self) -> &ElevationField {
        &self.surface_elevation_m
    }

    /// Returns total formation-process sediment leaving each cell.
    pub fn sediment_throughput_m3(&self) -> &[f64] {
        &self.sediment_throughput_m3
    }

    /// Returns total sediment exported from the modeled world.
    pub const fn sediment_export_m3(&self) -> f64 {
        self.sediment_export_m3
    }
}

impl<'de> Deserialize<'de> for SurfaceProcessSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SurfaceProcessSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.cell_count,
            wire.erosion_depth_m,
            wire.deposition_thickness_m,
            wire.surface_elevation_m,
            wire.sediment_throughput_m3,
            wire.sediment_export_m3,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_length(
    field: &'static str,
    found: usize,
    cell_count: u32,
) -> Result<(), SurfaceProcessValidationError> {
    let expected = cell_count as usize;
    if found != expected {
        return Err(SurfaceProcessValidationError::FieldLengthMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

fn validate_f32_range(
    field: &'static str,
    values: &[f32],
    min: f32,
    max: f32,
) -> Result<(), SurfaceProcessValidationError> {
    for (index, &found) in values.iter().enumerate() {
        if !found.is_finite() || !(min..=max).contains(&found) {
            return Err(SurfaceProcessValidationError::FieldValueOutOfRange {
                field,
                cell: CellId::from_raw(index as u32),
                found,
                min,
                max,
            });
        }
    }
    Ok(())
}

fn validate_sediment_volume(
    field: &'static str,
    cell: Option<CellId>,
    found: f64,
) -> Result<(), SurfaceProcessValidationError> {
    if !found.is_finite() || found < 0.0 {
        return Err(SurfaceProcessValidationError::InvalidSedimentVolume { field, cell, found });
    }
    Ok(())
}

#[derive(Default)]
struct CompensatedSum {
    sum: f64,
    compensation: f64,
}

impl CompensatedSum {
    fn add_checked(
        &mut self,
        value: f64,
        cell: CellId,
    ) -> Result<(), SurfaceProcessValidationError> {
        if !value.is_finite() {
            return Err(
                SurfaceProcessValidationError::NonFiniteDerivedSedimentVolume {
                    cell: Some(cell),
                    found: value,
                },
            );
        }
        let adjusted = value - self.compensation;
        let next = self.sum + adjusted;
        if !next.is_finite() {
            return Err(
                SurfaceProcessValidationError::NonFiniteDerivedSedimentVolume {
                    cell: Some(cell),
                    found: next,
                },
            );
        }
        self.compensation = (next - self.sum) - adjusted;
        self.sum = next;
        Ok(())
    }

    const fn total(&self) -> f64 {
        self.sum
    }
}

/// Errors returned when current-surface fields violate their V1 contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SurfaceProcessValidationError {
    /// The snapshot uses an unsupported schema version.
    #[error("unsupported surface-process schema {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
        /// The schema version supported by this engine.
        supported: u16,
    },
    /// A dense field length differs from the snapshot cell count.
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        /// The stable field name.
        field: &'static str,
        /// The required dense length.
        expected: usize,
        /// The actual dense length.
        found: usize,
    },
    /// A dense floating-point field contains an invalid value.
    #[error("field {field} value {found} at {cell:?} is outside finite {min}..={max}")]
    FieldValueOutOfRange {
        /// The stable field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The invalid value.
        found: f32,
        /// The inclusive minimum.
        min: f32,
        /// The inclusive maximum.
        max: f32,
    },
    /// A stored sediment volume is negative or non-finite.
    #[error("sediment volume {field} at {cell:?} must be finite and nonnegative, got {found}")]
    InvalidSedimentVolume {
        /// The stable volume field.
        field: &'static str,
        /// The affected cell for a dense field.
        cell: Option<CellId>,
        /// The invalid value.
        found: f64,
    },
    /// The current surface and spatial topology have different cardinalities.
    #[error("surface cell count {surface} does not match spatial count {spatial}")]
    SpatialCellCountMismatch {
        /// The current-surface count.
        surface: u32,
        /// The spatial count.
        spatial: usize,
    },
    /// The current surface and constructional relief have different cardinalities.
    #[error("surface cell count {surface} does not match relief count {relief}")]
    ReliefCellCountMismatch {
        /// The current-surface count.
        surface: u32,
        /// The constructional-relief count.
        relief: u32,
    },
    /// One current elevation does not match its explanatory components.
    #[error("cell {cell:?} surface {surface} does not match component result {calculated}")]
    SurfaceIdentityMismatch {
        /// The affected cell.
        cell: CellId,
        /// The stored current surface.
        surface: f32,
        /// The calculated current surface.
        calculated: f32,
    },
    /// An ocean cell contains a V1 fluvial erosion or deposition process.
    #[error("ocean cell {cell:?} has unsupported erosion {erosion} or deposition {deposition}")]
    OceanSurfaceProcess {
        /// The affected ocean cell.
        cell: CellId,
        /// The stored erosion depth.
        erosion: f32,
        /// The stored deposition thickness.
        deposition: f32,
    },
    /// A derived sediment volume overflowed finite storage.
    #[error("derived sediment volume at {cell:?} is non-finite: {found}")]
    NonFiniteDerivedSedimentVolume {
        /// The affected cell, or none for a global total.
        cell: Option<CellId>,
        /// The non-finite derived value.
        found: f64,
    },
    /// Global eroded volume differs from deposited plus exported volume.
    #[error(
        "eroded volume {eroded_volume_m3} differs from deposited {deposited_volume_m3} plus \
         exported {exported_volume_m3} by {difference_m3}, tolerance {tolerance_m3}"
    )]
    SedimentMassMismatch {
        /// The integrated eroded volume.
        eroded_volume_m3: f64,
        /// The integrated deposited volume.
        deposited_volume_m3: f64,
        /// The global exported volume.
        exported_volume_m3: f64,
        /// The absolute balance error.
        difference_m3: f64,
        /// The allowed absolute balance error for this total.
        tolerance_m3: f64,
    },
    /// The supplied spatial snapshot is itself invalid.
    #[error("invalid spatial input: {0}")]
    Spatial(#[from] SpatialValidationError),
    /// The supplied constructional relief is itself invalid.
    #[error("invalid relief input: {0}")]
    Relief(#[from] ReliefValidationError),
}
