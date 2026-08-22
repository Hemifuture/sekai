use thiserror::Error;

use super::state::{
    LayeredClimateState, LayeredStateError, DEEP_OCEAN_EQUILIBRIUM_OFFSET_C,
    LIQUID_MIXED_LAYER_MIN_C, OCEAN_EQUILIBRIUM_MAX_C, SUBSURFACE_OCEAN_MIN_C,
    THERMOCLINE_EQUILIBRIUM_OFFSET_C, UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C,
    UPPER_SPECIFIC_HUMIDITY_INITIAL_FRACTION,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{
    CirculationOperatorError, CirculationOperators, CubedSphereGrid, SecondOrderTransportWorkspace,
};
use crate::world::natural::{
    ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, ForcingError, PlanetForcing,
    CLIMATE_MONTH_COUNT, GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
};

const EARTH_ROTATION_RATE_RAD_S: f64 = 7.292_115_9e-5;
const STANDARD_GRAVITY_M_S2: f64 = 9.806_65;
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
const BATHYMETRIC_BOTTOM_DRAG_TIMESCALE_S: f64 = 90.0 * SECONDS_PER_DAY;
const BATHYMETRIC_BOTTOM_DRAG_REFERENCE_DEPTH_M: f64 = 1_000.0;
const OROGRAPHIC_CONDENSATION_DEPTH_M: f64 = 800.0;
const OROGRAPHIC_UPLIFT_MAX_M_S: f64 = 0.02;
// Horizontal sub-grid mixing closes unresolved baroclinic eddies and prevents
// grid-scale velocity fronts. The finite-volume conductance below makes these
// resolution-independent physical diffusivities rather than per-cell filters.
const ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S: f64 = 1_000_000.0;
const OCEAN_HORIZONTAL_EDDY_VISCOSITY_M2_S: f64 = 1_000.0;
// Effective hypsometric pressure couplings for the fixed 6 km / 4 km layers.
// The upper amplitude stays below the layer-depth validity bound; the lower
// amplitude is derived so the first baroclinic mode has zero column-integrated
// internal pressure force.
const LOWER_ATMOSPHERE_REFERENCE_THICKNESS_M: f64 = 6_000.0;
const UPPER_ATMOSPHERE_REFERENCE_THICKNESS_M: f64 = 4_000.0;
const C1_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K: f64 = 30.0;
const UPPER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K: f64 = 25.0;
const C2_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K: f64 = UPPER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K
    * UPPER_ATMOSPHERE_REFERENCE_THICKNESS_M
    / LOWER_ATMOSPHERE_REFERENCE_THICKNESS_M;
// The accelerated formation solve cannot resolve a multi-hundred-day
// synoptic-eddy spin-up. Its monthly-mean atmospheric momentum closure uses
// the annual-mean available-potential-energy velocity scale in a regular
// column-distributed Reynolds stress. The analytic stress converges eastward
// momentum outside the tropics and diverges it inside, while its spherical
// divergence has zero global axial torque in each resolved atmosphere layer.
const ATMOSPHERE_COLUMN_DEPTH_M: f64 = 10_000.0;
// Eady activity vanishes with |f| at the equator. U_e is total horizontal
// eddy speed. Because max(|sin(phi)|^2 cos(phi)^2)=1/4, C=2/3 retains at most
// U_e^2/6, one third of the Cauchy bound |u'v'|<=0.5 U_e^2.
const BAROCLINIC_REYNOLDS_STRESS_EFFICIENCY: f64 = 2.0 / 3.0;
// A retained pair must remain more tightly balanced than the public 1e-6
// exchange budget. Its magnitude may differ from the requested exchange by
// at most 0.1%; less representable cases use the bounded lattice search.
const PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE: f64 = 5.0e-7;
const PAIRED_EXCHANGE_RELATIVE_FLUX_ACCURACY: f64 = 1.0e-3;

pub(super) fn layered_equation_model_fingerprint(profile: ClimateModelProfile) -> [u8; 32] {
    let layout = ClimateLayerLayout::for_profile(profile);
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.global-circulation-equations.v3\0");
    hasher.update(&layout.fingerprint());
    for value in [
        EARTH_ROTATION_RATE_RAD_S,
        STANDARD_GRAVITY_M_S2,
        SEAWATER_THERMAL_EXPANSION_K_INV,
        MIXED_LAYER_REFERENCE_THICKNESS_M,
        COASTAL_FORM_DRAG_TIMESCALE_S,
        BATHYMETRIC_BOTTOM_DRAG_TIMESCALE_S,
        BATHYMETRIC_BOTTOM_DRAG_REFERENCE_DEPTH_M,
        OROGRAPHIC_CONDENSATION_DEPTH_M,
        OROGRAPHIC_UPLIFT_MAX_M_S,
        ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S,
        OCEAN_HORIZONTAL_EDDY_VISCOSITY_M2_S,
        C1_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K,
        C2_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K,
        UPPER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K,
        LOWER_ATMOSPHERE_REFERENCE_THICKNESS_M,
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
        5.0 * SECONDS_PER_DAY,
        3.0 * SECONDS_PER_DAY,
        GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
        super::generation::MAXIMUM_FAST_STEP_SECONDS,
        super::generation::FAST_CFL_TARGET,
        super::generation::REFERENCE_WAVE_SPEED_M_S,
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
        let (gravity, drag, height_relax, thermal_relax, thermal_gradient) =
            role_constants(profile, role);
        hasher.update(&[match role {
            ClimateLayerRole::LowerAtmosphere => 1,
            ClimateLayerRole::UpperAtmosphere => 2,
            ClimateLayerRole::OceanMixedLayer => 3,
            ClimateLayerRole::OceanThermocline => 4,
            ClimateLayerRole::DeepOceanReservoir => 5,
        }]);
        for value in [gravity, drag, height_relax, thermal_relax, thermal_gradient] {
            hasher.update(&value.to_bits().to_le_bytes());
        }
    }
    for semantic_id in [
        b"finite-volume-positive-permeability-v2".as_slice(),
        b"barth-jespersen-component-local-v2".as_slice(),
        b"split-explicit-dynamic-momentum-viscosity-rk3-v2".as_slice(),
        b"annual-mean-ape-eady-column-reynolds-stress-zero-torque-v5".as_slice(),
        b"paired-f32-exchange-projection-v2".as_slice(),
        b"depth-mean-boussinesq-steric-v1".as_slice(),
        b"resolved-temperature-pressure-gradient-v1".as_slice(),
        b"asr-limited-positive-radiative-heating-f32-projection-v1".as_slice(),
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

/// Projects an equal-and-opposite scalar exchange onto the two representable
/// `f32` tendency lattices. If an exchange is smaller than the resolution of
/// one side, retaining it on only the other side would create mass, momentum,
/// or energy. In that case the closest balanced representable pair is used;
/// an entirely unresolved exchange therefore becomes zero on both sides.
fn add_balanced_pair_to_f32(
    first: &mut f32,
    second: &mut f32,
    desired_first_delta: f64,
    first_weight: f64,
    second_weight: f64,
) -> (f64, f64) {
    add_balanced_pair_to_f32_with_tolerance(
        first,
        second,
        desired_first_delta,
        first_weight,
        second_weight,
        PAIRED_EXCHANGE_RELATIVE_BALANCE_TOLERANCE,
    )
}

fn add_balanced_pair_to_f32_with_tolerance(
    first: &mut f32,
    second: &mut f32,
    desired_first_delta: f64,
    first_weight: f64,
    second_weight: f64,
    balance_tolerance: f64,
) -> (f64, f64) {
    debug_assert!(desired_first_delta.is_finite());
    debug_assert!(first_weight.is_finite() && first_weight > 0.0);
    debug_assert!(second_weight.is_finite() && second_weight > 0.0);
    const FIRST_SEARCH_RADIUS: i32 = 8;
    const SECOND_SEARCH_RADIUS: i32 = 2;

    let first_before = *first;
    let second_before = *second;
    let desired_flux = first_weight * desired_first_delta;
    let first_center = (f64::from(first_before) + desired_first_delta) as f32;
    let second_center = (f64::from(second_before) - desired_flux / second_weight) as f32;

    // If neither representable tendency can retain the requested exchange,
    // the physically honest projection is zero on both sides.
    if first_center == first_before && second_center == second_before {
        return (0.0, 0.0);
    }

    // Accept the nearest balanced lattice pair when it meets both the strict
    // exchange budget and the locked magnitude-accuracy contract.
    let direct_first_delta = f64::from(first_center) - f64::from(first_before);
    let direct_second =
        (f64::from(second_before) - first_weight * direct_first_delta / second_weight) as f32;
    let direct_second_delta = f64::from(direct_second) - f64::from(second_before);
    let direct_first_flux = first_weight * direct_first_delta;
    let direct_second_flux = second_weight * direct_second_delta;
    let direct_scale = 0.5 * (direct_first_flux.abs() + direct_second_flux.abs());
    let direct_relative_residual = if direct_scale > 0.0 {
        (direct_first_flux + direct_second_flux).abs() / direct_scale
    } else {
        0.0
    };
    let direct_retained_flux = 0.5 * (direct_first_flux - direct_second_flux);
    let direct_flux_error = (direct_retained_flux - desired_flux).abs();
    let direct_flux_tolerance = desired_flux.abs() * PAIRED_EXCHANGE_RELATIVE_FLUX_ACCURACY;
    if direct_relative_residual <= balance_tolerance
        && (direct_flux_error <= direct_flux_tolerance || desired_flux == 0.0)
    {
        *first = first_center;
        *second = direct_second;
        return (direct_first_delta, direct_second_delta);
    }

    // Unequal-mass pairs can quantize more accurately when the second side is
    // rounded first, so try the symmetric construction before the search.
    let reverse_second_delta = f64::from(second_center) - f64::from(second_before);
    let reverse_first =
        (f64::from(first_before) - second_weight * reverse_second_delta / first_weight) as f32;
    let reverse_first_delta = f64::from(reverse_first) - f64::from(first_before);
    let reverse_first_flux = first_weight * reverse_first_delta;
    let reverse_second_flux = second_weight * reverse_second_delta;
    let reverse_scale = 0.5 * (reverse_first_flux.abs() + reverse_second_flux.abs());
    let reverse_relative_residual = if reverse_scale > 0.0 {
        (reverse_first_flux + reverse_second_flux).abs() / reverse_scale
    } else {
        0.0
    };
    let reverse_retained_flux = 0.5 * (reverse_first_flux - reverse_second_flux);
    let reverse_flux_error = (reverse_retained_flux - desired_flux).abs();
    if reverse_relative_residual <= balance_tolerance
        && (reverse_flux_error <= direct_flux_tolerance || desired_flux == 0.0)
    {
        *first = reverse_first;
        *second = second_center;
        return (reverse_first_delta, reverse_second_delta);
    }
    let mut best_first = first_before;
    let mut best_second = second_before;
    let mut best_flux_error = desired_flux.abs();
    let mut best_relative_residual = 0.0_f64;

    let mut consider_first = |first_candidate: f32| {
        let first_delta = f64::from(first_candidate) - f64::from(first_before);
        let matching_second =
            (f64::from(second_before) - first_weight * first_delta / second_weight) as f32;
        for second_offset in -SECOND_SEARCH_RADIUS..=SECOND_SEARCH_RADIUS {
            let second_candidate = offset_f32(matching_second, second_offset);
            if !second_candidate.is_finite() {
                continue;
            }
            let second_delta = f64::from(second_candidate) - f64::from(second_before);
            let first_flux = first_weight * first_delta;
            let second_flux = second_weight * second_delta;
            let scale = 0.5 * (first_flux.abs() + second_flux.abs());
            let relative_residual = if scale > 0.0 {
                (first_flux + second_flux).abs() / scale
            } else {
                0.0
            };
            if relative_residual > balance_tolerance {
                continue;
            }
            let retained_flux = 0.5 * (first_flux - second_flux);
            let flux_error = (retained_flux - desired_flux).abs();
            if flux_error < best_flux_error
                || (flux_error == best_flux_error && relative_residual < best_relative_residual)
            {
                best_first = first_candidate;
                best_second = second_candidate;
                best_flux_error = flux_error;
                best_relative_residual = relative_residual;
            }
        }
    };

    consider_first(first_before);
    for first_offset in -FIRST_SEARCH_RADIUS..=FIRST_SEARCH_RADIUS {
        consider_first(offset_f32(first_center, first_offset));
    }
    *first = best_first;
    *second = best_second;
    (
        f64::from(best_first) - f64::from(first_before),
        f64::from(best_second) - f64::from(second_before),
    )
}

fn offset_f32(mut value: f32, offset: i32) -> f32 {
    for _ in 0..offset.unsigned_abs() {
        value = if offset >= 0 {
            next_f32_up(value)
        } else {
            next_f32_down(value)
        };
    }
    value
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
    velocity_tendency_m_s2: Vec<[f32; 3]>,
    temperature_tendency_k_s: Vec<f32>,
}

/// Fully accounted instantaneous tendency shared by every time integrator.
#[derive(Debug, Clone, PartialEq)]
pub struct LayeredClimateTendency {
    active_layers: Vec<ActiveLayerTendency>,
    specific_humidity_tendency_s_inv: Vec<f32>,
    external_moisture_tendency_s_inv: Vec<f64>,
    upper_specific_humidity_tendency_s_inv: Option<Vec<f32>>,
    precipitation_rate_mm_s: Vec<f32>,
    orographic_precipitation_rate_mm_s: Vec<f32>,
    external_radiative_heat_flux_w_m2: Vec<f64>,
    deep_ocean_temperature_tendency_k_s: Option<Vec<f32>>,
    budget: LayeredTendencyBudget,
}

impl LayeredClimateTendency {
    fn zeroed(state: &LayeredClimateState) -> Self {
        let count = state.cell_count();
        Self {
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
            precipitation_rate_mm_s: vec![0.0; count],
            orographic_precipitation_rate_mm_s: vec![0.0; count],
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

    pub fn velocity_tendency_m_s2(&self, role: ClimateLayerRole) -> Option<&[[f32; 3]]> {
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

    pub fn precipitation_rate_mm_s(&self) -> &[f32] {
        &self.precipitation_rate_mm_s
    }

    pub fn orographic_precipitation_rate_mm_s(&self) -> &[f32] {
        &self.orographic_precipitation_rate_mm_s
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
        grid: &CubedSphereGrid,
        state: &LayeredClimateState,
        transported_humidity: &[f32],
        step_seconds: f64,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let atmospheric_column_mass = mass_per_area(state, ClimateLayerRole::LowerAtmosphere);
        for (
            cell,
            (
                (((external_tendency, physical_tendency), precipitation), orographic_precipitation),
                available_humidity,
            ),
        ) in self
            .external_moisture_tendency_s_inv
            .iter_mut()
            .zip(&mut self.specific_humidity_tendency_s_inv)
            .zip(&mut self.precipitation_rate_mm_s)
            .zip(&mut self.orographic_precipitation_rate_mm_s)
            .zip(transported_humidity)
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
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
            }
        }
        let mut precipitation_sink_rate_kg_s = 0.0;
        let mut external_net_rate_kg_s = 0.0;
        for (index, ((cell, precipitation), external_tendency)) in grid
            .cells()
            .iter()
            .zip(&self.precipitation_rate_mm_s)
            .zip(&self.external_moisture_tendency_s_inv)
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            precipitation_sink_rate_kg_s += cell.area_m2() * f64::from(*precipitation);
            external_net_rate_kg_s += cell.area_m2() * atmospheric_column_mass * *external_tendency;
        }
        self.budget.external_precipitation_sink_rate_kg_s = precipitation_sink_rate_kg_s;
        // Quantization is owned by the retained q tendency. Reconstruct the
        // effective evaporation source from that exact net plus the published
        // precipitation sink so the declared source-minus-sink identity is
        // bit-stable even after availability limiting.
        self.budget.external_moisture_source_rate_kg_s =
            self.budget.external_precipitation_sink_rate_kg_s + external_net_rate_kg_s;
        Ok(())
    }
}

/// One-evaluation physical-source and paired-exchange accounting.
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
}

impl LayeredTendencyBudget {
    pub const fn paired_heat_absolute_w(self) -> f64 {
        self.paired_heat_absolute_w
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
    vector_scratch: Vec<[f32; 3]>,
    transport: SecondOrderTransportWorkspace,
}

impl LayeredTendencyWorkspace {
    pub fn for_grid(grid: &CubedSphereGrid) -> Self {
        Self {
            cell_count: grid.cell_count(),
            edge_count: grid.edges().len(),
            open_edges: vec![1.0; grid.edges().len()],
            scalar_scratch: vec![0.0; grid.cell_count()],
            vector_scratch: vec![[0.0; 3]; grid.cell_count()],
            transport: SecondOrderTransportWorkspace::for_grid(grid),
        }
    }

    pub fn allocation_signature(&self) -> [usize; 15] {
        let transport = self.transport.allocation_signature();
        [
            self.open_edges.capacity(),
            self.scalar_scratch.capacity(),
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
        ]
    }
}

/// Integrator-neutral composition of dynamics, relaxation, phase change, and
/// paired vertical/surface exchanges.
#[derive(Debug, Clone, Copy)]
pub struct LayeredTendencySystem<'grid> {
    grid: &'grid CubedSphereGrid,
}

impl<'grid> LayeredTendencySystem<'grid> {
    pub const fn new(grid: &'grid CubedSphereGrid) -> Self {
        Self { grid }
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
            true,
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
            true,
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
            false,
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
        include_explicit_transport_and_moisture: bool,
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
            let height = state.height_anomaly_m(*role).expect("active role");
            let velocity = state.velocity_m_s(*role).expect("active role");
            let temperature = state.temperature_c(*role).expect("active role");
            let ocean = matches!(
                role,
                ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline
            );
            let permeability = if ocean {
                ocean_edge_permeability
            } else {
                &workspace.open_edges
            };
            let divergence = operators.divergence_with_permeability_cancellable(
                velocity,
                permeability,
                cancellation,
            )?;
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
            let thermal_gradient = operators.gradient_with_permeability_cancellable(
                temperature,
                permeability,
                cancellation,
            )?;
            let (reduced_gravity, drag_s_inv, height_relax_s, _, thermal_gradient_acceleration) =
                role_constants(state.profile(), *role);
            horizontal_velocity_diffusion(
                self.grid,
                velocity,
                permeability,
                if ocean {
                    OCEAN_HORIZONTAL_EDDY_VISCOSITY_M2_S
                } else {
                    ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S
                },
                &mut workspace.vector_scratch,
                cancellation,
            )?;
            let reference_thickness =
                f64::from(state.reference_thickness_m(*role).expect("active role"));
            if include_explicit_transport_and_moisture {
                let transported = operators.advect_scalar_monotone_second_order_into_cancellable(
                    temperature,
                    velocity,
                    permeability,
                    transport_step_seconds,
                    false,
                    &mut workspace.transport,
                    cancellation,
                )?;
                for (cell, (target, (transported, original))) in tendency
                    .layer_mut(*role)
                    .expect("active tendency role")
                    .temperature_tendency_k_s
                    .iter_mut()
                    .zip(transported.values().iter().zip(temperature))
                    .enumerate()
                {
                    if cell % 256 == 0 {
                        check_cancelled(cancellation)?;
                    }
                    *target = ((f64::from(*transported) - f64::from(*original))
                        / transport_step_seconds) as f32;
                }
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
                    layer.height_tendency_m_s[cell] =
                        (-reference_thickness * f64::from(divergence[cell])) as f32;
                    let before_height = layer.height_tendency_m_s[cell];
                    layer.height_tendency_m_s[cell] +=
                        (-f64::from(height[cell]) / height_relax_s) as f32;
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
                            - (drag_s_inv + coastal_drag_s_inv + bathymetric_bottom_drag_s_inv)
                                * f64::from(velocity[cell][component])
                            + thermal_gradient_acceleration
                                * f64::from(thermal_gradient[cell][component])
                            + f64::from(workspace.vector_scratch[cell][component]);
                    }
                    acceleration = tangentize(acceleration, radial);
                    layer.velocity_tendency_m_s2[cell] = acceleration.map(|value| value as f32);
                }
            }
            if is_atmosphere_role(*role) {
                tendency.budget.external_atmosphere_amount_rate_m3_s += external_amount_rate_m3_s;
            } else {
                tendency.budget.external_ocean_amount_rate_m3_s += external_amount_rate_m3_s;
            }
        }

        self.apply_external_radiation(state, forcing, month, &mut tendency, cancellation)?;

        if include_explicit_transport_and_moisture
            && state.profile() == ClimateModelProfile::C2LayeredV1
        {
            apply_baroclinic_reynolds_stress_closure(
                self.grid,
                state,
                forcing,
                &mut tendency,
                &mut workspace.scalar_scratch,
                cancellation,
            )?;
        }

        if include_explicit_transport_and_moisture {
            workspace
                .scalar_scratch
                .copy_from_slice(forcing.elevation_m());
            let terrain_gradient =
                operators.gradient_cancellable(&workspace.scalar_scratch, cancellation)?;
            let lower_velocity = state
                .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere is active");
            let transported_humidity = operators
                .advect_scalar_monotone_second_order_into_cancellable(
                    state.specific_humidity(),
                    lower_velocity,
                    &workspace.open_edges,
                    transport_step_seconds,
                    true,
                    &mut workspace.transport,
                    cancellation,
                )?;
            self.apply_moisture(
                state,
                forcing,
                &terrain_gradient,
                transported_humidity.values(),
                month,
                transport_step_seconds,
                &mut tendency,
                cancellation,
            )?;
            tendency.limit_external_moisture_to_transported_availability(
                self.grid,
                state,
                transported_humidity.values(),
                transport_step_seconds,
                cancellation,
            )?;
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
        if include_explicit_transport_and_moisture {
            tendency.enforce_moisture_availability(state, transport_step_seconds, cancellation)?;
        }
        // Add exchanges after every unrelated tendency so the budget measures
        // the increments that are actually retained in the final f32 arrays.
        // The moisture pair is symmetrically flux-limited against the
        // post-transport/post-condensation water still available in each
        // layer; no independent clipping follows it.
        self.apply_pair_exchanges(
            state,
            forcing,
            cancellation,
            &mut tendency,
            include_explicit_transport_and_moisture,
            transport_step_seconds,
        )?;
        self.validate_tendency(&tendency, cancellation)?;
        Ok(tendency)
    }

    /// Applies the gray-radiation source before internal heat exchanges.
    /// Positive heating is limited by the same per-cell ASR later used to
    /// publish OLR; negative radiative cooling remains unconstrained.
    fn apply_external_radiation(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        month: usize,
        tendency: &mut LayeredClimateTendency,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let roles = state.active_roles();
        debug_assert!(roles.len() <= 4);
        for cell in 0..self.grid.cell_count() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let absorbed_shortwave =
                f64::from(forcing.monthly_absorbed_shortwave_w_m2()[cell][month]);
            let mut baselines = [0.0_f32; 4];
            let mut radiative_tendencies = [0.0_f64; 4];
            let mut heat_capacities = [0.0_f64; 4];
            let mut positive_power = 0.0_f64;
            let mut negative_power = 0.0_f64;

            for (role_index, role) in roles.iter().copied().enumerate() {
                let baseline = tendency
                    .layer(role)
                    .expect("active tendency role")
                    .temperature_tendency_k_s[cell];
                let target_temperature = radiative_target_temperature_c(forcing, role, cell, month);
                let thermal_relax_s = role_constants(state.profile(), role).3;
                let requested = (target_temperature
                    - f64::from(state.temperature_c(role).unwrap()[cell]))
                    / thermal_relax_s;
                let candidate = baseline + requested as f32;
                let retained = f64::from(candidate) - f64::from(baseline);
                let heat_capacity = heat_capacity_per_area(state, role);
                let power = heat_capacity * retained;
                baselines[role_index] = baseline;
                radiative_tendencies[role_index] = retained;
                heat_capacities[role_index] = heat_capacity;
                if power >= 0.0 {
                    positive_power += power;
                } else {
                    negative_power += power;
                }
            }

            let raw_net_power = positive_power + negative_power;
            let positive_scale = if raw_net_power > absorbed_shortwave && positive_power > 0.0 {
                // Preserve cooling and proportionally limit only heating.
                ((absorbed_shortwave - negative_power) / positive_power).clamp(0.0, 1.0)
            } else {
                1.0
            };

            let mut retained_power = 0.0_f64;
            for (role_index, role) in roles.iter().copied().enumerate() {
                let retained = radiative_tendencies[role_index];
                let adjusted = if retained > 0.0 {
                    retained * positive_scale
                } else {
                    retained
                };
                let target = &mut tendency
                    .layer_mut(role)
                    .expect("active tendency role")
                    .temperature_tendency_k_s[cell];
                *target = baselines[role_index] + adjusted as f32;
                let actual_retained = f64::from(*target) - f64::from(baselines[role_index]);
                retained_power += heat_capacities[role_index] * actual_retained;
            }
            // Scaling and composing a retained tendency each introduce at
            // most one f32 rounding. Two deterministic ULP passes therefore
            // project any boundary overshoot back into the representable
            // feasible set without clipping the published flux afterward.
            for _ in 0..2 {
                if retained_power <= absorbed_shortwave {
                    break;
                }
                for (role_index, role) in roles.iter().copied().enumerate() {
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
                retained_power = roles
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
            tendency.budget.external_heat_rate_w +=
                self.grid.cells()[cell].area_m2() * retained_power;
        }
        Ok(())
    }

    /// Evaluates the fast shallow-water/Coriolis operator plus conservative
    /// paired momentum exchange and horizontal momentum diffusion. External
    /// relaxation, moisture sources/sinks, transport, drag, heat/moisture
    /// exchange, and diagnosed eddy stress remain slow. Velocity-dependent
    /// exchange and diffusion are re-evaluated at every RK stage so the split
    /// does not freeze their stability response for a whole macro step.
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
            operators.divergence_with_permeability_into_cancellable_validated(
                velocity,
                permeability,
                &mut workspace.scalar_scratch,
                &mut workspace.transport,
                cancellation,
            )?;
            operators.gradient_with_permeability_into_cancellable_validated(
                height,
                permeability,
                &mut workspace.vector_scratch,
                &mut workspace.transport,
                cancellation,
            )?;
            let reduced_gravity = role_constants(state.profile(), *role).0;
            let reference_thickness =
                f64::from(state.reference_thickness_m(*role).expect("active role"));
            let layer = tendency.layer_mut(*role).expect("active tendency role");
            for (cell, &cell_velocity) in velocity.iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                layer.height_tendency_m_s[cell] =
                    (-reference_thickness * f64::from(workspace.scalar_scratch[cell])) as f32;
                let radial = self.grid.cells()[cell].center_unit();
                let coriolis = operators.coriolis_cell_validated(
                    cell,
                    cell_velocity,
                    EARTH_ROTATION_RATE_RAD_S,
                );
                let acceleration = std::array::from_fn(|component| {
                    -reduced_gravity * f64::from(workspace.vector_scratch[cell][component])
                        + f64::from(coriolis[component])
                });
                layer.velocity_tendency_m_s2[cell] =
                    tangentize(acceleration, radial).map(|value| value as f32);
            }
            horizontal_velocity_diffusion(
                self.grid,
                velocity,
                permeability,
                if ocean {
                    OCEAN_HORIZONTAL_EDDY_VISCOSITY_M2_S
                } else {
                    ATMOSPHERE_HORIZONTAL_EDDY_VISCOSITY_M2_S
                },
                &mut workspace.vector_scratch,
                cancellation,
            )?;
            let layer = tendency.layer_mut(*role).expect("active tendency role");
            for cell in 0..self.grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                for component in 0..3 {
                    layer.velocity_tendency_m_s2[cell][component] +=
                        workspace.vector_scratch[cell][component];
                }
            }
        }
        self.apply_pair_momentum_exchanges(state, forcing, cancellation, &mut tendency)?;
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
        if forcing.grid_fingerprint() != self.grid.fingerprint()
            || forcing.cell_count() != self.grid.cell_count()
        {
            return Err(LayeredTendencyError::GridMismatch);
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

    fn apply_moisture(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        terrain_gradient: &[[f32; 3]],
        transported_humidity: &[f32],
        month: usize,
        step_seconds: f64,
        tendency: &mut LayeredClimateTendency,
        cancellation: &BuildCancellation,
    ) -> Result<(), LayeredTendencyError> {
        let lower_velocity = state
            .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
            .expect("lower atmosphere is active");
        let atmospheric_column_mass = mass_per_area(state, ClimateLayerRole::LowerAtmosphere);
        let moisture_exchange_time_s = state.upper_specific_humidity().map(|_| {
            ClimateLayerLayout::for_profile(state.profile())
                .exchange(
                    ClimateLayerRole::LowerAtmosphere,
                    ClimateLayerRole::UpperAtmosphere,
                )
                .and_then(|exchange| exchange.moisture_exchange_time_s())
                .expect("C2 lower-upper moisture exchange is declared")
        });
        for cell in 0..self.grid.cell_count() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let humidity = f64::from(state.specific_humidity()[cell]);
            let advected_midpoint = 0.5 * (humidity + f64::from(transported_humidity[cell]));
            let equilibrium = f64::from(forcing.equilibrium_specific_humidity()[cell][month]);
            let upslope_velocity = lower_velocity[cell]
                .iter()
                .zip(terrain_gradient[cell])
                .map(|(velocity, gradient)| f64::from(*velocity) * f64::from(gradient))
                .sum::<f64>()
                .clamp(0.0, OROGRAPHIC_UPLIFT_MAX_M_S);
            let land_fraction = f64::from(forcing.land_fraction()[cell]);
            let rates = |humidity: f64| {
                let evaporation = (equilibrium - humidity).max(0.0) / (5.0 * SECONDS_PER_DAY);
                let orographic_condensation =
                    humidity * upslope_velocity * land_fraction / OROGRAPHIC_CONDENSATION_DEPTH_M;
                let condensation = (humidity - equilibrium).max(0.0) / (3.0 * SECONDS_PER_DAY)
                    + orographic_condensation;
                (evaporation, condensation, orographic_condensation)
            };
            let (initial_evaporation, initial_condensation, _) = rates(advected_midpoint);
            let vertical_exchange = state.upper_specific_humidity().map_or(0.0, |upper| {
                let upper_mass = mass_per_area(state, ClimateLayerRole::UpperAtmosphere);
                let coupling_mass = atmospheric_column_mass.min(upper_mass);
                (f64::from(upper[cell]) - advected_midpoint) * coupling_mass
                    / moisture_exchange_time_s.expect("upper humidity has exchange")
                    / atmospheric_column_mass
            });
            let physical_midpoint = (advected_midpoint
                + 0.5
                    * step_seconds
                    * (initial_evaporation - initial_condensation + vertical_exchange))
                .max(0.0);
            let (evaporation, condensation, orographic_condensation) = rates(physical_midpoint);
            let evaporation_rate_kg_m2_s = (evaporation * atmospheric_column_mass) as f32;
            let precipitation_rate_kg_m2_s = (condensation * atmospheric_column_mass) as f32;
            tendency.specific_humidity_tendency_s_inv[cell] =
                ((f64::from(evaporation_rate_kg_m2_s) - f64::from(precipitation_rate_kg_m2_s))
                    / atmospheric_column_mass) as f32;
            tendency.external_moisture_tendency_s_inv[cell] =
                f64::from(tendency.specific_humidity_tendency_s_inv[cell]);
            tendency.precipitation_rate_mm_s[cell] = precipitation_rate_kg_m2_s;
            tendency.orographic_precipitation_rate_mm_s[cell] =
                (orographic_condensation * atmospheric_column_mass) as f32;
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
                let scale = if exchange.water_only() {
                    f64::from(1.0 - forcing.land_fraction()[cell])
                } else {
                    1.0
                };
                if scale == 0.0 {
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
                    ) = add_balanced_pair_to_f32(
                        &mut first_target,
                        &mut second_target,
                        scale * momentum.first_acceleration_m_s2[component],
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
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
        include_moisture_exchange: bool,
        moisture_step_seconds: f64,
    ) -> Result<(), LayeredTendencyError> {
        let layout = ClimateLayerLayout::for_profile(state.profile());
        for exchange in layout.exchanges().iter().copied().filter(|exchange| {
            exchange.heat_exchange_time_s().is_some()
                && exchange.momentum_exchange_time_s().is_some()
        }) {
            let first_role = exchange.first();
            let second_role = exchange.second();
            let heat_timescale = exchange
                .heat_exchange_time_s()
                .expect("filtered exchange has heat timescale");
            let momentum_timescale = exchange
                .momentum_exchange_time_s()
                .expect("filtered exchange has momentum timescale");
            let first_temperature = state.temperature_c(first_role).expect("pair role");
            let second_temperature = state.temperature_c(second_role).expect("pair role");
            let first_velocity = state.velocity_m_s(first_role).expect("pair role");
            let second_velocity = state.velocity_m_s(second_role).expect("pair role");
            let first_capacity = heat_capacity_per_area(state, first_role);
            let second_capacity = heat_capacity_per_area(state, second_role);
            let first_mass = mass_per_area(state, first_role);
            let second_mass = mass_per_area(state, second_role);
            for cell in 0..self.grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let scale = if exchange.water_only() {
                    f64::from(1.0 - forcing.land_fraction()[cell])
                } else {
                    1.0
                };
                if scale == 0.0 {
                    continue;
                }
                let heat = paired_heat_exchange(
                    f64::from(first_temperature[cell]),
                    f64::from(second_temperature[cell]),
                    first_capacity,
                    second_capacity,
                    heat_timescale,
                )?;
                let first_heat_delta = {
                    let target = &mut tendency
                        .layer_mut(first_role)
                        .expect("pair role")
                        .temperature_tendency_k_s[cell];
                    let before = *target;
                    *target += (scale * heat.first_tendency_k_s) as f32;
                    f64::from(*target) - f64::from(before)
                };
                let second_heat_delta = {
                    let target = &mut tendency
                        .layer_mut(second_role)
                        .expect("pair role")
                        .temperature_tendency_k_s[cell];
                    let before = *target;
                    *target += (scale * heat.second_tendency_k_s) as f32;
                    f64::from(*target) - f64::from(before)
                };
                let area = self.grid.cells()[cell].area_m2();
                let first_heat = first_capacity * first_heat_delta;
                let second_heat = second_capacity * second_heat_delta;
                tendency.budget.paired_heat_absolute_w +=
                    area * 0.5 * (first_heat.abs() + second_heat.abs());
                tendency.budget.paired_heat_residual_w += area * (first_heat + second_heat).abs();

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
                    ) = add_balanced_pair_to_f32(
                        &mut first_target,
                        &mut second_target,
                        scale * momentum.first_acceleration_m_s2[component],
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
                tendency.budget.paired_momentum_absolute_n +=
                    area * 0.5 * (norm(first_impulse) + norm(second_impulse));
                tendency.budget.paired_momentum_residual_n += area
                    * norm(std::array::from_fn(|component| {
                        first_impulse[component] + second_impulse[component]
                    }));
            }
        }

        if include_moisture_exchange {
            if let (Some(upper_humidity), Some(_)) = (
                state.upper_specific_humidity(),
                tendency.upper_specific_humidity_tendency_s_inv.as_ref(),
            ) {
                let lower_humidity = state.specific_humidity();
                let lower_mass = mass_per_area(state, ClimateLayerRole::LowerAtmosphere);
                let upper_mass = mass_per_area(state, ClimateLayerRole::UpperAtmosphere);
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

        if state.profile() == ClimateModelProfile::C2LayeredV1 {
            let thermocline = state
                .temperature_c(ClimateLayerRole::OceanThermocline)
                .expect("C2 thermocline");
            let deep = state.deep_ocean_temperature_c().expect("C2 deep reservoir");
            let thermocline_capacity =
                heat_capacity_per_area(state, ClimateLayerRole::OceanThermocline);
            let deep_capacity = 1_025.0 * 3_990.0 * 3_000.0;
            for cell in 0..self.grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                let timescale = layout
                    .exchange(
                        ClimateLayerRole::OceanThermocline,
                        ClimateLayerRole::DeepOceanReservoir,
                    )
                    .and_then(|exchange| exchange.heat_exchange_time_s())
                    .expect("C2 thermocline-deep heat exchange is declared");
                let exchange = paired_heat_exchange(
                    f64::from(thermocline[cell]),
                    f64::from(deep[cell]),
                    thermocline_capacity,
                    deep_capacity,
                    timescale,
                )?;
                let thermocline_delta = {
                    let target = &mut tendency
                        .layer_mut(ClimateLayerRole::OceanThermocline)
                        .expect("C2 thermocline")
                        .temperature_tendency_k_s[cell];
                    let before = *target;
                    *target += exchange.first_tendency_k_s as f32;
                    f64::from(*target) - f64::from(before)
                };
                let deep_delta = {
                    let target = &mut tendency
                        .deep_ocean_temperature_tendency_k_s
                        .as_mut()
                        .expect("C2 deep tendency")[cell];
                    let before = *target;
                    *target += exchange.second_tendency_k_s as f32;
                    f64::from(*target) - f64::from(before)
                };
                let area = self.grid.cells()[cell].area_m2();
                let thermocline_heat = thermocline_capacity * thermocline_delta;
                let deep_heat = deep_capacity * deep_delta;
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
            .min(super::generation::REFERENCE_WAVE_SPEED_M_S);
    Ok(AxisymmetricCirculationDiagnostic {
        equator_to_pole_contrast_k,
        eddy_velocity_scale_m_s,
        radius_m: grid.radius_m(),
    })
}

fn apply_baroclinic_reynolds_stress_closure(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    forcing: &PlanetForcing,
    tendency: &mut LayeredClimateTendency,
    raw_zonal_acceleration_m_s2: &mut [f32],
    cancellation: &BuildCancellation,
) -> Result<(), LayeredTendencyError> {
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
        let layer_mass_per_area = mass_per_area(state, role);
        let mut axial_torque_rate_n_m = 0.0_f64;
        let mut correction_inertia_kg_m2 = 0.0_f64;
        for (cell, raw_zonal_acceleration) in raw_zonal_acceleration_m_s2.iter_mut().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let radial = grid.cells()[cell].center_unit();
            let cosine_latitude = (radial[0] * radial[0] + radial[1] * radial[1]).sqrt();
            let raw = diagnostic.reynolds_stress_zonal_acceleration_m_s2(radial);
            *raw_zonal_acceleration = raw as f32;
            let area_mass = grid.cells()[cell].area_m2() * layer_mass_per_area;
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
                    (zonal_acceleration * east_component) as f32;
            }
        }
    }
    check_cancelled(cancellation)
}

fn role_constants(
    profile: ClimateModelProfile,
    role: ClimateLayerRole,
) -> (f64, f64, f64, f64, f64) {
    match role {
        ClimateLayerRole::LowerAtmosphere => (
            0.31,
            // One-day boundary-layer Rayleigh friction. The upper layer keeps
            // its ten-day free-tropospheric drag; using five days here left a
            // spurious inertial phase in the accelerated monthly continuation.
            1.0 / SECONDS_PER_DAY,
            7.0 * SECONDS_PER_DAY,
            10.0 * SECONDS_PER_DAY,
            // Fixed lower-layer hypsometric pressure coupling. Its sign and
            // amplitude are independent of morphology acceptance bands.
            if profile == ClimateModelProfile::C2LayeredV1 {
                C2_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K
            } else {
                C1_LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K
            },
        ),
        ClimateLayerRole::UpperAtmosphere => (
            0.45,
            1.0 / (10.0 * SECONDS_PER_DAY),
            12.0 * SECONDS_PER_DAY,
            20.0 * SECONDS_PER_DAY,
            -UPPER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K,
        ),
        ClimateLayerRole::OceanMixedLayer => (
            // This prognostic height is published as sea-surface height, so its
            // fast pressure mode is a free-surface mode and uses full gravity.
            STANDARD_GRAVITY_M_S2,
            1.0 / (30.0 * SECONDS_PER_DAY),
            90.0 * SECONDS_PER_DAY,
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
            5.0 * 365.25 * SECONDS_PER_DAY,
            // Fixed reduced gravity already closes this internal-interface
            // pressure response. Reapplying the surface-temperature gradient
            // here would double count the same baroclinic forcing.
            0.0,
        ),
        ClimateLayerRole::DeepOceanReservoir => unreachable!(),
    }
}

fn horizontal_velocity_diffusion(
    grid: &CubedSphereGrid,
    velocity_m_s: &[[f32; 3]],
    edge_permeability: &[f32],
    diffusivity_m2_s: f64,
    target_acceleration_m_s2: &mut [[f32; 3]],
    cancellation: &BuildCancellation,
) -> Result<(), LayeredTendencyError> {
    debug_assert_eq!(velocity_m_s.len(), grid.cell_count());
    debug_assert_eq!(edge_permeability.len(), grid.edges().len());
    debug_assert_eq!(target_acceleration_m_s2.len(), grid.cell_count());
    debug_assert!(diffusivity_m2_s.is_finite() && diffusivity_m2_s >= 0.0);
    target_acceleration_m_s2.fill([0.0; 3]);

    for (edge_index, edge) in grid.edges().iter().enumerate() {
        if edge_index % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        let permeability = f64::from(edge_permeability[edge_index]);
        if permeability == 0.0 || diffusivity_m2_s == 0.0 {
            continue;
        }
        let [first, second] = *edge.cells();
        let first = first as usize;
        let second = second as usize;
        let midpoint = edge.midpoint_unit();
        let first_at_midpoint = parallel_transport_tangent(
            velocity_m_s[first].map(f64::from),
            grid.cells()[first].center_unit(),
            midpoint,
        );
        let second_at_midpoint = parallel_transport_tangent(
            velocity_m_s[second].map(f64::from),
            grid.cells()[second].center_unit(),
            midpoint,
        );
        let conductance_m2_s =
            diffusivity_m2_s * permeability * edge.length_m() / edge.center_distance_m();
        for component in 0..3 {
            let flux_m3_s2 =
                conductance_m2_s * (second_at_midpoint[component] - first_at_midpoint[component]);
            target_acceleration_m_s2[first][component] +=
                (flux_m3_s2 / grid.cells()[first].area_m2()) as f32;
            target_acceleration_m_s2[second][component] -=
                (flux_m3_s2 / grid.cells()[second].area_m2()) as f32;
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

fn radiative_target_temperature_c(
    forcing: &PlanetForcing,
    role: ClimateLayerRole,
    cell: usize,
    month: usize,
) -> f64 {
    let raw = match role {
        ClimateLayerRole::LowerAtmosphere => forcing.equilibrium_air_temperature_c()[cell][month],
        ClimateLayerRole::UpperAtmosphere => {
            forcing.equilibrium_air_temperature_c()[cell][month]
                - UPPER_ATMOSPHERE_EQUILIBRIUM_OFFSET_C
        }
        ClimateLayerRole::OceanMixedLayer => {
            forcing.equilibrium_surface_temperature_c()[cell][month]
        }
        ClimateLayerRole::OceanThermocline => {
            forcing.equilibrium_surface_temperature_c()[cell][month]
                - THERMOCLINE_EQUILIBRIUM_OFFSET_C
        }
        ClimateLayerRole::DeepOceanReservoir => unreachable!(),
    };
    f64::from(match role {
        ClimateLayerRole::OceanMixedLayer => {
            raw.clamp(LIQUID_MIXED_LAYER_MIN_C, OCEAN_EQUILIBRIUM_MAX_C)
        }
        ClimateLayerRole::OceanThermocline => {
            raw.clamp(SUBSURFACE_OCEAN_MIN_C, OCEAN_EQUILIBRIUM_MAX_C)
        }
        _ => raw,
    })
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

fn heat_capacity_per_area(state: &LayeredClimateState, role: ClimateLayerRole) -> f64 {
    let layout = ClimateLayerLayout::for_profile(state.profile());
    let layer = layout
        .layers()
        .iter()
        .find(|layer| layer.role() == role)
        .expect("active role belongs to the profile layout");
    layer.density_kg_m3()
        * f64::from(state.reference_thickness_m(role).expect("active role"))
        * layer.heat_capacity_j_kg_k()
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
        add_balanced_pair_to_f32, apply_baroclinic_reynolds_stress_closure,
        diagnose_axisymmetric_circulation, dot, horizontal_velocity_diffusion, next_f32_down,
        role_constants, tangentize, LayeredClimateTendency, LayeredTendencySystem,
    };
    use crate::engine::BuildCancellation;
    use crate::generators::natural::circulation::CubedSphereGrid;
    use crate::generators::natural::global_circulation::LayeredClimateState;
    use crate::world::natural::{
        ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, PlanetForcing,
        CLIMATE_MONTH_COUNT,
    };

    #[test]
    fn mixed_layer_uses_free_surface_gravity_and_depth_mean_steric_acceleration() {
        let (gravity, _, _, _, steric_acceleration) = role_constants(
            ClimateModelProfile::C2LayeredV1,
            ClimateLayerRole::OceanMixedLayer,
        );
        let expected_steric_acceleration = 0.5 * 9.806_65 * 2.0e-4 * 100.0;

        assert!((gravity - 9.806_65).abs() <= f64::EPSILON);
        assert!((steric_acceleration - expected_steric_acceleration).abs() <= 1.0e-12);
    }

    #[test]
    fn atmospheric_baroclinic_pressure_mode_is_column_mass_neutral() {
        let lower = role_constants(
            ClimateModelProfile::C2LayeredV1,
            ClimateLayerRole::LowerAtmosphere,
        )
        .4;
        let upper = role_constants(
            ClimateModelProfile::C2LayeredV1,
            ClimateLayerRole::UpperAtmosphere,
        )
        .4;
        let column_weighted_coupling = 6_000.0 * lower + 4_000.0 * upper;
        assert!(column_weighted_coupling.abs() <= 1.0e-10);
        assert_eq!(
            role_constants(
                ClimateModelProfile::C1SingleLayerV1,
                ClimateLayerRole::LowerAtmosphere,
            )
            .4,
            30.0
        );
        assert_eq!(
            role_constants(
                ClimateModelProfile::C2LayeredV1,
                ClimateLayerRole::LowerAtmosphere,
            )
            .1,
            1.0 / 86_400.0
        );
    }

    #[test]
    fn horizontal_velocity_diffusion_dissipates_a_spike_without_crossing_closed_edges() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let mut velocity = vec![[0.0_f32; 3]; grid.cell_count()];
        velocity[0] = tangentize([1.0, 2.0, 3.0], grid.cells()[0].center_unit())
            .map(|component| component as f32);
        let mut tendency = vec![[0.0_f32; 3]; grid.cell_count()];
        horizontal_velocity_diffusion(
            &grid,
            &velocity,
            &vec![1.0; grid.edges().len()],
            100_000.0,
            &mut tendency,
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
                        .zip(tendency[cell])
                        .map(|(velocity, acceleration)| {
                            f64::from(*velocity) * f64::from(acceleration)
                        })
                        .sum::<f64>()
            })
            .sum::<f64>();
        assert!(kinetic_energy_tendency < 0.0);
        assert!(tendency.iter().skip(1).flatten().any(|value| *value != 0.0));

        horizontal_velocity_diffusion(
            &grid,
            &velocity,
            &vec![0.0; grid.edges().len()],
            100_000.0,
            &mut tendency,
            &BuildCancellation::new(),
        )
        .unwrap();
        assert!(tendency.iter().flatten().all(|value| *value == 0.0));
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
        let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let mut tendency = LayeredClimateTendency::zeroed(&state);
        apply_baroclinic_reynolds_stress_closure(
            &grid,
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
            let layer_mass_per_area =
                layer.density_kg_m3() * f64::from(state.reference_thickness_m(role).unwrap());
            let mut signed_torque = 0.0_f64;
            let mut absolute_torque = 0.0_f64;
            let mut tropical = (0.0_f64, 0_u32);
            let mut extratropical = (0.0_f64, 0_u32);
            let transition = (1.0_f64 / 3.0_f64.sqrt()).asin();
            for (cell, value) in grid.cells().iter().zip(acceleration) {
                assert!(value.iter().all(|component| component.is_finite()));
                let radial = cell.center_unit();
                let cosine = radial[0].hypot(radial[1]);
                if cosine <= f64::EPSILON.sqrt() {
                    assert!(value.iter().all(|component| *component == 0.0));
                    continue;
                }
                let east = [-radial[1] / cosine, radial[0] / cosine, 0.0];
                let zonal = dot(value.map(f64::from), east);
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
    fn representable_pair_projection_never_keeps_only_one_side_of_a_tiny_exchange() {
        let first_before = 1.0e-4_f32;
        let second_before = 1.0e-4_f32;
        let mut first = first_before;
        let mut second = second_before;
        let (first_delta, second_delta) =
            add_balanced_pair_to_f32(&mut first, &mut second, 4.0e-12, 7_350.0, 76_875.0);
        let first_flux = 7_350.0 * first_delta;
        let second_flux = 76_875.0 * second_delta;
        let scale = 0.5 * (first_flux.abs() + second_flux.abs());
        let relative = if scale > 0.0 {
            (first_flux + second_flux).abs() / scale
        } else {
            0.0
        };
        assert!(relative <= 1.0e-6, "unbalanced retained pair: {relative}");
        assert_eq!(first_delta, f64::from(first) - f64::from(first_before));
        assert_eq!(second_delta, f64::from(second) - f64::from(second_before));
        assert_eq!((first, second), (first_before, second_before));
    }

    #[test]
    fn representable_pair_projection_retains_a_resolved_exchange() {
        let mut first = 1.0e-4_f32;
        let mut second = -2.0e-5_f32;
        let desired_first_delta = 1.0e-4;
        let (first_delta, second_delta) = add_balanced_pair_to_f32(
            &mut first,
            &mut second,
            desired_first_delta,
            7_350.0,
            76_875.0,
        );
        let first_flux = 7_350.0 * first_delta;
        let second_flux = 76_875.0 * second_delta;
        let relative =
            (first_flux + second_flux).abs() / (0.5 * (first_flux.abs() + second_flux.abs()));
        assert!(relative <= 1.0e-6, "unbalanced retained pair: {relative}");
        assert!((first_delta - desired_first_delta).abs() / desired_first_delta <= 1.0e-3);
    }
}
