use std::collections::BTreeSet;
use std::sync::OnceLock;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::MantleGenerator;
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    GeologicSpec, MantleActivity, MantleFormationBias, MantleSnapshot, HEAT_FLOW_MAX_MW_M2,
    HEAT_FLOW_MIN_MW_M2, MAX_HOTSPOT_COUNT,
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
        StageIdentity::new("natural.mantle", 3, "sekai.core"),
    ))
}

fn generate(seed: u64, spec: &GeologicSpec) -> MantleSnapshot {
    generate_with_bias(seed, spec, MantleFormationBias::Neutral)
}

fn generate_with_bias(seed: u64, spec: &GeologicSpec, bias: MantleFormationBias) -> MantleSnapshot {
    MantleGenerator::generate(spatial_fixture(), spec, bias, &mut mantle_rng(seed)).unwrap()
}

#[test]
fn neutral_mantle_golden_remains_byte_stable() {
    let encoded = serde_json::to_vec(&generate(42, &GeologicSpec::default())).unwrap();
    assert_eq!(
        blake3::hash(&encoded).to_hex().as_str(),
        "9d7beb0e7739b223bacc64e19fbafaaaf314a72eb728bae1e3b091fb49580047"
    );
}

#[test]
fn volcanic_island_bias_adds_bounded_active_hotspots() {
    let spec = GeologicSpec {
        hotspot_count: 2,
        mantle_activity: MantleActivity::Quiet,
        ..GeologicSpec::default()
    };
    let neutral = generate_with_bias(42, &spec, MantleFormationBias::Neutral);
    let volcanic = generate_with_bias(42, &spec, MantleFormationBias::VolcanicIslands);

    assert_eq!(neutral.hotspots().len(), 2);
    assert!((9..=usize::from(MAX_HOTSPOT_COUNT)).contains(&volcanic.hotspots().len()));
    let mean_heat = |snapshot: &MantleSnapshot| {
        snapshot
            .heat_flow_mw_m2()
            .iter()
            .map(|&value| f64::from(value))
            .sum::<f64>()
            / snapshot.heat_flow_mw_m2().len() as f64
    };
    assert!(mean_heat(&volcanic) > mean_heat(&neutral));
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
fn hotspot_sources_avoid_the_closed_world_edge_margin() {
    let spatial = spatial_fixture();
    let snapshot = generate_with_bias(
        0x00C0_FFEE,
        &GeologicSpec::default(),
        MantleFormationBias::VolcanicIslands,
    );
    let bounds = spatial.bounds();
    let margin = bounds.width().get().min(bounds.height().get()) * 0.10;
    let min_x = bounds.min().x().get() + margin;
    let max_x = bounds.max().x().get() - margin;
    let min_y = bounds.min().y().get() + margin;
    let max_y = bounds.max().y().get() - margin;

    for hotspot in snapshot.hotspots() {
        let center = spatial.cell(hotspot.source_cell()).unwrap().centroid;
        assert!(
            (min_x..=max_x).contains(&center.x().get())
                && (min_y..=max_y).contains(&center.y().get()),
            "hotspot source {:?} at ({:.0}, {:.0}) entered the artificial edge margin",
            hotspot.source_cell(),
            center.x().get(),
            center.y().get(),
        );
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
