use std::collections::BTreeSet;
use std::f64::consts::PI;
use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::MantleGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    GeologicSpec, MantleActivity, MantleFormationBias, SphericalMantleSnapshot,
    HEAT_FLOW_MAX_MW_M2, HEAT_FLOW_MIN_MW_M2, MAX_HOTSPOT_COUNT,
};
use sekai::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

const GRAPH_LENGTH_QUANTIZATION: f64 = 1_000_000.0;

fn surface() -> &'static sekai::world::spatial::SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<sekai::world::spatial::SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 642,
        })
        .unwrap()
    })
}

fn mantle_rng(root_seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(root_seed),
        StageIdentity::new("natural.spherical-mantle", 1, "sekai.core"),
    ))
}

fn generate(
    root_seed: u64,
    spec: &GeologicSpec,
    bias: MantleFormationBias,
) -> SphericalMantleSnapshot {
    MantleGenerator::generate_spherical(surface(), spec, bias, &mut mantle_rng(root_seed)).unwrap()
}

fn quantized_graph_distances(source: CellId) -> Vec<u64> {
    let mut distances = vec![u64::MAX; surface().cells().len()];
    let mut visited = vec![false; surface().cells().len()];
    distances[source.raw() as usize] = 0;
    for _ in 0..surface().cells().len() {
        let Some(index) = (0..surface().cells().len())
            .filter(|&index| !visited[index])
            .min_by_key(|&index| (distances[index], index))
        else {
            break;
        };
        if distances[index] == u64::MAX {
            break;
        }
        visited[index] = true;
        let cell = CellId::from_raw(index as u32);
        for &edge_id in surface().cell_edges(cell).unwrap() {
            let edge = surface().edge(edge_id).unwrap();
            let neighbor = surface().opposite_cell(cell, edge_id).unwrap();
            let cost = ((edge.center_distance.get() / (PI * surface().radius().get()))
                * GRAPH_LENGTH_QUANTIZATION)
                .round()
                .max(1.0) as u64;
            let candidate = distances[index].saturating_add(cost);
            let neighbor_index = neighbor.raw() as usize;
            if candidate < distances[neighbor_index] {
                distances[neighbor_index] = candidate;
            }
        }
    }
    distances
}

#[test]
fn global_farthest_sources_and_strengths_are_repeatable_prefix_stable_and_seed_sensitive() {
    let spec = GeologicSpec {
        hotspot_count: 7,
        ..GeologicSpec::default()
    };
    let first = generate(0xC0_FFEE, &spec, MantleFormationBias::Neutral);
    let repeated = generate(0xC0_FFEE, &spec, MantleFormationBias::Neutral);
    let changed = generate(0xC0_FFEF, &spec, MantleFormationBias::Neutral);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    let decoded: SphericalMantleSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    assert_eq!(decoded, first);
    assert_ne!(first.hotspots(), changed.hotspots());
    first.validate_against(surface()).unwrap();

    let prefix = generate(
        0xC0_FFEE,
        &GeologicSpec {
            hotspot_count: 3,
            ..GeologicSpec::default()
        },
        MantleFormationBias::Neutral,
    );
    assert_eq!(prefix.hotspots(), &first.hotspots()[..3]);
    let sources = first
        .hotspots()
        .iter()
        .map(|hotspot| hotspot.source_cell())
        .collect::<BTreeSet<_>>();
    assert_eq!(sources.len(), usize::from(spec.hotspot_count));
}

#[test]
fn closed_surface_source_selection_has_no_edge_seam_or_pole_exclusion() {
    let mut saw_north = false;
    let mut saw_south = false;
    let mut saw_antimeridian = false;
    for seed in 1..=16 {
        let snapshot = generate(
            seed,
            &GeologicSpec {
                hotspot_count: 7,
                ..GeologicSpec::default()
            },
            MantleFormationBias::Neutral,
        );
        for hotspot in snapshot.hotspots() {
            let radial = surface()
                .cell(hotspot.source_cell())
                .unwrap()
                .centroid
                .components();
            saw_north |= radial[2] > 0.75;
            saw_south |= radial[2] < -0.75;
            saw_antimeridian |= radial[0] < -0.75;
        }
    }
    assert!(saw_north && saw_south && saw_antimeridian);
}

#[test]
fn one_hotspot_has_exact_compact_monotonic_graph_support() {
    let snapshot = generate(
        42,
        &GeologicSpec {
            hotspot_count: 1,
            mantle_activity: MantleActivity::Moderate,
            ..GeologicSpec::default()
        },
        MantleFormationBias::Neutral,
    );
    let hotspot = &snapshot.hotspots()[0];
    let distances = quantized_graph_distances(hotspot.source_cell());
    let support_distance = ((hotspot.support_radius_m().get() / (PI * surface().radius().get()))
        * GRAPH_LENGTH_QUANTIZATION)
        .round()
        .max(1.0) as u64;
    assert_eq!(
        snapshot.volcanic_influence()[hotspot.source_cell().raw() as usize],
        1.0
    );
    assert!(snapshot
        .volcanic_influence()
        .iter()
        .any(|&influence| influence > 0.0 && influence < 1.0));

    let mut ordered = distances
        .iter()
        .copied()
        .zip(snapshot.volcanic_influence().iter().copied())
        .collect::<Vec<_>>();
    ordered.sort_by_key(|&(distance, _)| distance);
    for pair in ordered.windows(2) {
        assert!(pair[0].1 + f32::EPSILON >= pair[1].1);
    }
    for (&distance, &influence) in distances.iter().zip(snapshot.volcanic_influence()) {
        if distance > support_distance {
            assert_eq!(influence, 0.0);
        }
    }
    assert!(snapshot
        .heat_flow_mw_m2()
        .iter()
        .all(|value| value.is_finite()
            && (HEAT_FLOW_MIN_MW_M2..=HEAT_FLOW_MAX_MW_M2).contains(value)));
}

#[test]
fn activity_background_support_and_volcanic_bias_are_ordered_and_bounded() {
    let zero_hotspot_background = [
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
            MantleFormationBias::Neutral,
        )
    });
    assert_eq!(zero_hotspot_background[0].heat_flow_mw_m2()[0], 45.0);
    assert_eq!(zero_hotspot_background[1].heat_flow_mw_m2()[0], 65.0);
    assert_eq!(zero_hotspot_background[2].heat_flow_mw_m2()[0], 85.0);
    assert!(zero_hotspot_background.iter().all(|snapshot| snapshot
        .volcanic_influence()
        .iter()
        .all(|&value| value == 0.0)));

    let supports = [
        MantleActivity::Quiet,
        MantleActivity::Moderate,
        MantleActivity::Active,
    ]
    .map(|mantle_activity| {
        generate(
            88,
            &GeologicSpec {
                hotspot_count: 1,
                mantle_activity,
                ..GeologicSpec::default()
            },
            MantleFormationBias::Neutral,
        )
        .hotspots()[0]
            .support_radius_m()
            .get()
    });
    assert!(supports[0] < supports[1] && supports[1] < supports[2]);

    let volcanic = generate(
        99,
        &GeologicSpec {
            hotspot_count: 2,
            mantle_activity: MantleActivity::Quiet,
            ..GeologicSpec::default()
        },
        MantleFormationBias::VolcanicIslands,
    );
    assert!((9..=usize::from(MAX_HOTSPOT_COUNT)).contains(&volcanic.hotspots().len()));
    assert!(volcanic
        .hotspots()
        .iter()
        .all(|hotspot| hotspot.support_radius_m().get() <= PI * surface().radius().get()));
}
