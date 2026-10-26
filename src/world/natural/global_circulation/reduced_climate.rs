//! World-layer contract of the A10 reduced-order diagnostic seasonal climate (R1).
//!
//! Design truth: `docs/superpowers/specs/2026-10-25-a10-reduced-order-climate-design.md`,
//! frozen at revision R5 (section 18). The model replaces the prognostic C2 integration with a
//! seasonal moist energy-balance model (E1), diagnostic winds (W1/W2), a monthly quasi-steady
//! vapour budget (Q1) and a Stommel streamfunction (O1). This module owns the constants of
//! those equations, the published field shape, the solve and budget reports, and the model
//! identity; the solver lives in `generators`.
//!
//! Constants reused from the C2 model (thermodynamics, bulk transfer, condensation, albedo
//! priors) keep their single definitions in the parent module and enter the model identity
//! through their existing constant fingerprints.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use super::{
    check_global_circulation_cancelled, hash_monthly_scalars, hash_monthly_vectors,
    hash_scalar_values, p4_momentum_constants_fingerprint, p4_thermodynamic_constants_fingerprint,
    validate_monthly_scalar, validate_monthly_vector3, validate_orographic_precipitation_identity,
    validate_scalar_values, water_cycle_relative_imbalance, CancellationCheck, ClimateReportError,
    GlobalCirculationValidationError, GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX,
    GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX, GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2,
    GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2, GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX,
    P4_MAX_SPECIFIC_HUMIDITY_KG_KG,
};
use crate::world::natural::{MonthlyScalarField, MonthlyVector3Field, CLIMATE_MONTH_COUNT};

/// Published climate schema whose field set is the reduced R1 shape (design section 9).
pub const GLOBAL_CIRCULATION_SCHEMA_V3: u16 = 3;

/// MEBM eddy diffusivity of near-surface moist static energy, `D`.
///
/// Siler, Roe & Armour (2018), J. Climate 31, 7481, DOI 10.1175/JCLI-D-18-0081.1, Fig. 2b:
/// the best fit to the ERA-Interim zonal-mean 2 m temperature, within 10 % of Hwang & Frierson
/// (2010), Rose et al. (2014) and Roe et al. (2015). Design section 4 E1.
pub const REDUCED_CLIMATE_MEBM_DIFFUSIVITY_M2_S: f64 = 1.16e6;
/// Width of the Gaussian Hadley-cell weight `w(x) = exp(-x^2 / sigma_x^2)` in `x = sin(lat)`.
///
/// Siler, Roe & Armour (2018) Eq. (6). Design section 4 W2.
pub const REDUCED_CLIMATE_HADLEY_WEIGHT_WIDTH_SIN_LATITUDE: f64 = 0.3;
/// Ratio of the tropical upper-troposphere MSE to the near-surface MSE at the thermal equator.
///
/// Siler, Roe & Armour (2018) Eq. (8) text, `h_T = 1.06 h(0)`; the gross moist stability of
/// the lower branch is `h_T - [h]` with the band-mean `[h]` (design sections 4 W2 and 5.6).
pub const REDUCED_CLIMATE_HADLEY_UPPER_MSE_RATIO: f64 = 1.06;
/// Top of the lower Hadley branch, where most water vapour is concentrated.
///
/// Siler, Roe & Armour (2018) Fig. 2 caption: the layer below 900 hPa.
pub const REDUCED_CLIMATE_HADLEY_LOWER_BRANCH_TOP_PRESSURE_PA: f64 = 90_000.0;
/// Intercept `A` of the empirical outgoing-longwave closure `OLR = A + B T`, T in degC.
///
/// TransEBM v1.0 (Ziegler & Rehfeld 2021, GMD 14, 2843, DOI 10.5194/gmd-14-2843-2021,
/// section 2 and Table 1) value `A_ref` for 315 ppm CO2; Budyko (1969) form. Adopted by the
/// user decision in design section 14.1-2.
pub const TRANSEBM_OLR_INTERCEPT_W_M2: f64 = 210.2;
/// Slope `B` of the empirical outgoing-longwave closure `OLR = A + B T`.
///
/// TransEBM v1.0 (Ziegler & Rehfeld 2021, Table 1). Design section 15.3 measured the NOAA
/// interpolated OLR against NCEP R1 2 m temperature at 1.80-1.98 W m-2 K-1, bracketing it.
pub const TRANSEBM_OLR_SLOPE_W_M2_K: f64 = 2.15;
/// Annual-mean surface temperature below which a land surface is permanently ice or snow
/// covered.
///
/// Budyko (1969), Tellus 21, 611; North (1975), JAS 32, 2033 uses the same edge for the step
/// albedo of his energy-balance model. Design section 14.2.
pub const BUDYKO_ICE_EDGE_ANNUAL_TEMPERATURE_C: f64 = -10.0;
/// Amplitude of the Legendre latitude term `a_lat P2(sin lat)` of the ice-free ocean planetary
/// albedo.
///
/// TransEBM v1.0 (Ziegler & Rehfeld 2021, Table 1); it was tuned together with the `A + B T`
/// closure and carries the zenith-angle and cloud brightening of high latitudes. Design
/// section 15.4-1.
pub const TRANSEBM_OCEAN_LATITUDE_ALBEDO: f64 = 0.08;
/// Amplitude of the Legendre latitude term of the ice-free land planetary albedo.
///
/// TransEBM v1.0 (Ziegler & Rehfeld 2021, Table 1). Design section 15.4-1.
pub const TRANSEBM_LAND_LATITUDE_ALBEDO: f64 = 0.05;
/// Heat capacity of the sea-ice surface type, which replaces the mixed-layer capacity where
/// the ocean is ice covered.
///
/// TransEBM v1.0 (Ziegler & Rehfeld 2021, Table 1, after Zhuang, North & Stevens 2017); the
/// ice insulates the ocean from the atmosphere (Wagner & Eisenman 2015, J. Climate 28, 3998).
/// Design section 15.4-2.
pub const TRANSEBM_SEA_ICE_HEAT_CAPACITY_J_M2_K: f64 = 4.83e6;
/// Width of the absolute-latitude bands of [`SUBMONTHLY_WIND_COMPONENT_SIGMA_M_S`].
pub const SUBMONTHLY_WIND_SIGMA_BAND_WIDTH_DEGREES: f64 = 5.0;
/// Standard deviation of each horizontal 10 m wind component about its monthly vector mean
/// over the ocean, in [`SUBMONTHLY_WIND_SIGMA_BAND_WIDTH_DEGREES`] bands of absolute latitude
/// from the equator, hemispherically symmetrized.
///
/// Measured for design revision R3 (section 16.2-1) from NCEP/NCAR Reanalysis 1 6-hourly 10 m
/// winds 2016-2020 (Kalnay et al. 1996, BAMS 77, 437; NOAA PSL
/// `ncep.reanalysis/surface_gauss/{u,v}wnd.10m.gauss.YYYY.nc` with the `land.sfc.gauss.nc`
/// mask, file MD5 sums in `r3/obs-6h-wind/wind_variability_6h_ocean.json`): per ocean grid
/// point and calendar month `sigma^2 = (var u + var v) / 2`, cos-latitude weights, northern and
/// southern band means averaged (85-90 degrees has ocean only in the north). With these values
/// the Rice expectation of the speed reproduces the observed monthly scalar mean ocean speed
/// (7.0 m/s) to -1.9 % (mean) and 3.1 % (RMS); the vector-mean speed is 34 % low.
pub const SUBMONTHLY_WIND_COMPONENT_SIGMA_M_S: [f64; 18] = [
    1.78, 1.92, 2.09, 2.40, 3.03, 3.86, 4.61, 5.13, 5.49, 5.70, 5.74, 5.55, 5.17, 4.41, 3.96, 3.43,
    2.88, 3.40,
];
/// Width of the Stommel western boundary layer `r / beta`, in mean work-grid centre spacings.
///
/// A numerical resolution criterion, not a physical constant: coarse ocean models must
/// resolve the frictional boundary layer by at least one grid interval (Bryan, Manabe &
/// Pacanowski 1975, JPO 5, 30; Griffies 2004, Fundamentals of Ocean Climate Models). With the
/// production 90-day bottom friction the layer is 6 km wide on a 460 km grid and the gyres
/// degenerate into zonal bands (design sections 5.6 and 12-5).
pub const STOMMEL_BOUNDARY_LAYER_WIDTH_GRID_SPACINGS: f64 = 1.0;

/// Relative residual at which every linear solve of the model stops.
///
/// Numerical: three orders of magnitude below the hard water and energy closure tolerances
/// ([`GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX`],
/// [`GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX`]), so the ledgers close by construction to
/// the solver tolerance (measured 1e-10 to 3e-10, design section 5.3).
pub const REDUCED_CLIMATE_LINEAR_RELATIVE_TOLERANCE: f64 = 1.0e-10;
/// Iteration limit of every linear solve; a solve that reaches it is not converged.
///
/// Numerical: the largest solve measured 1.6 thousand iterations per month (design 5.2).
pub const REDUCED_CLIMATE_LINEAR_MAX_ITERATIONS: u32 = 4_000;
/// Change of the annual-mean temperature below which the joint ice-mask and MSE-conductance
/// fixed point is converged.
///
/// Numerical: two orders of magnitude below the 0.1 K result difference that the joint
/// iteration was accepted at (design section 15.4).
pub const REDUCED_CLIMATE_ENERGY_FIXED_POINT_TOLERANCE_K: f64 = 1.0e-4;
/// Round limit of the joint ice-mask and MSE-conductance fixed point.
///
/// Numerical: the 17-seed corpus converged in 10-15 rounds (design section 15.5).
pub const REDUCED_CLIMATE_ENERGY_FIXED_POINT_MAX_ITERATIONS: u32 = 80;
/// Round limit of the annual-problem Picard iteration that follows a converged ice mask.
pub const REDUCED_CLIMATE_MEBM_PICARD_MAX_ITERATIONS: u32 = 30;
/// Round limit of the monthly vapour active-set iteration.
///
/// Numerical: the active sets converged in at most 7 rounds (design section 4 Q1).
pub const REDUCED_CLIMATE_WATER_ACTIVE_SET_MAX_ITERATIONS: u32 = 60;
/// Largest change of any cell's land evaporative fraction `E_land / P` at which the annual
/// Budyko fixed point is converged.
///
/// Numerical: the land runoff `(1 - E_land / P) P` then moves by less than 0.01 % of the
/// precipitation between the last two rounds.
pub const REDUCED_CLIMATE_LAND_EVAPOTRANSPIRATION_TOLERANCE: f64 = 1.0e-4;
/// Round limit of the annual Budyko land-evapotranspiration fixed point.
pub const REDUCED_CLIMATE_LAND_EVAPOTRANSPIRATION_MAX_ITERATIONS: u32 = 40;
/// History depth of the Anderson acceleration of the Budyko fixed point.
///
/// Anderson (1965), J. ACM 12, 547, DOI 10.1145/321296.321305, in the type-II form of Walker &
/// Ni (2011), SIAM J. Numer. Anal. 49, 1715, DOI 10.1137/10078356X. Numerical: plain Picard
/// contracts by about 0.89 per round and is still at 2.4e-3 after 40 rounds; depth 5 converged
/// all 102 corpus evaluations in 8-10 rounds (design section 19.3).
pub const REDUCED_CLIMATE_LAND_EVAPOTRANSPIRATION_ANDERSON_DEPTH: u32 = 5;

/// Long-term land evaporative fraction `E / P` of the Budyko (1974) curve,
/// `E / P = [phi tanh(1 / phi) (1 - exp(-phi))]^(1/2)`, for the aridity index
/// `phi = E_pot / P`.
///
/// Budyko, M. I. (1974), Climate and Life, Academic Press; the closed form as tabulated by
/// Zhang, Dawes & Walker (2001), Water Resour. Res. 37, 701, DOI 10.1029/2000WR900325, and
/// Arora (2002), J. Hydrol. 265, 164, DOI 10.1016/S0022-1694(02)00101-4. The form has no free
/// parameter. It obeys both limits of the water
/// balance, `E <= P` and `E <= E_pot`, approaching the energy limit `E / P -> phi` as `phi -> 0`
/// and the water limit `E / P -> 1` as `phi -> infinity`. A non-positive index (no potential
/// evaporation) gives zero; an infinite one (no precipitation) gives one.
///
/// # Arguments
///
/// * `aridity_index` - annual potential evaporation over annual precipitation, both per unit
///   land area
///
/// # Returns
///
/// The evaporated share of the land precipitation, in `[0, 1]`.
///
/// # Examples
///
/// ```
/// use sekai::world::natural::budyko_evaporative_fraction;
///
/// let humid = budyko_evaporative_fraction(0.5);
/// let arid = budyko_evaporative_fraction(4.0);
/// assert!(humid < 0.5 && arid > 0.9 && arid < 1.0);
/// ```
pub fn budyko_evaporative_fraction(aridity_index: f64) -> f64 {
    if aridity_index.is_nan() || aridity_index <= 0.0 {
        return 0.0;
    }
    if aridity_index.is_infinite() {
        return 1.0;
    }
    (aridity_index * aridity_index.recip().tanh() * -(-aridity_index).exp_m1()).sqrt()
}

/// Closed set of reduced climate models; replaces `ClimateModelProfile` when the causal chain
/// switches to the reduced model (design section 10 T8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReducedClimateModelProfile {
    /// Design revision R5: seasonal MEBM with ice, sea-ice surface type and TransEBM latitude
    /// albedo; diagnostic eddy-driven and Hadley winds; quasi-steady monthly vapour budget;
    /// Stommel barotropic ocean.
    R1ReducedV1,
}

const fn reduced_climate_profile_tag(profile: ReducedClimateModelProfile) -> u8 {
    match profile {
        ReducedClimateModelProfile::R1ReducedV1 => 1,
    }
}

/// Canonical identity of the reduced model's equations and every constant they read.
///
/// The constants it reuses from the C2 model enter through their existing constant
/// fingerprints, so a change to any of them changes this identity too.
pub fn reduced_climate_model_fingerprint(profile: ReducedClimateModelProfile) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.reduced-climate-equations.v1\0");
    hasher.update(&[reduced_climate_profile_tag(profile)]);
    for value in [
        REDUCED_CLIMATE_MEBM_DIFFUSIVITY_M2_S,
        REDUCED_CLIMATE_HADLEY_WEIGHT_WIDTH_SIN_LATITUDE,
        REDUCED_CLIMATE_HADLEY_UPPER_MSE_RATIO,
        REDUCED_CLIMATE_HADLEY_LOWER_BRANCH_TOP_PRESSURE_PA,
        TRANSEBM_OLR_INTERCEPT_W_M2,
        TRANSEBM_OLR_SLOPE_W_M2_K,
        BUDYKO_ICE_EDGE_ANNUAL_TEMPERATURE_C,
        TRANSEBM_OCEAN_LATITUDE_ALBEDO,
        TRANSEBM_LAND_LATITUDE_ALBEDO,
        TRANSEBM_SEA_ICE_HEAT_CAPACITY_J_M2_K,
        SUBMONTHLY_WIND_SIGMA_BAND_WIDTH_DEGREES,
        STOMMEL_BOUNDARY_LAYER_WIDTH_GRID_SPACINGS,
        REDUCED_CLIMATE_LINEAR_RELATIVE_TOLERANCE,
        REDUCED_CLIMATE_ENERGY_FIXED_POINT_TOLERANCE_K,
        REDUCED_CLIMATE_LAND_EVAPOTRANSPIRATION_TOLERANCE,
        super::LAND_SEA_SURFACE_DRAG_RATIO,
        super::LIQUID_MIXED_LAYER_MIN_C,
        super::SEAWATER_REFERENCE_DENSITY_KG_M3,
    ]
    .into_iter()
    .chain(SUBMONTHLY_WIND_COMPONENT_SIGMA_M_S)
    {
        hasher.update(&value.to_le_bytes());
    }
    for limit in [
        REDUCED_CLIMATE_LINEAR_MAX_ITERATIONS,
        REDUCED_CLIMATE_ENERGY_FIXED_POINT_MAX_ITERATIONS,
        REDUCED_CLIMATE_MEBM_PICARD_MAX_ITERATIONS,
        REDUCED_CLIMATE_WATER_ACTIVE_SET_MAX_ITERATIONS,
        REDUCED_CLIMATE_LAND_EVAPOTRANSPIRATION_MAX_ITERATIONS,
        REDUCED_CLIMATE_LAND_EVAPOTRANSPIRATION_ANDERSON_DEPTH,
    ] {
        hasher.update(&limit.to_le_bytes());
    }
    hasher.update(&p4_thermodynamic_constants_fingerprint());
    hasher.update(&p4_momentum_constants_fingerprint());
    *hasher.finalize().as_bytes()
}

/// Work and convergence evidence of one reduced evaluation (replaces the time-integration
/// report: the model has no cycles, steps, substeps or CFL; design section 8).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReducedClimateSolveReport {
    energy_fixed_point_iterations: u32,
    energy_linear_iterations: u64,
    water_active_set_iterations_max: u32,
    water_linear_iterations: u64,
    ocean_linear_iterations: u64,
    maximum_linear_relative_residual: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReducedClimateSolveReportWire {
    energy_fixed_point_iterations: u32,
    energy_linear_iterations: u64,
    water_active_set_iterations_max: u32,
    water_linear_iterations: u64,
    ocean_linear_iterations: u64,
    maximum_linear_relative_residual: f64,
}

impl ReducedClimateSolveReport {
    /// Records the work of one converged evaluation.
    ///
    /// # Arguments
    ///
    /// * `energy_fixed_point_iterations` - rounds of the joint ice-mask and conductance
    ///   fixed point
    /// * `energy_linear_iterations` - CG and COCG iterations of the energy problem
    /// * `water_active_set_iterations_max` - most active-set rounds of any month
    /// * `water_linear_iterations` - BiCGSTAB iterations of the twelve vapour months
    /// * `ocean_linear_iterations` - BiCGSTAB iterations of the twelve Stommel months
    /// * `maximum_linear_relative_residual` - largest final relative residual of any solve
    ///
    /// # Errors
    ///
    /// Returns [`ClimateReportError`] when a round count is zero or above its limit, or the
    /// residual is not finite, negative, or above
    /// [`REDUCED_CLIMATE_LINEAR_RELATIVE_TOLERANCE`] (an unconverged solve is never
    /// published).
    pub fn new(
        energy_fixed_point_iterations: u32,
        energy_linear_iterations: u64,
        water_active_set_iterations_max: u32,
        water_linear_iterations: u64,
        ocean_linear_iterations: u64,
        maximum_linear_relative_residual: f64,
    ) -> Result<Self, ClimateReportError> {
        let report = Self {
            energy_fixed_point_iterations,
            energy_linear_iterations,
            water_active_set_iterations_max,
            water_linear_iterations,
            ocean_linear_iterations,
            maximum_linear_relative_residual,
        };
        report.validate()?;
        Ok(report)
    }

    /// Rechecks the report invariants listed on [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns [`ClimateReportError`] for the first violated invariant.
    pub fn validate(&self) -> Result<(), ClimateReportError> {
        for (field, rounds, maximum) in [
            (
                "energy_fixed_point_iterations",
                self.energy_fixed_point_iterations,
                REDUCED_CLIMATE_ENERGY_FIXED_POINT_MAX_ITERATIONS,
            ),
            (
                "water_active_set_iterations_max",
                self.water_active_set_iterations_max,
                REDUCED_CLIMATE_WATER_ACTIVE_SET_MAX_ITERATIONS,
            ),
        ] {
            if rounds == 0 {
                return Err(ClimateReportError::ZeroWork { field });
            }
            if rounds > maximum {
                return Err(ClimateReportError::StatisticAboveMaximum {
                    field,
                    found: f64::from(rounds),
                    maximum: f64::from(maximum),
                });
            }
        }
        for (field, iterations) in [
            ("energy_linear_iterations", self.energy_linear_iterations),
            ("water_linear_iterations", self.water_linear_iterations),
            ("ocean_linear_iterations", self.ocean_linear_iterations),
        ] {
            if iterations == 0 {
                return Err(ClimateReportError::ZeroWork { field });
            }
        }
        let residual = self.maximum_linear_relative_residual;
        if !residual.is_finite() || residual < 0.0 {
            return Err(ClimateReportError::InvalidStatistic {
                field: "maximum_linear_relative_residual",
                found: residual,
            });
        }
        if residual > REDUCED_CLIMATE_LINEAR_RELATIVE_TOLERANCE {
            return Err(ClimateReportError::StatisticAboveMaximum {
                field: "maximum_linear_relative_residual",
                found: residual,
                maximum: REDUCED_CLIMATE_LINEAR_RELATIVE_TOLERANCE,
            });
        }
        Ok(())
    }

    /// Rounds of the joint ice-mask and MSE-conductance fixed point.
    pub const fn energy_fixed_point_iterations(&self) -> u32 {
        self.energy_fixed_point_iterations
    }

    /// CG and COCG iterations of the energy problem.
    pub const fn energy_linear_iterations(&self) -> u64 {
        self.energy_linear_iterations
    }

    /// Most active-set rounds of any vapour month.
    pub const fn water_active_set_iterations_max(&self) -> u32 {
        self.water_active_set_iterations_max
    }

    /// BiCGSTAB iterations of the twelve vapour months.
    pub const fn water_linear_iterations(&self) -> u64 {
        self.water_linear_iterations
    }

    /// BiCGSTAB iterations of the twelve Stommel months.
    pub const fn ocean_linear_iterations(&self) -> u64 {
        self.ocean_linear_iterations
    }

    /// Largest final relative residual of any linear solve.
    pub const fn maximum_linear_relative_residual(&self) -> f64 {
        self.maximum_linear_relative_residual
    }
}

impl<'de> Deserialize<'de> for ReducedClimateSolveReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ReducedClimateSolveReportWire::deserialize(deserializer)?;
        Self::new(
            wire.energy_fixed_point_iterations,
            wire.energy_linear_iterations,
            wire.water_active_set_iterations_max,
            wire.water_linear_iterations,
            wire.ocean_linear_iterations,
            wire.maximum_linear_relative_residual,
        )
        .map_err(D::Error::custom)
    }
}

/// Work-domain annual climatology and conservation closure of one reduced evaluation.
///
/// The atmosphere-mass, ocean-volume and paired-exchange errors of the C2 report are retired:
/// the reduced model has no layer mass and no paired exchanges (design section 8). The land
/// ledger is the single account of the P4/P5 land water balance (design section 18.1-2): P5
/// routes the land precipitation that the climate does not evapotranspire.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReducedClimateBudgetReport {
    moisture_relative_error: f64,
    energy_relative_error: f64,
    evaporation_global_mean_mm_day: f64,
    precipitation_global_mean_mm_day: f64,
    evaporation_precipitation_relative_imbalance: f64,
    absorbed_shortwave_global_mean_w_m2: f64,
    outgoing_longwave_global_mean_w_m2: f64,
    toa_net_radiation_global_mean_w_m2: f64,
    planetary_albedo_global_mean: f64,
    land_precipitation_mean_mm_day: f64,
    land_evapotranspiration_mean_mm_day: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReducedClimateBudgetReportWire {
    moisture_relative_error: f64,
    energy_relative_error: f64,
    evaporation_global_mean_mm_day: f64,
    precipitation_global_mean_mm_day: f64,
    evaporation_precipitation_relative_imbalance: f64,
    absorbed_shortwave_global_mean_w_m2: f64,
    outgoing_longwave_global_mean_w_m2: f64,
    toa_net_radiation_global_mean_w_m2: f64,
    planetary_albedo_global_mean: f64,
    land_precipitation_mean_mm_day: f64,
    land_evapotranspiration_mean_mm_day: f64,
}

/// Annual work-domain means and closure errors that make up a [`ReducedClimateBudgetReport`].
///
/// Global means are area weighted over the whole sphere; land means are weighted by the land
/// area of each work cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReducedClimateBudget {
    /// Largest monthly `|sum A (E - P)| / max(sum A E, sum A P)`.
    pub moisture_relative_error: f64,
    /// Annual `|sum A (ASR - OLR)| / sum A ASR`.
    pub energy_relative_error: f64,
    /// Global-mean evaporation, mm/day.
    pub evaporation_global_mean_mm_day: f64,
    /// Global-mean precipitation, mm/day.
    pub precipitation_global_mean_mm_day: f64,
    /// Global-mean absorbed shortwave, W/m2.
    pub absorbed_shortwave_global_mean_w_m2: f64,
    /// Global-mean outgoing longwave, W/m2.
    pub outgoing_longwave_global_mean_w_m2: f64,
    /// Area-mean planetary albedo.
    pub planetary_albedo_global_mean: f64,
    /// Precipitation per unit land area, mm/day.
    pub land_precipitation_mean_mm_day: f64,
    /// Land evapotranspiration per unit land area, mm/day.
    pub land_evapotranspiration_mean_mm_day: f64,
}

impl ReducedClimateBudgetReport {
    /// Derives the closure identities of `budget` and checks the hard ledgers.
    ///
    /// # Errors
    ///
    /// Returns [`ClimateReportError`] when a statistic is not finite or negative where it must
    /// not be, the moisture or energy error exceeds its maximum, the evaporation/precipitation
    /// imbalance or the TOA net radiation is not closed, or land evapotranspiration exceeds
    /// land precipitation.
    pub fn new(budget: ReducedClimateBudget) -> Result<Self, ClimateReportError> {
        let report = Self {
            moisture_relative_error: budget.moisture_relative_error,
            energy_relative_error: budget.energy_relative_error,
            evaporation_global_mean_mm_day: budget.evaporation_global_mean_mm_day,
            precipitation_global_mean_mm_day: budget.precipitation_global_mean_mm_day,
            evaporation_precipitation_relative_imbalance: water_cycle_relative_imbalance(
                budget.evaporation_global_mean_mm_day,
                budget.precipitation_global_mean_mm_day,
            ),
            absorbed_shortwave_global_mean_w_m2: budget.absorbed_shortwave_global_mean_w_m2,
            outgoing_longwave_global_mean_w_m2: budget.outgoing_longwave_global_mean_w_m2,
            toa_net_radiation_global_mean_w_m2: budget.absorbed_shortwave_global_mean_w_m2
                - budget.outgoing_longwave_global_mean_w_m2,
            planetary_albedo_global_mean: budget.planetary_albedo_global_mean,
            land_precipitation_mean_mm_day: budget.land_precipitation_mean_mm_day,
            land_evapotranspiration_mean_mm_day: budget.land_evapotranspiration_mean_mm_day,
        };
        report.validate()?;
        Ok(report)
    }

    /// Rechecks the invariants listed on [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns [`ClimateReportError`] for the first violated invariant.
    pub fn validate(&self) -> Result<(), ClimateReportError> {
        for (field, value, maximum) in [
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
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ClimateReportError::InvalidStatistic {
                    field,
                    found: value,
                });
            }
            if value > maximum {
                return Err(ClimateReportError::StatisticAboveMaximum {
                    field,
                    found: value,
                    maximum,
                });
            }
        }
        for (field, value) in [
            (
                "evaporation_global_mean_mm_day",
                self.evaporation_global_mean_mm_day,
            ),
            (
                "precipitation_global_mean_mm_day",
                self.precipitation_global_mean_mm_day,
            ),
            (
                "absorbed_shortwave_global_mean_w_m2",
                self.absorbed_shortwave_global_mean_w_m2,
            ),
            (
                "outgoing_longwave_global_mean_w_m2",
                self.outgoing_longwave_global_mean_w_m2,
            ),
            (
                "planetary_albedo_global_mean",
                self.planetary_albedo_global_mean,
            ),
            (
                "land_precipitation_mean_mm_day",
                self.land_precipitation_mean_mm_day,
            ),
            (
                "land_evapotranspiration_mean_mm_day",
                self.land_evapotranspiration_mean_mm_day,
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ClimateReportError::InvalidStatistic {
                    field,
                    found: value,
                });
            }
        }
        if self.planetary_albedo_global_mean > 1.0 {
            return Err(ClimateReportError::StatisticAboveMaximum {
                field: "planetary_albedo_global_mean",
                found: self.planetary_albedo_global_mean,
                maximum: 1.0,
            });
        }
        if self.land_evapotranspiration_mean_mm_day > self.land_precipitation_mean_mm_day {
            return Err(ClimateReportError::StatisticAboveMaximum {
                field: "land_evapotranspiration_mean_mm_day",
                found: self.land_evapotranspiration_mean_mm_day,
                maximum: self.land_precipitation_mean_mm_day,
            });
        }
        let imbalance = water_cycle_relative_imbalance(
            self.evaporation_global_mean_mm_day,
            self.precipitation_global_mean_mm_day,
        );
        if imbalance.to_bits() != self.evaporation_precipitation_relative_imbalance.to_bits() {
            return Err(ClimateReportError::StatisticIdentityMismatch {
                field: "evaporation_precipitation_relative_imbalance",
                found: self.evaporation_precipitation_relative_imbalance,
                expected: imbalance,
            });
        }
        if imbalance > GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX {
            return Err(ClimateReportError::WaterCycleNotClosed {
                evaporation_mm_day: self.evaporation_global_mean_mm_day,
                precipitation_mm_day: self.precipitation_global_mean_mm_day,
                relative_imbalance: imbalance,
                maximum: GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX,
            });
        }
        let net =
            self.absorbed_shortwave_global_mean_w_m2 - self.outgoing_longwave_global_mean_w_m2;
        if net.to_bits() != self.toa_net_radiation_global_mean_w_m2.to_bits() {
            return Err(ClimateReportError::StatisticIdentityMismatch {
                field: "toa_net_radiation_global_mean_w_m2",
                found: self.toa_net_radiation_global_mean_w_m2,
                expected: net,
            });
        }
        if net.abs() > GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2 {
            return Err(ClimateReportError::RadiativeCycleNotClosed {
                absorbed_shortwave_w_m2: self.absorbed_shortwave_global_mean_w_m2,
                outgoing_longwave_w_m2: self.outgoing_longwave_global_mean_w_m2,
                net_w_m2: net,
                maximum: GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2,
            });
        }
        Ok(())
    }

    /// Largest monthly relative evaporation/precipitation closure error.
    pub const fn moisture_relative_error(&self) -> f64 {
        self.moisture_relative_error
    }

    /// Annual relative TOA energy closure error.
    pub const fn energy_relative_error(&self) -> f64 {
        self.energy_relative_error
    }

    /// Global-mean evaporation, mm/day.
    pub const fn evaporation_global_mean_mm_day(&self) -> f64 {
        self.evaporation_global_mean_mm_day
    }

    /// Global-mean precipitation, mm/day.
    pub const fn precipitation_global_mean_mm_day(&self) -> f64 {
        self.precipitation_global_mean_mm_day
    }

    /// Relative global evaporation/precipitation imbalance.
    pub const fn evaporation_precipitation_relative_imbalance(&self) -> f64 {
        self.evaporation_precipitation_relative_imbalance
    }

    /// Global-mean absorbed shortwave, W/m2.
    pub const fn absorbed_shortwave_global_mean_w_m2(&self) -> f64 {
        self.absorbed_shortwave_global_mean_w_m2
    }

    /// Global-mean outgoing longwave, W/m2.
    pub const fn outgoing_longwave_global_mean_w_m2(&self) -> f64 {
        self.outgoing_longwave_global_mean_w_m2
    }

    /// Global-mean TOA net radiation, W/m2.
    pub const fn toa_net_radiation_global_mean_w_m2(&self) -> f64 {
        self.toa_net_radiation_global_mean_w_m2
    }

    /// Area-mean planetary albedo.
    pub const fn planetary_albedo_global_mean(&self) -> f64 {
        self.planetary_albedo_global_mean
    }

    /// Precipitation per unit land area, mm/day.
    pub const fn land_precipitation_mean_mm_day(&self) -> f64 {
        self.land_precipitation_mean_mm_day
    }

    /// Land evapotranspiration per unit land area, mm/day.
    pub const fn land_evapotranspiration_mean_mm_day(&self) -> f64 {
        self.land_evapotranspiration_mean_mm_day
    }
}

impl<'de> Deserialize<'de> for ReducedClimateBudgetReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ReducedClimateBudgetReportWire::deserialize(deserializer)?;
        let report = Self {
            moisture_relative_error: wire.moisture_relative_error,
            energy_relative_error: wire.energy_relative_error,
            evaporation_global_mean_mm_day: wire.evaporation_global_mean_mm_day,
            precipitation_global_mean_mm_day: wire.precipitation_global_mean_mm_day,
            evaporation_precipitation_relative_imbalance: wire
                .evaporation_precipitation_relative_imbalance,
            absorbed_shortwave_global_mean_w_m2: wire.absorbed_shortwave_global_mean_w_m2,
            outgoing_longwave_global_mean_w_m2: wire.outgoing_longwave_global_mean_w_m2,
            toa_net_radiation_global_mean_w_m2: wire.toa_net_radiation_global_mean_w_m2,
            planetary_albedo_global_mean: wire.planetary_albedo_global_mean,
            land_precipitation_mean_mm_day: wire.land_precipitation_mean_mm_day,
            land_evapotranspiration_mean_mm_day: wire.land_evapotranspiration_mean_mm_day,
        };
        report.validate().map_err(D::Error::custom)?;
        Ok(report)
    }
}

/// The R1 published field set on the authoritative surface (design section 9).
///
/// Air temperature carries the LOD-2 lapse-rate correction to each cell's own reference height
/// and specific humidity the relative-humidity-preserving correction (LOD-2 sections 3.1 and
/// 11.4). Land runoff is precipitation minus land evapotranspiration per unit land area, the
/// one land water account that P5 routes (design section 18.1-2). The C2 vertical structure
/// (upper wind, shear, thermocline, deep ocean, layer height anomalies) is not published.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReducedClimateFields {
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
    monthly_land_runoff_mm_day: MonthlyScalarField,
    monthly_sea_surface_height_m: MonthlyScalarField,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReducedClimateFieldsWire {
    near_surface_wind_m_s: MonthlyVector3Field,
    surface_ocean_current_m_s: MonthlyVector3Field,
    monthly_air_temperature_c: MonthlyScalarField,
    monthly_sea_surface_temperature_c: MonthlyScalarField,
    #[serde(deserialize_with = "super::deserialize_global_circulation_scalars")]
    surface_albedo: Vec<f32>,
    monthly_absorbed_shortwave_w_m2: MonthlyScalarField,
    monthly_outgoing_longwave_w_m2: MonthlyScalarField,
    monthly_specific_humidity: MonthlyScalarField,
    monthly_evaporation_mm_day: MonthlyScalarField,
    monthly_precipitation_mm_day: MonthlyScalarField,
    monthly_orographic_precipitation_mm_day: MonthlyScalarField,
    monthly_land_runoff_mm_day: MonthlyScalarField,
    monthly_sea_surface_height_m: MonthlyScalarField,
}

/// Owned R1 field values handed to [`ReducedClimateFields::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct ReducedClimateFieldValues {
    /// Near-surface wind, m/s, tangent to the sphere.
    pub near_surface_wind_m_s: MonthlyVector3Field,
    /// Surface ocean current, m/s, tangent to the sphere and zero on land.
    pub surface_ocean_current_m_s: MonthlyVector3Field,
    /// Near-surface air temperature at each cell's reference height, degC.
    pub monthly_air_temperature_c: MonthlyScalarField,
    /// Sea-surface temperature, degC.
    pub monthly_sea_surface_temperature_c: MonthlyScalarField,
    /// Annual surface albedo after the ice feedback.
    pub surface_albedo: Vec<f32>,
    /// Absorbed shortwave, W/m2.
    pub monthly_absorbed_shortwave_w_m2: MonthlyScalarField,
    /// Outgoing longwave, W/m2.
    pub monthly_outgoing_longwave_w_m2: MonthlyScalarField,
    /// Near-surface specific humidity, kg/kg.
    pub monthly_specific_humidity: MonthlyScalarField,
    /// Cell-mean evaporation, mm/day.
    pub monthly_evaporation_mm_day: MonthlyScalarField,
    /// Precipitation, mm/day.
    pub monthly_precipitation_mm_day: MonthlyScalarField,
    /// Orographic part of the precipitation, mm/day.
    pub monthly_orographic_precipitation_mm_day: MonthlyScalarField,
    /// Precipitation minus land evapotranspiration per unit land area, mm/day.
    pub monthly_land_runoff_mm_day: MonthlyScalarField,
    /// Sea-surface height diagnosed from the Stommel streamfunction, m.
    pub monthly_sea_surface_height_m: MonthlyScalarField,
}

impl ReducedClimateFields {
    /// Validates and adopts one complete R1 field set.
    ///
    /// # Errors
    ///
    /// Returns [`GlobalCirculationValidationError`] when the set is empty, a field length
    /// differs from the wind field, a value is not finite or outside its physical range, or a
    /// part of the precipitation (orographic share, land runoff) exceeds the total.
    pub fn new(
        values: ReducedClimateFieldValues,
    ) -> Result<Self, GlobalCirculationValidationError> {
        let fields = Self {
            near_surface_wind_m_s: values.near_surface_wind_m_s,
            surface_ocean_current_m_s: values.surface_ocean_current_m_s,
            monthly_air_temperature_c: values.monthly_air_temperature_c,
            monthly_sea_surface_temperature_c: values.monthly_sea_surface_temperature_c,
            surface_albedo: values.surface_albedo,
            monthly_absorbed_shortwave_w_m2: values.monthly_absorbed_shortwave_w_m2,
            monthly_outgoing_longwave_w_m2: values.monthly_outgoing_longwave_w_m2,
            monthly_specific_humidity: values.monthly_specific_humidity,
            monthly_evaporation_mm_day: values.monthly_evaporation_mm_day,
            monthly_precipitation_mm_day: values.monthly_precipitation_mm_day,
            monthly_orographic_precipitation_mm_day: values.monthly_orographic_precipitation_mm_day,
            monthly_land_runoff_mm_day: values.monthly_land_runoff_mm_day,
            monthly_sea_surface_height_m: values.monthly_sea_surface_height_m,
        };
        fields.validate_impl(None)?;
        Ok(fields)
    }

    /// Rechecks every field while polling `cancelled` between cell blocks.
    ///
    /// # Errors
    ///
    /// Returns [`GlobalCirculationValidationError::Cancelled`] when `cancelled` fires, else the
    /// errors of [`Self::new`].
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
        let expected = self.cell_count();
        if expected == 0 {
            return Err(GlobalCirculationValidationError::EmptyFields);
        }
        for (field, found) in self.field_lengths() {
            if found != expected {
                return Err(GlobalCirculationValidationError::FieldLengthMismatch {
                    field,
                    found,
                    expected,
                });
            }
        }
        // The vector and scalar bounds are the C2 publication bounds (one fact per field).
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
        for (field, values) in [
            (
                "monthly_absorbed_shortwave_w_m2",
                &self.monthly_absorbed_shortwave_w_m2,
            ),
            (
                "monthly_outgoing_longwave_w_m2",
                &self.monthly_outgoing_longwave_w_m2,
            ),
        ] {
            validate_monthly_scalar(
                field,
                values,
                0.0,
                GLOBAL_CIRCULATION_RADIATIVE_FLUX_MAX_W_M2 as f32,
                cancellation,
            )?;
        }
        validate_monthly_scalar(
            "monthly_specific_humidity",
            &self.monthly_specific_humidity,
            0.0,
            P4_MAX_SPECIFIC_HUMIDITY_KG_KG as f32,
            cancellation,
        )?;
        for (field, values) in [
            (
                "monthly_evaporation_mm_day",
                &self.monthly_evaporation_mm_day,
            ),
            (
                "monthly_precipitation_mm_day",
                &self.monthly_precipitation_mm_day,
            ),
            (
                "monthly_orographic_precipitation_mm_day",
                &self.monthly_orographic_precipitation_mm_day,
            ),
            (
                "monthly_land_runoff_mm_day",
                &self.monthly_land_runoff_mm_day,
            ),
        ] {
            validate_monthly_scalar(field, values, 0.0, f32::MAX, cancellation)?;
        }
        validate_orographic_precipitation_identity(
            &self.monthly_precipitation_mm_day,
            &self.monthly_orographic_precipitation_mm_day,
            cancellation,
        )?;
        validate_land_runoff_identity(
            &self.monthly_precipitation_mm_day,
            &self.monthly_land_runoff_mm_day,
            cancellation,
        )?;
        validate_monthly_scalar(
            "monthly_sea_surface_height_m",
            &self.monthly_sea_surface_height_m,
            -100.0,
            100.0,
            cancellation,
        )
    }

    fn field_lengths(&self) -> [(&'static str, usize); 13] {
        [
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
                "monthly_land_runoff_mm_day",
                self.monthly_land_runoff_mm_day.len(),
            ),
            (
                "monthly_sea_surface_height_m",
                self.monthly_sea_surface_height_m.len(),
            ),
        ]
    }

    /// Number of authoritative cells every field covers.
    pub fn cell_count(&self) -> usize {
        self.near_surface_wind_m_s.len()
    }

    /// Canonical hash of every published field in declaration order.
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint_impl(None)
            .expect("non-cancellable field hashing cannot be cancelled")
    }

    /// [`Self::fingerprint`], polling `cancelled` between cell blocks.
    ///
    /// # Errors
    ///
    /// Returns [`GlobalCirculationValidationError::Cancelled`] when `cancelled` fires.
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
        hasher.update(b"sekai.reduced-climate-state.v1\0");
        for field in [&self.near_surface_wind_m_s, &self.surface_ocean_current_m_s] {
            hash_monthly_vectors(&mut hasher, field.values(), cancellation)?;
        }
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
            &self.monthly_land_runoff_mm_day,
            &self.monthly_sea_surface_height_m,
        ] {
            hash_monthly_scalars(&mut hasher, field.values(), cancellation)?;
        }
        Ok(*hasher.finalize().as_bytes())
    }

    /// Near-surface wind, m/s.
    pub const fn near_surface_wind_m_s(&self) -> &MonthlyVector3Field {
        &self.near_surface_wind_m_s
    }

    /// Surface ocean current, m/s.
    pub const fn surface_ocean_current_m_s(&self) -> &MonthlyVector3Field {
        &self.surface_ocean_current_m_s
    }

    /// Near-surface air temperature at each cell's reference height, degC.
    pub const fn monthly_air_temperature_c(&self) -> &MonthlyScalarField {
        &self.monthly_air_temperature_c
    }

    /// Sea-surface temperature, degC.
    pub const fn monthly_sea_surface_temperature_c(&self) -> &MonthlyScalarField {
        &self.monthly_sea_surface_temperature_c
    }

    /// Annual surface albedo.
    pub fn surface_albedo(&self) -> &[f32] {
        &self.surface_albedo
    }

    /// Absorbed shortwave, W/m2.
    pub const fn monthly_absorbed_shortwave_w_m2(&self) -> &MonthlyScalarField {
        &self.monthly_absorbed_shortwave_w_m2
    }

    /// Outgoing longwave, W/m2.
    pub const fn monthly_outgoing_longwave_w_m2(&self) -> &MonthlyScalarField {
        &self.monthly_outgoing_longwave_w_m2
    }

    /// Near-surface specific humidity, kg/kg.
    pub const fn monthly_specific_humidity(&self) -> &MonthlyScalarField {
        &self.monthly_specific_humidity
    }

    /// Cell-mean evaporation, mm/day.
    pub const fn monthly_evaporation_mm_day(&self) -> &MonthlyScalarField {
        &self.monthly_evaporation_mm_day
    }

    /// Precipitation, mm/day.
    pub const fn monthly_precipitation_mm_day(&self) -> &MonthlyScalarField {
        &self.monthly_precipitation_mm_day
    }

    /// Orographic part of the precipitation, mm/day.
    pub const fn monthly_orographic_precipitation_mm_day(&self) -> &MonthlyScalarField {
        &self.monthly_orographic_precipitation_mm_day
    }

    /// Precipitation minus land evapotranspiration per unit land area, mm/day.
    pub const fn monthly_land_runoff_mm_day(&self) -> &MonthlyScalarField {
        &self.monthly_land_runoff_mm_day
    }

    /// Sea-surface height diagnosed from the Stommel streamfunction, m.
    pub const fn monthly_sea_surface_height_m(&self) -> &MonthlyScalarField {
        &self.monthly_sea_surface_height_m
    }
}

impl<'de> Deserialize<'de> for ReducedClimateFields {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ReducedClimateFieldsWire::deserialize(deserializer)?;
        Self::new(ReducedClimateFieldValues {
            near_surface_wind_m_s: wire.near_surface_wind_m_s,
            surface_ocean_current_m_s: wire.surface_ocean_current_m_s,
            monthly_air_temperature_c: wire.monthly_air_temperature_c,
            monthly_sea_surface_temperature_c: wire.monthly_sea_surface_temperature_c,
            surface_albedo: wire.surface_albedo,
            monthly_absorbed_shortwave_w_m2: wire.monthly_absorbed_shortwave_w_m2,
            monthly_outgoing_longwave_w_m2: wire.monthly_outgoing_longwave_w_m2,
            monthly_specific_humidity: wire.monthly_specific_humidity,
            monthly_evaporation_mm_day: wire.monthly_evaporation_mm_day,
            monthly_precipitation_mm_day: wire.monthly_precipitation_mm_day,
            monthly_orographic_precipitation_mm_day: wire.monthly_orographic_precipitation_mm_day,
            monthly_land_runoff_mm_day: wire.monthly_land_runoff_mm_day,
            monthly_sea_surface_height_m: wire.monthly_sea_surface_height_m,
        })
        .map_err(D::Error::custom)
    }
}

fn validate_land_runoff_identity(
    precipitation: &MonthlyScalarField,
    runoff: &MonthlyScalarField,
    cancellation: CancellationCheck<'_>,
) -> Result<(), GlobalCirculationValidationError> {
    for (cell, (total, part)) in precipitation
        .values()
        .iter()
        .zip(runoff.values())
        .enumerate()
    {
        if cell % 256 == 0 {
            check_global_circulation_cancelled(cancellation)?;
        }
        for month in 0..CLIMATE_MONTH_COUNT {
            if part[month] > total[month] {
                return Err(
                    GlobalCirculationValidationError::LandRunoffExceedsPrecipitation {
                        cell,
                        month,
                        runoff: part[month],
                        precipitation: total[month],
                    },
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monthly(cells: usize, value: f32) -> MonthlyScalarField {
        MonthlyScalarField::from_values(vec![[value; CLIMATE_MONTH_COUNT]; cells]).unwrap()
    }

    fn values(cells: usize) -> ReducedClimateFieldValues {
        let vectors = || {
            MonthlyVector3Field::from_values(vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; cells]).unwrap()
        };
        ReducedClimateFieldValues {
            near_surface_wind_m_s: vectors(),
            surface_ocean_current_m_s: vectors(),
            monthly_air_temperature_c: monthly(cells, 15.0),
            monthly_sea_surface_temperature_c: monthly(cells, 15.0),
            surface_albedo: vec![0.1; cells],
            monthly_absorbed_shortwave_w_m2: monthly(cells, 240.0),
            monthly_outgoing_longwave_w_m2: monthly(cells, 240.0),
            monthly_specific_humidity: monthly(cells, 0.01),
            monthly_evaporation_mm_day: monthly(cells, 2.7),
            monthly_precipitation_mm_day: monthly(cells, 2.7),
            monthly_orographic_precipitation_mm_day: monthly(cells, 0.1),
            monthly_land_runoff_mm_day: monthly(cells, 1.0),
            monthly_sea_surface_height_m: monthly(cells, 0.0),
        }
    }

    fn budget() -> ReducedClimateBudget {
        ReducedClimateBudget {
            moisture_relative_error: 1.0e-10,
            energy_relative_error: 1.0e-11,
            evaporation_global_mean_mm_day: 2.7,
            precipitation_global_mean_mm_day: 2.7,
            absorbed_shortwave_global_mean_w_m2: 240.0,
            outgoing_longwave_global_mean_w_m2: 240.0,
            planetary_albedo_global_mean: 0.3,
            land_precipitation_mean_mm_day: 2.2,
            land_evapotranspiration_mean_mm_day: 1.3,
        }
    }

    /// Contract: the R1 field set round-trips through its validated wire format with the same
    /// fingerprint, and the fingerprint sees the land-runoff field.
    #[test]
    fn fields_round_trip_and_fingerprint_every_field() {
        let fields = ReducedClimateFields::new(values(3)).unwrap();
        let json = serde_json::to_string(&fields).unwrap();
        let decoded: ReducedClimateFields = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, fields);
        assert_eq!(decoded.fingerprint(), fields.fingerprint());
        let mut other = values(3);
        other.monthly_land_runoff_mm_day = monthly(3, 1.5);
        assert_ne!(
            ReducedClimateFields::new(other).unwrap().fingerprint(),
            fields.fingerprint()
        );
    }

    /// Contract (design 18.3-2): land runoff is a share of precipitation, never more.
    #[test]
    fn land_runoff_above_precipitation_is_rejected() {
        let mut invalid = values(2);
        invalid.monthly_land_runoff_mm_day = monthly(2, 3.0);
        assert!(matches!(
            ReducedClimateFields::new(invalid),
            Err(GlobalCirculationValidationError::LandRunoffExceedsPrecipitation { .. })
        ));
        let mut short = values(2);
        short.monthly_sea_surface_height_m = monthly(1, 0.0);
        assert!(matches!(
            ReducedClimateFields::new(short),
            Err(GlobalCirculationValidationError::FieldLengthMismatch { .. })
        ));
    }

    /// Contract: the budget report derives its closure identities, enforces the hard ledgers,
    /// and refuses a forged identity on deserialization.
    #[test]
    fn budget_report_derives_identities_and_enforces_ledgers() {
        let report = ReducedClimateBudgetReport::new(budget()).unwrap();
        assert_eq!(report.toa_net_radiation_global_mean_w_m2(), 0.0);
        assert_eq!(report.evaporation_precipitation_relative_imbalance(), 0.0);
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            serde_json::from_str::<ReducedClimateBudgetReport>(&json).unwrap(),
            report
        );
        let forged = json.replace(
            "\"toa_net_radiation_global_mean_w_m2\":0.0",
            "\"toa_net_radiation_global_mean_w_m2\":1.0",
        );
        assert!(serde_json::from_str::<ReducedClimateBudgetReport>(&forged).is_err());
        let wet_land = ReducedClimateBudget {
            land_evapotranspiration_mean_mm_day: 2.3,
            ..budget()
        };
        assert!(ReducedClimateBudgetReport::new(wet_land).is_err());
        let open_toa = ReducedClimateBudget {
            outgoing_longwave_global_mean_w_m2: 220.0,
            ..budget()
        };
        assert!(matches!(
            ReducedClimateBudgetReport::new(open_toa),
            Err(ClimateReportError::RadiativeCycleNotClosed { .. })
        ));
    }

    /// Contract (design section 18.3-1): the Budyko curve respects both water-balance limits,
    /// `E <= P` and `E <= E_pot`, rises monotonically with aridity, and meets its energy and
    /// water limits; at aridity 1 the closed form is `sqrt(tanh 1 (1 - 1/e)) = 0.69385`.
    #[test]
    fn budyko_curve_obeys_its_limits() {
        let mut previous = 0.0;
        for step in 1..=400 {
            let aridity = f64::from(step) * 0.025;
            let fraction = budyko_evaporative_fraction(aridity);
            assert!(fraction > previous && fraction < 1.0 && fraction <= aridity);
            previous = fraction;
        }
        assert!((budyko_evaporative_fraction(1.0) - 0.69385).abs() < 1.0e-5);
        assert!((budyko_evaporative_fraction(1.0e-3) / 1.0e-3 - 1.0).abs() < 1.0e-3);
        assert!(1.0 - budyko_evaporative_fraction(50.0) < 1.0e-3);
        assert_eq!(budyko_evaporative_fraction(0.0), 0.0);
        assert_eq!(budyko_evaporative_fraction(f64::INFINITY), 1.0);
    }

    /// Contract: only converged evaluations produce a solve report.
    #[test]
    fn solve_report_rejects_unconverged_work() {
        assert!(ReducedClimateSolveReport::new(12, 2_700, 7, 1_400, 1_500, 5.0e-11).is_ok());
        assert!(ReducedClimateSolveReport::new(12, 2_700, 7, 1_400, 1_500, 1.0e-6).is_err());
        assert!(ReducedClimateSolveReport::new(0, 2_700, 7, 1_400, 1_500, 5.0e-11).is_err());
        assert!(ReducedClimateSolveReport::new(
            REDUCED_CLIMATE_ENERGY_FIXED_POINT_MAX_ITERATIONS + 1,
            2_700,
            7,
            1_400,
            1_500,
            5.0e-11
        )
        .is_err());
    }
}
