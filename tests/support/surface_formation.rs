use std::sync::OnceLock;

use sekai::engine::BuildCancellation;
use sekai::generators::natural::{
    GlobalCirculationGenerator, SurfaceFormationGenerator, SurfaceFormationInputs,
};
use sekai::world::natural::{
    ClimateModelProfile, ClimateSpec, GlobalCirculationSnapshot, HydroErosionSpec,
    NaturalQualityProfile, NaturalSurfaceFormationSnapshot,
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
impl SurfaceFormationFixture {
    pub fn inputs(&self) -> SurfaceFormationInputs<'_> {
        SurfaceFormationInputs {
            surface: self.upstream.bundle.authoritative_surface(),
            quality_profile: NaturalQualityProfile::Draft,
            tectonics: &self.upstream.evolved,
            substrate: &self.upstream.substrate,
            relief: &self.upstream.relief,
            domain: &self.upstream.domain,
            climate_spec: &self.climate_spec,
            initial_climate: &self.initial_climate,
            formation_spec: &self.formation_spec,
        }
    }
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

/// Runs the complete Draft solve once and shares it across every assertion.
#[allow(dead_code)]
pub fn published_formation() -> &'static NaturalSurfaceFormationSnapshot {
    static PUBLISHED: OnceLock<NaturalSurfaceFormationSnapshot> = OnceLock::new();
    PUBLISHED.get_or_init(|| {
        SurfaceFormationGenerator::generate(
            surface_formation_fixture().inputs(),
            &BuildCancellation::new(),
        )
        .unwrap()
    })
}
