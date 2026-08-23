use std::io::{self, Write};

use serde::Serialize;
use thiserror::Error;

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
    formation_elevation_from_components, surface_formation_state_fingerprint, ClimateSpec,
    ClimateWorkDomainSnapshot, EvolvedTectonicSnapshot, FormationElevationComponents,
    FormationResiduals, FormationSedimentFields, FormationSolveReport, FormationTerrainFields,
    GeologicSubstrateSnapshot, GlobalCirculationSnapshot, HydroErosionSpec, NaturalQualityProfile,
    NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot, SedimentBudgetReport,
    SphericalHydrologySnapshot, SurfaceFormationCapabilitySet, SurfaceFormationCheckpoint,
    SurfaceFormationUpstreamFingerprints, SurfaceFormationValidationError, ELEVATION_MAX_M,
    ELEVATION_MIN_M, FORMATION_TERRAIN_FIELDS_SCHEMA_V2, NATURAL_SURFACE_FORMATION_SCHEMA_V2,
    SEDIMENT_PROVENANCE_SOURCE_COUNT, SURFACE_FORMATION_MACRO_STEPS,
    SURFACE_FORMATION_MACRO_STEP_YEARS, SURFACE_FORMATION_MAX_OUTER_ITERATIONS,
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

/// Bounded climate-surface fixed point over the eight-macro-step solve.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceFormationGenerator;

impl SurfaceFormationGenerator {
    /// Runs the locked P5 solve and publishes one atomic formation product.
    pub fn generate(
        inputs: SurfaceFormationInputs<'_>,
        cancellation: &BuildCancellation,
    ) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError> {
        Self::generate_with_outer_iteration_limit(
            inputs,
            SURFACE_FORMATION_MAX_OUTER_ITERATIONS,
            cancellation,
        )
    }

    /// Same solve with a reduced outer-iteration budget.
    ///
    /// The budget can only be lowered, never raised past the locked maximum, so
    /// a caller can only make the fixed point fail: a non-converged candidate is
    /// still never published.
    pub fn generate_with_outer_iteration_limit(
        inputs: SurfaceFormationInputs<'_>,
        outer_iteration_limit: u8,
        cancellation: &BuildCancellation,
    ) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError> {
        if outer_iteration_limit == 0
            || outer_iteration_limit > SURFACE_FORMATION_MAX_OUTER_ITERATIONS
        {
            return Err(SurfaceFormationGenerationError::InvalidIterationLimit {
                found: outer_iteration_limit,
                maximum: SURFACE_FORMATION_MAX_OUTER_ITERATIONS,
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
        let mut previous_terrain = primary_relief_terrain(inputs)?;
        let mut previous_hydrology = FormationHydrologyGenerator::generate_from_validated(
            surface,
            &previous_terrain,
            inputs.substrate,
            &climate,
            inputs.formation_spec,
            cancellation,
        )?;
        let mut workspace = HillslopeWorkspace::default();
        let mut residuals = Vec::with_capacity(outer_iteration_limit as usize);

        for _ in 0..outer_iteration_limit {
            check_cancelled(cancellation)?;
            let solved = solve_geomorphic(inputs, &climate, &mut workspace, cancellation)?;
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
            let residual = residuals_between(
                &areas,
                total_area_m2,
                &previous_terrain,
                &previous_hydrology,
                &solved.terrain,
                &candidate_hydrology,
                cancellation,
            )?;
            residuals.push(residual);
            if std::env::var_os("SEKAI_P5_TRACE").is_some() {
                eprintln!(
                    "[p5-fp] iter {} elev_rms {:.3} m recv {:.5} logq {:.5} sed_rms {:.3} m coast {:.6} -> normalized_max {:.4}",
                    residuals.len(),
                    residual.elevation_rms_m(),
                    residual.receiver_changed_fraction(),
                    residual.log_discharge_rms(),
                    residual.sediment_thickness_rms_m(),
                    residual.coastline_area_changed_fraction(),
                    residual.normalized_max()
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
                    residuals,
                    dense_state_bytes,
                    cancellation,
                );
            }
            previous_terrain = solved.terrain;
            previous_hydrology = candidate_hydrology;
            climate = candidate_climate;
        }

        let last = *residuals
            .last()
            .expect("the validated budget runs at least one outer iteration");
        Err(SurfaceFormationGenerationError::NotConverged {
            outer_iterations: residuals.len() as u8,
            residuals: last,
        })
    }
}

/// Terrain, sediment ledger, and cumulative budget of one geomorphic solve.
struct GeomorphicSolve {
    terrain: FormationTerrainFields,
    budget: SedimentBudgetReport,
}

/// Exact retained component state shared by every macro step.
struct ComponentState {
    primary_elevation_m: Vec<f32>,
    tectonic_displacement_m: Vec<f64>,
    fluvial_erosion_m: Vec<f64>,
    hillslope_erosion_m: Vec<f64>,
    hillslope_deposition_m: Vec<f64>,
    routed_sediment_deposition_m: Vec<f64>,
    coastal_erosion_m: Vec<f64>,
    coastal_deposition_m: Vec<f64>,
    isostatic_response_m: Vec<f64>,
    elevation_m: Vec<f32>,
}

impl ComponentState {
    fn from_primary(primary_elevation_m: Vec<f32>) -> Self {
        let count = primary_elevation_m.len();
        let elevation_m = primary_elevation_m.clone();
        Self {
            primary_elevation_m,
            tectonic_displacement_m: vec![0.0; count],
            fluvial_erosion_m: vec![0.0; count],
            hillslope_erosion_m: vec![0.0; count],
            hillslope_deposition_m: vec![0.0; count],
            routed_sediment_deposition_m: vec![0.0; count],
            coastal_erosion_m: vec![0.0; count],
            coastal_deposition_m: vec![0.0; count],
            isostatic_response_m: vec![0.0; count],
            elevation_m,
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
            let elevation = formation_elevation_from_components(
                self.primary_elevation_m[index],
                self.tectonic_displacement_m[index] as f32,
                self.fluvial_erosion_m[index] as f32,
                self.hillslope_erosion_m[index] as f32,
                self.hillslope_deposition_m[index] as f32,
                self.routed_sediment_deposition_m[index] as f32,
                self.coastal_erosion_m[index] as f32,
                self.coastal_deposition_m[index] as f32,
                self.isostatic_response_m[index] as f32,
            );
            if !elevation.is_finite() || !(ELEVATION_MIN_M..=ELEVATION_MAX_M).contains(&elevation) {
                return Err(SurfaceFormationGenerationError::ElevationOutOfRange {
                    cell: CellId::from_raw(index as u32),
                    found: elevation,
                });
            }
            self.elevation_m[index] = elevation;
        }
        Ok(())
    }

    fn components(&self) -> Result<FormationElevationComponents, SurfaceFormationValidationError> {
        FormationElevationComponents::new(
            self.primary_elevation_m.clone(),
            quantize(&self.tectonic_displacement_m),
            quantize(&self.fluvial_erosion_m),
            quantize(&self.hillslope_erosion_m),
            quantize(&self.hillslope_deposition_m),
            quantize(&self.routed_sediment_deposition_m),
            quantize(&self.coastal_erosion_m),
            quantize(&self.coastal_deposition_m),
            quantize(&self.isostatic_response_m),
            self.elevation_m.clone(),
        )
    }
}

fn quantize(values: &[f64]) -> Vec<f32> {
    values.iter().map(|&value| value as f32).collect()
}

fn accumulate(target: &mut [f64], increments: &[f32]) {
    for (slot, &increment) in target.iter_mut().zip(increments) {
        *slot += f64::from(increment);
    }
}

/// Cumulative five-source sediment ledger across every macro step.
#[derive(Debug, Clone, Copy, Default)]
struct SedimentBudgetAccumulator {
    produced_mass_kg: f64,
    land_lake_deposited_mass_kg: f64,
    shelf_deposited_mass_kg: f64,
    deep_ocean_delivery_mass_kg: f64,
    final_in_transit_mass_kg: f64,
    produced_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
    accounted_by_source_kg: [f64; SEDIMENT_PROVENANCE_SOURCE_COUNT],
}

impl SedimentBudgetAccumulator {
    fn add(&mut self, report: &SedimentBudgetReport) {
        self.produced_mass_kg += report.produced_mass_kg();
        self.land_lake_deposited_mass_kg += report.land_lake_deposited_mass_kg();
        self.shelf_deposited_mass_kg += report.shelf_deposited_mass_kg();
        self.deep_ocean_delivery_mass_kg += report.deep_ocean_delivery_mass_kg();
        self.final_in_transit_mass_kg += report.final_in_transit_mass_kg();
        for source in 0..SEDIMENT_PROVENANCE_SOURCE_COUNT {
            self.produced_by_source_kg[source] += report.produced_by_source_kg()[source];
            self.accounted_by_source_kg[source] += report.accounted_by_source_kg()[source];
        }
    }

    fn finish(self) -> Result<SedimentBudgetReport, SurfaceFormationValidationError> {
        SedimentBudgetReport::new(
            self.produced_mass_kg,
            self.land_lake_deposited_mass_kg,
            self.shelf_deposited_mass_kg,
            self.deep_ocean_delivery_mass_kg,
            self.final_in_transit_mass_kg,
            self.produced_by_source_kg,
            self.accounted_by_source_kg,
        )
    }
}

/// Runs the eight `12,500 yr` macro steps from the immutable P3 initial state.
fn solve_geomorphic(
    inputs: SurfaceFormationInputs<'_>,
    climate: &GlobalCirculationSnapshot,
    workspace: &mut HillslopeWorkspace,
    cancellation: &BuildCancellation,
) -> Result<GeomorphicSolve, SurfaceFormationGenerationError> {
    let surface = inputs.surface;
    let annual_precipitation_mm = annual_precipitation_mm(climate, cancellation)?;
    let mut state = ComponentState::from_primary(inputs.relief.elevation_m().to_vec());
    let mut terrain = primary_relief_terrain(inputs)?;
    let mut budget = SedimentBudgetAccumulator::default();
    let step_years = SURFACE_FORMATION_MACRO_STEP_YEARS;

    for _ in 0..SURFACE_FORMATION_MACRO_STEPS {
        check_cancelled(cancellation)?;
        let hydrology = FormationHydrologyGenerator::generate_from_validated(
            surface,
            &terrain,
            inputs.substrate,
            climate,
            inputs.formation_spec,
            cancellation,
        )?;

        let stream = ImplicitStreamPowerSolver::advance_from_validated_snapshots(
            surface,
            &state.elevation_m,
            &hydrology,
            inputs.tectonics,
            inputs.substrate,
            step_years,
            cancellation,
        )?;
        accumulate(
            &mut state.tectonic_displacement_m,
            stream.tectonic_displacement_m(),
        );
        accumulate(&mut state.fluvial_erosion_m, stream.fluvial_erosion_m());
        state.refresh_elevation(cancellation)?;

        let hillslope = NonlinearHillslopeTransport::advance_from_validated_surface(
            surface,
            HillslopeInputs {
                elevation_m: &state.elevation_m,
                surface_water: hydrology.surface_water(),
                substrate_erodibility: inputs.substrate.erodibility(),
                fracture_intensity: inputs.substrate.fracture_intensity(),
                annual_precipitation_mm: &annual_precipitation_mm,
                substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
                sediment_sources: inputs.substrate.sediment_sources(),
            },
            step_years,
            workspace,
            cancellation,
        )?;
        accumulate(
            &mut state.hillslope_erosion_m,
            hillslope.hillslope_erosion_m(),
        );
        accumulate(
            &mut state.hillslope_deposition_m,
            hillslope.hillslope_deposition_m(),
        );
        state.refresh_elevation(cancellation)?;

        let coast_water = FormationSeaLevelSolver::solve_from_validated_surface(
            surface,
            &state.elevation_m,
            inputs.relief.water_inventory_m3(),
            cancellation,
        )?;

        let coast = CoastalExchange::advance_from_validated_surface(
            surface,
            CoastalInputs {
                elevation_m: &state.elevation_m,
                surface_water_geometry: coast_water.geometry(),
                substrate_erodibility: inputs.substrate.erodibility(),
                sediment_thickness_m: terrain.sediment().sediment_thickness_m(),
                substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
                sediment_sources: inputs.substrate.sediment_sources(),
                near_surface_wind_m_s: climate.fields().near_surface_wind_m_s().values(),
                surface_ocean_current_m_s: climate.fields().surface_ocean_current_m_s().values(),
            },
            step_years,
            cancellation,
        )?;
        accumulate(&mut state.coastal_erosion_m, coast.coastal_erosion_m());
        state.refresh_elevation(cancellation)?;

        let sediment = ProvenanceSedimentRouter::route_from_validated_surface(
            surface,
            SedimentInputs {
                elevation_m: &state.elevation_m,
                sea_level_m: coast_water.sea_level_m(),
                surface_water: hydrology.surface_water(),
                flow_receiver: hydrology.flow_receiver(),
                drainage_surface_elevation_m: hydrology.drainage_surface_elevation_m().values(),
                lake_depth_m: hydrology.lake_depth_m(),
                mean_annual_discharge_m3_s: hydrology.mean_annual_discharge_m3_s(),
                fluvial_erosion_m: stream.fluvial_erosion_m(),
                hillslope_removed_by_source_kg: hillslope.removed_by_source_kg(),
                hillslope_deposited_by_source_kg: hillslope.deposited_by_source_kg(),
                coastal_removed_by_source_kg: coast.removed_by_source_kg(),
                coastal_ocean_injection_by_source_kg: coast.ocean_injection_by_source_kg(),
                marine_exposure: coast.marine_exposure(),
                substrate_density_kg_m3: inputs.substrate.crust_density_kg_m3(),
                sediment_sources: inputs.substrate.sediment_sources(),
                previous_sediment_thickness_m: terrain.sediment().sediment_thickness_m(),
                previous_provenance_fraction: terrain.sediment().provenance_fraction(),
            },
            step_years,
            cancellation,
        )?;
        accumulate(
            &mut state.routed_sediment_deposition_m,
            sediment.routed_sediment_deposition_m(),
        );
        accumulate(
            &mut state.coastal_deposition_m,
            sediment.coastal_deposition_m(),
        );
        state.refresh_elevation(cancellation)?;
        budget.add(sediment.budget_report());

        let isostasy = LocalAiryIsostasy::apply_from_validated_surface(
            surface,
            &state.elevation_m,
            sediment.removed_mass_kg(),
            sediment.deposited_mass_kg(),
            cancellation,
        )?;
        accumulate(
            &mut state.isostatic_response_m,
            isostasy.isostatic_response_m(),
        );
        state.refresh_elevation(cancellation)?;

        let water = FormationSeaLevelSolver::solve_from_validated_surface(
            surface,
            &state.elevation_m,
            inputs.relief.water_inventory_m3(),
            cancellation,
        )?;
        terrain = FormationTerrainFields::new(
            FORMATION_TERRAIN_FIELDS_SCHEMA_V2,
            state.components()?,
            water.into_geometry(),
            inputs.relief.water_inventory_m3(),
            sediment.fields().clone(),
        )?;
    }

    Ok(GeomorphicSolve {
        terrain,
        budget: budget.finish()?,
    })
}

/// Builds the immutable P3 starting terrain with an empty sediment ledger.
fn primary_relief_terrain(
    inputs: SurfaceFormationInputs<'_>,
) -> Result<FormationTerrainFields, SurfaceFormationGenerationError> {
    let count = inputs.relief.elevation_m().len();
    let primary = inputs.relief.elevation_m().to_vec();
    let zero_f32 = vec![0.0_f32; count];
    let components = FormationElevationComponents::new(
        primary.clone(),
        zero_f32.clone(),
        zero_f32.clone(),
        zero_f32.clone(),
        zero_f32.clone(),
        zero_f32.clone(),
        zero_f32.clone(),
        zero_f32.clone(),
        zero_f32.clone(),
        primary,
    )?;
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
        FORMATION_TERRAIN_FIELDS_SCHEMA_V2,
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

#[allow(clippy::too_many_arguments)]
fn residuals_between(
    areas: &[f64],
    total_area_m2: f64,
    previous_terrain: &FormationTerrainFields,
    previous_hydrology: &SphericalHydrologySnapshot,
    candidate_terrain: &FormationTerrainFields,
    candidate_hydrology: &SphericalHydrologySnapshot,
    cancellation: &BuildCancellation,
) -> Result<FormationResiduals, SurfaceFormationGenerationError> {
    let previous_elevation = previous_terrain.final_elevation_m();
    let candidate_elevation = candidate_terrain.final_elevation_m();
    let previous_sediment = previous_terrain.sediment().sediment_thickness_m();
    let candidate_sediment = candidate_terrain.sediment().sediment_thickness_m();
    let previous_receiver = previous_hydrology.flow_receiver();
    let candidate_receiver = candidate_hydrology.flow_receiver();
    let previous_discharge = previous_hydrology.mean_annual_discharge_m3_s();
    let candidate_discharge = candidate_hydrology.mean_annual_discharge_m3_s();
    let previous_land = previous_terrain.land_ocean().raw_values();
    let candidate_land = candidate_terrain.land_ocean().raw_values();

    let mut elevation_square_sum = 0.0_f64;
    let mut sediment_square_sum = 0.0_f64;
    let mut discharge_square_sum = 0.0_f64;
    let mut receiver_changed_area = 0.0_f64;
    let mut coastline_changed_area = 0.0_f64;
    for (index, &area) in areas.iter().enumerate() {
        poll_cancelled(cancellation, index)?;
        let elevation_delta =
            f64::from(candidate_elevation[index]) - f64::from(previous_elevation[index]);
        elevation_square_sum += area * elevation_delta * elevation_delta;
        let sediment_delta =
            f64::from(candidate_sediment[index]) - f64::from(previous_sediment[index]);
        sediment_square_sum += area * sediment_delta * sediment_delta;
        let discharge_delta = f64::from(candidate_discharge[index]).ln_1p()
            - f64::from(previous_discharge[index]).ln_1p();
        discharge_square_sum += area * discharge_delta * discharge_delta;
        if candidate_receiver[index] != previous_receiver[index] {
            receiver_changed_area += area;
        }
        if candidate_land[index] != previous_land[index] {
            coastline_changed_area += area;
        }
    }

    Ok(FormationResiduals::new(
        (elevation_square_sum / total_area_m2).sqrt(),
        (receiver_changed_area / total_area_m2).clamp(0.0, 1.0),
        (discharge_square_sum / total_area_m2).sqrt(),
        (sediment_square_sum / total_area_m2).sqrt(),
        (coastline_changed_area / total_area_m2).clamp(0.0, 1.0),
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
    residuals: Vec<FormationResiduals>,
    dense_state_bytes: u64,
    cancellation: &BuildCancellation,
) -> Result<NaturalSurfaceFormationSnapshot, SurfaceFormationGenerationError> {
    check_cancelled(cancellation)?;
    let solve_report = FormationSolveReport::new(residuals, dense_state_bytes)?;
    let outer_iterations = solve_report.outer_iterations();
    let state_fingerprint =
        surface_formation_state_fingerprint(&solved.terrain, &hydrology, &climate);
    let checkpoint = SurfaceFormationCheckpoint::new(
        surface_ref,
        quality_profile,
        upstream,
        outer_iterations,
        state_fingerprint,
    )?;
    check_cancelled(cancellation)?;
    let snapshot = NaturalSurfaceFormationSnapshot::new(
        NATURAL_SURFACE_FORMATION_SCHEMA_V2,
        surface_ref,
        checkpoint,
        solved.terrain,
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
    /// The requested outer-iteration budget is outside the locked bound.
    #[error("outer iteration limit {found} is outside 1..={maximum}")]
    InvalidIterationLimit { found: u8, maximum: u8 },
    /// The bounded fixed point did not close within its iteration budget.
    ///
    /// Carries the final residual vector (spec §6: the typed failure
    /// carries the best report), so the panel names the failing
    /// component instead of one opaque number.
    #[error(
        "formation fixed point did not converge in {outer_iterations} outer iterations \
         (normalized residual {:.4}: elevation_rms {:.2} m, receiver_changed {:.5}, \
         log_discharge_rms {:.4}, sediment_rms {:.2} m, coastline_changed {:.6})",
        residuals.normalized_max(),
        residuals.elevation_rms_m(),
        residuals.receiver_changed_fraction(),
        residuals.log_discharge_rms(),
        residuals.sediment_thickness_rms_m(),
        residuals.coastline_area_changed_fraction()
    )]
    NotConverged {
        outer_iterations: u8,
        residuals: FormationResiduals,
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
    ElevationOutOfRange { cell: CellId, found: f32 },
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
