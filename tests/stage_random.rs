use rand::RngCore;
use sekai::engine::{derive_entity_seed, derive_stage_seed, StageIdentity, StageRng};
use sekai::world::RootSeed;

const TERRAIN_V1: StageIdentity = StageIdentity::new("terrain.elevation", 1, "sekai.core");

#[test]
fn stage_seed_is_repeatable_and_namespaced() {
    let root = RootSeed::new(42);
    let first = derive_stage_seed(root, TERRAIN_V1);
    let second = derive_stage_seed(root, TERRAIN_V1);
    let other = derive_stage_seed(
        root,
        StageIdentity::new("terrain.elevation", 1, "example.mod"),
    );

    assert_eq!(first, second);
    assert_ne!(first, other);
}

#[test]
fn stage_seed_uses_the_v1_blake3_byte_framing() {
    let identity = StageIdentity::new("terrain.elevation", 12, "sekai.core");
    let actual = derive_stage_seed(RootSeed::new(42), identity).into_bytes();

    let mut expected_hasher = blake3::Hasher::new();
    expected_hasher.update(b"sekai-stage-seed-v1\0");
    expected_hasher.update(&42_u64.to_le_bytes());
    expected_hasher.update(&(10_u64).to_le_bytes());
    expected_hasher.update(b"sekai.core");
    expected_hasher.update(&(17_u64).to_le_bytes());
    expected_hasher.update(b"terrain.elevation");
    expected_hasher.update(&12_u32.to_le_bytes());

    assert_eq!(actual, *expected_hasher.finalize().as_bytes());
}

#[test]
fn entity_streams_do_not_depend_on_iteration_order() {
    let stage = derive_stage_seed(RootSeed::new(7), TERRAIN_V1);
    let seed_10 = derive_entity_seed(stage, "cell", 10);
    let seed_20 = derive_entity_seed(stage, "cell", 20);
    let species_10 = derive_entity_seed(stage, "species", 10);

    let mut a = StageRng::from_seed(seed_10);
    let mut b = StageRng::from_seed(seed_20);
    let first_a = a.next_u64();
    let first_b = b.next_u64();

    let mut a_again = StageRng::from_seed(derive_entity_seed(stage, "cell", 10));
    assert_eq!(first_a, a_again.next_u64());
    assert_ne!(first_a, first_b);
    assert_ne!(seed_10, species_10);
}

#[test]
fn entity_seed_uses_the_v1_blake3_byte_framing() {
    let stage = derive_stage_seed(RootSeed::new(7), TERRAIN_V1);
    let actual = derive_entity_seed(stage, "cell", 10).into_bytes();

    let mut expected_hasher = blake3::Hasher::new();
    expected_hasher.update(b"sekai-entity-seed-v1\0");
    expected_hasher.update(&stage.into_bytes());
    expected_hasher.update(&(4_u64).to_le_bytes());
    expected_hasher.update(b"cell");
    expected_hasher.update(&10_u64.to_le_bytes());

    assert_eq!(actual, *expected_hasher.finalize().as_bytes());
}
