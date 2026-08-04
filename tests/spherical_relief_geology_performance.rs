use std::mem::size_of_val;
use std::time::Instant;

use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{
    GeologicGenerator, MantleGenerator, ReliefGenerator, TectonicGenerator,
};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    GeologicSpec, MantleFormationBias, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    TectonicSpec, WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

const COMBINED_GENERATION_BUDGET_MS: f64 = 5_000.0;
const PERSISTENT_MEMORY_BUDGET_BYTES: usize = 256 * 1024 * 1024;

fn stage_rng(name: &'static str) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new(name, 1, "sekai.performance"),
    ))
}

#[cfg(windows)]
fn process_working_set_bytes() -> Option<u64> {
    use std::process::Command;

    let script = format!("(Get-Process -Id {}).WorkingSet64", std::process::id());
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn process_working_set_bytes() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
        .map(|kilobytes| kilobytes * 1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn process_working_set_bytes() -> Option<u64> {
    None
}

#[test]
#[ignore = "release-only ~20,000-cell spherical relief and geology measurement"]
fn release_spherical_relief_and_geology_budget() {
    let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 20_000,
    })
    .unwrap();
    assert_eq!(surface.cells().len(), 20_252);
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let tectonic = TectonicGenerator::generate_spherical(
        &surface,
        &TectonicSpec::default(),
        &formation,
        &mut stage_rng("performance.tectonic"),
    )
    .unwrap();
    let geologic_spec = GeologicSpec::default();
    let mantle = MantleGenerator::generate_spherical(
        &surface,
        &geologic_spec,
        MantleFormationBias::Neutral,
        &mut stage_rng("performance.mantle"),
    )
    .unwrap();
    let baseline_working_set = process_working_set_bytes();

    let mut diagnostics = Vec::new();
    let relief_started = Instant::now();
    let relief = ReliefGenerator::generate_spherical(
        &surface,
        &tectonic,
        &mantle,
        &mut stage_rng("performance.relief"),
        &mut diagnostics,
    )
    .unwrap();
    let relief_elapsed = relief_started.elapsed();
    let relief_working_set = process_working_set_bytes();

    let geology_started = Instant::now();
    let geology = GeologicGenerator::generate_spherical(
        &surface,
        &tectonic,
        &mantle,
        &relief,
        &geologic_spec,
        &mut stage_rng("performance.geology"),
    )
    .unwrap();
    let geology_elapsed = geology_started.elapsed();
    let final_working_set = process_working_set_bytes();

    relief
        .validate_against(&surface, &tectonic, &mantle)
        .unwrap();
    geology
        .validate_against(&surface, &tectonic, &mantle, &relief)
        .unwrap();
    let relief_persistent_bytes = size_of_val(relief.crust_base_elevation_m().values())
        + size_of_val(relief.tectonic_offset_m().values())
        + size_of_val(relief.volcanic_offset_m().values())
        + size_of_val(relief.regional_offset_m().values())
        + size_of_val(relief.elevation_m().values())
        + size_of_val(relief.land_ocean().raw_values());
    let geology_persistent_bytes = size_of_val(geology.bedrock_kinds().raw_values())
        + size_of_val(geology.fracture_intensity())
        + size_of_val(geology.erosion_resistance())
        + size_of_val(geology.relative_permeability())
        + size_of_val(geology.metallic_mineral_potential())
        + size_of_val(geology.geothermal_potential())
        + size_of_val(geology.sedimentary_basin_potential());
    let persistent_bytes = relief_persistent_bytes + geology_persistent_bytes;
    let combined_ms = (relief_elapsed + geology_elapsed).as_secs_f64() * 1_000.0;
    let working_set_delta = baseline_working_set
        .zip(final_working_set)
        .map(|(before, after)| after.saturating_sub(before));

    eprintln!(
        "spherical_relief_geology_performance cells={} edges={} relief_ms={:.3} geology_ms={:.3} combined_ms={combined_ms:.3} diagnostics={} relief_persistent_bytes={relief_persistent_bytes} geology_persistent_bytes={geology_persistent_bytes} persistent_bytes={persistent_bytes} baseline_working_set_bytes={baseline_working_set:?} relief_working_set_bytes={relief_working_set:?} final_working_set_bytes={final_working_set:?} working_set_delta_bytes={working_set_delta:?}",
        surface.cells().len(),
        surface.edges().len(),
        relief_elapsed.as_secs_f64() * 1_000.0,
        geology_elapsed.as_secs_f64() * 1_000.0,
        diagnostics.len(),
    );
    assert!(
        combined_ms <= COMBINED_GENERATION_BUDGET_MS,
        "combined spherical relief/geology generation took {combined_ms:.3} ms; budget is {COMBINED_GENERATION_BUDGET_MS:.3} ms"
    );
    assert!(
        persistent_bytes <= PERSISTENT_MEMORY_BUDGET_BYTES,
        "spherical relief/geology data use {persistent_bytes} bytes; budget is {PERSISTENT_MEMORY_BUDGET_BYTES}"
    );
}
