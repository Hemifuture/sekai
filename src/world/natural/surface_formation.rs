use std::io::{self, Write};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    water_volume_at_sea_level_m3, GlobalCirculationSnapshot, LandOceanField, LandOceanKind,
    NaturalQualityProfile, SedimentSourceKind, SphericalHydrologySnapshot,
    CLIMATOLOGICAL_YEAR_SECONDS, ELEVATION_MAX_M, ELEVATION_MIN_M, WATER_VOLUME_RELATIVE_TOLERANCE,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceGeometryKind, SurfaceRef};
use crate::world::MAX_SPHERICAL_CELL_COUNT;

/// The first strict coupled geomorphic-formation product schema.
pub const NATURAL_SURFACE_FORMATION_SCHEMA_V1: u16 = 1;
/// The first strict P5 resume/checkpoint schema.
pub const SURFACE_FORMATION_CHECKPOINT_SCHEMA_V1: u16 = 1;
/// The first strict P5 retained terrain/process-field schema.
pub const FORMATION_TERRAIN_FIELDS_SCHEMA_V1: u16 = 1;
/// The fixed number of retained sediment-source provenance channels.
pub const SEDIMENT_PROVENANCE_SOURCE_COUNT: usize = 5;
/// The bounded outer climate/terrain fixed-point iteration count.
pub const SURFACE_FORMATION_MAX_OUTER_ITERATIONS: u8 = 4;
/// The complete geomorphic solve count within one outer iteration.
pub const SURFACE_FORMATION_MACRO_STEPS: u16 = 8;
/// The declared coarse-grained geomorphic formation horizon.
pub const SURFACE_FORMATION_HORIZON_YEARS: f64 = 100_000.0;
/// The fixed duration of each geomorphic macro step.
pub const SURFACE_FORMATION_MACRO_STEP_YEARS: f64 = 12_500.0;
/// Minimum precipitation fraction retained as effective P5 runoff.
pub const FORMATION_RUNOFF_MIN_FRACTION: f64 = 0.15;
/// Additional runoff fraction removed linearly by unit permeability.
pub const FORMATION_RUNOFF_PERMEABILITY_RANGE: f64 = 0.70;
/// Minimum depression depth retained as a P5 lake after centimeter routing.
pub const FORMATION_MINIMUM_LAKE_DEPTH_M: f64 = 1.0;
/// Drainage-area exponent in the locked stream-power law.
pub const FORMATION_STREAM_POWER_AREA_EXPONENT: f64 = 0.5;
/// Slope exponent in the locked stream-power law.
pub const FORMATION_STREAM_POWER_SLOPE_EXPONENT: f64 = 1.0;
/// Minimum active channel slope in the locked stream-power law.
pub const FORMATION_STREAM_POWER_SLOPE_THRESHOLD: f64 = 1.0e-5;
/// Reference annual erodibility in the locked stream-power law.
pub const FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR: f64 = 5.0e-6;
/// Baseline multiplier retained even for resistant substrate.
pub const FORMATION_STREAM_POWER_ERODIBILITY_BASE: f64 = 0.25;
/// Additional multiplier contributed by unit substrate erodibility.
pub const FORMATION_STREAM_POWER_ERODIBILITY_RANGE: f64 = 1.50;
/// Annual effective-runoff reference used by the erodibility response.
pub const FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM: f64 = 1_000.0;
/// Lower positive-runoff factor admitted by the stream-power response.
pub const FORMATION_STREAM_POWER_RUNOFF_FACTOR_MIN: f64 = 0.10;
/// Upper positive-runoff factor admitted by the stream-power response.
pub const FORMATION_STREAM_POWER_RUNOFF_FACTOR_MAX: f64 = 4.0;
/// Base nonlinear hillslope diffusivity.
pub const FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR: f64 = 5_000.0;
/// Tangent of the fixed 32-degree critical hillslope angle.
pub const FORMATION_HILLSLOPE_CRITICAL_SLOPE: f64 = 0.624_869_351_909_327_5;
/// Floor preventing the nonlinear critical-slope denominator from diverging.
pub const FORMATION_HILLSLOPE_DENOMINATOR_MIN: f64 = 0.10;
/// Baseline substrate multiplier for coarse hillslope transport.
pub const FORMATION_HILLSLOPE_ERODIBILITY_BASE: f64 = 0.25;
/// Additional hillslope multiplier contributed by unit erodibility.
pub const FORMATION_HILLSLOPE_ERODIBILITY_RANGE: f64 = 0.75;
/// Baseline fracture multiplier for coarse hillslope transport.
pub const FORMATION_HILLSLOPE_FRACTURE_BASE: f64 = 0.50;
/// Additional hillslope multiplier contributed by unit fracture intensity.
pub const FORMATION_HILLSLOPE_FRACTURE_RANGE: f64 = 0.50;
/// Baseline weathering multiplier at zero annual precipitation.
pub const FORMATION_HILLSLOPE_WEATHERING_BASE: f64 = 0.50;
/// Additional normalized wet-weather multiplier.
pub const FORMATION_HILLSLOPE_WEATHERING_RANGE: f64 = 0.50;
/// Annual precipitation reference used by the hillslope weathering factor.
pub const FORMATION_HILLSLOPE_PRECIPITATION_REFERENCE_MM: f64 = 1_000.0;
/// Maximum normalized annual precipitation before weathering saturates.
pub const FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX: f64 = 4.0;
/// Maximum fraction of local relief changed at either end of one edge per step.
pub const FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION: f64 = 0.25;
/// Reference routed-sediment transport concentration.
pub const FORMATION_SEDIMENT_CAPACITY_KG_M3: f64 = 0.5;
/// Slope scale in the bounded routed-sediment capacity response.
pub const FORMATION_SEDIMENT_SLOPE_SCALE: f64 = 0.001;
/// Fixed coarse alluvial bulk density used for every retained deposit.
pub const FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3: f64 = 1_800.0;
/// Maximum non-lake floodplain accommodation per macro step.
pub const FORMATION_FLOODPLAIN_ACCOMMODATION_M: f64 = 50.0;
/// Fixed coarse shelf-break depth limiting marine accommodation.
pub const FORMATION_SHELF_BREAK_DEPTH_M: f64 = 200.0;
/// Normal-wind scale in the bounded coastal exposure proxy.
pub const FORMATION_COASTAL_WIND_REFERENCE_M_S: f64 = 15.0;
/// Alongshore-current scale in the bounded coastal exposure proxy.
pub const FORMATION_COASTAL_CURRENT_REFERENCE_M_S: f64 = 1.0;
/// Sediment-cover thickness that halves coastal bedrock exposure.
pub const FORMATION_COASTAL_COVER_SHIELD_M: f64 = 10.0;
/// Exposure multiplier applied to marine transport capacity.
pub const FORMATION_MARINE_CAPACITY_EXPOSURE_RANGE: f64 = 4.0;
/// Maximum annual bedrock-coast erosion rate.
pub const FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR: f64 = 2.0e-5;
/// Mantle density used by the local Airy response.
pub const FORMATION_AIRY_MANTLE_DENSITY_KG_M3: f64 = 3_300.0;
/// Residence horizon used for the effective endorheic classification.
pub const FORMATION_ENDORHEIC_RESIDENCE_YEARS: f64 = 1_000.0;
/// Maximum relative residual for the global retained sediment ledger.
pub const SEDIMENT_BUDGET_RELATIVE_ERROR_MAX: f64 = 1.0e-8;
/// Maximum relative residual for each source-provenance sediment ledger.
pub const SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX: f64 = 1.0e-7;
/// Maximum conservative dense-owner report admitted by the P5 schema.
pub const SURFACE_FORMATION_DENSE_STATE_BYTES_MAX: u64 = 1_073_741_824;

/// Elevation RMS scale used by the outer fixed-point residual.
pub const FORMATION_ELEVATION_RESIDUAL_SCALE_M: f64 = 100.0;
/// Receiver-change scale used by the outer fixed-point residual.
pub const FORMATION_RECEIVER_RESIDUAL_SCALE: f64 = 0.05;
/// Log-discharge RMS scale used by the outer fixed-point residual.
pub const FORMATION_LOG_DISCHARGE_RESIDUAL_SCALE: f64 = 0.15;
/// Sediment-thickness RMS scale used by the outer fixed-point residual.
pub const FORMATION_SEDIMENT_RESIDUAL_SCALE_M: f64 = 10.0;
/// Coastline area-change scale used by the outer fixed-point residual.
pub const FORMATION_COASTLINE_RESIDUAL_SCALE: f64 = 0.005;

const MAX_FORMATION_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_COMPONENT_ABS_M: f32 = 100_000.0;
const MAX_SEDIMENT_THICKNESS_M: f32 = 100_000.0;
const PROVENANCE_SUM_TOLERANCE: f64 = 1.0e-6;

/// The one production geomorphic equation family admitted by P5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceFormationModelId {
    /// Priority-Flood + implicit stream power + paired nonlinear hillslope,
    /// provenance sediment, coastal exchange, and local Airy isostasy.
    PriorityFloodFastscapeSedimentHillslopeCoastIsostasyV1,
}

/// Returns the canonical identity of every equation and frozen P5 constant.
pub fn surface_formation_model_fingerprint() -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.surface-formation-equations.v1\0");
    hasher.update(&[surface_formation_model_tag(
        SurfaceFormationModelId::PriorityFloodFastscapeSedimentHillslopeCoastIsostasyV1,
    )]);
    for value in [
        SURFACE_FORMATION_HORIZON_YEARS,
        SURFACE_FORMATION_MACRO_STEP_YEARS,
        FORMATION_RUNOFF_MIN_FRACTION,
        FORMATION_RUNOFF_PERMEABILITY_RANGE,
        FORMATION_MINIMUM_LAKE_DEPTH_M,
        FORMATION_STREAM_POWER_AREA_EXPONENT,
        FORMATION_STREAM_POWER_SLOPE_EXPONENT,
        FORMATION_STREAM_POWER_SLOPE_THRESHOLD,
        FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR,
        FORMATION_STREAM_POWER_ERODIBILITY_BASE,
        FORMATION_STREAM_POWER_ERODIBILITY_RANGE,
        FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM,
        FORMATION_STREAM_POWER_RUNOFF_FACTOR_MIN,
        FORMATION_STREAM_POWER_RUNOFF_FACTOR_MAX,
        FORMATION_HILLSLOPE_DIFFUSIVITY_M2_PER_YEAR,
        FORMATION_HILLSLOPE_CRITICAL_SLOPE,
        FORMATION_HILLSLOPE_DENOMINATOR_MIN,
        FORMATION_HILLSLOPE_ERODIBILITY_BASE,
        FORMATION_HILLSLOPE_ERODIBILITY_RANGE,
        FORMATION_HILLSLOPE_FRACTURE_BASE,
        FORMATION_HILLSLOPE_FRACTURE_RANGE,
        FORMATION_HILLSLOPE_WEATHERING_BASE,
        FORMATION_HILLSLOPE_WEATHERING_RANGE,
        FORMATION_HILLSLOPE_PRECIPITATION_REFERENCE_MM,
        FORMATION_HILLSLOPE_PRECIPITATION_FACTOR_MAX,
        FORMATION_HILLSLOPE_RELIEF_LIMIT_FRACTION,
        FORMATION_SEDIMENT_CAPACITY_KG_M3,
        FORMATION_SEDIMENT_SLOPE_SCALE,
        FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
        FORMATION_FLOODPLAIN_ACCOMMODATION_M,
        FORMATION_SHELF_BREAK_DEPTH_M,
        FORMATION_COASTAL_WIND_REFERENCE_M_S,
        FORMATION_COASTAL_CURRENT_REFERENCE_M_S,
        FORMATION_COASTAL_COVER_SHIELD_M,
        FORMATION_MARINE_CAPACITY_EXPOSURE_RANGE,
        FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR,
        FORMATION_AIRY_MANTLE_DENSITY_KG_M3,
        FORMATION_ENDORHEIC_RESIDENCE_YEARS,
        CLIMATOLOGICAL_YEAR_SECONDS,
        FORMATION_ELEVATION_RESIDUAL_SCALE_M,
        FORMATION_RECEIVER_RESIDUAL_SCALE,
        FORMATION_LOG_DISCHARGE_RESIDUAL_SCALE,
        FORMATION_SEDIMENT_RESIDUAL_SCALE_M,
        FORMATION_COASTLINE_RESIDUAL_SCALE,
        SEDIMENT_BUDGET_RELATIVE_ERROR_MAX,
        SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX,
        WATER_VOLUME_RELATIVE_TOLERANCE,
    ] {
        hasher.update(&value.to_bits().to_le_bytes());
    }
    hasher.update(&SURFACE_FORMATION_MACRO_STEPS.to_le_bytes());
    hasher.update(&[SURFACE_FORMATION_MAX_OUTER_ITERATIONS]);
    hasher.update(b"priority-flood-stable-dag-v1\0");
    hasher.update(b"braun-willett-n1-backward-euler-v1\0");
    hasher.update(b"roering-paired-finite-volume-v1\0");
    hasher.update(b"five-source-upstream-sediment-ledger-v1\0");
    hasher.update(b"sources:felsic,mafic,volcaniclastic,sedimentary,metamorphic\0");
    hasher.update(
        b"elevation:primary+tectonic-fluvial-hillslope_erosion+hillslope_deposition+routed_deposition-coastal_erosion+coastal_deposition+isostatic\0",
    );
    hasher.update(b"fixed-water-volume-piecewise-linear-sea-level-v1\0");
    *hasher.finalize().as_bytes()
}

const fn surface_formation_model_tag(model: SurfaceFormationModelId) -> u8 {
    match model {
        SurfaceFormationModelId::PriorityFloodFastscapeSedimentHillslopeCoastIsostasyV1 => 1,
    }
}

const fn natural_quality_profile_tag(profile: NaturalQualityProfile) -> u8 {
    match profile {
        NaturalQualityProfile::Draft => 1,
        NaturalQualityProfile::Standard => 2,
        NaturalQualityProfile::High => 3,
    }
}

const fn surface_geometry_tag(kind: SurfaceGeometryKind) -> u8 {
    match kind {
        SurfaceGeometryKind::PlanarV1 => 1,
        SurfaceGeometryKind::SphericalV1 => 2,
        SurfaceGeometryKind::SphericalGeodesicV2 => 3,
    }
}

/// Exact upstream identities consumed by one P5 solve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceFormationUpstreamFingerprints {
    evolved_tectonic_fingerprint: [u8; 32],
    geologic_substrate_fingerprint: [u8; 32],
    primary_relief_fingerprint: [u8; 32],
    climate_work_domain_fingerprint: [u8; 32],
    climate_spec_fingerprint: [u8; 32],
    initial_climate_checkpoint_fingerprint: [u8; 32],
    formation_spec_fingerprint: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceFormationUpstreamFingerprintsWire {
    evolved_tectonic_fingerprint: [u8; 32],
    geologic_substrate_fingerprint: [u8; 32],
    primary_relief_fingerprint: [u8; 32],
    climate_work_domain_fingerprint: [u8; 32],
    climate_spec_fingerprint: [u8; 32],
    initial_climate_checkpoint_fingerprint: [u8; 32],
    formation_spec_fingerprint: [u8; 32],
}

impl SurfaceFormationUpstreamFingerprints {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        evolved_tectonic_fingerprint: [u8; 32],
        geologic_substrate_fingerprint: [u8; 32],
        primary_relief_fingerprint: [u8; 32],
        climate_work_domain_fingerprint: [u8; 32],
        climate_spec_fingerprint: [u8; 32],
        initial_climate_checkpoint_fingerprint: [u8; 32],
        formation_spec_fingerprint: [u8; 32],
    ) -> Result<Self, SurfaceFormationValidationError> {
        let fingerprints = Self {
            evolved_tectonic_fingerprint,
            geologic_substrate_fingerprint,
            primary_relief_fingerprint,
            climate_work_domain_fingerprint,
            climate_spec_fingerprint,
            initial_climate_checkpoint_fingerprint,
            formation_spec_fingerprint,
        };
        fingerprints.validate()?;
        Ok(fingerprints)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        for (field, value) in [
            (
                "evolved_tectonic_fingerprint",
                self.evolved_tectonic_fingerprint,
            ),
            (
                "geologic_substrate_fingerprint",
                self.geologic_substrate_fingerprint,
            ),
            (
                "primary_relief_fingerprint",
                self.primary_relief_fingerprint,
            ),
            (
                "climate_work_domain_fingerprint",
                self.climate_work_domain_fingerprint,
            ),
            ("climate_spec_fingerprint", self.climate_spec_fingerprint),
            (
                "initial_climate_checkpoint_fingerprint",
                self.initial_climate_checkpoint_fingerprint,
            ),
            (
                "formation_spec_fingerprint",
                self.formation_spec_fingerprint,
            ),
        ] {
            if value == [0; 32] {
                return Err(SurfaceFormationValidationError::ZeroFingerprint { field });
            }
        }
        Ok(())
    }

    fn update_hasher(&self, hasher: &mut blake3::Hasher) {
        hasher.update(&self.evolved_tectonic_fingerprint);
        hasher.update(&self.geologic_substrate_fingerprint);
        hasher.update(&self.primary_relief_fingerprint);
        hasher.update(&self.climate_work_domain_fingerprint);
        hasher.update(&self.climate_spec_fingerprint);
        hasher.update(&self.initial_climate_checkpoint_fingerprint);
        hasher.update(&self.formation_spec_fingerprint);
    }

    pub const fn evolved_tectonic_fingerprint(&self) -> &[u8; 32] {
        &self.evolved_tectonic_fingerprint
    }

    pub const fn geologic_substrate_fingerprint(&self) -> &[u8; 32] {
        &self.geologic_substrate_fingerprint
    }

    pub const fn primary_relief_fingerprint(&self) -> &[u8; 32] {
        &self.primary_relief_fingerprint
    }

    pub const fn climate_work_domain_fingerprint(&self) -> &[u8; 32] {
        &self.climate_work_domain_fingerprint
    }

    pub const fn climate_spec_fingerprint(&self) -> &[u8; 32] {
        &self.climate_spec_fingerprint
    }

    pub const fn initial_climate_checkpoint_fingerprint(&self) -> &[u8; 32] {
        &self.initial_climate_checkpoint_fingerprint
    }

    pub const fn formation_spec_fingerprint(&self) -> &[u8; 32] {
        &self.formation_spec_fingerprint
    }
}

impl<'de> Deserialize<'de> for SurfaceFormationUpstreamFingerprints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SurfaceFormationUpstreamFingerprintsWire::deserialize(deserializer)?;
        Self::new(
            wire.evolved_tectonic_fingerprint,
            wire.geologic_substrate_fingerprint,
            wire.primary_relief_fingerprint,
            wire.climate_work_domain_fingerprint,
            wire.climate_spec_fingerprint,
            wire.initial_climate_checkpoint_fingerprint,
            wire.formation_spec_fingerprint,
        )
        .map_err(D::Error::custom)
    }
}

/// Strict resume identity for one bounded outer P5 solve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceFormationCheckpoint {
    schema_version: u16,
    surface_ref: SurfaceRef,
    quality_profile: NaturalQualityProfile,
    model: SurfaceFormationModelId,
    model_fingerprint: [u8; 32],
    upstream: SurfaceFormationUpstreamFingerprints,
    outer_iterations: u8,
    state_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceFormationCheckpointWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    quality_profile: NaturalQualityProfile,
    model: SurfaceFormationModelId,
    model_fingerprint: [u8; 32],
    upstream: SurfaceFormationUpstreamFingerprints,
    outer_iterations: u8,
    state_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
}

impl SurfaceFormationCheckpoint {
    pub fn new(
        surface_ref: SurfaceRef,
        quality_profile: NaturalQualityProfile,
        upstream: SurfaceFormationUpstreamFingerprints,
        outer_iterations: u8,
        state_fingerprint: [u8; 32],
    ) -> Result<Self, SurfaceFormationValidationError> {
        let mut checkpoint = Self {
            schema_version: SURFACE_FORMATION_CHECKPOINT_SCHEMA_V1,
            surface_ref,
            quality_profile,
            model: SurfaceFormationModelId::PriorityFloodFastscapeSedimentHillslopeCoastIsostasyV1,
            model_fingerprint: surface_formation_model_fingerprint(),
            upstream,
            outer_iterations,
            state_fingerprint,
            fingerprint: [0; 32],
        };
        checkpoint.validate_identity()?;
        checkpoint.fingerprint = checkpoint.canonical_fingerprint();
        Ok(checkpoint)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        self.validate_identity()?;
        if self.fingerprint != self.canonical_fingerprint() {
            return Err(SurfaceFormationValidationError::CheckpointFingerprintMismatch);
        }
        Ok(())
    }

    fn validate_identity(&self) -> Result<(), SurfaceFormationValidationError> {
        if self.schema_version != SURFACE_FORMATION_CHECKPOINT_SCHEMA_V1 {
            return Err(SurfaceFormationValidationError::UnsupportedSchema {
                object: "surface_formation_checkpoint",
                found: self.schema_version,
                supported: SURFACE_FORMATION_CHECKPOINT_SCHEMA_V1,
            });
        }
        self.surface_ref.validate().map_err(|error| {
            SurfaceFormationValidationError::InvalidNested {
                role: "surface_ref",
                reason: error.to_string(),
            }
        })?;
        if !self.surface_ref.geometry_kind().is_spherical() {
            return Err(SurfaceFormationValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        self.upstream.validate()?;
        if self.model
            != SurfaceFormationModelId::PriorityFloodFastscapeSedimentHillslopeCoastIsostasyV1
            || self.model_fingerprint != surface_formation_model_fingerprint()
        {
            return Err(SurfaceFormationValidationError::ModelIdentityMismatch);
        }
        if !(1..=SURFACE_FORMATION_MAX_OUTER_ITERATIONS).contains(&self.outer_iterations) {
            return Err(SurfaceFormationValidationError::InvalidOuterIterations {
                found: self.outer_iterations,
                maximum: SURFACE_FORMATION_MAX_OUTER_ITERATIONS,
            });
        }
        if self.state_fingerprint == [0; 32] {
            return Err(SurfaceFormationValidationError::ZeroFingerprint {
                field: "state_fingerprint",
            });
        }
        Ok(())
    }

    fn canonical_fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.surface-formation-checkpoint.v1\0");
        hasher.update(&self.schema_version.to_le_bytes());
        update_surface_ref_hash(&mut hasher, self.surface_ref);
        hasher.update(&[natural_quality_profile_tag(self.quality_profile)]);
        hasher.update(&[surface_formation_model_tag(self.model)]);
        hasher.update(&self.model_fingerprint);
        self.upstream.update_hasher(&mut hasher);
        hasher.update(&[self.outer_iterations]);
        hasher.update(&self.state_fingerprint);
        *hasher.finalize().as_bytes()
    }

    pub fn validate_against(
        &self,
        surface_ref: SurfaceRef,
        quality_profile: NaturalQualityProfile,
        upstream: &SurfaceFormationUpstreamFingerprints,
    ) -> Result<(), SurfaceFormationValidationError> {
        self.validate()?;
        if self.surface_ref != surface_ref {
            return Err(
                SurfaceFormationValidationError::CheckpointIdentityMismatch {
                    field: "surface_ref",
                },
            );
        }
        if self.quality_profile != quality_profile {
            return Err(
                SurfaceFormationValidationError::CheckpointIdentityMismatch {
                    field: "quality_profile",
                },
            );
        }
        if &self.upstream != upstream {
            return Err(
                SurfaceFormationValidationError::CheckpointIdentityMismatch { field: "upstream" },
            );
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    pub const fn quality_profile(&self) -> NaturalQualityProfile {
        self.quality_profile
    }

    pub const fn model(&self) -> SurfaceFormationModelId {
        self.model
    }

    pub const fn model_fingerprint(&self) -> &[u8; 32] {
        &self.model_fingerprint
    }

    pub const fn upstream(&self) -> &SurfaceFormationUpstreamFingerprints {
        &self.upstream
    }

    pub const fn outer_iterations(&self) -> u8 {
        self.outer_iterations
    }

    pub const fn state_fingerprint(&self) -> &[u8; 32] {
        &self.state_fingerprint
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

impl<'de> Deserialize<'de> for SurfaceFormationCheckpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SurfaceFormationCheckpointWire::deserialize(deserializer)?;
        if wire.schema_version != SURFACE_FORMATION_CHECKPOINT_SCHEMA_V1 {
            return Err(D::Error::custom(
                SurfaceFormationValidationError::UnsupportedSchema {
                    object: "surface_formation_checkpoint",
                    found: wire.schema_version,
                    supported: SURFACE_FORMATION_CHECKPOINT_SCHEMA_V1,
                },
            ));
        }
        let checkpoint = Self::new(
            wire.surface_ref,
            wire.quality_profile,
            wire.upstream,
            wire.outer_iterations,
            wire.state_fingerprint,
        )
        .map_err(D::Error::custom)?;
        if checkpoint.model != wire.model
            || checkpoint.model_fingerprint != wire.model_fingerprint
            || checkpoint.fingerprint != wire.fingerprint
        {
            return Err(D::Error::custom(
                SurfaceFormationValidationError::CheckpointFingerprintMismatch,
            ));
        }
        Ok(checkpoint)
    }
}

/// Stable availability states for P5 and its explicit P6 handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceFormationCapabilityAvailability {
    Available,
    EvaluatedNotApplicable,
    Unavailable,
}

/// Canonical complete P5 capability inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceFormationCapabilityId {
    PriorityFloodHydrologyV2,
    ImplicitStreamPowerV1,
    NonlinearHillslopeTransportV1,
    ProvenanceSedimentV1,
    CoastalIsostaticResponseV1,
    ExplicitEvapotranspirationV1,
    GroundwaterFlowV1,
    GlacialErosionV1,
}

const SURFACE_FORMATION_CAPABILITY_IDS: [SurfaceFormationCapabilityId; 8] = [
    SurfaceFormationCapabilityId::PriorityFloodHydrologyV2,
    SurfaceFormationCapabilityId::ImplicitStreamPowerV1,
    SurfaceFormationCapabilityId::NonlinearHillslopeTransportV1,
    SurfaceFormationCapabilityId::ProvenanceSedimentV1,
    SurfaceFormationCapabilityId::CoastalIsostaticResponseV1,
    SurfaceFormationCapabilityId::ExplicitEvapotranspirationV1,
    SurfaceFormationCapabilityId::GroundwaterFlowV1,
    SurfaceFormationCapabilityId::GlacialErosionV1,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceFormationCapabilityStatus {
    id: SurfaceFormationCapabilityId,
    availability: SurfaceFormationCapabilityAvailability,
}

/// Complete, canonical availability declaration for the P5 product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceFormationCapabilitySet {
    statuses: Vec<SurfaceFormationCapabilityStatus>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceFormationCapabilitySetWire {
    #[serde(deserialize_with = "deserialize_formation_capabilities")]
    statuses: Vec<SurfaceFormationCapabilityStatus>,
}

impl SurfaceFormationCapabilitySet {
    pub fn p5() -> Self {
        Self {
            statuses: SURFACE_FORMATION_CAPABILITY_IDS
                .into_iter()
                .map(|id| SurfaceFormationCapabilityStatus {
                    availability: expected_capability_availability(id),
                    id,
                })
                .collect(),
        }
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        if self.statuses.len() != SURFACE_FORMATION_CAPABILITY_IDS.len() {
            return Err(
                SurfaceFormationValidationError::CapabilityInventoryMismatch {
                    found: self.statuses.len(),
                    expected: SURFACE_FORMATION_CAPABILITY_IDS.len(),
                },
            );
        }
        for (index, (status, expected)) in self
            .statuses
            .iter()
            .zip(SURFACE_FORMATION_CAPABILITY_IDS)
            .enumerate()
        {
            if status.id != expected
                || status.availability != expected_capability_availability(expected)
            {
                return Err(SurfaceFormationValidationError::NonCanonicalCapability { index });
            }
        }
        Ok(())
    }

    pub fn availability(
        &self,
        id: SurfaceFormationCapabilityId,
    ) -> SurfaceFormationCapabilityAvailability {
        self.statuses
            .iter()
            .find(|status| status.id == id)
            .map(|status| status.availability)
            .unwrap_or(SurfaceFormationCapabilityAvailability::Unavailable)
    }
}

impl Default for SurfaceFormationCapabilitySet {
    fn default() -> Self {
        Self::p5()
    }
}

impl<'de> Deserialize<'de> for SurfaceFormationCapabilitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SurfaceFormationCapabilitySetWire::deserialize(deserializer)?;
        let set = Self {
            statuses: wire.statuses,
        };
        set.validate().map_err(D::Error::custom)?;
        Ok(set)
    }
}

const fn expected_capability_availability(
    id: SurfaceFormationCapabilityId,
) -> SurfaceFormationCapabilityAvailability {
    match id {
        SurfaceFormationCapabilityId::PriorityFloodHydrologyV2
        | SurfaceFormationCapabilityId::ImplicitStreamPowerV1
        | SurfaceFormationCapabilityId::NonlinearHillslopeTransportV1
        | SurfaceFormationCapabilityId::ProvenanceSedimentV1
        | SurfaceFormationCapabilityId::CoastalIsostaticResponseV1 => {
            SurfaceFormationCapabilityAvailability::Available
        }
        SurfaceFormationCapabilityId::ExplicitEvapotranspirationV1
        | SurfaceFormationCapabilityId::GroundwaterFlowV1
        | SurfaceFormationCapabilityId::GlacialErosionV1 => {
            SurfaceFormationCapabilityAvailability::Unavailable
        }
    }
}

/// Returns the exact retained P5 elevation identity in its declared order.
#[allow(clippy::too_many_arguments)]
pub fn formation_elevation_from_components(
    primary_elevation_m: f32,
    tectonic_displacement_m: f32,
    fluvial_erosion_m: f32,
    hillslope_erosion_m: f32,
    hillslope_deposition_m: f32,
    routed_sediment_deposition_m: f32,
    coastal_erosion_m: f32,
    coastal_deposition_m: f32,
    isostatic_response_m: f32,
) -> f32 {
    (f64::from(primary_elevation_m) + f64::from(tectonic_displacement_m)
        - f64::from(fluvial_erosion_m)
        - f64::from(hillslope_erosion_m)
        + f64::from(hillslope_deposition_m)
        + f64::from(routed_sediment_deposition_m)
        - f64::from(coastal_erosion_m)
        + f64::from(coastal_deposition_m)
        + f64::from(isostatic_response_m)) as f32
}

/// Retained causal elevation components, aligned to authoritative cells.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormationElevationComponents {
    primary_elevation_m: Vec<f32>,
    tectonic_displacement_m: Vec<f32>,
    fluvial_erosion_m: Vec<f32>,
    hillslope_erosion_m: Vec<f32>,
    hillslope_deposition_m: Vec<f32>,
    routed_sediment_deposition_m: Vec<f32>,
    coastal_erosion_m: Vec<f32>,
    coastal_deposition_m: Vec<f32>,
    isostatic_response_m: Vec<f32>,
    final_elevation_m: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationElevationComponentsWire {
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    primary_elevation_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    tectonic_displacement_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    fluvial_erosion_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    hillslope_erosion_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    hillslope_deposition_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    routed_sediment_deposition_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    coastal_erosion_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    coastal_deposition_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    isostatic_response_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    final_elevation_m: Vec<f32>,
}

impl FormationElevationComponents {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        primary_elevation_m: Vec<f32>,
        tectonic_displacement_m: Vec<f32>,
        fluvial_erosion_m: Vec<f32>,
        hillslope_erosion_m: Vec<f32>,
        hillslope_deposition_m: Vec<f32>,
        routed_sediment_deposition_m: Vec<f32>,
        coastal_erosion_m: Vec<f32>,
        coastal_deposition_m: Vec<f32>,
        isostatic_response_m: Vec<f32>,
        final_elevation_m: Vec<f32>,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let components = Self {
            primary_elevation_m,
            tectonic_displacement_m,
            fluvial_erosion_m,
            hillslope_erosion_m,
            hillslope_deposition_m,
            routed_sediment_deposition_m,
            coastal_erosion_m,
            coastal_deposition_m,
            isostatic_response_m,
            final_elevation_m,
        };
        components.validate()?;
        Ok(components)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        let count = self.primary_elevation_m.len();
        validate_dense_count(count)?;
        for (field, found) in [
            (
                "tectonic_displacement_m",
                self.tectonic_displacement_m.len(),
            ),
            ("fluvial_erosion_m", self.fluvial_erosion_m.len()),
            ("hillslope_erosion_m", self.hillslope_erosion_m.len()),
            ("hillslope_deposition_m", self.hillslope_deposition_m.len()),
            (
                "routed_sediment_deposition_m",
                self.routed_sediment_deposition_m.len(),
            ),
            ("coastal_erosion_m", self.coastal_erosion_m.len()),
            ("coastal_deposition_m", self.coastal_deposition_m.len()),
            ("isostatic_response_m", self.isostatic_response_m.len()),
            ("final_elevation_m", self.final_elevation_m.len()),
        ] {
            validate_field_length(field, found, count)?;
        }
        validate_f32_slice(
            "primary_elevation_m",
            &self.primary_elevation_m,
            ELEVATION_MIN_M,
            ELEVATION_MAX_M,
        )?;
        validate_f32_slice(
            "tectonic_displacement_m",
            &self.tectonic_displacement_m,
            -MAX_COMPONENT_ABS_M,
            MAX_COMPONENT_ABS_M,
        )?;
        for (field, values) in [
            ("fluvial_erosion_m", self.fluvial_erosion_m.as_slice()),
            ("hillslope_erosion_m", self.hillslope_erosion_m.as_slice()),
            (
                "hillslope_deposition_m",
                self.hillslope_deposition_m.as_slice(),
            ),
            (
                "routed_sediment_deposition_m",
                self.routed_sediment_deposition_m.as_slice(),
            ),
            ("coastal_erosion_m", self.coastal_erosion_m.as_slice()),
            ("coastal_deposition_m", self.coastal_deposition_m.as_slice()),
        ] {
            validate_f32_slice(field, values, 0.0, MAX_COMPONENT_ABS_M)?;
        }
        validate_f32_slice(
            "isostatic_response_m",
            &self.isostatic_response_m,
            -MAX_COMPONENT_ABS_M,
            MAX_COMPONENT_ABS_M,
        )?;
        validate_f32_slice(
            "final_elevation_m",
            &self.final_elevation_m,
            ELEVATION_MIN_M,
            ELEVATION_MAX_M,
        )?;
        for index in 0..count {
            let expected = formation_elevation_from_components(
                self.primary_elevation_m[index],
                self.tectonic_displacement_m[index],
                self.fluvial_erosion_m[index],
                self.hillslope_erosion_m[index],
                self.hillslope_deposition_m[index],
                self.routed_sediment_deposition_m[index],
                self.coastal_erosion_m[index],
                self.coastal_deposition_m[index],
                self.isostatic_response_m[index],
            );
            if self.final_elevation_m[index].to_bits() != expected.to_bits() {
                return Err(SurfaceFormationValidationError::ComponentIdentityMismatch {
                    cell: index,
                    stored: self.final_elevation_m[index],
                    expected,
                });
            }
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.primary_elevation_m.len()
    }

    pub fn is_empty(&self) -> bool {
        self.primary_elevation_m.is_empty()
    }

    pub fn primary_elevation_m(&self) -> &[f32] {
        &self.primary_elevation_m
    }

    pub fn tectonic_displacement_m(&self) -> &[f32] {
        &self.tectonic_displacement_m
    }

    pub fn fluvial_erosion_m(&self) -> &[f32] {
        &self.fluvial_erosion_m
    }

    pub fn hillslope_erosion_m(&self) -> &[f32] {
        &self.hillslope_erosion_m
    }

    pub fn hillslope_deposition_m(&self) -> &[f32] {
        &self.hillslope_deposition_m
    }

    pub fn routed_sediment_deposition_m(&self) -> &[f32] {
        &self.routed_sediment_deposition_m
    }

    pub fn coastal_erosion_m(&self) -> &[f32] {
        &self.coastal_erosion_m
    }

    pub fn coastal_deposition_m(&self) -> &[f32] {
        &self.coastal_deposition_m
    }

    pub fn isostatic_response_m(&self) -> &[f32] {
        &self.isostatic_response_m
    }

    pub fn final_elevation_m(&self) -> &[f32] {
        &self.final_elevation_m
    }
}

impl<'de> Deserialize<'de> for FormationElevationComponents {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FormationElevationComponentsWire::deserialize(deserializer)?;
        Self::new(
            wire.primary_elevation_m,
            wire.tectonic_displacement_m,
            wire.fluvial_erosion_m,
            wire.hillslope_erosion_m,
            wire.hillslope_deposition_m,
            wire.routed_sediment_deposition_m,
            wire.coastal_erosion_m,
            wire.coastal_deposition_m,
            wire.isostatic_response_m,
            wire.final_elevation_m,
        )
        .map_err(D::Error::custom)
    }
}

/// Retained sediment state and routed-destination diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormationSedimentFields {
    sediment_thickness_m: Vec<f32>,
    provenance_fraction: Vec<[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    sediment_throughput_kg: Vec<f64>,
    shelf_delivery_kg: Vec<f64>,
    deep_ocean_delivery_kg: Vec<f64>,
    endorheic_storage_kg: Vec<f64>,
    delta_potential: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationSedimentFieldsWire {
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    sediment_thickness_m: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_provenance_values")]
    provenance_fraction: Vec<[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    #[serde(deserialize_with = "deserialize_formation_f64_values")]
    sediment_throughput_kg: Vec<f64>,
    #[serde(deserialize_with = "deserialize_formation_f64_values")]
    shelf_delivery_kg: Vec<f64>,
    #[serde(deserialize_with = "deserialize_formation_f64_values")]
    deep_ocean_delivery_kg: Vec<f64>,
    #[serde(deserialize_with = "deserialize_formation_f64_values")]
    endorheic_storage_kg: Vec<f64>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    delta_potential: Vec<f32>,
}

impl FormationSedimentFields {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sediment_thickness_m: Vec<f32>,
        provenance_fraction: Vec<[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
        sediment_throughput_kg: Vec<f64>,
        shelf_delivery_kg: Vec<f64>,
        deep_ocean_delivery_kg: Vec<f64>,
        endorheic_storage_kg: Vec<f64>,
        delta_potential: Vec<f32>,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let fields = Self {
            sediment_thickness_m,
            provenance_fraction,
            sediment_throughput_kg,
            shelf_delivery_kg,
            deep_ocean_delivery_kg,
            endorheic_storage_kg,
            delta_potential,
        };
        fields.validate()?;
        Ok(fields)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        let count = self.sediment_thickness_m.len();
        validate_dense_count(count)?;
        for (field, found) in [
            ("provenance_fraction", self.provenance_fraction.len()),
            ("sediment_throughput_kg", self.sediment_throughput_kg.len()),
            ("shelf_delivery_kg", self.shelf_delivery_kg.len()),
            ("deep_ocean_delivery_kg", self.deep_ocean_delivery_kg.len()),
            ("endorheic_storage_kg", self.endorheic_storage_kg.len()),
            ("delta_potential", self.delta_potential.len()),
        ] {
            validate_field_length(field, found, count)?;
        }
        validate_f32_slice(
            "sediment_thickness_m",
            &self.sediment_thickness_m,
            0.0,
            MAX_SEDIMENT_THICKNESS_M,
        )?;
        validate_f64_slice(
            "sediment_throughput_kg",
            &self.sediment_throughput_kg,
            0.0,
            f64::MAX,
        )?;
        validate_f64_slice("shelf_delivery_kg", &self.shelf_delivery_kg, 0.0, f64::MAX)?;
        validate_f64_slice(
            "deep_ocean_delivery_kg",
            &self.deep_ocean_delivery_kg,
            0.0,
            f64::MAX,
        )?;
        validate_f64_slice(
            "endorheic_storage_kg",
            &self.endorheic_storage_kg,
            0.0,
            f64::MAX,
        )?;
        validate_f32_slice("delta_potential", &self.delta_potential, 0.0, 1.0)?;
        for (cell, (fractions, &thickness)) in self
            .provenance_fraction
            .iter()
            .zip(&self.sediment_thickness_m)
            .enumerate()
        {
            let mut sum = 0.0_f64;
            for &value in fractions {
                if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                    return Err(SurfaceFormationValidationError::InvalidCellValue {
                        field: "provenance_fraction",
                        cell,
                        found: f64::from(value),
                    });
                }
                sum += f64::from(value);
            }
            let expected = if thickness == 0.0 { 0.0 } else { 1.0 };
            if (sum - expected).abs() > PROVENANCE_SUM_TOLERANCE {
                return Err(SurfaceFormationValidationError::ProvenanceSumMismatch {
                    cell,
                    thickness_m: thickness,
                    found: sum,
                    expected,
                });
            }
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.sediment_thickness_m.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sediment_thickness_m.is_empty()
    }

    pub fn sediment_thickness_m(&self) -> &[f32] {
        &self.sediment_thickness_m
    }

    pub fn provenance_fraction(&self) -> &[[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]] {
        &self.provenance_fraction
    }

    pub fn sediment_throughput_kg(&self) -> &[f64] {
        &self.sediment_throughput_kg
    }

    pub fn shelf_delivery_kg(&self) -> &[f64] {
        &self.shelf_delivery_kg
    }

    pub fn deep_ocean_delivery_kg(&self) -> &[f64] {
        &self.deep_ocean_delivery_kg
    }

    pub fn endorheic_storage_kg(&self) -> &[f64] {
        &self.endorheic_storage_kg
    }

    pub fn delta_potential(&self) -> &[f32] {
        &self.delta_potential
    }

    pub fn dominant_source(&self, cell: usize) -> Option<SedimentSourceKind> {
        if self.sediment_thickness_m.get(cell).copied()? == 0.0 {
            return None;
        }
        let fractions = self.provenance_fraction.get(cell)?;
        let (index, _) =
            fractions
                .iter()
                .enumerate()
                .max_by(|(left_index, left), (right_index, right)| {
                    left.total_cmp(right)
                        .then_with(|| right_index.cmp(left_index))
                })?;
        SedimentSourceKind::try_from_raw(index as u32).ok()
    }
}

impl<'de> Deserialize<'de> for FormationSedimentFields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FormationSedimentFieldsWire::deserialize(deserializer)?;
        Self::new(
            wire.sediment_thickness_m,
            wire.provenance_fraction,
            wire.sediment_throughput_kg,
            wire.shelf_delivery_kg,
            wire.deep_ocean_delivery_kg,
            wire.endorheic_storage_kg,
            wire.delta_potential,
        )
        .map_err(D::Error::custom)
    }
}

/// Complete retained P5 terrain, water, and sediment fields.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormationTerrainFields {
    schema_version: u16,
    elevation_components: FormationElevationComponents,
    sea_level_m: f32,
    water_inventory_m3: f64,
    realized_water_volume_m3: f64,
    land_ocean: LandOceanField,
    sediment: FormationSedimentFields,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationTerrainFieldsWire {
    schema_version: u16,
    elevation_components: FormationElevationComponents,
    sea_level_m: f32,
    water_inventory_m3: f64,
    realized_water_volume_m3: f64,
    #[serde(deserialize_with = "deserialize_formation_land_ocean")]
    land_ocean: LandOceanField,
    sediment: FormationSedimentFields,
}

impl FormationTerrainFields {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        elevation_components: FormationElevationComponents,
        sea_level_m: f32,
        water_inventory_m3: f64,
        realized_water_volume_m3: f64,
        land_ocean: LandOceanField,
        sediment: FormationSedimentFields,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let fields = Self {
            schema_version,
            elevation_components,
            sea_level_m,
            water_inventory_m3,
            realized_water_volume_m3,
            land_ocean,
            sediment,
        };
        fields.validate()?;
        Ok(fields)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        if self.schema_version != FORMATION_TERRAIN_FIELDS_SCHEMA_V1 {
            return Err(SurfaceFormationValidationError::UnsupportedSchema {
                object: "formation_terrain_fields",
                found: self.schema_version,
                supported: FORMATION_TERRAIN_FIELDS_SCHEMA_V1,
            });
        }
        self.elevation_components.validate()?;
        self.sediment.validate()?;
        let count = self.elevation_components.len();
        validate_field_length("land_ocean", self.land_ocean.len(), count)?;
        validate_field_length("sediment", self.sediment.len(), count)?;
        if !self.sea_level_m.is_finite()
            || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&self.sea_level_m)
        {
            return Err(SurfaceFormationValidationError::InvalidValue {
                field: "sea_level_m",
                found: f64::from(self.sea_level_m),
            });
        }
        for (field, value) in [
            ("water_inventory_m3", self.water_inventory_m3),
            ("realized_water_volume_m3", self.realized_water_volume_m3),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(SurfaceFormationValidationError::InvalidValue {
                    field,
                    found: value,
                });
            }
        }
        let water_error = relative_error(self.realized_water_volume_m3, self.water_inventory_m3);
        if water_error > WATER_VOLUME_RELATIVE_TOLERANCE {
            return Err(SurfaceFormationValidationError::WaterVolumeMismatch {
                stored: self.realized_water_volume_m3,
                expected: self.water_inventory_m3,
                relative_error: water_error,
            });
        }
        for (cell, &elevation) in self
            .elevation_components
            .final_elevation_m()
            .iter()
            .enumerate()
        {
            let expected = LandOceanKind::classify(elevation, self.sea_level_m);
            let found = self.land_ocean.get(cell).ok_or(
                SurfaceFormationValidationError::InvalidCellValue {
                    field: "land_ocean",
                    cell,
                    found: f64::NAN,
                },
            )?;
            if found != expected {
                return Err(SurfaceFormationValidationError::LandOceanMismatch {
                    cell,
                    found,
                    expected,
                });
            }
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.formation-terrain-fields.v1\0");
        hasher.update(&self.schema_version.to_le_bytes());
        for values in [
            self.elevation_components.primary_elevation_m(),
            self.elevation_components.tectonic_displacement_m(),
            self.elevation_components.fluvial_erosion_m(),
            self.elevation_components.hillslope_erosion_m(),
            self.elevation_components.hillslope_deposition_m(),
            self.elevation_components.routed_sediment_deposition_m(),
            self.elevation_components.coastal_erosion_m(),
            self.elevation_components.coastal_deposition_m(),
            self.elevation_components.isostatic_response_m(),
            self.elevation_components.final_elevation_m(),
            self.sediment.sediment_thickness_m(),
            self.sediment.delta_potential(),
        ] {
            update_f32_slice_hash(&mut hasher, values);
        }
        hasher.update(&self.sea_level_m.to_bits().to_le_bytes());
        hasher.update(&self.water_inventory_m3.to_bits().to_le_bytes());
        hasher.update(&self.realized_water_volume_m3.to_bits().to_le_bytes());
        for &value in self.land_ocean.raw_values() {
            hasher.update(&value.to_le_bytes());
        }
        for fractions in self.sediment.provenance_fraction() {
            update_f32_slice_hash(&mut hasher, fractions);
        }
        for values in [
            self.sediment.sediment_throughput_kg(),
            self.sediment.shelf_delivery_kg(),
            self.sediment.deep_ocean_delivery_kg(),
            self.sediment.endorheic_storage_kg(),
        ] {
            update_f64_slice_hash(&mut hasher, values);
        }
        *hasher.finalize().as_bytes()
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn elevation_components(&self) -> &FormationElevationComponents {
        &self.elevation_components
    }

    pub fn final_elevation_m(&self) -> &[f32] {
        self.elevation_components.final_elevation_m()
    }

    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }

    pub const fn water_inventory_m3(&self) -> f64 {
        self.water_inventory_m3
    }

    pub const fn realized_water_volume_m3(&self) -> f64 {
        self.realized_water_volume_m3
    }

    pub const fn land_ocean(&self) -> &LandOceanField {
        &self.land_ocean
    }

    pub const fn sediment(&self) -> &FormationSedimentFields {
        &self.sediment
    }
}

impl<'de> Deserialize<'de> for FormationTerrainFields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FormationTerrainFieldsWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.elevation_components,
            wire.sea_level_m,
            wire.water_inventory_m3,
            wire.realized_water_volume_m3,
            wire.land_ocean,
            wire.sediment,
        )
        .map_err(D::Error::custom)
    }
}

/// One complete outer-iteration fixed-point residual vector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormationResiduals {
    elevation_rms_m: f64,
    receiver_changed_fraction: f64,
    log_discharge_rms: f64,
    sediment_thickness_rms_m: f64,
    coastline_area_changed_fraction: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationResidualsWire {
    elevation_rms_m: f64,
    receiver_changed_fraction: f64,
    log_discharge_rms: f64,
    sediment_thickness_rms_m: f64,
    coastline_area_changed_fraction: f64,
}

impl FormationResiduals {
    pub fn new(
        elevation_rms_m: f64,
        receiver_changed_fraction: f64,
        log_discharge_rms: f64,
        sediment_thickness_rms_m: f64,
        coastline_area_changed_fraction: f64,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let residuals = Self {
            elevation_rms_m,
            receiver_changed_fraction,
            log_discharge_rms,
            sediment_thickness_rms_m,
            coastline_area_changed_fraction,
        };
        residuals.validate()?;
        Ok(residuals)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        for (field, value) in [
            ("elevation_rms_m", self.elevation_rms_m),
            ("receiver_changed_fraction", self.receiver_changed_fraction),
            ("log_discharge_rms", self.log_discharge_rms),
            ("sediment_thickness_rms_m", self.sediment_thickness_rms_m),
            (
                "coastline_area_changed_fraction",
                self.coastline_area_changed_fraction,
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(SurfaceFormationValidationError::InvalidValue {
                    field,
                    found: value,
                });
            }
        }
        for (field, value) in [
            ("receiver_changed_fraction", self.receiver_changed_fraction),
            (
                "coastline_area_changed_fraction",
                self.coastline_area_changed_fraction,
            ),
        ] {
            if value > 1.0 {
                return Err(SurfaceFormationValidationError::InvalidValue {
                    field,
                    found: value,
                });
            }
        }
        Ok(())
    }

    pub fn normalized_max(&self) -> f64 {
        (self.elevation_rms_m / FORMATION_ELEVATION_RESIDUAL_SCALE_M)
            .max(self.receiver_changed_fraction / FORMATION_RECEIVER_RESIDUAL_SCALE)
            .max(self.log_discharge_rms / FORMATION_LOG_DISCHARGE_RESIDUAL_SCALE)
            .max(self.sediment_thickness_rms_m / FORMATION_SEDIMENT_RESIDUAL_SCALE_M)
            .max(self.coastline_area_changed_fraction / FORMATION_COASTLINE_RESIDUAL_SCALE)
    }

    pub const fn elevation_rms_m(&self) -> f64 {
        self.elevation_rms_m
    }

    pub const fn receiver_changed_fraction(&self) -> f64 {
        self.receiver_changed_fraction
    }

    pub const fn log_discharge_rms(&self) -> f64 {
        self.log_discharge_rms
    }

    pub const fn sediment_thickness_rms_m(&self) -> f64 {
        self.sediment_thickness_rms_m
    }

    pub const fn coastline_area_changed_fraction(&self) -> f64 {
        self.coastline_area_changed_fraction
    }
}

impl<'de> Deserialize<'de> for FormationResiduals {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FormationResidualsWire::deserialize(deserializer)?;
        Self::new(
            wire.elevation_rms_m,
            wire.receiver_changed_fraction,
            wire.log_discharge_rms,
            wire.sediment_thickness_rms_m,
            wire.coastline_area_changed_fraction,
        )
        .map_err(D::Error::custom)
    }
}

/// Bounded work and fixed-point convergence evidence for P5.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormationSolveReport {
    outer_iterations: u8,
    geomorphic_macro_steps: u16,
    residuals: Vec<FormationResiduals>,
    converged: bool,
    dense_state_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationSolveReportWire {
    outer_iterations: u8,
    geomorphic_macro_steps: u16,
    #[serde(deserialize_with = "deserialize_formation_residuals")]
    residuals: Vec<FormationResiduals>,
    converged: bool,
    dense_state_bytes: u64,
}

impl FormationSolveReport {
    pub fn new(
        residuals: Vec<FormationResiduals>,
        dense_state_bytes: u64,
    ) -> Result<Self, SurfaceFormationValidationError> {
        if residuals.is_empty() || residuals.len() > SURFACE_FORMATION_MAX_OUTER_ITERATIONS as usize
        {
            return Err(SurfaceFormationValidationError::InvalidOuterIterations {
                found: residuals.len().min(u8::MAX as usize) as u8,
                maximum: SURFACE_FORMATION_MAX_OUTER_ITERATIONS,
            });
        }
        let outer_iterations = residuals.len() as u8;
        let geomorphic_macro_steps = u16::from(outer_iterations) * SURFACE_FORMATION_MACRO_STEPS;
        let converged = residuals
            .last()
            .is_some_and(|residual| residual.normalized_max() <= 1.0);
        let report = Self {
            outer_iterations,
            geomorphic_macro_steps,
            residuals,
            converged,
            dense_state_bytes,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        if self.residuals.len() != self.outer_iterations as usize
            || !(1..=SURFACE_FORMATION_MAX_OUTER_ITERATIONS).contains(&self.outer_iterations)
        {
            return Err(SurfaceFormationValidationError::InvalidOuterIterations {
                found: self.outer_iterations,
                maximum: SURFACE_FORMATION_MAX_OUTER_ITERATIONS,
            });
        }
        let expected_steps = u16::from(self.outer_iterations) * SURFACE_FORMATION_MACRO_STEPS;
        if self.geomorphic_macro_steps != expected_steps {
            return Err(SurfaceFormationValidationError::SolveWorkMismatch {
                field: "geomorphic_macro_steps",
            });
        }
        for residual in &self.residuals {
            residual.validate()?;
        }
        let expected_converged = self
            .residuals
            .last()
            .is_some_and(|residual| residual.normalized_max() <= 1.0);
        if self.converged != expected_converged || !self.converged {
            return Err(SurfaceFormationValidationError::FormationNotConverged {
                normalized_residual: self
                    .residuals
                    .last()
                    .map_or(f64::INFINITY, FormationResiduals::normalized_max),
            });
        }
        if self.dense_state_bytes == 0
            || self.dense_state_bytes > SURFACE_FORMATION_DENSE_STATE_BYTES_MAX
        {
            return Err(SurfaceFormationValidationError::InvalidDenseStateBytes {
                found: self.dense_state_bytes,
                maximum: SURFACE_FORMATION_DENSE_STATE_BYTES_MAX,
            });
        }
        Ok(())
    }

    pub const fn outer_iterations(&self) -> u8 {
        self.outer_iterations
    }

    pub const fn geomorphic_macro_steps(&self) -> u16 {
        self.geomorphic_macro_steps
    }

    pub fn residuals(&self) -> &[FormationResiduals] {
        &self.residuals
    }

    pub fn final_residual(&self) -> &FormationResiduals {
        self.residuals
            .last()
            .expect("validated solve report has at least one residual")
    }

    pub const fn converged(&self) -> bool {
        self.converged
    }

    pub const fn dense_state_bytes(&self) -> u64 {
        self.dense_state_bytes
    }
}

impl<'de> Deserialize<'de> for FormationSolveReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FormationSolveReportWire::deserialize(deserializer)?;
        let report = Self::new(wire.residuals, wire.dense_state_bytes).map_err(D::Error::custom)?;
        if report.outer_iterations != wire.outer_iterations
            || report.geomorphic_macro_steps != wire.geomorphic_macro_steps
            || report.converged != wire.converged
        {
            return Err(D::Error::custom(
                SurfaceFormationValidationError::SolveWorkMismatch {
                    field: "derived_report_fields",
                },
            ));
        }
        Ok(report)
    }
}

/// Retained global and five-source sediment mass closure.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SedimentBudgetReport {
    produced_mass_kg: f64,
    land_lake_deposited_mass_kg: f64,
    shelf_deposited_mass_kg: f64,
    deep_ocean_delivery_mass_kg: f64,
    final_in_transit_mass_kg: f64,
    produced_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    accounted_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    global_relative_error: f64,
    provenance_relative_errors: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SedimentBudgetReportWire {
    produced_mass_kg: f64,
    land_lake_deposited_mass_kg: f64,
    shelf_deposited_mass_kg: f64,
    deep_ocean_delivery_mass_kg: f64,
    final_in_transit_mass_kg: f64,
    produced_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    accounted_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    global_relative_error: f64,
    provenance_relative_errors: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
}

impl SedimentBudgetReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        produced_mass_kg: f64,
        land_lake_deposited_mass_kg: f64,
        shelf_deposited_mass_kg: f64,
        deep_ocean_delivery_mass_kg: f64,
        final_in_transit_mass_kg: f64,
        produced_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
        accounted_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    ) -> Result<Self, SurfaceFormationValidationError> {
        let accounted = land_lake_deposited_mass_kg
            + shelf_deposited_mass_kg
            + deep_ocean_delivery_mass_kg
            + final_in_transit_mass_kg;
        let global_relative_error = relative_error(produced_mass_kg, accounted);
        let provenance_relative_errors = std::array::from_fn(|index| {
            relative_error(produced_by_source_kg[index], accounted_by_source_kg[index])
        });
        let report = Self {
            produced_mass_kg,
            land_lake_deposited_mass_kg,
            shelf_deposited_mass_kg,
            deep_ocean_delivery_mass_kg,
            final_in_transit_mass_kg,
            produced_by_source_kg,
            accounted_by_source_kg,
            global_relative_error,
            provenance_relative_errors,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        for (field, value) in [
            ("produced_mass_kg", self.produced_mass_kg),
            (
                "land_lake_deposited_mass_kg",
                self.land_lake_deposited_mass_kg,
            ),
            ("shelf_deposited_mass_kg", self.shelf_deposited_mass_kg),
            (
                "deep_ocean_delivery_mass_kg",
                self.deep_ocean_delivery_mass_kg,
            ),
            ("final_in_transit_mass_kg", self.final_in_transit_mass_kg),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(SurfaceFormationValidationError::InvalidValue {
                    field,
                    found: value,
                });
            }
        }
        for (field, values) in [
            ("produced_by_source_kg", self.produced_by_source_kg),
            ("accounted_by_source_kg", self.accounted_by_source_kg),
        ] {
            for value in values {
                if !value.is_finite() || value < 0.0 {
                    return Err(SurfaceFormationValidationError::InvalidValue {
                        field,
                        found: value,
                    });
                }
            }
        }
        let accounted = self.accounted_mass_kg();
        let expected_global = relative_error(self.produced_mass_kg, accounted);
        if self.global_relative_error.to_bits() != expected_global.to_bits()
            || expected_global > SEDIMENT_BUDGET_RELATIVE_ERROR_MAX
        {
            return Err(SurfaceFormationValidationError::SedimentBudgetNotClosed {
                field: "global",
                relative_error: expected_global,
                maximum: SEDIMENT_BUDGET_RELATIVE_ERROR_MAX,
            });
        }
        let produced_source_sum = self.produced_by_source_kg.iter().sum::<f64>();
        let accounted_source_sum = self.accounted_by_source_kg.iter().sum::<f64>();
        for (field, left, right) in [
            (
                "produced_source_total",
                produced_source_sum,
                self.produced_mass_kg,
            ),
            ("accounted_source_total", accounted_source_sum, accounted),
        ] {
            let error = relative_error(left, right);
            if error > SEDIMENT_BUDGET_RELATIVE_ERROR_MAX {
                return Err(SurfaceFormationValidationError::SedimentBudgetNotClosed {
                    field,
                    relative_error: error,
                    maximum: SEDIMENT_BUDGET_RELATIVE_ERROR_MAX,
                });
            }
        }
        for index in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
            let expected = relative_error(
                self.produced_by_source_kg[index],
                self.accounted_by_source_kg[index],
            );
            if self.provenance_relative_errors[index].to_bits() != expected.to_bits()
                || expected > SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX
            {
                return Err(
                    SurfaceFormationValidationError::SedimentProvenanceNotClosed {
                        source_index: index,
                        relative_error: expected,
                        maximum: SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX,
                    },
                );
            }
        }
        Ok(())
    }

    pub const fn produced_mass_kg(&self) -> f64 {
        self.produced_mass_kg
    }

    pub fn accounted_mass_kg(&self) -> f64 {
        self.land_lake_deposited_mass_kg
            + self.shelf_deposited_mass_kg
            + self.deep_ocean_delivery_mass_kg
            + self.final_in_transit_mass_kg
    }

    pub const fn land_lake_deposited_mass_kg(&self) -> f64 {
        self.land_lake_deposited_mass_kg
    }

    pub const fn shelf_deposited_mass_kg(&self) -> f64 {
        self.shelf_deposited_mass_kg
    }

    pub const fn deep_ocean_delivery_mass_kg(&self) -> f64 {
        self.deep_ocean_delivery_mass_kg
    }

    pub const fn final_in_transit_mass_kg(&self) -> f64 {
        self.final_in_transit_mass_kg
    }

    pub const fn produced_by_source_kg(&self) -> &[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT] {
        &self.produced_by_source_kg
    }

    pub const fn accounted_by_source_kg(&self) -> &[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT] {
        &self.accounted_by_source_kg
    }

    pub const fn global_relative_error(&self) -> f64 {
        self.global_relative_error
    }

    pub const fn provenance_relative_errors(&self) -> &[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT] {
        &self.provenance_relative_errors
    }
}

impl<'de> Deserialize<'de> for SedimentBudgetReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SedimentBudgetReportWire::deserialize(deserializer)?;
        let report = Self::new(
            wire.produced_mass_kg,
            wire.land_lake_deposited_mass_kg,
            wire.shelf_deposited_mass_kg,
            wire.deep_ocean_delivery_mass_kg,
            wire.final_in_transit_mass_kg,
            wire.produced_by_source_kg,
            wire.accounted_by_source_kg,
        )
        .map_err(D::Error::custom)?;
        if report.global_relative_error.to_bits() != wire.global_relative_error.to_bits()
            || report.provenance_relative_errors.map(f64::to_bits)
                != wire.provenance_relative_errors.map(f64::to_bits)
        {
            return Err(D::Error::custom(
                SurfaceFormationValidationError::SolveWorkMismatch {
                    field: "derived_sediment_budget_errors",
                },
            ));
        }
        Ok(report)
    }
}

/// Returns the identity of all retained final P5 state.
pub fn surface_formation_state_fingerprint(
    terrain_fields: &FormationTerrainFields,
    hydrology: &SphericalHydrologySnapshot,
    formation_climate: &GlobalCirculationSnapshot,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.surface-formation-state.v1\0");
    hasher.update(&terrain_fields.fingerprint());
    hasher.update(b"hydrology-json-v2\0");
    serde_json::to_writer(Blake3Writer(&mut hasher), hydrology)
        .expect("validated hydrology always serializes to canonical JSON");
    hasher.update(b"formation-climate-checkpoint\0");
    hasher.update(formation_climate.checkpoint().fingerprint());
    *hasher.finalize().as_bytes()
}

/// Atomic, portable P5 formation snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NaturalSurfaceFormationSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    checkpoint: SurfaceFormationCheckpoint,
    terrain_fields: FormationTerrainFields,
    hydrology: SphericalHydrologySnapshot,
    formation_climate: GlobalCirculationSnapshot,
    solve_report: FormationSolveReport,
    sediment_budget_report: SedimentBudgetReport,
    capabilities: SurfaceFormationCapabilitySet,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NaturalSurfaceFormationSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    checkpoint: SurfaceFormationCheckpoint,
    terrain_fields: FormationTerrainFields,
    hydrology: SphericalHydrologySnapshot,
    formation_climate: GlobalCirculationSnapshot,
    solve_report: FormationSolveReport,
    sediment_budget_report: SedimentBudgetReport,
    capabilities: SurfaceFormationCapabilitySet,
}

impl NaturalSurfaceFormationSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        checkpoint: SurfaceFormationCheckpoint,
        terrain_fields: FormationTerrainFields,
        hydrology: SphericalHydrologySnapshot,
        formation_climate: GlobalCirculationSnapshot,
        solve_report: FormationSolveReport,
        sediment_budget_report: SedimentBudgetReport,
        capabilities: SurfaceFormationCapabilitySet,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            checkpoint,
            terrain_fields,
            hydrology,
            formation_climate,
            solve_report,
            sediment_budget_report,
            capabilities,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        if self.schema_version != NATURAL_SURFACE_FORMATION_SCHEMA_V1 {
            return Err(SurfaceFormationValidationError::UnsupportedSchema {
                object: "natural_surface_formation_snapshot",
                found: self.schema_version,
                supported: NATURAL_SURFACE_FORMATION_SCHEMA_V1,
            });
        }
        self.surface_ref.validate().map_err(|error| {
            SurfaceFormationValidationError::InvalidNested {
                role: "surface_ref",
                reason: error.to_string(),
            }
        })?;
        if !self.surface_ref.geometry_kind().is_spherical() {
            return Err(SurfaceFormationValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        self.checkpoint.validate()?;
        self.terrain_fields.validate()?;
        self.hydrology.validate().map_err(|error| {
            SurfaceFormationValidationError::InvalidNested {
                role: "hydrology",
                reason: error.to_string(),
            }
        })?;
        self.formation_climate.validate().map_err(|error| {
            SurfaceFormationValidationError::InvalidNested {
                role: "formation_climate",
                reason: error.to_string(),
            }
        })?;
        self.solve_report.validate()?;
        self.sediment_budget_report.validate()?;
        self.capabilities.validate()?;

        if self.checkpoint.surface_ref() != self.surface_ref {
            return Err(
                SurfaceFormationValidationError::CheckpointIdentityMismatch {
                    field: "surface_ref",
                },
            );
        }
        for (role, found) in [
            ("hydrology", self.hydrology.surface_ref()),
            ("formation_climate", self.formation_climate.surface_ref()),
        ] {
            if found != self.surface_ref {
                return Err(SurfaceFormationValidationError::NestedSurfaceMismatch {
                    role,
                    found,
                    expected: self.surface_ref,
                });
            }
        }
        if self.terrain_fields.final_elevation_m().len() != self.surface_ref.cell_count() as usize {
            return Err(SurfaceFormationValidationError::FieldLengthMismatch {
                field: "terrain_fields",
                found: self.terrain_fields.final_elevation_m().len(),
                expected: self.surface_ref.cell_count() as usize,
            });
        }
        if self.solve_report.outer_iterations() != self.checkpoint.outer_iterations() {
            return Err(SurfaceFormationValidationError::SolveWorkMismatch {
                field: "outer_iterations",
            });
        }
        if self.capabilities != SurfaceFormationCapabilitySet::p5() {
            return Err(SurfaceFormationValidationError::CapabilityProfileMismatch);
        }
        let expected_state = surface_formation_state_fingerprint(
            &self.terrain_fields,
            &self.hydrology,
            &self.formation_climate,
        );
        if self.checkpoint.state_fingerprint() != &expected_state {
            return Err(SurfaceFormationValidationError::StateFingerprintMismatch);
        }
        Ok(())
    }

    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SurfaceFormationValidationError> {
        self.validate()?;
        surface
            .validate()
            .map_err(|error| SurfaceFormationValidationError::InvalidNested {
                role: "authoritative_surface",
                reason: error.to_string(),
            })?;
        let authoritative = SurfaceRef::from_validated_spherical(surface).map_err(|error| {
            SurfaceFormationValidationError::InvalidNested {
                role: "authoritative_surface_identity",
                reason: error.to_string(),
            }
        })?;
        if authoritative != self.surface_ref {
            return Err(SurfaceFormationValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        self.hydrology.validate_against(surface).map_err(|error| {
            SurfaceFormationValidationError::InvalidNested {
                role: "hydrology",
                reason: error.to_string(),
            }
        })?;
        self.formation_climate
            .validate_against(surface)
            .map_err(|error| SurfaceFormationValidationError::InvalidNested {
                role: "formation_climate",
                reason: error.to_string(),
            })?;
        let areas = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .collect::<Vec<_>>();
        let calculated = water_volume_at_sea_level_m3(
            self.terrain_fields.final_elevation_m(),
            &areas,
            self.terrain_fields.sea_level_m(),
        )
        .map_err(|error| SurfaceFormationValidationError::InvalidNested {
            role: "water_volume",
            reason: error.to_string(),
        })?;
        let relative = relative_error(calculated, self.terrain_fields.realized_water_volume_m3());
        if relative > WATER_VOLUME_RELATIVE_TOLERANCE {
            return Err(SurfaceFormationValidationError::WaterVolumeMismatch {
                stored: self.terrain_fields.realized_water_volume_m3(),
                expected: calculated,
                relative_error: relative,
            });
        }
        Ok(())
    }

    pub fn validate_against_inputs(
        &self,
        surface: &SphericalSurfaceSnapshot,
        quality_profile: NaturalQualityProfile,
        upstream: &SurfaceFormationUpstreamFingerprints,
    ) -> Result<(), SurfaceFormationValidationError> {
        self.validate_against(surface)?;
        self.checkpoint
            .validate_against(self.surface_ref, quality_profile, upstream)
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    pub const fn checkpoint(&self) -> &SurfaceFormationCheckpoint {
        &self.checkpoint
    }

    pub const fn terrain_fields(&self) -> &FormationTerrainFields {
        &self.terrain_fields
    }

    pub const fn hydrology(&self) -> &SphericalHydrologySnapshot {
        &self.hydrology
    }

    pub const fn formation_climate(&self) -> &GlobalCirculationSnapshot {
        &self.formation_climate
    }

    pub const fn solve_report(&self) -> &FormationSolveReport {
        &self.solve_report
    }

    pub const fn sediment_budget_report(&self) -> &SedimentBudgetReport {
        &self.sediment_budget_report
    }

    pub const fn capabilities(&self) -> &SurfaceFormationCapabilitySet {
        &self.capabilities
    }
}

impl<'de> Deserialize<'de> for NaturalSurfaceFormationSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = NaturalSurfaceFormationSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.checkpoint,
            wire.terrain_fields,
            wire.hydrology,
            wire.formation_climate,
            wire.solve_report,
            wire.sediment_budget_report,
            wire.capabilities,
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SurfaceFormationValidationError {
    #[error("unsupported {object} schema {found}; supported schema is {supported}")]
    UnsupportedSchema {
        object: &'static str,
        found: u16,
        supported: u16,
    },
    #[error("invalid nested {role}: {reason}")]
    InvalidNested { role: &'static str, reason: String },
    #[error("surface formation requires a spherical surface, found {found:?}")]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    #[error("dense formation allocation is empty")]
    EmptyDenseAllocation,
    #[error("dense formation allocation has {found} cells, maximum is {maximum}")]
    DenseAllocationTooLarge { found: usize, maximum: usize },
    #[error("formation field {field} has {found} values, expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
    #[error("formation field {field} cell {cell} has invalid value {found}")]
    InvalidCellValue {
        field: &'static str,
        cell: usize,
        found: f64,
    },
    #[error("formation value {field} is invalid: {found}")]
    InvalidValue { field: &'static str, found: f64 },
    #[error(
        "formation elevation identity failed at cell {cell}: stored {stored}, expected {expected}"
    )]
    ComponentIdentityMismatch {
        cell: usize,
        stored: f32,
        expected: f32,
    },
    #[error(
        "sediment provenance at cell {cell} sums to {found} for thickness {thickness_m}, expected {expected}"
    )]
    ProvenanceSumMismatch {
        cell: usize,
        thickness_m: f32,
        found: f64,
        expected: f64,
    },
    #[error("land/ocean class at cell {cell} is {found:?}, expected {expected:?}")]
    LandOceanMismatch {
        cell: usize,
        found: LandOceanKind,
        expected: LandOceanKind,
    },
    #[error(
        "stored water volume {stored} differs from expected {expected} by {relative_error} relative"
    )]
    WaterVolumeMismatch {
        stored: f64,
        expected: f64,
        relative_error: f64,
    },
    #[error("{field} cannot be an all-zero fingerprint")]
    ZeroFingerprint { field: &'static str },
    #[error("surface-formation checkpoint fingerprint does not match its semantic fields")]
    CheckpointFingerprintMismatch,
    #[error("surface-formation model identity does not match the locked P5 equation")]
    ModelIdentityMismatch,
    #[error("outer iteration count {found} is outside 1..={maximum}")]
    InvalidOuterIterations { found: u8, maximum: u8 },
    #[error("checkpoint identity field {field} does not match authoritative input")]
    CheckpointIdentityMismatch { field: &'static str },
    #[error("capability inventory has {found} entries, expected {expected}")]
    CapabilityInventoryMismatch { found: usize, expected: usize },
    #[error("capability inventory is not canonical at index {index}")]
    NonCanonicalCapability { index: usize },
    #[error("formation solve report is inconsistent in {field}")]
    SolveWorkMismatch { field: &'static str },
    #[error(
        "formation fixed point did not converge; normalized residual is {normalized_residual}"
    )]
    FormationNotConverged { normalized_residual: f64 },
    #[error("dense state report is {found} bytes, expected 1..={maximum}")]
    InvalidDenseStateBytes { found: u64, maximum: u64 },
    #[error("sediment {field} ledger residual {relative_error} exceeds {maximum}")]
    SedimentBudgetNotClosed {
        field: &'static str,
        relative_error: f64,
        maximum: f64,
    },
    #[error("sediment source {source_index} residual {relative_error} exceeds {maximum}")]
    SedimentProvenanceNotClosed {
        source_index: usize,
        relative_error: f64,
        maximum: f64,
    },
    #[error("nested {role} surface {found:?} does not match {expected:?}")]
    NestedSurfaceMismatch {
        role: &'static str,
        found: SurfaceRef,
        expected: SurfaceRef,
    },
    #[error("formation capability set does not match the exact P5 profile")]
    CapabilityProfileMismatch,
    #[error("formation checkpoint state fingerprint does not match retained final state")]
    StateFingerprintMismatch,
    #[error("formation surface {snapshot:?} does not match authoritative {authoritative:?}")]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
}

fn validate_dense_count(count: usize) -> Result<(), SurfaceFormationValidationError> {
    if count == 0 {
        return Err(SurfaceFormationValidationError::EmptyDenseAllocation);
    }
    if count > MAX_FORMATION_CELLS {
        return Err(SurfaceFormationValidationError::DenseAllocationTooLarge {
            found: count,
            maximum: MAX_FORMATION_CELLS,
        });
    }
    Ok(())
}

fn validate_field_length(
    field: &'static str,
    found: usize,
    expected: usize,
) -> Result<(), SurfaceFormationValidationError> {
    if found != expected {
        return Err(SurfaceFormationValidationError::FieldLengthMismatch {
            field,
            found,
            expected,
        });
    }
    Ok(())
}

fn validate_f32_slice(
    field: &'static str,
    values: &[f32],
    minimum: f32,
    maximum: f32,
) -> Result<(), SurfaceFormationValidationError> {
    for (cell, &found) in values.iter().enumerate() {
        if !found.is_finite() || !(minimum..=maximum).contains(&found) {
            return Err(SurfaceFormationValidationError::InvalidCellValue {
                field,
                cell,
                found: f64::from(found),
            });
        }
    }
    Ok(())
}

fn validate_f64_slice(
    field: &'static str,
    values: &[f64],
    minimum: f64,
    maximum: f64,
) -> Result<(), SurfaceFormationValidationError> {
    for (cell, &found) in values.iter().enumerate() {
        if !found.is_finite() || !(minimum..=maximum).contains(&found) {
            return Err(SurfaceFormationValidationError::InvalidCellValue { field, cell, found });
        }
    }
    Ok(())
}

fn relative_error(left: f64, right: f64) -> f64 {
    (left - right).abs() / left.abs().max(right.abs()).max(1.0)
}

fn update_surface_ref_hash(hasher: &mut blake3::Hasher, surface_ref: SurfaceRef) {
    hasher.update(&[surface_geometry_tag(surface_ref.geometry_kind())]);
    hasher.update(&surface_ref.geometry_schema().to_le_bytes());
    hasher.update(&surface_ref.cell_count().to_le_bytes());
    hasher.update(&surface_ref.edge_count().to_le_bytes());
    hasher.update(&surface_ref.fingerprint());
}

fn update_f32_slice_hash(hasher: &mut blake3::Hasher, values: &[f32]) {
    hasher.update(&(values.len() as u64).to_le_bytes());
    for &value in values {
        hasher.update(&value.to_bits().to_le_bytes());
    }
}

fn update_f64_slice_hash(hasher: &mut blake3::Hasher, values: &[f64]) {
    hasher.update(&(values.len() as u64).to_le_bytes());
    for &value in values {
        hasher.update(&value.to_bits().to_le_bytes());
    }
}

struct Blake3Writer<'a>(&'a mut blake3::Hasher);

impl Write for Blake3Writer<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn deserialize_formation_f32_values<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_FORMATION_CELLS>(deserializer)
}

fn deserialize_formation_f64_values<'de, D>(deserializer: D) -> Result<Vec<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_FORMATION_CELLS>(deserializer)
}

fn deserialize_formation_provenance_values<'de, D>(
    deserializer: D,
) -> Result<Vec<[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_FORMATION_CELLS>(deserializer)
}

fn deserialize_formation_land_ocean<'de, D>(deserializer: D) -> Result<LandOceanField, D::Error>
where
    D: Deserializer<'de>,
{
    let values = deserialize_bounded_vec::<_, _, MAX_FORMATION_CELLS>(deserializer)?;
    LandOceanField::from_raw(values).map_err(D::Error::custom)
}

fn deserialize_formation_capabilities<'de, D>(
    deserializer: D,
) -> Result<Vec<SurfaceFormationCapabilityStatus>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, 8>(deserializer)
}

fn deserialize_formation_residuals<'de, D>(
    deserializer: D,
) -> Result<Vec<FormationResiduals>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, 4>(deserializer)
}
