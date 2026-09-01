use std::io::{self, Write};

use serde::Serialize;
use thiserror::Error;

use super::sediment::split_mass_by_weights;
use super::{
    CoastGenerationError, CoastalExchange, CoastalInputs, FormationHydrologyGenerationError,
    FormationHydrologyGenerator, FormationState, FormationStateError, HillslopeGenerationError,
    HillslopeInputs, HillslopeWorkspace, ImplicitStreamPowerSolver, IsostasyGenerationError,
    LocalAiryIsostasy, NonlinearHillslopeTransport, ProvenanceSedimentRouter,
    SedimentGenerationError, SedimentInputs, StreamPowerGenerationError,
};
use crate::engine::BuildCancellation;
use crate::generators::natural::formation::global_circulation::{
    GlobalCirculationGenerationError, GlobalCirculationGenerator, GlobalClimateForcing,
    GlobalClimateForcingBuilder, GlobalClimateForcingError,
};
use crate::generators::natural::surface_water_geometry::solve_physical_sea_level_exact;
use crate::world::natural::{
    expected_surface_formation_dense_state_bytes, formation_annual_precipitation_mm,
    formation_elevation_from_components, formation_relative_flux_imbalance,
    surface_formation_state_fingerprint, ClimateSpec, ClimateWorkDomainSnapshot,
    EvolvedTectonicSnapshot, FormationEvolutionReport, FormationProcessRates, FormationResiduals,
    FormationSedimentFields, FormationTerrainFields, GeologicSubstrateSnapshot,
    GlobalCirculationSnapshot, HydroErosionSpec, NaturalQualityProfile,
    NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot, SedimentBudgetReport,
    SphericalHydrologySnapshot, SurfaceFormationCapabilitySet, SurfaceFormationCheckpoint,
    SurfaceFormationUpstreamFingerprints, SurfaceFormationValidationError, WaterVolumeSolveError,
    ELEVATION_MAX_M, ELEVATION_MIN_M, FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    FORMATION_DETACHMENT_LIMITED_EFFECTIVE_SETTLING_VELOCITY_M_PER_YEAR,
    NATURAL_SURFACE_FORMATION_SCHEMA_V5, SEDIMENT_PROVENANCE_SOURCE_COUNT,
    SURFACE_FORMATION_HORIZON_YEARS,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef, SurfaceRefError};
use crate::world::CellId;

const CANCELLATION_POLL_MASK: usize = 255;
const FINGERPRINT_POLL_BYTES: usize = 64 * 1024;

/// Complete authoritative input set of one finite-time formation build.
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

/// Finite-time surface formation followed by one endpoint climate closure.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceFormationGenerator;

/// Final P5 snapshot plus the endpoint P4 sibling and forcing used to close it.
#[derive(Debug)]
pub(in crate::generators::natural) struct SurfaceFormationClosureOutput {
    surface: NaturalSurfaceFormationSnapshot,
    final_climate: GlobalCirculationSnapshot,
    final_climate_forcing: GlobalClimateForcing,
}

impl SurfaceFormationClosureOutput {
    /// Moves the closed P5 snapshot, sibling P4, and forcing to the outer coordinator.
    pub(in crate::generators::natural) fn into_parts(
        self,
    ) -> (
        NaturalSurfaceFormationSnapshot,
        GlobalCirculationSnapshot,
        GlobalClimateForcing,
    ) {
        (self.surface, self.final_climate, self.final_climate_forcing)
    }
}

impl SurfaceFormationGenerator {
    /// Closes one exact P3-derived state through finite-time P5 and endpoint P4.
    pub(in crate::generators::natural) fn generate_from_exact_state(
        inputs: SurfaceFormationInputs<'_>,
        state: FormationState,
        cancellation: &BuildCancellation,
    ) -> Result<SurfaceFormationClosureOutput, SurfaceFormationGenerationError> {
        let surface_ref = validate_inputs(inputs, cancellation)?;
        Self::generate_from_validated_state(inputs, state, surface_ref, cancellation)
    }

    fn generate_from_validated_state(
        inputs: SurfaceFormationInputs<'_>,
        mut state: FormationState,
        surface_ref: SurfaceRef,
        cancellation: &BuildCancellation,
    ) -> Result<SurfaceFormationClosureOutput, SurfaceFormationGenerationError> {
        let surface = inputs.surface;
        let start_process_inputs =
            SurfaceProcessInputs::from_generation(inputs, inputs.initial_climate);
        let advance_report = advance_surface_processes(
            &mut state,
            start_process_inputs,
            SURFACE_FORMATION_HORIZON_YEARS,
            cancellation,
        )?;
        let (advance_summary, final_sediment_fields) = advance_report.into_parts();
        let final_terrain =
            state.project_final_terrain(surface, final_sediment_fields, cancellation)?;
        let forcing = GlobalClimateForcingBuilder::build_for_formation_terrain(
            surface,
            &final_terrain,
            inputs.climate_spec,
            inputs.domain,
            cancellation,
        )?;
        let endpoint_climate = GlobalCirculationGenerator::generate(
            surface,
            inputs.domain,
            &forcing,
            inputs.initial_climate.profile(),
            cancellation,
        )?;
        let terminal_diagnostics = recompute_surface_diagnostics(
            &state,
            SurfaceProcessInputs::from_generation(inputs, &endpoint_climate),
            cancellation,
        )?;
        let evolution_report = build_evolution_report(
            surface,
            &state,
            advance_summary,
            &terminal_diagnostics,
            cancellation,
        )?;
        let upstream = upstream_fingerprints(inputs, &endpoint_climate, cancellation)?;
        let endpoint_checkpoint_fingerprint = *endpoint_climate.checkpoint().fingerprint();
        let surface = finalize_surface_formation(
            state,
            final_terrain,
            surface,
            surface_ref,
            inputs.quality_profile,
            endpoint_checkpoint_fingerprint,
            upstream,
            terminal_diagnostics,
            evolution_report,
            cancellation,
        )?;
        Ok(SurfaceFormationClosureOutput {
            surface,
            final_climate: endpoint_climate,
            final_climate_forcing: forcing,
        })
    }
}

#[derive(Debug, Clone, Copy)]
/// Borrowed physical inputs held fixed during one P5 advance window.
pub(in crate::generators::natural) struct SurfaceProcessInputs<'a> {
    pub surface: &'a SphericalSurfaceSnapshot,
    pub tectonics: &'a EvolvedTectonicSnapshot,
    pub substrate: &'a GeologicSubstrateSnapshot,
    pub climate: &'a GlobalCirculationSnapshot,
    pub formation_spec: &'a HydroErosionSpec,
    pub water_inventory_m3: f64,
}

impl<'a> SurfaceProcessInputs<'a> {
    fn from_generation(
        inputs: SurfaceFormationInputs<'a>,
        climate: &'a GlobalCirculationSnapshot,
    ) -> Self {
        Self {
            surface: inputs.surface,
            tectonics: inputs.tectonics,
            substrate: inputs.substrate,
            climate,
            formation_spec: inputs.formation_spec,
            water_inventory_m3: inputs.relief.water_inventory_m3(),
        }
    }
}

/// Private work report returned after a complete finite-time P5 advance.
#[derive(Debug)]
pub(in crate::generators::natural) struct SurfaceAdvanceReport {
    summary: SurfaceAdvanceSummary,
    final_sediment_fields: FormationSedimentFields,
}

impl SurfaceAdvanceReport {
    /// Moves the accepted work summary and matching final sediment projection data.
    pub(in crate::generators::natural) fn into_parts(
        self,
    ) -> (SurfaceAdvanceSummary, FormationSedimentFields) {
        (self.summary, self.final_sediment_fields)
    }
}

/// Copyable accepted-work identity retained while the final sediment fields move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::generators::natural) struct SurfaceAdvanceSummary {
    accepted_surface_substeps: u32,
    accepted_duration_years: f64,
}

impl SurfaceAdvanceSummary {
    /// Returns the number of accepted stable surface substeps.
    pub(in crate::generators::natural) const fn accepted_surface_substeps(self) -> u32 {
        self.accepted_surface_substeps
    }

    /// Returns the exact physical duration consumed by this advance.
    pub(in crate::generators::natural) const fn accepted_duration_years(self) -> f64 {
        self.accepted_duration_years
    }

    #[cfg(test)]
    pub(in crate::generators::natural) fn combined(
        self,
        other: Self,
        accepted_duration_years: f64,
    ) -> Result<Self, SurfaceFormationGenerationError> {
        let accepted_surface_substeps = self
            .accepted_surface_substeps
            .checked_add(other.accepted_surface_substeps)
            .ok_or(SurfaceFormationGenerationError::SurfaceAdvanceSubstepOverflow)?;
        if !accepted_duration_years.is_finite() || accepted_duration_years <= 0.0 {
            return Err(SurfaceFormationGenerationError::InvalidSurfaceDuration {
                found: accepted_duration_years,
            });
        }
        Ok(Self {
            accepted_surface_substeps,
            accepted_duration_years,
        })
    }
}

/// Endpoint hydrology, rates, and sediment budget evaluated without advancing time.
#[derive(Debug)]
pub(in crate::generators::natural) struct TerminalSurfaceDiagnostics {
    process_rates: ExactFormationProcessRates,
    hydrology: SphericalHydrologySnapshot,
    budget: SedimentBudgetReport,
    sediment_stock_change_kg_per_year: f64,
}

struct CurrentProcessEvaluation {
    process_rates: ExactFormationProcessRates,
    hydrology: SphericalHydrologySnapshot,
    sediment_fields: FormationSedimentFields,
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

struct FluvialCoverRemoval {
    removed_stock_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
    removed_by_source_kg: Vec<[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]>,
}

fn remove_fluvial_sediment_cover(
    surface: &SphericalSurfaceSnapshot,
    sediment_mass_by_source_kg: &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    substrate: &GeologicSubstrateSnapshot,
    fluvial_erosion_m: &[f64],
    cancellation: &BuildCancellation,
) -> Result<FluvialCoverRemoval, SurfaceFormationGenerationError> {
    let count = surface.cells().len();
    let mut removed_stock_by_source_kg = Vec::with_capacity(count);
    let mut removed_by_source_kg = Vec::with_capacity(count);
    for (index, &fluvial_erosion_m) in fluvial_erosion_m.iter().enumerate().take(count) {
        poll_cancelled(cancellation, index)?;
        let erosion_m = fluvial_erosion_m;
        let area_m2 = surface.cells()[index].area.get();
        let stock_mass_kg = sediment_mass_by_source_kg[index].iter().sum::<f64>();
        let stock_thickness_m = stock_mass_kg / (area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3);
        let (stock_erosion_m, removed_stock_kg) = if erosion_m >= stock_thickness_m {
            (stock_thickness_m, stock_mass_kg)
        } else {
            (
                erosion_m,
                erosion_m * area_m2 * FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
            )
        };
        let removed_stock_by_source =
            split_mass_by_weights(removed_stock_kg, sediment_mass_by_source_kg[index]);
        let mut by_source = removed_stock_by_source;
        let substrate_erosion_m = erosion_m - stock_erosion_m;
        let substrate_source = substrate
            .sediment_sources()
            .get(index)
            .expect("validated substrate covers every cell")
            .raw() as usize;
        by_source[substrate_source] +=
            substrate_erosion_m * area_m2 * f64::from(substrate.crust_density_kg_m3()[index]);
        removed_stock_by_source_kg.push(removed_stock_by_source);
        removed_by_source_kg.push(by_source);
    }
    Ok(FluvialCoverRemoval {
        removed_stock_by_source_kg,
        removed_by_source_kg,
    })
}

fn sum_sediment_stock_removal(
    fluvial_kg: &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    hillslope_kg: &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    coastal_kg: &[[f64; SEDIMENT_PROVENANCE_SOURCE_COUNT]],
    cancellation: &BuildCancellation,
) -> Result<Vec<f64>, SurfaceFormationGenerationError> {
    let mut total = Vec::with_capacity(fluvial_kg.len());
    for index in 0..fluvial_kg.len() {
        poll_cancelled(cancellation, index)?;
        total.push(
            fluvial_kg[index]
                .iter()
                .chain(&hillslope_kg[index])
                .chain(&coastal_kg[index])
                .sum(),
        );
    }
    Ok(total)
}

/// Advances the exact P5 state through the requested positive physical duration.
pub(in crate::generators::natural) fn advance_surface_processes(
    state: &mut FormationState,
    inputs: SurfaceProcessInputs<'_>,
    duration_years: f64,
    cancellation: &BuildCancellation,
) -> Result<SurfaceAdvanceReport, SurfaceFormationGenerationError> {
    if !duration_years.is_finite() || duration_years <= 0.0 {
        return Err(SurfaceFormationGenerationError::InvalidSurfaceDuration {
            found: duration_years,
        });
    }
    let base_tectonic_displacement_m = state.tectonic_displacement_m().to_vec();
    let mut remaining_years = duration_years;
    let mut accepted_surface_substeps = 0_u32;
    let mut final_sediment_fields = None;
    let mut workspace = HillslopeWorkspace::default();
    // Annualized rates of the previously accepted window, used as the domain
    // predictor for the next one. Before the first window there is no such
    // measurement; rather than pay for a full trial solve that has never bound
    // the step in any production corpus, the first attempt is optimistic and
    // the exact predictor is computed only if that attempt is actually
    // rejected (`probed` below).
    let mut predictor: Option<ExactFormationProcessRates> = None;

    while remaining_years > 0.0 {
        check_cancelled(cancellation)?;
        let mut step_years = remaining_years;
        if let Some(rates) = predictor.as_ref() {
            if let Some((cell, elevation_m, net_rate_m_per_year, boundary_m)) =
                blocked_by_elevation_domain(state.current_elevation_exact_m(), rates)
            {
                return Err(SurfaceFormationGenerationError::ElevationDomainExhausted {
                    cell,
                    elevation_m,
                    net_rate_m_per_year,
                    boundary_m,
                });
            }
            step_years = step_years.min(maximum_elevation_domain_step_years(
                state.current_elevation_exact_m(),
                rates,
            ));
        }
        if !step_years.is_finite() || step_years <= 0.0 {
            return Err(SurfaceFormationGenerationError::SurfaceAdvanceStalled { remaining_years });
        }
        let accepted_duration_before = duration_years - remaining_years;
        let mut probed = predictor.is_some();
        let mut candidate;
        let accepted;
        loop {
            candidate = state.clone();
            let context = SurfaceStepContext {
                inputs,
                base_tectonic_displacement_m: &base_tectonic_displacement_m,
                accepted_duration_before,
            };
            match advance_surface_window(
                &mut candidate,
                context,
                &mut step_years,
                &mut workspace,
                cancellation,
            ) {
                Ok(result) => {
                    accepted = result;
                    break;
                }
                Err(
                    SurfaceFormationGenerationError::ElevationOutOfRange { .. }
                    | SurfaceFormationGenerationError::StreamPower(
                        StreamPowerGenerationError::ElevationOutOfRange { .. },
                    )
                    | SurfaceFormationGenerationError::Isostasy(
                        IsostasyGenerationError::ElevationOutOfRange { .. },
                    ),
                ) => {
                    // Halving alone would need about a thousand full solves to
                    // walk down from the horizon, so the first rejection buys
                    // the exact domain limit once; later ones already have a
                    // measured predictor and only need to bisect.
                    step_years = if probed {
                        step_years * 0.5
                    } else {
                        probed = true;
                        let trial = evaluate_current_processes(
                            state,
                            inputs,
                            &mut workspace,
                            cancellation,
                        )?;
                        if let Some((cell, elevation_m, net_rate_m_per_year, boundary_m)) =
                            blocked_by_elevation_domain(
                                state.current_elevation_exact_m(),
                                &trial.process_rates,
                            )
                        {
                            return Err(
                                SurfaceFormationGenerationError::ElevationDomainExhausted {
                                    cell,
                                    elevation_m,
                                    net_rate_m_per_year,
                                    boundary_m,
                                },
                            );
                        }
                        maximum_elevation_domain_step_years(
                            state.current_elevation_exact_m(),
                            &trial.process_rates,
                        )
                        .min(step_years * 0.5)
                    };
                }
                Err(SurfaceFormationGenerationError::Hillslope(
                    HillslopeGenerationError::UnstableStep { maximum, .. },
                )) => {
                    step_years = step_years.min(maximum);
                }
                Err(error) => return Err(error),
            }
            if !step_years.is_finite() || step_years <= 0.0 {
                return Err(SurfaceFormationGenerationError::SurfaceAdvanceStalled {
                    remaining_years,
                });
            }
        }
        *state = candidate;
        final_sediment_fields = Some(accepted.sediment_fields);
        predictor = Some(accepted.process_rates);
        accepted_surface_substeps = accepted_surface_substeps
            .checked_add(1)
            .ok_or(SurfaceFormationGenerationError::SurfaceAdvanceSubstepOverflow)?;
        if step_years >= remaining_years {
            remaining_years = 0.0;
        } else {
            remaining_years -= step_years;
        }
    }

    Ok(SurfaceAdvanceReport {
        summary: SurfaceAdvanceSummary {
            accepted_surface_substeps,
            accepted_duration_years: duration_years,
        },
        final_sediment_fields: final_sediment_fields
            .expect("a positive completed duration accepts at least one surface substep"),
    })
}

/// Recomputes endpoint diagnostics on a clone while leaving the accepted state unchanged.
pub(in crate::generators::natural) fn recompute_surface_diagnostics(
    state: &FormationState,
    endpoint_inputs: SurfaceProcessInputs<'_>,
    cancellation: &BuildCancellation,
) -> Result<TerminalSurfaceDiagnostics, SurfaceFormationGenerationError> {
    let mut workspace = HillslopeWorkspace::default();
    let current = evaluate_current_processes(state, endpoint_inputs, &mut workspace, cancellation)?;
    Ok(TerminalSurfaceDiagnostics {
        process_rates: current.process_rates,
        hydrology: current.hydrology,
        budget: current.budget,
        sediment_stock_change_kg_per_year: current.sediment_stock_change_kg_per_year,
    })
}

/// Builds the published finite-time work report from endpoint diagnostics.
pub(in crate::generators::natural) fn build_evolution_report(
    surface: &SphericalSurfaceSnapshot,
    state: &FormationState,
    advance: SurfaceAdvanceSummary,
    terminal_diagnostics: &TerminalSurfaceDiagnostics,
    cancellation: &BuildCancellation,
) -> Result<FormationEvolutionReport, SurfaceFormationGenerationError> {
    let areas = cell_areas(surface, cancellation)?;
    let total_area_m2 = areas.iter().sum::<f64>();
    let current_rates = current_state_residuals(
        &areas,
        total_area_m2,
        state.current_elevation_exact_m(),
        &terminal_diagnostics.process_rates,
        terminal_diagnostics.sediment_stock_change_kg_per_year,
        terminal_diagnostics.budget.produced_mass_kg_per_year(),
        cancellation,
    )?;
    let dense_state_bytes = expected_surface_formation_dense_state_bytes(
        surface.cells().len() as u32,
        surface.edges().len() as u32,
    )
    .ok_or(SurfaceFormationGenerationError::AllocationOverflow)?;
    Ok(FormationEvolutionReport::new(
        advance.accepted_surface_substeps(),
        advance.accepted_duration_years(),
        current_rates,
        dense_state_bytes,
    )?)
}

fn evaluate_current_processes(
    state: &FormationState,
    inputs: SurfaceProcessInputs<'_>,
    workspace: &mut HillslopeWorkspace,
    cancellation: &BuildCancellation,
) -> Result<CurrentProcessEvaluation, SurfaceFormationGenerationError> {
    let base_tectonic_displacement_m = state.tectonic_displacement_m().to_vec();
    let mut trial = state.clone();
    let mut evaluation_step_years = 1.0;
    advance_surface_window(
        &mut trial,
        SurfaceStepContext {
            inputs,
            base_tectonic_displacement_m: &base_tectonic_displacement_m,
            accepted_duration_before: 0.0,
        },
        &mut evaluation_step_years,
        workspace,
        cancellation,
    )
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
    mean_m_per_year: f64,
}

fn surface_rate_statistics(
    areas: &[f64],
    total_area_m2: f64,
    rates: &ExactFormationProcessRates,
) -> SurfaceRateStatistics {
    let mut net_square_sum = 0.0_f64;
    let mut gross_square_sum = 0.0_f64;
    let mut net_sum = 0.0_f64;
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
    }
    let net_rms = (net_square_sum / total_area_m2).sqrt();
    let gross_rms_m_per_year = (gross_square_sum / total_area_m2).sqrt();
    SurfaceRateStatistics {
        net_rms_m_per_year: net_rms,
        gross_rms_m_per_year,
        mean_m_per_year: net_sum / total_area_m2,
    }
}

#[derive(Clone, Copy)]
struct SurfaceStepContext<'a> {
    inputs: SurfaceProcessInputs<'a>,
    base_tectonic_displacement_m: &'a [f64],
    accepted_duration_before: f64,
}

/// Advances one complete operator-split window on an unpublished exact candidate.
fn advance_surface_window(
    state: &mut FormationState,
    context: SurfaceStepContext<'_>,
    step_years: &mut f64,
    workspace: &mut HillslopeWorkspace,
    cancellation: &BuildCancellation,
) -> Result<CurrentProcessEvaluation, SurfaceFormationGenerationError> {
    check_cancelled(cancellation)?;
    let inputs = context.inputs;
    let surface = inputs.surface;
    let zero_sediment_transfer =
        vec![[0.0; SEDIMENT_PROVENANCE_SOURCE_COUNT]; surface.cells().len()];
    let annual_precipitation_mm = annual_precipitation_mm(inputs.climate, cancellation)?;
    let hydrology = FormationHydrologyGenerator::generate_from_validated_exact(
        surface,
        state.current_elevation_exact_m(),
        state.surface_water_geometry().land_ocean(),
        inputs.substrate,
        inputs.climate,
        inputs.formation_spec,
        cancellation,
    )?;
    let pre_step_hillslope_inputs = HillslopeInputs {
        elevation_m: state.current_elevation_exact_m(),
        surface_water: hydrology.surface_water(),
        substrate_erodibility: inputs.substrate.erodibility(),
        fracture_intensity: inputs.substrate.fracture_intensity(),
        annual_precipitation_mm: &annual_precipitation_mm,
        substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
        sediment_sources: inputs.substrate.sediment_sources(),
        sediment_mass_by_source_kg: state.sediment_stock().as_slice(),
    };
    let maximum_hillslope_step_years =
        NonlinearHillslopeTransport::maximum_stable_step_years_from_validated_surface(
            surface,
            pre_step_hillslope_inputs,
            workspace,
            cancellation,
        )?;
    *step_years = (*step_years).min(maximum_hillslope_step_years);

    let target_duration_years = context.accepted_duration_before + *step_years;
    let uplift_rate_mm_per_year = inputs.tectonics.forcing().uplift_rate_mm_per_year();
    let subsidence_rate_mm_per_year = inputs.tectonics.forcing().subsidence_rate_mm_per_year();
    let mut tectonic_target_m = Vec::with_capacity(context.base_tectonic_displacement_m.len());
    for (index, &base_displacement_m) in context.base_tectonic_displacement_m.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        tectonic_target_m.push(held_tectonic_displacement_target_m(
            base_displacement_m,
            uplift_rate_mm_per_year[index],
            subsidence_rate_mm_per_year[index],
            target_duration_years,
        )?);
    }
    let tectonic_displacement_m = tectonic_target_m
        .iter()
        .zip(state.tectonic_displacement_m())
        .map(|(&target_m, &current_m)| target_m - current_m)
        .collect::<Vec<_>>();

    let stream = ImplicitStreamPowerSolver::advance_from_validated_snapshots(
        surface,
        state.current_elevation_exact_m(),
        &hydrology,
        inputs.tectonics,
        inputs.substrate,
        *step_years,
        cancellation,
    )?;
    state.replace_tectonic_displacement_f64(&tectonic_target_m)?;
    state.apply_fluvial_erosion_f64(stream.fluvial_erosion_m())?;

    let fluvial_cover = remove_fluvial_sediment_cover(
        surface,
        state.sediment_stock().as_slice(),
        inputs.substrate,
        stream.fluvial_erosion_m(),
        cancellation,
    )?;
    state
        .sediment_stock_mut()
        .apply_transfer(
            &fluvial_cover.removed_stock_by_source_kg,
            &zero_sediment_transfer,
        )
        .map_err(
            |error| SurfaceFormationGenerationError::InvalidFormationState {
                reason: format!("fluvial cover removal: {error}"),
            },
        )?;

    let hillslope = NonlinearHillslopeTransport::advance_from_validated_surface(
        surface,
        HillslopeInputs {
            elevation_m: state.current_elevation_exact_m(),
            surface_water: hydrology.surface_water(),
            substrate_erodibility: inputs.substrate.erodibility(),
            fracture_intensity: inputs.substrate.fracture_intensity(),
            annual_precipitation_mm: &annual_precipitation_mm,
            substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
            sediment_sources: inputs.substrate.sediment_sources(),
            sediment_mass_by_source_kg: state.sediment_stock().as_slice(),
        },
        *step_years,
        workspace,
        cancellation,
    )?;
    state.apply_hillslope_erosion_f64(hillslope.hillslope_erosion_m())?;
    state.apply_hillslope_deposition_f64(hillslope.hillslope_deposition_m())?;
    state
        .sediment_stock_mut()
        .apply_transfer(
            hillslope.sediment_stock_removed_by_source_kg(),
            &zero_sediment_transfer,
        )
        .map_err(
            |error| SurfaceFormationGenerationError::InvalidFormationState {
                reason: format!("hillslope cover removal: {error}"),
            },
        )?;

    let coast_water = solve_physical_sea_level_exact(
        surface,
        state.current_elevation_exact_m(),
        inputs.water_inventory_m3,
        cancellation,
    )?
    .into_geometry();
    state.replace_surface_water_geometry(coast_water);
    let coast = CoastalExchange::advance_from_validated_surface(
        surface,
        CoastalInputs {
            elevation_m: state.current_elevation_exact_m(),
            ocean_area_fraction: state.surface_water_geometry().ocean_area_fraction(),
            wet_edge_fraction: state.surface_water_geometry().wet_edge_fraction(),
            substrate_erodibility: inputs.substrate.erodibility(),
            sediment_mass_by_source_kg: state.sediment_stock().as_slice(),
            substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
            sediment_sources: inputs.substrate.sediment_sources(),
            near_surface_wind_m_s: inputs.climate.fields().near_surface_wind_m_s().values(),
            surface_ocean_current_m_s: inputs.climate.fields().surface_ocean_current_m_s().values(),
        },
        *step_years,
        cancellation,
    )?;
    state.apply_coastal_erosion_f64(coast.coastal_erosion_m())?;
    state
        .sediment_stock_mut()
        .apply_transfer(
            coast.sediment_stock_removed_by_source_kg(),
            &zero_sediment_transfer,
        )
        .map_err(
            |error| SurfaceFormationGenerationError::InvalidFormationState {
                reason: format!("coastal cover removal: {error}"),
            },
        )?;

    let sediment_stock_removed_kg = sum_sediment_stock_removal(
        &fluvial_cover.removed_stock_by_source_kg,
        hillslope.sediment_stock_removed_by_source_kg(),
        coast.sediment_stock_removed_by_source_kg(),
        cancellation,
    )?;
    let sediment = ProvenanceSedimentRouter::route_from_validated_surface(
        surface,
        SedimentInputs {
            elevation_m: state.current_elevation_exact_m(),
            sea_level_m: state.surface_water_geometry().sea_level_m(),
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
            retained_sediment_mass_by_source_kg: state.sediment_stock().as_slice(),
        },
        *step_years,
        cancellation,
    )?;
    state.apply_routed_sediment_deposition_f64(sediment.routed_sediment_deposition_m())?;
    state.apply_coastal_deposition_f64(sediment.coastal_deposition_m())?;
    state
        .sediment_stock_mut()
        .apply_transfer(&zero_sediment_transfer, sediment.deposited_by_source_kg())
        .map_err(
            |error| SurfaceFormationGenerationError::InvalidFormationState {
                reason: format!("routed sediment deposition: {error}"),
            },
        )?;
    let budget = *sediment.budget_report();
    let sediment_stock_change_kg_per_year = sediment_stock_change_kg_per_year(
        sediment.deposited_mass_kg(),
        &sediment_stock_removed_kg,
        *step_years,
    );

    let isostatic_response_m = apply_local_airy_response(
        surface,
        state,
        sediment.removed_mass_kg(),
        sediment.deposited_mass_kg(),
        cancellation,
    )?;

    let water = solve_physical_sea_level_exact(
        surface,
        state.current_elevation_exact_m(),
        inputs.water_inventory_m3,
        cancellation,
    )?
    .into_geometry();
    state.replace_surface_water_geometry(water);
    Ok(CurrentProcessEvaluation {
        process_rates: ExactFormationProcessRates::annualized(
            ProcessDisplacements {
                tectonic_displacement_m: &tectonic_displacement_m,
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
        hydrology,
        sediment_fields: sediment.fields().clone(),
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

fn held_tectonic_displacement_target_m(
    base_displacement_m: f64,
    uplift_rate_mm_per_year: f32,
    subsidence_rate_mm_per_year: f32,
    accepted_duration_years: f64,
) -> Result<f64, SurfaceFormationGenerationError> {
    let target_m = base_displacement_m
        + (f64::from(uplift_rate_mm_per_year) - f64::from(subsidence_rate_mm_per_year)) / 1_000.0
            * accepted_duration_years;
    if !target_m.is_finite() {
        return Err(SurfaceFormationGenerationError::InvalidFormationState {
            reason: "held tectonic forcing produced a non-finite displacement".to_owned(),
        });
    }
    Ok(target_m)
}

fn quantize_f64(values: &[f64]) -> Vec<f32> {
    values.iter().map(|&value| value as f32).collect()
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

/// Atomically validates and assembles one projected final P5 snapshot.
#[allow(clippy::too_many_arguments)]
pub(in crate::generators::natural) fn finalize_surface_formation(
    state: FormationState,
    final_terrain: FormationTerrainFields,
    surface: &SphericalSurfaceSnapshot,
    surface_ref: SurfaceRef,
    quality_profile: NaturalQualityProfile,
    formation_climate_checkpoint_fingerprint: [u8; 32],
    upstream: SurfaceFormationUpstreamFingerprints,
    terminal_diagnostics: TerminalSurfaceDiagnostics,
    evolution_report: FormationEvolutionReport,
    cancellation: &BuildCancellation,
) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError> {
    check_cancelled(cancellation)?;
    if state.current_elevation_exact_m().len() != final_terrain.current_elevation_m().len()
        || state
            .surface_water_geometry()
            .total_water_volume_m3()
            .to_bits()
            != final_terrain.water_inventory_m3().to_bits()
    {
        return Err(SurfaceFormationGenerationError::InvalidFormationState {
            reason: "final terrain does not match the accepted exact state".to_owned(),
        });
    }
    if upstream.formation_climate_checkpoint_fingerprint()
        != &formation_climate_checkpoint_fingerprint
    {
        return Err(SurfaceFormationGenerationError::InvalidFormationState {
            reason: "endpoint climate checkpoint does not match final P5 upstream identity"
                .to_owned(),
        });
    }
    let process_rates = terminal_diagnostics.process_rates.to_wire()?;
    let state_fingerprint = surface_formation_state_fingerprint(
        &final_terrain,
        &process_rates,
        &terminal_diagnostics.hydrology,
    );
    let checkpoint =
        SurfaceFormationCheckpoint::new(surface_ref, quality_profile, upstream, state_fingerprint)?;
    check_cancelled(cancellation)?;
    let snapshot = NaturalSurfaceFormationSnapshot::new(
        NATURAL_SURFACE_FORMATION_SCHEMA_V5,
        surface_ref,
        checkpoint,
        final_terrain,
        process_rates,
        terminal_diagnostics.hydrology,
        evolution_report,
        terminal_diagnostics.budget,
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

/// Hashes the exact P2/P3/P4/P5 inputs retained by the final checkpoint.
pub(in crate::generators::natural) fn upstream_fingerprints(
    inputs: SurfaceFormationInputs<'_>,
    formation_climate: &GlobalCirculationSnapshot,
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
        *formation_climate.checkpoint().fingerprint(),
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

/// Failures from a complete finite-time surface-formation build.
#[derive(Debug, Error)]
pub enum SurfaceFormationGenerationError {
    /// Cooperative cancellation interrupted the build before publication.
    #[error("surface formation build cancelled")]
    Cancelled,
    /// The requested finite physical duration is invalid.
    #[error("surface formation duration must be positive and finite, found {found}")]
    InvalidSurfaceDuration { found: f64 },
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
    /// A nonzero current flux points out of the representable elevation domain.
    #[error(
        "surface advance exhausted the elevation domain at {cell:?}: elevation \
         {elevation_m} m has net rate {net_rate_m_per_year} m/yr toward {boundary_m} m"
    )]
    ElevationDomainExhausted {
        cell: CellId,
        elevation_m: f64,
        net_rate_m_per_year: f64,
        boundary_m: f64,
    },
    /// Stable step selection could not make positive progress.
    #[error("surface formation advance stalled with {remaining_years} years remaining")]
    SurfaceAdvanceStalled { remaining_years: f64 },
    /// The accepted substep counter overflowed before the requested duration completed.
    #[error("surface formation accepted substep counter overflowed")]
    SurfaceAdvanceSubstepOverflow,
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
    use super::super::state::formation_state_for_value;
    use super::{
        apply_local_airy_response, held_tectonic_displacement_target_m,
        sediment_stock_change_kg_per_year, ExactFormationProcessRates, ProcessDisplacements,
        FORMATION_ALLUVIAL_BULK_DENSITY_KG_M3,
    };
    use crate::engine::BuildCancellation;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        formation_elevation_from_components, ELEVATION_MAX_M, FORMATION_AIRY_MANTLE_DENSITY_KG_M3,
    };
    use crate::world::{Meters, SphericalSpaceSpec};

    #[test]
    fn held_tectonic_rate_is_integrated_once_from_the_entry_state() {
        let target = held_tectonic_displacement_target_m(3.0, 1.0, 0.0, 1_000.0)
            .expect("finite held forcing should integrate");

        assert_eq!(target.to_bits(), 4.0_f64.to_bits());
    }

    #[test]
    fn exact_process_rates_retain_values_below_the_f32_wire_ulp() {
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

    #[test]
    fn local_airy_preserves_exact_erosion_and_response_below_wire_ulp() {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(10_000.0).expect("test radius is positive"),
            target_cell_count: 42,
        })
        .expect("the production spherical fixture should build");
        let count = surface.cells().len();
        let area_m2 = surface.cells()[0].area.get();
        let airy_response_m = 0.000_260_834_617_f64;
        let eroded_density_kg_m3 = 2_827.0_f64;
        let eroded_thickness_m =
            airy_response_m * FORMATION_AIRY_MANTLE_DENSITY_KG_M3 / eroded_density_kg_m3;
        let mut components = formation_state_for_value(f64::from(ELEVATION_MAX_M));
        let mut erosion_m = vec![0.0_f64; count];
        erosion_m[0] = eroded_thickness_m;
        components
            .apply_fluvial_erosion_f64(&erosion_m)
            .expect("the exact erosion candidate should remain in range");

        let mut removed_mass_kg = vec![0.0_f64; count];
        removed_mass_kg[0] = FORMATION_AIRY_MANTLE_DENSITY_KG_M3 * area_m2 * airy_response_m;
        let response = apply_local_airy_response(
            &surface,
            &mut components,
            &removed_mass_kg,
            &vec![0.0; count],
            &BuildCancellation::new(),
        )
        .expect("the exact Airy response should remain in range");

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
}
