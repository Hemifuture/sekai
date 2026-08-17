use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, BuildCancellation, StageIdentity, StageRng};
use sekai::generators::natural::{
    classify_substrate_bedrock, EvolvedTectonicGenerator, GeologicSubstrateGenerationError,
    GeologicSubstrateGenerator,
};
use sekai::generators::spatial::{ProfileSurfaceBuilder, ProfileSurfaceBundle};
use sekai::world::natural::{
    effective_crust_density_kg_m3, BedrockKind, CrustKind, GeologicSpec, NaturalQualityProfile,
    ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::{Meters, RootSeed};

fn bundle() -> &'static ProfileSurfaceBundle {
    static BUNDLE: OnceLock<ProfileSurfaceBundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(6_371_000.0).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap()
    })
}

fn formation() -> ResolvedWorldFormation {
    ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Continents,
    )
    .unwrap()
}

fn evolved(seed: u64) -> sekai::world::natural::EvolvedTectonicSnapshot {
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
    ));
    EvolvedTectonicGenerator::generate(bundle(), &TectonicSpec::default(), &formation(), &mut rng)
        .unwrap()
}

fn substrate_rng(seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.geologic-substrate", 1, "sekai.core"),
    ))
}

#[test]
fn causal_lithology_priority_is_explicit_and_unit_bearing() {
    assert_eq!(
        classify_substrate_bedrock(CrustKind::Continental, 0.80, 40.0, 40.0, 20.0, 0.90),
        BedrockKind::Volcanic
    );
    assert_eq!(
        classify_substrate_bedrock(CrustKind::Continental, 0.0, 9.0, 0.0, 0.0, 0.20),
        BedrockKind::Metamorphic
    );
    assert_eq!(
        classify_substrate_bedrock(CrustKind::Continental, 0.0, 0.0, 0.0, 0.20, 0.20),
        BedrockKind::Sedimentary
    );
    assert_eq!(
        classify_substrate_bedrock(CrustKind::Continental, 0.0, 0.0, 0.0, 0.0, 0.20),
        BedrockKind::ContinentalCrystalline
    );
    assert_eq!(
        classify_substrate_bedrock(CrustKind::Oceanic, 0.0, 100.0, 100.0, 100.0, 1.0),
        BedrockKind::OceanicMafic
    );
}

#[test]
fn generator_copies_v5_facts_recomputes_density_and_preserves_causality() {
    let evolved = evolved(42);
    let surface = bundle().authoritative_surface();
    let mut rng = substrate_rng(42);
    let substrate = GeologicSubstrateGenerator::generate(
        surface,
        &evolved,
        &GeologicSpec::default(),
        &formation(),
        &mut rng,
    )
    .unwrap();

    substrate.validate_against(surface, &evolved).unwrap();
    for index in 0..surface.cells().len() {
        assert_eq!(
            substrate.crust_density_kg_m3()[index],
            effective_crust_density_kg_m3(
                evolved.material().continental_volume_m3()[index],
                evolved.material().oceanic_volume_m3()[index],
            )
            .unwrap()
        );
        assert_eq!(
            substrate.bedrock_kind(index).unwrap(),
            classify_substrate_bedrock(
                substrate.crust_kind(index).unwrap(),
                substrate.volcanic_influence()[index],
                evolved.forcing().shortening_rate_mm_per_year()[index],
                evolved.forcing().uplift_rate_mm_per_year()[index],
                evolved.forcing().subsidence_rate_mm_per_year()[index],
                substrate.fracture_intensity()[index],
            )
        );
    }
    for hotspot in substrate.mantle().hotspots() {
        assert_eq!(
            substrate
                .bedrock_kind(hotspot.source_cell().raw() as usize)
                .unwrap(),
            BedrockKind::Volcanic
        );
    }
}

#[test]
fn generation_is_byte_deterministic_for_one_stage_stream() {
    let evolved = evolved(43);
    let surface = bundle().authoritative_surface();
    let first = GeologicSubstrateGenerator::generate(
        surface,
        &evolved,
        &GeologicSpec::default(),
        &formation(),
        &mut substrate_rng(43),
    )
    .unwrap();
    let second = GeologicSubstrateGenerator::generate(
        surface,
        &evolved,
        &GeologicSpec::default(),
        &formation(),
        &mut substrate_rng(43),
    )
    .unwrap();

    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}

#[test]
fn generation_honors_an_already_cancelled_stream() {
    let evolved = evolved(44);
    let cancellation = BuildCancellation::new();
    cancellation.cancel();
    let seed = derive_stage_seed(
        RootSeed::new(44),
        StageIdentity::new("natural.geologic-substrate", 1, "sekai.core"),
    );
    let mut rng = StageRng::from_seed_with_cancellation(seed, &cancellation);
    let error = GeologicSubstrateGenerator::generate(
        bundle().authoritative_surface(),
        &evolved,
        &GeologicSpec::default(),
        &formation(),
        &mut rng,
    )
    .unwrap_err();

    assert_eq!(error, GeologicSubstrateGenerationError::Cancelled);
}
