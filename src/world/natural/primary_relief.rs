use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::geology::validate_bedrock_crust_compatibility;
use super::surface_water_geometry::surface_elevation_fingerprint;
use super::{
    BedrockKind, BedrockKindField, CrustKind, CrustKindField, EvolvedTectonicSnapshot,
    EvolvedTectonicValidationError, GeologicValidationError, LandOceanField, LandOceanKind,
    ReliefSpec, ReliefSpecError, SeaLevelPolicy, SphericalMantleSnapshot,
    SphericalMantleValidationError, SurfaceWaterGeometry, SurfaceWaterGeometryValidationError,
    TectonicValidationError, COMPONENT_IDENTITY_TOLERANCE_M, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
    CONTINENTAL_CRUST_MAX_THICKNESS_KM, CONTINENTAL_CRUST_MIN_THICKNESS_KM,
    CRUST_BASE_ELEVATION_MIN_M, ELEVATION_MAX_M, ELEVATION_MIN_M, MATERIAL_THICKNESS_TOLERANCE_KM,
    MAX_CRUST_AGE_MYR, OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM,
    VOLCANIC_OFFSET_MAX_M, VOLCANIC_OFFSET_MIN_M,
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
/// Mantle density in the P3 local Airy column balance (Turcotte & Schubert 2014).
pub(crate) const PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3: f64 = 3_300.0;
/// Continental reference thickness in the frozen P3 Airy column.
pub(crate) const PRIMARY_RELIEF_CONTINENTAL_REFERENCE_THICKNESS_KM: f64 = 35.0;
/// Continental freeboard paired with the frozen P3 Airy reference column.
const PRIMARY_RELIEF_CONTINENTAL_REFERENCE_FREEBOARD_M: f64 = 250.0;
/// Oceanic reference thickness paired with the frozen P3 buoyancy correction.
pub(crate) const PRIMARY_RELIEF_OCEANIC_REFERENCE_THICKNESS_KM: f64 = 7.0;
/// The single exact P3 continental Airy projection used by generation and
/// support-domain derivation.
pub(crate) const fn continental_airy_elevation_exact_m(
    thickness_km: f64,
    crust_density_kg_m3: f64,
) -> f64 {
    PRIMARY_RELIEF_CONTINENTAL_REFERENCE_FREEBOARD_M
        + (((PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3 - crust_density_kg_m3) * thickness_km
            - (PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3 - CONTINENTAL_CRUST_DENSITY_KG_M3 as f64)
                * PRIMARY_RELIEF_CONTINENTAL_REFERENCE_THICKNESS_KM)
            / PRIMARY_RELIEF_AIRY_MANTLE_DENSITY_KG_M3)
            * 1_000.0
}
/// Exact upper image of the frozen P3 continental Airy input domain.
///
/// The thickness argument is the ceiling the **published** V5 contract admits,
/// not the nominal one: a consumer re-derives thickness as
/// `volume_m3 / reference_area_m2`, so V5 accepts and publishes columns up to
/// [`MATERIAL_THICKNESS_TOLERANCE_KM`] above the nominal cap. Taking the image
/// at the nominal cap instead made P3 reject saturated-thickness columns that
/// P2 had legitimately published, at a cost of 0.15 mm of envelope.
pub(crate) const CRUST_BASE_ELEVATION_MAX_EXACT_M: f64 = continental_airy_elevation_exact_m(
    CONTINENTAL_CRUST_MAX_THICKNESS_KM as f64 + MATERIAL_THICKNESS_TOLERANCE_KM,
    CONTINENTAL_CRUST_DENSITY_KG_M3 as f64,
);
/// Outward-rounded `f32` wire envelope for the exact P3 crust-base domain.
///
/// The scientific working-state check uses
/// `CRUST_BASE_ELEVATION_MAX_EXACT_M`; this value only prevents the published
/// schema from rounding its upper bound inward (Goldberg 1991; Higham 2002).
pub const CRUST_BASE_ELEVATION_MAX_M: f32 = {
    let nearest = CRUST_BASE_ELEVATION_MAX_EXACT_M as f32;
    if nearest as f64 >= CRUST_BASE_ELEVATION_MAX_EXACT_M {
        nearest
    } else {
        f32::from_bits(nearest.to_bits() + 1)
    }
};
/// Inclusive safety floor for a published effective crust density.
pub const CRUST_DENSITY_MIN_KG_M3: f32 = 2_500.0;
/// Inclusive safety ceiling for a published effective crust density.
pub const CRUST_DENSITY_MAX_KG_M3: f32 = 3_200.0;
/// Physical primary relief with cause-only components and authoritative water geometry.
pub const PRIMARY_RELIEF_SCHEMA_V3: u16 = 3;
/// NOAA/NGDC Earth ocean inventory used by the locked P3 water budget.
pub const EARTH_OCEAN_VOLUME_M3: f64 = 1.335e18;
/// Earth-radius reference paired with the locked ocean inventory.
pub const EARTH_WATER_REFERENCE_RADIUS_M: f64 = 6_371_000.0;
/// Maximum relative error allowed after sea level is stored as `f32`.
pub const WATER_VOLUME_RELATIVE_TOLERANCE: f64 = 1.0e-6;
/// Area-weighted mean total sediment thickness over CRUST1.0 oceanic crustal
/// types (Laske et al. 2013; computed in the T0 calibration spec §5.2), the
/// Earth anchor of the P3 pelagic sediment blanket.
pub const EARTH_OCEANIC_SEDIMENT_MEAN_THICKNESS_M: f32 = 659.0;
/// Area-weighted mean age of Earth's ocean crust (Seton et al. 2020), paired
/// with the mean sediment thickness to give the blanket's accumulation rate.
pub const EARTH_OCEAN_CRUST_MEAN_AGE_MYR: f32 = 64.2;
/// Bulk density of the compacting deep-sea sediment column (Hamilton 1976,
/// 0-1 km average) used by the Sclater & Christie 1980 backstripping ratio.
pub const OCEANIC_SEDIMENT_DENSITY_KG_M3: f32 = 2_000.0;
/// Sea-water density in the backstripping ratio.
pub const OCEAN_WATER_DENSITY_KG_M3: f32 = 1_030.0;
/// Minimum author-constraint tolerance in physical area fraction.
pub const MIN_LAND_FRACTION_CONSTRAINT_TOLERANCE: f32 = 0.02;
/// Safety bound for the separately published passive-margin component.
pub const PASSIVE_MARGIN_OFFSET_ABS_MAX_M: f32 = 2_000.0;
/// Safety bound for the separately published conditioned-detail component.
pub const CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M: f32 = 2_500.0;

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

/// Whether the physical water budget happened to satisfy the authored land request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LandFractionConstraintStatus {
    /// The physical land fraction falls inside the declared discretization tolerance.
    Satisfied,
    /// The request cannot be met without violating the locked physical water inventory.
    Infeasible,
}

/// Immutable pre-erosion relief with separate causal components and a physical water budget.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrimaryReliefSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    isostatic_base_m: Vec<f32>,
    volcanic_construction_m: Vec<f32>,
    passive_margin_offset_m: Vec<f32>,
    conditioned_regional_detail_m: Vec<f32>,
    elevation_m: Vec<f32>,
    water_inventory_m3: f64,
    surface_water_geometry: SurfaceWaterGeometry,
    requested_land_fraction: f32,
    physical_land_fraction: f32,
    land_fraction_tolerance: f32,
    constraint_status: LandFractionConstraintStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrimaryReliefSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    isostatic_base_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    volcanic_construction_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    passive_margin_offset_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    conditioned_regional_detail_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_dense_f32")]
    elevation_m: Vec<f32>,
    water_inventory_m3: f64,
    surface_water_geometry: SurfaceWaterGeometry,
    requested_land_fraction: f32,
    physical_land_fraction: f32,
    land_fraction_tolerance: f32,
    constraint_status: LandFractionConstraintStatus,
}

impl PrimaryReliefSnapshot {
    /// Constructs a snapshot only after every self-contained P3 invariant holds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        isostatic_base_m: Vec<f32>,
        volcanic_construction_m: Vec<f32>,
        passive_margin_offset_m: Vec<f32>,
        conditioned_regional_detail_m: Vec<f32>,
        elevation_m: Vec<f32>,
        water_inventory_m3: f64,
        surface_water_geometry: SurfaceWaterGeometry,
        requested_land_fraction: f32,
        physical_land_fraction: f32,
        land_fraction_tolerance: f32,
        constraint_status: LandFractionConstraintStatus,
    ) -> Result<Self, PrimaryReliefValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            isostatic_base_m,
            volcanic_construction_m,
            passive_margin_offset_m,
            conditioned_regional_detail_m,
            elevation_m,
            water_inventory_m3,
            surface_water_geometry,
            requested_land_fraction,
            physical_land_fraction,
            land_fraction_tolerance,
            constraint_status,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Rechecks local component, identity, and budget-closure invariants.
    pub fn validate(&self) -> Result<(), PrimaryReliefValidationError> {
        if self.schema_version != PRIMARY_RELIEF_SCHEMA_V3 {
            return Err(PrimaryReliefValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: PRIMARY_RELIEF_SCHEMA_V3,
            });
        }
        self.surface_ref.validate()?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(PrimaryReliefValidationError::InvalidSurfaceKind {
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
        self.surface_water_geometry.validate()?;
        if self.surface_water_geometry.surface_ref() != self.surface_ref {
            return Err(PrimaryReliefValidationError::WaterGeometrySurfaceMismatch {
                snapshot: self.surface_ref,
                geometry: self.surface_water_geometry.surface_ref(),
            });
        }

        let expected = self.cell_count() as usize;
        for (field, values, minimum, maximum) in [
            (
                "isostatic_base_m",
                self.isostatic_base_m.as_slice(),
                CRUST_BASE_ELEVATION_MIN_M,
                CRUST_BASE_ELEVATION_MAX_M,
            ),
            (
                "volcanic_construction_m",
                self.volcanic_construction_m.as_slice(),
                VOLCANIC_OFFSET_MIN_M,
                VOLCANIC_OFFSET_MAX_M,
            ),
            (
                "passive_margin_offset_m",
                self.passive_margin_offset_m.as_slice(),
                -PASSIVE_MARGIN_OFFSET_ABS_MAX_M,
                PASSIVE_MARGIN_OFFSET_ABS_MAX_M,
            ),
            (
                "conditioned_regional_detail_m",
                self.conditioned_regional_detail_m.as_slice(),
                -CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M,
                CONDITIONED_REGIONAL_DETAIL_ABS_MAX_M,
            ),
            (
                "elevation_m",
                self.elevation_m.as_slice(),
                ELEVATION_MIN_M,
                ELEVATION_MAX_M,
            ),
        ] {
            validate_primary_field(field, values, expected, minimum, maximum)?;
        }

        for index in 0..expected {
            let cell = CellId::from_raw(index as u32);
            let calculated = self.isostatic_base_m[index]
                + self.volcanic_construction_m[index]
                + self.passive_margin_offset_m[index]
                + self.conditioned_regional_detail_m[index];
            if (self.elevation_m[index] - calculated).abs() > COMPONENT_IDENTITY_TOLERANCE_M {
                return Err(PrimaryReliefValidationError::ComponentIdentityMismatch {
                    cell,
                    elevation: self.elevation_m[index],
                    calculated,
                });
            }
        }

        if self.surface_water_geometry.elevation_fingerprint()
            != &surface_elevation_fingerprint(&self.elevation_m)
        {
            return Err(SurfaceWaterGeometryValidationError::ElevationFingerprintMismatch.into());
        }
        validate_non_negative_f64("water_inventory_m3", self.water_inventory_m3)?;
        let realized_water_volume_m3 = self.surface_water_geometry.total_water_volume_m3();
        validate_non_negative_f64("realized_water_volume_m3", realized_water_volume_m3)?;
        let relative_error =
            water_volume_relative_error(realized_water_volume_m3, self.water_inventory_m3);
        if relative_error > WATER_VOLUME_RELATIVE_TOLERANCE {
            return Err(PrimaryReliefValidationError::WaterVolumeClosureExceeded {
                realized: realized_water_volume_m3,
                inventory: self.water_inventory_m3,
                relative_error,
                maximum: WATER_VOLUME_RELATIVE_TOLERANCE,
            });
        }
        for (field, found) in [
            ("requested_land_fraction", self.requested_land_fraction),
            ("physical_land_fraction", self.physical_land_fraction),
            ("land_fraction_tolerance", self.land_fraction_tolerance),
        ] {
            if !found.is_finite() || !(0.0..=1.0).contains(&found) {
                return Err(PrimaryReliefValidationError::InvalidFraction { field, found });
            }
        }
        if self.land_fraction_tolerance < MIN_LAND_FRACTION_CONSTRAINT_TOLERANCE {
            return Err(PrimaryReliefValidationError::ConstraintToleranceTooSmall {
                found: self.land_fraction_tolerance,
                minimum: MIN_LAND_FRACTION_CONSTRAINT_TOLERANCE,
            });
        }
        let expected_status = constraint_status(
            self.requested_land_fraction,
            self.physical_land_fraction,
            self.land_fraction_tolerance,
        );
        if self.constraint_status != expected_status {
            return Err(PrimaryReliefValidationError::ConstraintStatusMismatch {
                stored: self.constraint_status,
                expected: expected_status,
            });
        }
        Ok(())
    }

    /// Recomputes surface-area water and authored-constraint quantities.
    pub fn validate_against_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief_spec: &ReliefSpec,
    ) -> Result<(), PrimaryReliefValidationError> {
        self.validate_against_surface_measurements(surface)?;
        self.validate_authored_policy(surface, relief_spec)
    }

    /// Validates the surface identity and authored policy without regenerating water geometry.
    pub fn validate_against_authoring(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief_spec: &ReliefSpec,
    ) -> Result<(), PrimaryReliefValidationError> {
        self.validate_against_surface_measurements(surface)?;
        self.validate_authored_policy(surface, relief_spec)
    }

    fn validate_authored_policy(
        &self,
        surface: &SphericalSurfaceSnapshot,
        relief_spec: &ReliefSpec,
    ) -> Result<(), PrimaryReliefValidationError> {
        relief_spec.validate()?;
        if self.requested_land_fraction.to_bits() != relief_spec.target_land_fraction.to_bits() {
            return Err(
                PrimaryReliefValidationError::RequestedLandFractionMismatch {
                    stored: self.requested_land_fraction,
                    authored: relief_spec.target_land_fraction,
                },
            );
        }
        if relief_spec.sea_level_policy == SeaLevelPolicy::TargetLandFraction
            && self.constraint_status != LandFractionConstraintStatus::Satisfied
        {
            return Err(
                PrimaryReliefValidationError::TargetLandFractionNotSatisfied {
                    requested: self.requested_land_fraction,
                    actual: self.physical_land_fraction,
                    tolerance: self.land_fraction_tolerance,
                },
            );
        }
        if relief_spec.sea_level_policy == SeaLevelPolicy::WaterInventory {
            let total_area = compensated_sum(surface.cells().iter().map(|cell| cell.area.get()));
            let expected_inventory = scaled_earth_ocean_inventory_m3(total_area)?
                * f64::from(relief_spec.water_inventory_ratio);
            validate_close_f64(
                "water_inventory_m3",
                self.water_inventory_m3,
                expected_inventory,
            )?;
        }
        Ok(())
    }

    /// Recomputes surface-bound measurements without an unavailable authoring policy.
    pub(crate) fn validate_against_surface_measurements(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), PrimaryReliefValidationError> {
        self.validate()?;
        surface.validate()?;
        let authoritative = SurfaceRef::for_spherical(surface);
        if self.surface_ref != authoritative {
            return Err(PrimaryReliefValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        self.surface_water_geometry
            .validate_against(surface, &self.elevation_m)?;
        let physical = self
            .surface_water_geometry
            .global_land_area_fraction(surface)?;
        if (self.physical_land_fraction - physical).abs() > 1.0e-6 {
            return Err(PrimaryReliefValidationError::PhysicalLandFractionMismatch {
                stored: self.physical_land_fraction,
                recomputed: physical,
            });
        }
        let tolerance = land_fraction_constraint_tolerance(surface)?;
        if self.land_fraction_tolerance.to_bits() != tolerance.to_bits() {
            return Err(
                PrimaryReliefValidationError::LandFractionToleranceMismatch {
                    stored: self.land_fraction_tolerance,
                    recomputed: tolerance,
                },
            );
        }
        Ok(())
    }

    /// Adds exact substrate identity validation to the surface and water checks.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
        substrate: &GeologicSubstrateSnapshot,
        relief_spec: &ReliefSpec,
    ) -> Result<(), PrimaryReliefValidationError> {
        substrate.validate_against_surface(surface)?;
        self.validate_against_surface(surface, relief_spec)
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

    pub const fn surface_water_geometry(&self) -> &SurfaceWaterGeometry {
        &self.surface_water_geometry
    }

    pub fn isostatic_base_m(&self) -> &[f32] {
        &self.isostatic_base_m
    }

    pub fn volcanic_construction_m(&self) -> &[f32] {
        &self.volcanic_construction_m
    }

    pub fn passive_margin_offset_m(&self) -> &[f32] {
        &self.passive_margin_offset_m
    }

    pub fn conditioned_regional_detail_m(&self) -> &[f32] {
        &self.conditioned_regional_detail_m
    }

    pub fn elevation_m(&self) -> &[f32] {
        &self.elevation_m
    }

    pub const fn sea_level_m(&self) -> f32 {
        self.surface_water_geometry.sea_level_m()
    }

    pub const fn land_ocean(&self) -> &LandOceanField {
        self.surface_water_geometry.land_ocean()
    }

    pub const fn water_inventory_m3(&self) -> f64 {
        self.water_inventory_m3
    }

    /// Returns inventory relative to the area-scaled Earth ocean reference.
    ///
    /// The ratio definition is frozen in the T0b design §3.3; keeping it here
    /// lets quality evidence and product presentation share the snapshot truth.
    pub(crate) fn water_inventory_ratio(
        &self,
        total_surface_area_m2: f64,
    ) -> Result<f64, PrimaryReliefValidationError> {
        Ok(self.water_inventory_m3 / scaled_earth_ocean_inventory_m3(total_surface_area_m2)?)
    }

    pub fn realized_water_volume_m3(&self) -> f64 {
        self.surface_water_geometry.total_water_volume_m3()
    }

    pub fn water_volume_relative_error(&self) -> f64 {
        water_volume_relative_error(self.realized_water_volume_m3(), self.water_inventory_m3)
    }

    pub const fn requested_land_fraction(&self) -> f32 {
        self.requested_land_fraction
    }

    pub const fn physical_land_fraction(&self) -> f32 {
        self.physical_land_fraction
    }

    pub const fn land_fraction_tolerance(&self) -> f32 {
        self.land_fraction_tolerance
    }

    pub const fn constraint_status(&self) -> LandFractionConstraintStatus {
        self.constraint_status
    }
}

impl<'de> Deserialize<'de> for PrimaryReliefSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PrimaryReliefSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.isostatic_base_m,
            wire.volcanic_construction_m,
            wire.passive_margin_offset_m,
            wire.conditioned_regional_detail_m,
            wire.elevation_m,
            wire.water_inventory_m3,
            wire.surface_water_geometry,
            wire.requested_land_fraction,
            wire.physical_land_fraction,
            wire.land_fraction_tolerance,
            wire.constraint_status,
        )
        .map_err(D::Error::custom)
    }
}

/// Result of the continuous P1 water-volume solve after `f32` publication.
#[derive(Debug, Clone, PartialEq)]
pub struct WaterVolumeSolution {
    geometry: SurfaceWaterGeometry,
    relative_error: f64,
}

impl WaterVolumeSolution {
    pub(crate) fn from_geometry(
        geometry: SurfaceWaterGeometry,
        water_inventory_m3: f64,
    ) -> Result<Self, WaterVolumeSolveError> {
        geometry.validate()?;
        if !water_inventory_m3.is_finite() || water_inventory_m3 < 0.0 {
            return Err(WaterVolumeSolveError::InvalidInventory {
                found: water_inventory_m3,
            });
        }
        let realized = geometry.total_water_volume_m3();
        let relative_error = water_volume_relative_error(realized, water_inventory_m3);
        if relative_error > WATER_VOLUME_RELATIVE_TOLERANCE {
            return Err(WaterVolumeSolveError::ClosureExceeded {
                realized,
                inventory: water_inventory_m3,
                relative_error,
                maximum: WATER_VOLUME_RELATIVE_TOLERANCE,
            });
        }
        Ok(Self {
            geometry,
            relative_error,
        })
    }

    pub const fn sea_level_m(&self) -> f32 {
        self.geometry.sea_level_m()
    }

    pub fn realized_water_volume_m3(&self) -> f64 {
        self.geometry.total_water_volume_m3()
    }

    pub const fn relative_error(&self) -> f64 {
        self.relative_error
    }

    pub const fn geometry(&self) -> &SurfaceWaterGeometry {
        &self.geometry
    }

    pub fn into_geometry(self) -> SurfaceWaterGeometry {
        self.geometry
    }
}

/// Scales the locked Earth ocean inventory by spherical surface area.
pub fn scaled_earth_ocean_inventory_m3(
    total_surface_area_m2: f64,
) -> Result<f64, WaterVolumeSolveError> {
    if !total_surface_area_m2.is_finite() || total_surface_area_m2 <= 0.0 {
        return Err(WaterVolumeSolveError::InvalidSurfaceArea {
            found: total_surface_area_m2,
        });
    }
    let reference_area = 4.0
        * std::f64::consts::PI
        * EARTH_WATER_REFERENCE_RADIUS_M
        * EARTH_WATER_REFERENCE_RADIUS_M;
    Ok(EARTH_OCEAN_VOLUME_M3 * (total_surface_area_m2 / reference_area))
}

/// Returns the larger of the locked 2% tolerance and one-cell area quantization.
pub fn land_fraction_constraint_tolerance(
    surface: &SphericalSurfaceSnapshot,
) -> Result<f32, PrimaryReliefValidationError> {
    surface.validate()?;
    let total = compensated_sum(surface.cells().iter().map(|cell| cell.area.get()));
    let maximum = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .fold(0.0_f64, f64::max);
    Ok((maximum / total).max(f64::from(MIN_LAND_FRACTION_CONSTRAINT_TOLERANCE)) as f32)
}

/// Derives constraint status without changing physical sea level.
pub fn constraint_status(
    requested: f32,
    physical: f32,
    tolerance: f32,
) -> LandFractionConstraintStatus {
    if (requested - physical).abs() <= tolerance {
        LandFractionConstraintStatus::Satisfied
    } else {
        LandFractionConstraintStatus::Infeasible
    }
}

fn validate_primary_field(
    field: &'static str,
    values: &[f32],
    expected: usize,
    minimum: f32,
    maximum: f32,
) -> Result<(), PrimaryReliefValidationError> {
    if values.len() != expected {
        return Err(PrimaryReliefValidationError::FieldLengthMismatch {
            field,
            expected,
            found: values.len(),
        });
    }
    for (index, &found) in values.iter().enumerate() {
        if !found.is_finite() || !(minimum..=maximum).contains(&found) {
            return Err(PrimaryReliefValidationError::FieldValueOutOfRange {
                field,
                cell: CellId::from_raw(index as u32),
                found,
                minimum,
                maximum,
            });
        }
    }
    Ok(())
}

fn validate_non_negative_f64(
    field: &'static str,
    found: f64,
) -> Result<(), PrimaryReliefValidationError> {
    if !found.is_finite() || found < 0.0 {
        return Err(PrimaryReliefValidationError::InvalidNonNegativeValue { field, found });
    }
    Ok(())
}

fn validate_close_f64(
    field: &'static str,
    stored: f64,
    recomputed: f64,
) -> Result<(), PrimaryReliefValidationError> {
    let relative_error = (stored - recomputed).abs() / recomputed.abs().max(1.0);
    if relative_error > 1.0e-12 {
        return Err(PrimaryReliefValidationError::RecomputedScalarMismatch {
            field,
            stored,
            recomputed,
            relative_error,
        });
    }
    Ok(())
}

pub(crate) fn water_volume_relative_error(realized: f64, inventory: f64) -> f64 {
    (realized - inventory).abs() / inventory.abs().max(1.0)
}

#[derive(Debug, Default)]
struct CompensatedSum {
    sum: f64,
    correction: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let next = self.sum + value;
        if self.sum.abs() >= value.abs() {
            self.correction += (self.sum - next) + value;
        } else {
            self.correction += (value - next) + self.sum;
        }
        self.sum = next;
    }

    fn total(&self) -> f64 {
        self.sum + self.correction
    }
}

fn compensated_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = CompensatedSum::default();
    for value in values {
        sum.add(value);
    }
    sum.total()
}

/// Failures from the continuous P1 physical-water operator.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum WaterVolumeSolveError {
    #[error("physical sea-level solve cancelled")]
    Cancelled,
    #[error("physical sea-level solve requires at least one cell")]
    EmptySurface,
    #[error("elevation count {elevations} differs from surface cell count {areas}")]
    LengthMismatch { elevations: usize, areas: usize },
    #[error("invalid elevation {found} at dense index {index}")]
    InvalidElevation { index: usize, found: f64 },
    #[error("invalid water inventory {found}")]
    InvalidInventory { found: f64 },
    #[error("invalid total surface area {found}")]
    InvalidSurfaceArea { found: f64 },
    #[error("invalid published sea level {found}")]
    InvalidSeaLevel { found: f64 },
    #[error("physical sea-level solve produced non-finite or unrepresentable level {found}")]
    NonFiniteSolution { found: f64 },
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("invalid surface-water geometry: {0}")]
    InvalidGeometry(#[from] SurfaceWaterGeometryValidationError),
    #[error("cell {cell:?} fan side {side} is not a valid positive-area triangle")]
    InvalidFanTriangle { cell: CellId, side: usize },
    #[error("{field} fraction {found} at index {index} is outside 0..=1")]
    InvalidWorkingFraction {
        field: &'static str,
        index: usize,
        found: f64,
    },
    #[error("{field} value {found} at index {index} must be finite and non-negative")]
    InvalidWorkingNonNegativeValue {
        field: &'static str,
        index: usize,
        found: f64,
    },
    #[error(
        "working water geometry surface {geometry:?} differs from authority {authoritative:?}"
    )]
    WorkingSurfaceMismatch {
        geometry: SurfaceRef,
        authoritative: SurfaceRef,
    },
    #[error(
        "cell {cell:?} exact land/ocean kind {exact:?} differs from projected wire {projected:?}"
    )]
    LandOceanProjectionMismatch {
        cell: CellId,
        exact: LandOceanKind,
        projected: LandOceanKind,
    },
    #[error(
        "quantized water volume {realized} differs from inventory {inventory} by {relative_error}; maximum is {maximum}"
    )]
    ClosureExceeded {
        realized: f64,
        inventory: f64,
        relative_error: f64,
        maximum: f64,
    },
}

/// Failures in the strict P3 primary-relief contract.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PrimaryReliefValidationError {
    #[error("unsupported primary relief schema {found}; supported version is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("invalid primary relief surface identity: {0}")]
    InvalidSurfaceRef(#[from] SurfaceRefError),
    #[error("primary relief requires spherical_v1 geometry, found {found:?}")]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    #[error(
        "surface-water geometry surface {geometry:?} differs from primary relief {snapshot:?}"
    )]
    WaterGeometrySurfaceMismatch {
        snapshot: SurfaceRef,
        geometry: SurfaceRef,
    },
    #[error("field {field} has length {found}; expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        expected: usize,
        found: usize,
    },
    #[error("{field} at {cell:?} is {found}; expected {minimum}..={maximum}")]
    FieldValueOutOfRange {
        field: &'static str,
        cell: CellId,
        found: f32,
        minimum: f32,
        maximum: f32,
    },
    #[error("cell {cell:?} elevation {elevation} differs from causal sum {calculated}")]
    ComponentIdentityMismatch {
        cell: CellId,
        elevation: f32,
        calculated: f32,
    },
    #[error("{field} must be finite and non-negative, found {found}")]
    InvalidNonNegativeValue { field: &'static str, found: f64 },
    #[error(
        "realized water {realized} differs from inventory {inventory} by {relative_error}; maximum is {maximum}"
    )]
    WaterVolumeClosureExceeded {
        realized: f64,
        inventory: f64,
        relative_error: f64,
        maximum: f64,
    },
    #[error("{field} fraction {found} is outside 0..=1")]
    InvalidFraction { field: &'static str, found: f32 },
    #[error("land-fraction tolerance {found} is below {minimum}")]
    ConstraintToleranceTooSmall { found: f32, minimum: f32 },
    #[error("constraint status {stored:?} differs from derived {expected:?}")]
    ConstraintStatusMismatch {
        stored: LandFractionConstraintStatus,
        expected: LandFractionConstraintStatus,
    },
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(#[from] SphericalSurfaceValidationError),
    #[error("invalid surface-water geometry: {0}")]
    InvalidSurfaceWaterGeometry(#[from] SurfaceWaterGeometryValidationError),
    #[error("primary relief surface {snapshot:?} differs from authority {authoritative:?}")]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    #[error("invalid relief authoring specification: {0}")]
    InvalidReliefSpec(#[from] ReliefSpecError),
    #[error("requested land fraction {stored} differs from authored {authored}")]
    RequestedLandFractionMismatch { stored: f32, authored: f32 },
    #[error(
        "target-driven sea level did not satisfy land fraction {requested}: actual {actual}, tolerance {tolerance}"
    )]
    TargetLandFractionNotSatisfied {
        requested: f32,
        actual: f32,
        tolerance: f32,
    },
    #[error("physical water solve is invalid: {0}")]
    InvalidWaterSolve(#[from] WaterVolumeSolveError),
    #[error("{field} stored {stored} differs from recomputed {recomputed} by {relative_error}")]
    RecomputedScalarMismatch {
        field: &'static str,
        stored: f64,
        recomputed: f64,
        relative_error: f64,
    },
    #[error("physical land fraction {stored} differs from recomputed {recomputed}")]
    PhysicalLandFractionMismatch { stored: f32, recomputed: f32 },
    #[error("land-fraction tolerance {stored} differs from recomputed {recomputed}")]
    LandFractionToleranceMismatch { stored: f32, recomputed: f32 },
    #[error("invalid geologic substrate upstream: {0}")]
    InvalidSubstrate(#[from] GeologicSubstrateValidationError),
}
