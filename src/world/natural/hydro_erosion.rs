use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    ClimateValidationError, ElevationField, GeologicSnapshot, GeologicValidationError,
    HydrologySnapshot, HydrologyValidationError, LandOceanKind, PreliminaryClimateSnapshot,
    ReliefSnapshot, ReliefValidationError, SurfaceProcessSnapshot, SurfaceProcessValidationError,
    SurfaceWaterKind, CLIMATE_MONTH_COUNT, SURFACE_IDENTITY_TOLERANCE_M,
};
use crate::world::spatial::{SpatialSnapshot, SpatialValidationError, Topology};
use crate::world::CellId;

/// The supported version of the atomic hydro-erosion snapshot schema.
pub const HYDRO_EROSION_SNAPSHOT_SCHEMA_V1: u16 = 1;
/// The surface-bound closed-sphere atomic hydro-erosion envelope schema.
pub const HYDRO_EROSION_SNAPSHOT_SCHEMA_V2: u16 = 2;
/// Allowed absolute rounding difference in the effective-runoff identity.
pub const RUNOFF_IDENTITY_TOLERANCE_MM: f32 = 0.05;

/// Atomic current-slice surface and hydrology output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HydroErosionSnapshot {
    schema_version: u16,
    surface: SurfaceProcessSnapshot,
    hydrology: HydrologySnapshot,
}

#[derive(Deserialize)]
struct HydroErosionSnapshotWire {
    schema_version: u16,
    surface: SurfaceProcessSnapshot,
    hydrology: HydrologySnapshot,
}

impl HydroErosionSnapshot {
    /// Constructs an atomic snapshot only when both subcontracts align.
    pub fn new(
        schema_version: u16,
        surface: SurfaceProcessSnapshot,
        hydrology: HydrologySnapshot,
    ) -> Result<Self, HydroErosionValidationError> {
        let snapshot = Self {
            schema_version,
            surface,
            hydrology,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks schema, subcontracts, and their shared dense cardinality.
    pub fn validate(&self) -> Result<(), HydroErosionValidationError> {
        if self.schema_version != HYDRO_EROSION_SNAPSHOT_SCHEMA_V1 {
            return Err(HydroErosionValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: HYDRO_EROSION_SNAPSHOT_SCHEMA_V1,
            });
        }
        self.surface.validate()?;
        self.hydrology.validate()?;
        if self.surface.cell_count() != self.hydrology.cell_count() {
            return Err(HydroErosionValidationError::CellCountMismatch {
                surface: self.surface.cell_count(),
                hydrology: self.hydrology.cell_count(),
            });
        }
        Ok(())
    }

    /// Validates all cross-domain identities against the exact stage inputs.
    pub fn validate_against(
        &self,
        spatial: &SpatialSnapshot,
        relief: &ReliefSnapshot,
        geology: &GeologicSnapshot,
        climate: &PreliminaryClimateSnapshot,
    ) -> Result<(), HydroErosionValidationError> {
        self.validate()?;
        spatial.validate()?;
        self.validate_relations(spatial, relief, geology, climate)
    }

    /// Rechecks all cross-domain identities when the spatial artifact is already validated.
    pub(crate) fn validate_against_validated_spatial(
        &self,
        spatial: &SpatialSnapshot,
        relief: &ReliefSnapshot,
        geology: &GeologicSnapshot,
        climate: &PreliminaryClimateSnapshot,
    ) -> Result<(), HydroErosionValidationError> {
        self.validate()?;
        self.validate_relations(spatial, relief, geology, climate)
    }

    fn validate_relations(
        &self,
        spatial: &SpatialSnapshot,
        relief: &ReliefSnapshot,
        geology: &GeologicSnapshot,
        climate: &PreliminaryClimateSnapshot,
    ) -> Result<(), HydroErosionValidationError> {
        if spatial.cell_count() != self.cell_count() as usize {
            return Err(HydroErosionValidationError::SpatialCellCountMismatch {
                snapshot: self.cell_count(),
                spatial: spatial.cell_count(),
            });
        }
        if relief.cell_count() != self.cell_count() {
            return Err(HydroErosionValidationError::ReliefCellCountMismatch {
                snapshot: self.cell_count(),
                relief: relief.cell_count(),
            });
        }
        if geology.cell_count() != self.cell_count() {
            return Err(HydroErosionValidationError::GeologyCellCountMismatch {
                snapshot: self.cell_count(),
                geology: geology.cell_count(),
            });
        }
        if climate.cell_count() != self.cell_count() {
            return Err(HydroErosionValidationError::ClimateCellCountMismatch {
                snapshot: self.cell_count(),
                climate: climate.cell_count(),
            });
        }

        relief.validate_against(spatial)?;
        geology.validate()?;
        climate.validate_against(spatial, relief)?;
        self.surface
            .validate_against_validated_spatial(spatial, relief)?;
        self.hydrology.validate_against_validated_spatial(spatial)?;
        validate_hydro_erosion_semantics(
            self.cell_count(),
            self.surface.surface_elevation_m(),
            &self.hydrology,
            relief.sea_level_m(),
            geology.relative_permeability(),
            climate.monthly_precipitation_mm().values(),
        )
    }

    /// Returns the serialized schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the shared dense cell count.
    pub const fn cell_count(&self) -> u32 {
        self.surface.cell_count()
    }

    /// Returns current-surface process fields.
    pub const fn surface(&self) -> &SurfaceProcessSnapshot {
        &self.surface
    }

    /// Returns current hydrology fields and records.
    pub const fn hydrology(&self) -> &HydrologySnapshot {
        &self.hydrology
    }
}

/// Rechecks the geometry-independent identities shared by planar and spherical composites.
///
/// All dense fields must already have passed their own cardinality contracts. Keeping this
/// relation pass here makes current-surface classification, lake depth, and effective runoff a
/// single semantic source for both geometry adapters.
pub(crate) fn validate_hydro_erosion_semantics(
    cell_count: u32,
    current_surface_elevation_m: &ElevationField,
    hydrology: &HydrologySnapshot,
    sea_level_m: f32,
    relative_permeability: &[f32],
    monthly_precipitation_mm: &[[f32; CLIMATE_MONTH_COUNT]],
) -> Result<(), HydroErosionValidationError> {
    for index in 0..cell_count as usize {
        let cell = CellId::from_raw(index as u32);
        let current_surface = current_surface_elevation_m.values()[index];
        let stored_water = hydrology
            .surface_water()
            .get(index)
            .expect("self-validated surface-water field decodes");
        let expected_ocean =
            LandOceanKind::classify(current_surface, sea_level_m) == LandOceanKind::Ocean;
        if expected_ocean != (stored_water == SurfaceWaterKind::Ocean) {
            return Err(HydroErosionValidationError::OceanClassificationMismatch {
                cell,
                current_surface,
                sea_level: sea_level_m,
                stored: stored_water,
                expected_ocean,
            });
        }

        let drainage_surface = hydrology.drainage_surface_elevation_m().values()[index];
        if drainage_surface + SURFACE_IDENTITY_TOLERANCE_M < current_surface {
            return Err(HydroErosionValidationError::DrainageSurfaceBelowCurrent {
                cell,
                current_surface,
                drainage_surface,
            });
        }
        if stored_water == SurfaceWaterKind::Lake {
            let calculated_depth = drainage_surface - current_surface;
            let stored_depth = hydrology.lake_depth_m()[index];
            if (stored_depth - calculated_depth).abs() > SURFACE_IDENTITY_TOLERANCE_M {
                return Err(HydroErosionValidationError::LakeDepthSurfaceMismatch {
                    cell,
                    stored: stored_depth,
                    calculated: calculated_depth,
                });
            }
        }

        for month in 0..CLIMATE_MONTH_COUNT {
            let stored_runoff = hydrology.monthly_local_runoff_mm()[index][month];
            if expected_ocean {
                if stored_runoff != 0.0 {
                    return Err(HydroErosionValidationError::OceanRunoffNonZero {
                        cell,
                        month,
                        found: stored_runoff,
                    });
                }
                continue;
            }

            let permeability = relative_permeability[index];
            let runoff_fraction = 0.85 + (0.20 - 0.85) * permeability;
            let precipitation = monthly_precipitation_mm[index][month];
            let calculated = precipitation * runoff_fraction;
            if (stored_runoff - calculated).abs() > RUNOFF_IDENTITY_TOLERANCE_MM {
                return Err(HydroErosionValidationError::RunoffIdentityMismatch {
                    cell,
                    month,
                    stored: stored_runoff,
                    calculated,
                });
            }
        }
    }
    Ok(())
}

impl<'de> Deserialize<'de> for HydroErosionSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = HydroErosionSnapshotWire::deserialize(deserializer)?;
        Self::new(wire.schema_version, wire.surface, wire.hydrology).map_err(D::Error::custom)
    }
}

/// Errors returned by the atomic hydro-erosion contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum HydroErosionValidationError {
    /// The composite schema is unsupported.
    #[error("unsupported hydro-erosion snapshot schema {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The rejected schema.
        found: u16,
        /// The supported schema.
        supported: u16,
    },
    /// The two atomic sub-snapshots have different dense cardinalities.
    #[error("surface cell count {surface} does not match hydrology count {hydrology}")]
    CellCountMismatch {
        /// The surface-process count.
        surface: u32,
        /// The hydrology count.
        hydrology: u32,
    },
    /// Spatial input cardinality differs from the atomic snapshot.
    #[error("hydro-erosion cell count {snapshot} does not match spatial count {spatial}")]
    SpatialCellCountMismatch {
        /// The atomic snapshot count.
        snapshot: u32,
        /// The spatial count.
        spatial: usize,
    },
    /// Constructional relief cardinality differs from the atomic snapshot.
    #[error("hydro-erosion cell count {snapshot} does not match relief count {relief}")]
    ReliefCellCountMismatch {
        /// The atomic snapshot count.
        snapshot: u32,
        /// The relief count.
        relief: u32,
    },
    /// Geology cardinality differs from the atomic snapshot.
    #[error("hydro-erosion cell count {snapshot} does not match geology count {geology}")]
    GeologyCellCountMismatch {
        /// The atomic snapshot count.
        snapshot: u32,
        /// The geology count.
        geology: u32,
    },
    /// Preliminary-climate cardinality differs from the atomic snapshot.
    #[error("hydro-erosion cell count {snapshot} does not match climate count {climate}")]
    ClimateCellCountMismatch {
        /// The atomic snapshot count.
        snapshot: u32,
        /// The climate count.
        climate: u32,
    },
    /// Current surface and formal sea level disagree with stored surface water.
    #[error(
        "cell {cell:?} current surface {current_surface} at sea level {sea_level} stores \
         {stored:?}; expected_ocean={expected_ocean}"
    )]
    OceanClassificationMismatch {
        /// The affected cell.
        cell: CellId,
        /// Current post-process elevation.
        current_surface: f32,
        /// Formal relief sea level.
        sea_level: f32,
        /// Stored surface-water category.
        stored: SurfaceWaterKind,
        /// Whether formal classification is ocean.
        expected_ocean: bool,
    },
    /// The drainage solve surface is below the real current surface.
    #[error(
        "cell {cell:?} drainage surface {drainage_surface} is below current surface \
         {current_surface}"
    )]
    DrainageSurfaceBelowCurrent {
        /// The affected cell.
        cell: CellId,
        /// Current real surface elevation.
        current_surface: f32,
        /// Priority-Flood drainage elevation.
        drainage_surface: f32,
    },
    /// Stored lake depth disagrees with drainage minus current surface.
    #[error("cell {cell:?} lake depth {stored} does not match surface difference {calculated}")]
    LakeDepthSurfaceMismatch {
        /// The affected cell.
        cell: CellId,
        /// Stored published lake depth.
        stored: f32,
        /// Calculated surface difference.
        calculated: f32,
    },
    /// A formal ocean cell publishes local runoff.
    #[error("ocean cell {cell:?} month {month} has nonzero local runoff {found}")]
    OceanRunoffNonZero {
        /// The affected ocean cell.
        cell: CellId,
        /// Zero-based climatological month.
        month: usize,
        /// The invalid runoff.
        found: f32,
    },
    /// Stored runoff differs from precipitation and permeability forcing.
    #[error(
        "cell {cell:?} month {month} runoff {stored} does not match model result {calculated}"
    )]
    RunoffIdentityMismatch {
        /// The affected cell.
        cell: CellId,
        /// Zero-based climatological month.
        month: usize,
        /// Stored effective runoff.
        stored: f32,
        /// Calculated effective runoff.
        calculated: f32,
    },
    /// The spatial input is invalid.
    #[error("invalid spatial input: {0}")]
    Spatial(#[from] SpatialValidationError),
    /// The relief input is invalid.
    #[error("invalid relief input: {0}")]
    Relief(#[from] ReliefValidationError),
    /// The geology input is invalid.
    #[error("invalid geology input: {0}")]
    Geology(#[from] GeologicValidationError),
    /// The climate input is invalid.
    #[error("invalid climate input: {0}")]
    Climate(#[from] ClimateValidationError),
    /// The surface-process sub-snapshot is invalid.
    #[error("invalid surface-process snapshot: {0}")]
    Surface(#[from] SurfaceProcessValidationError),
    /// The hydrology sub-snapshot is invalid.
    #[error("invalid hydrology snapshot: {0}")]
    Hydrology(#[from] HydrologyValidationError),
}
