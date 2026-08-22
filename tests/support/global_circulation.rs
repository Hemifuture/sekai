use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    ClimateWorkDomainBuilder, EvolvedTectonicGenerator, GeologicSubstrateGenerator,
    GlobalClimateForcing, GlobalClimateForcingBuilder, PrimaryReliefGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    ClimateSpec, ClimateWorkDomainSnapshot, EvolvedTectonicSnapshot, GeologicSpec,
    GeologicSubstrateSnapshot, NaturalQualityProfile, PrimaryReliefSnapshot, ReliefSpec,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

#[allow(dead_code)]
pub struct GlobalCirculationFixture {
    pub bundle: ProfileSurfaceBundle,
    pub evolved: EvolvedTectonicSnapshot,
    pub substrate: GeologicSubstrateSnapshot,
    pub relief: PrimaryReliefSnapshot,
    pub domain: ClimateWorkDomainSnapshot,
    pub forcing: GlobalClimateForcing,
}

/// Runs the default Continents V5 evolution, substrate and P3 relief for one
/// root seed on a shared Draft sphere, with the production stage identities.
#[allow(dead_code)]
pub fn build_primary_relief(
    bundle: &ProfileSurfaceBundle,
    seed: u64,
) -> (
    EvolvedTectonicSnapshot,
    GeologicSubstrateSnapshot,
    PrimaryReliefSnapshot,
) {
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap();
    let mut tectonic_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
    ));
    let evolved = EvolvedTectonicGenerator::generate(
        bundle,
        &TectonicSpec::default(),
        &formation,
        &mut tectonic_rng,
    )
    .unwrap();
    let mut substrate_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.geologic-substrate", 1, "sekai.core"),
    ));
    let substrate = GeologicSubstrateGenerator::generate(
        bundle.authoritative_surface(),
        &evolved,
        &GeologicSpec::default(),
        &formation,
        &mut substrate_rng,
    )
    .unwrap();
    let mut relief_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.primary-relief", 1, "sekai.core"),
    ));
    let mut diagnostics = Vec::new();
    let relief = PrimaryReliefGenerator::generate(
        bundle.authoritative_surface(),
        &evolved,
        &substrate,
        &ReliefSpec::default(),
        &mut relief_rng,
        &mut diagnostics,
    )
    .unwrap();
    (evolved, substrate, relief)
}

#[allow(dead_code)]
pub fn global_circulation_fixture() -> &'static GlobalCirculationFixture {
    static FIXTURE: OnceLock<GlobalCirculationFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let cancellation = BuildCancellation::new();
        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &cancellation,
        )
        .unwrap();
        let (evolved, substrate, relief) = build_primary_relief(&bundle, 42);
        let domain = ClimateWorkDomainBuilder::build(
            bundle.authoritative_surface(),
            NaturalQualityProfile::Draft,
            &cancellation,
        )
        .unwrap();
        let forcing = GlobalClimateForcingBuilder::build(
            bundle.authoritative_surface(),
            &relief,
            &ClimateSpec::default(),
            &domain,
            &cancellation,
        )
        .unwrap();
        GlobalCirculationFixture {
            bundle,
            evolved,
            substrate,
            relief,
            domain,
            forcing,
        }
    })
}
