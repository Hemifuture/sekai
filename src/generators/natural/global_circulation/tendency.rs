use thiserror::Error;

use super::state::{
    LayeredClimateState, LayeredStateError, LIQUID_MIXED_LAYER_MIN_C, SUBSURFACE_OCEAN_MIN_C,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{
    CirculationOperatorError, CirculationOperators, CubedSphereGrid, SecondOrderTransportWorkspace,
};
use crate::world::natural::{
    ClimateLayerRole, ClimateModelProfile, PlanetForcing, CLIMATE_MONTH_COUNT,
};

const EARTH_ROTATION_RATE_RAD_S: f64 = 7.292_115_9e-5;
const SECONDS_PER_DAY: f64 = 86_400.0;
const OROGRAPHIC_CONDENSATION_DEPTH_M: f64 = 800.0;
const OROGRAPHIC_UPLIFT_MAX_M_S: f64 = 0.02;
// Effective hypsometric pressure couplings for the fixed 6 km / 4 km layers.
// They keep the equilibrium geopotential contrast inside the layer-depth
// validity range while retaining a resolved baroclinic response.
const LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K: f64 = 30.0;
const UPPER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K: f64 = 25.0;

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
    precipitation_rate_mm_s: Vec<f32>,
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
            precipitation_rate_mm_s: vec![0.0; count],
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

    pub fn precipitation_rate_mm_s(&self) -> &[f32] {
        &self.precipitation_rate_mm_s
    }

    pub fn deep_ocean_temperature_tendency_k_s(&self) -> Option<&[f32]> {
        self.deep_ocean_temperature_tendency_k_s.as_deref()
    }

    pub const fn budget(&self) -> LayeredTendencyBudget {
        self.budget
    }
}

/// One-evaluation physical-source and paired-exchange accounting.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LayeredTendencyBudget {
    paired_heat_absolute_w_m2: f64,
    paired_heat_residual_w_m2: f64,
    paired_momentum_absolute_n_m2: f64,
    paired_momentum_residual_n_m2: f64,
    physical_moisture_source_kg_m2_s: f64,
    physical_precipitation_sink_kg_m2_s: f64,
    radiative_heat_tendency_k_s: f64,
}

impl LayeredTendencyBudget {
    pub const fn paired_heat_absolute_w_m2(self) -> f64 {
        self.paired_heat_absolute_w_m2
    }

    pub const fn paired_heat_residual_w_m2(self) -> f64 {
        self.paired_heat_residual_w_m2
    }

    pub const fn paired_momentum_absolute_n_m2(self) -> f64 {
        self.paired_momentum_absolute_n_m2
    }

    pub const fn paired_momentum_residual_n_m2(self) -> f64 {
        self.paired_momentum_residual_n_m2
    }

    pub const fn physical_moisture_source_kg_m2_s(self) -> f64 {
        self.physical_moisture_source_kg_m2_s
    }

    pub const fn physical_precipitation_sink_kg_m2_s(self) -> f64 {
        self.physical_precipitation_sink_kg_m2_s
    }

    pub const fn radiative_heat_tendency_k_s(self) -> f64 {
        self.radiative_heat_tendency_k_s
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

    pub fn allocation_signature(&self) -> [usize; 14] {
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
    ) -> Result<LayeredClimateTendency, LayeredTendencyError> {
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
            let divergence = operators.divergence_with_permeability(velocity, permeability)?;
            let height_gradient = operators.gradient_with_permeability(height, permeability)?;
            let coriolis = operators.coriolis(velocity, EARTH_ROTATION_RATE_RAD_S)?;
            let target = match role {
                ClimateLayerRole::LowerAtmosphere => forcing.equilibrium_air_temperature_c(),
                ClimateLayerRole::UpperAtmosphere => forcing.equilibrium_air_temperature_c(),
                ClimateLayerRole::OceanMixedLayer | ClimateLayerRole::OceanThermocline => {
                    forcing.equilibrium_surface_temperature_c()
                }
                ClimateLayerRole::DeepOceanReservoir => unreachable!(),
            };
            for (cell, scratch) in workspace.scalar_scratch.iter_mut().enumerate() {
                let raw = target[cell][month]
                    - if *role == ClimateLayerRole::UpperAtmosphere {
                        12.0
                    } else if *role == ClimateLayerRole::OceanThermocline {
                        8.0
                    } else {
                        0.0
                    };
                *scratch = match role {
                    ClimateLayerRole::OceanMixedLayer => raw.clamp(LIQUID_MIXED_LAYER_MIN_C, 40.0),
                    ClimateLayerRole::OceanThermocline => raw.clamp(SUBSURFACE_OCEAN_MIN_C, 40.0),
                    _ => raw,
                };
            }
            let thermal_gradient =
                operators.gradient_with_permeability(&workspace.scalar_scratch, permeability)?;
            let (reduced_gravity, drag_s_inv, height_relax_s, thermal_relax_s, thermal_pressure) =
                role_constants(*role);
            let reference_thickness =
                f64::from(state.reference_thickness_m(*role).expect("active role"));
            let mut radiative_absolute = 0.0_f64;
            {
                let layer = tendency.layer_mut(*role).expect("active tendency role");
                for cell in 0..self.grid.cell_count() {
                    layer.height_tendency_m_s[cell] = (-reference_thickness
                        * f64::from(divergence[cell])
                        - f64::from(height[cell]) / height_relax_s)
                        as f32;
                    let radial = self.grid.cells()[cell].center_unit();
                    let mut acceleration = [0.0_f64; 3];
                    for component in 0..3 {
                        acceleration[component] = -reduced_gravity
                            * f64::from(height_gradient[cell][component])
                            + f64::from(coriolis[cell][component])
                            - drag_s_inv * f64::from(velocity[cell][component])
                            - thermal_pressure * f64::from(thermal_gradient[cell][component]);
                    }
                    acceleration = tangentize(acceleration, radial);
                    layer.velocity_tendency_m_s2[cell] = acceleration.map(|value| value as f32);
                    let target_temperature = f64::from(workspace.scalar_scratch[cell]);
                    let radiative =
                        (target_temperature - f64::from(temperature[cell])) / thermal_relax_s;
                    layer.temperature_tendency_k_s[cell] = radiative as f32;
                    radiative_absolute += radiative.abs();
                }
            }
            tendency.budget.radiative_heat_tendency_k_s += radiative_absolute;

            if include_explicit_transport_and_moisture {
                let transported = operators.advect_scalar_monotone_second_order_into(
                    temperature,
                    velocity,
                    permeability,
                    1.0,
                    false,
                    &mut workspace.transport,
                )?;
                for (target, (transported, original)) in tendency
                    .layer_mut(*role)
                    .expect("active tendency role")
                    .temperature_tendency_k_s
                    .iter_mut()
                    .zip(transported.values().iter().zip(temperature))
                {
                    *target += transported - original;
                }
            }
        }

        if include_explicit_transport_and_moisture {
            workspace
                .scalar_scratch
                .copy_from_slice(forcing.elevation_m());
            let terrain_gradient = operators.gradient(&workspace.scalar_scratch)?;
            self.apply_moisture(state, forcing, &terrain_gradient, month, &mut tendency);
            let lower_velocity = state
                .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere is active");
            let transported_humidity = operators.advect_scalar_monotone_second_order_into(
                state.specific_humidity(),
                lower_velocity,
                &workspace.open_edges,
                1.0,
                true,
                &mut workspace.transport,
            )?;
            for (target, (transported, original)) in
                tendency.specific_humidity_tendency_s_inv.iter_mut().zip(
                    transported_humidity
                        .values()
                        .iter()
                        .zip(state.specific_humidity()),
                )
            {
                *target += transported - original;
            }
        }
        self.apply_pair_exchanges(state, forcing, cancellation, &mut tendency)?;
        self.validate_tendency(&tendency)?;
        Ok(tendency)
    }

    /// Evaluates only the fast linear shallow-water and Coriolis operator.
    /// Slow relaxation, thermodynamics, moisture, drag, and paired exchange
    /// are deliberately absent so split-explicit integration can apply them
    /// exactly once per macro step.
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
            let divergence = operators.divergence_with_permeability(velocity, permeability)?;
            let height_gradient = operators.gradient_with_permeability(height, permeability)?;
            let coriolis = operators.coriolis(velocity, EARTH_ROTATION_RATE_RAD_S)?;
            let reduced_gravity = role_constants(*role).0;
            let reference_thickness =
                f64::from(state.reference_thickness_m(*role).expect("active role"));
            let layer = tendency.layer_mut(*role).expect("active tendency role");
            for cell in 0..self.grid.cell_count() {
                layer.height_tendency_m_s[cell] =
                    (-reference_thickness * f64::from(divergence[cell])) as f32;
                let radial = self.grid.cells()[cell].center_unit();
                let acceleration = std::array::from_fn(|component| {
                    -reduced_gravity * f64::from(height_gradient[cell][component])
                        + f64::from(coriolis[cell][component])
                });
                layer.velocity_tendency_m_s2[cell] =
                    tangentize(acceleration, radial).map(|value| value as f32);
            }
        }
        self.validate_tendency(&tendency)?;
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
        state.validate_against(self.grid)?;
        forcing
            .validate()
            .map_err(|error| LayeredTendencyError::InvalidForcing {
                reason: error.to_string(),
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
        month: usize,
        tendency: &mut LayeredClimateTendency,
    ) {
        let lower_velocity = state
            .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
            .expect("lower atmosphere is active");
        let atmospheric_column_mass = mass_per_area(state, ClimateLayerRole::LowerAtmosphere);
        for cell in 0..self.grid.cell_count() {
            let humidity = f64::from(state.specific_humidity()[cell]);
            let equilibrium = f64::from(forcing.equilibrium_specific_humidity()[cell][month]);
            let evaporation = (equilibrium - humidity).max(0.0) / (5.0 * SECONDS_PER_DAY);
            let upslope_velocity = lower_velocity[cell]
                .iter()
                .zip(terrain_gradient[cell])
                .map(|(velocity, gradient)| f64::from(*velocity) * f64::from(gradient))
                .sum::<f64>()
                .clamp(0.0, OROGRAPHIC_UPLIFT_MAX_M_S);
            let orographic_condensation =
                humidity * upslope_velocity * f64::from(forcing.land_fraction()[cell])
                    / OROGRAPHIC_CONDENSATION_DEPTH_M;
            let condensation = (humidity - equilibrium).max(0.0) / (3.0 * SECONDS_PER_DAY)
                + orographic_condensation;
            tendency.specific_humidity_tendency_s_inv[cell] = (evaporation - condensation) as f32;
            tendency.precipitation_rate_mm_s[cell] =
                (condensation * atmospheric_column_mass) as f32;
            tendency.budget.physical_moisture_source_kg_m2_s +=
                evaporation * atmospheric_column_mass;
            tendency.budget.physical_precipitation_sink_kg_m2_s +=
                condensation * atmospheric_column_mass;
        }
    }

    fn apply_pair_exchanges(
        &self,
        state: &LayeredClimateState,
        forcing: &PlanetForcing,
        cancellation: &BuildCancellation,
        tendency: &mut LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        let mut pairs = vec![(
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::OceanMixedLayer,
            7.0 * SECONDS_PER_DAY,
            true,
        )];
        if state.profile() == ClimateModelProfile::C2LayeredV1 {
            pairs.extend([
                (
                    ClimateLayerRole::LowerAtmosphere,
                    ClimateLayerRole::UpperAtmosphere,
                    5.0 * SECONDS_PER_DAY,
                    false,
                ),
                (
                    ClimateLayerRole::OceanMixedLayer,
                    ClimateLayerRole::OceanThermocline,
                    90.0 * SECONDS_PER_DAY,
                    false,
                ),
            ]);
        }
        for (first_role, second_role, timescale, water_only) in pairs {
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
                let scale = if water_only {
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
                    timescale,
                )?;
                tendency
                    .layer_mut(first_role)
                    .expect("pair role")
                    .temperature_tendency_k_s[cell] += (scale * heat.first_tendency_k_s) as f32;
                tendency
                    .layer_mut(second_role)
                    .expect("pair role")
                    .temperature_tendency_k_s[cell] += (scale * heat.second_tendency_k_s) as f32;
                tendency.budget.paired_heat_absolute_w_m2 += scale * heat.extensive_flux_w_m2.abs();
                tendency.budget.paired_heat_residual_w_m2 += scale * heat.extensive_residual_w_m2;

                let momentum = paired_momentum_exchange(
                    first_velocity[cell].map(f64::from),
                    second_velocity[cell].map(f64::from),
                    first_mass,
                    second_mass,
                    timescale,
                )?;
                for component in 0..3 {
                    tendency
                        .layer_mut(first_role)
                        .expect("pair role")
                        .velocity_tendency_m_s2[cell][component] +=
                        (scale * momentum.first_acceleration_m_s2[component]) as f32;
                    tendency
                        .layer_mut(second_role)
                        .expect("pair role")
                        .velocity_tendency_m_s2[cell][component] +=
                        (scale * momentum.second_acceleration_m_s2[component]) as f32;
                }
                tendency.budget.paired_momentum_absolute_n_m2 += scale
                    * norm(std::array::from_fn(|component| {
                        first_mass * momentum.first_acceleration_m_s2[component]
                    }));
                tendency.budget.paired_momentum_residual_n_m2 +=
                    scale * momentum.extensive_residual_n_m2;
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
                let exchange = paired_heat_exchange(
                    f64::from(thermocline[cell]),
                    f64::from(deep[cell]),
                    thermocline_capacity,
                    deep_capacity,
                    200.0 * 365.25 * SECONDS_PER_DAY,
                )?;
                tendency
                    .layer_mut(ClimateLayerRole::OceanThermocline)
                    .expect("C2 thermocline")
                    .temperature_tendency_k_s[cell] += exchange.first_tendency_k_s as f32;
                tendency
                    .deep_ocean_temperature_tendency_k_s
                    .as_mut()
                    .expect("C2 deep tendency")[cell] += exchange.second_tendency_k_s as f32;
                tendency.budget.paired_heat_absolute_w_m2 += exchange.extensive_flux_w_m2.abs();
                tendency.budget.paired_heat_residual_w_m2 += exchange.extensive_residual_w_m2;
            }
        }
        Ok(())
    }

    fn validate_tendency(
        &self,
        tendency: &LayeredClimateTendency,
    ) -> Result<(), LayeredTendencyError> {
        for layer in &tendency.active_layers {
            if layer
                .height_tendency_m_s
                .iter()
                .chain(layer.temperature_tendency_k_s.iter())
                .any(|value| !value.is_finite())
                || layer
                    .velocity_tendency_m_s2
                    .iter()
                    .flatten()
                    .any(|value| !value.is_finite())
            {
                return Err(LayeredTendencyError::NonFiniteTendency { role: layer.role });
            }
        }
        Ok(())
    }
}

fn role_constants(role: ClimateLayerRole) -> (f64, f64, f64, f64, f64) {
    match role {
        ClimateLayerRole::LowerAtmosphere => (
            0.31,
            1.0 / (5.0 * SECONDS_PER_DAY),
            20.0 * SECONDS_PER_DAY,
            12.0 * SECONDS_PER_DAY,
            LOWER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K,
        ),
        ClimateLayerRole::UpperAtmosphere => (
            0.45,
            1.0 / (10.0 * SECONDS_PER_DAY),
            30.0 * SECONDS_PER_DAY,
            20.0 * SECONDS_PER_DAY,
            UPPER_ATMOSPHERE_THERMAL_PRESSURE_M2_S2_K,
        ),
        ClimateLayerRole::OceanMixedLayer => (
            0.02,
            1.0 / (30.0 * SECONDS_PER_DAY),
            90.0 * SECONDS_PER_DAY,
            90.0 * SECONDS_PER_DAY,
            0.8,
        ),
        ClimateLayerRole::OceanThermocline => (
            0.012,
            1.0 / (180.0 * SECONDS_PER_DAY),
            365.25 * SECONDS_PER_DAY,
            5.0 * 365.25 * SECONDS_PER_DAY,
            0.3,
        ),
        ClimateLayerRole::DeepOceanReservoir => unreachable!(),
    }
}

fn density(role: ClimateLayerRole) -> f64 {
    match role {
        ClimateLayerRole::LowerAtmosphere | ClimateLayerRole::UpperAtmosphere => 1.225,
        ClimateLayerRole::OceanMixedLayer
        | ClimateLayerRole::OceanThermocline
        | ClimateLayerRole::DeepOceanReservoir => 1_025.0,
    }
}

fn heat_capacity(role: ClimateLayerRole) -> f64 {
    match role {
        ClimateLayerRole::LowerAtmosphere | ClimateLayerRole::UpperAtmosphere => 1_004.0,
        ClimateLayerRole::OceanMixedLayer
        | ClimateLayerRole::OceanThermocline
        | ClimateLayerRole::DeepOceanReservoir => 3_990.0,
    }
}

fn mass_per_area(state: &LayeredClimateState, role: ClimateLayerRole) -> f64 {
    density(role) * f64::from(state.reference_thickness_m(role).expect("active role"))
}

fn heat_capacity_per_area(state: &LayeredClimateState, role: ClimateLayerRole) -> f64 {
    mass_per_area(state, role) * heat_capacity(role)
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
    Operator(#[from] CirculationOperatorError),
    #[error("invalid planet forcing: {reason}")]
    InvalidForcing { reason: String },
    #[error("layered state or forcing grid does not match the tendency system")]
    GridMismatch,
    #[error("month {found} is outside the 12-month climatology")]
    InvalidMonth { found: usize },
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
}
