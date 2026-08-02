use std::collections::{BTreeSet, VecDeque};
use std::sync::OnceLock;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::TectonicGenerator;
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    CrustKind, ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicSpec,
    WorldFormationPreset, CONTINENTAL_CRUST_MAX_THICKNESS_KM, CONTINENTAL_CRUST_MIN_THICKNESS_KM,
    OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::{SpatialSnapshot, Topology};
use sekai::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec, PlateId, RootSeed};

fn spatial_fixture() -> &'static SpatialSnapshot {
    static SPATIAL: OnceLock<SpatialSnapshot> = OnceLock::new();
    SPATIAL.get_or_init(|| {
        let space = PlanarSpaceSpec {
            width: Meters::new(2_000_000.0).unwrap(),
            height: Meters::new(1_200_000.0).unwrap(),
            target_cell_count: 576,
            boundary: BoundaryCondition::Closed,
        };
        PlanarVoronoiBuilder::build(&space, &mut ChaCha8Rng::seed_from_u64(9001)).unwrap()
    })
}

fn natural_rng(seed: u64) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.tectonics", 2, "sekai.core"),
    ))
}

fn generate(seed: u64, spec: &TectonicSpec) -> sekai::world::natural::TectonicSnapshot {
    generate_with_preset(seed, spec, ResolvedWorldFormationPreset::Continents)
}

fn quality_spatial_fixture() -> &'static SpatialSnapshot {
    static SPATIAL: OnceLock<SpatialSnapshot> = OnceLock::new();
    SPATIAL.get_or_init(|| {
        let space = PlanarSpaceSpec {
            width: Meters::new(20_000_000.0).unwrap(),
            height: Meters::new(10_000_000.0).unwrap(),
            target_cell_count: 20_000,
            boundary: BoundaryCondition::Closed,
        };
        PlanarVoronoiBuilder::build(&space, &mut ChaCha8Rng::seed_from_u64(0x5E_A1)).unwrap()
    })
}

fn generate_with_preset(
    seed: u64,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
) -> sekai::world::natural::TectonicSnapshot {
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        preset,
    )
    .unwrap();
    TectonicGenerator::generate(spatial_fixture(), spec, &formation, &mut natural_rng(seed))
        .unwrap()
}

fn generate_quality(
    seed: u64,
    preset: ResolvedWorldFormationPreset,
    continental_crust_fraction: f32,
) -> sekai::world::natural::TectonicSnapshot {
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        WorldFormationPreset::Continents,
        preset,
    )
    .unwrap();
    TectonicGenerator::generate(
        quality_spatial_fixture(),
        &TectonicSpec {
            continental_crust_fraction,
            ..TectonicSpec::default()
        },
        &formation,
        &mut natural_rng(seed),
    )
    .unwrap()
}

#[test]
fn generation_is_repeatable_and_root_seed_sensitive() {
    let spec = TectonicSpec::default();
    let first = generate(42, &spec);
    let repeated = generate(42, &spec);
    let different = generate(43, &spec);

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&repeated).unwrap()
    );
    assert!(
        first.cell_plates().raw_values() != different.cell_plates().raw_values()
            || first.crust_kinds().raw_values() != different.crust_kinds().raw_values()
    );
}

#[test]
fn configured_plates_are_non_empty_connected_and_exact() {
    let spec = TectonicSpec::default();
    let snapshot = generate(42, &spec);
    let mut counts = vec![0_usize; spec.plate_count as usize];
    for &plate in snapshot.cell_plates().raw_values() {
        counts[plate as usize] += 1;
    }

    assert_eq!(snapshot.plates().len(), spec.plate_count as usize);
    assert!(counts.iter().all(|&count| count > 0));
    snapshot.validate_against(spatial_fixture()).unwrap();
}

#[test]
fn crust_area_and_thickness_obey_physical_contracts() {
    let spec = TectonicSpec::default();
    let snapshot = generate(42, &spec);
    let spatial = spatial_fixture();
    let total_area = spatial.total_cell_area().get();
    let target = total_area * f64::from(spec.continental_crust_fraction);
    let mut continental_area = 0.0;
    let mut maximum_cell_area = 0.0_f64;
    let mut kind_counts = [0_usize; 2];

    for index in 0..spatial.cell_count() {
        let cell = CellId::from_raw(index as u32);
        let area = spatial.cell(cell).unwrap().area.get();
        maximum_cell_area = maximum_cell_area.max(area);
        let kind = snapshot.crust_kind(cell).unwrap();
        let thickness = snapshot.crust_thickness_for_cell(cell).unwrap();
        match kind {
            CrustKind::Oceanic => {
                kind_counts[0] += 1;
                assert!(
                    (OCEANIC_CRUST_MIN_THICKNESS_KM..=OCEANIC_CRUST_MAX_THICKNESS_KM)
                        .contains(&thickness)
                );
            }
            CrustKind::Continental => {
                kind_counts[1] += 1;
                continental_area += area;
                assert!(
                    (CONTINENTAL_CRUST_MIN_THICKNESS_KM..=CONTINENTAL_CRUST_MAX_THICKNESS_KM)
                        .contains(&thickness)
                );
            }
        }
    }

    assert!(kind_counts.iter().all(|&count| count > 0));
    assert!((continental_area - target).abs() <= maximum_cell_area);
    println!(
        "cells={} plates={} oceanic={} continental={} continental_fraction={:.3}",
        spatial.cell_count(),
        snapshot.plates().len(),
        kind_counts[0],
        kind_counts[1],
        continental_area / total_area
    );
}

#[test]
fn changing_plate_count_cannot_perturb_the_independent_crust_field() {
    let baseline = TectonicSpec::default();
    let more_plates = TectonicSpec {
        plate_count: baseline.plate_count + 5,
        ..baseline.clone()
    };
    let first = generate(91, &baseline);
    let second = generate(91, &more_plates);

    assert_eq!(
        first.crust_kinds().raw_values(),
        second.crust_kinds().raw_values()
    );
    assert_eq!(first.crust_thickness_km(), second.crust_thickness_km());
}

#[test]
fn changing_formation_preset_cannot_perturb_plate_state() {
    let spec = TectonicSpec::default();
    let baseline = generate_with_preset(91, &spec, ResolvedWorldFormationPreset::Continents);
    for preset in [
        ResolvedWorldFormationPreset::Archipelago,
        ResolvedWorldFormationPreset::Supercontinent,
        ResolvedWorldFormationPreset::GreatIsland,
        ResolvedWorldFormationPreset::VolcanicIslands,
    ] {
        let candidate = generate_with_preset(91, &spec, preset);
        assert_eq!(candidate.plates(), baseline.plates());
        assert_eq!(
            candidate.cell_plates().raw_values(),
            baseline.cell_plates().raw_values()
        );
    }
}

#[derive(Debug)]
struct ComponentMetrics {
    component_count: usize,
    major_component_count: usize,
    largest_share_of_continental: f64,
}

fn continental_component_metrics(
    snapshot: &sekai::world::natural::TectonicSnapshot,
    spatial: &SpatialSnapshot,
) -> ComponentMetrics {
    let mut visited = vec![false; spatial.cell_count()];
    let mut areas = Vec::new();
    for start_index in 0..spatial.cell_count() {
        if visited[start_index]
            || snapshot.crust_kind(CellId::from_raw(start_index as u32))
                != Some(CrustKind::Continental)
        {
            continue;
        }
        let mut area = 0.0_f64;
        let mut queue = VecDeque::from([CellId::from_raw(start_index as u32)]);
        visited[start_index] = true;
        while let Some(cell) = queue.pop_front() {
            area += spatial.cell(cell).unwrap().area.get();
            for &neighbor in spatial.neighbors(cell).unwrap() {
                let index = neighbor.raw() as usize;
                if !visited[index] && snapshot.crust_kind(neighbor) == Some(CrustKind::Continental)
                {
                    visited[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        areas.push(area);
    }
    areas.sort_by(|first, second| second.total_cmp(first));
    let total_continental = areas.iter().sum::<f64>();
    let major_threshold = spatial.total_cell_area().get() * 0.015;
    ComponentMetrics {
        component_count: areas.len(),
        major_component_count: areas
            .iter()
            .filter(|&&area| area >= major_threshold)
            .count(),
        largest_share_of_continental: areas.first().copied().unwrap_or(0.0) / total_continental,
    }
}

fn assert_boundary_crust_is_oceanic(
    snapshot: &sekai::world::natural::TectonicSnapshot,
    spatial: &SpatialSnapshot,
) {
    for edge in spatial.edges() {
        let ([Some(owner), None] | [None, Some(owner)]) = edge.cells else {
            continue;
        };
        assert_eq!(snapshot.crust_kind(owner), Some(CrustKind::Oceanic));
    }
}

fn assert_continental_area_matches(
    snapshot: &sekai::world::natural::TectonicSnapshot,
    spatial: &SpatialSnapshot,
    fraction: f32,
) {
    let mut continental_area = 0.0_f64;
    let mut maximum_cell_area = 0.0_f64;
    for index in 0..spatial.cell_count() {
        let cell = CellId::from_raw(index as u32);
        let area = spatial.cell(cell).unwrap().area.get();
        maximum_cell_area = maximum_cell_area.max(area);
        if snapshot.crust_kind(cell) == Some(CrustKind::Continental) {
            continental_area += area;
        }
    }
    let target = spatial.total_cell_area().get() * f64::from(fraction);
    assert!((continental_area - target).abs() <= maximum_cell_area);
}

#[test]
fn preset_crust_profiles_have_distinct_macro_topology() {
    let spatial = quality_spatial_fixture();
    let cases = [
        (ResolvedWorldFormationPreset::Continents, 0.38),
        (ResolvedWorldFormationPreset::Archipelago, 0.26),
        (ResolvedWorldFormationPreset::Supercontinent, 0.42),
        (ResolvedWorldFormationPreset::GreatIsland, 0.28),
        (ResolvedWorldFormationPreset::VolcanicIslands, 0.16),
    ];
    let mut metrics = Vec::new();
    for (preset, fraction) in cases {
        let snapshot = generate_quality(0xC0_FFEE, preset, fraction);
        assert_boundary_crust_is_oceanic(&snapshot, spatial);
        assert_continental_area_matches(&snapshot, spatial, fraction);
        let component_metrics = continental_component_metrics(&snapshot, spatial);
        metrics.push((preset, component_metrics));
    }

    let continents = &metrics[0].1;
    assert!((3..=6).contains(&continents.major_component_count));
    assert!(continents.largest_share_of_continental <= 0.55);

    let archipelago = &metrics[1].1;
    assert!(archipelago.component_count >= 8);
    assert!(archipelago.largest_share_of_continental <= 0.30);

    let supercontinent = &metrics[2].1;
    assert_eq!(supercontinent.major_component_count, 1);
    assert!(supercontinent.largest_share_of_continental >= 0.85);

    let great_island = &metrics[3].1;
    assert_eq!(great_island.major_component_count, 1);
    assert!((0.60..=0.90).contains(&great_island.largest_share_of_continental));
    assert!(great_island.component_count >= 2);

    let volcanic = &metrics[4].1;
    assert!(volcanic.component_count >= 6);
    assert!(volcanic.largest_share_of_continental <= 0.35);
}

#[test]
fn fixed_quality_fixture_crosses_plate_and_crust_boundaries() {
    let snapshot = generate(42, &TectonicSpec::default());
    let spatial = spatial_fixture();
    let mut kinds_by_plate = vec![BTreeSet::new(); snapshot.plates().len()];
    for index in 0..spatial.cell_count() {
        let cell = CellId::from_raw(index as u32);
        let plate = snapshot.plate_for_cell(cell).unwrap();
        kinds_by_plate[plate.raw() as usize].insert(snapshot.crust_kind(cell).unwrap().raw());
    }
    assert!(kinds_by_plate.iter().any(|kinds| kinds.len() > 1));
    assert!(has_crust_component_spanning_plates(&snapshot, spatial));
}

fn has_crust_component_spanning_plates(
    snapshot: &sekai::world::natural::TectonicSnapshot,
    spatial: &SpatialSnapshot,
) -> bool {
    let mut visited = vec![false; spatial.cell_count()];
    for start_index in 0..spatial.cell_count() {
        if visited[start_index] {
            continue;
        }
        let start = CellId::from_raw(start_index as u32);
        let kind = snapshot.crust_kind(start).unwrap();
        let mut plates = BTreeSet::<PlateId>::new();
        let mut queue = VecDeque::from([start]);
        visited[start_index] = true;
        while let Some(cell) = queue.pop_front() {
            plates.insert(snapshot.plate_for_cell(cell).unwrap());
            for &neighbor in spatial.neighbors(cell).unwrap() {
                let index = neighbor.raw() as usize;
                if !visited[index] && snapshot.crust_kind(neighbor) == Some(kind) {
                    visited[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        if plates.len() > 1 {
            return true;
        }
    }
    false
}
