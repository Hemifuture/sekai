//! The sole bounded current-state tectonic evolution loop.
//!
//! Every step consumes only `current`, writes only reusable `next`, commits
//! process actions once, and discards the overwritten state. No history or
//! alternate final-owner path exists here.

#![cfg_attr(not(test), allow(dead_code))]

use thiserror::Error;

use super::contacts::{build_contacts, ContactError};
use super::initial_state::{build_initial_state, InitialStateError};
use super::kinematics::{advance_samples, KinematicsError};
use super::model::{FormationTectonicRecipe, TectonicState};
use super::processes::{
    apply_collision, apply_divergent_extension, apply_subduction, commit_process_actions,
    fill_spreading_gaps, maybe_rift_plates, relax_current_crust, ProcessError,
};
use super::resample::{
    canonicalize_final_plates, resample_current_state, resampling_interval_steps,
    CanonicalTectonicState, ResampleError,
};
use super::workspace::TectonicWorkspace;
use crate::generators::natural::random::LabeledSubstreams;
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{ResolvedWorldFormationPreset, TectonicSpec, MIN_PLATE_COUNT};
use crate::world::spatial::SphericalSurfaceSnapshot;

pub(super) const EVOLUTION_STEP_COUNT: u16 = 128;
pub(super) const EVOLUTION_DELTA_MYR: f64 = 2.0;

pub(super) fn run_tectonic_evolution(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    formation: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
) -> Result<CanonicalTectonicState, RunnerError> {
    let current = evolve_current_state(surface, topology, spec, formation, streams)?;
    canonicalize_evolved_state(surface, current)
}

pub(super) fn evolve_current_state(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    formation: ResolvedWorldFormationPreset,
    streams: &LabeledSubstreams,
) -> Result<TectonicState, RunnerError> {
    let recipe = FormationTectonicRecipe::for_preset(formation);
    let initial = build_initial_state(surface, topology, spec, recipe, streams)?;
    let mut workspace = TectonicWorkspace::from_initial(initial);

    for step in 0..EVOLUTION_STEP_COUNT {
        let (current, next, coverage, events, actions) = workspace.step_parts();
        advance_samples(surface, topology, current, next, EVOLUTION_DELTA_MYR)?;
        build_contacts(surface, topology, next, coverage, events)?;
        actions.begin_step(next.samples.len());
        apply_subduction(surface, events, current, next, actions, recipe)?;
        apply_collision(surface, events, current, next, actions, recipe)?;
        apply_divergent_extension(surface, events, next, actions, EVOLUTION_DELTA_MYR as f32)?;
        fill_spreading_gaps(surface, events, current, next, actions, recipe)?;
        maybe_rift_plates(step, surface, current, next, actions, recipe, streams)?;
        relax_current_crust(
            surface,
            events,
            next,
            actions,
            recipe,
            EVOLUTION_DELTA_MYR as f32,
        )?;
        actions.preserve_minimum_live_lineages(
            &next.samples,
            &current.plates,
            &next.plates,
            usize::from(MIN_PLATE_COUNT),
        )?;
        commit_process_actions(next, actions)?;
        workspace.swap_current_next();
        if resample_due(&workspace) {
            resample_current_state(surface, topology, &mut workspace)?;
        }
    }
    if workspace.requires_resample() {
        resample_current_state(surface, topology, &mut workspace)?;
    }
    Ok(workspace.current)
}

pub(super) fn canonicalize_evolved_state(
    surface: &SphericalSurfaceSnapshot,
    current: TectonicState,
) -> Result<CanonicalTectonicState, RunnerError> {
    canonicalize_final_plates(surface, current).map_err(Into::into)
}

fn resample_due(workspace: &TectonicWorkspace) -> bool {
    workspace.steps_since_resample() >= resampling_interval_steps(&workspace.current)
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum RunnerError {
    #[error("initial tectonic state failed: {0}")]
    Initial(#[from] InitialStateError),
    #[error("rigid plate motion failed: {0}")]
    Kinematics(#[from] KinematicsError),
    #[error("contact classification failed: {0}")]
    Contacts(#[from] ContactError),
    #[error("tectonic process failed: {0}")]
    Process(#[from] ProcessError),
    #[error("tectonic resampling failed: {0}")]
    Resample(#[from] ResampleError),
}

#[cfg(test)]
mod tests {
    use super::run_tectonic_evolution;
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::spherical_tectonics::initial_state::build_initial_state;
    use crate::generators::natural::spherical_tectonics::model::FormationTectonicRecipe;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{ResolvedWorldFormationPreset, TectonicActivity, TectonicSpec};
    use crate::world::spatial::SphericalNaturalSurface;
    use crate::world::{Meters, RootSeed, SphericalSpaceSpec};

    #[test]
    fn final_owners_come_from_evolution_not_the_initial_voronoi_partition() {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(0x5EED_7EC7),
            StageIdentity::new("runner-evolution-test", 1, "sekai.test"),
        ));
        let streams = LabeledSubstreams::capture(&mut rng);
        let spec = TectonicSpec::default();
        let formation = ResolvedWorldFormationPreset::Continents;
        let initial = build_initial_state(
            &surface,
            &topology,
            &spec,
            FormationTectonicRecipe::for_preset(formation),
            &streams,
        )
        .unwrap()
        .initial_owners()
        .into_iter()
        .map(|lineage| lineage.raw())
        .collect::<Vec<_>>();
        let final_state =
            run_tectonic_evolution(&surface, &topology, &spec, formation, &streams).unwrap();

        assert_ne!(final_state.cell_plates.raw_values(), initial);
        assert_eq!(final_state.samples.len(), surface.cells().len());
        assert!((2..=64).contains(&final_state.plates.len()));
    }

    #[test]
    fn minimum_supported_plate_count_cannot_collapse_during_collision() {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(1.0).unwrap(),
            target_cell_count: 42,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(1),
            StageIdentity::new("matrix.tectonic", 1, "sekai.matrix"),
        ));
        let streams = LabeledSubstreams::capture(&mut rng);
        let spec = TectonicSpec {
            plate_count: 2,
            activity: TectonicActivity::Quiet,
            continental_crust_fraction: 0.42,
            ..TectonicSpec::default()
        };

        let final_state = run_tectonic_evolution(
            &surface,
            &topology,
            &spec,
            ResolvedWorldFormationPreset::Supercontinent,
            &streams,
        )
        .expect("the minimum supported plate configuration must remain publishable");

        assert!((2..=64).contains(&final_state.plates.len()));
    }
}
