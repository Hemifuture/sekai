use std::mem::size_of_val;
use std::time::Instant;

use sekai::generators::natural::HydroErosionGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BedrockKind, BedrockKindField, ElevationField, HydroErosionSpec, LandOceanField, LandOceanKind,
    MonthlyScalarField, MonthlyVector3Field, SphericalGeologicSnapshot,
    SphericalPreliminaryClimateSnapshot, SphericalReliefSnapshot, CLIMATE_MONTH_COUNT,
    GEOLOGIC_SNAPSHOT_SCHEMA_V2, PRELIMINARY_CLIMATE_SCHEMA_V2, RELIEF_SCHEMA_V4,
};
use sekai::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef};
use sekai::world::{Meters, SphericalSpaceSpec};

const GENERATION_BUDGET_MS: f64 = 5_000.0;
const PERSISTENT_MEMORY_BUDGET_BYTES: usize = 128 * 1024 * 1024;
const SERIALIZED_BUDGET_BYTES: usize = 64 * 1024 * 1024;

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
#[ignore = "release-only ~20,000-cell spherical hydro-erosion measurement"]
fn release_spherical_hydro_erosion_budget() {
    let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(6_371_000.0).unwrap(),
        target_cell_count: 20_000,
    })
    .unwrap();
    assert_eq!(surface.cells().len(), 20_252);
    let relief = mixed_relief(&surface);
    let geology = uniform_geology(&surface);
    let climate = uniform_climate(&surface, &relief);
    let baseline_working_set = process_working_set_bytes();

    let started = Instant::now();
    let snapshot = HydroErosionGenerator::generate_spherical(
        &surface,
        &relief,
        &geology,
        &climate,
        &HydroErosionSpec::default(),
    )
    .unwrap();
    let elapsed = started.elapsed();
    let final_working_set = process_working_set_bytes();
    snapshot
        .validate_against(&surface, &relief, &geology, &climate)
        .unwrap();

    let process = snapshot.surface();
    let hydrology = snapshot.hydrology();
    let persistent_bytes = size_of_val(&snapshot)
        + size_of_val(process.erosion_depth_m())
        + size_of_val(process.deposition_thickness_m())
        + size_of_val(process.surface_elevation_m().values())
        + size_of_val(process.sediment_throughput_m3())
        + size_of_val(hydrology.monthly_local_runoff_mm())
        + size_of_val(hydrology.monthly_discharge_m3_s())
        + size_of_val(hydrology.annual_local_runoff_mm())
        + size_of_val(hydrology.mean_annual_discharge_m3_s())
        + size_of_val(hydrology.drainage_area_km2())
        + size_of_val(hydrology.drainage_surface_elevation_m().values())
        + size_of_val(hydrology.lake_depth_m())
        + size_of_val(hydrology.surface_water().raw_values())
        + size_of_val(hydrology.flow_receiver())
        + size_of_val(hydrology.basin_id())
        + size_of_val(hydrology.strahler_order().raw_values())
        + size_of_val(hydrology.basins())
        + size_of_val(hydrology.lakes())
        + hydrology
            .lakes()
            .iter()
            .map(|lake| size_of_val(lake.cells()))
            .sum::<usize>()
        + size_of_val(hydrology.river_segments())
        + size_of_val(hydrology.river_segment_length_m());
    let serialized_bytes = serde_json::to_vec(&snapshot).unwrap().len();
    let elapsed_ms = elapsed.as_secs_f64() * 1_000.0;
    let working_set_delta = baseline_working_set
        .zip(final_working_set)
        .map(|(before, after)| after.saturating_sub(before));

    eprintln!(
        "spherical_hydro_erosion_performance cells={} edges={} elapsed_ms={elapsed_ms:.3} basins={} lakes={} rivers={} persistent_bytes={persistent_bytes} serialized_bytes={serialized_bytes} baseline_working_set_bytes={baseline_working_set:?} final_working_set_bytes={final_working_set:?} working_set_delta_bytes={working_set_delta:?} ocean_delivery_m3={} endorheic_storage_m3={}",
        surface.cells().len(),
        surface.edges().len(),
        hydrology.basins().len(),
        hydrology.lakes().len(),
        hydrology.river_segments().len(),
        process.sediment_ocean_delivery_m3(),
        process.sediment_endorheic_storage_m3(),
    );
    assert!(
        elapsed_ms <= GENERATION_BUDGET_MS,
        "spherical hydro-erosion took {elapsed_ms:.3} ms; budget is {GENERATION_BUDGET_MS:.3} ms"
    );
    assert!(
        persistent_bytes <= PERSISTENT_MEMORY_BUDGET_BYTES,
        "spherical hydro-erosion data use {persistent_bytes} bytes; budget is {PERSISTENT_MEMORY_BUDGET_BYTES}"
    );
    assert!(
        serialized_bytes <= SERIALIZED_BUDGET_BYTES,
        "serialized spherical hydro-erosion uses {serialized_bytes} bytes; budget is {SERIALIZED_BUDGET_BYTES}"
    );
}

fn mixed_relief(surface: &SphericalSurfaceSnapshot) -> SphericalReliefSnapshot {
    let elevation = ElevationField::from_values(
        surface
            .cells()
            .iter()
            .map(|cell| {
                let [x, _, z] = cell.centroid.components();
                (1_400.0 * x + 500.0 * z - 120.0) as f32
            })
            .collect(),
    )
    .unwrap();
    let zero = vec![0.0; surface.cells().len()];
    SphericalReliefSnapshot::new(
        RELIEF_SCHEMA_V4,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        0.0,
        elevation.clone(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero.clone()).unwrap(),
        ElevationField::from_values(zero).unwrap(),
        elevation.clone(),
        LandOceanField::classify(&elevation, 0.0),
    )
    .unwrap()
}

fn uniform_geology(surface: &SphericalSurfaceSnapshot) -> SphericalGeologicSnapshot {
    let count = surface.cells().len();
    SphericalGeologicSnapshot::new(
        GEOLOGIC_SNAPSHOT_SCHEMA_V2,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        BedrockKindField::from_kinds(vec![BedrockKind::ContinentalCrystalline; count]),
        vec![0.25; count],
        vec![0.5; count],
        vec![0.25; count],
        vec![0.0; count],
        vec![0.0; count],
        vec![0.0; count],
    )
    .unwrap()
}

fn uniform_climate(
    surface: &SphericalSurfaceSnapshot,
    relief: &SphericalReliefSnapshot,
) -> SphericalPreliminaryClimateSnapshot {
    let count = surface.cells().len();
    let precipitation_mm = 120.0;
    SphericalPreliminaryClimateSnapshot::new(
        PRELIMINARY_CLIMATE_SCHEMA_V2,
        SurfaceRef::try_for_spherical(surface).unwrap(),
        surface
            .cells()
            .iter()
            .map(|cell| cell.centroid.components()[2].asin().to_degrees() as f32)
            .collect(),
        relief
            .land_ocean()
            .raw_values()
            .iter()
            .map(|&kind| {
                if kind == LandOceanKind::Ocean.raw() {
                    1.0
                } else {
                    0.0
                }
            })
            .collect(),
        MonthlyScalarField::from_values(vec![[18.0; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        MonthlyScalarField::from_values(vec![[precipitation_mm; CLIMATE_MONTH_COUNT]; count])
            .unwrap(),
        MonthlyVector3Field::from_values(vec![[[0.0; 3]; CLIMATE_MONTH_COUNT]; count]).unwrap(),
        vec![18.0; count],
        vec![0.0; count],
        vec![precipitation_mm * CLIMATE_MONTH_COUNT as f32; count],
        vec![[0.0; 3]; count],
    )
    .unwrap()
}
