//! Deterministic Poisson rifting and perturbed spherical fractures.
//!
//! Cortial's area/continental-fraction Poisson model selects events. A fixed
//! point [3/3] Padé approximation avoids platform `exp` on state-changing
//! branches. Selected plate samples are split by a bounded, coherent perturbation
//! of spherical Voronoi distance and receive stable, never-reused lineages.

use super::{ProcessActions, ProcessError, ProcessStats};
use crate::generators::natural::fractal::FractalProfile;
use crate::generators::natural::morphology::noise::SphericalNoise3d;
use crate::generators::natural::random::{LabeledSubstreams, RIFT_EVENTS_V3_LABEL};
use crate::generators::natural::spherical_tectonics::model::{
    ActivePlate, FormationTectonicRecipe, LineageId, TectonicState,
};
use crate::world::natural::{CrustKind, SphericalPlateRotation, MAX_PLATE_COUNT};
use crate::world::spatial::{
    canonical_east_north_basis, project_tangent, SphericalSurfaceSnapshot, UnitVector3,
};

pub(super) const MAX_RIFT_RATE_PPM_PER_MYR: u32 = 10_000;
const BASE_RIFT_RATE_PPM_PER_MYR: f64 = 4_000.0;
const RIFT_WARP_AMPLITUDE: f64 = 0.025;
const MINIMUM_SAMPLES_PER_RIFT_CHILD: usize = 4;
// Disconnected accreted terranes become separate published plates. Keep half
// of the schema capacity in reserve for that final, factual split.
const MAXIMUM_RIFTED_LINEAGES: usize = MAX_PLATE_COUNT as usize;

pub(super) fn rate_q32_from_ppm(parts_per_million: u32) -> u64 {
    let numerator = u128::from(parts_per_million) * (1_u128 << 32) + 500_000;
    (numerator / 1_000_000) as u64
}

pub(super) fn poisson_threshold_q64(rate_q32_per_myr: u64, delta_myr: u16) -> u128 {
    let bounded_rate = rate_q32_per_myr.min(rate_q32_from_ppm(MAX_RIFT_RATE_PPM_PER_MYR));
    let x = u128::from(bounded_rate) * u128::from(delta_myr);
    if x == 0 {
        return 0;
    }
    let q32 = 1_u128 << 32;
    let q64 = 1_u128 << 64;
    let q96 = 1_u128 << 96;
    let x_squared = x * x;
    let x_cubed = x_squared * x;
    let numerator = 120 * x * q64 + 2 * x_cubed;
    let denominator = 120 * q96 + 60 * x * q64 + 12 * x_squared * q32 + x_cubed;
    ratio_to_q64(numerator, denominator)
}

pub(super) fn poisson_event(draw: u64, rate_q32_per_myr: u64, delta_myr: u16) -> bool {
    u128::from(draw) < poisson_threshold_q64(rate_q32_per_myr, delta_myr)
}

fn ratio_to_q64(numerator: u128, denominator: u128) -> u128 {
    let mut remainder = numerator;
    let mut quotient = 0_u128;
    for _ in 0..64 {
        remainder <<= 1;
        quotient <<= 1;
        if remainder >= denominator {
            remainder -= denominator;
            quotient |= 1;
        }
    }
    quotient
}

fn rift_rate_q32_with_scratch(
    surface: &SphericalSurfaceSnapshot,
    state: &TectonicState,
    lineage: LineageId,
    recipe: FormationTectonicRecipe,
    represented: &mut [u8],
) -> Result<u64, ProcessError> {
    if state.plate(lineage).is_none() {
        return Err(ProcessError::UnknownLineage { lineage });
    }
    debug_assert_eq!(represented.len(), surface.cells().len());
    represented.fill(0);
    let mut area = 0.0;
    let mut continental_area = 0.0;
    for (sample_index, sample) in state.samples.iter().enumerate() {
        if sample.owner != lineage {
            continue;
        }
        let cell_index = sample.anchor.raw() as usize;
        if cell_index >= represented.len() {
            return Err(ProcessError::InvalidAnchor {
                sample: sample_index,
                anchor: sample.anchor,
                cells: represented.len(),
            });
        }
        if represented[cell_index] == 0 {
            represented[cell_index] = 1;
            let cell_area = surface.cells()[cell_index].area.get();
            area += cell_area;
            if sample.kind == CrustKind::Continental {
                continental_area += cell_area;
            }
        }
    }
    if area <= 0.0 {
        return Ok(0);
    }
    let average_plate_area = surface.total_cell_area().get() / state.plates.len() as f64;
    let continental_fraction = continental_area / area;
    let area_factor = (area / average_plate_area).clamp(0.0, 2.0);
    let recipe_gain = f64::from(recipe.rift_rate_permille) / 1_000.0;
    let ppm = (BASE_RIFT_RATE_PPM_PER_MYR
        * recipe_gain
        * continental_fraction
        * continental_fraction
        * area_factor)
        .round()
        .clamp(0.0, f64::from(MAX_RIFT_RATE_PPM_PER_MYR)) as u32;
    Ok(rate_q32_from_ppm(ppm))
}

#[cfg(test)]
pub(super) fn rift_rate_q32(
    surface: &SphericalSurfaceSnapshot,
    state: &TectonicState,
    lineage: LineageId,
    recipe: FormationTectonicRecipe,
) -> Result<u64, ProcessError> {
    let mut represented = vec![0; surface.cells().len()];
    rift_rate_q32_with_scratch(surface, state, lineage, recipe, &mut represented)
}

pub(in crate::generators::natural::spherical_tectonics) fn maybe_rift_plates(
    step: u16,
    surface: &SphericalSurfaceSnapshot,
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
) -> Result<ProcessStats, ProcessError> {
    if current.samples.len() != next.samples.len() {
        return Err(ProcessError::StateCardinalityMismatch {
            current: current.samples.len(),
            next: next.samples.len(),
        });
    }
    actions.validate_for(next.samples.len())?;
    if next.plates.len() >= MAXIMUM_RIFTED_LINEAGES {
        return Ok(ProcessStats::default());
    }

    let mut stats = ProcessStats::default();
    let parents = current.plates.clone();
    for parent in parents {
        if next.plates.len() >= MAXIMUM_RIFTED_LINEAGES {
            break;
        }
        if actions.lineage_has_pending_changes(&current.samples, parent.lineage) {
            continue;
        }
        let rate = {
            let represented = actions.rift_scratch(surface.cells().len());
            rift_rate_q32_with_scratch(surface, current, parent.lineage, recipe, represented)?
        };
        let draw = streams.counter_u64(
            RIFT_EVENTS_V3_LABEL,
            &[u64::from(step), u64::from(parent.lineage.raw())],
        );
        if !poisson_event(draw, rate, 2) {
            continue;
        }
        let sample_indices = current
            .samples
            .iter()
            .enumerate()
            .filter(|(_, sample)| sample.owner == parent.lineage)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if sample_indices.len() < 2 * MINIMUM_SAMPLES_PER_RIFT_CHILD {
            continue;
        }
        let desired_children = 2
            + (streams.counter_u64(
                RIFT_EVENTS_V3_LABEL,
                &[u64::from(step), u64::from(parent.lineage.raw()), 1],
            ) % 3) as usize;
        let capacity_children = MAXIMUM_RIFTED_LINEAGES - next.plates.len() + 1;
        let child_count = desired_children
            .min(sample_indices.len() / MINIMUM_SAMPLES_PER_RIFT_CHILD)
            .min(capacity_children);
        if child_count < 2 {
            continue;
        }
        if !split_plate(
            step,
            surface,
            current,
            next,
            actions,
            recipe,
            streams,
            parent,
            &sample_indices,
            child_count,
        )? {
            continue;
        }
        stats.rift_events += 1;
        stats.spawned_lineages += child_count as u32;
        stats.transferred_samples += sample_indices.len() as u32;
    }
    Ok(stats)
}

#[allow(clippy::too_many_arguments)]
fn split_plate(
    step: u16,
    surface: &SphericalSurfaceSnapshot,
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
    streams: &LabeledSubstreams,
    parent: ActivePlate,
    sample_indices: &[usize],
    child_count: usize,
) -> Result<bool, ProcessError> {
    let seed_indices = farthest_sample_seeds(
        current,
        sample_indices,
        child_count,
        streams.counter_u64(
            RIFT_EVENTS_V3_LABEL,
            &[u64::from(step), u64::from(parent.lineage.raw()), 2],
        ),
    );
    let profile = FractalProfile {
        octaves: 2,
        frequency: recipe.base_scale_rad.recip(),
        lacunarity: 2.03,
        persistence: 0.5,
    };
    let noise = (0..child_count)
        .map(|child| {
            SphericalNoise3d::new(streams.counter_u64(
                RIFT_EVENTS_V3_LABEL,
                &[
                    u64::from(step),
                    u64::from(parent.lineage.raw()),
                    3 + child as u64,
                ],
            ) as u32)
        })
        .collect::<Vec<_>>();
    let mut assignments = Vec::with_capacity(sample_indices.len());
    let mut child_sizes = vec![0_usize; child_count];
    for &sample_index in sample_indices {
        let position = current.samples[sample_index].position;
        let forced_child = seed_indices.iter().position(|&seed| seed == sample_index);
        let child = forced_child.unwrap_or_else(|| {
            seed_indices
                .iter()
                .enumerate()
                .map(|(child, &seed)| {
                    let center = current.samples[seed].position;
                    let score = position.dot(center)
                        + RIFT_WARP_AMPLITUDE * noise[child].fbm(position, profile);
                    (child, score)
                })
                .max_by(|first, second| {
                    first
                        .1
                        .total_cmp(&second.1)
                        .then_with(|| second.0.cmp(&first.0))
                })
                .expect("a rift has at least two child seeds")
                .0
        });
        assignments.push(child);
        child_sizes[child] += 1;
    }
    if child_sizes
        .iter()
        .any(|&size| size < MINIMUM_SAMPLES_PER_RIFT_CHILD)
    {
        return Ok(false);
    }

    let mut lineages = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        lineages.push(
            next.allocate_lineage()
                .ok_or(ProcessError::LineageExhausted)?,
        );
    }
    for (&sample_index, child) in sample_indices.iter().zip(assignments) {
        actions.mark_transfer(sample_index, lineages[child])?;
    }

    next.plates.retain(|plate| plate.lineage != parent.lineage);
    for (child, (&lineage, &seed_index)) in lineages.iter().zip(&seed_indices).enumerate() {
        let seed = current.samples[seed_index];
        let rotation = divergent_rotation(parent.rotation, seed.position, child, child_count)?;
        rotation.validate_for_radius(surface.radius())?;
        next.plates
            .push(ActivePlate::new(lineage, seed.anchor, rotation));
    }
    next.plates.sort_by_key(|plate| plate.lineage);
    Ok(true)
}

fn farthest_sample_seeds(
    state: &TectonicState,
    sample_indices: &[usize],
    count: usize,
    draw: u64,
) -> Vec<usize> {
    let mut seeds = vec![sample_indices[draw as usize % sample_indices.len()]];
    while seeds.len() < count {
        let next = sample_indices
            .iter()
            .copied()
            .filter(|candidate| !seeds.contains(candidate))
            .max_by(|&first, &second| {
                minimum_seed_distance(state, first, &seeds)
                    .total_cmp(&minimum_seed_distance(state, second, &seeds))
                    .then_with(|| second.cmp(&first))
            })
            .expect("child count never exceeds the parent sample count");
        seeds.push(next);
    }
    seeds
}

fn minimum_seed_distance(state: &TectonicState, sample: usize, seeds: &[usize]) -> f64 {
    seeds
        .iter()
        .map(|&seed| {
            1.0 - state.samples[sample]
                .position
                .dot(state.samples[seed].position)
        })
        .min_by(f64::total_cmp)
        .expect("the rift seed set is non-empty")
}

fn divergent_rotation(
    parent: SphericalPlateRotation,
    seed: UnitVector3,
    child: usize,
    child_count: usize,
) -> Result<SphericalPlateRotation, ProcessError> {
    let pole = parent.pole();
    let mut tangent = project_tangent(seed.components(), pole);
    let tangent_norm = norm(tangent);
    if tangent_norm <= f64::EPSILON {
        tangent = canonical_east_north_basis(pole).0;
    } else {
        tangent = tangent.map(|component| component / tangent_norm);
    }
    let centered = child as f64 - (child_count - 1) as f64 * 0.5;
    let angle = 0.08 * centered;
    let pole_components = pole.components();
    let divergent_pole = UnitVector3::new(
        angle.cos() * pole_components[0] + angle.sin() * tangent[0],
        angle.cos() * pole_components[1] + angle.sin() * tangent[1],
        angle.cos() * pole_components[2] + angle.sin() * tangent[2],
    )?;
    Ok(SphericalPlateRotation::new(
        divergent_pole,
        parent.angular_rate_prad_per_year(),
    )?)
}

fn norm(vector: [f64; 3]) -> f64 {
    vector
        .into_iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::{
        maybe_rift_plates, poisson_event, poisson_threshold_q64, rate_q32_from_ppm, rift_rate_q32,
        MAX_RIFT_RATE_PPM_PER_MYR,
    };
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::random::{LabeledSubstreams, RIFT_EVENTS_V3_LABEL};
    use crate::generators::natural::spherical_tectonics::model::{
        ActivePlate, CrustSample, FormationTectonicRecipe, LineageId, TectonicState,
    };
    use crate::generators::natural::spherical_tectonics::processes::{
        commit_process_actions, ProcessActions,
    };
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, ResolvedWorldFormationPreset, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, MAX_PLATE_COUNT, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    fn surface(cells: u32) -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: cells,
        })
        .unwrap()
    }

    fn streams() -> LabeledSubstreams {
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(97),
            StageIdentity::new("rifting-test", 1, "sekai.test"),
        ));
        LabeledSubstreams::capture(&mut rng)
    }

    fn state(surface: &SphericalSurfaceSnapshot, plate_count: usize) -> TectonicState {
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let plates = (0..plate_count)
            .map(|index| {
                ActivePlate::new(
                    LineageId::from_raw(index as u32),
                    CellId::from_raw(index as u32),
                    rotation,
                )
            })
            .collect::<Vec<_>>();
        let samples = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let plate_index = if plate_count == 1 || index < surface.cells().len() * 3 / 4 {
                    0
                } else {
                    1 + index % (plate_count - 1)
                };
                CrustSample {
                    position: cell.site,
                    anchor: cell.id,
                    owner: LineageId::from_raw(plate_index as u32),
                    kind: if plate_index == 0 {
                        CrustKind::Continental
                    } else {
                        CrustKind::Oceanic
                    },
                    thickness_km: if plate_index == 0 { 40.0 } else { 7.0 },
                    age_myr: if plate_index == 0 {
                        CONTINENTAL_CRUST_AGE_SENTINEL_MYR
                    } else {
                        60.0
                    },
                    tectonic_elevation_m: if plate_index == 0 { 800.0 } else { -4_000.0 },
                    lineation: [0.0; 2],
                    orogeny: SphericalOrogenyKind::None,
                    orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
                }
            })
            .collect();
        TectonicState::new(samples, plates, plate_count as u32).unwrap()
    }

    fn copy_state(state: &TectonicState) -> TectonicState {
        TectonicState::new(
            state.samples.clone(),
            state.plates.clone(),
            state.next_lineage_raw(),
        )
        .unwrap()
    }

    #[test]
    fn fixed_point_pade_threshold_matches_complete_supported_rate_range() {
        let mut previous = 0_u128;
        for ppm in 0..=MAX_RIFT_RATE_PPM_PER_MYR {
            let rate = rate_q32_from_ppm(ppm);
            let threshold = poisson_threshold_q64(rate, 2);
            assert!(threshold >= previous);
            previous = threshold;
            let actual = threshold as f64 / 2.0_f64.powi(64);
            let lambda_delta = f64::from(ppm) / 1_000_000.0 * 2.0;
            let expected = 1.0 - (-lambda_delta).exp();
            assert!((actual - expected).abs() < 1.0 / 2.0_f64.powi(32));
        }
        let rate = rate_q32_from_ppm(4_000);
        assert!(!poisson_event(u64::MAX, rate, 2));
        assert!(poisson_event(0, rate, 2));
        assert_eq!(
            poisson_threshold_q64(u64::MAX, 2),
            poisson_threshold_q64(rate_q32_from_ppm(MAX_RIFT_RATE_PPM_PER_MYR), 2),
            "out-of-contract rates must clamp before fixed-point products"
        );
    }

    #[test]
    fn rift_is_counter_deterministic_divergent_bounded_and_never_reuses_lineages() {
        let surface = surface(162);
        let current = state(&surface, 2);
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let streams = streams();
        let rate = rift_rate_q32(&surface, &current, current.plates[0].lineage, recipe).unwrap();
        let step = (0..=u16::MAX)
            .find(|&step| {
                poisson_event(
                    streams.counter_u64(
                        RIFT_EVENTS_V3_LABEL,
                        &[u64::from(step), u64::from(current.plates[0].lineage.raw())],
                    ),
                    rate,
                    2,
                )
            })
            .expect("the supported deterministic rate must produce an event in the u16 domain");

        let run = || {
            let mut next = copy_state(&current);
            let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
            actions.begin_step(next.samples.len());
            let stats = maybe_rift_plates(
                step,
                &surface,
                &current,
                &mut next,
                &mut actions,
                recipe,
                &streams,
            )
            .unwrap();
            commit_process_actions(&mut next, &mut actions).unwrap();
            (next, stats)
        };
        let (first, first_stats) = run();
        let (second, second_stats) = run();
        assert_eq!(first_stats, second_stats);
        assert_eq!(first_stats.rift_events, 1);
        assert!((2..=4).contains(&first_stats.spawned_lineages));
        assert_eq!(first.samples, second.samples);
        assert_eq!(first.plates, second.plates);
        assert!(!first
            .plates
            .iter()
            .any(|plate| plate.lineage == LineageId::from_raw(0)));
        let children = first
            .plates
            .iter()
            .filter(|plate| plate.lineage.raw() >= 2)
            .collect::<Vec<_>>();
        assert_eq!(children.len() as u32, first_stats.spawned_lineages);
        assert!(children
            .windows(2)
            .all(|pair| pair[0].lineage < pair[1].lineage));
        assert!(children
            .windows(2)
            .any(|pair| pair[0].rotation != pair[1].rotation));
        assert!(first
            .samples
            .iter()
            .all(|sample| sample.owner != LineageId::from_raw(0)));
    }

    #[test]
    fn rifting_stops_at_the_global_plate_cap_before_drawing_or_allocating() {
        let surface = surface(162);
        let current = state(&surface, usize::from(MAX_PLATE_COUNT));
        let mut next = copy_state(&current);
        let before_samples = next.samples.clone();
        let before_plates = next.plates.clone();
        let before_lineage = next.next_lineage_raw();
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Archipelago);
        let streams = streams();
        let rate = rift_rate_q32(&surface, &current, current.plates[0].lineage, recipe).unwrap();
        let step = (0..=u16::MAX)
            .find(|&step| {
                poisson_event(
                    streams.counter_u64(
                        RIFT_EVENTS_V3_LABEL,
                        &[u64::from(step), u64::from(current.plates[0].lineage.raw())],
                    ),
                    rate,
                    2,
                )
            })
            .expect("the capacity fixture must exercise a would-be rift");
        let stats = maybe_rift_plates(
            step,
            &surface,
            &current,
            &mut next,
            &mut actions,
            recipe,
            &streams,
        )
        .unwrap();
        commit_process_actions(&mut next, &mut actions).unwrap();
        assert_eq!(stats.rift_events, 0);
        assert_eq!(next.samples, before_samples);
        assert_eq!(next.plates, before_plates);
        assert_eq!(next.next_lineage_raw(), before_lineage);
        assert_eq!(next.plates.len(), usize::from(MAX_PLATE_COUNT));
    }

    #[test]
    fn rifting_may_use_capacity_above_half_of_the_global_plate_cap() {
        let surface = surface(162);
        let current = state(&surface, usize::from(MAX_PLATE_COUNT) / 2 + 1);
        let mut next = copy_state(&current);
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Archipelago);
        let streams = streams();
        let rate = rift_rate_q32(&surface, &current, current.plates[0].lineage, recipe).unwrap();
        let step = (0..=u16::MAX)
            .find(|&step| {
                poisson_event(
                    streams.counter_u64(RIFT_EVENTS_V3_LABEL, &[u64::from(step), 0]),
                    rate,
                    2,
                )
            })
            .expect("the capacity fixture must exercise a rift above the half-cap");

        let stats = maybe_rift_plates(
            step,
            &surface,
            &current,
            &mut next,
            &mut actions,
            recipe,
            &streams,
        )
        .unwrap();
        commit_process_actions(&mut next, &mut actions).unwrap();

        assert_eq!(stats.rift_events, 1);
        assert!(next.plates.len() > current.plates.len());
        assert!(next.plates.len() <= usize::from(MAX_PLATE_COUNT));
    }

    #[test]
    fn rifting_requires_enough_area_for_every_child_plate() {
        let surface = surface(42);
        let mut current = state(&surface, 1);
        current.samples.truncate(7);
        let mut next = copy_state(&current);
        let before = next.samples.clone();
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let streams = streams();
        let rate = rift_rate_q32(&surface, &current, current.plates[0].lineage, recipe).unwrap();
        let step = (0..=u16::MAX)
            .find(|&step| {
                poisson_event(
                    streams.counter_u64(RIFT_EVENTS_V3_LABEL, &[u64::from(step), 0]),
                    rate,
                    2,
                )
            })
            .unwrap();

        let stats = maybe_rift_plates(
            step,
            &surface,
            &current,
            &mut next,
            &mut actions,
            recipe,
            &streams,
        )
        .unwrap();
        commit_process_actions(&mut next, &mut actions).unwrap();

        assert_eq!(stats.rift_events, 0);
        assert_eq!(next.samples, before);
        assert_eq!(next.plates.len(), 1);
    }

    #[test]
    fn rifting_does_not_delete_a_lineage_with_pending_process_actions() {
        let surface = surface(162);
        let current = state(&surface, 2);
        let mut next = copy_state(&current);
        let parent = current.plates[0].lineage;
        let incoming = current
            .samples
            .iter()
            .position(|sample| sample.owner != parent)
            .unwrap();
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());
        actions.mark_transfer(incoming, parent).unwrap();
        let mut spawned = next.samples[0];
        spawned.anchor = next.samples[incoming].anchor;
        actions.push_spawned(spawned);
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let streams = streams();
        let rate = rift_rate_q32(&surface, &current, parent, recipe).unwrap();
        let step = (0..=u16::MAX)
            .find(|&step| {
                poisson_event(
                    streams.counter_u64(
                        RIFT_EVENTS_V3_LABEL,
                        &[u64::from(step), u64::from(parent.raw())],
                    ),
                    rate,
                    2,
                )
            })
            .unwrap();

        let stats = maybe_rift_plates(
            step,
            &surface,
            &current,
            &mut next,
            &mut actions,
            recipe,
            &streams,
        )
        .unwrap();
        commit_process_actions(&mut next, &mut actions).unwrap();

        assert_eq!(stats.rift_events, 0);
        assert!(next.plate(parent).is_some());
        assert!(next
            .samples
            .iter()
            .all(|sample| next.plate(sample.owner).is_some()));
    }
}
