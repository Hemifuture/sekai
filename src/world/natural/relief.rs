use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::world::spatial::{SpatialSnapshot, Topology};
use crate::world::CellId;

use super::primary_relief::CRUST_BASE_ELEVATION_MAX_M;

/// The supported version of the serialized relief snapshot schema.
pub const RELIEF_SCHEMA_V1: u16 = 1;
/// The legacy supported relief schema with an explicit 4,000 m volcanic component.
pub const RELIEF_SCHEMA_V2: u16 = 2;
/// The current relief schema with deep-ocean volcanic edifices up to 6,000 m.
pub const RELIEF_SCHEMA_V3: u16 = 3;
/// The surface-bound relief schema used by authoritative spherical worlds.
pub const RELIEF_SCHEMA_V4: u16 = 4;
/// The minimum safe final elevation, in meters.
pub const ELEVATION_MIN_M: f32 = -11_000.0;
/// The maximum safe final elevation, in meters.
pub const ELEVATION_MAX_M: f32 = 9_000.0;
/// The minimum supported crust-base component, in meters.
pub const CRUST_BASE_ELEVATION_MIN_M: f32 = -9_000.0;
/// The minimum supported tectonic-event component, in meters.
pub const TECTONIC_OFFSET_MIN_M: f32 = -6_000.0;
/// The maximum supported tectonic-event component, in meters.
pub const TECTONIC_OFFSET_MAX_M: f32 = 7_000.0;
/// The minimum supported current volcanic-relief contribution, in meters.
pub const VOLCANIC_OFFSET_MIN_M: f32 = 0.0;
/// The maximum supported current volcanic-relief contribution, in meters.
pub const VOLCANIC_OFFSET_MAX_M: f32 = 6_000.0;
const VOLCANIC_OFFSET_V2_MAX_M: f32 = 4_000.0;
/// The minimum supported regional-relief component, in meters.
pub const REGIONAL_OFFSET_MIN_M: f32 = -3_000.0;
/// The maximum supported regional-relief component, in meters.
pub const REGIONAL_OFFSET_MAX_M: f32 = 3_000.0;
/// The allowed absolute rounding difference in the elevation component identity.
pub const COMPONENT_IDENTITY_TOLERANCE_M: f32 = 0.01;

/// A dense, finite field of elevations or elevation contributions in meters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ElevationField(Vec<f32>);

impl ElevationField {
    /// Constructs a field only when every value is finite.
    pub fn from_values(values: Vec<f32>) -> Result<Self, ReliefValidationError> {
        validate_finite_values("elevation_field", &values)?;
        Ok(Self(values))
    }

    /// Returns field values without copying them.
    pub fn values(&self) -> &[f32] {
        &self.0
    }

    /// Returns one dense value by index.
    pub fn get(&self, index: usize) -> Option<f32> {
        self.0.get(index).copied()
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

/// The sea-level classification of a cell after centimeter quantization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LandOceanKind {
    /// Elevation is at least one quantized centimeter below sea level.
    Ocean,
    /// Elevation is equal to or above sea level after centimeter quantization.
    Land,
}

impl LandOceanKind {
    pub(crate) fn quantized_centimeters_exact(elevation_m: f64) -> i64 {
        quantized_centimeters_exact(elevation_m)
    }

    pub(crate) fn meters_from_quantized_centimeters_exact(value: i64) -> Option<f64> {
        meters_from_quantized_centimeters_exact(value)
    }

    /// Decodes the stable V1 category value.
    pub fn try_from_raw(raw: u32) -> Result<Self, ReliefValidationError> {
        match raw {
            0 => Ok(Self::Ocean),
            1 => Ok(Self::Land),
            found => Err(ReliefValidationError::InvalidLandOceanKind { cell: None, found }),
        }
    }

    /// Returns the stable V1 category value.
    pub const fn raw(self) -> u32 {
        match self {
            Self::Ocean => 0,
            Self::Land => 1,
        }
    }

    /// Classifies one finite elevation against a finite sea level.
    ///
    /// Both values are rounded to the nearest centimeter. Equality belongs to
    /// land; ocean therefore requires a value at least one quantized centimeter
    /// below sea level.
    pub fn classify(elevation_m: f32, sea_level_m: f32) -> Self {
        Self::classify_exact(f64::from(elevation_m), f64::from(sea_level_m))
    }

    /// Classifies retained scientific state without first narrowing to wire precision.
    pub(crate) fn classify_exact(elevation_m: f64, sea_level_m: f64) -> Self {
        if quantized_centimeters_exact(elevation_m) < quantized_centimeters_exact(sea_level_m) {
            Self::Ocean
        } else {
            Self::Land
        }
    }
}

/// A dense, display-borrowable field of raw land/ocean categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LandOceanField(Vec<u32>);

impl LandOceanField {
    /// Encodes typed categories into stable raw category storage.
    pub fn from_kinds(values: Vec<LandOceanKind>) -> Self {
        Self(values.into_iter().map(LandOceanKind::raw).collect())
    }

    /// Validates and constructs a field from encoded V1 categories.
    pub fn from_raw(values: Vec<u32>) -> Result<Self, ReliefValidationError> {
        for (index, &value) in values.iter().enumerate() {
            LandOceanKind::try_from_raw(value).map_err(|_| {
                ReliefValidationError::InvalidLandOceanKind {
                    cell: Some(CellId::from_raw(index as u32)),
                    found: value,
                }
            })?;
        }
        Ok(Self(values))
    }

    /// Derives categories from a finite elevation field and sea level.
    pub fn classify(elevation: &ElevationField, sea_level_m: f32) -> Self {
        Self::from_kinds(
            elevation
                .values()
                .iter()
                .map(|&value| LandOceanKind::classify(value, sea_level_m))
                .collect(),
        )
    }

    /// Returns one typed category by dense index.
    pub fn get(&self, index: usize) -> Option<LandOceanKind> {
        self.0
            .get(index)
            .and_then(|&raw| LandOceanKind::try_from_raw(raw).ok())
    }

    /// Returns encoded categories without copying them.
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

/// Immutable explainable relief fields for the current natural world slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReliefSnapshot {
    schema_version: u16,
    cell_count: u32,
    sea_level_m: f32,
    crust_base_elevation_m: ElevationField,
    tectonic_offset_m: ElevationField,
    volcanic_offset_m: ElevationField,
    regional_offset_m: ElevationField,
    elevation_m: ElevationField,
    land_ocean_kind: LandOceanField,
}

impl ReliefSnapshot {
    /// Constructs a snapshot only when all invariants for a supported relief schema hold.
    pub fn new(
        schema_version: u16,
        cell_count: u32,
        sea_level_m: f32,
        crust_base_elevation_m: ElevationField,
        tectonic_offset_m: ElevationField,
        volcanic_offset_m: ElevationField,
        regional_offset_m: ElevationField,
        elevation_m: ElevationField,
        land_ocean_kind: LandOceanField,
    ) -> Result<Self, ReliefValidationError> {
        let snapshot = Self {
            schema_version,
            cell_count,
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

    /// Rechecks every self-contained relief invariant.
    pub fn validate(&self) -> Result<(), ReliefValidationError> {
        let volcanic_offset_max_m = match self.schema_version {
            RELIEF_SCHEMA_V2 => VOLCANIC_OFFSET_V2_MAX_M,
            RELIEF_SCHEMA_V3 => VOLCANIC_OFFSET_MAX_M,
            found => {
                return Err(ReliefValidationError::UnsupportedSchema {
                    found,
                    supported: RELIEF_SCHEMA_V3,
                });
            }
        };

        validate_relief_fields(
            self.cell_count,
            self.sea_level_m,
            volcanic_offset_max_m,
            ReliefFields {
                crust_base_elevation_m: &self.crust_base_elevation_m,
                tectonic_offset_m: &self.tectonic_offset_m,
                volcanic_offset_m: &self.volcanic_offset_m,
                regional_offset_m: &self.regional_offset_m,
                elevation_m: &self.elevation_m,
                land_ocean_kind: &self.land_ocean_kind,
            },
        )
    }

    /// Validates the cell alignment against a spatial snapshot.
    pub fn validate_against(&self, spatial: &SpatialSnapshot) -> Result<(), ReliefValidationError> {
        self.validate()?;
        if spatial.cell_count() != self.cell_count as usize {
            return Err(ReliefValidationError::SpatialCellCountMismatch {
                relief: self.cell_count,
                spatial: spatial.cell_count(),
            });
        }
        Ok(())
    }

    /// Returns the schema version.
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Returns the number of cell-aligned values.
    pub const fn cell_count(&self) -> u32 {
        self.cell_count
    }

    /// Returns the single global sea level, in meters.
    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }

    /// Returns the crust and continental-margin baseline field.
    pub const fn crust_base_elevation_m(&self) -> &ElevationField {
        &self.crust_base_elevation_m
    }

    /// Returns the net contribution from current tectonic boundary events.
    pub const fn tectonic_offset_m(&self) -> &ElevationField {
        &self.tectonic_offset_m
    }

    /// Returns the present-day mantle-driven volcanic relief contribution.
    pub const fn volcanic_offset_m(&self) -> &ElevationField {
        &self.volcanic_offset_m
    }

    /// Returns the graph-smoothed regional relief contribution.
    pub const fn regional_offset_m(&self) -> &ElevationField {
        &self.regional_offset_m
    }

    /// Returns final elevations in meters.
    pub const fn elevation_m(&self) -> &ElevationField {
        &self.elevation_m
    }

    /// Returns raw and typed land/ocean categories.
    pub const fn land_ocean(&self) -> &LandOceanField {
        &self.land_ocean_kind
    }

    /// Returns the final elevation for one cell.
    pub fn elevation_for_cell(&self, cell: CellId) -> Option<f32> {
        self.elevation_m.get(cell.raw() as usize)
    }

    /// Returns the land/ocean category for one cell.
    pub fn land_ocean_kind(&self, cell: CellId) -> Option<LandOceanKind> {
        self.land_ocean_kind.get(cell.raw() as usize)
    }
}

pub(crate) struct ReliefFields<'a> {
    pub(crate) crust_base_elevation_m: &'a ElevationField,
    pub(crate) tectonic_offset_m: &'a ElevationField,
    pub(crate) volcanic_offset_m: &'a ElevationField,
    pub(crate) regional_offset_m: &'a ElevationField,
    pub(crate) elevation_m: &'a ElevationField,
    pub(crate) land_ocean_kind: &'a LandOceanField,
}

pub(crate) fn validate_relief_fields(
    cell_count: u32,
    sea_level_m: f32,
    volcanic_offset_max_m: f32,
    fields: ReliefFields<'_>,
) -> Result<(), ReliefValidationError> {
    if !sea_level_m.is_finite() {
        return Err(ReliefValidationError::NonFiniteSeaLevel { found: sea_level_m });
    }

    for (name, field) in [
        ("crust_base_elevation_m", fields.crust_base_elevation_m),
        ("tectonic_offset_m", fields.tectonic_offset_m),
        ("volcanic_offset_m", fields.volcanic_offset_m),
        ("regional_offset_m", fields.regional_offset_m),
        ("elevation_m", fields.elevation_m),
    ] {
        validate_length(name, field.len(), cell_count)?;
        validate_finite_values(name, field.values())?;
    }
    validate_length("land_ocean_kind", fields.land_ocean_kind.len(), cell_count)?;

    validate_range(
        "crust_base_elevation_m",
        fields.crust_base_elevation_m.values(),
        CRUST_BASE_ELEVATION_MIN_M,
        CRUST_BASE_ELEVATION_MAX_M,
    )?;
    validate_range(
        "tectonic_offset_m",
        fields.tectonic_offset_m.values(),
        TECTONIC_OFFSET_MIN_M,
        TECTONIC_OFFSET_MAX_M,
    )?;
    validate_range(
        "volcanic_offset_m",
        fields.volcanic_offset_m.values(),
        VOLCANIC_OFFSET_MIN_M,
        volcanic_offset_max_m,
    )?;
    validate_range(
        "regional_offset_m",
        fields.regional_offset_m.values(),
        REGIONAL_OFFSET_MIN_M,
        REGIONAL_OFFSET_MAX_M,
    )?;
    validate_range(
        "elevation_m",
        fields.elevation_m.values(),
        ELEVATION_MIN_M,
        ELEVATION_MAX_M,
    )?;

    for index in 0..cell_count as usize {
        let cell = CellId::from_raw(index as u32);
        let base = fields.crust_base_elevation_m.values()[index];
        let tectonic = fields.tectonic_offset_m.values()[index];
        let volcanic = fields.volcanic_offset_m.values()[index];
        let regional = fields.regional_offset_m.values()[index];
        let elevation = fields.elevation_m.values()[index];
        let calculated = base + tectonic + volcanic + regional;
        if (elevation - calculated).abs() > COMPONENT_IDENTITY_TOLERANCE_M {
            return Err(ReliefValidationError::ComponentIdentityMismatch {
                cell,
                elevation,
                calculated,
            });
        }

        let raw_kind = fields.land_ocean_kind.raw_values()[index];
        let stored = LandOceanKind::try_from_raw(raw_kind).map_err(|_| {
            ReliefValidationError::InvalidLandOceanKind {
                cell: Some(cell),
                found: raw_kind,
            }
        })?;
        let expected = LandOceanKind::classify(elevation, sea_level_m);
        if stored != expected {
            return Err(ReliefValidationError::LandOceanMismatch {
                cell,
                stored,
                expected,
            });
        }
    }
    Ok(())
}

fn validate_length(
    field: &'static str,
    found: usize,
    expected: u32,
) -> Result<(), ReliefValidationError> {
    if found != expected as usize {
        return Err(ReliefValidationError::FieldLengthMismatch {
            field,
            expected: expected as usize,
            found,
        });
    }
    Ok(())
}

fn validate_finite_values(
    field: &'static str,
    values: &[f32],
) -> Result<(), ReliefValidationError> {
    for (index, &found) in values.iter().enumerate() {
        if !found.is_finite() {
            return Err(ReliefValidationError::NonFiniteFieldValue {
                field,
                cell: CellId::from_raw(index as u32),
                found,
            });
        }
    }
    Ok(())
}

fn validate_range(
    field: &'static str,
    values: &[f32],
    min: f32,
    max: f32,
) -> Result<(), ReliefValidationError> {
    for (index, &found) in values.iter().enumerate() {
        if !(min..=max).contains(&found) {
            return Err(ReliefValidationError::FieldValueOutOfRange {
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

pub(crate) fn quantized_centimeters_exact(value: f64) -> i64 {
    (value * 100.0).round() as i64
}

pub(crate) fn meters_from_quantized_centimeters_exact(value: i64) -> Option<f64> {
    let meters = value as f64 / 100.0;
    (meters.is_finite() && quantized_centimeters_exact(meters) == value).then_some(meters)
}

/// Errors returned when relief fields violate V2 numerical or alignment invariants.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ReliefValidationError {
    /// The snapshot uses an unsupported schema version.
    #[error("unsupported relief schema {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The schema version found in the snapshot.
        found: u16,
        /// The schema version supported by this engine.
        supported: u16,
    },
    /// Sea level is not finite.
    #[error("sea level must be finite, got {found}")]
    NonFiniteSeaLevel {
        /// The invalid sea level.
        found: f32,
    },
    /// A dense field has an unexpected cell-aligned length.
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        /// The stable field name.
        field: &'static str,
        /// The required length.
        expected: usize,
        /// The stored length.
        found: usize,
    },
    /// A dense floating-point field contains a non-finite value.
    #[error("field {field} contains non-finite value {found} at {cell:?}")]
    NonFiniteFieldValue {
        /// The stable field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The invalid value.
        found: f32,
    },
    /// A field value lies outside its V2 physical safety bound.
    #[error("field {field} value {found} at {cell:?} is outside {min}..={max}")]
    FieldValueOutOfRange {
        /// The stable field name.
        field: &'static str,
        /// The affected cell.
        cell: CellId,
        /// The invalid value.
        found: f32,
        /// The inclusive lower bound.
        min: f32,
        /// The inclusive upper bound.
        max: f32,
    },
    /// Final elevation does not equal the sum of its stored explanatory components.
    #[error("cell {cell:?} elevation {elevation} does not match component sum {calculated}")]
    ComponentIdentityMismatch {
        /// The affected cell.
        cell: CellId,
        /// The stored final elevation.
        elevation: f32,
        /// The calculated component sum.
        calculated: f32,
    },
    /// A raw land/ocean category does not decode under V1.
    #[error("invalid land/ocean category {found} at {cell:?}")]
    InvalidLandOceanKind {
        /// The affected cell when known.
        cell: Option<CellId>,
        /// The invalid raw category.
        found: u32,
    },
    /// A stored land/ocean category disagrees with centimeter-quantized elevation.
    #[error("cell {cell:?} stores {stored:?}; expected {expected:?}")]
    LandOceanMismatch {
        /// The affected cell.
        cell: CellId,
        /// The stored category.
        stored: LandOceanKind,
        /// The category derived from elevation and sea level.
        expected: LandOceanKind,
    },
    /// Relief and spatial cell cardinalities differ.
    #[error("relief cell count {relief} does not match spatial count {spatial}")]
    SpatialCellCountMismatch {
        /// The relief count.
        relief: u32,
        /// The spatial count.
        spatial: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::{quantized_centimeters_exact, LandOceanKind};

    #[test]
    fn exact_centimeter_classification_is_the_single_precision_source() {
        let elevation = 100.004_999_999_f64;
        let sea_level = 100.005_000_001_f64;

        assert_eq!(quantized_centimeters_exact(elevation), 10_000);
        assert_eq!(quantized_centimeters_exact(sea_level), 10_001);
        assert_eq!(
            LandOceanKind::classify_exact(elevation, sea_level),
            LandOceanKind::Ocean
        );
        assert_eq!(
            LandOceanKind::classify(100.0_f32, 100.0_f32),
            LandOceanKind::classify_exact(100.0, 100.0)
        );
    }
}
