use std::collections::BTreeSet;

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sekai::generators::spatial::{JitteredGridSites, PlanarVoronoiBuilder, SpatialBuildError};
use sekai::world::spatial::{SpatialSnapshot, SpatialValidationError, Topology};
use sekai::world::{BoundaryCondition, CellId, Meters, PlanarSpaceSpec, SpecError};

fn small_space() -> PlanarSpaceSpec {
    PlanarSpaceSpec {
        width: Meters::new(1_000.0).unwrap(),
        height: Meters::new(500.0).unwrap(),
        target_cell_count: 128,
        boundary: BoundaryCondition::Closed,
    }
}

struct ConstantRng(u64);

impl RngCore for ConstantRng {
    fn next_u32(&mut self) -> u32 {
        self.0 as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.0
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(std::mem::size_of::<u64>()) {
            chunk.copy_from_slice(&self.0.to_le_bytes()[..chunk.len()]);
        }
    }
}

#[test]
fn same_rng_seed_produces_identical_spatial_json() {
    let mut first_rng = ChaCha8Rng::seed_from_u64(42);
    let mut second_rng = ChaCha8Rng::seed_from_u64(42);

    let first = PlanarVoronoiBuilder::build(&small_space(), &mut first_rng).unwrap();
    let second = PlanarVoronoiBuilder::build(&small_space(), &mut second_rng).unwrap();

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}

#[test]
fn generated_partition_satisfies_spatial_invariants_and_count_bound() {
    let mut space = small_space();
    space.target_cell_count = 130;
    let mut rng = ChaCha8Rng::seed_from_u64(7);

    let snapshot = PlanarVoronoiBuilder::build(&space, &mut rng).unwrap();

    snapshot.validate().unwrap();
    assert!(snapshot.cell_count() >= 130);
    assert!(snapshot.cell_count() < 130 + 17);
}

#[test]
fn different_rng_seeds_change_at_least_one_site() {
    let mut first_rng = ChaCha8Rng::seed_from_u64(42);
    let mut second_rng = ChaCha8Rng::seed_from_u64(43);

    let first = PlanarVoronoiBuilder::build(&small_space(), &mut first_rng).unwrap();
    let second = PlanarVoronoiBuilder::build(&small_space(), &mut second_rng).unwrap();

    assert!((0..first.cell_count()).any(|index| {
        first.cell(CellId::from_raw(index as u32)).unwrap().site
            != second.cell(CellId::from_raw(index as u32)).unwrap().site
    }));
}

#[test]
fn generated_cell_ids_are_contiguous() {
    let mut rng = ChaCha8Rng::seed_from_u64(99);
    let snapshot = PlanarVoronoiBuilder::build(&small_space(), &mut rng).unwrap();

    for index in 0..snapshot.cell_count() {
        let expected = CellId::from_raw(index as u32);
        assert_eq!(snapshot.cell(expected).unwrap().id, expected);
    }
}

#[test]
fn invalid_aspect_ratio_is_rejected_before_rng_use() {
    struct PanicRng;

    impl RngCore for PanicRng {
        fn next_u32(&mut self) -> u32 {
            panic!("invalid space must be rejected before drawing randomness")
        }

        fn next_u64(&mut self) -> u64 {
            panic!("invalid space must be rejected before drawing randomness")
        }

        fn fill_bytes(&mut self, _dest: &mut [u8]) {
            panic!("invalid space must be rejected before drawing randomness")
        }
    }

    let space = PlanarSpaceSpec {
        width: Meters::new(1_000.0).unwrap(),
        height: Meters::new(1.0).unwrap(),
        target_cell_count: 128,
        boundary: BoundaryCondition::Closed,
    };
    assert!(matches!(
        space.validate(),
        Err(SpecError::AspectRatioOutOfRange { .. })
    ));

    assert!(matches!(
        PlanarVoronoiBuilder::build(&space, &mut PanicRng),
        Err(SpatialBuildError::InvalidSpec(
            SpecError::AspectRatioOutOfRange { .. }
        ))
    ));
}

#[test]
fn extreme_u64_seeds_generate_valid_partitions() {
    for seed in [0, 1, u64::MAX] {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let snapshot = PlanarVoronoiBuilder::build(&small_space(), &mut rng).unwrap();

        snapshot.validate().unwrap();
    }
}

#[test]
fn jittered_grid_sites_are_unique() {
    let mut rng = ChaCha8Rng::seed_from_u64(17);
    let generated = JitteredGridSites::generate(&small_space(), &mut rng).unwrap();

    let unique_sites: BTreeSet<_> = generated
        .sites()
        .iter()
        .map(|site| (site.x().get().to_bits(), site.y().get().to_bits()))
        .collect();

    assert_eq!(generated.columns(), 16);
    assert_eq!(generated.rows(), 8);
    assert_eq!(unique_sites.len(), generated.sites().len());
}

#[test]
fn jittered_sites_follow_y_major_slots_and_strict_count_bound() {
    let mut space = small_space();
    space.target_cell_count = 130;
    let mut rng = ChaCha8Rng::seed_from_u64(23);
    let generated = JitteredGridSites::generate(&space, &mut rng).unwrap();
    let cell_width = space.width.get() / generated.columns() as f64;
    let cell_height = space.height.get() / generated.rows() as f64;

    assert!(generated.sites().len() >= space.target_cell_count as usize);
    assert!(generated.sites().len() < space.target_cell_count as usize + generated.columns());
    for (index, site) in generated.sites().iter().enumerate() {
        let row = index / generated.columns();
        let column = index % generated.columns();
        assert!(site.x().get() > column as f64 * cell_width);
        assert!(site.x().get() < (column + 1) as f64 * cell_width);
        assert!(site.y().get() > row as f64 * cell_height);
        assert!(site.y().get() < (row + 1) as f64 * cell_height);
    }
}

#[test]
fn area_perturbations_below_contract_tolerances_are_accepted_and_above_are_rejected() {
    fn perturb_areas(snapshot: &SpatialSnapshot, relative_delta: f64) -> SpatialSnapshot {
        let mut wire = serde_json::to_value(snapshot).unwrap();
        for cell in wire["cells"].as_array_mut().unwrap() {
            let area = cell["area"].as_f64().unwrap();
            cell["area"] = serde_json::json!(area * (1.0 + relative_delta));
        }
        serde_json::from_value(wire).unwrap()
    }

    let mut rng = ChaCha8Rng::seed_from_u64(31);
    let snapshot = PlanarVoronoiBuilder::build(&small_space(), &mut rng).unwrap();
    let below = perturb_areas(&snapshot, 5.0e-9);
    let above = perturb_areas(&snapshot, 2.0e-7);

    below.validate().unwrap();
    assert!(matches!(
        above.validate(),
        Err(SpatialValidationError::AreaMismatch { .. }
            | SpatialValidationError::TotalAreaMismatch { .. })
    ));
}

#[test]
fn constant_rng_builds_repeatable_one_row_partition() {
    let space = PlanarSpaceSpec {
        width: Meters::new(1_600.0).unwrap(),
        height: Meters::new(100.0).unwrap(),
        target_cell_count: 16,
        boundary: BoundaryCondition::Closed,
    };
    let mut first_rng = ConstantRng(0);
    let mut second_rng = ConstantRng(0);

    let first = PlanarVoronoiBuilder::build(&space, &mut first_rng).unwrap();
    let second = PlanarVoronoiBuilder::build(&space, &mut second_rng).unwrap();

    assert_eq!(first.cell_count(), 16);
    first.validate().unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}

#[test]
fn constant_rng_builds_repeatable_one_column_partition() {
    let space = PlanarSpaceSpec {
        width: Meters::new(100.0).unwrap(),
        height: Meters::new(1_600.0).unwrap(),
        target_cell_count: 16,
        boundary: BoundaryCondition::Closed,
    };
    let mut first_rng = ConstantRng(u64::MAX);
    let mut second_rng = ConstantRng(u64::MAX);

    let first = PlanarVoronoiBuilder::build(&space, &mut first_rng).unwrap();
    let second = PlanarVoronoiBuilder::build(&space, &mut second_rng).unwrap();

    assert_eq!(first.cell_count(), 16);
    first.validate().unwrap();
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}
