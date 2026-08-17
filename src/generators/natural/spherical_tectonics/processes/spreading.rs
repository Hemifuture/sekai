//! Cortial-style oceanic crust creation in divergent coverage gaps.
//!
//! Empty coverage cells receive young ridge-high oceanic samples blended to
//! the nearest moving plate. This is the paper's continuous seafloor-generation
//! process expressed against the fixed authoritative sphere; samples are queued
//! and appended only by the shared action commit.

use super::{
    bounded_elevation, constants, event_lineation, ProcessActions, ProcessError, ProcessStats,
};
use crate::generators::natural::spherical_crust_physics::continental_isostatic_elevation_m;
use crate::generators::natural::spherical_tectonics::contacts::{ContactEvent, ContactKind};
use crate::generators::natural::spherical_tectonics::model::{
    CrustSample, FormationTectonicRecipe, MaterialColumn, TectonicState,
};
use crate::world::natural::{
    CrustKind, SphericalOrogenyKind, CONTINENTAL_CRUST_MIN_THICKNESS_KM,
    NO_OROGENY_AGE_SENTINEL_MYR,
};
use crate::world::spatial::SphericalSurfaceSnapshot;

// McKenzie-style homogeneous pure-shear extension expressed over one coarse
// rift zone.  The bounded per-step beta prevents a single 2 My step from
// exhausting continental crust while repeated divergence can still reach the
// public minimum-thickness contract.
const CONTINENTAL_RIFT_ZONE_WIDTH_M: f64 = 400_000.0;
const MAXIMUM_STEP_STRETCH_FACTOR: f64 = 1.2;

pub(in crate::generators::natural::spherical_tectonics) fn apply_divergent_extension(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    delta_myr: f32,
) -> Result<ProcessStats, ProcessError> {
    if !delta_myr.is_finite() || delta_myr < 0.0 {
        return Err(ProcessError::InvalidDeltaMyr { found: delta_myr });
    }
    actions.validate_for(next.samples.len())?;
    for event in events
        .iter()
        .filter(|event| event.kind == ContactKind::Divergence)
    {
        let speed = event.signed_normal_speed_mm_per_year.max(0.0);
        for &sample in event.sample_indices.iter().flatten() {
            let sample = sample as usize;
            if next.samples.get(sample).is_none() {
                return Err(ProcessError::ContactSampleOutOfBounds {
                    sample,
                    samples: next.samples.len(),
                });
            }
            actions.record_extensional_speed(sample, speed)?;
        }
    }

    let mut affected_samples = 0;
    for (index, (sample, speed)) in next
        .samples
        .iter_mut()
        .zip(actions.extensional_speeds_mm_per_year())
        .enumerate()
    {
        if *speed <= 0.0 || sample.kind != CrustKind::Continental {
            continue;
        }
        if surface.cell(sample.anchor).is_none() {
            return Err(ProcessError::InvalidAnchor {
                sample: index,
                anchor: sample.anchor,
                cells: surface.cells().len(),
            });
        }
        let extension_m = f64::from(*speed) * f64::from(delta_myr) * 1_000.0;
        let beta = (1.0 + extension_m / CONTINENTAL_RIFT_ZONE_WIDTH_M)
            .clamp(1.0, MAXIMUM_STEP_STRETCH_FACTOR);
        let old_thickness = sample.thickness_km;
        let new_thickness = (f64::from(old_thickness) / beta) as f32;
        let new_thickness = new_thickness.max(CONTINENTAL_CRUST_MIN_THICKNESS_KM);
        if new_thickness >= old_thickness {
            continue;
        }
        let subsidence = continental_isostatic_elevation_m(new_thickness)
            - continental_isostatic_elevation_m(old_thickness);
        sample.thickness_km = new_thickness;
        sample.tectonic_elevation_m = bounded_elevation(sample.tectonic_elevation_m + subsidence);
        affected_samples += 1;
    }
    Ok(ProcessStats {
        affected_samples,
        ..ProcessStats::default()
    })
}

pub(in crate::generators::natural::spherical_tectonics) fn fill_spreading_gaps(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    _recipe: FormationTectonicRecipe,
) -> Result<ProcessStats, ProcessError> {
    actions.validate_for(next.samples.len())?;
    let (divergence_by_cell, current_sample_by_cell, spawned) =
        actions.spreading_scratch(surface.cells().len());
    index_incident_divergence(surface, events, divergence_by_cell)?;
    index_current_samples(surface, current, current_sample_by_cell)?;
    let mut stats = ProcessStats::default();
    for gap in events.iter().filter(|event| event.kind == ContactKind::Gap) {
        let cell = surface
            .cell(gap.cell)
            .ok_or(ProcessError::UnknownCell { cell: gap.cell })?;
        let cell_index = gap.cell.raw() as usize;
        let divergence = divergence_by_cell[cell_index].map(|index| &events[index]);
        let owner = divergence
            .and_then(|event| closest_participant_owner(event, next, cell.centroid))
            .or_else(|| {
                current_sample_by_cell[cell_index]
                    .map(|sample_index| current.samples[sample_index].owner)
            })
            .or_else(|| closest_state_owner(current, cell.centroid))
            .ok_or(ProcessError::MissingDenseCurrentSample { cell: gap.cell })?;
        if next.plate(owner).is_none() {
            return Err(ProcessError::UnknownLineage { lineage: owner });
        }
        let lineation = event_lineation(surface, divergence.unwrap_or(gap), cell.centroid)?;
        spawned.push(CrustSample {
            position: cell.centroid,
            anchor: cell.id,
            owner,
            kind: CrustKind::Oceanic,
            thickness_km: 7.0,
            age_myr: 0.0,
            tectonic_elevation_m: constants::OCEANIC_RIDGE_ELEVATION_M,
            lineation,
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(CrustKind::Oceanic, cell.area.get(), 7.0)
                .expect("spreading creates bounded oceanic material"),
        });
        stats.spawned_samples += 1;
    }
    Ok(stats)
}

fn index_incident_divergence(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    by_cell: &mut [Option<usize>],
) -> Result<(), ProcessError> {
    debug_assert_eq!(by_cell.len(), surface.cells().len());
    for (event_index, event) in events.iter().enumerate() {
        if event.kind != ContactKind::Divergence {
            continue;
        }
        let Some(edge_id) = event.edge else {
            continue;
        };
        let edge = surface
            .edge(edge_id)
            .ok_or(ProcessError::UnknownEdge { edge: edge_id })?;
        for cell in edge.cells {
            let slot = by_cell
                .get_mut(cell.raw() as usize)
                .ok_or(ProcessError::UnknownCell { cell })?;
            if slot.is_none() {
                *slot = Some(event_index);
            }
        }
    }
    Ok(())
}

fn index_current_samples(
    surface: &SphericalSurfaceSnapshot,
    current: &TectonicState,
    by_cell: &mut [Option<usize>],
) -> Result<(), ProcessError> {
    debug_assert_eq!(by_cell.len(), surface.cells().len());
    for (sample_index, sample) in current.samples.iter().enumerate() {
        let cell_index = sample.anchor.raw() as usize;
        let cell = surface
            .cells()
            .get(cell_index)
            .ok_or(ProcessError::InvalidAnchor {
                sample: sample_index,
                anchor: sample.anchor,
                cells: surface.cells().len(),
            })?;
        let slot = &mut by_cell[cell_index];
        let replace = slot.is_none_or(|incumbent_index| {
            prefer_sample(
                sample_index,
                sample,
                incumbent_index,
                &current.samples[incumbent_index],
                cell.centroid,
            )
        });
        if replace {
            *slot = Some(sample_index);
        }
    }
    Ok(())
}

fn prefer_sample(
    candidate_index: usize,
    candidate: &CrustSample,
    incumbent_index: usize,
    incumbent: &CrustSample,
    direction: crate::world::spatial::UnitVector3,
) -> bool {
    candidate
        .position
        .dot(direction)
        .total_cmp(&incumbent.position.dot(direction))
        .then_with(|| incumbent.owner.cmp(&candidate.owner))
        .then_with(|| incumbent_index.cmp(&candidate_index))
        .is_gt()
}

fn closest_participant_owner(
    event: &ContactEvent,
    state: &TectonicState,
    direction: crate::world::spatial::UnitVector3,
) -> Option<crate::generators::natural::spherical_tectonics::model::LineageId> {
    closest_owner(
        event.sample_indices.iter().flatten().filter_map(|&index| {
            state
                .samples
                .get(index as usize)
                .map(|sample| (index as usize, sample))
        }),
        direction,
    )
}

fn closest_state_owner(
    state: &TectonicState,
    direction: crate::world::spatial::UnitVector3,
) -> Option<crate::generators::natural::spherical_tectonics::model::LineageId> {
    closest_owner(state.samples.iter().enumerate(), direction)
}

fn closest_owner<'a>(
    samples: impl Iterator<Item = (usize, &'a CrustSample)>,
    direction: crate::world::spatial::UnitVector3,
) -> Option<crate::generators::natural::spherical_tectonics::model::LineageId> {
    samples
        .max_by(|(first_index, first), (second_index, second)| {
            first
                .position
                .dot(direction)
                .total_cmp(&second.position.dot(direction))
                .then_with(|| second.owner.cmp(&first.owner))
                .then_with(|| second_index.cmp(first_index))
        })
        .map(|(_, sample)| sample.owner)
}

#[cfg(test)]
mod tests {
    use super::{apply_divergent_extension, fill_spreading_gaps};
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::spherical_tectonics::contacts::{ContactEvent, ContactKind};
    use crate::generators::natural::spherical_tectonics::initial_state::build_initial_state;
    use crate::generators::natural::spherical_tectonics::model::{
        FormationTectonicRecipe, TectonicState,
    };
    use crate::generators::natural::spherical_tectonics::processes::{
        commit_process_actions, constants, ProcessActions,
    };
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{CrustKind, ResolvedWorldFormationPreset, TectonicSpec};
    use crate::world::spatial::{SphericalNaturalSurface, SphericalSurfaceSnapshot};
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    fn fixture() -> (SphericalSurfaceSnapshot, NaturalTopologyIndex) {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        (surface, topology)
    }

    fn streams() -> LabeledSubstreams {
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("spreading-test", 1, "sekai.test"),
        ));
        LabeledSubstreams::capture(&mut rng)
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
    fn gap_spawns_young_ridge_high_ocean_without_changing_event_indices() {
        let (surface, topology) = fixture();
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            recipe,
            &streams(),
        )
        .unwrap();
        let mut next = copy_state(&current);
        let gap_cell = CellId::from_raw(3);
        let gap = ContactEvent {
            cell: gap_cell,
            edge: None,
            sample_indices: [None, None],
            lineages: [None, None],
            kind: ContactKind::Gap,
            signed_normal_speed_mm_per_year: 0.0,
            tangent_speed_mm_per_year: 0.0,
            overlap_depth: 0,
        };
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());
        let before_len = next.samples.len();
        let stats = fill_spreading_gaps(
            &surface,
            std::slice::from_ref(&gap),
            &current,
            &mut next,
            &mut actions,
            recipe,
        )
        .unwrap();
        let first_index_storage = actions.spreading_index_storage();
        actions.begin_step(next.samples.len());
        fill_spreading_gaps(
            &surface,
            std::slice::from_ref(&gap),
            &current,
            &mut next,
            &mut actions,
            recipe,
        )
        .unwrap();
        assert_eq!(
            actions.spreading_index_storage(),
            first_index_storage,
            "per-cell spreading indices must reuse their allocation between steps"
        );
        assert_eq!(next.samples.len(), before_len);
        commit_process_actions(&mut next, &mut actions).unwrap();
        let created = next.samples.last().unwrap();
        assert_eq!(next.samples.len(), before_len + 1);
        assert_eq!(created.anchor, gap_cell);
        assert_eq!(created.kind, CrustKind::Oceanic);
        assert_eq!(created.age_myr, 0.0);
        assert_eq!(
            created.tectonic_elevation_m,
            constants::OCEANIC_RIDGE_ELEVATION_M
        );
        assert!((created.lineation[0].hypot(created.lineation[1]) - 1.0).abs() <= 1.0e-5);
        assert_eq!(stats.spawned_samples, 1);
    }

    #[test]
    fn gap_fallback_uses_nearest_moving_material_not_a_dense_cell_index() {
        let (surface, topology) = fixture();
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let mut current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            recipe,
            &streams(),
        )
        .unwrap();
        let gap_cell = CellId::from_raw(3);
        let target = surface.cell(gap_cell).unwrap().centroid;
        let expected = current
            .samples
            .iter()
            .enumerate()
            .max_by(|(first_index, first), (second_index, second)| {
                first
                    .position
                    .dot(target)
                    .total_cmp(&second.position.dot(target))
                    .then_with(|| second.owner.cmp(&first.owner))
                    .then_with(|| second_index.cmp(first_index))
            })
            .unwrap()
            .1
            .owner;
        let wrong_index = current
            .samples
            .iter()
            .position(|sample| sample.owner != expected)
            .expect("the fixture has more than one lineage");
        current.samples.swap(gap_cell.raw() as usize, wrong_index);
        assert_ne!(current.samples[gap_cell.raw() as usize].owner, expected);
        let mut next = copy_state(&current);
        let gap = ContactEvent {
            cell: gap_cell,
            edge: None,
            sample_indices: [None, None],
            lineages: [None, None],
            kind: ContactKind::Gap,
            signed_normal_speed_mm_per_year: 0.0,
            tangent_speed_mm_per_year: 0.0,
            overlap_depth: 0,
        };
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());

        fill_spreading_gaps(&surface, &[gap], &current, &mut next, &mut actions, recipe).unwrap();
        commit_process_actions(&mut next, &mut actions).unwrap();

        assert_eq!(next.samples.last().unwrap().owner, expected);
    }

    #[test]
    fn continental_divergence_uses_bounded_pure_shear_thinning_and_isostatic_subsidence() {
        let (surface, topology) = fixture();
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            recipe,
            &streams(),
        )
        .unwrap();
        let mut next = copy_state(&current);
        let continental = next
            .samples
            .iter()
            .enumerate()
            .filter(|(_, sample)| sample.kind == CrustKind::Continental)
            .map(|(index, _)| index)
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(continental.len(), 2);
        let before = continental
            .iter()
            .map(|&index| {
                (
                    next.samples[index].thickness_km,
                    next.samples[index].tectonic_elevation_m,
                )
            })
            .collect::<Vec<_>>();
        let event = ContactEvent {
            cell: next.samples[continental[0]].anchor,
            edge: None,
            sample_indices: [Some(continental[0] as u32), Some(continental[1] as u32)],
            lineages: [
                Some(next.samples[continental[0]].owner),
                Some(next.samples[continental[1]].owner),
            ],
            kind: ContactKind::Divergence,
            signed_normal_speed_mm_per_year: 60.0,
            tangent_speed_mm_per_year: 0.0,
            overlap_depth: 0,
        };
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());

        let stats = apply_divergent_extension(
            &surface,
            &[event],
            &mut next,
            &mut actions,
            constants::DEFAULT_DELTA_MYR as f32,
        )
        .unwrap();

        assert_eq!(stats.affected_samples, 2);
        for (&index, &(old_thickness, old_elevation)) in continental.iter().zip(&before) {
            let sample = next.samples[index];
            assert!(sample.thickness_km < old_thickness);
            assert!(sample.tectonic_elevation_m < old_elevation);
            assert_eq!(sample.kind, CrustKind::Continental);
        }
        assert!(next
            .samples
            .iter()
            .enumerate()
            .filter(|(index, _)| !continental.contains(index))
            .all(|(index, sample)| *sample == current.samples[index]));
    }
}
