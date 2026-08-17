use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::geology::validate_bedrock_crust_compatibility;
use super::{
    BedrockKind, BedrockKindField, CrustKind, CrustKindField, EvolvedTectonicSnapshot,
    EvolvedTectonicValidationError, GeologicValidationError, SphericalMantleSnapshot,
    SphericalMantleValidationError, TectonicValidationError, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
    CONTINENTAL_CRUST_MAX_THICKNESS_KM, CONTINENTAL_CRUST_MIN_THICKNESS_KM, MAX_CRUST_AGE_MYR,
    OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    SphericalSurfaceSnapshot, SphericalSurfaceValidationError, SurfaceGeometryKind, SurfaceRef,
    SurfaceRefError,
};
use crate::world::{CellId, MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT};

/// The first strict V5-derived geologic-substrate schema.
pub const GEOLOGIC_SUBSTRATE_SCHEMA_V1: u16 = 1;
/// Continental material density used by the P3 volume-weighted crust recipe.
pub const CONTINENTAL_CRUST_DENSITY_KG_M3: f32 = 2_800.0;
/// Oceanic material density used by the P3 volume-weighted crust recipe.
pub const OCEANIC_CRUST_DENSITY_KG_M3: f32 = 2_950.0;
/// Inclusive safety floor for a published effective crust density.
pub const CRUST_DENSITY_MIN_KG_M3: f32 = 2_500.0;
/// Inclusive safety ceiling for a published effective crust density.
pub const CRUST_DENSITY_MAX_KG_M3: f32 = 3_200.0;

const MAX_SPHERICAL_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_SPHERICAL_EDGES: usize = MAX_SPHERICAL_EDGE_COUNT as usize;
const DENSITY_IDENTITY_TOLERANCE_KG_M3: f32 = 1.0e-3;

/// The broad source-rock class available to later sediment production.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SedimentSourceKind {
    /// Continental crystalline material supplying felsic clasts.
    Felsic,
    /// Oceanic mafic material supplying mafic clasts.
    Mafic,
    /// Current volcanic cover supplying volcaniclastic material.
    Volcaniclastic,
    /// Existing sedimentary cover supplying recycled sediment.
    Sedimentary,
    /// Resistant metamorphic source rock.
    Metamorphic,
}

impl SedimentSourceKind {
    /// Decodes the stable P3 category value.
    pub fn try_from_raw(raw: u32) -> Result<Self, GeologicSubstrateValidationError> {
        match raw {
            0 => Ok(Self::Felsic),
            1 => Ok(Self::Mafic),
            2 => Ok(Self::Volcaniclastic),
            3 => Ok(Self::Sedimentary),
            4 => Ok(Self::Metamorphic),
            found => {
                Err(GeologicSubstrateValidationError::InvalidSedimentSource { cell: None, found })
            }
        }
    }

    /// Returns the stable P3 category value.
    pub const fn raw(self) -> u32 {
        match self {
            Self::Felsic => 0,
            Self::Mafic => 1,
            Self::Volcaniclastic => 2,
            Self::Sedimentary => 3,
            Self::Metamorphic => 4,
        }
    }
}

/// Dense stable sediment-source categories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SedimentSourceKindField(Vec<u32>);

impl SedimentSourceKindField {
    /// Encodes typed categories.
    pub fn from_kinds(values: Vec<SedimentSourceKind>) -> Self {
        Self(values.into_iter().map(SedimentSourceKind::raw).collect())
    }

    /// Validates already encoded categories.
    pub fn from_raw(values: Vec<u32>) -> Result<Self, GeologicSubstrateValidationError> {
        for (index, &found) in values.iter().enumerate() {
            SedimentSourceKind::try_from_raw(found).map_err(|_| {
                GeologicSubstrateValidationError::InvalidSedimentSource {
                    cell: Some(CellId::from_raw(index as u32)),
                    found,
                }
            })?;
        }
        Ok(Self(values))
    }

    /// Returns one typed category.
    pub fn get(&self, index: usize) -> Option<SedimentSourceKind> {
        self.0
            .get(index)
            .and_then(|&raw| SedimentSourceKind::try_from_raw(raw).ok())
    }

    /// Returns stable raw categories.
    pub fn raw_values(&self) -> &[u32] {
        &self.0
    }

    /// Returns dense cardinality.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the field is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'de> Deserialize<'de> for SedimentSourceKindField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_raw(Vec::<u32>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Immutable V5-derived geologic substrate on one authoritative sphere.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeologicSubstrateSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    mantle: SphericalMantleSnapshot,
    crust_kinds: CrustKindField,
    crust_thickness_km: Vec<f32>,
    ocean_age_myr: Vec<f32>,
    crust_density_kg_m3: Vec<f32>,
    bedrock_kinds: BedrockKindField,
    fracture_intensity: Vec<f32>,
    erodibility: Vec<f32>,
    relative_permeability: Vec<f32>,
    sediment_sources: SedimentSourceKindField,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeologicSubstrateSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    mantle: SphericalMantleSnapshot,
    #[serde(deserialize_with = "deserialize_dense_u32")]
    crust_kinds: Vec<u32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    crust_thickness_km: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    ocean_age_myr: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    crust_density_kg_m3: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_u32")]
    bedrock_kinds: Vec<u32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    fracture_intensity: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    erodibility: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    relative_permeability: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_u32")]
    sediment_sources: Vec<u32>,
}

fn deserialize_dense_f32<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

fn deserialize_dense_u32<'de, D>(deserializer: D) -> Result<Vec<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_SPHERICAL_CELLS>(deserializer)
}

impl GeologicSubstrateSnapshot {
    /// Constructs a substrate only when every local contract holds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        mantle: SphericalMantleSnapshot,
        crust_kinds: CrustKindField,
        crust_thickness_km: Vec<f32>,
        ocean_age_myr: Vec<f32>,
        crust_density_kg_m3: Vec<f32>,
        bedrock_kinds: BedrockKindField,
        fracture_intensity: Vec<f32>,
        erodibility: Vec<f32>,
        relative_permeability: Vec<f32>,
        sediment_sources: SedimentSourceKindField,
    ) -> Result<Self, GeologicSubstrateValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            mantle,
            crust_kinds,
            crust_thickness_km,
            ocean_age_myr,
            crust_density_kg_m3,
            bedrock_kinds,
            fracture_intensity,
            erodibility,
            relative_permeability,
            sediment_sources,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks every self-contained substrate invariant.
    pub fn validate(&self) -> Result<(), GeologicSubstrateValidationError> {
        if self.schema_version != GEOLOGIC_SUBSTRATE_SCHEMA_V1 {
            return Err(GeologicSubstrateValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: GEOLOGIC_SUBSTRATE_SCHEMA_V1,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(GeologicSubstrateValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        validate_allocation(
            "surface_ref.cell_count",
            self.cell_count() as usize,
            MAX_SPHERICAL_CELLS,
        )?;
        validate_allocation(
            "surface_ref.edge_count",
            self.surface_ref.edge_count() as usize,
            MAX_SPHERICAL_EDGES,
        )?;
        self.mantle.validate()?;
        if self.mantle.surface_ref() != self.surface_ref {
            return Err(GeologicSubstrateValidationError::NestedSurfaceMismatch {
                field: "mantle",
                expected: self.surface_ref,
                found: self.mantle.surface_ref(),
            });
        }

        let expected = self.cell_count() as usize;
        for (field, found) in [
            ("crust_kinds", self.crust_kinds.len()),
            ("crust_thickness_km", self.crust_thickness_km.len()),
            ("ocean_age_myr", self.ocean_age_myr.len()),
            ("crust_density_kg_m3", self.crust_density_kg_m3.len()),
            ("bedrock_kinds", self.bedrock_kinds.len()),
            ("fracture_intensity", self.fracture_intensity.len()),
            ("erodibility", self.erodibility.len()),
            ("relative_permeability", self.relative_permeability.len()),
            ("sediment_sources", self.sediment_sources.len()),
        ] {
            if found != expected {
                return Err(GeologicSubstrateValidationError::FieldLengthMismatch {
                    field,
                    expected,
                    found,
                });
            }
        }
        validate_bedrock_crust_compatibility(
            self.cell_count(),
            &self.bedrock_kinds,
            &self.crust_kinds,
        )?;

        for index in 0..expected {
            let cell = CellId::from_raw(index as u32);
            let crust = self
                .crust_kinds
                .get(index)
                .ok_or(GeologicSubstrateValidationError::InvalidCrustKind { cell })?;
            let thickness = self.crust_thickness_km[index];
            let (minimum, maximum) = match crust {
                CrustKind::Continental => (
                    CONTINENTAL_CRUST_MIN_THICKNESS_KM,
                    CONTINENTAL_CRUST_MAX_THICKNESS_KM,
                ),
                CrustKind::Oceanic => (
                    OCEANIC_CRUST_MIN_THICKNESS_KM,
                    OCEANIC_CRUST_MAX_THICKNESS_KM,
                ),
            };
            if !thickness.is_finite() || !(minimum..=maximum).contains(&thickness) {
                return Err(GeologicSubstrateValidationError::InvalidCrustThickness {
                    cell,
                    found: thickness,
                    minimum,
                    maximum,
                });
            }
            let age = self.ocean_age_myr[index];
            let valid_age = match crust {
                CrustKind::Continental => age == CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                CrustKind::Oceanic => age.is_finite() && (0.0..=MAX_CRUST_AGE_MYR).contains(&age),
            };
            if !valid_age {
                return Err(GeologicSubstrateValidationError::InvalidOceanAge {
                    cell,
                    crust,
                    found: age,
                });
            }
            let density = self.crust_density_kg_m3[index];
            if !density.is_finite()
                || !(CRUST_DENSITY_MIN_KG_M3..=CRUST_DENSITY_MAX_KG_M3).contains(&density)
            {
                return Err(GeologicSubstrateValidationError::InvalidDensity {
                    cell,
                    found: density,
                });
            }
            for (field, found) in [
                ("fracture_intensity", self.fracture_intensity[index]),
                ("erodibility", self.erodibility[index]),
                ("relative_permeability", self.relative_permeability[index]),
            ] {
                if !found.is_finite() || !(0.0..=1.0).contains(&found) {
                    return Err(GeologicSubstrateValidationError::InvalidNormalizedValue {
                        field,
                        cell,
                        found,
                    });
                }
            }
            let bedrock = self
                .bedrock_kinds
                .get(index)
                .expect("validated dense bedrock field");
            let expected_source = sediment_source_for_bedrock(bedrock);
            let found_source = self.sediment_sources.get(index).ok_or(
                GeologicSubstrateValidationError::InvalidSedimentSource {
                    cell: Some(cell),
                    found: u32::MAX,
                },
            )?;
            if found_source != expected_source {
                return Err(GeologicSubstrateValidationError::SedimentSourceMismatch {
                    cell,
                    bedrock,
                    found: found_source,
                    expected: expected_source,
                });
            }
        }
        Ok(())
    }

    /// Rechecks the exact authoritative surface without requiring tectonics.
    pub fn validate_against_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), GeologicSubstrateValidationError> {
        self.validate()?;
        surface.validate()?;
        let authoritative = SurfaceRef::for_spherical(surface);
        if self.surface_ref != authoritative {
            return Err(GeologicSubstrateValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        self.mantle.validate_against(surface)?;
        Ok(())
    }

    /// Rechecks every copied V5 field and the density recipe.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
        evolved: &EvolvedTectonicSnapshot,
    ) -> Result<(), GeologicSubstrateValidationError> {
        self.validate_against_surface(surface)?;
        evolved.validate_against(surface)?;
        let compatibility = evolved.compatibility();
        if self.crust_kinds.raw_values() != compatibility.crust_kinds().raw_values()
            || self.crust_thickness_km != compatibility.crust_thickness_km()
            || self.ocean_age_myr != compatibility.crust_age_myr()
        {
            return Err(GeologicSubstrateValidationError::CopiedTectonicFieldMismatch);
        }
        for index in 0..self.cell_count() as usize {
            let continental = evolved.material().continental_volume_m3()[index];
            let oceanic = evolved.material().oceanic_volume_m3()[index];
            let expected = effective_crust_density_kg_m3(continental, oceanic)?;
            let found = self.crust_density_kg_m3[index];
            if (found - expected).abs() > DENSITY_IDENTITY_TOLERANCE_KG_M3 {
                return Err(GeologicSubstrateValidationError::DensityIdentityMismatch {
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

    pub const fn cell_count(&self) -> u32 {
        self.surface_ref.cell_count()
    }

    pub const fn mantle(&self) -> &SphericalMantleSnapshot {
        &self.mantle
    }

    pub const fn crust_kinds(&self) -> &CrustKindField {
        &self.crust_kinds
    }

    pub fn crust_kind(&self, index: usize) -> Option<CrustKind> {
        self.crust_kinds.get(index)
    }

    pub fn crust_thickness_km(&self) -> &[f32] {
        &self.crust_thickness_km
    }

    pub fn ocean_age_myr(&self) -> &[f32] {
        &self.ocean_age_myr
    }

    pub fn crust_density_kg_m3(&self) -> &[f32] {
        &self.crust_density_kg_m3
    }

    pub const fn bedrock_kinds(&self) -> &BedrockKindField {
        &self.bedrock_kinds
    }

    pub fn bedrock_kind(&self, index: usize) -> Option<BedrockKind> {
        self.bedrock_kinds.get(index)
    }

    pub fn fracture_intensity(&self) -> &[f32] {
        &self.fracture_intensity
    }

    pub fn erodibility(&self) -> &[f32] {
        &self.erodibility
    }

    pub fn relative_permeability(&self) -> &[f32] {
        &self.relative_permeability
    }

    pub const fn sediment_sources(&self) -> &SedimentSourceKindField {
        &self.sediment_sources
    }

    pub fn sediment_source(&self, index: usize) -> Option<SedimentSourceKind> {
        self.sediment_sources.get(index)
    }

    pub fn heat_flow_mw_m2(&self) -> &[f32] {
        self.mantle.heat_flow_mw_m2()
    }

    pub fn volcanic_influence(&self) -> &[f32] {
        self.mantle.volcanic_influence()
    }
}

impl<'de> Deserialize<'de> for GeologicSubstrateSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GeologicSubstrateSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.mantle,
            CrustKindField::from_raw(wire.crust_kinds).map_err(D::Error::custom)?,
            wire.crust_thickness_km,
            wire.ocean_age_myr,
            wire.crust_density_kg_m3,
            BedrockKindField::new(wire.bedrock_kinds).map_err(D::Error::custom)?,
            wire.fracture_intensity,
            wire.erodibility,
            wire.relative_permeability,
            SedimentSourceKindField::from_raw(wire.sediment_sources).map_err(D::Error::custom)?,
        )
        .map_err(D::Error::custom)
    }
}

/// Computes the exact P3 volume-weighted density recipe.
pub fn effective_crust_density_kg_m3(
    continental_volume_m3: f64,
    oceanic_volume_m3: f64,
) -> Result<f32, GeologicSubstrateValidationError> {
    let total = continental_volume_m3 + oceanic_volume_m3;
    if !continental_volume_m3.is_finite()
        || !oceanic_volume_m3.is_finite()
        || continental_volume_m3 < 0.0
        || oceanic_volume_m3 < 0.0
        || !total.is_finite()
        || total <= 0.0
    {
        return Err(GeologicSubstrateValidationError::InvalidMaterialVolume {
            continental_volume_m3,
            oceanic_volume_m3,
        });
    }
    Ok(
        ((continental_volume_m3 * f64::from(CONTINENTAL_CRUST_DENSITY_KG_M3)
            + oceanic_volume_m3 * f64::from(OCEANIC_CRUST_DENSITY_KG_M3))
            / total) as f32,
    )
}

/// Maps broad lithology to the stable downstream source-rock category.
pub const fn sediment_source_for_bedrock(bedrock: BedrockKind) -> SedimentSourceKind {
    match bedrock {
        BedrockKind::OceanicMafic => SedimentSourceKind::Mafic,
        BedrockKind::ContinentalCrystalline => SedimentSourceKind::Felsic,
        BedrockKind::Sedimentary => SedimentSourceKind::Sedimentary,
        BedrockKind::Metamorphic => SedimentSourceKind::Metamorphic,
        BedrockKind::Volcanic => SedimentSourceKind::Volcaniclastic,
    }
}

fn validate_allocation(
    field: &'static str,
    found: usize,
    maximum: usize,
) -> Result<(), GeologicSubstrateValidationError> {
    if found > maximum {
        return Err(GeologicSubstrateValidationError::AllocationExceedsLimit {
            field,
            found,
            maximum,
        });
    }
    Ok(())
}

/// Failures in the strict P3 geologic-substrate contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GeologicSubstrateValidationError {
    #[error("unsupported geologic substrate schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("invalid substrate surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    #[error("geologic substrate requires spherical_v1 geometry, found {found:?}")]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    #[error("{field} allocation {found} exceeds spherical limit {maximum}")]
    AllocationExceedsLimit {
        field: &'static str,
        found: usize,
        maximum: usize,
    },
    #[error("invalid nested mantle snapshot: {0}")]
    InvalidMantle(#[from] SphericalMantleValidationError),
    #[error("nested {field} surface {found:?} does not match substrate {expected:?}")]
    NestedSurfaceMismatch {
        field: &'static str,
        expected: SurfaceRef,
        found: SurfaceRef,
    },
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("invalid copied crust category at {cell:?}")]
    InvalidCrustKind { cell: CellId },
    #[error("invalid crust category field: {0}")]
    InvalidCrustField(#[from] TectonicValidationError),
    #[error("cell {cell:?} crust thickness {found} is outside {minimum}..={maximum} km")]
    InvalidCrustThickness {
        cell: CellId,
        found: f32,
        minimum: f32,
        maximum: f32,
    },
    #[error("cell {cell:?} {crust:?} crust has invalid ocean age {found}")]
    InvalidOceanAge {
        cell: CellId,
        crust: CrustKind,
        found: f32,
    },
    #[error("cell {cell:?} crust density {found} is outside the P3 safety envelope")]
    InvalidDensity { cell: CellId, found: f32 },
    #[error("{field} at {cell:?} is {found}; expected 0..=1")]
    InvalidNormalizedValue {
        field: &'static str,
        cell: CellId,
        found: f32,
    },
    #[error("invalid bedrock/crust relationship: {0}")]
    InvalidGeology(#[from] GeologicValidationError),
    #[error("invalid sediment-source category {found} at {cell:?}")]
    InvalidSedimentSource { cell: Option<CellId>, found: u32 },
    #[error("cell {cell:?} bedrock {bedrock:?} requires {expected:?}, found {found:?}")]
    SedimentSourceMismatch {
        cell: CellId,
        bedrock: BedrockKind,
        found: SedimentSourceKind,
        expected: SedimentSourceKind,
    },
    #[error("invalid authoritative spherical surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("substrate surface {snapshot:?} does not match authority {authoritative:?}")]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    #[error("invalid evolved tectonic upstream: {0}")]
    InvalidEvolvedTectonics(#[from] EvolvedTectonicValidationError),
    #[error("substrate copied crust fields disagree with evolved tectonics")]
    CopiedTectonicFieldMismatch,
    #[error(
        "invalid material volumes continental={continental_volume_m3}, oceanic={oceanic_volume_m3}"
    )]
    InvalidMaterialVolume {
        continental_volume_m3: f64,
        oceanic_volume_m3: f64,
    },
    #[error("cell {cell:?} density {found} differs from volume-weighted {expected}")]
    DensityIdentityMismatch {
        cell: CellId,
        found: f32,
        expected: f32,
    },
}
