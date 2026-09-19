use std::sync::{Arc, OnceLock};

use sekai::app::build_spherical_formation_external_artifacts;
use sekai::engine::{BuildEngine, MemoryStageCache};
use sekai::generators::natural::{causal_natural_formation_graph, NaturalFormationBundleArtifact};
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    GeologicSpec, NaturalQualityProfile, ReliefSpec, TectonicSpec, WorldFormationSpec,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, RootSeed};

/// One representative final bundle and its authoritative surface.
pub struct CausalFormationFixture {
    pub surface: SphericalSurfaceSnapshot,
    pub artifact: Arc<NaturalFormationBundleArtifact>,
}

/// Builds the production causal graph for one explicit surface/profile/seed.
pub fn build_causal_formation(
    surface: &SphericalSurfaceSnapshot,
    profile: NaturalQualityProfile,
    seed: u64,
) -> Arc<NaturalFormationBundleArtifact> {
    let root_seed = RootSeed::new(seed);
    let external = build_spherical_formation_external_artifacts(
        root_seed,
        profile,
        surface,
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &ReliefSpec::default(),
        &GeologicSpec::default(),
    )
    .unwrap();
    BuildEngine::new(causal_natural_formation_graph().unwrap())
        .build(root_seed, external, &mut MemoryStageCache::new())
        .unwrap()
        .artifacts
        .get::<NaturalFormationBundleArtifact>()
        .unwrap()
}

/// Returns the sole ordinary-test full-chain corpus member: Draft/seed 42.
pub fn causal_formation_fixture() -> &'static CausalFormationFixture {
    static FIXTURE: OnceLock<CausalFormationFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let surface = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &sekai::engine::BuildCancellation::new(),
        )
        .unwrap()
        .authoritative_surface()
        .clone();
        let artifact = build_causal_formation(&surface, NaturalQualityProfile::Draft, 42);
        CausalFormationFixture { surface, artifact }
    })
}
