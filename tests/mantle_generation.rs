use std::collections::BTreeSet;
use std::sync::OnceLock;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::MantleGenerator;
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    GeologicSpec, MantleActivity, MantleSnapshot, HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2,
};
use sekai::world::spatial::{SpatialSnapshot, Topology};
use sekai::world::{BoundaryCondition, Meters, PlanarSpaceSpec, RootSeed};

fn spatial_fixture() -> &'static SpatialSnapshot {
    static SPATIAL: OnceLock<SpatialSnapshot> = OnceLock::new();
    SPATIAL.get_or_init(|| {
        PlanarVoronoiBuilder::build(
            &PlanarSpaceSpec {
                width: Meters::new(2_000_000.0).unwrap(),
                height: Meters::new(1_200_000.0).unwrap(),
                target_cell_count: 576,
                boundary: BoundaryCondition::Closed,
            },
            &mut ChaCha8Rng::seed_from_u64(9_001),
        )
        .unwrap()
    })
}

fn mantle_rng(seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.mantle", 1, "sekai.core"),
    ))
}

fn generate(seed: u64, spec: &GeologicSpec) -> MantleSnapshot {
    MantleGenerator::generate(spatial_fixture(), spec, &mut mantle_rng(seed)).unwrap()
}

#[test]
fn configured_hotspots_are_exact_unique_and_spatially_valid() {
    for seed in [1, 42, 9_999] {
        let spec = GeologicSpec::default();
        let mantle = generate(seed, &spec);
        let sources: BTreeSet<_> = mantle
            .hotspots()
            .iter()
            .map(|hotspot| hotspot.source_cell())
            .collect();

        assert_eq!(mantle.cell_count() as usize, spatial_fixture().cell_count());
        assert_eq!(mantle.hotspots().len(), spec.hotspot_count as usize);
        assert_eq!(sources.len(), spec.hotspot_count as usize);
        mantle.validate_against(spatial_fixture()).unwrap();
    }
}

#[test]
fn generation_is_byte_repeatable_and_seed_sensitive() {
    let spec = GeologicSpec::default();
    let first = generate(42, &spec);
    let repeated = generate(42, &spec);
    let different = generate(43, &spec);

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    assert!(
        first
            .hotspots()
            .iter()
            .map(|hotspot| (hotspot.source_cell(), hotspot.strength_permille()))
            .collect::<Vec<_>>()
            != different
                .hotspots()
                .iter()
                .map(|hotspot| (hotspot.source_cell(), hotspot.strength_permille()))
                .collect::<Vec<_>>()
    );
}

#[test]
fn zero_hotspots_produces_only_activity_background() {
    for (activity, expected_background) in [
        (MantleActivity::Quiet, 45.0),
        (MantleActivity::Moderate, 65.0),
        (MantleActivity::Active, 85.0),
    ] {
        let snapshot = generate(
            42,
            &GeologicSpec {
                hotspot_count: 0,
                mantle_activity: activity,
                ..GeologicSpec::default()
            },
        );

        assert!(snapshot.hotspots().is_empty());
        assert!(snapshot
            .heat_flow_mw_m2()
            .iter()
            .all(|&value| value == expected_background));
        assert!(snapshot
            .volcanic_influence()
            .iter()
            .all(|&value| value == 0.0));
    }
}

#[test]
fn hotspot_sources_have_full_influence_and_all_fields_stay_bounded() {
    let snapshot = generate(42, &GeologicSpec::default());

    for hotspot in snapshot.hotspots() {
        assert_eq!(
            snapshot.volcanic_influence()[hotspot.source_cell().raw() as usize],
            1.0
        );
    }
    assert!(snapshot
        .heat_flow_mw_m2()
        .iter()
        .all(|value| value.is_finite()
            && (HEAT_FLOW_MIN_MW_M2..=HEAT_FLOW_MAX_MW_M2).contains(value)));
    assert!(snapshot
        .volcanic_influence()
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value)));
}

#[test]
fn activity_background_is_strictly_ordered() {
    let background = [
        MantleActivity::Quiet,
        MantleActivity::Moderate,
        MantleActivity::Active,
    ]
    .map(|mantle_activity| {
        generate(
            77,
            &GeologicSpec {
                hotspot_count: 0,
                mantle_activity,
                ..GeologicSpec::default()
            },
        )
        .heat_flow_mw_m2()[0]
    });

    assert!(background[0] < background[1]);
    assert!(background[1] < background[2]);
}

#[test]
fn hotspot_count_cannot_perturb_the_strength_substream_prefix() {
    let first = generate(
        123,
        &GeologicSpec {
            hotspot_count: 1,
            ..GeologicSpec::default()
        },
    );
    let more = generate(
        123,
        &GeologicSpec {
            hotspot_count: 7,
            ..GeologicSpec::default()
        },
    );

    assert_eq!(
        first.hotspots()[0].strength_permille(),
        more.hotspots()[0].strength_permille()
    );
}
