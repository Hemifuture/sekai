use std::io::{self, Write};

use serde::Serialize;
use thiserror::Error;

use super::sediment::split_mass_by_weights;
use super::{
    CoastGenerationError, CoastalExchange, CoastalInputs, FormationHydrologyGenerationError,
    FormationHydrologyGenerator, FormationSeaLevelSolver, HillslopeGenerationError,
    HillslopeInputs, HillslopeWorkspace, ImplicitStreamPowerSolver, IsostasyGenerationError,
    LocalAiryIsostasy, NonlinearHillslopeTransport, ProvenanceSedimentRouter,
    SedimentGenerationError, SedimentInputs, StreamPowerGenerationError,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::global_circulation::{
    GlobalCirculationGenerationError, GlobalCirculationGenerator, GlobalClimateForcingBuilder,
    GlobalClimateForcingError,
};
use crate::world::natural::{
    expected_surface_formation_dense_state_bytes, formation_annual_precipitation_mm,
    formation_elevation_from_components, formation_relative_flux_imbalance,
    surface_formation_state_fingerprint, ClimateSpec, ClimateWorkDomainSnapshot,
    EvolvedTectonicSnapshot, FormationElevationComponents, FormationProcessRates,
    FormationResiduals, FormationSedimentFields, FormationSolveReport, FormationTerrainFields,
    GeologicSubstrateSnapshot, GlobalCirculationSnapshot, HydroErosionSpec, NaturalQualityProfile,
    NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot, SedimentBudgetReport,
    SphericalHydrologySnapshot, SurfaceFormationCapabilitySet, SurfaceFormationCheckpoint,
    SurfaceFormationUpstreamFingerprints, SurfaceFormationValidationError, ELEVATION_MAX_M,
    ELEVATION_MIN_M, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    FORMATION_DETACHMENT_LIMITED_EFFECTIVE_SETTLING_VELOCITY_M_PER_YEAR,
    FORMATION_TERRAIN_FIELDS_SCHEMA_V3, NATURAL_SURFACE_FORMATION_SCHEMA_V3,
    SEDIMENT_PROVENANCE_SOURCE_COUNT, SURFACE_FORMATION_CONTINUATION_GROWTH_FACTOR,
    SURFACE_FORMATION_CONTINUATION_STEPS_PER_CLIMATE_SOLVE, SURFACE_FORMATION_MAX_CLIMATE_SOLVES,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef, SurfaceRefError};
use crate::world::CellId;

const CANCELLATION_POLL_MASK: usize = 255;
const FINGERPRINT_POLL_BYTES: usize = 64 * 1024;

/// Complete authoritative input set of one coupled formation solve.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceFormationInputs<'a> {
    pub surface: &'a SphericalSurfaceSnapshot,
    pub quality_profile: NaturalQualityProfile,
    pub tectonics: &'a EvolvedTectonicSnapshot,
    pub substrate: &'a GeologicSubstrateSnapshot,
    pub relief: &'a PrimaryReliefSnapshot,
    pub domain: &'a ClimateWorkDomainSnapshot,
    pub climate_spec: &'a ClimateSpec,
    pub initial_climate: &'a GlobalCirculationSnapshot,
    pub formation_spec: &'a HydroErosionSpec,
}

/// Bounded current-state climate and geomorphic equilibrium solve.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceFormationGenerator;

impl SurfaceFormationGenerator {
    /// Runs the locked P5 solve and publishes one atomic formation product.
    pub fn generate(
        inputs: SurfaceFormationInputs<'_>,
        cancellation: &BuildCancellation,
    ) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError> {
        Self::generate_with_climate_solve_limit(
            inputs,
            SURFACE_FORMATION_MAX_CLIMATE_SOLVES,
            cancellation,
        )
    }

    /// Same solve with a reduced climate-solve budget.
    ///
    /// The budget can only be lowered, never raised past the locked maximum, so
    /// a caller can only make the equilibrium solve fail: a non-converged candidate is
    /// still never published.
    pub fn generate_with_climate_solve_limit(
        inputs: SurfaceFormationInputs<'_>,
        climate_solve_limit: u16,
        cancellation: &BuildCancellation,
    ) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError> {
        if climate_solve_limit == 0 || climate_solve_limit > SURFACE_FORMATION_MAX_CLIMATE_SOLVES {
            return Err(SurfaceFormationGenerationError::InvalidIterationLimit {
                found: climate_solve_limit,
                maximum: SURFACE_FORMATION_MAX_CLIMATE_SOLVES,
            });
        }
        let surface = inputs.surface;
        let surface_ref = validate_inputs(inputs, cancellation)?;
        let upstream = upstream_fingerprints(inputs, cancellation)?;
        let areas = cell_areas(surface, cancellation)?;
        let total_area_m2 = areas.iter().sum::<f64>();
        let dense_state_bytes = expected_surface_formation_dense_state_bytes(
            surface.cells().len() as u32,
            surface.edges().len() as u32,
        )
        .ok_or(SurfaceFormationGenerationError::AllocationOverflow)?;

        let mut climate = inputs.initial_climate.clone();
        let mut state = initial_geomorphic_state(inputs)?;
        let mut workspace = HillslopeWorkspace::default();
        let mut climate_solve_count = 0_u16;
        let mut equilibrium_iterations = 0_u16;
        let mut terminal_residual = None;
        let mut continuation_step_years = f64::INFINITY;

        for _ in 0..climate_solve_limit {
            check_cancelled(cancellation)?;
            let mut solved = solve_geomorphic(
                inputs,
                &mut state,
                &climate,
                &areas,
                total_area_m2,
                &mut continuation_step_years,
                &mut workspace,
                cancellation,
            )?;
            let candidate_climate = {
                let forcing = GlobalClimateForcingBuilder::build_for_formation_terrain(
                    surface,
                    &solved.terrain,
                    inputs.climate_spec,
                    inputs.domain,
                    cancellation,
                )?;
                GlobalCirculationGenerator::generate(
                    surface,
                    inputs.domain,
                    &forcing,
                    inputs.initial_climate.profile(),
                    cancellation,
                )?
            };
            let candidate_hydrology = FormationHydrologyGenerator::generate_from_validated(
                surface,
                &solved.terrain,
                inputs.substrate,
                &candidate_climate,
                inputs.formation_spec,
                cancellation,
            )?;
            let current_processes = evaluate_current_processes(
                inputs,
                &state,
                &candidate_climate,
                &mut workspace,
                cancellation,
            )?;
            solved.process_rates = current_processes.process_rates;
            solved.budget = current_processes.budget;
            solved.sediment_stock_change_kg_per_year =
                current_processes.sediment_stock_change_kg_per_year;
            equilibrium_iterations += solved.accepted_continuation_steps;
            let residual = current_state_residuals(
                &areas,
                total_area_m2,
                solved.terrain.current_elevation_m(),
                &solved.process_rates,
                solved.sediment_stock_change_kg_per_year,
                solved.budget.produced_mass_kg_per_year(),
                cancellation,
            )?;
            climate_solve_count += 1;
            terminal_residual = Some(residual);
            if std::env::var_os("SEKAI_P5_TRACE").is_some() {
                eprintln!(
                    "[p5-equilibrium] climate_solve {} pseudo_step {:.3} net_rms {:.9} m/yr gross_rms {:.9} m/yr local_imbalance {:.9} mean_rate {:.9} m/yr mean_balance {:.9} relief_rate {:.9} m/yr relief_balance {:.9} sediment_stock_change {:.6e} kg/yr sediment_stock_ratio {:.9} -> normalized_max {:.4}",
                    climate_solve_count,
                    continuation_step_years,
                    residual.net_surface_rate_rms_m_per_year(),
                    residual.gross_surface_rate_rms_m_per_year(),
                    residual.local_surface_flux_imbalance_ratio(),
                    residual.mean_elevation_rate_m_per_year(),
                    residual.mean_elevation_flux_balance_ratio(),
                    residual.rms_relief_rate_m_per_year(),
                    residual.rms_relief_flux_balance_ratio(),
                    residual.sediment_stock_change_kg_per_year(),
                    residual.sediment_stock_change_ratio(),
                    residual.normalized_max()
                );
                trace_current_state(
                    inputs.surface,
                    inputs.substrate,
                    &areas,
                    total_area_m2,
                    &solved,
                    &candidate_hydrology,
                    &candidate_climate,
                    inputs.tectonics,
                    residual.net_surface_rate_rms_m_per_year(),
                );
            }
            if residual.normalized_max() <= 1.0 {
                return publish(
                    surface,
                    surface_ref,
                    inputs.quality_profile,
                    upstream,
                    solved,
                    candidate_hydrology,
                    candidate_climate,
                    equilibrium_iterations,
                    climate_solve_count,
                    residual,
                    dense_state_bytes,
                    cancellation,
                );
            }
            climate = candidate_climate;
        }

        let terminal_residual =
            terminal_residual.expect("the validated budget runs at least one climate solve");
        Err(SurfaceFormationGenerationError::NotConverged {
            climate_solve_count,
            terminal_residual,
        })
    }
}

/// Terrain, sediment ledger, and current flux budget of one geomorphic solve.
struct GeomorphicSolve {
    terrain: FormationTerrainFields,
    process_rates: FormationProcessRates,
    budget: SedimentBudgetReport,
    sediment_stock_change_kg_per_year: f64,
    accepted_continuation_steps: u16,
}

struct CurrentProcessEvaluation {
    process_rates: FormationProcessRates,
    budget: SedimentBudgetReport,
    sediment_stock_change_kg_per_year: f64,
}

/// Complete mutable state at one accepted continuation iterate.
#[derive(Clone)]
struct GeomorphicState {
    components: ComponentState,
    terrain: FormationTerrainFields,
}

/// Exact retained component state shared by every continuation update.
#[derive(Clone)]
struct ComponentState {
    primary_relief_m: Vec<f32>,
    equilibrium_adjustment_m: Vec<f64>,
    elevation_m: Vec<f32>,
}

impl ComponentState {
    fn from_primary(primary_elevation_m: Vec<f32>) -> Self {
        let count = primary_elevation_m.len();
        let elevation_m = primary_elevation_m.clone();
        Self {
            primary_relief_m: primary_elevation_m,
            equilibrium_adjustment_m: vec![0.0; count],
            elevation_m,
        }
    }

    fn apply_signed(&mut self, increments: &[f32], sign: f64) {
        for (adjustment, &increment) in self.equilibrium_adjustment_m.iter_mut().zip(increments) {
            *adjustment += sign * f64::from(increment);
        }
    }

    fn apply_signed_f64(&mut self, increments: &[f64], sign: f64) {
        for (adjustment, &increment) in self.equilibrium_adjustment_m.iter_mut().zip(increments) {
            *adjustment += sign * increment;
        }
    }

    /// Rebuilds the working elevation from the exact retained identity so the
    /// published components always reconstruct the published elevation.
    fn refresh_elevation(
        &mut self,
        cancellation: &BuildCancellation,
    ) -> Result<(), SurfaceFormationGenerationError> {
        for index in 0..self.elevation_m.len() {
            poll_cancelled(cancellation, index)?;
            let exact_elevation =
                f64::from(self.primary_relief_m[index]) + self.equilibrium_adjustment_m[index];
            if !exact_elevation.is_finite()
                || !(f64::from(ELEVATION_MIN_M)..=f64::from(ELEVATION_MAX_M))
                    .contains(&exact_elevation)
            {
                return Err(SurfaceFormationGenerationError::ElevationOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: exact_elevation,
                });
            }
            let elevation = formation_elevation_from_components(
                self.primary_relief_m[index],
                self.equilibrium_adjustment_m[index] as f32,
            );
            if !elevation.is_finite() || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&elevation) {
                return Err(SurfaceFormationGenerationError::ElevationOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: f64::from(elevation),
                });
            }
            self.elevation_m[index] = elevation;
        }
        Ok(())
    }

    fn components(&self) -> Result<FormationElevationComponents, SurfaceFormationValidationError> {
        let equilibrium_adjustment_m = self
            .equilibrium_adjustment_m
            .iter()
            .map(|&value| value as f32)
            .collect();
        FormationElevationComponents::new(
            self.primary_relief_m.clone(),
            equilibrium_adjustment_m,
            self.elevation_m.clone(),
        )
    }
}

fn quantize_nonnegative_not_above(value: f64) -> f32 {
    debug_assert!(value.is_finite() && value >= 0.0);
    let rounded = value as f32;
    if f64::from(rounded) <= value || rounded == 0.0 {
        rounded
    } else {
        f32::from_bits(rounded.to_bits() - 1)
    }
}

struct FluvialCoverRemoval {
    remaining_thickness_m: Vec<f32>,
    removed_stock_kg: Vec<f64>,
    removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
}

fn remove_fluvial_sediment_cover(
    surface: &SphericalSurfaceSnapshot,
    sediment: &FormationSedimentFields,
    substrate: &GeologicSubstrateSnapshot,
    fluvial_erosion_m: &[f32],
    cancellation: &BuildCancellation,
) -> Result<FluvialCoverRemoval, SurfaceFormationGenerationError> {
    let count = surface.cells().len();
    let mut remaining_thickness_m = Vec::with_capacity(count);
    let mut removed_stock_kg = Vec::with_capacity(count);
    let mut removed_by_source_kg = Vec::with_capacity(count);
    for (index, &fluvial_erosion_m) in fluvial_erosion_m.iter().enumerate().take(count) {
        poll_cancelled(cancellation, index)?;
        let erosion_m = f64::from(fluvial_erosion_m);
        let stock_thickness_m = f64::from(sediment.sediment_thickness_m()[index]);
        let stock_erosion_m = erosion_m.min(stock_thickness_m);
        let area_m2 = surface.cells()[index].area.get();
        let stock_mass_kg = stock_erosion_m * area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3;
        let weights = sediment.provenance_fraction()[index].map(f64::from);
        let mut by_source = split_mass_by_weights(stock_mass_kg, weights);
        let substrate_erosion_m = erosion_m - stock_erosion_m;
        let substrate_source = substrate
            .sediment_sources()
            .get(index)
            .expect("validated substrate covers every cell")
            .raw() as usize;
        by_source[substrate_source] +=
            substrate_erosion_m * area_m2 * f64::from(substrate.crust_density_kg_m3()[index]);
        remaining_thickness_m.push(quantize_nonnegative_not_above(
            stock_thickness_m - stock_erosion_m,
        ));
        removed_stock_kg.push(stock_mass_kg);
        removed_by_source_kg.push(by_source);
    }
    Ok(FluvialCoverRemoval {
        remaining_thickness_m,
        removed_stock_kg,
        removed_by_source_kg,
    })
}

fn remaining_sediment_thickness(
    surface: &SphericalSurfaceSnapshot,
    thickness_m: &[f32],
    removed_stock_kg: &[f64],
    cancellation: &BuildCancellation,
) -> Result<Vec<f32>, SurfaceFormationGenerationError> {
    let mut remaining = Vec::with_capacity(surface.cells().len());
    for index in 0..surface.cells().len() {
        poll_cancelled(cancellation, index)?;
        let removed_thickness_m = removed_stock_kg[index]
            / (surface.cells()[index].area.get() * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3);
        remaining.push(quantize_nonnegative_not_above(
            (f64::from(thickness_m[index]) - removed_thickness_m).max(0.0),
        ));
    }
    Ok(remaining)
}

fn sum_sediment_stock_removal(
    fluvial_kg: &[f64],
    hillslope_kg: &[f64],
    coastal_kg: &[f64],
    cancellation: &BuildCancellation,
) -> Result<Vec<f64>, SurfaceFormationGenerationError> {
    let mut total = Vec::with_capacity(fluvial_kg.len());
    for index in 0..fluvial_kg.len() {
        poll_cancelled(cancellation, index)?;
        total.push(fluvial_kg[index] + hillslope_kg[index] + coastal_kg[index]);
    }
    Ok(total)
}

/// Advances one private pseudo-transient batch without publishing its work states.
fn solve_geomorphic(
    inputs: SurfaceFormationInputs<'_>,
    state: &mut GeomorphicState,
    climate: &GlobalCirculationSnapshot,
    areas: &[f64],
    total_area_m2: f64,
    continuation_step_years: &mut f64,
    workspace: &mut HillslopeWorkspace,
    cancellation: &BuildCancellation,
) -> Result<GeomorphicSolve, SurfaceFormationGenerationError> {
    let mut current = evaluate_current_processes(inputs, state, climate, workspace, cancellation)?;
    let mut current_rate_residual =
        surface_rate_statistics(areas, total_area_m2, &current.process_rates).net_rms_m_per_year;
    let mut accepted_steps = 0_u16;
    while accepted_steps < SURFACE_FORMATION_CONTINUATION_STEPS_PER_CLIMATE_SOLVE {
        check_cancelled(cancellation)?;
        if let Some((cell, elevation_m, net_rate_m_per_year, boundary_m)) =
            blocked_by_elevation_domain(state.terrain.current_elevation_m(), &current.process_rates)
        {
            return Err(
                SurfaceFormationGenerationError::EquilibriumOutsideElevationDomain {
                    cell,
                    elevation_m,
                    net_rate_m_per_year,
                    boundary_m,
                },
            );
        }
        *continuation_step_years =
            (*continuation_step_years).min(maximum_elevation_domain_step_years(
                state.terrain.current_elevation_m(),
                &current.process_rates,
            ));
        if !continuation_step_years.is_finite() || *continuation_step_years <= 0.0 {
            break;
        }
        let mut candidate = state.clone();
        let advanced = advance_geomorphic_window(
            inputs,
            &mut candidate,
            climate,
            continuation_step_years,
            workspace,
            cancellation,
        );
        match advanced {
            Ok(_) => {}
            Err(
                SurfaceFormationGenerationError::ElevationOutOfRange { .. }
                | SurfaceFormationGenerationError::StreamPower(
                    StreamPowerGenerationError::ElevationOutOfRange { .. },
                )
                | SurfaceFormationGenerationError::Isostasy(
                    IsostasyGenerationError::ElevationOutOfRange { .. },
                ),
            ) => {
                *continuation_step_years *= 0.5;
                continue;
            }
            Err(SurfaceFormationGenerationError::Hillslope(
                HillslopeGenerationError::UnstableStep { maximum, .. },
            )) => {
                *continuation_step_years = maximum;
                continue;
            }
            Err(error) => return Err(error),
        }
        if candidate
            .terrain
            .current_elevation_m()
            .iter()
            .any(|&elevation| elevation <= ELEVATION_MIN_M || elevation >= ELEVATION_MAX_M)
        {
            *continuation_step_years *= 0.5;
            continue;
        }
        let candidate_processes =
            evaluate_current_processes(inputs, &candidate, climate, workspace, cancellation)?;
        let candidate_rate_residual =
            surface_rate_statistics(areas, total_area_m2, &candidate_processes.process_rates)
                .net_rms_m_per_year;
        let ratio = current_rate_residual / candidate_rate_residual;
        // PETSc TSPseudoTimeStepDefault uses successive-evolution relaxation
        // with its successful-step increment. A valid implicit candidate is
        // accepted even when its residual rises; the ratio contracts the next
        // pseudo-step in that case.
        *continuation_step_years *= SURFACE_FORMATION_CONTINUATION_GROWTH_FACTOR * ratio;
        *state = candidate;
        current = candidate_processes;
        current_rate_residual = candidate_rate_residual;
        accepted_steps += 1;
    }

    Ok(GeomorphicSolve {
        terrain: state.terrain.clone(),
        process_rates: current.process_rates,
        budget: current.budget,
        sediment_stock_change_kg_per_year: current.sediment_stock_change_kg_per_year,
        accepted_continuation_steps: accepted_steps,
    })
}

/// Evaluates the current annual process fluxes without accepting the trial state.
fn evaluate_current_processes(
    inputs: SurfaceFormationInputs<'_>,
    state: &GeomorphicState,
    climate: &GlobalCirculationSnapshot,
    workspace: &mut HillslopeWorkspace,
    cancellation: &BuildCancellation,
) -> Result<CurrentProcessEvaluation, SurfaceFormationGenerationError> {
    let mut trial = state.clone();
    let mut evaluation_step_years = 1.0;
    advance_geomorphic_window(
        inputs,
        &mut trial,
        climate,
        &mut evaluation_step_years,
        workspace,
        cancellation,
    )
}

fn trace_current_state(
    surface: &SphericalSurfaceSnapshot,
    substrate: &GeologicSubstrateSnapshot,
    areas: &[f64],
    total_area_m2: f64,
    solved: &GeomorphicSolve,
    hydrology: &SphericalHydrologySnapshot,
    climate: &GlobalCirculationSnapshot,
    tectonics: &EvolvedTectonicSnapshot,
    net_surface_rate_rms_m_per_year: f64,
) {
    let rate_statistics = surface_rate_statistics(areas, total_area_m2, &solved.process_rates);
    let mut mean_elevation_sum = 0.0_f64;
    let mut annual_precipitation_sum = 0.0_f64;
    let mut annual_runoff_sum = 0.0_f64;
    let mut minimum_elevation_m = f64::INFINITY;
    let mut maximum_elevation_m = f64::NEG_INFINITY;
    let mut maximum_elevation_cell = 0;
    let mut maximum_rate_cell = 0;
    let mut maximum_rate = 0.0_f64;
    let mut cells_at_elevation_bound = 0;
    for (index, &area_m2) in areas.iter().enumerate() {
        let elevation_m = f64::from(solved.terrain.current_elevation_m()[index]);
        mean_elevation_sum += area_m2 * elevation_m;
        minimum_elevation_m = minimum_elevation_m.min(elevation_m);
        if elevation_m > maximum_elevation_m {
            maximum_elevation_m = elevation_m;
            maximum_elevation_cell = index;
        }
        annual_precipitation_sum += area_m2
            * f64::from(formation_annual_precipitation_mm(
                &climate.fields().monthly_precipitation_mm_day().values()[index],
            ));
        annual_runoff_sum += area_m2 * f64::from(hydrology.annual_local_runoff_mm()[index]);
        let net_rate = net_surface_rate_at(&solved.process_rates, index);
        if net_rate.abs() > maximum_rate.abs() {
            maximum_rate = net_rate;
            maximum_rate_cell = index;
        }
        if solved.terrain.current_elevation_m()[index] == ELEVATION_MIN_M
            || solved.terrain.current_elevation_m()[index] == ELEVATION_MAX_M
        {
            cells_at_elevation_bound += 1;
        }
    }
    eprintln!(
        "[p5-state] net_surface_rate_rms={:.9} m/yr gross_surface_rate_rms={:.9} m/yr balance_ratio={:.9} mean_surface_rate={:.9} m/yr max_surface_rate={:.9} m/yr mean_elevation={:.6} m relief={:.6} m precipitation={:.6} mm/yr runoff={:.6} mm/yr sediment_yield={:.6e} kg/yr sediment_stock_change={:.6e} kg/yr",
        net_surface_rate_rms_m_per_year,
        rate_statistics.gross_rms_m_per_year,
        rate_statistics.balance_ratio,
        rate_statistics.mean_m_per_year,
        rate_statistics.max_abs_m_per_year,
        mean_elevation_sum / total_area_m2,
        maximum_elevation_m - minimum_elevation_m,
        annual_precipitation_sum / total_area_m2,
        annual_runoff_sum / total_area_m2,
        solved.budget.produced_mass_kg_per_year(),
        solved.sediment_stock_change_kg_per_year,
    );
    let rates = &solved.process_rates;
    eprintln!(
        "[p5-rate-max] cell={} elevation={:.3} sea={:.3} sediment={:.3} water={:?} receiver={:?} runoff={:.6} mm/yr drainage={:.6} km2 uplift={:.9} subsidence={:.9} mm/yr net={:.9} m/yr tectonic={:.9} fluvial={:.9} hill_erosion={:.9} hill_deposition={:.9} routed_deposition={:.9} coast_erosion={:.9} coast_deposition={:.9} isostasy={:.9} bounded_cells={}",
        maximum_rate_cell,
        solved.terrain.current_elevation_m()[maximum_rate_cell],
        solved.terrain.sea_level_m(),
        solved.terrain.sediment().sediment_thickness_m()[maximum_rate_cell],
        hydrology.surface_water().get(maximum_rate_cell),
        hydrology.flow_receiver()[maximum_rate_cell],
        hydrology.annual_local_runoff_mm()[maximum_rate_cell],
        hydrology.drainage_area_km2()[maximum_rate_cell],
        tectonics.forcing().uplift_rate_mm_per_year()[maximum_rate_cell],
        tectonics.forcing().subsidence_rate_mm_per_year()[maximum_rate_cell],
        maximum_rate,
        rates.tectonic_displacement_rate_m_per_year()[maximum_rate_cell],
        rates.fluvial_erosion_rate_m_per_year()[maximum_rate_cell],
        rates.hillslope_erosion_rate_m_per_year()[maximum_rate_cell],
        rates.hillslope_deposition_rate_m_per_year()[maximum_rate_cell],
        rates.routed_sediment_deposition_rate_m_per_year()[maximum_rate_cell],
        rates.coastal_erosion_rate_m_per_year()[maximum_rate_cell],
        rates.coastal_deposition_rate_m_per_year()[maximum_rate_cell],
        rates.isostatic_response_rate_m_per_year()[maximum_rate_cell],
        cells_at_elevation_bound,
    );
    eprintln!(
        "[p5-elevation-max] cell={} elevation={:.3} sea={:.3} sediment={:.3} water={:?} receiver={:?} runoff={:.6} mm/yr drainage={:.6} km2 uplift={:.9} subsidence={:.9} mm/yr net={:.9} m/yr tectonic={:.9} fluvial={:.9} hill_erosion={:.9} hill_deposition={:.9} routed_deposition={:.9} coast_erosion={:.9} coast_deposition={:.9} isostasy={:.9}",
        maximum_elevation_cell,
        solved.terrain.current_elevation_m()[maximum_elevation_cell],
        solved.terrain.sea_level_m(),
        solved.terrain.sediment().sediment_thickness_m()[maximum_elevation_cell],
        hydrology.surface_water().get(maximum_elevation_cell),
        hydrology.flow_receiver()[maximum_elevation_cell],
        hydrology.annual_local_runoff_mm()[maximum_elevation_cell],
        hydrology.drainage_area_km2()[maximum_elevation_cell],
        tectonics.forcing().uplift_rate_mm_per_year()[maximum_elevation_cell],
        tectonics.forcing().subsidence_rate_mm_per_year()[maximum_elevation_cell],
        net_surface_rate_at(rates, maximum_elevation_cell),
        rates.tectonic_displacement_rate_m_per_year()[maximum_elevation_cell],
        rates.fluvial_erosion_rate_m_per_year()[maximum_elevation_cell],
        rates.hillslope_erosion_rate_m_per_year()[maximum_elevation_cell],
        rates.hillslope_deposition_rate_m_per_year()[maximum_elevation_cell],
        rates.routed_sediment_deposition_rate_m_per_year()[maximum_elevation_cell],
        rates.coastal_erosion_rate_m_per_year()[maximum_elevation_cell],
        rates.coastal_deposition_rate_m_per_year()[maximum_elevation_cell],
        rates.isostatic_response_rate_m_per_year()[maximum_elevation_cell],
    );
    let receiver =
        hydrology.flow_receiver()[maximum_elevation_cell].map(|cell| cell.raw() as usize);
    let receiver_elevation_m = receiver
        .map(|index| solved.terrain.current_elevation_m()[index])
        .unwrap_or(f32::NAN);
    let receiver_drainage_elevation_m = receiver
        .map(|index| hydrology.drainage_surface_elevation_m().values()[index])
        .unwrap_or(f32::NAN);
    eprintln!(
        "[p5-elevation-max-context] cell={} area={:.6e} m2 receiver_elevation={:.6} m drainage_elevation={:.6} m receiver_drainage_elevation={:.6} m drainage_drop={:.6} m throughput={:.6e} kg/yr shelf={:.6e} kg/yr deep={:.6e} kg/yr endorheic={:.6e} kg/yr erodibility={:.6} fracture={:.6} density={:.6} kg/m3",
        maximum_elevation_cell,
        surface.cells()[maximum_elevation_cell].area.get(),
        receiver_elevation_m,
        hydrology.drainage_surface_elevation_m().values()[maximum_elevation_cell],
        receiver_drainage_elevation_m,
        hydrology.drainage_surface_elevation_m().values()[maximum_elevation_cell]
            - receiver_drainage_elevation_m,
        solved
            .terrain
            .sediment()
            .sediment_throughput_kg_per_year()[maximum_elevation_cell],
        solved.terrain.sediment().shelf_deposition_kg_per_year()[maximum_elevation_cell],
        solved.terrain.sediment().deep_ocean_export_kg_per_year()[maximum_elevation_cell],
        solved
            .terrain
            .sediment()
            .endorheic_deposition_kg_per_year()[maximum_elevation_cell],
        substrate.erodibility()[maximum_elevation_cell],
        substrate.fracture_intensity()[maximum_elevation_cell],
        substrate.crust_density_kg_m3()[maximum_elevation_cell],
    );
}

fn net_surface_rate_at(rates: &FormationProcessRates, index: usize) -> f64 {
    f64::from(rates.tectonic_displacement_rate_m_per_year()[index])
        - f64::from(rates.fluvial_erosion_rate_m_per_year()[index])
        - f64::from(rates.hillslope_erosion_rate_m_per_year()[index])
        + f64::from(rates.hillslope_deposition_rate_m_per_year()[index])
        + f64::from(rates.routed_sediment_deposition_rate_m_per_year()[index])
        - f64::from(rates.coastal_erosion_rate_m_per_year()[index])
        + f64::from(rates.coastal_deposition_rate_m_per_year()[index])
        + f64::from(rates.isostatic_response_rate_m_per_year()[index])
}

fn maximum_elevation_domain_step_years(elevation_m: &[f32], rates: &FormationProcessRates) -> f64 {
    elevation_m
        .iter()
        .enumerate()
        .filter_map(|(index, &elevation)| {
            let rate = net_surface_rate_at(rates, index);
            if rate > 0.0 {
                Some((f64::from(ELEVATION_MAX_M) - f64::from(elevation)) / rate)
            } else if rate < 0.0 {
                Some((f64::from(elevation) - f64::from(ELEVATION_MIN_M)) / -rate)
            } else {
                None
            }
        })
        .fold(f64::INFINITY, f64::min)
        .max(0.0)
}

fn blocked_by_elevation_domain(
    elevation_m: &[f32],
    rates: &FormationProcessRates,
) -> Option<(CellId, f32, f64, f32)> {
    elevation_m
        .iter()
        .enumerate()
        .find_map(|(index, &elevation)| {
            let rate = net_surface_rate_at(rates, index);
            let boundary =
                if rate > 0.0 && elevation >= f32::from_bits(ELEVATION_MAX_M.to_bits() - 1) {
                    ELEVATION_MAX_M
                } else if rate < 0.0 && elevation <= f32::from_bits(ELEVATION_MIN_M.to_bits() - 1) {
                    ELEVATION_MIN_M
                } else {
                    return None;
                };
            Some((CellId::from_raw(index as u32), elevation, rate, boundary))
        })
}

struct SurfaceRateStatistics {
    net_rms_m_per_year: f64,
    gross_rms_m_per_year: f64,
    balance_ratio: f64,
    mean_m_per_year: f64,
    max_abs_m_per_year: f64,
}

fn surface_rate_statistics(
    areas: &[f64],
    total_area_m2: f64,
    rates: &FormationProcessRates,
) -> SurfaceRateStatistics {
    let mut net_square_sum = 0.0_f64;
    let mut gross_square_sum = 0.0_f64;
    let mut net_sum = 0.0_f64;
    let mut max_abs_m_per_year = 0.0_f64;
    for (index, &area_m2) in areas.iter().enumerate() {
        let signed = [
            f64::from(rates.tectonic_displacement_rate_m_per_year()[index]),
            -f64::from(rates.fluvial_erosion_rate_m_per_year()[index]),
            -f64::from(rates.hillslope_erosion_rate_m_per_year()[index]),
            f64::from(rates.hillslope_deposition_rate_m_per_year()[index]),
            f64::from(rates.routed_sediment_deposition_rate_m_per_year()[index]),
            -f64::from(rates.coastal_erosion_rate_m_per_year()[index]),
            f64::from(rates.coastal_deposition_rate_m_per_year()[index]),
            f64::from(rates.isostatic_response_rate_m_per_year()[index]),
        ];
        let net = signed.iter().sum::<f64>();
        let gross = signed.iter().map(|value| value.abs()).sum::<f64>();
        net_square_sum += area_m2 * net * net;
        gross_square_sum += area_m2 * gross * gross;
        net_sum += area_m2 * net;
        max_abs_m_per_year = max_abs_m_per_year.max(net.abs());
    }
    let net_rms = (net_square_sum / total_area_m2).sqrt();
    let gross_rms_m_per_year = (gross_square_sum / total_area_m2).sqrt();
    SurfaceRateStatistics {
        net_rms_m_per_year: net_rms,
        gross_rms_m_per_year,
        balance_ratio: formation_relative_flux_imbalance(net_rms, gross_rms_m_per_year),
        mean_m_per_year: net_sum / total_area_m2,
        max_abs_m_per_year,
    }
}

fn initial_geomorphic_state(
    inputs: SurfaceFormationInputs<'_>,
) -> Result<GeomorphicState, SurfaceFormationGenerationError> {
    Ok(GeomorphicState {
        components: ComponentState::from_primary(inputs.relief.elevation_m().to_vec()),
        terrain: primary_relief_terrain(inputs)?,
    })
}

/// Advances one complete geomorphic window from the retained input state.
fn advance_geomorphic_window(
    inputs: SurfaceFormationInputs<'_>,
    state: &mut GeomorphicState,
    climate: &GlobalCirculationSnapshot,
    step_years: &mut f64,
    workspace: &mut HillslopeWorkspace,
    cancellation: &BuildCancellation,
) -> Result<CurrentProcessEvaluation, SurfaceFormationGenerationError> {
    check_cancelled(cancellation)?;
    let surface = inputs.surface;
    let annual_precipitation_mm = annual_precipitation_mm(climate, cancellation)?;
    let hydrology = FormationHydrologyGenerator::generate_from_validated(
        surface,
        &state.terrain,
        inputs.substrate,
        climate,
        inputs.formation_spec,
        cancellation,
    )?;
    let pre_step_hillslope_inputs = HillslopeInputs {
        elevation_m: &state.components.elevation_m,
        surface_water: hydrology.surface_water(),
        substrate_erodibility: inputs.substrate.erodibility(),
        fracture_intensity: inputs.substrate.fracture_intensity(),
        annual_precipitation_mm: &annual_precipitation_mm,
        substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
        sediment_sources: inputs.substrate.sediment_sources(),
        sediment_thickness_m: state.terrain.sediment().sediment_thickness_m(),
        sediment_provenance_fraction: state.terrain.sediment().provenance_fraction(),
    };
    let maximum_hillslope_step_years =
        NonlinearHillslopeTransport::maximum_stable_step_years_from_validated_surface(
            surface,
            pre_step_hillslope_inputs,
            workspace,
            cancellation,
        )?;
    *step_years = (*step_years).min(maximum_hillslope_step_years);

    let stream = ImplicitStreamPowerSolver::advance_from_validated_snapshots(
        surface,
        &state.components.elevation_m,
        &hydrology,
        inputs.tectonics,
        inputs.substrate,
        *step_years,
        cancellation,
    )?;
    state
        .components
        .apply_signed(stream.tectonic_displacement_m(), 1.0);
    state
        .components
        .apply_signed(stream.fluvial_erosion_m(), -1.0);
    state.components.refresh_elevation(cancellation)?;

    let fluvial_cover = remove_fluvial_sediment_cover(
        surface,
        state.terrain.sediment(),
        inputs.substrate,
        stream.fluvial_erosion_m(),
        cancellation,
    )?;

    let hillslope = NonlinearHillslopeTransport::advance_from_validated_surface(
        surface,
        HillslopeInputs {
            elevation_m: &state.components.elevation_m,
            surface_water: hydrology.surface_water(),
            substrate_erodibility: inputs.substrate.erodibility(),
            fracture_intensity: inputs.substrate.fracture_intensity(),
            annual_precipitation_mm: &annual_precipitation_mm,
            substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
            sediment_sources: inputs.substrate.sediment_sources(),
            sediment_thickness_m: &fluvial_cover.remaining_thickness_m,
            sediment_provenance_fraction: state.terrain.sediment().provenance_fraction(),
        },
        *step_years,
        workspace,
        cancellation,
    )?;
    state
        .components
        .apply_signed(hillslope.hillslope_erosion_m(), -1.0);
    state
        .components
        .apply_signed(hillslope.hillslope_deposition_m(), 1.0);
    state.components.refresh_elevation(cancellation)?;

    let coastal_cover_thickness_m = remaining_sediment_thickness(
        surface,
        &fluvial_cover.remaining_thickness_m,
        hillslope.sediment_stock_removed_kg(),
        cancellation,
    )?;

    let coast_water = FormationSeaLevelSolver::solve_from_validated_surface(
        surface,
        &state.components.elevation_m,
        inputs.relief.water_inventory_m3(),
        cancellation,
    )?;
    let coast = CoastalExchange::advance_from_validated_surface(
        surface,
        CoastalInputs {
            elevation_m: &state.components.elevation_m,
            surface_water_geometry: coast_water.geometry(),
            substrate_erodibility: inputs.substrate.erodibility(),
            sediment_thickness_m: &coastal_cover_thickness_m,
            sediment_provenance_fraction: state.terrain.sediment().provenance_fraction(),
            substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
            sediment_sources: inputs.substrate.sediment_sources(),
            near_surface_wind_m_s: climate.fields().near_surface_wind_m_s().values(),
            surface_ocean_current_m_s: climate.fields().surface_ocean_current_m_s().values(),
        },
        *step_years,
        cancellation,
    )?;
    state
        .components
        .apply_signed(coast.coastal_erosion_m(), -1.0);
    state.components.refresh_elevation(cancellation)?;

    let sediment_stock_removed_kg = sum_sediment_stock_removal(
        &fluvial_cover.removed_stock_kg,
        hillslope.sediment_stock_removed_kg(),
        coast.sediment_stock_removed_kg(),
        cancellation,
    )?;
    let sediment = ProvenanceSedimentRouter::route_from_validated_surface(
        surface,
        SedimentInputs {
            elevation_m: &state.components.elevation_m,
            sea_level_m: coast_water.sea_level_m(),
            surface_water: hydrology.surface_water(),
            flow_receiver: hydrology.flow_receiver(),
            mean_annual_discharge_m3_s: hydrology.mean_annual_discharge_m3_s(),
            effective_settling_velocity_m_per_year:
                FORMATION_DETACHMENT_LIMITED_EFFECTIVE_SETTLING_VELOCITY_M_PER_YEAR,
            fluvial_removed_by_source_kg: &fluvial_cover.removed_by_source_kg,
            hillslope_removed_by_source_kg: hillslope.removed_by_source_kg(),
            hillslope_deposited_by_source_kg: hillslope.deposited_by_source_kg(),
            coastal_removed_by_source_kg: coast.removed_by_source_kg(),
            coastal_ocean_injection_by_source_kg: coast.ocean_injection_by_source_kg(),
            marine_exposure: coast.marine_exposure(),
            previous_sediment_thickness_m: state.terrain.sediment().sediment_thickness_m(),
            previous_provenance_fraction: state.terrain.sediment().provenance_fraction(),
            sediment_stock_removed_kg: &sediment_stock_removed_kg,
        },
        *step_years,
        cancellation,
    )?;
    state
        .components
        .apply_signed(sediment.routed_sediment_deposition_m(), 1.0);
    state
        .components
        .apply_signed(sediment.coastal_deposition_m(), 1.0);
    state.components.refresh_elevation(cancellation)?;
    let budget = *sediment.budget_report();
    let sediment_stock_change_kg_per_year = sediment_stock_change_kg_per_year(
        sediment.deposited_mass_kg(),
        &sediment_stock_removed_kg,
        *step_years,
    );

    let isostatic_response_m = apply_local_airy_response(
        surface,
        &mut state.components,
        sediment.removed_mass_kg(),
        sediment.deposited_mass_kg(),
        cancellation,
    )?;

    let water = FormationSeaLevelSolver::solve_from_validated_surface(
        surface,
        &state.components.elevation_m,
        inputs.relief.water_inventory_m3(),
        cancellation,
    )?;
    state.terrain = FormationTerrainFields::new(
        FORMATION_TERRAIN_FIELDS_SCHEMA_V3,
        state.components.components()?,
        water.into_geometry(),
        inputs.relief.water_inventory_m3(),
        sediment.fields().clone(),
    )?;
    Ok(CurrentProcessEvaluation {
        process_rates: FormationProcessRates::new(
            annualize(stream.tectonic_displacement_m(), *step_years),
            annualize(stream.fluvial_erosion_m(), *step_years),
            annualize(hillslope.hillslope_erosion_m(), *step_years),
            annualize(hillslope.hillslope_deposition_m(), *step_years),
            annualize(sediment.routed_sediment_deposition_m(), *step_years),
            annualize(coast.coastal_erosion_m(), *step_years),
            annualize(sediment.coastal_deposition_m(), *step_years),
            annualize_f64(&isostatic_response_m, *step_years),
        )?,
        budget,
        sediment_stock_change_kg_per_year,
    })
}

fn apply_local_airy_response(
    surface: &SphericalSurfaceSnapshot,
    components: &mut ComponentState,
    removed_mass_kg: &[f64],
    deposited_mass_kg: &[f64],
    cancellation: &BuildCancellation,
) -> Result<Vec<f64>, SurfaceFormationGenerationError> {
    let response = LocalAiryIsostasy::response_from_validated_surface(
        surface,
        removed_mass_kg,
        deposited_mass_kg,
        cancellation,
    )?;
    components.apply_signed_f64(&response, 1.0);
    components.refresh_elevation(cancellation)?;
    Ok(response)
}

fn sediment_stock_change_kg_per_year(
    deposited_mass_kg: &[f64],
    sediment_stock_removed_kg: &[f64],
    step_years: f64,
) -> f64 {
    assert_eq!(deposited_mass_kg.len(), sediment_stock_removed_kg.len());
    deposited_mass_kg
        .iter()
        .zip(sediment_stock_removed_kg)
        .map(|(&deposited_kg, &removed_kg)| (deposited_kg - removed_kg) / step_years)
        .sum()
}

fn annualize(values: &[f32], step_years: f64) -> Vec<f32> {
    values
        .iter()
        .map(|&value| (f64::from(value) / step_years) as f32)
        .collect()
}

fn annualize_f64(values: &[f64], step_years: f64) -> Vec<f32> {
    values
        .iter()
        .map(|&value| (value / step_years) as f32)
        .collect()
}

/// Builds the immutable P3 starting terrain with an empty sediment ledger.
fn primary_relief_terrain(
    inputs: SurfaceFormationInputs<'_>,
) -> Result<FormationTerrainFields, SurfaceFormationGenerationError> {
    let count = inputs.relief.elevation_m().len();
    let primary = inputs.relief.elevation_m().to_vec();
    let zero_f32 = vec![0.0_f32; count];
    let components = FormationElevationComponents::new(primary.clone(), zero_f32.clone(), primary)?;
    let sediment = FormationSedimentFields::new(
        zero_f32.clone(),
        vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
        zero_f32,
    )?;
    Ok(FormationTerrainFields::new(
        FORMATION_TERRAIN_FIELDS_SCHEMA_V3,
        components,
        inputs.relief.surface_water_geometry().clone(),
        inputs.relief.water_inventory_m3(),
        sediment,
    )?)
}

/// Expands the published mean daily rates into the bounded annual hillslope
/// forcing through the single shared formation precipitation envelope.
fn annual_precipitation_mm(
    climate: &GlobalCirculationSnapshot,
    cancellation: &BuildCancellation,
) -> Result<Vec<f32>, SurfaceFormationGenerationError> {
    let monthly = climate.fields().monthly_precipitation_mm_day().values();
    let mut annual = Vec::with_capacity(monthly.len());
    for (index, months) in monthly.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        annual.push(formation_annual_precipitation_mm(months));
    }
    Ok(annual)
}

fn current_state_residuals(
    areas: &[f64],
    total_area_m2: f64,
    elevation_m: &[f32],
    rates: &FormationProcessRates,
    sediment_stock_change_kg_per_year: f64,
    sediment_production_kg_per_year: f64,
    cancellation: &BuildCancellation,
) -> Result<FormationResiduals, SurfaceFormationGenerationError> {
    let rate_statistics = surface_rate_statistics(areas, total_area_m2, rates);
    let net_surface_rate_rms_m_per_year = rate_statistics.net_rms_m_per_year;
    let mut mean_elevation_sum = 0.0_f64;
    for (index, &area_m2) in areas.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        mean_elevation_sum += area_m2 * f64::from(elevation_m[index]);
    }
    let mean_elevation_m = mean_elevation_sum / total_area_m2;
    let mut relief_variance_sum = 0.0_f64;
    let mut relief_rate_covariance_sum = 0.0_f64;
    for (index, &area_m2) in areas.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        let elevation_anomaly = f64::from(elevation_m[index]) - mean_elevation_m;
        let rate_anomaly = net_surface_rate_at(rates, index) - rate_statistics.mean_m_per_year;
        relief_variance_sum += area_m2 * elevation_anomaly * elevation_anomaly;
        relief_rate_covariance_sum += area_m2 * elevation_anomaly * rate_anomaly;
    }
    let rms_relief_m = (relief_variance_sum / total_area_m2).sqrt();
    let rms_relief_rate_m_per_year = if rms_relief_m == 0.0 {
        0.0
    } else {
        relief_rate_covariance_sum / total_area_m2 / rms_relief_m
    };
    let sediment_stock_change_ratio = formation_relative_flux_imbalance(
        sediment_stock_change_kg_per_year.abs(),
        sediment_production_kg_per_year,
    );
    Ok(FormationResiduals::new(
        net_surface_rate_rms_m_per_year,
        rate_statistics.gross_rms_m_per_year,
        rate_statistics.mean_m_per_year,
        rms_relief_rate_m_per_year,
        sediment_stock_change_kg_per_year,
        sediment_stock_change_ratio,
    )?)
}

#[allow(clippy::too_many_arguments)]
fn publish(
    surface: &SphericalSurfaceSnapshot,
    surface_ref: SurfaceRef,
    quality_profile: NaturalQualityProfile,
    upstream: SurfaceFormationUpstreamFingerprints,
    solved: GeomorphicSolve,
    hydrology: SphericalHydrologySnapshot,
    climate: GlobalCirculationSnapshot,
    equilibrium_iterations: u16,
    climate_solve_count: u16,
    terminal_residual: FormationResiduals,
    dense_state_bytes: u64,
    cancellation: &BuildCancellation,
) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError> {
    check_cancelled(cancellation)?;
    let solve_report = FormationSolveReport::new(
        equilibrium_iterations,
        climate_solve_count,
        terminal_residual,
        dense_state_bytes,
    )?;
    let state_fingerprint = surface_formation_state_fingerprint(
        &solved.terrain,
        &solved.process_rates,
        &hydrology,
        &climate,
    );
    let checkpoint =
        SurfaceFormationCheckpoint::new(surface_ref, quality_profile, upstream, state_fingerprint)?;
    check_cancelled(cancellation)?;
    let snapshot = NaturalSurfaceFormationSnapshot::new(
        NATURAL_SURFACE_FORMATION_SCHEMA_V3,
        surface_ref,
        checkpoint,
        solved.terrain,
        solved.process_rates,
        hydrology,
        climate,
        solve_report,
        solved.budget,
        SurfaceFormationCapabilitySet::p5(),
    )?;
    snapshot.validate_against(surface)?;
    check_cancelled(cancellation)?;
    Ok(snapshot)
}

fn validate_inputs(
    inputs: SurfaceFormationInputs<'_>,
    cancellation: &BuildCancellation,
) -> Result<SurfaceRef, SurfaceFormationGenerationError> {
    check_cancelled(cancellation)?;
    inputs
        .surface
        .validate_cancellable(&|| cancellation.is_cancelled())
        .map_err(|error| map_upstream(cancellation, "authoritative_surface", error))?;
    let surface_ref = SurfaceRef::from_validated_spherical(inputs.surface)?;
    check_cancelled(cancellation)?;
    inputs
        .formation_spec
        .validate()
        .map_err(|error| SurfaceFormationGenerationError::InvalidSpec(error.to_string()))?;
    inputs
        .climate_spec
        .validate()
        .map_err(|error| SurfaceFormationGenerationError::InvalidSpec(error.to_string()))?;
    check_cancelled(cancellation)?;
    inputs
        .tectonics
        .validate_against(inputs.surface)
        .map_err(|error| map_upstream(cancellation, "evolved_tectonics", error))?;
    check_cancelled(cancellation)?;
    inputs
        .substrate
        .validate_against_surface(inputs.surface)
        .map_err(|error| map_upstream(cancellation, "geologic_substrate", error))?;
    check_cancelled(cancellation)?;
    inputs
        .relief
        .validate()
        .map_err(|error| map_upstream(cancellation, "primary_relief", error))?;
    check_cancelled(cancellation)?;
    inputs
        .domain
        .validate_against_cancellable(inputs.surface, &|| cancellation.is_cancelled())
        .map_err(|error| map_upstream(cancellation, "climate_work_domain", error))?;
    check_cancelled(cancellation)?;
    inputs
        .initial_climate
        .validate_against_cancellable(inputs.surface, &|| cancellation.is_cancelled())
        .map_err(|error| map_upstream(cancellation, "initial_climate", error))?;
    for (role, found) in [
        ("evolved_tectonics", inputs.tectonics.surface_ref()),
        ("geologic_substrate", inputs.substrate.surface_ref()),
        ("primary_relief", inputs.relief.surface_ref()),
        ("initial_climate", inputs.initial_climate.surface_ref()),
    ] {
        if found != surface_ref {
            return Err(SurfaceFormationGenerationError::UpstreamSurfaceMismatch {
                role,
                found,
                expected: surface_ref,
            });
        }
    }
    if inputs.domain.profile() != inputs.quality_profile
        || inputs.initial_climate.checkpoint().quality_profile() != inputs.quality_profile
    {
        return Err(SurfaceFormationGenerationError::QualityProfileMismatch {
            expected: inputs.quality_profile,
        });
    }
    Ok(surface_ref)
}

fn map_upstream<E: std::fmt::Display>(
    cancellation: &BuildCancellation,
    role: &'static str,
    error: E,
) -> SurfaceFormationGenerationError {
    if cancellation.is_cancelled() {
        SurfaceFormationGenerationError::Cancelled
    } else {
        SurfaceFormationGenerationError::InvalidUpstream(format!("{role}: {error}"))
    }
}

fn cell_areas(
    surface: &SphericalSurfaceSnapshot,
    cancellation: &BuildCancellation,
) -> Result<Vec<f64>, SurfaceFormationGenerationError> {
    let mut areas = Vec::with_capacity(surface.cells().len());
    for (index, cell) in surface.cells().iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        areas.push(cell.area.get());
    }
    Ok(areas)
}

fn upstream_fingerprints(
    inputs: SurfaceFormationInputs<'_>,
    cancellation: &BuildCancellation,
) -> Result<SurfaceFormationUpstreamFingerprints, SurfaceFormationGenerationError> {
    Ok(SurfaceFormationUpstreamFingerprints::new(
        input_fingerprint(
            b"sekai.p5.evolved-tectonics.v1\0",
            inputs.tectonics,
            cancellation,
        )?,
        input_fingerprint(
            b"sekai.p5.geologic-substrate.v1\0",
            inputs.substrate,
            cancellation,
        )?,
        input_fingerprint(b"sekai.p5.primary-relief.v1\0", inputs.relief, cancellation)?,
        inputs
            .domain
            .fingerprint_cancellable(&|| cancellation.is_cancelled())
            .map_err(|error| map_upstream(cancellation, "climate_work_domain", error))?,
        input_fingerprint(
            b"sekai.p5.climate-spec.v1\0",
            inputs.climate_spec,
            cancellation,
        )?,
        *inputs.initial_climate.checkpoint().fingerprint(),
        input_fingerprint(
            b"sekai.p5.formation-spec.v1\0",
            inputs.formation_spec,
            cancellation,
        )?,
    )?)
}

/// Hashes one complete upstream product through canonical JSON.
fn input_fingerprint<T: Serialize>(
    domain: &[u8],
    value: &T,
    cancellation: &BuildCancellation,
) -> Result<[u8; 32], SurfaceFormationGenerationError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    let mut writer = CancellableHashWriter {
        hasher: &mut hasher,
        cancellation,
        pending: 0,
        cancelled: false,
    };
    let result = serde_json::to_writer(&mut writer, value);
    let cancelled = writer.cancelled;
    result.map_err(|error| {
        if cancelled {
            SurfaceFormationGenerationError::Cancelled
        } else {
            SurfaceFormationGenerationError::InputSerialization {
                reason: error.to_string(),
            }
        }
    })?;
    check_cancelled(cancellation)?;
    Ok(*hasher.finalize().as_bytes())
}

struct CancellableHashWriter<'a> {
    hasher: &'a mut blake3::Hasher,
    cancellation: &'a BuildCancellation,
    pending: usize,
    cancelled: bool,
}

impl Write for CancellableHashWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.pending += buffer.len();
        if self.pending >= FINGERPRINT_POLL_BYTES {
            self.pending = 0;
            if self.cancellation.is_cancelled() {
                self.cancelled = true;
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "surface formation input fingerprint cancelled",
                ));
            }
        }
        self.hasher.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn poll_cancelled(
    cancellation: &BuildCancellation,
    index: usize,
) -> Result<(), SurfaceFormationGenerationError> {
    if index & CANCELLATION_POLL_MASK == 0 {
        check_cancelled(cancellation)?;
    }
    Ok(())
}

fn check_cancelled(
    cancellation: &BuildCancellation,
) -> Result<(), SurfaceFormationGenerationError> {
    if cancellation.is_cancelled() {
        Err(SurfaceFormationGenerationError::Cancelled)
    } else {
        Ok(())
    }
}

/// Failures from the complete coupled formation solve.
#[derive(Debug, Error)]
pub enum SurfaceFormationGenerationError {
    /// Cooperative cancellation interrupted the solve before publication.
    #[error("surface formation solve cancelled")]
    Cancelled,
    /// The requested climate-solve budget is outside the locked bound.
    #[error("climate solve limit {found} is outside 1..={maximum}")]
    InvalidIterationLimit { found: u16, maximum: u16 },
    /// The bounded equilibrium solve did not close within its work budget.
    ///
    /// Carries the final residual vector (spec §6: the typed failure
    /// carries the best report), so the panel names the failing
    /// component instead of one opaque number.
    #[error(
        "formation equilibrium did not converge in {climate_solve_count} climate solves \
         (normalized residual {:.4}: net_surface_rate_rms {:.6e} m/yr, \
         gross_surface_rate_rms {:.6e} m/yr, local_surface_imbalance {:.6e}, \
         mean_elevation_rate {:.6e} m/yr ({:.6e}), rms_relief_rate {:.6e} m/yr ({:.6e}), \
         sediment_stock_change {:.6e} kg/yr, sediment_stock_ratio {:.6e})",
        terminal_residual.normalized_max(),
        terminal_residual.net_surface_rate_rms_m_per_year(),
        terminal_residual.gross_surface_rate_rms_m_per_year(),
        terminal_residual.local_surface_flux_imbalance_ratio(),
        terminal_residual.mean_elevation_rate_m_per_year(),
        terminal_residual.mean_elevation_flux_balance_ratio(),
        terminal_residual.rms_relief_rate_m_per_year(),
        terminal_residual.rms_relief_flux_balance_ratio(),
        terminal_residual.sediment_stock_change_kg_per_year(),
        terminal_residual.sediment_stock_change_ratio()
    )]
    NotConverged {
        climate_solve_count: u16,
        terminal_residual: FormationResiduals,
    },
    /// An upstream product belongs to a different authoritative surface.
    #[error("{role} belongs to surface {found:?} instead of {expected:?}")]
    UpstreamSurfaceMismatch {
        role: &'static str,
        found: SurfaceRef,
        expected: SurfaceRef,
    },
    /// An upstream product was resolved at a different quality profile.
    #[error("upstream products disagree with the requested {expected:?} profile")]
    QualityProfileMismatch { expected: NaturalQualityProfile },
    /// The retained identity left the publishable elevation range.
    #[error("cell {cell:?} reached elevation {found} outside the publishable range")]
    ElevationOutOfRange { cell: CellId, found: f64 },
    /// A nonzero terminal flux points out of the representable elevation domain.
    #[error(
        "current-state equilibrium lies outside the elevation domain at {cell:?}: elevation \
         {elevation_m} m has net rate {net_rate_m_per_year} m/yr toward {boundary_m} m"
    )]
    EquilibriumOutsideElevationDomain {
        cell: CellId,
        elevation_m: f32,
        net_rate_m_per_year: f64,
        boundary_m: f32,
    },
    /// The conservative dense-owner inventory overflowed its counter.
    #[error("surface formation dense allocation inventory overflowed")]
    AllocationOverflow,
    /// One upstream product could not be serialized for identity hashing.
    #[error("upstream identity serialization failed: {reason}")]
    InputSerialization { reason: String },
    /// The authoritative surface is not a valid spherical identity.
    #[error(transparent)]
    InvalidSurfaceIdentity(#[from] SurfaceRefError),
    /// The authoritative surface failed validation.
    #[error("invalid authoritative surface: {0}")]
    InvalidSurface(String),
    /// One resolved specification is invalid.
    #[error("invalid specification: {0}")]
    InvalidSpec(String),
    /// One upstream product failed contextual validation.
    #[error("invalid upstream product: {0}")]
    InvalidUpstream(String),
    /// The published formation product violated its own contract.
    #[error(transparent)]
    InvalidProduct(#[from] SurfaceFormationValidationError),
    /// The hydrology boundary failed.
    #[error(transparent)]
    Hydrology(#[from] FormationHydrologyGenerationError),
    /// The implicit stream-power kernel failed.
    #[error(transparent)]
    StreamPower(#[from] StreamPowerGenerationError),
    /// The paired hillslope kernel failed.
    #[error(transparent)]
    Hillslope(#[from] HillslopeGenerationError),
    /// The coastal exchange kernel failed.
    #[error(transparent)]
    Coast(#[from] CoastGenerationError),
    /// The provenance sediment router failed.
    #[error(transparent)]
    Sediment(#[from] SedimentGenerationError),
    /// The Airy response or physical sea-level solve failed.
    #[error(transparent)]
    Isostasy(#[from] IsostasyGenerationError),
    /// Rebuilding the production climate forcing failed.
    #[error(transparent)]
    ClimateForcing(#[from] GlobalClimateForcingError),
    /// The selected production circulation solve failed.
    #[error(transparent)]
    Climate(#[from] GlobalCirculationGenerationError),
}

#[cfg(test)]
mod tests {
    use super::{
        apply_local_airy_response, sediment_stock_change_kg_per_year, ComponentState,
        FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    };
    use crate::engine::BuildCancellation;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{ELEVATION_MAX_M, FORMATION_AIRY_MANTLE_DENSITY_KG_M3};
    use crate::world::{Meters, SphericalSpaceSpec};

    #[test]
    fn local_airy_uses_the_exact_component_state_at_an_f32_boundary() {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(10_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let count = surface.cells().len();
        let area_m2 = surface.cells()[0].area.get();
        let airy_response_m = 0.000_260_834_617_f64;
        let eroded_density_kg_m3 = 2_827.0_f64;
        let eroded_thickness_m =
            airy_response_m * FORMATION_AIRY_MANTLE_DENSITY_KG_M3 / eroded_density_kg_m3;
        let mut primary_elevation_m = vec![0.0_f32; count];
        primary_elevation_m[0] = ELEVATION_MAX_M;
        let mut components = ComponentState::from_primary(primary_elevation_m);
        let mut erosion_m = vec![0.0_f32; count];
        erosion_m[0] = eroded_thickness_m as f32;
        components.apply_signed(&erosion_m, -1.0);
        components
            .refresh_elevation(&BuildCancellation::new())
            .unwrap();
        assert_eq!(components.elevation_m[0], ELEVATION_MAX_M);

        let mut removed_mass_kg = vec![0.0_f64; count];
        removed_mass_kg[0] = FORMATION_AIRY_MANTLE_DENSITY_KG_M3 * area_m2 * airy_response_m;
        let response = apply_local_airy_response(
            &surface,
            &mut components,
            &removed_mass_kg,
            &vec![0.0; count],
            &BuildCancellation::new(),
        )
        .unwrap();

        assert_eq!(response[0], airy_response_m);
        assert!(components.equilibrium_adjustment_m[0] < 0.0);
        assert_eq!(components.elevation_m[0], ELEVATION_MAX_M);
    }

    #[test]
    fn exact_component_state_rejects_true_sub_ulp_elevation_overflow() {
        let mut components = ComponentState::from_primary(vec![ELEVATION_MAX_M]);
        components.apply_signed_f64(&[0.000_260_834_617], 1.0);

        assert!(matches!(
            components.refresh_elevation(&BuildCancellation::new()),
            Err(super::SurfaceFormationGenerationError::ElevationOutOfRange {
                cell,
                found,
            }) if cell.raw() == 0 && found > f64::from(ELEVATION_MAX_M)
        ));
    }

    #[test]
    fn f64_mass_ledger_preserves_stock_flux_below_f32_thickness_ulp() {
        let initial_thickness_m = 1_000_000.0_f32;
        let deposited_mass_kg = [4.0_f64];
        let removed_mass_kg = [1.0_f64];
        let step_years = 2.0;
        let thickness_change_m =
            (deposited_mass_kg[0] - removed_mass_kg[0]) / FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3;

        assert_eq!(
            (f64::from(initial_thickness_m) + thickness_change_m) as f32,
            initial_thickness_m,
            "the fixture change must disappear from an f32 thickness snapshot"
        );
        assert_eq!(
            sediment_stock_change_kg_per_year(&deposited_mass_kg, &removed_mass_kg, step_years,),
            1.5
        );
    }
}
