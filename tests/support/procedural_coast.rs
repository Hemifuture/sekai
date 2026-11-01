use std::time::Instant;

// The integration probe and the private stage candidate share the same evidence collector.
use super::coast_probe_crate as sekai;

use sekai::app::build_spherical_formation_external_artifacts;
use sekai::app::SphericalFormationFieldDocument;
use sekai::engine::{
    Artifact, BuildCancellation, BuildEngine, BuildOutcome, MemoryStageCache, StageGraph,
};
use sekai::generators::natural::{
    fibonacci_probe, ClimateWorkDomainArtifact, FormationDerivationInputs, HierarchicalEvaluator,
    NaturalFormationBundleArtifact,
};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    GeologicSpec, NaturalQualityProfile, ReliefSpec, SeaLevelPolicy, SurfaceWaterKind,
    TectonicSpec, WorldFormationPreset, WorldFormationSpec, EARTH_WATER_REFERENCE_RADIUS_M,
};
use sekai::world::{Meters, RootSeed};

pub(super) fn target25_relief() -> ReliefSpec {
    ReliefSpec {
        sea_level_policy: SeaLevelPolicy::TargetLandFraction,
        target_land_fraction: 0.25,
        ..ReliefSpec::default()
    }
}

pub(super) fn generate_procedural_coast_preview(
    profile: NaturalQualityProfile,
    relief: ReliefSpec,
    output_name: &str,
    graph: StageGraph,
) -> BuildOutcome {
    let seed = RootSeed::new(42);
    let surface = ProfileSurfaceBuilder::build(
        profile,
        Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = surface.authoritative_surface();
    let formation = WorldFormationSpec {
        preset: WorldFormationPreset::ProceduralContinents,
        ..WorldFormationSpec::default()
    };
    let external = build_spherical_formation_external_artifacts(
        seed,
        profile,
        surface,
        &formation,
        &TectonicSpec::default(),
        &relief,
        &GeologicSpec::default(),
    )
    .unwrap();
    let start = Instant::now();
    let product = BuildEngine::new(graph)
        .build(seed, external, &mut MemoryStageCache::new())
        .unwrap_or_else(|error| {
            write_report(
                output_name,
                &serde_json::json!({
                    "profile": profile,
                    "seed": 42,
                    "relief_spec": relief,
                    "generation_error": error.to_string(),
                }),
            );
            panic!("procedural coast candidate failed: {error}");
        });
    let elapsed_ms = start.elapsed().as_secs_f64() * 1_000.0;
    let artifact = product
        .artifacts
        .get::<NaturalFormationBundleArtifact>()
        .unwrap();
    artifact.validate().unwrap();
    let bundle = artifact.bundle();
    let domain = product
        .artifacts
        .get::<ClimateWorkDomainArtifact>()
        .unwrap();
    let domain = domain.snapshot();
    let terrain = bundle.surface_formation().terrain_fields();
    let geometry = terrain.surface_water_geometry();
    let document = SphericalFormationFieldDocument::from_build_outcome(&product).unwrap();
    assert_eq!(
        document.area_summary().actual_land_fraction(),
        f64::from(geometry.global_land_area_fraction(surface).unwrap()),
        "the UI must report the same fractional coastline area as the physical geometry"
    );
    assert_eq!(
        bundle
            .surface_formation()
            .checkpoint()
            .upstream()
            .formation_climate_checkpoint_fingerprint(),
        bundle.climate().checkpoint().fingerprint()
    );
    let mean_change = surface
        .cells()
        .iter()
        .zip(bundle.primary_relief().elevation_m())
        .zip(terrain.current_elevation_m())
        .map(|((cell, &before), &after)| {
            cell.area.get() * (f64::from(after) - f64::from(before)).abs()
        })
        .sum::<f64>()
        / surface.total_cell_area().get();
    assert!(
        mean_change > 0.0,
        "the candidate must traverse the surface-process chain"
    );
    let evaluator = HierarchicalEvaluator::from_formation_product(
        FormationDerivationInputs {
            surface,
            compatibility: bundle.tectonics().compatibility(),
            substrate: bundle.substrate(),
            formation: bundle.surface_formation(),
            climate: bundle.climate(),
        },
        seed,
    )
    .unwrap();
    // Same probe set as the existing hierarchical product gate. Report against
    // P5's fractional ocean geometry, not the old whole-cell color classification.
    let total = 16_384;
    let mut shoreline = Vec::new();
    for level in [1, 6] {
        let mut ocean = 0;
        for index in 0..total {
            let reading = evaluator.read(fibonacci_probe(index, total), level);
            ocean += usize::from(reading.water == SurfaceWaterKind::Ocean);
        }
        shoreline.push(serde_json::json!({
            "level": level,
            "ocean_fraction": ocean as f64 / total as f64,
            "ocean_fraction_absolute_error": (ocean as f64 / total as f64
                - (1.0 - f64::from(geometry.global_land_area_fraction(surface).unwrap()))).abs(),
        }));
    }
    let report = serde_json::json!({
        "relief_spec": relief,
        "seed": 42, "profile": profile, "preset": formation.preset,
        "source_cells": surface.cells().len(), "graph_elapsed_ms": elapsed_ms,
        "initial_land_fraction": bundle.primary_relief().surface_water_geometry()
            .global_land_area_fraction(surface).unwrap(),
        "final_land_fraction": geometry.global_land_area_fraction(surface).unwrap(),
        "sea_level_m": geometry.sea_level_m(),
        "mean_absolute_surface_process_change_m": mean_change,
        // These are the reports already evaluated against final terrain by the
        // production artifact, not a second implementation of its quality metrics.
        "climate_budget": bundle.climate().budget_report(),
        "climate_quality": bundle.climate_quality(),
        "primary_relief_quality": bundle.primary_relief_quality(),
        "water_inventory_m3": terrain.water_inventory_m3(),
        "realized_water_volume_m3": terrain.realized_water_volume_m3(),
        "sediment_budget": bundle.surface_formation().sediment_budget_report(),
        "identity": {
            "surface": bundle.surface_ref(),
            "climate_checkpoint": bundle.climate().checkpoint().fingerprint(),
            "climate_forcing": bundle.climate().checkpoint().forcing_fingerprint(),
            "climate_input": bundle.climate().checkpoint().input_fingerprint(),
            "final_terrain": terrain.fingerprint(),
            "surface_checkpoint": bundle.surface_formation().checkpoint().fingerprint(),
        },
        "work_domain": {
            "climate_model": bundle.climate().profile(),
            "climate_face_resolution": domain.face_resolution(),
            "climate_cells": domain.climate_surface().cells().len(),
            "source_mean_cell_area_m2": surface.total_cell_area().get()
                / surface.cells().len() as f64,
            "climate_mean_cell_area_m2": domain.climate_surface().total_cell_area().get()
                / domain.climate_surface().cells().len() as f64,
        },
        "surface_quality": bundle.surface_quality(),
        "tectonic_quality": bundle.tectonic_quality(),
        "fine_shoreline": shoreline,
    });
    write_report(output_name, &report);
    product
}

pub(super) fn write_report(output_name: &str, report: &serde_json::Value) {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/coast-climate-preview");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(output_name),
        serde_json::to_vec_pretty(report).unwrap(),
    )
    .unwrap();
}
