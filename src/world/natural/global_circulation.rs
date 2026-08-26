use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{
    MonthlyScalarField, MonthlyVector3Field, NaturalQualityProfile, CLIMATE_MONTH_COUNT,
    CLIMATOLOGICAL_YEAR_SECONDS, MEAN_SOLAR_DAY_SECONDS,
};
use crate::world::serde_bounded::deserialize_bounded_vec;
use crate::world::spatial::{
    ConservativeSurfaceMap, ConservativeSurfaceMapError, SphericalSurfaceSnapshot,
    SurfaceGeometryKind, SurfaceRef,
};
use crate::world::{CellId, MAX_SPHERICAL_CELL_COUNT};

const MAX_GLOBAL_CIRCULATION_CELLS: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const WATER_VAPOR_TO_DRY_AIR_MOLAR_MASS_RATIO: f64 = 0.622;
const BOLTON_SATURATION_REFERENCE_VAPOR_PRESSURE_PA: f64 = 611.2;
const BOLTON_SATURATION_EXPONENT_COEFFICIENT: f64 = 17.67;
const BOLTON_DEWPOINT_OFFSET_C: f64 = 243.5;
const BOLTON_LCL_TEMPERATURE_OFFSET_K: f64 = 56.0;
const BOLTON_LCL_LOG_COEFFICIENT_K: f64 = 800.0;

fn deserialize_global_circulation_scalars<'de, D>(deserializer: D) -> Result<Vec<f32>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, MAX_GLOBAL_CIRCULATION_CELLS>(deserializer)
}

/// The first strict schema for the reconstructable climate work domain.
pub const CLIMATE_WORK_DOMAIN_SCHEMA_V1: u16 = 1;
/// The physical-budget layered atmosphere-ocean climatology schema.
pub const GLOBAL_CIRCULATION_SCHEMA_V2: u16 = 2;
/// The first fixed-layout schema.
pub const CLIMATE_LAYER_LAYOUT_SCHEMA_V1: u16 = 1;
/// The forcing-phase continuation checkpoint identity schema.
pub const CLIMATE_CHECKPOINT_SCHEMA_V2: u16 = 2;
/// Maximum accepted radial component after publishing an `f32` tangent vector.
pub const GLOBAL_CIRCULATION_TANGENCY_TOLERANCE_M_S: f64 = 1.0e-4;
/// Maximum solver-reported relative mass, volume, moisture, or exchange error.
pub const GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX: f64 = 1.0e-6;
/// Energy integrates more source terms and uses a separately declared bound.
pub const GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX: f64 = 1.0e-5;
/// Maximum final-cycle mismatch between globally integrated evaporation and
/// precipitation. This is a periodic water-budget closure, not an Earth-like
/// precipitation target.
pub const GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX: f64 = 0.05;
/// Maximum absolute final-cycle net TOA radiative flux.
///
/// The `10 W/m2` structural gate rejects a climatology that is still rapidly
/// heating or cooling while remaining independent of an authored world's
/// Earth-likeness. The tighter CERES comparison belongs to quality evidence.
pub const GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2: f64 = 10.0;
/// Public convergence threshold; generation uses a stricter 0.24 guard.
pub const GLOBAL_CIRCULATION_FORMATION_RESIDUAL_MAX: f64 = 0.25;
/// Absolute public ceiling across Draft/Standard/High formation cycles.
pub const GLOBAL_CIRCULATION_FORMATION_CYCLES_MAX: u16 = 12;
/// SI integration time advanced for one climatological forcing phase.
///
/// This is a numerical stability choice, not the duration of a calendar month.
/// The V2 time contract records it separately from the twelve forcing phases.
/// Its value is the measured stable production step selected by the P4
/// integrator comparison recorded in
/// `2026-08-17-global-atmosphere-ocean-p4-integrator-selection.md`.
pub const GLOBAL_CIRCULATION_MACRO_STEP_SECONDS: f64 = 7_200.0;
/// U.S. Standard Atmosphere 1976 tropospheric environmental lapse rate.
///
/// P4 applies this only to the overlap-weighted emergent-land elevation in
/// its idealized lower-boundary forcing; it is not a resolved moist lapse
/// rate or a claim about every generated atmosphere.
pub const CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M: f64 = 0.0065;
/// Fixed sea-level pressure used by P4's single lower-atmosphere humidity
/// closure. P4 does not resolve pressure-dependent saturation within the
/// lower layer, so the limitation is explicit rather than inferred from
/// layer thickness. The value is the ISO 2533:1975 standard-atmosphere
/// sea-level pressure.
pub const P4_LOWER_LAYER_REFERENCE_PRESSURE_PA: f64 = 101_325.0;
/// Dry-air reference density shared by the P4 layout and surface fluxes.
///
/// This is the ISO 2533:1975 standard-atmosphere sea-level value. A fixed
/// density is consistent with P4's incompressible layer model; density-varying
/// moist thermodynamics remain outside this milestone.
pub const P4_REFERENCE_AIR_DENSITY_KG_M3: f64 = 1.225;
/// Standard dry-air specific heat used by every P4 atmospheric slab.
///
/// Adopted from the constants table accompanying Wallace & Hobbs (2006),
/// *Atmospheric Science: An Introductory Survey*, second edition.
pub const P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K: f64 = 1_004.0;
/// Conventional standard gravity used by P4 dynamics and dry parcel lifting.
///
/// This is the conventional value adopted by the 3rd CGPM (1901), Declaration
/// 2, DOI `10.59161/CGPM1901DECL2E`.
pub const STANDARD_GRAVITY_M_S2: f64 = 9.806_65;
/// Neutral bulk moisture-transfer coefficient over open water.
///
/// Large & Pond (1982), DOI
/// `10.1175/1520-0485(1982)012<0464:SALHFM>2.0.CO;2`, report `1.15e-3`
/// from dissipation measurements. P4 intentionally adds no unmeasured
/// minimum-wind or gustiness term.
pub const BULK_MOISTURE_TRANSFER_COEFFICIENT: f64 = 1.15e-3;
/// Reference near-surface relative humidity for forcing initialization.
///
/// Manabe & Wetherald (1967), DOI
/// `10.1175/1520-0469(1967)024<0241:TEOTAW>2.0.CO;2`, prescribe `0.77` at
/// the surface. This initializes P4; it is not a relaxation target.
pub const REFERENCE_SURFACE_RELATIVE_HUMIDITY: f64 = 0.77;
/// Constant latent heat used by P4's water-vapor phase-change ledger.
///
/// Frierson, Held & Zurita-Gotor (2006), DOI `10.1175/JAS3753.1`, use the
/// fixed `2.5 MJ/kg` reference in the idealized moist-GCM equations adopted by
/// P4. Temperature-dependent latent heat is deferred until thermodynamic state
/// complexity can support it without adding an orphaned approximation.
pub const WATER_VAPORIZATION_LATENT_HEAT_J_KG: f64 = 2.5e6;
/// GPCP V3.2 global annual-mean precipitation reference.
///
/// This adopts the annual global mean reported by Huffman et al. (2023), DOI
/// `10.1175/JCLI-D-23-0123.1`. It is Earth-default evidence, not a
/// player-world gate.
pub const EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY: f64 = 2.81;
/// Relative evidence envelope around the GPCP global precipitation mean.
///
/// This follows the multi-product global-mean spread synthesized by Adler et
/// al. (2017), DOI `10.1007/s10712-017-9416-4`, and applies only to the frozen
/// Earth-default corpus.
pub const EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE: f64 = 0.07;
/// Lower global latent-heat-flux evidence bound from Wild et al. (2015).
pub const WILD_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2: f64 = 70.0;
/// Upper global latent-heat-flux evidence bound from Wild et al. (2015).
///
/// Wild et al., DOI `10.1007/s00382-014-2430-z`, derive the adopted evidence
/// interval from water- and surface-energy-budget constraints.
pub const WILD_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2: f64 = 85.0;
/// Lower global latent-heat-flux evidence bound from Stephens et al. (2012).
pub const STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MIN_W_M2: f64 = 78.0;
/// Upper global latent-heat-flux evidence bound from Stephens et al. (2012).
///
/// The adopted interval follows Stephens et al., DOI `10.1038/ngeo1580`.
pub const STEPHENS_GLOBAL_LATENT_HEAT_FLUX_MAX_W_M2: f64 = 98.0;
/// Structural mass-fraction upper bound shared by legacy and layered humidity.
///
/// Specific humidity is water-vapor mass divided by total moist-air mass, so
/// `1` is the definition-derived ceiling rather than an Earth calibration.
pub const P4_MAX_SPECIFIC_HUMIDITY_KG_KG: f64 = 1.0;
/// Grid-mean relative-humidity threshold for unresolved large-scale cloud
/// condensation in the coarse P4 lower atmosphere.
///
/// The `0.9` lower-troposphere threshold follows the intermediate-complexity
/// SPEEDY formulation (Molteni 2003, DOI `10.1007/s00382-002-0268-2`). It is
/// distinct from both physical saturation and the initialization humidity.
pub const P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY: f64 = 0.9;
/// E-folding time for unresolved grid-mean large-scale condensation.
///
/// SPEEDY uses four hours for this coarse-grid closure. P4 integrates the
/// relaxation analytically over its physical step, so it cannot overshoot its
/// relative-humidity threshold when the step size changes.
pub const P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS: f64 = 4.0 * 3_600.0;
/// Broadband open-ocean albedo used by the idealized P4 lower boundary.
///
/// Payne (1972), DOI `10.1175/1520-0469(1972)029<0959:AOTSS>2.0.CO;2`,
/// measured `0.061 +/- 0.005` under heavily overcast skies. P4 rounds this
/// to `0.06` because it has no solar-angle-dependent ocean BRDF.
pub const P4_OPEN_OCEAN_SURFACE_ALBEDO: f64 = 0.06;
/// Snow-free land increment above the P4 open-ocean albedo.
///
/// The resulting full-land value is `0.22`. It is the frozen V1 aggregate
/// prior retained by the 17-seed calibration, not a universal vegetation
/// observation. Operational land-surface schemes commonly use roughly
/// `0.20` for crops and grasslands (Masson et al. 2003, DOI
/// `10.1175/1520-0442(2003)016<1261:AGDOLS>2.0.CO;2`).
pub const P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT: f64 = 0.16;
/// Maximum highland brightening above the snow-free P4 land prior.
///
/// Full brightened land therefore reaches `0.57`, within the MODIS
/// snow-covered ecosystem climatology reported by Moody et al. (2007), DOI
/// `10.1016/j.rse.2007.07.002`. This is a static highland proxy because P4
/// does not resolve snow mass, aging, impurities, clouds, or solar angle.
pub const P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT: f64 = 0.35;
/// Start of the frozen V1 geometric highland-brightening ramp in metres.
///
/// This authored terrain proxy is retained to preserve the measured P4
/// calibration corpus. It is not a physical snowline or an evidence gate;
/// snow accumulation and melt remain outside the P4 capability boundary.
pub const P4_HIGHLAND_ALBEDO_RAMP_ONSET_M: f64 = 1_500.0;
/// Elevation span of the frozen V1 geometric highland-brightening ramp.
///
/// Together with `P4_HIGHLAND_ALBEDO_RAMP_ONSET_M`, it reaches full
/// brightening at 5 km. The limitations documented on the onset apply here.
pub const P4_HIGHLAND_ALBEDO_RAMP_SPAN_M: f64 = 3_500.0;
/// IAU 2015 Resolution B3 nominal total solar irradiance at 1 au.
pub const EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2: f64 = 1_361.0;
/// CERES EBAF Ed4 global-mean incoming shortwave flux (Loeb et al. 2018).
pub const CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2: f64 = 340.0;
/// CERES EBAF Ed4 global-mean reflected shortwave flux (Loeb et al. 2018).
pub const CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2: f64 = 99.1;
/// CERES EBAF Ed4 global-mean outgoing longwave flux (Loeb et al. 2018).
pub const CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2: f64 = 240.0;
/// CERES global-mean absorbed shortwave derived from incoming minus reflected SW.
pub const CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2: f64 =
    CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2
        - CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2;
/// CERES global-mean TOA net radiation derived as ASR minus OLR.
pub const CERES_EBAF_TOA_NET_RADIATION_GLOBAL_MEAN_W_M2: f64 =
    CERES_EBAF_ABSORBED_SHORTWAVE_GLOBAL_MEAN_W_M2 - CERES_EBAF_OUTGOING_LONGWAVE_GLOBAL_MEAN_W_M2;
/// CERES EBAF Ed4 surface-up longwave flux (Kato et al. 2018).
pub const CERES_EBAF_SURFACE_UP_LONGWAVE_GLOBAL_MEAN_W_M2: f64 = 398.3;
/// Earth planetary albedo derived from the two CERES TOA shortwave fluxes.
pub const EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN: f64 =
    CERES_EBAF_REFLECTED_SHORTWAVE_GLOBAL_MEAN_W_M2
        / CERES_EBAF_INCOMING_SHORTWAVE_GLOBAL_MEAN_W_M2;
/// Area-weighted mean P4 surface albedo measured over the frozen 17-seed corpus.
///
/// The production probe and derivation are recorded in §2.2 of
/// `2026-08-23-p4-physical-budget-correction-design.md`.
pub const EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN: f64 = 0.094_949_501_628_588_96;
/// Unresolved atmospheric shortwave reflectance calibrated to CERES albedo.
///
/// Derived as `(planetary - surface) / (1 - surface)` from the two constants
/// above; it is not fitted to a generated temperature or precipitation field.
pub const EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE: f64 = (EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN
    - EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN)
    / (1.0 - EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN);
/// Stefan–Boltzmann constant from the 2018 CODATA/SI exact-constant relation.
pub const STEFAN_BOLTZMANN_CONSTANT_W_M2_K4: f64 = 5.670_374_419e-8;
/// Gray greenhouse temperature offset derived from CERES surface-up LW and ASR.
///
/// `34.19751176932721 K = (398.3/sigma)^0.25 - (240.9/sigma)^0.25` using
/// Loeb et al. (2018), Kato et al. (2018), and the CODATA constant above.
pub const EARTH_GRAY_GREENHOUSE_OFFSET_K: f64 = 34.197_511_769_327_21;
/// Structural serialization ceiling for nonnegative radiative flux fields.
///
/// Twice the IAU nominal irradiance leaves room for transient OLR while
/// rejecting corrupt infinities and implausible payloads; it is not a quality
/// target or an Earth-climate tuning coefficient.
pub const GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2: f64 =
    2.0 * EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2;
/// Locked dense-owner memory budget for the High C2 product.
pub const GLOBAL_CIRCULATION_DENSE_STATE_BYTES_MAX: u64 = 512 * 1024 * 1024;

/// Fingerprints every numeric fact consumed by the P4 moist-thermodynamic
/// closures, including the private Bolton/LCL coefficients.
///
/// Keeping this identity beside the formulas prevents a coefficient change
/// from bypassing the generator's equation identity merely because the
/// coefficient has the minimum private visibility.
pub(crate) fn p4_thermodynamic_constants_fingerprint() -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.p4-thermodynamic-constants.v1\0");
    for value in [
        WATER_VAPOR_TO_DRY_AIR_MOLAR_MASS_RATIO,
        BOLTON_SATURATION_REFERENCE_VAPOR_PRESSURE_PA,
        BOLTON_SATURATION_EXPONENT_COEFFICIENT,
        BOLTON_DEWPOINT_OFFSET_C,
        BOLTON_LCL_TEMPERATURE_OFFSET_K,
        BOLTON_LCL_LOG_COEFFICIENT_K,
        P4_LOWER_LAYER_REFERENCE_PRESSURE_PA,
        P4_REFERENCE_AIR_DENSITY_KG_M3,
        P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K,
        STANDARD_GRAVITY_M_S2,
        BULK_MOISTURE_TRANSFER_COEFFICIENT,
        WATER_VAPORIZATION_LATENT_HEAT_J_KG,
        P4_MAX_SPECIFIC_HUMIDITY_KG_KG,
        P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY,
        P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS,
    ] {
        hasher.update(&value.to_bits().to_le_bytes());
    }
    *hasher.finalize().as_bytes()
}

/// Combines the unresolved atmosphere and resolved surface without duplicating
/// the CERES calibration formula in generators, tests, quality, or UI code.
pub fn planetary_albedo_from_surface(surface_albedo: f64) -> f64 {
    EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE
        + (1.0 - EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE) * surface_albedo.clamp(0.0, 1.0)
}

/// Converts P4's water-equivalent evaporation rate to latent heat flux.
///
/// In P4, `1 mm` water equivalent is `1 kg/m2`; dividing the fixed latent
/// energy by the exact SI day makes this the only mm/day-to-W/m2 conversion
/// used by evidence and presentation.
pub fn latent_heat_flux_w_m2_from_evaporation_mm_day(evaporation_mm_day: f64) -> f64 {
    evaporation_mm_day * WATER_VAPORIZATION_LATENT_HEAT_J_KG / MEAN_SOLAR_DAY_SECONDS
}

/// Reduces twelve equal-duration climatological forcing phases to their mean.
///
/// P4 publishes monthly phase means rather than a weather trajectory. The
/// frozen time contract gives every phase the same climatological duration,
/// so the annual mean is the arithmetic mean of the twelve values.
pub fn climatological_monthly_mean(monthly: &[f32; CLIMATE_MONTH_COUNT]) -> f32 {
    (monthly.iter().map(|value| f64::from(*value)).sum::<f64>() / CLIMATE_MONTH_COUNT as f64) as f32
}

/// Expands twelve mean daily water-equivalent rates to one climatological total.
///
/// The result remains `f64` so a renderer can reject an unrepresentable `f32`
/// payload rather than silently clamp a physically published rate.
pub fn climatological_annual_total_mm(monthly_mm_day: &[f32; CLIMATE_MONTH_COUNT]) -> f64 {
    monthly_mm_day
        .iter()
        .map(|value| f64::from(*value))
        .sum::<f64>()
        / CLIMATE_MONTH_COUNT as f64
        * CLIMATOLOGICAL_YEAR_SECONDS
        / MEAN_SOLAR_DAY_SECONDS
}

/// Returns top-of-atmosphere absorbed shortwave power for one daily-mean solar
/// geometry fraction and resolved surface albedo.
pub fn absorbed_shortwave_w_m2(daily_mean_insolation_fraction: f64, surface_albedo: f64) -> f64 {
    EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2
        * daily_mean_insolation_fraction.max(0.0)
        * (1.0 - planetary_albedo_from_surface(surface_albedo))
}

/// Gray-body equilibrium surface temperature implied by absorbed shortwave.
pub fn gray_equilibrium_surface_temperature_c(absorbed_shortwave_w_m2: f64) -> f64 {
    (absorbed_shortwave_w_m2.max(0.0) / STEFAN_BOLTZMANN_CONSTANT_W_M2_K4).powf(0.25)
        + EARTH_GRAY_GREENHOUSE_OFFSET_K
        - 273.15
}

/// Linearized gray outgoing longwave around the local radiative-equilibrium
/// target. The intercept is the same ASR that constructed the target, so
/// authored lapse-rate offsets do not create a fictitious TOA source.
pub fn linearized_outgoing_longwave_w_m2(
    absorbed_shortwave_w_m2: f64,
    equilibrium_surface_temperature_c: f64,
    resolved_surface_temperature_c: f64,
) -> f64 {
    let equilibrium_emission_temperature_k =
        (equilibrium_surface_temperature_c + 273.15 - EARTH_GRAY_GREENHOUSE_OFFSET_K).max(1.0);
    let longwave_slope_w_m2_k =
        4.0 * STEFAN_BOLTZMANN_CONSTANT_W_M2_K4 * equilibrium_emission_temperature_k.powi(3);
    (absorbed_shortwave_w_m2
        + longwave_slope_w_m2_k
            * (resolved_surface_temperature_c - equilibrium_surface_temperature_c))
        .max(0.0)
}

/// Bolton (1980) saturation specific humidity at the fixed P4 lower-layer
/// reference pressure, in kg/kg.
///
/// Bolton's Eq. 10 gives saturation vapor pressure as
/// `611.2 exp(17.67 T / (T + 243.5)) Pa`; the denominator below converts
/// vapor pressure to specific humidity rather than mixing ratio.
pub fn saturation_specific_humidity_kg_kg(temperature_c: f64) -> f64 {
    saturation_specific_humidity_and_temperature_derivative(temperature_c).0
}

fn saturation_specific_humidity_and_temperature_derivative(temperature_c: f64) -> (f64, f64) {
    let saturation_vapor_pressure_pa = BOLTON_SATURATION_REFERENCE_VAPOR_PRESSURE_PA
        * (BOLTON_SATURATION_EXPONENT_COEFFICIENT * temperature_c
            / (temperature_c + BOLTON_DEWPOINT_OFFSET_C))
            .exp();
    let denominator = P4_LOWER_LAYER_REFERENCE_PRESSURE_PA
        - (1.0 - WATER_VAPOR_TO_DRY_AIR_MOLAR_MASS_RATIO) * saturation_vapor_pressure_pa;
    let raw_humidity =
        WATER_VAPOR_TO_DRY_AIR_MOLAR_MASS_RATIO * saturation_vapor_pressure_pa / denominator;
    let humidity = raw_humidity.clamp(0.0, P4_MAX_SPECIFIC_HUMIDITY_KG_KG);
    let derivative = if humidity != raw_humidity || !humidity.is_finite() {
        0.0
    } else {
        let vapor_pressure_temperature_derivative = saturation_vapor_pressure_pa
            * BOLTON_SATURATION_EXPONENT_COEFFICIENT
            * BOLTON_DEWPOINT_OFFSET_C
            / (temperature_c + BOLTON_DEWPOINT_OFFSET_C).powi(2);
        WATER_VAPOR_TO_DRY_AIR_MOLAR_MASS_RATIO
            * P4_LOWER_LAYER_REFERENCE_PRESSURE_PA
            * vapor_pressure_temperature_derivative
            / denominator.powi(2)
    };
    (humidity, derivative)
}

/// Diagnoses neutral near-surface air humidity from P4's deep lower slab.
///
/// Large–Pond bulk transfer is a near-surface neutral closure, whereas P4's
/// prognostic lower atmosphere has the deep slab extent declared by
/// `ClimateLayerLayout`. Directly subtracting that cold slab's specific
/// humidity from saturation at the warmer ocean surface spuriously counts the
/// slab's vertical temperature contrast as an air–sea humidity deficit. This
/// zero-parameter closure preserves the slab's resolved relative humidity
/// while evaluating it at the surface temperature, consistent with P4's
/// existing Manabe–Wetherald relative-humidity state.
pub fn neutral_surface_air_specific_humidity_kg_kg(
    surface_temperature_c: f64,
    lower_temperature_c: f64,
    lower_specific_humidity_kg_kg: f64,
) -> f64 {
    let lower_saturation = saturation_specific_humidity_kg_kg(lower_temperature_c);
    let relative_humidity = if lower_saturation > 0.0 {
        (lower_specific_humidity_kg_kg / lower_saturation).clamp(0.0, 1.0)
    } else {
        0.0
    };
    relative_humidity * saturation_specific_humidity_kg_kg(surface_temperature_c)
}

/// Large–Pond neutral bulk evaporation from an explicitly wet surface.
pub fn bulk_surface_evaporation_kg_m2_s(
    surface_temperature_c: f64,
    lower_specific_humidity_kg_kg: f64,
    lower_wind_speed_m_s: f64,
    water_fraction: f64,
) -> f64 {
    P4_REFERENCE_AIR_DENSITY_KG_M3
        * BULK_MOISTURE_TRANSFER_COEFFICIENT
        * lower_wind_speed_m_s.max(0.0)
        * (saturation_specific_humidity_kg_kg(surface_temperature_c)
            - lower_specific_humidity_kg_kg)
            .max(0.0)
        * water_fraction.clamp(0.0, 1.0)
}

/// Smith raw-upslope condensation source in kg/m2/s.
pub fn raw_orographic_condensation_kg_m2_s(
    lower_specific_humidity_kg_kg: f64,
    upslope_velocity_m_s: f64,
) -> f64 {
    P4_REFERENCE_AIR_DENSITY_KG_M3
        * lower_specific_humidity_kg_kg.max(0.0)
        * upslope_velocity_m_s.max(0.0)
}

/// Integrates the raw Smith upslope source only after a parcel reaches its LCL.
///
/// Smith & Barstad (2004), DOI
/// `10.1175/1520-0469(2004)061<1377:ALTOOP>2.0.CO;2`, derive their linear
/// source for saturated or near-saturated flow. Bolton's Eq. 15 diagnoses the
/// lifting-condensation temperature from the resolved temperature and
/// humidity; dry-adiabatic lifting converts it to LCL height. The returned
/// source is the raw Smith rate multiplied by the fraction of one resolved-cell
/// terrain ascent above the LCL. The ascent follows the wind-aligned slope over
/// the cell's area-derived characteristic length, so the physical tendency is
/// continuous, time-step independent, and introduces no empirical
/// relative-humidity switch.
pub fn lcl_adjusted_orographic_condensation_kg_m2_s(
    specific_humidity_kg_kg: f64,
    temperature_c: f64,
    upslope_velocity_m_s: f64,
    horizontal_wind_speed_m_s: f64,
    resolved_cell_area_m2: f64,
) -> f64 {
    if !specific_humidity_kg_kg.is_finite()
        || !temperature_c.is_finite()
        || !upslope_velocity_m_s.is_finite()
        || !horizontal_wind_speed_m_s.is_finite()
        || !resolved_cell_area_m2.is_finite()
        || specific_humidity_kg_kg <= 0.0
        || upslope_velocity_m_s <= 0.0
        || horizontal_wind_speed_m_s <= 0.0
        || resolved_cell_area_m2 <= 0.0
    {
        return 0.0;
    }
    let saturation = saturation_specific_humidity_kg_kg(temperature_c);
    if saturation <= 0.0 {
        return 0.0;
    }
    let raw_source = raw_orographic_condensation_kg_m2_s(
        specific_humidity_kg_kg.min(saturation),
        upslope_velocity_m_s,
    );
    if specific_humidity_kg_kg >= saturation {
        return raw_source;
    }

    let humidity = specific_humidity_kg_kg.min(P4_MAX_SPECIFIC_HUMIDITY_KG_KG);
    let vapor_pressure_pa = humidity * P4_LOWER_LAYER_REFERENCE_PRESSURE_PA
        / (WATER_VAPOR_TO_DRY_AIR_MOLAR_MASS_RATIO
            + (1.0 - WATER_VAPOR_TO_DRY_AIR_MOLAR_MASS_RATIO) * humidity);
    let logarithmic_pressure_ratio =
        (vapor_pressure_pa / BOLTON_SATURATION_REFERENCE_VAPOR_PRESSURE_PA).ln();
    let dewpoint_denominator = BOLTON_SATURATION_EXPONENT_COEFFICIENT - logarithmic_pressure_ratio;
    if !logarithmic_pressure_ratio.is_finite() || dewpoint_denominator <= 0.0 {
        return 0.0;
    }
    let dewpoint_k =
        BOLTON_DEWPOINT_OFFSET_C * logarithmic_pressure_ratio / dewpoint_denominator + 273.15;
    let temperature_k = temperature_c + 273.15;
    if dewpoint_k <= BOLTON_LCL_TEMPERATURE_OFFSET_K || temperature_k <= 0.0 {
        return 0.0;
    }
    let lcl_denominator = 1.0 / (dewpoint_k - BOLTON_LCL_TEMPERATURE_OFFSET_K)
        + (temperature_k / dewpoint_k).ln() / BOLTON_LCL_LOG_COEFFICIENT_K;
    if !lcl_denominator.is_finite() || lcl_denominator <= 0.0 {
        return 0.0;
    }
    let lcl_temperature_k = 1.0 / lcl_denominator + BOLTON_LCL_TEMPERATURE_OFFSET_K;
    let lcl_height_m = (temperature_k - lcl_temperature_k).max(0.0)
        * P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K
        / STANDARD_GRAVITY_M_S2;
    let uplift_m = upslope_velocity_m_s / horizontal_wind_speed_m_s * resolved_cell_area_m2.sqrt();
    let saturated_path_fraction = ((uplift_m - lcl_height_m) / uplift_m).clamp(0.0, 1.0);
    raw_source * saturated_path_fraction
}

/// Coarse-grid large-scale condensation with moist-enthalpy-conserving
/// saturation adjustment.
///
/// Supersaturation is first projected to saturation along the local
/// `c_p T + L_v q` conservation curve. Excess humidity above the unresolved
/// cloud threshold then decays analytically on that same curve. Expressing the
/// relaxation in threshold-relative humidity makes the endpoint independent
/// of how a physical interval is partitioned into numerical steps, while the
/// matching latent-heat tendency supplies exactly the diagnosed warming. The
/// bracketed Newton solve follows the safeguarded Newton/bisection pattern of
/// Press et al. (2007), *Numerical Recipes*, third edition, section 9.4.
pub fn large_scale_condensation_kg_m2_s(
    specific_humidity_kg_kg: f64,
    temperature_c: f64,
    atmospheric_column_mass_kg_m2: f64,
    step_seconds: f64,
) -> f64 {
    if !specific_humidity_kg_kg.is_finite()
        || !temperature_c.is_finite()
        || !atmospheric_column_mass_kg_m2.is_finite()
        || !step_seconds.is_finite()
        || specific_humidity_kg_kg <= 0.0
        || atmospheric_column_mass_kg_m2 <= 0.0
        || step_seconds <= 0.0
    {
        return 0.0;
    }
    let humidity = specific_humidity_kg_kg.max(0.0);
    let saturation = saturation_specific_humidity_kg_kg(temperature_c);
    let (saturation_adjusted_humidity, saturation_adjusted_temperature) = if humidity > saturation {
        let adjusted =
            solve_moist_enthalpy_humidity_endpoint(humidity, temperature_c, humidity, 1.0, 0.0);
        (
            adjusted,
            moist_enthalpy_temperature_c(humidity, temperature_c, adjusted),
        )
    } else {
        (humidity, temperature_c)
    };
    let cloudy_excess = (saturation_adjusted_humidity
        - P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY
            * saturation_specific_humidity_kg_kg(saturation_adjusted_temperature))
    .max(0.0);
    if cloudy_excess == 0.0 {
        return 0.0;
    }
    let remaining_cloudy_excess =
        cloudy_excess * (-step_seconds / P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS).exp();
    let adjusted_humidity = solve_moist_enthalpy_humidity_endpoint(
        humidity,
        temperature_c,
        saturation_adjusted_humidity,
        P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY,
        remaining_cloudy_excess,
    );
    atmospheric_column_mass_kg_m2 * (humidity - adjusted_humidity).max(0.0) / step_seconds
}

fn moist_enthalpy_temperature_c(
    initial_humidity_kg_kg: f64,
    initial_temperature_c: f64,
    adjusted_humidity_kg_kg: f64,
) -> f64 {
    initial_temperature_c
        + WATER_VAPORIZATION_LATENT_HEAT_J_KG * (initial_humidity_kg_kg - adjusted_humidity_kg_kg)
            / P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K
}

fn solve_moist_enthalpy_humidity_endpoint(
    initial_humidity_kg_kg: f64,
    initial_temperature_c: f64,
    upper_humidity_kg_kg: f64,
    relative_humidity: f64,
    remaining_cloudy_excess_kg_kg: f64,
) -> f64 {
    let residual_and_derivative = |adjusted_humidity_kg_kg: f64| {
        let adjusted_temperature_c = moist_enthalpy_temperature_c(
            initial_humidity_kg_kg,
            initial_temperature_c,
            adjusted_humidity_kg_kg,
        );
        let (saturation, saturation_temperature_derivative) =
            saturation_specific_humidity_and_temperature_derivative(adjusted_temperature_c);
        (
            adjusted_humidity_kg_kg
                - relative_humidity * saturation
                - remaining_cloudy_excess_kg_kg,
            1.0 + relative_humidity * WATER_VAPORIZATION_LATENT_HEAT_J_KG
                / P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K
                * saturation_temperature_derivative,
        )
    };
    let mut lower = 0.0;
    let mut upper = upper_humidity_kg_kg;
    debug_assert!(residual_and_derivative(lower).0 <= 0.0);
    debug_assert!(residual_and_derivative(upper).0 >= 0.0);
    let mut candidate = upper;
    for _ in 0..f64::MANTISSA_DIGITS {
        let (residual, derivative) = residual_and_derivative(candidate);
        if residual == 0.0 {
            return candidate;
        }
        if residual < 0.0 {
            lower = candidate;
        } else {
            upper = candidate;
        }
        let midpoint = lower + 0.5 * (upper - lower);
        if midpoint == lower || midpoint == upper {
            return midpoint;
        }
        let newton = candidate - residual / derivative;
        let next = if newton > lower && newton < upper && newton != candidate {
            newton
        } else {
            midpoint
        };
        if next == candidate {
            return next;
        }
        candidate = next;
    }
    lower + 0.5 * (upper - lower)
}

/// Symmetric relative mismatch used by the production water-cycle gate and
/// every downstream quality/UI consumer.
pub fn water_cycle_relative_imbalance(
    evaporation_global_mean_mm_day: f64,
    precipitation_global_mean_mm_day: f64,
) -> f64 {
    (evaporation_global_mean_mm_day - precipitation_global_mean_mm_day).abs()
        / evaporation_global_mean_mm_day
            .abs()
            .max(precipitation_global_mean_mm_day.abs())
            .max(f64::MIN_POSITIVE)
}

pub(crate) const fn global_circulation_owner_inventory() -> (u64, u64, u64, u64, u64) {
    // Conservative simultaneous dense-owner upper bound:
    //
    // states (7): generation state/before/previous-cycle plus split advanced
    // and RK3 stage-two/stage-three/result during assignment;
    // tendencies (5): retained full diagnostic plus the maximum nested
    // tendency construction allowance used by full/fast evaluation;
    // derivatives (5): frozen slow plus RK3 first/second/third and the
    // combine return value;
    // vector temporaries (3): height gradient, Coriolis acceleration, and
    // thermal gradient in the full tendency role loop. The persistent
    // workspace vector is counted separately in `workspace_bytes`;
    // publication outputs (1): projected vectors are moved into
    // `Monthly*Field` and then into `GlobalCirculationFields` without a
    // second dense allocation.
    (7, 5, 5, 3, 1)
}

const fn global_circulation_dense_profile_inventory(
    profile: ClimateModelProfile,
) -> (u64, u64, u64, u64, u64, u64) {
    match profile {
        ClimateModelProfile::C1SingleLayerV1 => (2, 1, 0, 16, 16, 1),
        // C2 work has four vector fields plus fourteen monthly scalar fields;
        // thermocline depth is derived at publication. The static output is
        // surface albedo.
        ClimateModelProfile::C2LayeredV1 => (4, 2, 1, 26, 27, 1),
    }
}

pub(crate) fn global_circulation_tendency_cell_bytes(profile: ClimateModelProfile) -> u64 {
    let (active_layers, humidity_fields, reservoir_fields, _, _, _) =
        global_circulation_dense_profile_inventory(profile);
    let f32_bytes = std::mem::size_of::<f32>() as u64;
    let f64_bytes = std::mem::size_of::<f64>() as u64;
    let layer_cell_bytes = 2 * f32_bytes + std::mem::size_of::<[f32; 3]>() as u64;
    active_layers * layer_cell_bytes
        + (humidity_fields + reservoir_fields + 3) * f32_bytes
        // The retained external moisture and radiative ledgers preserve the
        // exact extensive contributions per cell in f64.
        + 2 * f64_bytes
}

/// Returns the mechanically-derived conservative peak dense-owner inventory
/// for one locked climate product configuration.
pub fn expected_global_circulation_dense_state_bytes(
    quality_profile: NaturalQualityProfile,
    profile: ClimateModelProfile,
    output_cells: u32,
) -> Option<u64> {
    let face_resolution = u64::from(quality_profile.climate_face_resolution());
    let climate_cells = 6_u64
        .checked_mul(face_resolution)?
        .checked_mul(face_resolution)?;
    let climate_edges = climate_cells.checked_mul(2)?;
    let output_cells = u64::from(output_cells);
    let months = CLIMATE_MONTH_COUNT as u64;
    let f32_bytes = std::mem::size_of::<f32>() as u64;
    let u32_bytes = std::mem::size_of::<u32>() as u64;
    let f64_bytes = std::mem::size_of::<f64>() as u64;
    let vector_f32_bytes = std::mem::size_of::<[f32; 3]>() as u64;
    let vector_f64_bytes = std::mem::size_of::<[f64; 3]>() as u64;
    let (
        active_layers,
        humidity_fields,
        reservoir_fields,
        work_components,
        monthly_output_components,
        static_output_components,
    ) = global_circulation_dense_profile_inventory(profile);
    let (state_owners, tendency_owners, derivative_owners, vector_temps, publication_output_owners) =
        global_circulation_owner_inventory();

    let layer_cell_bytes = 2 * f32_bytes + vector_f32_bytes;
    let state_cell_bytes = active_layers
        .checked_mul(layer_cell_bytes)?
        .checked_add((humidity_fields + reservoir_fields) * f32_bytes)?;
    let tendency_cell_bytes = global_circulation_tendency_cell_bytes(profile);
    let transport_cell_bytes = vector_f64_bytes
        .checked_add(7 * f64_bytes)?
        .checked_add(f32_bytes + u32_bytes)?;
    let workspace_cell_bytes = f32_bytes
        .checked_add(f64_bytes)?
        .checked_add(vector_f32_bytes)?
        .checked_add(transport_cell_bytes)?;
    let workspace_edge_bytes = f32_bytes + 2 * f64_bytes;
    let workspace_bytes = climate_cells
        .checked_mul(workspace_cell_bytes)?
        .checked_add(climate_edges.checked_mul(workspace_edge_bytes)?)?;
    let work_bytes = climate_cells
        .checked_mul(work_components)?
        .checked_mul(months)?
        .checked_mul(f32_bytes)?;
    let state_owner_bytes = climate_cells
        .checked_mul(state_cell_bytes)?
        .checked_mul(state_owners)?;
    let tendency_owner_bytes = climate_cells
        .checked_mul(tendency_cell_bytes)?
        .checked_mul(tendency_owners)?;
    let derivative_owner_bytes = climate_cells
        .checked_mul(state_cell_bytes)?
        .checked_mul(derivative_owners)?;
    let vector_temp_bytes = climate_cells
        .checked_mul(vector_f32_bytes)?
        .checked_mul(vector_temps)?;
    let formation_peak = work_bytes
        .checked_add(state_owner_bytes)?
        .checked_add(tendency_owner_bytes)?
        .checked_add(derivative_owner_bytes)?
        .checked_add(vector_temp_bytes)?
        .checked_add(workspace_bytes.checked_mul(2)?)?;

    let output_bytes = output_cells
        .checked_mul(
            monthly_output_components
                .checked_mul(months)?
                .checked_add(static_output_components)?,
        )?
        .checked_mul(f32_bytes)?;
    let remap_scratch = climate_cells
        .checked_mul(std::mem::size_of::<[f64; 2]>() as u64 + f64_bytes)?
        .checked_add(
            output_cells.checked_mul(std::mem::size_of::<[f64; 2]>() as u64 + f64_bytes)?,
        )?;
    let publication_peak = work_bytes
        .checked_add(output_bytes.checked_mul(publication_output_owners)?)?
        .checked_add(remap_scratch)?;
    Some(formation_peak.max(publication_peak))
}

/// Closed scientific layer configurations supported by the climate core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateModelProfile {
    C1SingleLayerV1,
    C2LayeredV1,
}

/// Stable semantic roles; numerical layer indices never escape the solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateLayerRole {
    LowerAtmosphere,
    UpperAtmosphere,
    OceanMixedLayer,
    OceanThermocline,
    DeepOceanReservoir,
}

/// One immutable member of a fixed climate profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateLayerSpec {
    role: ClimateLayerRole,
    dynamically_active: bool,
    reference_thickness_m: f64,
    density_kg_m3: f64,
    heat_capacity_j_kg_k: f64,
}

impl ClimateLayerSpec {
    pub const fn role(&self) -> ClimateLayerRole {
        self.role
    }

    pub const fn dynamically_active(&self) -> bool {
        self.dynamically_active
    }

    pub const fn reference_thickness_m(&self) -> f64 {
        self.reference_thickness_m
    }

    pub const fn density_kg_m3(&self) -> f64 {
        self.density_kg_m3
    }

    pub const fn heat_capacity_j_kg_k(&self) -> f64 {
        self.heat_capacity_j_kg_k
    }
}

/// One canonical pair-specific internal exchange in the locked model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateLayerExchangeSpec {
    first: ClimateLayerRole,
    second: ClimateLayerRole,
    heat_exchange_time_s: Option<f64>,
    momentum_exchange_time_s: Option<f64>,
    moisture_exchange_time_s: Option<f64>,
    water_only: bool,
}

impl ClimateLayerExchangeSpec {
    pub const fn first(&self) -> ClimateLayerRole {
        self.first
    }

    pub const fn second(&self) -> ClimateLayerRole {
        self.second
    }

    pub const fn heat_exchange_time_s(&self) -> Option<f64> {
        self.heat_exchange_time_s
    }

    pub const fn momentum_exchange_time_s(&self) -> Option<f64> {
        self.momentum_exchange_time_s
    }

    pub const fn moisture_exchange_time_s(&self) -> Option<f64> {
        self.moisture_exchange_time_s
    }

    pub const fn water_only(&self) -> bool {
        self.water_only
    }
}

/// The exact layer inventory and declared physical reference constants.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateLayerLayout {
    schema_version: u16,
    profile: ClimateModelProfile,
    layers: Vec<ClimateLayerSpec>,
    exchanges: Vec<ClimateLayerExchangeSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateLayerLayoutWire {
    schema_version: u16,
    profile: ClimateModelProfile,
    #[serde(deserialize_with = "deserialize_climate_layers")]
    layers: Vec<ClimateLayerSpec>,
    #[serde(deserialize_with = "deserialize_climate_exchanges")]
    exchanges: Vec<ClimateLayerExchangeSpec>,
}

fn deserialize_climate_exchanges<'de, D>(
    deserializer: D,
) -> Result<Vec<ClimateLayerExchangeSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, 4>(deserializer)
}

fn deserialize_climate_layers<'de, D>(deserializer: D) -> Result<Vec<ClimateLayerSpec>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, 5>(deserializer)
}

impl ClimateLayerLayout {
    /// Returns the only legal layout for a closed model profile.
    pub fn for_profile(profile: ClimateModelProfile) -> Self {
        let atmosphere = |role, thickness| ClimateLayerSpec {
            role,
            dynamically_active: true,
            reference_thickness_m: thickness,
            density_kg_m3: P4_REFERENCE_AIR_DENSITY_KG_M3,
            heat_capacity_j_kg_k: P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K,
        };
        let ocean = |role, thickness, active| ClimateLayerSpec {
            role,
            dynamically_active: active,
            reference_thickness_m: thickness,
            density_kg_m3: 1_025.0,
            heat_capacity_j_kg_k: 3_990.0,
        };
        let lower_mixed = ClimateLayerExchangeSpec {
            first: ClimateLayerRole::LowerAtmosphere,
            second: ClimateLayerRole::OceanMixedLayer,
            heat_exchange_time_s: Some(6.0 * 86_400.0),
            momentum_exchange_time_s: Some(6.0 * 86_400.0),
            moisture_exchange_time_s: None,
            water_only: true,
        };
        let (layers, exchanges) = match profile {
            ClimateModelProfile::C1SingleLayerV1 => (
                vec![
                    atmosphere(ClimateLayerRole::LowerAtmosphere, 8_000.0),
                    ocean(ClimateLayerRole::OceanMixedLayer, 100.0, true),
                ],
                vec![lower_mixed],
            ),
            ClimateModelProfile::C2LayeredV1 => (
                vec![
                    atmosphere(ClimateLayerRole::LowerAtmosphere, 6_000.0),
                    atmosphere(ClimateLayerRole::UpperAtmosphere, 4_000.0),
                    ocean(ClimateLayerRole::OceanMixedLayer, 100.0, true),
                    ocean(ClimateLayerRole::OceanThermocline, 900.0, true),
                    ocean(ClimateLayerRole::DeepOceanReservoir, 3_000.0, false),
                ],
                vec![
                    lower_mixed,
                    ClimateLayerExchangeSpec {
                        first: ClimateLayerRole::LowerAtmosphere,
                        second: ClimateLayerRole::UpperAtmosphere,
                        heat_exchange_time_s: Some(5.0 * 86_400.0),
                        momentum_exchange_time_s: Some(5.0 * 86_400.0),
                        moisture_exchange_time_s: Some(5.0 * 86_400.0),
                        water_only: false,
                    },
                    ClimateLayerExchangeSpec {
                        first: ClimateLayerRole::OceanMixedLayer,
                        second: ClimateLayerRole::OceanThermocline,
                        heat_exchange_time_s: Some(90.0 * 86_400.0),
                        momentum_exchange_time_s: Some(90.0 * 86_400.0),
                        moisture_exchange_time_s: None,
                        water_only: true,
                    },
                    ClimateLayerExchangeSpec {
                        first: ClimateLayerRole::OceanThermocline,
                        second: ClimateLayerRole::DeepOceanReservoir,
                        heat_exchange_time_s: Some(200.0 * 365.25 * 86_400.0),
                        momentum_exchange_time_s: None,
                        moisture_exchange_time_s: None,
                        water_only: true,
                    },
                ],
            ),
        };
        Self {
            schema_version: CLIMATE_LAYER_LAYOUT_SCHEMA_V1,
            profile,
            layers,
            exchanges,
        }
    }

    pub fn validate(&self) -> Result<(), ClimateLayerLayoutError> {
        if self.schema_version != CLIMATE_LAYER_LAYOUT_SCHEMA_V1 {
            return Err(ClimateLayerLayoutError::UnsupportedSchema {
                found: self.schema_version,
                supported: CLIMATE_LAYER_LAYOUT_SCHEMA_V1,
            });
        }
        let expected = Self::for_profile(self.profile);
        if self.layers != expected.layers || self.exchanges != expected.exchanges {
            return Err(ClimateLayerLayoutError::ProfileDefinitionMismatch {
                profile: self.profile,
            });
        }
        Ok(())
    }

    pub const fn profile(&self) -> ClimateModelProfile {
        self.profile
    }

    pub fn layers(&self) -> &[ClimateLayerSpec] {
        &self.layers
    }

    pub fn exchanges(&self) -> &[ClimateLayerExchangeSpec] {
        &self.exchanges
    }

    pub fn exchange(
        &self,
        first: ClimateLayerRole,
        second: ClimateLayerRole,
    ) -> Option<ClimateLayerExchangeSpec> {
        self.exchanges
            .iter()
            .copied()
            .find(|exchange| exchange.first == first && exchange.second == second)
    }

    /// Fingerprints the scientific layer definition independently of serde.
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.climate-layer-layout.v1\0");
        hasher.update(&self.schema_version.to_le_bytes());
        hasher.update(&[model_profile_tag(self.profile)]);
        hasher.update(&(self.layers.len() as u32).to_le_bytes());
        for layer in &self.layers {
            hasher.update(&[layer_role_tag(layer.role)]);
            hasher.update(&[u8::from(layer.dynamically_active)]);
            for value in [
                layer.reference_thickness_m,
                layer.density_kg_m3,
                layer.heat_capacity_j_kg_k,
            ] {
                hasher.update(&value.to_bits().to_le_bytes());
            }
        }
        hasher.update(&(self.exchanges.len() as u32).to_le_bytes());
        for exchange in &self.exchanges {
            hasher.update(&[layer_role_tag(exchange.first)]);
            hasher.update(&[layer_role_tag(exchange.second)]);
            for value in [
                exchange.heat_exchange_time_s,
                exchange.momentum_exchange_time_s,
                exchange.moisture_exchange_time_s,
            ] {
                match value {
                    Some(value) => {
                        hasher.update(&[1]);
                        hasher.update(&value.to_bits().to_le_bytes());
                    }
                    None => {
                        hasher.update(&[0]);
                    }
                }
            }
            hasher.update(&[u8::from(exchange.water_only)]);
        }
        *hasher.finalize().as_bytes()
    }
}

impl<'de> Deserialize<'de> for ClimateLayerLayout {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateLayerLayoutWire::deserialize(deserializer)?;
        let layout = Self {
            schema_version: wire.schema_version,
            profile: wire.profile,
            layers: wire.layers,
            exchanges: wire.exchanges,
        };
        layout.validate().map_err(D::Error::custom)?;
        Ok(layout)
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateLayerLayoutError {
    #[error("unsupported climate layer-layout schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("serialized layers do not equal the fixed {profile:?} definition")]
    ProfileDefinitionMismatch { profile: ClimateModelProfile },
}

/// Product integrators that may own a published P4 snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionIntegratorId {
    ImexCrankNicolsonV1,
    SplitExplicitRk3V1,
}

/// Stable floating-point and reduction protocol used by resumable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateQuantizationId {
    DeterministicF64V1,
}

/// Capability IDs whose absence must never be inferred from a missing field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateCapabilityId {
    SeasonalMeanV1,
    VerticalStructureV1,
    SeaIceV1,
    LandSurfaceFeedbackV1,
    EquatorialVariabilityV1,
    TropicalCycloneClimatologyV1,
}

const ALL_CLIMATE_CAPABILITIES: [ClimateCapabilityId; 6] = [
    ClimateCapabilityId::SeasonalMeanV1,
    ClimateCapabilityId::VerticalStructureV1,
    ClimateCapabilityId::SeaIceV1,
    ClimateCapabilityId::LandSurfaceFeedbackV1,
    ClimateCapabilityId::EquatorialVariabilityV1,
    ClimateCapabilityId::TropicalCycloneClimatologyV1,
];

/// Explicit three-state capability outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClimateCapabilityAvailability {
    Unavailable,
    EvaluatedNotApplicable,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateCapabilityStatus {
    id: ClimateCapabilityId,
    availability: ClimateCapabilityAvailability,
}

/// A complete, canonical inventory of all known climate capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateCapabilitySet {
    statuses: Vec<ClimateCapabilityStatus>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateCapabilitySetWire {
    #[serde(deserialize_with = "deserialize_climate_capabilities")]
    statuses: Vec<ClimateCapabilityStatus>,
}

fn deserialize_climate_capabilities<'de, D>(
    deserializer: D,
) -> Result<Vec<ClimateCapabilityStatus>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec::<_, _, 6>(deserializer)
}

impl ClimateCapabilitySet {
    pub fn new(
        statuses: Vec<(ClimateCapabilityId, ClimateCapabilityAvailability)>,
    ) -> Result<Self, ClimateCapabilityError> {
        let mut statuses = statuses
            .into_iter()
            .map(|(id, availability)| ClimateCapabilityStatus { id, availability })
            .collect::<Vec<_>>();
        statuses.sort_by_key(|status| status.id);
        let set = Self { statuses };
        set.validate()?;
        Ok(set)
    }

    pub fn for_profile(profile: ClimateModelProfile) -> Self {
        Self::new(
            ALL_CLIMATE_CAPABILITIES
                .into_iter()
                .map(|id| {
                    let availability = match id {
                        ClimateCapabilityId::SeasonalMeanV1 => {
                            ClimateCapabilityAvailability::Available
                        }
                        ClimateCapabilityId::VerticalStructureV1
                            if profile == ClimateModelProfile::C2LayeredV1 =>
                        {
                            ClimateCapabilityAvailability::Available
                        }
                        _ => ClimateCapabilityAvailability::Unavailable,
                    };
                    (id, availability)
                })
                .collect(),
        )
        .expect("closed profile capability inventory is valid")
    }

    pub fn validate(&self) -> Result<(), ClimateCapabilityError> {
        if self.statuses.len() != ALL_CLIMATE_CAPABILITIES.len() {
            return Err(ClimateCapabilityError::IncompleteInventory {
                found: self.statuses.len(),
                expected: ALL_CLIMATE_CAPABILITIES.len(),
            });
        }
        for (index, expected) in ALL_CLIMATE_CAPABILITIES.iter().enumerate() {
            if self.statuses[index].id != *expected {
                return Err(ClimateCapabilityError::NonCanonicalInventory { index });
            }
        }
        Ok(())
    }

    pub fn availability(&self, id: ClimateCapabilityId) -> ClimateCapabilityAvailability {
        self.statuses
            .iter()
            .find(|status| status.id == id)
            .map(|status| status.availability)
            .expect("validated capability sets contain every closed ID")
    }
}

impl<'de> Deserialize<'de> for ClimateCapabilitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateCapabilitySetWire::deserialize(deserializer)?;
        Self::new(
            wire.statuses
                .into_iter()
                .map(|status| (status.id, status.availability))
                .collect(),
        )
        .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClimateCapabilityError {
    #[error("capability inventory has {found} entries, expected {expected}")]
    IncompleteInventory { found: usize, expected: usize },
    #[error("capability inventory is duplicate, missing, or out of canonical order at {index}")]
    NonCanonicalInventory { index: usize },
}

/// Strict identity and state hash for a resumable formation checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateCheckpoint {
    schema_version: u16,
    quality_profile: NaturalQualityProfile,
    profile: ClimateModelProfile,
    integrator: ProductionIntegratorId,
    grid_fingerprint: [u8; 32],
    forcing_fingerprint: [u8; 32],
    model_fingerprint: [u8; 32],
    input_fingerprint: [u8; 32],
    quantization: ClimateQuantizationId,
    completed_phase_steps: u32,
    state_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateCheckpointWire {
    schema_version: u16,
    quality_profile: NaturalQualityProfile,
    profile: ClimateModelProfile,
    integrator: ProductionIntegratorId,
    grid_fingerprint: [u8; 32],
    forcing_fingerprint: [u8; 32],
    model_fingerprint: [u8; 32],
    input_fingerprint: [u8; 32],
    quantization: ClimateQuantizationId,
    completed_phase_steps: u32,
    state_fingerprint: [u8; 32],
    fingerprint: [u8; 32],
}

impl ClimateCheckpoint {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        quality_profile: NaturalQualityProfile,
        profile: ClimateModelProfile,
        integrator: ProductionIntegratorId,
        grid_fingerprint: [u8; 32],
        forcing_fingerprint: [u8; 32],
        model_fingerprint: [u8; 32],
        input_fingerprint: [u8; 32],
        quantization: ClimateQuantizationId,
        completed_phase_steps: u32,
        state_fingerprint: [u8; 32],
    ) -> Result<Self, ClimateCheckpointError> {
        let mut checkpoint = Self {
            schema_version: CLIMATE_CHECKPOINT_SCHEMA_V2,
            quality_profile,
            profile,
            integrator,
            grid_fingerprint,
            forcing_fingerprint,
            model_fingerprint,
            input_fingerprint,
            quantization,
            completed_phase_steps,
            state_fingerprint,
            fingerprint: [0; 32],
        };
        checkpoint.validate_identity()?;
        checkpoint.fingerprint = checkpoint.canonical_fingerprint();
        Ok(checkpoint)
    }

    pub fn validate(&self) -> Result<(), ClimateCheckpointError> {
        self.validate_identity()?;
        let calculated = self.canonical_fingerprint();
        if self.fingerprint != calculated {
            return Err(ClimateCheckpointError::FingerprintMismatch);
        }
        Ok(())
    }

    fn validate_identity(&self) -> Result<(), ClimateCheckpointError> {
        if self.schema_version != CLIMATE_CHECKPOINT_SCHEMA_V2 {
            return Err(ClimateCheckpointError::UnsupportedSchema {
                found: self.schema_version,
                supported: CLIMATE_CHECKPOINT_SCHEMA_V2,
            });
        }
        for (field, fingerprint) in [
            ("grid_fingerprint", self.grid_fingerprint),
            ("forcing_fingerprint", self.forcing_fingerprint),
            ("model_fingerprint", self.model_fingerprint),
            ("input_fingerprint", self.input_fingerprint),
            ("state_fingerprint", self.state_fingerprint),
        ] {
            if fingerprint == [0; 32] {
                return Err(ClimateCheckpointError::ZeroFingerprint { field });
            }
        }
        if self.completed_phase_steps == 0
            || self.completed_phase_steps % u32::try_from(CLIMATE_MONTH_COUNT).unwrap_or(12) != 0
        {
            return Err(ClimateCheckpointError::InvalidCompletedPhaseSteps {
                found: self.completed_phase_steps,
            });
        }
        let maximum_phase_steps = u32::from(
            self.quality_profile
                .global_circulation_formation_cycles_max(),
        ) * CLIMATE_MONTH_COUNT as u32;
        if self.completed_phase_steps > maximum_phase_steps {
            return Err(ClimateCheckpointError::CompletedPhaseStepsExceedProfile {
                profile: self.quality_profile,
                found: self.completed_phase_steps,
                maximum: maximum_phase_steps,
            });
        }
        Ok(())
    }

    fn canonical_fingerprint(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.climate-checkpoint.v2\0");
        hasher.update(&self.schema_version.to_le_bytes());
        hasher.update(&[natural_quality_profile_tag(self.quality_profile)]);
        hasher.update(&[model_profile_tag(self.profile)]);
        hasher.update(&[integrator_tag(self.integrator)]);
        hasher.update(&self.grid_fingerprint);
        hasher.update(&self.forcing_fingerprint);
        hasher.update(&self.model_fingerprint);
        hasher.update(&self.input_fingerprint);
        hasher.update(&[quantization_tag(self.quantization)]);
        hasher.update(&self.completed_phase_steps.to_le_bytes());
        hasher.update(&self.state_fingerprint);
        *hasher.finalize().as_bytes()
    }

    pub const fn profile(&self) -> ClimateModelProfile {
        self.profile
    }

    pub const fn quality_profile(&self) -> NaturalQualityProfile {
        self.quality_profile
    }

    pub const fn integrator(&self) -> ProductionIntegratorId {
        self.integrator
    }

    pub const fn grid_fingerprint(&self) -> &[u8; 32] {
        &self.grid_fingerprint
    }

    pub const fn forcing_fingerprint(&self) -> &[u8; 32] {
        &self.forcing_fingerprint
    }

    pub const fn model_fingerprint(&self) -> &[u8; 32] {
        &self.model_fingerprint
    }

    pub const fn input_fingerprint(&self) -> &[u8; 32] {
        &self.input_fingerprint
    }

    pub const fn completed_phase_steps(&self) -> u32 {
        self.completed_phase_steps
    }

    pub const fn state_fingerprint(&self) -> &[u8; 32] {
        &self.state_fingerprint
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

impl<'de> Deserialize<'de> for ClimateCheckpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateCheckpointWire::deserialize(deserializer)?;
        let mut checkpoint = Self::new(
            wire.quality_profile,
            wire.profile,
            wire.integrator,
            wire.grid_fingerprint,
            wire.forcing_fingerprint,
            wire.model_fingerprint,
            wire.input_fingerprint,
            wire.quantization,
            wire.completed_phase_steps,
            wire.state_fingerprint,
        )
        .map_err(D::Error::custom)?;
        if wire.schema_version != CLIMATE_CHECKPOINT_SCHEMA_V2 {
            return Err(D::Error::custom(
                ClimateCheckpointError::UnsupportedSchema {
                    found: wire.schema_version,
                    supported: CLIMATE_CHECKPOINT_SCHEMA_V2,
                },
            ));
        }
        if checkpoint.fingerprint != wire.fingerprint {
            return Err(D::Error::custom(
                ClimateCheckpointError::FingerprintMismatch,
            ));
        }
        checkpoint.fingerprint = wire.fingerprint;
        Ok(checkpoint)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClimateCheckpointError {
    #[error("unsupported climate checkpoint schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("checkpoint {field} cannot be zero")]
    ZeroFingerprint { field: &'static str },
    #[error(
        "checkpoint completed phase steps {found} must be a positive whole forcing-phase cycle"
    )]
    InvalidCompletedPhaseSteps { found: u32 },
    #[error(
        "checkpoint completed phase steps {found} exceeds {profile:?} formation maximum {maximum}"
    )]
    CompletedPhaseStepsExceedProfile {
        profile: NaturalQualityProfile,
        found: u32,
        maximum: u32,
    },
    #[error("climate checkpoint fingerprint does not match its semantic fields")]
    FingerprintMismatch,
}

const fn model_profile_tag(profile: ClimateModelProfile) -> u8 {
    match profile {
        ClimateModelProfile::C1SingleLayerV1 => 1,
        ClimateModelProfile::C2LayeredV1 => 2,
    }
}

const fn natural_quality_profile_tag(profile: NaturalQualityProfile) -> u8 {
    match profile {
        NaturalQualityProfile::Draft => 1,
        NaturalQualityProfile::Standard => 2,
        NaturalQualityProfile::High => 3,
    }
}

const fn layer_role_tag(role: ClimateLayerRole) -> u8 {
    match role {
        ClimateLayerRole::LowerAtmosphere => 1,
        ClimateLayerRole::UpperAtmosphere => 2,
        ClimateLayerRole::OceanMixedLayer => 3,
        ClimateLayerRole::OceanThermocline => 4,
        ClimateLayerRole::DeepOceanReservoir => 5,
    }
}

const fn integrator_tag(integrator: ProductionIntegratorId) -> u8 {
    match integrator {
        ProductionIntegratorId::ImexCrankNicolsonV1 => 1,
        ProductionIntegratorId::SplitExplicitRk3V1 => 2,
    }
}

const fn quantization_tag(quantization: ClimateQuantizationId) -> u8 {
    match quantization {
        ClimateQuantizationId::DeterministicF64V1 => 1,
    }
}

/// Bounded formation and numerical-convergence evidence.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateSolveReport {
    formation_cycles: u16,
    continuation_steps: u64,
    integrated_model_seconds: u64,
    fast_substeps: u64,
    linear_iterations: u64,
    initial_residual: f64,
    final_residual: f64,
    maximum_cfl: f64,
    dense_state_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateSolveReportWire {
    formation_cycles: u16,
    continuation_steps: u64,
    integrated_model_seconds: u64,
    fast_substeps: u64,
    linear_iterations: u64,
    initial_residual: f64,
    final_residual: f64,
    maximum_cfl: f64,
    dense_state_bytes: u64,
}

impl ClimateSolveReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        formation_cycles: u16,
        continuation_steps: u64,
        fast_substeps: u64,
        linear_iterations: u64,
        initial_residual: f64,
        final_residual: f64,
        maximum_cfl: f64,
        dense_state_bytes: u64,
    ) -> Result<Self, ClimateReportError> {
        let integrated_model_seconds = continuation_steps
            .checked_mul(GLOBAL_CIRCULATION_MACRO_STEP_SECONDS as u64)
            .ok_or(ClimateReportError::WorkOverflow {
                field: "integrated_model_seconds",
            })?;
        let report = Self {
            formation_cycles,
            continuation_steps,
            integrated_model_seconds,
            fast_substeps,
            linear_iterations,
            initial_residual,
            final_residual,
            maximum_cfl,
            dense_state_bytes,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), ClimateReportError> {
        for (field, value) in [
            ("initial_residual", self.initial_residual),
            ("final_residual", self.final_residual),
            ("maximum_cfl", self.maximum_cfl),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ClimateReportError::InvalidStatistic {
                    field,
                    found: value,
                });
            }
        }
        if self.formation_cycles == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "formation_cycles",
            });
        }
        if self.formation_cycles > GLOBAL_CIRCULATION_FORMATION_CYCLES_MAX {
            return Err(ClimateReportError::StatisticAboveMaximum {
                field: "formation_cycles",
                found: f64::from(self.formation_cycles),
                maximum: f64::from(GLOBAL_CIRCULATION_FORMATION_CYCLES_MAX),
            });
        }
        if self.continuation_steps == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "continuation_steps",
            });
        }
        let expected_steps = u64::from(self.formation_cycles)
            .checked_mul(CLIMATE_MONTH_COUNT as u64)
            .ok_or(ClimateReportError::WorkOverflow {
                field: "continuation_steps",
            })?;
        if self.continuation_steps != expected_steps {
            return Err(ClimateReportError::WorkMismatch {
                field: "continuation_steps",
                found: self.continuation_steps,
                expected: expected_steps,
            });
        }
        let expected_seconds = self
            .continuation_steps
            .checked_mul(GLOBAL_CIRCULATION_MACRO_STEP_SECONDS as u64)
            .ok_or(ClimateReportError::WorkOverflow {
                field: "integrated_model_seconds",
            })?;
        if self.integrated_model_seconds != expected_seconds {
            return Err(ClimateReportError::WorkMismatch {
                field: "integrated_model_seconds",
                found: self.integrated_model_seconds,
                expected: expected_seconds,
            });
        }
        if self.fast_substeps == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "fast_substeps",
            });
        }
        if self.dense_state_bytes == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "dense_state_bytes",
            });
        }
        if self.dense_state_bytes > GLOBAL_CIRCULATION_DENSE_STATE_BYTES_MAX {
            return Err(ClimateReportError::StatisticAboveMaximum {
                field: "dense_state_bytes",
                found: self.dense_state_bytes as f64,
                maximum: GLOBAL_CIRCULATION_DENSE_STATE_BYTES_MAX as f64,
            });
        }
        if self.final_residual > self.initial_residual {
            return Err(ClimateReportError::ResidualIncreased {
                initial: self.initial_residual,
                final_value: self.final_residual,
            });
        }
        if self.final_residual > GLOBAL_CIRCULATION_FORMATION_RESIDUAL_MAX {
            return Err(ClimateReportError::StatisticAboveMaximum {
                field: "final_residual",
                found: self.final_residual,
                maximum: GLOBAL_CIRCULATION_FORMATION_RESIDUAL_MAX,
            });
        }
        if self.maximum_cfl > 1.0 {
            return Err(ClimateReportError::StatisticAboveMaximum {
                field: "maximum_cfl",
                found: self.maximum_cfl,
                maximum: 1.0,
            });
        }
        Ok(())
    }

    pub const fn formation_cycles(&self) -> u16 {
        self.formation_cycles
    }

    pub const fn continuation_steps(&self) -> u64 {
        self.continuation_steps
    }

    pub const fn integrated_model_seconds(&self) -> u64 {
        self.integrated_model_seconds
    }

    pub const fn fast_substeps(&self) -> u64 {
        self.fast_substeps
    }

    pub const fn linear_iterations(&self) -> u64 {
        self.linear_iterations
    }

    pub const fn final_residual(&self) -> f64 {
        self.final_residual
    }

    pub const fn maximum_cfl(&self) -> f64 {
        self.maximum_cfl
    }

    pub const fn dense_state_bytes(&self) -> u64 {
        self.dense_state_bytes
    }
}

impl<'de> Deserialize<'de> for ClimateSolveReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateSolveReportWire::deserialize(deserializer)?;
        let report = Self::new(
            wire.formation_cycles,
            wire.continuation_steps,
            wire.fast_substeps,
            wire.linear_iterations,
            wire.initial_residual,
            wire.final_residual,
            wire.maximum_cfl,
            wire.dense_state_bytes,
        )
        .map_err(D::Error::custom)?;
        if wire.integrated_model_seconds != report.integrated_model_seconds {
            return Err(D::Error::custom(ClimateReportError::WorkMismatch {
                field: "integrated_model_seconds",
                found: wire.integrated_model_seconds,
                expected: report.integrated_model_seconds,
            }));
        }
        Ok(report)
    }
}

/// Global conservation closure after physical sources and sinks are accounted for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateBudgetReport {
    atmosphere_mass_relative_error: f64,
    ocean_volume_relative_error: f64,
    moisture_relative_error: f64,
    energy_relative_error: f64,
    paired_exchange_relative_error: f64,
    evaporation_global_mean_mm_day: f64,
    precipitation_global_mean_mm_day: f64,
    evaporation_precipitation_relative_imbalance: f64,
    absorbed_shortwave_global_mean_w_m2: f64,
    outgoing_longwave_global_mean_w_m2: f64,
    toa_net_radiation_global_mean_w_m2: f64,
    planetary_albedo_global_mean: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateBudgetReportWire {
    atmosphere_mass_relative_error: f64,
    ocean_volume_relative_error: f64,
    moisture_relative_error: f64,
    energy_relative_error: f64,
    paired_exchange_relative_error: f64,
    evaporation_global_mean_mm_day: f64,
    precipitation_global_mean_mm_day: f64,
    evaporation_precipitation_relative_imbalance: f64,
    absorbed_shortwave_global_mean_w_m2: f64,
    outgoing_longwave_global_mean_w_m2: f64,
    toa_net_radiation_global_mean_w_m2: f64,
    planetary_albedo_global_mean: f64,
}

impl ClimateBudgetReport {
    pub fn new(
        atmosphere_mass_relative_error: f64,
        ocean_volume_relative_error: f64,
        moisture_relative_error: f64,
        energy_relative_error: f64,
        paired_exchange_relative_error: f64,
    ) -> Result<Self, ClimateReportError> {
        Self::new_with_climatology(
            atmosphere_mass_relative_error,
            ocean_volume_relative_error,
            moisture_relative_error,
            energy_relative_error,
            paired_exchange_relative_error,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_climatology(
        atmosphere_mass_relative_error: f64,
        ocean_volume_relative_error: f64,
        moisture_relative_error: f64,
        energy_relative_error: f64,
        paired_exchange_relative_error: f64,
        evaporation_global_mean_mm_day: f64,
        precipitation_global_mean_mm_day: f64,
        absorbed_shortwave_global_mean_w_m2: f64,
        outgoing_longwave_global_mean_w_m2: f64,
        planetary_albedo_global_mean: f64,
    ) -> Result<Self, ClimateReportError> {
        let evaporation_precipitation_relative_imbalance = water_cycle_relative_imbalance(
            evaporation_global_mean_mm_day,
            precipitation_global_mean_mm_day,
        );
        let toa_net_radiation_global_mean_w_m2 =
            absorbed_shortwave_global_mean_w_m2 - outgoing_longwave_global_mean_w_m2;
        let report = Self {
            atmosphere_mass_relative_error,
            ocean_volume_relative_error,
            moisture_relative_error,
            energy_relative_error,
            paired_exchange_relative_error,
            evaporation_global_mean_mm_day,
            precipitation_global_mean_mm_day,
            evaporation_precipitation_relative_imbalance,
            absorbed_shortwave_global_mean_w_m2,
            outgoing_longwave_global_mean_w_m2,
            toa_net_radiation_global_mean_w_m2,
            planetary_albedo_global_mean,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), ClimateReportError> {
        for (field, value, maximum) in [
            (
                "atmosphere_mass_relative_error",
                self.atmosphere_mass_relative_error,
                GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
            ),
            (
                "ocean_volume_relative_error",
                self.ocean_volume_relative_error,
                GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
            ),
            (
                "moisture_relative_error",
                self.moisture_relative_error,
                GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
            ),
            (
                "energy_relative_error",
                self.energy_relative_error,
                GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX,
            ),
            (
                "paired_exchange_relative_error",
                self.paired_exchange_relative_error,
                GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
            ),
        ] {
            validate_nonnegative_bounded(field, value, maximum)?;
        }
        for (field, value, maximum) in [
            (
                "evaporation_global_mean_mm_day",
                self.evaporation_global_mean_mm_day,
                f64::from(f32::MAX),
            ),
            (
                "precipitation_global_mean_mm_day",
                self.precipitation_global_mean_mm_day,
                f64::from(f32::MAX),
            ),
            (
                "absorbed_shortwave_global_mean_w_m2",
                self.absorbed_shortwave_global_mean_w_m2,
                GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2,
            ),
            (
                "outgoing_longwave_global_mean_w_m2",
                self.outgoing_longwave_global_mean_w_m2,
                GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2,
            ),
            (
                "planetary_albedo_global_mean",
                self.planetary_albedo_global_mean,
                1.0,
            ),
        ] {
            validate_nonnegative_bounded(field, value, maximum)?;
        }
        if !self.toa_net_radiation_global_mean_w_m2.is_finite() {
            return Err(ClimateReportError::InvalidStatistic {
                field: "toa_net_radiation_global_mean_w_m2",
                found: self.toa_net_radiation_global_mean_w_m2,
            });
        }
        if self.toa_net_radiation_global_mean_w_m2.abs() > GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2 {
            return Err(ClimateReportError::RadiativeCycleNotClosed {
                absorbed_shortwave_w_m2: self.absorbed_shortwave_global_mean_w_m2,
                outgoing_longwave_w_m2: self.outgoing_longwave_global_mean_w_m2,
                net_w_m2: self.toa_net_radiation_global_mean_w_m2,
                maximum: GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2,
            });
        }
        let expected_water_imbalance = water_cycle_relative_imbalance(
            self.evaporation_global_mean_mm_day,
            self.precipitation_global_mean_mm_day,
        );
        if expected_water_imbalance.to_bits()
            != self.evaporation_precipitation_relative_imbalance.to_bits()
        {
            return Err(ClimateReportError::StatisticIdentityMismatch {
                field: "evaporation_precipitation_relative_imbalance",
                found: self.evaporation_precipitation_relative_imbalance,
                expected: expected_water_imbalance,
            });
        }
        if self.evaporation_precipitation_relative_imbalance
            > GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX
        {
            return Err(ClimateReportError::WaterCycleNotClosed {
                evaporation_mm_day: self.evaporation_global_mean_mm_day,
                precipitation_mm_day: self.precipitation_global_mean_mm_day,
                relative_imbalance: self.evaporation_precipitation_relative_imbalance,
                maximum: GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX,
            });
        }
        let expected_toa =
            self.absorbed_shortwave_global_mean_w_m2 - self.outgoing_longwave_global_mean_w_m2;
        if expected_toa.to_bits() != self.toa_net_radiation_global_mean_w_m2.to_bits() {
            return Err(ClimateReportError::StatisticIdentityMismatch {
                field: "toa_net_radiation_global_mean_w_m2",
                found: self.toa_net_radiation_global_mean_w_m2,
                expected: expected_toa,
            });
        }
        Ok(())
    }

    pub const fn atmosphere_mass_relative_error(&self) -> f64 {
        self.atmosphere_mass_relative_error
    }

    pub const fn ocean_volume_relative_error(&self) -> f64 {
        self.ocean_volume_relative_error
    }

    pub const fn moisture_relative_error(&self) -> f64 {
        self.moisture_relative_error
    }

    pub const fn energy_relative_error(&self) -> f64 {
        self.energy_relative_error
    }

    pub const fn paired_exchange_relative_error(&self) -> f64 {
        self.paired_exchange_relative_error
    }

    pub const fn evaporation_global_mean_mm_day(&self) -> f64 {
        self.evaporation_global_mean_mm_day
    }

    pub const fn precipitation_global_mean_mm_day(&self) -> f64 {
        self.precipitation_global_mean_mm_day
    }

    pub const fn evaporation_precipitation_relative_imbalance(&self) -> f64 {
        self.evaporation_precipitation_relative_imbalance
    }

    /// Returns the signed global water-cycle residual in `mm/day`.
    pub fn evaporation_minus_precipitation_global_mean_mm_day(&self) -> f64 {
        self.evaporation_global_mean_mm_day - self.precipitation_global_mean_mm_day
    }

    pub const fn absorbed_shortwave_global_mean_w_m2(&self) -> f64 {
        self.absorbed_shortwave_global_mean_w_m2
    }

    pub const fn outgoing_longwave_global_mean_w_m2(&self) -> f64 {
        self.outgoing_longwave_global_mean_w_m2
    }

    pub const fn toa_net_radiation_global_mean_w_m2(&self) -> f64 {
        self.toa_net_radiation_global_mean_w_m2
    }

    pub const fn planetary_albedo_global_mean(&self) -> f64 {
        self.planetary_albedo_global_mean
    }
}

impl<'de> Deserialize<'de> for ClimateBudgetReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateBudgetReportWire::deserialize(deserializer)?;
        let report = Self {
            atmosphere_mass_relative_error: wire.atmosphere_mass_relative_error,
            ocean_volume_relative_error: wire.ocean_volume_relative_error,
            moisture_relative_error: wire.moisture_relative_error,
            energy_relative_error: wire.energy_relative_error,
            paired_exchange_relative_error: wire.paired_exchange_relative_error,
            evaporation_global_mean_mm_day: wire.evaporation_global_mean_mm_day,
            precipitation_global_mean_mm_day: wire.precipitation_global_mean_mm_day,
            evaporation_precipitation_relative_imbalance: wire
                .evaporation_precipitation_relative_imbalance,
            absorbed_shortwave_global_mean_w_m2: wire.absorbed_shortwave_global_mean_w_m2,
            outgoing_longwave_global_mean_w_m2: wire.outgoing_longwave_global_mean_w_m2,
            toa_net_radiation_global_mean_w_m2: wire.toa_net_radiation_global_mean_w_m2,
            planetary_albedo_global_mean: wire.planetary_albedo_global_mean,
        };
        report.validate().map_err(D::Error::custom)?;
        Ok(report)
    }
}

/// Conservative surface-bridge closure carried with the public result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateRemapReport {
    forward_source_margin_relative_error: f64,
    forward_target_margin_relative_error: f64,
    reverse_source_margin_relative_error: f64,
    reverse_target_margin_relative_error: f64,
    published_precipitation_relative_error: f64,
    forward_overlap_count: u32,
    reverse_overlap_count: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateRemapReportWire {
    forward_source_margin_relative_error: f64,
    forward_target_margin_relative_error: f64,
    reverse_source_margin_relative_error: f64,
    reverse_target_margin_relative_error: f64,
    published_precipitation_relative_error: f64,
    forward_overlap_count: u32,
    reverse_overlap_count: u32,
}

impl ClimateRemapReport {
    pub fn new(
        forward_source_margin_relative_error: f64,
        forward_target_margin_relative_error: f64,
        reverse_source_margin_relative_error: f64,
        reverse_target_margin_relative_error: f64,
        published_precipitation_relative_error: f64,
        forward_overlap_count: u32,
        reverse_overlap_count: u32,
    ) -> Result<Self, ClimateReportError> {
        let report = Self {
            forward_source_margin_relative_error,
            forward_target_margin_relative_error,
            reverse_source_margin_relative_error,
            reverse_target_margin_relative_error,
            published_precipitation_relative_error,
            forward_overlap_count,
            reverse_overlap_count,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), ClimateReportError> {
        for (field, value) in [
            (
                "forward_source_margin_relative_error",
                self.forward_source_margin_relative_error,
            ),
            (
                "forward_target_margin_relative_error",
                self.forward_target_margin_relative_error,
            ),
            (
                "reverse_source_margin_relative_error",
                self.reverse_source_margin_relative_error,
            ),
            (
                "reverse_target_margin_relative_error",
                self.reverse_target_margin_relative_error,
            ),
        ] {
            validate_nonnegative_bounded(field, value, 1.0e-10)?;
        }
        validate_nonnegative_bounded(
            "published_precipitation_relative_error",
            self.published_precipitation_relative_error,
            GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
        )?;
        if self.forward_overlap_count == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "forward_overlap_count",
            });
        }
        if self.reverse_overlap_count == 0 {
            return Err(ClimateReportError::ZeroWork {
                field: "reverse_overlap_count",
            });
        }
        Ok(())
    }

    pub const fn forward_overlap_count(&self) -> u32 {
        self.forward_overlap_count
    }

    pub const fn reverse_overlap_count(&self) -> u32 {
        self.reverse_overlap_count
    }

    pub const fn forward_source_margin_relative_error(&self) -> f64 {
        self.forward_source_margin_relative_error
    }

    pub const fn forward_target_margin_relative_error(&self) -> f64 {
        self.forward_target_margin_relative_error
    }

    pub const fn reverse_source_margin_relative_error(&self) -> f64 {
        self.reverse_source_margin_relative_error
    }

    pub const fn reverse_target_margin_relative_error(&self) -> f64 {
        self.reverse_target_margin_relative_error
    }

    pub const fn published_precipitation_relative_error(&self) -> f64 {
        self.published_precipitation_relative_error
    }
}

impl<'de> Deserialize<'de> for ClimateRemapReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateRemapReportWire::deserialize(deserializer)?;
        Self::new(
            wire.forward_source_margin_relative_error,
            wire.forward_target_margin_relative_error,
            wire.reverse_source_margin_relative_error,
            wire.reverse_target_margin_relative_error,
            wire.published_precipitation_relative_error,
            wire.forward_overlap_count,
            wire.reverse_overlap_count,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_nonnegative_bounded(
    field: &'static str,
    found: f64,
    maximum: f64,
) -> Result<(), ClimateReportError> {
    if !found.is_finite() || found < 0.0 {
        return Err(ClimateReportError::InvalidStatistic { field, found });
    }
    if found > maximum {
        return Err(ClimateReportError::StatisticAboveMaximum {
            field,
            found,
            maximum,
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateReportError {
    #[error("climate report {field} is invalid: {found}")]
    InvalidStatistic { field: &'static str, found: f64 },
    #[error("climate report {field} is zero")]
    ZeroWork { field: &'static str },
    #[error("climate report {field} work count overflowed")]
    WorkOverflow { field: &'static str },
    #[error("climate report {field} is {found}, expected {expected}")]
    WorkMismatch {
        field: &'static str,
        found: u64,
        expected: u64,
    },
    #[error("climate residual increased from {initial} to {final_value}")]
    ResidualIncreased { initial: f64, final_value: f64 },
    #[error("climate report {field} is {found}, maximum {maximum}")]
    StatisticAboveMaximum {
        field: &'static str,
        found: f64,
        maximum: f64,
    },
    #[error("climate report {field} identity is {found}, expected {expected}")]
    StatisticIdentityMismatch {
        field: &'static str,
        found: f64,
        expected: f64,
    },
    #[error(
        "final-cycle water budget is not closed: evaporation {evaporation_mm_day} mm/day, precipitation {precipitation_mm_day} mm/day, relative imbalance {relative_imbalance}, maximum {maximum}"
    )]
    WaterCycleNotClosed {
        evaporation_mm_day: f64,
        precipitation_mm_day: f64,
        relative_imbalance: f64,
        maximum: f64,
    },
    #[error(
        "final-cycle TOA budget is not closed: ASR {absorbed_shortwave_w_m2} W/m2, OLR {outgoing_longwave_w_m2} W/m2, net {net_w_m2} W/m2, absolute maximum {maximum}"
    )]
    RadiativeCycleNotClosed {
        absorbed_shortwave_w_m2: f64,
        outgoing_longwave_w_m2: f64,
        net_w_m2: f64,
        maximum: f64,
    },
}

/// Stable semantic monthly fields projected onto the authoritative surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCirculationFields {
    near_surface_wind_m_s: MonthlyVector3Field,
    upper_wind_m_s: Option<MonthlyVector3Field>,
    vertical_wind_shear_m_s: Option<MonthlyVector3Field>,
    surface_ocean_current_m_s: MonthlyVector3Field,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_sea_surface_temperature_c: MonthlyScalarField,
    surface_albedo: Vec<f32>,
    monthly_absorbed_shortwave_w_m2: MonthlyScalarField,
    monthly_outgoing_longwave_w_m2: MonthlyScalarField,
    monthly_thermocline_temperature_c: Option<MonthlyScalarField>,
    monthly_thermocline_depth_m: Option<MonthlyScalarField>,
    monthly_specific_humidity: MonthlyScalarField,
    monthly_evaporation_mm_day: MonthlyScalarField,
    monthly_precipitation_mm_day: MonthlyScalarField,
    monthly_orographic_precipitation_mm_day: MonthlyScalarField,
    monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
    monthly_upper_atmosphere_height_anomaly_m: Option<MonthlyScalarField>,
    monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
    monthly_thermocline_height_anomaly_m: Option<MonthlyScalarField>,
    monthly_deep_ocean_temperature_c: Option<MonthlyScalarField>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobalCirculationFieldsWire {
    near_surface_wind_m_s: MonthlyVector3Field,
    upper_wind_m_s: Option<MonthlyVector3Field>,
    vertical_wind_shear_m_s: Option<MonthlyVector3Field>,
    surface_ocean_current_m_s: MonthlyVector3Field,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_sea_surface_temperature_c: MonthlyScalarField,
    #[serde(deserialize_with = "deserialize_global_circulation_scalars")]
    surface_albedo: Vec<f32>,
    monthly_absorbed_shortwave_w_m2: MonthlyScalarField,
    monthly_outgoing_longwave_w_m2: MonthlyScalarField,
    monthly_thermocline_temperature_c: Option<MonthlyScalarField>,
    monthly_thermocline_depth_m: Option<MonthlyScalarField>,
    monthly_specific_humidity: MonthlyScalarField,
    monthly_evaporation_mm_day: MonthlyScalarField,
    monthly_precipitation_mm_day: MonthlyScalarField,
    monthly_orographic_precipitation_mm_day: MonthlyScalarField,
    monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
    monthly_upper_atmosphere_height_anomaly_m: Option<MonthlyScalarField>,
    monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
    monthly_thermocline_height_anomaly_m: Option<MonthlyScalarField>,
    monthly_deep_ocean_temperature_c: Option<MonthlyScalarField>,
}

impl GlobalCirculationFields {
    #[allow(clippy::too_many_arguments)]
    pub fn new_c1(
        near_surface_wind_m_s: MonthlyVector3Field,
        surface_ocean_current_m_s: MonthlyVector3Field,
        monthly_air_temperature_c: MonthlyScalarField,
        monthly_sea_surface_temperature_c: MonthlyScalarField,
        surface_albedo: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: MonthlyScalarField,
        monthly_outgoing_longwave_w_m2: MonthlyScalarField,
        monthly_specific_humidity: MonthlyScalarField,
        monthly_evaporation_mm_day: MonthlyScalarField,
        monthly_precipitation_mm_day: MonthlyScalarField,
        monthly_orographic_precipitation_mm_day: MonthlyScalarField,
        monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let fields = Self {
            near_surface_wind_m_s,
            upper_wind_m_s: None,
            vertical_wind_shear_m_s: None,
            surface_ocean_current_m_s,
            monthly_air_temperature_c,
            monthly_sea_surface_temperature_c,
            surface_albedo,
            monthly_absorbed_shortwave_w_m2,
            monthly_outgoing_longwave_w_m2,
            monthly_thermocline_temperature_c: None,
            monthly_thermocline_depth_m: None,
            monthly_specific_humidity,
            monthly_evaporation_mm_day,
            monthly_precipitation_mm_day,
            monthly_orographic_precipitation_mm_day,
            monthly_lower_atmosphere_height_anomaly_m,
            monthly_upper_atmosphere_height_anomaly_m: None,
            monthly_sea_surface_height_anomaly_m,
            monthly_thermocline_height_anomaly_m: None,
            monthly_deep_ocean_temperature_c: None,
        };
        fields.validate(ClimateModelProfile::C1SingleLayerV1, fields.cell_count())?;
        Ok(fields)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_c1_cancellable(
        near_surface_wind_m_s: MonthlyVector3Field,
        surface_ocean_current_m_s: MonthlyVector3Field,
        monthly_air_temperature_c: MonthlyScalarField,
        monthly_sea_surface_temperature_c: MonthlyScalarField,
        surface_albedo: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: MonthlyScalarField,
        monthly_outgoing_longwave_w_m2: MonthlyScalarField,
        monthly_specific_humidity: MonthlyScalarField,
        monthly_evaporation_mm_day: MonthlyScalarField,
        monthly_precipitation_mm_day: MonthlyScalarField,
        monthly_orographic_precipitation_mm_day: MonthlyScalarField,
        monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let fields = Self {
            near_surface_wind_m_s,
            upper_wind_m_s: None,
            vertical_wind_shear_m_s: None,
            surface_ocean_current_m_s,
            monthly_air_temperature_c,
            monthly_sea_surface_temperature_c,
            surface_albedo,
            monthly_absorbed_shortwave_w_m2,
            monthly_outgoing_longwave_w_m2,
            monthly_thermocline_temperature_c: None,
            monthly_thermocline_depth_m: None,
            monthly_specific_humidity,
            monthly_evaporation_mm_day,
            monthly_precipitation_mm_day,
            monthly_orographic_precipitation_mm_day,
            monthly_lower_atmosphere_height_anomaly_m,
            monthly_upper_atmosphere_height_anomaly_m: None,
            monthly_sea_surface_height_anomaly_m,
            monthly_thermocline_height_anomaly_m: None,
            monthly_deep_ocean_temperature_c: None,
        };
        fields.validate_cancellable(
            ClimateModelProfile::C1SingleLayerV1,
            fields.cell_count(),
            cancelled,
        )?;
        Ok(fields)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_c2(
        near_surface_wind_m_s: MonthlyVector3Field,
        upper_wind_m_s: MonthlyVector3Field,
        vertical_wind_shear_m_s: MonthlyVector3Field,
        surface_ocean_current_m_s: MonthlyVector3Field,
        monthly_air_temperature_c: MonthlyScalarField,
        monthly_sea_surface_temperature_c: MonthlyScalarField,
        surface_albedo: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: MonthlyScalarField,
        monthly_outgoing_longwave_w_m2: MonthlyScalarField,
        monthly_thermocline_temperature_c: MonthlyScalarField,
        monthly_thermocline_depth_m: MonthlyScalarField,
        monthly_specific_humidity: MonthlyScalarField,
        monthly_evaporation_mm_day: MonthlyScalarField,
        monthly_precipitation_mm_day: MonthlyScalarField,
        monthly_orographic_precipitation_mm_day: MonthlyScalarField,
        monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_upper_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
        monthly_thermocline_height_anomaly_m: MonthlyScalarField,
        monthly_deep_ocean_temperature_c: MonthlyScalarField,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let fields = Self {
            near_surface_wind_m_s,
            upper_wind_m_s: Some(upper_wind_m_s),
            vertical_wind_shear_m_s: Some(vertical_wind_shear_m_s),
            surface_ocean_current_m_s,
            monthly_air_temperature_c,
            monthly_sea_surface_temperature_c,
            surface_albedo,
            monthly_absorbed_shortwave_w_m2,
            monthly_outgoing_longwave_w_m2,
            monthly_thermocline_temperature_c: Some(monthly_thermocline_temperature_c),
            monthly_thermocline_depth_m: Some(monthly_thermocline_depth_m),
            monthly_specific_humidity,
            monthly_evaporation_mm_day,
            monthly_precipitation_mm_day,
            monthly_orographic_precipitation_mm_day,
            monthly_lower_atmosphere_height_anomaly_m,
            monthly_upper_atmosphere_height_anomaly_m: Some(
                monthly_upper_atmosphere_height_anomaly_m,
            ),
            monthly_sea_surface_height_anomaly_m,
            monthly_thermocline_height_anomaly_m: Some(monthly_thermocline_height_anomaly_m),
            monthly_deep_ocean_temperature_c: Some(monthly_deep_ocean_temperature_c),
        };
        fields.validate(ClimateModelProfile::C2LayeredV1, fields.cell_count())?;
        Ok(fields)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_c2_cancellable(
        near_surface_wind_m_s: MonthlyVector3Field,
        upper_wind_m_s: MonthlyVector3Field,
        vertical_wind_shear_m_s: MonthlyVector3Field,
        surface_ocean_current_m_s: MonthlyVector3Field,
        monthly_air_temperature_c: MonthlyScalarField,
        monthly_sea_surface_temperature_c: MonthlyScalarField,
        surface_albedo: Vec<f32>,
        monthly_absorbed_shortwave_w_m2: MonthlyScalarField,
        monthly_outgoing_longwave_w_m2: MonthlyScalarField,
        monthly_thermocline_temperature_c: MonthlyScalarField,
        monthly_thermocline_depth_m: MonthlyScalarField,
        monthly_specific_humidity: MonthlyScalarField,
        monthly_evaporation_mm_day: MonthlyScalarField,
        monthly_precipitation_mm_day: MonthlyScalarField,
        monthly_orographic_precipitation_mm_day: MonthlyScalarField,
        monthly_lower_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_upper_atmosphere_height_anomaly_m: MonthlyScalarField,
        monthly_sea_surface_height_anomaly_m: MonthlyScalarField,
        monthly_thermocline_height_anomaly_m: MonthlyScalarField,
        monthly_deep_ocean_temperature_c: MonthlyScalarField,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let fields = Self {
            near_surface_wind_m_s,
            upper_wind_m_s: Some(upper_wind_m_s),
            vertical_wind_shear_m_s: Some(vertical_wind_shear_m_s),
            surface_ocean_current_m_s,
            monthly_air_temperature_c,
            monthly_sea_surface_temperature_c,
            surface_albedo,
            monthly_absorbed_shortwave_w_m2,
            monthly_outgoing_longwave_w_m2,
            monthly_thermocline_temperature_c: Some(monthly_thermocline_temperature_c),
            monthly_thermocline_depth_m: Some(monthly_thermocline_depth_m),
            monthly_specific_humidity,
            monthly_evaporation_mm_day,
            monthly_precipitation_mm_day,
            monthly_orographic_precipitation_mm_day,
            monthly_lower_atmosphere_height_anomaly_m,
            monthly_upper_atmosphere_height_anomaly_m: Some(
                monthly_upper_atmosphere_height_anomaly_m,
            ),
            monthly_sea_surface_height_anomaly_m,
            monthly_thermocline_height_anomaly_m: Some(monthly_thermocline_height_anomaly_m),
            monthly_deep_ocean_temperature_c: Some(monthly_deep_ocean_temperature_c),
        };
        fields.validate_cancellable(
            ClimateModelProfile::C2LayeredV1,
            fields.cell_count(),
            cancelled,
        )?;
        Ok(fields)
    }

    fn inferred_profile(&self) -> Result<ClimateModelProfile, GlobalCirculationValidationError> {
        let optional_presence = [
            self.upper_wind_m_s.is_some(),
            self.vertical_wind_shear_m_s.is_some(),
            self.monthly_thermocline_temperature_c.is_some(),
            self.monthly_thermocline_depth_m.is_some(),
            self.monthly_upper_atmosphere_height_anomaly_m.is_some(),
            self.monthly_thermocline_height_anomaly_m.is_some(),
            self.monthly_deep_ocean_temperature_c.is_some(),
        ];
        if optional_presence.iter().all(|present| !present) {
            Ok(ClimateModelProfile::C1SingleLayerV1)
        } else if optional_presence.iter().all(|present| *present) {
            Ok(ClimateModelProfile::C2LayeredV1)
        } else {
            Err(GlobalCirculationValidationError::IncompleteVerticalFields)
        }
    }

    pub fn validate(
        &self,
        profile: ClimateModelProfile,
        expected_cells: usize,
    ) -> Result<(), GlobalCirculationValidationError> {
        self.validate_impl(profile, expected_cells, None)
    }

    /// Rechecks every dense field while cooperatively polling a caller-owned
    /// cancellation predicate. World data stays independent of the engine's
    /// cancellation type.
    pub fn validate_cancellable(
        &self,
        profile: ClimateModelProfile,
        expected_cells: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), GlobalCirculationValidationError> {
        self.validate_impl(profile, expected_cells, Some(cancelled))
    }

    fn validate_impl(
        &self,
        profile: ClimateModelProfile,
        expected_cells: usize,
        cancellation: CancellationCheck<'_>,
    ) -> Result<(), GlobalCirculationValidationError> {
        check_global_circulation_cancelled(cancellation)?;
        let inferred = self.inferred_profile()?;
        if inferred != profile {
            return Err(GlobalCirculationValidationError::FieldProfileMismatch {
                fields: inferred,
                snapshot: profile,
            });
        }
        if expected_cells == 0 {
            return Err(GlobalCirculationValidationError::EmptyFields);
        }
        for (field, found) in self.field_lengths() {
            if found != expected_cells {
                return Err(GlobalCirculationValidationError::FieldLengthMismatch {
                    field,
                    found,
                    expected: expected_cells,
                });
            }
        }
        validate_monthly_vector3(
            "near_surface_wind_m_s",
            &self.near_surface_wind_m_s,
            200.0,
            cancellation,
        )?;
        validate_monthly_vector3(
            "surface_ocean_current_m_s",
            &self.surface_ocean_current_m_s,
            20.0,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_air_temperature_c",
            &self.monthly_air_temperature_c,
            -120.0,
            80.0,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_sea_surface_temperature_c",
            &self.monthly_sea_surface_temperature_c,
            -5.0,
            60.0,
            cancellation,
        )?;
        validate_scalar_values(
            "surface_albedo",
            &self.surface_albedo,
            0.0,
            1.0,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_absorbed_shortwave_w_m2",
            &self.monthly_absorbed_shortwave_w_m2,
            0.0,
            GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2 as f32,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_outgoing_longwave_w_m2",
            &self.monthly_outgoing_longwave_w_m2,
            0.0,
            GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2 as f32,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_specific_humidity",
            &self.monthly_specific_humidity,
            0.0,
            P4_MAX_SPECIFIC_HUMIDITY_KG_KG as f32,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_evaporation_mm_day",
            &self.monthly_evaporation_mm_day,
            0.0,
            f32::MAX,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_precipitation_mm_day",
            &self.monthly_precipitation_mm_day,
            0.0,
            f32::MAX,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_orographic_precipitation_mm_day",
            &self.monthly_orographic_precipitation_mm_day,
            0.0,
            f32::MAX,
            cancellation,
        )?;
        validate_orographic_precipitation_identity(
            &self.monthly_precipitation_mm_day,
            &self.monthly_orographic_precipitation_mm_day,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_lower_atmosphere_height_anomaly_m",
            &self.monthly_lower_atmosphere_height_anomaly_m,
            -20_000.0,
            20_000.0,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_sea_surface_height_anomaly_m",
            &self.monthly_sea_surface_height_anomaly_m,
            -100.0,
            100.0,
            cancellation,
        )?;

        if profile == ClimateModelProfile::C2LayeredV1 {
            let upper = self.upper_wind_m_s.as_ref().expect("inferred C2");
            let shear = self.vertical_wind_shear_m_s.as_ref().expect("inferred C2");
            validate_monthly_vector3("upper_wind_m_s", upper, 200.0, cancellation)?;
            validate_monthly_vector3("vertical_wind_shear_m_s", shear, 300.0, cancellation)?;
            validate_shear_identity(&self.near_surface_wind_m_s, upper, shear, cancellation)?;
            validate_monthly_scalar(
                "monthly_thermocline_temperature_c",
                self.monthly_thermocline_temperature_c
                    .as_ref()
                    .expect("inferred C2"),
                -5.0,
                50.0,
                cancellation,
            )?;
            validate_monthly_scalar(
                "monthly_thermocline_depth_m",
                self.monthly_thermocline_depth_m
                    .as_ref()
                    .expect("inferred C2"),
                1.0,
                5_000.0,
                cancellation,
            )?;
            validate_monthly_scalar(
                "monthly_upper_atmosphere_height_anomaly_m",
                self.monthly_upper_atmosphere_height_anomaly_m
                    .as_ref()
                    .expect("inferred C2"),
                -20_000.0,
                20_000.0,
                cancellation,
            )?;
            validate_monthly_scalar(
                "monthly_thermocline_height_anomaly_m",
                self.monthly_thermocline_height_anomaly_m
                    .as_ref()
                    .expect("inferred C2"),
                -1_000.0,
                1_000.0,
                cancellation,
            )?;
            validate_thermocline_depth_identity(
                self.monthly_thermocline_depth_m
                    .as_ref()
                    .expect("inferred C2"),
                self.monthly_thermocline_height_anomaly_m
                    .as_ref()
                    .expect("inferred C2"),
                cancellation,
            )?;
            validate_monthly_scalar(
                "monthly_deep_ocean_temperature_c",
                self.monthly_deep_ocean_temperature_c
                    .as_ref()
                    .expect("inferred C2"),
                -5.0,
                40.0,
                cancellation,
            )?;
        }
        Ok(())
    }

    fn field_lengths(&self) -> Vec<(&'static str, usize)> {
        let mut lengths = vec![
            ("near_surface_wind_m_s", self.near_surface_wind_m_s.len()),
            (
                "surface_ocean_current_m_s",
                self.surface_ocean_current_m_s.len(),
            ),
            (
                "monthly_air_temperature_c",
                self.monthly_air_temperature_c.len(),
            ),
            (
                "monthly_sea_surface_temperature_c",
                self.monthly_sea_surface_temperature_c.len(),
            ),
            ("surface_albedo", self.surface_albedo.len()),
            (
                "monthly_absorbed_shortwave_w_m2",
                self.monthly_absorbed_shortwave_w_m2.len(),
            ),
            (
                "monthly_outgoing_longwave_w_m2",
                self.monthly_outgoing_longwave_w_m2.len(),
            ),
            (
                "monthly_specific_humidity",
                self.monthly_specific_humidity.len(),
            ),
            (
                "monthly_evaporation_mm_day",
                self.monthly_evaporation_mm_day.len(),
            ),
            (
                "monthly_precipitation_mm_day",
                self.monthly_precipitation_mm_day.len(),
            ),
            (
                "monthly_orographic_precipitation_mm_day",
                self.monthly_orographic_precipitation_mm_day.len(),
            ),
            (
                "monthly_lower_atmosphere_height_anomaly_m",
                self.monthly_lower_atmosphere_height_anomaly_m.len(),
            ),
            (
                "monthly_sea_surface_height_anomaly_m",
                self.monthly_sea_surface_height_anomaly_m.len(),
            ),
        ];
        for (name, field) in [
            ("upper_wind_m_s", self.upper_wind_m_s.as_ref()),
            (
                "vertical_wind_shear_m_s",
                self.vertical_wind_shear_m_s.as_ref(),
            ),
        ] {
            if let Some(field) = field {
                lengths.push((name, field.len()));
            }
        }
        for (name, field) in [
            (
                "monthly_thermocline_temperature_c",
                self.monthly_thermocline_temperature_c.as_ref(),
            ),
            (
                "monthly_thermocline_depth_m",
                self.monthly_thermocline_depth_m.as_ref(),
            ),
            (
                "monthly_upper_atmosphere_height_anomaly_m",
                self.monthly_upper_atmosphere_height_anomaly_m.as_ref(),
            ),
            (
                "monthly_thermocline_height_anomaly_m",
                self.monthly_thermocline_height_anomaly_m.as_ref(),
            ),
            (
                "monthly_deep_ocean_temperature_c",
                self.monthly_deep_ocean_temperature_c.as_ref(),
            ),
        ] {
            if let Some(field) = field {
                lengths.push((name, field.len()));
            }
        }
        lengths
    }

    pub fn cell_count(&self) -> usize {
        self.near_surface_wind_m_s.len()
    }

    /// Canonical hash of every published semantic field, including optional
    /// field presence. This is the checkpoint state identity.
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint_impl(None)
            .expect("non-cancellable field hashing cannot be cancelled")
    }

    pub fn fingerprint_cancellable(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<[u8; 32], GlobalCirculationValidationError> {
        self.fingerprint_impl(Some(cancelled))
    }

    fn fingerprint_impl(
        &self,
        cancellation: CancellationCheck<'_>,
    ) -> Result<[u8; 32], GlobalCirculationValidationError> {
        check_global_circulation_cancelled(cancellation)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.global-circulation-state.v3\0");
        hash_monthly_vectors(
            &mut hasher,
            self.near_surface_wind_m_s.values(),
            cancellation,
        )?;
        hash_optional_monthly_vectors(&mut hasher, self.upper_wind_m_s.as_ref(), cancellation)?;
        hash_optional_monthly_vectors(
            &mut hasher,
            self.vertical_wind_shear_m_s.as_ref(),
            cancellation,
        )?;
        hash_monthly_vectors(
            &mut hasher,
            self.surface_ocean_current_m_s.values(),
            cancellation,
        )?;
        hash_scalar_values(&mut hasher, &self.surface_albedo, cancellation)?;
        for field in [
            &self.monthly_air_temperature_c,
            &self.monthly_sea_surface_temperature_c,
            &self.monthly_absorbed_shortwave_w_m2,
            &self.monthly_outgoing_longwave_w_m2,
            &self.monthly_specific_humidity,
            &self.monthly_evaporation_mm_day,
            &self.monthly_precipitation_mm_day,
            &self.monthly_orographic_precipitation_mm_day,
            &self.monthly_lower_atmosphere_height_anomaly_m,
            &self.monthly_sea_surface_height_anomaly_m,
        ] {
            hash_monthly_scalars(&mut hasher, field.values(), cancellation)?;
        }
        for field in [
            self.monthly_thermocline_temperature_c.as_ref(),
            self.monthly_thermocline_depth_m.as_ref(),
            self.monthly_upper_atmosphere_height_anomaly_m.as_ref(),
            self.monthly_thermocline_height_anomaly_m.as_ref(),
            self.monthly_deep_ocean_temperature_c.as_ref(),
        ] {
            hash_optional_monthly_scalars(&mut hasher, field, cancellation)?;
        }
        Ok(*hasher.finalize().as_bytes())
    }

    pub const fn near_surface_wind_m_s(&self) -> &MonthlyVector3Field {
        &self.near_surface_wind_m_s
    }

    pub const fn upper_wind_m_s(&self) -> Option<&MonthlyVector3Field> {
        self.upper_wind_m_s.as_ref()
    }

    pub const fn vertical_wind_shear_m_s(&self) -> Option<&MonthlyVector3Field> {
        self.vertical_wind_shear_m_s.as_ref()
    }

    pub const fn surface_ocean_current_m_s(&self) -> &MonthlyVector3Field {
        &self.surface_ocean_current_m_s
    }

    pub const fn monthly_air_temperature_c(&self) -> &MonthlyScalarField {
        &self.monthly_air_temperature_c
    }

    pub const fn monthly_sea_surface_temperature_c(&self) -> &MonthlyScalarField {
        &self.monthly_sea_surface_temperature_c
    }

    pub fn surface_albedo(&self) -> &[f32] {
        &self.surface_albedo
    }

    pub const fn monthly_absorbed_shortwave_w_m2(&self) -> &MonthlyScalarField {
        &self.monthly_absorbed_shortwave_w_m2
    }

    pub const fn monthly_outgoing_longwave_w_m2(&self) -> &MonthlyScalarField {
        &self.monthly_outgoing_longwave_w_m2
    }

    pub const fn monthly_thermocline_temperature_c(&self) -> Option<&MonthlyScalarField> {
        self.monthly_thermocline_temperature_c.as_ref()
    }

    pub const fn monthly_thermocline_depth_m(&self) -> Option<&MonthlyScalarField> {
        self.monthly_thermocline_depth_m.as_ref()
    }

    pub const fn monthly_specific_humidity(&self) -> &MonthlyScalarField {
        &self.monthly_specific_humidity
    }

    pub const fn monthly_evaporation_mm_day(&self) -> &MonthlyScalarField {
        &self.monthly_evaporation_mm_day
    }

    pub const fn monthly_precipitation_mm_day(&self) -> &MonthlyScalarField {
        &self.monthly_precipitation_mm_day
    }

    pub const fn monthly_orographic_precipitation_mm_day(&self) -> &MonthlyScalarField {
        &self.monthly_orographic_precipitation_mm_day
    }

    pub const fn monthly_lower_atmosphere_height_anomaly_m(&self) -> &MonthlyScalarField {
        &self.monthly_lower_atmosphere_height_anomaly_m
    }

    pub const fn monthly_upper_atmosphere_height_anomaly_m(&self) -> Option<&MonthlyScalarField> {
        self.monthly_upper_atmosphere_height_anomaly_m.as_ref()
    }

    pub const fn monthly_sea_surface_height_anomaly_m(&self) -> &MonthlyScalarField {
        &self.monthly_sea_surface_height_anomaly_m
    }

    pub const fn monthly_thermocline_height_anomaly_m(&self) -> Option<&MonthlyScalarField> {
        self.monthly_thermocline_height_anomaly_m.as_ref()
    }

    pub const fn monthly_deep_ocean_temperature_c(&self) -> Option<&MonthlyScalarField> {
        self.monthly_deep_ocean_temperature_c.as_ref()
    }
}

impl<'de> Deserialize<'de> for GlobalCirculationFields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GlobalCirculationFieldsWire::deserialize(deserializer)?;
        let fields = Self {
            near_surface_wind_m_s: wire.near_surface_wind_m_s,
            upper_wind_m_s: wire.upper_wind_m_s,
            vertical_wind_shear_m_s: wire.vertical_wind_shear_m_s,
            surface_ocean_current_m_s: wire.surface_ocean_current_m_s,
            monthly_air_temperature_c: wire.monthly_air_temperature_c,
            monthly_sea_surface_temperature_c: wire.monthly_sea_surface_temperature_c,
            surface_albedo: wire.surface_albedo,
            monthly_absorbed_shortwave_w_m2: wire.monthly_absorbed_shortwave_w_m2,
            monthly_outgoing_longwave_w_m2: wire.monthly_outgoing_longwave_w_m2,
            monthly_thermocline_temperature_c: wire.monthly_thermocline_temperature_c,
            monthly_thermocline_depth_m: wire.monthly_thermocline_depth_m,
            monthly_specific_humidity: wire.monthly_specific_humidity,
            monthly_evaporation_mm_day: wire.monthly_evaporation_mm_day,
            monthly_precipitation_mm_day: wire.monthly_precipitation_mm_day,
            monthly_orographic_precipitation_mm_day: wire.monthly_orographic_precipitation_mm_day,
            monthly_lower_atmosphere_height_anomaly_m: wire
                .monthly_lower_atmosphere_height_anomaly_m,
            monthly_upper_atmosphere_height_anomaly_m: wire
                .monthly_upper_atmosphere_height_anomaly_m,
            monthly_sea_surface_height_anomaly_m: wire.monthly_sea_surface_height_anomaly_m,
            monthly_thermocline_height_anomaly_m: wire.monthly_thermocline_height_anomaly_m,
            monthly_deep_ocean_temperature_c: wire.monthly_deep_ocean_temperature_c,
        };
        let profile = fields.inferred_profile().map_err(D::Error::custom)?;
        fields
            .validate(profile, fields.cell_count())
            .map_err(D::Error::custom)?;
        Ok(fields)
    }
}

type CancellationCheck<'a> = Option<&'a dyn Fn() -> bool>;

fn check_global_circulation_cancelled(
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    if cancellation.is_some_and(|cancelled| cancelled()) {
        Err(GlobalCirculationValidationError::Cancelled)
    } else {
        Ok(())
    }
}

fn validate_scalar_values(
    field: &'static str,
    values: &[f32],
    minimum: f32,
    maximum: f32,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, value) in values.iter().copied().enumerate() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        if !value.is_finite() || value < minimum || value > maximum {
            return Err(GlobalCirculationValidationError::ScalarOutOfRange {
                field,
                cell,
                month: 0,
                found: value,
                minimum,
                maximum,
            });
        }
    }
    Ok(())
}

fn validate_monthly_scalar(
    field: &'static str,
    values: &MonthlyScalarField,
    minimum: f32,
    maximum: f32,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, months) in values.values().iter().enumerate() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for (month, value) in months.iter().copied().enumerate() {
            if !value.is_finite() || value < minimum || value > maximum {
                return Err(GlobalCirculationValidationError::ScalarOutOfRange {
                    field,
                    cell,
                    month,
                    found: value,
                    minimum,
                    maximum,
                });
            }
        }
    }
    Ok(())
}

fn validate_monthly_vector3(
    field: &'static str,
    values: &MonthlyVector3Field,
    component_abs_max: f32,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, months) in values.values().iter().enumerate() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for (month, vector) in months.iter().enumerate() {
            for (component, value) in vector.iter().copied().enumerate() {
                if !value.is_finite() || value.abs() > component_abs_max {
                    return Err(GlobalCirculationValidationError::VectorOutOfRange {
                        field,
                        cell,
                        month,
                        component,
                        found: value,
                        component_abs_max,
                    });
                }
            }
        }
    }
    Ok(())
}

fn validate_shear_identity(
    lower: &MonthlyVector3Field,
    upper: &MonthlyVector3Field,
    shear: &MonthlyVector3Field,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for cell in 0..lower.len() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for month in 0..CLIMATE_MONTH_COUNT {
            for component in 0..3 {
                let expected =
                    upper.values()[cell][month][component] - lower.values()[cell][month][component];
                let found = shear.values()[cell][month][component];
                if found != expected {
                    return Err(GlobalCirculationValidationError::ShearIdentityMismatch {
                        cell,
                        month,
                        component,
                        found,
                        expected,
                    });
                }
            }
        }
    }
    Ok(())
}

fn validate_thermocline_depth_identity(
    depth: &MonthlyScalarField,
    height: &MonthlyScalarField,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    let reference = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1)
        .layers()
        .iter()
        .find(|layer| layer.role() == ClimateLayerRole::OceanThermocline)
        .expect("locked C2 thermocline")
        .reference_thickness_m() as f32;
    for cell in 0..depth.len() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for month in 0..CLIMATE_MONTH_COUNT {
            let expected = reference + height.values()[cell][month];
            let found = depth.values()[cell][month];
            if found != expected {
                return Err(
                    GlobalCirculationValidationError::ThermoclineDepthIdentityMismatch {
                        cell,
                        month,
                        found,
                        expected,
                    },
                );
            }
        }
    }
    Ok(())
}

fn validate_orographic_precipitation_identity(
    total: &MonthlyScalarField,
    orographic: &MonthlyScalarField,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for cell in 0..total.len() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for month in 0..CLIMATE_MONTH_COUNT {
            let total_value = total.values()[cell][month];
            let orographic_value = orographic.values()[cell][month];
            if orographic_value > total_value {
                return Err(
                    GlobalCirculationValidationError::OrographicPrecipitationExceedsTotal {
                        cell,
                        month,
                        orographic: orographic_value,
                        total: total_value,
                    },
                );
            }
        }
    }
    Ok(())
}

fn hash_scalar_values(
    hasher: &mut blake3::Hasher,
    values: &[f32],
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (index, value) in values.iter().enumerate() {
        if index % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        hasher.update(&value.to_bits().to_le_bytes());
    }
    Ok(())
}

fn hash_monthly_scalars(
    hasher: &mut blake3::Hasher,
    values: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, months) in values.iter().enumerate() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for value in months {
            hasher.update(&value.to_bits().to_le_bytes());
        }
    }
    Ok(())
}

fn hash_monthly_vectors(
    hasher: &mut blake3::Hasher,
    values: &[[[f32; 3]; CLIMATE_MONTH_COUNT]],
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, months) in values.iter().enumerate() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for vector in months {
            for value in vector {
                hasher.update(&value.to_bits().to_le_bytes());
            }
        }
    }
    Ok(())
}

fn hash_optional_monthly_scalars(
    hasher: &mut blake3::Hasher,
    field: Option<&MonthlyScalarField>,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    hasher.update(&[u8::from(field.is_some())]);
    if let Some(field) = field {
        hash_monthly_scalars(hasher, field.values(), cancellation)?;
    }
    Ok(())
}

fn hash_optional_monthly_vectors(
    hasher: &mut blake3::Hasher,
    field: Option<&MonthlyVector3Field>,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    hasher.update(&[u8::from(field.is_some())]);
    if let Some(field) = field {
        hash_monthly_vectors(hasher, field.values(), cancellation)?;
    }
    Ok(())
}

/// Immutable P4 seasonal atmosphere-ocean facts on the authoritative sphere.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCirculationSnapshot {
    schema_version: u16,
    surface_ref: SurfaceRef,
    layout: ClimateLayerLayout,
    integrator: ProductionIntegratorId,
    capabilities: ClimateCapabilitySet,
    checkpoint: ClimateCheckpoint,
    solve_report: ClimateSolveReport,
    budget_report: ClimateBudgetReport,
    remap_report: ClimateRemapReport,
    fields: GlobalCirculationFields,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobalCirculationSnapshotWire {
    schema_version: u16,
    surface_ref: SurfaceRef,
    layout: ClimateLayerLayout,
    integrator: ProductionIntegratorId,
    capabilities: ClimateCapabilitySet,
    checkpoint: ClimateCheckpoint,
    solve_report: ClimateSolveReport,
    budget_report: ClimateBudgetReport,
    remap_report: ClimateRemapReport,
    fields: GlobalCirculationFields,
}

impl GlobalCirculationSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        surface_ref: SurfaceRef,
        layout: ClimateLayerLayout,
        integrator: ProductionIntegratorId,
        capabilities: ClimateCapabilitySet,
        checkpoint: ClimateCheckpoint,
        solve_report: ClimateSolveReport,
        budget_report: ClimateBudgetReport,
        remap_report: ClimateRemapReport,
        fields: GlobalCirculationFields,
    ) -> Result<Self, GlobalCirculationValidationError> {
        Self::new_impl(
            schema_version,
            surface_ref,
            layout,
            integrator,
            capabilities,
            checkpoint,
            solve_report,
            budget_report,
            remap_report,
            fields,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_cancellable(
        schema_version: u16,
        surface_ref: SurfaceRef,
        layout: ClimateLayerLayout,
        integrator: ProductionIntegratorId,
        capabilities: ClimateCapabilitySet,
        checkpoint: ClimateCheckpoint,
        solve_report: ClimateSolveReport,
        budget_report: ClimateBudgetReport,
        remap_report: ClimateRemapReport,
        fields: GlobalCirculationFields,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, GlobalCirculationValidationError> {
        Self::new_impl(
            schema_version,
            surface_ref,
            layout,
            integrator,
            capabilities,
            checkpoint,
            solve_report,
            budget_report,
            remap_report,
            fields,
            Some(cancelled),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_impl(
        schema_version: u16,
        surface_ref: SurfaceRef,
        layout: ClimateLayerLayout,
        integrator: ProductionIntegratorId,
        capabilities: ClimateCapabilitySet,
        checkpoint: ClimateCheckpoint,
        solve_report: ClimateSolveReport,
        budget_report: ClimateBudgetReport,
        remap_report: ClimateRemapReport,
        fields: GlobalCirculationFields,
        cancellation: CancellationCheck<'_>,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let snapshot = Self {
            schema_version,
            surface_ref,
            layout,
            integrator,
            capabilities,
            checkpoint,
            solve_report,
            budget_report,
            remap_report,
            fields,
        };
        snapshot.validate_impl(cancellation)?;
        Ok(snapshot)
    }

    /// Rechecks invariants that require only serialized identities and fields.
    pub fn validate(&self) -> Result<(), GlobalCirculationValidationError> {
        self.validate_impl(None)
    }

    pub fn validate_cancellable(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), GlobalCirculationValidationError> {
        self.validate_impl(Some(cancelled))
    }

    fn validate_impl(
        &self,
        cancellation: CancellationCheck<'_>,
    ) -> Result<(), GlobalCirculationValidationError> {
        check_global_circulation_cancelled(cancellation)?;
        if self.schema_version != GLOBAL_CIRCULATION_SCHEMA_V2 {
            return Err(GlobalCirculationValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: GLOBAL_CIRCULATION_SCHEMA_V2,
            });
        }
        self.surface_ref.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "surface_ref",
                reason: error.to_string(),
            }
        })?;
        if self.surface_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(GlobalCirculationValidationError::InvalidSurfaceKind {
                found: self.surface_ref.geometry_kind(),
            });
        }
        self.layout.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "layout",
                reason: error.to_string(),
            }
        })?;
        self.capabilities.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "capabilities",
                reason: error.to_string(),
            }
        })?;
        self.checkpoint.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "checkpoint",
                reason: error.to_string(),
            }
        })?;
        self.solve_report.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "solve_report",
                reason: error.to_string(),
            }
        })?;
        self.budget_report.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "budget_report",
                reason: error.to_string(),
            }
        })?;
        self.remap_report.validate().map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "remap_report",
                reason: error.to_string(),
            }
        })?;

        let profile = self.layout.profile();
        if self.checkpoint.profile() != profile {
            return Err(
                GlobalCirculationValidationError::CheckpointIdentityMismatch { field: "profile" },
            );
        }
        if self.checkpoint.integrator() != self.integrator {
            return Err(
                GlobalCirculationValidationError::CheckpointIdentityMismatch {
                    field: "integrator",
                },
            );
        }
        if self.checkpoint.model_fingerprint()
            != &crate::generators::natural::formation::global_circulation_model_fingerprint(profile)
        {
            return Err(
                GlobalCirculationValidationError::CheckpointIdentityMismatch {
                    field: "model_fingerprint",
                },
            );
        }
        let expected_phase_steps = u32::from(self.solve_report.formation_cycles())
            .checked_mul(CLIMATE_MONTH_COUNT as u32)
            .ok_or(GlobalCirculationValidationError::SolveWorkMismatch {
                field: "formation_cycles",
            })?;
        if self.checkpoint.completed_phase_steps() != expected_phase_steps {
            return Err(GlobalCirculationValidationError::SolveWorkMismatch {
                field: "completed_phase_steps",
            });
        }
        if self.solve_report.continuation_steps() != u64::from(expected_phase_steps) {
            return Err(GlobalCirculationValidationError::SolveWorkMismatch {
                field: "continuation_steps",
            });
        }
        if self.integrator == ProductionIntegratorId::SplitExplicitRk3V1
            && self.solve_report.linear_iterations() != 0
        {
            return Err(GlobalCirculationValidationError::SolveWorkMismatch {
                field: "linear_iterations",
            });
        }
        let minimum_fast_substeps = self
            .solve_report
            .continuation_steps()
            .checked_mul(6)
            .ok_or(GlobalCirculationValidationError::SolveWorkMismatch {
                field: "fast_substeps",
            })?;
        if self.integrator == ProductionIntegratorId::SplitExplicitRk3V1
            && self.solve_report.fast_substeps() < minimum_fast_substeps
        {
            return Err(GlobalCirculationValidationError::SolveWorkMismatch {
                field: "fast_substeps",
            });
        }
        let expected_dense_state_bytes = expected_global_circulation_dense_state_bytes(
            self.checkpoint.quality_profile(),
            profile,
            self.surface_ref.cell_count(),
        )
        .ok_or(GlobalCirculationValidationError::SolveWorkMismatch {
            field: "dense_state_bytes",
        })?;
        if self.solve_report.dense_state_bytes() != expected_dense_state_bytes {
            return Err(GlobalCirculationValidationError::SolveWorkMismatch {
                field: "dense_state_bytes",
            });
        }
        if self.capabilities != ClimateCapabilitySet::for_profile(profile) {
            return Err(GlobalCirculationValidationError::CapabilityProfileMismatch { profile });
        }
        self.fields.validate_impl(
            profile,
            self.surface_ref.cell_count() as usize,
            cancellation,
        )?;
        validate_positive_layer_depths(&self.layout, &self.fields, cancellation)?;
        if self.checkpoint.state_fingerprint() != &self.fields.fingerprint_impl(cancellation)? {
            return Err(
                GlobalCirculationValidationError::CheckpointIdentityMismatch {
                    field: "state_fingerprint",
                },
            );
        }
        Ok(())
    }

    /// Rechecks exact surface identity and every published vector's tangency.
    pub fn validate_against(
        &self,
        surface: &SphericalSurfaceSnapshot,
    ) -> Result<(), GlobalCirculationValidationError> {
        self.validate_against_impl(surface, None)
    }

    pub fn validate_against_cancellable(
        &self,
        surface: &SphericalSurfaceSnapshot,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), GlobalCirculationValidationError> {
        self.validate_against_impl(surface, Some(cancelled))
    }

    fn validate_against_impl(
        &self,
        surface: &SphericalSurfaceSnapshot,
        cancellation: CancellationCheck<'_>,
    ) -> Result<(), GlobalCirculationValidationError> {
        self.validate_impl(cancellation)?;
        check_global_circulation_cancelled(cancellation)?;
        // Safe Rust can only supply an immutable typed surface that was
        // validated by construction or deserialization. Preserve the full
        // standalone audit for the non-cancellable API, while the production
        // path checks identity and polls all snapshot/tangent scans.
        if cancellation.is_none() {
            surface.validate().map_err(|error| {
                GlobalCirculationValidationError::InvalidNested {
                    role: "authoritative_surface",
                    reason: error.to_string(),
                }
            })?;
        }
        check_global_circulation_cancelled(cancellation)?;
        let authoritative = SurfaceRef::from_validated_spherical(surface).map_err(|error| {
            GlobalCirculationValidationError::InvalidNested {
                role: "authoritative_surface_identity",
                reason: error.to_string(),
            }
        })?;
        if authoritative != self.surface_ref {
            return Err(GlobalCirculationValidationError::SurfaceMismatch {
                snapshot: self.surface_ref,
                authoritative,
            });
        }
        let reconstructed_grid = match cancellation {
            Some(cancelled) => {
                crate::generators::natural::circulation::CubedSphereGrid::new_cancellable(
                    self.checkpoint.quality_profile().climate_face_resolution(),
                    surface.radius().get(),
                    cancelled,
                )
            }
            None => crate::generators::natural::circulation::CubedSphereGrid::new(
                self.checkpoint.quality_profile().climate_face_resolution(),
                surface.radius().get(),
            ),
        }
        .map_err(|error| {
            if error == crate::generators::natural::circulation::CubedSphereGridError::Cancelled {
                GlobalCirculationValidationError::Cancelled
            } else {
                GlobalCirculationValidationError::InvalidNested {
                    role: "checkpoint_grid",
                    reason: error.to_string(),
                }
            }
        })?;
        if self.checkpoint.grid_fingerprint() != reconstructed_grid.fingerprint() {
            return Err(
                GlobalCirculationValidationError::CheckpointIdentityMismatch {
                    field: "grid_fingerprint",
                },
            );
        }
        validate_tangent_field(
            "near_surface_wind_m_s",
            self.fields.near_surface_wind_m_s(),
            surface,
            cancellation,
        )?;
        validate_tangent_field(
            "surface_ocean_current_m_s",
            self.fields.surface_ocean_current_m_s(),
            surface,
            cancellation,
        )?;
        if let Some(field) = self.fields.upper_wind_m_s() {
            validate_tangent_field("upper_wind_m_s", field, surface, cancellation)?;
        }
        if let Some(field) = self.fields.vertical_wind_shear_m_s() {
            validate_tangent_field("vertical_wind_shear_m_s", field, surface, cancellation)?;
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn surface_ref(&self) -> SurfaceRef {
        self.surface_ref
    }

    pub const fn profile(&self) -> ClimateModelProfile {
        self.layout.profile()
    }

    pub const fn layout(&self) -> &ClimateLayerLayout {
        &self.layout
    }

    pub const fn integrator(&self) -> ProductionIntegratorId {
        self.integrator
    }

    pub const fn capabilities(&self) -> &ClimateCapabilitySet {
        &self.capabilities
    }

    pub const fn checkpoint(&self) -> &ClimateCheckpoint {
        &self.checkpoint
    }

    pub const fn solve_report(&self) -> &ClimateSolveReport {
        &self.solve_report
    }

    pub const fn budget_report(&self) -> &ClimateBudgetReport {
        &self.budget_report
    }

    pub const fn remap_report(&self) -> &ClimateRemapReport {
        &self.remap_report
    }

    pub const fn fields(&self) -> &GlobalCirculationFields {
        &self.fields
    }
}

fn validate_positive_layer_depths(
    layout: &ClimateLayerLayout,
    fields: &GlobalCirculationFields,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    let reference = |role| {
        layout
            .layers()
            .iter()
            .find(|layer| layer.role() == role)
            .expect("validated fixed layout contains every profile layer")
            .reference_thickness_m()
    };
    validate_positive_layer_depth(
        ClimateLayerRole::LowerAtmosphere,
        reference(ClimateLayerRole::LowerAtmosphere),
        fields.monthly_lower_atmosphere_height_anomaly_m(),
        cancellation,
    )?;
    validate_positive_layer_depth(
        ClimateLayerRole::OceanMixedLayer,
        reference(ClimateLayerRole::OceanMixedLayer),
        fields.monthly_sea_surface_height_anomaly_m(),
        cancellation,
    )?;
    if let Some(upper) = fields.monthly_upper_atmosphere_height_anomaly_m() {
        validate_positive_layer_depth(
            ClimateLayerRole::UpperAtmosphere,
            reference(ClimateLayerRole::UpperAtmosphere),
            upper,
            cancellation,
        )?;
    }
    if let Some(thermocline) = fields.monthly_thermocline_height_anomaly_m() {
        validate_positive_layer_depth(
            ClimateLayerRole::OceanThermocline,
            reference(ClimateLayerRole::OceanThermocline),
            thermocline,
            cancellation,
        )?;
    }
    Ok(())
}

fn validate_positive_layer_depth(
    role: ClimateLayerRole,
    reference_m: f64,
    anomaly: &MonthlyScalarField,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, months) in anomaly.values().iter().enumerate() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for (month, anomaly_m) in months.iter().copied().enumerate() {
            let depth_m = reference_m + f64::from(anomaly_m);
            if depth_m <= 0.0 {
                return Err(GlobalCirculationValidationError::NonPositiveLayerDepth {
                    role,
                    cell,
                    month,
                    reference_m,
                    anomaly_m,
                    depth_m,
                });
            }
        }
    }
    Ok(())
}

impl<'de> Deserialize<'de> for GlobalCirculationSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GlobalCirculationSnapshotWire::deserialize(deserializer)?;
        Self::new(
            wire.schema_version,
            wire.surface_ref,
            wire.layout,
            wire.integrator,
            wire.capabilities,
            wire.checkpoint,
            wire.solve_report,
            wire.budget_report,
            wire.remap_report,
            wire.fields,
        )
        .map_err(D::Error::custom)
    }
}

fn validate_tangent_field(
    field: &'static str,
    values: &MonthlyVector3Field,
    surface: &SphericalSurfaceSnapshot,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, record) in surface.cells().iter().enumerate() {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        let radial = record.centroid.components();
        for (month, vector) in values.values()[cell].iter().enumerate() {
            let radial_component = f64::from(vector[0]) * radial[0]
                + f64::from(vector[1]) * radial[1]
                + f64::from(vector[2]) * radial[2];
            if radial_component.abs() > GLOBAL_CIRCULATION_TANGENCY_TOLERANCE_M_S {
                return Err(GlobalCirculationValidationError::NonTangentVector {
                    field,
                    cell: record.id,
                    month,
                    radial_component,
                });
            }
        }
    }
    Ok(())
}

/// Invalid public layered climate data or contradictory numerical evidence.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GlobalCirculationValidationError {
    #[error("global circulation validation was cancelled")]
    Cancelled,
    #[error("unsupported global circulation schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("invalid global circulation {role}: {reason}")]
    InvalidNested { role: &'static str, reason: String },
    #[error(
        "global circulation requires a spherical Voronoi V1 authoritative surface, found {found:?}"
    )]
    InvalidSurfaceKind { found: SurfaceGeometryKind },
    #[error("global circulation fields cannot be empty")]
    EmptyFields,
    #[error("vertical fields must be either the complete C2 set or all absent for C1")]
    IncompleteVerticalFields,
    #[error("field set implies {fields:?}, but snapshot declares {snapshot:?}")]
    FieldProfileMismatch {
        fields: ClimateModelProfile,
        snapshot: ClimateModelProfile,
    },
    #[error(
        "{role:?} layer depth is non-positive at cell {cell}, month {month}: {reference_m} + {anomaly_m} = {depth_m} m"
    )]
    NonPositiveLayerDepth {
        role: ClimateLayerRole,
        cell: usize,
        month: usize,
        reference_m: f64,
        anomaly_m: f32,
        depth_m: f64,
    },
    #[error("{field} has {found} cells, expected {expected}")]
    FieldLengthMismatch {
        field: &'static str,
        found: usize,
        expected: usize,
    },
    #[error("{field}[{cell}][{month}]={found} is outside {minimum}..={maximum}")]
    ScalarOutOfRange {
        field: &'static str,
        cell: usize,
        month: usize,
        found: f32,
        minimum: f32,
        maximum: f32,
    },
    #[error("{field}[{cell}][{month}][{component}]={found} exceeds component magnitude {component_abs_max}")]
    VectorOutOfRange {
        field: &'static str,
        cell: usize,
        month: usize,
        component: usize,
        found: f32,
        component_abs_max: f32,
    },
    #[error("vertical shear identity failed at cell {cell}, month {month}, component {component}: {found} != {expected}")]
    ShearIdentityMismatch {
        cell: usize,
        month: usize,
        component: usize,
        found: f32,
        expected: f32,
    },
    #[error(
        "thermocline depth identity failed at cell {cell}, month {month}: {found} != {expected}"
    )]
    ThermoclineDepthIdentityMismatch {
        cell: usize,
        month: usize,
        found: f32,
        expected: f32,
    },
    #[error(
        "orographic precipitation exceeds total precipitation at cell {cell}, month {month}: {orographic} > {total}"
    )]
    OrographicPrecipitationExceedsTotal {
        cell: usize,
        month: usize,
        orographic: f32,
        total: f32,
    },
    #[error("checkpoint {field} does not match snapshot identity")]
    CheckpointIdentityMismatch { field: &'static str },
    #[error("solve report and checkpoint disagree about {field}")]
    SolveWorkMismatch { field: &'static str },
    #[error("capabilities do not equal the locked P4 inventory for {profile:?}")]
    CapabilityProfileMismatch { profile: ClimateModelProfile },
    #[error(
        "snapshot surface {snapshot:?} does not match authoritative surface {authoritative:?}"
    )]
    SurfaceMismatch {
        snapshot: SurfaceRef,
        authoritative: SurfaceRef,
    },
    #[error("{field} at {cell:?}, month {month} has radial component {radial_component} m/s")]
    NonTangentVector {
        field: &'static str,
        cell: CellId,
        month: usize,
        radial_component: f64,
    },
}

/// A cubed-sphere climate grid and the exact conservative bridges to one
/// authoritative geodesic surface.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateWorkDomainSnapshot {
    schema_version: u16,
    profile: NaturalQualityProfile,
    face_resolution: u16,
    source_ref: SurfaceRef,
    climate_grid_fingerprint: [u8; 32],
    climate_surface: SphericalSurfaceSnapshot,
    source_to_climate: ConservativeSurfaceMap,
    climate_to_source: ConservativeSurfaceMap,
}

type WorkDomainCancellation<'a> = Option<&'a dyn Fn() -> bool>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClimateWorkDomainSnapshotWire {
    schema_version: u16,
    profile: NaturalQualityProfile,
    face_resolution: u16,
    source_ref: SurfaceRef,
    climate_grid_fingerprint: [u8; 32],
    climate_surface: SphericalSurfaceSnapshot,
    source_to_climate: ConservativeSurfaceMap,
    climate_to_source: ConservativeSurfaceMap,
}

impl ClimateWorkDomainSnapshot {
    /// Constructs the domain only after all cross-object identities close.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema_version: u16,
        profile: NaturalQualityProfile,
        face_resolution: u16,
        source_ref: SurfaceRef,
        climate_grid_fingerprint: [u8; 32],
        climate_surface: SphericalSurfaceSnapshot,
        source_to_climate: ConservativeSurfaceMap,
        climate_to_source: ConservativeSurfaceMap,
    ) -> Result<Self, ClimateWorkDomainValidationError> {
        let snapshot = Self {
            schema_version,
            profile,
            face_resolution,
            source_ref,
            climate_grid_fingerprint,
            climate_surface,
            source_to_climate,
            climate_to_source,
        };
        snapshot.validate()?;
        crate::generators::natural::validate_climate_work_domain_reconstruction(&snapshot)
            .map_err(
                |error| ClimateWorkDomainValidationError::NonCanonicalClimateGrid {
                    reason: error.to_string(),
                },
            )?;
        Ok(snapshot)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_cancellable(
        schema_version: u16,
        profile: NaturalQualityProfile,
        face_resolution: u16,
        source_ref: SurfaceRef,
        climate_grid_fingerprint: [u8; 32],
        climate_surface: SphericalSurfaceSnapshot,
        source_to_climate: ConservativeSurfaceMap,
        climate_to_source: ConservativeSurfaceMap,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, ClimateWorkDomainValidationError> {
        let snapshot = Self {
            schema_version,
            profile,
            face_resolution,
            source_ref,
            climate_grid_fingerprint,
            climate_surface,
            source_to_climate,
            climate_to_source,
        };
        snapshot.validate_cancellable(cancelled)?;
        check_work_domain_cancelled(Some(cancelled))?;
        crate::generators::natural::validate_climate_work_domain_reconstruction_cancellable(
            &snapshot, cancelled,
        )
        .map_err(|error| {
            if error == crate::generators::natural::ClimateWorkDomainBuildError::Cancelled {
                ClimateWorkDomainValidationError::Cancelled
            } else {
                ClimateWorkDomainValidationError::NonCanonicalClimateGrid {
                    reason: error.to_string(),
                }
            }
        })?;
        check_work_domain_cancelled(Some(cancelled))?;
        Ok(snapshot)
    }

    /// Rechecks the self-contained schema, topology counts, and map identities.
    pub fn validate(&self) -> Result<(), ClimateWorkDomainValidationError> {
        self.validate_impl(None)
    }

    pub fn validate_cancellable(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), ClimateWorkDomainValidationError> {
        self.validate_impl(Some(cancelled))
    }

    fn validate_impl(
        &self,
        cancellation: WorkDomainCancellation<'_>,
    ) -> Result<(), ClimateWorkDomainValidationError> {
        check_work_domain_cancelled(cancellation)?;
        if self.schema_version != CLIMATE_WORK_DOMAIN_SCHEMA_V1 {
            return Err(ClimateWorkDomainValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: CLIMATE_WORK_DOMAIN_SCHEMA_V1,
            });
        }
        let expected_resolution = self.profile.climate_face_resolution();
        if self.face_resolution != expected_resolution {
            return Err(ClimateWorkDomainValidationError::FaceResolutionMismatch {
                profile: self.profile,
                found: self.face_resolution,
                expected: expected_resolution,
            });
        }
        self.source_ref.validate().map_err(|error| {
            ClimateWorkDomainValidationError::InvalidSourceRef {
                reason: error.to_string(),
            }
        })?;
        if self.source_ref.geometry_kind() != SurfaceGeometryKind::SphericalV1 {
            return Err(ClimateWorkDomainValidationError::NonSphericalSource);
        }
        if self.climate_grid_fingerprint == [0; 32] {
            return Err(ClimateWorkDomainValidationError::ZeroGridFingerprint);
        }
        match cancellation {
            Some(cancelled) => self.climate_surface.validate_cancellable(cancelled),
            None => self.climate_surface.validate(),
        }
        .map_err(|error| {
            if error == crate::world::spatial::SphericalSurfaceValidationError::Cancelled {
                ClimateWorkDomainValidationError::Cancelled
            } else {
                ClimateWorkDomainValidationError::InvalidClimateSurface {
                    reason: error.to_string(),
                }
            }
        })?;

        let resolution = u32::from(self.face_resolution);
        let expected_cells = 6_u32 * resolution * resolution;
        let expected_edges = 2 * expected_cells;
        let expected_vertices = expected_cells + 2;
        let found_cells = self.climate_surface.cells().len() as u32;
        let found_edges = self.climate_surface.edges().len() as u32;
        let found_vertices = self.climate_surface.vertices().len() as u32;
        if (found_cells, found_edges, found_vertices)
            != (expected_cells, expected_edges, expected_vertices)
        {
            return Err(ClimateWorkDomainValidationError::CubedSphereCountMismatch {
                found_cells,
                found_edges,
                found_vertices,
                expected_cells,
                expected_edges,
                expected_vertices,
            });
        }

        validate_work_domain_map("source_to_climate", &self.source_to_climate, cancellation)?;
        validate_work_domain_map("climate_to_source", &self.climate_to_source, cancellation)?;
        let climate_ref =
            SurfaceRef::from_validated_spherical(&self.climate_surface).map_err(|error| {
                ClimateWorkDomainValidationError::InvalidClimateSurface {
                    reason: error.to_string(),
                }
            })?;
        validate_map_identity(
            "source_to_climate",
            &self.source_to_climate,
            self.source_ref,
            climate_ref,
        )?;
        validate_map_identity(
            "climate_to_source",
            &self.climate_to_source,
            climate_ref,
            self.source_ref,
        )?;
        validate_map_surface_areas(
            "source_to_climate target",
            self.source_to_climate.target_cell_areas_m2(),
            &self.climate_surface,
            cancellation,
        )?;
        validate_map_surface_areas(
            "climate_to_source source",
            self.climate_to_source.source_cell_areas_m2(),
            &self.climate_surface,
            cancellation,
        )?;
        validate_matching_map_areas(
            "authoritative source",
            self.source_to_climate.source_cell_areas_m2(),
            self.climate_to_source.target_cell_areas_m2(),
            cancellation,
        )?;
        validate_matching_map_areas(
            "climate surface",
            self.source_to_climate.target_cell_areas_m2(),
            self.climate_to_source.source_cell_areas_m2(),
            cancellation,
        )?;
        check_work_domain_cancelled(cancellation)?;
        Ok(())
    }

    /// Binds the serialized source identity and radius to the supplied surface.
    pub fn validate_against(
        &self,
        source: &SphericalSurfaceSnapshot,
    ) -> Result<(), ClimateWorkDomainValidationError> {
        self.validate_against_impl(source, None)
    }

    pub fn validate_against_cancellable(
        &self,
        source: &SphericalSurfaceSnapshot,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), ClimateWorkDomainValidationError> {
        self.validate_against_impl(source, Some(cancelled))
    }

    fn validate_against_impl(
        &self,
        source: &SphericalSurfaceSnapshot,
        cancellation: WorkDomainCancellation<'_>,
    ) -> Result<(), ClimateWorkDomainValidationError> {
        self.validate_impl(cancellation)?;
        match cancellation {
            Some(cancelled) => source.validate_cancellable(cancelled),
            None => source.validate(),
        }
        .map_err(|error| {
            if error == crate::world::spatial::SphericalSurfaceValidationError::Cancelled {
                ClimateWorkDomainValidationError::Cancelled
            } else {
                ClimateWorkDomainValidationError::InvalidAuthoritativeSurface {
                    reason: error.to_string(),
                }
            }
        })?;
        let found_ref = SurfaceRef::from_validated_spherical(source).map_err(|error| {
            ClimateWorkDomainValidationError::InvalidAuthoritativeSurface {
                reason: error.to_string(),
            }
        })?;
        if found_ref != self.source_ref {
            return Err(ClimateWorkDomainValidationError::SourceMismatch {
                stored: self.source_ref,
                found: found_ref,
            });
        }
        if source.radius().get().to_bits() != self.climate_surface.radius().get().to_bits() {
            return Err(ClimateWorkDomainValidationError::RadiusMismatch {
                source_m: source.radius().get(),
                climate_m: self.climate_surface.radius().get(),
            });
        }
        validate_map_surface_areas(
            "source_to_climate source",
            self.source_to_climate.source_cell_areas_m2(),
            source,
            cancellation,
        )?;
        validate_map_surface_areas(
            "climate_to_source target",
            self.climate_to_source.target_cell_areas_m2(),
            source,
            cancellation,
        )?;
        crate::generators::natural::validate_climate_work_domain_maps_against(
            self,
            source,
            cancellation,
        )
        .map_err(|error| {
            if error == crate::generators::natural::ClimateWorkDomainBuildError::Cancelled {
                ClimateWorkDomainValidationError::Cancelled
            } else {
                ClimateWorkDomainValidationError::NonCanonicalConservativeMaps {
                    reason: error.to_string(),
                }
            }
        })?;
        check_work_domain_cancelled(cancellation)?;
        Ok(())
    }

    /// Fingerprints the exact climate work domain, including both directed
    /// conservative maps rather than only their endpoint surfaces.
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint_impl(None)
            .expect("an uncancelled work-domain fingerprint cannot fail")
    }

    pub fn fingerprint_cancellable(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<[u8; 32], ConservativeSurfaceMapError> {
        self.fingerprint_impl(Some(cancelled))
    }

    fn fingerprint_impl(
        &self,
        cancellation: Option<&dyn Fn() -> bool>,
    ) -> Result<[u8; 32], ConservativeSurfaceMapError> {
        if cancellation.is_some_and(|cancelled| cancelled()) {
            return Err(ConservativeSurfaceMapError::Cancelled);
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"sekai.climate-work-domain.v1\0");
        hasher.update(&self.schema_version.to_le_bytes());
        let profile_tag = match self.profile {
            NaturalQualityProfile::Draft => 0_u8,
            NaturalQualityProfile::Standard => 1,
            NaturalQualityProfile::High => 2,
        };
        hasher.update(&[profile_tag]);
        hasher.update(&self.face_resolution.to_le_bytes());
        let source_kind_tag = match self.source_ref.geometry_kind() {
            SurfaceGeometryKind::PlanarV1 => 0_u8,
            SurfaceGeometryKind::SphericalV1 => 1,
            SurfaceGeometryKind::SphericalGeodesicV2 => 2,
        };
        hasher.update(&[source_kind_tag]);
        hasher.update(&self.source_ref.geometry_schema().to_le_bytes());
        hasher.update(&self.source_ref.cell_count().to_le_bytes());
        hasher.update(&self.source_ref.edge_count().to_le_bytes());
        hasher.update(&self.source_ref.fingerprint());
        hasher.update(&self.climate_grid_fingerprint);
        hasher.update(&self.climate_surface.fingerprint());
        let forward = match cancellation {
            Some(cancelled) => self.source_to_climate.fingerprint_cancellable(cancelled)?,
            None => self.source_to_climate.fingerprint(),
        };
        hasher.update(&forward);
        let reverse = match cancellation {
            Some(cancelled) => self.climate_to_source.fingerprint_cancellable(cancelled)?,
            None => self.climate_to_source.fingerprint(),
        };
        hasher.update(&reverse);
        Ok(*hasher.finalize().as_bytes())
    }

    /// Checks only the cross-object binding between two already validated,
    /// immutable snapshots. Constructors and deserializers establish the
    /// expensive internal topology/remap invariants once; hot generators use
    /// this constant-time boundary to avoid repeating them before cancellable
    /// work begins.
    pub(crate) fn validate_binding_against(
        &self,
        source: &SphericalSurfaceSnapshot,
    ) -> Result<(), ClimateWorkDomainValidationError> {
        let found_ref = SurfaceRef::from_validated_spherical(source).map_err(|error| {
            ClimateWorkDomainValidationError::InvalidAuthoritativeSurface {
                reason: error.to_string(),
            }
        })?;
        if found_ref != self.source_ref {
            return Err(ClimateWorkDomainValidationError::SourceMismatch {
                stored: self.source_ref,
                found: found_ref,
            });
        }
        if source.radius().get().to_bits() != self.climate_surface.radius().get().to_bits() {
            return Err(ClimateWorkDomainValidationError::RadiusMismatch {
                source_m: source.radius().get(),
                climate_m: self.climate_surface.radius().get(),
            });
        }
        Ok(())
    }

    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }

    pub const fn profile(&self) -> NaturalQualityProfile {
        self.profile
    }

    pub const fn face_resolution(&self) -> u16 {
        self.face_resolution
    }

    pub const fn source_ref(&self) -> SurfaceRef {
        self.source_ref
    }

    pub const fn climate_grid_fingerprint(&self) -> &[u8; 32] {
        &self.climate_grid_fingerprint
    }

    pub const fn climate_surface(&self) -> &SphericalSurfaceSnapshot {
        &self.climate_surface
    }

    pub const fn source_to_climate(&self) -> &ConservativeSurfaceMap {
        &self.source_to_climate
    }

    pub const fn climate_to_source(&self) -> &ConservativeSurfaceMap {
        &self.climate_to_source
    }
}

impl<'de> Deserialize<'de> for ClimateWorkDomainSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ClimateWorkDomainSnapshotWire::deserialize(deserializer)?;
        let snapshot = Self::new(
            wire.schema_version,
            wire.profile,
            wire.face_resolution,
            wire.source_ref,
            wire.climate_grid_fingerprint,
            wire.climate_surface,
            wire.source_to_climate,
            wire.climate_to_source,
        )
        .map_err(D::Error::custom)?;
        Ok(snapshot)
    }
}

fn validate_map_identity(
    role: &'static str,
    map: &ConservativeSurfaceMap,
    expected_source: SurfaceRef,
    expected_target: SurfaceRef,
) -> Result<(), ClimateWorkDomainValidationError> {
    if map.source_ref() != expected_source || map.target_ref() != expected_target {
        return Err(ClimateWorkDomainValidationError::MapIdentityMismatch { role });
    }
    Ok(())
}

fn validate_map_surface_areas(
    role: &'static str,
    stored: &[f64],
    surface: &SphericalSurfaceSnapshot,
    cancellation: WorkDomainCancellation<'_>,
) -> Result<(), ClimateWorkDomainValidationError> {
    if stored.len() != surface.cells().len() {
        return Err(ClimateWorkDomainValidationError::MapAreaCountMismatch {
            role,
            stored: stored.len(),
            expected: surface.cells().len(),
        });
    }
    for (index, (&stored_m2, cell)) in stored.iter().zip(surface.cells()).enumerate() {
        poll_work_domain_cancelled(index, cancellation)?;
        let expected_m2 = cell.area.get();
        if stored_m2.to_bits() != expected_m2.to_bits() {
            return Err(ClimateWorkDomainValidationError::MapSurfaceAreaMismatch {
                role,
                cell: CellId::from_raw(index as u32),
                stored_m2,
                expected_m2,
            });
        }
    }
    Ok(())
}

fn validate_matching_map_areas(
    role: &'static str,
    first: &[f64],
    second: &[f64],
    cancellation: WorkDomainCancellation<'_>,
) -> Result<(), ClimateWorkDomainValidationError> {
    if first.len() != second.len() {
        return Err(ClimateWorkDomainValidationError::MapAreaCountMismatch {
            role,
            stored: first.len(),
            expected: second.len(),
        });
    }
    for (index, (&first_m2, &second_m2)) in first.iter().zip(second).enumerate() {
        poll_work_domain_cancelled(index, cancellation)?;
        if first_m2.to_bits() != second_m2.to_bits() {
            return Err(ClimateWorkDomainValidationError::CrossMapAreaMismatch {
                role,
                cell: CellId::from_raw(index as u32),
                first_m2,
                second_m2,
            });
        }
    }
    Ok(())
}

fn validate_work_domain_map(
    role: &'static str,
    map: &ConservativeSurfaceMap,
    cancellation: WorkDomainCancellation<'_>,
) -> Result<(), ClimateWorkDomainValidationError> {
    let result = match cancellation {
        Some(cancelled) => {
            let mut cancelled = || cancelled();
            map.validate_cancellable(&mut cancelled)
        }
        None => map.validate(),
    };
    result.map_err(|error| {
        if error == ConservativeSurfaceMapError::Cancelled {
            ClimateWorkDomainValidationError::Cancelled
        } else {
            ClimateWorkDomainValidationError::InvalidMap {
                role,
                reason: error.to_string(),
            }
        }
    })
}

fn poll_work_domain_cancelled(
    index: usize,
    cancellation: WorkDomainCancellation<'_>,
) -> Result<(), ClimateWorkDomainValidationError> {
    if index % 256 == 0 {
        check_work_domain_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_work_domain_cancelled(
    cancellation: WorkDomainCancellation<'_>,
) -> Result<(), ClimateWorkDomainValidationError> {
    if cancellation.is_some_and(|cancelled| cancelled()) {
        Err(ClimateWorkDomainValidationError::Cancelled)
    } else {
        Ok(())
    }
}

/// Invalid serialized or cross-linked climate work-domain data.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ClimateWorkDomainValidationError {
    #[error("climate work-domain validation was cancelled")]
    Cancelled,
    #[error("unsupported climate work-domain schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u16, supported: u16 },
    #[error("{profile:?} climate face resolution is {found}, expected {expected}")]
    FaceResolutionMismatch {
        profile: NaturalQualityProfile,
        found: u16,
        expected: u16,
    },
    #[error("invalid authoritative source identity: {reason}")]
    InvalidSourceRef { reason: String },
    #[error("the climate work-domain source must be spherical")]
    NonSphericalSource,
    #[error("the climate work-grid fingerprint cannot be zero")]
    ZeroGridFingerprint,
    #[error("the climate work grid is not the canonical reconstructable cubed sphere: {reason}")]
    NonCanonicalClimateGrid { reason: String },
    #[error("invalid climate surface: {reason}")]
    InvalidClimateSurface { reason: String },
    #[error("cubed-sphere counts are cells={found_cells}, edges={found_edges}, vertices={found_vertices}; expected cells={expected_cells}, edges={expected_edges}, vertices={expected_vertices}")]
    CubedSphereCountMismatch {
        found_cells: u32,
        found_edges: u32,
        found_vertices: u32,
        expected_cells: u32,
        expected_edges: u32,
        expected_vertices: u32,
    },
    #[error("invalid {role} conservative map: {reason}")]
    InvalidMap { role: &'static str, reason: String },
    #[error("{role} map source/target identity does not match the work domain")]
    MapIdentityMismatch { role: &'static str },
    #[error("{role} map stores {stored} cell areas, expected {expected}")]
    MapAreaCountMismatch {
        role: &'static str,
        stored: usize,
        expected: usize,
    },
    #[error(
        "{role} map area at {cell:?} is {stored_m2} m^2, expected surface area {expected_m2} m^2"
    )]
    MapSurfaceAreaMismatch {
        role: &'static str,
        cell: CellId,
        stored_m2: f64,
        expected_m2: f64,
    },
    #[error(
        "the two directed maps disagree about {role} area at {cell:?}: {first_m2} vs {second_m2} m^2"
    )]
    CrossMapAreaMismatch {
        role: &'static str,
        cell: CellId,
        first_m2: f64,
        second_m2: f64,
    },
    #[error("invalid supplied authoritative surface: {reason}")]
    InvalidAuthoritativeSurface { reason: String },
    #[error("work-domain source identity {stored:?} does not match supplied surface {found:?}")]
    SourceMismatch {
        stored: SurfaceRef,
        found: SurfaceRef,
    },
    #[error("authoritative radius {source_m} m differs from climate radius {climate_m} m")]
    RadiusMismatch { source_m: f64, climate_m: f64 },
    #[error("conservative maps are not the canonical overlap geometry: {reason}")]
    NonCanonicalConservativeMaps { reason: String },
}
