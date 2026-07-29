use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{CrustKind, MantleSnapshot, ReliefSnapshot, TectonicSnapshot};
use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::CellId;

/// The supported version of the serialized geologic substrate schema.
pub const GEOLOGIC_SNAPSHOT_SCHEMA_V1: u16 = 1;

/// A broad present-day surface bedrock class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BedrockKind {
    /// Mafic oceanic crust and its shallow intrusive or extrusive cover.
    OceanicMafic,
    /// Stable continental crystalline basement.
    ContinentalCrystalline,
    /// Consolidated sedimentary cover or basin fill.
    Sedimentary,
    /// Metamorphosed continental material.
    Metamorphic,
    /// Current-slice volcanic cover on either crust class.
    Volcanic,
}

impl BedrockKind {
    /// Decodes the stable V1 category value.
    pub fn try_from_raw(raw: u32) -> Result<Self, GeologicValidationError> {
        match raw {
            0 => Ok(Self::OceanicMafic),
            1 => Ok(Self::ContinentalCrystalline),
            2 => Ok(Self::Sedimentary),
            3 => Ok(Self::Metamorphic),
            4 => Ok(Self::Volcanic),
            found => Err(GeologicValidationError::InvalidBedrockKind { cell: None, found }),
        }
    }

    /// Returns the stable V1 category value.
    pub const fn raw(self) -> u32 {
        match self {
            Self::OceanicMafic => 0,
            Self::ContinentalCrystalline => 1,
            Self::Sedimentary => 2,
            Self::Metamorphic => 3,
            Self::Volcanic => 4,
        }
    }
}

/// A dense display-borrowable field of stable bedrock category codes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BedrockKindField(Vec<u32>);

impl BedrockKindField {
    /// Validates and constructs a field from encoded V1 values.
    pub fn new(values: Vec<u32>) -> Result<Self, GeologicValidationError> {
        for (index, &value) in values.iter().enumerate() {
            BedrockKind::try_from_raw(value).map_err(|_| {
                GeologicValidationError::InvalidBedrockKind {
                    cell: Some(CellId::from_raw(index as u32)),
                    found: value,
                }
            })?;
        }
        Ok(Self(values))
    }

    /// Encodes typed categories into stable raw storage.
    pub fn from_kinds(values: Vec<BedrockKind>) -> Self {
        Self(values.into_iter().map(BedrockKind::raw).collect())
    }

    /// Returns one typed category by dense index.
    pub fn get(&self, index: usize) -> Option<BedrockKind> {
        self.0
            .get(index)
            .and_then(|&raw| BedrockKind::try_from_raw(raw).ok())
    }

    /// Returns encoded categories without copying.
    pub fn raw_values(&self) -> &[u32] {
        &self.0
    }

    /// Returns the number of dense values.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether this field contains no values.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for BedrockKindField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Vec::<u32>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Immutable present-day surface geology and geologic permissiveness fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GeologicSnapshot {
    schema_version: u16,
    cell_count: u32,
    bedrock_kinds: BedrockKindField,
    fracture_intensity: Vec<f32>,
    erosion_resistance: Vec<f32>,
    relative_permeability: Vec<f32>,
    metallic_mineral_potential: Vec<f32>,
    geothermal_potential: Vec<f32>,
    sedimentary_basin_potential: Vec<f32>,
}

#[derive(Deserialize)]
struct GeologicSnapshotWire {
    schema_version: u16,
    cell_count: u32,
    bedrock_kinds: BedrockKindField,
    fracture_intensity: Vec<f32>,
    erosion_resistance: Vec<f32>,
    relative_permeability: Vec<f32>,
    metallic_mineral_potential: Vec<f32>,
    geothermal_potential: Vec<f32>,
    sedimentary_basin_potential: Vec<f32>,
}

impl GeologicSnapshot {
    /// Constructs a snapshot only when every V1 dense invariant holds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        cell_count: u32,
        bedrock_kinds: BedrockKindField,
        fracture_intensity: Vec<f32>,
        erosion_resistance: Vec<f32>,
        relative_permeability: Vec<f32>,
        metallic_mineral_potential: Vec<f32>,
        geothermal_potential: Vec<f32>,
        sedimentary_basin_potential: Vec<f32>,
    ) -> Result<Self, GeologicValidationError> {
        let snapshot = Self {
            schema_version,
            cell_count,
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

    /// Rechecks every self-contained geologic invariant.
    pub fn validate(&self) -> Result<(), GeologicValidationError> {
        if self.schema_version != GEOLOGIC_SNAPSHOT_SCHEMA_V1 {
            return Err(GeologicValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: GEOLOGIC_SNAPSHOT_SCHEMA_V1,
            });
        }

        validate_length("bedrock_kinds", self.bedrock_kinds.len(), self.cell_count)?;
        for (index, &raw) in self.bedrock_kinds.raw_values().iter().enumerate() {
            BedrockKind::try_from_raw(raw).map_err(|_| {
                GeologicValidationError::InvalidBedrockKind {
                    cell: Some(CellId::from_raw(index as u32)),
                    found: raw,
                }
            })?;
        }
        for (name, values) in [
            ("fracture_intensity", self.fracture_intensity.as_slice()),
            ("erosion_resistance", self.erosion_resistance.as_slice()),
            (
                "relative_permeability",
                self.relative_permeability.as_slice(),
            ),
            (
                "metallic_mineral_potential",
                self.metallic_mineral_potential.as_slice(),
            ),
            ("geothermal_potential", self.geothermal_potential.as_slice()),
            (
                "sedimentary_basin_potential",
                self.sedimentary_basin_potential.as_slice(),
            ),
        ] {
            validate_length(name, values.len(), self.cell_count)?;
            validate_unit_interval(name, values)?;
        }
        Ok(())
    }

    /// Validates alignment and bedrock/crust compatibility against all upstream snapshots.
    pub fn validate_against(
        &self,
        spatial: &SpatialSnapshot,
        tectonic: &TectonicSnapshot,
        mantle: &MantleSnapshot,
        relief: &ReliefSnapshot,
    ) -> Result<(), GeologicValidationError> {
        self.validate()?;
        if spatial.cell_count() != self.cell_count as usize {
            return Err(GeologicValidationError::SpatialCellCountMismatch {
                geologic: self.cell_count,
                spatial: spatial.cell_count(),
            });
        }
        if tectonic.cell_count() != self.cell_count {
            return Err(GeologicValidationError::TectonicCellCountMismatch {
                geologic: self.cell_count,
                tectonic: tectonic.cell_count(),
            });
        }
        if mantle.cell_count() != self.cell_count {
            return Err(GeologicValidationError::MantleCellCountMismatch {
                geologic: self.cell_count,
                mantle: mantle.cell_count(),
            });
        }
        if relief.cell_count() != self.cell_count {
            return Err(GeologicValidationError::ReliefCellCountMismatch {
                geologic: self.cell_count,
                relief: relief.cell_count(),
            });
        }

        for index in 0..self.cell_count as usize {
            let cell = CellId::from_raw(index as u32);
            let bedrock = self
                .bedrock_kinds
                .get(index)
                .expect("dense bedrock field was validated");
            let crust = tectonic
                .crust_kind(cell)
                .expect("tectonic cell count and local field were validated");
            let compatible = match bedrock {
                BedrockKind::OceanicMafic => crust == CrustKind::Oceanic,
                BedrockKind::ContinentalCrystalline | BedrockKind::Metamorphic => {
                    crust == CrustKind::Continental
                }
                BedrockKind::Sedimentary | BedrockKind::Volcanic => true,
            };
            if !compatible {
                return Err(GeologicValidationError::BedrockCrustMismatch {
                    cell,
                    bedrock,
                    crust,
                });
            }
        }
        Ok(())
    }

    /// Returns the serialized schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the exact dense spatial-cell cardinality.
    pub const fn cell_count(&self) -> u32 {
        self.cell_count
    }

    /// Returns stable raw and typed bedrock categories.
    pub const fn bedrock_kinds(&self) -> &BedrockKindField {
        &self.bedrock_kinds
    }

    /// Returns normalized current fracture intensity without copying.
    pub fn fracture_intensity(&self) -> &[f32] {
        &self.fracture_intensity
    }

    /// Returns normalized resistance to erosion without copying.
    pub fn erosion_resistance(&self) -> &[f32] {
        &self.erosion_resistance
    }

    /// Returns normalized relative permeability without copying.
    pub fn relative_permeability(&self) -> &[f32] {
        &self.relative_permeability
    }

    /// Returns relative metallic-mineral formation potential without copying.
    pub fn metallic_mineral_potential(&self) -> &[f32] {
        &self.metallic_mineral_potential
    }

    /// Returns relative geothermal potential without copying.
    pub fn geothermal_potential(&self) -> &[f32] {
        &self.geothermal_potential
    }

    /// Returns relative sedimentary-basin formation potential without copying.
    pub fn sedimentary_basin_potential(&self) -> &[f32] {
        &self.sedimentary_basin_potential
    }

    /// Returns the bedrock category for one cell.
    pub fn bedrock_kind(&self, cell: CellId) -> Option<BedrockKind> {
        self.bedrock_kinds.get(cell.raw() as usize)
    }
}

impl<'de> Deserialize<'de> for GeologicSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GeologicSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.cell_count,
            wire.bedrock_kinds,
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

fn validate_length(
    field: &'static str,
    found: usize,
    cell_count: u32,
) -> Result<(), GeologicValidationError> {
    let expected = cell_count as usize;
    if found != expected {
        return Err(GeologicValidationError::FieldLengthMismatch {
            field,
            expected,
            found,
        });
    }
    Ok(())
}

fn validate_unit_interval(
    field: &'static str,
    values: &[f32],
) -> Result<(), GeologicValidationError> {
    for (index, &found) in values.iter().enumerate() {
        if !found.is_finite() || !(0.0..=1.0).contains(&found) {
            return Err(GeologicValidationError::FieldValueOutOfRange {
                field,
                cell: CellId::from_raw(index as u32),
                found,
            });
        }
    }
    Ok(())
}

/// Errors returned when geologic substrate fields violate a V1 contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GeologicValidationError {
    /// The snapshot uses a schema version that this engine does not support.
    #[error("unsupported geologic snapshot schema {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The rejected schema version.
        found: u16,
        /// The supported schema version.
        supported: u16,
    },
    /// A raw bedrock category does not decode under V1.
    #[error("invalid bedrock category {found} at {cell:?}")]
    InvalidBedrockKind {
        /// The affected cell when known.
        cell: Option<CellId>,
        /// The invalid raw category.
        found: u32,
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
    /// A continuous field value is non-finite or outside zero to one.
    #[error("field {field} value {found} at {cell:?} is outside finite 0..=1")]
    FieldValueOutOfRange {
        /// The stable field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The rejected value.
        found: f32,
    },
    /// The geologic and spatial cell cardinalities differ.
    #[error("geologic cell count {geologic} does not match spatial count {spatial}")]
    SpatialCellCountMismatch {
        /// The geologic snapshot count.
        geologic: u32,
        /// The spatial topology count.
        spatial: usize,
    },
    /// The geologic and tectonic cell cardinalities differ.
    #[error("geologic cell count {geologic} does not match tectonic count {tectonic}")]
    TectonicCellCountMismatch {
        /// The geologic snapshot count.
        geologic: u32,
        /// The tectonic snapshot count.
        tectonic: u32,
    },
    /// The geologic and mantle cell cardinalities differ.
    #[error("geologic cell count {geologic} does not match mantle count {mantle}")]
    MantleCellCountMismatch {
        /// The geologic snapshot count.
        geologic: u32,
        /// The mantle snapshot count.
        mantle: u32,
    },
    /// The geologic and relief cell cardinalities differ.
    #[error("geologic cell count {geologic} does not match relief count {relief}")]
    ReliefCellCountMismatch {
        /// The geologic snapshot count.
        geologic: u32,
        /// The relief snapshot count.
        relief: u32,
    },
    /// A crystalline bedrock class is incompatible with its underlying crust class.
    #[error("bedrock {bedrock:?} at {cell:?} is incompatible with crust {crust:?}")]
    BedrockCrustMismatch {
        /// The affected cell.
        cell: CellId,
        /// The incompatible surface bedrock.
        bedrock: BedrockKind,
        /// The underlying crust class.
        crust: CrustKind,
    },
}
