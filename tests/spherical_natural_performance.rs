use std::mem::size_of_val;
use std::time::Instant;

use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::{MantleGenerator, TectonicGenerator};
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
#[ignore = "release-only ~20,000-cell spherical tectonic and mantle measurement"]
fn release_spherical_tectonic_and_mantle_budget() {
    let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 20_000,
    })
    .unwrap();
    assert_eq!(surface.cells().len(), 20_252);
    let baseline_working_set = process_working_set_bytes();

    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let tectonic_started = Instant::now();
    let tectonic = TectonicGenerator::generate_spherical(
        &surface,
        &TectonicSpec::default(),
        &formation,
        &mut stage_rng("performance.tectonic"),
    )
    .unwrap();
    let tectonic_elapsed = tectonic_started.elapsed();
    let tectonic_working_set = process_working_set_bytes();

    let mantle_started = Instant::now();
    let mantle = MantleGenerator::generate_spherical(
        &surface,
        &GeologicSpec::default(),
        MantleFormationBias::Neutral,
        &mut stage_rng("performance.mantle"),
    )
    .unwrap();
    let mantle_elapsed = mantle_started.elapsed();
    let final_working_set = process_working_set_bytes();

    tectonic.validate_against(&surface).unwrap();
    mantle.validate_against(&surface).unwrap();
    let tectonic_persistent_bytes = size_of_val(tectonic.plates())
        + size_of_val(tectonic.cell_plates().raw_values())
        + size_of_val(tectonic.crust_kinds().raw_values())
        + size_of_val(tectonic.crust_thickness_km())
        + size_of_val(tectonic.boundaries())
        + size_of_val(tectonic.boundary_segments())
        + tectonic
            .boundary_segments()
            .iter()
            .map(|segment| size_of_val(segment.member_edges()))
            .sum::<usize>();
    let mantle_persistent_bytes = size_of_val(mantle.hotspots())
        + size_of_val(mantle.heat_flow_mw_m2())
        + size_of_val(mantle.volcanic_influence());
    let persistent_bytes = tectonic_persistent_bytes + mantle_persistent_bytes;
    let combined_ms = (tectonic_elapsed + mantle_elapsed).as_secs_f64() * 1_000.0;
    let working_set_delta = baseline_working_set
        .zip(final_working_set)
        .map(|(before, after)| after.saturating_sub(before));

    eprintln!(
        "spherical_natural_performance cells={} edges={} plates={} boundary_segments={} hotspots={} tectonic_ms={:.3} mantle_ms={:.3} combined_ms={combined_ms:.3} tectonic_persistent_bytes={tectonic_persistent_bytes} mantle_persistent_bytes={mantle_persistent_bytes} persistent_bytes={persistent_bytes} baseline_working_set_bytes={baseline_working_set:?} tectonic_working_set_bytes={tectonic_working_set:?} final_working_set_bytes={final_working_set:?} working_set_delta_bytes={working_set_delta:?}",
        surface.cells().len(),
        surface.edges().len(),
        tectonic.plates().len(),
        tectonic.boundary_segments().len(),
        mantle.hotspots().len(),
        tectonic_elapsed.as_secs_f64() * 1_000.0,
        mantle_elapsed.as_secs_f64() * 1_000.0,
    );
    assert!(
        combined_ms <= COMBINED_GENERATION_BUDGET_MS,
        "combined spherical generation took {combined_ms:.3} ms; budget is {COMBINED_GENERATION_BUDGET_MS:.3} ms"
    );
    assert!(
        persistent_bytes <= PERSISTENT_MEMORY_BUDGET_BYTES,
        "spherical tectonic and mantle data use {persistent_bytes} bytes; budget is {PERSISTENT_MEMORY_BUDGET_BYTES}"
    );
}
