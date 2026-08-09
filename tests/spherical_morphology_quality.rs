use std::collections::VecDeque;
use std::f64::consts::PI;
use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, StageIdentity, StageRng};
use sekai::generators::natural::TectonicGenerator;
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    CrustKind, ResolvedWorldFormation, ResolvedWorldFormationPreset, SphericalTectonicSnapshot,
    TectonicSpec, WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::{central_angle, SphericalSurfaceSnapshot, UnitVector3};
use sekai::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const QUALITY_CELL_COUNT: u32 = 642;
const QUALITY_SEEDS: std::ops::Range<u64> = 0..16;

#[derive(Debug)]
struct PlateMetrics {
    max_min_area_ratio: f64,
    area_cv: f64,
    median_normalized_perimeter: f64,
    plates_below_one_percent: usize,
    aspect_ratios: Vec<f64>,
}

#[derive(Debug)]
struct ContinentalMetrics {
    major_count: usize,
    normalized_coast_perimeter: f64,
    maximum_major_radial_variation: f64,
}

#[derive(Debug)]
struct ContinentalComponent {
    cells: Vec<CellId>,
    area_m2: f64,
    coast_perimeter_m: f64,
}

fn quality_surface() -> &'static SphericalSurfaceSnapshot {
    static SURFACE: OnceLock<SphericalSurfaceSnapshot> = OnceLock::new();
    SURFACE.get_or_init(|| build_surface(QUALITY_CELL_COUNT))
}

fn build_surface(target_cell_count: u32) -> SphericalSurfaceSnapshot {
    GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(EARTH_RADIUS_M).unwrap(),
        target_cell_count,
    })
    .unwrap()
}

fn requested_preset(preset: ResolvedWorldFormationPreset) -> WorldFormationPreset {
    match preset {
        ResolvedWorldFormationPreset::Continents => WorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Archipelago => WorldFormationPreset::Archipelago,
        ResolvedWorldFormationPreset::Supercontinent => WorldFormationPreset::Supercontinent,
        ResolvedWorldFormationPreset::GreatIsland => WorldFormationPreset::GreatIsland,
        ResolvedWorldFormationPreset::VolcanicIslands => WorldFormationPreset::VolcanicIslands,
    }
}

fn preset_fraction(preset: ResolvedWorldFormationPreset) -> f32 {
    match preset {
        ResolvedWorldFormationPreset::Continents => 0.38,
        ResolvedWorldFormationPreset::Archipelago => 0.26,
        ResolvedWorldFormationPreset::Supercontinent => 0.42,
        ResolvedWorldFormationPreset::GreatIsland => 0.28,
        ResolvedWorldFormationPreset::VolcanicIslands => 0.16,
    }
}

fn generate(
    surface: &SphericalSurfaceSnapshot,
    seed: u64,
    preset: ResolvedWorldFormationPreset,
) -> SphericalTectonicSnapshot {
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        requested_preset(preset),
        preset,
    )
    .unwrap();
    let spec = TectonicSpec {
        continental_crust_fraction: preset_fraction(preset),
        ..TectonicSpec::default()
    };
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.spherical-tectonics", 2, "sekai.core"),
    ));
    TectonicGenerator::generate_spherical(surface, &spec, &formation, &mut rng).unwrap()
}

fn plate_metrics(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
) -> PlateMetrics {
    let plate_count = snapshot.plates().len();
    let mut areas = vec![0.0_f64; plate_count];
    let mut perimeters = vec![0.0_f64; plate_count];
    let mut plate_cells = vec![Vec::new(); plate_count];
    for cell in surface.cells() {
        let plate = snapshot.plate_for_cell(cell.id).unwrap().raw() as usize;
        areas[plate] += cell.area.get();
        plate_cells[plate].push(cell.id);
    }
    for edge in surface.edges() {
        let owners = edge
            .cells
            .map(|cell| snapshot.plate_for_cell(cell).unwrap());
        if owners[0] != owners[1] {
            perimeters[owners[0].raw() as usize] += edge.length.get();
            perimeters[owners[1].raw() as usize] += edge.length.get();
        }
    }
    let total_area = surface.total_cell_area().get();
    let mean = total_area / plate_count as f64;
    let variance = areas.iter().map(|area| (area - mean).powi(2)).sum::<f64>() / plate_count as f64;
    let mut normalized_perimeters = areas
        .iter()
        .zip(&perimeters)
        .map(|(&area, &perimeter)| {
            perimeter / equal_area_spherical_circle_perimeter(surface.radius().get(), area)
        })
        .collect::<Vec<_>>();
    normalized_perimeters.sort_by(f64::total_cmp);
    PlateMetrics {
        max_min_area_ratio: areas.iter().copied().fold(0.0, f64::max)
            / areas.iter().copied().fold(f64::INFINITY, f64::min),
        area_cv: variance.sqrt() / mean,
        median_normalized_perimeter: median(&normalized_perimeters),
        plates_below_one_percent: areas
            .iter()
            .filter(|&&area| area / total_area < 0.01)
            .count(),
        aspect_ratios: plate_cells
            .iter()
            .map(|cells| tangent_covariance_aspect_ratio(surface, cells))
            .collect(),
    }
}

fn equal_area_spherical_circle_perimeter(radius_m: f64, area_m2: f64) -> f64 {
    let cosine = (1.0 - area_m2 / (2.0 * PI * radius_m.powi(2))).clamp(-1.0, 1.0);
    2.0 * PI * radius_m * cosine.acos().sin()
}

fn normalize(vector: [f64; 3]) -> [f64; 3] {
    let length = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    vector.map(|value| value / length)
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn area_centroid(surface: &SphericalSurfaceSnapshot, cells: &[CellId]) -> UnitVector3 {
    let sum = cells.iter().fold([0.0; 3], |mut sum, &cell| {
        let record = surface.cell(cell).unwrap();
        for (target, component) in sum.iter_mut().zip(record.centroid.components()) {
            *target += record.area.get() * component;
        }
        sum
    });
    UnitVector3::new(sum[0], sum[1], sum[2]).unwrap()
}

fn tangent_basis(radial: UnitVector3) -> ([f64; 3], [f64; 3]) {
    let radial = radial.components();
    let reference = if radial[2].abs() < 0.8 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let east = normalize(cross(reference, radial));
    let north = normalize(cross(radial, east));
    (east, north)
}

fn tangent_covariance_aspect_ratio(surface: &SphericalSurfaceSnapshot, cells: &[CellId]) -> f64 {
    let center = area_centroid(surface, cells);
    let center_components = center.components();
    let (east, north) = tangent_basis(center);
    let mut samples = Vec::with_capacity(cells.len());
    let mut total_weight = 0.0;
    for &cell in cells {
        let record = surface.cell(cell).unwrap();
        let radial = record.centroid.components();
        let cosine = dot(center_components, radial).clamp(-1.0, 1.0);
        let angle = cosine.acos();
        let tangent = [
            radial[0] - center_components[0] * cosine,
            radial[1] - center_components[1] * cosine,
            radial[2] - center_components[2] * cosine,
        ];
        let tangent_length = dot(tangent, tangent).sqrt();
        let coordinates = if tangent_length <= f64::EPSILON {
            [0.0, 0.0]
        } else {
            let scale = angle * surface.radius().get() / tangent_length;
            [dot(tangent, east) * scale, dot(tangent, north) * scale]
        };
        samples.push((coordinates, record.area.get()));
        total_weight += record.area.get();
    }
    let mean = samples.iter().fold([0.0; 2], |mut sum, (point, weight)| {
        sum[0] += point[0] * weight;
        sum[1] += point[1] * weight;
        sum
    });
    let mean = [mean[0] / total_weight, mean[1] / total_weight];
    let [xx, xy, yy] = samples.iter().fold([0.0; 3], |mut sum, (point, weight)| {
        let x = point[0] - mean[0];
        let y = point[1] - mean[1];
        sum[0] += x * x * weight;
        sum[1] += x * y * weight;
        sum[2] += y * y * weight;
        sum
    });
    let xx = xx / total_weight;
    let xy = xy / total_weight;
    let yy = yy / total_weight;
    let trace = xx + yy;
    let discriminant = ((xx - yy).powi(2) + 4.0 * xy.powi(2)).sqrt();
    let maximum = ((trace + discriminant) * 0.5).max(0.0);
    let minimum = ((trace - discriminant) * 0.5).max(f64::EPSILON);
    (maximum / minimum).sqrt()
}

fn continental_components(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
) -> Vec<ContinentalComponent> {
    let mut visited = vec![false; surface.cells().len()];
    let mut components = Vec::new();
    for cell in surface.cells() {
        let start = cell.id.raw() as usize;
        if visited[start] || snapshot.crust_kind(cell.id) != Some(CrustKind::Continental) {
            continue;
        }
        visited[start] = true;
        let mut queue = VecDeque::from([cell.id]);
        let mut cells = Vec::new();
        let mut area_m2 = 0.0;
        let mut coast_perimeter_m = 0.0;
        while let Some(current) = queue.pop_front() {
            cells.push(current);
            area_m2 += surface.cell(current).unwrap().area.get();
            for &edge_id in surface.cell_edges(current).unwrap() {
                let edge = surface.edge(edge_id).unwrap();
                let neighbor = surface.opposite_cell(current, edge_id).unwrap();
                if snapshot.crust_kind(neighbor) == Some(CrustKind::Continental) {
                    let index = neighbor.raw() as usize;
                    if !visited[index] {
                        visited[index] = true;
                        queue.push_back(neighbor);
                    }
                } else {
                    coast_perimeter_m += edge.length.get();
                }
            }
        }
        components.push(ContinentalComponent {
            cells,
            area_m2,
            coast_perimeter_m,
        });
    }
    components
}

fn boundary_radial_variation(
    surface: &SphericalSurfaceSnapshot,
    component: &ContinentalComponent,
    snapshot: &SphericalTectonicSnapshot,
) -> f64 {
    let center = area_centroid(surface, &component.cells);
    let mut distances = Vec::new();
    for &cell in &component.cells {
        let record = surface.cell(cell).unwrap();
        if record.boundary_edges.iter().any(|&edge| {
            let neighbor = surface.opposite_cell(cell, edge).unwrap();
            snapshot.crust_kind(neighbor) == Some(CrustKind::Oceanic)
        }) {
            distances.push(central_angle(center, record.centroid) * surface.radius().get());
        }
    }
    let mean = distances.iter().sum::<f64>() / distances.len() as f64;
    let variance = distances
        .iter()
        .map(|distance| (distance - mean).powi(2))
        .sum::<f64>()
        / distances.len() as f64;
    variance.sqrt() / mean
}

fn continental_metrics(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
) -> ContinentalMetrics {
    let components = continental_components(surface, snapshot);
    let total_area = components
        .iter()
        .map(|component| component.area_m2)
        .sum::<f64>();
    let major = components
        .iter()
        .filter(|component| component.area_m2 >= total_area * 0.10)
        .collect::<Vec<_>>();
    let actual_perimeter = major
        .iter()
        .map(|component| component.coast_perimeter_m)
        .sum::<f64>();
    let circular_perimeter = major
        .iter()
        .map(|component| {
            equal_area_spherical_circle_perimeter(surface.radius().get(), component.area_m2)
        })
        .sum::<f64>();
    ContinentalMetrics {
        major_count: major.len(),
        normalized_coast_perimeter: actual_perimeter / circular_perimeter,
        maximum_major_radial_variation: major
            .iter()
            .map(|component| boundary_radial_variation(surface, component, snapshot))
            .fold(0.0, f64::max),
    }
}

fn median_cell_angular_diameter(surface: &SphericalSurfaceSnapshot) -> f64 {
    let radius_squared = surface.radius().get().powi(2);
    let mut diameters = surface
        .cells()
        .iter()
        .map(|cell| {
            let unit_area = cell.area.get() / radius_squared;
            2.0 * (1.0 - unit_area / (2.0 * PI)).clamp(-1.0, 1.0).acos()
        })
        .collect::<Vec<_>>();
    diameters.sort_by(f64::total_cmp);
    median(&diameters)
}

#[derive(Debug, Clone, Copy)]
struct CoastPlateMetrics {
    direct_overlap: f64,
    buffered_overlap: f64,
    surface_buffer_coverage: f64,
}

fn coast_plate_metrics(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
) -> CoastPlateMetrics {
    let plate_boundaries = surface
        .edges()
        .iter()
        .filter(|edge| {
            snapshot.plate_for_cell(edge.cells[0]) != snapshot.plate_for_cell(edge.cells[1])
        })
        .map(|edge| edge.midpoint)
        .collect::<Vec<_>>();
    let buffer = median_cell_angular_diameter(surface);
    let mut coast_length = 0.0;
    let mut direct_overlap_length = 0.0;
    let mut buffered_overlap_length = 0.0;
    let mut surface_length = 0.0;
    let mut buffered_surface_length = 0.0;
    for edge in surface.edges() {
        surface_length += edge.length.get();
        let is_plate_boundary =
            snapshot.plate_for_cell(edge.cells[0]) != snapshot.plate_for_cell(edge.cells[1]);
        let is_buffered = plate_boundaries
            .iter()
            .any(|&midpoint| central_angle(edge.midpoint, midpoint) <= buffer);
        if is_buffered {
            buffered_surface_length += edge.length.get();
        }
        if snapshot.crust_kind(edge.cells[0]) == snapshot.crust_kind(edge.cells[1]) {
            continue;
        }
        coast_length += edge.length.get();
        if is_plate_boundary {
            direct_overlap_length += edge.length.get();
        }
        if is_buffered {
            buffered_overlap_length += edge.length.get();
        }
    }
    CoastPlateMetrics {
        direct_overlap: direct_overlap_length / coast_length,
        buffered_overlap: buffered_overlap_length / coast_length,
        surface_buffer_coverage: buffered_surface_length / surface_length,
    }
}

fn median(values: &[f64]) -> f64 {
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

#[test]
fn default_plate_morphology_is_varied_without_fragmenting() {
    let surface = quality_surface();
    let mut failures = Vec::new();
    let mut elongated_total = 0_usize;
    let mut plate_total = 0_usize;
    let mut area_cv_total = 0.0_f64;
    for seed in QUALITY_SEEDS {
        let snapshot = generate(surface, seed, ResolvedWorldFormationPreset::Continents);
        let metrics = plate_metrics(surface, &snapshot);
        let elongated = metrics
            .aspect_ratios
            .iter()
            .filter(|&&ratio| ratio > 1.25)
            .count();
        let valid = (2.5..=8.0).contains(&metrics.max_min_area_ratio)
            && (0.27..=0.75).contains(&metrics.area_cv)
            && (1.15..=2.60).contains(&metrics.median_normalized_perimeter)
            && metrics.plates_below_one_percent == 0
            && elongated * 4 >= snapshot.plates().len();
        elongated_total += elongated;
        plate_total += snapshot.plates().len();
        area_cv_total += metrics.area_cv;
        eprintln!("plate seed {seed}: elongated={elongated} {metrics:?}");
        if !valid {
            failures.push(format!("seed {seed}: elongated={elongated} {metrics:?}"));
        }
    }
    if elongated_total * 2 < plate_total {
        failures.push(format!(
            "only {elongated_total}/{plate_total} plates are elongated across the seed matrix"
        ));
    }
    let mean_area_cv = area_cv_total / (QUALITY_SEEDS.end - QUALITY_SEEDS.start) as f64;
    if mean_area_cv < 0.30 {
        failures.push(format!(
            "mean plate area coefficient of variation is only {mean_area_cv}"
        ));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn formation_presets_have_distinct_non_round_continental_morphology() {
    let surface = quality_surface();
    let mut failures = Vec::new();
    let cases = [
        (ResolvedWorldFormationPreset::Continents, 3..=5),
        (ResolvedWorldFormationPreset::Supercontinent, 1..=1),
        (ResolvedWorldFormationPreset::Archipelago, 2..=6),
        (ResolvedWorldFormationPreset::GreatIsland, 1..=1),
        (ResolvedWorldFormationPreset::VolcanicIslands, 0..=2),
    ];
    for seed in QUALITY_SEEDS {
        for (preset, expected_major) in &cases {
            let snapshot = generate(surface, seed, *preset);
            let metrics = continental_metrics(surface, &snapshot);
            let perimeter_required = matches!(
                preset,
                ResolvedWorldFormationPreset::Continents
                    | ResolvedWorldFormationPreset::Supercontinent
            );
            let valid = expected_major.contains(&metrics.major_count)
                && (!perimeter_required
                    || (1.35..=3.75).contains(&metrics.normalized_coast_perimeter))
                && (*preset != ResolvedWorldFormationPreset::Continents
                    || metrics.maximum_major_radial_variation > 0.18);
            eprintln!("continent seed {seed}, {preset:?}: {metrics:?}");
            if !valid {
                failures.push(format!("seed {seed}, {preset:?}: {metrics:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn default_coasts_are_related_to_but_not_locked_to_plate_boundaries() {
    let surface = quality_surface();
    let mut failures = Vec::new();
    for seed in QUALITY_SEEDS {
        let snapshot = generate(surface, seed, ResolvedWorldFormationPreset::Continents);
        let metrics = coast_plate_metrics(surface, &snapshot);
        eprintln!("coast seed {seed}: {metrics:?}");
        if !(0.01..=0.35).contains(&metrics.direct_overlap)
            || metrics.buffered_overlap > metrics.surface_buffer_coverage + 0.12
        {
            failures.push(format!("seed {seed}: {metrics:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn nearest_fine_cells(
    coarse: &SphericalSurfaceSnapshot,
    fine: &SphericalSurfaceSnapshot,
) -> Vec<CellId> {
    coarse
        .cells()
        .iter()
        .map(|coarse_cell| {
            fine.cells()
                .iter()
                .max_by(|first, second| {
                    dot(
                        coarse_cell.centroid.components(),
                        first.centroid.components(),
                    )
                    .total_cmp(&dot(
                        coarse_cell.centroid.components(),
                        second.centroid.components(),
                    ))
                })
                .unwrap()
                .id
        })
        .collect()
}

fn optimally_matched_owner_agreement(
    coarse_surface: &SphericalSurfaceSnapshot,
    coarse: &SphericalTectonicSnapshot,
    fine: &SphericalTectonicSnapshot,
    nearest_fine: &[CellId],
) -> f64 {
    let plate_count = coarse.plates().len();
    let mut weights = vec![vec![0.0_f64; plate_count]; plate_count];
    for (cell, &fine_cell) in coarse_surface.cells().iter().zip(nearest_fine) {
        let coarse_plate = coarse.plate_for_cell(cell.id).unwrap().raw() as usize;
        let fine_plate = fine.plate_for_cell(fine_cell).unwrap().raw() as usize;
        weights[coarse_plate][fine_plate] += cell.area.get();
    }
    let state_count = 1_usize << plate_count;
    let mut best = vec![f64::NEG_INFINITY; state_count];
    best[0] = 0.0;
    for mask in 0..state_count {
        let row = mask.count_ones() as usize;
        if row >= plate_count || !best[mask].is_finite() {
            continue;
        }
        for (column, &weight) in weights[row].iter().enumerate() {
            let bit = 1_usize << column;
            if mask & bit == 0 {
                best[mask | bit] = best[mask | bit].max(best[mask] + weight);
            }
        }
    }
    best[state_count - 1] / coarse_surface.total_cell_area().get()
}

fn continental_mask_jaccard(
    coarse_surface: &SphericalSurfaceSnapshot,
    coarse: &SphericalTectonicSnapshot,
    fine: &SphericalTectonicSnapshot,
    nearest_fine: &[CellId],
) -> f64 {
    let mut intersection = 0.0;
    let mut union = 0.0;
    for (cell, &fine_cell) in coarse_surface.cells().iter().zip(nearest_fine) {
        let coarse_land = coarse.crust_kind(cell.id) == Some(CrustKind::Continental);
        let fine_land = fine.crust_kind(fine_cell) == Some(CrustKind::Continental);
        if coarse_land || fine_land {
            union += cell.area.get();
            if coarse_land && fine_land {
                intersection += cell.area.get();
            }
        }
    }
    intersection / union
}

#[test]
#[ignore = "release-only 5k/20k morphology resolution gate"]
fn field_morphology_is_resolution_invariant() {
    let coarse_surface = build_surface(5_000);
    let fine_surface = build_surface(20_000);
    let coarse = generate(
        &coarse_surface,
        42,
        ResolvedWorldFormationPreset::Continents,
    );
    let fine = generate(&fine_surface, 42, ResolvedWorldFormationPreset::Continents);
    let nearest_fine = nearest_fine_cells(&coarse_surface, &fine_surface);
    let coarse_perimeter = plate_metrics(&coarse_surface, &coarse).median_normalized_perimeter;
    let fine_perimeter = plate_metrics(&fine_surface, &fine).median_normalized_perimeter;
    let perimeter_difference =
        (coarse_perimeter - fine_perimeter).abs() / coarse_perimeter.max(fine_perimeter);
    let fine_scale_perimeter_gain = (fine_perimeter - coarse_perimeter) / coarse_perimeter;
    let owner_agreement =
        optimally_matched_owner_agreement(&coarse_surface, &coarse, &fine, &nearest_fine);
    let crust_jaccard = continental_mask_jaccard(&coarse_surface, &coarse, &fine, &nearest_fine);
    let coarse_major = continental_metrics(&coarse_surface, &coarse).major_count;
    let fine_major = continental_metrics(&fine_surface, &fine).major_count;

    eprintln!(
        "resolution morphology: coarse_cells={} fine_cells={} coarse_perimeter={coarse_perimeter:.4} fine_perimeter={fine_perimeter:.4} fine_scale_gain={fine_scale_perimeter_gain:.4} owner_agreement={owner_agreement:.4} crust_jaccard={crust_jaccard:.4} major={coarse_major}/{fine_major}",
        coarse_surface.cells().len(),
        fine_surface.cells().len(),
    );
    assert!(perimeter_difference <= 0.15);
    assert!(
        (0.04..=0.15).contains(&fine_scale_perimeter_gain),
        "the 20k partition must reveal bounded field detail without becoming resolution-defined"
    );
    assert!(owner_agreement >= 0.90);
    assert!(crust_jaccard >= 0.65);
    assert_eq!(coarse_major, fine_major);
}
