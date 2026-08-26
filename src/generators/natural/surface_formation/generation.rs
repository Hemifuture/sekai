use std::io::{self, Write};

use serde::Serialize;
use thiserror::Error;

use super::super::surface_water_geometry::solve_physical_sea_level_exact;
use super::sediment::split_mass_by_weights;
use super::{
    CoastGenerationError, CoastalExchange, CoastalInputs, FormationHydrologyGenerationError,
    FormationHydrologyGenerator, FormationState, FormationStateError, HillslopeGenerationError,
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
    EvolvedTectonicSnapshot, FormationProcessRates, FormationResiduals, FormationSedimentFields,
    FormationSolveReport, FormationTerrainFields, GeologicSubstrateSnapshot,
    GlobalCirculationSnapshot, HydroErosionSpec, NaturalQualityProfile,
    NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot, SedimentBudgetReport,
    SphericalHydrologySnapshot, SurfaceFormationCapabilitySet, SurfaceFormationCheckpoint,
    SurfaceFormationUpstreamFingerprints, SurfaceFormationValidationError, WaterVolumeSolveError,
    ELEVATION_MAX_M, ELEVATION_MIN_M, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    FORMATION_DETACHMENT_LIMITED_EFFECTIVE_SETTLING_VELOCITY_M_PER_YEAR,
    FORMATION_TERRAIN_FIELDS_SCHEMA_V4, NATURAL_SURFACE_FORMATION_SCHEMA_V3,
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
            let candidate_hydrology = FormationHydrologyGenerator::generate_from_validated_exact(
                surface,
                state.components.current_elevation_exact_m(),
                state.components.surface_water_geometry().land_ocean(),
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
                state.components.current_elevation_exact_m(),
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
    process_rates: ExactFormationProcessRates,
    budget: SedimentBudgetReport,
    sediment_stock_change_kg_per_year: f64,
    accepted_continuation_steps: u16,
}

struct CurrentProcessEvaluation {
    process_rates: ExactFormationProcessRates,
    budget: SedimentBudgetReport,
    sediment_stock_change_kg_per_year: f64,
}

#[derive(Debug, Clone, Copy)]
struct ProcessDisplacements<'a> {
    tectonic_displacement_m: &'a [f64],
    fluvial_erosion_m: &'a [f64],
    hillslope_erosion_m: &'a [f64],
    hillslope_deposition_m: &'a [f64],
    routed_sediment_deposition_m: &'a [f64],
    coastal_erosion_m: &'a [f64],
    coastal_deposition_m: &'a [f64],
    isostatic_response_m: &'a [f64],
}

#[derive(Debug)]
struct ExactFormationProcessRates {
    tectonic_displacement_rate_m_per_year: Vec<f64>,
    fluvial_erosion_rate_m_per_year: Vec<f64>,
    hillslope_erosion_rate_m_per_year: Vec<f64>,
    hillslope_deposition_rate_m_per_year: Vec<f64>,
    routed_sediment_deposition_rate_m_per_year: Vec<f64>,
    coastal_erosion_rate_m_per_year: Vec<f64>,
    coastal_deposition_rate_m_per_year: Vec<f64>,
    isostatic_response_rate_m_per_year: Vec<f64>,
}

impl ExactFormationProcessRates {
    fn annualized(
        displacements: ProcessDisplacements<'_>,
        step_years: f64,
    ) -> Result<Self, SurfaceFormationGenerationError> {
        if !step_years.is_finite() || step_years <= 0.0 {
            return Err(SurfaceFormationGenerationError::InvalidFormationState {
                reason: format!(
                    "exact process step years must be positive and finite, found {step_years}"
                ),
            });
        }
        let rates = Self {
            tectonic_displacement_rate_m_per_year: annualize_exact(
                displacements.tectonic_displacement_m,
                step_years,
            ),
            fluvial_erosion_rate_m_per_year: annualize_exact(
                displacements.fluvial_erosion_m,
                step_years,
            ),
            hillslope_erosion_rate_m_per_year: annualize_exact(
                displacements.hillslope_erosion_m,
                step_years,
            ),
            hillslope_deposition_rate_m_per_year: annualize_exact(
                displacements.hillslope_deposition_m,
                step_years,
            ),
            routed_sediment_deposition_rate_m_per_year: annualize_exact(
                displacements.routed_sediment_deposition_m,
                step_years,
            ),
            coastal_erosion_rate_m_per_year: annualize_exact(
                displacements.coastal_erosion_m,
                step_years,
            ),
            coastal_deposition_rate_m_per_year: annualize_exact(
                displacements.coastal_deposition_m,
                step_years,
            ),
            isostatic_response_rate_m_per_year: annualize_exact(
                displacements.isostatic_response_m,
                step_years,
            ),
        };
        rates.validate()?;
        Ok(rates)
    }

    fn validate(&self) -> Result<(), SurfaceFormationGenerationError> {
        let expected = self.tectonic_displacement_rate_m_per_year.len();
        for (field, values, nonnegative) in [
            (
                "tectonic_displacement_rate_m_per_year",
                self.tectonic_displacement_rate_m_per_year.as_slice(),
                false,
            ),
            (
                "fluvial_erosion_rate_m_per_year",
                self.fluvial_erosion_rate_m_per_year.as_slice(),
                true,
            ),
            (
                "hillslope_erosion_rate_m_per_year",
                self.hillslope_erosion_rate_m_per_year.as_slice(),
                true,
            ),
            (
                "hillslope_deposition_rate_m_per_year",
                self.hillslope_deposition_rate_m_per_year.as_slice(),
                true,
            ),
            (
                "routed_sediment_deposition_rate_m_per_year",
                self.routed_sediment_deposition_rate_m_per_year.as_slice(),
                true,
            ),
            (
                "coastal_erosion_rate_m_per_year",
                self.coastal_erosion_rate_m_per_year.as_slice(),
                true,
            ),
            (
                "coastal_deposition_rate_m_per_year",
                self.coastal_deposition_rate_m_per_year.as_slice(),
                true,
            ),
            (
                "isostatic_response_rate_m_per_year",
                self.isostatic_response_rate_m_per_year.as_slice(),
                false,
            ),
        ] {
            if values.len() != expected {
                return Err(SurfaceFormationGenerationError::InvalidFormationState {
                    reason: format!(
                        "exact process field {field} has {} cells instead of {expected}",
                        values.len()
                    ),
                });
            }
            if let Some((index, &found)) = values
                .iter()
                .enumerate()
                .find(|(_, value)| !value.is_finite() || (nonnegative && **value < 0.0))
            {
                return Err(SurfaceFormationGenerationError::InvalidFormationState {
                    reason: format!(
                        "exact process field {field} has invalid value {found} at cell {}",
                        CellId::from_raw(index as u32).raw()
                    ),
                });
            }
        }
        Ok(())
    }

    fn to_wire(&self) -> Result<FormationProcessRates, SurfaceFormationValidationError> {
        FormationProcessRates::new(
            quantize_f64(&self.tectonic_displacement_rate_m_per_year),
            quantize_f64(&self.fluvial_erosion_rate_m_per_year),
            quantize_f64(&self.hillslope_erosion_rate_m_per_year),
            quantize_f64(&self.hillslope_deposition_rate_m_per_year),
            quantize_f64(&self.routed_sediment_deposition_rate_m_per_year),
            quantize_f64(&self.coastal_erosion_rate_m_per_year),
            quantize_f64(&self.coastal_deposition_rate_m_per_year),
            quantize_f64(&self.isostatic_response_rate_m_per_year),
        )
    }

    fn tectonic_displacement_rate_m_per_year(&self) -> &[f64] {
        &self.tectonic_displacement_rate_m_per_year
    }

    fn fluvial_erosion_rate_m_per_year(&self) -> &[f64] {
        &self.fluvial_erosion_rate_m_per_year
    }

    fn hillslope_erosion_rate_m_per_year(&self) -> &[f64] {
        &self.hillslope_erosion_rate_m_per_year
    }

    fn hillslope_deposition_rate_m_per_year(&self) -> &[f64] {
        &self.hillslope_deposition_rate_m_per_year
    }

    fn routed_sediment_deposition_rate_m_per_year(&self) -> &[f64] {
        &self.routed_sediment_deposition_rate_m_per_year
    }

    fn coastal_erosion_rate_m_per_year(&self) -> &[f64] {
        &self.coastal_erosion_rate_m_per_year
    }

    fn coastal_deposition_rate_m_per_year(&self) -> &[f64] {
        &self.coastal_deposition_rate_m_per_year
    }

    fn isostatic_response_rate_m_per_year(&self) -> &[f64] {
        &self.isostatic_response_rate_m_per_year
    }
}

/// Complete mutable state at one accepted continuation iterate.
#[derive(Clone)]
struct GeomorphicState {
    components: FormationState,
    terrain: FormationTerrainFields,
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
    fluvial_erosion_m: &[f64],
    cancellation: &BuildCancellation,
) -> Result<FluvialCoverRemoval, SurfaceFormationGenerationError> {
    let count = surface.cells().len();
    let mut remaining_thickness_m = Vec::with_capacity(count);
    let mut removed_stock_kg = Vec::with_capacity(count);
    let mut removed_by_source_kg = Vec::with_capacity(count);
    for (index, &fluvial_erosion_m) in fluvial_erosion_m.iter().enumerate().take(count) {
        poll_cancelled(cancellation, index)?;
        let erosion_m = fluvial_erosion_m;
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
            blocked_by_elevation_domain(
                state.components.current_elevation_exact_m(),
                &current.process_rates,
            )
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
                state.components.current_elevation_exact_m(),
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
            .components
            .current_elevation_exact_m()
            .iter()
            .any(|&elevation| {
                elevation <= f64::from(ELEVATION_MIN_M) || elevation >= f64::from(ELEVATION_MAX_M)
            })
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

fn net_surface_rate_at(rates: &ExactFormationProcessRates, index: usize) -> f64 {
    formation_elevation_from_components(
        0.0,
        rates.tectonic_displacement_rate_m_per_year()[index],
        rates.fluvial_erosion_rate_m_per_year()[index],
        rates.hillslope_erosion_rate_m_per_year()[index],
        rates.hillslope_deposition_rate_m_per_year()[index],
        rates.routed_sediment_deposition_rate_m_per_year()[index],
        rates.coastal_erosion_rate_m_per_year()[index],
        rates.coastal_deposition_rate_m_per_year()[index],
        rates.isostatic_response_rate_m_per_year()[index],
    )
}

fn maximum_elevation_domain_step_years(
    elevation_m: &[f64],
    rates: &ExactFormationProcessRates,
) -> f64 {
    elevation_m
        .iter()
        .enumerate()
        .filter_map(|(index, &elevation)| {
            let rate = net_surface_rate_at(rates, index);
            if rate > 0.0 {
                Some((f64::from(ELEVATION_MAX_M) - elevation) / rate)
            } else if rate < 0.0 {
                Some((elevation - f64::from(ELEVATION_MIN_M)) / -rate)
            } else {
                None
            }
        })
        .fold(f64::INFINITY, f64::min)
        .max(0.0)
}

fn blocked_by_elevation_domain(
    elevation_m: &[f64],
    rates: &ExactFormationProcessRates,
) -> Option<(CellId, f64, f64, f64)> {
    elevation_m
        .iter()
        .enumerate()
        .find_map(|(index, &elevation)| {
            let rate = net_surface_rate_at(rates, index);
            let boundary = if rate > 0.0 && elevation >= f64::from(ELEVATION_MAX_M) {
                f64::from(ELEVATION_MAX_M)
            } else if rate < 0.0 && elevation <= f64::from(ELEVATION_MIN_M) {
                f64::from(ELEVATION_MIN_M)
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
    rates: &ExactFormationProcessRates,
) -> SurfaceRateStatistics {
    let mut net_square_sum = 0.0_f64;
    let mut gross_square_sum = 0.0_f64;
    let mut net_sum = 0.0_f64;
    let mut max_abs_m_per_year = 0.0_f64;
    for (index, &area_m2) in areas.iter().enumerate() {
        let signed = [
            rates.tectonic_displacement_rate_m_per_year()[index],
            rates.fluvial_erosion_rate_m_per_year()[index],
            rates.hillslope_erosion_rate_m_per_year()[index],
            rates.hillslope_deposition_rate_m_per_year()[index],
            rates.routed_sediment_deposition_rate_m_per_year()[index],
            rates.coastal_erosion_rate_m_per_year()[index],
            rates.coastal_deposition_rate_m_per_year()[index],
            rates.isostatic_response_rate_m_per_year()[index],
        ];
        let net = net_surface_rate_at(rates, index);
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
    let components = FormationState::from_legacy_primary_wire_for_migration(inputs.relief)?;
    let terrain = primary_relief_terrain(inputs, &components)?;
    Ok(GeomorphicState {
        components,
        terrain,
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
    let hydrology = FormationHydrologyGenerator::generate_from_validated_exact(
        surface,
        state.components.current_elevation_exact_m(),
        state.components.surface_water_geometry().land_ocean(),
        inputs.substrate,
        climate,
        inputs.formation_spec,
        cancellation,
    )?;
    let pre_step_hillslope_inputs = HillslopeInputs {
        elevation_m: state.components.current_elevation_exact_m(),
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
        state.components.current_elevation_exact_m(),
        &hydrology,
        inputs.tectonics,
        inputs.substrate,
        *step_years,
        cancellation,
    )?;
    state
        .components
        .apply_tectonic_displacement_f64(stream.tectonic_displacement_m())?;
    state
        .components
        .apply_fluvial_erosion_f64(stream.fluvial_erosion_m())?;

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
            elevation_m: state.components.current_elevation_exact_m(),
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
        .apply_hillslope_erosion_f64(hillslope.hillslope_erosion_m())?;
    state
        .components
        .apply_hillslope_deposition_f64(hillslope.hillslope_deposition_m())?;

    let coastal_cover_thickness_m = remaining_sediment_thickness(
        surface,
        &fluvial_cover.remaining_thickness_m,
        hillslope.sediment_stock_removed_kg(),
        cancellation,
    )?;

    let coast_water = solve_physical_sea_level_exact(
        surface,
        state.components.current_elevation_exact_m(),
        inputs.relief.water_inventory_m3(),
        cancellation,
    )?
    .into_geometry();
    state.components.replace_surface_water_geometry(coast_water);
    let coast = CoastalExchange::advance_from_validated_surface(
        surface,
        CoastalInputs {
            elevation_m: state.components.current_elevation_exact_m(),
            ocean_area_fraction: state
                .components
                .surface_water_geometry()
                .ocean_area_fraction(),
            wet_edge_fraction: state
                .components
                .surface_water_geometry()
                .wet_edge_fraction(),
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
        .apply_coastal_erosion_f64(coast.coastal_erosion_m())?;

    let sediment_stock_removed_kg = sum_sediment_stock_removal(
        &fluvial_cover.removed_stock_kg,
        hillslope.sediment_stock_removed_kg(),
        coast.sediment_stock_removed_kg(),
        cancellation,
    )?;
    let sediment = ProvenanceSedimentRouter::route_from_validated_surface(
        surface,
        SedimentInputs {
            elevation_m: state.components.current_elevation_exact_m(),
            sea_level_m: state.components.surface_water_geometry().sea_level_m(),
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
        .apply_routed_sediment_deposition_f64(sediment.routed_sediment_deposition_m())?;
    state
        .components
        .apply_coastal_deposition_f64(sediment.coastal_deposition_m())?;
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

    let water = solve_physical_sea_level_exact(
        surface,
        state.components.current_elevation_exact_m(),
        inputs.relief.water_inventory_m3(),
        cancellation,
    )?
    .into_geometry();
    state.components.replace_surface_water_geometry(water);
    let wire_components = state.components.wire_components()?;
    let wire_water = state.components.surface_water_geometry().to_wire(
        surface,
        wire_components.final_elevation_m(),
        cancellation,
    )?;
    state.terrain = FormationTerrainFields::new(
        FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
        wire_components,
        wire_water,
        inputs.relief.water_inventory_m3(),
        sediment.fields().clone(),
    )?;
    Ok(CurrentProcessEvaluation {
        process_rates: ExactFormationProcessRates::annualized(
            ProcessDisplacements {
                tectonic_displacement_m: stream.tectonic_displacement_m(),
                fluvial_erosion_m: stream.fluvial_erosion_m(),
                hillslope_erosion_m: hillslope.hillslope_erosion_m(),
                hillslope_deposition_m: hillslope.hillslope_deposition_m(),
                routed_sediment_deposition_m: sediment.routed_sediment_deposition_m(),
                coastal_erosion_m: coast.coastal_erosion_m(),
                coastal_deposition_m: sediment.coastal_deposition_m(),
                isostatic_response_m: &isostatic_response_m,
            },
            *step_years,
        )?,
        budget,
        sediment_stock_change_kg_per_year,
    })
}

fn apply_local_airy_response(
    surface: &SphericalSurfaceSnapshot,
    components: &mut FormationState,
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
    components.apply_isostatic_response_f64(&response)?;
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

fn annualize_exact(values: &[f64], step_years: f64) -> Vec<f64> {
    values.iter().map(|&value| value / step_years).collect()
}

fn quantize_f64(values: &[f64]) -> Vec<f32> {
    values.iter().map(|&value| value as f32).collect()
}

/// Builds the immutable P3 starting terrain with an empty sediment ledger.
fn primary_relief_terrain(
    inputs: SurfaceFormationInputs<'_>,
    state: &FormationState,
) -> Result<FormationTerrainFields, SurfaceFormationGenerationError> {
    let count = inputs.relief.elevation_m().len();
    let zero_f32 = vec![0.0_f32; count];
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
        FORMATION_TERRAIN_FIELDS_SCHEMA_V4,
        state.wire_components()?,
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
    elevation_m: &[f64],
    rates: &ExactFormationProcessRates,
    sediment_stock_change_kg_per_year: f64,
    sediment_production_kg_per_year: f64,
    cancellation: &BuildCancellation,
) -> Result<FormationResiduals, SurfaceFormationGenerationError> {
    let rate_statistics = surface_rate_statistics(areas, total_area_m2, rates);
    let net_surface_rate_rms_m_per_year = rate_statistics.net_rms_m_per_year;
    let mut mean_elevation_sum = 0.0_f64;
    for (index, &area_m2) in areas.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        mean_elevation_sum += area_m2 * elevation_m[index];
    }
    let mean_elevation_m = mean_elevation_sum / total_area_m2;
    let mut relief_variance_sum = 0.0_f64;
    let mut relief_rate_covariance_sum = 0.0_f64;
    for (index, &area_m2) in areas.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        let elevation_anomaly = elevation_m[index] - mean_elevation_m;
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
    let process_rates = solved.process_rates.to_wire()?;
    let solve_report = FormationSolveReport::new(
        equilibrium_iterations,
        climate_solve_count,
        terminal_residual,
        dense_state_bytes,
    )?;
    let state_fingerprint =
        surface_formation_state_fingerprint(&solved.terrain, &process_rates, &hydrology, &climate);
    let checkpoint =
        SurfaceFormationCheckpoint::new(surface_ref, quality_profile, upstream, state_fingerprint)?;
    check_cancelled(cancellation)?;
    let snapshot = NaturalSurfaceFormationSnapshot::new(
        NATURAL_SURFACE_FORMATION_SCHEMA_V3,
        surface_ref,
        checkpoint,
        solved.terrain,
        process_rates,
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
        elevation_m: f64,
        net_rate_m_per_year: f64,
        boundary_m: f64,
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
    /// The retained exact causal state violated a non-elevation invariant.
    #[error("invalid retained formation state: {reason}")]
    InvalidFormationState { reason: String },
    /// Rebuilding an exact or projected water-volume closure failed.
    #[error(transparent)]
    WaterGeometry(#[from] WaterVolumeSolveError),
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

impl From<FormationStateError> for SurfaceFormationGenerationError {
    fn from(error: FormationStateError) -> Self {
        match error {
            FormationStateError::ElevationOutOfRange { cell, found } => {
                Self::ElevationOutOfRange { cell, found }
            }
            other => Self::InvalidFormationState {
                reason: other.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::time::{Duration, Instant};

    use super::super::state::formation_state_for_value;
    use super::{
        advance_geomorphic_window, apply_local_airy_response, initial_geomorphic_state,
        sediment_stock_change_kg_per_year, ExactFormationProcessRates, ProcessDisplacements,
        SurfaceFormationInputs, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    };
    use crate::engine::{
        derive_stage_seed, BuildArtifacts, BuildCancellation, Diagnostic, Stage, StageIdentity,
        StageRng,
    };
    use crate::generators::natural::{
        ClimateWorkDomainBuilder, EvolvedTectonicGenerator, EvolvedTectonicStage,
        GeologicSubstrateGenerator, GeologicSubstrateStage, GlobalCirculationGenerator,
        GlobalClimateForcingBuilder, NaturalQualityProfileArtifact, PrimaryReliefGenerator,
        PrimaryReliefStage,
    };
    use crate::generators::spatial::{
        GeodesicVoronoiBuilder, ProfileSurfaceBuilder, ProfileSurfaceBundle,
    };
    use crate::world::natural::{
        formation_elevation_from_components, ClimateModelProfile, ClimateSpec,
        ClimateWorkDomainSnapshot, EvolvedTectonicSnapshot, GeologicSpec,
        GeologicSubstrateSnapshot, GlobalCirculationSnapshot, HydroErosionSpec,
        NaturalQualityProfile, PrimaryReliefSnapshot, ReliefSpec, ResolvedWorldFormation,
        ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
        EARTH_WATER_REFERENCE_RADIUS_M, ELEVATION_MAX_M, ELEVATION_MIN_M,
        FORMATION_AIRY_MANTLE_DENSITY_KG_M3, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        SURFACE_FORMATION_HORIZON_YEARS,
    };
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    const PRE_MIGRATION_SEED: u64 = 42;

    #[test]
    fn solver_feedback_retains_process_rates_below_the_f32_wire_ulp() {
        let zero = [0.0_f64];
        let sub_wire_ulp = f64::from(f32::from_bits(1)) * 0.25;
        let tectonic = [sub_wire_ulp];
        let rates = ExactFormationProcessRates::annualized(
            ProcessDisplacements {
                tectonic_displacement_m: &tectonic,
                fluvial_erosion_m: &zero,
                hillslope_erosion_m: &zero,
                hillslope_deposition_m: &zero,
                routed_sediment_deposition_m: &zero,
                coastal_erosion_m: &zero,
                coastal_deposition_m: &zero,
                isostatic_response_m: &zero,
            },
            1.0,
        )
        .expect("one-cell exact process rates should validate");

        assert_eq!(
            rates.tectonic_displacement_rate_m_per_year()[0].to_bits(),
            sub_wire_ulp.to_bits()
        );
    }

    struct PreMigrationFixture {
        bundle: ProfileSurfaceBundle,
        evolved: EvolvedTectonicSnapshot,
        substrate: GeologicSubstrateSnapshot,
        relief: PrimaryReliefSnapshot,
        domain: ClimateWorkDomainSnapshot,
        climate: GlobalCirculationSnapshot,
        climate_spec: ClimateSpec,
        formation_spec: HydroErosionSpec,
        setup_elapsed: Duration,
    }

    impl PreMigrationFixture {
        fn inputs(&self) -> SurfaceFormationInputs<'_> {
            SurfaceFormationInputs {
                surface: self.bundle.authoritative_surface(),
                quality_profile: NaturalQualityProfile::Draft,
                tectonics: &self.evolved,
                substrate: &self.substrate,
                relief: &self.relief,
                domain: &self.domain,
                climate_spec: &self.climate_spec,
                initial_climate: &self.climate,
                formation_spec: &self.formation_spec,
            }
        }
    }

    #[test]
    fn local_airy_preserves_exact_erosion_and_response_below_wire_ulp() {
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
        let mut components = formation_state_for_value(f64::from(ELEVATION_MAX_M));
        let mut erosion_m = vec![0.0_f64; count];
        erosion_m[0] = eroded_thickness_m;
        components.apply_fluvial_erosion_f64(&erosion_m).unwrap();

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
        assert!(components.fluvial_erosion_m()[0] > 0.0);
        assert!(components.isostatic_response_m()[0] > 0.0);
        let expected = formation_elevation_from_components(
            f64::from(ELEVATION_MAX_M),
            0.0,
            eroded_thickness_m,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            airy_response_m,
        );
        assert_eq!(
            components.current_elevation_exact_m()[0].to_bits(),
            expected.to_bits()
        );
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

    #[test]
    #[ignore = "release-only pre-migration P5 one-advance cost probe"]
    fn pre_migration_one_advance_records_bounded_cost_evidence() {
        let evidence = pre_migration_one_advance_evidence();
        let json = serde_json::to_vec_pretty(&evidence)
            .expect("the bounded P5 probe evidence must serialize");
        assert_no_json_arrays(&evidence);

        let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("natural-quality")
            .join("p5");
        assert!(output.ends_with("target/natural-quality/p5"));
        std::fs::create_dir_all(&output).expect("the P5 evidence directory must be writable");
        let path = output.join("pre-migration-one-advance.json");
        std::fs::write(&path, &json).expect("the P5 one-advance evidence must be writable");

        assert_eq!(
            evidence["requested_duration_years"]
                .as_f64()
                .expect("requested duration is numeric")
                .to_bits(),
            SURFACE_FORMATION_HORIZON_YEARS.to_bits()
        );
        assert_eq!(evidence["prefix"]["accepted_window_count"], 1);
        assert_eq!(evidence["prefix"]["typed_failure"], serde_json::Value::Null);
        assert_eq!(evidence["full_horizon_observed"], true);
        eprintln!(
            "P5 pre-migration one-advance path={} bytes={} blake3={}",
            path.display(),
            json.len(),
            blake3::hash(&json).to_hex()
        );
    }

    fn pre_migration_one_advance_evidence() -> serde_json::Value {
        let cancellation = BuildCancellation::new();
        let fixture = pre_migration_fixture(&cancellation);
        let inputs = fixture.inputs();
        super::validate_inputs(inputs, &cancellation)
            .expect("the production probe inputs must validate");
        let mut candidate =
            initial_geomorphic_state(inputs).expect("the production initial P5 state is valid");
        let mut workspace = super::HillslopeWorkspace::default();
        let selection_started = Instant::now();
        let selection = super::evaluate_current_processes(
            inputs,
            &candidate,
            inputs.initial_climate,
            &mut workspace,
            &cancellation,
        );
        let step_selection_micros = selection_started.elapsed().as_micros();
        let (mut accepted_duration_years, selection_failure) = match selection {
            Err(error) => (0.0, Some(format!("{error:?}"))),
            Ok(current) => {
                if let Some((cell, elevation_m, net_rate_m_per_year, boundary_m)) =
                    super::blocked_by_elevation_domain(
                        candidate.components.current_elevation_exact_m(),
                        &current.process_rates,
                    )
                {
                    (
                        0.0,
                        Some(format!(
                            "{:?}",
                            super::SurfaceFormationGenerationError::
                                EquilibriumOutsideElevationDomain {
                                    cell,
                                    elevation_m,
                                    net_rate_m_per_year,
                                    boundary_m,
                                }
                        )),
                    )
                } else {
                    let maximum = super::maximum_elevation_domain_step_years(
                        candidate.components.current_elevation_exact_m(),
                        &current.process_rates,
                    );
                    let selected = SURFACE_FORMATION_HORIZON_YEARS.min(maximum);
                    let failure = (!(selected.is_finite() && selected > 0.0))
                        .then(|| "MeasurementReturnedInvalidPreselectedDuration".to_owned());
                    (selected, failure)
                }
            }
        };
        let window_started = Instant::now();
        let result = selection_failure.is_none().then(|| {
            advance_geomorphic_window(
                inputs,
                &mut candidate,
                inputs.initial_climate,
                &mut accepted_duration_years,
                &mut workspace,
                &cancellation,
            )
        });
        let kernel_micros = window_started.elapsed().as_micros();
        let reached_elevation_boundary = result.as_ref().is_some_and(Result::is_ok)
            && candidate
                .terrain
                .current_elevation_m()
                .iter()
                .any(|&value| value <= ELEVATION_MIN_M || value >= ELEVATION_MAX_M);
        let invalid_duration = !accepted_duration_years.is_finite()
            || accepted_duration_years <= 0.0
            || accepted_duration_years > SURFACE_FORMATION_HORIZON_YEARS;
        let typed_failure = match (selection_failure, result) {
            (Some(error), _) => Some(error),
            (None, Some(Err(error))) => Some(format!("{error:?}")),
            (None, Some(Ok(_))) if reached_elevation_boundary => {
                Some("MeasurementReachedElevationBoundary".to_owned())
            }
            (None, Some(Ok(_))) if invalid_duration => {
                Some("MeasurementReturnedInvalidDuration".to_owned())
            }
            (None, Some(Ok(_))) => None,
            (None, None) => Some("MeasurementSkippedProductionWindow".to_owned()),
        };
        let accepted = typed_failure.is_none();
        let accepted_duration_years = if accepted {
            accepted_duration_years
        } else {
            0.0
        };
        let accepted_window_count = u64::from(accepted);
        let rejected_window_count = u64::from(!accepted);
        let full_horizon_observed =
            accepted_duration_years.to_bits() == SURFACE_FORMATION_HORIZON_YEARS.to_bits();
        let estimated_window_count = accepted
            .then(|| (SURFACE_FORMATION_HORIZON_YEARS / accepted_duration_years).ceil() as u64);
        let accepted_window_micros = step_selection_micros
            .checked_add(kernel_micros)
            .expect("one observed P5 window duration fits in u128 microseconds");
        let estimated_kernel_micros =
            estimated_window_count.and_then(|count| kernel_micros.checked_mul(u128::from(count)));
        let estimated_selection_micros = estimated_window_count
            .and_then(|count| step_selection_micros.checked_mul(u128::from(count)));
        let estimated_advance_micros = estimated_kernel_micros
            .zip(estimated_selection_micros)
            .and_then(|(kernel, selection)| kernel.checked_add(selection));
        let incomplete_reason = if let Some(failure) = typed_failure.as_deref() {
            Some(format!(
                "one production window ended at typed failure {failure}"
            ))
        } else if full_horizon_observed {
            None
        } else {
            Some("the one-window prefix did not consume the full formation horizon".to_owned())
        };

        serde_json::json!({
            "schema_version": 1,
            "machine_profile": format!(
                "{}-{}-release",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            "seed": PRE_MIGRATION_SEED,
            "quality_profile": NaturalQualityProfile::Draft,
            "requested_duration_years": SURFACE_FORMATION_HORIZON_YEARS,
            "upstream_setup_micros": fixture.setup_elapsed.as_micros(),
            "identities": {
                "surface_fingerprint": hex(&fixture.bundle.authoritative_surface().fingerprint()),
                "profile_artifact_fingerprint": profile_artifact_fingerprint(
                    NaturalQualityProfile::Draft,
                ),
                "forcing_fingerprint": hex(fixture.climate.checkpoint().forcing_fingerprint()),
            },
            "prefix": {
                "accepted_duration_years": accepted_duration_years,
                "accepted_window_count": accepted_window_count,
                "rejected_window_count": rejected_window_count,
                "wall_time_micros": accepted_window_micros,
                "step_selection_micros": step_selection_micros,
                "kernel_micros": kernel_micros,
                "accepted_window_cost_summary": {
                    "observed_window_count": accepted_window_count,
                    "total_micros": accepted.then_some(accepted_window_micros),
                    "minimum_micros": accepted.then_some(accepted_window_micros),
                    "maximum_micros": accepted.then_some(accepted_window_micros),
                    "mean_micros": accepted.then_some(accepted_window_micros as f64),
                    "minimum_duration_years": accepted.then_some(accepted_duration_years),
                    "maximum_duration_years": accepted.then_some(accepted_duration_years),
                    "mean_duration_years": accepted.then_some(accepted_duration_years),
                },
                "completed_requested_duration": full_horizon_observed,
                "incomplete_reason": incomplete_reason,
                "typed_failure": typed_failure,
            },
            "full_cost_estimate": {
                "basis": "linear projection from one production stable window; research evidence only, not an acceptance gate",
                "estimated_accepted_window_count": estimated_window_count,
                "estimated_step_selection_wall_time_micros": estimated_selection_micros,
                "estimated_kernel_wall_time_micros": estimated_kernel_micros,
                "estimated_advance_wall_time_micros": estimated_advance_micros,
                "estimated_total_wall_time_micros": estimated_advance_micros
                    .and_then(|value| value.checked_add(fixture.setup_elapsed.as_micros())),
            },
            "full_measurement_source": full_horizon_observed.then_some("prefix"),
            "full_not_run_reason": (!full_horizon_observed).then_some(
                "full repeated advance was not run because this probe retains only its mandatory one-window prefix"
            ),
            "full_horizon_observed": full_horizon_observed,
        })
    }

    fn pre_migration_fixture(cancellation: &BuildCancellation) -> PreMigrationFixture {
        let started = Instant::now();
        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(EARTH_WATER_REFERENCE_RADIUS_M)
                .expect("the world reference radius is positive"),
            cancellation,
        )
        .expect("the Draft production surface must build");
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        )
        .expect("the frozen Continents formation preset must resolve");
        let mut evolved_rng = test_stage_rng(&EvolvedTectonicStage);
        let evolved = EvolvedTectonicGenerator::generate(
            &bundle,
            &TectonicSpec::default(),
            &formation,
            &mut evolved_rng,
        )
        .expect("the Draft production tectonics must build");
        let mut substrate_rng = test_stage_rng(&GeologicSubstrateStage);
        let substrate = GeologicSubstrateGenerator::generate(
            bundle.authoritative_surface(),
            &evolved,
            &GeologicSpec::default(),
            &formation,
            &mut substrate_rng,
        )
        .expect("the Draft production substrate must build");
        let mut relief_rng = test_stage_rng(&PrimaryReliefStage);
        let mut diagnostics = Vec::<Diagnostic>::new();
        let relief = PrimaryReliefGenerator::generate(
            bundle.authoritative_surface(),
            &evolved,
            &substrate,
            &ReliefSpec::default(),
            &mut relief_rng,
            &mut diagnostics,
        )
        .expect("the Draft production relief must build");
        let domain = ClimateWorkDomainBuilder::build(
            bundle.authoritative_surface(),
            NaturalQualityProfile::Draft,
            cancellation,
        )
        .expect("the Draft climate work domain must build");
        let climate_spec = ClimateSpec::default();
        let forcing = GlobalClimateForcingBuilder::build(
            bundle.authoritative_surface(),
            &relief,
            &climate_spec,
            &domain,
            cancellation,
        )
        .expect("the start-climate forcing must build");
        let climate = GlobalCirculationGenerator::generate(
            bundle.authoritative_surface(),
            &domain,
            &forcing,
            ClimateModelProfile::C2LayeredV1,
            cancellation,
        )
        .expect("the Draft start climate must build");
        PreMigrationFixture {
            bundle,
            evolved,
            substrate,
            relief,
            domain,
            climate,
            climate_spec,
            formation_spec: HydroErosionSpec::default(),
            setup_elapsed: started.elapsed(),
        }
    }

    fn test_stage_rng(stage: &impl Stage) -> StageRng {
        StageRng::from_seed(derive_stage_seed(
            RootSeed::new(PRE_MIGRATION_SEED),
            StageIdentity::new(stage.id().as_str(), stage.version(), stage.namespace()),
        ))
    }

    fn profile_artifact_fingerprint(profile: NaturalQualityProfile) -> String {
        let mut artifacts = BuildArtifacts::default();
        artifacts
            .insert(NaturalQualityProfileArtifact::new(profile))
            .expect("the selected production profile artifact must validate");
        hex(artifacts
            .hash::<NaturalQualityProfileArtifact>()
            .expect("the selected production profile artifact must hash")
            .as_bytes())
    }

    fn hex(bytes: &[u8; 32]) -> String {
        let mut encoded = String::with_capacity(64);
        for byte in bytes {
            write!(&mut encoded, "{byte:02x}").expect("writing into a String cannot fail");
        }
        encoded
    }

    fn assert_no_json_arrays(value: &serde_json::Value) {
        match value {
            serde_json::Value::Array(_) => {
                panic!("the streaming cost probe must not retain terrain or history arrays")
            }
            serde_json::Value::Object(entries) => {
                for child in entries.values() {
                    assert_no_json_arrays(child);
                }
            }
            _ => {}
        }
    }
}
