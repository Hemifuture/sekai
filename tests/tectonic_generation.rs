use std::collections::{BTreeSet, VecDeque};
use std::sync::OnceLock;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::TectonicGenerator;
use sekai::generators::spatial::PlanarVoronoiBuilder;
use sekai::world::natural::{
    CrustKind, TectonicSpec, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, OCEANIC_CRUST_MAX_THICKNESS_KM,
    OCEANIC_CRUST_MIN_THICKNESS_KM,
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
        StageIdentity::new("natural.tectonics", 1, "sekai.core"),
    ))
}

fn generate(seed: u64, spec: &TectonicSpec) -> sekai::world::natural::TectonicSnapshot {
    TectonicGenerator::generate(spatial_fixture(), spec, &mut natural_rng(seed)).unwrap()
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
