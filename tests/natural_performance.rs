use std::mem::{size_of, size_of_val};
use std::time::Instant;

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicArtifact,
    GeologicSpecArtifact, HydroErosionArtifact, HydroErosionSpecArtifact, MantleArtifact,
    PreliminaryClimateArtifact, ReliefArtifact, RulePackSetArtifact, TectonicArtifact,
    TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    ClimateSpec, GeologicSpec, HydroErosionSpec, TectonicSpec, WorldFormationSpec,
};
use sekai::world::spatial::Topology;
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

const HYDRO_EROSION_STAGE_BUDGET_MS: f64 = 350.0;
const HYDRO_EROSION_MEMORY_BUDGET_BYTES: usize = 8 * 1024 * 1024;

#[test]
#[ignore = "release-only 20,000-cell performance and memory budget"]
fn release_default_hydro_erosion_budget() {
    let mut external = ExternalArtifacts::new();
    external
        .insert(PlanarSpaceArtifact::new(PlanarSpaceSpec {
            width: Meters::new(20_000_000.0).unwrap(),
            height: Meters::new(10_000_000.0).unwrap(),
            target_cell_count: 20_000,
            boundary: BoundaryCondition::Closed,
        }))
        .unwrap();
    external
        .insert(TectonicSpecArtifact::new(TectonicSpec::default()))
        .unwrap();
    external
        .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
        .unwrap();
    external
        .insert(ClimateSpecArtifact::new(ClimateSpec::default()))
        .unwrap();
    external
        .insert(HydroErosionSpecArtifact::new(HydroErosionSpec::default()))
        .unwrap();
    external
        .insert(WorldFormationSpecArtifact::new(
            WorldFormationSpec::default(),
        ))
        .unwrap();
    external
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    external
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();

    let started = Instant::now();
    let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(RootSeed::new(42), external, &mut MemoryStageCache::new())
        .unwrap();
    let total = started.elapsed();
    let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();
    let tectonic = outcome.artifacts.get::<TectonicArtifact>().unwrap();
    let mantle = outcome.artifacts.get::<MantleArtifact>().unwrap();
    let relief = outcome.artifacts.get::<ReliefArtifact>().unwrap();
    let geology = outcome.artifacts.get::<GeologicArtifact>().unwrap();
    let climate = outcome
        .artifacts
        .get::<PreliminaryClimateArtifact>()
        .unwrap();
    let hydro_erosion = outcome.artifacts.get::<HydroErosionArtifact>().unwrap();
    let spatial = spatial.snapshot();
    let tectonic = tectonic.snapshot();
    let mantle = mantle.snapshot();
    let relief = relief.snapshot();
    let geology = geology.snapshot();
    let climate = climate.snapshot();
    let hydro_erosion = hydro_erosion.snapshot();
    let surface = hydro_erosion.surface();
    let hydrology = hydro_erosion.hydrology();
    tectonic.validate_against(spatial).unwrap();
    mantle.validate_against(spatial).unwrap();
    relief.validate_against(spatial).unwrap();
    geology
        .validate_against(spatial, tectonic, mantle, relief)
        .unwrap();
    climate.validate_against(spatial, relief).unwrap();
    hydro_erosion
        .validate_against(spatial, relief, geology, climate)
        .unwrap();
    assert_eq!(outcome.report.stages().len(), 16);

    let hydro_dense_bytes = size_of_val(surface.erosion_depth_m())
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
        + size_of::<f64>();
    assert!(
        hydro_dense_bytes <= HYDRO_EROSION_MEMORY_BUDGET_BYTES,
        "hydro-erosion data uses {hydro_dense_bytes} bytes; budget is \
         {HYDRO_EROSION_MEMORY_BUDGET_BYTES}"
    );

    let hydro_stage = outcome
        .report
        .stages()
        .iter()
        .find(|stage| stage.stage_id() == "natural.hydro-erosion")
        .expect("production report contains the hydro-erosion stage");
    if cfg!(debug_assertions) {
        eprintln!(
            "debug build: hydro stage timing is informational only ({:.3} ms)",
            hydro_stage.duration().as_secs_f64() * 1000.0
        );
    } else {
        assert!(
            hydro_stage.duration().as_secs_f64() * 1000.0 <= HYDRO_EROSION_STAGE_BUDGET_MS,
            "hydro-erosion stage took {:.3} ms; release budget is {:.3} ms",
            hydro_stage.duration().as_secs_f64() * 1000.0,
            HYDRO_EROSION_STAGE_BUDGET_MS
        );
    }

    let climate_dense_bytes = size_of_val(climate.latitude_degrees())
        + size_of_val(climate.maritime_influence())
        + size_of_val(climate.monthly_air_temperature_c().values())
        + size_of_val(climate.monthly_precipitation_mm().values())
        + size_of_val(climate.monthly_wind_m_s().values())
        + size_of_val(climate.mean_annual_air_temperature_c())
        + size_of_val(climate.temperature_seasonality_c())
        + size_of_val(climate.annual_precipitation_mm())
        + size_of_val(climate.prevailing_wind_m_s());
    let dense_bytes = size_of_val(tectonic.cell_plates().raw_values())
        + size_of_val(tectonic.crust_kinds().raw_values())
        + size_of_val(tectonic.crust_thickness_km())
        + size_of_val(tectonic.boundaries())
        + size_of_val(relief.crust_base_elevation_m().values())
        + size_of_val(relief.tectonic_offset_m().values())
        + size_of_val(relief.volcanic_offset_m().values())
        + size_of_val(relief.regional_offset_m().values())
        + size_of_val(relief.elevation_m().values())
        + size_of_val(relief.land_ocean().raw_values())
        + size_of_val(mantle.heat_flow_mw_m2())
        + size_of_val(mantle.volcanic_influence())
        + size_of_val(geology.bedrock_kinds().raw_values())
        + size_of_val(geology.fracture_intensity())
        + size_of_val(geology.erosion_resistance())
        + size_of_val(geology.relative_permeability())
        + size_of_val(geology.metallic_mineral_potential())
        + size_of_val(geology.geothermal_potential())
        + size_of_val(geology.sedimentary_basin_potential())
        + climate_dense_bytes
        + hydro_dense_bytes;
    let continental = tectonic
        .crust_kinds()
        .raw_values()
        .iter()
        .filter(|&&kind| kind == 1)
        .count() as f32
        / spatial.cell_count() as f32;
    let land = relief
        .land_ocean()
        .raw_values()
        .iter()
        .filter(|&&kind| kind == 1)
        .count() as f32
        / spatial.cell_count() as f32;

    for stage in outcome.report.stages() {
        eprintln!(
            "stage={} elapsed_ms={:.3} cache_hit={}",
            stage.stage_id(),
            stage.duration().as_secs_f64() * 1000.0,
            stage.cache_hit()
        );
    }
    eprintln!(
        "total_ms={:.3} cells={} edges={} plates={} segments={} dense_bytes={} climate_dense_bytes={} hydro_dense_bytes={} continental_fraction={continental:.4} land_fraction={land:.4}",
        total.as_secs_f64() * 1000.0,
        spatial.cell_count(),
        spatial.edges().len(),
        tectonic.plates().len(),
        tectonic.boundary_segments().len(),
        dense_bytes,
        climate_dense_bytes,
        hydro_dense_bytes,
    );

    assert_eq!(spatial.cell_count(), 20_000);
    assert_eq!(tectonic.cell_count(), 20_000);
    assert_eq!(mantle.cell_count(), 20_000);
    assert_eq!(relief.cell_count(), 20_000);
    assert_eq!(geology.cell_count(), 20_000);
    assert_eq!(climate.cell_count(), 20_000);
    assert_eq!(hydro_erosion.cell_count(), 20_000);
}
