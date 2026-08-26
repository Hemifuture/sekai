use std::sync::OnceLock;

use sekai::engine::BuildCancellation;
use sekai::generators::natural::GlobalCirculationGenerator;
use sekai::world::natural::{
    ClimateModelProfile, ClimateSpec, GlobalCirculationSnapshot, HydroErosionSpec,
};

use super::global_circulation::{global_circulation_fixture, GlobalCirculationFixture};

/// Frozen P4 product and resolved P5 specification shared by every solve.
#[allow(dead_code)]
pub struct SurfaceFormationFixture {
    pub upstream: &'static GlobalCirculationFixture,
    pub initial_climate: GlobalCirculationSnapshot,
    pub climate_spec: ClimateSpec,
    pub formation_spec: HydroErosionSpec,
}

#[allow(dead_code)]
pub fn surface_formation_fixture() -> &'static SurfaceFormationFixture {
    static FIXTURE: OnceLock<SurfaceFormationFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let upstream = global_circulation_fixture();
        let initial_climate = GlobalCirculationGenerator::generate(
            upstream.bundle.authoritative_surface(),
            &upstream.domain,
            &upstream.forcing,
            ClimateModelProfile::C2LayeredV1,
            &BuildCancellation::new(),
        )
        .unwrap();
        SurfaceFormationFixture {
            upstream,
            initial_climate,
            climate_spec: ClimateSpec::default(),
            formation_spec: HydroErosionSpec::default(),
        }
    })
}
