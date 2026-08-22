use thiserror::Error;

use super::project::{
    project_monthly_extensive_rate_cancellable, project_monthly_intensive_scalar_cancellable,
    project_monthly_tangent_vectors_cancellable,
};
use super::{
    ClimateIntegratorError, ClimateProjectionError, GlobalClimateForcing, LayeredClimateState,
    LayeredStateError, LayeredTendencyError, LayeredTendencySystem, SplitExplicitRk3Integrator,
    SELECTED_PRODUCTION_INTEGRATOR,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{CubedSphereGrid, CubedSphereGridError};
#[cfg(test)]
use crate::world::natural::PlanetForcing;
use crate::world::natural::{
    expected_global_circulation_dense_state_bytes, ClimateBudgetReport, ClimateCapabilitySet,
    ClimateCheckpoint, ClimateCheckpointError, ClimateLayerLayout, ClimateLayerRole,
    ClimateModelProfile, ClimateQuantizationId, ClimateRemapReport, ClimateReportError,
    ClimateSolveReport, ClimateValidationError, ClimateWorkDomainSnapshot,
    ClimateWorkDomainValidationError, GlobalCirculationFields, GlobalCirculationSnapshot,
    GlobalCirculationValidationError, MonthlyScalarField, MonthlyVector3Field, CLIMATE_MONTH_COUNT,
    GLOBAL_CIRCULATION_MACRO_STEP_SECONDS, GLOBAL_CIRCULATION_SCHEMA_V2,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};

pub(super) const MAXIMUM_FAST_STEP_SECONDS: f64 = 1_200.0;
pub(super) const FAST_CFL_TARGET: f64 = 0.20;
pub(super) const REFERENCE_WAVE_SPEED_M_S: f64 = 65.0;
pub(super) const EARTH_ROTATION_RATE_RAD_S: f64 = 7.292_115_9e-5;
pub(super) const FORMATION_RESIDUAL_TARGET: f64 = 0.24;

// The P5 fixed point measures discharge that this solve's precipitation
// drives, so P5's discharge tolerance must sit above the climate
// convergence target — a tighter downstream tolerance is unreachable
// noise-chasing (P5 spec amendment A2).
const _: () = assert!(
    crate::world::natural::FORMATION_LOG_DISCHARGE_RESIDUAL_SCALE > FORMATION_RESIDUAL_TARGET
);

#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalCirculationGenerator;

/// Stable coarse phases for progress reporting and deterministic cancellation
/// tests. Observers run synchronously on the solver thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalCirculationPhase {
    SolverEntered,
    TransportStarted,
    TransportCompleted,
    FastSubstepsStarted,
    FastSubstepCompleted,
    ProjectionStarted,
    ProjectionFieldCompleted,
    ProjectionHalfway,
    PublicationStarted,
    FinalValidationStarted,
    StateFingerprintCompleted,
}

impl GlobalCirculationGenerator {
    pub fn generate(
        surface: &SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &GlobalClimateForcing,
        profile: ClimateModelProfile,
        cancellation: &BuildCancellation,
    ) -> Result<GlobalCirculationSnapshot, GlobalCirculationGenerationError> {
        domain.validate_against_cancellable(surface, &|| cancellation.is_cancelled())?;
        Self::generate_from_validated(surface, domain, forcing, profile, cancellation)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn generate_with_phase_observer<F>(
        surface: &SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &GlobalClimateForcing,
        profile: ClimateModelProfile,
        cancellation: &BuildCancellation,
        observer: F,
    ) -> Result<GlobalCirculationSnapshot, GlobalCirculationGenerationError>
    where
        F: FnMut(GlobalCirculationPhase),
    {
        domain.validate_against_cancellable(surface, &|| cancellation.is_cancelled())?;
        Self::generate_from_validated_with_phase_observer(
            surface,
            domain,
            forcing,
            profile,
            cancellation,
            observer,
        )
    }

    pub(crate) fn generate_from_validated(
        surface: &SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &GlobalClimateForcing,
        profile: ClimateModelProfile,
        cancellation: &BuildCancellation,
    ) -> Result<GlobalCirculationSnapshot, GlobalCirculationGenerationError> {
        Self::generate_from_validated_with_phase_observer(
            surface,
            domain,
            forcing,
            profile,
            cancellation,
            |_| {},
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_from_validated_with_phase_observer<F>(
        surface: &SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &GlobalClimateForcing,
        profile: ClimateModelProfile,
        cancellation: &BuildCancellation,
        mut observer: F,
    ) -> Result<GlobalCirculationSnapshot, GlobalCirculationGenerationError>
    where
        F: FnMut(GlobalCirculationPhase),
    {
        check_cancelled(cancellation)?;
        let surface_ref = SurfaceRef::from_validated_spherical(surface).map_err(|error| {
            GlobalCirculationGenerationError::InvalidSurfaceIdentity {
                reason: error.to_string(),
            }
        })?;
        domain.validate_binding_against(surface)?;
        check_cancelled(cancellation)?;
        forcing.validate_payload_against_cancellable(domain, cancellation)?;
        check_cancelled(cancellation)?;
        let grid = CubedSphereGrid::new_cancellable(
            domain.face_resolution(),
            surface.radius().get(),
            &|| cancellation.is_cancelled(),
        )
        .map_err(map_grid_error)?;
        check_cancelled(cancellation)?;
        if grid.fingerprint() != domain.climate_grid_fingerprint()
            || grid
                .to_surface_snapshot_cancellable(&|| cancellation.is_cancelled())
                .map_err(map_grid_error)?
                != *domain.climate_surface()
        {
            return Err(GlobalCirculationGenerationError::GridReconstructionMismatch);
        }
        check_cancelled(cancellation)?;
        let layout = ClimateLayerLayout::for_profile(profile);
        layout
            .validate()
            .map_err(|error| GlobalCirculationGenerationError::InvalidLayout {
                reason: error.to_string(),
            })?;
        let maximum_formation_cycles = domain.profile().global_circulation_formation_cycles_max();
        let fast_step_seconds = stable_fast_step_seconds(&grid);
        let integrator = SplitExplicitRk3Integrator::new(&grid, fast_step_seconds)?;
        let planet = forcing.planet_forcing();
        let mut state =
            LayeredClimateState::from_forcing_cancellable(&grid, &layout, planet, 0, cancellation)
                .map_err(map_state_error)?;
        check_cancelled(cancellation)?;
        let mut previous_cycle = state
            .clone_cancellable(cancellation)
            .map_err(map_state_error)?;
        let mut initial_residual = 0.0_f64;
        let mut final_residual = 0.0_f64;
        let mut continuation_steps = 0_u64;
        let mut fast_substeps = 0_u64;
        let mut maximum_cfl = 0.0_f64;
        let mut budgets = BudgetAccumulator::new(&grid, &layout, &state, cancellation)?;
        let mut work = WorkClimatology::new(grid.cell_count(), profile);
        let tendency_system = LayeredTendencySystem::new(&grid);
        observer(GlobalCirculationPhase::SolverEntered);
        check_cancelled(cancellation)?;

        let mut formation_cycles = 0_u16;
        for cycle in 0..maximum_formation_cycles {
            for month in 0..CLIMATE_MONTH_COUNT {
                check_cancelled(cancellation)?;
                let before = state
                    .clone_cancellable(cancellation)
                    .map_err(map_state_error)?;
                observer(GlobalCirculationPhase::TransportStarted);
                check_cancelled(cancellation)?;
                let declared = tendency_system.evaluate_for_step(
                    &before,
                    planet,
                    forcing.ocean_edge_permeability(),
                    month,
                    GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
                    cancellation,
                )?;
                observer(GlobalCirculationPhase::TransportCompleted);
                let result = integrator.advance_with_phase_observer(
                    &before,
                    planet,
                    forcing.ocean_edge_permeability(),
                    month,
                    GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
                    cancellation,
                    &mut observer,
                )?;
                let diagnostics = result.diagnostics();
                state = result.into_state();
                state
                    .enforce_full_land_ocean_velocity(planet, cancellation)
                    .map_err(map_state_error)?;
                state
                    .validate_against_cancellable(&grid, cancellation)
                    .map_err(map_state_error)?;
                budgets.record(
                    &grid,
                    &layout,
                    &before,
                    &state,
                    &declared,
                    GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
                    cancellation,
                )?;
                work.record_month(&state, &declared, month, cancellation)?;
                continuation_steps += 1;
                fast_substeps += u64::from(diagnostics.fast_substeps());
                maximum_cfl = maximum_cfl.max(diagnostics.maximum_cfl());
            }
            let residual = relative_state_residual(&grid, &previous_cycle, &state, cancellation)?;
            if cycle == 0 {
                initial_residual = residual;
            }
            final_residual = residual;
            formation_cycles = cycle + 1;
            previous_cycle = state
                .clone_cancellable(cancellation)
                .map_err(map_state_error)?;
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
                cycles: formation_cycles,
                residual: final_residual,
                target: FORMATION_RESIDUAL_TARGET,
            });
        }
        check_cancelled(cancellation)?;

        observer(GlobalCirculationPhase::ProjectionStarted);
        check_cancelled(cancellation)?;
        let (fields, published_precipitation_relative_error) =
            work.project(surface, domain, forcing, cancellation, &mut observer)?;
        let dense_state_bytes = expected_global_circulation_dense_state_bytes(
            domain.profile(),
            profile,
            surface.cells().len() as u32,
        )
        .ok_or(GlobalCirculationGenerationError::AllocationOverflow)?;
        let solve_report = ClimateSolveReport::new(
            formation_cycles,
            continuation_steps,
            fast_substeps,
            0,
            initial_residual,
            final_residual,
            maximum_cfl,
            dense_state_bytes,
        )?;
        let budget_report = budgets.finish()?;
        let remap_report = remap_report(domain, published_precipitation_relative_error)?;
        observer(GlobalCirculationPhase::FinalValidationStarted);
        let cancelled = || cancellation.is_cancelled();
        let state_fingerprint = fields.fingerprint_cancellable(&cancelled)?;
        observer(GlobalCirculationPhase::StateFingerprintCompleted);
        let input_fingerprint =
            input_fingerprint(surface_ref, domain, forcing, &layout, cancellation)?;
        let checkpoint = ClimateCheckpoint::new(
            domain.profile(),
            profile,
            SELECTED_PRODUCTION_INTEGRATOR,
            *grid.fingerprint(),
            *forcing.fingerprint(),
            super::global_circulation_model_fingerprint(profile),
            input_fingerprint,
            ClimateQuantizationId::DeterministicF64V1,
            u32::from(formation_cycles) * CLIMATE_MONTH_COUNT as u32,
            state_fingerprint,
        )?;
        let snapshot = GlobalCirculationSnapshot::new_cancellable(
            GLOBAL_CIRCULATION_SCHEMA_V2,
            surface_ref,
            layout,
            SELECTED_PRODUCTION_INTEGRATOR,
            ClimateCapabilitySet::for_profile(profile),
            checkpoint,
            solve_report,
            budget_report,
            remap_report,
            fields,
            &cancelled,
        )?;
        snapshot.validate_against_cancellable(surface, &cancelled)?;
        Ok(snapshot)
    }
}

fn stable_fast_step_seconds(grid: &CubedSphereGrid) -> f64 {
    let wave_limit = FAST_CFL_TARGET * grid.minimum_center_distance_m() / REFERENCE_WAVE_SPEED_M_S;
    let rotation_limit = FAST_CFL_TARGET / (2.0 * EARTH_ROTATION_RATE_RAD_S);
    MAXIMUM_FAST_STEP_SECONDS
        .min(wave_limit)
        .min(rotation_limit)
}

fn relative_state_residual(
    grid: &CubedSphereGrid,
    previous: &LayeredClimateState,
    current: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    super::climate_state_formation_residual_cancellable(grid, previous, current, cancellation)
        .map_err(Into::into)
}

struct WorkClimatology {
    lower_wind: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    upper_wind: Option<Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>>,
    ocean_current: Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>,
    air_temperature: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    sea_temperature: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    thermocline_temperature: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    precipitation: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    orographic_precipitation: Vec<[f32; CLIMATE_MONTH_COUNT]>,
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
            humidity: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            precipitation: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            orographic_precipitation: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
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
        cancellation: &BuildCancellation,
    ) -> Result<(), GlobalCirculationGenerationError> {
        copy_vector_month(
            &mut self.lower_wind,
            state
                .velocity_m_s(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere"),
            month,
            cancellation,
        )?;
        copy_vector_month(
            &mut self.ocean_current,
            state
                .velocity_m_s(ClimateLayerRole::OceanMixedLayer)
                .expect("mixed layer"),
            month,
            cancellation,
        )?;
        copy_scalar_month(
            &mut self.air_temperature,
            state
                .temperature_c(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere"),
            month,
            cancellation,
        )?;
        copy_scalar_month(
            &mut self.sea_temperature,
            state
                .temperature_c(ClimateLayerRole::OceanMixedLayer)
                .expect("mixed layer"),
            month,
            cancellation,
        )?;
        copy_scalar_month(
            &mut self.humidity,
            state.specific_humidity(),
            month,
            cancellation,
        )?;
        for (cell, (target, rate)) in self
            .precipitation
            .iter_mut()
            .zip(tendency.precipitation_rate_mm_s())
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            target[month] = *rate * 86_400.0;
        }
        for (cell, (target, rate)) in self
            .orographic_precipitation
            .iter_mut()
            .zip(tendency.orographic_precipitation_rate_mm_s())
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            target[month] = *rate * 86_400.0;
        }
        copy_scalar_month(
            &mut self.lower_height,
            state
                .height_anomaly_m(ClimateLayerRole::LowerAtmosphere)
                .expect("lower atmosphere"),
            month,
            cancellation,
        )?;
        copy_scalar_month(
            &mut self.sea_height,
            state
                .height_anomaly_m(ClimateLayerRole::OceanMixedLayer)
                .expect("mixed layer"),
            month,
            cancellation,
        )?;
        if let Some(target) = &mut self.upper_wind {
            copy_vector_month(
                target,
                state
                    .velocity_m_s(ClimateLayerRole::UpperAtmosphere)
                    .expect("C2 upper atmosphere"),
                month,
                cancellation,
            )?;
        }
        if let Some(target) = &mut self.thermocline_temperature {
            copy_scalar_month(
                target,
                state
                    .temperature_c(ClimateLayerRole::OceanThermocline)
                    .expect("C2 thermocline"),
                month,
                cancellation,
            )?;
        }
        if let Some(target) = &mut self.upper_height {
            copy_scalar_month(
                target,
                state
                    .height_anomaly_m(ClimateLayerRole::UpperAtmosphere)
                    .expect("C2 upper atmosphere"),
                month,
                cancellation,
            )?;
        }
        if let Some(target) = &mut self.thermocline_height {
            copy_scalar_month(
                target,
                state
                    .height_anomaly_m(ClimateLayerRole::OceanThermocline)
                    .expect("C2 thermocline"),
                month,
                cancellation,
            )?;
        }
        if let (Some(target), Some(deep)) =
            (&mut self.deep_temperature, state.deep_ocean_temperature_c())
        {
            copy_scalar_month(target, deep, month, cancellation)?;
        }
        check_cancelled(cancellation)?;
        Ok(())
    }

    fn project(
        self,
        surface: &SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &GlobalClimateForcing,
        cancellation: &BuildCancellation,
        observer: &mut impl FnMut(GlobalCirculationPhase),
    ) -> Result<(GlobalCirculationFields, f64), GlobalCirculationGenerationError> {
        let lower_wind = project_monthly_tangent_vectors_cancellable(
            domain,
            surface,
            &self.lower_wind,
            cancellation,
        )?;
        observer(GlobalCirculationPhase::ProjectionFieldCompleted);
        let mut ocean_current = project_monthly_tangent_vectors_cancellable(
            domain,
            surface,
            &self.ocean_current,
            cancellation,
        )?;
        observer(GlobalCirculationPhase::PublicationStarted);
        for (cell, (vectors, &is_land)) in ocean_current
            .iter_mut()
            .zip(forcing.source_land_mask())
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            if is_land != 0 {
                *vectors = [[0.0; 3]; CLIMATE_MONTH_COUNT];
            }
        }
        let air = project_monthly_intensive_scalar_cancellable(
            domain,
            &self.air_temperature,
            cancellation,
        )?;
        let sea = project_monthly_intensive_scalar_cancellable(
            domain,
            &self.sea_temperature,
            cancellation,
        )?;
        let humidity =
            project_monthly_intensive_scalar_cancellable(domain, &self.humidity, cancellation)?;
        let precipitation =
            project_monthly_extensive_rate_cancellable(domain, &self.precipitation, cancellation)?;
        let orographic_precipitation = project_monthly_extensive_rate_cancellable(
            domain,
            &self.orographic_precipitation,
            cancellation,
        )?;
        let precipitation_relative_error = precipitation
            .max_relative_conservation_error()
            .max(orographic_precipitation.max_relative_conservation_error());
        let lower_height =
            project_monthly_intensive_scalar_cancellable(domain, &self.lower_height, cancellation)?;
        let sea_height =
            project_monthly_intensive_scalar_cancellable(domain, &self.sea_height, cancellation)?;
        observer(GlobalCirculationPhase::ProjectionHalfway);
        check_cancelled(cancellation)?;
        if let (
            Some(upper_wind),
            Some(thermocline_temperature),
            Some(upper_height),
            Some(thermocline_height),
            Some(deep_temperature),
        ) = (
            self.upper_wind,
            self.thermocline_temperature,
            self.upper_height,
            self.thermocline_height,
            self.deep_temperature,
        ) {
            let upper_wind = project_monthly_tangent_vectors_cancellable(
                domain,
                surface,
                &upper_wind,
                cancellation,
            )?;
            let mut shear = Vec::with_capacity(upper_wind.len());
            for (cell, (upper, lower)) in upper_wind.iter().zip(&lower_wind).enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                shear.push(std::array::from_fn(|month| {
                    std::array::from_fn(|component| {
                        upper[month][component] - lower[month][component]
                    })
                }));
            }
            let projected_thermocline_height = project_monthly_intensive_scalar_cancellable(
                domain,
                &thermocline_height,
                cancellation,
            )?;
            let reference_depth = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1)
                .layers()
                .iter()
                .find(|layer| layer.role() == ClimateLayerRole::OceanThermocline)
                .expect("locked C2 thermocline")
                .reference_thickness_m() as f32;
            let mut thermocline_depth =
                Vec::with_capacity(projected_thermocline_height.values().len());
            for (cell, months) in projected_thermocline_height.values().iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                thermocline_depth.push(months.map(|height| reference_depth + height));
            }
            let thermocline_temperature = project_monthly_intensive_scalar_cancellable(
                domain,
                &thermocline_temperature,
                cancellation,
            )?;
            let upper_height =
                project_monthly_intensive_scalar_cancellable(domain, &upper_height, cancellation)?;
            let deep_temperature = project_monthly_intensive_scalar_cancellable(
                domain,
                &deep_temperature,
                cancellation,
            )?;
            let cancelled = || cancellation.is_cancelled();
            let fields = GlobalCirculationFields::new_c2_cancellable(
                MonthlyVector3Field::from_values_cancellable(lower_wind, &cancelled)?,
                MonthlyVector3Field::from_values_cancellable(upper_wind, &cancelled)?,
                MonthlyVector3Field::from_values_cancellable(shear, &cancelled)?,
                MonthlyVector3Field::from_values_cancellable(ocean_current, &cancelled)?,
                MonthlyScalarField::from_values_cancellable(air.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(sea.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(
                    thermocline_temperature.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(thermocline_depth, &cancelled)?,
                MonthlyScalarField::from_values_cancellable(humidity.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(
                    precipitation.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    orographic_precipitation.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    lower_height.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    upper_height.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(sea_height.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(
                    projected_thermocline_height.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    deep_temperature.into_values(),
                    &cancelled,
                )?,
                &cancelled,
            )?;
            Ok((fields, precipitation_relative_error))
        } else {
            let cancelled = || cancellation.is_cancelled();
            let fields = GlobalCirculationFields::new_c1_cancellable(
                MonthlyVector3Field::from_values_cancellable(lower_wind, &cancelled)?,
                MonthlyVector3Field::from_values_cancellable(ocean_current, &cancelled)?,
                MonthlyScalarField::from_values_cancellable(air.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(sea.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(humidity.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(
                    precipitation.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    orographic_precipitation.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    lower_height.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(sea_height.into_values(), &cancelled)?,
                &cancelled,
            )?;
            Ok((fields, precipitation_relative_error))
        }
    }
}

fn copy_scalar_month(
    target: &mut [[f32; CLIMATE_MONTH_COUNT]],
    source: &[f32],
    month: usize,
    cancellation: &BuildCancellation,
) -> Result<(), GlobalCirculationGenerationError> {
    for (cell, (target, source)) in target.iter_mut().zip(source).enumerate() {
        if cell % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        target[month] = *source;
    }
    Ok(())
}

fn copy_vector_month(
    target: &mut [[[f32; 3]; CLIMATE_MONTH_COUNT]],
    source: &[[f32; 3]],
    month: usize,
    cancellation: &BuildCancellation,
) -> Result<(), GlobalCirculationGenerationError> {
    for (cell, (target, source)) in target.iter_mut().zip(source).enumerate() {
        if cell % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        target[month] = *source;
    }
    Ok(())
}

struct BudgetAccumulator {
    atmosphere_residual: f64,
    ocean_residual: f64,
    moisture_residual: f64,
    energy_residual: f64,
    paired_heat_residual: f64,
    paired_momentum_residual: f64,
    paired_moisture_residual: f64,
    atmosphere_scale: f64,
    ocean_scale: f64,
    moisture_scale: f64,
    energy_scale: f64,
    paired_heat_scale: f64,
    paired_momentum_scale: f64,
    paired_moisture_scale: f64,
}

impl BudgetAccumulator {
    fn new(
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        state: &LayeredClimateState,
        cancellation: &BuildCancellation,
    ) -> Result<Self, GlobalCirculationGenerationError> {
        Ok(Self {
            atmosphere_residual: 0.0,
            ocean_residual: 0.0,
            moisture_residual: 0.0,
            energy_residual: 0.0,
            paired_heat_residual: 0.0,
            paired_momentum_residual: 0.0,
            paired_moisture_residual: 0.0,
            atmosphere_scale: layer_amount_total(grid, state, true, cancellation)?
                .abs()
                .max(1.0),
            ocean_scale: layer_amount_total(grid, state, false, cancellation)?
                .abs()
                .max(1.0),
            moisture_scale: moisture_total(grid, state, cancellation)?.abs().max(1.0),
            energy_scale: energy_total(grid, layout, state, cancellation)?
                .abs()
                .max(1.0),
            paired_heat_scale: 0.0,
            paired_momentum_scale: 0.0,
            paired_moisture_scale: 0.0,
        })
    }

    fn record(
        &mut self,
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        before: &LayeredClimateState,
        after: &LayeredClimateState,
        tendency: &super::LayeredClimateTendency,
        dt: f64,
        cancellation: &BuildCancellation,
    ) -> Result<(), GlobalCirculationGenerationError> {
        let tendency_budget = tendency.budget();
        let atmosphere_expected = tendency_budget.external_atmosphere_amount_rate_m3_s() * dt;
        let ocean_expected = tendency_budget.external_ocean_amount_rate_m3_s() * dt;
        // Accumulate the signed global closure over the complete formation
        // interval. Computing per-cell deltas avoids subtracting two planet-
        // scale totals, and deferring the absolute value until `finish` avoids
        // turning unbiased f32 integration roundoff into an artificial drift
        // that grows with resolution and macro-step count.
        self.atmosphere_residual +=
            layer_amount_change_total(grid, before, after, true, cancellation)?
                - atmosphere_expected;
        self.ocean_residual +=
            layer_amount_change_total(grid, before, after, false, cancellation)? - ocean_expected;
        let moisture_expected = tendency_budget.external_moisture_net_rate_kg_s() * dt;
        self.moisture_residual +=
            moisture_change_total(grid, before, after, cancellation)? - moisture_expected;
        let energy_expected = tendency_budget.external_heat_rate_w() * dt;
        self.energy_residual +=
            energy_change_total(grid, layout, before, after, cancellation)? - energy_expected;
        self.paired_heat_residual += tendency_budget.paired_heat_residual_w().abs();
        self.paired_momentum_residual += tendency_budget.paired_momentum_residual_n().abs();
        self.paired_moisture_residual += tendency_budget.paired_moisture_residual_kg_s().abs();
        self.paired_heat_scale += tendency_budget.paired_heat_absolute_w();
        self.paired_momentum_scale += tendency_budget.paired_momentum_absolute_n();
        self.paired_moisture_scale += tendency_budget.paired_moisture_absolute_kg_s();
        check_cancelled(cancellation)?;
        Ok(())
    }

    fn finish(self) -> Result<ClimateBudgetReport, ClimateReportError> {
        let component_pair_errors = [
            relative_exchange_error(self.paired_heat_residual, self.paired_heat_scale),
            relative_exchange_error(self.paired_momentum_residual, self.paired_momentum_scale),
            relative_exchange_error(self.paired_moisture_residual, self.paired_moisture_scale),
        ];
        let paired_relative_error = component_pair_errors.into_iter().fold(0.0_f64, f64::max);
        ClimateBudgetReport::new(
            self.atmosphere_residual.abs() / self.atmosphere_scale,
            self.ocean_residual.abs() / self.ocean_scale,
            self.moisture_residual.abs() / self.moisture_scale,
            self.energy_residual.abs() / self.energy_scale,
            paired_relative_error,
        )
    }
}

fn relative_exchange_error(residual: f64, scale: f64) -> f64 {
    if scale > 0.0 {
        residual / scale
    } else {
        residual.abs()
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
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    let mut total = 0.0;
    for role in state
        .active_roles()
        .iter()
        .filter(|role| is_atmosphere(**role) == atmosphere)
    {
        let reference = f64::from(state.reference_thickness_m(*role).expect("active role"));
        for (index, (cell, anomaly)) in grid
            .cells()
            .iter()
            .zip(state.height_anomaly_m(*role).expect("active role"))
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            total += cell.area_m2() * (reference + f64::from(*anomaly));
        }
    }
    Ok(total)
}

fn layer_amount_change_total(
    grid: &CubedSphereGrid,
    before: &LayeredClimateState,
    after: &LayeredClimateState,
    atmosphere: bool,
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    let mut total = 0.0;
    for role in before
        .active_roles()
        .iter()
        .filter(|role| is_atmosphere(**role) == atmosphere)
    {
        for (index, (cell, (before_value, after_value))) in grid
            .cells()
            .iter()
            .zip(
                before
                    .height_anomaly_m(*role)
                    .expect("active before role")
                    .iter()
                    .zip(after.height_anomaly_m(*role).expect("active after role")),
            )
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            total += cell.area_m2() * (f64::from(*after_value) - f64::from(*before_value));
        }
    }
    Ok(total)
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

fn moisture_total(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    let lower_mass = match state.profile() {
        ClimateModelProfile::C1SingleLayerV1 => 1.225 * 8_000.0,
        ClimateModelProfile::C2LayeredV1 => 1.225 * 6_000.0,
    };
    let mut total = 0.0;
    for (index, (cell, value)) in grid
        .cells()
        .iter()
        .zip(state.specific_humidity())
        .enumerate()
    {
        if index % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        total += cell.area_m2() * lower_mass * f64::from(*value);
    }
    if let Some(upper) = state.upper_specific_humidity() {
        let upper_mass = 1.225 * 4_000.0;
        for (index, (cell, value)) in grid.cells().iter().zip(upper).enumerate() {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            total += cell.area_m2() * upper_mass * f64::from(*value);
        }
    }
    Ok(total)
}

fn moisture_change_total(
    grid: &CubedSphereGrid,
    before: &LayeredClimateState,
    after: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    let lower_mass = match before.profile() {
        ClimateModelProfile::C1SingleLayerV1 => 1.225 * 8_000.0,
        ClimateModelProfile::C2LayeredV1 => 1.225 * 6_000.0,
    };
    let mut total = 0.0;
    for (index, (cell, (before_value, after_value))) in grid
        .cells()
        .iter()
        .zip(
            before
                .specific_humidity()
                .iter()
                .zip(after.specific_humidity()),
        )
        .enumerate()
    {
        if index % 256 == 0 {
            check_cancelled(cancellation)?;
        }
        total += cell.area_m2() * lower_mass * (f64::from(*after_value) - f64::from(*before_value));
    }
    if let (Some(before), Some(after)) = (
        before.upper_specific_humidity(),
        after.upper_specific_humidity(),
    ) {
        let upper_mass = 1.225 * 4_000.0;
        for (index, (cell, (before_value, after_value))) in grid
            .cells()
            .iter()
            .zip(before.iter().zip(after))
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            total +=
                cell.area_m2() * upper_mass * (f64::from(*after_value) - f64::from(*before_value));
        }
    }
    Ok(total)
}

fn energy_total(
    grid: &CubedSphereGrid,
    layout: &ClimateLayerLayout,
    state: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    let mut total = 0.0_f64;
    for role in state.active_roles() {
        let capacity = layer_heat_capacity_per_area(layout, *role);
        for (index, (cell, value)) in grid
            .cells()
            .iter()
            .zip(state.temperature_c(*role).expect("active role"))
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            total += cell.area_m2() * capacity * (f64::from(*value) + 273.15);
        }
    }
    if let Some(deep) = state.deep_ocean_temperature_c() {
        let capacity = layer_heat_capacity_per_area(layout, ClimateLayerRole::DeepOceanReservoir);
        for (index, (cell, value)) in grid.cells().iter().zip(deep).enumerate() {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            total += cell.area_m2() * capacity * (f64::from(*value) + 273.15);
        }
    }
    Ok(total)
}

fn energy_change_total(
    grid: &CubedSphereGrid,
    layout: &ClimateLayerLayout,
    before: &LayeredClimateState,
    after: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    let mut total = 0.0_f64;
    for role in before.active_roles() {
        let capacity = layer_heat_capacity_per_area(layout, *role);
        for (index, (cell, (before_value, after_value))) in grid
            .cells()
            .iter()
            .zip(
                before
                    .temperature_c(*role)
                    .expect("active before role")
                    .iter()
                    .zip(after.temperature_c(*role).expect("active after role")),
            )
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            total +=
                cell.area_m2() * capacity * (f64::from(*after_value) - f64::from(*before_value));
        }
    }
    if let (Some(before), Some(after)) = (
        before.deep_ocean_temperature_c(),
        after.deep_ocean_temperature_c(),
    ) {
        let capacity = layer_heat_capacity_per_area(layout, ClimateLayerRole::DeepOceanReservoir);
        for (index, (cell, (before_value, after_value))) in grid
            .cells()
            .iter()
            .zip(before.iter().zip(after))
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            total +=
                cell.area_m2() * capacity * (f64::from(*after_value) - f64::from(*before_value));
        }
    }
    Ok(total)
}

fn remap_report(
    domain: &ClimateWorkDomainSnapshot,
    published_precipitation_relative_error: f64,
) -> Result<ClimateRemapReport, GlobalCirculationGenerationError> {
    let forward = domain.source_to_climate();
    let reverse = domain.climate_to_source();
    Ok(ClimateRemapReport::new(
        forward.solve_stats().max_source_margin_relative_error(),
        forward.solve_stats().max_target_margin_relative_error(),
        reverse.solve_stats().max_source_margin_relative_error(),
        reverse.solve_stats().max_target_margin_relative_error(),
        published_precipitation_relative_error,
        u32::try_from(forward.overlap_count())
            .map_err(|_| GlobalCirculationGenerationError::AllocationOverflow)?,
        u32::try_from(reverse.overlap_count())
            .map_err(|_| GlobalCirculationGenerationError::AllocationOverflow)?,
    )?)
}

fn input_fingerprint(
    surface_ref: SurfaceRef,
    domain: &ClimateWorkDomainSnapshot,
    forcing: &GlobalClimateForcing,
    layout: &ClimateLayerLayout,
    cancellation: &BuildCancellation,
) -> Result<[u8; 32], GlobalCirculationGenerationError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.global-circulation-input.v1\0");
    hasher.update(&surface_ref.fingerprint());
    let domain_fingerprint = domain
        .fingerprint_cancellable(&|| cancellation.is_cancelled())
        .map_err(|error| {
            if error == crate::world::spatial::ConservativeSurfaceMapError::Cancelled {
                GlobalCirculationGenerationError::Cancelled
            } else {
                GlobalCirculationGenerationError::InputFingerprint {
                    reason: error.to_string(),
                }
            }
        })?;
    hasher.update(&domain_fingerprint);
    hasher.update(forcing.fingerprint());
    hasher.update(&super::global_circulation_model_fingerprint(
        layout.profile(),
    ));
    Ok(*hasher.finalize().as_bytes())
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

fn map_grid_error(error: CubedSphereGridError) -> GlobalCirculationGenerationError {
    if error == CubedSphereGridError::Cancelled {
        GlobalCirculationGenerationError::Cancelled
    } else {
        GlobalCirculationGenerationError::Grid(error)
    }
}

fn map_state_error(error: LayeredStateError) -> GlobalCirculationGenerationError {
    if error == LayeredStateError::Cancelled {
        GlobalCirculationGenerationError::Cancelled
    } else {
        GlobalCirculationGenerationError::State(error)
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum GlobalCirculationGenerationError {
    #[error("global circulation generation was cancelled")]
    Cancelled,
    #[error("invalid authoritative surface identity: {reason}")]
    InvalidSurfaceIdentity { reason: String },
    #[error("climate input fingerprint failed: {reason}")]
    InputFingerprint { reason: String },
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
    WorkDomain(ClimateWorkDomainValidationError),
    #[error(transparent)]
    Grid(CubedSphereGridError),
    #[error(transparent)]
    Forcing(super::GlobalClimateForcingError),
    #[error(transparent)]
    State(LayeredStateError),
    #[error(transparent)]
    Tendency(LayeredTendencyError),
    #[error(transparent)]
    Integrator(ClimateIntegratorError),
    #[error(transparent)]
    Projection(ClimateProjectionError),
    #[error(transparent)]
    ClimateField(ClimateValidationError),
    #[error(transparent)]
    Validation(GlobalCirculationValidationError),
    #[error(transparent)]
    Report(#[from] ClimateReportError),
    #[error(transparent)]
    Checkpoint(#[from] ClimateCheckpointError),
}

impl From<ClimateWorkDomainValidationError> for GlobalCirculationGenerationError {
    fn from(error: ClimateWorkDomainValidationError) -> Self {
        if error == ClimateWorkDomainValidationError::Cancelled {
            Self::Cancelled
        } else {
            Self::WorkDomain(error)
        }
    }
}

impl From<CubedSphereGridError> for GlobalCirculationGenerationError {
    fn from(error: CubedSphereGridError) -> Self {
        map_grid_error(error)
    }
}

impl From<super::GlobalClimateForcingError> for GlobalCirculationGenerationError {
    fn from(error: super::GlobalClimateForcingError) -> Self {
        if error == super::GlobalClimateForcingError::Cancelled {
            Self::Cancelled
        } else {
            Self::Forcing(error)
        }
    }
}

impl From<LayeredStateError> for GlobalCirculationGenerationError {
    fn from(error: LayeredStateError) -> Self {
        map_state_error(error)
    }
}

impl From<LayeredTendencyError> for GlobalCirculationGenerationError {
    fn from(error: LayeredTendencyError) -> Self {
        if error == LayeredTendencyError::Cancelled {
            Self::Cancelled
        } else {
            Self::Tendency(error)
        }
    }
}

impl From<ClimateIntegratorError> for GlobalCirculationGenerationError {
    fn from(error: ClimateIntegratorError) -> Self {
        if error == ClimateIntegratorError::Cancelled {
            Self::Cancelled
        } else {
            Self::Integrator(error)
        }
    }
}

impl From<ClimateProjectionError> for GlobalCirculationGenerationError {
    fn from(error: ClimateProjectionError) -> Self {
        if error == ClimateProjectionError::Cancelled {
            Self::Cancelled
        } else {
            Self::Projection(error)
        }
    }
}

impl From<GlobalCirculationValidationError> for GlobalCirculationGenerationError {
    fn from(error: GlobalCirculationValidationError) -> Self {
        if error == GlobalCirculationValidationError::Cancelled {
            Self::Cancelled
        } else {
            Self::Validation(error)
        }
    }
}

impl From<ClimateValidationError> for GlobalCirculationGenerationError {
    fn from(error: ClimateValidationError) -> Self {
        if error == ClimateValidationError::Cancelled {
            Self::Cancelled
        } else {
            Self::ClimateField(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::natural::{
        global_circulation_owner_inventory, global_circulation_tendency_cell_bytes,
    };

    #[test]
    fn c2_tendency_owner_formula_includes_the_f64_external_moisture_ledger() {
        // Four layer tendency records (20 bytes each), five f32 scalar
        // fields, and the retained external-moisture f64 ledger.
        assert_eq!(
            global_circulation_tendency_cell_bytes(ClimateModelProfile::C2LayeredV1),
            108
        );
    }

    #[test]
    fn formation_memory_inventory_covers_assignment_and_rk3_combine_peaks() {
        assert_eq!(global_circulation_owner_inventory(), (7, 5, 5, 3));
    }

    #[test]
    fn thermal_energy_scale_uses_a_positive_absolute_temperature_origin() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![1.0; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1);
        let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let total = energy_total(&grid, &layout, &state, &BuildCancellation::new()).unwrap();
        assert!(total.is_finite() && total > 0.0);
    }

    #[test]
    fn public_budget_rejects_an_unledgered_internal_moisture_leak() {
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let cell_count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; cell_count],
            vec![0.0; cell_count],
            vec![0.0; cell_count],
            vec![1.0; cell_count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; cell_count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; cell_count],
            vec![[0.008; CLIMATE_MONTH_COUNT]; cell_count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1);
        let before = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        let tendency = LayeredTendencySystem::new(&grid)
            .evaluate_for_step(
                &before,
                &forcing,
                &vec![1.0; grid.edges().len()],
                0,
                GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
                &BuildCancellation::new(),
            )
            .unwrap();
        assert_eq!(tendency.budget().external_moisture_net_rate_kg_s(), 0.0);

        let mut leaked = before.clone();
        leaked.specific_humidity_mut()[0] += 0.001;
        let cancellation = BuildCancellation::new();
        let mut budgets = BudgetAccumulator::new(&grid, &layout, &before, &cancellation).unwrap();
        budgets
            .record(
                &grid,
                &layout,
                &before,
                &leaked,
                &tendency,
                GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
                &cancellation,
            )
            .unwrap();
        assert!(matches!(
            budgets.finish(),
            Err(ClimateReportError::StatisticAboveMaximum {
                field: "moisture_relative_error",
                ..
            })
        ));
    }
}
