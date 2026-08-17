//! Cortial-style subduction transfer curves and effects.
//!
//! Appendix-A distance/speed transfer curves drive a bounded trench on the
//! descending sample and uplift on the overriding sample. Material decides the
//! descending side before this module; noise is intentionally absent.

use super::{
    bounded_elevation, constants, event_distance_m, event_lineation, event_speed, ProcessActions,
    ProcessError, ProcessStats,
};
use crate::generators::natural::spherical_tectonics::contacts::{ContactEvent, ContactKind};
use crate::generators::natural::spherical_tectonics::model::{
    FormationTectonicRecipe, TectonicState,
};
use crate::world::natural::{CrustKind, SphericalOrogenyKind};
use crate::world::spatial::SphericalSurfaceSnapshot;

pub(super) fn subduction_profile(distance_m: f64, speed_mm_per_year: f64, gain: f64) -> (f32, f32) {
    if !distance_m.is_finite()
        || !speed_mm_per_year.is_finite()
        || !gain.is_finite()
        || distance_m < 0.0
        || speed_mm_per_year <= 0.0
        || gain <= 0.0
        || distance_m >= constants::SUBDUCTION_MAX_DISTANCE_M
    {
        return (0.0, 0.0);
    }
    let distance_weight = if distance_m <= constants::SUBDUCTION_PEAK_DISTANCE_M {
        smoothstep(distance_m / constants::SUBDUCTION_PEAK_DISTANCE_M)
    } else {
        1.0 - smoothstep(
            (distance_m - constants::SUBDUCTION_PEAK_DISTANCE_M)
                / (constants::SUBDUCTION_MAX_DISTANCE_M - constants::SUBDUCTION_PEAK_DISTANCE_M),
        )
    };
    let speed_weight =
        (speed_mm_per_year / constants::REFERENCE_PLATE_SPEED_MM_PER_YEAR).clamp(0.0, 1.0);
    let response = distance_weight * speed_weight * gain;
    let trench_span =
        f64::from(constants::ABYSSAL_PLAIN_ELEVATION_M - constants::OCEANIC_TRENCH_ELEVATION_M);
    let uplift_per_step_m =
        constants::BASE_SUBDUCTION_UPLIFT_MM_PER_YEAR * constants::DEFAULT_DELTA_MYR * 1_000.0;
    (
        -(trench_span * response) as f32,
        (uplift_per_step_m * response) as f32,
    )
}

pub(in crate::generators::natural::spherical_tectonics) fn apply_subduction(
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
    let gain = f64::from(recipe.subduction_gain_permille) / 1_000.0;
    let mut stats = ProcessStats::default();
    for event in events {
        let ContactKind::OceanicSubduction { descending } = event.kind else {
            continue;
        };
        let [first, second] = participant_indices(event, next.samples.len())?;
        let descending_index = if next.samples[first].owner == descending {
            first
        } else if next.samples[second].owner == descending {
            second
        } else {
            return Err(ProcessError::MissingDescendingSide { descending });
        };
        let overriding_index = if descending_index == first {
            second
        } else {
            first
        };
        let descending_distance =
            event_distance_m(surface, event, next.samples[descending_index].position)?;
        let overriding_distance =
            event_distance_m(surface, event, next.samples[overriding_index].position)?;
        let speed = event_speed(event);
        let (trench, _) = subduction_profile(descending_distance, speed, gain);
        let (_, raw_uplift) = subduction_profile(overriding_distance, speed, gain);
        let descending_elevation = next.samples[descending_index].tectonic_elevation_m;
        let normalized_height = ((descending_elevation - constants::OCEANIC_TRENCH_ELEVATION_M)
            / (constants::HIGHEST_CONTINENTAL_ELEVATION_M - constants::OCEANIC_TRENCH_ELEVATION_M))
            .clamp(0.0, 1.0);
        let uplift = raw_uplift * normalized_height * normalized_height;
        let lineation = event_lineation(surface, event, next.samples[overriding_index].position)?;

        actions.record_subduction_trench(descending_index, trench)?;
        actions.record_subduction_uplift(overriding_index, uplift, lineation)?;
        if event.overlap_depth > 0 {
            actions.mark_remove(descending_index)?;
            stats.removed_samples += 1;
        }
        stats.subduction_events += 1;
    }
    for (index, effect) in actions.subduction_effects().iter().copied().enumerate() {
        if effect.trench_m == 0.0 && effect.uplift_m == 0.0 {
            continue;
        }
        let sample = &mut next.samples[index];
        sample.tectonic_elevation_m =
            bounded_elevation(sample.tectonic_elevation_m + effect.trench_m + effect.uplift_m);
        if effect.uplift_m > 0.0 && sample.kind == CrustKind::Continental {
            sample.orogeny = SphericalOrogenyKind::Andean;
            sample.orogeny_age_myr = 0.0;
            sample.lineation = effect.uplift_lineation;
        }
        stats.affected_samples += 1;
    }
    Ok(stats)
}

/// Conservative V5 subduction. The geometric transfer curves remain the
/// Cortial-style V4 curves, but overlap consumes only the descending oceanic
/// component. A mixed column keeps its continental component; a pure oceanic
/// column is removed atomically by the V5 action commit.
pub(in crate::generators::natural::spherical_tectonics) fn apply_subduction_v5(
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
    let gain = f64::from(recipe.subduction_gain_permille) / 1_000.0;
    let mut stats = ProcessStats::default();
    for event in events {
        let ContactKind::OceanicSubduction { descending } = event.kind else {
            continue;
        };
        let [first, second] = participant_indices(event, next.samples.len())?;
        let descending_index = if next.samples[first].owner == descending {
            first
        } else if next.samples[second].owner == descending {
            second
        } else {
            return Err(ProcessError::MissingDescendingSide { descending });
        };
        let overriding_index = if descending_index == first {
            second
        } else {
            first
        };
        let descending_distance =
            event_distance_m(surface, event, next.samples[descending_index].position)?;
        let overriding_distance =
            event_distance_m(surface, event, next.samples[overriding_index].position)?;
        let speed = event_speed(event);
        let (trench, _) = subduction_profile(descending_distance, speed, gain);
        let (_, raw_uplift) = subduction_profile(overriding_distance, speed, gain);
        let descending_elevation = next.samples[descending_index].tectonic_elevation_m;
        let normalized_height = ((descending_elevation - constants::OCEANIC_TRENCH_ELEVATION_M)
            / (constants::HIGHEST_CONTINENTAL_ELEVATION_M - constants::OCEANIC_TRENCH_ELEVATION_M))
            .clamp(0.0, 1.0);
        let uplift = raw_uplift * normalized_height * normalized_height;
        let lineation = event_lineation(surface, event, next.samples[overriding_index].position)?;

        actions.record_subduction_trench(descending_index, trench)?;
        actions.record_subduction_uplift(overriding_index, uplift, lineation)?;
        if event.overlap_depth > 0
            && actions.stage_oceanic_subduction(
                descending_index,
                next.samples[descending_index].material,
            )?
        {
            stats.removed_samples += u32::from(
                next.samples[descending_index]
                    .material
                    .continental_reference_area_m2()
                    == 0.0,
            );
        }
        stats.subduction_events += 1;
    }
    for (index, effect) in actions.subduction_effects().iter().copied().enumerate() {
        if effect.trench_m == 0.0 && effect.uplift_m == 0.0 {
            continue;
        }
        let sample = &mut next.samples[index];
        sample.tectonic_elevation_m =
            bounded_elevation(sample.tectonic_elevation_m + effect.trench_m + effect.uplift_m);
        if effect.uplift_m > 0.0 && sample.kind == CrustKind::Continental {
            sample.orogeny = SphericalOrogenyKind::Andean;
            sample.orogeny_age_myr = 0.0;
            sample.lineation = effect.uplift_lineation;
        }
        stats.affected_samples += 1;
    }
    Ok(stats)
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
    use super::{apply_subduction, apply_subduction_v5, subduction_profile};
    use crate::generators::natural::spherical_tectonics::contacts::{ContactEvent, ContactKind};
    use crate::generators::natural::spherical_tectonics::model::{
        ActivePlate, CrustSample, EvolutionMaterialLedger, FormationTectonicRecipe, LineageId,
        MaterialColumn, TectonicState,
    };
    use crate::generators::natural::spherical_tectonics::processes::{
        commit_process_actions_v5, constants, ProcessActions,
    };
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, ResolvedWorldFormationPreset, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
    use crate::world::{Meters, SphericalSpaceSpec};

    fn surface() -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap()
    }

    fn state_and_event(
        surface: &SphericalSurfaceSnapshot,
        ocean_age: f32,
    ) -> (TectonicState, TectonicState, ContactEvent) {
        let edge = &surface.edges()[0];
        let descending = LineageId::from_raw(0);
        let overriding = LineageId::from_raw(1);
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let ocean = CrustSample {
            position: surface.cell(edge.cells[0]).unwrap().site,
            anchor: edge.cells[0],
            owner: descending,
            kind: CrustKind::Oceanic,
            thickness_km: 7.0,
            age_myr: ocean_age,
            tectonic_elevation_m: -4_000.0,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(
                CrustKind::Oceanic,
                surface.cell(edge.cells[0]).unwrap().area.get(),
                7.0,
            )
            .unwrap(),
        };
        let continent = CrustSample {
            position: surface.cell(edge.cells[1]).unwrap().site,
            anchor: edge.cells[1],
            owner: overriding,
            kind: CrustKind::Continental,
            thickness_km: 40.0,
            age_myr: CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
            tectonic_elevation_m: 500.0,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(
                CrustKind::Continental,
                surface.cell(edge.cells[1]).unwrap().area.get(),
                40.0,
            )
            .unwrap(),
        };
        let plates = vec![
            ActivePlate::new(descending, edge.cells[0], rotation),
            ActivePlate::new(overriding, edge.cells[1], rotation),
        ];
        let current = TectonicState::new(vec![ocean, continent], plates.clone(), 2).unwrap();
        let next = TectonicState::new(vec![ocean, continent], plates, 2).unwrap();
        let event = ContactEvent {
            cell: edge.cells[0],
            edge: Some(edge.id),
            sample_indices: [Some(0), Some(1)],
            lineages: [Some(descending), Some(overriding)],
            kind: ContactKind::OceanicSubduction { descending },
            signed_normal_speed_mm_per_year: -80.0,
            tangent_speed_mm_per_year: 10.0,
            overlap_depth: 1,
        };
        (current, next, event)
    }

    #[test]
    fn appendix_a_subduction_curve_has_bounded_endpoints_and_monotone_branches() {
        assert_eq!(constants::SUBDUCTION_MAX_DISTANCE_M, 1_800_000.0);
        assert_eq!(constants::BASE_SUBDUCTION_UPLIFT_MM_PER_YEAR, 0.6);
        let at_front = subduction_profile(0.0, 100.0, 1.0);
        let before_peak =
            subduction_profile(constants::SUBDUCTION_PEAK_DISTANCE_M * 0.5, 100.0, 1.0);
        let peak = subduction_profile(constants::SUBDUCTION_PEAK_DISTANCE_M, 100.0, 1.0);
        let after_peak = subduction_profile(
            (constants::SUBDUCTION_PEAK_DISTANCE_M + constants::SUBDUCTION_MAX_DISTANCE_M) * 0.5,
            100.0,
            1.0,
        );
        let outside = subduction_profile(constants::SUBDUCTION_MAX_DISTANCE_M, 100.0, 1.0);
        assert_eq!(at_front, (0.0, 0.0));
        assert!(before_peak.0 < 0.0 && before_peak.1 > 0.0);
        assert!(peak.0 < before_peak.0 && peak.1 > before_peak.1);
        assert!(after_peak.0 > peak.0 && after_peak.1 < peak.1);
        assert_eq!(outside, (0.0, 0.0));
        assert!(subduction_profile(constants::SUBDUCTION_PEAK_DISTANCE_M, 40.0, 1.0).1 < peak.1);
        assert!(subduction_profile(constants::SUBDUCTION_PEAK_DISTANCE_M, 100.0, 1.2).1 > peak.1);
    }

    #[test]
    fn subduction_deepens_descending_crust_and_creates_andean_uplift() {
        let surface = surface();
        let (current, mut next, event) = state_and_event(&surface, 120.0);
        let baseline_ocean = next.samples[0].tectonic_elevation_m;
        let baseline_overriding = next.samples[1].tectonic_elevation_m;
        let mut actions = ProcessActions::with_sample_capacity(2);
        actions.begin_step(2);
        let stats = apply_subduction(
            &surface,
            &[event],
            &current,
            &mut next,
            &mut actions,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
        )
        .unwrap();

        let descending = next.samples[0];
        let overriding = next.samples[1];
        assert!(descending.tectonic_elevation_m < baseline_ocean);
        assert!(overriding.tectonic_elevation_m > baseline_overriding);
        assert_eq!(overriding.orogeny, SphericalOrogenyKind::Andean);
        assert_eq!(overriding.orogeny_age_myr, 0.0);
        assert!((overriding.lineation[0].hypot(overriding.lineation[1]) - 1.0).abs() <= 1.0e-5);
        assert_eq!(
            next.samples.len(),
            2,
            "processes may not invalidate event indices"
        );
        assert_eq!(stats.subduction_events, 1);
        assert_eq!(stats.removed_samples, 1);
    }

    #[test]
    fn one_sample_receives_one_strongest_subduction_response_per_step() {
        let surface = surface();
        let (current, mut once, mut event) = state_and_event(&surface, 120.0);
        let (_, mut repeated, _) = state_and_event(&surface, 120.0);
        event.overlap_depth = 0;
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);

        let mut once_actions = ProcessActions::with_sample_capacity(2);
        once_actions.begin_step(2);
        apply_subduction(
            &surface,
            std::slice::from_ref(&event),
            &current,
            &mut once,
            &mut once_actions,
            recipe,
        )
        .unwrap();

        let mut repeated_actions = ProcessActions::with_sample_capacity(2);
        repeated_actions.begin_step(2);
        apply_subduction(
            &surface,
            &[event.clone(), event],
            &current,
            &mut repeated,
            &mut repeated_actions,
            recipe,
        )
        .unwrap();

        assert_eq!(
            repeated.samples, once.samples,
            "sampling the same continuous front twice doubled its elevation response"
        );
    }

    #[test]
    fn evolved_subduction_consumes_oceanic_component_before_continental_material() {
        let surface = surface();
        let (mut current, mut next, event) = state_and_event(&surface, 120.0);
        let cell_area = surface.cell(current.samples[0].anchor).unwrap().area.get();
        let mixed = MaterialColumn::new(
            cell_area * 0.25,
            cell_area * 0.25 * 35_000.0,
            cell_area * 0.75,
            cell_area * 0.75 * 7_000.0,
        )
        .unwrap();
        current.samples[0].material = mixed;
        current.samples[0].synchronize_compatibility_from_material();
        next.samples[0] = current.samples[0];
        let initial_continental = current
            .material_totals()
            .unwrap()
            .continental()
            .reference_area_m2();
        let consumed_oceanic = mixed.oceanic_amount().unwrap();
        let mut ledger = EvolutionMaterialLedger::capture_initial(&current).unwrap();
        let mut actions = ProcessActions::with_sample_capacity(2);
        actions.begin_step(2);

        apply_subduction_v5(
            &surface,
            &[event],
            &current,
            &mut next,
            &mut actions,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
        )
        .unwrap();
        commit_process_actions_v5(&mut next, &mut actions, &mut ledger).unwrap();

        assert_eq!(
            next.samples.len(),
            2,
            "the continental remnant must survive"
        );
        assert_eq!(
            next.material_totals()
                .unwrap()
                .continental()
                .reference_area_m2(),
            initial_continental
        );
        assert_eq!(next.samples[0].material.oceanic_reference_area_m2(), 0.0);
        assert_eq!(next.samples[0].kind, CrustKind::Continental);
        assert_eq!(
            ledger.processes().unwrap().oceanic_subducted(),
            consumed_oceanic
        );
        assert_eq!(
            ledger
                .processes()
                .unwrap()
                .continental_consumed()
                .reference_area_m2(),
            0.0
        );
        ledger.control_budget(&next).unwrap();
    }
}
