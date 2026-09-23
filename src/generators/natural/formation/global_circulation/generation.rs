use thiserror::Error;

use super::project::{
    project_intensive_scalar_cancellable, project_monthly_extensive_rate_cancellable,
    project_monthly_intensive_scalar_cancellable, project_monthly_tangent_vectors_cancellable,
};
use super::{
    ClimateIntegratorError, ClimateProjectionError, ClimateStepResult, GlobalClimateForcing,
    LayeredClimateState, LayeredStateError, LayeredTendencyError, LayeredTendencySystem,
    LayeredTendencyWorkspace, SplitExplicitRk3Integrator, SELECTED_PRODUCTION_INTEGRATOR,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::circulation::{
    CirculationOperators, CubedSphereGrid, CubedSphereGridError,
};
use crate::generators::natural::surface_water_geometry::build_surface_water_geometry;
use crate::generators::spatial::{
    remap_extensive_f64_cancellable, remap_intensive_f32_cancellable, ConservativeRemapError,
    ConservativeSurfaceMapBuilder,
};
use crate::world::natural::PlanetForcing;
use crate::world::natural::{
    atmosphere_surface_wind_weights, expected_global_circulation_dense_state_bytes,
    reconstruct_atmosphere_surface_wind_m_s, water_cycle_relative_imbalance, ClimateBudgetReport,
    ClimateCapabilitySet, ClimateCheckpoint, ClimateCheckpointError, ClimateLayerLayout,
    ClimateLayerRole, ClimateModelProfile, ClimateQuantizationId, ClimateRemapReport,
    ClimateReportError, ClimateSolveReport, ClimateValidationError, ClimateWorkDomainSnapshot,
    ClimateWorkDomainValidationError, GlobalCirculationFields, GlobalCirculationSnapshot,
    GlobalCirculationValidationError, MonthlyScalarField, MonthlyVector3Field, CLIMATE_MONTH_COUNT,
    EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2, EARTH_ROTATION_RATE_RAD_S,
    GLOBAL_CIRCULATION_FAST_CFL_TARGET, GLOBAL_CIRCULATION_MACRO_STEP_SECONDS,
    GLOBAL_CIRCULATION_MECHANICAL_COARSE_CYCLES,
    GLOBAL_CIRCULATION_MECHANICAL_COARSE_RESOLUTION_DIVISOR,
    GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S, GLOBAL_CIRCULATION_SCHEMA_V2,
    GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2, GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX,
    WATER_VAPORIZATION_LATENT_HEAT_J_KG,
};
use crate::world::spatial::{ConservativeSurfaceMap, SphericalSurfaceSnapshot, SurfaceRef};

pub(super) const FORMATION_RESIDUAL_TARGET: f64 = 0.24;

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
        let planet = forcing.planet_forcing();
        let terrain_floor_m = LayeredTendencySystem::lower_atmosphere_terrain_floor_m(
            forcing.relative_elevation_m(),
            planet.land_fraction(),
        );
        let integrator = SplitExplicitRk3Integrator::new_with_terrain(
            &grid,
            forcing,
            &terrain_floor_m,
            fast_step_seconds,
        )?;
        let mut state = LayeredClimateState::from_annual_mean_forcing_cancellable(
            &grid,
            &layout,
            planet,
            forcing.sea_level_m(),
            cancellation,
        )
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
        let mut final_budgets = None;
        let mut final_cycle_budget = FinalCycleBudget::default();
        let mut work = WorkClimatology::new(grid.cell_count(), profile);
        let tendency_system = integrator.tendency_system();
        let (slow_step_count, slow_step_seconds) =
            SplitExplicitRk3Integrator::slow_step_plan(GLOBAL_CIRCULATION_MACRO_STEP_SECONDS);
        observer(GlobalCirculationPhase::SolverEntered);
        check_cancelled(cancellation)?;

        let mut formation_cycles = 0_u16;
        for cycle in 0..maximum_formation_cycles {
            let mut cycle_budgets = BudgetAccumulator::new(&grid, &layout, &state, cancellation)?;
            for month in 0..CLIMATE_MONTH_COUNT {
                for slow_step in 0..slow_step_count {
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
                        slow_step_seconds,
                        cancellation,
                    )?;
                    observer(GlobalCirculationPhase::TransportCompleted);
                    let result = integrator.advance_with_declared_tendency_and_phase_observer(
                        &before,
                        planet,
                        forcing.ocean_edge_permeability(),
                        slow_step_seconds,
                        &declared,
                        cancellation,
                        &mut observer,
                    )?;
                    let diagnostics = result.diagnostics();
                    cycle_budgets.record(
                        &grid,
                        &layout,
                        &before,
                        &result,
                        &declared,
                        slow_step_seconds,
                        cancellation,
                    )?;
                    state = result.into_state();
                    state
                        .enforce_full_land_ocean_velocity(planet, cancellation)
                        .map_err(map_state_error)?;
                    state
                        .validate_against_cancellable(&grid, cancellation)
                        .map_err(map_state_error)?;
                    work.record_month(
                        &state,
                        &declared,
                        forcing,
                        (month, slow_step),
                        cancellation,
                    )?;
                    fast_substeps += u64::from(diagnostics.fast_substeps());
                    maximum_cfl = maximum_cfl.max(diagnostics.maximum_cfl());
                }
                // The time contract counts complete forcing phases; internal
                // numerical refinement must not extend the physical interval.
                continuation_steps += 1;
            }
            let residual = relative_state_residual(&grid, &previous_cycle, &state, cancellation)?;
            if cycle == 0 {
                initial_residual = residual;
            }
            final_residual = residual;
            formation_cycles = cycle + 1;
            final_cycle_budget = work.final_cycle_budget(&grid, forcing, cancellation)?;
            let hard_closures_pass = final_cycle_budget.hard_closures_pass();
            final_budgets = Some(cycle_budgets);
            // Publish only after the private initial guess has run a coupled cycle.
            // The profile horizon and every final coupled-cycle gate stay unchanged.
            if profile == ClimateModelProfile::C2LayeredV1
                && cycle == 0
                && cycle + 1 < maximum_formation_cycles
            {
                initialize_atmosphere_mechanics(
                    (&grid, &integrator, &terrain_floor_m),
                    forcing,
                    &work,
                    &mut state,
                    cancellation,
                )?;
                previous_cycle = state
                    .clone_cancellable(cancellation)
                    .map_err(map_state_error)?;
                continue;
            }
            let needs_moisture_preconditioning = continuation_needs_moisture_preconditioning(
                profile,
                final_residual,
                final_cycle_budget,
                cycle + 1,
                maximum_formation_cycles,
            );
            if needs_moisture_preconditioning {
                precondition_periodic_moisture(
                    &grid,
                    &tendency_system,
                    planet,
                    forcing.ocean_edge_permeability(),
                    &work,
                    &mut state,
                    cancellation,
                )?;
            }
            previous_cycle = state
                .clone_cancellable(cancellation)
                .map_err(map_state_error)?;
            if cycle >= 3 && final_residual <= FORMATION_RESIDUAL_TARGET && hard_closures_pass {
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
        let budget_report = final_budgets
            .expect("at least one formation cycle is required")
            .finish(final_cycle_budget)?;
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

/// Resting-state fast step from the wave and Coriolis Courant limits; the
/// integrator tightens it further from the measured flow speed of each
/// macro step.
fn stable_fast_step_seconds(grid: &CubedSphereGrid) -> f64 {
    let wave_limit = GLOBAL_CIRCULATION_FAST_CFL_TARGET * grid.minimum_center_distance_m()
        / GLOBAL_CIRCULATION_REFERENCE_WAVE_SPEED_M_S;
    let rotation_limit = GLOBAL_CIRCULATION_FAST_CFL_TARGET / (2.0 * EARTH_ROTATION_RATE_RAD_S);
    wave_limit.min(rotation_limit)
}

// Projected mechanical block iteration; this is not an independent
// frozen-scalar mechanical RHS. The production split still applies its scalar
// endpoint before fast thermal pressure, and recomputes Q at every declaration.
// Preceding-month endpoints approximate both slow-step scalar inputs. Therefore
// no equivalence to an exact coupled periodic fixed point is claimed.
// Only atmospheric H/u are injected; the caller must run another coupled cycle.
fn initialize_atmosphere_mechanics(
    context: (&CubedSphereGrid, &SplitExplicitRk3Integrator<'_>, &[f32]),
    forcing: &GlobalClimateForcing,
    background: &WorkClimatology,
    state: &mut LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<(), GlobalCirculationGenerationError> {
    let (fine_grid, fine_integrator, fine_terrain_floor) = context;
    let cancelled = || cancellation.is_cancelled();
    let fine_planet = forcing.planet_forcing();
    let fine_context = MechanicalCycleContext {
        grid: fine_grid,
        integrator: fine_integrator,
        forcing: fine_planet,
        ocean_edge_permeability: forcing.ocean_edge_permeability(),
        terrain_floor: fine_terrain_floor,
        temperature: [
            &background.air_temperature,
            background
                .upper_air_temperature
                .as_ref()
                .expect("C2 upper temperature"),
            &background.sea_temperature,
            background
                .thermocline_temperature
                .as_ref()
                .expect("C2 thermocline temperature"),
        ],
        humidity: [
            &background.humidity,
            background
                .upper_humidity
                .as_ref()
                .expect("C2 upper humidity"),
        ],
        deep_temperature: background
            .deep_temperature
            .as_ref()
            .expect("C2 deep temperature"),
    };
    // A failure of the true fine map is not hidden as an optional coarse failure.
    // Do not pack x until this expensive call has released its RK stacks.
    let mut fine_image = state
        .clone_cancellable(cancellation)
        .map_err(map_state_error)?;
    advance_mechanical_cycle(&fine_context, &mut fine_image, cancellation)?;
    let fine_image_amounts =
        pack_mechanical_amounts(fine_grid, &fine_image, fine_terrain_floor, cancellation)?;
    drop(fine_image);
    // Everything inside the attempt is private. Rejection leaves the original
    // fine entry, including all its scalars, completely unchanged.
    let attempt = (|| -> Result<LayeredClimateState, GlobalCirculationGenerationError> {
        let coarse_grid = CubedSphereGrid::new_cancellable(
            fine_grid.face_resolution() / GLOBAL_CIRCULATION_MECHANICAL_COARSE_RESOLUTION_DIVISOR,
            fine_grid.radius_m(),
            &cancelled,
        )
        .map_err(map_grid_error)?;
        let fine_surface = fine_grid
            .to_surface_snapshot_cancellable(&cancelled)
            .map_err(map_grid_error)?;
        let coarse_surface = coarse_grid
            .to_surface_snapshot_cancellable(&cancelled)
            .map_err(map_grid_error)?;
        let restriction = ConservativeSurfaceMapBuilder::build_cancellable(
            &fine_surface,
            &coarse_surface,
            &cancelled,
        )
        .map_err(map_mechanical_remap_error)?;
        let prolongation = ConservativeSurfaceMapBuilder::build_cancellable(
            &coarse_surface,
            &fine_surface,
            &cancelled,
        )
        .map_err(map_mechanical_remap_error)?;
        let restrict = |values: &[f32]| {
            remap_intensive_f32_cancellable(&restriction, values, &cancelled)
                .map_err(map_mechanical_remap_error)
        };
        let monthly = |values: &[[f32; CLIMATE_MONTH_COUNT]]| {
            remap_mechanical_monthly_background(&restriction, values, cancellation)
        };

        let coarse_planet = PlanetForcing::new_cancellable_with_ocean_depth(
            *coarse_grid.fingerprint(),
            restrict(fine_planet.elevation_m())?,
            restrict(fine_planet.land_fraction())?,
            restrict(fine_planet.ocean_depth_m())?,
            restrict(fine_planet.surface_albedo())?,
            restrict(fine_planet.surface_moisture_availability())?,
            monthly(fine_planet.monthly_absorbed_shortwave_w_m2())?,
            monthly(fine_planet.equilibrium_air_temperature_c())?,
            monthly(fine_planet.equilibrium_surface_temperature_c())?,
            monthly(fine_planet.equilibrium_specific_humidity())?,
            &cancelled,
        )
        .map_err(|error| map_mechanical_input_error(error, cancellation))?;
        let relative_elevation = restrict(forcing.relative_elevation_m())?;
        // Recompute the coarse production floor; transferring actual H below
        // keeps this nonlinear terrain diagnostic from silently creating mass.
        let terrain_floor = LayeredTendencySystem::lower_atmosphere_terrain_floor_m(
            &relative_elevation,
            coarse_planet.land_fraction(),
        );
        let terrain_gradient = CirculationOperators::new(&coarse_grid)
            .gradient_cancellable(&relative_elevation, cancellation)
            .map_err(|error| map_mechanical_input_error(error, cancellation))?;
        let evapotranspiration = restrict(forcing.land_evapotranspiration_fraction())?;
        let water_geometry = build_surface_water_geometry(
            &coarse_surface,
            coarse_planet.elevation_m(),
            forcing.sea_level_m(),
            cancellation,
        )
        .map_err(|error| map_mechanical_input_error(error, cancellation))?;
        let coarse_integrator = SplitExplicitRk3Integrator::with_tendency_system(
            &coarse_grid,
            LayeredTendencySystem::with_terrain(
                &coarse_grid,
                &terrain_gradient,
                &terrain_floor,
                &evapotranspiration,
                forcing.sea_level_m(),
            ),
            stable_fast_step_seconds(&coarse_grid),
        )?;
        let air_temperature = monthly(&background.air_temperature)?;
        let upper_temperature = monthly(
            background
                .upper_air_temperature
                .as_ref()
                .expect("C2 upper temperature background"),
        )?;
        let sea_temperature = monthly(&background.sea_temperature)?;
        let thermocline_temperature = monthly(
            background
                .thermocline_temperature
                .as_ref()
                .expect("C2 thermocline temperature background"),
        )?;
        let humidity = monthly(&background.humidity)?;
        let upper_humidity = monthly(
            background
                .upper_humidity
                .as_ref()
                .expect("C2 upper humidity background"),
        )?;
        let deep_temperature = monthly(
            background
                .deep_temperature
                .as_ref()
                .expect("C2 deep temperature background"),
        )?;
        let layout = ClimateLayerLayout::for_profile(state.profile());
        let mut candidate = LayeredClimateState::from_annual_mean_forcing_cancellable(
            &coarse_grid,
            &layout,
            &coarse_planet,
            forcing.sea_level_m(),
            cancellation,
        )
        .map_err(map_state_error)?;
        let tau = remap_mechanical_amounts(
            &restriction,
            &coarse_grid,
            &fine_image_amounts,
            cancellation,
        )?;
        drop(fine_image_amounts);
        let mut fine_amounts =
            pack_mechanical_amounts(fine_grid, state, fine_terrain_floor, cancellation)?;
        let restricted_entry =
            remap_mechanical_amounts(&restriction, &coarse_grid, &fine_amounts, cancellation)?;
        drop(restriction);
        drop(fine_surface);
        drop(coarse_surface);
        let fine_volume: f64 = fine_amounts[..2].iter().flatten().map(|cell| cell[0]).sum();
        let coarse_context = MechanicalCycleContext {
            grid: &coarse_grid,
            integrator: &coarse_integrator,
            forcing: &coarse_planet,
            ocean_edge_permeability: water_geometry.wet_edge_fraction(),
            terrain_floor: &terrain_floor,
            temperature: [
                &air_temperature,
                &upper_temperature,
                &sea_temperature,
                &thermocline_temperature,
            ],
            humidity: [&humidity, &upper_humidity],
            deep_temperature: &deep_temperature,
        };
        unpack_mechanical_amounts(
            &coarse_grid,
            (candidate.active_roles(), &restricted_entry),
            &mut candidate,
            &terrain_floor,
            cancellation,
        )?;
        candidate.enforce_full_land_ocean_velocity(&coarse_planet, cancellation)?;
        advance_mechanical_cycle(&coarse_context, &mut candidate, cancellation)?;
        let first_coarse_image =
            pack_mechanical_amounts(&coarse_grid, &candidate, &terrain_floor, cancellation)?;
        // PETSc FAS: Nc(y)=Nc(Rx)-R Nf(x), with N=x-Phi(x).
        // Thus tau=R Phi_f(x)-Phi_c(Rx), and y_next=Phi_c(y)+tau.
        // This first coarse evaluation serves both tau and update number one.
        let (tau, mut y) = fas_first_coarse_update(tau, first_coarse_image, cancellation)?;
        for coarse_cycle in 0..GLOBAL_CIRCULATION_MECHANICAL_COARSE_CYCLES {
            if coarse_cycle != 0 {
                advance_mechanical_cycle(&coarse_context, &mut candidate, cancellation)?;
                y = pack_mechanical_amounts(
                    &coarse_grid,
                    &candidate,
                    &terrain_floor,
                    cancellation,
                )?;
                add_scaled_mechanical_amounts(&mut y, 1.0, &tau, cancellation)?;
            }
            // Preserve raw y for the final correction. Packing the rounded
            // state here would perturb even an exact fine fixed point.
            unpack_mechanical_amounts(
                &coarse_grid,
                (candidate.active_roles(), &y),
                &mut candidate,
                &terrain_floor,
                cancellation,
            )?;
            candidate.enforce_full_land_ocean_velocity(&coarse_planet, cancellation)?;
        }
        add_scaled_mechanical_amounts(&mut y, -1.0, &restricted_entry, cancellation)?;
        // Restricted nonlinear correction: the coupled coarse solve supplies
        // only atmospheric H/P to the fine trial. Its ocean state stays local.
        let atmosphere_roles = [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ];
        let correction = remap_mechanical_amounts(
            &prolongation,
            fine_grid,
            &y[..atmosphere_roles.len()],
            cancellation,
        )?;
        add_scaled_mechanical_amounts(
            &mut fine_amounts[..atmosphere_roles.len()],
            1.0,
            &correction,
            cancellation,
        )?;
        drop(correction);
        let mut trial = state
            .clone_cancellable(cancellation)
            .map_err(map_state_error)?;
        let retained_volume = unpack_mechanical_amounts(
            fine_grid,
            (&atmosphere_roles, &fine_amounts[..atmosphere_roles.len()]),
            &mut trial,
            fine_terrain_floor,
            cancellation,
        )?;
        // The original fine ocean H/u were never unpacked or reprojected;
        // its already-enforced land constraint therefore remains unchanged.
        let volume_error = (retained_volume - fine_volume).abs() / fine_volume;
        if !volume_error.is_finite()
            || volume_error > crate::world::natural::GLOBAL_CIRCULATION_BUDGET_RELATIVE_ERROR_MAX
        {
            return Err(ClimateProjectionError::InvalidDomain {
                reason: format!("FAS atmosphere volume relative error {volume_error} exceeds existing budget limit"),
            }.into());
        }
        Ok(trial)
    })();
    match attempt {
        Ok(trial) => {
            *state = trial;
        }
        Err(GlobalCirculationGenerationError::Cancelled) => {
            return Err(GlobalCirculationGenerationError::Cancelled);
        }
        // Optional coarse rejection leaves the original fine entry unchanged.
        Err(_) => {}
    }
    check_cancelled(cancellation)?;
    Ok(())
}

fn map_mechanical_remap_error(error: ConservativeRemapError) -> GlobalCirculationGenerationError {
    if error == ConservativeRemapError::Cancelled {
        GlobalCirculationGenerationError::Cancelled
    } else {
        ClimateProjectionError::Remap(error).into()
    }
}

fn map_mechanical_input_error(
    error: impl std::fmt::Display,
    cancellation: &BuildCancellation,
) -> GlobalCirculationGenerationError {
    if cancellation.is_cancelled() {
        GlobalCirculationGenerationError::Cancelled
    } else {
        GlobalCirculationGenerationError::InvalidForcing {
            reason: error.to_string(),
        }
    }
}

fn remap_mechanical_monthly_background(
    map: &ConservativeSurfaceMap,
    source: &[[f32; CLIMATE_MONTH_COUNT]],
    cancellation: &BuildCancellation,
) -> Result<Vec<[f32; CLIMATE_MONTH_COUNT]>, GlobalCirculationGenerationError> {
    let mut result = vec![[0.0; CLIMATE_MONTH_COUNT]; map.target_ref().cell_count() as usize];
    let mut column = vec![0.0; source.len()];
    for month in 0..CLIMATE_MONTH_COUNT {
        check_cancelled(cancellation)?;
        for (target, row) in column.iter_mut().zip(source) {
            *target = row[month];
        }
        let mapped = remap_intensive_f32_cancellable(map, &column, &|| cancellation.is_cancelled())
            .map_err(map_mechanical_remap_error)?;
        for (row, value) in result.iter_mut().zip(mapped) {
            row[month] = value;
        }
    }
    Ok(result)
}

// The seven monthly backgrounds are borrowed by both real grid consumers.
// A cycle always starts at January and resets the preceding-month endpoint
// before each production slow step; it is the existing projected map.
struct MechanicalCycleContext<'a, 'grid> {
    grid: &'grid CubedSphereGrid,
    integrator: &'a SplitExplicitRk3Integrator<'grid>,
    forcing: &'a PlanetForcing,
    ocean_edge_permeability: &'a [f32],
    terrain_floor: &'a [f32],
    temperature: [&'a [[f32; CLIMATE_MONTH_COUNT]]; 4],
    humidity: [&'a [[f32; CLIMATE_MONTH_COUNT]]; 2],
    deep_temperature: &'a [[f32; CLIMATE_MONTH_COUNT]],
}

fn advance_mechanical_cycle(
    context: &MechanicalCycleContext<'_, '_>,
    state: &mut LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<(), GlobalCirculationGenerationError> {
    let (slow_steps, dt) =
        SplitExplicitRk3Integrator::slow_step_plan(GLOBAL_CIRCULATION_MACRO_STEP_SECONDS);
    for month in 0..CLIMATE_MONTH_COUNT {
        let previous = (month + CLIMATE_MONTH_COUNT - 1) % CLIMATE_MONTH_COUNT;
        for _ in 0..slow_steps {
            check_cancelled(cancellation)?;
            for (layer, &role) in state.active_roles().iter().enumerate() {
                for (cell, (target, background)) in state
                    .temperature_c_mut(role)
                    .expect("C2 active temperature")
                    .iter_mut()
                    .zip(context.temperature[layer])
                    .enumerate()
                {
                    if cell % 256 == 0 {
                        check_cancelled(cancellation)?;
                    }
                    *target = background[previous];
                }
            }
            for (cell, (target, background)) in state
                .specific_humidity_mut()
                .iter_mut()
                .zip(context.humidity[0])
                .enumerate()
            {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                *target = background[previous];
            }
            for (cell, (target, background)) in state
                .upper_specific_humidity_mut()
                .expect("C2 upper humidity")
                .iter_mut()
                .zip(context.humidity[1])
                .enumerate()
            {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                *target = background[previous];
            }
            for (cell, (target, background)) in state
                .deep_ocean_temperature_c_mut()
                .expect("C2 deep temperature")
                .iter_mut()
                .zip(context.deep_temperature)
                .enumerate()
            {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                *target = background[previous];
            }
            let result = context.integrator.advance(
                state,
                context.forcing,
                context.ocean_edge_permeability,
                month,
                dt,
                cancellation,
            )?;
            *state = result.into_state();
            state.enforce_full_land_ocean_velocity(context.forcing, cancellation)?;
            state
                .validate_against_cancellable(context.grid, cancellation)
                .map_err(map_state_error)?;
            for &role in state.active_roles() {
                for (cell, &height) in state
                    .height_anomaly_m(role)
                    .expect("active height")
                    .iter()
                    .enumerate()
                {
                    if cell % 256 == 0 {
                        check_cancelled(cancellation)?;
                    }
                    LayeredTendencySystem::validated_fluid_layer_thickness_m(
                        f64::from(state.reference_thickness_m(role).expect("active reference")),
                        height,
                        if role == ClimateLayerRole::LowerAtmosphere {
                            context.terrain_floor[cell]
                        } else {
                            0.0
                        },
                        role,
                        cell,
                    )?;
                }
            }
        }
    }
    Ok(())
}

// Each active layer stores per-cell (V, Px, Py, Pz), with V=A*H_actual.
// This transient f64 coordinate is also used for signed defects. It is never
// interpreted as a physical state until explicitly unpacked and validated.
type MechanicalAmounts = Vec<Vec<[f64; 4]>>;

fn pack_mechanical_amounts(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    floor: &[f32],
    cancellation: &BuildCancellation,
) -> Result<MechanicalAmounts, GlobalCirculationGenerationError> {
    let layout = ClimateLayerLayout::for_profile(state.profile());
    let mut amounts = Vec::with_capacity(state.active_roles().len());
    for &role in state.active_roles() {
        let rho = layout
            .layers()
            .iter()
            .find(|spec| spec.role() == role)
            .expect("active layer")
            .density_kg_m3();
        let mut values = Vec::with_capacity(grid.cell_count());
        for (cell, geometry) in grid.cells().iter().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let h = LayeredTendencySystem::validated_fluid_layer_thickness_m(
                f64::from(state.reference_thickness_m(role).expect("active reference")),
                state.height_anomaly_m(role).expect("active height")[cell],
                if role == ClimateLayerRole::LowerAtmosphere {
                    floor[cell]
                } else {
                    0.0
                },
                role,
                cell,
            )?;
            let volume = geometry.area_m2() * h;
            let velocity = state.velocity_m_s(role).expect("active velocity")[cell];
            values.push([
                volume,
                rho * volume * f64::from(velocity[0]),
                rho * volume * f64::from(velocity[1]),
                rho * volume * f64::from(velocity[2]),
            ]);
        }
        amounts.push(values);
    }
    Ok(amounts)
}

fn remap_mechanical_amounts(
    map: &ConservativeSurfaceMap,
    target_grid: &CubedSphereGrid,
    source: &[Vec<[f64; 4]>],
    cancellation: &BuildCancellation,
) -> Result<MechanicalAmounts, GlobalCirculationGenerationError> {
    let mut result = Vec::with_capacity(source.len());
    for layer in source {
        let mut target = vec![[0.0; 4]; map.target_ref().cell_count() as usize];
        let mut column = Vec::with_capacity(layer.len());
        for component in 0..4 {
            check_cancelled(cancellation)?;
            column.clear();
            for (cell, value) in layer.iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                column.push(value[component]);
            }
            let mapped =
                remap_extensive_f64_cancellable(map, &column, &|| cancellation.is_cancelled())
                    .map_err(map_mechanical_remap_error)?;
            for (cell, (value, &amount)) in target.iter_mut().zip(mapped.values()).enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                value[component] = amount;
            }
        }
        for (cell, value) in target.iter_mut().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let radial = target_grid.cells()[cell].center_unit();
            let radial = crate::world::spatial::UnitVector3::new(radial[0], radial[1], radial[2])
                .expect("validated grid center has a finite nonzero direction");
            let tangent =
                crate::world::spatial::project_tangent([value[1], value[2], value[3]], radial);
            value[1..].copy_from_slice(&tangent);
        }
        result.push(target);
    }
    Ok(result)
}

fn add_scaled_mechanical_amounts(
    target: &mut [Vec<[f64; 4]>],
    scale: f64,
    increment: &[Vec<[f64; 4]>],
    cancellation: &BuildCancellation,
) -> Result<(), GlobalCirculationGenerationError> {
    for (layer, change) in target.iter_mut().zip(increment) {
        for (cell, (value, delta)) in layer.iter_mut().zip(change).enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            for component in 0..4 {
                value[component] += scale * delta[component];
            }
        }
    }
    Ok(())
}

fn fas_first_coarse_update(
    mut restricted_fine_image: MechanicalAmounts,
    mut first_coarse_image: MechanicalAmounts,
    cancellation: &BuildCancellation,
) -> Result<(MechanicalAmounts, MechanicalAmounts), GlobalCirculationGenerationError> {
    add_scaled_mechanical_amounts(
        &mut restricted_fine_image,
        -1.0,
        &first_coarse_image,
        cancellation,
    )?;
    add_scaled_mechanical_amounts(
        &mut first_coarse_image,
        1.0,
        &restricted_fine_image,
        cancellation,
    )?;
    Ok((restricted_fine_image, first_coarse_image))
}

fn unpack_mechanical_amounts(
    grid: &CubedSphereGrid,
    (roles, amounts): (&[ClimateLayerRole], &[Vec<[f64; 4]>]),
    state: &mut LayeredClimateState,
    floor: &[f32],
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    let layout = ClimateLayerLayout::for_profile(state.profile());
    let operators = CirculationOperators::new(grid);
    let mut atmosphere_volume = 0.0;
    for (&role, layer_amounts) in roles.iter().zip(amounts) {
        let rho = layout
            .layers()
            .iter()
            .find(|spec| spec.role() == role)
            .expect("active layer")
            .density_kg_m3();
        let reference = f64::from(state.reference_thickness_m(role).expect("active reference"));
        let mut intended_volume = 0.0;
        let mut retained_volume = 0.0;
        let mut representation_bound = 0.0;
        for (cell, (geometry, value)) in grid.cells().iter().zip(layer_amounts).enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            if value.iter().any(|value| !value.is_finite()) || value[0] <= 0.0 {
                return Err(ClimateProjectionError::InvalidDomain {
                    reason: format!("nonphysical FAS amount at {role:?}[{cell}]: {value:?}"),
                }
                .into());
            }
            let terrain = if role == ClimateLayerRole::LowerAtmosphere {
                floor[cell]
            } else {
                0.0
            };
            let exact_eta = value[0] / geometry.area_m2() - reference + f64::from(terrain);
            let eta = exact_eta as f32;
            let h = LayeredTendencySystem::validated_fluid_layer_thickness_m(
                reference, eta, terrain, role, cell,
            )?;
            let mass = rho * geometry.area_m2() * h;
            let velocity = [
                (value[1] / mass) as f32,
                (value[2] / mass) as f32,
                (value[3] / mass) as f32,
            ];
            if velocity.iter().any(|value| !value.is_finite()) {
                return Err(ClimateProjectionError::InvalidDomain {
                    reason: format!("nonfinite FAS velocity at {role:?}[{cell}]"),
                }
                .into());
            }
            state.height_anomaly_m_mut(role).expect("active height")[cell] = eta;
            state.velocity_m_s_mut(role).expect("active velocity")[cell] =
                operators.tangentize_cell_validated(cell, velocity);
            intended_volume += value[0];
            retained_volume += geometry.area_m2() * h;
            representation_bound += geometry.area_m2()
                * (f64::from(f32::EPSILON) * exact_eta.abs() + f64::from(f32::MIN_POSITIVE));
        }
        let count = grid.cell_count() as f64;
        representation_bound += count * f64::EPSILON / (1.0 - count * f64::EPSILON)
            * (intended_volume.abs() + retained_volume.abs());
        if (retained_volume - intended_volume).abs() > representation_bound {
            return Err(ClimateProjectionError::InvalidDomain {
                reason: format!("FAS amount quantization {role:?}: volume error {} exceeds {representation_bound}",
                    retained_volume - intended_volume),
            }.into());
        }
        if matches!(
            role,
            ClimateLayerRole::LowerAtmosphere | ClimateLayerRole::UpperAtmosphere
        ) {
            atmosphere_volume += retained_volume;
        }
    }
    state
        .validate_against_cancellable(grid, cancellation)
        .map_err(map_state_error)?;
    Ok(atmosphere_volume)
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
    thermocline_current: Option<Vec<[[f32; 3]; CLIMATE_MONTH_COUNT]>>,
    air_temperature: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    upper_air_temperature: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    upper_humidity: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    sea_temperature: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    thermocline_temperature: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    humidity: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    evaporation: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    precipitation: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    orographic_precipitation: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    absorbed_shortwave: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    outgoing_longwave: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    lower_height: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    upper_height: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    sea_height: Vec<[f32; CLIMATE_MONTH_COUNT]>,
    thermocline_height: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
    deep_temperature: Option<Vec<[f32; CLIMATE_MONTH_COUNT]>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct FinalCycleBudget {
    evaporation_global_mean_mm_day: f64,
    precipitation_global_mean_mm_day: f64,
    absorbed_shortwave_global_mean_w_m2: f64,
    outgoing_longwave_global_mean_w_m2: f64,
    planetary_albedo_global_mean: f64,
}

impl FinalCycleBudget {
    fn water_closure_passes(self) -> bool {
        water_cycle_relative_imbalance(
            self.evaporation_global_mean_mm_day,
            self.precipitation_global_mean_mm_day,
        ) <= GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX
    }

    fn radiative_closure_passes(self) -> bool {
        (self.absorbed_shortwave_global_mean_w_m2 - self.outgoing_longwave_global_mean_w_m2).abs()
            <= GLOBAL_CIRCULATION_TOA_NET_ABS_MAX_W_M2
    }

    fn hard_closures_pass(self) -> bool {
        self.water_closure_passes() && self.radiative_closure_passes()
    }
}

fn continuation_needs_moisture_preconditioning(
    profile: ClimateModelProfile,
    state_residual: f64,
    budget: FinalCycleBudget,
    completed_cycles: u16,
    maximum_cycles: u16,
) -> bool {
    let background_ready = match profile {
        ClimateModelProfile::C1SingleLayerV1 => {
            state_residual <= GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX
        }
        ClimateModelProfile::C2LayeredV1 => {
            state_residual <= FORMATION_RESIDUAL_TARGET
                && (budget.radiative_closure_passes() || completed_cycles >= maximum_cycles / 2)
        }
    };
    completed_cycles < maximum_cycles && background_ready && !budget.hard_closures_pass()
}

fn moisture_root_residual(budget: FinalCycleBudget) -> f64 {
    let scale = budget
        .evaporation_global_mean_mm_day
        .max(budget.precipitation_global_mean_mm_day);
    if scale == 0.0 {
        return 0.0;
    }
    (budget.evaporation_global_mean_mm_day - budget.precipitation_global_mean_mm_day) / scale
}

fn periodic_moisture_root_target() -> f64 {
    0.0
}

/// Chooses only a moisture initial guess for the next coupled cycle.
///
/// Candidate budgets replay the exact production thermodynamic/moisture
/// tendency over the latest monthly velocity path. The approximation is
/// never published: the normal split-explicit cycle must independently pass
/// the state, water, and TOA gates. Ridders' bracket-preserving method (1979),
/// DOI `10.1109/TCS.1979.1084580`, projects toward exact periodic balance
/// (`E - P = 0`). Any probe already inside the public feasible interval is
/// accepted; the following full coupled cycle remains the authoritative
/// verification.
/// The number of probes is bounded by the precision of the `f32` state it
/// scales, rather than an independently tuned iteration count. A still-open
/// budget may invoke the same bounded correction again within the profile's
/// existing formation-cycle horizon.
fn precondition_periodic_moisture(
    grid: &CubedSphereGrid,
    tendency_system: &LayeredTendencySystem<'_>,
    forcing: &crate::world::natural::PlanetForcing,
    ocean_edge_permeability: &[f32],
    monthly_background: &WorkClimatology,
    state: &mut LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<(), GlobalCirculationGenerationError> {
    let base = state
        .clone_cancellable(cancellation)
        .map_err(map_state_error)?;
    let base_budget = probe_thermodynamic_water_cycle(
        grid,
        tendency_system,
        forcing,
        ocean_edge_permeability,
        monthly_background,
        &base,
        cancellation,
    )?;
    if base_budget.water_closure_passes() {
        return Ok(());
    }
    let base_residual = moisture_root_residual(base_budget);
    let wet = base_residual > 0.0;
    let mut boundary = base
        .clone_cancellable(cancellation)
        .map_err(map_state_error)?;
    set_moisture_boundary(&mut boundary, wet);
    let (initial_boundary_scale, initial_midpoint_scale) = initial_moisture_probe_scales(wet);
    let mut boundary_probe = evaluate_scaled_moisture_probe(
        grid,
        tendency_system,
        forcing,
        ocean_edge_permeability,
        monthly_background,
        &base,
        &boundary,
        initial_boundary_scale,
        cancellation,
    )?;
    let midpoint_probe = evaluate_scaled_moisture_probe(
        grid,
        tendency_system,
        forcing,
        ocean_edge_permeability,
        monthly_background,
        &base,
        &boundary,
        initial_midpoint_scale,
        cancellation,
    )?;
    let target_residual = periodic_moisture_root_target();
    let objective = |budget| moisture_root_residual(budget) - target_residual;
    let base_difference = objective(base_budget);
    let midpoint_residual = moisture_root_residual(midpoint_probe.budget);
    if midpoint_residual.abs() <= GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX {
        *state = midpoint_probe.initial;
        return Ok(());
    }
    let boundary_residual = moisture_root_residual(boundary_probe.budget);
    if boundary_residual.abs() <= GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX {
        *state = boundary_probe.initial;
        return Ok(());
    }
    let mut boundary_scale = boundary_probe.scale;
    let mut boundary_difference = objective(boundary_probe.budget);
    let mut cached_midpoint = None;
    if boundary_difference.signum() != base_difference.signum() {
        cached_midpoint = Some(midpoint_probe);
    } else {
        for _ in 1..f32::MANTISSA_DIGITS {
            check_cancelled(cancellation)?;
            boundary_scale = if wet {
                2.0 * boundary_scale
            } else {
                0.5 * boundary_scale
            };
            boundary_probe = evaluate_scaled_moisture_probe(
                grid,
                tendency_system,
                forcing,
                ocean_edge_permeability,
                monthly_background,
                &base,
                &boundary,
                boundary_scale,
                cancellation,
            )?;
            let residual = moisture_root_residual(boundary_probe.budget);
            if residual.abs() <= GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX {
                *state = boundary_probe.initial;
                return Ok(());
            }
            boundary_difference = objective(boundary_probe.budget);
            if boundary_difference.signum() != base_difference.signum() {
                break;
            }
        }
    }
    let boundary_difference =
        (boundary_difference.signum() != base_difference.signum()).then_some(boundary_difference);
    let Some(boundary_difference) = boundary_difference else {
        return Ok(());
    };

    let (mut left_scale, mut left_difference, mut right_scale, mut right_difference) =
        if boundary_scale < 1.0 {
            (boundary_scale, boundary_difference, 1.0, base_difference)
        } else {
            (1.0, base_difference, boundary_scale, boundary_difference)
        };
    let mut best_scale = if base_difference.abs() <= boundary_difference.abs() {
        1.0
    } else {
        boundary_scale
    };
    let mut best_difference = base_difference.abs().min(boundary_difference.abs());
    for _ in 0..f32::MANTISSA_DIGITS {
        check_cancelled(cancellation)?;
        let midpoint_scale = 0.5 * (left_scale + right_scale);
        let midpoint = if let Some(probe) = cached_midpoint.take() {
            debug_assert_eq!(probe.scale.to_bits(), midpoint_scale.to_bits());
            probe
        } else {
            evaluate_scaled_moisture_probe(
                grid,
                tendency_system,
                forcing,
                ocean_edge_permeability,
                monthly_background,
                &base,
                &boundary,
                midpoint_scale,
                cancellation,
            )?
        };
        let midpoint_budget = midpoint.budget;
        let midpoint_residual = moisture_root_residual(midpoint_budget);
        let midpoint_difference = objective(midpoint_budget);
        if midpoint_difference.abs() < best_difference {
            best_difference = midpoint_difference.abs();
            best_scale = midpoint_scale;
        }
        if midpoint_residual.abs() <= GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX {
            *state = midpoint.initial;
            return Ok(());
        }
        let Some(scale) = ridders_candidate(
            left_scale,
            left_difference,
            midpoint_scale,
            midpoint_difference,
            right_scale,
            right_difference,
        ) else {
            if midpoint_difference.signum() == left_difference.signum() {
                left_scale = midpoint_scale;
                left_difference = midpoint_difference;
            } else {
                right_scale = midpoint_scale;
                right_difference = midpoint_difference;
            }
            continue;
        };
        let candidate = evaluate_scaled_moisture_probe(
            grid,
            tendency_system,
            forcing,
            ocean_edge_permeability,
            monthly_background,
            &base,
            &boundary,
            scale,
            cancellation,
        )?;
        let budget = candidate.budget;
        let residual = moisture_root_residual(budget);
        let difference = objective(budget);
        if difference.abs() < best_difference {
            best_difference = difference.abs();
            best_scale = scale;
        }
        if residual.abs() <= GLOBAL_CIRCULATION_WATER_CYCLE_RELATIVE_IMBALANCE_MAX {
            *state = candidate.initial;
            return Ok(());
        }
        if midpoint_difference.signum() != difference.signum() {
            if midpoint_scale < scale {
                left_scale = midpoint_scale;
                left_difference = midpoint_difference;
                right_scale = scale;
                right_difference = difference;
            } else {
                left_scale = scale;
                left_difference = difference;
                right_scale = midpoint_scale;
                right_difference = midpoint_difference;
            }
        } else if left_difference.signum() != difference.signum() {
            right_scale = scale;
            right_difference = difference;
        } else {
            left_scale = scale;
            left_difference = difference;
        }
    }
    scale_moisture(&base, &boundary, best_scale, state);
    Ok(())
}

struct MoistureProbe {
    scale: f64,
    initial: LayeredClimateState,
    budget: FinalCycleBudget,
}

const fn initial_moisture_probe_scales(wet: bool) -> (f64, f64) {
    let boundary = if wet { 2.0 } else { 0.5 };
    (boundary, 0.5 * (1.0 + boundary))
}

#[allow(clippy::too_many_arguments)]
fn evaluate_scaled_moisture_probe(
    grid: &CubedSphereGrid,
    tendency_system: &LayeredTendencySystem<'_>,
    forcing: &crate::world::natural::PlanetForcing,
    ocean_edge_permeability: &[f32],
    monthly_background: &WorkClimatology,
    base: &LayeredClimateState,
    boundary: &LayeredClimateState,
    scale: f64,
    cancellation: &BuildCancellation,
) -> Result<MoistureProbe, GlobalCirculationGenerationError> {
    let mut initial = base
        .clone_cancellable(cancellation)
        .map_err(map_state_error)?;
    scale_moisture(base, boundary, scale, &mut initial);
    let budget = probe_thermodynamic_water_cycle(
        grid,
        tendency_system,
        forcing,
        ocean_edge_permeability,
        monthly_background,
        &initial,
        cancellation,
    )?;
    Ok(MoistureProbe {
        scale,
        initial,
        budget,
    })
}

/// Ridders' (1979) bracket-preserving exponential interpolation step.
///
/// DOI `10.1109/TCS.1979.1084580`. Returning `None` leaves the caller with
/// the already-evaluated bisection midpoint, so numerical degeneracy never
/// weakens the enclosing sign bracket or adds an ad-hoc tolerance.
fn ridders_candidate(
    left: f64,
    left_value: f64,
    midpoint: f64,
    midpoint_value: f64,
    right: f64,
    right_value: f64,
) -> Option<f64> {
    let radicand = midpoint_value.mul_add(midpoint_value, -left_value * right_value);
    if !radicand.is_finite() || radicand <= 0.0 {
        return None;
    }
    let candidate = midpoint
        + (midpoint - left) * (left_value - right_value).signum() * midpoint_value
            / radicand.sqrt();
    (candidate.is_finite() && candidate > left && candidate < right).then_some(candidate)
}

fn set_moisture_boundary(state: &mut LayeredClimateState, wet: bool) {
    if wet {
        let lower_temperature = state
            .temperature_c(ClimateLayerRole::LowerAtmosphere)
            .expect("lower atmosphere")
            .to_vec();
        for (humidity, temperature) in state
            .specific_humidity_mut()
            .iter_mut()
            .zip(lower_temperature)
        {
            *humidity = (*humidity).max(crate::world::natural::saturation_specific_humidity_kg_kg(
                f64::from(temperature),
            ) as f32);
        }
        if let Some(upper_temperature) = state
            .temperature_c(ClimateLayerRole::UpperAtmosphere)
            .map(<[f32]>::to_vec)
        {
            for (humidity, temperature) in state
                .upper_specific_humidity_mut()
                .expect("C2 upper humidity")
                .iter_mut()
                .zip(upper_temperature)
            {
                *humidity =
                    (*humidity).max(crate::world::natural::saturation_specific_humidity_kg_kg(
                        f64::from(temperature),
                    ) as f32);
            }
        }
    } else {
        state.specific_humidity_mut().fill(0.0);
        if let Some(upper) = state.upper_specific_humidity_mut() {
            upper.fill(0.0);
        }
    }
}

fn scale_moisture(
    base: &LayeredClimateState,
    boundary: &LayeredClimateState,
    scale: f64,
    target: &mut LayeredClimateState,
) {
    for (target, (base, boundary)) in target.specific_humidity_mut().iter_mut().zip(
        base.specific_humidity()
            .iter()
            .zip(boundary.specific_humidity()),
    ) {
        *target = scale_moisture_value(*base, *boundary, scale);
    }
    if let (Some(target), Some(base), Some(boundary)) = (
        target.upper_specific_humidity_mut(),
        base.upper_specific_humidity(),
        boundary.upper_specific_humidity(),
    ) {
        for (target, (base, boundary)) in target.iter_mut().zip(base.iter().zip(boundary)) {
            *target = scale_moisture_value(*base, *boundary, scale);
        }
    }
}

/// Parameterizes the scalar water-inventory search without changing its
/// normalized spatial tracer pattern before a cell reaches saturation.
///
/// Drying and wetting apply one common multiplier and wet cells cap only at
/// the physical saturation boundary. This is the unique one-parameter
/// homotopy with those two invariants; unlike additive
/// interpolation toward saturation, it cannot inject most of the trial water
/// into an already-hot, initially dry mountain cell merely because that
/// cell's saturation value is large.
fn scale_moisture_value(base: f32, boundary: f32, scale: f64) -> f32 {
    debug_assert!(scale.is_finite() && scale >= 0.0);
    let scaled = f64::from(base) * scale;
    if scale <= 1.0 {
        scaled as f32
    } else {
        scaled.min(f64::from(boundary)) as f32
    }
}

fn probe_thermodynamic_water_cycle(
    grid: &CubedSphereGrid,
    tendency_system: &LayeredTendencySystem<'_>,
    forcing: &crate::world::natural::PlanetForcing,
    ocean_edge_permeability: &[f32],
    monthly_background: &WorkClimatology,
    initial: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<FinalCycleBudget, GlobalCirculationGenerationError> {
    let mut state = initial
        .clone_cancellable(cancellation)
        .map_err(map_state_error)?;
    let mut evaluation_state = state
        .clone_cancellable(cancellation)
        .map_err(map_state_error)?;
    let mut tendency_workspace = LayeredTendencyWorkspace::for_grid(grid);
    let total_area = grid.cells().iter().map(|cell| cell.area_m2()).sum::<f64>();
    let mut evaporation = 0.0_f64;
    let mut precipitation = 0.0_f64;
    let (slow_step_count, slow_step_seconds) =
        SplitExplicitRk3Integrator::slow_step_plan(GLOBAL_CIRCULATION_MACRO_STEP_SECONDS);
    let phase_weight = slow_step_seconds / GLOBAL_CIRCULATION_MACRO_STEP_SECONDS;
    for month in 0..CLIMATE_MONTH_COUNT {
        check_cancelled(cancellation)?;
        let background_month = (month + CLIMATE_MONTH_COUNT - 1) % CLIMATE_MONTH_COUNT;
        for _ in 0..slow_step_count {
            evaluation_state
                .specific_humidity_mut()
                .copy_from_slice(state.specific_humidity());
            if let (Some(target), Some(source)) = (
                evaluation_state.upper_specific_humidity_mut(),
                state.upper_specific_humidity(),
            ) {
                target.copy_from_slice(source);
            }
            for role in state.active_roles() {
                evaluation_state
                    .temperature_c_mut(*role)
                    .expect("active temperature role")
                    .copy_from_slice(state.temperature_c(*role).expect("active temperature role"));
            }
            if let (Some(target), Some(source)) = (
                evaluation_state.deep_ocean_temperature_c_mut(),
                state.deep_ocean_temperature_c(),
            ) {
                target.copy_from_slice(source);
            }
            for cell in 0..grid.cell_count() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                evaluation_state
                    .velocity_m_s_mut(ClimateLayerRole::LowerAtmosphere)
                    .expect("lower atmosphere")[cell] =
                    monthly_background.lower_wind[cell][background_month];
                evaluation_state
                    .velocity_m_s_mut(ClimateLayerRole::OceanMixedLayer)
                    .expect("mixed layer")[cell] =
                    monthly_background.ocean_current[cell][background_month];
                if let (Some(target), Some(source)) = (
                    evaluation_state.velocity_m_s_mut(ClimateLayerRole::UpperAtmosphere),
                    monthly_background.upper_wind.as_ref(),
                ) {
                    target[cell] = source[cell][background_month];
                }
                if let (Some(target), Some(source)) = (
                    evaluation_state.velocity_m_s_mut(ClimateLayerRole::OceanThermocline),
                    monthly_background.thermocline_current.as_ref(),
                ) {
                    target[cell] = source[cell][background_month];
                }
                evaluation_state
                    .height_anomaly_m_mut(ClimateLayerRole::LowerAtmosphere)
                    .expect("lower atmosphere")[cell] =
                    monthly_background.lower_height[cell][background_month];
                evaluation_state
                    .height_anomaly_m_mut(ClimateLayerRole::OceanMixedLayer)
                    .expect("mixed layer")[cell] =
                    monthly_background.sea_height[cell][background_month];
                if let (Some(target), Some(source)) = (
                    evaluation_state.height_anomaly_m_mut(ClimateLayerRole::UpperAtmosphere),
                    monthly_background.upper_height.as_ref(),
                ) {
                    target[cell] = source[cell][background_month];
                }
                if let (Some(target), Some(source)) = (
                    evaluation_state.height_anomaly_m_mut(ClimateLayerRole::OceanThermocline),
                    monthly_background.thermocline_height.as_ref(),
                ) {
                    target[cell] = source[cell][background_month];
                }
            }
            let tendency = tendency_system
                .evaluate_thermodynamic_moisture_with_workspace_for_step(
                    &evaluation_state,
                    forcing,
                    ocean_edge_permeability,
                    month,
                    slow_step_seconds,
                    cancellation,
                    &mut tendency_workspace,
                )?;
            for (cell, record) in grid.cells().iter().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                evaporation += phase_weight
                    * record.area_m2()
                    * f64::from(tendency.evaporation_rate_mm_s()[cell]);
                precipitation += phase_weight
                    * record.area_m2()
                    * f64::from(tendency.precipitation_rate_mm_s()[cell]);
            }
            for (humidity, rate) in state
                .specific_humidity_mut()
                .iter_mut()
                .zip(tendency.specific_humidity_tendency_s_inv())
            {
                *humidity =
                    (f64::from(*humidity) + slow_step_seconds * f64::from(*rate)).max(0.0) as f32;
            }
            if let (Some(humidity), Some(rate)) = (
                state.upper_specific_humidity_mut(),
                tendency.upper_specific_humidity_tendency_s_inv(),
            ) {
                for (humidity, rate) in humidity.iter_mut().zip(rate) {
                    *humidity = (f64::from(*humidity) + slow_step_seconds * f64::from(*rate))
                        .max(0.0) as f32;
                }
            }
            for role in state.active_roles().to_vec() {
                for (temperature, rate) in state
                    .temperature_c_mut(role)
                    .expect("active temperature role")
                    .iter_mut()
                    .zip(
                        tendency
                            .temperature_tendency_k_s(role)
                            .expect("active temperature tendency"),
                    )
                {
                    *temperature =
                        (f64::from(*temperature) + slow_step_seconds * f64::from(*rate)) as f32;
                }
            }
            if let (Some(temperature), Some(rate)) = (
                state.deep_ocean_temperature_c_mut(),
                tendency.deep_ocean_temperature_tendency_k_s(),
            ) {
                for (temperature, rate) in temperature.iter_mut().zip(rate) {
                    *temperature =
                        (f64::from(*temperature) + slow_step_seconds * f64::from(*rate)) as f32;
                }
            }
            state
                .validate_against_cancellable(grid, cancellation)
                .map_err(map_state_error)?;
        }
    }
    let weight = total_area * CLIMATE_MONTH_COUNT as f64;
    Ok(FinalCycleBudget {
        evaporation_global_mean_mm_day: evaporation * 86_400.0 / weight,
        precipitation_global_mean_mm_day: precipitation * 86_400.0 / weight,
        ..FinalCycleBudget::default()
    })
}

impl WorkClimatology {
    fn new(count: usize, profile: ClimateModelProfile) -> Self {
        let c2 = profile == ClimateModelProfile::C2LayeredV1;
        Self {
            lower_wind: vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count],
            upper_wind: c2.then(|| vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count]),
            ocean_current: vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count],
            thermocline_current: c2.then(|| vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count]),
            air_temperature: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            upper_air_temperature: c2.then(|| vec![[0.0; CLIMATE_MONTH_COUNT]; count]),
            upper_humidity: c2.then(|| vec![[0.0; CLIMATE_MONTH_COUNT]; count]),
            sea_temperature: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            thermocline_temperature: c2.then(|| vec![[0.0; CLIMATE_MONTH_COUNT]; count]),
            humidity: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            evaporation: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            precipitation: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            orographic_precipitation: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            absorbed_shortwave: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            outgoing_longwave: vec![[0.0; CLIMATE_MONTH_COUNT]; count],
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
        forcing: &GlobalClimateForcing,
        phase_substep: (usize, u32),
        cancellation: &BuildCancellation,
    ) -> Result<(), GlobalCirculationGenerationError> {
        let (month, slow_step) = phase_substep;
        if let Some(target) = &mut self.upper_air_temperature {
            copy_scalar_month(
                target,
                state
                    .temperature_c(ClimateLayerRole::UpperAtmosphere)
                    .expect("C2 upper temperature"),
                month,
                cancellation,
            )?;
        }
        if let Some(target) = &mut self.upper_humidity {
            copy_scalar_month(
                target,
                state.upper_specific_humidity().expect("C2 upper humidity"),
                month,
                cancellation,
            )?;
        }
        // Equal-duration steps come from the integrator's shared plan. Reset
        // on the first sample of each new cycle; retain month-end state fields
        // while averaging fluxes over the complete forcing phase.
        let mean_flux =
            |previous: f32, sample: f64| mean_monthly_substep_flux(previous, sample, slow_step);
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
        if let Some(target) = &mut self.thermocline_current {
            copy_vector_month(
                target,
                state
                    .velocity_m_s(ClimateLayerRole::OceanThermocline)
                    .expect("C2 thermocline"),
                month,
                cancellation,
            )?;
        }
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
            .evaporation
            .iter_mut()
            .zip(tendency.evaporation_rate_mm_s())
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            target[month] = mean_flux(target[month], f64::from(*rate) * 86_400.0);
        }
        for (cell, (target, rate)) in self
            .precipitation
            .iter_mut()
            .zip(tendency.precipitation_rate_mm_s())
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            target[month] = mean_flux(target[month], f64::from(*rate) * 86_400.0);
        }
        for cell in 0..self.absorbed_shortwave.len() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let absorbed_shortwave =
                f64::from(forcing.planet_forcing().monthly_absorbed_shortwave_w_m2()[cell][month]);
            let outgoing_longwave =
                absorbed_shortwave - tendency.external_radiative_heat_flux_w_m2()[cell];
            if !absorbed_shortwave.is_finite()
                || absorbed_shortwave < 0.0
                || !outgoing_longwave.is_finite()
                || outgoing_longwave < 0.0
            {
                return Err(GlobalCirculationGenerationError::InvalidRadiativeFlux {
                    cell,
                    month,
                    absorbed_shortwave,
                    outgoing_longwave,
                });
            }
            self.absorbed_shortwave[cell][month] =
                mean_flux(self.absorbed_shortwave[cell][month], absorbed_shortwave);
            self.outgoing_longwave[cell][month] =
                mean_flux(self.outgoing_longwave[cell][month], outgoing_longwave);
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
            target[month] = mean_flux(target[month], f64::from(*rate) * 86_400.0);
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

    fn final_cycle_budget(
        &self,
        grid: &CubedSphereGrid,
        forcing: &GlobalClimateForcing,
        cancellation: &BuildCancellation,
    ) -> Result<FinalCycleBudget, GlobalCirculationGenerationError> {
        let mut evaporation = 0.0_f64;
        let mut precipitation = 0.0_f64;
        let mut absorbed_shortwave = 0.0_f64;
        let mut outgoing_longwave = 0.0_f64;
        let mut incoming_shortwave = 0.0_f64;
        let mut reflected_shortwave = 0.0_f64;
        let mut area_months = 0.0_f64;
        for (cell, record) in grid.cells().iter().enumerate() {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let area = record.area_m2();
            let planetary_albedo = crate::world::natural::planetary_albedo_from_surface(f64::from(
                forcing.planet_forcing().surface_albedo()[cell],
            ));
            for month in 0..CLIMATE_MONTH_COUNT {
                evaporation += area * f64::from(self.evaporation[cell][month]);
                precipitation += area * f64::from(self.precipitation[cell][month]);
                absorbed_shortwave += area * f64::from(self.absorbed_shortwave[cell][month]);
                outgoing_longwave += area * f64::from(self.outgoing_longwave[cell][month]);
                let incoming = EARTH_NOMINAL_TOTAL_SOLAR_IRRADIANCE_W_M2
                    * f64::from(forcing.monthly_insolation_fraction()[cell][month]);
                incoming_shortwave += area * incoming;
                reflected_shortwave += area * incoming * planetary_albedo;
                area_months += area;
            }
        }
        check_cancelled(cancellation)?;
        if !area_months.is_finite() || area_months <= 0.0 || incoming_shortwave <= 0.0 {
            return Err(GlobalCirculationGenerationError::InvalidFinalCycleBudget);
        }
        Ok(FinalCycleBudget {
            evaporation_global_mean_mm_day: evaporation / area_months,
            precipitation_global_mean_mm_day: precipitation / area_months,
            absorbed_shortwave_global_mean_w_m2: absorbed_shortwave / area_months,
            outgoing_longwave_global_mean_w_m2: outgoing_longwave / area_months,
            planetary_albedo_global_mean: reflected_shortwave / incoming_shortwave,
        })
    }

    fn project(
        mut self,
        surface: &SphericalSurfaceSnapshot,
        domain: &ClimateWorkDomainSnapshot,
        forcing: &GlobalClimateForcing,
        cancellation: &BuildCancellation,
        observer: &mut impl FnMut(GlobalCirculationPhase),
    ) -> Result<(GlobalCirculationFields, f64), GlobalCirculationGenerationError> {
        if let Some(upper_wind) = &self.upper_wind {
            let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
            let reference_depths = [
                ClimateLayerRole::LowerAtmosphere,
                ClimateLayerRole::UpperAtmosphere,
            ]
            .map(|role| {
                layout
                    .layers()
                    .iter()
                    .find(|layer| layer.role() == role)
                    .expect("C2 atmosphere is declared")
                    .reference_thickness_m()
            });
            let terrain_floor = LayeredTendencySystem::lower_atmosphere_terrain_floor_m(
                forcing.relative_elevation_m(),
                forcing.planet_forcing().land_fraction(),
            );
            let upper_height = self.upper_height.as_ref().expect("C2 upper height");
            // Monthly backgrounds retain layer means for solver replay. Only
            // the consumed publication buffer becomes a surface diagnostic;
            // reconstruct on the work grid before horizontal remapping.
            for (cell, lower_months) in self.lower_wind.iter_mut().enumerate() {
                if cell % 256 == 0 {
                    check_cancelled(cancellation)?;
                }
                for (month, lower) in lower_months.iter_mut().enumerate() {
                    let lower_depth = LayeredTendencySystem::validated_fluid_layer_thickness_m(
                        reference_depths[0],
                        self.lower_height[cell][month],
                        terrain_floor[cell],
                        ClimateLayerRole::LowerAtmosphere,
                        cell,
                    )?;
                    let upper_depth = LayeredTendencySystem::validated_fluid_layer_thickness_m(
                        reference_depths[1],
                        upper_height[cell][month],
                        0.0,
                        ClimateLayerRole::UpperAtmosphere,
                        cell,
                    )?;
                    *lower = reconstruct_atmosphere_surface_wind_m_s(
                        *lower,
                        upper_wind[cell][month],
                        atmosphere_surface_wind_weights(lower_depth, upper_depth),
                    )
                    .map(|value| value as f32);
                }
            }
        }
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
        // The ocean current is defined on water only. Conservative remapping
        // from the coarse work grid spreads a wet work cell's velocity across
        // every source cell it covers, including the dry ones, so publication
        // masks the field with the released land/ocean classification rather
        // than with the sub-cell land area fraction: a cell can sit above sea
        // level, and therefore publish as land, while still holding some water
        // area (design 2026-09-03 A4 Task 7).
        for (cell, (vectors, &publishes_land)) in ocean_current
            .iter_mut()
            .zip(forcing.source_publishes_land())
            .enumerate()
        {
            if cell % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            if publishes_land == 1.0 {
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
        let evaporation =
            project_monthly_extensive_rate_cancellable(domain, &self.evaporation, cancellation)?;
        let precipitation =
            project_monthly_extensive_rate_cancellable(domain, &self.precipitation, cancellation)?;
        let orographic_precipitation = project_monthly_extensive_rate_cancellable(
            domain,
            &self.orographic_precipitation,
            cancellation,
        )?;
        let surface_albedo = project_intensive_scalar_cancellable(
            domain,
            forcing.planet_forcing().surface_albedo(),
            cancellation,
        )?;
        let absorbed_shortwave = project_monthly_extensive_rate_cancellable(
            domain,
            &self.absorbed_shortwave,
            cancellation,
        )?;
        let outgoing_longwave = project_monthly_extensive_rate_cancellable(
            domain,
            &self.outgoing_longwave,
            cancellation,
        )?;
        let precipitation_relative_error = precipitation
            .max_relative_conservation_error()
            .max(evaporation.max_relative_conservation_error())
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
                surface_albedo,
                MonthlyScalarField::from_values_cancellable(
                    absorbed_shortwave.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    outgoing_longwave.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    thermocline_temperature.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(thermocline_depth, &cancelled)?,
                MonthlyScalarField::from_values_cancellable(humidity.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(evaporation.into_values(), &cancelled)?,
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
                surface_albedo,
                MonthlyScalarField::from_values_cancellable(
                    absorbed_shortwave.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(
                    outgoing_longwave.into_values(),
                    &cancelled,
                )?,
                MonthlyScalarField::from_values_cancellable(humidity.into_values(), &cancelled)?,
                MonthlyScalarField::from_values_cancellable(evaporation.into_values(), &cancelled)?,
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

fn mean_monthly_substep_flux(previous: f32, sample: f64, slow_step: u32) -> f32 {
    ((f64::from(previous) * f64::from(slow_step) + sample) / f64::from(slow_step + 1)) as f32
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
    external_heat_scale: f64,
    external_moisture_scale: f64,
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
            external_heat_scale: 0.0,
            external_moisture_scale: 0.0,
        })
    }

    fn record(
        &mut self,
        grid: &CubedSphereGrid,
        layout: &ClimateLayerLayout,
        before: &LayeredClimateState,
        step_result: &ClimateStepResult,
        tendency: &super::LayeredClimateTendency,
        dt: f64,
        cancellation: &BuildCancellation,
    ) -> Result<(), GlobalCirculationGenerationError> {
        let after = step_result.state();
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
        let energy_expected =
            tendency_budget.external_heat_rate_w() * dt + step_result.ocean_mass_source_heat_j();
        self.energy_residual +=
            energy_change_total(grid, layout, before, after, cancellation)? - energy_expected;
        self.paired_heat_residual += tendency_budget.paired_heat_residual_w().abs();
        self.paired_momentum_residual += tendency_budget.paired_momentum_residual_n().abs();
        self.paired_moisture_residual += tendency_budget.paired_moisture_residual_kg_s().abs();
        self.paired_heat_scale += tendency_budget.paired_heat_absolute_w();
        self.external_heat_scale += tendency_budget.external_heat_absolute_w();
        self.paired_momentum_scale += tendency_budget.paired_momentum_absolute_n();
        self.paired_moisture_scale += tendency_budget.paired_moisture_absolute_kg_s();
        self.external_moisture_scale += tendency_budget.external_moisture_source_rate_kg_s().abs()
            + tendency_budget
                .external_precipitation_sink_rate_kg_s()
                .abs();
        check_cancelled(cancellation)?;
        Ok(())
    }

    fn finish(
        self,
        final_cycle: FinalCycleBudget,
    ) -> Result<ClimateBudgetReport, ClimateReportError> {
        let component_pair_errors = [
            // Milestone A4 (§6.4): the paired heat residual is `f32`
            // quantization of increments added to a lattice that also carries
            // the external radiative term, so its floor is set by that term,
            // not by the exchange. Measuring it against the exchange alone is
            // ill posed: as the layers approach mutual equilibrium the
            // exchange legitimately goes to zero while the quantization floor
            // does not. It is measured against all the heat the same lattice
            // carries.
            relative_exchange_error(
                self.paired_heat_residual,
                self.paired_heat_scale + self.external_heat_scale,
            ),
            relative_exchange_error(self.paired_momentum_residual, self.paired_momentum_scale),
            // Same well-posedness argument as the heat component above
            // (design 2026-09-03 A4 §6.4): the residual is `f32` quantization
            // of a lattice that also carries evaporation and precipitation,
            // and the inter-layer moisture exchange legitimately shrinks as
            // the layers equilibrate, so it is measured against all the water
            // the lattice carries.
            relative_exchange_error(
                self.paired_moisture_residual,
                self.paired_moisture_scale + self.external_moisture_scale,
            ),
        ];
        let paired_relative_error = component_pair_errors.into_iter().fold(0.0_f64, f64::max);
        ClimateBudgetReport::new_with_climatology(
            self.atmosphere_residual.abs() / self.atmosphere_scale,
            self.ocean_residual.abs() / self.ocean_scale,
            self.moisture_residual.abs() / self.moisture_scale,
            self.energy_residual.abs() / self.energy_scale,
            paired_relative_error,
            final_cycle.evaporation_global_mean_mm_day,
            final_cycle.precipitation_global_mean_mm_day,
            final_cycle.absorbed_shortwave_global_mean_w_m2,
            final_cycle.outgoing_longwave_global_mean_w_m2,
            final_cycle.planetary_albedo_global_mean,
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

/// Water mass, so every conversion below uses the moisture column mass rather
/// than the dry-air one (design 2026-09-03 A5 Task 1).
fn moisture_total(
    grid: &CubedSphereGrid,
    state: &LayeredClimateState,
    cancellation: &BuildCancellation,
) -> Result<f64, GlobalCirculationGenerationError> {
    let layout = ClimateLayerLayout::for_profile(state.profile());
    let lower_mass = layout.moisture_column_mass_per_area(ClimateLayerRole::LowerAtmosphere);
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
        let upper_mass = layout.moisture_column_mass_per_area(ClimateLayerRole::UpperAtmosphere);
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
    let layout = ClimateLayerLayout::for_profile(before.profile());
    let lower_mass = layout.moisture_column_mass_per_area(ClimateLayerRole::LowerAtmosphere);
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
        let upper_mass = layout.moisture_column_mass_per_area(ClimateLayerRole::UpperAtmosphere);
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
        let layer = layer_spec(layout, *role).expect("active energy layer");
        for (index, (cell, value)) in grid
            .cells()
            .iter()
            .zip(state.temperature_c(*role).expect("active role"))
            .enumerate()
        {
            if index % 256 == 0 {
                check_cancelled(cancellation)?;
            }
            let capacity = super::tendency::cell_heat_capacity_per_area(state, layer, index)?;
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
    total += WATER_VAPORIZATION_LATENT_HEAT_J_KG * moisture_total(grid, state, cancellation)?;
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
        let layer = layer_spec(layout, *role).expect("active energy layer");
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
            let before_capacity =
                super::tendency::cell_heat_capacity_per_area(before, layer, index)?;
            let after_capacity = super::tendency::cell_heat_capacity_per_area(after, layer, index)?;
            total += cell.area_m2()
                * (after_capacity * (f64::from(*after_value) - f64::from(*before_value))
                    + (after_capacity - before_capacity) * (f64::from(*before_value) + 273.15));
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
    total += WATER_VAPORIZATION_LATENT_HEAT_J_KG
        * moisture_change_total(grid, before, after, cancellation)?;
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
    #[error(
        "invalid radiative flux at [{cell}][{month}]: ASR {absorbed_shortwave}, OLR {outgoing_longwave} W/m2"
    )]
    InvalidRadiativeFlux {
        cell: usize,
        month: usize,
        absorbed_shortwave: f64,
        outgoing_longwave: f64,
    },
    #[error("final-cycle climate budget has a non-positive integration weight")]
    InvalidFinalCycleBudget,
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
    use crate::world::natural::global_circulation_tendency_cell_bytes;

    #[test]
    fn fas_first_coarse_update_has_the_restricted_fine_image_and_fixed_point() {
        let cancellation = BuildCancellation::new();
        let restricted_fine = vec![vec![[8.0, 1.0, 4.0, 0.0]]];
        let first_coarse = vec![vec![[2.0, 3.0, -1.0, 0.0]]];
        let (tau, mut first_update) =
            fas_first_coarse_update(restricted_fine.clone(), first_coarse, &cancellation).unwrap();
        assert_eq!(tau, vec![vec![[6.0, -2.0, 5.0, 0.0]]]);
        assert_eq!(first_update, restricted_fine);
        // At a fine fixed point R Phi_f(x)=Rx. Its coarse correction is zero,
        // even though the uncorrected coarse map has a different fixed point.
        add_scaled_mechanical_amounts(&mut first_update, -1.0, &restricted_fine, &cancellation)
            .unwrap();
        assert_eq!(first_update, vec![vec![[0.0; 4]]]);
    }

    #[test]
    fn fas_signed_transfer_preserves_actual_volume_and_dissipates_projection_energy() {
        // No time stepping: the new risk is transfer of H/P across different
        // terrain floors, not any already-tested circulation source.
        let cancellation = BuildCancellation::new();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let make = |n, elevation| {
            let grid = CubedSphereGrid::new(n, 6_371_000.0).unwrap();
            let count = grid.cell_count();
            let forcing = PlanetForcing::new(
                *grid.fingerprint(),
                vec![elevation; count],
                vec![1.0; count],
                vec![0.3; count],
                vec![0.0; count],
                vec![[240.0; CLIMATE_MONTH_COUNT]; count],
                vec![[10.0; CLIMATE_MONTH_COUNT]; count],
                vec![[14.0; CLIMATE_MONTH_COUNT]; count],
                vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            )
            .unwrap();
            let state = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
            let floor = LayeredTendencySystem::lower_atmosphere_terrain_floor_m(
                forcing.elevation_m(),
                forcing.land_fraction(),
            );
            (grid, forcing, state, floor)
        };
        let (source_grid, _, mut source, source_floor) = make(2, 500.0);
        let (target_grid, _, mut target, target_floor) = make(4, 200.0);
        let ocean_roles = [
            ClimateLayerRole::OceanMixedLayer,
            ClimateLayerRole::OceanThermocline,
        ];
        for role in ocean_roles {
            for (cell, height) in target
                .height_anomaly_m_mut(role)
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                *height = (cell % 3 + 1) as f32 * 0.125;
            }
            // Signed zero detects an unnecessary velocity rewrite while still
            // satisfying the fixture's full-land zero-ocean-current constraint.
            target
                .velocity_m_s_mut(role)
                .unwrap()
                .fill([-0.0, 0.0, -0.0]);
        }
        let original_ocean_bits = ocean_roles.map(|role| {
            (
                target
                    .height_anomaly_m(role)
                    .unwrap()
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                target
                    .velocity_m_s(role)
                    .unwrap()
                    .iter()
                    .map(|value| value.map(f32::to_bits))
                    .collect::<Vec<_>>(),
            )
        });
        for role in [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ] {
            let reference = f64::from(source.reference_thickness_m(role).unwrap());
            for (cell, geometry) in source_grid.cells().iter().enumerate() {
                let radial = geometry.center_unit();
                // Nonuniform actual H catches remapping u instead of rho H u.
                let depth = reference * (1.0 + 0.125 * radial[2]);
                source.height_anomaly_m_mut(role).unwrap()[cell] = (depth - reference) as f32
                    + if role == ClimateLayerRole::LowerAtmosphere {
                        source_floor[cell]
                    } else {
                        0.0
                    };
                source.velocity_m_s_mut(role).unwrap()[cell] =
                    CirculationOperators::new(&source_grid).tangentize_cell_validated(
                        cell,
                        [(-10.0 * radial[1]) as f32, (10.0 * radial[0]) as f32, 0.0],
                    );
            }
        }
        let original_target_temperature = target
            .temperature_c(ClimateLayerRole::LowerAtmosphere)
            .unwrap()
            .to_vec();
        let map = ConservativeSurfaceMapBuilder::build_cancellable(
            &source_grid.to_surface_snapshot().unwrap(),
            &target_grid.to_surface_snapshot().unwrap(),
            || false,
        )
        .unwrap();
        let source_amounts =
            pack_mechanical_amounts(&source_grid, &source, &source_floor, &cancellation).unwrap();
        let mapped =
            remap_mechanical_amounts(&map, &target_grid, &source_amounts, &cancellation).unwrap();
        let mut negative = source_amounts.clone();
        for layer in &mut negative {
            for cell in layer {
                for component in cell {
                    *component = -*component;
                }
            }
        }
        let negative_mapped =
            remap_mechanical_amounts(&map, &target_grid, &negative, &cancellation).unwrap();
        for (positive, negative) in mapped
            .iter()
            .flatten()
            .zip(negative_mapped.iter().flatten())
        {
            assert_eq!(positive.map(|value| -value), *negative,
                "signed FAS restriction/prolongation must stay linear, including f64 tangent projection");
        }
        let atmosphere_roles = [
            ClimateLayerRole::LowerAtmosphere,
            ClimateLayerRole::UpperAtmosphere,
        ];
        let retained_air_volume = unpack_mechanical_amounts(
            &target_grid,
            (&atmosphere_roles, &mapped[..atmosphere_roles.len()]),
            &mut target,
            &target_floor,
            &cancellation,
        )
        .unwrap();
        for (role, (height_bits, velocity_bits)) in ocean_roles.into_iter().zip(original_ocean_bits)
        {
            assert!(
                target
                    .height_anomaly_m(role)
                    .unwrap()
                    .iter()
                    .map(|value| value.to_bits())
                    .eq(height_bits),
                "atmosphere-only FAS injection must not rewrite ocean height"
            );
            assert!(
                target
                    .velocity_m_s(role)
                    .unwrap()
                    .iter()
                    .map(|value| value.map(f32::to_bits))
                    .eq(velocity_bits),
                "atmosphere-only FAS injection must not rewrite ocean velocity"
            );
        }
        assert!(retained_air_volume > 0.0);
        assert_eq!(
            target
                .temperature_c(ClimateLayerRole::LowerAtmosphere)
                .unwrap(),
            original_target_temperature
        );
        let retained =
            pack_mechanical_amounts(&target_grid, &target, &target_floor, &cancellation).unwrap();
        for (layer, &role) in source.active_roles()[..2].iter().enumerate() {
            let rho = layout
                .layers()
                .iter()
                .find(|spec| spec.role() == role)
                .unwrap()
                .density_kg_m3();
            let kinetic_energy = |values: &[[f64; 4]]| -> f64 {
                values
                    .iter()
                    .map(|value| {
                        value[1..].iter().map(|p| p * p).sum::<f64>() / (2.0 * rho * value[0])
                    })
                    .sum()
            };
            assert!(
                kinetic_energy(&retained[layer]) < kinetic_energy(&source_amounts[layer]),
                "positive mass remap and tangent projection cannot create kinetic energy"
            );
        }
    }

    #[test]
    fn monthly_flux_retains_both_substeps_and_resets_the_previous_cycle() {
        // The monthly ledger must retain the first half-step flux and must
        // not average it with the same month's previous formation cycle.
        let first = mean_monthly_substep_flux(99.0, 2.0, 0);
        let whole_phase = mean_monthly_substep_flux(first, 10.0, 1);
        assert_eq!(whole_phase, 6.0);
        assert_eq!(mean_monthly_substep_flux(whole_phase, 4.0, 0), 4.0);
    }

    #[test]
    fn c2_tendency_owner_formula_includes_f64_momentum_and_all_retained_fields() {
        // Four mixed-precision layer records; lower/upper humidity, deep
        // temperature and five water rates; two external ledgers and Q.
        assert_eq!(
            global_circulation_tendency_cell_bytes(ClimateModelProfile::C2LayeredV1),
            4 * (2 * std::mem::size_of::<f32>() + std::mem::size_of::<[f64; 3]>()) as u64
                + 8 * std::mem::size_of::<f32>() as u64
                + 3 * std::mem::size_of::<f64>() as u64
        );
    }

    #[test]
    fn wet_moisture_homotopy_preserves_the_existing_spatial_pattern_until_saturation() {
        let scale = 2.0;
        let first = scale_moisture_value(0.01, 0.20, scale);
        let second = scale_moisture_value(0.02, 0.40, scale);

        assert_eq!(first.to_bits(), 0.02_f32.to_bits());
        assert_eq!(second.to_bits(), 0.04_f32.to_bits());
        assert_eq!((second / first).to_bits(), 2.0_f32.to_bits());
        assert_eq!(
            scale_moisture_value(0.15, 0.20, scale).to_bits(),
            0.20_f32.to_bits()
        );
    }

    #[test]
    fn dry_moisture_homotopy_removes_the_same_fraction_everywhere() {
        assert_eq!(
            scale_moisture_value(0.08, 0.0, 0.75).to_bits(),
            0.06_f32.to_bits()
        );
    }

    #[test]
    fn an_open_toa_budget_still_preconditions_the_next_water_cycle() {
        let budget = FinalCycleBudget {
            evaporation_global_mean_mm_day: 3.0,
            precipitation_global_mean_mm_day: 3.0,
            absorbed_shortwave_global_mean_w_m2: 240.0,
            outgoing_longwave_global_mean_w_m2: 229.0,
            planetary_albedo_global_mean: 0.3,
        };

        assert!(continuation_needs_moisture_preconditioning(
            ClimateModelProfile::C2LayeredV1,
            FORMATION_RESIDUAL_TARGET,
            budget,
            6,
            8,
        ));
    }

    #[test]
    fn moisture_projection_root_is_exact_periodic_balance() {
        assert_eq!(periodic_moisture_root_target().to_bits(), 0.0_f64.to_bits());
    }

    #[test]
    fn initial_probes_cover_the_geometric_boundary_and_its_midpoint() {
        assert_eq!(initial_moisture_probe_scales(true), (2.0, 1.5));
        assert_eq!(initial_moisture_probe_scales(false), (0.5, 0.75));
    }

    #[test]
    fn ridders_candidate_improves_a_bracketed_nonlinear_root_without_a_tolerance() {
        let candidate = ridders_candidate(1.0, -1.0, 1.5, 0.25, 2.0, 2.0).unwrap();

        assert!(candidate > 1.0 && candidate < 2.0);
        assert!((candidate * candidate - 2.0).abs() < (1.5_f64 * 1.5 - 2.0).abs());
    }

    #[test]
    fn radiative_energy_ledger_matches_the_compressed_thermal_step() {
        // A uniform dry land column isolates radiation without a full world
        // solve or a deep-ocean energy scale that could hide the clock error.
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
        let count = grid.cell_count();
        let forcing = PlanetForcing::new(
            *grid.fingerprint(),
            vec![0.0; count],
            vec![1.0; count],
            vec![0.0; count],
            vec![0.0; count],
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C1SingleLayerV1);
        let mut before = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        before
            .temperature_c_mut(ClimateLayerRole::LowerAtmosphere)
            .unwrap()
            .fill(4.0);
        let cancellation = BuildCancellation::new();
        let step = GLOBAL_CIRCULATION_MACRO_STEP_SECONDS;
        let tendency = LayeredTendencySystem::new(&grid)
            .evaluate_for_step(
                &before,
                &forcing,
                &vec![0.0; grid.edges().len()],
                0,
                step,
                &cancellation,
            )
            .unwrap();
        assert_eq!(tendency.budget().external_moisture_net_rate_kg_s(), 0.0);
        let mut after = before.clone();
        for role in before.active_roles() {
            for (temperature, rate) in after
                .temperature_c_mut(*role)
                .unwrap()
                .iter_mut()
                .zip(tendency.temperature_tendency_k_s(*role).unwrap())
            {
                *temperature = (f64::from(*temperature) + step * f64::from(*rate)) as f32;
            }
        }
        let observed = energy_change_total(&grid, &layout, &before, &after, &cancellation).unwrap();
        let expected = step * tendency.budget().external_heat_rate_w();
        assert!(observed.abs() > 0.0);
        let relative_error = (observed - expected).abs() / observed.abs();
        assert!(
            relative_error <= crate::world::natural::GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX,
            "radiation clock mismatch: observed={observed}, ledger={expected}, error={relative_error}"
        );
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
            vec![[240.0; CLIMATE_MONTH_COUNT]; count],
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
    fn ocean_heat_and_mass_endpoint_close_the_actual_capacity_energy_ledger() {
        // A uniform wet column has no transport. This isolates simultaneous
        // local heating and the existing ocean thickness source; a dry or
        // zero-height fixture would miss the mass source's carried heat.
        let grid = CubedSphereGrid::new(2, 6_371_000.0).unwrap();
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
            vec![[0.0; CLIMATE_MONTH_COUNT]; count],
        )
        .unwrap();
        let layout = ClimateLayerLayout::for_profile(ClimateModelProfile::C2LayeredV1);
        let mut before = LayeredClimateState::from_forcing(&grid, &layout, &forcing, 0).unwrap();
        before
            .height_anomaly_m_mut(ClimateLayerRole::OceanMixedLayer)
            .unwrap()
            .fill(20.0);
        before
            .temperature_c_mut(ClimateLayerRole::OceanMixedLayer)
            .unwrap()
            .fill(35.0);
        let cancellation = BuildCancellation::new();
        let permeability = vec![1.0; grid.edges().len()];
        let step =
            SplitExplicitRk3Integrator::slow_step_plan(GLOBAL_CIRCULATION_MACRO_STEP_SECONDS).1;
        let integrator = SplitExplicitRk3Integrator::new(&grid, 300.0).unwrap();
        let declared = integrator
            .tendency_system()
            .evaluate_for_step(&before, &forcing, &permeability, 0, step, &cancellation)
            .unwrap();
        let result = integrator
            .advance_with_declared_tendency_and_phase_observer(
                &before,
                &forcing,
                &permeability,
                step,
                &declared,
                &cancellation,
                &mut |_| {},
            )
            .unwrap();
        let observed =
            energy_change_total(&grid, &layout, &before, result.state(), &cancellation).unwrap();
        // Isolate the retained local thermal endpoint. The radiation-clock
        // and pair tests already cover its requested heat; this contract
        // checks the extra heat carried by changing H, without treating
        // the temperature's final f32 storage rounding as another source.
        let fast = integrator
            .tendency_system()
            .evaluate_fast(&before, &forcing, &permeability, 0, &cancellation)
            .unwrap();
        let full_derivative =
            super::super::rk3::ClimateDerivative::from_tendency(&before, &declared, &cancellation)
                .unwrap();
        let fast_derivative =
            super::super::rk3::ClimateDerivative::from_tendency(&before, &fast, &cancellation)
                .unwrap();
        let local = full_derivative
            .subtract(&fast_derivative, &cancellation)
            .unwrap();
        let mut thermal_endpoint = before.clone();
        super::super::rk3::apply_scalar_endpoint(
            &before,
            &local,
            step,
            &mut thermal_endpoint,
            &cancellation,
        )
        .unwrap();
        for role in [
            ClimateLayerRole::OceanMixedLayer,
            ClimateLayerRole::OceanThermocline,
        ] {
            thermal_endpoint
                .height_anomaly_m_mut(role)
                .unwrap()
                .copy_from_slice(before.height_anomaly_m(role).unwrap());
        }
        let retained_heat =
            energy_change_total(&grid, &layout, &before, &thermal_endpoint, &cancellation).unwrap();
        let carried = result.ocean_mass_source_heat_j();
        assert!(carried != 0.0);
        let error =
            (observed - retained_heat - carried).abs() / (retained_heat.abs() + carried.abs());
        assert!(
            error <= crate::world::natural::GLOBAL_CIRCULATION_ENERGY_RELATIVE_ERROR_MAX,
            "actual ocean energy does not close: error={error}, observed={observed}, local={retained_heat}, carried={carried}"
        );
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
            vec![[240.0; CLIMATE_MONTH_COUNT]; cell_count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; cell_count],
            vec![[15.0; CLIMATE_MONTH_COUNT]; cell_count],
            // Below the condensation threshold so the state is quiescent and
            // any moisture change is the injected leak (design A5 Task 1).
            vec![[0.006; CLIMATE_MONTH_COUNT]; cell_count],
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
        let leaked = ClimateStepResult::new(
            leaked,
            super::super::ClimateIntegratorDiagnostics::default(),
            vec![0.0; cell_count],
        );
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
            budgets.finish(FinalCycleBudget::default()),
            Err(ClimateReportError::StatisticAboveMaximum {
                field: "moisture_relative_error",
                ..
            })
        ));
    }
}
