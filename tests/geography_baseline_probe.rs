//! G0 geography-baseline probe (spec 2026-08-26-g0-geography-baseline-design).
//!
//! Diagnostic writer, not a gate. Run explicitly:
//! `cargo test --release --test geography_baseline_probe probe_g0_geography_baseline -- --ignored --nocapture`

mod support;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use sekai::engine::BuildCancellation;
use sekai::generators::natural::evaluate_primary_relief_quality;
use sekai::generators::spatial::ProfileSurfaceBuilder;
use sekai::world::natural::{
    hypsometric_mean, hypsometric_quantile, hypsometric_share_below, sort_hypsometric_samples,
    BoundaryKind, CrustKind, EvolvedTectonicSnapshot, GeologicSubstrateSnapshot, LandOceanKind,
    NaturalQualityProfile, PrimaryReliefSnapshot, ResolvedFormationTimeline,
    ResolvedWorldFormationPreset, TectonicSpec, EARTH_WATER_REFERENCE_RADIUS_M,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::Meters;
use serde::Serialize;
use support::global_circulation::try_build_primary_relief_for;

const PRIMARY_SEED: u64 = 42;
const CONTRAST_SEED: u64 = 3;
const PRESETS: [ResolvedWorldFormationPreset; 3] = [
    ResolvedWorldFormationPreset::Continents,
    ResolvedWorldFormationPreset::Supercontinent,
    ResolvedWorldFormationPreset::Archipelago,
];
const T0_LOWLAND_CEILING_M: f32 = 100.0;
const SHELF_CEILING_M: f32 = 1_000.0;
const DISTANCE_BINS: [u32; 5] = [0, 1, 2, 4, 8];
const NEARLY_CONTINENTAL_PLATE: f64 = 0.8;
const NEARLY_OCEANIC_PLATE: f64 = 0.2;
const CONTINENTAL_COVERAGE_TARGET: f64 = 0.9;

#[derive(Debug, Clone, Serialize)]
struct GeographyBaselineWorld {
    preset: String,
    seed: u64,
    elapsed_ms: u128,
    error: Option<String>,
    report: Option<GeographyBaselineReport>,
}

#[derive(Debug, Clone, Serialize)]
struct GeographyBaselineReport {
    preset: String,
    seed: u64,
    elapsed_ms: u128,
    cells: usize,
    plates: usize,
    mechanical_fragmentation_count: u32,
    continental_area_fraction: f64,
    oceanic_area_fraction: f64,
    crust_component_count: usize,
    crust_max_area_share: f64,
    crust_majority_plate_share: f64,
    crust_mean_plates_per_component: f64,
    plates_to_cover_90pct_crust: usize,
    nearly_continental_plates: usize,
    nearly_oceanic_plates: usize,
    plate_crust_fraction_min: f64,
    plate_crust_fraction_median: f64,
    plate_crust_fraction_max: f64,
    land_component_count: usize,
    land_max_area_share: f64,
    continental_inundation_share: f64,
    physical_land_fraction: f64,
    land_relief_p05_m: f32,
    land_relief_p25_m: f32,
    land_relief_p50_m: f32,
    land_relief_p75_m: f32,
    land_relief_p95_m: f32,
    land_relief_mean_m: f64,
    land_share_below_100m: f64,
    ocean_depth_p50_m: f32,
    wet_area_shallower_than_1000m_share: f64,
    ocean_age_p05_myr: f32,
    ocean_age_p25_myr: f32,
    ocean_age_p50_myr: f32,
    ocean_age_p75_myr: f32,
    ocean_age_p95_myr: f32,
    ridge_adjacent_age_p50_myr: f32,
    age_le_horizon_oceanic_share: f64,
    spreading_created_over_final_oceanic: f64,
    coverage_created_over_final_oceanic: f64,
    created_over_final_oceanic: f64,
    old_young_ocean_depth_separation_m: Option<f64>,
    ridge_edge_count: usize,
    plate_boundary_edge_count: usize,
    age_by_ridge_hops: [f32; 5],
    age_by_plate_boundary_hops: [f32; 5],
}

#[test]
fn connected_components_only_merge_masked_neighbors() {
    let adj = vec![vec![1], vec![0, 2], vec![1], vec![], vec![5], vec![4]];
    let mask = [true, true, true, false, true, true];
    let areas = [1.0, 2.0, 3.0, 10.0, 4.0, 5.0];
    let stats = component_stats(adj.len(), &adj, &mask, &areas);
    assert_eq!(stats.count, 2);
    assert!((stats.max_area_share - 9.0 / 15.0).abs() < 1e-12);
}

#[test]
fn inundation_share_is_continental_ocean_over_continental() {
    let kinds = [
        CrustKind::Continental,
        CrustKind::Continental,
        CrustKind::Oceanic,
    ];
    let land = [
        LandOceanKind::Land,
        LandOceanKind::Ocean,
        LandOceanKind::Ocean,
    ];
    let areas = [2.0, 3.0, 10.0];
    let share = continental_inundation_share(&kinds, &land, &areas);
    assert!((share - 0.6).abs() < 1e-12);
}

#[test]
#[ignore]
fn probe_g0_geography_baseline() {
    let cancellation = BuildCancellation::new();
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
        &cancellation,
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let tectonic_spec = TectonicSpec::default();
    let mut worlds = Vec::with_capacity(PRESETS.len() * 2);
    for preset in PRESETS {
        for seed in [PRIMARY_SEED, CONTRAST_SEED] {
            let started = Instant::now();
            match try_build_primary_relief_for(&bundle, seed, preset, &tectonic_spec) {
                Ok((evolved, substrate, relief)) => {
                    let elapsed_ms = started.elapsed().as_millis();
                    let report = measure_world(
                        preset, seed, elapsed_ms, surface, &evolved, &substrate, &relief,
                    );
                    print_report(&report);
                    worlds.push(GeographyBaselineWorld {
                        preset: format!("{preset:?}"),
                        seed,
                        elapsed_ms,
                        error: None,
                        report: Some(report),
                    });
                }
                Err(error) => {
                    let elapsed_ms = started.elapsed().as_millis();
                    println!(
                        "\n== G0 {preset:?} seed={seed} FAILED after {elapsed_ms} ms ==\n{error}"
                    );
                    worlds.push(GeographyBaselineWorld {
                        preset: format!("{preset:?}"),
                        seed,
                        elapsed_ms,
                        error: Some(error),
                        report: None,
                    });
                }
            }
        }
    }
    let dir = output_dir();
    let path = dir.join("baseline.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&worlds).unwrap()).unwrap();
    println!("wrote {}", path.display());
    assert_eq!(worlds.len(), 6);
    assert!(
        worlds.iter().any(|world| world.report.is_some()),
        "G0 corpus produced no measurable P2+P3 worlds"
    );
}

/// G1e §5 evidence: wet regions not connected to the main ocean after P3,
/// their crust composition, and land connectivity, per preset on the draft
/// corpus at the recommended continental fraction. Run explicitly:
/// `cargo test --release --test geography_baseline_probe probe_g1e_inland_water -- --ignored --nocapture`
#[test]
#[ignore]
fn probe_g1e_inland_water() {
    let cancellation = BuildCancellation::new();
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
        &cancellation,
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let adj = adjacency(surface);
    let areas: Vec<f64> = surface.cells().iter().map(|cell| cell.area.get()).collect();
    let total: f64 = areas.iter().sum();
    for preset in [
        ResolvedWorldFormationPreset::Archipelago,
        ResolvedWorldFormationPreset::Continents,
    ] {
        for seed in [PRIMARY_SEED, CONTRAST_SEED] {
            for plate_count in [12_u16, 22] {
                let spec = TectonicSpec {
                    plate_count,
                    continental_crust_fraction: preset.recommended_continental_crust_fraction(),
                    ..TectonicSpec::default()
                };
                let (_, substrate, relief) =
                    match try_build_primary_relief_for(&bundle, seed, preset, &spec) {
                        Ok(built) => built,
                        Err(error) => {
                            println!(
                            "G1e-water {preset:?} seed={seed} plates={plate_count} FAILED {error}"
                        );
                            continue;
                        }
                    };
                let n = surface.cells().len();
                let kinds: Vec<CrustKind> = (0..n)
                    .map(|index| substrate.crust_kind(index).expect("every cell has crust"))
                    .collect();
                let wet: Vec<bool> = (0..n)
                    .map(|index| {
                        relief.land_ocean().get(index).expect("land/ocean") == LandOceanKind::Ocean
                    })
                    .collect();
                let land: Vec<bool> = wet.iter().map(|value| !value).collect();
                let wet_components = flood_components(&adj, &wet, &areas);
                let land_stats = component_stats(n, &adj, &land, &areas);
                let land_area: f64 = land
                    .iter()
                    .zip(&areas)
                    .filter(|(is_land, _)| **is_land)
                    .map(|(_, area)| area)
                    .sum();
                let mut inland = 0.0;
                let mut inland_continental = 0.0;
                let mut inland_count = 0_usize;
                for component in wet_components.iter().skip(1) {
                    inland_count += 1;
                    for &cell in component {
                        inland += areas[cell];
                        if kinds[cell] == CrustKind::Continental {
                            inland_continental += areas[cell];
                        }
                    }
                }
                let continental_area: f64 = kinds
                    .iter()
                    .zip(&areas)
                    .filter(|(kind, _)| **kind == CrustKind::Continental)
                    .map(|(_, area)| area)
                    .sum();
                let submerged: f64 = (0..n)
                    .filter(|&index| kinds[index] == CrustKind::Continental && wet[index])
                    .map(|index| areas[index])
                    .sum();
                println!(
                    "G1e-water {preset:?} seed={seed} plates={plate_count} land_frac={:.3} land_n={} land_max={:.3} inland_n={inland_count} inland_area_share={:.4} inland_continental_share={:.3} continental_submerged={:.3}",
                    land_area / total,
                    land_stats.count,
                    land_stats.max_area_share,
                    inland / total,
                    if inland > 0.0 { inland_continental / inland } else { 0.0 },
                    submerged / continental_area,
                );
            }
        }
    }
}

/// Connected components of `mask`, largest area first.
fn flood_components(adj: &[Vec<usize>], mask: &[bool], areas: &[f64]) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut components = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        let mut queue = VecDeque::from([start]);
        let mut members = vec![start];
        while let Some(cell) = queue.pop_front() {
            for &neighbor in &adj[cell] {
                if mask[neighbor] && !seen[neighbor] {
                    seen[neighbor] = true;
                    queue.push_back(neighbor);
                    members.push(neighbor);
                }
            }
        }
        components.push(members);
    }
    components.sort_by(|first, second| {
        let a: f64 = first.iter().map(|&cell| areas[cell]).sum();
        let b: f64 = second.iter().map(|&cell| areas[cell]).sum();
        b.total_cmp(&a)
    });
    components
}

fn output_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("g0");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn measure_world(
    preset: ResolvedWorldFormationPreset,
    seed: u64,
    elapsed_ms: u128,
    surface: &SphericalSurfaceSnapshot,
    evolved: &EvolvedTectonicSnapshot,
    substrate: &GeologicSubstrateSnapshot,
    relief: &PrimaryReliefSnapshot,
) -> GeographyBaselineReport {
    let n = surface.cells().len();
    let adj = adjacency(surface);
    let areas: Vec<f64> = surface.cells().iter().map(|cell| cell.area.get()).collect();
    let total_area: f64 = areas.iter().sum();
    let kinds: Vec<CrustKind> = (0..n)
        .map(|index| substrate.crust_kind(index).expect("every cell has crust"))
        .collect();
    let land: Vec<LandOceanKind> = (0..n)
        .map(|index| {
            relief
                .land_ocean()
                .get(index)
                .expect("every cell has land/ocean")
        })
        .collect();
    let plates: Vec<u32> = (0..n)
        .map(|index| {
            evolved
                .compatibility()
                .cell_plates()
                .get(index)
                .expect("every cell has a plate")
                .raw()
        })
        .collect();
    let continental_mask: Vec<bool> = kinds
        .iter()
        .map(|&kind| kind == CrustKind::Continental)
        .collect();
    let land_mask: Vec<bool> = land
        .iter()
        .map(|&kind| kind == LandOceanKind::Land)
        .collect();
    let crust = component_stats(n, &adj, &continental_mask, &areas);
    let land_stats = component_stats(n, &adj, &land_mask, &areas);
    let plate_overlap = plate_overlap_stats(n, &continental_mask, &plates, &areas, &crust);
    let budget = *evolved.material_budget();
    let processes = budget.processes();
    let final_oceanic = budget.final_authoritative().oceanic().reference_area_m2();
    let spreading = processes.oceanic_spreading_created().reference_area_m2();
    let coverage = processes.oceanic_coverage_created().reference_area_m2();
    let quality = evaluate_primary_relief_quality(surface, evolved, substrate, relief).unwrap();
    let old_young = quality
        .metrics()
        .iter()
        .find(|metric| metric.id().name() == "old-young-ocean-depth-separation-m")
        .and_then(|metric| metric.value());

    let mut land_samples = Vec::new();
    let mut ocean_samples = Vec::new();
    let mut ocean_age_samples = Vec::new();
    let sea = relief.sea_level_m();
    let mut continental_area = 0.0;
    let mut oceanic_area = 0.0;
    let mut age_le_horizon_area = 0.0;
    let mut wet_shallow_area = 0.0;
    let mut ocean_area = 0.0;
    let horizon = ResolvedFormationTimeline::sekai_reference().total_duration_myr() as f32;
    for (index, &area) in areas.iter().enumerate() {
        match kinds[index] {
            CrustKind::Continental => continental_area += area,
            CrustKind::Oceanic => {
                oceanic_area += area;
                let age = substrate.ocean_age_myr()[index];
                ocean_age_samples.push((age, area));
                if age <= horizon {
                    age_le_horizon_area += area;
                }
            }
        }
        let elevation = relief.elevation_m()[index];
        let relief_m = elevation - sea;
        match land[index] {
            LandOceanKind::Land => land_samples.push((relief_m, area)),
            LandOceanKind::Ocean => {
                ocean_samples.push((-relief_m, area));
                ocean_area += area;
                if -relief_m < SHELF_CEILING_M {
                    wet_shallow_area += area;
                }
            }
        }
    }
    sort_hypsometric_samples(&mut land_samples);
    sort_hypsometric_samples(&mut ocean_samples);
    sort_hypsometric_samples(&mut ocean_age_samples);

    let (ridge_seeds, boundary_seeds, ridge_edges, boundary_edges) =
        boundary_seeds(surface, evolved);
    let ridge_distance = graph_distance(&adj, &ridge_seeds);
    let boundary_distance = graph_distance(&adj, &boundary_seeds);
    let ridge_adjacent: Vec<(f32, f64)> = (0..n)
        .filter(|&index| kinds[index] == CrustKind::Oceanic && ridge_distance[index] == 0)
        .map(|index| (substrate.ocean_age_myr()[index], areas[index]))
        .collect();
    let mut ridge_adjacent_sorted = ridge_adjacent;
    sort_hypsometric_samples(&mut ridge_adjacent_sorted);

    GeographyBaselineReport {
        preset: format!("{preset:?}"),
        seed,
        elapsed_ms,
        cells: n,
        plates: evolved.compatibility().plates().len(),
        mechanical_fragmentation_count: evolved.lineage_budget().mechanical_fragmentation_count(),
        continental_area_fraction: continental_area / total_area,
        oceanic_area_fraction: oceanic_area / total_area,
        crust_component_count: crust.count,
        crust_max_area_share: crust.max_area_share,
        crust_majority_plate_share: plate_overlap.majority_share,
        crust_mean_plates_per_component: plate_overlap.mean_plates_per_component,
        plates_to_cover_90pct_crust: plate_overlap.plates_to_cover_90pct,
        nearly_continental_plates: plate_overlap.nearly_continental_plates,
        nearly_oceanic_plates: plate_overlap.nearly_oceanic_plates,
        plate_crust_fraction_min: plate_overlap.fraction_min,
        plate_crust_fraction_median: plate_overlap.fraction_median,
        plate_crust_fraction_max: plate_overlap.fraction_max,
        land_component_count: land_stats.count,
        land_max_area_share: land_stats.max_area_share,
        continental_inundation_share: continental_inundation_share(&kinds, &land, &areas),
        physical_land_fraction: f64::from(relief.physical_land_fraction()),
        land_relief_p05_m: hypsometric_quantile(&land_samples, 0.05),
        land_relief_p25_m: hypsometric_quantile(&land_samples, 0.25),
        land_relief_p50_m: hypsometric_quantile(&land_samples, 0.50),
        land_relief_p75_m: hypsometric_quantile(&land_samples, 0.75),
        land_relief_p95_m: hypsometric_quantile(&land_samples, 0.95),
        land_relief_mean_m: hypsometric_mean(&land_samples),
        land_share_below_100m: hypsometric_share_below(&land_samples, T0_LOWLAND_CEILING_M),
        ocean_depth_p50_m: hypsometric_quantile(&ocean_samples, 0.50),
        wet_area_shallower_than_1000m_share: if ocean_area > 0.0 {
            wet_shallow_area / ocean_area
        } else {
            f64::NAN
        },
        ocean_age_p05_myr: hypsometric_quantile(&ocean_age_samples, 0.05),
        ocean_age_p25_myr: hypsometric_quantile(&ocean_age_samples, 0.25),
        ocean_age_p50_myr: hypsometric_quantile(&ocean_age_samples, 0.50),
        ocean_age_p75_myr: hypsometric_quantile(&ocean_age_samples, 0.75),
        ocean_age_p95_myr: hypsometric_quantile(&ocean_age_samples, 0.95),
        ridge_adjacent_age_p50_myr: hypsometric_quantile(&ridge_adjacent_sorted, 0.50),
        age_le_horizon_oceanic_share: if oceanic_area > 0.0 {
            age_le_horizon_area / oceanic_area
        } else {
            f64::NAN
        },
        spreading_created_over_final_oceanic: spreading / final_oceanic,
        coverage_created_over_final_oceanic: coverage / final_oceanic,
        created_over_final_oceanic: (spreading + coverage) / final_oceanic,
        old_young_ocean_depth_separation_m: old_young,
        ridge_edge_count: ridge_edges,
        plate_boundary_edge_count: boundary_edges,
        age_by_ridge_hops: age_by_hops(
            n,
            &kinds,
            substrate.ocean_age_myr(),
            &areas,
            &ridge_distance,
        ),
        age_by_plate_boundary_hops: age_by_hops(
            n,
            &kinds,
            substrate.ocean_age_myr(),
            &areas,
            &boundary_distance,
        ),
    }
}

fn print_report(report: &GeographyBaselineReport) {
    println!(
        "\n== G0 {} seed={} cells={} plates={} fragmentations={} {elapsed} ms ==",
        report.preset,
        report.seed,
        report.cells,
        report.plates,
        report.mechanical_fragmentation_count,
        elapsed = report.elapsed_ms
    );
    println!(
        "crust area={:.4} components={} max_share={:.4} majority_plate={:.4} plates/component={:.2} plates_for_90%={} nearly_full={} nearly_ocean={}",
        report.continental_area_fraction,
        report.crust_component_count,
        report.crust_max_area_share,
        report.crust_majority_plate_share,
        report.crust_mean_plates_per_component,
        report.plates_to_cover_90pct_crust,
        report.nearly_continental_plates,
        report.nearly_oceanic_plates
    );
    println!(
        "plate crust fraction min/med/max={:.3}/{:.3}/{:.3}",
        report.plate_crust_fraction_min,
        report.plate_crust_fraction_median,
        report.plate_crust_fraction_max
    );
    println!(
        "land components={} max_share={:.4} inundation={:.4} physical_land={:.4}",
        report.land_component_count,
        report.land_max_area_share,
        report.continental_inundation_share,
        report.physical_land_fraction
    );
    println!(
        "T0 land p05/p25/p50/p75/p95={:.1}/{:.1}/{:.1}/{:.1}/{:.1} mean={:.1} <100m={:.4} ocean_p50={:.1} shelf<1000m={:.4}",
        report.land_relief_p05_m,
        report.land_relief_p25_m,
        report.land_relief_p50_m,
        report.land_relief_p75_m,
        report.land_relief_p95_m,
        report.land_relief_mean_m,
        report.land_share_below_100m,
        report.ocean_depth_p50_m,
        report.wet_area_shallower_than_1000m_share
    );
    println!(
        "ocean age p05/p25/p50/p75/p95={:.1}/{:.1}/{:.1}/{:.1}/{:.1} ridge-adj p50={:.1} age<=256Myr={:.4}",
        report.ocean_age_p05_myr,
        report.ocean_age_p25_myr,
        report.ocean_age_p50_myr,
        report.ocean_age_p75_myr,
        report.ocean_age_p95_myr,
        report.ridge_adjacent_age_p50_myr,
        report.age_le_horizon_oceanic_share
    );
    println!(
        "created/final oceanic spreading={:.4} coverage={:.4} sum={:.4} old-young depth={:?} ridge_edges={} boundary_edges={}",
        report.spreading_created_over_final_oceanic,
        report.coverage_created_over_final_oceanic,
        report.created_over_final_oceanic,
        report.old_young_ocean_depth_separation_m,
        report.ridge_edge_count,
        report.plate_boundary_edge_count
    );
    println!(
        "age by ridge hops {:?}: {:?}",
        DISTANCE_BINS, report.age_by_ridge_hops
    );
    println!(
        "age by plate-boundary hops {:?}: {:?}",
        DISTANCE_BINS, report.age_by_plate_boundary_hops
    );
}

struct ComponentStats {
    count: usize,
    max_area_share: f64,
    roots: Vec<usize>,
}

fn component_stats(n: usize, adj: &[Vec<usize>], mask: &[bool], areas: &[f64]) -> ComponentStats {
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for (index, neighbors) in adj.iter().enumerate() {
        if !mask[index] {
            continue;
        }
        for &neighbor in neighbors {
            if !mask[neighbor] {
                continue;
            }
            let first = find(&mut parent, index);
            let second = find(&mut parent, neighbor);
            if first == second {
                continue;
            }
            let (keep, child) = if first < second {
                (first, second)
            } else {
                (second, first)
            };
            parent[child] = keep;
        }
    }
    let mut resolved = vec![0; n];
    let mut area_by_root = vec![0.0_f64; n];
    let mut seen = vec![false; n];
    let mut masked_area = 0.0;
    let mut count = 0;
    let mut max_area = 0.0_f64;
    for index in 0..n {
        if !mask[index] {
            continue;
        }
        let root = find(&mut parent, index);
        resolved[index] = root;
        if !seen[root] {
            seen[root] = true;
            count += 1;
        }
        area_by_root[root] += areas[index];
        max_area = max_area.max(area_by_root[root]);
        masked_area += areas[index];
    }
    ComponentStats {
        count,
        max_area_share: if masked_area > 0.0 {
            max_area / masked_area
        } else {
            f64::NAN
        },
        roots: resolved,
    }
}

struct PlateOverlap {
    majority_share: f64,
    mean_plates_per_component: f64,
    plates_to_cover_90pct: usize,
    nearly_continental_plates: usize,
    nearly_oceanic_plates: usize,
    fraction_min: f64,
    fraction_median: f64,
    fraction_max: f64,
}

fn plate_overlap_stats(
    n: usize,
    continental: &[bool],
    plates: &[u32],
    areas: &[f64],
    crust: &ComponentStats,
) -> PlateOverlap {
    let mut plate_total = Vec::new();
    let mut plate_crust = Vec::new();
    let grow = |table: &mut Vec<f64>, plate: u32, area: f64| {
        let index = plate as usize;
        if table.len() <= index {
            table.resize(index + 1, 0.0);
        }
        table[index] += area;
    };
    let mut continental_area = 0.0;
    for index in 0..n {
        grow(&mut plate_total, plates[index], areas[index]);
        if continental[index] {
            grow(&mut plate_crust, plates[index], areas[index]);
            continental_area += areas[index];
        }
    }
    let mut fractions: Vec<f64> = plate_total
        .iter()
        .enumerate()
        .filter(|(_, &total)| total > 0.0)
        .map(|(plate, &total)| plate_crust.get(plate).copied().unwrap_or(0.0) / total)
        .collect();
    fractions.sort_by(f64::total_cmp);
    let nearly_continental_plates = fractions
        .iter()
        .filter(|&&value| value >= NEARLY_CONTINENTAL_PLATE)
        .count();
    let nearly_oceanic_plates = fractions
        .iter()
        .filter(|&&value| value <= NEARLY_OCEANIC_PLATE)
        .count();

    let mut crust_by_plate: Vec<(f64, u32)> = plate_crust
        .iter()
        .enumerate()
        .filter(|(_, &area)| area > 0.0)
        .map(|(plate, &area)| (area, plate as u32))
        .collect();
    crust_by_plate.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut covered = 0.0;
    let mut plates_to_cover_90pct = 0;
    for &(area, _) in &crust_by_plate {
        covered += area;
        plates_to_cover_90pct += 1;
        if covered >= CONTINENTAL_COVERAGE_TARGET * continental_area {
            break;
        }
    }

    let mut majority_area = 0.0;
    let mut plates_weighted = 0.0;
    let unique_roots: BTreeSet<usize> = crust
        .roots
        .iter()
        .enumerate()
        .filter(|(index, _)| continental[*index])
        .map(|(_, &root)| root)
        .collect();
    for root in unique_roots {
        let mut by_plate: BTreeMap<u32, f64> = BTreeMap::new();
        let mut component_area = 0.0;
        for index in 0..n {
            if continental[index] && crust.roots[index] == root {
                *by_plate.entry(plates[index]).or_insert(0.0) += areas[index];
                component_area += areas[index];
            }
        }
        if component_area <= 0.0 {
            continue;
        }
        let majority = by_plate.values().copied().fold(0.0, f64::max);
        majority_area += majority;
        plates_weighted += by_plate.len() as f64 * component_area;
    }

    PlateOverlap {
        majority_share: if continental_area > 0.0 {
            majority_area / continental_area
        } else {
            f64::NAN
        },
        mean_plates_per_component: if continental_area > 0.0 {
            plates_weighted / continental_area
        } else {
            f64::NAN
        },
        plates_to_cover_90pct,
        nearly_continental_plates,
        nearly_oceanic_plates,
        fraction_min: fractions.first().copied().unwrap_or(f64::NAN),
        fraction_median: if fractions.is_empty() {
            f64::NAN
        } else {
            fractions[fractions.len() / 2]
        },
        fraction_max: fractions.last().copied().unwrap_or(f64::NAN),
    }
}

fn continental_inundation_share(kinds: &[CrustKind], land: &[LandOceanKind], areas: &[f64]) -> f64 {
    let mut continental = 0.0;
    let mut inundated = 0.0;
    for index in 0..kinds.len() {
        if kinds[index] != CrustKind::Continental {
            continue;
        }
        continental += areas[index];
        if land[index] == LandOceanKind::Ocean {
            inundated += areas[index];
        }
    }
    if continental > 0.0 {
        inundated / continental
    } else {
        f64::NAN
    }
}

fn adjacency(surface: &SphericalSurfaceSnapshot) -> Vec<Vec<usize>> {
    let mut adj = vec![Vec::new(); surface.cells().len()];
    for edge in surface.edges() {
        let first = edge.cells[0].raw() as usize;
        let second = edge.cells[1].raw() as usize;
        adj[first].push(second);
        adj[second].push(first);
    }
    adj
}

fn boundary_seeds(
    surface: &SphericalSurfaceSnapshot,
    evolved: &EvolvedTectonicSnapshot,
) -> (Vec<usize>, Vec<usize>, usize, usize) {
    let mut ridge = Vec::new();
    let mut boundary = Vec::new();
    let mut ridge_edges = 0;
    let mut boundary_edges = 0;
    let mut ridge_mark = vec![false; surface.cells().len()];
    let mut boundary_mark = vec![false; surface.cells().len()];
    for (edge, record) in surface
        .edges()
        .iter()
        .zip(evolved.compatibility().boundaries())
    {
        if record.kind == BoundaryKind::None {
            continue;
        }
        boundary_edges += 1;
        for cell in edge.cells {
            let index = cell.raw() as usize;
            if !boundary_mark[index] {
                boundary_mark[index] = true;
                boundary.push(index);
            }
        }
        if record.kind == BoundaryKind::OceanicRidge {
            ridge_edges += 1;
            for cell in edge.cells {
                let index = cell.raw() as usize;
                if !ridge_mark[index] {
                    ridge_mark[index] = true;
                    ridge.push(index);
                }
            }
        }
    }
    (ridge, boundary, ridge_edges, boundary_edges)
}

fn graph_distance(adj: &[Vec<usize>], seeds: &[usize]) -> Vec<u32> {
    let mut distance = vec![u32::MAX; adj.len()];
    let mut queue = VecDeque::new();
    for &seed in seeds {
        distance[seed] = 0;
        queue.push_back(seed);
    }
    while let Some(index) = queue.pop_front() {
        let next = distance[index] + 1;
        for &neighbor in &adj[index] {
            if next < distance[neighbor] {
                distance[neighbor] = next;
                queue.push_back(neighbor);
            }
        }
    }
    distance
}

fn age_by_hops(
    n: usize,
    kinds: &[CrustKind],
    ages: &[f32],
    areas: &[f64],
    distance: &[u32],
) -> [f32; 5] {
    let mut out = [f32::NAN; 5];
    for (slot, &hops) in DISTANCE_BINS.iter().enumerate() {
        let mut samples = Vec::new();
        for index in 0..n {
            if kinds[index] == CrustKind::Oceanic && distance[index] == hops {
                samples.push((ages[index], areas[index]));
            }
        }
        sort_hypsometric_samples(&mut samples);
        out[slot] = hypsometric_quantile(&samples, 0.50);
    }
    out
}
