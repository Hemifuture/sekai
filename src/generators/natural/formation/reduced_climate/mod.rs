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
pub(in crate::generators::natural) mod publish;

use rayon::prelude::*;

use crate::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
use crate::world::natural::{
    absorbed_shortwave_w_m2, bulk_surface_evaporation_kg_m2_s,
    gray_equilibrium_surface_temperature_c, gray_longwave_slope_w_m2_k,
    lcl_adjusted_orographic_condensation_kg_m2_s, p4_seasonal_storage_heat_capacities_j_m2_k,
    saturation_specific_humidity_kg_kg, ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile,
    ClimateSpec, BULK_MOISTURE_TRANSFER_COEFFICIENT, CLIMATE_MONTH_COUNT,
    CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M, EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE,
    EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN, EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN,
    EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2, EARTH_ROTATION_RATE_RAD_S,
    P4_DRY_AIR_SPECIFIC_HEAT_CAPACITY_J_KG_K, P4_HIGHLAND_ALBEDO_RAMP_ONSET_M,
    P4_HIGHLAND_ALBEDO_RAMP_SPAN_M, P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT,
    P4_LARGE_SCALE_CONDENSATION_RELATIVE_HUMIDITY, P4_LARGE_SCALE_CONDENSATION_RELAXATION_SECONDS,
    P4_LOWER_LAYER_REFERENCE_PRESSURE_PA, P4_OPEN_OCEAN_SURFACE_ALBEDO,
    P4_REFERENCE_AIR_DENSITY_KG_M3, P4_SEA_ICE_SURFACE_ALBEDO,
    P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT, REFERENCE_SURFACE_RELATIVE_HUMIDITY,
    SECONDS_PER_CLIMATOLOGICAL_MONTH, STANDARD_GRAVITY_M_S2, WATER_VAPORIZATION_LATENT_HEAT_J_KG,
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

/// Budyko (1969), Tellus 21, 611: the permanent ice/snow edge sits where the
/// annual-mean surface temperature is -10 C; North (1975), JAS 32, 2033,
/// adopts the same edge temperature for the step albedo of his EBM.
const BUDYKO_ICE_EDGE_ANNUAL_TEMPERATURE_C: f64 = -10.0;
const ICE_ALBEDO_MAX_ITERATIONS: usize = 80;

/// TransEBM v1.0 (Ziegler & Rehfeld 2021, Table 1): latitude dependence of the
/// ice-free planetary albedo, `a + a_lat P2(sin lat)`, for ocean and land. The
/// A + B T closure above was tuned together with this albedo structure; the
/// P2 term carries the zenith-angle and cloud brightening of high latitudes
/// that a latitude-independent atmospheric reflectance cannot represent.
const TRANSEBM_OCEAN_LATITUDE_ALBEDO: f64 = 0.08;
const TRANSEBM_LAND_LATITUDE_ALBEDO: f64 = 0.05;
/// TransEBM v1.0 (Ziegler & Rehfeld 2021, Table 1, after Zhuang et al. 2017):
/// heat capacity of the sea-ice surface type, which replaces the mixed-layer
/// capacity where the ocean is ice covered (the ice insulates the ocean from
/// the atmosphere; Wagner & Eisenman 2015).
const TRANSEBM_SEA_ICE_HEAT_CAPACITY_J_M2_K: f64 = 4.83e6;
/// SPEEDY (Molteni 2003, Clim. Dyn. 20, 175; Kucharski et al. 2013, BAMS 94,
/// 25; reference code speedy.f90 `surface_fluxes.f90`, `vgust = 5.0`): wind
/// speed of sub-grid-scale gusts added in quadrature to the resolved wind in
/// the bulk surface-flux formulae. The bulk flux is nonlinear in the wind, so
/// a mean wind without its variability underestimates the mean flux
/// (Esbensen & Reynolds 1981, JPO 11, 457).
const SPEEDY_SUBGRID_GUST_WIND_M_S: f64 = 5.0;
/// Standard deviation of each horizontal 10 m wind component about its
/// monthly vector mean over the ocean, in 5-degree |latitude| bands from the
/// equator (0-5, 5-10, ..., 85-90 degrees), hemispherically symmetrized.
/// Measured for design revision R3 from NCEP/NCAR Reanalysis 1 6-hourly
/// 10 m winds 2016-2020 (Kalnay et al. 1996; NOAA PSL
/// `ncep.reanalysis/surface_gauss/{u,v}wnd.10m.gauss.YYYY.nc` with the
/// `land.sfc.gauss.nc` mask, MD5 sums in the design): per ocean grid point
/// and calendar month `sigma^2 = (var u + var v) / 2`, cos-latitude weights,
/// northern and southern band means averaged (85-90 degrees has ocean only in
/// the north). 6-hourly is the finest R1 sampling; the bulk formula applies
/// to instantaneous wind. With these values the Rice expectation reproduces
/// the observed monthly scalar mean ocean speed (7.0 m/s) to -1.9 % (mean)
/// and 3.1 % (RMS), while the vector-mean speed is 34 % low.
const SUBMONTHLY_WIND_COMPONENT_SIGMA_M_S: [f64; 18] = [
    1.78, 1.92, 2.09, 2.40, 3.03, 3.86, 4.61, 5.13, 5.49, 5.70, 5.74, 5.55, 5.17, 4.41, 3.96, 3.43,
    2.88, 3.40,
];

/// Physics added in design revision R2 (probe comparison axes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub(crate) struct ReducedPhysics {
    /// TransEBM Legendre latitude term on the ice-free planetary albedo.
    pub(crate) latitude_albedo: bool,
    /// Ocean parts whose annual mixed-layer temperature is below the
    /// seawater freezing point become the sea-ice surface type: sea-ice
    /// albedo, sea-ice heat capacity, no open-water evaporation.
    pub(crate) sea_ice_surface_type: bool,
    /// R2: bulk evaporation uses `sqrt(|V|^2 + V_gust^2)` with the SPEEDY
    /// gust constant instead of the diagnostic monthly-mean wind speed.
    pub(crate) evaporation_gustiness: bool,
    /// R3: bulk evaporation uses the expected scalar speed of an isotropic
    /// Gaussian wind about the monthly mean (Rice distribution; Wright &
    /// Thompson 1983) with the observed sub-monthly component spread.
    pub(crate) evaporation_wind_variability: bool,
    /// R3: the eddy vapour flux is the latent part of the MEBM eddy flux,
    /// `(1 - w) D (p_s / g) grad q` (Siler et al. 2018), instead of
    /// diffusing the column water `W = M_q q` with the same `D`.
    pub(crate) column_consistent_eddy_moisture: bool,
    /// R3: land evapotranspiration is the bucket recycling share `beta P`
    /// but never more than the potential evaporation of a wet surface
    /// (Manabe 1969 bucket, `E <= E_pot`).
    pub(crate) land_potential_evaporation_cap: bool,
    /// R3 experiment: the Siler Hadley share acts on the meridional part of
    /// the MSE flux only (the zonal-mean theory has no zonal overturning).
    pub(crate) hadley_meridional_only: bool,
    /// R3: the vapour column is advected zonally by the latitude-band mean of
    /// the eddy-driven zonal wind (the zonal-mean flow of the W1 balance),
    /// not by the local near-surface wind whose land drag slows it at coasts.
    pub(crate) band_mean_zonal_vapour_advection: bool,
    /// R4: the band-mean zonal flow advects the vapour column through edge
    /// fluxes taken as streamfunction differences between the edge vertices,
    /// `F_e = psi(v-) - psi(v+)` with `psi = -a int u dphi`, so the discrete
    /// divergence of the advecting flow vanishes identically (the C-grid
    /// streamfunction construction of Arakawa & Lamb 1977; Ringler et al.
    /// 2010). Takes precedence over `band_mean_zonal_vapour_advection`.
    pub(crate) nondivergent_zonal_vapour_advection: bool,
}

/// Streamfunction `psi(phi) = -a int_{-pi/2}^{phi} u dphi'` of a zonal flow
/// that is piecewise linear in latitude through the band-centre values and
/// constant beyond the outermost centres; exact integral of that profile.
struct ZonalStreamfunction {
    nodes: Vec<f64>,
    speed: Vec<f64>,
    psi: Vec<f64>,
    radius_m: f64,
}

impl ZonalStreamfunction {
    fn new(band_speed: &[f64], radius_m: f64) -> Self {
        let bands = band_speed.len();
        let half_pi = std::f64::consts::FRAC_PI_2;
        let mut nodes = Vec::with_capacity(bands + 2);
        let mut speed = Vec::with_capacity(bands + 2);
        nodes.push(-half_pi);
        speed.push(band_speed[0]);
        for (band, &u) in band_speed.iter().enumerate() {
            nodes.push(-half_pi + (band as f64 + 0.5) * std::f64::consts::PI / bands as f64);
            speed.push(u);
        }
        nodes.push(half_pi);
        speed.push(band_speed[bands - 1]);
        let mut psi = vec![0.0; nodes.len()];
        for k in 1..nodes.len() {
            let width = nodes[k] - nodes[k - 1];
            psi[k] = psi[k - 1] - radius_m * 0.5 * (speed[k] + speed[k - 1]) * width;
        }
        Self {
            nodes,
            speed,
            psi,
            radius_m,
        }
    }

    fn at(&self, latitude: f64) -> f64 {
        let k = self
            .nodes
            .partition_point(|&node| node <= latitude)
            .clamp(1, self.nodes.len() - 1)
            - 1;
        let width = self.nodes[k + 1] - self.nodes[k];
        let offset = (latitude - self.nodes[k]).clamp(0.0, width);
        let slope = (self.speed[k + 1] - self.speed[k]) / width;
        self.psi[k] - self.radius_m * (self.speed[k] * offset + 0.5 * slope * offset * offset)
    }
}

/// Land evaporation regime of one cell in the vapour active set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LandEvaporation {
    /// Steady bucket recycling, `E = beta P`.
    Recycling,
    /// Energy/atmosphere limited, `E = E_pot(W)` of a wet land surface.
    Potential,
    /// The air is at least saturated over land, `E = 0`.
    Zero,
}

/// Expected speed `E|V|` of a wind whose components are isotropic Gaussian
/// with standard deviation `sigma` about a mean vector of magnitude `nu`: the
/// Rice mean `sigma sqrt(pi/2) L_1/2(-nu^2 / 2 sigma^2)` (Rice 1945), written
/// with exponentially scaled modified Bessel functions.
fn rice_mean_speed(nu: f64, sigma: f64) -> f64 {
    if sigma <= 0.0 {
        return nu;
    }
    let x = nu * nu / (4.0 * sigma * sigma);
    sigma
        * (std::f64::consts::PI / 2.0).sqrt()
        * ((1.0 + 2.0 * x) * scaled_bessel_i0(x) + 2.0 * x * scaled_bessel_i1(x))
}

/// `exp(-x) I0(x)` for `x >= 0`, Abramowitz & Stegun (1964) 9.8.1 / 9.8.2
/// (relative error below 2e-7).
fn scaled_bessel_i0(x: f64) -> f64 {
    if x < 3.75 {
        let t = (x / 3.75).powi(2);
        let i0 = 1.0
            + t * (3.515_622_9
                + t * (3.089_942_4
                    + t * (1.206_749_2 + t * (0.265_973_2 + t * (0.036_076_8 + t * 0.004_581_3)))));
        i0 * (-x).exp()
    } else {
        let t = 3.75 / x;
        (0.398_942_28
            + t * (0.013_285_92
                + t * (0.002_253_19
                    + t * (-0.001_575_65
                        + t * (0.009_162_81
                            + t * (-0.020_577_06
                                + t * (0.026_355_37 + t * (-0.016_476_33 + t * 0.003_923_77))))))))
            / x.sqrt()
    }
}

/// `exp(-x) I1(x)` for `x >= 0`, Abramowitz & Stegun (1964) 9.8.3 / 9.8.4.
fn scaled_bessel_i1(x: f64) -> f64 {
    if x < 3.75 {
        let t = (x / 3.75).powi(2);
        let i1 = x
            * (0.5
                + t * (0.878_905_94
                    + t * (0.514_988_69
                        + t * (0.150_849_34
                            + t * (0.026_587_33 + t * (0.003_015_32 + t * 0.000_324_11))))));
        i1 * (-x).exp()
    } else {
        let t = 3.75 / x;
        (0.398_942_28
            + t * (-0.039_880_24
                + t * (-0.003_620_18
                    + t * (0.001_638_01
                        + t * (-0.010_315_55
                            + t * (0.022_829_67
                                + t * (-0.028_953_12 + t * (0.017_876_54 - t * 0.004_200_59))))))))
            / x.sqrt()
    }
}

/// Observed sub-monthly wind component spread at a latitude, linearly
/// interpolated between the 5-degree band centres.
fn submonthly_wind_sigma_m_s(latitude_rad: f64) -> f64 {
    let bands = SUBMONTHLY_WIND_COMPONENT_SIGMA_M_S;
    let position = (latitude_rad.abs().to_degrees() - 2.5) / 5.0;
    if position <= 0.0 {
        return bands[0];
    }
    let lower = (position.floor() as usize).min(bands.len() - 1);
    let upper = (lower + 1).min(bands.len() - 1);
    let fraction = (position - lower as f64).min(1.0);
    bands[lower] + (bands[upper] - bands[lower]) * fraction
}

/// Temperature-dependent ice/snow albedo feedback (probe comparison axis).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum IceAlbedoFeedback {
    /// Ice-free surface albedo, as production P4 (A4 section 4.4).
    None,
    /// Budyko edge with the production sea-ice and full-snow albedos, keeping
    /// the production CERES atmospheric reflectance (the A4 double count).
    ProductionReflectance,
    /// Same ice, with the production reflectance derivation re-evaluated on a
    /// surface-albedo mean that includes this world's ice increment.
    RecalibratedReflectance,
    /// Same ice, with the atmospheric reflectance solved so the world's
    /// insolation-weighted planetary albedo (global ASR) equals its ice-free
    /// value: the CERES calibration already contains Earth's ice, so explicit
    /// ice may redistribute but not add global reflection.
    PlanetaryAlbedoPreserving,
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
    /// Latitudes (radians) of the two edge vertices, ordered so that the
    /// first lies behind and the second ahead of `midpoint x normal`.
    pub(crate) vertex_latitude_rad: [f64; 2],
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
        let vertices = grid.vertices();
        let edges: Vec<ReducedEdge> = grid
            .edges()
            .iter()
            .map(|edge| {
                let midpoint = edge.midpoint_unit();
                let x = midpoint[2];
                let along = cross(midpoint, edge.normal_from_first());
                let [first, second] = edge.vertices().map(|v| vertices[v as usize]);
                let latitude = |v: [f64; 3]| (v[2] / norm(v)).clamp(-1.0, 1.0).asin();
                let ahead = dot(
                    [
                        second[0] - first[0],
                        second[1] - first[1],
                        second[2] - first[2],
                    ],
                    along,
                ) > 0.0;
                let vertex_latitude_rad = if ahead {
                    [latitude(first), latitude(second)]
                } else {
                    [latitude(second), latitude(first)]
                };
                ReducedEdge {
                    vertex_latitude_rad,
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
    pub(crate) monthly_insolation_fraction: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    /// Fully ice/snow-covered cell albedo: production full-snow land prior and
    /// production sea-ice albedo, weighted by land fraction.
    pub(crate) ice_covered_albedo: Vec<f64>,
    pub(crate) ice: IceAlbedoFeedback,
    pub(crate) physics: ReducedPhysics,
    /// Ice-free planetary-albedo latitude increment per cell (zero unless
    /// `physics.latitude_albedo`).
    pub(crate) latitude_albedo_increment: Vec<f64>,
    /// Ice-covered share of the ocean part of each cell (sea-ice surface type).
    pub(crate) sea_ice: Vec<f64>,
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
        ice: IceAlbedoFeedback,
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
        let monthly_insolation_fraction = forcing
            .monthly_insolation_fraction()
            .iter()
            .map(|m| m.map(f64::from))
            .collect();
        let full_snow_land_albedo = P4_OPEN_OCEAN_SURFACE_ALBEDO
            + P4_SNOW_FREE_LAND_SURFACE_ALBEDO_INCREMENT
            + P4_HIGHLAND_SURFACE_ALBEDO_INCREMENT;
        let ice_covered_albedo = land_fraction
            .iter()
            .map(|land| land * full_snow_land_albedo + (1.0 - land) * P4_SEA_ICE_SURFACE_ALBEDO)
            .collect();
        Self {
            land_fraction,
            reference_height_m,
            reference_height_gradient,
            surface_albedo,
            monthly_absorbed_shortwave_w_m2,
            monthly_insolation_fraction,
            ice_covered_albedo,
            ice,
            physics: ReducedPhysics::default(),
            latitude_albedo_increment: vec![0.0; grid.cell_count()],
            sea_ice: vec![0.0; grid.cell_count()],
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

    /// Enables the R2 physics.
    pub(crate) fn with_physics(mut self, grid: &ReducedGrid, physics: ReducedPhysics) -> Self {
        self.physics = physics;
        self.latitude_albedo_increment = (0..grid.cell_count())
            .map(|cell| {
                if !physics.latitude_albedo {
                    return 0.0;
                }
                let x = grid.center[cell][2];
                let legendre = 0.5 * (3.0 * x * x - 1.0);
                let land = self.land_fraction[cell];
                (TRANSEBM_OCEAN_LATITUDE_ALBEDO * (1.0 - land)
                    + TRANSEBM_LAND_LATITUDE_ALBEDO * land)
                    * legendre
            })
            .collect();
        self
    }

    /// Open-water share of a cell (ocean part not covered by sea ice).
    fn open_water_fraction(&self, cell: usize) -> f64 {
        (1.0 - self.land_fraction[cell]) * (1.0 - self.sea_ice[cell])
    }
}

/// Work-domain monthly result of one reduced evaluation.
#[derive(Debug, Clone)]
pub(crate) struct ReducedWorkClimate {
    /// Final surface albedo and ASR (after the ice feedback, if enabled).
    pub(crate) surface_albedo: Vec<f64>,
    pub(crate) monthly_absorbed_shortwave_w_m2: Vec<[f64; CLIMATE_MONTH_COUNT]>,
    pub(crate) ice_fraction: Vec<f64>,
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
    /// Annual vapour convergence (eddy, Hadley, zonal), mm/day.
    pub(crate) vapour_convergence_mm_day: Vec<[f64; 3]>,
    pub(crate) diagnostics: ReducedDiagnostics,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub(crate) struct ReducedDiagnostics {
    pub(crate) ice_feedback: Option<IceAlbedoFeedback>,
    pub(crate) ice_iterations: usize,
    pub(crate) ice_mask_converged: bool,
    pub(crate) ice_area_fraction: f64,
    pub(crate) sea_ice_ocean_area_fraction: f64,
    pub(crate) atmospheric_shortwave_reflectance: f64,
    pub(crate) mebm_picard_iterations: usize,
    pub(crate) mebm_last_picard_change_k: f64,
    pub(crate) energy_solve_seconds: f64,
    pub(crate) monthly_solve_seconds: f64,
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
    let mut effective = inputs.clone();
    let energy_started = std::time::Instant::now();
    let (energy, ice_fraction) =
        solve_energy_with_ice_feedback(grid, &mut effective, parallel, &mut diagnostics);
    diagnostics.energy_solve_seconds = energy_started.elapsed().as_secs_f64();
    let months_started = std::time::Instant::now();
    let inputs = &effective;
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
        let hadley = hadley_lower_branch(
            grid,
            &mse,
            column_mass,
            inputs.physics.hadley_meridional_only,
        );
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

    diagnostics.monthly_solve_seconds = months_started.elapsed().as_secs_f64();
    let n = grid.cell_count();
    let mut out = ReducedWorkClimate {
        surface_albedo: inputs.surface_albedo.clone(),
        monthly_absorbed_shortwave_w_m2: inputs.monthly_absorbed_shortwave_w_m2.clone(),
        ice_fraction,
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
        vapour_convergence_mm_day: vec![[0.0; 3]; n],
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
            for (sum, value) in out.vapour_convergence_mm_day[cell]
                .iter_mut()
                .zip(water.convergence[cell])
            {
                *sum += value / area * 86_400.0 / CLIMATE_MONTH_COUNT as f64;
            }
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

/// Iterates the MEBM with the Budyko ice/snow edge until the ice mask is
/// stable. A cell's land part is ice/snow covered when its annual air
/// temperature at the reference height is below the edge temperature. Its
/// water part uses the same edge on the annual sea-level temperature, or,
/// with the R2 sea-ice surface type, the seawater freezing point.
fn solve_energy_with_ice_feedback(
    grid: &ReducedGrid,
    inputs: &mut ReducedInputs,
    parallel: bool,
    diagnostics: &mut ReducedDiagnostics,
) -> (Vec<[f64; CLIMATE_MONTH_COUNT]>, Vec<f64>) {
    let n = grid.cell_count();
    let ice_free_albedo = inputs.surface_albedo.clone();
    let total_area: f64 = grid.area_m2.iter().sum();
    let area_mean = |values: &[f64]| {
        values
            .iter()
            .zip(&grid.area_m2)
            .map(|(a, w)| a * w)
            .sum::<f64>()
            / total_area
    };
    let ice_free_mean = area_mean(&ice_free_albedo);
    // Insolation-weighted annual sums for the planetary-albedo-preserving case.
    let insolation_area: Vec<f64> = (0..n)
        .map(|c| grid.area_m2[c] * inputs.monthly_insolation_fraction[c].iter().sum::<f64>())
        .collect();
    let ice_free_absorbed: f64 = (1.0 - EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE)
        * (0..n)
            .map(|c| insolation_area[c] * (1.0 - ice_free_albedo[c].clamp(0.0, 1.0)))
            .sum::<f64>();
    diagnostics.ice_feedback = Some(inputs.ice);
    diagnostics.atmospheric_shortwave_reflectance = EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE;
    let without_ice = inputs.ice == IceAlbedoFeedback::None;
    if without_ice && !inputs.physics.latitude_albedo {
        diagnostics.ice_mask_converged = true;
        let energy = solve_moist_energy_balance(
            grid,
            inputs,
            parallel,
            None,
            EnergySolve::Seasonal,
            diagnostics,
        );
        return (energy, vec![0.0; n]);
    }
    let mut ice_fraction = vec![0.0_f64; n];
    let mut sea_ice = vec![0.0_f64; n];
    // The ice mask reads only the annual mean, so the mask iteration solves
    // the k = 0 problem alone (warm-started) and the seasonal harmonics run
    // once with the converged albedo and heat capacity.
    let mut annual: Option<Vec<f64>> = None;
    diagnostics.ice_mask_converged = false;
    for iteration in 0..ICE_ALBEDO_MAX_ITERATIONS {
        if let (false, Some(theta_annual)) = (without_ice, annual.as_ref()) {
            let mut next_ice = vec![0.0_f64; n];
            let mut next_sea = vec![0.0_f64; n];
            for cell in 0..n {
                let theta = theta_annual[cell];
                let air =
                    theta - CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M * inputs.reference_height_m[cell];
                let land = inputs.land_fraction[cell];
                let below = |t: f64, edge: f64| f64::from(u8::from(t < edge));
                let sea = if inputs.physics.sea_ice_surface_type {
                    below(theta, LIQUID_MIXED_LAYER_MIN_C)
                } else {
                    below(theta, BUDYKO_ICE_EDGE_ANNUAL_TEMPERATURE_C)
                };
                next_ice[cell] =
                    land * below(air, BUDYKO_ICE_EDGE_ANNUAL_TEMPERATURE_C) + (1.0 - land) * sea;
                next_sea[cell] = sea;
            }
            if next_ice == ice_fraction
                && next_sea == sea_ice
                && diagnostics.mebm_last_picard_change_k < MEBM_PICARD_TOLERANCE_K
            {
                diagnostics.ice_mask_converged = true;
                break;
            }
            ice_fraction = next_ice;
            sea_ice = next_sea;
        }
        diagnostics.ice_iterations = iteration + 1;
        let albedo: Vec<f64> = (0..n)
            .map(|c| {
                ice_free_albedo[c]
                    + ice_fraction[c] * (inputs.ice_covered_albedo[c] - ice_free_albedo[c])
            })
            .collect();
        // Ice-free planetary-albedo latitude increment (TransEBM P2 term);
        // ice and snow keep their own albedo without a latitude term.
        let latitude: Vec<f64> = (0..n)
            .map(|c| inputs.latitude_albedo_increment[c] * (1.0 - ice_fraction[c]))
            .collect();
        let reflectance = match inputs.ice {
            IceAlbedoFeedback::RecalibratedReflectance => {
                // Production derivation r = (A_p - a_s) / (1 - a_s), with the
                // corpus surface mean shifted by this world's ice increment.
                let surface = EARTH_CALIBRATION_SURFACE_ALBEDO_GLOBAL_MEAN
                    + (area_mean(&albedo) - ice_free_mean);
                (EARTH_CERES_PLANETARY_ALBEDO_GLOBAL_MEAN - surface) / (1.0 - surface)
            }
            IceAlbedoFeedback::PlanetaryAlbedoPreserving => {
                // (1 - r') sum(I (1 - a)) - sum(I d) = (1 - r) sum(I (1 - a_free)).
                let absorbed: f64 = (0..n)
                    .map(|c| insolation_area[c] * (1.0 - albedo[c].clamp(0.0, 1.0)))
                    .sum();
                let brightening: f64 = (0..n).map(|c| insolation_area[c] * latitude[c]).sum();
                1.0 - (ice_free_absorbed + brightening) / absorbed
            }
            IceAlbedoFeedback::None | IceAlbedoFeedback::ProductionReflectance => {
                EARTH_ATMOSPHERIC_SHORTWAVE_REFLECTANCE
            }
        };
        diagnostics.atmospheric_shortwave_reflectance = reflectance;
        for (cell, (absorbed, insolation)) in inputs
            .monthly_absorbed_shortwave_w_m2
            .iter_mut()
            .zip(&inputs.monthly_insolation_fraction)
            .enumerate()
        {
            let planetary =
                (reflectance + (1.0 - reflectance) * albedo[cell].clamp(0.0, 1.0) + latitude[cell])
                    .clamp(0.0, 1.0);
            *absorbed = insolation.map(|f| {
                EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2 * f.max(0.0) * (1.0 - planetary)
            });
        }
        inputs.surface_albedo = albedo;
        if inputs.physics.sea_ice_surface_type {
            inputs.sea_ice.clone_from(&sea_ice);
        }
        // One Picard step per mask update: the conductance and the ice mask
        // converge together instead of nesting a full Picard solve per mask.
        let energy = solve_moist_energy_balance(
            grid,
            inputs,
            parallel,
            annual.as_deref(),
            EnergySolve::AnnualSteps(1),
            diagnostics,
        );
        annual = Some(energy.iter().map(|m| m[0]).collect());
        if without_ice && diagnostics.mebm_last_picard_change_k < MEBM_PICARD_TOLERANCE_K {
            diagnostics.ice_mask_converged = true;
            break;
        }
    }
    let energy = solve_moist_energy_balance(
        grid,
        inputs,
        parallel,
        annual.as_deref(),
        EnergySolve::Seasonal,
        diagnostics,
    );
    diagnostics.ice_area_fraction = area_mean(&ice_fraction);
    diagnostics.sea_ice_ocean_area_fraction = (0..n)
        .map(|c| grid.area_m2[c] * (1.0 - inputs.land_fraction[c]) * sea_ice[c])
        .sum::<f64>()
        / (0..n)
            .map(|c| grid.area_m2[c] * (1.0 - inputs.land_fraction[c]))
            .sum::<f64>()
            .max(f64::MIN_POSITIVE);
    (energy, ice_fraction)
}

/// Work requested from one MEBM evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnergySolve {
    /// At most this many Picard steps of the annual (k = 0) problem; the
    /// result repeats the annual mean in every month.
    AnnualSteps(usize),
    /// Annual problem to the Picard tolerance, then the seasonal harmonics.
    Seasonal,
}

/// E1: periodic seasonal MEBM month means of sea-level-reduced temperature.
fn solve_moist_energy_balance(
    grid: &ReducedGrid,
    inputs: &ReducedInputs,
    parallel: bool,
    initial_annual: Option<&[f64]>,
    work: EnergySolve,
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
    // Sea-ice surface type: the ice insulates the mixed layer, so the ocean
    // part stores heat with the sea-ice capacity instead (TransEBM Table 1).
    let capacity: Vec<f64> = (0..n)
        .map(|c| {
            let ice = inputs.sea_ice[c];
            air_capacity
                + (1.0 - inputs.land_fraction[c])
                    * ((1.0 - ice) * mixed_capacity + ice * TRANSEBM_SEA_ICE_HEAT_CAPACITY_J_M2_K)
        })
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
    let mut annual = initial_annual.map_or_else(|| theta_eq.clone(), <[f64]>::to_vec);
    let mut conductance = vec![0.0_f64; grid.edges.len()];
    let mut iterations = 0;
    let picard_limit = match work {
        EnergySolve::AnnualSteps(steps) => steps,
        EnergySolve::Seasonal => MEBM_PICARD_MAX_ITERATIONS,
    };
    for picard in 0..picard_limit {
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
        diagnostics.mebm_last_picard_change_k = change;
        if change < MEBM_PICARD_TOLERANCE_K {
            break;
        }
    }
    diagnostics.mebm_picard_iterations += iterations;
    if work != EnergySolve::Seasonal {
        return annual.iter().map(|&t| [t; CLIMATE_MONTH_COUNT]).collect();
    }

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
fn hadley_lower_branch(
    grid: &ReducedGrid,
    mse: &[f64],
    column_mass: f64,
    meridional_only: bool,
) -> HadleyBranch {
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
            let projection = if meridional_only {
                let north = cross(edge.midpoint, east_unit(edge.midpoint));
                dot(edge.normal, north).powi(2)
            } else {
                1.0
            };
            // Lower branch flows against the energy flux.
            -edge.hadley_weight * projection * flux / gross_moist_stability
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
    /// Annual-budget diagnostic: vapour convergence by eddy diffusion, the
    /// Hadley lower branch and zonal advection, kg/s per cell.
    convergence: Vec<[f64; 3]>,
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
    let evaporation_speed: Vec<f64> = speed
        .iter()
        .zip(&grid.latitude_rad)
        .map(|(&s, &latitude)| {
            if inputs.physics.evaporation_wind_variability {
                rice_mean_speed(s, submonthly_wind_sigma_m_s(latitude))
            } else if inputs.physics.evaporation_gustiness {
                s.hypot(SPEEDY_SUBGRID_GUST_WIND_M_S)
            } else {
                s
            }
        })
        .collect();
    // Eddy vapour diffusivity on the column water W = M_q q: the MEBM latent
    // eddy flux (p_s/g) D grad q equals (p_s/g)/M_q D grad W.
    let eddy_vapour_scale = if inputs.physics.column_consistent_eddy_moisture {
        P4_LOWER_LAYER_REFERENCE_PRESSURE_PA / STANDARD_GRAVITY_M_S2 / mq
    } else {
        1.0
    };
    let sst: Vec<f64> = month
        .theta
        .iter()
        .map(|t| t.max(LIQUID_MIXED_LAYER_MIN_C))
        .collect();
    let evaporation_scale = P4_REFERENCE_AIR_DENSITY_KG_M3 * BULK_MOISTURE_TRANSFER_COEFFICIENT;
    // Linear bulk evaporation E = a - b W from the production formula.
    let a: Vec<f64> = (0..n)
        .map(|c| {
            bulk_surface_evaporation_kg_m2_s(
                sst[c],
                0.0,
                evaporation_speed[c],
                inputs.open_water_fraction(c),
            )
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

    // Zonal advecting wind: local near-surface wind, or its band mean.
    let band_zonal: Vec<f64> = {
        let mut sum = vec![0.0_f64; grid.band_count];
        let mut weight = vec![0.0_f64; grid.band_count];
        for cell in 0..n {
            sum[grid.cell_band[cell]] += grid.area_m2[cell] * month.zonal[cell];
            weight[grid.cell_band[cell]] += grid.area_m2[cell];
        }
        sum.iter()
            .zip(&weight)
            .map(|(s, w)| s / w.max(f64::MIN_POSITIVE))
            .collect()
    };
    let zonal_streamfunction = inputs
        .physics
        .nondivergent_zonal_vapour_advection
        .then(|| ZonalStreamfunction::new(&band_zonal, grid.radius_m));
    // Per-edge transport coefficients, kept for the convergence diagnostic.
    let mut edge_transport = Vec::with_capacity(grid.edges.len());
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
        let diffusion =
            (1.0 - edge.hadley_weight) * MEBM_DIFFUSIVITY_M2_S * eddy_vapour_scale * edge.length_m
                / edge.center_distance_m;
        transport_diag[i] += diffusion;
        transport_diag[j] += diffusion;
        transport_off[i][si] -= diffusion;
        transport_off[j][sj] -= diffusion;
        // Hadley lower branch carries near-surface q = W / M_q upwind.
        let hadley = month.hadley_mass_flux[edge_id] / mq;
        // Zonal-mean eddy-driven wind advects the column, first-order upwind.
        let mid_east = east_unit(edge.midpoint);
        let zonal_mid = if inputs.physics.band_mean_zonal_vapour_advection {
            band_zonal[grid.edge_band[edge_id]]
        } else {
            0.5 * (month.zonal[i] + month.zonal[j])
        };
        let zonal_flux = match &zonal_streamfunction {
            Some(psi) => psi.at(edge.vertex_latitude_rad[0]) - psi.at(edge.vertex_latitude_rad[1]),
            None => zonal_mid * dot(mid_east, edge.normal) * edge.length_m,
        };
        let advective = zonal_flux + hadley;
        edge_transport.push((diffusion, hadley, advective));
        if advective > 0.0 {
            transport_diag[i] += advective;
            transport_off[j][sj] -= advective;
        } else {
            transport_diag[j] -= advective;
            transport_off[i][si] += advective;
        }
    }

    // Wet-land potential evaporation E_pot = a_land - b_land W: the
    // production bulk transfer law over the land share with fresh-water
    // saturation at the reference-height air temperature (Manabe 1969).
    let land_transfer: Vec<f64> = (0..n)
        .map(|c| {
            if inputs.physics.land_potential_evaporation_cap {
                evaporation_scale * evaporation_speed[c] * inputs.land_fraction[c]
            } else {
                0.0
            }
        })
        .collect();
    let a_land: Vec<f64> = (0..n)
        .map(|c| land_transfer[c] * saturation_specific_humidity_kg_kg(month.air[c]))
        .collect();
    let b_land: Vec<f64> = land_transfer.iter().map(|t| t / mq).collect();
    let mut water: Vec<f64> = critical.clone();
    let mut large_scale_active = vec![true; n];
    let mut evaporation_active = vec![true; n];
    let mut land_regime = vec![LandEvaporation::Recycling; n];
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
            let ocean_sink = if evaporation_active[c] { b[c] } else { 0.0 };
            let ocean_source = if evaporation_active[c] { a[c] } else { 0.0 };
            let (sink, source) = match land_regime[c] {
                LandEvaporation::Recycling => (
                    (1.0 - beta) * (ls + orographic_rate[c]) + ocean_sink,
                    (1.0 - beta) * ls * critical[c] + ocean_source,
                ),
                LandEvaporation::Potential => (
                    ls + orographic_rate[c] + b_land[c] + ocean_sink,
                    ls * critical[c] + a_land[c] + ocean_source,
                ),
                LandEvaporation::Zero => (
                    ls + orographic_rate[c] + ocean_sink,
                    ls * critical[c] + ocean_source,
                ),
            };
            diag[c] = transport_diag[c] + area * sink;
            rhs[c] = area * source;
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
            if inputs.physics.land_potential_evaporation_cap {
                let precipitation = if ls {
                    relaxation * (water[c] - critical[c])
                } else {
                    0.0
                } + orographic_rate[c] * water[c];
                let potential = a_land[c] - b_land[c] * water[c];
                let recycled = inputs.land_evapotranspiration_fraction[c] * precipitation;
                let regime = if potential <= 0.0 && land_transfer[c] > 0.0 {
                    LandEvaporation::Zero
                } else if land_transfer[c] > 0.0 && recycled > potential {
                    LandEvaporation::Potential
                } else {
                    LandEvaporation::Recycling
                };
                changed |= regime != land_regime[c];
                land_regime[c] = regime;
            }
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
        let land = match land_regime[c] {
            LandEvaporation::Recycling => {
                inputs.land_evapotranspiration_fraction[c] * precipitation[c]
            }
            LandEvaporation::Potential => a_land[c] - b_land[c] * water[c],
            LandEvaporation::Zero => 0.0,
        };
        evaporation[c] = ocean + land;
    }
    let mut convergence = vec![[0.0_f64; 3]; n];
    for (edge, &(diffusion, hadley, advective)) in grid.edges.iter().zip(&edge_transport) {
        let [i, j] = edge.cells.map(|c| c as usize);
        let upwind = if advective > 0.0 { water[i] } else { water[j] };
        let fluxes = [
            diffusion * (water[i] - water[j]),
            hadley * upwind,
            (advective - hadley) * upwind,
        ];
        for (component, flux) in fluxes.into_iter().enumerate() {
            convergence[i][component] -= flux;
            convergence[j][component] += flux;
        }
    }
    WaterSolution {
        convergence,
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

#[cfg(test)]
mod tests {
    use super::{rice_mean_speed, submonthly_wind_sigma_m_s, ZonalStreamfunction};

    /// Rice mean limits: Rayleigh mean `sigma sqrt(pi/2)` at zero mean wind
    /// and `nu + sigma^2 / (2 nu)` for a strong mean wind.
    #[test]
    fn rice_mean_speed_matches_its_limits() {
        let sigma = 3.0;
        let rayleigh = sigma * (std::f64::consts::PI / 2.0).sqrt();
        assert!((rice_mean_speed(0.0, sigma) - rayleigh).abs() < 1.0e-6);
        let nu = 40.0;
        let asymptote = nu + sigma * sigma / (2.0 * nu);
        assert!((rice_mean_speed(nu, sigma) - asymptote).abs() / asymptote < 1.0e-4);
        // Continuity across the 3.75 branch of the Bessel approximations.
        let below = rice_mean_speed((4.0_f64 * 3.749_999).sqrt() * sigma, sigma);
        let above = rice_mean_speed((4.0_f64 * 3.750_001).sqrt() * sigma, sigma);
        assert!((below - above).abs() < 1.0e-5);
    }

    /// psi is the exact integral of the piecewise-linear profile: a uniform
    /// flow u gives psi(phi) = -a u (phi + pi/2).
    #[test]
    fn zonal_streamfunction_integrates_the_band_profile() {
        let radius = 6.371e6;
        let uniform = ZonalStreamfunction::new(&[5.0; 18], radius);
        let phi = 0.3_f64;
        let expected = -radius * 5.0 * (phi + std::f64::consts::FRAC_PI_2);
        assert!((uniform.at(phi) - expected).abs() / expected.abs() < 1.0e-12);
        // d psi / d phi = -a u at a band centre of a varying profile.
        let profile: Vec<f64> = (0..18).map(|b| (b as f64 - 8.5).sin() * 7.0).collect();
        let psi = ZonalStreamfunction::new(&profile, radius);
        let centre = -std::f64::consts::FRAC_PI_2 + 4.5 * std::f64::consts::PI / 18.0;
        let h = 1.0e-6;
        let derivative = (psi.at(centre + h) - psi.at(centre - h)) / (2.0 * h);
        assert!((derivative + radius * profile[4]).abs() < 1.0e-3 * radius);
    }

    #[test]
    fn submonthly_sigma_interpolates_band_centres() {
        assert!((submonthly_wind_sigma_m_s(0.0) - 1.78).abs() < 1.0e-12);
        let mid = submonthly_wind_sigma_m_s(5.0_f64.to_radians());
        assert!((mid - 0.5 * (1.78 + 1.92)).abs() < 1.0e-12);
        assert!((submonthly_wind_sigma_m_s(-90.0_f64.to_radians()) - 3.40).abs() < 1.0e-12);
    }
}
