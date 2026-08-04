use std::mem::size_of_val;
use std::time::Instant;

use sekai::generators::natural::ClimateGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    ClimateSpec, ElevationField, LandOceanField, SphericalReliefSnapshot, RELIEF_SCHEMA_V4,
};
use sekai::world::spatial::SurfaceRef;
use sekai::world::{Meters, SphericalSpaceSpec};

const CLIMATE_GENERATION_BUDGET_MS: f64 = 5_000.0;
const PERSISTENT_MEMORY_BUDGET_BYTES: usize = 64 * 1024 * 1024;

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
#[ignore = "release-only ~20,000-cell spherical preliminary-climate measurement"]
fn release_spherical_preliminary_climate_budget() {
    let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 20_000,
    })
    .unwrap();
    assert_eq!(surface.cells().len(), 20_252);
    let elevation = surface
        .cells()
        .iter()
        .map(|cell| {
            let radial = cell.centroid.components();
            if radial[0] + radial[2] * 0.35 > 0.05 {
                180.0 + radial[2].abs() as f32 * 900.0
            } else {
                -2_800.0
            }
        })
        .collect::<Vec<_>>();
    let zero = vec![0.0; elevation.len()];
    let final_elevation = ElevationField::from_values(elevation.clone()).unwrap();
    let relief = SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::for_spherical(&surface),
        0.0,
        ElevationField::from_values(elevation).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero).unwrap(),
        final_elevation.clone(),
        LandOceanField::classify(&final_elevation, 0.0),
    )
    .unwrap();
    let baseline_working_set = process_working_set_bytes();

    let started = Instant::now();
    let climate =
        ClimateGenerator::generate_spherical(&surface, &relief, &ClimateSpec::default()).unwrap();
    let elapsed = started.elapsed();
    let final_working_set = process_working_set_bytes();
    climate.validate_against(&surface, &relief).unwrap();

    let persistent_bytes = size_of_val(climate.latitude_degrees())
        + size_of_val(climate.maritime_influence())
        + size_of_val(climate.monthly_air_temperature_c().values())
        + size_of_val(climate.monthly_precipitation_mm().values())
        + size_of_val(climate.monthly_wind_m_s().values())
        + size_of_val(climate.mean_annual_air_temperature_c())
        + size_of_val(climate.temperature_seasonality_c())
        + size_of_val(climate.annual_precipitation_mm())
        + size_of_val(climate.prevailing_wind_m_s());
    let elapsed_ms = elapsed.as_secs_f64() * 1_000.0;
    let working_set_delta = baseline_working_set
        .zip(final_working_set)
        .map(|(before, after)| after.saturating_sub(before));

    eprintln!(
        "spherical_climate_performance cells={} edges={} climate_ms={elapsed_ms:.3} persistent_bytes={persistent_bytes} baseline_working_set_bytes={baseline_working_set:?} final_working_set_bytes={final_working_set:?} working_set_delta_bytes={working_set_delta:?}",
        surface.cells().len(),
        surface.edges().len(),
    );
    assert!(
        elapsed_ms <= CLIMATE_GENERATION_BUDGET_MS,
        "spherical preliminary-climate generation took {elapsed_ms:.3} ms; budget is {CLIMATE_GENERATION_BUDGET_MS:.3} ms"
    );
    assert!(
        persistent_bytes <= PERSISTENT_MEMORY_BUDGET_BYTES,
        "spherical preliminary-climate data use {persistent_bytes} bytes; budget is {PERSISTENT_MEMORY_BUDGET_BYTES}"
    );
}
