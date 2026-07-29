use std::mem::size_of_val;
use std::time::Instant;

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicArtifact,
    GeologicSpecArtifact, MantleArtifact, ReliefArtifact, RulePackSetArtifact, TectonicArtifact,
    TectonicSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{ClimateSpec, GeologicSpec, TectonicSpec};
use sekai::world::spatial::Topology;
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

#[test]
#[ignore = "records a machine-specific release baseline without enforcing a duration"]
fn profile_default_natural_foundation() {
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
    let spatial = spatial.snapshot();
    let tectonic = tectonic.snapshot();
    let mantle = mantle.snapshot();
    let relief = relief.snapshot();
    let geology = geology.snapshot();
    tectonic.validate_against(spatial).unwrap();
    mantle.validate_against(spatial).unwrap();
    relief.validate_against(spatial).unwrap();
    geology
        .validate_against(spatial, tectonic, mantle, relief)
        .unwrap();
    assert_eq!(outcome.report.stages().len(), 9);

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
        + size_of_val(geology.sedimentary_basin_potential());
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
        "total_ms={:.3} cells={} edges={} plates={} segments={} dense_bytes={} continental_fraction={continental:.4} land_fraction={land:.4}",
        total.as_secs_f64() * 1000.0,
        spatial.cell_count(),
        spatial.edges().len(),
        tectonic.plates().len(),
        tectonic.boundary_segments().len(),
        dense_bytes,
    );

    assert_eq!(spatial.cell_count(), 20_000);
    assert_eq!(tectonic.cell_count(), 20_000);
    assert_eq!(mantle.cell_count(), 20_000);
    assert_eq!(relief.cell_count(), 20_000);
    assert_eq!(geology.cell_count(), 20_000);
}
