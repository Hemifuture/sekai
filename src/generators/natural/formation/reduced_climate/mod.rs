//! A10 offline prototype of a reduced-order, diagnostic seasonal hydro-thermal
//! climate on the existing P4 work domain.
//!
//! This module is compiled only for tests and is not reachable from the
//! production graph. It exists to measure cost, ledgers, and quality before
//! the design in
//! `docs/superpowers/specs/2026-10-25-a10-reduced-order-climate-design.md`
//! is decided. Every coefficient is either a production fact reused through
//! its single source or a probe-local constant documented with its primary
//! source; probe-local duplicates of private production constants are marked.
//!
//! Components (see the design for sources):
//! - E1 seasonal moist energy-balance model (MEBM) in sea-level-reduced
//!   temperature, solved for its exact periodic Crank-Nicolson month means by
//!   a discrete Fourier decomposition (one real CG and five complex COCG
//!   solves);
//! - W1 zonal-mean eddy-driven surface zonal wind from the column angular
//!   momentum balance with the production Peixoto & Oort eddy profile;
//! - W2 Hadley lower branch from the Siler, Roe & Armour (2018) partition of
//!   the diffusive MSE flux, reconstructed to cell vectors (Perot 2000);
//! - Q1 monthly quasi-steady column-vapour budget with eddy diffusion, Hadley
//!   and zonal advection, bulk ocean evaporation, land recycling, threshold
//!   condensation and Smith upslope condensation, solved as one M-matrix per
//!   month with an active-set loop (E and P close by construction);
//! - O1 Stommel wind-driven barotropic streamfunction per month.

#[cfg(test)]
mod probe;

use rayon::prelude::*;

use crate::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
use crate::world::natural::{
    absorbed_shortwave_w_m2, bulk_surface_evaporation_kg_m2_s,
    gray_equilibrium_surface_temperature_c, gray_longwave_slope_w_m2_k,
    lcl_adjusted_orographic_condensation_kg_m2_s, p4_seasonal_storage_heat_capacities_j_m2_k,
    saturation_specific_humidity_kg_kg, ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile,
    ClimateSpec, BULK_MOISTURE_TRANSFER_COEFFICIENT, CLIMATE_MONTH_COUNT,
    CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M, EARTH_ROTATION_RATE_RAD_S,
    P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K, P4_HIGHLAND_ALBEDO_RAMP_ONSET_M,
    P4_HIGHLAND_ALBEDO_RAMP_SPAN_M, P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT,
    P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY, P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS,
    P4_LOWER_LAYER_REFERENCE_PRESSURE_PA, P4_OPEN_OCEAN_SURFACE_ALBEDO,
    P4_REFERENCE_AIR_DENSITY_KG_M3, P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT,
    REFERENCE_SURFACE_RELATIVE_HUMIDITY, SECONDS_PER_CLIMATOLOGICAL_MONTH, STANDARD_GRAVITY_M_S2,
    WATER_VAPORIZATION_LATENT_HEAT_J_KG,
};

use super::global_circulation::GlobalClimateForcing;

/// MEBM eddy diffusivity of near-surface moist static energy.
/// Siler, Roe & Armour (2018), J. Climate 31, 7481, DOI 10.1175/JCLI-D-18-0081.1,
/// Fig. 2b: best fit to ERA-Interim zonal-mean 2 m temperature, within 10 % of
/// Hwang & Frierson (2010), Rose et al. (2014) and Roe et al. (2015).
const MEBM_DIFFUSIVITY_M2_S: f64 = 1.16e6;
/// Siler et al. (2018) Eq. (6): Gaussian Hadley-cell weight width in sin(lat).
const HADLEY_WEIGHT_WIDTH_SIN_LATITUDE: f64 = 0.3;
/// Siler et al. (2018) Eq. (8) text: tropical upper-troposphere MSE is 1.06 h(0).
const HADLEY_UPPER_MSE_RATIO: f64 = 1.06;
/// Siler et al. (2018) Fig. 2 caption: the lower Hadley branch is the layer
/// below 900 hPa, where most water vapour is concentrated.
const HADLEY_LOWER_BRANCH_TOP_PRESSURE_PA: f64 = 90_000.0;
/// Probe duplicate of the private production `LAND_SEA_SURFACE_DRAG_RATIO`
/// (Garratt 1992 §4.1; design 2026-09-02 A2 §5).
const LAND_SEA_SURFACE_DRAG_RATIO: f64 = 3.0;
/// Probe duplicate of the private production `LIQUID_MIXED_LAYER_MIN_C`
/// (enthalpy-method freezing floor of the mixed layer, `state.rs`).
const LIQUID_MIXED_LAYER_MIN_C: f64 = -2.0;
/// Seawater density of the production C2 layout ocean layers.
const SEAWATER_DENSITY_KG_M3: f64 = 1_025.0;

/// TransEBM v1.0 (Ziegler & Rehfeld 2021, GMD 14, 2843, DOI
/// 10.5194/gmd-14-2843-2021, Sect. 2): empirical Budyko-type OLR `A + B T`
/// with `A_ref = 210.2 W m-2` (315 ppm CO2) and `B = 2.15 W m-2 K-1`, T in C.
const TRANSEBM_OLR_INTERCEPT_W_M2: f64 = 210.2;
const TRANSEBM_OLR_SLOPE_W_M2_K: f64 = 2.15;

/// Outgoing-longwave closure of the MEBM (probe sensitivity axis).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum OutgoingLongwaveClosure {
    /// Production gray linearization around the local gray target.
    ProductionGray,
    /// Empirical `A + B T` of Budyko (1969) / North (1975) as used by TransEBM.
    BudykoTransEbm,
}

const LINEAR_TOLERANCE: f64 = 1.0e-10;
const LINEAR_MAX_ITERATIONS: usize = 4_000;
const MEBM_PICARD_MAX_ITERATIONS: usize = 30;
const MEBM_PICARD_TOLERANCE_K: f64 = 1.0e-4;
const WATER_ACTIVE_SET_MAX_ITERATIONS: usize = 60;

/// Seed-independent geometry of the work domain (pre-warmable with the grid).
#[derive(Debug, Clone)]
pub(crate) struct ReducedGrid {
    pub(crate) radius_m: f64,
    pub(crate) area_m2: Vec<f64>,
    pub(crate) center: Vec<[f64; 3]>,
    pub(crate) latitude_rad: Vec<f64>,
    pub(crate) east: Vec<[f64; 3]>,
    pub(crate) edges: Vec<ReducedEdge>,
    /// Edge indices and owner slot (0 = edge's first cell) per cell.
    pub(crate) cell_edges: Vec<[(u32, u8); 4]>,
    /// Latitude bands of the production axisymmetric band geometry.
    pub(crate) band_count: usize,
    pub(crate) cell_band: Vec<usize>,
    pub(crate) edge_band: Vec<usize>,
    /// Mean centre spacing, for the boundary-layer-resolving Stommel friction.
    pub(crate) mean_spacing_m: f64,
    operators_grid: CubedSphereGrid,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReducedEdge {
    pub(crate) cells: [u32; 2],
    pub(crate) length_m: f64,
    pub(crate) center_distance_m: f64,
    /// Unit normal pointing from `cells[0]` to `cells[1]`.
    pub(crate) normal: [f64; 3],
    pub(crate) midpoint: [f64; 3],
    pub(crate) displacement: [[f64; 3]; 2],
    /// Siler et al. (2018) Hadley weight at the edge midpoint latitude.
    pub(crate) hadley_weight: f64,
}

impl ReducedGrid {
    pub(crate) fn new(grid: &CubedSphereGrid) -> Self {
        let cells = grid.cells();
        let mut area_m2 = Vec::with_capacity(cells.len());
        let mut center = Vec::with_capacity(cells.len());
        let mut latitude_rad = Vec::with_capacity(cells.len());
        let mut east = Vec::with_capacity(cells.len());
        for cell in cells {
            let c = cell.center_unit();
            area_m2.push(cell.area_m2());
            center.push(c);
            latitude_rad.push(c[2].clamp(-1.0, 1.0).asin());
            east.push(east_unit(c));
        }
        let edges: Vec<ReducedEdge> = grid
            .edges()
            .iter()
            .map(|edge| {
                let midpoint = edge.midpoint_unit();
                let x = midpoint[2];
                ReducedEdge {
                    cells: *edge.cells(),
                    length_m: edge.length_m(),
                    center_distance_m: edge.center_distance_m(),
                    normal: edge.normal_from_first(),
                    midpoint,
                    displacement: [
                        edge.midpoint_displacement_m(0),
                        edge.midpoint_displacement_m(1),
                    ],
                    hadley_weight: (-(x * x)
                        / (HADLEY_WEIGHT_WIDTH_SIN_LATITUDE * HADLEY_WEIGHT_WIDTH_SIN_LATITUDE))
                        .exp(),
                }
            })
            .collect();
        let cell_edges = cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                cell.edges().map(|edge_id| {
                    let edge = &edges[edge_id as usize];
                    let slot = u8::from(edge.cells[0] as usize != index);
                    (edge_id, slot)
                })
            })
            .collect();
        let band_count = crate::world::natural::global_circulation_axisymmetric_band_count(
            grid.face_resolution(),
        );
        let band_of = |sin_latitude: f64| {
            let latitude = sin_latitude.clamp(-1.0, 1.0).asin();
            (((latitude / std::f64::consts::PI + 0.5) * band_count as f64).floor() as usize)
                .min(band_count - 1)
        };
        let cell_band = center.iter().map(|c| band_of(c[2])).collect();
        let edge_band = edges.iter().map(|e| band_of(e.midpoint[2])).collect();
        let mean_spacing_m =
            edges.iter().map(|e| e.center_distance_m).sum::<f64>() / edges.len() as f64;
        Self {
            radius_m: grid.radius_m(),
            band_count,
            cell_band,
            edge_band,
            mean_spacing_m,
            area_m2,
            center,
            latitude_rad,
            east,
            edges,
            cell_edges,
            operators_grid: grid.clone(),
        }
    }

    pub(crate) fn cell_count(&self) -> usize {
        self.area_m2.len()
    }
}

/// Seed-dependent per-cell inputs derived from the production forcing.
#[derive(Debug, Clone)]
pub(crate) struct ReducedInputs {
    pub(crate) land_fraction: Vec<f64>,
    /// Scheme C reference height `H = M h` (LOD-2 §3.2) on the work grid.
    pub(crate) reference_height_m: Vec<f64>,
    pub(crate) reference_height_gradient: Vec<[f64; 3]>,
    pub(crate) surface_albedo: Vec<f64>,
    pub(crate) monthly_absorbed_shortwave_w_m2: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) land_evapotranspiration_fraction: Vec<f64>,
    pub(crate) temperature_offset_c: f64,
    pub(crate) moisture_scale: f64,
    pub(crate) longwave: OutgoingLongwaveClosure,
}

impl ReducedInputs {
    /// Rebuilds albedo and ASR with the scheme-C reference height, reusing
    /// the production insolation, albedo, and ASR functions.
    pub(crate) fn from_forcing(
        grid: &ReducedGrid,
        forcing: &GlobalClimateForcing,
        spec: &ClimateSpec,
        reference_height_m: Vec<f64>,
        longwave: OutgoingLongwaveClosure,
    ) -> Self {
        let planet = forcing.planet_forcing();
        let land_fraction: Vec<f64> = planet
            .land_fraction()
            .iter()
            .map(|&v| f64::from(v))
            .collect();
        let mut surface_albedo = Vec::with_capacity(grid.cell_count());
        let mut monthly_absorbed_shortwave_w_m2 = Vec::with_capacity(grid.cell_count());
        for (index, insolation) in forcing.monthly_insolation_fraction().iter().enumerate() {
            let land = land_fraction[index];
            let snow_prior = ((reference_height_m[index] - P4_HIGHLAND_ALBEDO_RAMP_ONSET_M)
                / P4_HIGHLAND_ALBEDO_RAMP_SPAN_M)
                .clamp(0.0, 1.0);
            let albedo = P4_OPEN_OCEAN_SURFACE_ALBEDO
                + P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT * land
                + P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT * snow_prior * land;
            // The production forcing rounds albedo through f32 before ASR.
            let albedo = f64::from(albedo as f32);
            surface_albedo.push(albedo);
            monthly_absorbed_shortwave_w_m2.push(
                insolation.map(|fraction| absorbed_shortwave_w_m2(f64::from(fraction), albedo)),
            );
        }
        let operators = CirculationOperators::new(&grid.operators_grid);
        let height_f32: Vec<f32> = reference_height_m.iter().map(|&v| v as f32).collect();
        let reference_height_gradient = operators
            .gradient(&height_f32)
            .expect("finite reference height")
            .into_iter()
            .map(|g| g.map(f64::from))
            .collect();
        Self {
            land_fraction,
            reference_height_m,
            reference_height_gradient,
            surface_albedo,
            monthly_absorbed_shortwave_w_m2,
            land_evapotranspiration_fraction: forcing
                .land_evapotranspiration_fraction()
                .iter()
                .map(|&v| f64::from(v))
                .collect(),
            temperature_offset_c: f64::from(spec.temperature_offset_c()),
            moisture_scale: f64::from(spec.moisture_scale()),
            longwave,
        }
    }
}

/// Work-domain monthly result of one reduced evaluation.
#[derive(Debug, Clone)]
pub(crate) struct ReducedWorkClimate {
    /// Sea-level-reduced near-surface temperature (P4/LOD-2 coordinate).
    pub(crate) theta_c: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    /// Near-surface air temperature at the work-cell reference height.
    pub(crate) air_temperature_c: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) sea_surface_temperature_c: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) outgoing_longwave_w_m2: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) wind_m_s: Vec<[[f64; 3]; CLIMATE_MONTH_COUNT]>,
    pub(crate) ocean_current_m_s: Vec<[[f64; 3]; CLIMATE_MONTH_COUNT]>,
    pub(crate) sea_surface_height_m: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) specific_humidity: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) evaporation_mm_day: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) precipitation_mm_day: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) orographic_precipitation_mm_day: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) diagnostics: ReducedDiagnostics,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub(crate) struct ReducedDiagnostics {
    pub(crate) mebm_picard_iterations: usize,
    pub(crate) mebm_linear_iterations: usize,
    pub(crate) mebm_max_relative_residual: f64,
    pub(crate) toa_net_global_mean_w_m2: f64,
    pub(crate) toa_relative_error: f64,
    pub(crate) negative_olr_cell_months: usize,
    pub(crate) water_active_set_iterations_max: usize,
    pub(crate) water_linear_iterations: usize,
    pub(crate) water_max_relative_residual: f64,
    pub(crate) evaporation_global_mean_mm_day: f64,
    pub(crate) precipitation_global_mean_mm_day: f64,
    pub(crate) water_relative_imbalance: f64,
    pub(crate) water_max_monthly_relative_imbalance: f64,
    pub(crate) precipitable_water_global_mean_kg_m2: f64,
    pub(crate) residence_time_days: f64,
    pub(crate) ocean_linear_iterations: usize,
    pub(crate) ocean_max_relative_residual: f64,
    pub(crate) min_air_temperature_c: f64,
    pub(crate) max_air_temperature_c: f64,
}

/// Runs one complete reduced evaluation on the work grid.
pub(crate) fn solve_reduced_climate(
    grid: &ReducedGrid,
    inputs: &ReducedInputs,
    parallel: bool,
) -> ReducedWorkClimate {
    let mut diagnostics = ReducedDiagnostics {
        min_air_temperature_c: f64::INFINITY,
        max_air_temperature_c: f64::NEG_INFINITY,
        ..ReducedDiagnostics::default()
    };
    let energy = solve_moist_energy_balance(grid, inputs, parallel, &mut diagnostics);
    let months: Vec<usize> = (0..CLIMATE_MONTH_COUNT).collect();
    let column_mass = P4_LOWER_LAYER_REFERENCE_PRESSURE_PA / STANDARD_GRAVITY_M_S2;
    let vapour_column_mass = vapour_column_mass_kg_m2();
    let relative_humidity =
        (REFERENCE_SURFACE_RELATIVE_HUMIDITY * inputs.moisture_scale).clamp(0.0, 1.0);

    let month_work = |month: usize| {
        let theta: Vec<f64> = energy.iter().map(|m| m[month]).collect();
        let air: Vec<f64> = theta
            .iter()
            .zip(&inputs.reference_height_m)
            .map(|(t, h)| t - CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M * h)
            .collect();
        let mse: Vec<f64> = theta
            .iter()
            .map(|&t| moist_static_energy_j_kg(t, relative_humidity))
            .collect();
        let hadley = hadley_lower_branch(grid, &mse, column_mass);
        let zonal = eddy_driven_zonal_wind(grid, inputs, column_mass);
        let wind: Vec<[f64; 3]> = (0..grid.cell_count())
            .map(|cell| {
                let u = scale(grid.east[cell], zonal[cell]);
                tangent(add(u, hadley.cell_velocity[cell]), grid.center[cell])
            })
            .collect();
        let water = solve_monthly_water(
            grid,
            inputs,
            &WaterMonth {
                theta: &theta,
                air: &air,
                wind: &wind,
                zonal: &zonal,
                hadley_mass_flux: &hadley.edge_mass_flux_kg_s,
                vapour_column_mass,
            },
        );
        let ocean = solve_monthly_stommel(grid, inputs, &wind);
        (theta, air, wind, water, ocean)
    };
    let results: Vec<_> = if parallel {
        months.par_iter().map(|&m| month_work(m)).collect()
    } else {
        months.iter().map(|&m| month_work(m)).collect()
    };

    let n = grid.cell_count();
    let mut out = ReducedWorkClimate {
        theta_c: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        air_temperature_c: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        sea_surface_temperature_c: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        outgoing_longwave_w_m2: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        wind_m_s: vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; n],
        ocean_current_m_s: vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; n],
        sea_surface_height_m: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        specific_humidity: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        evaporation_mm_day: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        precipitation_mm_day: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        orographic_precipitation_mm_day: vec![[0.0; CLIMATE_MONTH_COUNT]; n],
        diagnostics: ReducedDiagnostics::default(),
    };
    let annual_asr: Vec<f64> = inputs
        .monthly_absorbed_shortwave_w_m2
        .iter()
        .map(|m| m.iter().sum::<f64>() / CLIMATE_MONTH_COUNT as f64)
        .collect();
    let (theta_eq, slope) = radiative_equilibrium(inputs, &annual_asr);
    let mut evaporation = 0.0;
    let mut precipitation = 0.0;
    let mut water_mass = 0.0;
    let mut total_area = 0.0;
    for (month, (theta, air, wind, water, ocean)) in results.into_iter().enumerate() {
        diagnostics.water_active_set_iterations_max = diagnostics
            .water_active_set_iterations_max
            .max(water.active_set_iterations);
        diagnostics.water_linear_iterations += water.linear_iterations;
        diagnostics.water_max_relative_residual = diagnostics
            .water_max_relative_residual
            .max(water.relative_residual);
        diagnostics.ocean_linear_iterations += ocean.linear_iterations;
        diagnostics.ocean_max_relative_residual = diagnostics
            .ocean_max_relative_residual
            .max(ocean.relative_residual);
        let mut month_e = 0.0;
        let mut month_p = 0.0;
        for cell in 0..n {
            let area = grid.area_m2[cell];
            out.theta_c[cell][month] = theta[cell];
            out.air_temperature_c[cell][month] = air[cell];
            out.sea_surface_temperature_c[cell][month] = theta[cell].max(LIQUID_MIXED_LAYER_MIN_C);
            let olr = annual_asr[cell] + slope[cell] * (theta[cell] - theta_eq[cell]);
            if olr < 0.0 {
                diagnostics.negative_olr_cell_months += 1;
            }
            out.outgoing_longwave_w_m2[cell][month] = olr;
            out.wind_m_s[cell][month] = wind[cell];
            out.ocean_current_m_s[cell][month] = ocean.current[cell];
            out.sea_surface_height_m[cell][month] = ocean.sea_surface_height[cell];
            out.specific_humidity[cell][month] = water.column_water[cell] / vapour_column_mass;
            out.evaporation_mm_day[cell][month] = water.evaporation[cell] * 86_400.0;
            out.precipitation_mm_day[cell][month] = water.precipitation[cell] * 86_400.0;
            out.orographic_precipitation_mm_day[cell][month] = water.orographic[cell] * 86_400.0;
            month_e += area * water.evaporation[cell];
            month_p += area * water.precipitation[cell];
            water_mass += area * water.column_water[cell];
            if month == 0 {
                total_area += area;
            }
            diagnostics.min_air_temperature_c = diagnostics.min_air_temperature_c.min(air[cell]);
            diagnostics.max_air_temperature_c = diagnostics.max_air_temperature_c.max(air[cell]);
        }
        diagnostics.water_max_monthly_relative_imbalance = diagnostics
            .water_max_monthly_relative_imbalance
            .max((month_e - month_p).abs() / month_e.max(month_p).max(f64::MIN_POSITIVE));
        evaporation += month_e;
        precipitation += month_p;
    }
    let weight = total_area * CLIMATE_MONTH_COUNT as f64;
    diagnostics.evaporation_global_mean_mm_day = evaporation / weight * 86_400.0;
    diagnostics.precipitation_global_mean_mm_day = precipitation / weight * 86_400.0;
    diagnostics.water_relative_imbalance =
        (evaporation - precipitation).abs() / evaporation.max(precipitation).max(f64::MIN_POSITIVE);
    diagnostics.precipitable_water_global_mean_kg_m2 = water_mass / weight;
    diagnostics.residence_time_days = diagnostics.precipitable_water_global_mean_kg_m2
        / diagnostics
            .precipitation_global_mean_mm_day
            .max(f64::MIN_POSITIVE);
    let mut asr_total = 0.0;
    let mut olr_total = 0.0;
    for ((area, asr), olr) in grid
        .area_m2
        .iter()
        .zip(&annual_asr)
        .zip(&out.outgoing_longwave_w_m2)
    {
        asr_total += area * asr;
        olr_total += area * olr.iter().sum::<f64>() / CLIMATE_MONTH_COUNT as f64;
    }
    diagnostics.toa_net_global_mean_w_m2 = (asr_total - olr_total) / total_area;
    diagnostics.toa_relative_error = (asr_total - olr_total).abs() / asr_total;
    out.diagnostics = diagnostics;
    out
}

/// Total reference vapour column mass of the production two-layer profile,
/// the single source converting near-surface specific humidity to water mass.
pub(crate) fn vapour_column_mass_kg_m2() -> f64 {
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    layout.moisture_column_mass_per_area(ClimateLayerRole::LowerAtmosphere)
        + layout.moisture_column_mass_per_area(ClimateLayerRole::UpperAtmosphere)
}

fn moist_static_energy_j_kg(temperature_c: f64, relative_humidity: f64) -> f64 {
    P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K * (temperature_c + 273.15)
        + WATER_VAPORIZATION_LATENT_HEAT_J_KG
            * relative_humidity
            * saturation_specific_humidity_kg_kg(temperature_c)
}

/// Sea-level equilibrium target and OLR slope so that every closure reads
/// `OLR = ASR_ann + B (theta - theta_eq)`. The gray closure is the production
/// pair of `forcing.rs`; the Budyko closure evaluates `A + B T` at the
/// reference-height temperature `T = theta - Gamma H` minus the global offset.
fn radiative_equilibrium(inputs: &ReducedInputs, annual_asr: &[f64]) -> (Vec<f64>, Vec<f64>) {
    annual_asr
        .iter()
        .zip(&inputs.reference_height_m)
        .map(|(&asr, &height)| match inputs.longwave {
            OutgoingLongwaveClosure::ProductionGray => {
                let theta =
                    gray_equilibrium_surface_temperature_c(asr) + inputs.temperature_offset_c;
                let slope = gray_longwave_slope_w_m2_k(
                    theta - CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M * height,
                );
                (theta, slope)
            }
            OutgoingLongwaveClosure::BudykoTransEbm => {
                let theta = (asr - TRANSEBM_OLR_INTERCEPT_W_M2) / TRANSEBM_OLR_SLOPE_W_M2_K
                    + CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M * height
                    + inputs.temperature_offset_c;
                (theta, TRANSEBM_OLR_SLOPE_W_M2_K)
            }
        })
        .unzip()
}

/// E1: periodic seasonal MEBM month means of sea-level-reduced temperature.
fn solve_moist_energy_balance(
    grid: &ReducedGrid,
    inputs: &ReducedInputs,
    parallel: bool,
    diagnostics: &mut ReducedDiagnostics,
) -> Vec<[f64; CLIMATE_MONTH_COUNT]> {
    let n = grid.cell_count();
    let column_mass = P4_LOWER_LAYER_REFERENCE_PRESSURE_PA / STANDARD_GRAVITY_M_S2;
    let relative_humidity =
        (REFERENCE_SURFACE_RELATIVE_HUMIDITY * inputs.moisture_scale).clamp(0.0, 1.0);
    let annual_asr: Vec<f64> = inputs
        .monthly_absorbed_shortwave_w_m2
        .iter()
        .map(|m| m.iter().sum::<f64>() / CLIMATE_MONTH_COUNT as f64)
        .collect();
    let (theta_eq, slope) = radiative_equilibrium(inputs, &annual_asr);
    let (air_capacity, mixed_capacity) = p4_seasonal_storage_heat_capacities_j_m2_k();
    let capacity: Vec<f64> = inputs
        .land_fraction
        .iter()
        .map(|land| air_capacity + (1.0 - land) * mixed_capacity)
        .collect();
    // Forcing G_m = ASR_m - ASR_ann + B theta_eq; its DFT gives the right sides.
    let forcing: Vec<[f64; CLIMATE_MONTH_COUNT]> = (0..n)
        .map(|cell| {
            std::array::from_fn(|month| {
                inputs.monthly_absorbed_shortwave_w_m2[cell][month] - annual_asr[cell]
                    + slope[cell] * theta_eq[cell]
            })
        })
        .collect();

    // Picard on the secant MSE conductance at the annual mean (k = 0, real CG).
    let mut annual = theta_eq.clone();
    let mut conductance = vec![0.0_f64; grid.edges.len()];
    let mut iterations = 0;
    for picard in 0..MEBM_PICARD_MAX_ITERATIONS {
        for (edge, k) in grid.edges.iter().zip(conductance.iter_mut()) {
            let [a, b] = edge.cells.map(|c| annual[c as usize]);
            let secant = if (a - b).abs() > 1.0e-6 {
                (moist_static_energy_j_kg(a, relative_humidity)
                    - moist_static_energy_j_kg(b, relative_humidity))
                    / (a - b)
            } else {
                let mid = 0.5 * (a + b);
                (moist_static_energy_j_kg(mid + 1.0e-3, relative_humidity)
                    - moist_static_energy_j_kg(mid - 1.0e-3, relative_humidity))
                    / 2.0e-3
            };
            *k = column_mass * MEBM_DIFFUSIVITY_M2_S * edge.length_m / edge.center_distance_m
                * secant;
        }
        let diag: Vec<f64> = (0..n).map(|c| grid.area_m2[c] * slope[c]).collect();
        let rhs: Vec<f64> = (0..n)
            .map(|c| grid.area_m2[c] * forcing[c].iter().sum::<f64>() / CLIMATE_MONTH_COUNT as f64)
            .collect();
        let system = SymmetricSystem::new(grid, &diag, &conductance);
        let solve = system.solve_real(&rhs, &annual);
        diagnostics.mebm_linear_iterations += solve.iterations;
        diagnostics.mebm_max_relative_residual = diagnostics
            .mebm_max_relative_residual
            .max(solve.relative_residual);
        let change = solve
            .values
            .iter()
            .zip(&annual)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f64, f64::max);
        annual = solve.values;
        iterations = picard + 1;
        if change < MEBM_PICARD_TOLERANCE_K {
            break;
        }
    }
    diagnostics.mebm_picard_iterations = iterations;

    // Harmonics k = 1..5 of the exact periodic Crank-Nicolson month means;
    // k = 6 vanishes identically (see the design, §E1).
    let step = SECONDS_PER_CLIMATOLOGICAL_MONTH;
    let harmonic = |k: usize| {
        let shift = 2.0 / step * (std::f64::consts::PI * k as f64 / 12.0).tan();
        let rhs: Vec<Complex> = (0..n)
            .map(|cell| {
                let mut sum = Complex::ZERO;
                for (month, value) in forcing[cell].iter().enumerate() {
                    let phase = -std::f64::consts::TAU * (k * month) as f64 / 12.0;
                    sum = sum + Complex::from_polar(*value, phase);
                }
                sum.scale(grid.area_m2[cell] / CLIMATE_MONTH_COUNT as f64)
            })
            .collect();
        let diag: Vec<Complex> = (0..n)
            .map(|c| Complex {
                re: grid.area_m2[c] * slope[c],
                im: grid.area_m2[c] * shift * capacity[c],
            })
            .collect();
        SymmetricSystem::new_complex(grid, &diag, &conductance).solve_complex(&rhs)
    };
    let harmonics: Vec<ComplexSolve> = if parallel {
        (1..=5).into_par_iter().map(harmonic).collect()
    } else {
        (1..=5).map(harmonic).collect()
    };
    for solve in &harmonics {
        diagnostics.mebm_linear_iterations += solve.iterations;
        diagnostics.mebm_max_relative_residual = diagnostics
            .mebm_max_relative_residual
            .max(solve.relative_residual);
    }
    (0..n)
        .map(|cell| {
            std::array::from_fn(|month| {
                let mut value = annual[cell];
                for (index, solve) in harmonics.iter().enumerate() {
                    let k = index + 1;
                    let phase = std::f64::consts::TAU * (k * month) as f64 / 12.0;
                    let z = solve.values[cell] * Complex::from_polar(1.0, phase);
                    value += 2.0 * z.re;
                }
                value
            })
        })
        .collect()
}

struct HadleyBranch {
    /// Signed lower-branch mass flux per edge, positive from cell 0 to cell 1.
    edge_mass_flux_kg_s: Vec<f64>,
    cell_velocity: Vec<[f64; 3]>,
}

/// W2: Siler et al. (2018) Hadley partition of the diffusive MSE flux. The
/// lower branch moves toward higher MSE with mass flux `w F / g`, where the
/// gross moist stability `g = h_T - [h]` uses Siler's zonal-mean form: `[h]` is
/// the band-mean MSE at the edge latitude and `h_T = 1.06 max [h]` (their
/// `1.06 h(0)` taken at the thermal equator of the month).
fn hadley_lower_branch(grid: &ReducedGrid, mse: &[f64], column_mass: f64) -> HadleyBranch {
    let bands = grid.band_count;
    let mut band_sum = vec![0.0_f64; bands];
    let mut band_area = vec![0.0_f64; bands];
    for (cell, &h) in mse.iter().enumerate() {
        let band = grid.cell_band[cell];
        band_sum[band] += grid.area_m2[cell] * h;
        band_area[band] += grid.area_m2[cell];
    }
    let band_mean: Vec<f64> = band_sum
        .iter()
        .zip(&band_area)
        .map(|(s, a)| s / a)
        .collect();
    let upper_mse = HADLEY_UPPER_MSE_RATIO * band_mean.iter().copied().fold(f64::MIN, f64::max);
    let lower_branch_mass = (P4_LOWER_LAYER_REFERENCE_PRESSURE_PA
        - HADLEY_LOWER_BRANCH_TOP_PRESSURE_PA)
        / STANDARD_GRAVITY_M_S2;
    let edge_mass_flux_kg_s: Vec<f64> = grid
        .edges
        .iter()
        .enumerate()
        .map(|(edge_id, edge)| {
            let [a, b] = edge.cells.map(|c| mse[c as usize]);
            // Total diffusive MSE flux from cell 0 to cell 1, in W.
            let flux = column_mass * MEBM_DIFFUSIVITY_M2_S * edge.length_m / edge.center_distance_m
                * (a - b);
            let gross_moist_stability = upper_mse - band_mean[grid.edge_band[edge_id]];
            // Lower branch flows against the energy flux.
            -edge.hadley_weight * flux / gross_moist_stability
        })
        .collect();
    let mut cell_velocity = vec![[0.0; 3]; grid.cell_count()];
    for (edge, &mass_flux) in grid.edges.iter().zip(&edge_mass_flux_kg_s) {
        let normal_velocity = mass_flux / (lower_branch_mass * edge.length_m);
        for slot in 0..2 {
            let cell = edge.cells[slot] as usize;
            let outward = if slot == 0 {
                normal_velocity
            } else {
                -normal_velocity
            };
            // Perot (2000) reconstruction: u = (1/A) sum (u.n) l (x_f - x_c).
            let contribution = scale(
                edge.displacement[slot],
                outward * edge.length_m / grid.area_m2[cell],
            );
            cell_velocity[cell] = add(cell_velocity[cell], contribution);
        }
    }
    for (cell, velocity) in cell_velocity.iter_mut().enumerate() {
        *velocity = tangent(*velocity, grid.center[cell]);
    }
    HadleyBranch {
        edge_mass_flux_kg_s,
        cell_velocity,
    }
}

/// W1: zonal surface wind balancing the column eddy torque with the bulk
/// Large & Yeager (2004) drag, `rho C_D|U| u = M_col a_eddy(lat)`.
fn eddy_driven_zonal_wind(
    grid: &ReducedGrid,
    inputs: &ReducedInputs,
    column_mass: f64,
) -> Vec<f64> {
    (0..grid.cell_count())
        .map(|cell| {
            let acceleration = crate::world::natural::observed_transient_eddy_acceleration_m_s2(
                grid.latitude_rad[cell],
                grid.radius_m,
            );
            let stress = column_mass * acceleration;
            let drag_ratio = 1.0 + (LAND_SEA_SURFACE_DRAG_RATIO - 1.0) * inputs.land_fraction[cell];
            let speed = solve_drag_speed(stress.abs(), drag_ratio);
            speed * stress.signum()
        })
        .collect()
}

fn solve_drag_speed(stress: f64, drag_ratio: f64) -> f64 {
    let stress_at = |speed: f64| {
        P4_REFERENCE_AIR_DENSITY_KG_M3
            * drag_ratio
            * crate::world::natural::neutral_surface_momentum_transfer_velocity_m_s(speed)
            * speed
    };
    let (mut low, mut high) = (0.0_f64, 100.0_f64);
    for _ in 0..80 {
        let mid = 0.5 * (low + high);
        if stress_at(mid) < stress {
            low = mid;
        } else {
            high = mid;
        }
    }
    0.5 * (low + high)
}

struct WaterMonth<'a> {
    theta: &'a [f64],
    air: &'a [f64],
    wind: &'a [[f64; 3]],
    zonal: &'a [f64],
    hadley_mass_flux: &'a [f64],
    vapour_column_mass: f64,
}

struct WaterSolution {
    column_water: Vec<f64>,
    evaporation: Vec<f64>,
    precipitation: Vec<f64>,
    orographic: Vec<f64>,
    active_set_iterations: usize,
    linear_iterations: usize,
    relative_residual: f64,
}

/// Q1: monthly quasi-steady column-vapour budget.
fn solve_monthly_water(
    grid: &ReducedGrid,
    inputs: &ReducedInputs,
    month: &WaterMonth<'_>,
) -> WaterSolution {
    let n = grid.cell_count();
    let mq = month.vapour_column_mass;
    let speed: Vec<f64> = month.wind.iter().map(|w| norm(*w)).collect();
    let sst: Vec<f64> = month
        .theta
        .iter()
        .map(|t| t.max(LIQUID_MIXED_LAYER_MIN_C))
        .collect();
    let evaporation_scale = P4_REFERENCE_AIR_DENSITY_KG_M3 * BULK_MOISTURE_TRANSFER_COEFFICIENT;
    // Linear bulk evaporation E = a - b W from the production formula.
    let a: Vec<f64> = (0..n)
        .map(|c| {
            bulk_surface_evaporation_kg_m2_s(sst[c], 0.0, speed[c], 1.0 - inputs.land_fraction[c])
        })
        .collect();
    let sea_saturation: Vec<f64> = (0..n)
        .map(|c| bulk_surface_evaporation_kg_m2_s(sst[c], 0.0, 1.0, 1.0) / evaporation_scale)
        .collect();
    let b: Vec<f64> = (0..n)
        .map(|c| {
            if sea_saturation[c] > 0.0 {
                a[c] / (sea_saturation[c] * mq)
            } else {
                0.0
            }
        })
        .collect();
    let critical: Vec<f64> = month
        .air
        .iter()
        .map(|&t| {
            P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY
                * saturation_specific_humidity_kg_kg(t)
                * mq
        })
        .collect();
    let upslope: Vec<f64> = (0..n)
        .map(|c| dot(month.wind[c], inputs.reference_height_gradient[c]).max(0.0))
        .collect();
    let relaxation = 1.0 / P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS;

    // Transport rows: off-diagonal (neighbour slot) and diagonal outflow.
    let mut transport_diag = vec![0.0_f64; n];
    let mut transport_off = vec![[0.0_f64; 4]; n];
    let slot_of = |cell: usize, edge_id: u32| -> usize {
        grid.cell_edges[cell]
            .iter()
            .position(|(e, _)| *e == edge_id)
            .expect("edge belongs to cell")
    };
    for (edge_id, edge) in grid.edges.iter().enumerate() {
        let [i, j] = edge.cells.map(|c| c as usize);
        let si = slot_of(i, edge_id as u32);
        let sj = slot_of(j, edge_id as u32);
        // Eddy part of the Siler partition diffuses vapour: F = -(1-w) D grad W.
        let diffusion = (1.0 - edge.hadley_weight) * MEBM_DIFFUSIVITY_M2_S * edge.length_m
            / edge.center_distance_m;
        transport_diag[i] += diffusion;
        transport_diag[j] += diffusion;
        transport_off[i][si] -= diffusion;
        transport_off[j][sj] -= diffusion;
        // Hadley lower branch carries near-surface q = W / M_q upwind.
        let hadley = month.hadley_mass_flux[edge_id] / mq;
        // Zonal-mean eddy-driven wind advects the column, first-order upwind.
        let mid_east = east_unit(edge.midpoint);
        let zonal_mid = 0.5 * (month.zonal[i] + month.zonal[j]);
        let advective = zonal_mid * dot(mid_east, edge.normal) * edge.length_m + hadley;
        if advective > 0.0 {
            transport_diag[i] += advective;
            transport_off[j][sj] -= advective;
        } else {
            transport_diag[j] -= advective;
            transport_off[i][si] += advective;
        }
    }

    let mut water: Vec<f64> = critical.clone();
    let mut large_scale_active = vec![true; n];
    let mut evaporation_active = vec![true; n];
    let mut orographic_rate = vec![0.0_f64; n];
    let mut iterations = 0;
    let mut linear_iterations = 0;
    let mut relative_residual = 0.0_f64;
    for pass in 0..WATER_ACTIVE_SET_MAX_ITERATIONS {
        for c in 0..n {
            let q = (water[c] / mq).max(1.0e-12);
            let condensation = lcl_adjusted_orographic_condensation_kg_m2_s(
                q,
                month.air[c],
                upslope[c],
                speed[c],
                grid.area_m2[c],
            );
            orographic_rate[c] = condensation / (q * mq);
        }
        let mut diag = vec![0.0_f64; n];
        let mut rhs = vec![0.0_f64; n];
        for c in 0..n {
            let area = grid.area_m2[c];
            let beta = inputs.land_evapotranspiration_fraction[c];
            let ls = if large_scale_active[c] {
                relaxation
            } else {
                0.0
            };
            let sink = (1.0 - beta) * (ls + orographic_rate[c])
                + if evaporation_active[c] { b[c] } else { 0.0 };
            diag[c] = transport_diag[c] + area * sink;
            rhs[c] = area
                * ((1.0 - beta) * ls * critical[c]
                    + if evaporation_active[c] { a[c] } else { 0.0 });
        }
        let solve = solve_bicgstab(grid, &diag, &transport_off, &rhs, &water);
        linear_iterations += solve.iterations;
        relative_residual = relative_residual.max(solve.relative_residual);
        water = solve.values;
        iterations = pass + 1;
        let mut changed = false;
        for c in 0..n {
            let ls = water[c] > critical[c];
            let ev = a[c] - b[c] * water[c] > 0.0;
            changed |= ls != large_scale_active[c] || ev != evaporation_active[c];
            large_scale_active[c] = ls;
            evaporation_active[c] = ev;
        }
        if !changed && pass > 0 {
            break;
        }
    }
    let mut evaporation = vec![0.0; n];
    let mut precipitation = vec![0.0; n];
    let mut orographic = vec![0.0; n];
    for c in 0..n {
        let ls = if large_scale_active[c] {
            relaxation * (water[c] - critical[c])
        } else {
            0.0
        };
        orographic[c] = orographic_rate[c] * water[c];
        precipitation[c] = ls + orographic[c];
        let ocean = if evaporation_active[c] {
            a[c] - b[c] * water[c]
        } else {
            0.0
        };
        evaporation[c] = ocean + inputs.land_evapotranspiration_fraction[c] * precipitation[c];
    }
    WaterSolution {
        column_water: water,
        evaporation,
        precipitation,
        orographic,
        active_set_iterations: iterations,
        linear_iterations,
        relative_residual,
    }
}

struct OceanSolution {
    current: Vec<[f64; 3]>,
    sea_surface_height: Vec<f64>,
    linear_iterations: usize,
    relative_residual: f64,
}

/// O1: Stommel (1948) `r lap psi + beta psi_x = curl tau / rho_0` on wet
/// cells (land fraction < 1/2) with `psi = 0` on land; surface velocity is the
/// geostrophic transport over the production mixed layer plus thermocline.
fn solve_monthly_stommel(
    grid: &ReducedGrid,
    inputs: &ReducedInputs,
    wind: &[[f64; 3]],
) -> OceanSolution {
    let n = grid.cell_count();
    let wet: Vec<bool> = inputs.land_fraction.iter().map(|&l| l < 0.5).collect();
    let stress: Vec<[f64; 3]> = (0..n)
        .map(|c| {
            let speed = norm(wind[c]);
            scale(
                wind[c],
                P4_REFERENCE_AIR_DENSITY_KG_M3
                    * crate::world::natural::neutral_surface_momentum_transfer_velocity_m_s(speed),
            )
        })
        .collect();
    // Numerical closure, not a physical constant: the Stommel layer width
    // r / beta is set to the mean grid spacing at the equatorial beta, so the
    // western boundary current is resolved on the coarse work grid (the
    // boundary-layer resolution criterion of coarse ocean models; see design).
    let friction = 2.0 * EARTH_ROTATION_RATE_RAD_S / grid.radius_m * grid.mean_spacing_m;
    let mut diag = vec![0.0_f64; n];
    let mut off = vec![[0.0_f64; 4]; n];
    let mut rhs = vec![0.0_f64; n];
    for c in 0..n {
        if !wet[c] {
            diag[c] = 1.0;
            continue;
        }
        let beta = 2.0 * EARTH_ROTATION_RATE_RAD_S * grid.latitude_rad[c].cos() / grid.radius_m;
        let mut curl = 0.0;
        for (slot, &(edge_id, owner)) in grid.cell_edges[c].iter().enumerate() {
            let edge = &grid.edges[edge_id as usize];
            let outward = if owner == 0 {
                edge.normal
            } else {
                scale(edge.normal, -1.0)
            };
            let other = edge.cells[1 - owner as usize] as usize;
            let tangent_ccw = cross(edge.midpoint, outward);
            let edge_stress = scale(add(stress[c], stress[other]), 0.5);
            curl += dot(edge_stress, tangent_ccw) * edge.length_m;
            let laplacian = friction * edge.length_m / edge.center_distance_m;
            let eastward = dot(grid.east[c], outward);
            let beta_term = if eastward > 0.0 {
                beta * eastward * edge.length_m
            } else {
                0.0
            };
            diag[c] += laplacian + beta_term;
            off[c][slot] -= laplacian + beta_term;
        }
        rhs[c] = -curl / SEAWATER_DENSITY_KG_M3;
        // Dry neighbours hold psi = 0 and drop out of the row.
        for (slot, &(edge_id, owner)) in grid.cell_edges[c].iter().enumerate() {
            let other = grid.edges[edge_id as usize].cells[1 - owner as usize] as usize;
            if !wet[other] {
                off[c][slot] = 0.0;
            }
        }
    }
    let solve = solve_bicgstab(grid, &diag, &off, &rhs, &vec![0.0; n]);
    let psi = solve.values;
    let operators = CirculationOperators::new(&grid.operators_grid);
    let psi_f32: Vec<f32> = psi.iter().map(|&v| v as f32).collect();
    let gradient = operators.gradient(&psi_f32).expect("finite streamfunction");
    let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
    let wind_driven_depth: f64 = layout
        .layers()
        .iter()
        .filter(|layer| {
            matches!(
                layer.role(),
                ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline
            )
        })
        .map(|layer| layer.reference_thickness_m())
        .sum();
    let mut current = vec![[0.0; 3]; n];
    let mut sea_surface_height = vec![0.0; n];
    for c in 0..n {
        if !wet[c] {
            continue;
        }
        let g = gradient[c].map(f64::from);
        current[c] = tangent(
            scale(cross(grid.center[c], g), 1.0 / wind_driven_depth),
            grid.center[c],
        );
        let coriolis = 2.0 * EARTH_ROTATION_RATE_RAD_S * grid.latitude_rad[c].sin();
        sea_surface_height[c] = coriolis * psi[c] / (STANDARD_GRAVITY_M_S2 * wind_driven_depth);
    }
    OceanSolution {
        current,
        sea_surface_height,
        linear_iterations: solve.iterations,
        relative_residual: solve.relative_residual,
    }
}

// ---------------------------------------------------------------------------
// Linear algebra on the four-neighbour cubed-sphere stencil.

pub(crate) struct RealSolve {
    pub(crate) values: Vec<f64>,
    pub(crate) iterations: usize,
    pub(crate) relative_residual: f64,
}

struct ComplexSolve {
    values: Vec<Complex>,
    iterations: usize,
    relative_residual: f64,
}

struct SymmetricSystem<'a, T> {
    grid: &'a ReducedGrid,
    diag: Vec<T>,
    conductance: &'a [f64],
}

impl<'a> SymmetricSystem<'a, f64> {
    /// `(diag + sum K) x_i - sum K x_j`.
    fn new(grid: &'a ReducedGrid, diag: &[f64], conductance: &'a [f64]) -> Self {
        let mut full = diag.to_vec();
        for (edge, k) in grid.edges.iter().zip(conductance) {
            full[edge.cells[0] as usize] += k;
            full[edge.cells[1] as usize] += k;
        }
        Self {
            grid,
            diag: full,
            conductance,
        }
    }

    fn apply(&self, x: &[f64], y: &mut [f64]) {
        for (yi, (d, xi)) in y.iter_mut().zip(self.diag.iter().zip(x)) {
            *yi = d * xi;
        }
        for (edge, k) in self.grid.edges.iter().zip(self.conductance) {
            let [i, j] = edge.cells.map(|c| c as usize);
            y[i] -= k * x[j];
            y[j] -= k * x[i];
        }
    }

    /// Jacobi-preconditioned conjugate gradients.
    fn solve_real(&self, b: &[f64], initial: &[f64]) -> RealSolve {
        let n = b.len();
        let mut x = initial.to_vec();
        let mut r = vec![0.0; n];
        self.apply(&x, &mut r);
        for (ri, bi) in r.iter_mut().zip(b) {
            *ri = bi - *ri;
        }
        let b_norm = b
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt()
            .max(f64::MIN_POSITIVE);
        let mut z: Vec<f64> = r.iter().zip(&self.diag).map(|(r, d)| r / d).collect();
        let mut p = z.clone();
        let mut rz: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
        let mut q = vec![0.0; n];
        let mut iterations = 0;
        let mut residual = r.iter().map(|v| v * v).sum::<f64>().sqrt() / b_norm;
        while residual > LINEAR_TOLERANCE && iterations < LINEAR_MAX_ITERATIONS {
            self.apply(&p, &mut q);
            let alpha = rz / p.iter().zip(&q).map(|(a, b)| a * b).sum::<f64>();
            for i in 0..n {
                x[i] += alpha * p[i];
                r[i] -= alpha * q[i];
            }
            for i in 0..n {
                z[i] = r[i] / self.diag[i];
            }
            let rz_next: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
            let beta = rz_next / rz;
            rz = rz_next;
            for i in 0..n {
                p[i] = z[i] + beta * p[i];
            }
            iterations += 1;
            residual = r.iter().map(|v| v * v).sum::<f64>().sqrt() / b_norm;
        }
        RealSolve {
            values: x,
            iterations,
            relative_residual: residual,
        }
    }
}

impl<'a> SymmetricSystem<'a, Complex> {
    fn new_complex(grid: &'a ReducedGrid, diag: &[Complex], conductance: &'a [f64]) -> Self {
        let mut full = diag.to_vec();
        for (edge, k) in grid.edges.iter().zip(conductance) {
            full[edge.cells[0] as usize].re += k;
            full[edge.cells[1] as usize].re += k;
        }
        Self {
            grid,
            diag: full,
            conductance,
        }
    }

    fn apply(&self, x: &[Complex], y: &mut [Complex]) {
        for (yi, (d, xi)) in y.iter_mut().zip(self.diag.iter().zip(x)) {
            *yi = *d * *xi;
        }
        for (edge, k) in self.grid.edges.iter().zip(self.conductance) {
            let [i, j] = edge.cells.map(|c| c as usize);
            y[i] = y[i] - x[j].scale(*k);
            y[j] = y[j] - x[i].scale(*k);
        }
    }

    /// Jacobi-preconditioned COCG (van der Vorst & Melissen 1990) for the
    /// complex-symmetric shifted system; dot products are unconjugated.
    fn solve_complex(&self, b: &[Complex]) -> ComplexSolve {
        let n = b.len();
        let mut x = vec![Complex::ZERO; n];
        let mut r = b.to_vec();
        let norm2 = |v: &[Complex]| {
            v.iter()
                .map(|c| c.re * c.re + c.im * c.im)
                .sum::<f64>()
                .sqrt()
        };
        let b_norm = norm2(b).max(f64::MIN_POSITIVE);
        let bilinear = |a: &[Complex], c: &[Complex]| {
            a.iter()
                .zip(c)
                .fold(Complex::ZERO, |sum, (u, v)| sum + *u * *v)
        };
        let mut z: Vec<Complex> = r.iter().zip(&self.diag).map(|(r, d)| *r / *d).collect();
        let mut p = z.clone();
        let mut rz = bilinear(&r, &z);
        let mut q = vec![Complex::ZERO; n];
        let mut iterations = 0;
        let mut residual = norm2(&r) / b_norm;
        while residual > LINEAR_TOLERANCE && iterations < LINEAR_MAX_ITERATIONS {
            self.apply(&p, &mut q);
            let alpha = rz / bilinear(&p, &q);
            for i in 0..n {
                x[i] = x[i] + alpha * p[i];
                r[i] = r[i] - alpha * q[i];
            }
            for i in 0..n {
                z[i] = r[i] / self.diag[i];
            }
            let rz_next = bilinear(&r, &z);
            let beta = rz_next / rz;
            rz = rz_next;
            for i in 0..n {
                p[i] = z[i] + beta * p[i];
            }
            iterations += 1;
            residual = norm2(&r) / b_norm;
        }
        ComplexSolve {
            values: x,
            iterations,
            relative_residual: residual,
        }
    }
}

/// Jacobi-preconditioned BiCGSTAB (van der Vorst 1992) on a general
/// four-neighbour row stencil `diag_i x_i + sum_s off_is x_nbr(i,s)`.
fn solve_bicgstab(
    grid: &ReducedGrid,
    diag: &[f64],
    off: &[[f64; 4]],
    b: &[f64],
    initial: &[f64],
) -> RealSolve {
    let n = b.len();
    let neighbour = |cell: usize, slot: usize| {
        let (edge_id, owner) = grid.cell_edges[cell][slot];
        grid.edges[edge_id as usize].cells[1 - owner as usize] as usize
    };
    let neighbours: Vec<[usize; 4]> = (0..n)
        .map(|c| std::array::from_fn(|s| neighbour(c, s)))
        .collect();
    let apply = |x: &[f64], y: &mut [f64]| {
        for c in 0..n {
            let mut v = diag[c] * x[c];
            for s in 0..4 {
                v += off[c][s] * x[neighbours[c][s]];
            }
            y[c] = v;
        }
    };
    let dotp = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut x = initial.to_vec();
    let mut r = vec![0.0; n];
    apply(&x, &mut r);
    for (ri, bi) in r.iter_mut().zip(b) {
        *ri = bi - *ri;
    }
    let b_norm = dotp(b, b).sqrt().max(f64::MIN_POSITIVE);
    let r_hat = r.clone();
    let (mut rho, mut alpha, mut omega) = (1.0_f64, 1.0_f64, 1.0_f64);
    let mut v = vec![0.0; n];
    let mut p = vec![0.0; n];
    let mut y = vec![0.0; n];
    let mut s = vec![0.0; n];
    let mut z = vec![0.0; n];
    let mut t = vec![0.0; n];
    let mut iterations = 0;
    let mut residual = dotp(&r, &r).sqrt() / b_norm;
    while residual > LINEAR_TOLERANCE && iterations < LINEAR_MAX_ITERATIONS {
        let rho_next = dotp(&r_hat, &r);
        if rho_next == 0.0 {
            break;
        }
        let beta = (rho_next / rho) * (alpha / omega);
        rho = rho_next;
        for i in 0..n {
            p[i] = r[i] + beta * (p[i] - omega * v[i]);
            y[i] = p[i] / diag[i];
        }
        apply(&y, &mut v);
        alpha = rho / dotp(&r_hat, &v);
        for i in 0..n {
            s[i] = r[i] - alpha * v[i];
            z[i] = s[i] / diag[i];
        }
        apply(&z, &mut t);
        let tt = dotp(&t, &t);
        omega = if tt > 0.0 { dotp(&t, &s) / tt } else { 0.0 };
        for i in 0..n {
            x[i] += alpha * y[i] + omega * z[i];
            r[i] = s[i] - omega * t[i];
        }
        iterations += 1;
        residual = dotp(&r, &r).sqrt() / b_norm;
        if omega == 0.0 {
            break;
        }
    }
    RealSolve {
        values: x,
        iterations,
        relative_residual: residual,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const ZERO: Self = Self { re: 0.0, im: 0.0 };

    fn from_polar(magnitude: f64, phase: f64) -> Self {
        Self {
            re: magnitude * phase.cos(),
            im: magnitude * phase.sin(),
        }
    }

    fn scale(self, factor: f64) -> Self {
        Self {
            re: self.re * factor,
            im: self.im * factor,
        }
    }
}

impl std::ops::Add for Complex {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }
}

impl std::ops::Sub for Complex {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self {
            re: self.re - o.re,
            im: self.im - o.im,
        }
    }
}

impl std::ops::Mul for Complex {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
}

impl std::ops::Div for Complex {
    type Output = Self;
    fn div(self, o: Self) -> Self {
        let d = o.re * o.re + o.im * o.im;
        Self {
            re: (self.re * o.re + self.im * o.im) / d,
            im: (self.im * o.re - self.re * o.im) / d,
        }
    }
}

// ---------------------------------------------------------------------------
// Small vector helpers.

pub(crate) fn east_unit(center: [f64; 3]) -> [f64; 3] {
    let east = [-center[1], center[0], 0.0];
    let length = norm(east);
    if length <= 1.0e-12 {
        [1.0, 0.0, 0.0]
    } else {
        scale(east, 1.0 / length)
    }
}

fn tangent(v: [f64; 3], center: [f64; 3]) -> [f64; 3] {
    add(v, scale(center, -dot(v, center)))
}

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
