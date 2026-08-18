use std::collections::BTreeMap;

use sekai::generators::spatial::{
    ConservativeRemapError, ConservativeSurfaceMapBuilder, GeodesicVoronoiBuilder,
};
use sekai::world::spatial::{ConservativeSurfaceMap, SurfaceRef};
use sekai::world::{CellId, Meters, SphericalSpaceSpec};

const RADIUS_M: f64 = 6_371_000.0;

fn surface(target_cell_count: u32) -> sekai::world::spatial::SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(RADIUS_M).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn overlap_inventory(map: &ConservativeSurfaceMap) -> BTreeMap<(u32, u32), f64> {
    let mut inventory = BTreeMap::new();
    for target in 0..map.target_ref().cell_count() {
        for weight in map.target_row(CellId::from_raw(target)).unwrap() {
            assert!(inventory
                .insert((target, weight.source_cell().raw()), weight.area_m2())
                .is_none());
        }
    }
    inventory
}

#[test]
fn identical_surface_builds_an_exact_identity_map() {
    let source = surface(42);
    let map = ConservativeSurfaceMapBuilder::build(&source, &source).unwrap();
    assert_eq!(map.source_ref(), SurfaceRef::for_spherical(&source));
    assert_eq!(map.target_ref(), SurfaceRef::for_spherical(&source));
    assert_eq!(map.overlap_count(), source.cells().len());
    assert_eq!(map.solve_stats().balance_iterations(), 0);
    assert_eq!(map.solve_stats().max_source_margin_relative_error(), 0.0);
    assert_eq!(map.solve_stats().max_target_margin_relative_error(), 0.0);
    for (index, cell) in source.cells().iter().enumerate() {
        let row = map.target_row(CellId::from_raw(index as u32)).unwrap();
        assert_eq!(row.len(), 1);
        assert_eq!(row[0].source_cell(), cell.id);
        assert_eq!(row[0].area_m2().to_bits(), cell.area.get().to_bits());
        assert_eq!(
            row[0].tangent_transform().coefficients(),
            [1.0, 0.0, 0.0, 1.0]
        );
    }
}

#[test]
fn coarse_fine_maps_close_both_margins_and_transpose_the_same_geometry() {
    let coarse = surface(42);
    let fine = surface(162);
    let coarse_to_fine = ConservativeSurfaceMapBuilder::build(&coarse, &fine).unwrap();
    let fine_to_coarse = ConservativeSurfaceMapBuilder::build(&fine, &coarse).unwrap();

    for map in [&coarse_to_fine, &fine_to_coarse] {
        map.validate().unwrap();
        assert!(map.solve_stats().max_source_margin_relative_error() <= 1.0e-10);
        assert!(map.solve_stats().max_target_margin_relative_error() <= 1.0e-10);
        assert!(map.solve_stats().max_relative_geometric_adjustment() <= 1.0e-4);
        assert!(map.overlap_count() >= fine.cells().len());
        assert!(map.overlap_count() <= fine.cells().len() * 8);
        let overlap_total = map
            .weights()
            .iter()
            .map(|weight| weight.area_m2())
            .sum::<f64>();
        let sphere_area = fine.total_cell_area().get();
        assert!((overlap_total - sphere_area).abs() / sphere_area <= 1.0e-10);
    }

    let forward = overlap_inventory(&coarse_to_fine);
    let reverse = overlap_inventory(&fine_to_coarse);
    assert_eq!(forward.len(), reverse.len());
    for ((fine_cell, coarse_cell), area) in forward {
        let reverse_area = reverse.get(&(coarse_cell, fine_cell)).unwrap();
        assert_eq!(area.to_bits(), reverse_area.to_bits());
    }
}

#[test]
fn map_generation_is_byte_deterministic_and_rejects_different_planet_radii() {
    let source = surface(42);
    let target = surface(162);
    let first = ConservativeSurfaceMapBuilder::build(&source, &target).unwrap();
    let second = ConservativeSurfaceMapBuilder::build(&source, &target).unwrap();
    let first_bytes = serde_json::to_vec(&first).unwrap();
    let second_bytes = serde_json::to_vec(&second).unwrap();
    assert_eq!(first_bytes, second_bytes);
    assert_eq!(blake3::hash(&first_bytes), blake3::hash(&second_bytes));

    let other_radius = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(RADIUS_M + 1.0).unwrap(),
        target_cell_count: 162,
    })
    .unwrap();
    assert!(matches!(
        ConservativeSurfaceMapBuilder::build(&source, &other_radius),
        Err(ConservativeRemapError::RadiusMismatch { .. })
    ));
}

#[test]
fn cancellable_builder_returns_no_partial_map() {
    let source = surface(42);
    let target = surface(162);
    let mut checks = 0_u32;
    let result = ConservativeSurfaceMapBuilder::build_cancellable(&source, &target, || {
        checks += 1;
        checks >= 2
    });
    assert!(matches!(result, Err(ConservativeRemapError::Cancelled)));
    assert!(
        checks <= 4,
        "cancellation checks were not bounded: {checks}"
    );
}

#[test]
fn cancellable_builder_polls_through_final_sparse_publication_and_validation() {
    let source = surface(42);
    let target = surface(162);
    let mut baseline_checks = 0_u64;
    let expected = ConservativeSurfaceMapBuilder::build_cancellable(&source, &target, || {
        baseline_checks += 1;
        false
    })
    .unwrap();
    assert!(baseline_checks > 32, "only {baseline_checks} total polls");

    let cancel_at = baseline_checks - 2;
    let mut observed = 0_u64;
    let cancelled = ConservativeSurfaceMapBuilder::build_cancellable(&source, &target, || {
        observed += 1;
        observed >= cancel_at
    });
    assert_eq!(cancelled, Err(ConservativeRemapError::Cancelled));
    assert!(observed >= cancel_at);

    let rebuilt = ConservativeSurfaceMapBuilder::build(&source, &target).unwrap();
    assert_eq!(expected, rebuilt);
}

#[test]
#[ignore = "Draft product-scale conservative-map measurement"]
fn draft_control_to_authoritative_map_meets_product_closure() {
    let control = surface(4_842);
    let authoritative = surface(20_000);
    let started = std::time::Instant::now();
    let map = ConservativeSurfaceMapBuilder::build(&control, &authoritative).unwrap();
    println!(
        "draft_conservative_map source={} target={} overlaps={} iterations={} source_error={} target_error={} adjustment={} elapsed_ms={:.3}",
        map.source_ref().cell_count(),
        map.target_ref().cell_count(),
        map.overlap_count(),
        map.solve_stats().balance_iterations(),
        map.solve_stats().max_source_margin_relative_error(),
        map.solve_stats().max_target_margin_relative_error(),
        map.solve_stats().max_relative_geometric_adjustment(),
        started.elapsed().as_secs_f64() * 1_000.0,
    );
    assert_eq!(map.source_ref().cell_count(), 4_842);
    assert_eq!(map.target_ref().cell_count(), 20_252);
    assert!(map.solve_stats().max_source_margin_relative_error() <= 1.0e-10);
    assert!(map.solve_stats().max_target_margin_relative_error() <= 1.0e-10);
}
