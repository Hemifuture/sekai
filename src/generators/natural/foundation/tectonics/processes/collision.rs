//! Cortial-style continental collision and terrane accretion.
//!
//! Connected continental samples are treated as terranes. Appendix-A area and
//! speed scaling controls a bounded Himalayan uplift; ownership transfer is a
//! one-shot action only after actual overlap, matching the paper's slab-break
//! and suturing event without retaining a transfer history.

use super::spreading::MAXIMUM_STEP_STRETCH_FACTOR;
use super::{
    bounded_elevation, constants, event_lineation, event_speed, ProcessActions, ProcessError,
    ProcessStats,
};
use crate::generators::natural::foundation::tectonics::contacts::{ContactEvent, ContactKind};
use crate::generators::natural::foundation::tectonics::model::{
    EvolutionMaterialLedger, FormationTectonicRecipe, TectonicState,
};
use crate::world::natural::{CrustKind, SphericalOrogenyKind, MAXIMUM_PLATE_AREA_FRACTION};
use crate::world::spatial::SphericalSurfaceSnapshot;

// Homogeneous pure-shear shortening expressed over one coarse orogenic belt,
// the inverse of the rift-zone extension (England & McKenzie 1982 thin-sheet
// thickening; Cortial et al. 2019 convergent thickening). The belt width is the
// same coarse scale as the rift zone so the two processes stay symmetric.
const CONTINENTAL_COLLISION_ZONE_WIDTH_M: f64 = 400_000.0;

pub(super) fn collision_uplift_m(
    terrane_area_m2: f64,
    speed_mm_per_year: f64,
    overlap: f64,
) -> f32 {
    if !terrane_area_m2.is_finite()
        || !speed_mm_per_year.is_finite()
        || !overlap.is_finite()
        || terrane_area_m2 <= 0.0
        || speed_mm_per_year <= 0.0
        || overlap <= 0.0
    {
        return 0.0;
    }
    let area_km2 = terrane_area_m2 / 1_000_000.0;
    let speed_weight = (speed_mm_per_year / constants::REFERENCE_PLATE_SPEED_MM_PER_YEAR)
        .clamp(0.0, 1.2)
        .sqrt();
    let overlap_weight = smoothstep(overlap.clamp(0.0, 1.0));
    (constants::COLLISION_COEFFICIENT_PER_KM * area_km2 * 1_000.0 * speed_weight * overlap_weight)
        .min(f64::from(constants::HIGHEST_CONTINENTAL_ELEVATION_M)) as f32
}

pub(super) fn should_force_terrane_subduction(
    terrane_area_m2: f64,
    average_plate_area_m2: f64,
) -> bool {
    terrane_area_m2.is_finite()
        && average_plate_area_m2.is_finite()
        && terrane_area_m2 > 0.0
        && terrane_area_m2
            < average_plate_area_m2 * constants::FORCED_SUBDUCTION_TERRANE_AREA_FRACTION
}

pub(in crate::generators::natural::foundation::tectonics) fn apply_collision(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
) -> Result<ProcessStats, ProcessError> {
    if current.samples.len() != next.samples.len() {
        return Err(ProcessError::StateCardinalityMismatch {
            current: current.samples.len(),
            next: next.samples.len(),
        });
    }
    actions.validate_for(next.samples.len())?;
    let average_plate_area = surface.total_cell_area().get() / next.plates.len() as f64;
    let gain = f64::from(recipe.subduction_gain_permille) / 1_000.0;
    let mut stats = ProcessStats::default();
    for event in events {
        if event.kind != ContactKind::ContinentalCollision {
            continue;
        }
        // Cortial collision is a discrete slab-break/suture event. A mere
        // convergent boundary may persist for many steps, but must not add the
        // same terrane-area surge every two million years before overlap.
        if event.overlap_depth < constants::COLLISION_TRANSFER_OVERLAP_DEPTH {
            continue;
        }
        let [first, second] = participant_indices(event, next.samples.len())?;
        if next.samples[first].kind != CrustKind::Continental
            || next.samples[second].kind != CrustKind::Continental
        {
            return Err(ProcessError::NonContinentalCollision);
        }
        let (first_area_m2, second_area_m2) = {
            let scratch = actions.terrane_scratch(surface.cells().len());
            let first_area_m2 = terrane_members(
                surface,
                next,
                first,
                &mut *scratch.represented,
                &mut *scratch.reached,
                &mut *scratch.stack,
                &mut *scratch.first_samples,
            )?;
            let second_area_m2 = terrane_members(
                surface,
                next,
                second,
                &mut *scratch.represented,
                &mut *scratch.reached,
                &mut *scratch.stack,
                &mut *scratch.second_samples,
            )?;
            (first_area_m2, second_area_m2)
        };
        let (moving_is_first, moving_area_m2, receiving) = if first_area_m2 < second_area_m2
            || (first_area_m2 == second_area_m2
                && next.samples[first].owner < next.samples[second].owner)
        {
            (true, first_area_m2, next.samples[second].owner)
        } else {
            (false, second_area_m2, next.samples[first].owner)
        };
        let forced = should_force_terrane_subduction(moving_area_m2, average_plate_area);
        let overlap = if event.overlap_depth == 0 {
            0.35
        } else {
            (0.35 + 0.25 * f64::from(event.overlap_depth)).min(1.0)
        };
        let uplift = collision_uplift_m(moving_area_m2, event_speed(event), overlap) * gain as f32;
        let first_lineation = event_lineation(surface, event, next.samples[first].position)?;
        let second_lineation = event_lineation(surface, event, next.samples[second].position)?;
        for (sample_index, lineation) in [(first, first_lineation), (second, second_lineation)] {
            next.samples[sample_index].tectonic_elevation_m =
                bounded_elevation(next.samples[sample_index].tectonic_elevation_m + uplift);
            next.samples[sample_index].orogeny = SphericalOrogenyKind::Himalayan;
            next.samples[sample_index].orogeny_age_myr = 0.0;
            next.samples[sample_index].lineation = lineation;
        }
        let transferred = actions.mark_terrane_transfer(moving_is_first, receiving)? as u32;
        stats.transferred_samples += transferred;
        stats.terrane_transfer_events += u32::from(transferred > 0);
        stats.collision_events += 1;
        stats.affected_samples += 2;
        stats.forced_subductions += u32::from(forced);
    }
    Ok(stats)
}

/// V5 collision retains the reference uplift/suture response but rejects an
/// ownership transfer that would breach the published 45% spherical-area cap.
pub(in crate::generators::natural::foundation::tectonics) fn apply_collision_v5(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
    ledger: &mut EvolutionMaterialLedger,
    delta_myr: f32,
) -> Result<ProcessStats, ProcessError> {
    if current.samples.len() != next.samples.len() {
        return Err(ProcessError::StateCardinalityMismatch {
            current: current.samples.len(),
            next: next.samples.len(),
        });
    }
    if !delta_myr.is_finite() || delta_myr < 0.0 {
        return Err(ProcessError::InvalidDeltaMyr { found: delta_myr });
    }
    actions.validate_for(next.samples.len())?;
    let average_plate_area = surface.total_cell_area().get() / next.plates.len() as f64;
    let maximum_plate_area = surface.total_cell_area().get() * MAXIMUM_PLATE_AREA_FRACTION;
    let gain = f64::from(recipe.subduction_gain_permille) / 1_000.0;
    let mut stats = ProcessStats::default();
    for event in events {
        if event.kind != ContactKind::ContinentalCollision
            || event.overlap_depth < constants::COLLISION_TRANSFER_OVERLAP_DEPTH
        {
            continue;
        }
        let [first, second] = participant_indices(event, next.samples.len())?;
        if next.samples[first].kind != CrustKind::Continental
            || next.samples[second].kind != CrustKind::Continental
        {
            return Err(ProcessError::NonContinentalCollision);
        }
        let convergence = (-event.signed_normal_speed_mm_per_year).max(0.0);
        actions.record_shortening_speed(first, convergence)?;
        actions.record_shortening_speed(second, convergence)?;
        let (first_area_m2, second_area_m2) = {
            let scratch = actions.terrane_scratch(surface.cells().len());
            let first_area_m2 = terrane_members(
                surface,
                next,
                first,
                &mut *scratch.represented,
                &mut *scratch.reached,
                &mut *scratch.stack,
                &mut *scratch.first_samples,
            )?;
            let second_area_m2 = terrane_members(
                surface,
                next,
                second,
                &mut *scratch.represented,
                &mut *scratch.reached,
                &mut *scratch.stack,
                &mut *scratch.second_samples,
            )?;
            (first_area_m2, second_area_m2)
        };
        let (moving_is_first, moving_area_m2, receiving) = if first_area_m2 < second_area_m2
            || (first_area_m2 == second_area_m2
                && next.samples[first].owner < next.samples[second].owner)
        {
            (true, first_area_m2, next.samples[second].owner)
        } else {
            (false, second_area_m2, next.samples[first].owner)
        };
        let forced = should_force_terrane_subduction(moving_area_m2, average_plate_area);
        let overlap = (0.35 + 0.25 * f64::from(event.overlap_depth)).min(1.0);
        let uplift = collision_uplift_m(moving_area_m2, event_speed(event), overlap) * gain as f32;
        let first_lineation = event_lineation(surface, event, next.samples[first].position)?;
        let second_lineation = event_lineation(surface, event, next.samples[second].position)?;
        for (sample_index, lineation) in [(first, first_lineation), (second, second_lineation)] {
            next.samples[sample_index].tectonic_elevation_m =
                bounded_elevation(next.samples[sample_index].tectonic_elevation_m + uplift);
            next.samples[sample_index].orogeny = SphericalOrogenyKind::Himalayan;
            next.samples[sample_index].orogeny_age_myr = 0.0;
            next.samples[sample_index].lineation = lineation;
        }
        let receiving_area = {
            let represented = actions.rift_scratch(surface.cells().len());
            lineage_occupied_area(surface, next, receiving, represented)?
        };
        if receiving_area + moving_area_m2 <= maximum_plate_area {
            let transferred = actions.mark_terrane_transfer(moving_is_first, receiving)? as u32;
            stats.transferred_samples += transferred;
            stats.terrane_transfer_events += u32::from(transferred > 0);
            stats.forced_subductions += u32::from(forced);
        }
        stats.collision_events += 1;
        stats.affected_samples += 2;
    }
    shorten_colliding_columns(next, actions, ledger, delta_myr)?;
    Ok(stats)
}

/// Thickens every continental column that converged this step by the bounded
/// pure-shear factor of its recorded convergence, retiring the lost reference
/// area into the ledger. Volume is preserved, so the crust root grows exactly as
/// the rift process thins it. The compatibility elevation is left alone: P3
/// derives the orogenic height from the thickened material through its Airy
/// column, so writing the same uplift into the inherited elevation would count
/// it twice.
fn shorten_colliding_columns(
    next: &mut TectonicState,
    actions: &ProcessActions,
    ledger: &mut EvolutionMaterialLedger,
    delta_myr: f32,
) -> Result<(), ProcessError> {
    for (sample, &speed) in next
        .samples
        .iter_mut()
        .zip(actions.shortening_speeds_mm_per_year())
    {
        if sample.material.continental_thickness_km().is_none() {
            continue;
        }
        if speed <= 0.0 {
            continue;
        }
        let shortening_m = f64::from(speed) * f64::from(delta_myr) * 1_000.0;
        let requested_beta = (1.0 + shortening_m / CONTINENTAL_COLLISION_ZONE_WIDTH_M)
            .clamp(1.0, MAXIMUM_STEP_STRETCH_FACTOR);
        let remaining_area_loss = ledger.remaining_collision_shortening_area_m2();
        if remaining_area_loss <= 0.0 {
            continue;
        }
        let area = sample.material.continental_reference_area_m2();
        let budget_beta = if remaining_area_loss < area {
            area / (area - remaining_area_loss)
        } else {
            f64::INFINITY
        };
        let beta = requested_beta.min(budget_beta);
        let (shortened, area_loss) = sample.material.shorten_continental_pure_shear(beta)?;
        if area_loss <= 0.0 {
            continue;
        }
        sample.material = shortened;
        sample.synchronize_compatibility_from_material();
        ledger.record_collision_shortening_area_loss(area_loss);
    }
    Ok(())
}

fn lineage_occupied_area(
    surface: &SphericalSurfaceSnapshot,
    state: &TectonicState,
    lineage: crate::generators::natural::foundation::tectonics::model::LineageId,
    represented: &mut [u8],
) -> Result<f64, ProcessError> {
    represented.fill(0);
    let mut area = 0.0;
    for (sample_index, sample) in state.samples.iter().enumerate() {
        if sample.owner != lineage {
            continue;
        }
        let cell = sample.anchor.raw() as usize;
        if cell >= represented.len() {
            return Err(ProcessError::InvalidAnchor {
                sample: sample_index,
                anchor: sample.anchor,
                cells: represented.len(),
            });
        }
        if represented[cell] == 0 {
            represented[cell] = 1;
            area += surface.cells()[cell].area.get();
        }
    }
    Ok(area)
}

fn terrane_members(
    surface: &SphericalSurfaceSnapshot,
    state: &TectonicState,
    root_sample: usize,
    represented: &mut [u8],
    reached: &mut [u8],
    stack: &mut Vec<crate::world::CellId>,
    sample_indices: &mut Vec<usize>,
) -> Result<f64, ProcessError> {
    let root = state
        .samples
        .get(root_sample)
        .ok_or(ProcessError::ContactSampleOutOfBounds {
            sample: root_sample,
            samples: state.samples.len(),
        })?;
    if root.kind != CrustKind::Continental {
        return Err(ProcessError::NonContinentalCollision);
    }
    debug_assert_eq!(represented.len(), surface.cells().len());
    debug_assert_eq!(reached.len(), surface.cells().len());
    represented.fill(0);
    reached.fill(0);
    stack.clear();
    sample_indices.clear();
    for sample in &state.samples {
        if sample.owner == root.owner && sample.kind == CrustKind::Continental {
            let anchor = sample.anchor.raw() as usize;
            if anchor >= represented.len() {
                return Err(ProcessError::UnknownCell {
                    cell: sample.anchor,
                });
            }
            represented[anchor] = 1;
        }
    }
    let root_cell = root.anchor.raw() as usize;
    if root_cell >= represented.len() || represented[root_cell] == 0 {
        return Err(ProcessError::EmptyTerrane {
            sample: root_sample,
        });
    }
    stack.push(root.anchor);
    reached[root_cell] = 1;
    while let Some(cell_id) = stack.pop() {
        let cell = surface
            .cell(cell_id)
            .ok_or(ProcessError::UnknownCell { cell: cell_id })?;
        for &edge_id in &cell.boundary_edges {
            let edge = surface
                .edge(edge_id)
                .ok_or(ProcessError::UnknownEdge { edge: edge_id })?;
            let neighbor = if edge.cells[0] == cell_id {
                edge.cells[1]
            } else {
                edge.cells[0]
            };
            let index = neighbor.raw() as usize;
            if represented[index] != 0 && reached[index] == 0 {
                reached[index] = 1;
                stack.push(neighbor);
            }
        }
    }
    let area_m2 = reached
        .iter()
        .enumerate()
        .filter(|(_, is_reached)| **is_reached != 0)
        .map(|(index, _)| surface.cells()[index].area.get())
        .sum();
    sample_indices.extend(
        state
            .samples
            .iter()
            .enumerate()
            .filter_map(|(index, sample)| {
                (sample.owner == root.owner
                    && sample.kind == CrustKind::Continental
                    && reached[sample.anchor.raw() as usize] != 0)
                    .then_some(index)
            }),
    );
    if sample_indices.is_empty() || area_m2 <= 0.0 {
        return Err(ProcessError::EmptyTerrane {
            sample: root_sample,
        });
    }
    Ok(area_m2)
}

fn participant_indices(
    event: &ContactEvent,
    sample_count: usize,
) -> Result<[usize; 2], ProcessError> {
    let [Some(first), Some(second)] = event.sample_indices else {
        return Err(ProcessError::MissingContactParticipants);
    };
    let indices = [first as usize, second as usize];
    for &sample in &indices {
        if sample >= sample_count {
            return Err(ProcessError::ContactSampleOutOfBounds {
                sample,
                samples: sample_count,
            });
        }
    }
    Ok(indices)
}

fn smoothstep(value: f64) -> f64 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_collision, apply_collision_v5, collision_uplift_m, should_force_terrane_subduction,
    };
    use crate::generators::natural::foundation::tectonics::contacts::{ContactEvent, ContactKind};
    use crate::generators::natural::foundation::tectonics::model::{
        ActivePlate, CrustSample, EvolutionMaterialLedger, FormationTectonicRecipe, LineageId,
        MaterialColumn, TectonicState,
    };
    use crate::generators::natural::foundation::tectonics::processes::{
        commit_process_actions, constants, ProcessActions,
    };
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, ResolvedWorldFormationPreset, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
    use crate::world::{CellId, Meters, SphericalSpaceSpec};

    fn surface() -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap()
    }

    fn continental_sample(
        surface: &SphericalSurfaceSnapshot,
        cell: CellId,
        owner: LineageId,
    ) -> CrustSample {
        CrustSample {
            position: surface.cell(cell).unwrap().site,
            anchor: cell,
            owner,
            kind: CrustKind::Continental,
            thickness_km: 40.0,
            age_myr: CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
            tectonic_elevation_m: 700.0,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(
                CrustKind::Continental,
                surface.cell(cell).unwrap().area.get(),
                40.0,
            )
            .unwrap(),
        }
    }

    #[test]
    fn appendix_a_collision_curve_scales_with_area_speed_and_overlap() {
        assert_eq!(constants::COLLISION_MAX_DISTANCE_M, 4_200_000.0);
        assert_eq!(constants::COLLISION_COEFFICIENT_PER_KM, 1.3e-5);
        assert_eq!(collision_uplift_m(1.0e12, 80.0, 0.0), 0.0);
        let base = collision_uplift_m(1.0e12, 40.0, 0.5);
        assert!(base > 0.0);
        assert!(collision_uplift_m(2.0e12, 40.0, 0.5) > base);
        assert!(collision_uplift_m(1.0e12, 80.0, 0.5) > base);
        assert!(collision_uplift_m(1.0e12, 40.0, 0.9) > base);
        assert!(should_force_terrane_subduction(0.1e12, 1.0e12));
        assert!(!should_force_terrane_subduction(0.4e12, 1.0e12));
    }

    #[test]
    fn collision_uplifts_himalayan_crust_and_transfers_only_after_overlap() {
        let surface = surface();
        let edge = &surface.edges()[0];
        let small = LineageId::from_raw(0);
        let large = LineageId::from_raw(1);
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let samples = vec![
            continental_sample(&surface, edge.cells[0], small),
            continental_sample(&surface, edge.cells[1], large),
        ];
        let plates = vec![
            ActivePlate::new(small, edge.cells[0], rotation),
            ActivePlate::new(large, edge.cells[1], rotation),
        ];
        let current = TectonicState::new(samples.clone(), plates.clone(), 2).unwrap();
        let material_before = current.material_totals().unwrap();
        let mut next = TectonicState::new(samples, plates, 2).unwrap();
        let event = ContactEvent {
            cell: edge.cells[0],
            edge: Some(edge.id),
            sample_indices: [Some(0), Some(1)],
            lineages: [Some(small), Some(large)],
            kind: ContactKind::ContinentalCollision,
            signed_normal_speed_mm_per_year: -90.0,
            tangent_speed_mm_per_year: 5.0,
            overlap_depth: 0,
        };
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let mut actions = ProcessActions::with_sample_capacity(2);
        actions.begin_step(2);
        let stats = apply_collision(
            &surface,
            std::slice::from_ref(&event),
            &current,
            &mut next,
            &mut actions,
            recipe,
        )
        .unwrap();
        assert!(next.samples.iter().all(|sample| {
            sample.tectonic_elevation_m == 700.0
                && sample.orogeny == SphericalOrogenyKind::None
                && sample.lineation == [0.0; 2]
        }));
        assert_eq!(stats.collision_events, 0);
        assert_eq!(stats.forced_subductions, 0);
        commit_process_actions(&mut next, &mut actions).unwrap();
        assert_eq!(
            next.samples[0].owner, small,
            "edge contact alone must not transfer"
        );

        let mut overlapped = TectonicState::new(
            current.samples.clone(),
            current.plates.clone(),
            current.next_lineage_raw(),
        )
        .unwrap();
        let mut overlap_event = event;
        overlap_event.edge = None;
        overlap_event.overlap_depth = 1;
        let mut actions = ProcessActions::with_sample_capacity(2);
        actions.begin_step(2);
        let stats = apply_collision(
            &surface,
            &[overlap_event],
            &current,
            &mut overlapped,
            &mut actions,
            recipe,
        )
        .unwrap();
        assert!(overlapped.samples.iter().all(|sample| {
            sample.tectonic_elevation_m > 700.0
                && sample.orogeny == SphericalOrogenyKind::Himalayan
                && sample.orogeny_age_myr == 0.0
                && (sample.lineation[0].hypot(sample.lineation[1]) - 1.0).abs() <= 1.0e-5
        }));
        assert_eq!(stats.collision_events, 1);
        assert_eq!(stats.forced_subductions, 1);
        commit_process_actions(&mut overlapped, &mut actions).unwrap();
        assert_eq!(overlapped.samples[0].owner, large);
        assert_eq!(overlapped.samples[1].owner, large);
        assert_eq!(overlapped.material_totals().unwrap(), material_before);
    }

    #[test]
    fn evolved_collision_rejects_a_transfer_above_the_hard_area_cap() {
        let surface = surface();
        let edge = &surface.edges()[0];
        let moving = LineageId::from_raw(0);
        let receiving = LineageId::from_raw(1);
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let samples = surface
            .cells()
            .iter()
            .map(|cell| {
                continental_sample(
                    &surface,
                    cell.id,
                    if cell.id == edge.cells[0] {
                        moving
                    } else {
                        receiving
                    },
                )
            })
            .collect::<Vec<_>>();
        let plates = vec![
            ActivePlate::new(moving, edge.cells[0], rotation),
            ActivePlate::new(receiving, edge.cells[1], rotation),
        ];
        let current = TectonicState::new(samples.clone(), plates.clone(), 2).unwrap();
        let mut next = TectonicState::new(samples, plates, 2).unwrap();
        let event = ContactEvent {
            cell: edge.cells[0],
            edge: Some(edge.id),
            sample_indices: [Some(edge.cells[0].raw()), Some(edge.cells[1].raw())],
            lineages: [Some(moving), Some(receiving)],
            kind: ContactKind::ContinentalCollision,
            signed_normal_speed_mm_per_year: -90.0,
            tangent_speed_mm_per_year: 5.0,
            overlap_depth: 1,
        };
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());
        let mut ledger = EvolutionMaterialLedger::capture_initial(&current).unwrap();
        let material_before = current.material_totals().unwrap();
        let stats = apply_collision_v5(
            &surface,
            &[event],
            &current,
            &mut next,
            &mut actions,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
            &mut ledger,
            2.0,
        )
        .unwrap();
        commit_process_actions(&mut next, &mut actions).unwrap();

        assert_eq!(stats.collision_events, 1);
        assert_eq!(stats.transferred_samples, 0);
        assert_eq!(stats.terrane_transfer_events, 0);
        assert_eq!(next.samples[edge.cells[0].raw() as usize].owner, moving);

        // Pure-shear shortening: both participants thicken by the bounded
        // step factor (90 mm/yr over 2 Myr = 180 km against the 400 km belt
        // asks for 1.45, capped at 1.2), volume is bit-preserved, and the
        // retired area is exactly the ledger's recorded loss.
        let totals_after = next.material_totals().unwrap();
        assert_eq!(
            totals_after.continental().volume_m3(),
            material_before.continental().volume_m3()
        );
        let lost_area = material_before.continental().reference_area_m2()
            - totals_after.continental().reference_area_m2();
        assert!(lost_area > 0.0);
        let processes = ledger.processes().unwrap();
        assert!(
            (processes.collision_shortening_continental_area_loss_m2() - lost_area).abs()
                <= lost_area * 1.0e-12
        );
        for cell in edge.cells {
            let sample = next.samples[cell.raw() as usize];
            let thickness = sample.material.continental_thickness_km().unwrap();
            assert!((thickness - 40.0 * 1.2).abs() <= 1.0e-3, "{thickness}");
            assert!(sample.tectonic_elevation_m > 700.0);
        }
        let untouched = next
            .samples
            .iter()
            .enumerate()
            .filter(|(index, _)| !edge.cells.iter().any(|cell| cell.raw() as usize == *index))
            .all(|(_, sample)| sample.material.continental_thickness_km() == Some(40.0));
        assert!(untouched, "shortening must stay on the converging columns");
        assert!(
            ledger.remaining_collision_shortening_area_m2()
                < material_before.continental().reference_area_m2() * 0.15
        );
    }
}
