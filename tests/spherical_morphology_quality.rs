use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::f64::consts::PI;
use std::sync::OnceLock;

use sekai::engine::{derive_stage_seed, Diagnostic, StageIdentity, StageRng};
use sekai::generators::natural::{MantleGenerator, ReliefGenerator, TectonicGenerator};
use sekai::generators::spatial::GeodesicVoronoiBuilder;
use sekai::world::natural::{
    CrustKind, GeologicSpec, LandOceanKind, ReliefSpec, ResolvedWorldFormation,
    ResolvedWorldFormationPreset, SphericalReliefSnapshot, SphericalTectonicSnapshot, TectonicSpec,
    WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
};
use sekai::world::spatial::{
    central_angle, project_tangent, SphericalSurfaceSnapshot, UnitVector3,
};
use sekai::world::{
    CellId, EdgeId, Meters, PlateId, RootSeed, SphericalSpaceSpec, SurfaceVertexId,
};

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const QUALITY_CELL_COUNT: u32 = 642;
const QUALITY_SEEDS: [u64; 17] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 42];

#[derive(Debug)]
struct PlateMetrics {
    median_normalized_perimeter: f64,
    area_fractions: Vec<f64>,
    normalized_perimeters: Vec<f64>,
    aspect_ratios: Vec<f64>,
}

#[derive(Debug)]
struct ContinentalComponent {
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
    generate_with_continental_fraction(surface, seed, preset, preset_fraction(preset))
}

fn generate_with_continental_fraction(
    surface: &SphericalSurfaceSnapshot,
    seed: u64,
    preset: ResolvedWorldFormationPreset,
    continental_crust_fraction: f32,
) -> SphericalTectonicSnapshot {
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        requested_preset(preset),
        preset,
    )
    .unwrap();
    let spec = TectonicSpec {
        continental_crust_fraction,
        ..TectonicSpec::default()
    };
    let mut rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.spherical-tectonics", 3, "sekai.core"),
    ));
    TectonicGenerator::generate_spherical(surface, &spec, &formation, &mut rng).unwrap_or_else(
        |error| {
            panic!(
                "tectonic generation failed: seed={seed}, preset={preset:?}, continental={continental_crust_fraction}: {error:?}"
            )
        },
    )
}

fn generate_relief(
    surface: &SphericalSurfaceSnapshot,
    seed: u64,
    preset: ResolvedWorldFormationPreset,
    tectonic: &SphericalTectonicSnapshot,
) -> SphericalReliefSnapshot {
    generate_relief_with_target(
        surface,
        seed,
        preset,
        tectonic,
        ReliefSpec::default().target_land_fraction,
    )
}

fn generate_relief_with_target(
    surface: &SphericalSurfaceSnapshot,
    seed: u64,
    preset: ResolvedWorldFormationPreset,
    tectonic: &SphericalTectonicSnapshot,
    target_land_fraction: f32,
) -> SphericalReliefSnapshot {
    let formation = ResolvedWorldFormation::new(
        RESOLVED_WORLD_FORMATION_SCHEMA_V1,
        requested_preset(preset),
        preset,
    )
    .unwrap();
    let mut mantle_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.spherical-mantle", 1, "sekai.core"),
    ));
    let mantle = MantleGenerator::generate_spherical(
        surface,
        &GeologicSpec::default(),
        formation.mantle_bias(),
        &mut mantle_rng,
    )
    .unwrap();
    let mut relief_rng = StageRng::from_seed(derive_stage_seed(
        RootSeed::new(seed),
        StageIdentity::new("natural.spherical-relief", 3, "sekai.core"),
    ));
    ReliefGenerator::generate_spherical(
        surface,
        tectonic,
        &mantle,
        &ReliefSpec {
            target_land_fraction,
            ..ReliefSpec::default()
        },
        &mut relief_rng,
        &mut Vec::<Diagnostic>::new(),
    )
    .unwrap()
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
    let normalized_perimeters = areas
        .iter()
        .zip(&perimeters)
        .map(|(&area, &perimeter)| {
            perimeter / equal_area_spherical_circle_perimeter(surface.radius().get(), area)
        })
        .collect::<Vec<_>>();
    let mut sorted_normalized_perimeters = normalized_perimeters.clone();
    sorted_normalized_perimeters.sort_by(f64::total_cmp);
    let area_fractions = areas.iter().map(|area| area / total_area).collect();
    PlateMetrics {
        median_normalized_perimeter: median(&sorted_normalized_perimeters),
        area_fractions,
        normalized_perimeters,
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
    buffered_overlap: f64,
}

fn coast_plate_metrics(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
    relief: &SphericalReliefSnapshot,
) -> CoastPlateMetrics {
    let plate_boundaries = surface
        .edges()
        .iter()
        .filter(|edge| {
            snapshot.plate_for_cell(edge.cells[0]) != snapshot.plate_for_cell(edge.cells[1])
        })
        .map(|edge| edge.midpoint)
        .collect::<Vec<_>>();
    // A one-cell-diameter band extends half a diameter to either side of the
    // plate boundary. Treating the diameter as the per-side radius would
    // silently measure a two-cell-diameter band.
    let buffer_radius = median_cell_angular_diameter(surface) * 0.5;
    let mut coast_length = 0.0;
    let mut buffered_overlap_length = 0.0;
    for edge in surface.edges() {
        let is_buffered = plate_boundaries
            .iter()
            .any(|&midpoint| central_angle(edge.midpoint, midpoint) <= buffer_radius);
        if relief.land_ocean_kind(edge.cells[0]) == relief.land_ocean_kind(edge.cells[1]) {
            continue;
        }
        coast_length += edge.length.get();
        if is_buffered {
            buffered_overlap_length += edge.length.get();
        }
    }
    CoastPlateMetrics {
        buffered_overlap: buffered_overlap_length / coast_length,
    }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

fn plate_pair(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
    edge_id: EdgeId,
) -> Option<[PlateId; 2]> {
    let edge = surface.edge(edge_id).unwrap();
    let owners = edge
        .cells
        .map(|cell| snapshot.plate_for_cell(cell).unwrap());
    if owners[0] == owners[1] {
        None
    } else if owners[0] < owners[1] {
        Some(owners)
    } else {
        Some([owners[1], owners[0]])
    }
}

fn macro_boundary_tortuosities(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
    minimum_length_m: f64,
) -> Vec<f64> {
    let mut by_pair = BTreeMap::<[PlateId; 2], Vec<EdgeId>>::new();
    for edge in surface.edges() {
        if let Some(pair) = plate_pair(surface, snapshot, edge.id) {
            by_pair.entry(pair).or_default().push(edge.id);
        }
    }

    let mut tortuosities = Vec::new();
    for edges in by_pair.into_values() {
        let mut incident = BTreeMap::<SurfaceVertexId, Vec<EdgeId>>::new();
        for &edge_id in &edges {
            for vertex in surface.edge(edge_id).unwrap().vertices {
                incident.entry(vertex).or_default().push(edge_id);
            }
        }
        let mut unseen = edges.into_iter().collect::<BTreeSet<_>>();
        while let Some(seed) = unseen.iter().next().copied() {
            let mut queue = VecDeque::from([seed]);
            let mut component = Vec::new();
            unseen.remove(&seed);
            while let Some(edge_id) = queue.pop_front() {
                component.push(edge_id);
                for vertex in surface.edge(edge_id).unwrap().vertices {
                    for &neighbor in &incident[&vertex] {
                        if unseen.remove(&neighbor) {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
            let path_length_m = component
                .iter()
                .map(|&edge_id| surface.edge(edge_id).unwrap().length.get())
                .sum::<f64>();
            if path_length_m < minimum_length_m {
                continue;
            }
            let mut degree = BTreeMap::<SurfaceVertexId, usize>::new();
            for &edge_id in &component {
                for vertex in surface.edge(edge_id).unwrap().vertices {
                    *degree.entry(vertex).or_default() += 1;
                }
            }
            let endpoints = degree
                .into_iter()
                .filter_map(|(vertex, count)| (count == 1).then_some(vertex))
                .collect::<Vec<_>>();
            if endpoints.len() != 2 {
                continue;
            }
            let chord_m = central_angle(
                surface.vertex(endpoints[0]).unwrap().position,
                surface.vertex(endpoints[1]).unwrap().position,
            ) * surface.radius().get();
            if chord_m > f64::EPSILON {
                tortuosities.push(path_length_m / chord_m);
            }
        }
    }
    tortuosities
}

fn trace_boundary_branch(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
    incident: &[Vec<EdgeId>],
    start: SurfaceVertexId,
    first_edge: EdgeId,
    target_length_m: f64,
) -> SurfaceVertexId {
    let pair = plate_pair(surface, snapshot, first_edge).unwrap();
    let mut previous_vertex = start;
    let mut edge_id = first_edge;
    let mut length_m = 0.0;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(edge_id) {
            return previous_vertex;
        }
        let edge = surface.edge(edge_id).unwrap();
        let next_vertex = if edge.vertices[0] == previous_vertex {
            edge.vertices[1]
        } else {
            edge.vertices[0]
        };
        length_m += edge.length.get();
        if length_m >= target_length_m {
            return next_vertex;
        }
        let candidates = incident[next_vertex.raw() as usize]
            .iter()
            .copied()
            .filter(|&candidate| candidate != edge_id)
            .filter(|&candidate| plate_pair(surface, snapshot, candidate) == Some(pair))
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return next_vertex;
        }
        previous_vertex = next_vertex;
        edge_id = candidates[0];
    }
}

fn macro_triple_junction_angles_deg(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
    trace_length_m: f64,
) -> Vec<f64> {
    let mut incident = vec![Vec::new(); surface.vertices().len()];
    for edge in surface.edges() {
        if plate_pair(surface, snapshot, edge.id).is_some() {
            for vertex in edge.vertices {
                incident[vertex.raw() as usize].push(edge.id);
            }
        }
    }

    let mut angles = Vec::new();
    for vertex in surface.vertices() {
        let edges = &incident[vertex.id.raw() as usize];
        let owners = edges
            .iter()
            .flat_map(|&edge| plate_pair(surface, snapshot, edge).unwrap())
            .collect::<BTreeSet<_>>();
        if owners.len() != 3 || edges.len() != 3 {
            continue;
        }
        let radial = vertex.position;
        let (east, north) = tangent_basis(radial);
        let mut azimuths = edges
            .iter()
            .filter_map(|&edge| {
                let endpoint = trace_boundary_branch(
                    surface,
                    snapshot,
                    &incident,
                    vertex.id,
                    edge,
                    trace_length_m,
                );
                let tangent = project_tangent(
                    surface.vertex(endpoint).unwrap().position.components(),
                    radial,
                );
                let length = dot(tangent, tangent).sqrt();
                (length > f64::EPSILON).then(|| {
                    let direction = tangent.map(|component| component / length);
                    dot(direction, north).atan2(dot(direction, east))
                })
            })
            .collect::<Vec<_>>();
        if azimuths.len() != 3 {
            continue;
        }
        azimuths.sort_by(f64::total_cmp);
        for index in 0..3 {
            let next = if index == 2 {
                azimuths[0] + 2.0 * PI
            } else {
                azimuths[index + 1]
            };
            angles.push((next - azimuths[index]).to_degrees());
        }
    }
    angles
}

#[test]
fn multi_seed_macro_boundaries_reject_voronoi_honeycombs() {
    let surface = quality_surface();
    let mut non_micro_perimeters = Vec::new();
    let mut non_micro_aspects = Vec::new();
    let mut tortuosities = Vec::new();
    let mut triple_angles = Vec::new();
    let mut coast_overlaps = Vec::new();
    for seed in QUALITY_SEEDS {
        let snapshot = generate(surface, seed, ResolvedWorldFormationPreset::Continents);
        let relief = generate_relief(
            surface,
            seed,
            ResolvedWorldFormationPreset::Continents,
            &snapshot,
        );
        let metrics = plate_metrics(surface, &snapshot);
        for index in 0..snapshot.plates().len() {
            if metrics.area_fractions[index] >= 0.01 {
                non_micro_perimeters.push(metrics.normalized_perimeters[index]);
                non_micro_aspects.push(metrics.aspect_ratios[index]);
            }
        }
        tortuosities.extend(macro_boundary_tortuosities(surface, &snapshot, 750_000.0));
        triple_angles.extend(macro_triple_junction_angles_deg(
            surface, &snapshot, 750_000.0,
        ));
        coast_overlaps.push(coast_plate_metrics(surface, &snapshot, &relief).buffered_overlap);
        eprintln!("plate seed {seed}: {metrics:?}");
    }

    non_micro_perimeters.sort_by(f64::total_cmp);
    non_micro_aspects.sort_by(f64::total_cmp);
    tortuosities.sort_by(f64::total_cmp);
    triple_angles.sort_by(f64::total_cmp);
    let mut sorted_coast_overlaps = coast_overlaps.clone();
    sorted_coast_overlaps.sort_by(f64::total_cmp);
    let elongated_fraction = non_micro_aspects
        .iter()
        .filter(|&&ratio| ratio > 1.25)
        .count() as f64
        / non_micro_aspects.len() as f64;
    let straight_arc_fraction = tortuosities.iter().filter(|&&ratio| ratio <= 1.02).count() as f64
        / tortuosities.len() as f64;
    let regular_triple_fraction = triple_angles
        .iter()
        .filter(|&&angle| (angle - 120.0).abs() <= 10.0)
        .count() as f64
        / triple_angles.len() as f64;
    eprintln!(
        "anti-voronoi aggregate: plates={} perimeter_median={:.4} aspect_median={:.4} elongated={elongated_fraction:.4} arcs={} tortuosity_median={:.4} straight={straight_arc_fraction:.4} triple_angles={} triple_median={:.4} regular120={regular_triple_fraction:.4}",
        non_micro_aspects.len(),
        median(&non_micro_perimeters),
        median(&non_micro_aspects),
        tortuosities.len(),
        median(&tortuosities),
        triple_angles.len(),
        median(&triple_angles),
    );
    let coast_in_band_fraction = coast_overlaps
        .iter()
        .filter(|&&overlap| (0.10..=0.55).contains(&overlap))
        .count() as f64
        / coast_overlaps.len() as f64;
    eprintln!(
        "coast/plate one-cell overlap={coast_overlaps:?} median={:.4} in_band={coast_in_band_fraction:.4}",
        median(&sorted_coast_overlaps),
    );

    assert!(!non_micro_aspects.is_empty());
    assert!(!tortuosities.is_empty());
    assert!(!triple_angles.is_empty());
    assert!(median(&non_micro_perimeters) >= 1.25);
    assert!(elongated_fraction >= 0.50);
    assert!(straight_arc_fraction <= 0.65);
    assert!(regular_triple_fraction <= 0.65);
    // This is a multi-seed statistical property, not a per-world coastline
    // quota: active-margin-dominated and passive-margin-dominated worlds are
    // both legitimate. Freeze the central tendency, require at least 13/17
    // seeds in the nontrivial/non-dominant band, and reject any dominant
    // plate-outline coastline outlier.
    assert!((0.10..=0.55).contains(&median(&sorted_coast_overlaps)));
    assert!(coast_in_band_fraction >= 0.75);
    assert!(coast_overlaps.iter().all(|overlap| *overlap <= 0.65));
}

#[test]
fn formation_presets_preserve_statistical_intent_without_fixed_final_topology() {
    let surface = quality_surface();
    let presets = [
        ResolvedWorldFormationPreset::Continents,
        ResolvedWorldFormationPreset::Supercontinent,
        ResolvedWorldFormationPreset::Archipelago,
        ResolvedWorldFormationPreset::GreatIsland,
        ResolvedWorldFormationPreset::VolcanicIslands,
    ];
    let mut continental_fractions = BTreeMap::new();
    let mut land_fraction_errors = BTreeMap::new();
    let mut final_counts = BTreeMap::new();
    for preset in presets {
        continental_fractions.insert(preset, Vec::new());
        land_fraction_errors.insert(preset, Vec::new());
        final_counts.insert(preset, BTreeSet::new());
    }
    for seed in QUALITY_SEEDS {
        for preset in presets {
            let snapshot = generate(surface, seed, preset);
            let target = preset.recommended_land_fraction();
            let relief = generate_relief_with_target(surface, seed, preset, &snapshot, target);
            let actual = land_area_fraction(surface, &relief);
            let error = (actual - f64::from(target)).abs();
            let cutoff_plateau = cutoff_plateau_area_fraction(surface, &relief);
            assert!(
                error <= cutoff_plateau + 1.0e-12,
                "{preset:?}/{seed}: target={target}, actual={actual}, cutoff={cutoff_plateau}"
            );
            assert!(
                surface.cells().iter().any(|cell| {
                    let crust_land = snapshot.crust_kind(cell.id) == Some(CrustKind::Continental);
                    let emergent = relief.land_ocean_kind(cell.id) == Some(LandOceanKind::Land);
                    crust_land != emergent
                }),
                "{preset:?}/{seed}: emergent land must not collapse to crust-kind identity"
            );
            continental_fractions
                .get_mut(&preset)
                .unwrap()
                .push(continental_area_fraction(surface, &snapshot));
            land_fraction_errors.get_mut(&preset).unwrap().push(error);
            final_counts
                .get_mut(&preset)
                .unwrap()
                .insert(snapshot.plates().len());
        }
    }

    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
    let continental_mean = presets
        .map(|preset| (preset, mean(&continental_fractions[&preset])))
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    eprintln!(
        "formation aggregate continental={continental_mean:?} land_errors={land_fraction_errors:?} final_counts={final_counts:?}"
    );

    use ResolvedWorldFormationPreset::{
        Archipelago, Continents, GreatIsland, Supercontinent, VolcanicIslands,
    };
    assert!(continental_mean[&Supercontinent] > continental_mean[&Continents]);
    assert!(continental_mean[&Continents] > continental_mean[&GreatIsland]);
    assert!(continental_mean[&GreatIsland] > continental_mean[&Archipelago]);
    assert!(continental_mean[&Archipelago] > continental_mean[&VolcanicIslands]);
    assert!(final_counts.values().all(|counts| !counts.is_empty()));
}

#[test]
fn authored_initial_continental_fraction_is_statistically_monotonic_after_evolution() {
    let surface = quality_surface();
    let mut outcomes = Vec::with_capacity(QUALITY_SEEDS.len());
    for seed in QUALITY_SEEDS {
        let actual = [0.20_f32, 0.38, 0.55].map(|requested| {
            let snapshot = generate_with_continental_fraction(
                surface,
                seed,
                ResolvedWorldFormationPreset::Continents,
                requested,
            );
            let fraction = continental_area_fraction(surface, &snapshot);
            assert!(
                fraction > 0.0 && fraction < 1.0,
                "seed {seed}, request {requested}: both crust classes must survive"
            );
            fraction
        });
        outcomes.push(actual);
    }

    let mean = std::array::from_fn::<_, 3, _>(|band| {
        outcomes.iter().map(|actual| actual[band]).sum::<f64>() / outcomes.len() as f64
    });
    let median_delta = |lower: usize, upper: usize| {
        let mut deltas = outcomes
            .iter()
            .map(|actual| actual[upper] - actual[lower])
            .collect::<Vec<_>>();
        deltas.sort_by(f64::total_cmp);
        deltas[deltas.len() / 2]
    };
    let paired_medians = [median_delta(0, 1), median_delta(1, 2)];
    assert!(
        mean[0] < mean[1] && mean[1] < mean[2],
        "evolved continental ensemble means were not monotonic: means={mean:?}, outcomes={outcomes:?}"
    );
    assert!(
        paired_medians.into_iter().all(|delta| delta > 0.0),
        "the typical paired response must increase: medians={paired_medians:?}, outcomes={outcomes:?}"
    );
    eprintln!("initial-crust response means={mean:?}, paired median deltas={paired_medians:?}");
}

#[test]
fn authored_land_target_changes_only_sea_level_and_mask_for_17_seeds() {
    let surface = quality_surface();
    for seed in QUALITY_SEEDS {
        let tectonic = generate(surface, seed, ResolvedWorldFormationPreset::Continents);
        let reliefs = [0.20_f32, 0.38, 0.55].map(|target| {
            generate_relief_with_target(
                surface,
                seed,
                ResolvedWorldFormationPreset::Continents,
                &tectonic,
                target,
            )
        });
        let baseline_bits = reliefs[0]
            .elevation_m()
            .values()
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>();
        let actual = reliefs.each_ref().map(|relief| {
            assert_eq!(
                relief
                    .elevation_m()
                    .values()
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                baseline_bits,
                "seed {seed}: target land fraction changed height data"
            );
            land_area_fraction(surface, relief)
        });
        assert!(
            actual[0] <= actual[1] && actual[1] <= actual[2],
            "seed {seed}: actual land fraction was not monotonic: {actual:?}"
        );
        assert!(
            reliefs[0].sea_level_m() >= reliefs[1].sea_level_m()
                && reliefs[1].sea_level_m() >= reliefs[2].sea_level_m(),
            "seed {seed}: sea level must fall as requested land grows"
        );
        for (target, (relief, actual)) in [0.20_f32, 0.38, 0.55]
            .into_iter()
            .zip(reliefs.iter().zip(actual))
        {
            let error = (actual - f64::from(target)).abs();
            assert!(
                error <= cutoff_plateau_area_fraction(surface, relief) + 1.0e-12,
                "seed {seed}: target={target}, actual={actual}"
            );
        }
    }
}

fn continental_area_fraction(
    surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
) -> f64 {
    surface
        .cells()
        .iter()
        .filter(|cell| snapshot.crust_kind(cell.id) == Some(CrustKind::Continental))
        .map(|cell| cell.area.get())
        .sum::<f64>()
        / surface.total_cell_area().get()
}

fn land_area_fraction(surface: &SphericalSurfaceSnapshot, relief: &SphericalReliefSnapshot) -> f64 {
    surface
        .cells()
        .iter()
        .filter(|cell| relief.land_ocean_kind(cell.id) == Some(LandOceanKind::Land))
        .map(|cell| cell.area.get())
        .sum::<f64>()
        / surface.total_cell_area().get()
}

fn cutoff_plateau_area_fraction(
    surface: &SphericalSurfaceSnapshot,
    relief: &SphericalReliefSnapshot,
) -> f64 {
    let quantized_sea_level = (f64::from(relief.sea_level_m()) * 100.0).round() as i64;
    surface
        .cells()
        .iter()
        .zip(relief.elevation_m().values())
        .filter(|(_, elevation)| {
            (f64::from(**elevation) * 100.0).round() as i64 == quantized_sea_level
        })
        .map(|(cell, _)| cell.area.get())
        .sum::<f64>()
        / surface.total_cell_area().get()
}

#[derive(Debug, Clone, Copy)]
struct ResolutionMetrics {
    plate_perimeter: f64,
    total_coast: f64,
    major_coast: f64,
    continental_fraction: f64,
    plate_count: f64,
    major_continents: f64,
}

fn resolution_metrics(
    surface: &SphericalSurfaceSnapshot,
    analysis_surface: &SphericalSurfaceSnapshot,
    seed: u64,
) -> ResolutionMetrics {
    let snapshot = generate(surface, seed, ResolvedWorldFormationPreset::Continents);
    let continents = analyze_continents_at_scale(analysis_surface, surface, &snapshot);
    ResolutionMetrics {
        plate_perimeter: plate_metrics(surface, &snapshot).median_normalized_perimeter,
        total_coast: continents.total_normalized_coast_perimeter,
        major_coast: continents.normalized_coast_perimeter,
        continental_fraction: continental_area_fraction(surface, &snapshot),
        plate_count: snapshot.plates().len() as f64,
        major_continents: continents.major_count as f64,
    }
}

fn metric_median(metrics: &[ResolutionMetrics], select: impl Fn(&ResolutionMetrics) -> f64) -> f64 {
    let mut values = metrics.iter().map(select).collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    median(&values)
}

fn relative_difference(first: f64, second: f64) -> f64 {
    (first - second).abs() / first.abs().max(second.abs()).max(f64::EPSILON)
}

#[derive(Debug, Clone, Copy)]
struct ScaleAnalyzedContinentalMetrics {
    major_count: usize,
    normalized_coast_perimeter: f64,
    total_normalized_coast_perimeter: f64,
}

fn analyze_continents_at_scale(
    analysis_surface: &SphericalSurfaceSnapshot,
    source_surface: &SphericalSurfaceSnapshot,
    snapshot: &SphericalTectonicSnapshot,
) -> ScaleAnalyzedContinentalMetrics {
    let continental = analysis_surface
        .cells()
        .iter()
        .map(|analysis_cell| {
            let source = source_surface
                .cells()
                .iter()
                .max_by(|first, second| {
                    analysis_cell
                        .centroid
                        .dot(first.centroid)
                        .total_cmp(&analysis_cell.centroid.dot(second.centroid))
                })
                .expect("validated source surface has cells");
            snapshot.crust_kind(source.id) == Some(CrustKind::Continental)
        })
        .collect::<Vec<_>>();
    let mut visited = vec![false; continental.len()];
    let mut components = Vec::new();
    for cell in analysis_surface.cells() {
        let start = cell.id.raw() as usize;
        if visited[start] || !continental[start] {
            continue;
        }
        visited[start] = true;
        let mut queue = VecDeque::from([cell.id]);
        let mut area_m2 = 0.0;
        let mut coast_perimeter_m = 0.0;
        while let Some(current) = queue.pop_front() {
            area_m2 += analysis_surface.cell(current).unwrap().area.get();
            for &edge_id in analysis_surface.cell_edges(current).unwrap() {
                let edge = analysis_surface.edge(edge_id).unwrap();
                let neighbor = analysis_surface.opposite_cell(current, edge_id).unwrap();
                let index = neighbor.raw() as usize;
                if continental[index] {
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
            area_m2,
            coast_perimeter_m,
        });
    }
    let area = components
        .iter()
        .map(|component| component.area_m2)
        .sum::<f64>();
    let perimeter = components
        .iter()
        .map(|component| component.coast_perimeter_m)
        .sum::<f64>();
    let major = components
        .iter()
        .filter(|component| component.area_m2 >= area * 0.10)
        .collect::<Vec<_>>();
    let mut major_perimeters = major
        .iter()
        .map(|component| {
            component.coast_perimeter_m
                / equal_area_spherical_circle_perimeter(
                    analysis_surface.radius().get(),
                    component.area_m2,
                )
        })
        .collect::<Vec<_>>();
    major_perimeters.sort_by(f64::total_cmp);
    ScaleAnalyzedContinentalMetrics {
        major_count: major.len(),
        normalized_coast_perimeter: median(&major_perimeters),
        total_normalized_coast_perimeter: perimeter
            / equal_area_spherical_circle_perimeter(analysis_surface.radius().get(), area),
    }
}

#[test]
#[ignore = "release-only 5k/20k morphology resolution gate"]
fn field_morphology_is_resolution_invariant() {
    let coarse_surface = build_surface(5_000);
    let fine_surface = build_surface(20_000);
    let analysis_surface = build_surface(642);
    // Evolved plates are intentionally not CellId-stable across discretizations:
    // contact order is chaotic at that level. Compare physical-scale ensemble
    // statistics, as required by the design, rather than spatial identity.
    const RESOLUTION_SEEDS: [u64; 5] = [42, 3, 7, 11, 19];
    let coarse =
        RESOLUTION_SEEDS.map(|seed| resolution_metrics(&coarse_surface, &analysis_surface, seed));
    let fine =
        RESOLUTION_SEEDS.map(|seed| resolution_metrics(&fine_surface, &analysis_surface, seed));

    let coarse_perimeter = metric_median(&coarse, |metric| metric.plate_perimeter);
    let fine_perimeter = metric_median(&fine, |metric| metric.plate_perimeter);
    let coarse_total_coast = metric_median(&coarse, |metric| metric.total_coast);
    let fine_total_coast = metric_median(&fine, |metric| metric.total_coast);
    let coarse_major_coast = metric_median(&coarse, |metric| metric.major_coast);
    let fine_major_coast = metric_median(&fine, |metric| metric.major_coast);
    let coarse_land_fraction = metric_median(&coarse, |metric| metric.continental_fraction);
    let fine_land_fraction = metric_median(&fine, |metric| metric.continental_fraction);
    let coarse_plate_count = metric_median(&coarse, |metric| metric.plate_count);
    let fine_plate_count = metric_median(&fine, |metric| metric.plate_count);
    let coarse_major = metric_median(&coarse, |metric| metric.major_continents);
    let fine_major = metric_median(&fine, |metric| metric.major_continents);

    eprintln!(
        "resolution morphology: coarse_cells={} fine_cells={} plate_perimeter={coarse_perimeter:.4}/{fine_perimeter:.4} major_coast={coarse_major_coast:.4}/{fine_major_coast:.4} total_coast={coarse_total_coast:.4}/{fine_total_coast:.4} continental={coarse_land_fraction:.4}/{fine_land_fraction:.4} plates={coarse_plate_count:.1}/{fine_plate_count:.1} major={coarse_major:.1}/{fine_major:.1}; coarse={coarse:?}; fine={fine:?}",
        coarse_surface.cells().len(),
        fine_surface.cells().len(),
    );
    assert!(relative_difference(coarse_perimeter, fine_perimeter) <= 0.20);
    assert!(relative_difference(coarse_total_coast, fine_total_coast) <= 0.20);
    assert!(relative_difference(coarse_major_coast, fine_major_coast) <= 0.20);
    assert!((coarse_land_fraction - fine_land_fraction).abs() <= 0.04);
    assert!((coarse_plate_count - fine_plate_count).abs() <= 2.0);
    assert!((coarse_major - fine_major).abs() <= 1.0);
}
