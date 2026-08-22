//! PlaTec-style coherent initial lithosphere on the authoritative sphere.
//!
//! This is only the start state for the bounded Cortial evolution. Stable
//! farthest-point seeds define a perturbed spherical Voronoi partition, while
//! independent coherent fields initialize material, thickness and ocean age.
//! No connectivity trimming, region growth or final-land shaping occurs here.

#![cfg_attr(not(test), allow(dead_code))]

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use rand::RngCore;
use thiserror::Error;

use super::model::{
    ActivePlate, CrustSample, FormationTectonicRecipe, LineageId, MaterialColumn,
    TectonicModelError, TectonicState,
};
use crate::generators::natural::fractal::FractalProfile;
use crate::generators::natural::morphology::noise::SphericalNoise3d;
use crate::generators::natural::random::{
    LabeledSubstreams, INITIAL_CRUST_V3_LABEL, INITIAL_DOMAINS_V5_LABEL, INITIAL_PLATES_V3_LABEL,
    PLATE_MOTION_V3_LABEL,
};
use crate::generators::natural::spherical_crust_physics::{
    continental_isostatic_elevation_m, oceanic_plate_cooling_elevation_m,
};
use crate::generators::natural::topology::{
    farthest_point_seeds, multi_source_distance, NaturalTopologyIndex,
};
use crate::world::natural::{
    CrustKind, NaturalSpecError, SphericalOrogenyKind, SphericalPlateRotation,
    SphericalTectonicValidationError, TectonicActivity, TectonicSpec,
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
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<TectonicState, InitialStateError> {
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
    let samples = initial_crust_samples(surface, spec, recipe, streams, &owners);
    TectonicState::new(samples, plates, spec.plate_count.into()).map_err(Into::into)
}

pub(super) fn build_initial_state_v5(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<TectonicState, InitialStateError> {
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
    let samples = initial_crust_samples(surface, spec, recipe, streams, &owners);
    TectonicState::new(samples, plates, spec.plate_count.into()).map_err(Into::into)
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
    spec: &TectonicSpec,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
    owners: &[LineageId],
) -> Vec<CrustSample> {
    let mut rng = streams.stream(INITIAL_CRUST_V3_LABEL);
    let crust_noise = SphericalNoise3d::new(rng.next_u32());
    let thickness_noise = SphericalNoise3d::new(rng.next_u32());
    let age_noise = SphericalNoise3d::new(rng.next_u32());
    let scores = surface
        .cells()
        .iter()
        .map(|cell| crust_noise.fbm(cell.centroid, recipe.initial_crust_profile))
        .collect::<Vec<_>>();
    let continental = continental_quantile(surface, &scores, spec.continental_crust_fraction);
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

fn continental_quantile(
    surface: &SphericalSurfaceSnapshot,
    scores: &[f64],
    target_fraction: f32,
) -> Vec<bool> {
    let mut order = (0..scores.len()).collect::<Vec<_>>();
    order.sort_by(|&first, &second| {
        scores[second]
            .total_cmp(&scores[first])
            .then_with(|| first.cmp(&second))
    });
    let target = surface.total_cell_area().get() * f64::from(target_fraction);
    let mut selected_count = 0;
    let mut selected_area = 0.0;
    for (rank, &index) in order.iter().enumerate() {
        let next_area = selected_area + surface.cells()[index].area.get();
        if (next_area - target).abs() <= (selected_area - target).abs() {
            selected_area = next_area;
            selected_count = rank + 1;
        } else {
            break;
        }
    }
    let mut selected = vec![false; scores.len()];
    for &index in order.iter().take(selected_count) {
        selected[index] = true;
    }
    selected
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
    use std::collections::{BTreeMap, VecDeque};

    use super::{build_initial_state, build_initial_state_v5};
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::spherical_tectonics::model::FormationTectonicRecipe;
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
        owners: &[crate::generators::natural::spherical_tectonics::model::LineageId],
        owner: crate::generators::natural::spherical_tectonics::model::LineageId,
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

    #[test]
    fn initial_continental_inventory_reproduces_the_platform_table() {
        let (surface, topology) = fixture(642);
        let spec = TectonicSpec::default();
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let state =
            build_initial_state_v5(&surface, &topology, &spec, recipe, &streams(42)).unwrap();
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
            let recipe =
                FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
            let state =
                build_initial_state(&surface, &topology, &spec, recipe, &streams(71)).unwrap();
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

            let continental_area = surface
                .cells()
                .iter()
                .zip(&state.samples)
                .filter(|(_, sample)| sample.kind == CrustKind::Continental)
                .map(|(cell, _)| cell.area.get())
                .sum::<f64>();
            let target =
                surface.total_cell_area().get() * f64::from(spec.continental_crust_fraction);
            let max_cell_area = surface
                .cells()
                .iter()
                .map(|cell| cell.area.get())
                .max_by(f64::total_cmp)
                .unwrap();
            assert!((continental_area - target).abs() <= max_cell_area);
        }
    }

    #[test]
    fn initial_state_is_seeded_and_formation_spectrum_changes_coherence() {
        let (surface, topology) = fixture(642);
        let spec = TectonicSpec::default();
        let build = |seed, preset| {
            build_initial_state(
                &surface,
                &topology,
                &spec,
                FormationTectonicRecipe::for_preset(preset),
                &streams(seed),
            )
            .unwrap()
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
        let boundary_fraction =
            |state: &crate::generators::natural::spherical_tectonics::model::TectonicState| {
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
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let first =
            build_initial_state_v5(&surface, &topology, &spec, recipe, &streams(42)).unwrap();
        let repeated =
            build_initial_state_v5(&surface, &topology, &spec, recipe, &streams(42)).unwrap();
        let legacy = build_initial_state(&surface, &topology, &spec, recipe, &streams(42)).unwrap();
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
    fn initial_formation_matrix_preserves_area_and_orders_coherent_scale() {
        use ResolvedWorldFormationPreset::{
            Archipelago, Continents, GreatIsland, Supercontinent, VolcanicIslands,
        };

        let (surface, topology) = fixture(642);
        let seeds = [
            42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
        ];
        let cases = [
            (Continents, 0.38_f32),
            (Archipelago, 0.26),
            (Supercontinent, 0.42),
            (GreatIsland, 0.28),
            (VolcanicIslands, 0.16),
        ];
        let maximum_cell_area = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .fold(0.0, f64::max);
        let total_edge_length = surface
            .edges()
            .iter()
            .map(|edge| edge.length.get())
            .sum::<f64>();
        let mut boundary_fractions = BTreeMap::<_, Vec<f64>>::new();

        for seed in seeds {
            for (preset, target_fraction) in cases {
                let spec = TectonicSpec {
                    continental_crust_fraction: target_fraction,
                    ..TectonicSpec::default()
                };
                let state = build_initial_state(
                    &surface,
                    &topology,
                    &spec,
                    FormationTectonicRecipe::for_preset(preset),
                    &streams(seed),
                )
                .unwrap();
                let continental_area = surface
                    .cells()
                    .iter()
                    .zip(&state.samples)
                    .filter(|(_, sample)| sample.kind == CrustKind::Continental)
                    .map(|(cell, _)| cell.area.get())
                    .sum::<f64>();
                let target_area = surface.total_cell_area().get() * f64::from(target_fraction);
                assert!(
                    (continental_area - target_area).abs() <= maximum_cell_area,
                    "seed {seed}, {preset:?}: {continental_area} vs {target_area}"
                );
                let cross_kind_length = surface
                    .edges()
                    .iter()
                    .filter(|edge| {
                        let [first, second] = edge
                            .cells
                            .map(|cell| state.samples[cell.raw() as usize].kind);
                        first != second
                    })
                    .map(|edge| edge.length.get())
                    .sum::<f64>();
                boundary_fractions
                    .entry(preset)
                    .or_default()
                    .push(cross_kind_length / total_edge_length);
            }
        }

        let mean = |preset| {
            let values = &boundary_fractions[&preset];
            values.iter().sum::<f64>() / values.len() as f64
        };
        let large_scale = (mean(Supercontinent) + mean(GreatIsland)) * 0.5;
        let island_scale = (mean(Archipelago) + mean(VolcanicIslands)) * 0.5;
        assert!(large_scale < island_scale, "{boundary_fractions:?}");
        assert!(
            mean(Continents) < mean(Archipelago),
            "{boundary_fractions:?}"
        );
    }

    #[test]
    fn authored_continental_fraction_hits_area_and_nests_masks_for_17_seeds() {
        let (surface, topology) = fixture(642);
        let seeds = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 42];
        let fractions = [0.20_f32, 0.38, 0.55];
        let maximum_cell_area = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .fold(0.0, f64::max);

        for seed in seeds {
            let masks = fractions.map(|requested| {
                let state = build_initial_state(
                    &surface,
                    &topology,
                    &TectonicSpec {
                        continental_crust_fraction: requested,
                        ..TectonicSpec::default()
                    },
                    FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
                    &streams(seed),
                )
                .unwrap();
                let actual_area = surface
                    .cells()
                    .iter()
                    .zip(&state.samples)
                    .filter(|(_, sample)| sample.kind == CrustKind::Continental)
                    .map(|(cell, _)| cell.area.get())
                    .sum::<f64>();
                let target_area = surface.total_cell_area().get() * f64::from(requested);
                assert!(
                    (actual_area - target_area).abs() <= maximum_cell_area,
                    "seed {seed}, request {requested}: {actual_area} vs {target_area}"
                );
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
}
