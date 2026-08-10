//! Cortial-style oceanic crust creation in divergent coverage gaps.
//!
//! Empty coverage cells receive young ridge-high oceanic samples blended to
//! the nearest moving plate. This is the paper's continuous seafloor-generation
//! process expressed against the fixed authoritative sphere; samples are queued
//! and appended only by the shared action commit.

use super::{constants, event_lineation, ProcessActions, ProcessError, ProcessStats};
use crate::generators::natural::spherical_tectonics::contacts::{ContactEvent, ContactKind};
use crate::generators::natural::spherical_tectonics::model::{
    CrustSample, FormationTectonicRecipe, TectonicState,
};
use crate::world::natural::{CrustKind, SphericalOrogenyKind, NO_OROGENY_AGE_SENTINEL_MYR};
use crate::world::spatial::SphericalSurfaceSnapshot;

pub(in crate::generators::natural::spherical_tectonics) fn fill_spreading_gaps(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    _recipe: FormationTectonicRecipe,
) -> Result<ProcessStats, ProcessError> {
    actions.validate_for(next.samples.len())?;
    let mut stats = ProcessStats::default();
    for gap in events.iter().filter(|event| event.kind == ContactKind::Gap) {
        let cell = surface
            .cell(gap.cell)
            .ok_or(ProcessError::UnknownCell { cell: gap.cell })?;
        let divergence = events.iter().find(|event| {
            event.kind == ContactKind::Divergence
                && event.edge.is_some_and(|edge_id| {
                    surface
                        .edge(edge_id)
                        .is_some_and(|edge| edge.cells.contains(&gap.cell))
                })
        });
        let owner = divergence
            .and_then(|event| closest_participant_owner(event, next, cell.centroid))
            .or_else(|| closest_state_owner(current, cell.centroid))
            .ok_or(ProcessError::MissingDenseCurrentSample { cell: gap.cell })?;
        if next.plate(owner).is_none() {
            return Err(ProcessError::UnknownLineage { lineage: owner });
        }
        let lineation = event_lineation(surface, divergence.unwrap_or(gap), cell.centroid)?;
        actions.push_spawned(CrustSample {
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
        });
        stats.spawned_samples += 1;
    }
    Ok(stats)
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
    use super::fill_spreading_gaps;
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
        let stats =
            fill_spreading_gaps(&surface, &[gap], &current, &mut next, &mut actions, recipe)
                .unwrap();
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
}
