use std::collections::VecDeque;
use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{MantleGenerator, ReliefGenerator, TectonicGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    BoundaryKind, CrustKind, GeologicSpec, LandOceanKind, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, SphericalOrogenyKind, TectonicSpec, WorldFormationPreset,
    RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{Meters, RootSeed, SphericalSpaceSpec};

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const CAUSALITY_CELL_COUNT: u32 = 2_562;
const CAUSALITY_SEEDS: [u64; 17] = [
    42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
];

fn surface() -> &'static SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(EARTH_RADIUS_M).unwrap(),
            target_cell_count: CAUSALITY_CELL_COUNT,
        })
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

fn rng(seed: u64, stage: &'static str, version: u32) -> StageRng {
    StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new(stage, version, "sekai.core"),
    ))
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

#[test]
fn current_crust_material_is_coherent_without_cell_checkerboarding() {
    let surface = surface();
    let formation = formation();
    let mut cross_kind_edge_fractions = Vec::new();
    let mut checkerboard_cell_fractions = Vec::new();
    let mut tiny_component_area_fractions = Vec::new();

    for seed in CAUSALITY_SEEDS {
        let tectonic = TectonicGenerator::generate_spherical(
            surface,
            &TectonicSpec::default(),
            &formation,
            &mut rng(seed, "natural.spherical-tectonics", 3),
        )
        .unwrap();
        let total_edge_length = surface
            .edges()
            .iter()
            .map(|edge| edge.length.get())
            .sum::<f64>();
        let cross_kind_length = surface
            .edges()
            .iter()
            .filter(|edge| tectonic.crust_kind(edge.cells[0]) != tectonic.crust_kind(edge.cells[1]))
            .map(|edge| edge.length.get())
            .sum::<f64>();
        cross_kind_edge_fractions.push(cross_kind_length / total_edge_length);

        let checkerboard_cells = surface
            .cells()
            .iter()
            .filter(|cell| {
                let kind = tectonic.crust_kind(cell.id);
                let (opposite, count) = cell
                    .boundary_edges
                    .iter()
                    .map(|&edge| surface.opposite_cell(cell.id, edge).unwrap())
                    .fold((0_usize, 0_usize), |(opposite, count), neighbor| {
                        (
                            opposite + usize::from(tectonic.crust_kind(neighbor) != kind),
                            count + 1,
                        )
                    });
                opposite * 2 >= count
            })
            .count();
        checkerboard_cell_fractions.push(checkerboard_cells as f64 / surface.cells().len() as f64);

        let mut visited = vec![false; surface.cells().len()];
        let mut tiny_area = 0.0;
        for cell in surface.cells() {
            let start = cell.id.raw() as usize;
            if visited[start] {
                continue;
            }
            visited[start] = true;
            let kind = tectonic.crust_kind(cell.id);
            let mut pending = VecDeque::from([cell.id]);
            let mut component = Vec::new();
            while let Some(current) = pending.pop_front() {
                component.push(current);
                for &edge in surface.cell_edges(current).unwrap() {
                    let neighbor = surface.opposite_cell(current, edge).unwrap();
                    let index = neighbor.raw() as usize;
                    if !visited[index] && tectonic.crust_kind(neighbor) == kind {
                        visited[index] = true;
                        pending.push_back(neighbor);
                    }
                }
            }
            if component.len() <= 3 {
                tiny_area += component
                    .into_iter()
                    .map(|cell| surface.cell(cell).unwrap().area.get())
                    .sum::<f64>();
            }
        }
        tiny_component_area_fractions.push(tiny_area / surface.total_cell_area().get());
    }

    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
    println!(
        "material coherence: cross-kind edge={:?} mean={:.4}, checkerboard={:?} mean={:.4}, tiny-area={:?} mean={:.4}",
        cross_kind_edge_fractions,
        mean(&cross_kind_edge_fractions),
        checkerboard_cell_fractions,
        mean(&checkerboard_cell_fractions),
        tiny_component_area_fractions,
        mean(&tiny_component_area_fractions),
    );
    assert!(mean(&cross_kind_edge_fractions) <= 0.06);
    assert!(cross_kind_edge_fractions
        .iter()
        .all(|fraction| *fraction <= 0.075));
    assert!(mean(&checkerboard_cell_fractions) <= 0.05);
    assert!(checkerboard_cell_fractions
        .iter()
        .all(|fraction| *fraction <= 0.065));
    assert!(mean(&tiny_component_area_fractions) <= 0.001);
    assert!(tiny_component_area_fractions
        .iter()
        .all(|fraction| *fraction <= 0.003));
}

#[test]
fn final_current_state_preserves_tectonic_cause_and_side_across_seeds() {
    let surface = surface();
    let formation = formation();
    let mut subduction_relief_difference = Vec::new();
    let mut ridge_ages = Vec::new();
    let mut ridge_elevations = Vec::new();
    let mut old_ocean_ages = Vec::new();
    let mut old_ocean_elevations = Vec::new();
    let mut collision_offsets = Vec::new();
    let mut transform_offsets = Vec::new();
    let mut transform_signed_offsets = Vec::new();
    let mut convergent_offsets = Vec::new();
    let mut convergent_signed_offsets = Vec::new();
    let mut andean_overriding = 0_usize;
    let mut andean_descending = 0_usize;
    let mut himalayan_collision = 0_usize;
    let mut convergent_active_orogenic = 0_usize;
    let mut convergent_endpoint_count = 0_usize;
    let mut transform_active_orogenic = 0_usize;
    let mut transform_endpoint_count = 0_usize;
    let mut land_continental_intersection_m2 = 0.0;
    let mut land_continental_union_m2 = 0.0;
    let mut continental_area_m2 = 0.0;
    let mut continental_land_area_m2 = 0.0;
    let mut continental_coarse_elevations = Vec::new();
    let mut continental_final_elevations = Vec::new();
    let mut continental_coarse_by_orogeny = [Vec::new(), Vec::new(), Vec::new()];
    let mut continental_age_by_orogeny = [Vec::new(), Vec::new(), Vec::new()];

    for seed in CAUSALITY_SEEDS {
        let tectonic = TectonicGenerator::generate_spherical(
            surface,
            &TectonicSpec::default(),
            &formation,
            &mut rng(seed, "natural.spherical-tectonics", 3),
        )
        .unwrap();
        let mantle = MantleGenerator::generate_spherical(
            surface,
            &GeologicSpec::default(),
            formation.mantle_bias(),
            &mut rng(seed, "natural.spherical-mantle", 1),
        )
        .unwrap();
        let relief = ReliefGenerator::generate_spherical(
            surface,
            &tectonic,
            &mantle,
            &ReliefSpec::default(),
            &mut rng(seed, "natural.spherical-relief", 2),
            &mut Vec::<Diagnostic>::new(),
        )
        .unwrap();

        for cell in surface.cells() {
            let index = cell.id.raw() as usize;
            let continental = tectonic.crust_kind(cell.id) == Some(CrustKind::Continental);
            let land = relief.land_ocean_kind(cell.id) == Some(LandOceanKind::Land);
            if continental && land {
                land_continental_intersection_m2 += cell.area.get();
                continental_land_area_m2 += cell.area.get();
            }
            if continental {
                continental_area_m2 += cell.area.get();
                continental_coarse_elevations
                    .push(f64::from(tectonic.tectonic_elevation_m()[index]));
                continental_final_elevations.push(f64::from(relief.elevation_m().values()[index]));
                let orogeny_index = match tectonic.orogeny_kind()[index] {
                    SphericalOrogenyKind::None => 0,
                    SphericalOrogenyKind::Andean => 1,
                    SphericalOrogenyKind::Himalayan => 2,
                };
                continental_coarse_by_orogeny[orogeny_index]
                    .push(f64::from(tectonic.tectonic_elevation_m()[index]));
                if orogeny_index != 0 {
                    continental_age_by_orogeny[orogeny_index]
                        .push(f64::from(tectonic.orogeny_age_myr()[index]));
                }
            }
            if continental || land {
                land_continental_union_m2 += cell.area.get();
            }
            if tectonic.crust_kind(cell.id) == Some(CrustKind::Oceanic)
                && tectonic.crust_age_myr()[index] >= 75.0
            {
                old_ocean_ages.push(f64::from(tectonic.crust_age_myr()[index]));
                old_ocean_elevations.push(f64::from(tectonic.tectonic_elevation_m()[index]));
            }
        }

        for edge in surface.edges() {
            let boundary = tectonic.boundaries()[edge.id.raw() as usize];
            let indices = edge.cells.map(|cell| cell.raw() as usize);
            let elevations = indices.map(|index| tectonic.tectonic_elevation_m()[index]);
            let offsets = indices.map(|index| relief.tectonic_offset_m().values()[index]);
            match boundary.kind {
                BoundaryKind::Subduction => {
                    let descending = boundary.subducting_plate.unwrap();
                    let descending_index =
                        usize::from(tectonic.plate_for_cell(edge.cells[1]).unwrap() == descending);
                    let overriding_index = 1 - descending_index;
                    subduction_relief_difference.push(f64::from(
                        elevations[overriding_index] - elevations[descending_index],
                    ));
                    convergent_offsets.push(f64::from(offsets[overriding_index].abs()));
                    convergent_signed_offsets.push(f64::from(offsets[overriding_index]));
                    convergent_endpoint_count += 1;
                    convergent_active_orogenic += usize::from(
                        tectonic.orogeny_kind()[indices[overriding_index]]
                            != SphericalOrogenyKind::None
                            && tectonic.orogeny_age_myr()[indices[overriding_index]] <= 32.0,
                    );
                    if tectonic.orogeny_kind()[indices[overriding_index]]
                        == SphericalOrogenyKind::Andean
                    {
                        andean_overriding += 1;
                    }
                    if tectonic.orogeny_kind()[indices[descending_index]]
                        == SphericalOrogenyKind::Andean
                    {
                        andean_descending += 1;
                    }
                }
                BoundaryKind::OceanicRidge => {
                    for index in indices {
                        if tectonic.crust_kinds().get(index) == Some(CrustKind::Oceanic) {
                            ridge_ages.push(f64::from(tectonic.crust_age_myr()[index]));
                            ridge_elevations
                                .push(f64::from(tectonic.tectonic_elevation_m()[index]));
                        }
                    }
                }
                BoundaryKind::ContinentalCollision => {
                    for (side, index) in indices.into_iter().enumerate() {
                        collision_offsets.push(f64::from(offsets[side]));
                        convergent_offsets.push(f64::from(offsets[side].abs()));
                        convergent_signed_offsets.push(f64::from(offsets[side]));
                        if tectonic.orogeny_kind()[index] == SphericalOrogenyKind::Himalayan {
                            himalayan_collision += 1;
                        }
                        convergent_endpoint_count += 1;
                        convergent_active_orogenic += usize::from(
                            tectonic.orogeny_kind()[index] != SphericalOrogenyKind::None
                                && tectonic.orogeny_age_myr()[index] <= 32.0,
                        );
                    }
                }
                BoundaryKind::Transform => {
                    transform_offsets.extend(offsets.map(|offset| f64::from(offset.abs())));
                    transform_signed_offsets.extend(offsets.map(f64::from));
                    for index in indices {
                        transform_endpoint_count += 1;
                        transform_active_orogenic += usize::from(
                            tectonic.orogeny_kind()[index] != SphericalOrogenyKind::None
                                && tectonic.orogeny_age_myr()[index] <= 32.0,
                        );
                    }
                }
                BoundaryKind::None | BoundaryKind::Weak | BoundaryKind::ContinentalRift => {}
            }
        }
    }

    assert!(
        !subduction_relief_difference.is_empty(),
        "seed matrix has no subduction"
    );
    assert!(!ridge_ages.is_empty(), "seed matrix has no oceanic ridge");
    assert!(
        !old_ocean_ages.is_empty(),
        "seed matrix has no old oceanic crust"
    );
    assert!(
        !collision_offsets.is_empty(),
        "seed matrix has no continental collision"
    );
    assert!(
        !transform_offsets.is_empty(),
        "seed matrix has no transform boundary"
    );

    let subduction_correct_fraction = subduction_relief_difference
        .iter()
        .filter(|difference| **difference > 0.0)
        .count() as f64
        / subduction_relief_difference.len() as f64;
    let subduction_difference_median = median(&mut subduction_relief_difference);
    let ridge_age_median = median(&mut ridge_ages);
    let old_ocean_age_median = median(&mut old_ocean_ages);
    let ridge_elevation_median = median(&mut ridge_elevations);
    let old_ocean_elevation_median = median(&mut old_ocean_elevations);
    let collision_offset_median = median(&mut collision_offsets);
    let transform_offset_median = median(&mut transform_offsets);
    let transform_signed_median = median(&mut transform_signed_offsets);
    let convergent_offset_median = median(&mut convergent_offsets);
    let convergent_signed_median = median(&mut convergent_signed_offsets);
    let land_crust_jaccard = land_continental_intersection_m2 / land_continental_union_m2;
    let continental_land_fraction = continental_land_area_m2 / continental_area_m2;
    let continental_coarse_median = median(&mut continental_coarse_elevations);
    let continental_final_median = median(&mut continental_final_elevations);
    let continental_orogeny_summary = continental_coarse_by_orogeny
        .iter_mut()
        .map(|values| (values.len(), (!values.is_empty()).then(|| median(values))))
        .collect::<Vec<_>>();
    let continental_orogeny_age_summary = continental_age_by_orogeny
        .iter_mut()
        .map(|values| (values.len(), (!values.is_empty()).then(|| median(values))))
        .collect::<Vec<_>>();
    let convergent_active_fraction =
        convergent_active_orogenic as f64 / convergent_endpoint_count as f64;
    let transform_active_fraction =
        transform_active_orogenic as f64 / transform_endpoint_count as f64;
    eprintln!(
        "causality aggregate: subduction={} correct={subduction_correct_fraction:.4} median_delta={subduction_difference_median:.1}m andean_override={andean_overriding} andean_descend={andean_descending}; ridge={} age={ridge_age_median:.2}mya elev={ridge_elevation_median:.1}m; old_ocean={} age={old_ocean_age_median:.2}mya elev={old_ocean_elevation_median:.1}m; collision={} offset={collision_offset_median:.1}m himalayan={himalayan_collision}; transform={} abs_offset={transform_offset_median:.1}m signed={transform_signed_median:.1}m active={transform_active_fraction:.4} convergent_abs={convergent_offset_median:.1}m signed={convergent_signed_median:.1}m active={convergent_active_fraction:.4}; continental land={continental_land_fraction:.4} coarse_median={continental_coarse_median:.1}m final_median={continental_final_median:.1}m by_orogeny={continental_orogeny_summary:?} ages={continental_orogeny_age_summary:?}; land/crust_jaccard={land_crust_jaccard:.4}",
        subduction_relief_difference.len(),
        ridge_elevations.len(),
        old_ocean_elevations.len(),
        collision_offsets.len(),
        transform_offsets.len(),
    );

    // Exact process-unit oracles lock the applied side. At the final current
    // boundary, conservative material-interface regularization may expose an
    // inherited elevation from an earlier contact, so the aggregate freezes a
    // strict positive majority plus the stronger Andean-side invariant.
    assert!(subduction_correct_fraction >= 0.55);
    assert!(subduction_difference_median > 250.0);
    assert!(andean_overriding > 0);
    assert_eq!(andean_descending, 0);
    assert!(himalayan_collision > 0);
    assert!(ridge_age_median < old_ocean_age_median);
    assert!(ridge_elevation_median > old_ocean_elevation_median);
    assert!(collision_offset_median > 0.0);
    assert!(transform_active_fraction < convergent_active_fraction * 0.5);
    assert!((0.55..=0.85).contains(&continental_land_fraction));
    assert!((0.45..=0.85).contains(&land_crust_jaccard));
}
