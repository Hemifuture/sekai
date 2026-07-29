use std::mem::size_of_val;
use std::time::Instant;

use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    natural_foundation_graph, ReliefArtifact, TectonicArtifact, TectonicSpecArtifact,
};
use sekai::generators::spatial::{PlanarSpaceArtifact, SpatialArtifact};
use sekai::world::natural::TectonicSpec;
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

    let started = Instant::now();
    let outcome = BuildEngine::new(natural_foundation_graph().unwrap())
        .build(RootSeed::new(42), external, &mut MemoryStageCache::new())
        .unwrap();
    let total = started.elapsed();
    let spatial = outcome.artifacts.get::<SpatialArtifact>().unwrap();
    let tectonic = outcome.artifacts.get::<TectonicArtifact>().unwrap();
    let relief = outcome.artifacts.get::<ReliefArtifact>().unwrap();
    let spatial = spatial.snapshot();
    let tectonic = tectonic.snapshot();
    let relief = relief.snapshot();

    let dense_bytes = size_of_val(tectonic.cell_plates().raw_values())
        + size_of_val(tectonic.crust_kinds().raw_values())
        + size_of_val(tectonic.crust_thickness_km())
        + size_of_val(tectonic.boundaries())
        + size_of_val(relief.crust_base_elevation_m().values())
        + size_of_val(relief.tectonic_offset_m().values())
        + size_of_val(relief.regional_offset_m().values())
        + size_of_val(relief.elevation_m().values())
        + size_of_val(relief.land_ocean().raw_values());
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
    assert_eq!(relief.cell_count(), 20_000);
}
