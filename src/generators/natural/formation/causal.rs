use thiserror::Error;

use super::super::random::LabeledSubstreams;
use super::super::{
    EvolvedTectonicGenerationError, EvolvedTectonicGenerator, GeologicSubstrateGenerationError,
    GeologicSubstrateGenerator, GlobalCirculationGenerationError, GlobalCirculationGenerator,
    GlobalClimateForcing, GlobalClimateForcingBuilder, GlobalClimateForcingError,
    PrimaryReliefGenerationError, PrimaryReliefGenerator, SurfaceFormationGenerationError,
    SurfaceFormationGenerator, SurfaceFormationInputs,
};
use super::surface_formation::{FormationState, FormationStateError};
use crate::engine::{BuildCancellation, StageRng};
use crate::generators::spatial::ProfileSurfaceBundle;
use crate::world::natural::{
    ClimateModelProfile, ClimateSpec, ClimateWorkDomainSnapshot, EvolvedTectonicSnapshot,
    GeologicSpec, GeologicSubstrateSnapshot, GlobalCirculationSnapshot, HydroErosionSpec,
    NaturalQualityProfile, NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot, ReliefSpec,
    ResolvedWorldFormation, TectonicSpec,
};

/// Borrowed resolved inputs for one complete causal natural-formation build.
#[derive(Debug, Clone, Copy)]
pub(in crate::generators::natural) struct CausalNaturalFormationInputs<'a> {
    /// P1 authoritative and tectonic-control surfaces plus their conservative map.
    pub profile_bundle: &'a ProfileSurfaceBundle,
    /// Product quality profile shared by the climate domain and final state.
    pub quality_profile: NaturalQualityProfile,
    /// Resolved P2 physical parameters.
    pub tectonic_spec: &'a TectonicSpec,
    /// Resolved finite P2 formation schedule and morphology.
    pub formation: &'a ResolvedWorldFormation,
    /// Resolved P3 substrate parameters.
    pub geologic_spec: &'a GeologicSpec,
    /// Resolved P3 relief and water-inventory parameters.
    pub relief_spec: &'a ReliefSpec,
    /// Validated P4 work domain for the authoritative surface.
    pub climate_domain: &'a ClimateWorkDomainSnapshot,
    /// Resolved P4 forcing parameters.
    pub climate_spec: &'a ClimateSpec,
    /// Resolved P5 surface-process parameters.
    pub surface_spec: &'a HydroErosionSpec,
}

/// Private complete output awaiting atomic bundle publication.
#[derive(Debug)]
pub(in crate::generators::natural) struct CausalFormationOutput {
    pub evolved_tectonics: EvolvedTectonicSnapshot,
    pub geologic_substrate: GeologicSubstrateSnapshot,
    pub primary_relief: PrimaryReliefSnapshot,
    pub final_climate: GlobalCirculationSnapshot,
    pub surface: NaturalSurfaceFormationSnapshot,
    pub final_climate_forcing: GlobalClimateForcing,
}

/// Runs the one production Lie-style causal split without publishing partial artifacts.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::generators::natural) struct CausalNaturalFormationGenerator;

impl CausalNaturalFormationGenerator {
    /// Builds final P2/P3/P4/P5 candidates and returns them only after endpoint closure.
    pub(in crate::generators::natural) fn generate_working(
        inputs: CausalNaturalFormationInputs<'_>,
        rng: &mut StageRng,
        cancellation: &BuildCancellation,
    ) -> Result<CausalFormationOutput, CausalFormationGenerationError> {
        if rng.is_cancelled() || cancellation.is_cancelled() {
            return Err(CausalFormationGenerationError::Cancelled);
        }
        let streams = LabeledSubstreams::capture(rng);
        let surface = inputs.profile_bundle.authoritative_surface();
        let evolved_tectonics = EvolvedTectonicGenerator::generate_from_streams(
            inputs.profile_bundle,
            inputs.tectonic_spec,
            inputs.formation,
            &streams,
        )?;
        let geologic_substrate = GeologicSubstrateGenerator::generate_from_streams(
            surface,
            &evolved_tectonics,
            inputs.geologic_spec,
            inputs.formation,
            &streams,
        )?;
        let (primary_working, primary_relief) =
            PrimaryReliefGenerator::generate_working_from_streams(
                surface,
                &evolved_tectonics,
                &geologic_substrate,
                inputs.relief_spec,
                &streams,
                cancellation,
            )?;
        let formation_state = FormationState::from_primary_working(&primary_working)?;
        // The public forcing build is this chain's one full validation of the
        // work domain against `surface`; every later consumer trusts it.
        let start_forcing = GlobalClimateForcingBuilder::build(
            surface,
            &primary_relief,
            geologic_substrate.relative_permeability(),
            inputs.climate_spec,
            inputs.climate_domain,
            cancellation,
        )?;
        let (start_climate, start_correction) =
            GlobalCirculationGenerator::generate_capturing_start_correction(
                surface,
                inputs.climate_domain,
                &start_forcing,
                ClimateModelProfile::C2LayeredV1,
                cancellation,
            )?;
        let closure = SurfaceFormationGenerator::generate_from_exact_state(
            SurfaceFormationInputs {
                surface,
                quality_profile: inputs.quality_profile,
                tectonics: &evolved_tectonics,
                substrate: &geologic_substrate,
                relief: &primary_relief,
                domain: inputs.climate_domain,
                climate_spec: inputs.climate_spec,
                initial_climate: &start_climate,
                formation_spec: inputs.surface_spec,
            },
            formation_state,
            start_correction.as_ref(),
            cancellation,
        )?;
        // The carrier lives only from the start solve to the endpoint solve.
        drop(start_correction);
        let (surface, final_climate, final_climate_forcing) = closure.into_parts();
        if final_climate.checkpoint().forcing_fingerprint() != final_climate_forcing.fingerprint() {
            return Err(CausalFormationGenerationError::EndpointForcingIdentityMismatch);
        }
        // Each sibling's producer already ended with `validate_against` on this
        // surface and the same upstream siblings, so the chain trusts them;
        // the bundle checks cross-sibling identity and publication
        // revalidates it in full.
        Ok(CausalFormationOutput {
            evolved_tectonics,
            geologic_substrate,
            primary_relief,
            final_climate,
            surface,
            final_climate_forcing,
        })
    }
}

/// Failures that prevent atomic causal-formation publication.
#[derive(Debug, Error)]
pub(in crate::generators::natural) enum CausalFormationGenerationError {
    /// Cooperative cancellation prevented a complete output.
    #[error("causal natural formation was cancelled")]
    Cancelled,
    /// The final P4 checkpoint was not built from the retained P5 forcing.
    #[error("endpoint climate forcing identity does not match final P5 terrain")]
    EndpointForcingIdentityMismatch,
    /// The offline reference schedule left a final sibling missing or inconsistent.
    #[cfg(test)]
    #[error("invalid final {role} candidate: {reason}")]
    InvalidFinalCandidate { role: &'static str, reason: String },
    /// P2 failed before a final tectonic candidate existed.
    #[error(transparent)]
    EvolvedTectonics(#[from] EvolvedTectonicGenerationError),
    /// P3 substrate construction failed.
    #[error(transparent)]
    GeologicSubstrate(#[from] GeologicSubstrateGenerationError),
    /// P3 exact relief construction or projection failed.
    #[error(transparent)]
    PrimaryRelief(#[from] PrimaryReliefGenerationError),
    /// P3 exact state could not initialize P5.
    #[error(transparent)]
    FormationState(#[from] FormationStateError),
    /// Start or endpoint P4 forcing construction failed.
    #[error(transparent)]
    ClimateForcing(#[from] GlobalClimateForcingError),
    /// Start or endpoint P4 solve failed.
    #[error(transparent)]
    Climate(#[from] GlobalCirculationGenerationError),
    /// Finite-time P5 advance or endpoint closure failed.
    #[error(transparent)]
    SurfaceFormation(#[from] SurfaceFormationGenerationError),
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Instant;

    use serde::Serialize;

    use super::{
        CausalFormationGenerationError, CausalFormationOutput, CausalNaturalFormationGenerator,
        CausalNaturalFormationInputs, FormationState, GeologicSubstrateGenerator,
        GlobalCirculationGenerator, LabeledSubstreams, PrimaryReliefGenerator,
        SurfaceFormationGenerationError, SurfaceFormationInputs,
    };
    use crate::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
    use crate::generators::natural::formation::surface_formation::generation::{
        advance_surface_processes, build_evolution_report, finalize_surface_formation,
        recompute_surface_diagnostics, upstream_fingerprints, SurfaceAdvanceSummary,
        SurfaceProcessInputs,
    };
    use crate::generators::natural::foundation::tectonics::{
        generate_evolved_spherical_with_test_resample_observer, EvolvedPublicationError,
    };
    use crate::generators::natural::surface_water_geometry::solve_physical_sea_level_exact;
    use crate::generators::natural::{ClimateWorkDomainBuilder, GlobalClimateForcingBuilder};
    use crate::generators::spatial::ProfileSurfaceBuilder;
    use crate::world::natural::{
        ClimateModelProfile, ClimateSpec, FormationSedimentFields, FormationTerrainFields,
        GeologicSpec, GeologicSubstrateSnapshot, GlobalCirculationSnapshot, HydroErosionSpec,
        NaturalQualityProfile, NaturalSurfaceFormationSnapshot, PrimaryReliefSnapshot, ReliefSpec,
        ResolvedFormationTimeline, ResolvedWorldFormation, ResolvedWorldFormationPreset,
        TectonicSpec, WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        SURFACE_FORMATION_HORIZON_YEARS,
    };
    use crate::world::spatial::SurfaceRef;
    use crate::world::{Meters, RootSeed};

    const OFFLINE_REFERENCE_SEED: u64 = 42;

    #[test]
    fn causal_split_publishes_one_self_consistent_final_state() {
        let cancellation = BuildCancellation::new();
        let profile = NaturalQualityProfile::Draft;
        let bundle = ProfileSurfaceBuilder::build(
            profile,
            Meters::new(6_371_000.0).expect("Earth radius is positive"),
            &cancellation,
        )
        .expect("the Draft profile bundle should build");
        let domain =
            ClimateWorkDomainBuilder::build(bundle.authoritative_surface(), profile, &cancellation)
                .expect("the Draft climate domain should build");
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        )
        .expect("the Continents formation should resolve")
        .with_test_timeline(ResolvedFormationTimeline::test_prefix(2));
        let tectonic_spec = TectonicSpec::default();
        let geologic_spec = GeologicSpec::default();
        let relief_spec = ReliefSpec::default();
        let climate_spec = ClimateSpec::default();
        let surface_spec = HydroErosionSpec::default();
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.causal-formation", 5, "sekai.core"),
        ));

        let output = CausalNaturalFormationGenerator::generate_working(
            CausalNaturalFormationInputs {
                profile_bundle: &bundle,
                quality_profile: profile,
                tectonic_spec: &tectonic_spec,
                formation: &formation,
                geologic_spec: &geologic_spec,
                relief_spec: &relief_spec,
                climate_domain: &domain,
                climate_spec: &climate_spec,
                surface_spec: &surface_spec,
            },
            &mut rng,
            &cancellation,
        )
        .expect("the bounded causal split should publish atomically");

        assert_eq!(
            output
                .surface
                .evolution_report()
                .integrated_duration_years()
                .to_bits(),
            SURFACE_FORMATION_HORIZON_YEARS.to_bits()
        );
        assert_eq!(
            output
                .surface
                .terrain_fields()
                .elevation_components()
                .primary_elevation_m(),
            output.primary_relief.elevation_m()
        );
        let rebuilt_forcing = GlobalClimateForcingBuilder::build_for_formation_terrain(
            bundle.authoritative_surface(),
            output.surface.terrain_fields(),
            output.geologic_substrate.relative_permeability(),
            &climate_spec,
            &domain,
            &cancellation,
        )
        .expect("the final terrain should rebuild its endpoint forcing");
        assert_eq!(
            output.final_climate_forcing.fingerprint(),
            rebuilt_forcing.fingerprint()
        );
        assert_eq!(
            output.final_climate.checkpoint().forcing_fingerprint(),
            output.final_climate_forcing.fingerprint()
        );
        assert_eq!(
            output
                .surface
                .checkpoint()
                .upstream()
                .formation_climate_checkpoint_fingerprint(),
            output.final_climate.checkpoint().fingerprint()
        );
    }

    #[test]
    #[ignore = "release-only Standard/seed 42 resample-boundary coupling reference"]
    fn compare_production_split_with_high_cost_reference() {
        let cancellation = BuildCancellation::new();
        let profile = NaturalQualityProfile::Standard;
        let bundle = ProfileSurfaceBuilder::build(
            profile,
            Meters::new(6_371_000.0).expect("Earth radius is positive"),
            &cancellation,
        )
        .expect("the Standard profile bundle should build");
        let domain =
            ClimateWorkDomainBuilder::build(bundle.authoritative_surface(), profile, &cancellation)
                .expect("the Standard climate domain should build");
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        )
        .expect("the Continents formation should resolve");
        let tectonic_spec = TectonicSpec::default();
        let geologic_spec = GeologicSpec::default();
        let relief_spec = ReliefSpec::default();
        let climate_spec = ClimateSpec::default();
        let surface_spec = HydroErosionSpec::default();
        let inputs = CausalNaturalFormationInputs {
            profile_bundle: &bundle,
            quality_profile: profile,
            tectonic_spec: &tectonic_spec,
            formation: &formation,
            geologic_spec: &geologic_spec,
            relief_spec: &relief_spec,
            climate_domain: &domain,
            climate_spec: &climate_spec,
            surface_spec: &surface_spec,
        };
        let stage_seed = derive_stage_seed(
            RootSeed::new(OFFLINE_REFERENCE_SEED),
            StageIdentity::new("natural.causal-formation", 5, "sekai.core"),
        );

        let production_started = Instant::now();
        let production = CausalNaturalFormationGenerator::generate_working(
            inputs,
            &mut StageRng::from_seed_with_cancellation(stage_seed, &cancellation),
            &cancellation,
        )
        .expect("the production split should publish");
        let production_millis = production_started.elapsed().as_millis();

        let reference_started = Instant::now();
        let mut reference_stream_rng =
            StageRng::from_seed_with_cancellation(stage_seed, &cancellation);
        let reference_streams = LabeledSubstreams::capture(&mut reference_stream_rng);
        let reference = generate_resample_boundary_reference(
            inputs,
            &mut StageRng::from_seed_with_cancellation(stage_seed, &cancellation),
            &reference_streams,
            &cancellation,
        )
        .expect("the resample-boundary reference should publish");
        let reference_millis = reference_started.elapsed().as_millis();

        let evidence = OfflineCouplingEvidence {
            schema_version: 1,
            seed: OFFLINE_REFERENCE_SEED,
            profile,
            production_p4_solves: 2,
            reference_p4_solves: reference
                .window_count
                .checked_mul(3)
                .expect("the bounded reference P4 count should fit u32"),
            reference_windows: reference.window_count,
            production_millis,
            reference_millis,
            final_source_products_equal: production.evolved_tectonics
                == reference.output.evolved_tectonics
                && production.geologic_substrate == reference.output.geologic_substrate
                && production.primary_relief == reference.output.primary_relief,
            differences: final_state_differences(
                &production.surface,
                &reference.output.surface,
                production.final_climate == reference.output.final_climate,
            ),
            production_surface: &production.surface,
            reference_surface: &reference.output.surface,
        };
        let json = serde_json::to_vec_pretty(&evidence)
            .expect("validated offline evidence should serialize");
        let directory = PathBuf::from("target/natural-quality/causal-formation");
        fs::create_dir_all(&directory).expect("the offline evidence directory should exist");
        let path = directory.join("offline-reference-standard-seed-42.json");
        fs::write(&path, &json).expect("the offline evidence JSON should be written");
        let checksum = blake3::hash(&json).to_hex().to_string();
        fs::write(
            path.with_extension("json.blake3"),
            format!("{checksum}  offline-reference-standard-seed-42.json\n"),
        )
        .expect("the offline evidence checksum should be written");
    }

    struct OfflineReferenceOutput {
        output: CausalFormationOutput,
        window_count: u32,
    }

    fn generate_resample_boundary_reference(
        inputs: CausalNaturalFormationInputs<'_>,
        rng: &mut StageRng,
        streams: &LabeledSubstreams,
        cancellation: &BuildCancellation,
    ) -> Result<OfflineReferenceOutput, CausalFormationGenerationError> {
        let surface = inputs.profile_bundle.authoritative_surface();
        let total_steps = inputs.formation.timeline().step_count();
        let mut state: Option<FormationState> = None;
        let mut previous_steps = 0_u16;
        let mut integrated_duration_years = 0.0_f64;
        let mut total_summary: Option<SurfaceAdvanceSummary> = None;
        let mut retained_sediment: Option<FormationSedimentFields> = None;
        let mut previous_primary_elevation_m: Option<Vec<f64>> = None;
        let mut final_substrate: Option<GeologicSubstrateSnapshot> = None;
        let mut final_primary_relief: Option<PrimaryReliefSnapshot> = None;
        let mut final_terrain: Option<FormationTerrainFields> = None;
        let mut final_climate: Option<GlobalCirculationSnapshot> = None;
        let mut final_climate_forcing = None;
        let mut window_count = 0_u32;

        let evolved_tectonics = generate_evolved_spherical_with_test_resample_observer(
            inputs.profile_bundle,
            inputs.tectonic_spec,
            inputs.formation,
            rng,
            |accepted_steps, evolved| {
                let result = (|| -> Result<(), CausalFormationGenerationError> {
                    if accepted_steps <= previous_steps || accepted_steps > total_steps {
                        return Err(CausalFormationGenerationError::InvalidFinalCandidate {
                            role: "offline_reference_schedule",
                            reason: format!(
                                "accepted P2 steps must increase within 1..={total_steps}, found {accepted_steps} after {previous_steps}"
                            ),
                        });
                    }
                    let substrate = GeologicSubstrateGenerator::generate_from_streams(
                        surface,
                        evolved,
                        inputs.geologic_spec,
                        inputs.formation,
                        streams,
                    )?;
                    let (primary_working, primary_relief) =
                        PrimaryReliefGenerator::generate_working_from_streams(
                            surface,
                            evolved,
                            &substrate,
                            inputs.relief_spec,
                            streams,
                            cancellation,
                        )?;
                    if let Some(current) = state.as_mut() {
                        current.replace_primary_for_offline_reference(
                            previous_primary_elevation_m.as_deref().ok_or_else(|| {
                                CausalFormationGenerationError::InvalidFinalCandidate {
                                    role: "offline_reference_primary",
                                    reason: "the previous exact P3 primary is missing".to_owned(),
                                }
                            })?,
                            primary_working.elevation_exact_m(),
                        )?;
                    } else {
                        state = Some(FormationState::from_primary_working(&primary_working)?);
                    }
                    previous_primary_elevation_m =
                        Some(primary_working.elevation_exact_m().to_vec());
                    let current = state.as_mut().expect("the reference state was initialized");
                    let water = solve_physical_sea_level_exact(
                        surface,
                        current.current_elevation_exact_m(),
                        primary_relief.water_inventory_m3(),
                        cancellation,
                    )
                    .map_err(SurfaceFormationGenerationError::from)?
                    .into_geometry();
                    current.replace_surface_water_geometry(water);

                    let start_forcing = if let Some(sediment) = retained_sediment.take() {
                        let start_terrain = current.project_final_terrain(
                            surface,
                            sediment,
                            primary_relief.water_inventory_m3(),
                            cancellation,
                        )?;
                        GlobalClimateForcingBuilder::build_for_formation_terrain(
                            surface,
                            &start_terrain,
                            substrate.relative_permeability(),
                            inputs.climate_spec,
                            inputs.climate_domain,
                            cancellation,
                        )?
                    } else {
                        GlobalClimateForcingBuilder::build(
                            surface,
                            &primary_relief,
                            substrate.relative_permeability(),
                            inputs.climate_spec,
                            inputs.climate_domain,
                            cancellation,
                        )?
                    };
                    let start_climate = GlobalCirculationGenerator::generate(
                        surface,
                        inputs.climate_domain,
                        &start_forcing,
                        ClimateModelProfile::C2LayeredV1,
                        cancellation,
                    )?;
                    let cumulative_duration_years = if accepted_steps == total_steps {
                        SURFACE_FORMATION_HORIZON_YEARS
                    } else {
                        SURFACE_FORMATION_HORIZON_YEARS * f64::from(accepted_steps)
                            / f64::from(total_steps)
                    };
                    let window_duration_years =
                        cumulative_duration_years - integrated_duration_years;
                    if !window_duration_years.is_finite() || window_duration_years <= 0.0 {
                        return Err(CausalFormationGenerationError::InvalidFinalCandidate {
                            role: "offline_reference_schedule",
                            reason: format!(
                                "P5 window duration is invalid at P2 step {accepted_steps}: {window_duration_years} years"
                            ),
                        });
                    }
                    let half_window_years = window_duration_years / 2.0;
                    let formation_inputs = SurfaceFormationInputs {
                        surface,
                        quality_profile: inputs.quality_profile,
                        tectonics: evolved,
                        substrate: &substrate,
                        relief: &primary_relief,
                        domain: inputs.climate_domain,
                        climate_spec: inputs.climate_spec,
                        initial_climate: &start_climate,
                        formation_spec: inputs.surface_spec,
                    };
                    let first = advance_surface_processes(
                        current,
                        surface_process_inputs(formation_inputs, &start_climate),
                        half_window_years,
                        cancellation,
                    )?;
                    let (first_summary, midpoint_sediment) = first.into_parts();
                    let midpoint_terrain = current.project_final_terrain(
                        surface,
                        midpoint_sediment,
                        primary_relief.water_inventory_m3(),
                        cancellation,
                    )?;
                    let midpoint_forcing =
                        GlobalClimateForcingBuilder::build_for_formation_terrain(
                            surface,
                            &midpoint_terrain,
                            substrate.relative_permeability(),
                            inputs.climate_spec,
                            inputs.climate_domain,
                            cancellation,
                        )?;
                    let midpoint_climate = GlobalCirculationGenerator::generate(
                        surface,
                        inputs.climate_domain,
                        &midpoint_forcing,
                        ClimateModelProfile::C2LayeredV1,
                        cancellation,
                    )?;
                    let second = advance_surface_processes(
                        current,
                        surface_process_inputs(formation_inputs, &midpoint_climate),
                        half_window_years,
                        cancellation,
                    )?;
                    let (second_summary, endpoint_sediment) = second.into_parts();
                    let endpoint_terrain = current.project_final_terrain(
                        surface,
                        endpoint_sediment,
                        primary_relief.water_inventory_m3(),
                        cancellation,
                    )?;
                    let endpoint_forcing =
                        GlobalClimateForcingBuilder::build_for_formation_terrain(
                            surface,
                            &endpoint_terrain,
                            substrate.relative_permeability(),
                            inputs.climate_spec,
                            inputs.climate_domain,
                            cancellation,
                        )?;
                    let endpoint_climate = GlobalCirculationGenerator::generate(
                        surface,
                        inputs.climate_domain,
                        &endpoint_forcing,
                        ClimateModelProfile::C2LayeredV1,
                        cancellation,
                    )?;
                    let window_summary = first_summary.combined(
                        second_summary,
                        window_duration_years,
                    )?;
                    total_summary = Some(match total_summary.take() {
                        Some(summary) => summary.combined(
                            window_summary,
                            cumulative_duration_years,
                        )?,
                        None => window_summary,
                    });
                    integrated_duration_years = cumulative_duration_years;
                    previous_steps = accepted_steps;
                    window_count = window_count
                        .checked_add(1)
                        .ok_or(CausalFormationGenerationError::InvalidFinalCandidate {
                            role: "offline_reference_schedule",
                            reason: "reference window count overflowed".to_owned(),
                        })?;

                    if accepted_steps == total_steps {
                        final_substrate = Some(substrate);
                        final_primary_relief = Some(primary_relief);
                        final_terrain = Some(endpoint_terrain);
                        final_climate = Some(endpoint_climate);
                        final_climate_forcing = Some(endpoint_forcing);
                    } else {
                        retained_sediment = Some(endpoint_terrain.sediment().clone());
                    }
                    Ok(())
                })();
                result.map_err(|error| EvolvedPublicationError::Runner(error.to_string()))
            },
        )
        .map_err(|error| CausalFormationGenerationError::InvalidFinalCandidate {
            role: "offline_reference_tectonics",
            reason: error.to_string(),
        })?;
        if previous_steps != total_steps
            || integrated_duration_years.to_bits() != SURFACE_FORMATION_HORIZON_YEARS.to_bits()
        {
            return Err(CausalFormationGenerationError::InvalidFinalCandidate {
                role: "offline_reference_schedule",
                reason: format!(
                    "reference ended at P2 step {previous_steps}/{total_steps} and P5 duration {integrated_duration_years}"
                ),
            });
        }
        let state = state.ok_or_else(|| CausalFormationGenerationError::InvalidFinalCandidate {
            role: "offline_reference_state",
            reason: "no legal P2 resample boundary was observed".to_owned(),
        })?;
        let geologic_substrate = final_substrate.ok_or_else(|| {
            CausalFormationGenerationError::InvalidFinalCandidate {
                role: "offline_reference_substrate",
                reason: "the final P3 substrate is missing".to_owned(),
            }
        })?;
        let primary_relief = final_primary_relief.ok_or_else(|| {
            CausalFormationGenerationError::InvalidFinalCandidate {
                role: "offline_reference_primary",
                reason: "the final P3 relief is missing".to_owned(),
            }
        })?;
        let final_terrain =
            final_terrain.ok_or_else(|| CausalFormationGenerationError::InvalidFinalCandidate {
                role: "offline_reference_terrain",
                reason: "the final P5 terrain is missing".to_owned(),
            })?;
        let final_climate =
            final_climate.ok_or_else(|| CausalFormationGenerationError::InvalidFinalCandidate {
                role: "offline_reference_climate",
                reason: "the final endpoint P4 state is missing".to_owned(),
            })?;
        let final_climate_forcing = final_climate_forcing.ok_or_else(|| {
            CausalFormationGenerationError::InvalidFinalCandidate {
                role: "offline_reference_forcing",
                reason: "the final endpoint P4 forcing is missing".to_owned(),
            }
        })?;
        let summary =
            total_summary.ok_or_else(|| CausalFormationGenerationError::InvalidFinalCandidate {
                role: "offline_reference_schedule",
                reason: "the cumulative P5 work summary is missing".to_owned(),
            })?;
        let final_inputs = SurfaceFormationInputs {
            surface,
            quality_profile: inputs.quality_profile,
            tectonics: &evolved_tectonics,
            substrate: &geologic_substrate,
            relief: &primary_relief,
            domain: inputs.climate_domain,
            climate_spec: inputs.climate_spec,
            initial_climate: &final_climate,
            formation_spec: inputs.surface_spec,
        };
        let terminal_diagnostics = recompute_surface_diagnostics(
            &state,
            surface_process_inputs(final_inputs, &final_climate),
            cancellation,
        )?;
        let evolution_report = build_evolution_report(
            surface,
            &state,
            summary,
            &terminal_diagnostics,
            cancellation,
        )?;
        let upstream = upstream_fingerprints(final_inputs, &final_climate, cancellation)?;
        let final_climate_checkpoint_fingerprint = *final_climate.checkpoint().fingerprint();
        let surface_snapshot = finalize_surface_formation(
            state,
            final_terrain,
            surface,
            SurfaceRef::for_spherical(surface),
            inputs.quality_profile,
            final_climate_checkpoint_fingerprint,
            upstream,
            terminal_diagnostics,
            evolution_report,
            cancellation,
        )?;
        // The observer validated this substrate only against its boundary
        // snapshot; bind it to the separately published final P2 sibling.
        geologic_substrate
            .validate_against(surface, &evolved_tectonics)
            .map_err(
                |error| CausalFormationGenerationError::InvalidFinalCandidate {
                    role: "geologic_substrate",
                    reason: error.to_string(),
                },
            )?;
        let output = CausalFormationOutput {
            evolved_tectonics,
            geologic_substrate,
            primary_relief,
            final_climate,
            surface: surface_snapshot,
            final_climate_forcing,
        };
        Ok(OfflineReferenceOutput {
            output,
            window_count,
        })
    }

    fn surface_process_inputs<'a>(
        inputs: SurfaceFormationInputs<'a>,
        climate: &'a crate::world::natural::GlobalCirculationSnapshot,
    ) -> SurfaceProcessInputs<'a> {
        SurfaceProcessInputs {
            surface: inputs.surface,
            tectonics: inputs.tectonics,
            substrate: inputs.substrate,
            climate,
            formation_spec: inputs.formation_spec,
            water_inventory_m3: inputs.relief.water_inventory_m3(),
        }
    }

    #[derive(Serialize)]
    struct OfflineCouplingEvidence<'a> {
        schema_version: u16,
        seed: u64,
        profile: NaturalQualityProfile,
        production_p4_solves: u32,
        reference_p4_solves: u32,
        reference_windows: u32,
        production_millis: u128,
        reference_millis: u128,
        final_source_products_equal: bool,
        differences: FinalStateDifferences,
        production_surface: &'a NaturalSurfaceFormationSnapshot,
        reference_surface: &'a NaturalSurfaceFormationSnapshot,
    }

    #[derive(Serialize)]
    struct FinalStateDifferences {
        elevation_components_max_abs_m: ElevationComponentDifferences,
        sea_level_abs_m: f64,
        ocean_area_fraction_max_abs: f64,
        cell_water_volume_max_abs_m3: f64,
        endpoint_climate_equal: bool,
    }

    #[derive(Serialize)]
    struct ElevationComponentDifferences {
        primary_elevation_m: f64,
        tectonic_displacement_m: f64,
        fluvial_erosion_m: f64,
        hillslope_erosion_m: f64,
        hillslope_deposition_m: f64,
        routed_sediment_deposition_m: f64,
        coastal_erosion_m: f64,
        coastal_deposition_m: f64,
        isostatic_response_m: f64,
        final_elevation_m: f64,
    }

    fn final_state_differences(
        production: &NaturalSurfaceFormationSnapshot,
        reference: &NaturalSurfaceFormationSnapshot,
        endpoint_climate_equal: bool,
    ) -> FinalStateDifferences {
        let production_terrain = production.terrain_fields();
        let reference_terrain = reference.terrain_fields();
        let left = production_terrain.elevation_components();
        let right = reference_terrain.elevation_components();
        let left_water = production_terrain.surface_water_geometry();
        let right_water = reference_terrain.surface_water_geometry();
        FinalStateDifferences {
            elevation_components_max_abs_m: ElevationComponentDifferences {
                primary_elevation_m: max_abs_f32(
                    left.primary_elevation_m(),
                    right.primary_elevation_m(),
                ),
                tectonic_displacement_m: max_abs_f32(
                    left.tectonic_displacement_m(),
                    right.tectonic_displacement_m(),
                ),
                fluvial_erosion_m: max_abs_f32(left.fluvial_erosion_m(), right.fluvial_erosion_m()),
                hillslope_erosion_m: max_abs_f32(
                    left.hillslope_erosion_m(),
                    right.hillslope_erosion_m(),
                ),
                hillslope_deposition_m: max_abs_f32(
                    left.hillslope_deposition_m(),
                    right.hillslope_deposition_m(),
                ),
                routed_sediment_deposition_m: max_abs_f32(
                    left.routed_sediment_deposition_m(),
                    right.routed_sediment_deposition_m(),
                ),
                coastal_erosion_m: max_abs_f32(left.coastal_erosion_m(), right.coastal_erosion_m()),
                coastal_deposition_m: max_abs_f32(
                    left.coastal_deposition_m(),
                    right.coastal_deposition_m(),
                ),
                isostatic_response_m: max_abs_f32(
                    left.isostatic_response_m(),
                    right.isostatic_response_m(),
                ),
                final_elevation_m: max_abs_f32(left.final_elevation_m(), right.final_elevation_m()),
            },
            sea_level_abs_m: f64::from(
                (left_water.sea_level_m() - right_water.sea_level_m()).abs(),
            ),
            ocean_area_fraction_max_abs: max_abs_f32(
                left_water.ocean_area_fraction(),
                right_water.ocean_area_fraction(),
            ),
            cell_water_volume_max_abs_m3: max_abs_f64(
                left_water.cell_water_volume_m3(),
                right_water.cell_water_volume_m3(),
            ),
            endpoint_climate_equal,
        }
    }

    fn max_abs_f32(left: &[f32], right: &[f32]) -> f64 {
        assert_eq!(left.len(), right.len());
        left.iter()
            .zip(right)
            .map(|(&left, &right)| f64::from((left - right).abs()))
            .fold(0.0, f64::max)
    }

    fn max_abs_f64(left: &[f64], right: &[f64]) -> f64 {
        assert_eq!(left.len(), right.len());
        left.iter()
            .zip(right)
            .map(|(&left, &right)| (left - right).abs())
            .fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod timing_probe {
    use std::time::Instant;

    use super::{
        EvolvedTectonicGenerator, FormationState, GeologicSubstrateGenerator,
        GlobalCirculationGenerator, GlobalClimateForcingBuilder, LabeledSubstreams,
        PrimaryReliefGenerator, SurfaceFormationGenerator, SurfaceFormationInputs,
    };
    use crate::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
    use crate::generators::natural::ClimateWorkDomainBuilder;
    use crate::generators::spatial::ProfileSurfaceBuilder;
    use crate::world::natural::{
        ClimateModelProfile, ClimateSpec, GeologicSpec, GlobalCirculationSnapshot,
        HydroErosionSpec, NaturalQualityProfile, ReliefSpec, ResolvedWorldFormation,
        ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
    };
    use crate::world::{Meters, RootSeed};

    fn report(profile: NaturalQualityProfile, stage: &str, started: Instant) {
        eprintln!(
            "[probe {profile:?}] {stage:<18} {:>8.3} s",
            started.elapsed().as_secs_f64()
        );
    }

    fn solve_summary(climate: &GlobalCirculationSnapshot) -> String {
        let solve = climate.solve_report();
        format!(
            "cycles={} steps={} fast_substeps={} final_residual={:.4}",
            solve.formation_cycles(),
            solve.continuation_steps(),
            solve.fast_substeps(),
            solve.final_residual()
        )
    }

    /// Wall-clock breakdown of the production causal chain for one profile.
    ///
    /// `SEKAI_PROBE_PROFILE=draft|standard|high` selects the profile;
    /// `SEKAI_PROBE_TAG=<tag>` additionally writes both climate snapshots as
    /// JSON under `SEKAI_PROBE_DIR` (default `target/probe-dumps`) so two
    /// numerical variants can be compared field by field.
    #[test]
    #[ignore = "release-only wall-clock stage breakdown"]
    fn stage_timing_probe() {
        let profile = match std::env::var("SEKAI_PROBE_PROFILE").as_deref() {
            Ok("standard") => NaturalQualityProfile::Standard,
            Ok("high") => NaturalQualityProfile::High,
            _ => NaturalQualityProfile::Draft,
        };
        let cancellation = BuildCancellation::new();
        let total = Instant::now();
        let started = Instant::now();
        let bundle =
            ProfileSurfaceBuilder::build(profile, Meters::new(6_371_000.0).unwrap(), &cancellation)
                .unwrap();
        report(profile, "P1 bundle", started);
        let started = Instant::now();
        let domain =
            ClimateWorkDomainBuilder::build(bundle.authoritative_surface(), profile, &cancellation)
                .unwrap();
        report(profile, "climate domain", started);
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        )
        .unwrap();
        let tectonic_spec = TectonicSpec::default();
        let geologic_spec = GeologicSpec::default();
        let relief_spec = ReliefSpec::default();
        let climate_spec = ClimateSpec::default();
        let surface_spec = HydroErosionSpec::default();
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.causal-formation", 5, "sekai.core"),
        ));
        let streams = LabeledSubstreams::capture(&mut rng);
        let surface = bundle.authoritative_surface();

        let started = Instant::now();
        let evolved = EvolvedTectonicGenerator::generate_from_streams(
            &bundle,
            &tectonic_spec,
            &formation,
            &streams,
        )
        .unwrap();
        report(profile, "P2 tectonics", started);
        let started = Instant::now();
        let substrate = GeologicSubstrateGenerator::generate_from_streams(
            surface,
            &evolved,
            &geologic_spec,
            &formation,
            &streams,
        )
        .unwrap();
        report(profile, "P3 substrate", started);
        let started = Instant::now();
        let (primary_working, primary_relief) =
            PrimaryReliefGenerator::generate_working_from_streams(
                surface,
                &evolved,
                &substrate,
                &relief_spec,
                &streams,
                &cancellation,
            )
            .unwrap();
        let formation_state = FormationState::from_primary_working(&primary_working).unwrap();
        report(profile, "P3 relief", started);
        let started = Instant::now();
        let start_forcing = GlobalClimateForcingBuilder::build(
            surface,
            &primary_relief,
            substrate.relative_permeability(),
            &climate_spec,
            &domain,
            &cancellation,
        )
        .unwrap();
        report(profile, "P4 start forcing", started);
        let started = Instant::now();
        let (start_climate, start_correction) =
            GlobalCirculationGenerator::generate_capturing_start_correction(
                surface,
                &domain,
                &start_forcing,
                ClimateModelProfile::C2LayeredV1,
                &cancellation,
            )
            .unwrap();
        report(profile, "P4 start solve", started);
        eprintln!("[probe {profile:?}]    {}", solve_summary(&start_climate));

        let started = Instant::now();
        let closure = SurfaceFormationGenerator::generate_from_exact_state(
            SurfaceFormationInputs {
                surface,
                quality_profile: profile,
                tectonics: &evolved,
                substrate: &substrate,
                relief: &primary_relief,
                domain: &domain,
                climate_spec: &climate_spec,
                initial_climate: &start_climate,
                formation_spec: &surface_spec,
            },
            formation_state,
            start_correction.as_ref(),
            &cancellation,
        )
        .unwrap();
        drop(start_correction);
        let (surface_out, final_climate, _forcing) = closure.into_parts();
        report(profile, "P5 + endpoint P4", started);
        eprintln!(
            "[probe {profile:?}]    P5 substeps={} endpoint {}",
            surface_out.evolution_report().accepted_surface_substeps(),
            solve_summary(&final_climate)
        );
        report(profile, "TOTAL", total);

        if let Ok(tag) = std::env::var("SEKAI_PROBE_TAG") {
            let dir = std::path::PathBuf::from(
                std::env::var("SEKAI_PROBE_DIR").unwrap_or_else(|_| "target/probe-dumps".into()),
            );
            std::fs::create_dir_all(&dir).unwrap();
            for (name, climate) in [("start", &start_climate), ("end", &final_climate)] {
                let file =
                    std::fs::File::create(dir.join(format!("{tag}-{profile:?}-{name}.json")))
                        .unwrap();
                serde_json::to_writer(std::io::BufWriter::new(file), climate).unwrap();
            }
        }
    }
}

/// A8b §9.2 offline reference comparison of the endpoint P4 initial guesses.
#[cfg(test)]
mod endpoint_reuse_comparison {
    use std::time::Instant;

    use serde::Serialize;

    use super::super::global_circulation::{
        offline::{self, OfflineCapture, OfflineSchedule},
        EndpointInjection, StartAtmosphereFasCorrection,
    };
    use super::{
        EvolvedTectonicGenerator, FormationState, GeologicSubstrateGenerator,
        GlobalCirculationGenerator, GlobalClimateForcingBuilder, LabeledSubstreams,
        PrimaryReliefGenerator,
    };
    use crate::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
    use crate::generators::natural::formation::surface_formation::generation::{
        advance_surface_processes, SurfaceProcessInputs,
    };
    use crate::generators::natural::quality::evaluate_global_circulation_quality_for_formation_cancellable;
    use crate::generators::natural::ClimateWorkDomainBuilder;
    use crate::generators::spatial::ProfileSurfaceBuilder;
    use crate::world::natural::{
        ClimateModelProfile, ClimateSpec, GeologicSpec, GlobalCirculationFields,
        GlobalCirculationSnapshot, HydroErosionSpec, NaturalQualityProfile, QualityMetric,
        ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicActivity,
        TectonicSpec, WorldFormationPreset, EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE,
        EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        SURFACE_FORMATION_HORIZON_YEARS,
    };
    use crate::world::{Meters, RootSeed};

    /// The frozen Active seeds of `tests/formation_seed_sweep.rs`
    /// (`DEFAULT_SEEDS`); an integration test cannot be imported here.
    const SWEEP_SEEDS: [u64; 8] = [
        14_500_982_625_797_044_708,
        11_728_661_556_832_718_318,
        12_307_902_622_372_036_327,
        4_153_584_564_816_370_805,
        7_499_274_601_121_372_060,
        5_744_981_634_611_769_885,
        5_828_067_323_727_643_059,
        7_755_195_920_647_622_459,
    ];
    /// The stage-probe seed; it alone also runs the 16-cycle reference R16.
    const PROBE_SEED: u64 = 42;
    /// FAS iteration-error floor of A5 §7.58: twice the production coarse cycles.
    const T96_COARSE_CYCLES: u16 = 96;
    /// R16 horizon of A8b §9.2(c).
    const R16_FORMATION_CYCLES: u16 = 16;

    type FieldErrors = Vec<(&'static str, Vec<(&'static str, f64)>)>;

    #[derive(Serialize)]
    struct SeedComparison {
        seed: u64,
        profile: NaturalQualityProfile,
        activity: TectonicActivity,
        runs: Vec<RunRecord>,
        /// RMS(run - R) / RMS(R) per field, for every run with fields.
        error_vs_r: FieldErrors,
        error_vs_r16: FieldErrors,
        /// RMS(run - T) / RMS(T) per field; `S` is the cold start solve.
        distance_to_t: FieldErrors,
        /// ‖δ_s − δ_e‖ / ‖δ_e‖ per atmosphere layer, for V and P.
        correction_drift: Option<Vec<LayerDrift>>,
        quality: Vec<QualityRecord>,
    }

    #[derive(Debug, Clone, Serialize)]
    struct RunRecord {
        label: &'static str,
        outcome: String,
        cycles: usize,
        residuals: Vec<f64>,
        millis: u128,
    }

    #[derive(Serialize)]
    struct LayerDrift {
        layer: &'static str,
        volume: f64,
        momentum: f64,
    }

    #[derive(Serialize)]
    struct QualityRecord {
        label: &'static str,
        gpcp_precipitation_mm_day: Option<f64>,
        gpcp_envelope_mm_day: [f64; 2],
        gpcp_inside: Option<bool>,
        metrics: Vec<QualityMetric>,
    }

    struct Run {
        record: RunRecord,
        snapshot: Option<GlobalCirculationSnapshot>,
        capture: OfflineCapture,
    }

    /// One P4 solve under `schedule`, keeping what it captured.
    fn timed(
        label: &'static str,
        schedule: OfflineSchedule,
        solve: impl FnOnce() -> Result<(GlobalCirculationSnapshot, String), String>,
    ) -> Run {
        let started = Instant::now();
        let (result, capture) = offline::run(schedule, solve);
        let millis = started.elapsed().as_millis();
        let (snapshot, outcome) = match result {
            Ok((snapshot, outcome)) => (Some(snapshot), outcome),
            Err(error) => (None, format!("Err: {error}")),
        };
        let record = RunRecord {
            label,
            outcome,
            cycles: capture.residuals.len(),
            residuals: capture.residuals.clone(),
            millis,
        };
        eprintln!(
            "[a8b] {label:<4} {} cycles {} residuals {:?} {:.1} s",
            record.outcome,
            record.cycles,
            record.residuals,
            millis as f64 / 1000.0
        );
        Run {
            record,
            snapshot,
            capture,
        }
    }

    /// The §9.2 fields, flattened over cells, months and vector components.
    fn compared_fields(fields: &GlobalCirculationFields) -> [(&'static str, &[f32]); 12] {
        const C2: &str = "the C2 profile publishes every compared field";
        [
            (
                "near_surface_wind",
                fields
                    .near_surface_wind_m_s()
                    .values()
                    .as_flattened()
                    .as_flattened(),
            ),
            (
                "upper_wind",
                fields
                    .upper_wind_m_s()
                    .expect(C2)
                    .values()
                    .as_flattened()
                    .as_flattened(),
            ),
            (
                "surface_current",
                fields
                    .surface_ocean_current_m_s()
                    .values()
                    .as_flattened()
                    .as_flattened(),
            ),
            (
                "precipitation",
                fields
                    .monthly_precipitation_mm_day()
                    .values()
                    .as_flattened(),
            ),
            (
                "orographic_precipitation",
                fields
                    .monthly_orographic_precipitation_mm_day()
                    .values()
                    .as_flattened(),
            ),
            (
                "evaporation",
                fields.monthly_evaporation_mm_day().values().as_flattened(),
            ),
            (
                "specific_humidity",
                fields.monthly_specific_humidity().values().as_flattened(),
            ),
            (
                "air_temperature",
                fields.monthly_air_temperature_c().values().as_flattened(),
            ),
            (
                "lower_height_anomaly",
                fields
                    .monthly_lower_atmosphere_height_anomaly_m()
                    .values()
                    .as_flattened(),
            ),
            (
                "upper_height_anomaly",
                fields
                    .monthly_upper_atmosphere_height_anomaly_m()
                    .expect(C2)
                    .values()
                    .as_flattened(),
            ),
            (
                "sea_surface_height",
                fields
                    .monthly_sea_surface_height_anomaly_m()
                    .values()
                    .as_flattened(),
            ),
            (
                "thermocline_height",
                fields
                    .monthly_thermocline_height_anomaly_m()
                    .expect(C2)
                    .values()
                    .as_flattened(),
            ),
        ]
    }

    /// Unweighted RMS(candidate − reference) / RMS(reference) per field, the
    /// A7 §7.2 measure.
    fn relative_errors(
        candidate: &GlobalCirculationFields,
        reference: &GlobalCirculationFields,
    ) -> Vec<(&'static str, f64)> {
        compared_fields(candidate)
            .into_iter()
            .zip(compared_fields(reference))
            .map(|((name, candidate), (_, reference))| {
                assert_eq!(candidate.len(), reference.len());
                let (difference, norm) = candidate.iter().zip(reference).fold(
                    (0.0_f64, 0.0_f64),
                    |(difference, norm), (&candidate, &reference)| {
                        let delta = f64::from(candidate) - f64::from(reference);
                        (
                            difference + delta * delta,
                            norm + f64::from(reference).powi(2),
                        )
                    },
                );
                (name, (difference / norm).sqrt())
            })
            .collect()
    }

    fn errors_against(runs: &[&Run], reference: Option<&Run>) -> FieldErrors {
        let Some(reference) = reference.and_then(|run| run.capture.fields.as_ref()) else {
            return Vec::new();
        };
        runs.iter()
            .filter_map(|run| {
                let fields = run.capture.fields.as_ref()?;
                Some((run.record.label, relative_errors(fields, reference)))
            })
            .collect()
    }

    /// ‖δ_s − δ_e‖ / ‖δ_e‖ per layer, for the volume `V` and the momentum `P`.
    fn correction_drift(start: &[Vec<[f64; 4]>], endpoint: &[Vec<[f64; 4]>]) -> Vec<LayerDrift> {
        ["lower_atmosphere", "upper_atmosphere"]
            .into_iter()
            .zip(start.iter().zip(endpoint))
            .map(|(layer, (start, endpoint))| {
                let mut sums = [0.0_f64; 4];
                for (start, endpoint) in start.iter().zip(endpoint) {
                    sums[0] += (start[0] - endpoint[0]).powi(2);
                    sums[1] += endpoint[0].powi(2);
                    for component in 1..4 {
                        sums[2] += (start[component] - endpoint[component]).powi(2);
                        sums[3] += endpoint[component].powi(2);
                    }
                }
                LayerDrift {
                    layer,
                    volume: (sums[0] / sums[1]).sqrt(),
                    momentum: (sums[2] / sums[3]).sqrt(),
                }
            })
            .collect()
    }

    /// Per seed, one start solve and one P5 advance, then on that bitwise
    /// endpoint forcing: T (today's cold endpoint), U and U′ (the two reuse
    /// forms), R (cold, every one of the profile's cycles), T96 (96 coarse
    /// FAS cycles) and, for seed 42, R16. Writes one JSON per seed.
    ///
    /// `SEKAI_COMPARE_PROFILE=draft|standard`, `SEKAI_COMPARE_SEEDS=a,b`
    /// (default 42 plus the sweep seeds) and `SEKAI_COMPARE_DIR` (default
    /// `target/a8b-compare`) select the run. Seed 42 is the stage probe's
    /// world (default activity); the sweep seeds are the sweep's Active worlds.
    #[test]
    #[ignore = "release-only A8b §9.2 endpoint reference comparison"]
    fn endpoint_reuse_reference_comparison() {
        let profile = match std::env::var("SEKAI_COMPARE_PROFILE").as_deref() {
            Ok("standard") => NaturalQualityProfile::Standard,
            _ => NaturalQualityProfile::Draft,
        };
        let seeds: Vec<u64> = match std::env::var("SEKAI_COMPARE_SEEDS") {
            Ok(list) => list
                .split(',')
                .map(|seed| {
                    seed.trim()
                        .parse()
                        .expect("SEKAI_COMPARE_SEEDS holds u64 seeds")
                })
                .collect(),
            Err(_) => std::iter::once(PROBE_SEED).chain(SWEEP_SEEDS).collect(),
        };
        let directory = std::path::PathBuf::from(
            std::env::var("SEKAI_COMPARE_DIR").unwrap_or_else(|_| "target/a8b-compare".into()),
        );
        std::fs::create_dir_all(&directory).unwrap();
        let cancellation = BuildCancellation::new();
        let bundle =
            ProfileSurfaceBuilder::build(profile, Meters::new(6_371_000.0).unwrap(), &cancellation)
                .unwrap();
        let surface = bundle.authoritative_surface();
        let domain = ClimateWorkDomainBuilder::build(surface, profile, &cancellation).unwrap();
        let formation = ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Continents,
        )
        .unwrap();
        let geologic_spec = GeologicSpec::default();
        let relief_spec = ReliefSpec::default();
        let climate_spec = ClimateSpec::default();
        let surface_spec = HydroErosionSpec::default();
        let (gpcp_minimum, gpcp_maximum) = (
            EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY
                * (1.0 - EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE),
            EARTH_GLOBAL_PRECIPITATION_REFERENCE_MM_DAY
                * (1.0 + EARTH_GLOBAL_PRECIPITATION_EVIDENCE_RELATIVE_TOLERANCE),
        );

        for seed in seeds {
            let activity = if seed == PROBE_SEED {
                TectonicSpec::default().activity
            } else {
                TectonicActivity::Active
            };
            let tectonic_spec = TectonicSpec {
                activity,
                ..TectonicSpec::default()
            };
            eprintln!("[a8b] seed {seed} {profile:?} {activity:?}");
            let mut rng = StageRng::from_seed(derive_stage_seed(
                RootSeed::new(seed),
                StageIdentity::new("natural.causal-formation", 5, "sekai.core"),
            ));
            let streams = LabeledSubstreams::capture(&mut rng);
            let evolved = EvolvedTectonicGenerator::generate_from_streams(
                &bundle,
                &tectonic_spec,
                &formation,
                &streams,
            )
            .unwrap();
            let substrate = GeologicSubstrateGenerator::generate_from_streams(
                surface,
                &evolved,
                &geologic_spec,
                &formation,
                &streams,
            )
            .unwrap();
            let (primary_working, primary_relief) =
                PrimaryReliefGenerator::generate_working_from_streams(
                    surface,
                    &evolved,
                    &substrate,
                    &relief_spec,
                    &streams,
                    &cancellation,
                )
                .unwrap();
            let mut state = FormationState::from_primary_working(&primary_working).unwrap();
            drop(primary_working);
            let start_forcing = GlobalClimateForcingBuilder::build(
                surface,
                &primary_relief,
                substrate.relative_permeability(),
                &climate_spec,
                &domain,
                &cancellation,
            )
            .unwrap();
            let mut carrier = None;
            let start = timed("S", OfflineSchedule::default(), || {
                GlobalCirculationGenerator::generate_capturing_start_correction(
                    surface,
                    &domain,
                    &start_forcing,
                    ClimateModelProfile::C2LayeredV1,
                    &cancellation,
                )
                .map(|(snapshot, correction)| {
                    let outcome = format!("captured carrier: {}", correction.is_some());
                    carrier = correction;
                    (snapshot, outcome)
                })
                .map_err(|error| error.to_string())
            });
            let start_climate = start
                .snapshot
                .as_ref()
                .expect("the start P4 solve publishes");

            // The production P5 advance and endpoint forcing of
            // `generate_from_validated_state`, run once for every endpoint.
            let (_, final_sediment) = advance_surface_processes(
                &mut state,
                SurfaceProcessInputs {
                    surface,
                    tectonics: &evolved,
                    substrate: &substrate,
                    climate: start_climate,
                    formation_spec: &surface_spec,
                    water_inventory_m3: primary_relief.water_inventory_m3(),
                },
                SURFACE_FORMATION_HORIZON_YEARS,
                &cancellation,
            )
            .unwrap()
            .into_parts();
            let final_terrain = state
                .project_final_terrain(
                    surface,
                    final_sediment,
                    primary_relief.water_inventory_m3(),
                    &cancellation,
                )
                .unwrap();
            drop(state);
            let forcing = GlobalClimateForcingBuilder::build_for_formation_terrain(
                surface,
                &final_terrain,
                substrate.relative_permeability(),
                &climate_spec,
                &domain,
                &cancellation,
            )
            .unwrap();
            let endpoint =
                |label: &'static str,
                 schedule: OfflineSchedule,
                 carrier: Option<&StartAtmosphereFasCorrection>| {
                    timed(label, schedule, || {
                        GlobalCirculationGenerator::generate_endpoint(
                            surface,
                            &domain,
                            &forcing,
                            start_climate,
                            carrier,
                            &cancellation,
                        )
                        .map(|(snapshot, guess)| (snapshot, format!("{guess:?}")))
                        .map_err(|error| error.to_string())
                    })
                };
            let reuse = |injection: EndpointInjection| {
                carrier
                    .as_ref()
                    .map(|carrier| carrier.with_injection(injection))
            };
            let differential = reuse(EndpointInjection::Differential);
            let floor_corrected = reuse(EndpointInjection::FloorCorrectedAbsolute);
            let production = OfflineSchedule::default();
            let t = endpoint("T", production, None);
            let u = endpoint("U", production, differential.as_ref());
            let u_prime = endpoint("U'", production, floor_corrected.as_ref());
            let r = endpoint(
                "R",
                OfflineSchedule {
                    run_every_cycle: true,
                    ..production
                },
                None,
            );
            let t96 = endpoint(
                "T96",
                OfflineSchedule {
                    mechanical_coarse_cycles: Some(T96_COARSE_CYCLES),
                    ..production
                },
                None,
            );
            let r16 = (seed == PROBE_SEED).then(|| {
                endpoint(
                    "R16",
                    OfflineSchedule {
                        maximum_formation_cycles: Some(R16_FORMATION_CYCLES),
                        run_every_cycle: true,
                        ..production
                    },
                    None,
                )
            });

            let quality = [&t, &u, &u_prime]
                .into_iter()
                .filter_map(|run| {
                    let report = evaluate_global_circulation_quality_for_formation_cancellable(
                        surface,
                        &final_terrain,
                        &forcing,
                        run.snapshot.as_ref()?,
                        &cancellation,
                    )
                    .unwrap();
                    let precipitation = report
                        .metrics()
                        .iter()
                        .find(|metric| metric.id().name() == "precipitation-global-mean-mm-day")
                        .and_then(QualityMetric::value);
                    Some(QualityRecord {
                        label: run.record.label,
                        gpcp_precipitation_mm_day: precipitation,
                        gpcp_envelope_mm_day: [gpcp_minimum, gpcp_maximum],
                        gpcp_inside: precipitation
                            .map(|value| (gpcp_minimum..=gpcp_maximum).contains(&value)),
                        metrics: report.metrics().to_vec(),
                    })
                })
                .collect();
            let candidates = [&t, &u, &u_prime, &t96, &r];
            let comparison = SeedComparison {
                seed,
                profile,
                activity,
                runs: [&start, &t, &u, &u_prime, &r, &t96]
                    .into_iter()
                    .chain(r16.as_ref())
                    .map(|run| run.record.clone())
                    .collect(),
                error_vs_r: errors_against(&candidates[..4], Some(&r)),
                error_vs_r16: errors_against(&candidates, r16.as_ref()),
                distance_to_t: errors_against(&[&start, &u, &u_prime, &t96, &r], Some(&t)),
                correction_drift: start
                    .capture
                    .fas_correction
                    .as_deref()
                    .zip(t.capture.fas_correction.as_deref())
                    .map(|(start, endpoint)| correction_drift(start, endpoint)),
                quality,
            };
            let path = directory.join(format!("{profile:?}-{seed}.json"));
            std::fs::write(&path, serde_json::to_vec_pretty(&comparison).unwrap()).unwrap();
            eprintln!("[a8b] wrote {}", path.display());
        }
    }
}
