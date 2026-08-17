use thiserror::Error;

use super::{
    project_monthly_extensive_rate, project_monthly_intensive_scalar,
    project_monthly_tangent_vectors, ClimateIntegratorError, ClimateProjectionError,
    GlobalClimateForcing, LayeredClimateState, LayeredStateError, LayeredTendencyError,
    LayeredTendencySystem, SplitExplicitRk3Integrator, SELECTED_PRODUCTION_INTEGRATOR,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{CubedSphereGrid, CubedSphereGridError};
use crate::world::natural::{
    ClimateBudgetReport, ClimateCapabilitySet, ClimateCheckpoint, ClimateCheckpointError,
    ClimateLayerLayout, ClimateLayerRole, ClimateModelProfile, ClimateQuantizationId,
    ClimateRemapReport, ClimateReportError, ClimateSolveReport, ClimateValidationError,
    ClimateWorkDomainSnapshot, ClimateWorkDomainValidationError, GlobalCirculationFields,
    GlobalCirculationSnapshot, GlobalCirculationValidationError, MonthlyScalarField,
    MonthlyVector3Field, PlanetForcing, CLIMATE_MONTH_COUNT, GLOBAL_CIRCULATION_SCHEMA_V1,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

const MACRO_STEP_SECONDS: f64 = 7_200.0;
const MAXIMUM_FAST_STEP_SECONDS: f64 = 1_200.0;
const FAST_CFL_TARGET: f64 = 0.35;
const REFERENCE_WAVE_SPEED_M_S: f64 = 65.0;
const EARTH_ROTATION_RATE_RAD_S: f64 = 7.292_115_9e-5;
const FORMATION_RESIDUAL_TARGET: f64 = 0.25;

#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalCirculationGenerator;

impl GlobalCirculationGenerator {
    pub fn generate(
        surface: &SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &GlobalClimateForcing,
        profile: ClimateModelProfile,
        cancellation: &BuildCancellation,
    ) -> Result<GlobalCirculationSnapshot, GlobalCirculationGenerationError> {
        check_cancelled(cancellation)?;
        domain.validate_against(surface)?;
        forcing.validate_against(domain)?;
        let grid = CubedSphereGrid::new(domain.face_resolution(), surface.radius().get())?;
        if grid.fingerprint() != domain.climate_grid_fingerprint()
            || grid.to_surface_snapshot()? != *domain.climate_surface()
        {
            return Err(GlobalCirculationGenerationError::GridReconstructionMismatch);
        }
        let layout = ClimateLayerLayout::for_profile(profile);
        layout
            .validate()
            .map_err(|error| GlobalCirculationGenerationError::InvalidLayout {
                reason: error.to_string(),
            })?;
        let maximum_formation_cycles = maximum_formation_cycles(domain);
        let fast_step_seconds = stable_fast_step_seconds(&grid);
        let integrator = SplitExplicitRk3Integrator::new(&grid, fast_step_seconds)?;
        let planet = forcing.planet_forcing();
        let mut state = LayeredClimateState::from_forcing(&grid, &layout, planet, 0)?;
        let mut previous_annual = state.clone();
        let mut initial_residual = 0.0_f64;
        let mut final_residual = 0.0_f64;
        let mut macro_steps = 0_u64;
        let mut fast_substeps = 0_u64;
        let mut maximum_cfl = 0.0_f64;
        let mut budgets = BudgetAccumulator::new(&grid, &layout, &state);
        let mut work = WorkClimatology::new(grid.cell_count(), profile);
        let tendency_system = LayeredTendencySystem::new(&grid);

        let mut formation_years = 0_u16;
        for year in 0..maximum_formation_cycles {
            for month in 0..CLIMATE_MONTH_COUNT {
                check_cancelled(cancellation)?;
                let before = state.clone();
                let declared = tendency_system.evaluate(
                    &before,
                    planet,
                    forcing.ocean_edge_permeability(),
                    month,
                    cancellation,
                )?;
                let result = integrator.advance(
                    &before,
                    planet,
                    forcing.ocean_edge_permeability(),
                    month,
                    MACRO_STEP_SECONDS,
                    cancellation,
                )?;
                let diagnostics = result.diagnostics();
                state = result.into_state();
                enforce_ocean_land_mask(&mut state, planet);
                state.validate_against(&grid)?;
                budgets.record(
                    &grid,
                    &layout,
                    &before,
                    &state,
                    &declared,
                    MACRO_STEP_SECONDS,
                );
                work.record_month(&state, &declared, month);
                macro_steps += 1;
                fast_substeps += u64::from(diagnostics.fast_substeps());
                maximum_cfl = maximum_cfl.max(diagnostics.maximum_cfl());
            }
            let residual = relative_state_residual(&grid, &previous_annual, &state)?;
            if year == 0 {
                initial_residual = residual;
            }
            final_residual = residual;
            formation_years = year + 1;
            previous_annual = state.clone();
            if final_residual <= FORMATION_RESIDUAL_TARGET {
                break;
            }
        }
        if final_residual > initial_residual + 1.0e-12 {
            return Err(
                GlobalCirculationGenerationError::FormationResidualIncreased {
                    initial: initial_residual,
                    final_value: final_residual,
                },
            );
        }
        if final_residual > FORMATION_RESIDUAL_TARGET {
            return Err(GlobalCirculationGenerationError::FormationNotConverged {
                cycles: formation_years,
                residual: final_residual,
                target: FORMATION_RESIDUAL_TARGET,
            });
        }
        check_cancelled(cancellation)?;

        let fields = work.project(surface, domain, planet)?;
        let dense_state_bytes = dense_state_bytes(&grid, profile, surface.cells().len())?;
        let solve_report = ClimateSolveReport::new(
            formation_years,
            macro_steps,
            fast_substeps,
            0,
            initial_residual,
            final_residual,
            maximum_cfl,
            dense_state_bytes,
        )?;
        let budget_report = budgets.finish()?;
        let remap_report = remap_report(domain)?;
        let state_fingerprint = fields_fingerprint(&fields, profile);
        let input_fingerprint = input_fingerprint(surface, domain, forcing, &layout);
        let checkpoint = ClimateCheckpoint::new(
            profile,
            SELECTED_PRODUCTION_INTEGRATOR,
            *grid.fingerprint(),
            *forcing.fingerprint(),
            layout.fingerprint(),
            input_fingerprint,
            ClimateQuantizationId::DeterministicF64V1,
            u32::from(formation_years) * CLIMATE_MONTH_COUNT as u32,
            state_fingerprint,
        )?;
        let snapshot = GlobalCirculationSnapshot::new(
            GLOBAL_CIRCULATION_SCHEMA_V1,
            SurfaceRef::for_spherical(surface),
            layout,
            SELECTED_PRODUCTION_INTEGRATOR,
            ClimateCapabilitySet::for_profile(profile),
            checkpoint,
            solve_report,
            budget_report,
            remap_report,
            fields,
        )?;
        snapshot.validate_against(surface)?;
        Ok(snapshot)
    }
}

fn maximum_formation_cycles(domain: &ClimateWorkDomainSnapshot) -> u16 {
    match domain.profile() {
        crate::world::natural::NaturalQualityProfile::Draft => 8,
        crate::world::natural::NaturalQualityProfile::Standard => 10,
        crate::world::natural::NaturalQualityProfile::High => 12,
    }
}

fn stable_fast_step_seconds(grid: &CubedSphereGrid) -> f64 {
    let wave_limit = FAST_CFL_TARGET * grid.minimum_center_distance_m() / REFERENCE_WAVE_SPEED_M_S;
    let rotation_limit = FAST_CFL_TARGET / (2.0 * EARTH_ROTATION_RATE_RAD_S);
    MAXIMUM_FAST_STEP_SECONDS
        .min(wave_limit)
        .min(rotation_limit)
}

fn enforce_ocean_land_mask(state: &mut LayeredClimateState, forcing: &PlanetForcing) {
    for role in [
        ClimateLayerRole::OceanMixedLayer,
        ClimateLayerRole::OceanThermocline,
    ] {
        if let Some(velocity) = state.velocity_m_s_mut(role) {
            for (vector, land_fraction) in velocity.iter_mut().zip(forcing.land_fraction()) {
                let ocean_fraction = (1.0 - *land_fraction).clamp(0.0, 1.0);
                for component in vector {
                    *component *= ocean_fraction;
                }
            }
        }
    }
}

fn relative_state_residual(
    grid: &CubedSphereGrid,
    previous: &LayeredClimateState,
    current: &LayeredClimateState,
) -> Result<f64, GlobalCirculationGenerationError> {
    let difference = super::climate_state_rms_difference(grid, previous, current)?;
    let origin = LayeredClimateState::from_forcing(
        grid,
        &ClimateLayerLayout::for_profile(current.profile()),
        &uniform_reference_forcing(grid, current.profile())?,
        0,
    )?;
    let magnitude = super::climate_state_rms_difference(grid, current, &origin)?.max(1.0);
    Ok(difference / magnitude)
}

fn uniform_reference_forcing(
    grid: &CubedSphereGrid,
    _profile: ClimateModelProfile,
) -> Result<PlanetForcing, GlobalCirculationGenerationError> {
    let count = grid.cell_count();
    PlanetForcing::new(
        *grid.fingerprint(),
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![1.0; count],
        vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        vec![[0.0; CLIMATE_MONTH_COUNT]; count],
    )
    .map_err(|error| GlobalCirculationGenerationError::InvalidForcing {
        reason: error.to_string(),
    })
}

struct WorkClimatology {
    lower_wind: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    upper_wind: Option<Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>>,
    ocean_current: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    air_temperature: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    sea_temperature: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    thermocline_temperature: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    thermocline_depth: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    precipitation: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    lower_height: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    upper_height: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    sea_height: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    thermocline_height: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    deep_temperature: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
}

impl WorkClimatology {
    fn new(count: usize, profile: ClimateModelProfile) -> Self {
        let c2 = profile == ClimateModelProfile::C2LayeredV1;
        Self {
            lower_wind: vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count],
            upper_wind: c2.then(|| vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count]),
            ocean_current: vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count],
            air_temperature: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            sea_temperature: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            thermocline_temperature: c2.then(|| vec![[0.0; CLIMATE_MONTH_COUNT]; count]),
            thermocline_depth: c2.then(|| vec![[0.0; CLIMATE_MONTH_COUNT]; count]),
            humidity: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            precipitation: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            lower_height: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            upper_height: c2.then(|| vec![[0.0; CLIMATE_MONTH_COUNT]; count]),
            sea_height: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            thermocline_height: c2.then(|| vec![[0.0; CLIMATE_MONTH_COUNT]; count]),
            deep_temperature: c2.then(|| vec![[0.0; CLIMATE_MONTH_COUNT]; count]),
        }
    }

    fn record_month(
        &mut self,
        state: &LayeredClimateState,
        tendency: &super::LayeredClimateTendency,
        month: usize,
    ) {
        copy_vector_month(
            &mut self.lower_wind,
            state
                .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere"),
            month,
        );
        copy_vector_month(
            &mut self.ocean_current,
            state
                .velocity_m_s(ClimateLayerRole::OceanMixedLayer)
                .expect("mixed layer"),
            month,
        );
        copy_scalar_month(
            &mut self.air_temperature,
            state
                .temperature_c(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere"),
            month,
        );
        copy_scalar_month(
            &mut self.sea_temperature,
            state
                .temperature_c(ClimateLayerRole::OceanMixedLayer)
                .expect("mixed layer"),
            month,
        );
        copy_scalar_month(&mut self.humidity, state.specific_humidity(), month);
        for (target, rate) in self
            .precipitation
            .iter_mut()
            .zip(tendency.precipitation_rate_mm_s())
        {
            target[month] = *rate * 86_400.0;
        }
        copy_scalar_month(
            &mut self.lower_height,
            state
                .height_anomaly_m(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere"),
            month,
        );
        copy_scalar_month(
            &mut self.sea_height,
            state
                .height_anomaly_m(ClimateLayerRole::OceanMixedLayer)
                .expect("mixed layer"),
            month,
        );
        if let Some(target) = &mut self.upper_wind {
            copy_vector_month(
                target,
                state
                    .velocity_m_s(ClimateLayerRole::UpperAtmosphere)
                    .expect("C2 upper atmosphere"),
                month,
            );
        }
        if let Some(target) = &mut self.thermocline_temperature {
            copy_scalar_month(
                target,
                state
                    .temperature_c(ClimateLayerRole::OceanThermocline)
                    .expect("C2 thermocline"),
                month,
            );
        }
        if let Some(target) = &mut self.thermocline_depth {
            let reference = state
                .reference_thickness_m(ClimateLayerRole::OceanThermocline)
                .expect("C2 thermocline");
            for (target, anomaly) in target.iter_mut().zip(
                state
                    .height_anomaly_m(ClimateLayerRole::OceanThermocline)
                    .expect("C2 thermocline"),
            ) {
                target[month] = reference + *anomaly;
            }
        }
        if let Some(target) = &mut self.upper_height {
            copy_scalar_month(
                target,
                state
                    .height_anomaly_m(ClimateLayerRole::UpperAtmosphere)
                    .expect("C2 upper atmosphere"),
                month,
            );
        }
        if let Some(target) = &mut self.thermocline_height {
            copy_scalar_month(
                target,
                state
                    .height_anomaly_m(ClimateLayerRole::OceanThermocline)
                    .expect("C2 thermocline"),
                month,
            );
        }
        if let (Some(target), Some(deep)) =
            (&mut self.deep_temperature, state.deep_ocean_temperature_c())
        {
            copy_scalar_month(target, deep, month);
        }
    }

    fn project(
        self,
        surface: &SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &PlanetForcing,
    ) -> Result<GlobalCirculationFields, GlobalCirculationGenerationError> {
        let lower_wind = project_monthly_tangent_vectors(domain, surface, &self.lower_wind)?;
        let mut ocean_current =
            project_monthly_tangent_vectors(domain, surface, &self.ocean_current)?;
        let projected_land = project_monthly_intensive_scalar(
            domain,
            &forcing
                .land_fraction()
                .iter()
                .map(|value| [*value; CLIMATE_MONTH_COUNT])
                .collect::<Vec<_>>(),
        )?;
        for (vectors, land) in ocean_current.iter_mut().zip(projected_land.values()) {
            for month in 0..CLIMATE_MONTH_COUNT {
                let ocean_fraction = (1.0 - land[month]).clamp(0.0, 1.0);
                for component in &mut vectors[month] {
                    *component *= ocean_fraction;
                }
            }
        }
        let air = project_monthly_intensive_scalar(domain, &self.air_temperature)?;
        let sea = project_monthly_intensive_scalar(domain, &self.sea_temperature)?;
        let humidity = project_monthly_intensive_scalar(domain, &self.humidity)?;
        let precipitation = project_monthly_extensive_rate(domain, &self.precipitation)?;
        let lower_height = project_monthly_intensive_scalar(domain, &self.lower_height)?;
        let sea_height = project_monthly_intensive_scalar(domain, &self.sea_height)?;
        if let (
            Some(upper_wind),
            Some(thermocline_temperature),
            Some(thermocline_depth),
            Some(upper_height),
            Some(thermocline_height),
            Some(deep_temperature),
        ) = (
            self.upper_wind,
            self.thermocline_temperature,
            self.thermocline_depth,
            self.upper_height,
            self.thermocline_height,
            self.deep_temperature,
        ) {
            let upper_wind = project_monthly_tangent_vectors(domain, surface, &upper_wind)?;
            let shear = upper_wind
                .iter()
                .zip(&lower_wind)
                .map(|(upper, lower)| {
                    std::array::from_fn(|month| {
                        std::array::from_fn(|component| {
                            upper[month][component] - lower[month][component]
                        })
                    })
                })
                .collect::<Vec<_>>();
            Ok(GlobalCirculationFields::new_c2(
                MonthlyVector3Field::from_values(lower_wind)?,
                MonthlyVector3Field::from_values(upper_wind)?,
                MonthlyVector3Field::from_values(shear)?,
                MonthlyVector3Field::from_values(ocean_current)?,
                MonthlyScalarField::from_values(air.values().to_vec())?,
                MonthlyScalarField::from_values(sea.values().to_vec())?,
                MonthlyScalarField::from_values(
                    project_monthly_intensive_scalar(domain, &thermocline_temperature)?
                        .values()
                        .to_vec(),
                )?,
                MonthlyScalarField::from_values(
                    project_monthly_intensive_scalar(domain, &thermocline_depth)?
                        .values()
                        .to_vec(),
                )?,
                MonthlyScalarField::from_values(humidity.values().to_vec())?,
                MonthlyScalarField::from_values(precipitation.values().to_vec())?,
                MonthlyScalarField::from_values(lower_height.values().to_vec())?,
                MonthlyScalarField::from_values(
                    project_monthly_intensive_scalar(domain, &upper_height)?
                        .values()
                        .to_vec(),
                )?,
                MonthlyScalarField::from_values(sea_height.values().to_vec())?,
                MonthlyScalarField::from_values(
                    project_monthly_intensive_scalar(domain, &thermocline_height)?
                        .values()
                        .to_vec(),
                )?,
                MonthlyScalarField::from_values(
                    project_monthly_intensive_scalar(domain, &deep_temperature)?
                        .values()
                        .to_vec(),
                )?,
            )?)
        } else {
            Ok(GlobalCirculationFields::new_c1(
                MonthlyVector3Field::from_values(lower_wind)?,
                MonthlyVector3Field::from_values(ocean_current)?,
                MonthlyScalarField::from_values(air.values().to_vec())?,
                MonthlyScalarField::from_values(sea.values().to_vec())?,
                MonthlyScalarField::from_values(humidity.values().to_vec())?,
                MonthlyScalarField::from_values(precipitation.values().to_vec())?,
                MonthlyScalarField::from_values(lower_height.values().to_vec())?,
                MonthlyScalarField::from_values(sea_height.values().to_vec())?,
            )?)
        }
    }
}

fn copy_scalar_month(target: &mut [[f32; CLIMATE_MONTH_COUNT]], source: &[f32], month: usize) {
    for (target, source) in target.iter_mut().zip(source) {
        target[month] = *source;
    }
}

fn copy_vector_month(
    target: &mut [[[f32; 3]; CLIMATE_MONTH_COUNT]],
    source: &[[f32; 3]],
    month: usize,
) {
    for (target, source) in target.iter_mut().zip(source) {
        target[month] = *source;
    }
}

struct BudgetAccumulator {
    atmosphere_residual: f64,
    ocean_residual: f64,
    moisture_residual: f64,
    energy_residual: f64,
    paired_residual: f64,
    atmosphere_scale: f64,
    ocean_scale: f64,
    moisture_scale: f64,
    energy_scale: f64,
    paired_scale: f64,
}

impl BudgetAccumulator {
    fn new(
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        state: &LayeredClimateState,
    ) -> Self {
        Self {
            atmosphere_residual: 0.0,
            ocean_residual: 0.0,
            moisture_residual: 0.0,
            energy_residual: 0.0,
            paired_residual: 0.0,
            atmosphere_scale: layer_amount_total(grid, state, true).abs().max(1.0),
            ocean_scale: layer_amount_total(grid, state, false).abs().max(1.0),
            moisture_scale: moisture_total(grid, state).abs().max(1.0),
            energy_scale: energy_total(grid, layout, state).abs().max(1.0),
            paired_scale: 1.0,
        }
    }

    fn record(
        &mut self,
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        before: &LayeredClimateState,
        after: &LayeredClimateState,
        tendency: &super::LayeredClimateTendency,
        dt: f64,
    ) {
        let atmosphere_expected = height_tendency_total(grid, tendency, true) * dt;
        let ocean_expected = height_tendency_total(grid, tendency, false) * dt;
        self.atmosphere_residual += ((layer_amount_total(grid, after, true)
            - layer_amount_total(grid, before, true))
            - atmosphere_expected)
            .abs();
        self.ocean_residual += ((layer_amount_total(grid, after, false)
            - layer_amount_total(grid, before, false))
            - ocean_expected)
            .abs();
        let lower_mass = layer_mass_per_area(layout, ClimateLayerRole::LowerAtmosphere);
        let moisture_expected = grid
            .cells()
            .iter()
            .zip(tendency.specific_humidity_tendency_s_inv())
            .map(|(cell, value)| cell.area_m2() * lower_mass * f64::from(*value) * dt)
            .sum::<f64>();
        self.moisture_residual += ((moisture_total(grid, after) - moisture_total(grid, before))
            - moisture_expected)
            .abs();
        let energy_expected = energy_tendency_total(grid, layout, tendency) * dt;
        self.energy_residual += ((energy_total(grid, layout, after)
            - energy_total(grid, layout, before))
            - energy_expected)
            .abs();
        let tendency_budget = tendency.budget();
        self.paired_residual += tendency_budget.paired_heat_residual_w_m2().abs()
            + tendency_budget.paired_momentum_residual_n_m2().abs();
        self.paired_scale += tendency_budget.paired_heat_absolute_w_m2()
            + tendency_budget.paired_momentum_absolute_n_m2();
    }

    fn finish(self) -> Result<ClimateBudgetReport, ClimateReportError> {
        ClimateBudgetReport::new(
            self.atmosphere_residual / self.atmosphere_scale,
            self.ocean_residual / self.ocean_scale,
            self.moisture_residual / self.moisture_scale,
            self.energy_residual / self.energy_scale,
            self.paired_residual / self.paired_scale,
        )
    }
}

fn is_atmosphere(role: ClimateLayerRole) -> bool {
    matches!(
        role,
        ClimateLayerRole::LowerAtmosphere | ClimateLayerRole::UpperAtmosphere
    )
}

fn layer_amount_total(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    atmosphere: bool,
) -> f64 {
    state
        .active_roles()
        .iter()
        .filter(|role| is_atmosphere(**role) == atmosphere)
        .map(|role| {
            let reference = f64::from(state.reference_thickness_m(*role).expect("active role"));
            grid.cells()
                .iter()
                .zip(state.height_anomaly_m(*role).expect("active role"))
                .map(|(cell, anomaly)| cell.area_m2() * (reference + f64::from(*anomaly)))
                .sum::<f64>()
        })
        .sum()
}

fn height_tendency_total(
    grid: &CubedSphereGrid,
    tendency: &super::LayeredClimateTendency,
    atmosphere: bool,
) -> f64 {
    [
        ClimateLayerRole::LowerAtmosphere,
        ClimateLayerRole::UpperAtmosphere,
        ClimateLayerRole::OceanMixedLayer,
        ClimateLayerRole::OceanThermocline,
    ]
    .into_iter()
    .filter(|role| is_atmosphere(*role) == atmosphere)
    .filter_map(|role| tendency.height_tendency_m_s(role))
    .map(|values| {
        grid.cells()
            .iter()
            .zip(values)
            .map(|(cell, value)| cell.area_m2() * f64::from(*value))
            .sum::<f64>()
    })
    .sum()
}

fn layer_spec(
    layout: &ClimateLayerLayout,
    role: ClimateLayerRole,
) -> Option<&crate::world::natural::ClimateLayerSpec> {
    layout.layers().iter().find(|layer| layer.role() == role)
}

fn layer_mass_per_area(layout: &ClimateLayerLayout, role: ClimateLayerRole) -> f64 {
    let layer = layer_spec(layout, role).expect("fixed layout role");
    layer.density_kg_m3() * layer.reference_thickness_m()
}

fn layer_heat_capacity_per_area(layout: &ClimateLayerLayout, role: ClimateLayerRole) -> f64 {
    let layer = layer_spec(layout, role).expect("fixed layout role");
    layer_mass_per_area(layout, role) * layer.heat_capacity_j_kg_k()
}

fn moisture_total(grid: &CubedSphereGrid, state: &LayeredClimateState) -> f64 {
    let mass = match state.profile() {
        ClimateModelProfile::C1SingleLayerV1 => 1.225 * 8_000.0,
        ClimateModelProfile::C2LayeredV1 => 1.225 * 6_000.0,
    };
    grid.cells()
        .iter()
        .zip(state.specific_humidity())
        .map(|(cell, value)| cell.area_m2() * mass * f64::from(*value))
        .sum()
}

fn energy_total(
    grid: &CubedSphereGrid,
    layout: &ClimateLayerLayout,
    state: &LayeredClimateState,
) -> f64 {
    let mut total = 0.0_f64;
    for role in state.active_roles() {
        let capacity = layer_heat_capacity_per_area(layout, *role);
        total += grid
            .cells()
            .iter()
            .zip(state.temperature_c(*role).expect("active role"))
            .map(|(cell, value)| cell.area_m2() * capacity * f64::from(*value))
            .sum::<f64>();
    }
    if let Some(deep) = state.deep_ocean_temperature_c() {
        let capacity = layer_heat_capacity_per_area(layout, ClimateLayerRole::DeepOceanReservoir);
        total += grid
            .cells()
            .iter()
            .zip(deep)
            .map(|(cell, value)| cell.area_m2() * capacity * f64::from(*value))
            .sum::<f64>();
    }
    total
}

fn energy_tendency_total(
    grid: &CubedSphereGrid,
    layout: &ClimateLayerLayout,
    tendency: &super::LayeredClimateTendency,
) -> f64 {
    let mut total = 0.0_f64;
    for role in [
        ClimateLayerRole::LowerAtmosphere,
        ClimateLayerRole::UpperAtmosphere,
        ClimateLayerRole::OceanMixedLayer,
        ClimateLayerRole::OceanThermocline,
    ] {
        if let Some(values) = tendency.temperature_tendency_k_s(role) {
            let capacity = layer_heat_capacity_per_area(layout, role);
            total += grid
                .cells()
                .iter()
                .zip(values)
                .map(|(cell, value)| cell.area_m2() * capacity * f64::from(*value))
                .sum::<f64>();
        }
    }
    if let Some(values) = tendency.deep_ocean_temperature_tendency_k_s() {
        let capacity = layer_heat_capacity_per_area(layout, ClimateLayerRole::DeepOceanReservoir);
        total += grid
            .cells()
            .iter()
            .zip(values)
            .map(|(cell, value)| cell.area_m2() * capacity * f64::from(*value))
            .sum::<f64>();
    }
    total
}

fn remap_report(
    domain: &ClimateWorkDomainSnapshot,
) -> Result<ClimateRemapReport, GlobalCirculationGenerationError> {
    let forward = domain.source_to_climate();
    let reverse = domain.climate_to_source();
    Ok(ClimateRemapReport::new(
        forward.solve_stats().max_source_margin_relative_error(),
        forward.solve_stats().max_target_margin_relative_error(),
        reverse.solve_stats().max_source_margin_relative_error(),
        reverse.solve_stats().max_target_margin_relative_error(),
        u32::try_from(forward.overlap_count())
            .map_err(|_| GlobalCirculationGenerationError::AllocationOverflow)?,
        u32::try_from(reverse.overlap_count())
            .map_err(|_| GlobalCirculationGenerationError::AllocationOverflow)?,
    )?)
}

fn input_fingerprint(
    surface: &SphericalSurfaceSnapshot,
    domain: &ClimateWorkDomainSnapshot,
    forcing: &GlobalClimateForcing,
    layout: &ClimateLayerLayout,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.global-circulation-input.v1\0");
    hasher.update(&SurfaceRef::for_spherical(surface).fingerprint());
    hasher.update(domain.climate_grid_fingerprint());
    hasher.update(forcing.fingerprint());
    hasher.update(&layout.fingerprint());
    *hasher.finalize().as_bytes()
}

fn fields_fingerprint(fields: &GlobalCirculationFields, profile: ClimateModelProfile) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.global-circulation-state.v1\0");
    hash_vectors(&mut hasher, fields.near_surface_wind_m_s().values());
    hash_vectors(&mut hasher, fields.surface_ocean_current_m_s().values());
    for scalar in [
        fields.monthly_air_temperature_c(),
        fields.monthly_sea_surface_temperature_c(),
        fields.monthly_specific_humidity(),
        fields.monthly_precipitation_mm_day(),
        fields.monthly_lower_atmosphere_height_anomaly_m(),
        fields.monthly_sea_surface_height_anomaly_m(),
    ] {
        hash_scalars(&mut hasher, scalar.values());
    }
    if profile == ClimateModelProfile::C2LayeredV1 {
        hash_vectors(&mut hasher, fields.upper_wind_m_s().expect("C2").values());
        hash_vectors(
            &mut hasher,
            fields.vertical_wind_shear_m_s().expect("C2").values(),
        );
        for scalar in [
            fields.monthly_thermocline_temperature_c().expect("C2"),
            fields.monthly_thermocline_depth_m().expect("C2"),
            fields
                .monthly_upper_atmosphere_height_anomaly_m()
                .expect("C2"),
            fields.monthly_thermocline_height_anomaly_m().expect("C2"),
            fields.monthly_deep_ocean_temperature_c().expect("C2"),
        ] {
            hash_scalars(&mut hasher, scalar.values());
        }
    }
    *hasher.finalize().as_bytes()
}

fn hash_scalars(hasher: &mut blake3::Hasher, values: &[[f32; CLIMATE_MONTH_COUNT]]) {
    for months in values {
        for value in months {
            hasher.update(&value.to_bits().to_le_bytes());
        }
    }
}

fn hash_vectors(hasher: &mut blake3::Hasher, values: &[[[f32; 3]; CLIMATE_MONTH_COUNT]]) {
    for months in values {
        for vector in months {
            for value in vector {
                hasher.update(&value.to_bits().to_le_bytes());
            }
        }
    }
}

fn dense_state_bytes(
    grid: &CubedSphereGrid,
    profile: ClimateModelProfile,
    output_cells: usize,
) -> Result<u64, GlobalCirculationGenerationError> {
    let active = match profile {
        ClimateModelProfile::C1SingleLayerV1 => 2_usize,
        ClimateModelProfile::C2LayeredV1 => 4_usize,
    };
    let work_scalars = active
        .checked_mul(5)
        .and_then(|value| value.checked_add(2))
        .and_then(|value| value.checked_mul(grid.cell_count()))
        .and_then(|value| value.checked_mul(size_of::<f32>()))
        .ok_or(GlobalCirculationGenerationError::AllocationOverflow)?;
    let output_fields = if profile == ClimateModelProfile::C2LayeredV1 {
        21_usize
    } else {
        11_usize
    };
    let output = output_fields
        .checked_mul(CLIMATE_MONTH_COUNT)
        .and_then(|value| value.checked_mul(output_cells))
        .and_then(|value| value.checked_mul(size_of::<f32>()))
        .ok_or(GlobalCirculationGenerationError::AllocationOverflow)?;
    u64::try_from(work_scalars + output)
        .map_err(|_| GlobalCirculationGenerationError::AllocationOverflow)
}

fn check_cancelled(
    cancellation: &BuildCancellation,
) -> Result<(), GlobalCirculationGenerationError> {
    if cancellation.is_cancelled() {
        Err(GlobalCirculationGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum GlobalCirculationGenerationError {
    #[error("global circulation generation was cancelled")]
    Cancelled,
    #[error("climate work grid reconstruction changed identity")]
    GridReconstructionMismatch,
    #[error("invalid climate layer layout: {reason}")]
    InvalidLayout { reason: String },
    #[error("invalid climate forcing: {reason}")]
    InvalidForcing { reason: String },
    #[error("annual formation residual increased from {initial} to {final_value}")]
    FormationResidualIncreased { initial: f64, final_value: f64 },
    #[error(
        "global circulation did not converge after {cycles} formation cycles: residual {residual} exceeds {target}"
    )]
    FormationNotConverged {
        cycles: u16,
        residual: f64,
        target: f64,
    },
    #[error("global circulation dense allocation size overflowed")]
    AllocationOverflow,
    #[error(transparent)]
    WorkDomain(#[from] ClimateWorkDomainValidationError),
    #[error(transparent)]
    Grid(#[from] CubedSphereGridError),
    #[error(transparent)]
    Forcing(#[from] super::GlobalClimateForcingError),
    #[error(transparent)]
    State(#[from] LayeredStateError),
    #[error(transparent)]
    Tendency(#[from] LayeredTendencyError),
    #[error(transparent)]
    Integrator(#[from] ClimateIntegratorError),
    #[error(transparent)]
    Projection(#[from] ClimateProjectionError),
    #[error(transparent)]
    ClimateField(#[from] ClimateValidationError),
    #[error(transparent)]
    Validation(#[from] GlobalCirculationValidationError),
    #[error(transparent)]
    Report(#[from] ClimateReportError),
    #[error(transparent)]
    Checkpoint(#[from] ClimateCheckpointError),
}
