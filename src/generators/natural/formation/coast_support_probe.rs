//! Offline candidates bypass only the experimental profile admission check.

use super::*;
use crate as coast_probe_crate;

mod evidence {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/procedural_coast.rs"
    ));
}

struct ProceduralCoastCandidateStage;

impl Stage for ProceduralCoastCandidateStage {
    type Inputs = CausalNaturalFormationStageInputs;
    type Output = NaturalFormationBundleArtifact;

    fn id(&self) -> StageId {
        CausalNaturalFormationStage.id()
    }

    fn version(&self) -> u32 {
        CausalNaturalFormationStage.version()
    }

    fn namespace(&self) -> &'static str {
        CausalNaturalFormationStage.namespace()
    }

    fn run(
        &self,
        inputs: Self::Inputs,
        rng: &mut StageRng,
        _diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<Self::Output, StageError> {
        rng.check_cancelled().map_err(|_| cancelled())?;
        let cancellation = rng.cancellation_signal();
        let profile_bundle = ProfileSurfaceBuilder::complete(
            inputs.profile.profile(),
            inputs.surface.snapshot(),
            &cancellation,
        )
        .map_err(profile_failure)?;
        // Use the unchanged production generator, quality/budget validators,
        // stage identity and random streams; only the admission guard is absent.
        NaturalFormationBundleArtifact::generate(
            CausalNaturalFormationInputs {
                profile_bundle: &profile_bundle,
                quality_profile: inputs.profile.profile(),
                tectonic_spec: inputs.resolved_tectonic.input().spec(),
                formation: inputs.formation.formation(),
                geologic_spec: inputs.resolved_geologic.input().spec(),
                relief_spec: inputs.relief_spec.spec(),
                climate_domain: inputs.domain.snapshot(),
                climate_spec: inputs.resolved_climate.input().spec(),
                surface_spec: inputs.resolved_hydro_erosion.input().spec(),
            },
            rng,
            &cancellation,
        )
        .map_err(product_failure)
    }
}

fn run_candidate(profile: NaturalQualityProfile, output_name: &str) {
    let graph = StageGraphBuilder::new()
        .external::<NaturalQualityProfileArtifact>()
        .external::<ResolvedTectonicInputArtifact>()
        .external::<ResolvedWorldFormationArtifact>()
        .external::<ResolvedGeologicInputArtifact>()
        .external::<ReliefSpecArtifact>()
        .external::<ResolvedClimateInputArtifact>()
        .external::<ResolvedHydroErosionInputArtifact>()
        .external::<SphericalSurfaceArtifact>()
        .stage(ClimateWorkDomainStage)
        .stage(ProceduralCoastCandidateStage)
        .build()
        .unwrap();
    let product = evidence::generate_procedural_coast_preview(
        profile,
        evidence::target25_relief(),
        output_name,
        graph,
    );
    write_wind_jump_witness(&product, output_name);
}

fn write_wind_jump_witness(product: &crate::engine::BuildOutcome, output_name: &str) {
    use crate::generators::natural::quality::take_wind_jump_location;
    use crate::generators::natural::GlobalClimateForcingBuilder;

    let (edge_index, month, rms_m_s) = take_wind_jump_location()
        .expect("the production quality operator observed a nonzero wind jump");
    let surface = product.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
    let surface = surface.snapshot();
    let domain = product
        .artifacts
        .get::<ClimateWorkDomainArtifact>()
        .unwrap();
    let domain = domain.snapshot();
    let artifact = product
        .artifacts
        .get::<NaturalFormationBundleArtifact>()
        .unwrap();
    let bundle = artifact.bundle();
    let terrain = bundle.surface_formation().terrain_fields();
    let fields = bundle.climate().fields();
    let climate_input = product
        .artifacts
        .get::<ResolvedClimateInputArtifact>()
        .unwrap();
    let forcing = GlobalClimateForcingBuilder::build_for_formation_terrain(
        surface,
        terrain,
        bundle.substrate().relative_permeability(),
        climate_input.input().spec(),
        domain,
        &BuildCancellation::new(),
    )
    .unwrap();
    assert_eq!(
        forcing.fingerprint(),
        bundle.climate().checkpoint().forcing_fingerprint()
    );
    let cells = surface.edges()[edge_index].cells.map(|id| {
        let index = id.raw() as usize;
        let contributors = domain
            .climate_to_source()
            .target_row(id)
            .unwrap()
            .iter()
            .map(|weight| {
                let work_cell = weight.source_cell().raw() as usize;
                serde_json::json!({
                    "work_cell": work_cell,
                    "position": domain.climate_surface().cells()[work_cell].centroid,
                    "overlap_area_m2": weight.area_m2(),
                    "land_fraction": forcing.planet_forcing().land_fraction()[work_cell],
                    "relative_elevation_m": forcing.relative_elevation_m()[work_cell],
                    "terrain_gradient": forcing.terrain_gradient_m_per_m()[work_cell],
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "cell": index,
            "position": surface.cells()[index].centroid,
            "area_m2": surface.cells()[index].area.get(),
            "elevation_m": terrain.current_elevation_m()[index],
            "land_fraction": forcing.source_land_fraction()[index],
            "publishes_land": forcing.source_publishes_land()[index],
            "wind_m_s": fields.near_surface_wind_m_s().values()[index][month],
            "upper_wind_m_s": fields.upper_wind_m_s().map(|wind| wind.values()[index][month]),
            "work_contributors": contributors,
        })
    });
    evidence::write_report(
        &output_name.replace("full-chain", "wind-jump"),
        &serde_json::json!({
            "edge_index": edge_index, "month_zero_based": month,
            "rms_wind_m_s": rms_m_s, "cells": cells,
            "forcing_fingerprint": forcing.fingerprint(),
            "climate_fingerprint": bundle.climate().checkpoint().fingerprint(),
        }),
    );
}

#[test]
#[ignore = "offline procedural coast Standard support candidate; run in Release"]
fn procedural_coast_standard42_support_candidate() {
    run_candidate(
        NaturalQualityProfile::Standard,
        "full-chain-target25-standard.json",
    );
}

#[test]
#[ignore = "offline procedural coast High support candidate; run only after Standard"]
fn procedural_coast_high42_support_candidate() {
    run_candidate(NaturalQualityProfile::High, "full-chain-target25-high.json");
}
