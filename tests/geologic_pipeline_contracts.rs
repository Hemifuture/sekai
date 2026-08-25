use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, BuildCancellation, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{
    EvolvedTectonicGenerator, GeologicSubstrateGenerator, PrimaryReliefGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    EvolvedTectonicSnapshot, GeologicSpec, GeologicSubstrateSnapshot, NaturalQualityProfile,
    PrimaryReliefSnapshot, ReliefSpec, ResolvedWorldFormation, ResolvedWorldFormationPreset,
    TectonicSpec, WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

struct Fixture {
    bundle: ProfileSurfaceBundle,
    evolved: EvolvedTectonicSnapshot,
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let mut tectonic_rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
        ));
        let evolved = EvolvedTectonicGenerator::generate(
            &bundle,
            &TectonicSpec::default(),
            &formation(),
            &mut tectonic_rng,
        )
        .unwrap();
        Fixture { bundle, evolved }
    })
}

fn generate_p3_from_evolved(
    fixture: &Fixture,
    evolved: &EvolvedTectonicSnapshot,
) -> (GeologicSubstrateSnapshot, PrimaryReliefSnapshot) {
    let surface = fixture.bundle.authoritative_surface();
    let mut substrate_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new("natural.geologic-substrate", 1, "sekai.core"),
    ));
    let substrate = GeologicSubstrateGenerator::generate(
        surface,
        evolved,
        &GeologicSpec::default(),
        &formation(),
        &mut substrate_rng,
    )
    .unwrap();
    let mut relief_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(42),
        StageIdentity::new("natural.primary-relief", 3, "sekai.core"),
    ));
    let relief = PrimaryReliefGenerator::generate(
        surface,
        evolved,
        &substrate,
        &ReliefSpec::default(),
        &mut relief_rng,
        &mut Vec::<Diagnostic>::new(),
    )
    .unwrap();
    (substrate, relief)
}

#[test]
fn compatibility_elevation_alone_cannot_change_authoritative_p3() {
    let fixture = fixture();
    let mut wire = serde_json::to_value(&fixture.evolved).unwrap();
    let values = wire["compatibility"]["crust"]["tectonic_elevation_m"]
        .as_array_mut()
        .unwrap();
    for (index, value) in values.iter_mut().enumerate() {
        *value = serde_json::json!(-8_000.0_f32 + index as f32 * 0.125);
    }
    let mutated: EvolvedTectonicSnapshot = serde_json::from_value(wire).unwrap();

    let original = generate_p3_from_evolved(fixture, &fixture.evolved);
    let changed = generate_p3_from_evolved(fixture, &mutated);

    assert_eq!(changed, original);
}
