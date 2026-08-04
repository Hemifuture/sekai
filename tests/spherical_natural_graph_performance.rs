use std::mem::size_of_val;
use std::time::{Duration, Instant};

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    legacy_planar_natural_foundation_graph, spherical_natural_foundation_graph,
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, HydroErosionSpecArtifact,
    ResolvedWorldFormationArtifact, RulePackSetArtifact, SphericalGeologicArtifact,
    SphericalHydroErosionArtifact, SphericalMantleArtifact, SphericalPreliminaryClimateArtifact,
    SphericalReliefArtifact, SphericalTectonicArtifact, TectonicSpecArtifact,
    WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{
    PlanarSpaceArtifact, SphericalSpaceArtifact, SphericalSurfaceArtifact,
};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    spherical_natural_field_registry, ClimateSpec, GeologicSpec, HydroErosionSpec, TectonicSpec,
    WorldFormationSpec,
};
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed, SphericalSpaceSpec};
use serde::Serialize;

const ROOT_SEED: RootSeed = RootSeed::new(42);
const TARGET_CELL_COUNT: u32 = 20_000;
const EARTH_RADIUS_M: f64 = 6_371_000.0;
const SPHERE_TIME_BUDGET: Duration = Duration::from_secs(5);
const SPHERE_TO_PLANAR_TIME_RATIO_BUDGET: f64 = 2.5;
const ADDITIONAL_PEAK_WORKING_SET_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct ArtifactBytes {
    persistent: usize,
    serialized: usize,
}

impl ArtifactBytes {
    fn measure<T: Serialize>(artifact: &T, persistent: usize) -> Self {
        Self {
            persistent,
            serialized: serde_json::to_vec(artifact).unwrap().len(),
        }
    }
}

#[cfg(windows)]
fn process_working_set_bytes() -> Option<u64> {
    windows_process_memory_property("WorkingSet64")
}

#[cfg(windows)]
fn process_peak_working_set_bytes() -> Option<u64> {
    windows_process_memory_property("PeakWorkingSet64")
}

#[cfg(windows)]
fn windows_process_memory_property(property: &str) -> Option<u64> {
    use std::process::Command;

    let script = format!("(Get-Process -Id {}).{property}", std::process::id());
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    output.status.success().then_some(())?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn process_working_set_bytes() -> Option<u64> {
    linux_process_status_bytes("VmRSS:")
}

#[cfg(target_os = "linux")]
fn process_peak_working_set_bytes() -> Option<u64> {
    linux_process_status_bytes("VmHWM:")
}

#[cfg(target_os = "linux")]
fn linux_process_status_bytes(field: &str) -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix(field))?
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

#[cfg(not(any(windows, target_os = "linux")))]
fn process_peak_working_set_bytes() -> Option<u64> {
    None
}

#[test]
#[ignore = "release-only 20,000-cell planar/spherical full-graph acceptance"]
fn release_spherical_natural_full_graph_budget() {
    let planar_engine = BuildEngine::new(legacy_planar_natural_foundation_graph().unwrap());
    let planar_external = planar_external_artifacts();
    let mut planar_cache = MemoryStageCache::new();
    let planar_started = Instant::now();
    let planar_outcome = planar_engine
        .build(ROOT_SEED, planar_external, &mut planar_cache)
        .unwrap();
    let planar_elapsed = planar_started.elapsed();
    assert_eq!(planar_outcome.report.stages().len(), 16);
    drop(planar_outcome);
    drop(planar_cache);
    let planar_peak_working_set = process_peak_working_set_bytes();

    let sphere_engine = BuildEngine::new(spherical_natural_foundation_graph().unwrap());
    let sphere_external = spherical_external_artifacts();
    let mut sphere_cache = MemoryStageCache::new();
    let baseline_working_set = process_working_set_bytes();
    let sphere_started = Instant::now();
    let sphere_outcome = sphere_engine
        .build(ROOT_SEED, sphere_external, &mut sphere_cache)
        .unwrap();
    let sphere_elapsed = sphere_started.elapsed();
    let sphere_peak_working_set = process_peak_working_set_bytes();

    let surface = sphere_outcome
        .artifacts
        .get::<SphericalSurfaceArtifact>()
        .unwrap();
    let formation = sphere_outcome
        .artifacts
        .get::<ResolvedWorldFormationArtifact>()
        .unwrap();
    let tectonic = sphere_outcome
        .artifacts
        .get::<SphericalTectonicArtifact>()
        .unwrap();
    let mantle = sphere_outcome
        .artifacts
        .get::<SphericalMantleArtifact>()
        .unwrap();
    let relief = sphere_outcome
        .artifacts
        .get::<SphericalReliefArtifact>()
        .unwrap();
    let geology = sphere_outcome
        .artifacts
        .get::<SphericalGeologicArtifact>()
        .unwrap();
    let climate = sphere_outcome
        .artifacts
        .get::<SphericalPreliminaryClimateArtifact>()
        .unwrap();
    let hydro = sphere_outcome
        .artifacts
        .get::<SphericalHydroErosionArtifact>()
        .unwrap();

    validate_final_product(
        &surface, &formation, &tectonic, &mantle, &relief, &geology, &climate, &hydro,
    );
    assert_eq!(sphere_outcome.report.stages().len(), 16);
    let provenance = sphere_outcome.verified_provenance().unwrap();
    assert_eq!(provenance.root_seed(), ROOT_SEED);
    assert_eq!(
        Some(provenance.result_hash()),
        sphere_outcome.report.result_hash()
    );
    let final_working_set = process_working_set_bytes();
    let additional_working_set_bytes = baseline_working_set
        .zip(final_working_set)
        .map(|(before, after)| after.saturating_sub(before));
    let additional_peak_working_set_bytes = planar_peak_working_set
        .zip(sphere_peak_working_set)
        .map(|(planar_peak, sphere_peak)| sphere_peak.saturating_sub(planar_peak));

    let surface_bytes = ArtifactBytes::measure(
        surface.as_ref(),
        spherical_surface_persistent_bytes(surface.as_ref()),
    );
    let formation_bytes =
        ArtifactBytes::measure(formation.as_ref(), size_of_val(formation.as_ref()));
    let tectonic_bytes = ArtifactBytes::measure(
        tectonic.as_ref(),
        spherical_tectonic_persistent_bytes(tectonic.as_ref()),
    );
    let mantle_bytes = ArtifactBytes::measure(
        mantle.as_ref(),
        spherical_mantle_persistent_bytes(mantle.as_ref()),
    );
    let relief_bytes = ArtifactBytes::measure(
        relief.as_ref(),
        spherical_relief_persistent_bytes(relief.as_ref()),
    );
    let geology_bytes = ArtifactBytes::measure(
        geology.as_ref(),
        spherical_geology_persistent_bytes(geology.as_ref()),
    );
    let climate_bytes = ArtifactBytes::measure(
        climate.as_ref(),
        spherical_climate_persistent_bytes(climate.as_ref()),
    );
    let hydro_bytes = ArtifactBytes::measure(
        hydro.as_ref(),
        spherical_hydro_persistent_bytes(hydro.as_ref()),
    );
    let artifact_bytes = [
        surface_bytes,
        formation_bytes,
        tectonic_bytes,
        mantle_bytes,
        relief_bytes,
        geology_bytes,
        climate_bytes,
        hydro_bytes,
    ];
    let persistent_total_bytes = artifact_bytes
        .iter()
        .map(|bytes| bytes.persistent)
        .sum::<usize>();
    let serialized_total_bytes = artifact_bytes
        .iter()
        .map(|bytes| bytes.serialized)
        .sum::<usize>();
    let sphere_to_planar_ratio = sphere_elapsed.as_secs_f64() / planar_elapsed.as_secs_f64();
    let stage_timings_ms = sphere_outcome
        .report
        .stages()
        .iter()
        .map(|stage| {
            format!(
                "{}:{:.3}",
                stage.stage_id(),
                stage.duration().as_secs_f64() * 1_000.0
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let surface_snapshot = surface.snapshot();
    let tectonic_snapshot = tectonic.snapshot();
    let mantle_snapshot = mantle.snapshot();
    let hydro_snapshot = hydro.snapshot().hydrology();

    eprintln!(
        "spherical_natural_graph_performance planar_ms={:.3} sphere_ms={:.3} sphere_to_planar_ratio={sphere_to_planar_ratio:.6} stages={} cells={} vertices={} edges={} plates={} boundary_segments={} hotspots={} basins={} lakes={} rivers={} persistent_surface_bytes={} persistent_formation_bytes={} persistent_tectonic_bytes={} persistent_mantle_bytes={} persistent_relief_bytes={} persistent_geology_bytes={} persistent_climate_bytes={} persistent_hydro_bytes={} persistent_total_bytes={persistent_total_bytes} serialized_surface_bytes={} serialized_formation_bytes={} serialized_tectonic_bytes={} serialized_mantle_bytes={} serialized_relief_bytes={} serialized_geology_bytes={} serialized_climate_bytes={} serialized_hydro_bytes={} serialized_total_bytes={serialized_total_bytes} baseline_working_set_bytes={baseline_working_set:?} final_working_set_bytes={final_working_set:?} additional_working_set_bytes={additional_working_set_bytes:?} planar_peak_working_set_bytes={planar_peak_working_set:?} sphere_peak_working_set_bytes={sphere_peak_working_set:?} additional_peak_working_set_bytes={additional_peak_working_set_bytes:?} stage_timings_ms={stage_timings_ms}",
        planar_elapsed.as_secs_f64() * 1_000.0,
        sphere_elapsed.as_secs_f64() * 1_000.0,
        sphere_outcome.report.stages().len(),
        surface_snapshot.cells().len(),
        surface_snapshot.vertices().len(),
        surface_snapshot.edges().len(),
        tectonic_snapshot.plates().len(),
        tectonic_snapshot.boundary_segments().len(),
        mantle_snapshot.hotspots().len(),
        hydro_snapshot.basins().len(),
        hydro_snapshot.lakes().len(),
        hydro_snapshot.river_segments().len(),
        surface_bytes.persistent,
        formation_bytes.persistent,
        tectonic_bytes.persistent,
        mantle_bytes.persistent,
        relief_bytes.persistent,
        geology_bytes.persistent,
        climate_bytes.persistent,
        hydro_bytes.persistent,
        surface_bytes.serialized,
        formation_bytes.serialized,
        tectonic_bytes.serialized,
        mantle_bytes.serialized,
        relief_bytes.serialized,
        geology_bytes.serialized,
        climate_bytes.serialized,
        hydro_bytes.serialized,
    );

    assert!(
        sphere_elapsed <= SPHERE_TIME_BUDGET,
        "spherical graph took {:.3} ms; budget is {:.3} ms",
        sphere_elapsed.as_secs_f64() * 1_000.0,
        SPHERE_TIME_BUDGET.as_secs_f64() * 1_000.0
    );
    assert!(
        sphere_elapsed.as_secs_f64()
            <= planar_elapsed.as_secs_f64() * SPHERE_TO_PLANAR_TIME_RATIO_BUDGET,
        "spherical graph took {:.3} ms versus planar {:.3} ms ({sphere_to_planar_ratio:.3}x); budget is {:.3}x",
        sphere_elapsed.as_secs_f64() * 1_000.0,
        planar_elapsed.as_secs_f64() * 1_000.0,
        SPHERE_TO_PLANAR_TIME_RATIO_BUDGET,
    );
    if let Some(additional_peak_working_set_bytes) = additional_peak_working_set_bytes {
        assert!(
            additional_peak_working_set_bytes <= ADDITIONAL_PEAK_WORKING_SET_BUDGET_BYTES,
            "spherical graph added {additional_peak_working_set_bytes} peak working-set bytes above the planar peak; budget is {ADDITIONAL_PEAK_WORKING_SET_BUDGET_BYTES}"
        );
    }
}

fn planar_external_artifacts() -> ExternalArtifacts {
    let mut artifacts = common_external_artifacts();
    artifacts
        .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
            width: Meters::new(20_000_000.0).unwrap(),
            height: Meters::new(10_000_000.0).unwrap(),
            target_cell_count: TARGET_CELL_COUNT,
            boundary: BoundaryCondition::Closed,
        }))
        .unwrap();
    artifacts
}

fn spherical_external_artifacts() -> ExternalArtifacts {
    let mut artifacts = common_external_artifacts();
    artifacts
        .insert(SphericalSpaceArtifact::new(SphericalSpaceSpec {
            radius: Meters::new(EARTH_RADIUS_M).unwrap(),
            target_cell_count: TARGET_CELL_COUNT,
        }))
        .unwrap();
    artifacts
}

fn common_external_artifacts() -> ExternalArtifacts {
    let mut artifacts = ExternalArtifacts::new();
    artifacts
        .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
        .unwrap();
    artifacts
        .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
        .unwrap();
    artifacts
        .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
        .unwrap();
    artifacts
        .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
        .unwrap();
    artifacts
        .insert(WorldFormationSpecArtifact::new(
            WorldFormationSpec::default(),
        ))
        .unwrap();
    artifacts
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    artifacts
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();
    artifacts
}

#[allow(clippy::too_many_arguments)]
fn validate_final_product(
    surface: &SphericalSurfaceArtifact,
    formation: &ResolvedWorldFormationArtifact,
    tectonic: &SphericalTectonicArtifact,
    mantle: &SphericalMantleArtifact,
    relief: &SphericalReliefArtifact,
    geology: &SphericalGeologicArtifact,
    climate: &SphericalPreliminaryClimateArtifact,
    hydro: &SphericalHydroErosionArtifact,
) {
    let surface_snapshot = surface.snapshot();
    surface_snapshot.validate().unwrap();
    formation.formation().validate().unwrap();
    tectonic
        .snapshot()
        .validate_against(surface_snapshot)
        .unwrap();
    mantle
        .snapshot()
        .validate_against(surface_snapshot)
        .unwrap();
    relief
        .snapshot()
        .validate_against(surface_snapshot, tectonic.snapshot(), mantle.snapshot())
        .unwrap();
    geology
        .snapshot()
        .validate_against(
            surface_snapshot,
            tectonic.snapshot(),
            mantle.snapshot(),
            relief.snapshot(),
        )
        .unwrap();
    climate
        .snapshot()
        .validate_against(surface_snapshot, relief.snapshot())
        .unwrap();
    hydro
        .snapshot()
        .validate_against(
            surface_snapshot,
            relief.snapshot(),
            geology.snapshot(),
            climate.snapshot(),
        )
        .unwrap();
    let plate_count = u16::try_from(tectonic.snapshot().plates().len()).unwrap();
    let registry =
        spherical_natural_field_registry(plate_count, surface_snapshot.total_cell_area().get())
            .unwrap();
    assert_eq!(registry.len(), 36);
}

fn spherical_surface_persistent_bytes(artifact: &SphericalSurfaceArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.vertices())
        + size_of_val(snapshot.cells())
        + snapshot
            .cells()
            .iter()
            .map(|cell| {
                size_of_val(cell.boundary_vertices.as_slice())
                    + size_of_val(cell.boundary_edges.as_slice())
            })
            .sum::<usize>()
        + size_of_val(snapshot.edges())
}

fn spherical_tectonic_persistent_bytes(artifact: &SphericalTectonicArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.plates())
        + size_of_val(snapshot.cell_plates().raw_values())
        + size_of_val(snapshot.crust_kinds().raw_values())
        + size_of_val(snapshot.crust_thickness_km())
        + size_of_val(snapshot.boundaries())
        + size_of_val(snapshot.boundary_segments())
        + snapshot
            .boundary_segments()
            .iter()
            .map(|segment| size_of_val(segment.member_edges()))
            .sum::<usize>()
}

fn spherical_mantle_persistent_bytes(artifact: &SphericalMantleArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.hotspots())
        + size_of_val(snapshot.heat_flow_mw_m2())
        + size_of_val(snapshot.volcanic_influence())
}

fn spherical_relief_persistent_bytes(artifact: &SphericalReliefArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.crust_base_elevation_m().values())
        + size_of_val(snapshot.tectonic_offset_m().values())
        + size_of_val(snapshot.volcanic_offset_m().values())
        + size_of_val(snapshot.regional_offset_m().values())
        + size_of_val(snapshot.elevation_m().values())
        + size_of_val(snapshot.land_ocean().raw_values())
}

fn spherical_geology_persistent_bytes(artifact: &SphericalGeologicArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.bedrock_kinds().raw_values())
        + size_of_val(snapshot.fracture_intensity())
        + size_of_val(snapshot.erosion_resistance())
        + size_of_val(snapshot.relative_permeability())
        + size_of_val(snapshot.metallic_mineral_potential())
        + size_of_val(snapshot.geothermal_potential())
        + size_of_val(snapshot.sedimentary_basin_potential())
}

fn spherical_climate_persistent_bytes(artifact: &SphericalPreliminaryClimateArtifact) -> usize {
    let snapshot = artifact.snapshot();
    size_of_val(artifact)
        + size_of_val(snapshot.latitude_degrees())
        + size_of_val(snapshot.maritime_influence())
        + size_of_val(snapshot.monthly_air_temperature_c().values())
        + size_of_val(snapshot.monthly_precipitation_mm().values())
        + size_of_val(snapshot.monthly_wind_m_s().values())
        + size_of_val(snapshot.mean_annual_air_temperature_c())
        + size_of_val(snapshot.temperature_seasonality_c())
        + size_of_val(snapshot.annual_precipitation_mm())
        + size_of_val(snapshot.prevailing_wind_m_s())
}

fn spherical_hydro_persistent_bytes(artifact: &SphericalHydroErosionArtifact) -> usize {
    let snapshot = artifact.snapshot();
    let surface = snapshot.surface();
    let hydrology = snapshot.hydrology();
    size_of_val(artifact)
        + size_of_val(surface.erosion_depth_m())
        + size_of_val(surface.deposition_thickness_m())
        + size_of_val(surface.surface_elevation_m().values())
        + size_of_val(surface.sediment_throughput_m3())
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
        + size_of_val(hydrology.river_segment_length_m())
}
