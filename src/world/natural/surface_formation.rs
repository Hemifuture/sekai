use std::io::{self, Write};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::surface_water_geometry::surface_elevation_fingerprint;
use super::{
    LandOceanField, NaturalQualityProfile, SedimentSourceKind, SphericalHydrologySnapshot,
    SurfaceWaterGeometry, SurfaceWaterGeometryValidationError, ANNUAL_PRECIPITATION_MAX_MM,
    CLIMATE_MONTH_COUNT, CLIMATOLOGICAL_YEAR_SECONDS, ELEVATION_MAX_M, ELEVATION_MIN_M,
    MEAN_SOLAR_DAY_SECONDS, WATER_VOLUME_RELATIVE_TOLERANCE,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceGeometryKind, SurfaceRef};
use crate::world::MAX_SPHERICAL_CELL_COUNT;

/// Finite-time formation product retaining one authoritative fractional water geometry.
pub const NATURAL_SURFACE_FORMATION_SCHEMA_V5: u16 = 5;
/// Current-state-only P5 checkpoint schema.
pub const SURFACE_FORMATION_CHECKPOINT_SCHEMA_V2: u16 = 2;
/// Retained terrain with nine causal elevation components and one water geometry.
pub const FORMATION_TERRAIN_FIELDS_SCHEMA_V4: u16 = 4;
/// The fixed number of retained sediment-source provenance channels.
pub const SEDIMENT_PROVENANCE_SOURCE_COUNT: usize = 5;
/// Declared coarse-grained physical time consumed by one P5 map formation.
///
/// This product parameter is restored from the frozen P5 contract by the
/// 2026-08-25 geologic-pipeline design amendment, section 0.1(9). It is not a
/// claim that terrestrial geomorphology reaches equilibrium over this span.
pub const SURFACE_FORMATION_HORIZON_YEARS: f64 = 100_000.0;
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
///
/// `K` in `E = K * A_eff^m * (S - S_c)` with `A_eff` in square metres and
/// `m = 0.5`, so its unit is `1/yr`. Direct laboratory or field values for `K`
/// are not transferable between exponent pairs, catchment scales and lithology
/// suites, so it is pinned against the one quantity the model shares with
/// observation: the area-weighted land denudation rate. Cosmogenic `10Be`
/// compilations give a global basin median of `54 m/Myr` (Portenga & Bierman
/// 2011, *GSA Today* 21(8), 4-10, DOI `10.1130/G111A.1`; Willenbring, Codilean
/// & McElroy 2013, *Geology* 41(3), 343-346, DOI `10.1130/G33918.1`), which the
/// global suspended flux to the ocean independently supports (Milliman &
/// Farnsworth 2011). `tests/formation_denudation.rs` measures the published
/// budget with production operators and gates it on that band.
///
/// Measured on Draft seeds 42 and 3 (2026-09-02, audit remediation A0 task 7):
/// the predecessor `5.0e-6` produced `672` and `747 m/Myr`, an order of
/// magnitude above every observational compilation, and its excess only stayed
/// invisible while `V_eff = 0` dumped the whole eroded mass into the ocean.
/// Sweeping `3.0e-7 -> 24/26`, `7.0e-7 -> 74/78` and this value `-> 49/52`
/// selects it as the closest match to the observed median. The result also sits
/// inside the `1e-7..1e-5` span Stock & Montgomery (1999), *JGR* 104,
/// 4983-4993, DOI `10.1029/98JB02139`, report for comparable exponent pairs,
/// where the predecessor sat at the very top.
pub const FORMATION_STREAM_POWER_REFERENCE_ERODIBILITY_PER_YEAR: f64 = 5.0e-7;
/// Baseline multiplier retained even for resistant substrate.
pub const FORMATION_STREAM_POWER_ERODIBILITY_BASE: f64 = 0.25;
/// Additional multiplier contributed by unit substrate erodibility.
pub const FORMATION_STREAM_POWER_ERODIBILITY_RANGE: f64 = 1.50;
/// Reference annual runoff that converts a discharge into an effective
/// drainage area for the stream-power law.
///
/// `A^m` is only a legitimate stand-in for `Q^m` where runoff is spatially
/// uniform (Whipple & Tucker 1999, *JGR* 104, 17661-17674, DOI
/// `10.1029/1999JB900120`), which a solved non-uniform climate is not. P5
/// therefore uses the integrated discharge it already routes and expresses it
/// as `A_eff = Q / P_ref`, which keeps `K`'s unit and reduces exactly to the
/// drainage area in a world whose runoff is `P_ref` everywhere.
pub const FORMATION_STREAM_POWER_RUNOFF_REFERENCE_MM: f64 = 1_000.0;
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
/// Fixed coarse alluvial bulk density used for every retained deposit.
pub const FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3: f64 = 1_800.0;
/// T1 valley-relief presentation reference; P5 sediment routing does not use it.
pub const FORMATION_FLOODPLAIN_ACCOMMODATION_M: f64 = 50.0;
/// Dimensionless Davy-Lague sediment deposition coefficient `G = V_eff / P`.
///
/// Davy & Lague (2009), *JGR-ES* 114, F03007, DOI `10.1029/2008JF001146`,
/// equations (7)-(9), write fluvial deposition as `V_eff * Q_s / Q`. Yuan et
/// al. (2019), *JGR-ES* 124, 1346-1365, DOI `10.1029/2018JF004867`, remove the
/// dimensional ambiguity of `V_eff` by expressing it as this ratio against the
/// runoff rate `P`, which makes the per-cell deposited share a pure area ratio
/// `G*A_cell / (A_upstream + G*A_cell)` and the along-reach total
/// `G*ln(A_out/A_in)`, that is, independent of the grid the reach is sampled
/// on. Guerit et al. (2019), *Geology* 47(9), 853-856, DOI
/// `10.1130/G46356.1`, invert experimental and natural landscapes for
/// `G = 0.4..1.2`; this adopts the center of that range.
///
/// The predecessor of this constant pinned `V_eff = 0`, the infinite
/// transport-length end member, which made every cell with a downstream
/// receiver deposit exactly nothing and sent all eroded mass straight to the
/// ocean. That left the corpus with no valley fill, no floodplains and no
/// deltas, and is what failed the `corpus-median-land-area-share-below-100m`
/// and `corpus-median-land-relief-p05-m` hypsometric envelopes.
pub const FORMATION_SEDIMENT_DEPOSITION_COEFFICIENT: f64 = 1.0;
/// Fixed coarse shelf-break depth limiting marine accommodation.
pub const FORMATION_SHELF_BREAK_DEPTH_M: f64 = 200.0;
/// Normal-wind scale in the bounded coastal exposure proxy.
pub const FORMATION_COASTAL_WIND_REFERENCE_M_S: f64 = 15.0;
/// Alongshore-current scale in the bounded coastal exposure proxy.
pub const FORMATION_COASTAL_CURRENT_REFERENCE_M_S: f64 = 1.0;
/// Sediment-cover thickness that halves coastal bedrock exposure.
pub const FORMATION_COASTAL_COVER_SHIELD_M: f64 = 10.0;
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

/// Converts published P4 mean daily rates into the single bounded monthly
/// formation precipitation envelope every P5 process forcing is derived from.
///
/// P4 admits mean rates far above the published annual-precipitation envelope
/// that the rest of the natural pipeline shares. A cell above that envelope is
/// scaled by one factor across all twelve months, so monthly totals stay
/// consistent with their own annual sum and with every derived rate. The
/// envelope stays in `f64`; each consumer quantizes its own published field
/// exactly once.
pub fn formation_monthly_precipitation_mm(
    monthly_mm_day: &[f32; CLIMATE_MONTH_COUNT],
) -> [f64; CLIMATE_MONTH_COUNT] {
    let days_per_month =
        CLIMATOLOGICAL_YEAR_SECONDS / CLIMATE_MONTH_COUNT as f64 / MEAN_SOLAR_DAY_SECONDS;
    let mut monthly: [f64; CLIMATE_MONTH_COUNT] =
        std::array::from_fn(|month| f64::from(monthly_mm_day[month]) * days_per_month);
    let annual = monthly.iter().sum::<f64>();
    if annual > f64::from(ANNUAL_PRECIPITATION_MAX_MM) {
        let scale = f64::from(ANNUAL_PRECIPITATION_MAX_MM) / annual;
        for value in &mut monthly {
            *value *= scale;
        }
    }
    monthly
}

/// Returns the bounded annual formation precipitation of one cell.
pub fn formation_annual_precipitation_mm(monthly_mm_day: &[f32; CLIMATE_MONTH_COUNT]) -> f32 {
    formation_monthly_precipitation_mm(monthly_mm_day)
        .iter()
        .sum::<f64>() as f32
}

/// Conservative inventory of every dense buffer one P5 solve owns at once.
///
/// The nested production circulation reports its own dense ownership through
/// `ClimateSolveReport`; this inventory covers the formation-owned buffers.
pub fn expected_surface_formation_dense_state_bytes(
    cell_count: u32,
    edge_count: u32,
) -> Option<u64> {
    /// Current and candidate exact states: primary, eight signed components,
    /// and the rebuilt retained elevation.
    const EXACT_ELEVATION_STATE_BYTES_PER_CELL: u64 = 2 * 10 * 8;
    /// Current and candidate exact rates for all eight formation processes.
    const EXACT_PROCESS_RATE_BYTES_PER_CELL: u64 = 2 * 8 * 8;
    /// Current state, cloned trial state, transfer candidate, zero transfer,
    /// and router result for all five exact sediment-source masses.
    const EXACT_SEDIMENT_STOCK_BYTES_PER_CELL: u64 = 5 * 5 * 8;
    /// Current state, cloned trial state, and replacement exact water geometry:
    /// two `f64` cell fields plus one `u32` land/ocean field.
    const EXACT_WATER_BYTES_PER_CELL: u64 = 3 * (2 * 8 + 4);
    /// The sole final wire water geometry: one `f32`, one `f64`, and one `u32`
    /// cell field.
    const WIRE_WATER_BYTES_PER_CELL: u64 = 4 + 8 + 4;
    /// One retained final terrain plus one transient terminal sediment
    /// diagnostic. Terrain owns ten `f32` components; each sediment field owns
    /// thickness, five provenance fractions, four `f64` ledgers, and delta
    /// potential.
    const SEDIMENT_FIELDS_BYTES_PER_CELL: u64 = 4 + 5 * 4 + 4 * 8 + 4;
    const RETAINED_TERRAIN_BYTES_PER_CELL: u64 = 10 * 4 + 2 * SEDIMENT_FIELDS_BYTES_PER_CELL;
    /// Previous and candidate hydrology: monthly runoff and discharge, five
    /// dense `f32` fields, receiver, basin, Strahler order, and surface water.
    const HYDROLOGY_BYTES_PER_CELL: u64 = 2 * (2 * 12 * 4 + 5 * 4 + 8 + 8 + 4 + 4);
    /// Paired hillslope workspace request, limit, mass, and source buffers.
    const HILLSLOPE_WORKSPACE_BYTES_PER_CELL: u64 = 6 * 8 + 2 * 5 * 8;
    /// Sediment packets, deposited packets, six `f64` ledgers, and the stable
    /// upstream-to-downstream order.
    const SEDIMENT_ROUTER_BYTES_PER_CELL: u64 = 2 * 10 * 8 + 6 * 8 + 4;
    /// One requested paired transfer per authoritative edge.
    const HILLSLOPE_WORKSPACE_BYTES_PER_EDGE: u64 = 32;
    /// Three exact working wet-edge fractions plus the sole final wire field.
    const WATER_BYTES_PER_EDGE: u64 = 3 * 8 + 4;

    let per_cell = EXACT_ELEVATION_STATE_BYTES_PER_CELL
        + EXACT_PROCESS_RATE_BYTES_PER_CELL
        + EXACT_SEDIMENT_STOCK_BYTES_PER_CELL
        + EXACT_WATER_BYTES_PER_CELL
        + WIRE_WATER_BYTES_PER_CELL
        + RETAINED_TERRAIN_BYTES_PER_CELL
        + HYDROLOGY_BYTES_PER_CELL
        + HILLSLOPE_WORKSPACE_BYTES_PER_CELL
        + SEDIMENT_ROUTER_BYTES_PER_CELL;
    let per_edge = HILLSLOPE_WORKSPACE_BYTES_PER_EDGE + WATER_BYTES_PER_EDGE;
    u64::from(cell_count)
        .checked_mul(per_cell)?
        .checked_add(u64::from(edge_count).checked_mul(per_edge)?)
        .filter(|bytes| *bytes > 0 && *bytes <= SURFACE_FORMATION_DENSE_STATE_BYTES_MAX)
}

const MAX_FORMATION_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const MAX_COMPONENT_ABS_M: f32 = 100_000.0;
const MAX_SEDIMENT_THICKNESS_M: f32 = 100_000.0;
const PROVENANCE_SUM_TOLERANCE: f64 = 1.0e-6;

/// The one production geomorphic equation family admitted by P5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceFormationModelId {
    /// Priority-Flood + implicit stream power + paired nonlinear hillslope,
    /// Davy-Lague sediment continuity with runoff-scaled deposition, coastal
    /// exchange, and local Airy isostasy.
    PriorityFloodFastscapeDavyLagueHillslopeCoastIsostasyFiniteTimeV4,
}

/// Returns the canonical identity of every equation and frozen P5 constant.
pub fn surface_formation_model_fingerprint() -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.surface-formation-equations.v7\0");
    hasher.update(&[surface_formation_model_tag(
        SurfaceFormationModelId::PriorityFloodFastscapeDavyLagueHillslopeCoastIsostasyFiniteTimeV4,
    )]);
    for value in [
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
        FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
        FORMATION_SEDIMENT_DEPOSITION_COEFFICIENT,
        FORMATION_SHELF_BREAK_DEPTH_M,
        FORMATION_COASTAL_WIND_REFERENCE_M_S,
        FORMATION_COASTAL_CURRENT_REFERENCE_M_S,
        FORMATION_COASTAL_COVER_SHIELD_M,
        FORMATION_COASTAL_EROSION_MAX_M_PER_YEAR,
        FORMATION_AIRY_MANTLE_DENSITY_KG_M3,
        FORMATION_ENDORHEIC_RESIDENCE_YEARS,
        CLIMATOLOGICAL_YEAR_SECONDS,
        MEAN_SOLAR_DAY_SECONDS,
        f64::from(ANNUAL_PRECIPITATION_MAX_MM),
        SURFACE_FORMATION_HORIZON_YEARS,
        SEDIMENT_BUDGET_RELATIVE_ERROR_MAX,
        SEDIMENT_PROVENANCE_RELATIVE_ERROR_MAX,
        WATER_VOLUME_RELATIVE_TOLERANCE,
    ] {
        hasher.update(&value.to_bits().to_le_bytes());
    }
    hasher.update(b"priority-flood-stable-dag-v1\0");
    hasher.update(b"braun-willett-n1-backward-euler-v1\0");
    hasher.update(b"roering-paired-finite-volume-v1\0");
    hasher.update(b"davy-lague-analytic-five-source-sediment-continuity-v1\0");
    hasher.update(b"fluvial-transport:davy-lague-runoff-scaled-v-eff\0");
    hasher.update(b"finite-time-held-tectonic-forcing-v1\0");
    hasher.update(b"sources:felsic,mafic,volcaniclastic,sedimentary,metamorphic\0");
    hasher.update(b"nine-causal-elevation-components-v4\0");
    hasher.update(&FORMATION_TERRAIN_FIELDS_SCHEMA_V4.to_le_bytes());
    hasher.update(b"fixed-water-volume-piecewise-linear-sea-level-v1\0");
    *hasher.finalize().as_bytes()
}

const fn surface_formation_model_tag(model: SurfaceFormationModelId) -> u8 {
    match model {
        SurfaceFormationModelId::PriorityFloodFastscapeDavyLagueHillslopeCoastIsostasyFiniteTimeV4 => {
            4
        }
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

/// Exact upstream identities consumed by one P5 formation build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceFormationUpstreamFingerprints {
    evolved_tectonic_fingerprint: [u8; 32],
    geologic_substrate_fingerprint: [u8; 32],
    primary_relief_fingerprint: [u8; 32],
    climate_work_domain_fingerprint: [u8; 32],
    climate_spec_fingerprint: [u8; 32],
    formation_climate_checkpoint_fingerprint: [u8; 32],
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
    formation_climate_checkpoint_fingerprint: [u8; 32],
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
        formation_climate_checkpoint_fingerprint: [u8; 32],
        formation_spec_fingerprint: [u8; 32],
    ) -> Result<Self, SurfaceFormationValidationError> {
        let fingerprints = Self {
            evolved_tectonic_fingerprint,
            geologic_substrate_fingerprint,
            primary_relief_fingerprint,
            climate_work_domain_fingerprint,
            climate_spec_fingerprint,
            formation_climate_checkpoint_fingerprint,
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
                "formation_climate_checkpoint_fingerprint",
                self.formation_climate_checkpoint_fingerprint,
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
        hasher.update(&self.formation_climate_checkpoint_fingerprint);
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

    pub const fn formation_climate_checkpoint_fingerprint(&self) -> &[u8; 32] {
        &self.formation_climate_checkpoint_fingerprint
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
            wire.formation_climate_checkpoint_fingerprint,
            wire.formation_spec_fingerprint,
        )
        .map_err(D::Error::custom)
    }
}

/// Identity of one atomically published current P5 state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceFormationCheckpoint {
    schema_version: u16,
    surface_ref: SurfaceRef,
    quality_profile: NaturalQualityProfile,
    model: SurfaceFormationModelId,
    model_fingerprint: [u8; 32],
    upstream: SurfaceFormationUpstreamFingerprints,
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
    state_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
}

impl SurfaceFormationCheckpoint {
    pub fn new(
        surface_ref: SurfaceRef,
        quality_profile: NaturalQualityProfile,
        upstream: SurfaceFormationUpstreamFingerprints,
        state_fingerprint: [u8; 32],
    ) -> Result<Self, SurfaceFormationValidationError> {
        let mut checkpoint = Self {
            schema_version: SURFACE_FORMATION_CHECKPOINT_SCHEMA_V2,
            surface_ref,
            quality_profile,
            model:
                SurfaceFormationModelId::PriorityFloodFastscapeDavyLagueHillslopeCoastIsostasyFiniteTimeV4,
            model_fingerprint: surface_formation_model_fingerprint(),
            upstream,
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
        if self.schema_version != SURFACE_FORMATION_CHECKPOINT_SCHEMA_V2 {
            return Err(SurfaceFormationValidationError::UnsupportedSchema {
                object: "surface_formation_checkpoint",
                found: self.schema_version,
                supported: SURFACE_FORMATION_CHECKPOINT_SCHEMA_V2,
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
            != SurfaceFormationModelId::PriorityFloodFastscapeDavyLagueHillslopeCoastIsostasyFiniteTimeV4
            || self.model_fingerprint != surface_formation_model_fingerprint()
        {
            return Err(SurfaceFormationValidationError::ModelIdentityMismatch);
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
        hasher.update(b"sekai.surface-formation-checkpoint.v2\0");
        hasher.update(&self.schema_version.to_le_bytes());
        update_surface_ref_hash(&mut hasher, self.surface_ref);
        hasher.update(&[natural_quality_profile_tag(self.quality_profile)]);
        hasher.update(&[surface_formation_model_tag(self.model)]);
        hasher.update(&self.model_fingerprint);
        self.upstream.update_hasher(&mut hasher);
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
        if wire.schema_version != SURFACE_FORMATION_CHECKPOINT_SCHEMA_V2 {
            return Err(D::Error::custom(
                SurfaceFormationValidationError::UnsupportedSchema {
                    object: "surface_formation_checkpoint",
                    found: wire.schema_version,
                    supported: SURFACE_FORMATION_CHECKPOINT_SCHEMA_V2,
                },
            ));
        }
        let checkpoint = Self::new(
            wire.surface_ref,
            wire.quality_profile,
            wire.upstream,
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

/// Returns the exact retained P5 elevation identity in its declared causal order.
///
/// Tectonic displacement and isostatic response are signed and added. Fluvial,
/// hillslope, and coastal erosion are nonnegative depths and subtracted.
/// Hillslope, routed-sediment, and coastal deposition are nonnegative
/// thicknesses and added.
#[allow(clippy::too_many_arguments)]
pub fn formation_elevation_from_components(
    primary_elevation_m: f64,
    tectonic_displacement_m: f64,
    fluvial_erosion_m: f64,
    hillslope_erosion_m: f64,
    hillslope_deposition_m: f64,
    routed_sediment_deposition_m: f64,
    coastal_erosion_m: f64,
    coastal_deposition_m: f64,
    isostatic_response_m: f64,
) -> f64 {
    primary_elevation_m + tectonic_displacement_m - fluvial_erosion_m - hillslope_erosion_m
        + hillslope_deposition_m
        + routed_sediment_deposition_m
        - coastal_erosion_m
        + coastal_deposition_m
        + isostatic_response_m
}

/// Final current elevation and its nine retained causal components.
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
                f64::from(self.primary_elevation_m[index]),
                f64::from(self.tectonic_displacement_m[index]),
                f64::from(self.fluvial_erosion_m[index]),
                f64::from(self.hillslope_erosion_m[index]),
                f64::from(self.hillslope_deposition_m[index]),
                f64::from(self.routed_sediment_deposition_m[index]),
                f64::from(self.coastal_erosion_m[index]),
                f64::from(self.coastal_deposition_m[index]),
                f64::from(self.isostatic_response_m[index]),
            ) as f32;
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
    sediment_throughput_kg_per_year: Vec<f64>,
    shelf_deposition_kg_per_year: Vec<f64>,
    deep_ocean_export_kg_per_year: Vec<f64>,
    endorheic_deposition_kg_per_year: Vec<f64>,
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
    sediment_throughput_kg_per_year: Vec<f64>,
    #[serde(deserialize_with = "deserialize_formation_f64_values")]
    shelf_deposition_kg_per_year: Vec<f64>,
    #[serde(deserialize_with = "deserialize_formation_f64_values")]
    deep_ocean_export_kg_per_year: Vec<f64>,
    #[serde(deserialize_with = "deserialize_formation_f64_values")]
    endorheic_deposition_kg_per_year: Vec<f64>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    delta_potential: Vec<f32>,
}

impl FormationSedimentFields {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sediment_thickness_m: Vec<f32>,
        provenance_fraction: Vec<[f32; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
        sediment_throughput_kg_per_year: Vec<f64>,
        shelf_deposition_kg_per_year: Vec<f64>,
        deep_ocean_export_kg_per_year: Vec<f64>,
        endorheic_deposition_kg_per_year: Vec<f64>,
        delta_potential: Vec<f32>,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let fields = Self {
            sediment_thickness_m,
            provenance_fraction,
            sediment_throughput_kg_per_year,
            shelf_deposition_kg_per_year,
            deep_ocean_export_kg_per_year,
            endorheic_deposition_kg_per_year,
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
            (
                "sediment_throughput_kg_per_year",
                self.sediment_throughput_kg_per_year.len(),
            ),
            (
                "shelf_deposition_kg_per_year",
                self.shelf_deposition_kg_per_year.len(),
            ),
            (
                "deep_ocean_export_kg_per_year",
                self.deep_ocean_export_kg_per_year.len(),
            ),
            (
                "endorheic_deposition_kg_per_year",
                self.endorheic_deposition_kg_per_year.len(),
            ),
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
            "sediment_throughput_kg_per_year",
            &self.sediment_throughput_kg_per_year,
            0.0,
            f64::MAX,
        )?;
        validate_f64_slice(
            "shelf_deposition_kg_per_year",
            &self.shelf_deposition_kg_per_year,
            0.0,
            f64::MAX,
        )?;
        validate_f64_slice(
            "deep_ocean_export_kg_per_year",
            &self.deep_ocean_export_kg_per_year,
            0.0,
            f64::MAX,
        )?;
        validate_f64_slice(
            "endorheic_deposition_kg_per_year",
            &self.endorheic_deposition_kg_per_year,
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

    pub fn sediment_throughput_kg_per_year(&self) -> &[f64] {
        &self.sediment_throughput_kg_per_year
    }

    pub fn shelf_deposition_kg_per_year(&self) -> &[f64] {
        &self.shelf_deposition_kg_per_year
    }

    pub fn deep_ocean_export_kg_per_year(&self) -> &[f64] {
        &self.deep_ocean_export_kg_per_year
    }

    pub fn endorheic_deposition_kg_per_year(&self) -> &[f64] {
        &self.endorheic_deposition_kg_per_year
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
            wire.sediment_throughput_kg_per_year,
            wire.shelf_deposition_kg_per_year,
            wire.deep_ocean_export_kg_per_year,
            wire.endorheic_deposition_kg_per_year,
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
    surface_water_geometry: SurfaceWaterGeometry,
    water_inventory_m3: f64,
    sediment: FormationSedimentFields,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationTerrainFieldsWire {
    schema_version: u16,
    elevation_components: FormationElevationComponents,
    surface_water_geometry: SurfaceWaterGeometry,
    water_inventory_m3: f64,
    sediment: FormationSedimentFields,
}

impl FormationTerrainFields {
    pub fn new(
        schema_version: u16,
        elevation_components: FormationElevationComponents,
        surface_water_geometry: SurfaceWaterGeometry,
        water_inventory_m3: f64,
        sediment: FormationSedimentFields,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let fields = Self {
            schema_version,
            elevation_components,
            surface_water_geometry,
            water_inventory_m3,
            sediment,
        };
        fields.validate()?;
        Ok(fields)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        if self.schema_version != FORMATION_TERRAIN_FIELDS_SCHEMA_V4 {
            return Err(SurfaceFormationValidationError::UnsupportedSchema {
                object: "formation_terrain_fields",
                found: self.schema_version,
                supported: FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
            });
        }
        self.elevation_components.validate()?;
        self.surface_water_geometry.validate()?;
        self.sediment.validate()?;
        let count = self.elevation_components.len();
        validate_field_length(
            "surface_water_geometry",
            self.surface_water_geometry.ocean_area_fraction().len(),
            count,
        )?;
        validate_field_length("sediment", self.sediment.len(), count)?;
        if self.surface_water_geometry.elevation_fingerprint()
            != &surface_elevation_fingerprint(self.elevation_components.final_elevation_m())
        {
            return Err(SurfaceWaterGeometryValidationError::ElevationFingerprintMismatch.into());
        }
        for (field, value) in [("water_inventory_m3", self.water_inventory_m3)] {
            if !value.is_finite() || value < 0.0 {
                return Err(SurfaceFormationValidationError::InvalidValue {
                    field,
                    found: value,
                });
            }
        }
        let realized_water_volume_m3 = self.surface_water_geometry.total_water_volume_m3();
        if !realized_water_volume_m3.is_finite() {
            return Err(SurfaceFormationValidationError::InvalidValue {
                field: "realized_water_volume_m3",
                found: realized_water_volume_m3,
            });
        }
        let water_error = relative_error(realized_water_volume_m3, self.water_inventory_m3);
        if water_error > WATER_VOLUME_RELATIVE_TOLERANCE {
            return Err(SurfaceFormationValidationError::WaterVolumeMismatch {
                stored: realized_water_volume_m3,
                expected: self.water_inventory_m3,
                relative_error: water_error,
            });
        }
        Ok(())
    }

    pub fn validate_against_surface(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), SurfaceFormationValidationError> {
        self.validate()?;
        self.surface_water_geometry
            .validate_against(surface, self.current_elevation_m())?;
        Ok(())
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.formation-terrain-fields.v4\0");
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
        hasher.update(self.surface_water_geometry.fingerprint());
        hasher.update(&self.water_inventory_m3.to_bits().to_le_bytes());
        for fractions in self.sediment.provenance_fraction() {
            update_f32_slice_hash(&mut hasher, fractions);
        }
        for values in [
            self.sediment.sediment_throughput_kg_per_year(),
            self.sediment.shelf_deposition_kg_per_year(),
            self.sediment.deep_ocean_export_kg_per_year(),
            self.sediment.endorheic_deposition_kg_per_year(),
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

    pub fn current_elevation_m(&self) -> &[f32] {
        self.elevation_components.final_elevation_m()
    }

    pub const fn surface_water_geometry(&self) -> &SurfaceWaterGeometry {
        &self.surface_water_geometry
    }

    pub const fn sea_level_m(&self) -> f32 {
        self.surface_water_geometry.sea_level_m()
    }

    pub const fn water_inventory_m3(&self) -> f64 {
        self.water_inventory_m3
    }

    pub fn realized_water_volume_m3(&self) -> f64 {
        self.surface_water_geometry.total_water_volume_m3()
    }

    pub const fn land_ocean(&self) -> &LandOceanField {
        self.surface_water_geometry.land_ocean()
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
            wire.surface_water_geometry,
            wire.water_inventory_m3,
            wire.sediment,
        )
        .map_err(D::Error::custom)
    }
}

/// Current geomorphic process rates evaluated on the published terrain.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormationProcessRates {
    tectonic_displacement_rate_m_per_year: Vec<f32>,
    fluvial_erosion_rate_m_per_year: Vec<f32>,
    hillslope_erosion_rate_m_per_year: Vec<f32>,
    hillslope_deposition_rate_m_per_year: Vec<f32>,
    routed_sediment_deposition_rate_m_per_year: Vec<f32>,
    coastal_erosion_rate_m_per_year: Vec<f32>,
    coastal_deposition_rate_m_per_year: Vec<f32>,
    isostatic_response_rate_m_per_year: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationProcessRatesWire {
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    tectonic_displacement_rate_m_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    fluvial_erosion_rate_m_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    hillslope_erosion_rate_m_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    hillslope_deposition_rate_m_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    routed_sediment_deposition_rate_m_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    coastal_erosion_rate_m_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    coastal_deposition_rate_m_per_year: Vec<f32>,
    #[serde(deserialize_with = "deserialize_formation_f32_values")]
    isostatic_response_rate_m_per_year: Vec<f32>,
}

impl FormationProcessRates {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tectonic_displacement_rate_m_per_year: Vec<f32>,
        fluvial_erosion_rate_m_per_year: Vec<f32>,
        hillslope_erosion_rate_m_per_year: Vec<f32>,
        hillslope_deposition_rate_m_per_year: Vec<f32>,
        routed_sediment_deposition_rate_m_per_year: Vec<f32>,
        coastal_erosion_rate_m_per_year: Vec<f32>,
        coastal_deposition_rate_m_per_year: Vec<f32>,
        isostatic_response_rate_m_per_year: Vec<f32>,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let rates = Self {
            tectonic_displacement_rate_m_per_year,
            fluvial_erosion_rate_m_per_year,
            hillslope_erosion_rate_m_per_year,
            hillslope_deposition_rate_m_per_year,
            routed_sediment_deposition_rate_m_per_year,
            coastal_erosion_rate_m_per_year,
            coastal_deposition_rate_m_per_year,
            isostatic_response_rate_m_per_year,
        };
        rates.validate()?;
        Ok(rates)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        let count = self.tectonic_displacement_rate_m_per_year.len();
        validate_dense_count(count)?;
        for (field, found) in [
            (
                "fluvial_erosion_rate_m_per_year",
                self.fluvial_erosion_rate_m_per_year.len(),
            ),
            (
                "hillslope_erosion_rate_m_per_year",
                self.hillslope_erosion_rate_m_per_year.len(),
            ),
            (
                "hillslope_deposition_rate_m_per_year",
                self.hillslope_deposition_rate_m_per_year.len(),
            ),
            (
                "routed_sediment_deposition_rate_m_per_year",
                self.routed_sediment_deposition_rate_m_per_year.len(),
            ),
            (
                "coastal_erosion_rate_m_per_year",
                self.coastal_erosion_rate_m_per_year.len(),
            ),
            (
                "coastal_deposition_rate_m_per_year",
                self.coastal_deposition_rate_m_per_year.len(),
            ),
            (
                "isostatic_response_rate_m_per_year",
                self.isostatic_response_rate_m_per_year.len(),
            ),
        ] {
            validate_field_length(field, found, count)?;
        }
        validate_f32_slice(
            "tectonic_displacement_rate_m_per_year",
            &self.tectonic_displacement_rate_m_per_year,
            -MAX_COMPONENT_ABS_M,
            MAX_COMPONENT_ABS_M,
        )?;
        validate_f32_slice(
            "isostatic_response_rate_m_per_year",
            &self.isostatic_response_rate_m_per_year,
            -MAX_COMPONENT_ABS_M,
            MAX_COMPONENT_ABS_M,
        )?;
        for (field, values) in [
            (
                "fluvial_erosion_rate_m_per_year",
                self.fluvial_erosion_rate_m_per_year.as_slice(),
            ),
            (
                "hillslope_erosion_rate_m_per_year",
                self.hillslope_erosion_rate_m_per_year.as_slice(),
            ),
            (
                "hillslope_deposition_rate_m_per_year",
                self.hillslope_deposition_rate_m_per_year.as_slice(),
            ),
            (
                "routed_sediment_deposition_rate_m_per_year",
                self.routed_sediment_deposition_rate_m_per_year.as_slice(),
            ),
            (
                "coastal_erosion_rate_m_per_year",
                self.coastal_erosion_rate_m_per_year.as_slice(),
            ),
            (
                "coastal_deposition_rate_m_per_year",
                self.coastal_deposition_rate_m_per_year.as_slice(),
            ),
        ] {
            validate_f32_slice(field, values, 0.0, MAX_COMPONENT_ABS_M)?;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.tectonic_displacement_rate_m_per_year.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tectonic_displacement_rate_m_per_year.is_empty()
    }

    pub fn tectonic_displacement_rate_m_per_year(&self) -> &[f32] {
        &self.tectonic_displacement_rate_m_per_year
    }

    pub fn fluvial_erosion_rate_m_per_year(&self) -> &[f32] {
        &self.fluvial_erosion_rate_m_per_year
    }

    pub fn hillslope_erosion_rate_m_per_year(&self) -> &[f32] {
        &self.hillslope_erosion_rate_m_per_year
    }

    pub fn hillslope_deposition_rate_m_per_year(&self) -> &[f32] {
        &self.hillslope_deposition_rate_m_per_year
    }

    pub fn routed_sediment_deposition_rate_m_per_year(&self) -> &[f32] {
        &self.routed_sediment_deposition_rate_m_per_year
    }

    pub fn coastal_erosion_rate_m_per_year(&self) -> &[f32] {
        &self.coastal_erosion_rate_m_per_year
    }

    pub fn coastal_deposition_rate_m_per_year(&self) -> &[f32] {
        &self.coastal_deposition_rate_m_per_year
    }

    pub fn isostatic_response_rate_m_per_year(&self) -> &[f32] {
        &self.isostatic_response_rate_m_per_year
    }
}

impl<'de> Deserialize<'de> for FormationProcessRates {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FormationProcessRatesWire::deserialize(deserializer)?;
        Self::new(
            wire.tectonic_displacement_rate_m_per_year,
            wire.fluvial_erosion_rate_m_per_year,
            wire.hillslope_erosion_rate_m_per_year,
            wire.hillslope_deposition_rate_m_per_year,
            wire.routed_sediment_deposition_rate_m_per_year,
            wire.coastal_erosion_rate_m_per_year,
            wire.coastal_deposition_rate_m_per_year,
            wire.isostatic_response_rate_m_per_year,
        )
        .map_err(D::Error::custom)
    }
}

/// Terminal physical residuals evaluated on one current P5 state.
///
/// Every dimensional value is an instantaneous rate. No field compares two
/// iterates or records a pseudo-time path.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormationResiduals {
    net_surface_rate_rms_m_per_year: f64,
    gross_surface_rate_rms_m_per_year: f64,
    mean_elevation_rate_m_per_year: f64,
    rms_relief_rate_m_per_year: f64,
    sediment_stock_change_kg_per_year: f64,
    sediment_stock_change_ratio: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationResidualsWire {
    net_surface_rate_rms_m_per_year: f64,
    gross_surface_rate_rms_m_per_year: f64,
    mean_elevation_rate_m_per_year: f64,
    rms_relief_rate_m_per_year: f64,
    sediment_stock_change_kg_per_year: f64,
    sediment_stock_change_ratio: f64,
}

impl FormationResiduals {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        net_surface_rate_rms_m_per_year: f64,
        gross_surface_rate_rms_m_per_year: f64,
        mean_elevation_rate_m_per_year: f64,
        rms_relief_rate_m_per_year: f64,
        sediment_stock_change_kg_per_year: f64,
        sediment_stock_change_ratio: f64,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let residuals = Self {
            net_surface_rate_rms_m_per_year,
            gross_surface_rate_rms_m_per_year,
            mean_elevation_rate_m_per_year,
            rms_relief_rate_m_per_year,
            sediment_stock_change_kg_per_year,
            sediment_stock_change_ratio,
        };
        residuals.validate()?;
        Ok(residuals)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        for (field, value) in [
            (
                "net_surface_rate_rms_m_per_year",
                self.net_surface_rate_rms_m_per_year,
            ),
            (
                "gross_surface_rate_rms_m_per_year",
                self.gross_surface_rate_rms_m_per_year,
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
            (
                "mean_elevation_rate_m_per_year",
                self.mean_elevation_rate_m_per_year,
            ),
            (
                "rms_relief_rate_m_per_year",
                self.rms_relief_rate_m_per_year,
            ),
            (
                "sediment_stock_change_kg_per_year",
                self.sediment_stock_change_kg_per_year,
            ),
        ] {
            if !value.is_finite() {
                return Err(SurfaceFormationValidationError::InvalidValue {
                    field,
                    found: value,
                });
            }
        }
        if self.net_surface_rate_rms_m_per_year > self.gross_surface_rate_rms_m_per_year {
            return Err(SurfaceFormationValidationError::InvalidValue {
                field: "net_surface_rate_rms_m_per_year",
                found: self.net_surface_rate_rms_m_per_year,
            });
        }
        if !self.sediment_stock_change_ratio.is_finite()
            || !(0.0..=1.0).contains(&self.sediment_stock_change_ratio)
        {
            return Err(SurfaceFormationValidationError::InvalidValue {
                field: "sediment_stock_change_ratio",
                found: self.sediment_stock_change_ratio,
            });
        }
        Ok(())
    }

    /// Area-weighted local disequilibrium retained as a diagnostic.
    ///
    /// Quasi-steady landscapes can keep simultaneous local erosion and
    /// deposition, so this value is deliberately not a publication gate.
    pub fn local_surface_flux_imbalance_ratio(&self) -> f64 {
        formation_relative_flux_imbalance(
            self.net_surface_rate_rms_m_per_year,
            self.gross_surface_rate_rms_m_per_year,
        )
    }

    pub fn mean_elevation_flux_balance_ratio(&self) -> f64 {
        formation_relative_flux_imbalance(
            self.mean_elevation_rate_m_per_year.abs(),
            self.gross_surface_rate_rms_m_per_year,
        )
    }

    pub fn rms_relief_flux_balance_ratio(&self) -> f64 {
        formation_relative_flux_imbalance(
            self.rms_relief_rate_m_per_year.abs(),
            self.gross_surface_rate_rms_m_per_year,
        )
    }

    pub const fn net_surface_rate_rms_m_per_year(&self) -> f64 {
        self.net_surface_rate_rms_m_per_year
    }

    pub const fn gross_surface_rate_rms_m_per_year(&self) -> f64 {
        self.gross_surface_rate_rms_m_per_year
    }

    pub const fn mean_elevation_rate_m_per_year(&self) -> f64 {
        self.mean_elevation_rate_m_per_year
    }

    pub const fn rms_relief_rate_m_per_year(&self) -> f64 {
        self.rms_relief_rate_m_per_year
    }

    pub const fn sediment_stock_change_kg_per_year(&self) -> f64 {
        self.sediment_stock_change_kg_per_year
    }

    pub const fn sediment_stock_change_ratio(&self) -> f64 {
        self.sediment_stock_change_ratio
    }
}

/// Symmetric current-flux backward error with no arbitrary zero floor.
pub(crate) fn formation_relative_flux_imbalance(net_abs: f64, gross_abs: f64) -> f64 {
    debug_assert!(net_abs.is_finite() && net_abs >= 0.0);
    debug_assert!(gross_abs.is_finite() && gross_abs >= 0.0);
    if net_abs == 0.0 {
        0.0
    } else {
        net_abs / gross_abs.max(net_abs)
    }
}

impl<'de> Deserialize<'de> for FormationResiduals {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FormationResidualsWire::deserialize(deserializer)?;
        Self::new(
            wire.net_surface_rate_rms_m_per_year,
            wire.gross_surface_rate_rms_m_per_year,
            wire.mean_elevation_rate_m_per_year,
            wire.rms_relief_rate_m_per_year,
            wire.sediment_stock_change_kg_per_year,
            wire.sediment_stock_change_ratio,
        )
        .map_err(D::Error::custom)
    }
}

/// Finite physical-time work and current-rate evidence for one P5 state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormationEvolutionReport {
    accepted_surface_substeps: u32,
    integrated_duration_years: f64,
    current_rates: FormationResiduals,
    dense_state_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormationEvolutionReportWire {
    accepted_surface_substeps: u32,
    integrated_duration_years: f64,
    current_rates: FormationResiduals,
    dense_state_bytes: u64,
}

impl FormationEvolutionReport {
    /// Creates validated work and current-rate evidence for a complete P5 horizon.
    pub fn new(
        accepted_surface_substeps: u32,
        integrated_duration_years: f64,
        current_rates: FormationResiduals,
        dense_state_bytes: u64,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let report = Self {
            accepted_surface_substeps,
            integrated_duration_years,
            current_rates,
            dense_state_bytes,
        };
        report.validate()?;
        Ok(report)
    }

    /// Validates duration, work count, rates, and dense-memory bounds.
    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        if self.accepted_surface_substeps == 0 {
            return Err(
                SurfaceFormationValidationError::InvalidAcceptedSurfaceSubsteps {
                    found: self.accepted_surface_substeps,
                },
            );
        }
        if self.integrated_duration_years.to_bits() != SURFACE_FORMATION_HORIZON_YEARS.to_bits() {
            return Err(SurfaceFormationValidationError::InvalidIntegratedDuration {
                found: self.integrated_duration_years,
                expected: SURFACE_FORMATION_HORIZON_YEARS,
            });
        }
        self.current_rates.validate()?;
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

    /// Returns the number of accepted stable surface substeps.
    pub const fn accepted_surface_substeps(&self) -> u32 {
        self.accepted_surface_substeps
    }

    /// Returns the exact finite P5 duration integrated in years.
    pub const fn integrated_duration_years(&self) -> f64 {
        self.integrated_duration_years
    }

    /// Returns named endpoint process-rate diagnostics.
    pub const fn current_rates(&self) -> &FormationResiduals {
        &self.current_rates
    }

    /// Returns the conservative dense-memory inventory in bytes.
    pub const fn dense_state_bytes(&self) -> u64 {
        self.dense_state_bytes
    }
}

impl<'de> Deserialize<'de> for FormationEvolutionReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = FormationEvolutionReportWire::deserialize(deserializer)?;
        Self::new(
            wire.accepted_surface_substeps,
            wire.integrated_duration_years,
            wire.current_rates,
            wire.dense_state_bytes,
        )
        .map_err(D::Error::custom)
    }
}

/// Retained global and five-source sediment mass closure.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SedimentBudgetReport {
    produced_mass_kg_per_year: f64,
    land_lake_deposition_kg_per_year: f64,
    shelf_deposition_kg_per_year: f64,
    deep_ocean_export_kg_per_year: f64,
    in_transit_kg_per_year: f64,
    produced_by_source_kg_per_year: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    accounted_by_source_kg_per_year: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    global_relative_error: f64,
    provenance_relative_errors: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SedimentBudgetReportWire {
    produced_mass_kg_per_year: f64,
    land_lake_deposition_kg_per_year: f64,
    shelf_deposition_kg_per_year: f64,
    deep_ocean_export_kg_per_year: f64,
    in_transit_kg_per_year: f64,
    produced_by_source_kg_per_year: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    accounted_by_source_kg_per_year: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    global_relative_error: f64,
    provenance_relative_errors: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
}

impl SedimentBudgetReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        produced_mass_kg_per_year: f64,
        land_lake_deposition_kg_per_year: f64,
        shelf_deposition_kg_per_year: f64,
        deep_ocean_export_kg_per_year: f64,
        in_transit_kg_per_year: f64,
        produced_by_source_kg_per_year: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
        accounted_by_source_kg_per_year: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    ) -> Result<Self, SurfaceFormationValidationError> {
        let accounted = land_lake_deposition_kg_per_year
            + shelf_deposition_kg_per_year
            + deep_ocean_export_kg_per_year
            + in_transit_kg_per_year;
        let global_relative_error = relative_error(produced_mass_kg_per_year, accounted);
        let provenance_relative_errors = std::array::from_fn(|index| {
            relative_error(
                produced_by_source_kg_per_year[index],
                accounted_by_source_kg_per_year[index],
            )
        });
        let report = Self {
            produced_mass_kg_per_year,
            land_lake_deposition_kg_per_year,
            shelf_deposition_kg_per_year,
            deep_ocean_export_kg_per_year,
            in_transit_kg_per_year,
            produced_by_source_kg_per_year,
            accounted_by_source_kg_per_year,
            global_relative_error,
            provenance_relative_errors,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        for (field, value) in [
            ("produced_mass_kg_per_year", self.produced_mass_kg_per_year),
            (
                "land_lake_deposition_kg_per_year",
                self.land_lake_deposition_kg_per_year,
            ),
            (
                "shelf_deposition_kg_per_year",
                self.shelf_deposition_kg_per_year,
            ),
            (
                "deep_ocean_export_kg_per_year",
                self.deep_ocean_export_kg_per_year,
            ),
            ("in_transit_kg_per_year", self.in_transit_kg_per_year),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(SurfaceFormationValidationError::InvalidValue {
                    field,
                    found: value,
                });
            }
        }
        for (field, values) in [
            (
                "produced_by_source_kg_per_year",
                self.produced_by_source_kg_per_year,
            ),
            (
                "accounted_by_source_kg_per_year",
                self.accounted_by_source_kg_per_year,
            ),
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
        let accounted = self.accounted_mass_kg_per_year();
        let expected_global = relative_error(self.produced_mass_kg_per_year, accounted);
        if self.global_relative_error.to_bits() != expected_global.to_bits()
            || expected_global > SEDIMENT_BUDGET_RELATIVE_ERROR_MAX
        {
            return Err(SurfaceFormationValidationError::SedimentBudgetNotClosed {
                field: "global",
                relative_error: expected_global,
                maximum: SEDIMENT_BUDGET_RELATIVE_ERROR_MAX,
            });
        }
        let produced_source_sum = self.produced_by_source_kg_per_year.iter().sum::<f64>();
        let accounted_source_sum = self.accounted_by_source_kg_per_year.iter().sum::<f64>();
        for (field, left, right) in [
            (
                "produced_source_total",
                produced_source_sum,
                self.produced_mass_kg_per_year,
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
                self.produced_by_source_kg_per_year[index],
                self.accounted_by_source_kg_per_year[index],
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

    pub const fn produced_mass_kg_per_year(&self) -> f64 {
        self.produced_mass_kg_per_year
    }

    pub fn accounted_mass_kg_per_year(&self) -> f64 {
        self.land_lake_deposition_kg_per_year
            + self.shelf_deposition_kg_per_year
            + self.deep_ocean_export_kg_per_year
            + self.in_transit_kg_per_year
    }

    pub const fn land_lake_deposition_kg_per_year(&self) -> f64 {
        self.land_lake_deposition_kg_per_year
    }

    pub const fn shelf_deposition_kg_per_year(&self) -> f64 {
        self.shelf_deposition_kg_per_year
    }

    pub const fn deep_ocean_export_kg_per_year(&self) -> f64 {
        self.deep_ocean_export_kg_per_year
    }

    pub const fn in_transit_kg_per_year(&self) -> f64 {
        self.in_transit_kg_per_year
    }

    pub const fn produced_by_source_kg_per_year(&self) -> &[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT] {
        &self.produced_by_source_kg_per_year
    }

    pub const fn accounted_by_source_kg_per_year(
        &self,
    ) -> &[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT] {
        &self.accounted_by_source_kg_per_year
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
            wire.produced_mass_kg_per_year,
            wire.land_lake_deposition_kg_per_year,
            wire.shelf_deposition_kg_per_year,
            wire.deep_ocean_export_kg_per_year,
            wire.in_transit_kg_per_year,
            wire.produced_by_source_kg_per_year,
            wire.accounted_by_source_kg_per_year,
        )
        .map_err(D::Error::custom)?;
        if report.global_relative_error.to_bits() != wire.global_relative_error.to_bits()
            || report.provenance_relative_errors.map(f64::to_bits)
                != wire.provenance_relative_errors.map(f64::to_bits)
        {
            return Err(D::Error::custom(
                SurfaceFormationValidationError::DerivedFieldMismatch {
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
    process_rates: &FormationProcessRates,
    hydrology: &SphericalHydrologySnapshot,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.surface-formation-state.v3\0");
    hasher.update(&terrain_fields.fingerprint());
    hasher.update(b"process-rates-json-v1\0");
    serde_json::to_writer(Blake3Writer(&mut hasher), process_rates)
        .expect("validated process rates always serialize to canonical JSON");
    hasher.update(b"hydrology-json-v2\0");
    serde_json::to_writer(Blake3Writer(&mut hasher), hydrology)
        .expect("validated hydrology always serializes to canonical JSON");
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
    process_rates: FormationProcessRates,
    hydrology: SphericalHydrologySnapshot,
    evolution_report: FormationEvolutionReport,
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
    process_rates: FormationProcessRates,
    hydrology: SphericalHydrologySnapshot,
    evolution_report: FormationEvolutionReport,
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
        process_rates: FormationProcessRates,
        hydrology: SphericalHydrologySnapshot,
        evolution_report: FormationEvolutionReport,
        sediment_budget_report: SedimentBudgetReport,
        capabilities: SurfaceFormationCapabilitySet,
    ) -> Result<Self, SurfaceFormationValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            checkpoint,
            terrain_fields,
            process_rates,
            hydrology,
            evolution_report,
            sediment_budget_report,
            capabilities,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), SurfaceFormationValidationError> {
        if self.schema_version != NATURAL_SURFACE_FORMATION_SCHEMA_V5 {
            return Err(SurfaceFormationValidationError::UnsupportedSchema {
                object: "natural_surface_formation_snapshot",
                found: self.schema_version,
                supported: NATURAL_SURFACE_FORMATION_SCHEMA_V5,
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
        self.process_rates.validate()?;
        self.hydrology.validate().map_err(|error| {
            SurfaceFormationValidationError::InvalidNested {
                role: "hydrology",
                reason: error.to_string(),
            }
        })?;
        self.evolution_report.validate()?;
        self.sediment_budget_report.validate()?;
        self.capabilities.validate()?;

        let residual = self.evolution_report.current_rates();
        let expected_stock_ratio = formation_relative_flux_imbalance(
            residual.sediment_stock_change_kg_per_year().abs(),
            self.sediment_budget_report.produced_mass_kg_per_year(),
        );
        if residual.sediment_stock_change_ratio().to_bits() != expected_stock_ratio.to_bits() {
            return Err(SurfaceFormationValidationError::DerivedFieldMismatch {
                field: "sediment_stock_change_ratio",
            });
        }

        if self.checkpoint.surface_ref() != self.surface_ref {
            return Err(
                SurfaceFormationValidationError::CheckpointIdentityMismatch {
                    field: "surface_ref",
                },
            );
        }
        for (role, found) in [("hydrology", self.hydrology.surface_ref())] {
            if found != self.surface_ref {
                return Err(SurfaceFormationValidationError::NestedSurfaceMismatch {
                    role,
                    found,
                    expected: self.surface_ref,
                });
            }
        }
        if self.terrain_fields.current_elevation_m().len() != self.surface_ref.cell_count() as usize
        {
            return Err(SurfaceFormationValidationError::FieldLengthMismatch {
                field: "terrain_fields",
                found: self.terrain_fields.current_elevation_m().len(),
                expected: self.surface_ref.cell_count() as usize,
            });
        }
        if self.process_rates.len() != self.surface_ref.cell_count() as usize {
            return Err(SurfaceFormationValidationError::FieldLengthMismatch {
                field: "process_rates",
                found: self.process_rates.len(),
                expected: self.surface_ref.cell_count() as usize,
            });
        }
        if self.terrain_fields.surface_water_geometry().surface_ref() != self.surface_ref {
            return Err(SurfaceFormationValidationError::NestedSurfaceMismatch {
                role: "terrain_fields",
                found: self.terrain_fields.surface_water_geometry().surface_ref(),
                expected: self.surface_ref,
            });
        }
        if self.capabilities != SurfaceFormationCapabilitySet::p5() {
            return Err(SurfaceFormationValidationError::CapabilityProfileMismatch);
        }
        let expected_state = surface_formation_state_fingerprint(
            &self.terrain_fields,
            &self.process_rates,
            &self.hydrology,
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
        self.terrain_fields.validate_against_surface(surface)?;
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

    pub const fn process_rates(&self) -> &FormationProcessRates {
        &self.process_rates
    }

    pub const fn hydrology(&self) -> &SphericalHydrologySnapshot {
        &self.hydrology
    }

    pub const fn evolution_report(&self) -> &FormationEvolutionReport {
        &self.evolution_report
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
            wire.process_rates,
            wire.hydrology,
            wire.evolution_report,
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
    #[error("invalid surface-water geometry: {0}")]
    InvalidSurfaceWaterGeometry(#[from] SurfaceWaterGeometryValidationError),
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
    #[error("accepted surface substep count must be positive, found {found}")]
    InvalidAcceptedSurfaceSubsteps { found: u32 },
    #[error("integrated surface duration is {found} years; expected {expected}")]
    InvalidIntegratedDuration { found: f64, expected: f64 },
    #[error("checkpoint identity field {field} does not match authoritative input")]
    CheckpointIdentityMismatch { field: &'static str },
    #[error("capability inventory has {found} entries, expected {expected}")]
    CapabilityInventoryMismatch { found: usize, expected: usize },
    #[error("capability inventory is not canonical at index {index}")]
    NonCanonicalCapability { index: usize },
    #[error("surface-formation derived field is inconsistent in {field}")]
    DerivedFieldMismatch { field: &'static str },
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

fn deserialize_formation_capabilities<'de, D>(
    deserializer: D,
) -> Result<Vec<SurfaceFormationCapabilityStatus>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, 8>(deserializer)
}
