//! PlaTec-style coherent initial lithosphere on the authoritative sphere.
//!
//! Stable farthest-point seeds define the plate partition. Continental crust
//! is a connected domain grown from nuclei on plate representatives; thickness
//! and ocean age remain independent coherent fields.

#![cfg_attr(not(test), allow(dead_code))]

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use rand::RngCore;
use thiserror::Error;

use super::model::{
    ActivePlate, CrustSample, FormationTectonicRecipe, LineageId, MaterialColumn,
    TectonicModelError, TectonicState,
};
use crate::generators::natural::foundation::crust_physics::{
    continental_isostatic_elevation_m, oceanic_plate_cooling_elevation_m,
};
use crate::generators::natural::fractal::FractalProfile;
use crate::generators::natural::morphology::noise::SphericalNoise3d;
use crate::generators::natural::random::{
    LabeledSubstreams, INITIAL_CRUST_V3_LABEL, INITIAL_DOMAINS_V5_LABEL, INITIAL_PLATES_V3_LABEL,
    PLATE_MOTION_V3_LABEL,
};
use crate::generators::natural::topology::{
    farthest_point_seeds, multi_source_distance, NaturalTopologyIndex,
};
use crate::world::natural::{
    CrustKind, NaturalSpecError, ResolvedWorldFormationPreset, SphericalOrogenyKind,
    SphericalPlateRotation, SphericalTectonicValidationError, TectonicActivity, TectonicSpec,
    CONTINENTAL_CRUST_AGE_SENTINEL_MYR, CRUST1_PLATFORM_THICKNESS_QUANTILES_KM, MAX_CRUST_AGE_MYR,
    NO_OROGENY_AGE_SENTINEL_MYR,
};
use crate::world::spatial::{project_tangent, SphericalSurfaceSnapshot, UnitVector3};
use crate::world::CellId;

const MAXIMUM_SEED_WARP_RAD: f64 = 0.07;
const OCEANIC_THICKNESS_BASE_KM: f64 = 5.0;
const OCEANIC_THICKNESS_SPAN_KM: f64 = 5.0;
const INITIAL_OCEANIC_AGE_MIN_MYR: f64 = 8.0;
const INITIAL_OCEANIC_AGE_SPAN_MYR: f64 = 172.0;
const MINIMUM_ANISOTROPIC_EDGE_FACTOR: f64 = 0.70;
const MAXIMUM_ANISOTROPIC_EDGE_FACTOR: f64 = 1.30;

pub(super) fn build_initial_state(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
) -> Result<TectonicState, InitialStateError> {
    let recipe = FormationTectonicRecipe::for_preset(preset);
    spec.validate()?;
    let cell_count = surface.cells().len();
    if cell_count != topology.cell_count() {
        return Err(InitialStateError::CardinalityMismatch {
            surface_cells: cell_count,
            topology_cells: topology.cell_count(),
        });
    }
    let plate_count = usize::from(spec.plate_count);
    if plate_count > cell_count {
        return Err(InitialStateError::PlateCountExceedsCells {
            plates: plate_count,
            cells: cell_count,
        });
    }

    let (seeds, centers) = initial_plate_centers(surface, topology, plate_count, recipe, streams)?;
    let owners = assign_nearest_centers(surface, &centers);
    validate_initial_partition(topology, &seeds, &owners)?;
    let plates = initial_plates(surface, spec.activity, &seeds, streams)?;
    let samples = initial_crust_samples(surface, topology, spec, preset, streams, &owners, &seeds);
    TectonicState::new(samples, plates, spec.plate_count.into()).map_err(Into::into)
}

pub(super) fn build_initial_state_v5(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
) -> Result<TectonicState, InitialStateError> {
    let recipe = FormationTectonicRecipe::for_preset(preset);
    spec.validate()?;
    let cell_count = surface.cells().len();
    if cell_count != topology.cell_count() {
        return Err(InitialStateError::CardinalityMismatch {
            surface_cells: cell_count,
            topology_cells: topology.cell_count(),
        });
    }
    let plate_count = usize::from(spec.plate_count);
    if plate_count > cell_count {
        return Err(InitialStateError::PlateCountExceedsCells {
            plates: plate_count,
            cells: cell_count,
        });
    }
    let mut domain_rng = streams.stream(INITIAL_DOMAINS_V5_LABEL);
    let seeds = irregular_blue_noise_seeds(topology, plate_count, domain_rng.next_u64(), streams);
    let owners = assign_anisotropic_domains(surface, topology, &seeds, recipe, streams)?;
    validate_initial_partition(topology, &seeds, &owners)?;
    let plates = initial_plates(surface, spec.activity, &seeds, streams)?;
    let samples = initial_crust_samples(surface, topology, spec, preset, streams, &owners, &seeds);
    let mut state = TectonicState::new(samples, plates, spec.plate_count.into())?;
    mark_dominant_continental_lineages(&mut state, surface, topology);
    mark_opening_phase_lineages(&mut state, preset);
    Ok(state)
}

fn irregular_blue_noise_seeds(
    topology: &NaturalTopologyIndex,
    count: usize,
    first_draw: u64,
    streams: &LabeledSubstreams,
) -> Vec<CellId> {
    if count == 0 {
        return Vec::new();
    }
    let cell_count = topology.cell_count();
    let mut selected = vec![false; cell_count];
    let first = first_draw as usize % cell_count;
    selected[first] = true;
    let mut seeds = vec![CellId::from_raw(first as u32)];
    while seeds.len() < count {
        let distances = multi_source_distance(topology, &seeds, None);
        let mut candidates = (0..cell_count)
            .filter(|&index| !selected[index])
            .collect::<Vec<_>>();
        candidates.sort_by(|&first, &second| {
            distances[second]
                .cmp(&distances[first])
                .then_with(|| first.cmp(&second))
        });
        // Draw from the well-separated frontier instead of taking its single
        // extreme every time. This retains blue-noise spacing without forcing
        // the near-equilateral Delaunay triangles that caused V4's 120-degree
        // honeycomb signature.
        let frontier = (cell_count / count.max(1)).clamp(1, candidates.len());
        let rank = streams.counter_u64(
            INITIAL_DOMAINS_V5_LABEL,
            &[2, seeds.len() as u64, first_draw],
        ) as usize
            % frontier;
        let next = candidates[rank];
        selected[next] = true;
        seeds.push(CellId::from_raw(next as u32));
    }
    seeds
}

fn assign_anisotropic_domains(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    seeds: &[CellId],
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<Vec<LineageId>, InitialStateError> {
    let profile = FractalProfile {
        octaves: 2,
        frequency: recipe.base_scale_rad.recip(),
        lacunarity: 2.03,
        persistence: 0.5,
    };
    let noise = (0..seeds.len())
        .map(|lineage| {
            std::array::from_fn(|channel| {
                SphericalNoise3d::new(streams.counter_u64(
                    INITIAL_DOMAINS_V5_LABEL,
                    &[1, lineage as u64, channel as u64],
                ) as u32)
            })
        })
        .collect::<Vec<_>>();
    let mut costs = vec![u64::MAX; topology.cell_count()];
    let mut owners = vec![None; topology.cell_count()];
    let mut pending = BinaryHeap::new();
    for (lineage, &seed) in seeds.iter().enumerate() {
        let index = seed.raw() as usize;
        costs[index] = 0;
        owners[index] = Some(LineageId::from_raw(lineage as u32));
        pending.push(Reverse((0_u64, lineage as u32, seed.raw())));
    }
    while let Some(Reverse((cost, raw_lineage, raw_cell))) = pending.pop() {
        let cell = raw_cell as usize;
        let lineage = LineageId::from_raw(raw_lineage);
        if costs[cell] != cost || owners[cell] != Some(lineage) {
            continue;
        }
        for arc in &topology.arcs()[cell] {
            let edge = surface
                .edge(arc.edge)
                .expect("topology edges originate from the same validated surface");
            let direction = project_tangent(
                surface.cells()[arc.neighbor.raw() as usize]
                    .centroid
                    .components(),
                edge.midpoint,
            );
            let factor = anisotropic_edge_factor(
                &noise[raw_lineage as usize],
                edge.midpoint,
                direction,
                profile,
            );
            let traversal = ((arc.traversal_cost as f64) * factor).round().max(1.0) as u64;
            let candidate = cost.saturating_add(traversal);
            let neighbor = arc.neighbor.raw() as usize;
            let candidate_key = (candidate, raw_lineage);
            let incumbent_key = (
                costs[neighbor],
                owners[neighbor].map_or(u32::MAX, LineageId::raw),
            );
            if candidate_key < incumbent_key {
                costs[neighbor] = candidate;
                owners[neighbor] = Some(lineage);
                pending.push(Reverse((candidate, raw_lineage, arc.neighbor.raw())));
            }
        }
    }
    owners
        .into_iter()
        .enumerate()
        .map(|(index, owner)| {
            owner.ok_or(InitialStateError::UnassignedAnisotropicCell {
                cell: CellId::from_raw(index as u32),
            })
        })
        .collect()
}

fn anisotropic_edge_factor(
    noise: &[SphericalNoise3d; 4],
    position: UnitVector3,
    edge_direction: [f64; 3],
    profile: FractalProfile,
) -> f64 {
    let scalar = (4.0 * noise[0].fbm(position, profile)).clamp(-1.0, 1.0);
    let fabric = std::array::from_fn(|axis| noise[axis + 1].fbm(position, profile));
    let fabric = project_tangent(fabric, position);
    let fabric_norm = norm(fabric);
    let edge_norm = norm(edge_direction);
    let directional = if fabric_norm > f64::EPSILON && edge_norm > f64::EPSILON {
        let agreement = fabric
            .into_iter()
            .zip(edge_direction)
            .map(|(first, second)| first * second)
            .sum::<f64>()
            .abs()
            / (fabric_norm * edge_norm);
        2.0 * agreement.clamp(0.0, 1.0) - 1.0
    } else {
        0.0
    };
    let signal = (0.35 * scalar + 0.65 * directional).clamp(-1.0, 1.0);
    (1.0 + 0.3 * signal).clamp(
        MINIMUM_ANISOTROPIC_EDGE_FACTOR,
        MAXIMUM_ANISOTROPIC_EDGE_FACTOR,
    )
}

fn initial_plate_centers(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    plate_count: usize,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<(Vec<CellId>, Vec<UnitVector3>), InitialStateError> {
    let mut rng = streams.stream(INITIAL_PLATES_V3_LABEL);
    let seeds = farthest_point_seeds(topology, plate_count, rng.next_u64());
    let warp_noise = [
        SphericalNoise3d::new(rng.next_u32()),
        SphericalNoise3d::new(rng.next_u32()),
        SphericalNoise3d::new(rng.next_u32()),
    ];
    let warp_profile = FractalProfile {
        octaves: 2,
        frequency: recipe.base_scale_rad.recip(),
        lacunarity: 2.03,
        persistence: 0.5,
    };
    let maximum_warp = (recipe.base_scale_rad * 0.06).min(MAXIMUM_SEED_WARP_RAD);
    let centers = seeds
        .iter()
        .map(|&seed| {
            let radial = surface
                .cell(seed)
                .expect("farthest-point seeds are authoritative cells")
                .centroid;
            let variation = std::array::from_fn(|axis| warp_noise[axis].fbm(radial, warp_profile));
            perturb_direction(radial, variation, maximum_warp)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((seeds, centers))
}

fn perturb_direction(
    radial: UnitVector3,
    variation: [f64; 3],
    maximum_angle: f64,
) -> Result<UnitVector3, InitialStateError> {
    let tangent = project_tangent(variation, radial);
    let tangent_norm = norm(tangent);
    if tangent_norm <= f64::EPSILON {
        return Ok(radial);
    }
    let tangent_unit = tangent.map(|component| component / tangent_norm);
    let angle = maximum_angle * (tangent_norm / 3.0_f64.sqrt()).clamp(0.0, 1.0);
    let [x, y, z] = radial.components();
    UnitVector3::new(
        angle.cos() * x + angle.sin() * tangent_unit[0],
        angle.cos() * y + angle.sin() * tangent_unit[1],
        angle.cos() * z + angle.sin() * tangent_unit[2],
    )
    .map_err(|_| InitialStateError::InvalidWarpedDirection)
}

fn assign_nearest_centers(
    surface: &SphericalSurfaceSnapshot,
    centers: &[UnitVector3],
) -> Vec<LineageId> {
    surface
        .cells()
        .iter()
        .map(|cell| {
            let mut best_index = 0_usize;
            let mut best_dot = cell.centroid.dot(centers[0]);
            for (index, &center) in centers.iter().enumerate().skip(1) {
                let candidate = cell.centroid.dot(center);
                if candidate > best_dot {
                    best_dot = candidate;
                    best_index = index;
                }
            }
            LineageId::from_raw(best_index as u32)
        })
        .collect()
}

fn validate_initial_partition(
    topology: &NaturalTopologyIndex,
    seeds: &[CellId],
    owners: &[LineageId],
) -> Result<(), InitialStateError> {
    for (index, &seed) in seeds.iter().enumerate() {
        let lineage = LineageId::from_raw(index as u32);
        if owners[seed.raw() as usize] != lineage {
            return Err(InitialStateError::SeedOwnershipLost { lineage, seed });
        }
        let expected = owners.iter().filter(|&&owner| owner == lineage).count();
        let reached = connected_owner_count(topology, owners, lineage);
        if reached != expected {
            return Err(InitialStateError::DisconnectedInitialPlate {
                lineage,
                reached,
                expected,
            });
        }
    }
    Ok(())
}

fn connected_owner_count(
    topology: &NaturalTopologyIndex,
    owners: &[LineageId],
    owner: LineageId,
) -> usize {
    let Some(start) = owners.iter().position(|&candidate| candidate == owner) else {
        return 0;
    };
    let mut reached = vec![false; owners.len()];
    let mut queue = VecDeque::from([start]);
    reached[start] = true;
    let mut count = 0;
    while let Some(index) = queue.pop_front() {
        count += 1;
        for arc in &topology.arcs()[index] {
            let neighbor = arc.neighbor.raw() as usize;
            if !reached[neighbor] && owners[neighbor] == owner {
                reached[neighbor] = true;
                queue.push_back(neighbor);
            }
        }
    }
    count
}

fn initial_plates(
    surface: &SphericalSurfaceSnapshot,
    activity: TectonicActivity,
    seeds: &[CellId],
    streams: &LabeledSubstreams,
) -> Result<Vec<ActivePlate>, InitialStateError> {
    let mut rng = streams.stream(PLATE_MOTION_V3_LABEL);
    seeds
        .iter()
        .copied()
        .enumerate()
        .map(|(index, representative)| {
            let pole = random_unit_direction(&mut rng)?;
            let unit = unit_interval(rng.next_u64());
            let (minimum_speed, maximum_speed) = match activity {
                TectonicActivity::Quiet => (20.0, 50.0),
                TectonicActivity::Moderate => (40.0, 90.0),
                TectonicActivity::Active => (60.0, 120.0),
            };
            let speed_mm_per_year = minimum_speed + (maximum_speed - minimum_speed) * unit;
            let angular_rate =
                (speed_mm_per_year * 1.0e9_f64 / surface.radius().get()).round() as u64;
            let rotation = SphericalPlateRotation::new(pole, angular_rate.max(1))?;
            rotation.validate_for_radius(surface.radius())?;
            Ok(ActivePlate::new(
                LineageId::from_raw(index as u32),
                representative,
                rotation,
            ))
        })
        .collect()
}

fn initial_crust_samples(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    preset: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
    owners: &[LineageId],
    plate_reps: &[CellId],
) -> Vec<CrustSample> {
    let recipe = FormationTectonicRecipe::for_preset(preset);
    let mut rng = streams.stream(INITIAL_CRUST_V3_LABEL);
    let thickness_noise = SphericalNoise3d::new(rng.next_u32());
    let age_noise = SphericalNoise3d::new(rng.next_u32());
    let continental = continental_from_plate_nuclei(
        surface,
        topology,
        plate_reps,
        preset,
        streams,
        spec.continental_crust_fraction,
    );
    let thickness_signals = surface
        .cells()
        .iter()
        .map(|cell| thickness_noise.fbm(cell.centroid, recipe.initial_crust_profile))
        .collect::<Vec<_>>();
    let platform_thickness_km =
        platform_thickness_by_rank(surface, &thickness_signals, &continental);

    surface
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let thickness_signal = normalized_signal(thickness_signals[index]);
            let age_signal =
                normalized_signal(age_noise.fbm(cell.centroid, recipe.initial_crust_profile));
            let (kind, thickness_km, age_myr, tectonic_elevation_m) = if continental[index] {
                let thickness_km = platform_thickness_km[index];
                (
                    CrustKind::Continental,
                    thickness_km,
                    CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                    continental_isostatic_elevation_m(thickness_km),
                )
            } else {
                let age = INITIAL_OCEANIC_AGE_MIN_MYR + INITIAL_OCEANIC_AGE_SPAN_MYR * age_signal;
                let thickness_km = (OCEANIC_THICKNESS_BASE_KM
                    + OCEANIC_THICKNESS_SPAN_KM * thickness_signal)
                    as f32;
                (
                    CrustKind::Oceanic,
                    thickness_km,
                    age.min(f64::from(MAX_CRUST_AGE_MYR)) as f32,
                    oceanic_plate_cooling_elevation_m(age as f32, thickness_km),
                )
            };
            CrustSample {
                position: cell.centroid,
                anchor: cell.id,
                owner: owners[index],
                kind,
                thickness_km,
                age_myr,
                tectonic_elevation_m,
                lineation: [0.0; 2],
                orogeny: SphericalOrogenyKind::None,
                orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
                material: MaterialColumn::pure(kind, cell.area.get(), thickness_km)
                    .expect("validated initialized crust has a valid material column"),
            }
        })
        .collect()
}

fn continental_from_plate_nuclei(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    plate_reps: &[CellId],
    preset: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
    target_fraction: f32,
) -> Vec<bool> {
    let nuclei = select_continental_nuclei(surface, plate_reps, preset, streams);
    let (owner, dist) = hop_nearest_nuclei(topology, &nuclei);
    let primary_first = preset.satellite_nucleus_count() > 0;
    let order = continental_cell_order(&owner, &dist, primary_first);
    area_prefix_mask(surface, &order, target_fraction)
}

fn mark_dominant_continental_lineages(
    state: &mut TectonicState,
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
) {
    let cell_count = surface.cells().len();
    let mut continental = vec![false; cell_count];
    let mut owner_at = vec![None; cell_count];
    for sample in &state.samples {
        let index = sample.anchor.raw() as usize;
        if index >= cell_count {
            continue;
        }
        owner_at[index] = Some(sample.owner);
        if sample.kind == CrustKind::Continental {
            continental[index] = true;
        }
    }
    let mut seen = vec![false; cell_count];
    let mut best_area = 0.0;
    let mut best_cells = Vec::new();
    for start in 0..cell_count {
        if !continental[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let mut cells = vec![start];
        let mut area = 0.0;
        while let Some(cell) = stack.pop() {
            area += surface.cells()[cell].area.get();
            for arc in &topology.arcs()[cell] {
                let neighbor = arc.neighbor.raw() as usize;
                if neighbor >= cell_count || !continental[neighbor] || seen[neighbor] {
                    continue;
                }
                seen[neighbor] = true;
                stack.push(neighbor);
                cells.push(neighbor);
            }
        }
        let replace = area > best_area
            || (area == best_area
                && cells.first().copied().unwrap_or(usize::MAX)
                    < best_cells.first().copied().unwrap_or(usize::MAX));
        if replace {
            best_area = area;
            best_cells = cells;
        }
    }
    for cell in best_cells {
        if let Some(lineage) = owner_at[cell] {
            state.initiation.mark_dominant(lineage);
        }
    }
}

fn mark_opening_phase_lineages(state: &mut TectonicState, preset: ResolvedWorldFormationPreset) {
    if preset != ResolvedWorldFormationPreset::Archipelago {
        return;
    }
    for sample in &state.samples {
        if sample.kind == CrustKind::Continental {
            state.initiation.mark_opening_phase(sample.owner);
        }
    }
}

fn select_continental_nuclei(
    surface: &SphericalSurfaceSnapshot,
    plate_reps: &[CellId],
    preset: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
) -> Vec<CellId> {
    let plate_count = plate_reps.len();
    debug_assert!(plate_count >= 1);
    let first = (streams.counter_u64(INITIAL_CRUST_V3_LABEL, &[0]) as usize) % plate_count;
    let total = if preset.satellite_nucleus_count() > 0 {
        1 + usize::from(preset.satellite_nucleus_count()).min(plate_count - 1)
    } else {
        usize::from(preset.continental_nucleus_count()).clamp(1, plate_count)
    };
    farthest_plate_representatives(surface, plate_reps, first, total)
}

fn farthest_plate_representatives(
    surface: &SphericalSurfaceSnapshot,
    plate_reps: &[CellId],
    first_index: usize,
    total: usize,
) -> Vec<CellId> {
    let mut selected = Vec::with_capacity(total);
    let mut chosen = vec![false; plate_reps.len()];
    selected.push(plate_reps[first_index]);
    chosen[first_index] = true;
    while selected.len() < total {
        let mut best: Option<(f64, usize)> = None;
        for (index, &rep) in plate_reps.iter().enumerate() {
            if chosen[index] {
                continue;
            }
            let centroid = surface.cells()[rep.raw() as usize].centroid;
            let nearest_dot = selected
                .iter()
                .map(|&seed| centroid.dot(surface.cells()[seed.raw() as usize].centroid))
                .fold(f64::NEG_INFINITY, f64::max);
            let take = match best {
                None => true,
                Some((best_dot, best_index)) => match nearest_dot.total_cmp(&best_dot) {
                    std::cmp::Ordering::Less => true,
                    std::cmp::Ordering::Greater => false,
                    std::cmp::Ordering::Equal => index < best_index,
                },
            };
            if take {
                best = Some((nearest_dot, index));
            }
        }
        let index = best.expect("at least one unselected plate remains").1;
        chosen[index] = true;
        selected.push(plate_reps[index]);
    }
    selected
}

fn hop_nearest_nuclei(topology: &NaturalTopologyIndex, nuclei: &[CellId]) -> (Vec<u32>, Vec<u32>) {
    let neighbors = topology
        .arcs()
        .iter()
        .map(|arcs| {
            arcs.iter()
                .map(|arc| arc.neighbor.raw() as usize)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let sources = nuclei
        .iter()
        .map(|cell| cell.raw() as usize)
        .collect::<Vec<_>>();
    hop_nearest_from_arcs(&neighbors, &sources)
}

fn hop_nearest_from_arcs(arcs: &[Vec<usize>], nuclei: &[usize]) -> (Vec<u32>, Vec<u32>) {
    let cell_count = arcs.len();
    let mut dist = vec![u32::MAX; cell_count];
    let mut owner = vec![u32::MAX; cell_count];
    for (nucleus_index, &start) in nuclei.iter().enumerate() {
        let mut seen = vec![false; cell_count];
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        let mut depths = vec![0_u32; cell_count];
        while let Some(cell) = queue.pop_front() {
            let depth = depths[cell];
            if depth < dist[cell] {
                dist[cell] = depth;
                owner[cell] = nucleus_index as u32;
            }
            for &neighbor in &arcs[cell] {
                if seen[neighbor] {
                    continue;
                }
                seen[neighbor] = true;
                depths[neighbor] = depth + 1;
                queue.push_back(neighbor);
            }
        }
    }
    (owner, dist)
}

fn continental_cell_order(owner: &[u32], dist: &[u32], primary_first: bool) -> Vec<usize> {
    let mut order = (0..owner.len()).collect::<Vec<_>>();
    order.sort_by(|&first, &second| {
        if primary_first {
            let first_primary = owner[first] == 0;
            let second_primary = owner[second] == 0;
            second_primary
                .cmp(&first_primary)
                .then(dist[first].cmp(&dist[second]))
                .then(owner[first].cmp(&owner[second]))
                .then(first.cmp(&second))
        } else {
            dist[first]
                .cmp(&dist[second])
                .then(owner[first].cmp(&owner[second]))
                .then(first.cmp(&second))
        }
    });
    order
}

fn area_prefix_mask(
    surface: &SphericalSurfaceSnapshot,
    order: &[usize],
    target_fraction: f32,
) -> Vec<bool> {
    let areas = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .collect::<Vec<_>>();
    area_prefix_from_areas(
        &areas,
        order,
        surface.total_cell_area().get() * f64::from(target_fraction),
    )
}

fn area_prefix_from_areas(areas: &[f64], order: &[usize], target: f64) -> Vec<bool> {
    let mut selected_count = 0;
    let mut selected_area = 0.0;
    for (rank, &index) in order.iter().enumerate() {
        let next_area = selected_area + areas[index];
        if (next_area - target).abs() <= (selected_area - target).abs() {
            selected_area = next_area;
            selected_count = rank + 1;
        } else {
            break;
        }
    }
    let mut selected = vec![false; areas.len()];
    for &index in order.iter().take(selected_count) {
        selected[index] = true;
    }
    selected
}

/// Gives every continental cell the CRUST1.0 platform thickness at the
/// area-weighted rank of its coherent thickness signal (cumulative-area cell
/// midpoints), so the initial inventory reproduces the frozen stable-platform
/// CDF while the noise still decides where the thicker and thinner platforms
/// sit.
fn platform_thickness_by_rank(
    surface: &SphericalSurfaceSnapshot,
    signals: &[f64],
    continental: &[bool],
) -> Vec<f32> {
    let mut order = (0..signals.len())
        .filter(|&index| continental[index])
        .collect::<Vec<_>>();
    order.sort_by(|&first, &second| {
        signals[first]
            .total_cmp(&signals[second])
            .then_with(|| first.cmp(&second))
    });
    let total_area = order
        .iter()
        .map(|&index| surface.cells()[index].area.get())
        .sum::<f64>();
    let mut thickness = vec![0.0_f32; signals.len()];
    let mut cumulative_area = 0.0;
    for &index in &order {
        let area = surface.cells()[index].area.get();
        thickness[index] = platform_thickness_km((cumulative_area + area * 0.5) / total_area);
        cumulative_area += area;
    }
    thickness
}

/// Interpolates the frozen CRUST1.0 platform quantile table at an area
/// quantile in `[0, 1]`.
fn platform_thickness_km(area_quantile: f64) -> f32 {
    let knots = CRUST1_PLATFORM_THICKNESS_QUANTILES_KM;
    let position = area_quantile.clamp(0.0, 1.0) * (knots.len() - 1) as f64;
    let lower = (position.floor() as usize).min(knots.len() - 2);
    let fraction = position - lower as f64;
    let (start, end) = (f64::from(knots[lower]), f64::from(knots[lower + 1]));
    (start + (end - start) * fraction) as f32
}

fn random_unit_direction(rng: &mut impl RngCore) -> Result<UnitVector3, InitialStateError> {
    let vertical = unit_interval(rng.next_u64()).mul_add(2.0, -1.0);
    let azimuth = std::f64::consts::TAU * unit_interval(rng.next_u64());
    let horizontal = (1.0 - vertical * vertical).max(0.0).sqrt();
    UnitVector3::new(
        horizontal * azimuth.cos(),
        horizontal * azimuth.sin(),
        vertical,
    )
    .map_err(|_| InitialStateError::InvalidRotationAxis)
}

fn unit_interval(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}

fn normalized_signal(signal: f64) -> f64 {
    signal.mul_add(0.5, 0.5).clamp(0.0, 1.0)
}

fn norm(vector: [f64; 3]) -> f64 {
    vector
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt()
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum InitialStateError {
    #[error("invalid tectonic specification: {0}")]
    InvalidSpec(#[from] NaturalSpecError),
    #[error(
        "initial tectonics surface has {surface_cells} cells but topology has {topology_cells}"
    )]
    CardinalityMismatch {
        surface_cells: usize,
        topology_cells: usize,
    },
    #[error("requested {plates} initial plates for only {cells} surface cells")]
    PlateCountExceedsCells { plates: usize, cells: usize },
    #[error("a coherent seed warp did not produce a finite spherical direction")]
    InvalidWarpedDirection,
    #[error("the plate-motion stream did not produce a finite rotation axis")]
    InvalidRotationAxis,
    #[error("invalid initial plate rotation: {0}")]
    InvalidRotation(#[from] SphericalTectonicValidationError),
    #[error("initial lineage {lineage:?} lost representative seed {seed:?}")]
    SeedOwnershipLost { lineage: LineageId, seed: CellId },
    #[error("initial lineage {lineage:?} reaches {reached} of {expected} owned cells")]
    DisconnectedInitialPlate {
        lineage: LineageId,
        reached: usize,
        expected: usize,
    },
    #[error("anisotropic initial-domain solve did not reach {cell:?}")]
    UnassignedAnisotropicCell { cell: CellId },
    #[error("invalid transient tectonic state: {0}")]
    InvalidState(#[from] TectonicModelError),
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{
        area_prefix_from_areas, build_initial_state, build_initial_state_v5,
        continental_cell_order, hop_nearest_from_arcs,
    };
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, ResolvedWorldFormationPreset, SphericalOrogenyKind, TectonicSpec,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
        CONTINENTAL_CRUST_MIN_THICKNESS_KM, CRUST1_PLATFORM_THICKNESS_QUANTILES_KM,
        MAX_CRUST_AGE_MYR, NO_OROGENY_AGE_SENTINEL_MYR, OCEANIC_CRUST_MAX_THICKNESS_KM,
        OCEANIC_CRUST_MIN_THICKNESS_KM,
    };
    use crate::world::spatial::SphericalNaturalSurface;
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    fn fixture(
        cells: u32,
    ) -> (
        crate::world::spatial::SphericalSurfaceSnapshot,
        NaturalTopologyIndex,
    ) {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: cells,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        (surface, topology)
    }

    fn streams(seed: u64) -> LabeledSubstreams {
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(seed),
            StageIdentity::new("initial-tectonics-test", 1, "sekai.test"),
        ));
        LabeledSubstreams::capture(&mut rng)
    }

    fn connected_owner_count(
        topology: &NaturalTopologyIndex,
        owners: &[crate::generators::natural::foundation::tectonics::model::LineageId],
        owner: crate::generators::natural::foundation::tectonics::model::LineageId,
    ) -> usize {
        let Some(start) = owners.iter().position(|&candidate| candidate == owner) else {
            return 0;
        };
        let mut reached = vec![false; owners.len()];
        let mut queue = VecDeque::from([start]);
        reached[start] = true;
        let mut count = 0;
        while let Some(index) = queue.pop_front() {
            count += 1;
            for arc in &topology.arcs()[index] {
                let neighbor = arc.neighbor.raw() as usize;
                if !reached[neighbor] && owners[neighbor] == owner {
                    reached[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        count
    }

    fn continental_area(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        state: &crate::generators::natural::foundation::tectonics::model::TectonicState,
    ) -> f64 {
        surface
            .cells()
            .iter()
            .zip(&state.samples)
            .filter(|(_, sample)| sample.kind == CrustKind::Continental)
            .map(|(cell, _)| cell.area.get())
            .sum::<f64>()
    }

    fn mixed_plate_count(
        state: &crate::generators::natural::foundation::tectonics::model::TectonicState,
    ) -> usize {
        let mut kinds = vec![
            (false, false);
            usize::from(TectonicSpec::default().plate_count).max(
                state
                    .samples
                    .iter()
                    .map(|sample| sample.owner.raw() as usize + 1)
                    .max()
                    .unwrap_or(0),
            )
        ];
        for sample in &state.samples {
            let slot = &mut kinds[sample.owner.raw() as usize];
            match sample.kind {
                CrustKind::Continental => slot.0 = true,
                CrustKind::Oceanic => slot.1 = true,
            }
        }
        kinds
            .iter()
            .filter(|&&(continental, oceanic)| continental && oceanic)
            .count()
    }

    fn has_cross_plate_continental_edge(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        state: &crate::generators::natural::foundation::tectonics::model::TectonicState,
    ) -> bool {
        surface.edges().iter().any(|edge| {
            let [first, second] = edge.cells.map(|cell| &state.samples[cell.raw() as usize]);
            first.kind == CrustKind::Continental
                && second.kind == CrustKind::Continental
                && first.owner != second.owner
        })
    }

    #[test]
    fn hop_nearest_keeps_equal_distance_cells_on_the_earlier_nucleus() {
        let arcs = vec![vec![1], vec![0, 2], vec![1, 3], vec![2]];
        let (owner, dist) = hop_nearest_from_arcs(&arcs, &[0, 3]);
        assert_eq!(owner, vec![0, 0, 1, 1]);
        assert_eq!(dist, vec![0, 1, 1, 0]);
    }

    #[test]
    fn great_island_order_takes_the_primary_domain_before_satellites() {
        let owner = [0, 0, 1, 1];
        let dist = [0, 1, 0, 1];
        assert_eq!(
            continental_cell_order(&owner, &dist, true),
            vec![0, 1, 2, 3]
        );
        assert_eq!(
            continental_cell_order(&owner, &dist, false),
            vec![0, 2, 1, 3]
        );
    }

    #[test]
    fn area_prefix_nests_when_the_target_grows() {
        let areas = [1.0, 1.0, 1.0, 1.0];
        let order = [0, 1, 2, 3];
        let low = area_prefix_from_areas(&areas, &order, 2.0);
        let high = area_prefix_from_areas(&areas, &order, 3.0);
        assert_eq!(low, vec![true, true, false, false]);
        assert_eq!(high, vec![true, true, true, false]);
        assert!(low
            .iter()
            .zip(&high)
            .all(|(first, second)| !first || *second));
    }

    #[test]
    fn initial_continental_inventory_reproduces_the_platform_table() {
        let (surface, topology) = fixture(642);
        let spec = TectonicSpec::default();
        let state = build_initial_state_v5(
            &surface,
            &topology,
            &spec,
            ResolvedWorldFormationPreset::Continents,
            &streams(42),
        )
        .unwrap();
        let mut samples = state
            .samples
            .iter()
            .filter(|sample| sample.kind == CrustKind::Continental)
            .map(|sample| {
                (
                    f64::from(sample.thickness_km),
                    surface.cells()[sample.anchor.raw() as usize].area.get(),
                )
            })
            .collect::<Vec<_>>();
        samples.sort_by(|first, second| first.0.total_cmp(&second.0));
        let total = samples.iter().map(|sample| sample.1).sum::<f64>();
        let quantile = |q: f64| {
            let mut cumulative = 0.0;
            samples
                .iter()
                .find(|&&(_, area)| {
                    cumulative += area;
                    cumulative >= q * total
                })
                .map_or(f64::NAN, |sample| sample.0)
        };
        let table = CRUST1_PLATFORM_THICKNESS_QUANTILES_KM;
        assert!(
            (quantile(0.5) - f64::from(table[10])).abs() <= 0.6,
            "{}",
            quantile(0.5)
        );
        assert!(
            (quantile(0.05) - f64::from(table[1])).abs() <= 1.0,
            "{}",
            quantile(0.05)
        );
        assert!(
            (quantile(0.95) - f64::from(table[19])).abs() <= 1.0,
            "{}",
            quantile(0.95)
        );
        assert!(samples.first().unwrap().0 >= f64::from(table[0]));
        assert!(samples.last().unwrap().0 <= f64::from(table[20]));
        assert!(state
            .samples
            .iter()
            .filter(|sample| sample.kind == CrustKind::Continental)
            .all(|sample| {
                sample.material.continental_thickness_km() == Some(sample.thickness_km)
            }));
    }

    #[test]
    fn initial_state_is_dense_connected_area_bounded_and_materially_valid() {
        for cells in [42, 162, 642] {
            let (surface, topology) = fixture(cells);
            let spec = TectonicSpec::default();
            let state = build_initial_state(
                &surface,
                &topology,
                &spec,
                ResolvedWorldFormationPreset::Continents,
                &streams(71),
            )
            .unwrap();
            let owners = state.initial_owners();

            assert_eq!(state.samples.len(), surface.cells().len());
            assert_eq!(state.plates.len(), usize::from(spec.plate_count));
            assert_eq!(owners.len(), surface.cells().len());
            for (index, sample) in state.samples.iter().enumerate() {
                assert_eq!(sample.anchor, CellId::from_raw(index as u32));
                let norm = sample
                    .position
                    .components()
                    .iter()
                    .map(|component| component * component)
                    .sum::<f64>()
                    .sqrt();
                assert!((norm - 1.0).abs() <= 16.0 * f64::EPSILON);
                assert_eq!(sample.lineation, [0.0; 2]);
                assert_eq!(sample.orogeny, SphericalOrogenyKind::None);
                assert_eq!(sample.orogeny_age_myr, NO_OROGENY_AGE_SENTINEL_MYR);
                match sample.kind {
                    CrustKind::Continental => {
                        assert!((CONTINENTAL_CRUST_MIN_THICKNESS_KM
                            ..=CONTINENTAL_CRUST_MAX_THICKNESS_KM)
                            .contains(&sample.thickness_km));
                        assert_eq!(sample.age_myr, CONTINENTAL_CRUST_AGE_SENTINEL_MYR);
                    }
                    CrustKind::Oceanic => {
                        assert!(
                            (OCEANIC_CRUST_MIN_THICKNESS_KM..=OCEANIC_CRUST_MAX_THICKNESS_KM)
                                .contains(&sample.thickness_km)
                        );
                        assert!((0.0..=MAX_CRUST_AGE_MYR).contains(&sample.age_myr));
                    }
                }
            }

            for plate in &state.plates {
                let owned = owners
                    .iter()
                    .filter(|&&owner| owner == plate.lineage)
                    .count();
                assert!(owned > 0);
                assert_eq!(
                    connected_owner_count(&topology, &owners, plate.lineage),
                    owned
                );
                assert_eq!(owners[plate.representative.raw() as usize], plate.lineage);
            }

            let target =
                surface.total_cell_area().get() * f64::from(spec.continental_crust_fraction);
            let max_cell_area = surface
                .cells()
                .iter()
                .map(|cell| cell.area.get())
                .max_by(f64::total_cmp)
                .unwrap();
            assert!((continental_area(&surface, &state) - target).abs() <= max_cell_area);
        }
    }

    #[test]
    fn initial_state_is_seeded_and_presets_change_crust_not_via_crust_streams() {
        let (surface, topology) = fixture(642);
        let spec = TectonicSpec::default();
        let build = |seed, preset| {
            build_initial_state(&surface, &topology, &spec, preset, &streams(seed)).unwrap()
        };
        let first = build(91, ResolvedWorldFormationPreset::Continents);
        let repeated = build(91, ResolvedWorldFormationPreset::Continents);
        let changed = build(92, ResolvedWorldFormationPreset::Continents);
        assert_eq!(first.initial_owners(), repeated.initial_owners());
        assert_eq!(
            first
                .samples
                .iter()
                .map(|sample| (
                    sample.kind,
                    sample.thickness_km.to_bits(),
                    sample.age_myr.to_bits()
                ))
                .collect::<Vec<_>>(),
            repeated
                .samples
                .iter()
                .map(|sample| (
                    sample.kind,
                    sample.thickness_km.to_bits(),
                    sample.age_myr.to_bits()
                ))
                .collect::<Vec<_>>()
        );
        assert_ne!(first.initial_owners(), changed.initial_owners());

        let supercontinent = build(91, ResolvedWorldFormationPreset::Supercontinent);
        let archipelago = build(91, ResolvedWorldFormationPreset::Archipelago);
        assert_ne!(
            first
                .samples
                .iter()
                .map(|sample| sample.kind)
                .collect::<Vec<_>>(),
            supercontinent
                .samples
                .iter()
                .map(|sample| sample.kind)
                .collect::<Vec<_>>()
        );
        let boundary_fraction =
            |state: &crate::generators::natural::foundation::tectonics::model::TectonicState| {
                let cross_kind = surface
                    .edges()
                    .iter()
                    .filter(|edge| {
                        let [first, second] = edge
                            .cells
                            .map(|cell| state.samples[cell.raw() as usize].kind);
                        first != second
                    })
                    .count();
                cross_kind as f64 / surface.edges().len() as f64
            };
        assert!(boundary_fraction(&supercontinent) < boundary_fraction(&archipelago));
    }

    #[test]
    fn evolved_initial_domains_are_connected_deterministic_and_not_nearest_center_voronoi() {
        let (surface, topology) = fixture(642);
        let spec = TectonicSpec {
            plate_count: 12,
            continental_crust_fraction: 0.38,
            ..TectonicSpec::default()
        };
        let first = build_initial_state_v5(
            &surface,
            &topology,
            &spec,
            ResolvedWorldFormationPreset::Continents,
            &streams(42),
        )
        .unwrap();
        let repeated = build_initial_state_v5(
            &surface,
            &topology,
            &spec,
            ResolvedWorldFormationPreset::Continents,
            &streams(42),
        )
        .unwrap();
        let legacy = build_initial_state(
            &surface,
            &topology,
            &spec,
            ResolvedWorldFormationPreset::Continents,
            &streams(42),
        )
        .unwrap();
        let owners = first.initial_owners();

        assert_eq!(owners, repeated.initial_owners());
        assert_ne!(owners, legacy.initial_owners());
        assert!(
            owners
                .iter()
                .zip(legacy.initial_owners())
                .filter(|(first, second)| **first != *second)
                .count()
                > surface.cells().len() / 50
        );
        for plate in &first.plates {
            let expected = owners
                .iter()
                .filter(|&&owner| owner == plate.lineage)
                .count();
            assert_eq!(
                connected_owner_count(&topology, &owners, plate.lineage),
                expected
            );
            assert!(expected > 0);
        }
    }

    #[test]
    fn opening_presets_hit_area_mix_plates_and_stay_distinguishable() {
        use ResolvedWorldFormationPreset::{
            Archipelago, Continents, GreatIsland, Supercontinent, VolcanicIslands,
        };

        let (surface, topology) = fixture(642);
        let maximum_cell_area = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .fold(0.0, f64::max);
        let cases = [
            Continents,
            Archipelago,
            Supercontinent,
            GreatIsland,
            VolcanicIslands,
        ];
        for seed in [42_u64, 3] {
            let mut masks = Vec::new();
            for preset in cases {
                let target_fraction = preset.recommended_continental_crust_fraction();
                let spec = TectonicSpec {
                    continental_crust_fraction: target_fraction,
                    ..TectonicSpec::default()
                };
                let state =
                    build_initial_state_v5(&surface, &topology, &spec, preset, &streams(seed))
                        .unwrap();
                let target_area = surface.total_cell_area().get() * f64::from(target_fraction);
                assert!(
                    (continental_area(&surface, &state) - target_area).abs() <= maximum_cell_area,
                    "seed {seed}, {preset:?}"
                );
                assert!(
                    mixed_plate_count(&state) > 0,
                    "seed {seed}, {preset:?} has no mixed plates"
                );
                if preset == Supercontinent {
                    assert!(
                        has_cross_plate_continental_edge(&surface, &state),
                        "seed {seed}: supercontinent crust stayed inside one plate"
                    );
                }
                masks.push(
                    state
                        .samples
                        .iter()
                        .map(|sample| sample.kind)
                        .collect::<Vec<_>>(),
                );
            }
            assert!(masks.windows(2).any(|pair| pair[0] != pair[1]));
        }
    }

    #[test]
    fn v5_opening_marks_dominant_lineages_on_the_largest_continental_component() {
        let (surface, topology) = fixture(642);
        for preset in [
            ResolvedWorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Supercontinent,
            ResolvedWorldFormationPreset::GreatIsland,
        ] {
            let state = build_initial_state_v5(
                &surface,
                &topology,
                &TectonicSpec::default(),
                preset,
                &streams(42),
            )
            .unwrap();
            assert!(
                state.samples.iter().any(|sample| {
                    sample.kind == CrustKind::Continental
                        && state.initiation.is_dominant(sample.owner)
                }),
                "{preset:?} opening did not tag the largest continental mass"
            );
        }
    }

    #[test]
    fn archipelago_opening_tags_continental_plates_as_opening_phase() {
        let (surface, topology) = fixture(642);
        let archipelago = build_initial_state_v5(
            &surface,
            &topology,
            &TectonicSpec {
                plate_count: 22,
                continental_crust_fraction: ResolvedWorldFormationPreset::Archipelago
                    .recommended_continental_crust_fraction(),
                ..TectonicSpec::default()
            },
            ResolvedWorldFormationPreset::Archipelago,
            &streams(42),
        )
        .unwrap();
        let continents = build_initial_state_v5(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
            &streams(42),
        )
        .unwrap();

        let mut tagged = 0_usize;
        let mut untagged_oceanic = 0_usize;
        for plate in &archipelago.plates {
            let carries_continent = archipelago.samples.iter().any(|sample| {
                sample.owner == plate.lineage && sample.kind == CrustKind::Continental
            });
            if carries_continent {
                tagged += 1;
                assert!(
                    archipelago.initiation.is_opening_phase(plate.lineage),
                    "Archipelago continental plate {:?} must be opening-phase",
                    plate.lineage
                );
            } else {
                untagged_oceanic += 1;
                assert!(
                    !archipelago.initiation.is_opening_phase(plate.lineage),
                    "purely oceanic plate {:?} must not be opening-phase",
                    plate.lineage
                );
            }
        }
        assert!(
            tagged >= 2,
            "Archipelago opening needs several continental plates, tagged={tagged}"
        );
        assert!(
            untagged_oceanic >= 1,
            "22-plate Archipelago should leave oceanic plates untagged, untagged={untagged_oceanic}"
        );
        assert!(
            continents
                .samples
                .iter()
                .all(|sample| !continents.initiation.is_opening_phase(sample.owner)),
            "Continents opening must not tag Atlantic-phase seaways"
        );
    }

    #[test]
    fn authored_continental_fraction_nests_for_two_seeds() {
        let (surface, topology) = fixture(642);
        let fractions = [0.20_f32, 0.38, 0.55];
        let maximum_cell_area = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .fold(0.0, f64::max);

        for seed in [42_u64, 3] {
            let states = fractions.map(|requested| {
                let state = build_initial_state_v5(
                    &surface,
                    &topology,
                    &TectonicSpec {
                        continental_crust_fraction: requested,
                        ..TectonicSpec::default()
                    },
                    ResolvedWorldFormationPreset::Continents,
                    &streams(seed),
                )
                .unwrap();
                let target_area = surface.total_cell_area().get() * f64::from(requested);
                assert!(
                    (continental_area(&surface, &state) - target_area).abs() <= maximum_cell_area,
                    "seed {seed}, request {requested}"
                );
                state
            });
            assert_eq!(states[0].initial_owners(), states[1].initial_owners());
            assert_eq!(states[1].initial_owners(), states[2].initial_owners());
            let masks = states.map(|state| {
                state
                    .samples
                    .iter()
                    .map(|sample| sample.kind == CrustKind::Continental)
                    .collect::<Vec<_>>()
            });
            for (index, ((low, middle), high)) in
                masks[0].iter().zip(&masks[1]).zip(&masks[2]).enumerate()
            {
                assert!(
                    !*low || *middle,
                    "seed {seed}: 20% continental cell {index} disappeared at 38%"
                );
                assert!(
                    !*middle || *high,
                    "seed {seed}: 38% continental cell {index} disappeared at 55%"
                );
            }
        }
    }

    #[test]
    fn changing_plate_count_changes_opening_crust_kinds() {
        let (surface, topology) = fixture(642);
        let twelve = build_initial_state_v5(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
            &streams(42),
        )
        .unwrap();
        let seventeen = build_initial_state_v5(
            &surface,
            &topology,
            &TectonicSpec {
                plate_count: 17,
                ..TectonicSpec::default()
            },
            ResolvedWorldFormationPreset::Continents,
            &streams(42),
        )
        .unwrap();
        assert_ne!(twelve.initial_owners(), seventeen.initial_owners());
        assert_ne!(
            twelve
                .samples
                .iter()
                .map(|sample| sample.kind)
                .collect::<Vec<_>>(),
            seventeen
                .samples
                .iter()
                .map(|sample| sample.kind)
                .collect::<Vec<_>>()
        );
    }
}
