mod viscosity;

use thiserror::Error;

use super::state::{
    axisymmetric_band_count, axisymmetric_bands, role_reference_temperature_c,
    upper_atmosphere_reference_air_temperature_c, LayeredClimateState, LayeredStateError,
    DEEP_OCEAN_EQUILIBRIUM_OFFSET_C, LIQUID_MIXED_LAYER_MIN_C, OCEAN_EQUILIBRIUM_MAX_C,
    SUBSURFACE_OCEAN_MIN_C, THERMOCLINE_EQUILIBRIUM_OFFSET_C,
    UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C, UPPER_SPECIFIC_HUMIDITY_INITIAL_FRACTION,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{
    donor_layer_edge_amount_rate_m3_s, CirculationOperatorError, CirculationOperators,
    CubedSphereGrid, LayerTransportFields, SecondOrderTransportWorkspace,
};
use crate::world::natural::{
    bulk_surface_evaporation_kg_m2_s, gray_longwave_slope_w_m2_k, large_scale_condensation_kg_m2_s,
    lcl_adjusted_orographic_condensation_kg_m2_s, linearized_outgoing_longwave_w_m2,
    p4_momentum_constants_fingerprint, p4_thermodynamic_constants_fingerprint, ClimateLayerLayout,
    ClimateLayerRole, ClimateLayerSpec, ClimateModelProfile, ForcingError, PlanetForcing,
    ATMOSPHERE_COLUMN_DEPTH_M, BAROCLINIC_REYNOLDS_STRESS_EFFICIENCY,
    BOUNDARY_LAYER_CONVECTIVE_VENTING_SECONDS, BOUNDARY_LAYER_DRY_VENTING_SECONDS,
    CLIMATE_MONTH_COUNT, EARTH_ROTATION_RATE_RAD_S, GLOBAL_CIRCULATION_FAST_CFL_TARGET,
    GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION, GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
    GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S, LOWER_ATMOSPHERE_REFERENCE_THICKNESS_M,
    STANDARD_GRAVITY_M_S2, UPPER_ATMOSPHERE_REFERENCE_THICKNESS_M,
    WATER_VAPORIZATION_LATENT_HEAT_J_KG,
};

const SEAWATER_THERMAL_EXPANSION_K_INV: f64 = 2.0e-4;
const MIXED_LAYER_REFERENCE_THICKNESS_M: f64 = 100.0;
const MIXED_LAYER_STERIC_ACCELERATION_M2_S2_K: f64 = 0.5
    * STANDARD_GRAVITY_M_S2
    * SEAWATER_THERMAL_EXPANSION_K_INV
    * MIXED_LAYER_REFERENCE_THICKNESS_M;
const SECONDS_PER_DAY: f64 = 86_400.0;
// Partial coastal cells represent unresolved shelf, island, and bottom form
// drag. The term belongs to the shared momentum equation so every candidate
// integrator sees identical physics; it is never applied as a post-step mask.
const COASTAL_FORM_DRAG_TIMESCALE_S: f64 = SECONDS_PER_DAY;
// The lower-atmosphere Rayleigh rate r = C_D |U| / H_bl scales with the bulk
// surface drag coefficient: about 1.2e-3 over the open sea against 3e-3
// (grassland) to 1e-2 (forest) over land (Garratt 1992, The Atmospheric
// Boundary Layer, §4.1; Stull 1988, §7). The land value is pinned at the
// conservative grassland ratio; the one-day sea rate is the existing constant
// in `role_constants`, and partial cells interpolate by land fraction.
const LAND_SEA_SURFACE_DRAG_RATIO: f64 = 3.0;
const BATHYMETRIC_BOTTOM_DRAG_TIMESCALE_S: f64 = 90.0 * SECONDS_PER_DAY;
const BATHYMETRIC_BOTTOM_DRAG_REFERENCE_DEPTH_M: f64 = 1_000.0;
// Horizontal sub-grid mixing closes unresolved baroclinic eddies and prevents
// grid-scale velocity fronts. The finite-volume conductance below makes these
// resolution-independent physical diffusivities rather than per-cell filters.
const ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S: f64 = 1_000_000.0;
// Transient baroclinic eddies carry most of the extratropical poleward
// moisture flux and none of them fit on a 24-48 cell cubed face, so the
// resolved mean flow has to be completed by a down-gradient closure. Held
// (1999), Tellus A 51, 59-70, DOI 10.3402/tellusa.v51i1.12305, puts the
// tropospheric macroturbulent tracer diffusivity at O(1e6 m^2/s); moist energy
// balance models reproduce the observed meridional latent transport with a
// spatially uniform diffusivity of that magnitude (North 1975, JAS 32,
// 2033-2043; Flannery 1984, JAS 41, 414-421; Siler, Roe & Armour 2018,
// J. Climate 31, 7481-7493, DOI 10.1175/JCLI-D-18-0081.1). The finite-volume
// conductance below makes it a resolution-independent physical diffusivity,
// exactly as for the momentum viscosity above.
const ATMOSPHERE_HORIZONTAL_EDDY_MOISTURE_DIFFUSIVITY_M2_S: f64 = 1_000_000.0;
const OCEAN_HORIZONTAL_EDDY_VISCOSITY_M2_S: f64 = 1_000.0;
// The lower layer carries the terrain as a floor under its fixed top (shallow
// water over topography, Vallis 2017 §3.1). The floor is capped so the layer
// never thins below one sixth of its reference depth: Earth's highest plateau
// leaves about 1 km under a 6 km layer, and this bound is a numerical
// safeguard, not a physical parameter.
const LOWER_ATMOSPHERE_MIN_THICKNESS_M: f64 = LOWER_ATMOSPHERE_REFERENCE_THICKNESS_M / 6.0;
const C1_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K: f64 = 30.0;
// Retained momentum increments are checked against these existing balance
// and physical-flux accuracy contracts in the narrow exchange regressions.
const PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE: f64 = 5.0e-7;
const PAIRED_EXCHANGE_RELATIVE_FLUX_ACCURACY: f64 = 1.0e-3;

pub(super) fn layered_equation_model_fingerprint(profile: ClimateModelProfile) -> [u8; 32] {
    let layout = ClimateLayerLayout::for_profile(profile);
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.global-circulation-equations.v18\0");
    hasher.update(&layout.fingerprint());
    hasher.update(&p4_thermodynamic_constants_fingerprint());
    hasher.update(&p4_momentum_constants_fingerprint());
    for value in [
        EARTH_ROTATION_RATE_RAD_S,
        GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION,
        SEAWATER_THERMAL_EXPANSION_K_INV,
        MIXED_LAYER_REFERENCE_THICKNESS_M,
        COASTAL_FORM_DRAG_TIMESCALE_S,
        BATHYMETRIC_BOTTOM_DRAG_TIMESCALE_S,
        BATHYMETRIC_BOTTOM_DRAG_REFERENCE_DEPTH_M,
        ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S,
        ATMOSPHERE_HORIZONTAL_EDDY_MOISTURE_DIFFUSIVITY_M2_S,
        OCEAN_HORIZONTAL_EDDY_VISCOSITY_M2_S,
        C1_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K,
        LOWER_ATMOSPHERE_REFERENCE_THICKNESS_M,
        LOWER_ATMOSPHERE_MIN_THICKNESS_M,
        LAND_SEA_SURFACE_DRAG_RATIO,
        UPPER_ATMOSPHERE_REFERENCE_THICKNESS_M,
        ATMOSPHERE_COLUMN_DEPTH_M,
        BAROCLINIC_REYNOLDS_STRESS_EFFICIENCY,
        PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE,
        PAIRED_EXCHANGE_RELATIVE_FLUX_ACCURACY,
        f64::from(LIQUID_MIXED_LAYER_MIN_C),
        f64::from(SUBSURFACE_OCEAN_MIN_C),
        f64::from(OCEAN_EQUILIBRIUM_MAX_C),
        f64::from(UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C),
        f64::from(THERMOCLINE_EQUILIBRIUM_OFFSET_C),
        f64::from(DEEP_OCEAN_EQUILIBRIUM_OFFSET_C),
        f64::from(UPPER_SPECIFIC_HUMIDITY_INITIAL_FRACTION),
        GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
        crate::world::natural::GLOBAL_CIRCULATION_MAXIMUM_SLOW_STEP_SECONDS,
        GLOBAL_CIRCULATION_FAST_CFL_TARGET,
        GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S,
        super::generation::FORMATION_RESIDUAL_TARGET,
        super::rk3::FORMATION_TEMPERATURE_SCALE_K,
        super::rk3::FORMATION_ATMOSPHERE_SPEED_SCALE_M_S,
        super::rk3::FORMATION_OCEAN_SPEED_SCALE_M_S,
        super::rk3::FORMATION_SPECIFIC_HUMIDITY_SCALE,
    ] {
        hasher.update(&value.to_bits().to_le_bytes());
    }
    for role in layout
        .layers()
        .iter()
        .filter(|layer| layer.dynamically_active())
        .map(|layer| layer.role())
    {
        let (gravity, drag, height_relax, thermal_gradient) = role_constants(profile, role);
        hasher.update(&[match role {
            ClimateLayerRole::LowerAtmosphere => 1,
            ClimateLayerRole::UpperAtmosphere => 2,
            ClimateLayerRole::OceanMixedLayer => 3,
            ClimateLayerRole::OceanThermocline => 4,
            ClimateLayerRole::DeepOceanReservoir => 5,
        }]);
        for value in [gravity, drag, height_relax, thermal_gradient] {
            hasher.update(&value.to_bits().to_le_bytes());
        }
    }
    for semantic_id in [
        b"finite-volume-positive-permeability-v2".as_slice(),
        b"barth-jespersen-component-local-v2".as_slice(),
        b"split-explicit-dynamic-actual-mass-adjoint-viscosity-cfl-rk3-v4".as_slice(),
        b"candidate-c2-atmosphere-shared-limited-linear-depth-flux-v1".as_slice(),
        b"c2-atmosphere-dual-p1-deviatoric-strain-actual-mass-viscosity-v1".as_slice(),
        b"annual-mean-ape-eady-column-reynolds-stress-zero-torque-v5".as_slice(),
        b"actual-layer-mass-mechanical-stress-and-eddy-torque-v1".as_slice(),
        b"linear-surface-wind-and-adjoint-air-sea-stress-v1".as_slice(),
        b"held-suarez-land-friction-affine-basis-integral-v1".as_slice(),
        b"f64-momentum-source-accumulation-until-state-storage-v1".as_slice(),
        b"compressed-local-conductance-and-solver-clock-heat-budget-v1".as_slice(),
        b"pending-thermal-predictor-for-coupled-local-heat-exchange-v1".as_slice(),
        b"deep-heat-exchange-only-in-thermodynamic-endpoint-v1".as_slice(),
        b"bounded-slow-step-subdivision-before-fast-rk3-v1".as_slice(),
        b"actual-ocean-heat-capacity-and-donor-depth-heat-content-rk-v1".as_slice(),
        b"local-ocean-thickness-source-carries-post-thermal-endpoint-heat-v1".as_slice(),
        b"depth-mean-boussinesq-steric-v1".as_slice(),
        b"resolved-temperature-pressure-gradient-v1".as_slice(),
        b"full-hydrostatic-fixed-top-thermal-pressure-and-wave-bound-v1".as_slice(),
        b"donor-upwind-nonlinear-layer-continuity-v1".as_slice(),
        b"axisymmetric-finite-venting-steady-moisture-budget-v1".as_slice(),
        b"convergent-moisture-excess-condenses-latent-heat-exported-v1".as_slice(),
        b"single-lower-boundary-linearized-gray-longwave-v1".as_slice(),
        b"reference-stratification-anomaly-heat-exchange-v1".as_slice(),
        b"subsurface-temperature-floor-pair-flux-limiter-v1".as_slice(),
        b"bolton-lcl-neutral-surface-rh-large-pond-smith-speedy-coupled-phase-change-v5".as_slice(),
        b"thermodynamic-endpoint-before-fast-thermal-pressure-v1".as_slice(),
        b"lower-upper-condensation-latent-heat-v1".as_slice(),
        b"sensible-plus-vapor-latent-energy-ledger-v1".as_slice(),
        b"signed-external-extensive-ledger-v2".as_slice(),
        b"fieldwise-area-weighted-formation-residual-v2".as_slice(),
    ] {
        hasher.update(&(semantic_id.len() as u32).to_le_bytes());
        hasher.update(semantic_id);
    }
    *hasher.finalize().as_bytes()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairedHeatExchange {
    first_tendency_k_s: f64,
    second_tendency_k_s: f64,
    extensive_flux_w_m2: f64,
    extensive_residual_w_m2: f64,
}

impl PairedHeatExchange {
    pub const fn first_tendency_k_s(self) -> f64 {
        self.first_tendency_k_s
    }

    pub const fn second_tendency_k_s(self) -> f64 {
        self.second_tendency_k_s
    }

    pub const fn extensive_flux_w_m2(self) -> f64 {
        self.extensive_flux_w_m2
    }

    pub const fn extensive_residual_w_m2(self) -> f64 {
        self.extensive_residual_w_m2
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairedMomentumExchange {
    first_acceleration_m_s2: [f64; 3],
    second_acceleration_m_s2: [f64; 3],
    extensive_residual_n_m2: f64,
}

impl PairedMomentumExchange {
    pub const fn first_acceleration_m_s2(self) -> [f64; 3] {
        self.first_acceleration_m_s2
    }

    pub const fn second_acceleration_m_s2(self) -> [f64; 3] {
        self.second_acceleration_m_s2
    }

    pub const fn extensive_residual_n_m2(self) -> f64 {
        self.extensive_residual_n_m2
    }
}

/// Computes one equal-and-opposite heat transfer in extensive units.
pub fn paired_heat_exchange(
    first_temperature_k: f64,
    second_temperature_k: f64,
    first_heat_capacity_j_m2_k: f64,
    second_heat_capacity_j_m2_k: f64,
    exchange_time_s: f64,
) -> Result<PairedHeatExchange, LayeredTendencyError> {
    for (field, value) in [
        ("first_temperature_k", first_temperature_k),
        ("second_temperature_k", second_temperature_k),
        ("first_heat_capacity_j_m2_k", first_heat_capacity_j_m2_k),
        ("second_heat_capacity_j_m2_k", second_heat_capacity_j_m2_k),
        ("exchange_time_s", exchange_time_s),
    ] {
        if !value.is_finite() {
            return Err(LayeredTendencyError::InvalidExchangeValue {
                field,
                found: value,
            });
        }
    }
    if first_heat_capacity_j_m2_k <= 0.0
        || second_heat_capacity_j_m2_k <= 0.0
        || exchange_time_s <= 0.0
    {
        return Err(LayeredTendencyError::NonPositiveExchangeScale);
    }
    let coupling_capacity = first_heat_capacity_j_m2_k.min(second_heat_capacity_j_m2_k);
    let flux = (second_temperature_k - first_temperature_k) * coupling_capacity / exchange_time_s;
    let first = flux / first_heat_capacity_j_m2_k;
    let second = -flux / second_heat_capacity_j_m2_k;
    let residual = first_heat_capacity_j_m2_k * first + second_heat_capacity_j_m2_k * second;
    Ok(PairedHeatExchange {
        first_tendency_k_s: first,
        second_tendency_k_s: second,
        extensive_flux_w_m2: flux,
        extensive_residual_w_m2: residual,
    })
}

/// Exchanges heat between departures from the cell/month equilibrium
/// stratification, rather than erasing that stratification itself.
///
/// The same reference helper used by state initialization includes both the
/// fixed vertical offsets and ocean-temperature bounds. Subtracting those
/// references makes every initialized column—including one at a bound—a
/// zero-flux state while preserving equal-and-opposite extensive heat
/// transfer for anomalies. Applying the raw Celsius difference here would
/// continuously drain the radiative lower boundary merely because the model
/// initialized a physically ordered column.
#[allow(clippy::too_many_arguments)]
fn equilibrium_anomaly_heat_exchange(
    first_temperature_c: f32,
    first_reference_temperature_c: f32,
    second_temperature_c: f32,
    second_reference_temperature_c: f32,
    first_heat_capacity_j_m2_k: f64,
    second_heat_capacity_j_m2_k: f64,
    exchange_time_s: f64,
) -> Result<PairedHeatExchange, LayeredTendencyError> {
    paired_heat_exchange(
        f64::from(first_temperature_c) - f64::from(first_reference_temperature_c),
        f64::from(second_temperature_c) - f64::from(second_reference_temperature_c),
        first_heat_capacity_j_m2_k,
        second_heat_capacity_j_m2_k,
        exchange_time_s,
    )
}

/// Rescales only an internal pair flux when it would cool a subsurface ocean
/// reservoir through the already-declared physical state floor. Because the
/// same factor multiplies both sides, the limiter cannot create or destroy
/// heat; it only suppresses the infeasible portion of the exchange. This is
/// the two-reservoir form of the conservative positivity-preserving flux
/// limiting described by Hu, Adams & Shu (2013), DOI
/// `10.1016/j.jcp.2013.01.024`.
fn subsurface_pair_exchange_scale_for_step(
    roles: [ClimateLayerRole; 2],
    temperatures_c: [f32; 2],
    baseline_tendencies_k_s: [f32; 2],
    pair_tendencies_k_s: [f64; 2],
    step_seconds: f64,
) -> f64 {
    let mut scale = 1.0_f64;
    for side in 0..2 {
        if !matches!(
            roles[side],
            ClimateLayerRole::OceanThermocline | ClimateLayerRole::DeepOceanReservoir
        ) || pair_tendencies_k_s[side] >= 0.0
        {
            continue;
        }
        let baseline_end = f64::from(temperatures_c[side])
            + step_seconds * f64::from(baseline_tendencies_k_s[side]);
        let available_cooling = (baseline_end - f64::from(SUBSURFACE_OCEAN_MIN_C)).max(0.0);
        let requested_cooling = -step_seconds * pair_tendencies_k_s[side];
        scale = scale.min(available_cooling / requested_cooling);
    }
    scale.clamp(0.0, 1.0)
}

/// Computes one equal-and-opposite horizontal momentum transfer.
pub fn paired_momentum_exchange(
    first_velocity_m_s: [f64; 3],
    second_velocity_m_s: [f64; 3],
    first_mass_kg_m2: f64,
    second_mass_kg_m2: f64,
    exchange_time_s: f64,
) -> Result<PairedMomentumExchange, LayeredTendencyError> {
    if first_velocity_m_s
        .iter()
        .chain(second_velocity_m_s.iter())
        .any(|value| !value.is_finite())
    {
        return Err(LayeredTendencyError::InvalidExchangeVector);
    }
    if !first_mass_kg_m2.is_finite()
        || !second_mass_kg_m2.is_finite()
        || !exchange_time_s.is_finite()
    {
        return Err(LayeredTendencyError::InvalidExchangeValue {
            field: "momentum_exchange_scale",
            found: f64::NAN,
        });
    }
    if first_mass_kg_m2 <= 0.0 || second_mass_kg_m2 <= 0.0 || exchange_time_s <= 0.0 {
        return Err(LayeredTendencyError::NonPositiveExchangeScale);
    }
    let coupling_mass = first_mass_kg_m2.min(second_mass_kg_m2);
    let impulse = std::array::from_fn(|component| {
        (second_velocity_m_s[component] - first_velocity_m_s[component]) * coupling_mass
            / exchange_time_s
    });
    let first = impulse.map(|value| value / first_mass_kg_m2);
    let second = impulse.map(|value| -value / second_mass_kg_m2);
    let residual = std::array::from_fn::<_, 3, _>(|component| {
        first_mass_kg_m2 * first[component] + second_mass_kg_m2 * second[component]
    });
    Ok(PairedMomentumExchange {
        first_acceleration_m_s2: first,
        second_acceleration_m_s2: second,
        extensive_residual_n_m2: norm(residual),
    })
}

/// Adds one physical surface stress through the transpose reconstruction.
/// Momentum sources accumulate in f64 until the integrator stores the state;
/// rounding each source into an existing f32 tendency can erase its reaction.
fn add_surface_stress_component(
    targets: &mut [f64; 3],
    masses: [f64; 3],
    weights: [f64; 2],
    stress: f64,
) -> [f64; 3] {
    let before = *targets;
    let forces = [-weights[0] * stress, -weights[1] * stress, stress];
    for (side, target) in targets.iter_mut().enumerate() {
        *target += forces[side] / masses[side];
    }
    std::array::from_fn(|side| targets[side] - before[side])
}

fn add_balanced_pair_to_f64(
    first: &mut f64,
    second: &mut f64,
    desired_first_delta: f64,
    first_weight: f64,
    second_weight: f64,
) -> (f64, f64) {
    let before = (*first, *second);
    let flux = first_weight * desired_first_delta;
    *first += desired_first_delta;
    *second -= flux / second_weight;
    (*first - before.0, *second - before.1)
}

fn next_f32_up(value: f32) -> f32 {
    if value.is_nan() || value == f32::INFINITY {
        return value;
    }
    if value == 0.0 {
        return f32::from_bits(1);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits + 1 } else { bits - 1 })
}

fn next_f32_down(value: f32) -> f32 {
    if value.is_nan() || value == f32::NEG_INFINITY {
        return value;
    }
    if value == 0.0 {
        return f32::from_bits(0x8000_0001);
    }
    let bits = value.to_bits();
    f32::from_bits(if value > 0.0 { bits - 1 } else { bits + 1 })
}

#[derive(Debug, Clone, PartialEq)]
struct ActiveLayerTendency {
    role: ClimateLayerRole,
    height_tendency_m_s: Vec<f32>,
    velocity_tendency_m_s2: Vec<[f64; 3]>,
    temperature_tendency_k_s: Vec<f32>,
}

/// Fully accounted instantaneous tendency shared by every time integrator.
#[derive(Debug, Clone, PartialEq)]
pub struct LayeredClimateTendency {
    active_layers: Vec<ActiveLayerTendency>,
    overturning_exchange_m_s: Option<Vec<f64>>,
    momentum_transport_rate_s_inv: f64,
    specific_humidity_tendency_s_inv: Vec<f32>,
    external_moisture_tendency_s_inv: Vec<f64>,
    upper_specific_humidity_tendency_s_inv: Option<Vec<f32>>,
    evaporation_rate_mm_s: Vec<f32>,
    /// Part of `evaporation_rate_mm_s` supplied by land evapotranspiration;
    /// its latent heat is taken from the lower atmosphere, not the mixed layer.
    land_evapotranspiration_rate_mm_s: Vec<f32>,
    precipitation_rate_mm_s: Vec<f32>,
    orographic_precipitation_rate_mm_s: Vec<f32>,
    /// Part of `precipitation_rate_mm_s` condensed from moisture that
    /// converged beyond the transport bound; its latent heat is exported by
    /// the overturning instead of warming a layer.
    convective_precipitation_rate_mm_s: Vec<f32>,
    external_radiative_heat_flux_w_m2: Vec<f64>,
    deep_ocean_temperature_tendency_k_s: Option<Vec<f32>>,
    budget: LayeredTendencyBudget,
}

impl LayeredClimateTendency {
    fn zeroed(state: &LayeredClimateState) -> Self {
        let count = state.cell_count();
        Self {
            overturning_exchange_m_s: None,
            momentum_transport_rate_s_inv: 0.0,
            active_layers: state
                .active_roles()
                .iter()
                .map(|role| ActiveLayerTendency {
                    role: *role,
                    height_tendency_m_s: vec![0.0; count],
                    velocity_tendency_m_s2: vec![[0.0; 3]; count],
                    temperature_tendency_k_s: vec![0.0; count],
                })
                .collect(),
            specific_humidity_tendency_s_inv: vec![0.0; count],
            external_moisture_tendency_s_inv: vec![0.0; count],
            upper_specific_humidity_tendency_s_inv: state
                .upper_specific_humidity()
                .map(|_| vec![0.0; count]),
            evaporation_rate_mm_s: vec![0.0; count],
            land_evapotranspiration_rate_mm_s: vec![0.0; count],
            precipitation_rate_mm_s: vec![0.0; count],
            orographic_precipitation_rate_mm_s: vec![0.0; count],
            convective_precipitation_rate_mm_s: vec![0.0; count],
            external_radiative_heat_flux_w_m2: vec![0.0; count],
            deep_ocean_temperature_tendency_k_s: state
                .deep_ocean_temperature_c()
                .map(|_| vec![0.0; count]),
            budget: LayeredTendencyBudget::default(),
        }
    }

    fn layer(&self, role: ClimateLayerRole) -> Option<&ActiveLayerTendency> {
        self.active_layers.iter().find(|layer| layer.role == role)
    }

    fn layer_mut(&mut self, role: ClimateLayerRole) -> Option<&mut ActiveLayerTendency> {
        self.active_layers
            .iter_mut()
            .find(|layer| layer.role == role)
    }

    pub fn height_tendency_m_s(&self, role: ClimateLayerRole) -> Option<&[f32]> {
        self.layer(role)
            .map(|layer| layer.height_tendency_m_s.as_slice())
    }

    pub(super) fn overturning_exchange_m_s(&self) -> Option<&[f64]> {
        self.overturning_exchange_m_s.as_deref()
    }

    pub(super) const fn momentum_transport_rate_s_inv(&self) -> f64 {
        self.momentum_transport_rate_s_inv
    }

    /// Returns the accumulated acceleration in m/s² for an active layer.
    /// Sources remain f64 until the integrator stores the next velocity state.
    pub fn velocity_tendency_m_s2(&self, role: ClimateLayerRole) -> Option<&[[f64; 3]]> {
        self.layer(role)
            .map(|layer| layer.velocity_tendency_m_s2.as_slice())
    }

    pub fn temperature_tendency_k_s(&self, role: ClimateLayerRole) -> Option<&[f32]> {
        self.layer(role)
            .map(|layer| layer.temperature_tendency_k_s.as_slice())
    }

    pub fn specific_humidity_tendency_s_inv(&self) -> &[f32] {
        &self.specific_humidity_tendency_s_inv
    }

    pub fn upper_specific_humidity_tendency_s_inv(&self) -> Option<&[f32]> {
        self.upper_specific_humidity_tendency_s_inv.as_deref()
    }

    pub fn evaporation_rate_mm_s(&self) -> &[f32] {
        &self.evaporation_rate_mm_s
    }

    pub fn precipitation_rate_mm_s(&self) -> &[f32] {
        &self.precipitation_rate_mm_s
    }

    pub fn orographic_precipitation_rate_mm_s(&self) -> &[f32] {
        &self.orographic_precipitation_rate_mm_s
    }

    pub fn convective_precipitation_rate_mm_s(&self) -> &[f32] {
        &self.convective_precipitation_rate_mm_s
    }

    pub fn external_radiative_heat_flux_w_m2(&self) -> &[f64] {
        &self.external_radiative_heat_flux_w_m2
    }

    pub fn deep_ocean_temperature_tendency_k_s(&self) -> Option<&[f32]> {
        self.deep_ocean_temperature_tendency_k_s.as_deref()
    }

    pub const fn budget(&self) -> LayeredTendencyBudget {
        self.budget
    }

    fn enforce_moisture_availability(
        &mut self,
        state: &LayeredClimateState,
        step_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        for (cell, (humidity_tendency, available_humidity)) in self
            .specific_humidity_tendency_s_inv
            .iter_mut()
            .zip(state.specific_humidity())
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let minimum_tendency = -f64::from(*available_humidity) / step_seconds;
            if f64::from(*humidity_tendency) < minimum_tendency {
                // Bias the f32 result one small relative margin toward zero so
                // the integrator's final cast cannot cross the physical floor.
                let bounded = (minimum_tendency * (1.0 - 8.0 * f64::from(f32::EPSILON))) as f32;
                *humidity_tendency = bounded;
            }
        }
        // This floor owns only the final f32 composition of transport plus
        // physical source/sink. It must not relabel a transport/quantization
        // correction as evaporation; any such correction remains visible to
        // the complete external-source closure budget.
        if let (Some(tendency), Some(humidity)) = (
            &mut self.upper_specific_humidity_tendency_s_inv,
            state.upper_specific_humidity(),
        ) {
            for (cell, (tendency, available)) in tendency.iter_mut().zip(humidity).enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let minimum_tendency = -f64::from(*available) / step_seconds;
                if f64::from(*tendency) < minimum_tendency {
                    *tendency = (minimum_tendency * (1.0 - 8.0 * f64::from(f32::EPSILON))) as f32;
                }
            }
        }
        Ok(())
    }

    fn limit_external_moisture_to_transported_availability(
        &mut self,
        state: &LayeredClimateState,
        transported_humidity: &[f32],
        step_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let atmospheric_column_mass =
            moisture_column_mass_per_area(state, ClimateLayerRole::LowerAtmosphere);
        for (
            cell,
            (
                (
                    (
                        (((external_tendency, physical_tendency), evaporation), precipitation),
                        orographic_precipitation,
                    ),
                    convective_precipitation,
                ),
                available_humidity,
            ),
        ) in self
            .external_moisture_tendency_s_inv
            .iter_mut()
            .zip(&mut self.specific_humidity_tendency_s_inv)
            .zip(&mut self.evaporation_rate_mm_s)
            .zip(&mut self.precipitation_rate_mm_s)
            .zip(&mut self.orographic_precipitation_rate_mm_s)
            .zip(&mut self.convective_precipitation_rate_mm_s)
            .zip(transported_humidity)
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let requested_evaporation = *evaporation;
            let minimum_tendency = -f64::from(*available_humidity) / step_seconds;
            if *external_tendency < minimum_tendency {
                let bounded = (minimum_tendency * (1.0 - 8.0 * f64::from(f32::EPSILON))) as f32;
                let removed_sink = f64::from(bounded) - *external_tendency;
                *physical_tendency = bounded;
                *external_tendency = f64::from(bounded);
                let original_precipitation = f64::from(*precipitation);
                *precipitation = (original_precipitation - removed_sink * atmospheric_column_mass)
                    .max(0.0) as f32;
                let retained_fraction = if original_precipitation > 0.0 {
                    (f64::from(*precipitation) / original_precipitation).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                *orographic_precipitation =
                    (f64::from(*orographic_precipitation) * retained_fraction) as f32;
                *convective_precipitation =
                    (f64::from(*convective_precipitation) * retained_fraction) as f32;
            }
            *evaporation = if requested_evaporation == 0.0 {
                0.0
            } else {
                (f64::from(*precipitation) + atmospheric_column_mass * *external_tendency).max(0.0)
                    as f32
            };
        }
        Ok(())
    }

    fn refresh_external_moisture_budget(
        &mut self,
        grid: &CubedSphereGrid,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let mut evaporation_source_rate_kg_s = 0.0;
        let mut precipitation_sink_rate_kg_s = 0.0;
        for (index, ((cell, evaporation), precipitation)) in grid
            .cells()
            .iter()
            .zip(&self.evaporation_rate_mm_s)
            .zip(&self.precipitation_rate_mm_s)
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            evaporation_source_rate_kg_s += cell.area_m2() * f64::from(*evaporation);
            precipitation_sink_rate_kg_s += cell.area_m2() * f64::from(*precipitation);
        }
        self.budget.external_moisture_source_rate_kg_s = evaporation_source_rate_kg_s;
        self.budget.external_precipitation_sink_rate_kg_s = precipitation_sink_rate_kg_s;
        Ok(())
    }
}

/// One-evaluation source and paired-exchange accounting on the solver clock.
/// Heat powers use physical heat capacities and the applied temperature rates;
/// published TOA radiation is kept separately in the physical-flux array.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LayeredTendencyBudget {
    paired_heat_absolute_w: f64,
    paired_heat_residual_w: f64,
    paired_momentum_absolute_n: f64,
    paired_momentum_residual_n: f64,
    paired_moisture_absolute_kg_s: f64,
    paired_moisture_residual_kg_s: f64,
    external_atmosphere_amount_rate_m3_s: f64,
    external_ocean_amount_rate_m3_s: f64,
    external_moisture_source_rate_kg_s: f64,
    external_precipitation_sink_rate_kg_s: f64,
    external_heat_rate_w: f64,
    external_heat_absolute_w: f64,
}

impl LayeredTendencyBudget {
    pub const fn paired_heat_absolute_w(self) -> f64 {
        self.paired_heat_absolute_w
    }

    /// Absolute external heat power on the solver clock, including radiation
    /// and convective latent export. It shares the physical-capacity ledger
    /// with paired heat; the physical TOA flux is reported separately.
    pub const fn external_heat_absolute_w(self) -> f64 {
        self.external_heat_absolute_w
    }

    pub const fn paired_heat_residual_w(self) -> f64 {
        self.paired_heat_residual_w
    }

    pub const fn paired_momentum_absolute_n(self) -> f64 {
        self.paired_momentum_absolute_n
    }

    pub const fn paired_momentum_residual_n(self) -> f64 {
        self.paired_momentum_residual_n
    }

    pub const fn paired_moisture_absolute_kg_s(self) -> f64 {
        self.paired_moisture_absolute_kg_s
    }

    pub const fn paired_moisture_residual_kg_s(self) -> f64 {
        self.paired_moisture_residual_kg_s
    }

    pub const fn external_atmosphere_amount_rate_m3_s(self) -> f64 {
        self.external_atmosphere_amount_rate_m3_s
    }

    pub const fn external_ocean_amount_rate_m3_s(self) -> f64 {
        self.external_ocean_amount_rate_m3_s
    }

    pub const fn external_moisture_source_rate_kg_s(self) -> f64 {
        self.external_moisture_source_rate_kg_s
    }

    pub const fn external_precipitation_sink_rate_kg_s(self) -> f64 {
        self.external_precipitation_sink_rate_kg_s
    }

    pub const fn external_moisture_net_rate_kg_s(self) -> f64 {
        self.external_moisture_source_rate_kg_s - self.external_precipitation_sink_rate_kg_s
    }

    pub const fn external_heat_rate_w(self) -> f64 {
        self.external_heat_rate_w
    }
}

/// Reusable dense scratch storage owned by a formation driver.
#[derive(Debug, Clone, PartialEq)]
pub struct LayeredTendencyWorkspace {
    cell_count: usize,
    edge_count: usize,
    open_edges: Vec<f32>,
    scalar_scratch: Vec<f32>,
    thickness_tendency_m_s: Vec<f64>,
    /// Upper horizontal thickness tendency held until its layer is assembled;
    /// moisture-dependent vertical venting is declared later in the endpoint.
    upper_thickness_tendency_m_s: Vec<f64>,
    /// Latitude band of every cell for the axisymmetric closure, one band per
    /// cubed-sphere cell row (`2 * face_resolution` bands), computed once per
    /// workspace.
    axisymmetric_band: Vec<u32>,
    band_area_m2: Vec<f64>,
    band_exchange_m3_s: Vec<f64>,
    vector_scratch: Vec<[f32; 3]>,
    transport: SecondOrderTransportWorkspace,
    atmosphere_strain: Option<viscosity::AtmosphereStrainWorkspace>,
}

#[derive(Debug)]
/// Quantized T gradients owned by one scalar endpoint and borrowed by fast stages.
pub(super) struct AtmosphericTemperatureGradients {
    lower_temperature: Vec<[f32; 3]>,
    upper_temperature: Vec<[f32; 3]>,
}

#[derive(Debug)]
struct AtmosphericPressureGradients<'temperature> {
    temperature: &'temperature AtmosphericTemperatureGradients,
    lower_height: Vec<[f32; 3]>,
    upper_height: Vec<[f32; 3]>,
}

impl LayeredTendencyWorkspace {
    pub fn for_grid(grid: &CubedSphereGrid) -> Self {
        Self {
            cell_count: grid.cell_count(),
            edge_count: grid.edges().len(),
            open_edges: vec![1.0; grid.edges().len()],
            scalar_scratch: vec![0.0; grid.cell_count()],
            thickness_tendency_m_s: vec![0.0; grid.cell_count()],
            upper_thickness_tendency_m_s: vec![0.0; grid.cell_count()],
            axisymmetric_band: axisymmetric_bands(grid),
            band_area_m2: vec![0.0; axisymmetric_band_count(grid)],
            band_exchange_m3_s: vec![0.0; axisymmetric_band_count(grid)],
            vector_scratch: vec![[0.0; 3]; grid.cell_count()],
            transport: SecondOrderTransportWorkspace::for_grid(grid),
            atmosphere_strain: None,
        }
    }

    pub fn allocation_signature(&self) -> [usize; 22] {
        let transport = self.transport.allocation_signature();
        [
            self.open_edges.capacity(),
            self.scalar_scratch.capacity(),
            self.thickness_tendency_m_s.capacity(),
            self.upper_thickness_tendency_m_s.capacity(),
            self.axisymmetric_band.capacity(),
            self.band_area_m2.capacity(),
            self.band_exchange_m3_s.capacity(),
            self.vector_scratch.capacity(),
            transport[0],
            transport[1],
            transport[2],
            transport[3],
            transport[4],
            transport[5],
            transport[6],
            transport[7],
            transport[8],
            transport[9],
            transport[10],
            transport[11],
            self.atmosphere_strain
                .as_ref()
                .map_or(0, |cache| cache.triangles.capacity()),
            self.atmosphere_strain
                .as_ref()
                .map_or(0, |cache| cache.acceleration.capacity()),
        ]
    }
}

/// Integrator-neutral composition of dynamics, relaxation, phase change, and
/// paired vertical/surface exchanges.
#[derive(Debug, Clone, Copy)]
pub struct LayeredTendencySystem<'grid> {
    grid: &'grid CubedSphereGrid,
    terrain_gradient_m_per_m: Option<&'grid [[f32; 3]]>,
    /// Land surface height under the lower atmosphere, per work cell; `None`
    /// integrates over a flat floor (test and comparison harnesses).
    terrain_floor_m: Option<&'grid [f32]>,
    sea_level_m: f32,
    /// Share of each cell's precipitation that land evaporates back (steady
    /// water balance with the P5 runoff partition); `None` keeps land dry.
    land_evapotranspiration_fraction: Option<&'grid [f32]>,
    forcing_prevalidated: bool,
}

#[derive(Debug, Clone, Copy)]
enum TendencyEvaluationMode {
    FullEndpoint,
    ThermodynamicMoistureEndpoint,
    LinearImplicit,
    SmoothDynamics,
}

impl TendencyEvaluationMode {
    const fn includes_explicit_transport_and_moisture(self) -> bool {
        matches!(
            self,
            Self::FullEndpoint | Self::ThermodynamicMoistureEndpoint
        )
    }

    const fn includes_dynamics(self) -> bool {
        !matches!(self, Self::ThermodynamicMoistureEndpoint)
    }

    const fn includes_thermodynamics(self) -> bool {
        !matches!(self, Self::SmoothDynamics)
    }

    const fn uses_explicit_dynamics(self) -> bool {
        matches!(self, Self::FullEndpoint | Self::SmoothDynamics)
    }
}

impl<'grid> LayeredTendencySystem<'grid> {
    pub const fn new(grid: &'grid CubedSphereGrid) -> Self {
        Self {
            grid,
            terrain_gradient_m_per_m: None,
            terrain_floor_m: None,
            sea_level_m: 0.0,
            land_evapotranspiration_fraction: None,
            forcing_prevalidated: false,
        }
    }

    /// Reuses the immutable terrain derivative already owned by the validated
    /// global-climate forcing artifact.
    pub(crate) const fn with_terrain(
        grid: &'grid CubedSphereGrid,
        terrain_gradient_m_per_m: &'grid [[f32; 3]],
        terrain_floor_m: &'grid [f32],
        land_evapotranspiration_fraction: &'grid [f32],
        sea_level_m: f32,
    ) -> Self {
        Self {
            grid,
            terrain_gradient_m_per_m: Some(terrain_gradient_m_per_m),
            terrain_floor_m: Some(terrain_floor_m),
            sea_level_m,
            land_evapotranspiration_fraction: Some(land_evapotranspiration_fraction),
            forcing_prevalidated: true,
        }
    }

    /// Only the lower atmosphere sits on the terrain; every other layer rests
    /// on the layer below it.
    fn layer_terrain_floor(&self, role: ClimateLayerRole) -> Option<&'grid [f32]> {
        match role {
            ClimateLayerRole::LowerAtmosphere => self.terrain_floor_m,
            _ => None,
        }
    }

    fn fluid_layer_thickness_m(
        &self,
        state: &LayeredClimateState,
        role: ClimateLayerRole,
        cell: usize,
    ) -> Result<f64, LayeredTendencyError> {
        Self::validated_fluid_layer_thickness_m(
            f64::from(state.reference_thickness_m(role).expect("active layer")),
            state.height_anomaly_m(role).expect("active layer")[cell],
            self.layer_terrain_floor(role)
                .map_or(0.0, |floor| floor[cell]),
            role,
            cell,
        )
    }

    /// Validates the same actual depth for dynamics and final surface-wind reconstruction.
    pub(super) fn validated_fluid_layer_thickness_m(
        reference_depth_m: f64,
        height_anomaly_m: f32,
        terrain_floor_m: f32,
        role: ClimateLayerRole,
        cell: usize,
    ) -> Result<f64, LayeredTendencyError> {
        let depth = reference_depth_m + f64::from(height_anomaly_m) - f64::from(terrain_floor_m);
        if !depth.is_finite() || depth <= 0.0 {
            return Err(LayeredTendencyError::InvalidFluidThickness {
                role,
                cell,
                found: depth,
            });
        }
        Ok(depth)
    }

    /// C2 stress accelerates the actual fluid column, including displaced
    /// interfaces and the terrain floor: tau / (rho H). This is the finite
    /// volume surface-force conversion used by MITgcm's momentum forcing;
    /// the same mass must weight internal exchanges and axial eddy torque.
    fn momentum_mass_per_area(
        &self,
        state: &LayeredClimateState,
        role: ClimateLayerRole,
        reference_mass: f64,
        cell: usize,
    ) -> Result<f64, LayeredTendencyError> {
        if state.profile() == ClimateModelProfile::C1SingleLayerV1 {
            // C1 retains the reference-mass linear momentum approximation.
            return Ok(reference_mass);
        }
        Ok(
            reference_mass * self.fluid_layer_thickness_m(state, role, cell)?
                / f64::from(state.reference_thickness_m(role).expect("active layer")),
        )
    }

    fn atmospheric_surface_wind(
        &self,
        state: &LayeredClimateState,
        cell: usize,
    ) -> Result<([f64; 3], [f64; 2]), LayeredTendencyError> {
        let lower = state
            .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
            .expect("lower atmosphere")[cell];
        let Some(upper) = state.velocity_m_s(ClimateLayerRole::UpperAtmosphere) else {
            return Ok((lower.map(f64::from), [1.0, 0.0]));
        };
        let weights = crate::world::natural::atmosphere_surface_wind_weights(
            self.fluid_layer_thickness_m(state, ClimateLayerRole::LowerAtmosphere, cell)?,
            self.fluid_layer_thickness_m(state, ClimateLayerRole::UpperAtmosphere, cell)?,
        );
        Ok((
            crate::world::natural::reconstruct_atmosphere_surface_wind_m_s(
                lower,
                upper[cell],
                weights,
            ),
            weights,
        ))
    }

    /// Terrain floor under the lower atmosphere for one work grid: land
    /// fraction times land height above sea level, capped so the layer keeps
    /// at least `LOWER_ATMOSPHERE_MIN_THICKNESS_M`.
    pub(crate) fn lower_atmosphere_terrain_floor_m(
        relative_elevation_m: &[f32],
        land_fraction: &[f32],
    ) -> Vec<f32> {
        let cap =
            (LOWER_ATMOSPHERE_REFERENCE_THICKNESS_M - LOWER_ATMOSPHERE_MIN_THICKNESS_M) as f32;
        relative_elevation_m
            .iter()
            .zip(land_fraction)
            .map(|(&elevation, &land)| (elevation.max(0.0) * land).min(cap))
            .collect()
    }

    #[cfg(test)]
    pub(super) fn apply_overturning_exchange(
        &self,
        state: &mut LayeredClimateState,
        exchanges_m_s: &[f64],
        step_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        for (cell, &exchange) in exchanges_m_s.iter().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            if exchange == 0.0 {
                continue;
            }
            let (donor, receiver) = if exchange > 0.0 {
                (
                    ClimateLayerRole::LowerAtmosphere,
                    ClimateLayerRole::UpperAtmosphere,
                )
            } else {
                (
                    ClimateLayerRole::UpperAtmosphere,
                    ClimateLayerRole::LowerAtmosphere,
                )
            };
            let donor_height = state.height_anomaly_m(donor).expect("atmosphere")[cell];
            let receiver_height = state.height_anomaly_m(receiver).expect("atmosphere")[cell];
            let donor_mass = self.fluid_layer_thickness_m(state, donor, cell)?;
            let receiver_mass = self.fluid_layer_thickness_m(state, receiver, cell)?;
            let requested = exchange.abs() * step_seconds;
            if requested >= donor_mass {
                return Err(LayeredTendencyError::InvalidFluidThickness {
                    role: donor,
                    cell,
                    found: donor_mass - requested,
                });
            }
            let donor_after = (f64::from(donor_height) - requested) as f32;
            let receiver_after = (f64::from(receiver_height) + requested) as f32;
            let donor_delta = f64::from(donor_after) - f64::from(donor_height);
            let receiver_delta = f64::from(receiver_after) - f64::from(receiver_height);
            let donor_remaining = donor_mass + donor_delta;
            if donor_remaining <= 0.0 {
                return Err(LayeredTendencyError::InvalidFluidThickness {
                    role: donor,
                    cell,
                    found: donor_remaining,
                });
            }
            let donor_velocity = state.velocity_m_s(donor).expect("atmosphere")[cell];
            let receiver_velocity = state.velocity_m_s(receiver).expect("atmosphere")[cell];
            state.height_anomaly_m_mut(donor).expect("atmosphere")[cell] = donor_after;
            state.height_anomaly_m_mut(receiver).expect("atmosphere")[cell] = receiver_after;
            state.velocity_m_s_mut(receiver).expect("atmosphere")[cell] =
                std::array::from_fn(|component| {
                    ((receiver_mass * f64::from(receiver_velocity[component])
                        - donor_delta * f64::from(donor_velocity[component]))
                        / (receiver_mass + receiver_delta)) as f32
                });
        }
        check_cancelled(cancellation)
    }

    /// Adds a previously declared interface exchange to both mass and momentum.
    ///
    /// `state` supplies the stage's actual donor masses and velocities;
    /// `exchanges_m_s` is the unchanged signed declaration from the slow endpoint.
    /// Accumulates into `tendency`, returning cancellation or invalid-depth errors.
    pub(super) fn apply_declared_overturning_tendency(
        &self,
        state: &LayeredClimateState,
        exchanges_m_s: &[f64],
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        self.apply_declared_overturning_momentum(state, exchanges_m_s, cancellation, tendency)?;
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            let sign = if role == ClimateLayerRole::LowerAtmosphere {
                -1.0
            } else {
                1.0
            };
            for (target, &rate) in tendency
                .layer_mut(role)
                .expect("C2 atmosphere")
                .height_tendency_m_s
                .iter_mut()
                .zip(exchanges_m_s)
            {
                *target = (f64::from(*target) + sign * rate) as f32;
            }
        }
        Ok(())
    }

    pub(super) fn apply_declared_overturning_momentum(
        &self,
        state: &LayeredClimateState,
        exchanges_m_s: &[f64],
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        let mut maximum_exchange_rate = 0.0_f64;
        for (cell, &exchange) in exchanges_m_s.iter().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            if exchange == 0.0 {
                continue;
            }
            let (donor, receiver) = if exchange > 0.0 {
                (
                    ClimateLayerRole::LowerAtmosphere,
                    ClimateLayerRole::UpperAtmosphere,
                )
            } else {
                (
                    ClimateLayerRole::UpperAtmosphere,
                    ClimateLayerRole::LowerAtmosphere,
                )
            };
            let donor_depth = self.fluid_layer_thickness_m(state, donor, cell)?;
            let receiver_depth = self.fluid_layer_thickness_m(state, receiver, cell)?;
            let rate = exchange.abs() / receiver_depth;
            maximum_exchange_rate =
                maximum_exchange_rate.max(rate.max(exchange.abs() / donor_depth));
            let donor_velocity = state.velocity_m_s(donor).expect("C2")[cell];
            let receiver_velocity = state.velocity_m_s(receiver).expect("C2")[cell];
            let target = &mut tendency
                .layer_mut(receiver)
                .expect("C2")
                .velocity_tendency_m_s2[cell];
            for component in 0..3 {
                target[component] += rate
                    * (f64::from(donor_velocity[component])
                        - f64::from(receiver_velocity[component]));
            }
        }
        tendency.momentum_transport_rate_s_inv += maximum_exchange_rate;
        check_cancelled(cancellation)
    }

    pub fn evaluate(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        let mut workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        self.evaluate_with_workspace(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            &mut workspace,
        )
    }

    /// Evaluates the slow tendency with conservative transport limited over
    /// the actual integration horizon rather than over an arbitrary unit step.
    pub fn evaluate_for_step(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        step_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        let mut workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        self.evaluate_with_workspace_for_step(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            step_seconds,
            cancellation,
            &mut workspace,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_with_workspace(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        self.evaluate_with_workspace_mode(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            workspace,
            TendencyEvaluationMode::FullEndpoint,
            1.0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn evaluate_with_workspace_for_step(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        step_seconds: f64,
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        self.evaluate_with_workspace_mode(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            workspace,
            TendencyEvaluationMode::FullEndpoint,
            step_seconds,
        )
    }

    /// Evaluates exactly the production temperature, moisture, and phase
    /// tendencies on a prescribed dynamical background.
    ///
    /// The periodic water-inventory preconditioner advances only scalar
    /// state. Skipping velocity and layer-thickness operators here avoids
    /// computing values that the probe deliberately freezes, while the same
    /// production transport, radiation, exchange, and phase-change helpers
    /// remain the sole scalar implementation.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn evaluate_thermodynamic_moisture_with_workspace_for_step(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        step_seconds: f64,
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        self.evaluate_with_workspace_mode(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            workspace,
            TendencyEvaluationMode::ThermodynamicMoistureEndpoint,
            step_seconds,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn evaluate_linear_implicit_with_workspace(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        self.evaluate_with_workspace_mode(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            workspace,
            TendencyEvaluationMode::LinearImplicit,
            1.0,
        )
    }

    /// Evaluates smooth dynamics and C2 ocean heat transport. Local ocean
    /// thickness relaxation, radiation, thermal exchange, phase change and
    /// atmospheric scalar endpoints are applied before these RK stages.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn evaluate_smooth_dynamics_with_workspace(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        self.evaluate_with_workspace_mode(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            workspace,
            TendencyEvaluationMode::SmoothDynamics,
            1.0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_with_workspace_mode(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
        mode: TendencyEvaluationMode,
        transport_step_seconds: f64,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        if !transport_step_seconds.is_finite() || transport_step_seconds <= 0.0 {
            return Err(LayeredTendencyError::InvalidTransportStep {
                found: transport_step_seconds,
            });
        }
        self.validate_inputs(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            workspace,
        )?;

        let operators = CirculationOperators::new(self.grid);
        let mut tendency = LayeredClimateTendency::zeroed(state);
        for role in state.active_roles() {
            check_cancelled(cancellation)?;
            let velocity = state.velocity_m_s(*role).expect("active role");
            let temperature = state.temperature_c(*role).expect("active role");
            let ocean = matches!(
                role,
                ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline
            );
            if mode.includes_explicit_transport_and_moisture()
                && !(state.profile() == ClimateModelProfile::C2LayeredV1 && ocean)
            {
                self.temperature_transport_tendency_for_step(
                    state,
                    (*role, ocean_edge_permeability),
                    transport_step_seconds,
                    workspace,
                    cancellation,
                )?;
                tendency
                    .layer_mut(*role)
                    .expect("active tendency role")
                    .temperature_tendency_k_s
                    .copy_from_slice(&workspace.scalar_scratch);
            }
            let permeability = if ocean {
                ocean_edge_permeability
            } else {
                &workspace.open_edges
            };
            if !mode.includes_dynamics() {
                continue;
            }
            let height = state.height_anomaly_m(*role).expect("active role");
            let height_gradient = operators.gradient_with_permeability_cancellable(
                height,
                permeability,
                cancellation,
            )?;
            let coriolis = operators.coriolis_cancellable(
                velocity,
                EARTH_ROTATION_RATE_RAD_S,
                cancellation,
            )?;
            let thermal_gradient = if state.profile() == ClimateModelProfile::C2LayeredV1
                && (is_atmosphere_role(*role) || mode.uses_explicit_dynamics())
            {
                None
            } else {
                Some(operators.gradient_with_permeability_cancellable(
                    temperature,
                    permeability,
                    cancellation,
                )?)
            };
            let (reduced_gravity, drag_s_inv, height_relax_s, thermal_gradient_acceleration) =
                role_constants(state.profile(), *role);
            let viscous_rate = self.horizontal_velocity_diffusion(
                state,
                *role,
                ocean_edge_permeability,
                workspace,
                cancellation,
            )?;
            tendency.momentum_transport_rate_s_inv =
                tendency.momentum_transport_rate_s_inv.max(viscous_rate);
            let permeability = if ocean {
                ocean_edge_permeability
            } else {
                &workspace.open_edges
            };
            let layered_atmosphere = state.profile() == ClimateModelProfile::C2LayeredV1;
            if layered_atmosphere && is_atmosphere_role(*role) && mode.uses_explicit_dynamics() {
                // The shared reconstructed face flux below updates both H
                // and momentum after the pointwise dynamical assembly.
                workspace.thickness_tendency_m_s.fill(0.0);
            } else if layered_atmosphere && *role == ClimateLayerRole::LowerAtmosphere {
                // Retain both horizontal tendencies. Finite axisymmetric
                // venting is declared after its moisture budget is available.
                self.layer_thickness_tendency_into(
                    &operators,
                    state,
                    ClimateLayerRole::LowerAtmosphere,
                    &workspace.open_edges,
                    mode.uses_explicit_dynamics(),
                    &mut workspace.thickness_tendency_m_s,
                    cancellation,
                )?;
                self.layer_thickness_tendency_into(
                    &operators,
                    state,
                    ClimateLayerRole::UpperAtmosphere,
                    &workspace.open_edges,
                    mode.uses_explicit_dynamics(),
                    &mut workspace.upper_thickness_tendency_m_s,
                    cancellation,
                )?;
            } else if layered_atmosphere && *role == ClimateLayerRole::UpperAtmosphere {
                workspace
                    .thickness_tendency_m_s
                    .copy_from_slice(&workspace.upper_thickness_tendency_m_s);
            } else {
                self.layer_thickness_tendency_into(
                    &operators,
                    state,
                    *role,
                    permeability,
                    mode.uses_explicit_dynamics(),
                    &mut workspace.thickness_tendency_m_s,
                    cancellation,
                )?;
            }
            let mut external_amount_rate_m3_s = 0.0_f64;
            {
                let layer = tendency
                    .active_layers
                    .iter_mut()
                    .find(|layer| layer.role == *role)
                    .expect("active tendency role");
                for cell in 0..self.grid.cell_count() {
                    if cell % 256 == 0 {
                        check_cancelled(cancellation)?;
                    }
                    layer.height_tendency_m_s[cell] = workspace.thickness_tendency_m_s[cell] as f32;
                    let before_height = layer.height_tendency_m_s[cell];
                    if !(state.profile() == ClimateModelProfile::C2LayeredV1
                        && (is_atmosphere_role(*role)
                            || matches!(mode, TendencyEvaluationMode::SmoothDynamics)))
                    {
                        layer.height_tendency_m_s[cell] +=
                            (-f64::from(height[cell]) / height_relax_s) as f32;
                    }
                    let retained_external_height =
                        f64::from(layer.height_tendency_m_s[cell]) - f64::from(before_height);
                    external_amount_rate_m3_s +=
                        self.grid.cells()[cell].area_m2() * retained_external_height;
                    let radial = self.grid.cells()[cell].center_unit();
                    let coastal_drag_s_inv = if ocean {
                        f64::from(forcing.land_fraction()[cell]) / COASTAL_FORM_DRAG_TIMESCALE_S
                    } else {
                        0.0
                    };
                    let surface_drag_s_inv = if *role == ClimateLayerRole::LowerAtmosphere {
                        drag_s_inv
                            * (1.0
                                + (LAND_SEA_SURFACE_DRAG_RATIO - 1.0)
                                    * f64::from(forcing.land_fraction()[cell]))
                    } else {
                        drag_s_inv
                    };
                    let bathymetric_bottom_drag_s_inv =
                        if *role == ClimateLayerRole::OceanThermocline {
                            let water_fraction = 1.0 - f64::from(forcing.land_fraction()[cell]);
                            let depth_m = f64::from(forcing.ocean_depth_m()[cell]);
                            water_fraction
                                * (BATHYMETRIC_BOTTOM_DRAG_REFERENCE_DEPTH_M
                                    / depth_m.max(BATHYMETRIC_BOTTOM_DRAG_REFERENCE_DEPTH_M))
                                / BATHYMETRIC_BOTTOM_DRAG_TIMESCALE_S
                        } else {
                            0.0
                        };
                    let mut acceleration = [0.0_f64; 3];
                    for component in 0..3 {
                        acceleration[component] = -reduced_gravity
                            * f64::from(height_gradient[cell][component])
                            + f64::from(coriolis[cell][component])
                            - (surface_drag_s_inv
                                + coastal_drag_s_inv
                                + bathymetric_bottom_drag_s_inv)
                                * f64::from(velocity[cell][component])
                            + thermal_gradient_acceleration
                                * thermal_gradient
                                    .as_ref()
                                    .map_or(0.0, |gradient| f64::from(gradient[cell][component]))
                            + f64::from(workspace.vector_scratch[cell][component]);
                    }
                    acceleration = tangentize(acceleration, radial);
                    layer.velocity_tendency_m_s2[cell] = acceleration;
                }
            }
            if is_atmosphere_role(*role) {
                tendency.budget.external_atmosphere_amount_rate_m3_s += external_amount_rate_m3_s;
            } else {
                tendency.budget.external_ocean_amount_rate_m3_s += external_amount_rate_m3_s;
            }
        }

        if mode.includes_dynamics() {
            self.apply_land_surface_drag(state, forcing, cancellation, &mut tendency)?;
        }

        if mode.includes_thermodynamics() {
            self.apply_external_radiation(
                state,
                forcing,
                month,
                transport_step_seconds,
                &mut tendency,
                cancellation,
            )?;
        }

        if mode.includes_dynamics()
            && mode.uses_explicit_dynamics()
            && state.profile() == ClimateModelProfile::C2LayeredV1
        {
            apply_baroclinic_reynolds_stress_closure(
                self,
                state,
                forcing,
                &mut tendency,
                &mut workspace.scalar_scratch,
                cancellation,
            )?;
        }

        if mode.includes_explicit_transport_and_moisture() {
            let computed_terrain_gradient;
            let terrain_gradient = if let Some(terrain_gradient) = self.terrain_gradient_m_per_m {
                terrain_gradient
            } else {
                workspace
                    .scalar_scratch
                    .copy_from_slice(forcing.elevation_m());
                computed_terrain_gradient =
                    operators.gradient_cancellable(&workspace.scalar_scratch, cancellation)?;
                &computed_terrain_gradient
            };
            let lower_velocity = state
                .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere is active");
            let transported_humidity = operators
                .advect_scalar_monotone_second_order_retaining_convergent_excess_cancellable(
                    state.specific_humidity(),
                    lower_velocity,
                    &workspace.open_edges,
                    transport_step_seconds,
                    &mut workspace.transport,
                    cancellation,
                )?;
            self.apply_moisture(
                state,
                forcing,
                terrain_gradient,
                transported_humidity.values(),
                transported_humidity.convergent_excess(),
                transport_step_seconds,
                &mut tendency,
                cancellation,
            )?;
            tendency.limit_external_moisture_to_transported_availability(
                state,
                transported_humidity.values(),
                transport_step_seconds,
                cancellation,
            )?;
            self.apply_phase_change_latent_heat(state, &mut tendency, cancellation)?;
            for (cell, (target, (transported, original))) in tendency
                .specific_humidity_tendency_s_inv
                .iter_mut()
                .zip(
                    transported_humidity
                        .values()
                        .iter()
                        .zip(state.specific_humidity()),
                )
                .enumerate()
            {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                *target += ((f64::from(*transported) - f64::from(*original))
                    / transport_step_seconds) as f32;
            }
            // Poleward moisture transport in the real extratropics is carried
            // by transient baroclinic eddies, which a 24-48 cell cubed face
            // cannot resolve; advecting only the mean flow leaves the high
            // latitudes with no vapour and therefore no condensation at all.
            accumulate_horizontal_scalar_diffusion(
                self.grid,
                transported_humidity.values(),
                &workspace.open_edges,
                ATMOSPHERE_HORIZONTAL_EDDY_MOISTURE_DIFFUSIVITY_M2_S,
                &mut tendency.specific_humidity_tendency_s_inv,
                cancellation,
            )?;
            if let (Some(upper_humidity), Some(upper_tendency)) = (
                state.upper_specific_humidity(),
                &mut tendency.upper_specific_humidity_tendency_s_inv,
            ) {
                let upper_velocity = state
                    .velocity_m_s(ClimateLayerRole::UpperAtmosphere)
                    .expect("C2 upper atmosphere");
                let transported_upper = operators
                    .advect_scalar_monotone_second_order_into_cancellable(
                        upper_humidity,
                        upper_velocity,
                        &workspace.open_edges,
                        transport_step_seconds,
                        true,
                        &mut workspace.transport,
                        cancellation,
                    )?;
                for (cell, (target, (transported, original))) in upper_tendency
                    .iter_mut()
                    .zip(transported_upper.values().iter().zip(upper_humidity))
                    .enumerate()
                {
                    if cell % 256 == 0 {
                        check_cancelled(cancellation)?;
                    }
                    *target += ((f64::from(*transported) - f64::from(*original))
                        / transport_step_seconds) as f32;
                }
            }
        }
        if mode.includes_explicit_transport_and_moisture() {
            tendency.enforce_moisture_availability(state, transport_step_seconds, cancellation)?;
        }
        if mode.includes_dynamics() && state.profile() == ClimateModelProfile::C2LayeredV1 {
            let temperature_gradients =
                self.atmospheric_temperature_gradients(state, Some(forcing), cancellation)?;
            let pressure_gradients = self.atmospheric_pressure_gradients(
                state,
                &temperature_gradients,
                &workspace.open_edges,
                cancellation,
            )?;
            self.apply_atmospheric_thermal_pressure(
                state,
                Some(forcing),
                &pressure_gradients,
                cancellation,
                &mut tendency,
            )?;
            self.apply_common_surface_pressure(
                state,
                &pressure_gradients,
                cancellation,
                &mut tendency,
            )?;
        }
        if mode.uses_explicit_dynamics() {
            self.apply_horizontal_momentum_transport(
                state,
                cancellation,
                &mut tendency,
                workspace,
            )?;
        }
        // Add pairs after their local and transport predictors so the budget
        // measures their retained increments. Moisture-dependent venting follows.
        // The moisture pair is symmetrically flux-limited against the
        // post-transport/post-condensation water still available in each
        // layer; no independent clipping follows it.
        self.apply_pair_exchanges(
            state,
            forcing,
            month,
            cancellation,
            &mut tendency,
            mode.includes_thermodynamics(),
            mode.includes_dynamics(),
            mode.includes_explicit_transport_and_moisture(),
            transport_step_seconds,
        )?;
        if mode.includes_explicit_transport_and_moisture() {
            if mode.includes_dynamics() && state.profile() == ClimateModelProfile::C2LayeredV1 {
                close_axisymmetric_baroclinic_thickness(self.grid, state, workspace, &mut tendency);
                let exchange = tendency
                    .overturning_exchange_m_s
                    .take()
                    .expect("C2 venting");
                self.apply_declared_overturning_tendency(
                    state,
                    &exchange,
                    cancellation,
                    &mut tendency,
                )?;
                tendency.overturning_exchange_m_s = Some(exchange);
            }
            self.apply_upper_condensation_after_exchange(
                state,
                transport_step_seconds,
                &mut tendency,
                cancellation,
            )?;
            tendency.refresh_external_moisture_budget(self.grid, cancellation)?;
        }
        if mode.uses_explicit_dynamics() {
            // Local pair predictors must not include fast ocean transport:
            // subtracting full - fast would not undo that nonlinear coupling.
            self.apply_ocean_fast_thermodynamics(
                state,
                ocean_edge_permeability,
                cancellation,
                workspace,
                &mut tendency,
            )?;
        }
        self.validate_tendency(&tendency, cancellation)?;
        Ok(tendency)
    }

    /// Divergence-driven thickness tendency of one layer: the donor-thickness
    /// conservative form for explicit dynamics, the linearized reference-depth
    /// form otherwise.
    #[allow(clippy::too_many_arguments)]
    fn layer_thickness_tendency_into(
        &self,
        operators: &CirculationOperators<'_>,
        state: &LayeredClimateState,
        role: ClimateLayerRole,
        permeability: &[f32],
        explicit: bool,
        target_m_s: &mut [f64],
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let height = state.height_anomaly_m(role).expect("active role");
        let velocity = state.velocity_m_s(role).expect("active role");
        let reference_thickness =
            f64::from(state.reference_thickness_m(role).expect("active role"));
        let terrain_floor = self.layer_terrain_floor(role);
        if explicit {
            return conservative_layer_thickness_tendency(
                self.grid,
                reference_thickness,
                terrain_floor,
                height,
                velocity,
                permeability,
                target_m_s,
                cancellation,
            );
        }
        let divergence = operators.divergence_with_permeability_cancellable(
            velocity,
            permeability,
            cancellation,
        )?;
        for (cell, (target, divergence)) in target_m_s.iter_mut().zip(divergence).enumerate() {
            let floor_m = terrain_floor.map_or(0.0, |floor| f64::from(floor[cell]));
            *target = -(reference_thickness - floor_m) * f64::from(divergence);
        }
        Ok(())
    }

    /// Applies one linearized TOA gray-radiation source to the resolved lower
    /// boundary before internal heat exchanges. Fractional cells partition
    /// the same power between the land proxy (lower air) and mixed layer;
    /// upper and subsurface reservoirs receive energy only by internal
    /// exchange, so TOA power is never counted once per active layer.
    fn apply_external_radiation(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        month: usize,
        step_seconds: f64,
        tendency: &mut LayeredClimateTendency,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        const SURFACE_ROLES: [ClimateLayerRole; 2] = [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::OceanMixedLayer,
        ];
        let layout = ClimateLayerLayout::for_profile(state.profile());
        for cell in 0..self.grid.cell_count() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let absorbed_shortwave =
                f64::from(forcing.monthly_absorbed_shortwave_w_m2()[cell][month]);
            let water_fraction = f64::from(forcing.surface_moisture_availability()[cell]);
            let weights = [1.0 - water_fraction, water_fraction];
            let resolved_temperatures = [
                f64::from(
                    state
                        .temperature_c(ClimateLayerRole::LowerAtmosphere)
                        .expect("lower atmosphere is active")[cell],
                ),
                f64::from(
                    state
                        .temperature_c(ClimateLayerRole::OceanMixedLayer)
                        .expect("mixed layer is active")[cell],
                ),
            ];
            // Milestone A4 (§3.3): the gray longwave is linearized about the
            // annual-mean state (`A + B T`), so seasonal storage shows up as a
            // seasonal TOA imbalance instead of being forced to zero every
            // month. The monthly targets are storage-consistent (§3.1), so
            // their 12-month means are the annual targets.
            let annual_mean = |months: &[f32; CLIMATE_MONTH_COUNT]| {
                months.iter().copied().map(f64::from).sum::<f64>() / CLIMATE_MONTH_COUNT as f64
            };
            let annual_absorbed_shortwave =
                annual_mean(&forcing.monthly_absorbed_shortwave_w_m2()[cell]);
            let equilibrium_temperatures = [
                annual_mean(&forcing.equilibrium_air_temperature_c()[cell]),
                annual_mean(&forcing.equilibrium_surface_temperature_c()[cell]).clamp(
                    f64::from(LIQUID_MIXED_LAYER_MIN_C),
                    f64::from(OCEAN_EQUILIBRIUM_MAX_C),
                ),
            ];
            let resolved_surface_temperature = weights
                .iter()
                .zip(resolved_temperatures)
                .map(|(weight, value)| weight * value)
                .sum::<f64>();
            let equilibrium_surface_temperature = weights
                .iter()
                .zip(equilibrium_temperatures)
                .map(|(weight, value)| weight * value)
                .sum::<f64>();
            let outgoing_longwave = linearized_outgoing_longwave_w_m2(
                annual_absorbed_shortwave,
                equilibrium_surface_temperature,
                resolved_surface_temperature,
            );
            let mut baselines = [0.0_f32; 2];
            let mut heat_capacities = [0.0_f64; 2];
            let mut retained_power = 0.0_f64;

            // Milestone A4 (§6.2): the formation sweeps a year per model day,
            // so the radiatively forced layers carry the heat capacity that
            // the compressed clock presents (Bryan 1984 distorted physics).
            // The same capacity converts the retained tendency back to power
            // below, so the published flux stays physical.
            for (role_index, role) in SURFACE_ROLES.iter().copied().enumerate() {
                let spec = layout
                    .layers()
                    .iter()
                    .find(|layer| layer.role() == role)
                    .expect("surface role belongs to the layout");
                heat_capacities[role_index] =
                    formation_thermal_heat_capacity_per_area(state, spec, cell)?;
            }
            // External relaxation is a slow term applied once per macro step,
            // and the compressed air capacity puts its rate near the step, so
            // an explicit application would ring. Damping the requested power
            // by the closed-form implicit factor makes one forward step equal
            // one backward-Euler step of the same relaxation, which is
            // monotone for any step. The weighted surface temperature relaxes
            // at `B * sum(w_i^2 / C_i)` because role `i` receives `w_i` of the
            // power and contributes `w_i` of the temperature.
            let relaxation_rate_per_s = gray_longwave_slope_w_m2_k(equilibrium_surface_temperature)
                * weights
                    .iter()
                    .zip(heat_capacities)
                    .map(|(weight, capacity)| weight * weight / capacity)
                    .sum::<f64>();
            let requested_power = (absorbed_shortwave - outgoing_longwave)
                / (1.0 + step_seconds * relaxation_rate_per_s);

            for (role_index, role) in SURFACE_ROLES.iter().copied().enumerate() {
                let baseline = tendency
                    .layer(role)
                    .expect("active tendency role")
                    .temperature_tendency_k_s[cell];
                let heat_capacity = heat_capacities[role_index];
                baselines[role_index] = baseline;
                let target = &mut tendency
                    .layer_mut(role)
                    .expect("active tendency role")
                    .temperature_tendency_k_s[cell];
                *target = baseline + (requested_power * weights[role_index] / heat_capacity) as f32;
                let actual_retained = f64::from(*target) - f64::from(baselines[role_index]);
                retained_power += heat_capacities[role_index] * actual_retained;
            }
            // Composing into an existing f32 tendency can cross the exact
            // OLR=0 boundary by one ULP. Project that equation state, not the
            // published flux, back into the representable feasible set.
            for _ in 0..2 {
                if retained_power <= absorbed_shortwave {
                    break;
                }
                for (role_index, role) in SURFACE_ROLES.iter().copied().enumerate() {
                    if retained_power <= absorbed_shortwave {
                        break;
                    }
                    let target = &mut tendency
                        .layer_mut(role)
                        .expect("active tendency role")
                        .temperature_tendency_k_s[cell];
                    if *target <= baselines[role_index] {
                        continue;
                    }
                    let required_power_reduction = retained_power - absorbed_shortwave;
                    let exact_target =
                        f64::from(*target) - required_power_reduction / heat_capacities[role_index];
                    let mut projected = exact_target as f32;
                    if f64::from(projected) > exact_target {
                        projected = next_f32_down(projected);
                    }
                    if projected >= *target {
                        projected = next_f32_down(*target);
                    }
                    *target = projected.max(baselines[role_index]);
                }
                retained_power = SURFACE_ROLES
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(role_index, role)| {
                        heat_capacities[role_index]
                            * (f64::from(
                                tendency
                                    .layer(role)
                                    .expect("active tendency role")
                                    .temperature_tendency_k_s[cell],
                            ) - f64::from(baselines[role_index]))
                    })
                    .sum();
            }
            if retained_power > absorbed_shortwave {
                return Err(
                    LayeredTendencyError::RadiativeHeatingExceedsAbsorbedShortwave {
                        cell,
                        month,
                        retained_w_m2: retained_power,
                        absorbed_w_m2: absorbed_shortwave,
                    },
                );
            }
            tendency.external_radiative_heat_flux_w_m2[cell] = retained_power;
            let area = self.grid.cells()[cell].area_m2();
            // The state ledger measures physical C * dT on the solver clock.
            // Only this accelerated source changes clock; latent heat retains
            // its physical rate, and the TOA array above retains physical flux.
            let solver_power = retained_power * GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION;
            tendency.budget.external_heat_rate_w += area * solver_power;
            tendency.budget.external_heat_absolute_w += area * solver_power.abs();
        }
        Ok(())
    }

    /// Evaluates fast layer dynamics, momentum exchange/diffusion and C2
    /// ocean heat transport. Atmospheric thermal pressure and ocean steric
    /// pressure consume each RK stage. Local heat, moisture, external height
    /// relaxation and the remaining slow forces stay in the declared endpoint.
    pub fn evaluate_fast(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        let mut workspace = LayeredTendencyWorkspace::for_grid(self.grid);
        self.evaluate_fast_with_workspace(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            &mut workspace,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_fast_with_workspace(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        self.validate_inputs(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            workspace,
        )?;
        self.evaluate_fast_with_workspace_validated(
            state,
            forcing,
            ocean_edge_permeability,
            cancellation,
            workspace,
        )
    }

    pub(crate) fn validate_fast_inputs(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
        workspace: &LayeredTendencyWorkspace,
    ) -> Result<(), LayeredTendencyError> {
        self.validate_inputs(
            state,
            forcing,
            ocean_edge_permeability,
            month,
            cancellation,
            workspace,
        )
    }

    /// Fast kernel for an integrator that already validated the immutable
    /// forcing/domain and whose RK stage constructor validates every state.
    /// The public entry point above remains the strict untrusted boundary.
    pub(crate) fn evaluate_fast_with_workspace_validated(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        self.evaluate_fast_with_temperature_gradients_validated(
            state,
            forcing,
            ocean_edge_permeability,
            cancellation,
            (workspace, None),
        )
    }

    /// The borrowed temperature gradients belong to one fixed scalar endpoint;
    /// every stage still computes its own actual depths and height gradients.
    /// The caller retains the validated grid, forcing and atmospheric T for
    /// this borrow. Domain and cancellation errors propagate from the RHS.
    pub(super) fn evaluate_fast_with_temperature_gradients_validated(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        cancellation: &BuildCancellation,
        workspace_and_temperature: (
            &mut LayeredTendencyWorkspace,
            Option<&AtmosphericTemperatureGradients>,
        ),
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        let (workspace, endpoint_temperature) = workspace_and_temperature;
        debug_assert_eq!(forcing.grid_fingerprint(), self.grid.fingerprint());
        debug_assert_eq!(forcing.cell_count(), self.grid.cell_count());
        debug_assert_eq!(ocean_edge_permeability.len(), self.grid.edges().len());
        debug_assert_eq!(workspace.cell_count, self.grid.cell_count());
        debug_assert_eq!(workspace.edge_count, self.grid.edges().len());
        let operators = CirculationOperators::new(self.grid);
        let mut tendency = LayeredClimateTendency::zeroed(state);
        for role in state.active_roles() {
            check_cancelled(cancellation)?;
            let height = state.height_anomaly_m(*role).expect("active role");
            let velocity = state.velocity_m_s(*role).expect("active role");
            let ocean = matches!(
                role,
                ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline
            );
            let permeability = if ocean {
                ocean_edge_permeability
            } else {
                &workspace.open_edges
            };
            let reference_thickness =
                f64::from(state.reference_thickness_m(*role).expect("active role"));
            operators.gradient_and_donor_layer_thickness_tendency_into_cancellable_validated(
                height,
                velocity,
                permeability,
                reference_thickness,
                self.layer_terrain_floor(*role),
                !(state.profile() == ClimateModelProfile::C2LayeredV1 && is_atmosphere_role(*role)),
                &mut workspace.vector_scratch,
                &mut workspace.thickness_tendency_m_s,
                &mut workspace.transport,
                cancellation,
            )?;
            let reduced_gravity = role_constants(state.profile(), *role).0;
            let layer = tendency.layer_mut(*role).expect("active tendency role");
            for (cell, &cell_velocity) in velocity.iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                layer.height_tendency_m_s[cell] = workspace.thickness_tendency_m_s[cell] as f32;
                let radial = self.grid.cells()[cell].center_unit();
                let coriolis = operators.coriolis_cell_projected_validated(
                    cell,
                    cell_velocity,
                    EARTH_ROTATION_RATE_RAD_S,
                );
                let acceleration = std::array::from_fn(|component| {
                    -reduced_gravity * f64::from(workspace.vector_scratch[cell][component])
                        + f64::from(coriolis[component])
                });
                layer.velocity_tendency_m_s2[cell] = tangentize(acceleration, radial);
            }
            let viscous_rate = self.horizontal_velocity_diffusion(
                state,
                *role,
                ocean_edge_permeability,
                workspace,
                cancellation,
            )?;
            tendency.momentum_transport_rate_s_inv =
                tendency.momentum_transport_rate_s_inv.max(viscous_rate);
            let layer = tendency.layer_mut(*role).expect("active tendency role");
            for cell in 0..self.grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                for component in 0..3 {
                    layer.velocity_tendency_m_s2[cell][component] +=
                        f64::from(workspace.vector_scratch[cell][component]);
                }
            }
        }
        if state.profile() == ClimateModelProfile::C2LayeredV1 {
            let fresh_temperature = if endpoint_temperature.is_none() {
                Some(self.atmospheric_temperature_gradients(state, Some(forcing), cancellation)?)
            } else {
                None
            };
            let temperature_gradients = endpoint_temperature
                .or(fresh_temperature.as_ref())
                .expect("C2 temperature gradients are supplied or evaluated");
            let pressure_gradients = self.atmospheric_pressure_gradients(
                state,
                temperature_gradients,
                &workspace.open_edges,
                cancellation,
            )?;
            self.apply_common_surface_pressure(
                state,
                &pressure_gradients,
                cancellation,
                &mut tendency,
            )?;
            self.apply_atmospheric_thermal_pressure(
                state,
                Some(forcing),
                &pressure_gradients,
                cancellation,
                &mut tendency,
            )?;
        }
        self.apply_horizontal_momentum_transport(state, cancellation, &mut tendency, workspace)?;
        self.apply_pair_momentum_exchanges(state, forcing, cancellation, &mut tendency)?;
        self.apply_ocean_fast_thermodynamics(
            state,
            ocean_edge_permeability,
            cancellation,
            workspace,
            &mut tendency,
        )?;
        self.validate_tendency(&tendency, cancellation)?;
        Ok(tendency)
    }

    /// The moving ocean columns carry temperature with the same donor-depth
    /// flux as their continuity equation (ROMS, Shchepetkin & McWilliams 2005,
    /// Eqs. 1.14-1.15). Steric pressure consumes that stage's temperature.
    fn apply_ocean_fast_thermodynamics(
        &self,
        state: &LayeredClimateState,
        permeability: &[f32],
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
        tendency: &mut LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        if state.profile() != ClimateModelProfile::C2LayeredV1 {
            return Ok(());
        }
        let operators = CirculationOperators::new(self.grid);
        for role in [
            ClimateLayerRole::OceanMixedLayer,
            ClimateLayerRole::OceanThermocline,
        ] {
            // The C2 ocean branch supplies an instantaneous derivative; its
            // interval belongs to the RK stages, not the scalar endpoint.
            self.temperature_transport_tendency_for_step(
                state,
                (role, permeability),
                1.0,
                workspace,
                cancellation,
            )?;
            let layer = tendency.layer_mut(role).expect("C2 ocean layer");
            for (target, transport) in layer
                .temperature_tendency_k_s
                .iter_mut()
                .zip(&workspace.scalar_scratch)
            {
                *target += transport;
            }
            let coefficient = role_constants(state.profile(), role).3;
            if coefficient == 0.0 {
                continue;
            }
            operators.gradient_into_cancellable_validated(
                state.temperature_c(role).expect("C2 ocean temperature"),
                permeability,
                &mut workspace.vector_scratch,
                &mut workspace.transport,
                cancellation,
            )?;
            for (cell, acceleration) in layer.velocity_tendency_m_s2.iter_mut().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let thermal = tangentize(
                    workspace.vector_scratch[cell].map(|value| coefficient * f64::from(value)),
                    self.grid.cells()[cell].center_unit(),
                );
                for (target, increment) in acceleration.iter_mut().zip(thermal) {
                    *target += increment;
                }
            }
        }
        Ok(())
    }

    fn horizontal_velocity_diffusion(
        &self,
        state: &LayeredClimateState,
        role: ClimateLayerRole,
        ocean_edge_permeability: &[f32],
        workspace: &mut LayeredTendencyWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<f64, LayeredTendencyError> {
        if state.profile() == ClimateModelProfile::C2LayeredV1 && is_atmosphere_role(role) {
            return self.atmosphere_strain_diffusion(state, role, workspace, cancellation);
        }
        let edge_permeability = if is_atmosphere_role(role) {
            &workspace.open_edges
        } else {
            ocean_edge_permeability
        };
        let target_acceleration_m_s2 = &mut workspace.vector_scratch;
        let grid = self.grid;
        let velocity_m_s = state.velocity_m_s(role).expect("active velocity layer");
        let diffusivity_m2_s = if is_atmosphere_role(role) {
            ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S
        } else {
            OCEAN_HORIZONTAL_EDDY_VISCOSITY_M2_S
        };
        let actual_mass = state.profile() == ClimateModelProfile::C2LayeredV1;
        debug_assert_eq!(edge_permeability.len(), grid.edges().len());
        debug_assert_eq!(target_acceleration_m_s2.len(), grid.cell_count());
        target_acceleration_m_s2.fill([0.0; 3]);
        let mut maximum_rate = 0.0_f64;

        for (edge_index, edge) in grid.edges().iter().enumerate() {
            if edge_index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let permeability = f64::from(edge_permeability[edge_index]);
            if permeability == 0.0 {
                continue;
            }
            let [first, second] = edge.cells().map(|cell| cell as usize);
            let midpoint = edge.midpoint_unit();
            let first_radial = grid.cells()[first].center_unit();
            let second_radial = grid.cells()[second].center_unit();
            let first_at_midpoint = parallel_transport_tangent(
                velocity_m_s[first].map(f64::from),
                first_radial,
                midpoint,
            );
            let second_at_midpoint = parallel_transport_tangent(
                velocity_m_s[second].map(f64::from),
                second_radial,
                midpoint,
            );
            let mut conductance =
                diffusivity_m2_s * permeability * edge.length_m() / edge.center_distance_m();
            let (first_depth, second_depth) = if actual_mass {
                let depths = (
                    self.fluid_layer_thickness_m(state, role, first)?,
                    self.fluid_layer_thickness_m(state, role, second)?,
                );
                // Popinet (2020), Basilisk layered/diffusion.h:
                // H du/dt = div(nu H grad u). A symmetric face depth and
                // the adjoint (inverse) isometric transport make each face's
                // actual-column power -rho * conductance * |delta u|^2.
                conductance *= 0.5 * (depths.0 + depths.1);
                depths
            } else {
                (1.0, 1.0)
            };
            if actual_mass {
                // Degree times the largest incident conductance bounds each
                // row sum without a per-cell buffer. For frozen positive H,
                // the mass-weighted self-adjoint viscous spectrum lies in
                // [-2 * maximum_rate, 0] by the Gershgorin bound.
                for (cell, depth) in [(first, first_depth), (second, second_depth)] {
                    let geometry = &grid.cells()[cell];
                    let row_bound =
                        geometry.edges().len() as f64 * conductance / (geometry.area_m2() * depth);
                    maximum_rate = maximum_rate.max(row_bound);
                }
            }
            let flux = std::array::from_fn(|component| {
                conductance * (second_at_midpoint[component] - first_at_midpoint[component])
            });
            let (first_flux, second_flux) = if actual_mass {
                (
                    parallel_transport_tangent(flux, midpoint, first_radial),
                    parallel_transport_tangent(flux, midpoint, second_radial),
                )
            } else {
                // Retain C1's reference-area, projected-vector approximation.
                (flux, flux)
            };
            for component in 0..3 {
                target_acceleration_m_s2[first][component] +=
                    (first_flux[component] / (grid.cells()[first].area_m2() * first_depth)) as f32;
                target_acceleration_m_s2[second][component] -= (second_flux[component]
                    / (grid.cells()[second].area_m2() * second_depth))
                    as f32;
            }
        }
        for (cell, acceleration) in target_acceleration_m_s2.iter_mut().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            *acceleration = tangentize(
                acceleration.map(f64::from),
                grid.cells()[cell].center_unit(),
            )
            .map(|component| component as f32);
        }
        check_cancelled(cancellation)?;
        Ok(maximum_rate)
    }

    fn apply_horizontal_momentum_transport(
        &self,
        state: &LayeredClimateState,
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<(), LayeredTendencyError> {
        if state.profile() != ClimateModelProfile::C2LayeredV1 {
            return Ok(());
        }
        let mut maximum_rate = 0.0_f64;
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            let velocity = state.velocity_m_s(role).expect("C2 atmosphere");
            // Reuse this existing f64 cell buffer for actual depth; the old
            // donor Hdot is no longer needed for explicit C2 atmosphere.
            for (cell, depth) in workspace.thickness_tendency_m_s.iter_mut().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                *depth = self.fluid_layer_thickness_m(state, role, cell)?;
            }
            let operators = CirculationOperators::new(self.grid);
            let (volume_flux, face_depth) = operators.reconstruct_layer_faces_cancellable(
                &workspace.thickness_tendency_m_s,
                velocity,
                &workspace.open_edges,
                &mut workspace.transport,
                cancellation,
            )?;
            let layer = tendency.layer_mut(role).expect("C2 atmosphere");
            for (cell, geometry) in self.grid.cells().iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let volume =
                    geometry.area_m2() * self.fluid_layer_thickness_m(state, role, cell)?;
                let mut acceleration = [0.0_f64; 3];
                let mut exchange_rate = 0.0;
                let mut amount_rate = 0.0;
                for &edge_index in geometry.edges() {
                    let edge_index = edge_index as usize;
                    let edge = &self.grid.edges()[edge_index];
                    let flux = volume_flux[edge_index] * face_depth[edge_index];
                    let [first, second] = edge.cells().map(|index| index as usize);
                    amount_rate += if cell == first { -flux } else { flux };
                    exchange_rate += flux.abs() / volume;
                    // Chandrashekar (2013): F*(u_i+u_j)/2 preserves the
                    // convective kinetic energy with the same mass flux F.
                    // Subtract only u*dH_horizontal in converting H*u to u;
                    // vertical exchange has its own donor momentum operator.
                    for (component, target) in acceleration.iter_mut().enumerate() {
                        *target += 0.5 * flux / volume
                            * (f64::from(velocity[first][component])
                                - f64::from(velocity[second][component]));
                    }
                }
                layer.height_tendency_m_s[cell] = (amount_rate / geometry.area_m2()) as f32;
                maximum_rate = maximum_rate.max(exchange_rate);
                let acceleration = tangentize(acceleration, geometry.center_unit());
                for (target, acceleration) in layer.velocity_tendency_m_s2[cell]
                    .iter_mut()
                    .zip(acceleration)
                {
                    *target += acceleration;
                }
            }
        }
        // The combined operator needs a sum bound; assigning the advective
        // rate here would erase the already retained viscous restriction.
        tendency.momentum_transport_rate_s_inv += maximum_rate;
        Ok(())
    }

    fn atmospheric_reference_height_m(&self, forcing: Option<&PlanetForcing>, cell: usize) -> f64 {
        forcing.map_or(0.0, |forcing| {
            crate::world::natural::atmospheric_reference_surface_height_m(
                forcing.elevation_m()[cell] - self.sea_level_m,
                forcing.land_fraction()[cell],
            )
        })
    }

    /// Builds both C2 gradients with the unchanged reference-height correction
    /// and tangent quantization. Propagates gradient and cancellation errors.
    pub(super) fn atmospheric_temperature_gradients(
        &self,
        state: &LayeredClimateState,
        forcing: Option<&PlanetForcing>,
        cancellation: &BuildCancellation,
    ) -> Result<AtmosphericTemperatureGradients, LayeredTendencyError> {
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        let operators = CirculationOperators::new(self.grid);
        let open = vec![1.0; self.grid.edges().len()];
        let gradient = |role| -> Result<Vec<[f32; 3]>, LayeredTendencyError> {
            let mut corrected = Vec::with_capacity(self.grid.cell_count());
            for (cell, &temperature) in state.temperature_c(role).expect("C2").iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                corrected.push(
                    (f64::from(temperature)
                        + crate::world::natural::CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M
                            * self.atmospheric_reference_height_m(forcing, cell))
                        as f32,
                );
            }
            Ok(
                operators.gradient_with_permeability_cancellable(
                    &corrected,
                    &open,
                    cancellation,
                )?,
            )
        };
        let lower_gradient = gradient(lower)?;
        let upper_gradient = gradient(upper)?;
        Ok(AtmosphericTemperatureGradients {
            lower_temperature: lower_gradient,
            upper_temperature: upper_gradient,
        })
    }

    fn atmospheric_pressure_gradients<'temperature>(
        &self,
        state: &LayeredClimateState,
        temperature: &'temperature AtmosphericTemperatureGradients,
        open: &[f32],
        cancellation: &BuildCancellation,
    ) -> Result<AtmosphericPressureGradients<'temperature>, LayeredTendencyError> {
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        let operators = CirculationOperators::new(self.grid);
        let interface_gradient = operators.gradient_with_permeability_cancellable(
            state.height_anomaly_m(lower).expect("C2"),
            open,
            cancellation,
        )?;
        let upper_height_gradient = operators.gradient_with_permeability_cancellable(
            state.height_anomaly_m(upper).expect("C2"),
            open,
            cancellation,
        )?;
        Ok(AtmosphericPressureGradients {
            temperature,
            lower_height: interface_gradient,
            upper_height: upper_height_gradient,
        })
    }

    fn apply_atmospheric_thermal_pressure(
        &self,
        state: &LayeredClimateState,
        forcing: Option<&PlanetForcing>,
        gradients: &AtmosphericPressureGradients<'_>,
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        if state.profile() != ClimateModelProfile::C2LayeredV1 {
            return Ok(());
        }
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        let lower_gradient = &gradients.temperature.lower_temperature;
        let upper_gradient = &gradients.temperature.upper_temperature;
        let interface_gradient = &gradients.lower_height;
        let upper_height_gradient = &gradients.upper_height;
        let internal_gravity =
            role_constants(state.profile(), lower).0 + role_constants(state.profile(), upper).0;
        for cell in 0..self.grid.cell_count() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let lower_depth = self.fluid_layer_thickness_m(state, lower, cell)?;
            let upper_depth = self.fluid_layer_thickness_m(state, upper, cell)?;
            let buoyancy_difference = atmospheric_thermal_buoyancy_difference_m_s2(state, cell);
            let upper_buoyancy = atmospheric_unlapsed_upper_buoyancy_m_s2(state, cell)
                + atmospheric_thermal_buoyancy_m_s2(
                    crate::world::natural::CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M
                        * self.atmospheric_reference_height_m(forcing, cell),
                );
            let surface_gravity = STANDARD_GRAVITY_M_S2 - upper_buoyancy;
            if !surface_gravity.is_finite() || surface_gravity <= 0.0 {
                return Err(LayeredTendencyError::UnstableAtmosphericFreeSurface {
                    cell,
                    effective_gravity_m_s2: surface_gravity,
                });
            }
            if !buoyancy_difference.is_finite() || internal_gravity <= buoyancy_difference {
                return Err(LayeredTendencyError::UnstableAtmosphericStratification {
                    cell,
                    reduced_gravity_m_s2: internal_gravity - buoyancy_difference,
                });
            }
            // Hydrostatic integration at fixed physical height precedes layer
            // averaging. Differentiating an actual-depth weighted T instead
            // would introduce a spurious bottom-slope pressure term.
            let difference = std::array::from_fn(|component| {
                0.5 * atmospheric_thermal_buoyancy_m_s2(
                    lower_depth * f64::from(lower_gradient[cell][component])
                        + upper_depth * f64::from(upper_gradient[cell][component]),
                ) + buoyancy_difference * f64::from(interface_gradient[cell][component])
            });
            let difference = tangentize(difference, self.grid.cells()[cell].center_unit());
            // Fixed top pressure gives BT_U = U grad(b_U)/2 + b_U grad(eta).
            // Retain its common mode: deleting it also deletes the thermal
            // bottom-pressure torque over terrain (A5 §7.52).
            let upper_acceleration = tangentize(
                std::array::from_fn(|component| {
                    0.5 * upper_depth
                        * atmospheric_thermal_buoyancy_m_s2(f64::from(
                            upper_gradient[cell][component],
                        ))
                        + upper_buoyancy
                            * (f64::from(interface_gradient[cell][component])
                                + f64::from(upper_height_gradient[cell][component]))
                }),
                self.grid.cells()[cell].center_unit(),
            );
            for (component, difference) in difference.into_iter().enumerate() {
                tendency
                    .layer_mut(lower)
                    .expect("C2")
                    .velocity_tendency_m_s2[cell][component] +=
                    upper_acceleration[component] + difference;
                tendency
                    .layer_mut(upper)
                    .expect("C2")
                    .velocity_tendency_m_s2[cell][component] += upper_acceleration[component];
            }
        }
        Ok(())
    }

    fn apply_common_surface_pressure(
        &self,
        state: &LayeredClimateState,
        gradients: &AtmosphericPressureGradients<'_>,
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        if state.profile() != ClimateModelProfile::C2LayeredV1 {
            return Ok(());
        }
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        let lower_gradient = &gradients.lower_height;
        let upper_gradient = &gradients.upper_height;
        let upper_gravity = role_constants(state.profile(), upper).0;
        for role in [lower, upper] {
            let (lower_coefficient, upper_coefficient) = if role == lower {
                (STANDARD_GRAVITY_M_S2 + upper_gravity, STANDARD_GRAVITY_M_S2)
            } else {
                (STANDARD_GRAVITY_M_S2, STANDARD_GRAVITY_M_S2 - upper_gravity)
            };
            let velocities = &mut tendency.layer_mut(role).expect("C2").velocity_tendency_m_s2;
            for (cell, velocity) in velocities.iter_mut().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                for (component, target) in velocity.iter_mut().enumerate() {
                    *target -= lower_coefficient * f64::from(lower_gradient[cell][component])
                        + upper_coefficient * f64::from(upper_gradient[cell][component]);
                }
            }
        }
        Ok(())
    }

    /// Diagnoses the pressure-acceleration change caused only by replacing one
    /// validated thermodynamic endpoint with another.
    ///
    /// C1 uses this linear correction after its scalar endpoint. C2 evaluates
    /// thermal pressure in each fast stage; the difference remains available
    /// to the direct pressure-operator regression.
    pub(crate) fn evaluate_thermal_pressure_endpoint_difference_with_workspace_validated(
        &self,
        before: &LayeredClimateState,
        after: &LayeredClimateState,
        ocean_edge_permeability: &[f32],
        cancellation: &BuildCancellation,
        workspace: &mut LayeredTendencyWorkspace,
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
        debug_assert_eq!(before.profile(), after.profile());
        debug_assert_eq!(before.grid_fingerprint(), after.grid_fingerprint());
        debug_assert_eq!(before.grid_fingerprint(), self.grid.fingerprint());
        debug_assert_eq!(ocean_edge_permeability.len(), self.grid.edges().len());
        let operators = CirculationOperators::new(self.grid);
        let mut tendency = LayeredClimateTendency::zeroed(before);
        if before.profile() == ClimateModelProfile::C2LayeredV1 {
            let mut previous = LayeredClimateTendency::zeroed(before);
            self.apply_atmospheric_thermal_pressure(
                before,
                None,
                &self.atmospheric_pressure_gradients(
                    before,
                    &self.atmospheric_temperature_gradients(before, None, cancellation)?,
                    &workspace.open_edges,
                    cancellation,
                )?,
                cancellation,
                &mut previous,
            )?;
            self.apply_atmospheric_thermal_pressure(
                after,
                None,
                &self.atmospheric_pressure_gradients(
                    after,
                    &self.atmospheric_temperature_gradients(after, None, cancellation)?,
                    &workspace.open_edges,
                    cancellation,
                )?,
                cancellation,
                &mut tendency,
            )?;
            for role in [
                ClimateLayerRole::LowerAtmosphere,
                ClimateLayerRole::UpperAtmosphere,
            ] {
                for (target, &old) in tendency
                    .layer_mut(role)
                    .expect("C2")
                    .velocity_tendency_m_s2
                    .iter_mut()
                    .flatten()
                    .zip(
                        previous
                            .layer(role)
                            .expect("C2")
                            .velocity_tendency_m_s2
                            .iter()
                            .flatten(),
                    )
                {
                    *target -= old;
                }
            }
        }
        for role in before.active_roles() {
            check_cancelled(cancellation)?;
            if before.profile() == ClimateModelProfile::C2LayeredV1 && is_atmosphere_role(*role) {
                continue;
            }
            let before_temperature = before.temperature_c(*role).expect("active role");
            let after_temperature = after.temperature_c(*role).expect("active role");
            for (target, (&after, &before)) in workspace
                .scalar_scratch
                .iter_mut()
                .zip(after_temperature.iter().zip(before_temperature))
            {
                *target = after - before;
            }
            let ocean = matches!(
                role,
                ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline
            );
            let permeability = if ocean {
                ocean_edge_permeability
            } else {
                &workspace.open_edges
            };
            operators.gradient_into_cancellable_validated(
                &workspace.scalar_scratch,
                permeability,
                &mut workspace.vector_scratch,
                &mut workspace.transport,
                cancellation,
            )?;
            let thermal_gradient_acceleration = role_constants(before.profile(), *role).3;
            let layer = tendency.layer_mut(*role).expect("active tendency role");
            for cell in 0..self.grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let radial = self.grid.cells()[cell].center_unit();
                layer.velocity_tendency_m_s2[cell] = tangentize(
                    workspace.vector_scratch[cell]
                        .map(|value| thermal_gradient_acceleration * f64::from(value)),
                    radial,
                );
            }
        }
        self.validate_tendency(&tendency, cancellation)?;
        Ok(tendency)
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_inputs(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        ocean_edge_permeability: &[f32],
        month: usize,
        cancellation: &BuildCancellation,
        workspace: &LayeredTendencyWorkspace,
    ) -> Result<(), LayeredTendencyError> {
        check_cancelled(cancellation)?;
        if month >= CLIMATE_MONTH_COUNT {
            return Err(LayeredTendencyError::InvalidMonth { found: month });
        }
        state
            .validate_against_cancellable(self.grid, cancellation)
            .map_err(|error| {
                if error == LayeredStateError::Cancelled {
                    LayeredTendencyError::Cancelled
                } else {
                    LayeredTendencyError::State(error)
                }
            })?;
        if !self.forcing_prevalidated {
            forcing
                .validate_cancellable(&|| cancellation.is_cancelled())
                .map_err(|error| {
                    if error == ForcingError::Cancelled {
                        LayeredTendencyError::Cancelled
                    } else {
                        LayeredTendencyError::InvalidForcing {
                            reason: error.to_string(),
                        }
                    }
                })?;
        }
        if forcing.grid_fingerprint() != self.grid.fingerprint()
            || forcing.cell_count() != self.grid.cell_count()
        {
            return Err(LayeredTendencyError::GridMismatch);
        }
        if let Some(terrain_gradient) = self.terrain_gradient_m_per_m {
            if terrain_gradient.len() != self.grid.cell_count() {
                return Err(LayeredTendencyError::TerrainGradientLengthMismatch {
                    found: terrain_gradient.len(),
                    expected: self.grid.cell_count(),
                });
            }
            for (cell, value) in terrain_gradient.iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                if value.iter().any(|component| !component.is_finite()) {
                    return Err(LayeredTendencyError::InvalidTerrainGradient { cell });
                }
            }
        }
        if ocean_edge_permeability.len() != self.grid.edges().len() {
            return Err(LayeredTendencyError::PermeabilityLengthMismatch {
                found: ocean_edge_permeability.len(),
                expected: self.grid.edges().len(),
            });
        }
        for (edge, value) in ocean_edge_permeability.iter().copied().enumerate() {
            if edge % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(LayeredTendencyError::InvalidPermeability { edge, found: value });
            }
        }
        if workspace.cell_count != self.grid.cell_count()
            || workspace.edge_count != self.grid.edges().len()
        {
            return Err(LayeredTendencyError::WorkspaceGridMismatch);
        }
        Ok(())
    }

    /// Writes the selected layer's transport-only temperature tendency into
    /// the reusable scalar buffer; local thermodynamics consume it afterwards.
    fn temperature_transport_tendency_for_step(
        &self,
        state: &LayeredClimateState,
        layer: (ClimateLayerRole, &[f32]),
        step_seconds: f64,
        workspace: &mut LayeredTendencyWorkspace,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let (role, ocean_edge_permeability) = layer;
        if state.profile() == ClimateModelProfile::C2LayeredV1 && !is_atmosphere_role(role) {
            let temperature = state.temperature_c(role).expect("C2 ocean temperature");
            let fields = LayerTransportFields {
                velocity_m_s: state.velocity_m_s(role).expect("C2 ocean velocity"),
                height_anomaly_m: state.height_anomaly_m(role).expect("C2 ocean height"),
                reference_thickness_m: f64::from(
                    state.reference_thickness_m(role).expect("C2 ocean"),
                ),
                terrain_floor_m: None,
            };
            for (cell, geometry) in self.grid.cells().iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let mut content_difference_rate = 0.0;
                for &edge_index in geometry.edges() {
                    let edge = &self.grid.edges()[edge_index as usize];
                    let flux = donor_layer_edge_amount_rate_m3_s(
                        edge,
                        ocean_edge_permeability[edge_index as usize],
                        fields,
                    );
                    let [first, second] = edge.cells().map(|index| index as usize);
                    let outward_flux = if cell == first { flux } else { -flux };
                    if outward_flux < 0.0 {
                        let donor = if cell == first { second } else { first };
                        content_difference_rate -= outward_flux
                            * (f64::from(temperature[donor]) - f64::from(temperature[cell]));
                    }
                }
                // This is (HTdot - T*Hdot)/H, with outgoing terms cancelled
                // before rounding. The integrator reconstructs HTdot and
                // advances H and HT with identical RK stage weights.
                workspace.scalar_scratch[cell] = (content_difference_rate
                    / (geometry.area_m2() * self.fluid_layer_thickness_m(state, role, cell)?))
                    as f32;
            }
            return Ok(());
        }
        let operators = CirculationOperators::new(self.grid);
        let temperature = state.temperature_c(role).expect("active role");
        let velocity = state.velocity_m_s(role).expect("active role");
        let permeability = if is_atmosphere_role(role) {
            &workspace.open_edges
        } else {
            ocean_edge_permeability
        };
        let intensive_transport;
        let conservative_transport;
        let transported =
            if state.profile() == ClimateModelProfile::C2LayeredV1 && is_atmosphere_role(role) {
                // Temperature is intensive: compressing a parcel cannot
                // multiply its value by a factor that depends on °C vs K.
                intensive_transport = operators.advect_scalar_upwind_tracer_subcycled_cancellable(
                    temperature,
                    velocity,
                    permeability,
                    step_seconds,
                    cancellation,
                )?;
                intensive_transport.values()
            } else {
                conservative_transport = operators
                    .advect_scalar_monotone_second_order_into_cancellable(
                        temperature,
                        velocity,
                        permeability,
                        step_seconds,
                        false,
                        &mut workspace.transport,
                        cancellation,
                    )?;
                conservative_transport.values()
            };
        for (cell, (target, (transported, original))) in workspace
            .scalar_scratch
            .iter_mut()
            .zip(transported.iter().zip(temperature))
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            *target = ((f64::from(*transported) - f64::from(*original)) / step_seconds) as f32;
        }
        if state.profile() == ClimateModelProfile::C2LayeredV1 && is_atmosphere_role(role) {
            let transport = &mut workspace.scalar_scratch;
            workspace.band_area_m2.fill(0.0);
            workspace.band_exchange_m3_s.fill(0.0);
            for (cell, geometry) in self.grid.cells().iter().enumerate() {
                let band = workspace.axisymmetric_band[cell] as usize;
                workspace.band_area_m2[band] += geometry.area_m2();
                workspace.band_exchange_m3_s[band] +=
                    geometry.area_m2() * f64::from(transport[cell]);
            }
            for (cell, target) in transport.iter_mut().enumerate() {
                let band = workspace.axisymmetric_band[cell] as usize;
                *target = (f64::from(*target)
                    - workspace.band_exchange_m3_s[band] / workspace.band_area_m2[band])
                    as f32;
            }
        }

        Ok(())
    }

    fn apply_moisture(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        terrain_gradient: &[[f32; 3]],
        transported_humidity: &[f32],
        convergent_excess: &[f64],
        step_seconds: f64,
        tendency: &mut LayeredClimateTendency,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let lower_velocity = state
            .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
            .expect("lower atmosphere is active");
        // Orographic lifting is done by the air that crosses the ridge. With
        // the lower layer now flowing around terrain (A2), the over-flow above
        // the dividing streamline (Sheppard 1956; Hunt & Snyder 1980) is the
        // upper layer in C2; C1 has no upper layer and keeps the lower wind.
        let lifting_velocity = state
            .velocity_m_s(ClimateLayerRole::UpperAtmosphere)
            .unwrap_or(lower_velocity);
        let lower_temperature = state
            .temperature_c(ClimateLayerRole::LowerAtmosphere)
            .expect("lower atmosphere is active");
        let surface_temperature = state
            .temperature_c(ClimateLayerRole::OceanMixedLayer)
            .expect("mixed layer is active");
        let atmospheric_column_mass =
            moisture_column_mass_per_area(state, ClimateLayerRole::LowerAtmosphere);
        let atmospheric_dry_mass = mass_per_area(state, ClimateLayerRole::LowerAtmosphere);
        for cell in 0..self.grid.cell_count() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let transported = f64::from(transported_humidity[cell]);
            let (surface_wind, _) = self.atmospheric_surface_wind(state, cell)?;
            let wind_speed = if state.profile() == ClimateModelProfile::C2LayeredV1 {
                let ocean_velocity = state
                    .velocity_m_s(ClimateLayerRole::OceanMixedLayer)
                    .expect("mixed layer")[cell];
                norm(std::array::from_fn(|component| {
                    surface_wind[component] - f64::from(ocean_velocity[component])
                }))
            } else {
                norm(surface_wind)
            };
            let lifting_speed = lifting_velocity[cell]
                .iter()
                .map(|component| f64::from(*component).powi(2))
                .sum::<f64>()
                .sqrt();
            let surface_temperature_c = f64::from(surface_temperature[cell]);
            // A5 q is already near-surface humidity; neutral bulk transfer
            // retains its actual deficit relative to saturation at the sea.
            let evaporation_rate_kg_m2_s = bulk_surface_evaporation_kg_m2_s(
                surface_temperature_c,
                transported,
                wind_speed,
                f64::from(forcing.surface_moisture_availability()[cell]),
            );
            let upslope_velocity = lifting_velocity[cell]
                .iter()
                .zip(terrain_gradient[cell])
                .map(|(velocity, gradient)| f64::from(*velocity) * f64::from(gradient))
                .sum::<f64>();
            let land_fraction = f64::from(forcing.land_fraction()[cell]);
            let ocean_evaporation =
                transported + step_seconds * evaporation_rate_kg_m2_s / atmospheric_column_mass;
            let orographic_rate_kg_m2_s = lcl_adjusted_orographic_condensation_kg_m2_s(
                transported,
                f64::from(lower_temperature[cell]),
                upslope_velocity,
                lifting_speed,
                self.grid.cells()[cell].area_m2(),
            ) * land_fraction;
            // Moisture that converged beyond the transport bound is the water
            // carried by the mass that must leave the layer upward; it
            // condenses as it rises (Kuo 1965: converged moisture precipitates).
            let convective_rate_kg_m2_s =
                convergent_excess[cell].max(0.0) * atmospheric_column_mass / step_seconds;
            let lifted_rate_kg_m2_s = orographic_rate_kg_m2_s + convective_rate_kg_m2_s;
            // Land returns the non-runoff share of its own precipitation to
            // the atmosphere (steady bucket balance E = P - R, Manabe 1969,
            // with the P5 runoff partition). One Picard pass: condense once
            // without recycling, evaporate that share, condense again.
            let recycling_fraction = self
                .land_evapotranspiration_fraction
                .map_or(0.0, |fraction| f64::from(fraction[cell]));
            let first_pass_precipitation = (large_scale_condensation_kg_m2_s(
                ocean_evaporation,
                f64::from(lower_temperature[cell]),
                atmospheric_column_mass,
                atmospheric_dry_mass,
                step_seconds,
            ) + lifted_rate_kg_m2_s)
                .min(ocean_evaporation.max(0.0) * atmospheric_column_mass / step_seconds);
            let land_evapotranspiration_rate_kg_m2_s =
                recycling_fraction * first_pass_precipitation.max(0.0);
            let evaporation_rate_kg_m2_s =
                evaporation_rate_kg_m2_s + land_evapotranspiration_rate_kg_m2_s;
            let after_evaporation = ocean_evaporation
                + step_seconds * land_evapotranspiration_rate_kg_m2_s / atmospheric_column_mass;
            let large_scale_condensation_rate_kg_m2_s = large_scale_condensation_kg_m2_s(
                after_evaporation,
                f64::from(lower_temperature[cell]),
                atmospheric_column_mass,
                atmospheric_dry_mass,
                step_seconds,
            );
            let available_rate_kg_m2_s =
                after_evaporation.max(0.0) * atmospheric_column_mass / step_seconds;
            let requested_precipitation_rate_kg_m2_s = (large_scale_condensation_rate_kg_m2_s
                + lifted_rate_kg_m2_s)
                .min(available_rate_kg_m2_s);
            let desired_end = after_evaporation
                - step_seconds * requested_precipitation_rate_kg_m2_s / atmospheric_column_mass;
            let desired_tendency = (desired_end - transported) / step_seconds;
            let mut retained_tendency = desired_tendency as f32;
            if transported + step_seconds * f64::from(retained_tendency) > desired_end {
                retained_tendency = next_f32_down(retained_tendency);
            }
            if transported + step_seconds * f64::from(retained_tendency) < 0.0 {
                retained_tendency = next_f32_up(retained_tendency);
            }
            let retained_precipitation = if requested_precipitation_rate_kg_m2_s == 0.0 {
                0.0
            } else {
                (evaporation_rate_kg_m2_s - atmospheric_column_mass * f64::from(retained_tendency))
                    .max(0.0)
            };
            let retained_lifted_fraction = if requested_precipitation_rate_kg_m2_s > 0.0 {
                (retained_precipitation / requested_precipitation_rate_kg_m2_s).clamp(0.0, 1.0)
            } else {
                0.0
            };
            tendency.specific_humidity_tendency_s_inv[cell] = retained_tendency;
            tendency.external_moisture_tendency_s_inv[cell] =
                f64::from(tendency.specific_humidity_tendency_s_inv[cell]);
            tendency.evaporation_rate_mm_s[cell] = evaporation_rate_kg_m2_s as f32;
            tendency.land_evapotranspiration_rate_mm_s[cell] =
                land_evapotranspiration_rate_kg_m2_s as f32;
            tendency.precipitation_rate_mm_s[cell] = retained_precipitation as f32;
            tendency.orographic_precipitation_rate_mm_s[cell] =
                (orographic_rate_kg_m2_s * retained_lifted_fraction) as f32;
            tendency.convective_precipitation_rate_mm_s[cell] =
                (convective_rate_kg_m2_s * retained_lifted_fraction) as f32;
        }
        Ok(())
    }

    fn apply_phase_change_latent_heat(
        &self,
        state: &LayeredClimateState,
        tendency: &mut LayeredClimateTendency,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let layout = ClimateLayerLayout::for_profile(state.profile());
        let lower_spec = layout
            .layers()
            .iter()
            .find(|layer| layer.role() == ClimateLayerRole::LowerAtmosphere)
            .expect("lower atmosphere belongs to the layout");
        let lower_capacity = cell_heat_capacity_per_area(state, lower_spec, 0)?;
        let surface_spec = layout
            .layers()
            .iter()
            .find(|layer| layer.role() == ClimateLayerRole::OceanMixedLayer)
            .expect("mixed layer belongs to the layout");
        for cell in 0..self.grid.cell_count() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let surface_capacity = cell_heat_capacity_per_area(state, surface_spec, cell)?;
            let land_evapotranspiration =
                f64::from(tendency.land_evapotranspiration_rate_mm_s[cell]);
            let ocean_evaporation =
                f64::from(tendency.evaporation_rate_mm_s[cell]) - land_evapotranspiration;
            // Convective condensation of converged moisture releases its
            // latent heat in the ascending branch, where the adiabatic cooling
            // of that ascent balances it (weak temperature gradient, Sobel,
            // Nilsson & Polvani 2001). The heat becomes potential energy of the
            // overturning and is exported by its upper branch, whose
            // zonal-mean transport the radiative prior already owns (A4 §6.1);
            // depositing it here would count it twice and drive the lower
            // layer's pressure into a moisture-convergence runaway. It is
            // booked as an explicit external energy sink instead.
            let convective = f64::from(tendency.convective_precipitation_rate_mm_s[cell]);
            let condensation = f64::from(tendency.precipitation_rate_mm_s[cell]) - convective;
            let exported_power_w_m2 = WATER_VAPORIZATION_LATENT_HEAT_J_KG * convective;
            let area = self.grid.cells()[cell].area_m2();
            tendency.budget.external_heat_rate_w -= area * exported_power_w_m2;
            tendency.budget.external_heat_absolute_w += area * exported_power_w_m2;
            // Over land the radiation scheme already treats the lower
            // atmosphere as the surface, so land evapotranspiration draws its
            // latent heat there; only sea evaporation cools the mixed layer.
            let lower = &mut tendency
                .layer_mut(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere is active")
                .temperature_tendency_k_s[cell];
            *lower += (WATER_VAPORIZATION_LATENT_HEAT_J_KG
                * (condensation - land_evapotranspiration)
                / lower_capacity) as f32;
            let surface = &mut tendency
                .layer_mut(ClimateLayerRole::OceanMixedLayer)
                .expect("mixed layer is active")
                .temperature_tendency_k_s[cell];
            *surface -=
                (WATER_VAPORIZATION_LATENT_HEAT_J_KG * ocean_evaporation / surface_capacity) as f32;
        }
        Ok(())
    }

    fn apply_upper_condensation_after_exchange(
        &self,
        state: &LayeredClimateState,
        step_seconds: f64,
        tendency: &mut LayeredClimateTendency,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let Some(upper_humidity) = state.upper_specific_humidity() else {
            return Ok(());
        };
        let upper_temperature = state
            .temperature_c(ClimateLayerRole::UpperAtmosphere)
            .expect("C2 upper atmosphere");
        let upper_mass = moisture_column_mass_per_area(state, ClimateLayerRole::UpperAtmosphere);
        let upper_dry_mass = mass_per_area(state, ClimateLayerRole::UpperAtmosphere);
        let layout = ClimateLayerLayout::for_profile(state.profile());
        let upper_spec = layout
            .layers()
            .iter()
            .find(|layer| layer.role() == ClimateLayerRole::UpperAtmosphere)
            .expect("upper atmosphere belongs to the layout");
        let upper_capacity = cell_heat_capacity_per_area(state, upper_spec, 0)?;
        for cell in 0..self.grid.cell_count() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let before_tendency = f64::from(
                tendency
                    .upper_specific_humidity_tendency_s_inv
                    .as_ref()
                    .expect("C2 upper moisture tendency")[cell],
            );
            let predicted_humidity =
                (f64::from(upper_humidity[cell]) + step_seconds * before_tendency).max(0.0);
            let requested_precipitation = large_scale_condensation_kg_m2_s(
                predicted_humidity,
                f64::from(upper_temperature[cell]),
                upper_mass,
                upper_dry_mass,
                step_seconds,
            );
            if requested_precipitation == 0.0 {
                continue;
            }
            let desired_end =
                predicted_humidity - step_seconds * requested_precipitation / upper_mass;
            let desired_tendency = (desired_end - f64::from(upper_humidity[cell])) / step_seconds;
            let mut retained_tendency = desired_tendency as f32;
            let mut retained_end =
                f64::from(upper_humidity[cell]) + step_seconds * f64::from(retained_tendency);
            if retained_end > desired_end {
                retained_tendency = next_f32_down(retained_tendency);
                retained_end =
                    f64::from(upper_humidity[cell]) + step_seconds * f64::from(retained_tendency);
            }
            if retained_end < 0.0 {
                retained_tendency = next_f32_up(retained_tendency);
            }
            let retained_precipitation =
                (upper_mass * (before_tendency - f64::from(retained_tendency))).max(0.0);
            tendency
                .upper_specific_humidity_tendency_s_inv
                .as_mut()
                .expect("C2 upper moisture tendency")[cell] = retained_tendency;
            tendency.precipitation_rate_mm_s[cell] += retained_precipitation as f32;
            tendency
                .layer_mut(ClimateLayerRole::UpperAtmosphere)
                .expect("C2 upper atmosphere")
                .temperature_tendency_k_s[cell] += (WATER_VAPORIZATION_LATENT_HEAT_J_KG
                * retained_precipitation
                / upper_capacity) as f32;
        }
        Ok(())
    }

    fn apply_land_surface_drag(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        if state.profile() != ClimateModelProfile::C2LayeredV1 {
            return Ok(());
        }
        let roles = [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ];
        let velocities = roles.map(|role| state.velocity_m_s(role).expect("C2 atmosphere"));
        let reference_masses = roles.map(|role| mass_per_area(state, role));
        for cell in 0..self.grid.cell_count() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let land_scale = LAND_SEA_SURFACE_DRAG_RATIO * f64::from(forcing.land_fraction()[cell]);
            if land_scale == 0.0 {
                continue;
            }
            let depths = [
                self.fluid_layer_thickness_m(state, roles[0], cell)?,
                self.fluid_layer_thickness_m(state, roles[1], cell)?,
            ];
            let forces = crate::world::natural::held_suarez_land_friction_forces_n_m2(
                depths,
                velocities.map(|velocity| velocity[cell]),
            );
            for (layer_index, role) in roles.into_iter().enumerate() {
                let mass =
                    self.momentum_mass_per_area(state, role, reference_masses[layer_index], cell)?;
                let target = &mut tendency
                    .layer_mut(role)
                    .expect("C2 atmosphere tendency")
                    .velocity_tendency_m_s2[cell];
                for component in 0..3 {
                    target[component] += land_scale * forces[layer_index][component] / mass;
                }
            }
        }
        Ok(())
    }

    fn apply_pair_momentum_exchanges(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        let layout = ClimateLayerLayout::for_profile(state.profile());
        for exchange in layout
            .exchanges()
            .iter()
            .copied()
            .filter(|exchange| exchange.momentum_exchange_time_s().is_some())
        {
            let first_role = exchange.first();
            let second_role = exchange.second();
            let momentum_timescale = exchange
                .momentum_exchange_time_s()
                .expect("filtered exchange has momentum timescale");
            let first_velocity = state.velocity_m_s(first_role).expect("pair role");
            let second_velocity = state.velocity_m_s(second_role).expect("pair role");
            let first_mass = mass_per_area(state, first_role);
            let second_mass = mass_per_area(state, second_role);
            for cell in 0..self.grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let water_scale = if exchange.water_only() {
                    f64::from(1.0 - forcing.land_fraction()[cell])
                } else {
                    1.0
                };
                if water_scale == 0.0 {
                    continue;
                }
                let first_mass =
                    self.momentum_mass_per_area(state, first_role, first_mass, cell)?;
                let second_mass =
                    self.momentum_mass_per_area(state, second_role, second_mass, cell)?;
                let surface_exchange = state.profile() == ClimateModelProfile::C2LayeredV1
                    && first_role == ClimateLayerRole::LowerAtmosphere
                    && second_role == ClimateLayerRole::OceanMixedLayer;
                if surface_exchange {
                    let upper_role = ClimateLayerRole::UpperAtmosphere;
                    let roles = [first_role, upper_role, second_role];
                    let masses = [
                        first_mass,
                        self.momentum_mass_per_area(
                            state,
                            upper_role,
                            mass_per_area(state, upper_role),
                            cell,
                        )?,
                        second_mass,
                    ];
                    let (boundary_velocity, weights) =
                        self.atmospheric_surface_wind(state, cell)?;
                    let relative = std::array::from_fn::<_, 3, _>(|component| {
                        boundary_velocity[component] - f64::from(second_velocity[cell][component])
                    });
                    let conductance = water_scale
                        * crate::world::natural::P4_REFERENCE_AIR_DENSITY_KG_M3
                        * crate::world::natural::neutral_surface_momentum_transfer_velocity_m_s(
                            norm(relative),
                        );
                    let mut forces = [[0.0_f64; 3]; 3];
                    for component in 0..3 {
                        let mut targets = roles.map(|role| {
                            tendency
                                .layer(role)
                                .expect("surface role")
                                .velocity_tendency_m_s2[cell][component]
                        });
                        let deltas = add_surface_stress_component(
                            &mut targets,
                            masses,
                            weights,
                            conductance * relative[component],
                        );
                        for side in 0..3 {
                            tendency
                                .layer_mut(roles[side])
                                .expect("surface role")
                                .velocity_tendency_m_s2[cell][component] = targets[side];
                            forces[side][component] = masses[side] * deltas[side];
                        }
                    }
                    let area = self.grid.cells()[cell].area_m2();
                    tendency.budget.paired_momentum_absolute_n +=
                        area * 0.5 * forces.into_iter().map(norm).sum::<f64>();
                    tendency.budget.paired_momentum_residual_n += area
                        * norm(std::array::from_fn(|component| {
                            forces.iter().map(|force| force[component]).sum()
                        }));
                    continue;
                }
                let momentum = paired_momentum_exchange(
                    first_velocity[cell].map(f64::from),
                    second_velocity[cell].map(f64::from),
                    first_mass,
                    second_mass,
                    momentum_timescale,
                )?;
                let mut first_momentum_delta = [0.0_f64; 3];
                let mut second_momentum_delta = [0.0_f64; 3];
                for component in 0..3 {
                    let mut first_target = tendency
                        .layer(first_role)
                        .expect("pair role")
                        .velocity_tendency_m_s2[cell][component];
                    let mut second_target = tendency
                        .layer(second_role)
                        .expect("pair role")
                        .velocity_tendency_m_s2[cell][component];
                    (
                        first_momentum_delta[component],
                        second_momentum_delta[component],
                    ) = add_balanced_pair_to_f64(
                        &mut first_target,
                        &mut second_target,
                        water_scale * momentum.first_acceleration_m_s2[component],
                        first_mass,
                        second_mass,
                    );
                    tendency
                        .layer_mut(first_role)
                        .expect("pair role")
                        .velocity_tendency_m_s2[cell][component] = first_target;
                    tendency
                        .layer_mut(second_role)
                        .expect("pair role")
                        .velocity_tendency_m_s2[cell][component] = second_target;
                }
                let first_impulse = first_momentum_delta.map(|value| first_mass * value);
                let second_impulse = second_momentum_delta.map(|value| second_mass * value);
                let area = self.grid.cells()[cell].area_m2();
                tendency.budget.paired_momentum_absolute_n +=
                    area * 0.5 * (norm(first_impulse) + norm(second_impulse));
                tendency.budget.paired_momentum_residual_n += area
                    * norm(std::array::from_fn(|component| {
                        first_impulse[component] + second_impulse[component]
                    }));
            }
        }
        check_cancelled(cancellation)
    }

    fn apply_pair_exchanges(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        month: usize,
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
        include_heat_exchange: bool,
        include_momentum_exchange: bool,
        include_moisture_exchange: bool,
        moisture_step_seconds: f64,
    ) -> Result<(), LayeredTendencyError> {
        if include_momentum_exchange {
            self.apply_pair_momentum_exchanges(state, forcing, cancellation, tendency)?;
        }
        let layout = ClimateLayerLayout::for_profile(state.profile());
        let upper_reference_c = if include_heat_exchange {
            upper_atmosphere_reference_air_temperature_c(
                self.grid,
                forcing,
                self.sea_level_m,
                Some(month),
                Some(cancellation),
            )?
        } else {
            Vec::new()
        };
        for exchange in layout.exchanges().iter().copied().filter(|exchange| {
            exchange.heat_exchange_time_s().is_some()
                && state.temperature_c(exchange.first()).is_some()
                && state.temperature_c(exchange.second()).is_some()
        }) {
            let first_role = exchange.first();
            let second_role = exchange.second();
            let heat_timescale = exchange
                .heat_exchange_time_s()
                .expect("filtered exchange has heat timescale");
            let first_temperature = state.temperature_c(first_role).expect("pair role");
            let second_temperature = state.temperature_c(second_role).expect("pair role");
            let first_spec = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == first_role)
                .expect("pair role belongs to the layout");
            let second_spec = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == second_role)
                .expect("pair role belongs to the layout");
            for cell in 0..self.grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let water_scale = if exchange.water_only() {
                    f64::from(1.0 - forcing.land_fraction()[cell])
                } else {
                    1.0
                };
                if water_scale == 0.0 {
                    continue;
                }
                let area = self.grid.cells()[cell].area_m2();
                if include_heat_exchange {
                    let first_physical_capacity =
                        cell_heat_capacity_per_area(state, first_spec, cell)?;
                    let second_physical_capacity =
                        cell_heat_capacity_per_area(state, second_spec, cell)?;
                    let air_reference = forcing.equilibrium_air_temperature_c()[cell][month];
                    let surface_reference =
                        forcing.equilibrium_surface_temperature_c()[cell][month];
                    let first_reference = role_reference_temperature_c(
                        first_role,
                        air_reference,
                        surface_reference,
                        upper_reference_c[cell],
                    );
                    let second_reference = role_reference_temperature_c(
                        second_role,
                        air_reference,
                        surface_reference,
                        upper_reference_c[cell],
                    );
                    // IMEX Euler exchanges heat at the explicit-source predictor.
                    // Shared layers include earlier pair corrections: the declared
                    // layout order composes conservative pairwise BE steps.
                    let first_predictor = (f64::from(first_temperature[cell])
                        + moisture_step_seconds
                            * f64::from(
                                tendency
                                    .layer(first_role)
                                    .expect("pair role")
                                    .temperature_tendency_k_s[cell],
                            )) as f32;
                    let second_predictor = (f64::from(second_temperature[cell])
                        + moisture_step_seconds
                            * f64::from(
                                tendency
                                    .layer(second_role)
                                    .expect("pair role")
                                    .temperature_tendency_k_s[cell],
                            )) as f32;
                    let heat = equilibrium_anomaly_heat_exchange(
                        first_predictor,
                        first_reference,
                        second_predictor,
                        second_reference,
                        first_physical_capacity,
                        second_physical_capacity,
                        heat_timescale,
                    )?;
                    // Bryan (1984): reduce storage without reducing physical
                    // conductance. The wet fraction belongs to the exchange
                    // rate, so it must also enter the backward-Euler factor.
                    let formation_rate_scale =
                        GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION * water_scale;
                    let implicit = implicit_pair_relaxation_factor(
                        (f64::from(first_predictor) - f64::from(first_reference))
                            - (f64::from(second_predictor) - f64::from(second_reference)),
                        formation_rate_scale * (heat.first_tendency_k_s - heat.second_tendency_k_s),
                        moisture_step_seconds,
                    );
                    let pair_tendencies = [
                        formation_rate_scale * implicit * heat.first_tendency_k_s,
                        formation_rate_scale * implicit * heat.second_tendency_k_s,
                    ];
                    let heat_scale = subsurface_pair_exchange_scale_for_step(
                        [first_role, second_role],
                        [first_temperature[cell], second_temperature[cell]],
                        [
                            tendency
                                .layer(first_role)
                                .expect("pair role")
                                .temperature_tendency_k_s[cell],
                            tendency
                                .layer(second_role)
                                .expect("pair role")
                                .temperature_tendency_k_s[cell],
                        ],
                        pair_tendencies,
                        moisture_step_seconds,
                    );
                    let first_heat_delta = {
                        let target = &mut tendency
                            .layer_mut(first_role)
                            .expect("pair role")
                            .temperature_tendency_k_s[cell];
                        let before = *target;
                        *target += (heat_scale * pair_tendencies[0]) as f32;
                        f64::from(*target) - f64::from(before)
                    };
                    let second_heat_delta = {
                        let target = &mut tendency
                            .layer_mut(second_role)
                            .expect("pair role")
                            .temperature_tendency_k_s[cell];
                        let before = *target;
                        *target += (heat_scale * pair_tendencies[1]) as f32;
                        f64::from(*target) - f64::from(before)
                    };
                    let first_heat = first_physical_capacity * first_heat_delta;
                    let second_heat = second_physical_capacity * second_heat_delta;
                    tendency.budget.paired_heat_absolute_w +=
                        area * 0.5 * (first_heat.abs() + second_heat.abs());
                    tendency.budget.paired_heat_residual_w +=
                        area * (first_heat + second_heat).abs();
                }
            }
        }

        if include_moisture_exchange {
            if let (Some(upper_humidity), Some(_)) = (
                state.upper_specific_humidity(),
                tendency.upper_specific_humidity_tendency_s_inv.as_ref(),
            ) {
                let lower_humidity = state.specific_humidity();
                let lower_mass =
                    moisture_column_mass_per_area(state, ClimateLayerRole::LowerAtmosphere);
                let upper_mass =
                    moisture_column_mass_per_area(state, ClimateLayerRole::UpperAtmosphere);
                let coupling_mass = lower_mass.min(upper_mass);
                let timescale = layout
                    .exchange(
                        ClimateLayerRole::LowerAtmosphere,
                        ClimateLayerRole::UpperAtmosphere,
                    )
                    .and_then(|exchange| exchange.moisture_exchange_time_s())
                    .expect("C2 lower-upper moisture exchange is declared");
                for cell in 0..self.grid.cell_count() {
                    if cell % 256 == 0 {
                        check_cancelled(cancellation)?;
                    }
                    let desired_flux = (f64::from(upper_humidity[cell])
                        - f64::from(lower_humidity[cell]))
                        * coupling_mass
                        / timescale;
                    let lower_after_base = (f64::from(lower_humidity[cell])
                        + moisture_step_seconds
                            * f64::from(tendency.specific_humidity_tendency_s_inv[cell]))
                    .max(0.0);
                    let upper_after_base = (f64::from(upper_humidity[cell])
                        + moisture_step_seconds
                            * f64::from(
                                tendency
                                    .upper_specific_humidity_tendency_s_inv
                                    .as_ref()
                                    .expect("C2 upper moisture")[cell],
                            ))
                    .max(0.0);
                    let maximum_upper_outflow = upper_after_base * upper_mass
                        / moisture_step_seconds
                        * (1.0 - 8.0 * f64::from(f32::EPSILON));
                    let maximum_lower_outflow = lower_after_base * lower_mass
                        / moisture_step_seconds
                        * (1.0 - 8.0 * f64::from(f32::EPSILON));
                    let flux = desired_flux.clamp(-maximum_lower_outflow, maximum_upper_outflow);
                    let lower_delta = {
                        let target = &mut tendency.specific_humidity_tendency_s_inv[cell];
                        let before = *target;
                        *target += (flux / lower_mass) as f32;
                        f64::from(*target) - f64::from(before)
                    };
                    let upper_delta = {
                        let target = &mut tendency
                            .upper_specific_humidity_tendency_s_inv
                            .as_mut()
                            .expect("C2 upper moisture")[cell];
                        let before = *target;
                        *target += (-flux / upper_mass) as f32;
                        f64::from(*target) - f64::from(before)
                    };
                    let area = self.grid.cells()[cell].area_m2();
                    let lower_extensive = lower_mass * lower_delta;
                    let upper_extensive = upper_mass * upper_delta;
                    tendency.budget.paired_moisture_absolute_kg_s +=
                        area * 0.5 * (lower_extensive.abs() + upper_extensive.abs());
                    tendency.budget.paired_moisture_residual_kg_s +=
                        area * (lower_extensive + upper_extensive).abs();
                }
            }
        }

        if include_heat_exchange && state.profile() == ClimateModelProfile::C2LayeredV1 {
            let thermocline = state
                .temperature_c(ClimateLayerRole::OceanThermocline)
                .expect("C2 thermocline");
            let deep = state.deep_ocean_temperature_c().expect("C2 deep reservoir");
            let thermocline_spec = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == ClimateLayerRole::OceanThermocline)
                .expect("C2 thermocline belongs to the layout");
            let deep_spec = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == ClimateLayerRole::DeepOceanReservoir)
                .expect("C2 deep reservoir belongs to the layout");
            let deep_physical_capacity = cell_heat_capacity_per_area(state, deep_spec, 0)?;
            let deep_exchange = layout
                .exchange(
                    ClimateLayerRole::OceanThermocline,
                    ClimateLayerRole::DeepOceanReservoir,
                )
                .expect("C2 thermocline-deep heat exchange is declared");
            let timescale = deep_exchange
                .heat_exchange_time_s()
                .expect("C2 thermocline-deep heat exchange has a timescale");
            for cell in 0..self.grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let thermocline_physical_capacity =
                    cell_heat_capacity_per_area(state, thermocline_spec, cell)?;
                let water_scale = if deep_exchange.water_only() {
                    f64::from(1.0 - forcing.land_fraction()[cell])
                } else {
                    1.0
                };
                if water_scale == 0.0 {
                    continue;
                }
                let air_reference = forcing.equilibrium_air_temperature_c()[cell][month];
                let surface_reference = forcing.equilibrium_surface_temperature_c()[cell][month];
                let thermocline_reference = role_reference_temperature_c(
                    ClimateLayerRole::OceanThermocline,
                    air_reference,
                    surface_reference,
                    air_reference,
                );
                let deep_reference = role_reference_temperature_c(
                    ClimateLayerRole::DeepOceanReservoir,
                    air_reference,
                    surface_reference,
                    air_reference,
                );
                let thermocline_predictor = (f64::from(thermocline[cell])
                    + moisture_step_seconds
                        * f64::from(
                            tendency
                                .layer(ClimateLayerRole::OceanThermocline)
                                .expect("C2 thermocline")
                                .temperature_tendency_k_s[cell],
                        )) as f32;
                let deep_predictor = (f64::from(deep[cell])
                    + moisture_step_seconds
                        * f64::from(
                            tendency
                                .deep_ocean_temperature_tendency_k_s
                                .as_ref()
                                .expect("C2 deep tendency")[cell],
                        )) as f32;
                let exchange = equilibrium_anomaly_heat_exchange(
                    thermocline_predictor,
                    thermocline_reference,
                    deep_predictor,
                    deep_reference,
                    thermocline_physical_capacity,
                    deep_physical_capacity,
                    timescale,
                )?;
                let formation_rate_scale =
                    GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION * water_scale;
                let implicit = implicit_pair_relaxation_factor(
                    (f64::from(thermocline_predictor) - f64::from(thermocline_reference))
                        - (f64::from(deep_predictor) - f64::from(deep_reference)),
                    formation_rate_scale
                        * (exchange.first_tendency_k_s - exchange.second_tendency_k_s),
                    moisture_step_seconds,
                );
                let pair_tendencies = [
                    formation_rate_scale * implicit * exchange.first_tendency_k_s,
                    formation_rate_scale * implicit * exchange.second_tendency_k_s,
                ];
                let heat_scale = subsurface_pair_exchange_scale_for_step(
                    [
                        ClimateLayerRole::OceanThermocline,
                        ClimateLayerRole::DeepOceanReservoir,
                    ],
                    [thermocline[cell], deep[cell]],
                    [
                        tendency
                            .layer(ClimateLayerRole::OceanThermocline)
                            .expect("C2 thermocline")
                            .temperature_tendency_k_s[cell],
                        tendency
                            .deep_ocean_temperature_tendency_k_s
                            .as_ref()
                            .expect("C2 deep tendency")[cell],
                    ],
                    pair_tendencies,
                    moisture_step_seconds,
                );
                let thermocline_delta = {
                    let target = &mut tendency
                        .layer_mut(ClimateLayerRole::OceanThermocline)
                        .expect("C2 thermocline")
                        .temperature_tendency_k_s[cell];
                    let before = *target;
                    *target += (heat_scale * pair_tendencies[0]) as f32;
                    f64::from(*target) - f64::from(before)
                };
                let deep_delta = {
                    let target = &mut tendency
                        .deep_ocean_temperature_tendency_k_s
                        .as_mut()
                        .expect("C2 deep tendency")[cell];
                    let before = *target;
                    *target += (heat_scale * pair_tendencies[1]) as f32;
                    f64::from(*target) - f64::from(before)
                };
                let area = self.grid.cells()[cell].area_m2();
                let thermocline_heat = thermocline_physical_capacity * thermocline_delta;
                let deep_heat = deep_physical_capacity * deep_delta;
                tendency.budget.paired_heat_absolute_w +=
                    area * 0.5 * (thermocline_heat.abs() + deep_heat.abs());
                tendency.budget.paired_heat_residual_w +=
                    area * (thermocline_heat + deep_heat).abs();
            }
        }
        Ok(())
    }

    fn validate_tendency(
        &self,
        tendency: &LayeredClimateTendency,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        for layer in &tendency.active_layers {
            for cell in 0..layer.height_tendency_m_s.len() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                if !layer.height_tendency_m_s[cell].is_finite()
                    || !layer.temperature_tendency_k_s[cell].is_finite()
                    || layer.velocity_tendency_m_s2[cell]
                        .iter()
                        .any(|value| !value.is_finite())
                {
                    return Err(LayeredTendencyError::NonFiniteTendency { role: layer.role });
                }
            }
        }
        for (cell, value) in tendency.specific_humidity_tendency_s_inv.iter().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            if !value.is_finite() {
                return Err(LayeredTendencyError::NonFiniteMoistureTendency);
            }
        }
        if let Some(upper) = &tendency.upper_specific_humidity_tendency_s_inv {
            for (cell, value) in upper.iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                if !value.is_finite() {
                    return Err(LayeredTendencyError::NonFiniteMoistureTendency);
                }
            }
        }
        for (field, values) in [
            ("evaporation_rate_mm_s", &tendency.evaporation_rate_mm_s),
            (
                "land_evapotranspiration_rate_mm_s",
                &tendency.land_evapotranspiration_rate_mm_s,
            ),
            ("precipitation_rate_mm_s", &tendency.precipitation_rate_mm_s),
            (
                "orographic_precipitation_rate_mm_s",
                &tendency.orographic_precipitation_rate_mm_s,
            ),
        ] {
            for (cell, value) in values.iter().copied().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                if !value.is_finite() || value < 0.0 {
                    return Err(LayeredTendencyError::InvalidPhaseChangeFlux {
                        field,
                        cell,
                        found: value,
                    });
                }
            }
        }
        check_cancelled(cancellation)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct AxisymmetricCirculationDiagnostic {
    equator_to_pole_contrast_k: f64,
    eddy_velocity_scale_m_s: f64,
    radius_m: f64,
}

impl AxisymmetricCirculationDiagnostic {
    fn reynolds_stress_zonal_acceleration_m_s2(self, radial: [f64; 3]) -> f64 {
        if self.equator_to_pole_contrast_k <= 0.0 || self.eddy_velocity_scale_m_s <= 0.0 {
            return 0.0;
        }
        let sine_latitude = radial[2].clamp(-1.0, 1.0);
        let cosine_latitude = (radial[0] * radial[0] + radial[1] * radial[1]).sqrt();
        // This is the exact spherical divergence of the Eady-activity-weighted
        //   u'v' = C U_R^2 sin(phi)|sin(phi)| cos(phi)^2.
        // It vanishes at the equator and poles, is regular across both, and
        // its area/lever-arm weighted global axial torque integrates to zero.
        BAROCLINIC_REYNOLDS_STRESS_EFFICIENCY
            * self.eddy_velocity_scale_m_s
            * self.eddy_velocity_scale_m_s
            / self.radius_m
            * 2.0
            * sine_latitude.abs()
            * cosine_latitude
            * (3.0 * sine_latitude * sine_latitude - 1.0)
    }
}

fn diagnose_axisymmetric_circulation(
    grid: &CubedSphereGrid,
    equilibrium_air_temperature_c: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: &BuildCancellation,
) -> Result<AxisymmetricCirculationDiagnostic, LayeredTendencyError> {
    debug_assert_eq!(equilibrium_air_temperature_c.len(), grid.cell_count());
    let mut weight = 0.0_f64;
    let mut weighted_latitude_mode = 0.0_f64;
    let mut weighted_temperature_k = 0.0_f64;
    let mut weighted_mode_square = 0.0_f64;
    let mut weighted_mode_temperature = 0.0_f64;
    for (cell, (geometry, temperature)) in grid
        .cells()
        .iter()
        .zip(equilibrium_air_temperature_c)
        .enumerate()
    {
        if cell % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        let area = geometry.area_m2();
        let latitude_mode = geometry.center_unit()[2].powi(2);
        // Synoptic eddy momentum transport has multi-month memory that the
        // accelerated one-macro-step-per-month formation procedure does not
        // resolve. Diagnose its stationary background from the exact annual
        // mean thermal forcing; resolved monthly pressure/radiative terms
        // still own the seasonal response.
        let temperature_k = temperature
            .iter()
            .map(|value| f64::from(*value) + 273.15)
            .sum::<f64>()
            / CLIMATE_MONTH_COUNT as f64;
        weight += area;
        weighted_latitude_mode += area * latitude_mode;
        weighted_temperature_k += area * temperature_k;
        weighted_mode_square += area * latitude_mode * latitude_mode;
        weighted_mode_temperature += area * latitude_mode * temperature_k;
    }
    check_cancelled(cancellation)?;
    let reference_temperature_k = weighted_temperature_k / weight;
    let centered_mode_square =
        weighted_mode_square - weighted_latitude_mode * weighted_latitude_mode / weight;
    let centered_mode_temperature =
        weighted_mode_temperature - weighted_latitude_mode * weighted_temperature_k / weight;
    let fitted_slope_k = if centered_mode_square > 0.0 {
        centered_mode_temperature / centered_mode_square
    } else {
        0.0
    };
    let equator_to_pole_contrast_k = if fitted_slope_k < -1.0e-9 {
        -fitted_slope_k
    } else {
        0.0
    };
    let eddy_velocity_scale_m_s =
        (STANDARD_GRAVITY_M_S2 * ATMOSPHERE_COLUMN_DEPTH_M * equator_to_pole_contrast_k
            / reference_temperature_k)
            .sqrt()
            .min(GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S);
    Ok(AxisymmetricCirculationDiagnostic {
        equator_to_pole_contrast_k,
        eddy_velocity_scale_m_s,
        radius_m: grid.radius_m(),
    })
}

fn apply_baroclinic_reynolds_stress_closure(
    system: &LayeredTendencySystem<'_>,
    state: &LayeredClimateState,
    forcing: &PlanetForcing,
    tendency: &mut LayeredClimateTendency,
    raw_zonal_acceleration_m_s2: &mut [f32],
    cancellation: &BuildCancellation,
) -> Result<(), LayeredTendencyError> {
    let grid = system.grid;
    let diagnostic = diagnose_axisymmetric_circulation(
        grid,
        forcing.equilibrium_air_temperature_c(),
        cancellation,
    )?;
    for role in state
        .active_roles()
        .iter()
        .copied()
        .filter(|role| is_atmosphere_role(*role))
    {
        let reference_mass_per_area = mass_per_area(state, role);
        let mut axial_torque_rate_n_m = 0.0_f64;
        let mut correction_inertia_kg_m2 = 0.0_f64;
        for (cell, raw_zonal_acceleration) in raw_zonal_acceleration_m_s2.iter_mut().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let radial = grid.cells()[cell].center_unit();
            let cosine_latitude = (radial[0] * radial[0] + radial[1] * radial[1]).sqrt();
            let raw = if state.profile() == ClimateModelProfile::C2LayeredV1 {
                crate::world::natural::observed_transient_eddy_acceleration_m_s2(
                    radial[2].clamp(-1.0, 1.0).asin(),
                    grid.radius_m(),
                ) * (diagnostic.eddy_velocity_scale_m_s
                    / GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S)
                    .powi(2)
            } else {
                diagnostic.reynolds_stress_zonal_acceleration_m_s2(radial)
            };
            *raw_zonal_acceleration = raw as f32;
            let area_mass = grid.cells()[cell].area_m2()
                * system.momentum_mass_per_area(state, role, reference_mass_per_area, cell)?;
            let axial_lever_arm_m = grid.radius_m() * cosine_latitude;
            axial_torque_rate_n_m +=
                area_mass * axial_lever_arm_m * f64::from(*raw_zonal_acceleration);
            correction_inertia_kg_m2 += area_mass * axial_lever_arm_m * cosine_latitude;
        }
        let uniform_angular_acceleration = if correction_inertia_kg_m2 > 0.0 {
            axial_torque_rate_n_m / correction_inertia_kg_m2
        } else {
            0.0
        };
        let layer = tendency
            .layer_mut(role)
            .expect("active atmosphere tendency");
        for (cell, raw_zonal_acceleration) in
            raw_zonal_acceleration_m_s2.iter().copied().enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let radial = grid.cells()[cell].center_unit();
            let cosine_latitude = (radial[0] * radial[0] + radial[1] * radial[1]).sqrt();
            if cosine_latitude <= f64::EPSILON.sqrt() {
                continue;
            }
            let east = [
                -radial[1] / cosine_latitude,
                radial[0] / cosine_latitude,
                0.0,
            ];
            let zonal_acceleration =
                f64::from(raw_zonal_acceleration) - uniform_angular_acceleration * cosine_latitude;
            for (component, east_component) in east.into_iter().enumerate() {
                layer.velocity_tendency_m_s2[cell][component] +=
                    zonal_acceleration * east_component;
            }
        }
    }
    check_cancelled(cancellation)
}

/// Bounds the frozen C2 pressure wave speed from positive layer depths and
/// buoyancy bounds. Returns m/s, or the cell's depth/stratification error.
/// `upper_buoyancy_lower_bound` omits the nonnegative terrain lapse.
pub(super) fn atmospheric_fast_mode_speed_m_s(
    lower_depth: f64,
    upper_depth: f64,
    buoyancy_difference: f64,
    upper_buoyancy_lower_bound: f64,
    cell: usize,
) -> Result<f64, LayeredTendencyError> {
    let profile = ClimateModelProfile::C2LayeredV1;
    let internal_gravity = role_constants(profile, ClimateLayerRole::LowerAtmosphere).0
        + role_constants(profile, ClimateLayerRole::UpperAtmosphere).0;
    // With positive surface/internal gravity, c_plus^2 <= trace(G diag(H)).
    // Omitting the nonnegative lapse gives a lower bound on upper buoyancy;
    // omitting terrain gives an upper bound on depth. Both increase the trace.
    // The pressure operator rejects either nonpositive physical gravity.
    for (role, depth) in [
        (ClimateLayerRole::LowerAtmosphere, lower_depth),
        (ClimateLayerRole::UpperAtmosphere, upper_depth),
    ] {
        if !depth.is_finite() || depth <= 0.0 {
            return Err(LayeredTendencyError::InvalidFluidThickness {
                role,
                cell,
                found: depth,
            });
        }
    }
    let surface_gravity = STANDARD_GRAVITY_M_S2 - upper_buoyancy_lower_bound;
    if !surface_gravity.is_finite() || surface_gravity <= 0.0 {
        return Err(LayeredTendencyError::UnstableAtmosphericFreeSurface {
            cell,
            effective_gravity_m_s2: surface_gravity,
        });
    }
    let reduced_gravity = internal_gravity - buoyancy_difference;
    if !reduced_gravity.is_finite() || reduced_gravity <= 0.0 {
        return Err(LayeredTendencyError::UnstableAtmosphericStratification {
            cell,
            reduced_gravity_m_s2: reduced_gravity,
        });
    }
    let trace_bound = surface_gravity * (lower_depth + upper_depth) + reduced_gravity * lower_depth;
    if !trace_bound.is_finite() || trace_bound <= 0.0 {
        return Err(LayeredTendencyError::InvalidAtmosphericWaveMode {
            cell,
            squared_speed_bound_m2_s2: trace_bound,
        });
    }
    Ok(trace_bound.sqrt())
}

fn role_constants(profile: ClimateModelProfile, role: ClimateLayerRole) -> (f64, f64, f64, f64) {
    match role {
        ClimateLayerRole::LowerAtmosphere => (
            0.31,
            // C2 integrates friction against both atmosphere basis functions
            // in apply_land_surface_drag; C1 retains its original drag.
            if profile == ClimateModelProfile::C2LayeredV1 {
                0.0
            } else {
                1.0 / SECONDS_PER_DAY
            },
            7.0 * SECONDS_PER_DAY,
            // C2 thermal pressure is integrated from actual layer depths by
            // apply_atmospheric_thermal_pressure, with no fixed coefficient.
            if profile == ClimateModelProfile::C2LayeredV1 {
                0.0
            } else {
                C1_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K
            },
        ),
        ClimateLayerRole::UpperAtmosphere => (0.45, 0.0, 12.0 * SECONDS_PER_DAY, 0.0),
        ClimateLayerRole::OceanMixedLayer => (
            // This prognostic height is published as sea-surface height, so its
            // fast pressure mode is a free-surface mode and uses full gravity.
            STANDARD_GRAVITY_M_S2,
            1.0 / (30.0 * SECONDS_PER_DAY),
            90.0 * SECONDS_PER_DAY,
            // Depth-mean hydrostatic Boussinesq coupling 1/2 g * alpha * H.
            // The positive sign makes a warm, expanded water column build a
            // higher steric free surface.
            MIXED_LAYER_STERIC_ACCELERATION_M2_S2_K,
        ),
        ClimateLayerRole::OceanThermocline => (
            0.012,
            1.0 / (180.0 * SECONDS_PER_DAY),
            365.25 * SECONDS_PER_DAY,
            // Fixed reduced gravity already closes this internal-interface
            // pressure response. Reapplying the surface-temperature gradient
            // here would double count the same baroclinic forcing.
            0.0,
        ),
        ClimateLayerRole::DeepOceanReservoir => unreachable!(),
    }
}

/// Finite relaxation of the resolved axisymmetric interface (A5 §7.40).
///
/// Battisti, Sarachik & Hirst (1999), Eqs. (10), (16)-(22), retain the
/// interface back pressure while moisture selects its venting time. Project
/// the interface first, then the moisture-dependent rate: P epsilon P zeta.
/// This retains the declared axisymmetric subspace, not P(epsilon zeta).
fn close_axisymmetric_baroclinic_thickness(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    workspace: &mut LayeredTendencyWorkspace,
    tendency: &mut LayeredClimateTendency,
) {
    let lower = ClimateLayerRole::LowerAtmosphere;
    let upper = ClimateLayerRole::UpperAtmosphere;
    let lower_thickness = f64::from(
        state
            .reference_thickness_m(lower)
            .expect("C2 lower atmosphere"),
    );
    let upper_thickness = f64::from(
        state
            .reference_thickness_m(upper)
            .expect("C2 upper atmosphere"),
    );
    let lower_fraction = lower_thickness / (lower_thickness + upper_thickness);
    let lower_height = state.height_anomaly_m(lower).expect("C2 lower atmosphere");
    let upper_height = state.height_anomaly_m(upper).expect("C2 upper atmosphere");
    let moisture_mass = moisture_column_mass_per_area(state, lower);
    workspace.band_area_m2.fill(0.0);
    workspace.band_exchange_m3_s.fill(0.0);
    // Horizontal H tendencies have already been copied into the final layers.
    // This f64 scratch is free until the subsequent fast ocean operator.
    let band_rate_area = &mut workspace.thickness_tendency_m_s[..workspace.band_area_m2.len()];
    band_rate_area.fill(0.0);
    for (cell, geometry) in grid.cells().iter().enumerate() {
        let interface = (1.0 - lower_fraction) * f64::from(lower_height[cell])
            - lower_fraction * f64::from(upper_height[cell]);
        // Remove the retained local E-P tendency, then add E. This is the
        // steady moisture supply, including horizontal transport and vertical
        // mixing, without cancelling a large precipitation sink against qdot.
        // Actual water storage remains in the prognostic humidity equation.
        let steady_moisture_supply = f64::from(tendency.evaporation_rate_mm_s[cell])
            + moisture_mass
                * (f64::from(tendency.specific_humidity_tendency_s_inv[cell])
                    - tendency.external_moisture_tendency_s_inv[cell]);
        let timescale = if steady_moisture_supply > 0.0 {
            BOUNDARY_LAYER_CONVECTIVE_VENTING_SECONDS
        } else {
            BOUNDARY_LAYER_DRY_VENTING_SECONDS
        };
        let band = workspace.axisymmetric_band[cell] as usize;
        let area = geometry.area_m2();
        workspace.band_area_m2[band] += area;
        workspace.band_exchange_m3_s[band] += area * interface;
        band_rate_area[band] += area / timescale;
    }
    for (band, &rate_area) in band_rate_area.iter().enumerate() {
        let area = workspace.band_area_m2[band];
        if area > 0.0 {
            workspace.band_exchange_m3_s[band] *= rate_area / area;
        }
    }
    let exchange: Vec<f64> = workspace
        .axisymmetric_band
        .iter()
        .map(|&band| {
            workspace.band_exchange_m3_s[band as usize] / workspace.band_area_m2[band as usize]
        })
        .collect();
    // Preserve the f64 declaration for the stage-wise donor generator and
    // its exact local reference; rounded height differences never redefine Q.
    tendency.overturning_exchange_m_s = Some(exchange);
}

fn conservative_layer_thickness_tendency(
    grid: &CubedSphereGrid,
    reference_thickness_m: f64,
    terrain_floor_m: Option<&[f32]>,
    height_anomaly_m: &[f32],
    velocity_m_s: &[[f32; 3]],
    edge_permeability: &[f32],
    target_m_s: &mut [f64],
    cancellation: &BuildCancellation,
) -> Result<(), LayeredTendencyError> {
    debug_assert_eq!(height_anomaly_m.len(), grid.cell_count());
    debug_assert_eq!(velocity_m_s.len(), grid.cell_count());
    debug_assert_eq!(edge_permeability.len(), grid.edges().len());
    debug_assert_eq!(target_m_s.len(), grid.cell_count());
    target_m_s.fill(0.0);
    for (edge_index, (edge, permeability)) in grid.edges().iter().zip(edge_permeability).enumerate()
    {
        if edge_index % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        if *permeability <= 0.0 {
            continue;
        }
        let [first, second] = *edge.cells();
        let first = first as usize;
        let second = second as usize;
        let amount_rate_m3_s = donor_layer_edge_amount_rate_m3_s(
            edge,
            *permeability,
            LayerTransportFields {
                velocity_m_s,
                height_anomaly_m,
                reference_thickness_m,
                terrain_floor_m,
            },
        );
        target_m_s[first] -= amount_rate_m3_s / grid.cells()[first].area_m2();
        target_m_s[second] += amount_rate_m3_s / grid.cells()[second].area_m2();
    }
    check_cancelled(cancellation)
}

/// Adds the conservative two-point-flux horizontal diffusion of one scalar
/// field to an existing tendency.
///
/// This uses the C1 geometric conductance of
/// [`LayeredTendencySystem::horizontal_velocity_diffusion`]: the same
/// finite-volume conductance `D * perm * L_edge / d_centers`, so the same
/// resolution independence, and no parallel transport because a scalar has no
/// direction to carry. Every edge moves one amount out of one cell and into
/// the other, so the area-weighted global integral is unchanged and the term
/// is a transport, never a source.
///
/// # Arguments
///
/// * `values` - the diffused field, one sample per grid cell
/// * `edge_permeability` - per-edge open fraction, `0.0` for a closed edge
/// * `diffusivity_m2_s` - the eddy diffusivity, finite and non-negative
/// * `target_tendency_s_inv` - accumulated into, never cleared
fn accumulate_horizontal_scalar_diffusion(
    grid: &CubedSphereGrid,
    values: &[f32],
    edge_permeability: &[f32],
    diffusivity_m2_s: f64,
    target_tendency_s_inv: &mut [f32],
    cancellation: &BuildCancellation,
) -> Result<(), LayeredTendencyError> {
    debug_assert_eq!(values.len(), grid.cell_count());
    debug_assert_eq!(edge_permeability.len(), grid.edges().len());
    debug_assert_eq!(target_tendency_s_inv.len(), grid.cell_count());
    debug_assert!(diffusivity_m2_s.is_finite() && diffusivity_m2_s >= 0.0);
    if diffusivity_m2_s == 0.0 {
        return Ok(());
    }

    for (edge_index, edge) in grid.edges().iter().enumerate() {
        if edge_index % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        let permeability = f64::from(edge_permeability[edge_index]);
        if permeability == 0.0 {
            continue;
        }
        let [first, second] = *edge.cells();
        let first = first as usize;
        let second = second as usize;
        let conductance_m2_s =
            diffusivity_m2_s * edge.midpoint_unit()[2].abs() * permeability * edge.length_m()
                / edge.center_distance_m();
        let amount_rate = conductance_m2_s * (f64::from(values[second]) - f64::from(values[first]));
        target_tendency_s_inv[first] += (amount_rate / grid.cells()[first].area_m2()) as f32;
        target_tendency_s_inv[second] -= (amount_rate / grid.cells()[second].area_m2()) as f32;
    }
    check_cancelled(cancellation)
}

fn parallel_transport_tangent(
    tangent_vector: [f64; 3],
    from_radial: [f64; 3],
    to_radial: [f64; 3],
) -> [f64; 3] {
    let denominator = 1.0 + dot(from_radial, to_radial);
    debug_assert!(denominator > 0.0);
    let correction = dot(tangent_vector, to_radial) / denominator;
    [
        tangent_vector[0] - correction * (from_radial[0] + to_radial[0]),
        tangent_vector[1] - correction * (from_radial[1] + to_radial[1]),
        tangent_vector[2] - correction * (from_radial[2] + to_radial[2]),
    ]
}

fn is_atmosphere_role(role: ClimateLayerRole) -> bool {
    matches!(
        role,
        ClimateLayerRole::LowerAtmosphere | ClimateLayerRole::UpperAtmosphere
    )
}

fn atmospheric_thermal_buoyancy_m_s2(temperature_anomaly_c: f64) -> f64 {
    STANDARD_GRAVITY_M_S2 * temperature_anomaly_c
        / (crate::world::natural::STANDARD_ATMOSPHERE_SEA_LEVEL_TEMPERATURE_C + 273.15)
}

/// Returns upper buoyancy in m/s² before the nonnegative terrain correction.
/// `cell` must index the validated C2 state; the value is a CFL lower bound.
pub(super) fn atmospheric_unlapsed_upper_buoyancy_m_s2(
    state: &LayeredClimateState,
    cell: usize,
) -> f64 {
    atmospheric_thermal_buoyancy_m_s2(
        f64::from(
            state
                .temperature_c(ClimateLayerRole::UpperAtmosphere)
                .expect("C2 upper")[cell],
        ) + f64::from(UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C)
            - crate::world::natural::STANDARD_ATMOSPHERE_SEA_LEVEL_TEMPERATURE_C,
    )
}

pub(super) fn atmospheric_thermal_buoyancy_difference_m_s2(
    state: &LayeredClimateState,
    cell: usize,
) -> f64 {
    let lower = ClimateLayerRole::LowerAtmosphere;
    let upper = ClimateLayerRole::UpperAtmosphere;
    // The shared terrain lapse correction cancels between the two layers.
    atmospheric_thermal_buoyancy_m_s2(
        f64::from(state.temperature_c(lower).expect("C2 lower layer")[cell])
            - f64::from(state.temperature_c(upper).expect("C2 upper layer")[cell])
            - f64::from(UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C),
    )
}

fn mass_per_area(state: &LayeredClimateState, role: ClimateLayerRole) -> f64 {
    let layout = ClimateLayerLayout::for_profile(state.profile());
    let layer = layout
        .layers()
        .iter()
        .find(|layer| layer.role() == role)
        .expect("active role belongs to the profile layout");
    layer.density_kg_m3() * f64::from(state.reference_thickness_m(role).expect("active role"))
}

/// Heat capacity that the formation's compressed clock presents to a local
/// thermodynamic relaxation (design 2026-09-03 A4 §6.2; Bryan 1984 distorted
/// physics).
///
/// Radiation uses this effective capacity. Pair relaxation keeps its physical
/// conductance and applies the same compression to its rate separately;
/// scaling both capacities inside the conductance would cancel the speedup.
/// Transport, momentum, moisture and their latent heat keep physical rates
/// under the frozen A4 prior/transport division.
fn formation_thermal_heat_capacity_per_area(
    state: &LayeredClimateState,
    layer: &ClimateLayerSpec,
    cell: usize,
) -> Result<f64, LayeredTendencyError> {
    Ok(cell_heat_capacity_per_area(state, layer, cell)?
        / GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION)
}

/// Physical storage capacity of a work cell. Moving C2 ocean columns use
/// their validated actual thickness; A4 atmospheric storage and the inactive
/// deep reservoir retain their reference-column definitions.
/// Returns J/(m2 K) for the supplied layout layer and cell, or the existing
/// thickness error if an active ocean column is nonfinite or nonpositive.
pub(super) fn cell_heat_capacity_per_area(
    state: &LayeredClimateState,
    layer: &ClimateLayerSpec,
    cell: usize,
) -> Result<f64, LayeredTendencyError> {
    let role = layer.role();
    let thickness = if state.profile() == ClimateModelProfile::C2LayeredV1
        && matches!(
            role,
            ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline
        ) {
        LayeredTendencySystem::validated_fluid_layer_thickness_m(
            f64::from(
                state
                    .reference_thickness_m(role)
                    .expect("active ocean layer"),
            ),
            state.height_anomaly_m(role).expect("active ocean layer")[cell],
            0.0,
            role,
            cell,
        )?
    } else {
        layer.reference_thickness_m()
    };
    Ok(layer.density_kg_m3() * thickness * layer.heat_capacity_j_kg_k())
}

/// Closed-form factor that makes one forward application of a linear pair
/// relaxation equal one backward-Euler step of the same relaxation.
///
/// The compressed clock puts several exchange rates above the macro step,
/// where an explicit application would ring or diverge. The rate is read back
/// from the returned tendencies, so the factor needs no knowledge of the pair
/// formula, and because the same factor multiplies both sides it cannot
/// disturb the equal-and-opposite extensive budget.
fn implicit_pair_relaxation_factor(
    anomaly_difference_k: f64,
    difference_rate_k_s: f64,
    step_seconds: f64,
) -> f64 {
    if anomaly_difference_k == 0.0 {
        return 1.0;
    }
    let rate_per_s = (-difference_rate_k_s / anomaly_difference_k).max(0.0);
    1.0 / (1.0 + step_seconds * rate_per_s)
}

/// Water-vapour column mass of one layer (design 2026-09-03 A5 Task 1).
///
/// Every conversion between the prognostic mixing ratio and a water mass uses
/// this, never the dry-air column mass: the layer's humidity is a
/// near-surface value while its dry-air mass spans the whole slab, and water
/// is concentrated in the lowest couple of kilometres.
fn moisture_column_mass_per_area(state: &LayeredClimateState, role: ClimateLayerRole) -> f64 {
    ClimateLayerLayout::for_profile(state.profile()).moisture_column_mass_per_area(role)
}

fn tangentize(vector: [f64; 3], radial: [f64; 3]) -> [f64; 3] {
    let radial_component = dot(vector, radial);
    [
        vector[0] - radial_component * radial[0],
        vector[1] - radial_component * radial[1],
        vector[2] - radial_component * radial[2],
    ]
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn check_cancelled(cancellation: &BuildCancellation) -> Result<(), LayeredTendencyError> {
    if cancellation.is_cancelled() {
        Err(LayeredTendencyError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum LayeredTendencyError {
    /// The frozen pressure matrix did not yield a finite positive wave bound.
    #[error("atmospheric cell {cell} has invalid squared wave-speed bound {squared_speed_bound_m2_s2} m2/s2")]
    InvalidAtmosphericWaveMode {
        cell: usize,
        squared_speed_bound_m2_s2: f64,
    },
    /// Thermal expansion made the fixed-top free-surface pressure unsupported.
    #[error("atmospheric cell {cell} has nonpositive effective surface gravity {effective_gravity_m_s2} m/s2")]
    UnstableAtmosphericFreeSurface {
        cell: usize,
        effective_gravity_m_s2: f64,
    },
    #[error("atmospheric cell {cell} has nonpositive internal reduced gravity {reduced_gravity_m_s2} m/s2")]
    UnstableAtmosphericStratification {
        cell: usize,
        reduced_gravity_m_s2: f64,
    },
    #[error("{role:?} cell {cell} has invalid available fluid thickness {found} m")]
    InvalidFluidThickness {
        role: ClimateLayerRole,
        cell: usize,
        found: f64,
    },
    #[error("layered tendency evaluation was cancelled")]
    Cancelled,
    #[error(transparent)]
    State(#[from] LayeredStateError),
    #[error(transparent)]
    Operator(CirculationOperatorError),
    #[error("invalid planet forcing: {reason}")]
    InvalidForcing { reason: String },
    #[error("layered state or forcing grid does not match the tendency system")]
    GridMismatch,
    #[error("terrain gradient has {found} cells, expected {expected}")]
    TerrainGradientLengthMismatch { found: usize, expected: usize },
    #[error("terrain gradient cell {cell} contains a non-finite component")]
    InvalidTerrainGradient { cell: usize },
    #[error("month {found} is outside the 12-month climatology")]
    InvalidMonth { found: usize },
    #[error("transport integration horizon {found} seconds must be finite and positive")]
    InvalidTransportStep { found: f64 },
    #[error("ocean permeability has {found} edges, expected {expected}")]
    PermeabilityLengthMismatch { found: usize, expected: usize },
    #[error("ocean permeability edge {edge} is invalid: {found}")]
    InvalidPermeability { edge: usize, found: f32 },
    #[error("tendency workspace belongs to a different grid")]
    WorkspaceGridMismatch,
    #[error("exchange {field} is invalid: {found}")]
    InvalidExchangeValue { field: &'static str, found: f64 },
    #[error("exchange heat capacity, mass, and timescale must be positive")]
    NonPositiveExchangeScale,
    #[error("exchange velocity contains a non-finite component")]
    InvalidExchangeVector,
    #[error("{role:?} produced a non-finite tendency")]
    NonFiniteTendency { role: ClimateLayerRole },
    #[error("moisture produced a non-finite tendency")]
    NonFiniteMoistureTendency,
    #[error("phase-change flux {field}[{cell}] is invalid: {found}")]
    InvalidPhaseChangeFlux {
        field: &'static str,
        cell: usize,
        found: f32,
    },
    #[error(
        "radiative heating at [{cell}][{month}] retained {retained_w_m2} W/m2 above absorbed shortwave {absorbed_w_m2} W/m2"
    )]
    RadiativeHeatingExceedsAbsorbedShortwave {
        cell: usize,
        month: usize,
        retained_w_m2: f64,
        absorbed_w_m2: f64,
    },
}

impl From<CirculationOperatorError> for LayeredTendencyError {
    fn from(error: CirculationOperatorError) -> Self {
        if error == CirculationOperatorError::Cancelled {
            Self::Cancelled
        } else {
            Self::Operator(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        accumulate_horizontal_scalar_diffusion, apply_baroclinic_reynolds_stress_closure,
        atmospheric_thermal_buoyancy_difference_m_s2, atmospheric_thermal_buoyancy_m_s2,
        axisymmetric_band_count, axisymmetric_bands, close_axisymmetric_baroclinic_thickness,
        conservative_layer_thickness_tendency, diagnose_axisymmetric_circulation, dot,
        equilibrium_anomaly_heat_exchange, next_f32_down, role_constants,
        subsurface_pair_exchange_scale_for_step, tangentize, LayeredClimateTendency,
        LayeredTendencySystem, LayeredTendencyWorkspace,
        ATMOSPHERE_HORIZONTAL_EDDY_MOISTURE_DIFFUSIVITY_M2_S, SUBSURFACE_OCEAN_MIN_C,
        UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C,
    };

    #[test]
    fn pair_heat_responds_to_the_pending_nonpair_temperature_endpoint() {
        // IMEX Euler applies the implicit exchange to the explicit-source
        // predictor. Compare that composition with the existing isolated
        // pair operator; an equilibrium initial state makes a stale-state
        // exchange exactly zero. C1 and the C2 reservoir use separate paths.
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let step = super::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS;
        let source_rate = (1.0 / step) as f32;
        let cancellation = BuildCancellation::new();
        let system = LayeredTendencySystem::new(&grid);
        for profile in [
            ClimateModelProfile::C1SingleLayerV1,
            ClimateModelProfile::C2LayeredV1,
        ] {
            let layout = ClimateLayerLayout::for_profile(profile);
            let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
            let mut predictor = state.clone();
            let mut actual = LayeredClimateTendency::zeroed(&state);
            let deep = profile == ClimateModelProfile::C2LayeredV1;
            let (predicted_temperature, source) = if deep {
                (
                    predictor.deep_ocean_temperature_c_mut().unwrap(),
                    actual.deep_ocean_temperature_tendency_k_s.as_mut().unwrap(),
                )
            } else {
                (
                    predictor
                        .temperature_c_mut(ClimateLayerRole::LowerAtmosphere)
                        .unwrap(),
                    &mut actual
                        .layer_mut(ClimateLayerRole::LowerAtmosphere)
                        .unwrap()
                        .temperature_tendency_k_s,
                )
            };
            for (temperature, rate) in predicted_temperature.iter_mut().zip(source) {
                *rate = source_rate;
                *temperature = (f64::from(*temperature) + step * f64::from(source_rate)) as f32;
            }
            let mut expected = LayeredClimateTendency::zeroed(&predictor);
            for (input, tendency) in [(&predictor, &mut expected), (&state, &mut actual)] {
                system
                    .apply_pair_exchanges(
                        input,
                        &forcing,
                        0,
                        &cancellation,
                        tendency,
                        true,
                        false,
                        false,
                        step,
                    )
                    .unwrap();
            }
            let receiver = if deep {
                ClimateLayerRole::OceanThermocline
            } else {
                ClimateLayerRole::OceanMixedLayer
            };
            let expected = f64::from(expected.layer(receiver).unwrap().temperature_tendency_k_s[0]);
            let actual = f64::from(actual.layer(receiver).unwrap().temperature_tendency_k_s[0]);
            assert!(expected > 0.0, "predictor must heat the receiving layer");
            assert!(
                (actual - expected).abs() <= expected.abs() * 1.0e-6,
                "{profile:?}: pending-source pair={actual}, predictor pair={expected}"
            );
        }
    }

    #[test]
    fn formation_pair_heat_matches_the_compressed_physical_backward_euler_step() {
        // Exercise the composition that previously scaled both storage and
        // conductance, cancelling the declared acceleration. Isolated
        // receivers cover atmosphere, partial-water and moving-ocean paths;
        // a full formation solve cannot give a more specific regression signal.
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.75; count],
            vec![0.25; count],
            vec![0.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let step = super::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS;
        let compression = super::GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION;
        for (first, second) in [
            (
                ClimateLayerRole::LowerAtmosphere,
                ClimateLayerRole::UpperAtmosphere,
            ),
            (
                ClimateLayerRole::LowerAtmosphere,
                ClimateLayerRole::OceanMixedLayer,
            ),
            (
                ClimateLayerRole::OceanMixedLayer,
                ClimateLayerRole::OceanThermocline,
            ),
            (
                ClimateLayerRole::OceanThermocline,
                ClimateLayerRole::DeepOceanReservoir,
            ),
        ] {
            // C1 isolates the air/mixed-layer pair; in C2 its lower-air
            // receiver participates in the following atmosphere pair too.
            let profile = if second == ClimateLayerRole::OceanMixedLayer {
                ClimateModelProfile::C1SingleLayerV1
            } else {
                ClimateModelProfile::C2LayeredV1
            };
            let layout = ClimateLayerLayout::for_profile(profile);
            let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
            let deep = second == ClimateLayerRole::DeepOceanReservoir;
            if matches!(
                first,
                ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline
            ) {
                let extra_depth = state.reference_thickness_m(first).unwrap();
                state.height_anomaly_m_mut(first).unwrap().fill(extra_depth);
            }
            let warmed = if deep {
                state.deep_ocean_temperature_c_mut().unwrap()
            } else {
                state.temperature_c_mut(second).unwrap()
            };
            for value in warmed {
                *value += 2.0;
            }
            let exchange = layout.exchange(first, second).unwrap();
            let first_spec = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == first)
                .unwrap();
            let second_spec = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == second)
                .unwrap();
            let physical = super::paired_heat_exchange(
                0.0,
                2.0,
                super::cell_heat_capacity_per_area(&state, first_spec, 0).unwrap(),
                super::cell_heat_capacity_per_area(&state, second_spec, 0).unwrap(),
                exchange.heat_exchange_time_s().unwrap(),
            )
            .unwrap();
            // Scaling the physical integration interval also scales the wet
            // fraction of the conductance, including its backward-Euler rate.
            let wet_fraction = if exchange.water_only() { 0.25 } else { 1.0 };
            let physical_step = step * compression * wet_fraction;
            let implicit = super::implicit_pair_relaxation_factor(
                -2.0,
                physical.first_tendency_k_s() - physical.second_tendency_k_s(),
                physical_step,
            );
            let expected_delta = physical_step
                * implicit
                * if deep {
                    physical.second_tendency_k_s()
                } else {
                    physical.first_tendency_k_s()
                };
            let mut tendency = LayeredClimateTendency::zeroed(&state);
            LayeredTendencySystem::new(&grid)
                .apply_pair_exchanges(
                    &state,
                    &forcing,
                    0,
                    &BuildCancellation::new(),
                    &mut tendency,
                    true,
                    false,
                    false,
                    step,
                )
                .unwrap();
            let retained = if deep {
                tendency
                    .deep_ocean_temperature_tendency_k_s
                    .as_ref()
                    .unwrap()[0]
            } else {
                tendency.layer(first).unwrap().temperature_tendency_k_s[0]
            };
            let actual_delta = step * f64::from(retained);
            assert!(
                (actual_delta - expected_delta).abs() <= expected_delta.abs() * 1.0e-6,
                "{first:?}/{second:?}: actual={actual_delta}, expected={expected_delta}"
            );
        }
    }

    #[test]
    fn deep_heat_exchange_belongs_only_to_thermodynamic_endpoint_modes() {
        // The explicit reference integrator applies scalar endpoints before
        // its smooth RK stages. A single uniform column isolates the deep
        // pair, catching repeated exchange without running an integrator.
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.75; count],
            vec![0.25; count],
            vec![0.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        for temperature in state.deep_ocean_temperature_c_mut().unwrap() {
            *temperature += 2.0;
        }
        let system = LayeredTendencySystem::new(&grid);
        let permeability = vec![1.0; grid.edges().len()];
        let cancellation = BuildCancellation::new();
        let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
        for mode in [
            super::TendencyEvaluationMode::FullEndpoint,
            super::TendencyEvaluationMode::ThermodynamicMoistureEndpoint,
            super::TendencyEvaluationMode::SmoothDynamics,
        ] {
            let tendency = system
                .evaluate_with_workspace_mode(
                    &state,
                    &forcing,
                    &permeability,
                    0,
                    &cancellation,
                    &mut workspace,
                    mode,
                    super::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
                )
                .unwrap();
            let thermocline = tendency
                .temperature_tendency_k_s(ClimateLayerRole::OceanThermocline)
                .unwrap()[0];
            let deep = tendency.deep_ocean_temperature_tendency_k_s().unwrap()[0];
            if matches!(mode, super::TendencyEvaluationMode::SmoothDynamics) {
                assert_eq!(
                    thermocline, 0.0,
                    "smooth dynamics repeats deep heat exchange"
                );
                assert_eq!(deep, 0.0, "smooth dynamics repeats deep heat exchange");
            } else {
                assert!(thermocline > 0.0, "{mode:?} must retain deep heat exchange");
                assert!(deep < 0.0, "{mode:?} must retain deep heat exchange");
            }
        }
    }

    #[test]
    fn declared_reference_stratification_has_zero_internal_heat_flux() {
        for forcing_temperature in [20.0, -90.0] {
            for (first_role, second_role) in [
                (
                    ClimateLayerRole::LowerAtmosphere,
                    ClimateLayerRole::UpperAtmosphere,
                ),
                (
                    ClimateLayerRole::OceanMixedLayer,
                    ClimateLayerRole::OceanThermocline,
                ),
                (
                    ClimateLayerRole::OceanThermocline,
                    ClimateLayerRole::DeepOceanReservoir,
                ),
            ] {
                let first_reference = role_reference_temperature_c(
                    first_role,
                    forcing_temperature,
                    forcing_temperature,
                    forcing_temperature,
                );
                let second_reference = role_reference_temperature_c(
                    second_role,
                    forcing_temperature,
                    forcing_temperature,
                    forcing_temperature,
                );
                let exchange = equilibrium_anomaly_heat_exchange(
                    first_reference,
                    first_reference,
                    second_reference,
                    second_reference,
                    2.0e7,
                    4.0e9,
                    86_400.0,
                )
                .unwrap();
                assert_eq!(exchange.extensive_flux_w_m2().to_bits(), 0.0_f64.to_bits());
            }
        }
    }

    #[test]
    fn c2_internal_ocean_exchanges_exist_only_over_water() {
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        for (first, second) in [
            (
                ClimateLayerRole::OceanMixedLayer,
                ClimateLayerRole::OceanThermocline,
            ),
            (
                ClimateLayerRole::OceanThermocline,
                ClimateLayerRole::DeepOceanReservoir,
            ),
        ] {
            let exchange = layout
                .exchange(first, second)
                .expect("C2 internal ocean exchange is declared");
            assert!(exchange.water_only(), "{first:?}/{second:?}");
        }
    }

    #[test]
    fn subsurface_pair_exchange_cannot_cool_through_its_declared_floor() {
        let scale = subsurface_pair_exchange_scale_for_step(
            [
                ClimateLayerRole::OceanMixedLayer,
                ClimateLayerRole::OceanThermocline,
            ],
            [1.0, SUBSURFACE_OCEAN_MIN_C],
            [0.0, 0.0],
            [1.0e-7, -1.0e-9],
            7_200.0,
        );

        assert_eq!(scale.to_bits(), 0.0_f64.to_bits());

        let temperature = SUBSURFACE_OCEAN_MIN_C + 0.001;
        let partial = subsurface_pair_exchange_scale_for_step(
            [
                ClimateLayerRole::OceanThermocline,
                ClimateLayerRole::DeepOceanReservoir,
            ],
            [temperature, 2.0],
            [0.0, 0.0],
            [-1.0e-6, 3.0e-10],
            7_200.0,
        );
        let limited_end = f64::from(temperature) + 7_200.0 * partial * -1.0e-6;
        assert!(partial > 0.0 && partial < 1.0);
        assert!(limited_end >= f64::from(SUBSURFACE_OCEAN_MIN_C) - f64::EPSILON);
    }

    use crate::engine::BuildCancellation;
    use crate::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
    use crate::generators::natural::formation::global_circulation::{
        state::role_reference_temperature_c, LayeredClimateState,
    };

    #[test]
    fn upper_atmosphere_reference_confines_cold_surface_anomaly_below_interface() {
        // Lindzen & Nigam (1987); Battisti, Sarachik & Hirst (1999) Eq. (4)-(5)
        // and section 2b: a cold non-zonal surface anomaly belongs to the
        // lower troposphere while the free troposphere keeps the zonal-mean
        // structure; a column warmer than that mean convects and keeps its
        // own equilibrium stratification. Over a plateau half-sphere with an
        // albedo-type cold anomaly the plateau upper layer must sit at the
        // band mean after sea-level reduction, the sea upper layer at its own
        // offset, both columns must be zero-flux states of the paired heat
        // exchange, and only the plateau column may be the more stable one.
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let lapse = crate::world::natural::CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M;
        let plateau_m = 3_500.0_f32;
        let cold_anomaly_c = 20.0_f32;
        let elevation = grid
            .cells()
            .iter()
            .map(|cell| {
                if cell.center_unit()[0] > 0.0 {
                    plateau_m
                } else {
                    0.0
                }
            })
            .collect::<Vec<f32>>();
        let air = elevation
            .iter()
            .map(|&height| {
                let anomaly = if height > 0.0 { cold_anomaly_c } else { 0.0 };
                [15.0 - (lapse * f64::from(height)) as f32 - anomaly; CLIMATE_MONTH_COUNT]
            })
            .collect::<Vec<_>>();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            elevation.clone(),
            vec![1.0; count],
            vec![0.25; count],
            vec![0.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            air.clone(),
            air,
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let bands = axisymmetric_bands(&grid);
        let reduced = |role: ClimateLayerRole, cell: usize| {
            f64::from(state.temperature_c(role).unwrap()[cell]) + lapse * f64::from(elevation[cell])
        };
        let mut band_area = vec![0.0_f64; axisymmetric_band_count(&grid)];
        let mut band_lower_sum = vec![0.0_f64; band_area.len()];
        for (cell, geometry) in grid.cells().iter().enumerate() {
            let expected_lower = if elevation[cell] > 0.0 {
                15.0 - f64::from(cold_anomaly_c)
            } else {
                15.0
            };
            let lower = reduced(ClimateLayerRole::LowerAtmosphere, cell);
            assert!((lower - expected_lower).abs() < 1.0e-3);
            band_area[bands[cell] as usize] += geometry.area_m2();
            band_lower_sum[bands[cell] as usize] += geometry.area_m2() * lower;
        }
        let offset = f64::from(UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C);
        for cell in 0..count {
            let band_mean = band_lower_sum[bands[cell] as usize] / band_area[bands[cell] as usize];
            let upper = reduced(ClimateLayerRole::UpperAtmosphere, cell);
            let expected = if elevation[cell] > 0.0 {
                band_mean - offset
            } else {
                15.0 - offset
            };
            assert!(
                (upper - expected).abs() < 1.0e-3,
                "cell {cell}: {upper} vs {expected}"
            );
        }
        // This contract concerns the reference pair alone. A full endpoint
        // may create thermal departures that its predictor must exchange.
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        LayeredTendencySystem::new(&grid)
            .apply_pair_exchanges(
                &state,
                &forcing,
                0,
                &BuildCancellation::new(),
                &mut tendency,
                true,
                false,
                false,
                crate::world::natural::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
            )
            .unwrap();
        assert_eq!(tendency.budget.paired_heat_absolute_w, 0.0);
        let plateau_cell = (0..count).find(|&cell| elevation[cell] > 0.0).unwrap();
        let sea_cell = (0..count)
            .find(|&cell| elevation[cell] == 0.0 && bands[cell] == bands[plateau_cell])
            .unwrap();
        let plateau = atmospheric_thermal_buoyancy_difference_m_s2(&state, plateau_cell);
        let sea = atmospheric_thermal_buoyancy_difference_m_s2(&state, sea_cell);
        let plateau_band = bands[plateau_cell] as usize;
        let expected = atmospheric_thermal_buoyancy_m_s2(
            15.0 - f64::from(cold_anomaly_c)
                - band_lower_sum[plateau_band] / band_area[plateau_band],
        );
        assert!(plateau < 0.0 && (plateau - expected).abs() < 1.0e-5 * expected.abs());
        assert!(sea.abs() < 1.0e-6);
    }
    use crate::world::natural::{
        ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
        CLIMATE_MONTH_COUNT,
    };

    #[test]
    fn mixed_layer_uses_free_surface_gravity_and_depth_mean_steric_acceleration() {
        let (gravity, _, _, steric_acceleration) = role_constants(
            ClimateModelProfile::C2LayeredV1,
            ClimateLayerRole::OceanMixedLayer,
        );
        let expected_steric_acceleration = 0.5 * 9.806_65 * 2.0e-4 * 100.0;

        assert!((gravity - 9.806_65).abs() <= f64::EPSILON);
        assert!((steric_acceleration - expected_steric_acceleration).abs() <= 1.0e-12);
    }

    #[test]
    fn fast_operator_retains_quadratic_momentum_transport() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            for (cell, geometry) in grid.cells().iter().enumerate() {
                let radial = geometry.center_unit();
                state.velocity_m_s_mut(role).unwrap()[cell] = tangentize(
                    [
                        12.0 * radial[2] - 3.0 * radial[1],
                        5.0 * radial[0],
                        2.0 * radial[1],
                    ],
                    radial,
                )
                .map(|value| value as f32);
            }
        }
        let evaluate = |state: &LayeredClimateState| {
            LayeredTendencySystem::new(&grid)
                .evaluate_fast(
                    state,
                    &forcing,
                    &vec![1.0; grid.edges().len()],
                    0,
                    &BuildCancellation::new(),
                )
                .unwrap()
        };
        let forward = evaluate(&state);
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            for value in state.velocity_m_s_mut(role).unwrap().iter_mut().flatten() {
                *value = -*value;
            }
        }
        let reversed = evaluate(&state);
        // With uniform heights, Coriolis, drag, and viscosity are odd in u.
        // The quadratic transport must leave an even component above roundoff.
        let mut even_squared = 0.0_f64;
        let mut total_squared = 0.0_f64;
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            for (&first, &second) in forward
                .velocity_tendency_m_s2(role)
                .unwrap()
                .iter()
                .flatten()
                .zip(
                    reversed
                        .velocity_tendency_m_s2(role)
                        .unwrap()
                        .iter()
                        .flatten(),
                )
            {
                even_squared += (first + second).powi(2);
                total_squared += first.powi(2) + second.powi(2);
            }
        }
        assert!(
            even_squared > f64::from(f32::EPSILON) * total_squared,
            "fast operator is missing quadratic momentum transport: {even_squared}/{total_squared}"
        );

        // Reuse the fixture to check the convective energy contract at
        // nonuniform positive depths, including a thin lower layer.
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            let reference = f64::from(state.reference_thickness_m(role).unwrap());
            for (cell, geometry) in grid.cells().iter().enumerate() {
                state.height_anomaly_m_mut(role).unwrap()[cell] =
                    (reference * (-0.9 + 0.09 * geometry.center_unit()[2])) as f32;
            }
        }
        let system = LayeredTendencySystem::new(&grid);
        let mut transport = LayeredClimateTendency::zeroed(&state);
        let mut transport_workspace = LayeredTendencyWorkspace::for_grid(&grid);
        // Reuse these nonuniform depths on the smallest grid with non-axis
        // centers: the shared pressure input must retain the public operator's
        // tangent quantization, not the fast fused kernel's plain f32 cast.
        let cancellation = BuildCancellation::new();
        let temperature_gradients = system
            .atmospheric_temperature_gradients(&state, Some(&forcing), &cancellation)
            .unwrap();
        let gradients = system
            .atmospheric_pressure_gradients(
                &state,
                &temperature_gradients,
                &transport_workspace.open_edges,
                &cancellation,
            )
            .unwrap();
        for (role, shared) in [
            (ClimateLayerRole::LowerAtmosphere, &gradients.lower_height),
            (ClimateLayerRole::UpperAtmosphere, &gradients.upper_height),
        ] {
            let separate = CirculationOperators::new(&grid)
                .gradient_with_permeability_cancellable(
                    state.height_anomaly_m(role).unwrap(),
                    &transport_workspace.open_edges,
                    &cancellation,
                )
                .unwrap();
            assert!(shared
                .iter()
                .flatten()
                .zip(separate.iter().flatten())
                .all(|(shared, separate)| shared.to_bits() == separate.to_bits()));
        }
        drop(gradients);
        system
            .apply_horizontal_momentum_transport(
                &state,
                &BuildCancellation::new(),
                &mut transport,
                &mut transport_workspace,
            )
            .unwrap();
        let mut power = 0.0_f64;
        let mut absolute_power = 0.0_f64;
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            let actual_depth = (0..count)
                .map(|cell| system.fluid_layer_thickness_m(&state, role, cell).unwrap())
                .collect::<Vec<_>>();
            let operators = CirculationOperators::new(&grid);
            let (_, face_depth) = operators
                .reconstruct_layer_faces_cancellable(
                    &actual_depth,
                    state.velocity_m_s(role).unwrap(),
                    &vec![1.0; grid.edges().len()],
                    &mut transport_workspace.transport,
                    &BuildCancellation::new(),
                )
                .unwrap();
            assert!(face_depth
                .iter()
                .all(|depth| depth.is_finite() && *depth > 0.0));
            for (cell, geometry) in grid.cells().iter().enumerate() {
                let velocity = state.velocity_m_s(role).unwrap()[cell].map(f64::from);
                let acceleration = transport.velocity_tendency_m_s2(role).unwrap()[cell];
                let thickness = system.fluid_layer_thickness_m(&state, role, cell).unwrap();
                let momentum_power = geometry.area_m2() * thickness * dot(velocity, acceleration);
                let mass_power = 0.5
                    * geometry.area_m2()
                    * dot(velocity, velocity)
                    * f64::from(transport.height_tendency_m_s(role).unwrap()[cell]);
                power += momentum_power + mass_power;
                absolute_power += momentum_power.abs() + mass_power.abs();
            }
        }
        assert!(
            power.abs() <= f64::from(f32::EPSILON) * absolute_power,
            "convective kinetic energy residual {power} / {absolute_power}"
        );
    }

    #[test]
    #[ignore = "offline 6/12/24 same-state AAM error study; no exact-AAM acceptance threshold"]
    fn probe_c2_limited_depth_flux_refinement() {
        let cancellation = BuildCancellation::new();
        let mut previous_even: Option<[f64; 2]> = None;
        for resolution in [6, 12, 24] {
            let grid = CubedSphereGrid::new(resolution, 6_371_000.0).unwrap();
            let count = grid.cell_count();
            let forcing = PlanetForcing::new(
                *grid.fingerprint(),
                vec![0.0; count],
                vec![0.0; count],
                vec![0.0; count],
                vec![1.0; count],
                vec![[240.0; CLIMATE_MONTH_COUNT]; count],
                vec![[15.0; CLIMATE_MONTH_COUNT]; count],
                vec![[15.0; CLIMATE_MONTH_COUNT]; count],
                vec![[0.001; CLIMATE_MONTH_COUNT]; count],
            )
            .unwrap();
            let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
            let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
            let role = ClimateLayerRole::LowerAtmosphere;
            let reference = state.reference_thickness_m(role).unwrap();
            let density = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == role)
                .unwrap()
                .density_kg_m3();
            for (cell, geometry) in grid.cells().iter().enumerate() {
                let r = geometry.center_unit();
                state.height_anomaly_m_mut(role).unwrap()[cell] =
                    (0.25 * f64::from(reference) * (r[0].powi(2) + r[1].powi(2))) as f32;
                state.velocity_m_s_mut(role).unwrap()[cell] =
                    tangentize([0.0, 0.0, r[2]], r).map(|value| value as f32);
            }
            let system = LayeredTendencySystem::new(&grid);
            let operators = CirculationOperators::new(&grid);
            let open = vec![1.0; grid.edges().len()];
            let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
            let mut old_height_rate = vec![0.0_f64; count];
            let mut torques = [[0.0_f64; 2]; 2]; // flow direction x old/new transport
            let mut minimum_face_depth = f64::INFINITY;
            let mut maximum_rate = 0.0_f64;
            let mut maximum_relative_ke_residual = 0.0_f64;
            for (direction, direction_torques) in torques.iter_mut().enumerate() {
                if direction == 1 {
                    for value in state.velocity_m_s_mut(role).unwrap().iter_mut().flatten() {
                        *value = -*value;
                    }
                }
                // The donor path remains a real ocean/C1 consumer, so the
                // baseline is the production operator, not a copied formula.
                system
                    .layer_thickness_tendency_into(
                        &operators,
                        &state,
                        role,
                        &open,
                        true,
                        &mut old_height_rate,
                        &cancellation,
                    )
                    .unwrap();
                let mut candidate = LayeredClimateTendency::zeroed(&state);
                system
                    .apply_horizontal_momentum_transport(
                        &state,
                        &cancellation,
                        &mut candidate,
                        &mut workspace,
                    )
                    .unwrap();
                maximum_rate = maximum_rate.max(candidate.momentum_transport_rate_s_inv());
                let coriolis = operators
                    .coriolis_cancellable(
                        state.velocity_m_s(role).unwrap(),
                        super::EARTH_ROTATION_RATE_RAD_S,
                        &cancellation,
                    )
                    .unwrap();
                let mut kinetic_power = 0.0;
                let mut absolute_power = 0.0;
                for (cell, geometry) in grid.cells().iter().enumerate() {
                    let r = geometry.center_unit();
                    let depth = system.fluid_layer_thickness_m(&state, role, cell).unwrap();
                    let factor = density * geometry.area_m2();
                    let acceleration = coriolis[cell].map(f64::from);
                    let coriolis_torque = factor
                        * depth
                        * grid.radius_m()
                        * (r[0] * acceleration[1] - r[1] * acceleration[0]);
                    let planetary_weight = factor
                        * super::EARTH_ROTATION_RATE_RAD_S
                        * grid.radius_m().powi(2)
                        * (r[0].powi(2) + r[1].powi(2));
                    let candidate_height_rate =
                        f64::from(candidate.height_tendency_m_s(role).unwrap()[cell]);
                    direction_torques[0] +=
                        coriolis_torque + planetary_weight * old_height_rate[cell];
                    direction_torques[1] +=
                        coriolis_torque + planetary_weight * candidate_height_rate;
                    let velocity = state.velocity_m_s(role).unwrap()[cell].map(f64::from);
                    let force_power = factor
                        * depth
                        * dot(
                            velocity,
                            candidate.velocity_tendency_m_s2(role).unwrap()[cell],
                        );
                    let mass_power = 0.5 * factor * dot(velocity, velocity) * candidate_height_rate;
                    kinetic_power += force_power + mass_power;
                    absolute_power += force_power.abs() + mass_power.abs();
                }
                assert!(
                    kinetic_power.abs() <= f64::from(f32::EPSILON) * absolute_power,
                    "shared-flux KE residual {kinetic_power}/{absolute_power}"
                );
                maximum_relative_ke_residual = maximum_relative_ke_residual
                    .max(kinetic_power.abs() / absolute_power.max(f64::MIN_POSITIVE));
                for (cell, depth) in workspace.thickness_tendency_m_s.iter_mut().enumerate() {
                    *depth = system.fluid_layer_thickness_m(&state, role, cell).unwrap();
                }
                let (_, face_depth) = operators
                    .reconstruct_layer_faces_cancellable(
                        &workspace.thickness_tendency_m_s,
                        state.velocity_m_s(role).unwrap(),
                        &open,
                        &mut workspace.transport,
                        &cancellation,
                    )
                    .unwrap();
                minimum_face_depth = face_depth
                    .iter()
                    .copied()
                    .fold(minimum_face_depth, f64::min);
                assert!(minimum_face_depth.is_finite() && minimum_face_depth > 0.0);
            }
            let even = [torques[0][0] + torques[1][0], torques[0][1] + torques[1][1]];
            let refinement_ratios =
                previous_even.map(|previous| [previous[0] / even[0], previous[1] / even[1]]);
            eprintln!("[limited-depth-flux-study] resolution={resolution} old_new_even_Nm={even:?} previous_over_current={refinement_ratios:?} min_face_H={minimum_face_depth:.9} max_rate_s_inv={maximum_rate:.9e} relative_KE_residual={maximum_relative_ke_residual:.9e} directional_old_new={torques:?}");
            previous_even = Some(even);
            // Zero horizontal velocity must still give exactly zero transport,
            // despite nonuniform depth. This is not a pressure-balance claim.
            for active_role in state.active_roles().to_vec() {
                state.velocity_m_s_mut(active_role).unwrap().fill([0.0; 3]);
            }
            let mut resting = LayeredClimateTendency::zeroed(&state);
            system
                .apply_horizontal_momentum_transport(
                    &state,
                    &cancellation,
                    &mut resting,
                    &mut workspace,
                )
                .unwrap();
            for active_role in [role, ClimateLayerRole::UpperAtmosphere] {
                assert!(resting
                    .height_tendency_m_s(active_role)
                    .unwrap()
                    .iter()
                    .all(|rate| *rate == 0.0));
                assert!(resting
                    .velocity_tendency_m_s2(active_role)
                    .unwrap()
                    .iter()
                    .flatten()
                    .all(|rate| *rate == 0.0));
            }
        }
    }

    #[test]
    fn declared_overturning_transfers_full_amount_and_conserves_momentum() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        state.height_anomaly_m_mut(lower).unwrap().fill(-4_000.0);
        for (cell, geometry) in grid.cells().iter().enumerate() {
            state.velocity_m_s_mut(lower).unwrap()[cell] =
                tangentize([8.0, -3.0, 1.0], geometry.center_unit()).map(|value| value as f32);
            state.velocity_m_s_mut(upper).unwrap()[cell] =
                tangentize([-2.0, 4.0, 0.0], geometry.center_unit()).map(|value| value as f32);
        }
        let before = state.clone();
        let floor = vec![500.0; count];
        let gradient = vec![[0.0; 3]; count];
        let land_evaporation = vec![0.0; count];
        let system =
            LayeredTendencySystem::with_terrain(&grid, &gradient, &floor, &land_evaporation, 0.0);
        let step = crate::world::natural::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS;
        let mut exchanges = vec![0.0; count];
        exchanges[0] = 1_000.0 / step;
        exchanges[1] = -1_000.0 / step;
        system
            .apply_overturning_exchange(&mut state, &exchanges, step, &BuildCancellation::new())
            .unwrap();
        for (cell, &exchange) in exchanges.iter().enumerate().take(2) {
            let lower_change = f64::from(state.height_anomaly_m(lower).unwrap()[cell])
                - f64::from(before.height_anomaly_m(lower).unwrap()[cell]);
            assert_eq!(
                lower_change,
                -exchange * step,
                "declared Q must not be reduced when the donor has enough mass"
            );
            for component in 0..3 {
                let momentum = |state: &LayeredClimateState| {
                    [lower, upper]
                        .iter()
                        .map(|&role| {
                            system.fluid_layer_thickness_m(state, role, cell).unwrap()
                                * f64::from(state.velocity_m_s(role).unwrap()[cell][component])
                        })
                        .sum::<f64>()
                };
                let scale = [lower, upper]
                    .iter()
                    .map(|&role| {
                        system.fluid_layer_thickness_m(&before, role, cell).unwrap()
                            * f64::from(before.velocity_m_s(role).unwrap()[cell][component]).abs()
                    })
                    .sum::<f64>();
                assert!(
                    (momentum(&state) - momentum(&before)).abs() <= f64::from(f32::EPSILON) * scale
                );
            }
        }
        let mut half_steps = before.clone();
        for _ in 0..2 {
            system
                .apply_overturning_exchange(
                    &mut half_steps,
                    &exchanges,
                    step / 2.0,
                    &BuildCancellation::new(),
                )
                .unwrap();
        }
        for role in [lower, upper] {
            assert_eq!(
                state.height_anomaly_m(role),
                half_steps.height_anomaly_m(role)
            );
            for (&full, &half) in state
                .velocity_m_s(role)
                .unwrap()
                .iter()
                .flatten()
                .zip(half_steps.velocity_m_s(role).unwrap().iter().flatten())
            {
                assert!((full - half).abs() <= 2.0 * f32::EPSILON * full.abs().max(half.abs()));
            }
        }
        // Exercise the production stage generator against the isolated exact
        // donor solution at three step sizes, allowing accumulated f32 roundoff.
        let mut errors = Vec::new();
        for divisions in [1, 2, 4] {
            let mut integrated = before.clone();
            for _ in 0..divisions {
                integrated = super::super::rk3::rk3_step_with(
                    &grid,
                    &integrated,
                    step / f64::from(divisions),
                    &BuildCancellation::new(),
                    |stage| {
                        let cancellation = BuildCancellation::new();
                        let mut derivative = LayeredClimateTendency::zeroed(stage);
                        for (cell, &exchange) in exchanges.iter().enumerate() {
                            derivative.layer_mut(lower).unwrap().height_tendency_m_s[cell] =
                                -exchange as f32;
                            derivative.layer_mut(upper).unwrap().height_tendency_m_s[cell] =
                                exchange as f32;
                        }
                        system.apply_declared_overturning_momentum(
                            stage,
                            &exchanges,
                            &cancellation,
                            &mut derivative,
                        )?;
                        super::super::rk3::ClimateDerivative::from_tendency(
                            stage,
                            &derivative,
                            &cancellation,
                        )
                    },
                )
                .unwrap();
            }
            let error = [lower, upper]
                .iter()
                .flat_map(|&role| {
                    integrated
                        .velocity_m_s(role)
                        .unwrap()
                        .iter()
                        .flatten()
                        .zip(state.velocity_m_s(role).unwrap().iter().flatten())
                })
                .map(|(&actual, &expected)| f64::from(actual - expected).abs())
                .fold(0.0_f64, f64::max);
            errors.push(error);
        }
        let velocity_scale = [lower, upper]
            .iter()
            .flat_map(|&role| before.velocity_m_s(role).unwrap().iter().flatten())
            .map(|&value| f64::from(value).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            errors.iter().zip([1, 2, 4]).all(|(&error, steps)| error
                <= f64::from(steps) * f64::from(f32::EPSILON) * velocity_scale),
            "stage exchange differs from the frozen-Q donor solution: {errors:?}"
        );
        let mut exhausted = before.clone();
        exchanges.fill(0.0);
        exchanges[0] = system.fluid_layer_thickness_m(&before, lower, 0).unwrap() / step;
        assert!(
            matches!(system.apply_overturning_exchange(&mut exhausted, &exchanges, step, &BuildCancellation::new()),
            Err(super::LayeredTendencyError::InvalidFluidThickness { role, cell: 0, .. }) if role == lower)
        );
    }

    #[test]
    fn atmospheric_mechanical_exchange_conserves_actual_column_momentum() {
        use super::{mass_per_area, PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE};
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![1.0; count],
            vec![0.25; count],
            vec![0.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        for (cell, geometry) in grid.cells().iter().enumerate() {
            state.velocity_m_s_mut(upper).unwrap()[cell] =
                tangentize([2.0, 1.0, 0.0], geometry.center_unit()).map(|v| v as f32);
        }
        let system = LayeredTendencySystem::new(&grid);
        state.height_anomaly_m_mut(lower).unwrap().fill(-5_000.0);
        state.height_anomaly_m_mut(upper).unwrap().fill(5_000.0);
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        system
            .apply_pair_momentum_exchanges(
                &state,
                &forcing,
                &BuildCancellation::new(),
                &mut tendency,
            )
            .unwrap();
        for cell in 0..count {
            let mass = |role| {
                mass_per_area(&state, role)
                    * (1.0
                        + f64::from(state.height_anomaly_m(role).unwrap()[cell])
                            / f64::from(state.reference_thickness_m(role).unwrap()))
            };
            for component in 0..3 {
                let first =
                    mass(lower) * tendency.velocity_tendency_m_s2(lower).unwrap()[cell][component];
                let second =
                    mass(upper) * tendency.velocity_tendency_m_s2(upper).unwrap()[cell][component];
                assert!(
                    (first + second).abs()
                        <= PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE
                            * (first.abs() + second.abs()),
                    "mechanical exchange creates column momentum: {first} + {second}"
                );
            }
        }
    }

    #[test]
    fn equal_inertia_layers_have_reciprocal_interface_pressure_response() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let initial = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        let responses = [(lower, upper), (upper, lower)].map(|(source, target)| {
            let mut state = initial.clone();
            for (cell, value) in state
                .height_anomaly_m_mut(source)
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                *value = (12.0 * grid.cells()[cell].center_unit()[0]) as f32;
            }
            LayeredTendencySystem::new(&grid)
                .evaluate_fast(
                    &state,
                    &forcing,
                    &vec![1.0; grid.edges().len()],
                    0,
                    &BuildCancellation::new(),
                )
                .unwrap()
                .velocity_tendency_m_s2(target)
                .unwrap()
                .to_vec()
        });
        for (first, second) in responses[0]
            .iter()
            .flatten()
            .zip(responses[1].iter().flatten())
        {
            assert!(
                (first - second).abs() <= f64::from(f32::EPSILON) * (first.abs() + second.abs()),
                "equal-inertia pressure response is asymmetric: {first} vs {second}"
            );
        }
    }

    #[test]
    fn closed_two_layer_atmosphere_has_no_external_height_source() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        state
            .height_anomaly_m_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap()
            .fill(12.0);
        let tendency = LayeredTendencySystem::new(&grid)
            .evaluate(
                &state,
                &forcing,
                &vec![1.0; grid.edges().len()],
                0,
                &BuildCancellation::new(),
            )
            .unwrap();
        assert_eq!(tendency.budget.external_atmosphere_amount_rate_m3_s(), 0.0);
        // Resting horizontal flow has no transport. The existing interface
        // may vent, but its declared mass source stays internal to each column.
        for (cell, &exchange) in tendency
            .overturning_exchange_m_s
            .as_ref()
            .unwrap()
            .iter()
            .enumerate()
        {
            let lower = tendency
                .height_tendency_m_s(ClimateLayerRole::LowerAtmosphere)
                .unwrap()[cell];
            let upper = tendency
                .height_tendency_m_s(ClimateLayerRole::UpperAtmosphere)
                .unwrap()[cell];
            assert_eq!(lower + upper, 0.0);
            assert_eq!(lower, (-exchange) as f32);
            assert_eq!(upper, exchange as f32);
        }
    }

    #[test]
    fn fast_endpoint_temperature_gradients_follow_changing_heights_bitwise() {
        // One six-cell fixture isolates the endpoint lifetime: T stays fixed
        // while both interfaces move, so cached H would fail this comparison.
        let (grid, forcing, mut state) = axisymmetric_venting_fixture(0.0);
        let cancellation = BuildCancellation::new();
        let system = LayeredTendencySystem::new(&grid);
        let open = vec![1.0; grid.edges().len()];
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            for (cell, geometry) in grid.cells().iter().enumerate() {
                state.temperature_c_mut(role).unwrap()[cell] += geometry.center_unit()[1] as f32;
            }
        }
        let temperature = system
            .atmospheric_temperature_gradients(&state, Some(&forcing), &cancellation)
            .unwrap();
        let mut cached_workspace = LayeredTendencyWorkspace::for_grid(&grid);
        let mut fresh_workspace = LayeredTendencyWorkspace::for_grid(&grid);
        let mut previous_velocity: Option<Vec<[f64; 3]>> = None;
        for stage in 0..2 {
            for role in [
                ClimateLayerRole::LowerAtmosphere,
                ClimateLayerRole::UpperAtmosphere,
            ] {
                for (cell, geometry) in grid.cells().iter().enumerate() {
                    state.height_anomaly_m_mut(role).unwrap()[cell] =
                        (stage as f64 * 20.0 * geometry.center_unit()[0]) as f32;
                }
            }
            let fresh = system
                .evaluate_fast_with_workspace(
                    &state,
                    &forcing,
                    &open,
                    0,
                    &cancellation,
                    &mut fresh_workspace,
                )
                .unwrap();
            let cached = system
                .evaluate_fast_with_temperature_gradients_validated(
                    &state,
                    &forcing,
                    &open,
                    &cancellation,
                    (&mut cached_workspace, Some(&temperature)),
                )
                .unwrap();
            assert_eq!(cached, fresh);
            for &role in state.active_roles() {
                assert!(cached
                    .velocity_tendency_m_s2(role)
                    .unwrap()
                    .iter()
                    .flatten()
                    .zip(fresh.velocity_tendency_m_s2(role).unwrap().iter().flatten())
                    .all(|(cached, fresh)| cached.to_bits() == fresh.to_bits()));
            }
            let velocity = cached
                .velocity_tendency_m_s2(ClimateLayerRole::LowerAtmosphere)
                .unwrap();
            if let Some(previous) = &previous_velocity {
                assert_ne!(
                    velocity,
                    previous.as_slice(),
                    "changed H must change the pressure response"
                );
            }
            previous_velocity = Some(velocity.to_vec());
        }
    }

    #[test]
    fn unsupported_common_buoyancy_is_rejected_before_cfl_reduction() {
        let (grid, forcing, mut state) = axisymmetric_venting_fixture(0.0);
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            for temperature in state.temperature_c_mut(role).unwrap() {
                *temperature += 300.0;
            }
        }
        let cancellation = BuildCancellation::new();
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        assert!(matches!(
            LayeredTendencySystem::new(&grid).apply_atmospheric_thermal_pressure(
                &state,
                Some(&forcing),
                &LayeredTendencySystem::new(&grid)
                    .atmospheric_pressure_gradients(
                        &state,
                        &LayeredTendencySystem::new(&grid)
                            .atmospheric_temperature_gradients(
                                &state,
                                Some(&forcing),
                                &cancellation
                            )
                            .unwrap(),
                        &vec![1.0; grid.edges().len()],
                        &cancellation,
                    )
                    .unwrap(),
                &cancellation,
                &mut tendency,
            ),
            Err(super::LayeredTendencyError::UnstableAtmosphericFreeSurface { .. }),
        ));
        assert!(
            super::super::rk3::estimate_cfl(&grid, &state, 1.0, &cancellation).is_err(),
            "max must not swallow an unsupported wave mode as NaN"
        );
    }

    #[test]
    fn scalar_cooling_replans_the_thermal_fast_step() {
        // A six-cell, smaller-radius pressure fixture makes the entry and
        // endpoint CFL straddle one substep without generating a full world.
        let grid = CubedSphereGrid::new(1, 500_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let state = LayeredClimateState::from_forcing(
            &grid,
            &ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1),
            &forcing,
            0,
        )
        .unwrap();
        let cancellation = BuildCancellation::new();
        let cooling = 50.0_f32;
        let mut cold = state.clone();
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            for temperature in cold.temperature_c_mut(role).unwrap() {
                *temperature -= cooling;
            }
        }
        let entry_rate =
            super::super::rk3::estimate_cfl(&grid, &state, 1.0, &cancellation).unwrap();
        let endpoint_rate =
            super::super::rk3::estimate_cfl(&grid, &cold, 1.0, &cancellation).unwrap();
        assert!(endpoint_rate > entry_rate);
        let target = crate::world::natural::GLOBAL_CIRCULATION_FAST_CFL_TARGET;
        let step = target / (0.5 * (entry_rate + endpoint_rate));
        assert_eq!(
            super::super::SplitExplicitRk3Integrator::slow_step_plan(step).0,
            1
        );
        let mut declared = LayeredClimateTendency::zeroed(&state);
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            declared
                .layer_mut(role)
                .unwrap()
                .temperature_tendency_k_s
                .fill((-f64::from(cooling) / step) as f32);
        }
        let result = super::super::SplitExplicitRk3Integrator::new(&grid, step)
            .unwrap()
            .advance_with_declared_tendency_and_phase_observer(
                &state,
                &forcing,
                &vec![1.0; grid.edges().len()],
                step,
                &declared,
                &cancellation,
                &mut |_| {},
            )
            .unwrap();
        assert!(
            result.diagnostics().fast_substeps() > 1,
            "entry-only planning missed the colder endpoint pressure mode"
        );
        assert!(result.diagnostics().maximum_cfl() <= target);
    }

    #[test]
    fn uniform_buoyancy_responds_to_the_actual_top_surface_slope() {
        // Constant density anomaly changes free-surface gravity. The old
        // column projection deletes this response even on a six-cell grid.
        let (grid, forcing, mut state) = axisymmetric_venting_fixture(0.0);
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        let temperature_anomaly = 5.0_f32;
        let reference = crate::world::natural::STANDARD_ATMOSPHERE_SEA_LEVEL_TEMPERATURE_C as f32;
        state
            .temperature_c_mut(lower)
            .unwrap()
            .fill(reference + temperature_anomaly);
        state
            .temperature_c_mut(upper)
            .unwrap()
            .fill(reference + temperature_anomaly - UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C);
        for (height, cell) in state
            .height_anomaly_m_mut(upper)
            .unwrap()
            .iter_mut()
            .zip(grid.cells())
        {
            *height = (20.0 * cell.center_unit()[0]) as f32;
        }
        let cancellation = BuildCancellation::new();
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        LayeredTendencySystem::new(&grid)
            .apply_atmospheric_thermal_pressure(
                &state,
                Some(&forcing),
                &LayeredTendencySystem::new(&grid)
                    .atmospheric_pressure_gradients(
                        &state,
                        &LayeredTendencySystem::new(&grid)
                            .atmospheric_temperature_gradients(
                                &state,
                                Some(&forcing),
                                &cancellation,
                            )
                            .unwrap(),
                        &vec![1.0; grid.edges().len()],
                        &cancellation,
                    )
                    .unwrap(),
                &cancellation,
                &mut tendency,
            )
            .unwrap();
        let gradient = CirculationOperators::new(&grid)
            .gradient_with_permeability_cancellable(
                state.height_anomaly_m(upper).unwrap(),
                &vec![1.0; grid.edges().len()],
                &cancellation,
            )
            .unwrap();
        let buoyancy = atmospheric_thermal_buoyancy_m_s2(f64::from(temperature_anomaly));
        let mut expected_norm = 0.0_f64;
        let mut error = 0.0_f64;
        for role in [lower, upper] {
            for (actual, gradient) in tendency
                .velocity_tendency_m_s2(role)
                .unwrap()
                .iter()
                .zip(&gradient)
            {
                for component in 0..3 {
                    let expected = buoyancy * f64::from(gradient[component]);
                    expected_norm = expected_norm.max(expected.abs());
                    error = error.max((actual[component] - expected).abs());
                }
            }
        }
        assert!(expected_norm > 0.0);
        assert!(
            error <= 32.0 * f64::from(f32::EPSILON) * expected_norm,
            "top-pressure response missing: error={error}, expected={expected_norm}"
        );
    }

    #[test]
    fn lower_thermal_anomaly_cannot_change_pressure_above_a_flat_interface() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        for lower_height_anomaly in [0.0, -4_000.0] {
            let mut before =
                LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
            before
                .height_anomaly_m_mut(ClimateLayerRole::LowerAtmosphere)
                .unwrap()
                .fill(lower_height_anomaly);
            let mut after = before.clone();
            for (cell, geometry) in grid.cells().iter().enumerate() {
                after
                    .temperature_c_mut(ClimateLayerRole::LowerAtmosphere)
                    .unwrap()[cell] += (3.0 * geometry.center_unit()[0]) as f32;
            }
            let tendency = LayeredTendencySystem::new(&grid)
                .evaluate_thermal_pressure_endpoint_difference_with_workspace_validated(
                    &before,
                    &after,
                    &vec![1.0; grid.edges().len()],
                    &BuildCancellation::new(),
                    &mut LayeredTendencyWorkspace::for_grid(&grid),
                )
                .unwrap();
            let lower = ClimateLayerRole::LowerAtmosphere;
            let upper = ClimateLayerRole::UpperAtmosphere;
            assert!(tendency
                .velocity_tendency_m_s2(lower)
                .unwrap()
                .iter()
                .any(|value| super::norm(*value) > 0.0));
            assert!(
                tendency
                    .velocity_tendency_m_s2(upper)
                    .unwrap()
                    .iter()
                    .all(|value| *value == [0.0; 3]),
                "fixed top pressure cannot depend on a thermal anomaly below the interface"
            );
        }
    }

    #[test]
    fn atmospheric_reference_lapse_does_not_drive_horizontal_pressure_wind() {
        // The same reference atmosphere sampled at two terrain heights has
        // no horizontal thermal anomaly. A small operator fixture isolates
        // the coordinate error without solving a generated world's climate.
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let elevation = grid
            .cells()
            .iter()
            .map(|cell| {
                if cell.center_unit()[0] > 0.0 {
                    2_000.0
                } else {
                    0.0
                }
            })
            .collect::<Vec<f32>>();
        // The production forcing publishes the surface target already reduced
        // by the reference lapse over the orography.
        let air = elevation
            .iter()
            .map(|&height| {
                [15.0 - (crate::world::natural::CLIMATE_OROGRAPHIC_LAPSE_RATE_C_PER_M
                    * f64::from(height)) as f32; CLIMATE_MONTH_COUNT]
            })
            .collect::<Vec<_>>();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            elevation.clone(),
            vec![1.0; count],
            vec![0.25; count],
            vec![0.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            air.clone(),
            air,
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let tendency = LayeredTendencySystem::new(&grid)
            .evaluate(
                &state,
                &forcing,
                &vec![1.0; grid.edges().len()],
                0,
                &BuildCancellation::new(),
            )
            .unwrap();
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            assert!(tendency
                .layer(role)
                .unwrap()
                .velocity_tendency_m_s2
                .iter()
                .flatten()
                .all(|value| *value == 0.0));
        }
        // Varying actual lower depth must not turn a horizontally uniform
        // reference anomaly into a bottom-slope thermal pressure force.
        let terrain_gradient = vec![[0.0; 3]; count];
        let evaporation_fraction = vec![1.0; count];
        let system = LayeredTendencySystem::with_terrain(
            &grid,
            &terrain_gradient,
            &elevation,
            &evaporation_fraction,
            0.0,
        );
        let mut thermal = LayeredClimateTendency::zeroed(&state);
        system
            .apply_atmospheric_thermal_pressure(
                &state,
                Some(&forcing),
                &system
                    .atmospheric_pressure_gradients(
                        &state,
                        &system
                            .atmospheric_temperature_gradients(
                                &state,
                                Some(&forcing),
                                &BuildCancellation::new(),
                            )
                            .unwrap(),
                        &vec![1.0; grid.edges().len()],
                        &BuildCancellation::new(),
                    )
                    .unwrap(),
                &BuildCancellation::new(),
                &mut thermal,
            )
            .unwrap();
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            assert!(thermal
                .velocity_tendency_m_s2(role)
                .unwrap()
                .iter()
                .flatten()
                .all(|value| *value == 0.0));
        }
    }

    #[test]
    fn atmospheric_moisture_diffusion_does_not_cross_the_equator() {
        // A hemispheric step isolates the inappropriate tropical eddy flux:
        // only edges on the equator see a gradient. No climate solve is needed.
        let grid = CubedSphereGrid::new(4, 6_371_000.0).unwrap();
        let values = grid
            .cells()
            .iter()
            .map(|cell| {
                if cell.center_unit()[2] > 0.0 {
                    0.02
                } else {
                    0.01
                }
            })
            .collect::<Vec<f32>>();
        let mut tendency = vec![0.0; grid.cell_count()];
        accumulate_horizontal_scalar_diffusion(
            &grid,
            &values,
            &vec![1.0; grid.edges().len()],
            ATMOSPHERE_HORIZONTAL_EDDY_MOISTURE_DIFFUSIVITY_M2_S,
            &mut tendency,
            &BuildCancellation::new(),
        )
        .unwrap();
        assert!(tendency.iter().all(|value| *value == 0.0));
    }

    #[test]
    fn c2_horizontal_viscosity_dissipates_actual_column_kinetic_energy() {
        // One open face isolates the finite-volume viscosity contract. Equal
        // area weighting can accelerate the heavier, slower column enough
        // to create energy; a full circulation solve would obscure this sign.
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let role = ClimateLayerRole::LowerAtmosphere;
        let [first, second] = grid.edges()[0].cells().map(|cell| cell as usize);
        let first_radial = grid.cells()[first].center_unit();
        let second_radial = grid.cells()[second].center_unit();
        let common_tangent = [
            first_radial[1] * second_radial[2] - first_radial[2] * second_radial[1],
            first_radial[2] * second_radial[0] - first_radial[0] * second_radial[2],
            first_radial[0] * second_radial[1] - first_radial[1] * second_radial[0],
        ];
        state.velocity_m_s_mut(role).unwrap().fill([0.0; 3]);
        state.velocity_m_s_mut(role).unwrap()[first] = common_tangent.map(|value| value as f32);
        state.velocity_m_s_mut(role).unwrap()[second] =
            common_tangent.map(|value| (1.5 * value) as f32);
        let reference_depth = state.reference_thickness_m(role).unwrap();
        state.height_anomaly_m_mut(role).unwrap()[second] = -0.5 * reference_depth;
        let mut permeability = vec![0.0; grid.edges().len()];
        permeability[0] = 1.0;
        let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
        let system = LayeredTendencySystem::new(&grid);
        system
            .horizontal_velocity_diffusion(
                &state,
                role,
                &permeability,
                &mut workspace,
                &BuildCancellation::new(),
            )
            .unwrap();
        let layer = layout
            .layers()
            .iter()
            .find(|layer| layer.role() == role)
            .unwrap();
        let power = [first, second]
            .into_iter()
            .map(|cell| {
                let mass = layer.density_kg_m3()
                    * system.fluid_layer_thickness_m(&state, role, cell).unwrap()
                    * grid.cells()[cell].area_m2();
                mass * dot(
                    state.velocity_m_s(role).unwrap()[cell].map(f64::from),
                    workspace.vector_scratch[cell].map(f64::from),
                )
            })
            .sum::<f64>();
        assert!(
            power < 0.0,
            "actual-column viscous power must be negative: {power}"
        );

        // Reuse this face to measure a legal thin column's viscous diagonal
        // from the production operator itself. Zero background flow must not
        // hide this stiffness from the fast-step planner's reported rate.
        for active_role in state.active_roles().to_vec() {
            state.velocity_m_s_mut(active_role).unwrap().fill([0.0; 3]);
        }
        state.height_anomaly_m_mut(role).unwrap()[first] =
            -reference_depth + reference_depth / 1024.0;
        let resting = system
            .evaluate_fast(
                &state,
                &forcing,
                &permeability,
                0,
                &BuildCancellation::new(),
            )
            .unwrap();
        state.velocity_m_s_mut(role).unwrap()[first] = common_tangent.map(|value| value as f32);
        system
            .horizontal_velocity_diffusion(
                &state,
                role,
                &permeability,
                &mut workspace,
                &BuildCancellation::new(),
            )
            .unwrap();
        let viscous_rate = -dot(
            common_tangent,
            workspace.vector_scratch[first].map(f64::from),
        ) / dot(common_tangent, common_tangent);
        assert!(viscous_rate > 0.0);
        assert!(
            resting.momentum_transport_rate_s_inv() >= viscous_rate,
            "reported rate {} misses the thin-column viscous rate {viscous_rate}",
            resting.momentum_transport_rate_s_inv()
        );
    }

    #[test]
    fn horizontal_velocity_diffusion_dissipates_a_spike_without_crossing_closed_edges() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let role = ClimateLayerRole::LowerAtmosphere;
        state.velocity_m_s_mut(role).unwrap().fill([0.0; 3]);
        state.velocity_m_s_mut(role).unwrap()[0] =
            tangentize([1.0, 2.0, 3.0], grid.cells()[0].center_unit())
                .map(|component| component as f32);
        let velocity = state.velocity_m_s(role).unwrap();
        let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
        let system = LayeredTendencySystem::new(&grid);
        system
            .horizontal_velocity_diffusion(
                &state,
                role,
                &vec![1.0; grid.edges().len()],
                &mut workspace,
                &BuildCancellation::new(),
            )
            .unwrap();

        let kinetic_energy_tendency = grid
            .cells()
            .iter()
            .enumerate()
            .map(|(cell, geometry)| {
                geometry.area_m2()
                    * velocity[cell]
                        .iter()
                        .zip(workspace.vector_scratch[cell])
                        .map(|(velocity, acceleration)| {
                            f64::from(*velocity) * f64::from(acceleration)
                        })
                        .sum::<f64>()
            })
            .sum::<f64>();
        assert!(kinetic_energy_tendency < 0.0);
        assert!(workspace
            .vector_scratch
            .iter()
            .skip(1)
            .flatten()
            .any(|value| *value != 0.0));

        workspace.open_edges.fill(0.0);
        system
            .horizontal_velocity_diffusion(
                &state,
                role,
                &vec![0.0; grid.edges().len()],
                &mut workspace,
                &BuildCancellation::new(),
            )
            .unwrap();
        assert!(workspace
            .vector_scratch
            .iter()
            .flatten()
            .all(|value| *value == 0.0));
    }

    #[test]
    fn reusable_fast_gradients_match_standalone_operator_bit_for_bit() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let height = (0..grid.cell_count())
            .map(|cell| 10.0 * cell as f32)
            .collect::<Vec<_>>();
        let velocity = grid
            .cells()
            .iter()
            .enumerate()
            .map(|(cell, geometry)| {
                tangentize([2.0 + cell as f64, -1.0, 0.5], geometry.center_unit())
                    .map(|value| value as f32)
            })
            .collect::<Vec<_>>();
        let permeability = vec![1.0; grid.edges().len()];
        let cancellation = BuildCancellation::new();
        let operators = CirculationOperators::new(&grid);
        let expected_gradient = operators
            .gradient_with_permeability_cancellable(&height, &permeability, &cancellation)
            .unwrap();
        // The fused stage gradient skips the public representable-vector
        // correction: it must equal the exact f64 tangent cast to f32.
        let expected_stage_gradient = operators
            .gradient_f64_with_permeability(
                &height
                    .iter()
                    .map(|value| f64::from(*value))
                    .collect::<Vec<_>>(),
                &permeability,
            )
            .unwrap()
            .into_iter()
            .map(|gradient| gradient.map(|component| component as f32))
            .collect::<Vec<_>>();
        let mut expected_thickness = vec![0.0; grid.cell_count()];
        conservative_layer_thickness_tendency(
            &grid,
            6_000.0,
            None,
            &height,
            &velocity,
            &permeability,
            &mut expected_thickness,
            &cancellation,
        )
        .unwrap();
        let mut fused_gradient = vec![[0.0; 3]; grid.cell_count()];
        let mut fused_thickness = vec![0.0; grid.cell_count()];
        let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);

        operators
            .gradient_and_donor_layer_thickness_tendency_into_cancellable_validated(
                &height,
                &velocity,
                &permeability,
                6_000.0,
                None,
                true,
                &mut fused_gradient,
                &mut fused_thickness,
                &mut workspace.transport,
                &cancellation,
            )
            .unwrap();

        assert_eq!(fused_gradient, expected_stage_gradient);
        assert_eq!(fused_thickness, expected_thickness);

        let allocation_signature = workspace.transport.allocation_signature();
        let mut reused_gradient = vec![[0.0; 3]; grid.cell_count()];
        operators
            .gradient_into_cancellable_validated(
                &height,
                &permeability,
                &mut reused_gradient,
                &mut workspace.transport,
                &cancellation,
            )
            .unwrap();
        assert_eq!(reused_gradient, expected_gradient);
        assert_eq!(
            workspace.transport.allocation_signature(),
            allocation_signature
        );
    }

    #[test]
    fn fast_tendency_reevaluates_velocity_diffusion_at_each_stage() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let spike = tangentize([1.0, 2.0, 3.0], grid.cells()[0].center_unit())
            .map(|component| component as f32);
        for role in state.active_roles().to_vec() {
            state.velocity_m_s_mut(role).unwrap()[0] = spike;
        }

        let tendency = LayeredTendencySystem::new(&grid)
            .evaluate_fast(
                &state,
                &forcing,
                &vec![1.0; grid.edges().len()],
                0,
                &BuildCancellation::new(),
            )
            .unwrap();

        assert!(tendency
            .velocity_tendency_m_s2(ClimateLayerRole::LowerAtmosphere)
            .unwrap()
            .iter()
            .skip(1)
            .flatten()
            .any(|value| *value != 0.0));
    }

    #[test]
    fn frozen_background_scalar_probe_matches_the_local_full_endpoint() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            (0..count).map(|cell| 50.0 * cell as f32).collect(),
            vec![0.25; count],
            vec![0.25; count],
            vec![0.75; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[18.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        for role in state.active_roles().to_vec() {
            for (cell, velocity) in state.velocity_m_s_mut(role).unwrap().iter_mut().enumerate() {
                *velocity = tangentize(
                    [4.0 + cell as f64 * 0.01, -2.0, 1.0],
                    grid.cells()[cell].center_unit(),
                )
                .map(|component| component as f32);
            }
            if !super::is_atmosphere_role(role) {
                for (cell, temperature) in state
                    .temperature_c_mut(role)
                    .unwrap()
                    .iter_mut()
                    .enumerate()
                {
                    *temperature += 2.0 * grid.cells()[cell].center_unit()[2] as f32;
                }
            }
        }
        let permeability = vec![1.0; grid.edges().len()];
        let cancellation = BuildCancellation::new();
        let system = LayeredTendencySystem::new(&grid);
        let full = system
            .evaluate_for_step(&state, &forcing, &permeability, 0, 7_200.0, &cancellation)
            .unwrap();
        let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
        let scalar = system
            .evaluate_thermodynamic_moisture_with_workspace_for_step(
                &state,
                &forcing,
                &permeability,
                0,
                7_200.0,
                &cancellation,
                &mut workspace,
            )
            .unwrap();
        let terrain_gradient = CirculationOperators::new(&grid)
            .gradient(forcing.elevation_m())
            .unwrap();
        let flat_floor = vec![0.0_f32; grid.cell_count()];
        let dry_land = vec![0.0_f32; grid.cell_count()];
        let supplied = LayeredTendencySystem::with_terrain(
            &grid,
            &terrain_gradient,
            &flat_floor,
            &dry_land,
            0.0,
        )
        .evaluate_thermodynamic_moisture_with_workspace_for_step(
            &state,
            &forcing,
            &permeability,
            0,
            7_200.0,
            &cancellation,
            &mut workspace,
        )
        .unwrap();

        let fast = system
            .evaluate_fast(&state, &forcing, &permeability, 0, &cancellation)
            .unwrap();
        for role in state.active_roles() {
            for ((local, transport), full) in scalar
                .temperature_tendency_k_s(*role)
                .unwrap()
                .iter()
                .zip(fast.temperature_tendency_k_s(*role).unwrap())
                .zip(full.temperature_tendency_k_s(*role).unwrap())
            {
                assert_eq!(*local + *transport, *full);
            }
        }
        assert_eq!(
            scalar.specific_humidity_tendency_s_inv(),
            full.specific_humidity_tendency_s_inv()
        );
        assert_eq!(
            scalar.upper_specific_humidity_tendency_s_inv(),
            full.upper_specific_humidity_tendency_s_inv()
        );
        assert_eq!(scalar.evaporation_rate_mm_s(), full.evaporation_rate_mm_s());
        assert_eq!(
            scalar.precipitation_rate_mm_s(),
            full.precipitation_rate_mm_s()
        );
        assert_eq!(
            scalar.orographic_precipitation_rate_mm_s(),
            full.orographic_precipitation_rate_mm_s()
        );
        assert_eq!(
            scalar.external_radiative_heat_flux_w_m2(),
            full.external_radiative_heat_flux_w_m2()
        );
        assert_eq!(supplied, scalar);
    }

    #[test]
    fn axisymmetric_circulation_is_thermally_causal_and_not_band_authored() {
        let grid = CubedSphereGrid::new(8, 6_371_000.0).unwrap();
        let uniform = vec![[15.0_f32; CLIMATE_MONTH_COUNT]; grid.cell_count()];
        let uniform_diagnostic =
            diagnose_axisymmetric_circulation(&grid, &uniform, &BuildCancellation::new()).unwrap();
        assert_eq!(uniform_diagnostic.equator_to_pole_contrast_k, 0.0);
        assert_eq!(uniform_diagnostic.eddy_velocity_scale_m_s, 0.0);
        for cell in grid.cells() {
            assert_eq!(
                uniform_diagnostic.reynolds_stress_zonal_acceleration_m_s2(cell.center_unit()),
                0.0
            );
        }

        let contrast_k = 4.0_f64;
        let equilibrium = grid
            .cells()
            .iter()
            .map(|cell| {
                let sin_latitude = cell.center_unit()[2];
                [(18.0 - contrast_k * sin_latitude * sin_latitude) as f32; CLIMATE_MONTH_COUNT]
            })
            .collect::<Vec<_>>();
        let diagnostic =
            diagnose_axisymmetric_circulation(&grid, &equilibrium, &BuildCancellation::new())
                .unwrap();
        assert!((diagnostic.equator_to_pole_contrast_k - contrast_k).abs() <= 1.0e-4);
        let seasonal = grid
            .cells()
            .iter()
            .map(|cell| {
                let sine_square = cell.center_unit()[2].powi(2);
                std::array::from_fn(|month| {
                    let monthly_contrast = if month < CLIMATE_MONTH_COUNT / 2 {
                        0.0
                    } else {
                        2.0 * contrast_k
                    };
                    (18.0 - monthly_contrast * sine_square) as f32
                })
            })
            .collect::<Vec<_>>();
        let seasonal =
            diagnose_axisymmetric_circulation(&grid, &seasonal, &BuildCancellation::new()).unwrap();
        assert!((seasonal.equator_to_pole_contrast_k - contrast_k).abs() <= 1.0e-4);
        let total_area_m2 = grid.cells().iter().map(|cell| cell.area_m2()).sum::<f64>();
        let reference_temperature_k = grid
            .cells()
            .iter()
            .zip(&equilibrium)
            .map(|(cell, months)| cell.area_m2() * (f64::from(months[0]) + 273.15))
            .sum::<f64>()
            / total_area_m2;
        let expected_eddy_scale = (9.806_65 * 10_000.0 * diagnostic.equator_to_pole_contrast_k
            / reference_temperature_k)
            .sqrt();
        assert!((diagnostic.eddy_velocity_scale_m_s - expected_eddy_scale).abs() <= 1.0e-12);

        let stress_convergence_transition_rad = (1.0_f64 / 3.0_f64.sqrt()).asin();
        let tropical = [
            (0.5 * stress_convergence_transition_rad).cos(),
            0.0,
            (0.5 * stress_convergence_transition_rad).sin(),
        ];
        let extratropical = [
            (1.5 * stress_convergence_transition_rad).cos(),
            0.0,
            (1.5 * stress_convergence_transition_rad).sin(),
        ];
        assert!(diagnostic.reynolds_stress_zonal_acceleration_m_s2(tropical) < 0.0);
        assert!(diagnostic.reynolds_stress_zonal_acceleration_m_s2(extratropical) > 0.0);

        let stronger = equilibrium
            .iter()
            .enumerate()
            .map(|(cell, _)| {
                let sin_latitude = grid.cells()[cell].center_unit()[2];
                [(18.0 - 2.0 * contrast_k * sin_latitude * sin_latitude) as f32;
                    CLIMATE_MONTH_COUNT]
            })
            .collect::<Vec<_>>();
        let stronger =
            diagnose_axisymmetric_circulation(&grid, &stronger, &BuildCancellation::new()).unwrap();
        assert!(stronger.eddy_velocity_scale_m_s > diagnostic.eddy_velocity_scale_m_s);
        assert!(
            stronger
                .reynolds_stress_zonal_acceleration_m_s2(extratropical)
                .abs()
                > diagnostic
                    .reynolds_stress_zonal_acceleration_m_s2(extratropical)
                    .abs()
        );
    }

    #[test]
    fn convective_condensation_of_converged_moisture_exports_its_latent_heat() {
        // A lower-layer flow converging on one pole piles moisture up beyond
        // the transport bound there; that surplus must fall as convective
        // precipitation whose latent heat is booked as an external export
        // instead of warming the lower layer, so the energy ledger closes on
        // radiation plus that export alone.
        let grid = CubedSphereGrid::new(4, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[25.0; CLIMATE_MONTH_COUNT]; count],
            vec![[25.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.014; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let velocity: Vec<[f32; 3]> = grid
            .cells()
            .iter()
            .map(|cell| {
                let radial = cell.center_unit();
                let axial = [0.0, 0.0, 10.0];
                let along = radial.iter().zip(axial).map(|(r, a)| r * a).sum::<f64>();
                [
                    (axial[0] - along * radial[0]) as f32,
                    (axial[1] - along * radial[1]) as f32,
                    (axial[2] - along * radial[2]) as f32,
                ]
            })
            .collect();
        state
            .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap()
            .copy_from_slice(&velocity);
        let system = LayeredTendencySystem::new(&grid);
        let permeability = vec![1.0_f32; grid.edges().len()];
        let tendency = system
            .evaluate_for_step(
                &state,
                &forcing,
                &permeability,
                0,
                7_200.0,
                &BuildCancellation::new(),
            )
            .unwrap();
        let convective_export_w = grid
            .cells()
            .iter()
            .zip(tendency.convective_precipitation_rate_mm_s())
            .map(|(cell, rate)| {
                cell.area_m2()
                    * crate::world::natural::WATER_VAPORIZATION_LATENT_HEAT_J_KG
                    * f64::from(*rate)
            })
            .sum::<f64>();
        assert!(convective_export_w > 0.0);
        for (convective, total) in tendency
            .convective_precipitation_rate_mm_s()
            .iter()
            .zip(tendency.precipitation_rate_mm_s())
        {
            assert!(*convective >= 0.0 && *convective <= *total);
        }
        let radiative_w = grid
            .cells()
            .iter()
            .zip(tendency.external_radiative_heat_flux_w_m2())
            .map(|(cell, flux)| cell.area_m2() * flux)
            .sum::<f64>();
        let expected = crate::world::natural::GLOBAL_CIRCULATION_FORMATION_TIME_COMPRESSION
            * radiative_w
            - convective_export_w;
        assert!(
            (tendency.budget().external_heat_rate_w() - expected).abs()
                <= 1.0e-9 * expected.abs().max(1.0)
        );
    }

    #[test]
    fn axisymmetric_venting_retains_convergence_at_an_unperturbed_interface() {
        // Finite venting cannot cancel the first convergence that creates
        // its interface anomaly (Battisti et al. 1999, Eq. 10).
        let (grid, _, state) = axisymmetric_venting_fixture(0.0);
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        tendency
            .layer_mut(lower)
            .unwrap()
            .height_tendency_m_s
            .fill(1.0e-3);
        tendency
            .layer_mut(upper)
            .unwrap()
            .height_tendency_m_s
            .fill(-1.0e-3);
        let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
        close_axisymmetric_baroclinic_thickness(&grid, &state, &mut workspace, &mut tendency);
        let exchange = tendency.overturning_exchange_m_s.take().unwrap();
        LayeredTendencySystem::new(&grid)
            .apply_declared_overturning_tendency(
                &state,
                &exchange,
                &BuildCancellation::new(),
                &mut tendency,
            )
            .unwrap();
        for (&lower, &upper) in tendency
            .height_tendency_m_s(lower)
            .unwrap()
            .iter()
            .zip(tendency.height_tendency_m_s(upper).unwrap())
        {
            assert_eq!(lower + upper, 0.0, "the column source remains internal");
            assert_eq!(
                lower, 1.0e-3,
                "zero interface anomaly must retain its initial convergence"
            );
        }
    }

    #[test]
    fn axisymmetric_venting_leaves_nonaxisymmetric_interfaces_untouched() {
        let (grid, _, mut state) = axisymmetric_venting_fixture(0.0);
        for (cell, geometry) in grid.cells().iter().enumerate() {
            let height = (100.0 * geometry.center_unit()[0]) as f32;
            state
                .height_anomaly_m_mut(ClimateLayerRole::LowerAtmosphere)
                .unwrap()[cell] = height;
            state
                .height_anomaly_m_mut(ClimateLayerRole::UpperAtmosphere)
                .unwrap()[cell] = -height;
        }
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
        close_axisymmetric_baroclinic_thickness(&grid, &state, &mut workspace, &mut tendency);
        assert!(tendency
            .overturning_exchange_m_s
            .unwrap()
            .iter()
            .all(|rate| rate.abs() <= 1.0e-12));
    }

    #[test]
    fn axisymmetric_venting_relaxes_an_existing_interface_without_convergence() {
        let (grid, forcing, mut state) = axisymmetric_venting_fixture(0.0);
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        state.height_anomaly_m_mut(lower).unwrap().fill(100.0);
        state.height_anomaly_m_mut(upper).unwrap().fill(-100.0);
        let step = super::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS;
        let tendency = LayeredTendencySystem::new(&grid)
            .evaluate_for_step(
                &state,
                &forcing,
                &vec![1.0; grid.edges().len()],
                0,
                step,
                &BuildCancellation::new(),
            )
            .unwrap();
        for (cell, &exchange) in tendency
            .overturning_exchange_m_s
            .as_ref()
            .unwrap()
            .iter()
            .enumerate()
        {
            assert!(
                exchange > 0.0,
                "an existing interface must vent without fresh convergence"
            );
            assert!(
                step * exchange < 100.0,
                "finite venting preserves a nonzero interface"
            );
            assert!(
                (exchange * super::BOUNDARY_LAYER_DRY_VENTING_SECONDS - 100.0).abs()
                    <= 8.0 * f64::EPSILON * 100.0,
                "dry venting uses the uncompressed author timescale"
            );
            assert_eq!(
                tendency.height_tendency_m_s(lower).unwrap()[cell],
                -tendency.height_tendency_m_s(upper).unwrap()[cell],
                "venting is internal to each atmosphere column"
            );
        }
    }

    #[test]
    fn axisymmetric_venting_responds_to_the_production_moisture_budget() {
        // Identical geometry, velocities and interface; only the surface
        // moisture supply differs. Neither initial column stores vapour or
        // precipitates, so rain itself cannot be the wet/dry switch.
        let exchanges = [0.0, 1.0].map(|availability| {
            let (grid, forcing, mut state) = axisymmetric_venting_fixture(availability);
            let lower = ClimateLayerRole::LowerAtmosphere;
            let upper = ClimateLayerRole::UpperAtmosphere;
            state.height_anomaly_m_mut(lower).unwrap().fill(100.0);
            state.height_anomaly_m_mut(upper).unwrap().fill(-100.0);
            for (cell, geometry) in grid.cells().iter().enumerate() {
                let radial = geometry.center_unit();
                state.velocity_m_s_mut(lower).unwrap()[cell] =
                    [(-5.0 * radial[1]) as f32, (5.0 * radial[0]) as f32, 0.0];
            }
            LayeredTendencySystem::new(&grid)
                .evaluate_for_step(
                    &state,
                    &forcing,
                    &vec![1.0; grid.edges().len()],
                    0,
                    super::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
                    &BuildCancellation::new(),
                )
                .unwrap()
                .overturning_exchange_m_s
                .unwrap()
        });
        assert!(
            exchanges[1]
                .iter()
                .zip(&exchanges[0])
                .any(|(wet, dry)| wet > dry),
            "the production evaporation budget must select faster moist venting"
        );
    }

    #[test]
    fn surface_saturated_air_cannot_evaporate_by_reinterpreting_its_temperature() {
        // A5 stores near-surface q: air warmer than the sea can be unsaturated
        // locally while already having the sea's saturation humidity. Exercise
        // the production source stage without transport changing that premise.
        let (grid, forcing, mut state) = axisymmetric_venting_fixture(1.0);
        let lower = ClimateLayerRole::LowerAtmosphere;
        let surface = ClimateLayerRole::OceanMixedLayer;
        let surface_temperature_c = 15.0;
        let air_temperature_c = 25.0;
        let saturation =
            crate::world::natural::saturation_specific_humidity_kg_kg(surface_temperature_c);
        let mut humidity = saturation as f32;
        // Store saturation on its upper f32 neighbour so rounding cannot
        // manufacture a positive deficit in the direct bulk oracle.
        if f64::from(humidity) < saturation {
            humidity = super::next_f32_up(humidity);
        }
        state.specific_humidity_mut().fill(humidity);
        state
            .temperature_c_mut(lower)
            .unwrap()
            .fill(air_temperature_c as f32);
        state
            .temperature_c_mut(surface)
            .unwrap()
            .fill(surface_temperature_c as f32);
        for (cell, geometry) in grid.cells().iter().enumerate() {
            state.velocity_m_s_mut(lower).unwrap()[cell] =
                tangentize([5.0, 3.0, 1.0], geometry.center_unit())
                    .map(|component| component as f32);
        }
        let system = LayeredTendencySystem::new(&grid);
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        system
            .apply_moisture(
                &state,
                &forcing,
                &vec![[0.0; 3]; grid.cell_count()],
                state.specific_humidity(),
                &vec![0.0; grid.cell_count()],
                super::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
                &mut tendency,
                &BuildCancellation::new(),
            )
            .unwrap();
        for cell in 0..grid.cell_count() {
            // The fixture leaves ocean currents zero; this is the same
            // reconstructed relative surface wind consumed by apply_moisture.
            let (surface_wind, _) = system.atmospheric_surface_wind(&state, cell).unwrap();
            let speed = super::norm(surface_wind);
            assert!(speed > 0.0);
            let expected = super::bulk_surface_evaporation_kg_m2_s(
                surface_temperature_c,
                f64::from(humidity),
                speed,
                f64::from(forcing.surface_moisture_availability()[cell]),
            );
            assert_eq!(expected, 0.0);
            assert_eq!(
                tendency.evaporation_rate_mm_s()[cell],
                expected as f32,
                "cell {cell}: warm near-surface air already saturates the colder sea"
            );
        }
    }

    fn axisymmetric_venting_fixture(
        moisture_availability: f32,
    ) -> (CubedSphereGrid, PlanetForcing, LayeredClimateState) {
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![moisture_availability; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        (grid, forcing, state)
    }

    #[test]
    fn single_layer_mechanical_exchange_preserves_reference_mass_contract() {
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let lower = ClimateLayerRole::LowerAtmosphere;
        let mixed = ClimateLayerRole::OceanMixedLayer;
        state.height_anomaly_m_mut(lower).unwrap().fill(-3_000.0);
        for (cell, geometry) in grid.cells().iter().enumerate() {
            state.velocity_m_s_mut(lower).unwrap()[cell] =
                tangentize([6.0, 0.0, 0.0], geometry.center_unit()).map(|v| v as f32);
        }
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        LayeredTendencySystem::new(&grid)
            .apply_pair_momentum_exchanges(
                &state,
                &forcing,
                &BuildCancellation::new(),
                &mut tendency,
            )
            .unwrap();
        // C1 advances linear reference-depth continuity, unlike C2's
        // conservative actual-depth momentum. Its exchange must keep that mass.
        for cell in 0..count {
            for component in 0..3 {
                let first = super::mass_per_area(&state, lower)
                    * tendency.velocity_tendency_m_s2(lower).unwrap()[cell][component];
                let second = super::mass_per_area(&state, mixed)
                    * tendency.velocity_tendency_m_s2(mixed).unwrap()[cell][component];
                assert!(
                    (first + second).abs()
                        <= super::PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE
                            * (first.abs() + second.abs())
                );
            }
        }
    }

    #[test]
    fn surface_stress_pair_quantization_preserves_dissipation() {
        // Candidate regression: independently balanced surface pairs can
        // perturb their transpose weights enough to reverse the small net
        // work when the reconstructed surface wind nearly cancels.
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let layers = [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
            ClimateLayerRole::OceanMixedLayer,
        ]
        .map(|role| {
            layout
                .layers()
                .iter()
                .find(|layer| layer.role() == role)
                .unwrap()
        });
        let masses = layers.map(|layer| layer.density_kg_m3() * layer.reference_thickness_m());
        let weights = crate::world::natural::atmosphere_surface_wind_weights(
            layers[0].reference_thickness_m(),
            layers[1].reference_thickness_m(),
        );
        let velocities = [10.0_f32, 26.6637_f32, 0.0_f32];
        let surface = crate::world::natural::reconstruct_atmosphere_surface_wind_m_s(
            [velocities[0], 0.0, 0.0],
            [velocities[1], 0.0, 0.0],
            weights,
        );
        let conductance = crate::world::natural::P4_REFERENCE_AIR_DENSITY_KG_M3
            * crate::world::natural::neutral_surface_momentum_transfer_velocity_m_s(super::norm(
                surface,
            ));
        let stress = conductance * surface[0];
        let mut targets = [1.0e-5_f32, -1.0e-5_f32, 0.0].map(f64::from);
        let deltas = super::add_surface_stress_component(&mut targets, masses, weights, stress);
        let retained_forces: [f64; 3] = std::array::from_fn(|side| masses[side] * deltas[side]);
        let desired_forces = [-weights[0] * stress, -weights[1] * stress, stress];
        let scale = 0.5 * retained_forces.iter().map(|force| force.abs()).sum::<f64>();
        assert!(
            retained_forces.iter().sum::<f64>().abs()
                <= super::PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE * scale
        );
        for (retained, desired) in retained_forces.into_iter().zip(desired_forces) {
            assert!(
                (retained - desired).abs()
                    <= super::PAIRED_EXCHANGE_RELATIVE_FLUX_ACCURACY * desired.abs()
            );
        }
        let retained_power: f64 = retained_forces
            .iter()
            .zip(velocities)
            .map(|(force, velocity)| force * f64::from(velocity))
            .sum();
        let requested_power = -conductance * dot(surface, surface);
        assert!(requested_power < 0.0);
        assert!(
            retained_power <= 0.0,
            "surface quantization adds energy: retained={retained_power}, requested={requested_power}"
        );
    }

    #[test]
    fn surface_stress_retains_exchange_below_f32_baseline_resolution() {
        // This physical force was lost in the air but resolved in the ocean
        // when each source was composed onto an f32 tendency. Accumulating
        // before state quantization must preserve all three reactions.
        let weights = crate::world::natural::atmosphere_surface_wind_weights(3.0, 2.0);
        let mut targets = [1.0_f64, 1.0, 0.0];
        let forces = super::add_surface_stress_component(&mut targets, [1.0; 3], weights, 1.0e-10);
        assert!(forces[0] < 0.0 && forces[1] > 0.0 && forces[2] > 0.0);
        let scale = 0.5 * forces.iter().map(|force| force.abs()).sum::<f64>();
        assert!(
            forces.iter().sum::<f64>().abs()
                <= super::PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE * scale
        );
        for (retained, weight) in forces.into_iter().zip([-weights[0], -weights[1], 1.0]) {
            let desired = weight * 1.0e-10;
            assert!(
                (retained - desired).abs()
                    <= super::PAIRED_EXCHANGE_RELATIVE_FLUX_ACCURACY * desired.abs()
            );
        }
    }

    #[test]
    fn ocean_surface_stress_uses_reconstructed_surface_wind() {
        // One uniform column distinguishes surface wind from the lower-layer
        // mean: the prescribed shear reverses the reconstructed surface wind.
        // Internal air exchange cannot supply the ocean's reaction force.
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let lower = ClimateLayerRole::LowerAtmosphere;
        let upper = ClimateLayerRole::UpperAtmosphere;
        let mixed = ClimateLayerRole::OceanMixedLayer;
        for (role, speed) in [(lower, 1.0), (upper, 4.0)] {
            for (cell, geometry) in grid.cells().iter().enumerate() {
                state.velocity_m_s_mut(role).unwrap()[cell] =
                    tangentize([speed, 0.0, 0.0], geometry.center_unit()).map(|v| v as f32);
            }
        }
        state.height_anomaly_m_mut(mixed).unwrap().fill(50.0);
        let cell = state
            .velocity_m_s(lower)
            .unwrap()
            .iter()
            .position(|velocity| super::norm(velocity.map(f64::from)) > 0.0)
            .unwrap();
        let system = LayeredTendencySystem::new(&grid);
        let weights = crate::world::natural::atmosphere_surface_wind_weights(
            system.fluid_layer_thickness_m(&state, lower, cell).unwrap(),
            system.fluid_layer_thickness_m(&state, upper, cell).unwrap(),
        );
        let surface = crate::world::natural::reconstruct_atmosphere_surface_wind_m_s(
            state.velocity_m_s(lower).unwrap()[cell],
            state.velocity_m_s(upper).unwrap()[cell],
            weights,
        );
        assert!(
            dot(
                surface,
                state.velocity_m_s(lower).unwrap()[cell].map(f64::from)
            ) < 0.0
        );
        let mixed_mass = system
            .momentum_mass_per_area(&state, mixed, super::mass_per_area(&state, mixed), cell)
            .unwrap();
        let bulk_scale = crate::world::natural::P4_REFERENCE_AIR_DENSITY_KG_M3
            * crate::world::natural::neutral_surface_momentum_transfer_velocity_m_s(super::norm(
                surface,
            ));
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        system
            .apply_pair_momentum_exchanges(
                &state,
                &forcing,
                &BuildCancellation::new(),
                &mut tendency,
            )
            .unwrap();
        // Remove the independently dissipative air/air pair: it must not hide
        // a surface-force projection that adds kinetic energy to the column.
        let atmosphere_masses = [lower, upper].map(|role| {
            system
                .momentum_mass_per_area(&state, role, super::mass_per_area(&state, role), cell)
                .unwrap()
        });
        let internal = layout
            .exchange(lower, upper)
            .and_then(|exchange| exchange.momentum_exchange_time_s())
            .map_or([[0.0; 3]; 2], |timescale| {
                let exchange = super::paired_momentum_exchange(
                    state.velocity_m_s(lower).unwrap()[cell].map(f64::from),
                    state.velocity_m_s(upper).unwrap()[cell].map(f64::from),
                    atmosphere_masses[0],
                    atmosphere_masses[1],
                    timescale,
                )
                .unwrap();
                [
                    exchange.first_acceleration_m_s2,
                    exchange.second_acceleration_m_s2,
                ]
            });
        let mut surface_power = 0.0;
        for (index, (role, internal_acceleration)) in [(lower, internal[0]), (upper, internal[1])]
            .into_iter()
            .enumerate()
        {
            let acceleration = std::array::from_fn(|component| {
                tendency.velocity_tendency_m_s2(role).unwrap()[cell][component]
                    - internal_acceleration[component]
            });
            surface_power += atmosphere_masses[index]
                * dot(
                    state.velocity_m_s(role).unwrap()[cell].map(f64::from),
                    acceleration,
                );
        }
        assert!(
            surface_power < 0.0,
            "surface source adds kinetic energy: {surface_power}"
        );
        for (component, surface_component) in surface.into_iter().enumerate() {
            let expected = bulk_scale * surface_component / mixed_mass;
            let actual = tendency.velocity_tendency_m_s2(mixed).unwrap()[cell][component];
            assert!(
                (actual - expected).abs()
                    <= super::PAIRED_EXCHANGE_RELATIVE_FLUX_ACCURACY * expected.abs(),
                "surface stress: actual={actual}, expected={expected}"
            );
            let mut force = 0.0;
            let mut absolute_force = 0.0;
            for role in state.active_roles() {
                let mass = system
                    .momentum_mass_per_area(
                        &state,
                        *role,
                        super::mass_per_area(&state, *role),
                        cell,
                    )
                    .unwrap();
                let retained =
                    mass * tendency.velocity_tendency_m_s2(*role).unwrap()[cell][component];
                force += retained;
                absolute_force += retained.abs();
            }
            assert!(
                force.abs() <= super::PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE * absolute_force,
                "surface stress breaks actual-mass balance: {force} / {absolute_force}"
            );
        }
    }

    #[test]
    fn mechanical_pairs_conserve_actual_mass_with_surface_and_ocean_exchange() {
        // A single all-water column exercises all three mechanical interfaces;
        // the full climate solve is unnecessary to catch reference-mass forces.
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        for (role, anomaly, speed) in [
            (ClimateLayerRole::LowerAtmosphere, -3_000.0, 6.0),
            (ClimateLayerRole::UpperAtmosphere, 3_000.0, 6.0),
            (ClimateLayerRole::OceanMixedLayer, 50.0, 0.0),
            (ClimateLayerRole::OceanThermocline, -50.0, 1.0),
        ] {
            state.height_anomaly_m_mut(role).unwrap().fill(anomaly);
            for (cell, geometry) in grid.cells().iter().enumerate() {
                state.velocity_m_s_mut(role).unwrap()[cell] =
                    tangentize([speed, 0.0, 0.0], geometry.center_unit()).map(|v| v as f32);
            }
        }
        let system = LayeredTendencySystem::new(&grid);
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        system
            .apply_pair_momentum_exchanges(
                &state,
                &forcing,
                &BuildCancellation::new(),
                &mut tendency,
            )
            .unwrap();
        for cell in 0..count {
            for component in 0..3 {
                let mut force = 0.0;
                let mut absolute_force = 0.0;
                for layer in layout
                    .layers()
                    .iter()
                    .filter(|layer| layer.dynamically_active())
                {
                    let mass = layer.density_kg_m3()
                        * system
                            .fluid_layer_thickness_m(&state, layer.role(), cell)
                            .unwrap();
                    let retained = mass
                        * tendency.velocity_tendency_m_s2(layer.role()).unwrap()[cell][component];
                    force += retained;
                    absolute_force += retained.abs();
                }
                assert!(
                    force.abs()
                        <= super::PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE * absolute_force,
                    "actual mechanical force is not balanced: {force} / {absolute_force}"
                );
            }
        }
    }

    #[test]
    fn reynolds_stress_closure_is_pole_regular_and_axial_torque_neutral_after_quantization() {
        let grid = CubedSphereGrid::new(3, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let equilibrium = grid
            .cells()
            .iter()
            .map(|cell| [(18.0 - 54.0 * cell.center_unit()[2].powi(2)) as f32; CLIMATE_MONTH_COUNT])
            .collect::<Vec<_>>();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            equilibrium.clone(),
            equilibrium,
            vec![[0.008; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        // Latitude-dependent layer mass must not turn internal eddy transport
        // into a net torque. Constant reference depths hide this regression.
        for (cell, geometry) in grid.cells().iter().enumerate() {
            let anomaly = (2_000.0 * geometry.center_unit()[2].powi(2)) as f32;
            state
                .height_anomaly_m_mut(ClimateLayerRole::LowerAtmosphere)
                .unwrap()[cell] = -anomaly;
            state
                .height_anomaly_m_mut(ClimateLayerRole::UpperAtmosphere)
                .unwrap()[cell] = anomaly;
        }
        let system = LayeredTendencySystem::new(&grid);
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        apply_baroclinic_reynolds_stress_closure(
            &system,
            &state,
            &forcing,
            &mut tendency,
            &mut vec![0.0; count],
            &BuildCancellation::new(),
        )
        .unwrap();

        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            let acceleration = tendency.velocity_tendency_m_s2(role).unwrap();
            let layer = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == role)
                .unwrap();
            let mut signed_torque = 0.0_f64;
            let mut absolute_torque = 0.0_f64;
            let mut tropical = (0.0_f64, 0_u32);
            let mut extratropical = (0.0_f64, 0_u32);
            let transition = (1.0_f64 / 3.0_f64.sqrt()).asin();
            for (index, (cell, value)) in grid.cells().iter().zip(acceleration).enumerate() {
                let layer_mass_per_area = layer.density_kg_m3()
                    * system.fluid_layer_thickness_m(&state, role, index).unwrap();
                assert!(value.iter().all(|component| component.is_finite()));
                let radial = cell.center_unit();
                let cosine = radial[0].hypot(radial[1]);
                if cosine <= f64::EPSILON.sqrt() {
                    assert!(value.iter().all(|component| *component == 0.0));
                    continue;
                }
                let east = [-radial[1] / cosine, radial[0] / cosine, 0.0];
                let zonal = dot(*value, east);
                let absolute_latitude = radial[2].asin().abs();
                if absolute_latitude < 0.5 * transition {
                    tropical.0 += zonal;
                    tropical.1 += 1;
                } else if absolute_latitude > 1.5 * transition {
                    extratropical.0 += zonal;
                    extratropical.1 += 1;
                }
                let torque =
                    cell.area_m2() * layer_mass_per_area * grid.radius_m() * cosine * zonal;
                signed_torque += torque;
                absolute_torque += torque.abs();
            }
            assert!(tropical.1 > 0 && tropical.0 < 0.0);
            assert!(extratropical.1 > 0 && extratropical.0 > 0.0);
            assert!(absolute_torque > 0.0);
            assert!(signed_torque.abs() / absolute_torque <= 1.0e-6);
        }
    }

    #[test]
    fn a_transport_floor_correction_is_not_relabelled_as_external_evaporation() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; grid.cell_count()],
            vec![0.0; grid.cell_count()],
            vec![0.0; grid.cell_count()],
            vec![1.0; grid.cell_count()],
            vec![[240.0; CLIMATE_MONTH_COUNT]; grid.cell_count()],
            vec![[15.0; CLIMATE_MONTH_COUNT]; grid.cell_count()],
            vec![[15.0; CLIMATE_MONTH_COUNT]; grid.cell_count()],
            vec![[0.01; CLIMATE_MONTH_COUNT]; grid.cell_count()],
        )
        .unwrap();
        let state = LayeredClimateState::from_forcing(
            &grid,
            &ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1),
            &forcing,
            0,
        )
        .unwrap();
        let step_seconds = 7_200.0;
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        let exact_floor = (-f64::from(state.specific_humidity()[0]) / step_seconds) as f32;
        tendency.specific_humidity_tendency_s_inv[0] = next_f32_down(exact_floor);

        tendency
            .enforce_moisture_availability(&state, step_seconds, &BuildCancellation::new())
            .unwrap();

        assert!(
            f64::from(state.specific_humidity()[0])
                + step_seconds * f64::from(tendency.specific_humidity_tendency_s_inv[0])
                >= 0.0
        );
        assert_eq!(tendency.budget.external_moisture_source_rate_kg_s, 0.0);
        assert_eq!(tendency.budget.external_precipitation_sink_rate_kg_s, 0.0);
        assert_eq!(tendency.budget.external_moisture_net_rate_kg_s(), 0.0);
    }

    #[test]
    fn ocean_latent_cooling_uses_the_actual_column_capacity() {
        // A prescribed evaporation flux isolates its heat consumer from bulk
        // aerodynamic feedback, transport, and the subsequent implicit pairs.
        let grid = CubedSphereGrid::new(1, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.25; count],
            vec![1.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.001; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let role = ClimateLayerRole::OceanMixedLayer;
        let spec = layout
            .layers()
            .iter()
            .find(|layer| layer.role() == role)
            .unwrap();
        let system = LayeredTendencySystem::new(&grid);
        let cancellation = BuildCancellation::new();
        let reference_capacity = super::cell_heat_capacity_per_area(&state, spec, 0).unwrap();
        let mut before = LayeredClimateTendency::zeroed(&state);
        before.evaporation_rate_mm_s.fill(1.0e-5);
        system
            .apply_phase_change_latent_heat(&state, &mut before, &cancellation)
            .unwrap();
        let extra_depth = state.reference_thickness_m(role).unwrap();
        state.height_anomaly_m_mut(role).unwrap().fill(extra_depth);
        let actual_capacity = super::cell_heat_capacity_per_area(&state, spec, 0).unwrap();
        assert_eq!(actual_capacity, 2.0 * reference_capacity);
        let mut after = LayeredClimateTendency::zeroed(&state);
        after
            .evaporation_rate_mm_s
            .copy_from_slice(&before.evaporation_rate_mm_s);
        system
            .apply_phase_change_latent_heat(&state, &mut after, &cancellation)
            .unwrap();
        for (old, new) in before
            .temperature_tendency_k_s(role)
            .unwrap()
            .iter()
            .zip(after.temperature_tendency_k_s(role).unwrap())
        {
            assert!(*old < 0.0);
            assert_eq!(*old, 2.0 * *new);
        }
    }

    #[test]
    fn mixed_layer_temperature_transport_does_not_depend_on_temperature_origin() {
        // Reuse the atmospheric origin regression's nonuniform scalar and
        // divergent tangent flow, now through the actual ocean consumer.
        // A small origin shift keeps both fixtures inside the physical state
        // bounds. The production transport stage is isolated from local heat, so its
        // output isolates this contract from radiation and pending pair heat.
        let grid = CubedSphereGrid::new(4, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.25; count],
            vec![0.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let system = LayeredTendencySystem::new(&grid);
        let cancellation = BuildCancellation::new();
        let step = super::GLOBAL_CIRCULATION_MACRO_STEP_SECONDS;
        let role = ClimateLayerRole::OceanMixedLayer;
        let transport_endpoints = [0.0, 10.0].map(|origin| {
            let mut state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
            for (cell, geometry) in grid.cells().iter().enumerate() {
                let radial = geometry.center_unit();
                state.temperature_c_mut(role).unwrap()[cell] =
                    (15.0 + 4.0 * radial[0] - 2.0 * radial[2] + origin) as f32;
                state.velocity_m_s_mut(role).unwrap()[cell] =
                    tangentize([0.0, 0.0, 1.0], radial).map(|value| value as f32);
            }
            let mut workspace = LayeredTendencyWorkspace::for_grid(&grid);
            system
                .temperature_transport_tendency_for_step(
                    &state,
                    (role, &vec![1.0; grid.edges().len()]),
                    step,
                    &mut workspace,
                    &cancellation,
                )
                .unwrap();
            let endpoint = workspace
                .scalar_scratch
                .iter()
                .map(|&rate| step * f64::from(rate))
                .collect::<Vec<_>>();
            let spec = layout
                .layers()
                .iter()
                .find(|layer| layer.role() == role)
                .unwrap();
            let capacity = super::cell_heat_capacity_per_area(&state, spec, 0).unwrap();
            let fast = system
                .evaluate_fast(
                    &state,
                    &forcing,
                    &vec![1.0; grid.edges().len()],
                    0,
                    &cancellation,
                )
                .unwrap();
            let heat_change = grid
                .cells()
                .iter()
                .zip(&endpoint)
                .enumerate()
                .map(|(index, (cell, delta))| {
                    let height = system.fluid_layer_thickness_m(&state, role, index).unwrap();
                    cell.area_m2()
                        * capacity
                        * (delta
                            + step
                                * f64::from(state.temperature_c(role).unwrap()[index])
                                * f64::from(fast.height_tendency_m_s(role).unwrap()[index])
                                / height)
                })
                .sum::<f64>();
            let heat_scale = grid
                .cells()
                .iter()
                .zip(state.temperature_c(role).unwrap())
                .map(|(cell, value)| cell.area_m2() * capacity * f64::from(*value).abs())
                .sum::<f64>();
            assert!(heat_change.abs() <= 8.0 * f64::from(f32::EPSILON) * heat_scale);
            state
                .temperature_c_mut(role)
                .unwrap()
                .fill((15.0 + origin) as f32);
            system
                .temperature_transport_tendency_for_step(
                    &state,
                    (role, &vec![1.0; grid.edges().len()]),
                    step,
                    &mut workspace,
                    &cancellation,
                )
                .unwrap();
            assert!(workspace.scalar_scratch.iter().all(|rate| *rate == 0.0));
            endpoint
        });
        // Initial states and composed tendencies are f32; this is a rounding
        // allowance at the larger temperature origin, not a climate tolerance.
        let tolerance = 8.0 * f64::from(f32::EPSILON) * 31.0;
        let maximum_error = transport_endpoints[0]
            .iter()
            .zip(&transport_endpoints[1])
            .map(|(first, second)| (first - second).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            maximum_error <= tolerance,
            "mixed-layer temperature-origin error {maximum_error} K exceeds {tolerance}"
        );
    }
}
