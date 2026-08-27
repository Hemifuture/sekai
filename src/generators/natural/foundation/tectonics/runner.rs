//! The sole bounded current-state tectonic evolution loop.
//!
//! Every step consumes only `current`, writes only reusable `next`, commits
//! process actions once, and discards the overwritten state. No history or
//! alternate final-owner path exists here.

#![cfg_attr(not(test), allow(dead_code))]

use thiserror::Error;

use super::contacts::{build_contacts, ContactError};
use super::forcing::{evaluate_present_day_forcing, ForcingError};
use super::initial_state::{build_initial_state, build_initial_state_v5, InitialStateError};
use super::kinematics::{advance_samples, KinematicsError};
use super::model::{
    EvolutionLineageLedger, EvolutionMaterialLedger, FormationTectonicRecipe, TectonicState,
};
use super::processes::{
    advance_solid_crust_ages, apply_collision, apply_collision_v5, apply_divergent_extension,
    apply_divergent_extension_v5, apply_subduction, apply_subduction_v5, commit_process_actions,
    commit_process_actions_v5, fill_spreading_gaps, fill_spreading_gaps_v5, maybe_rift_plates,
    mechanically_fragment_oversized_plates_v5, relax_legacy_compatibility_elevation, ProcessError,
};
use super::resample::{
    canonicalize_final_plates, resample_current_state, resample_current_state_v5,
    resampling_interval_steps, CanonicalTectonicState, ResampleError,
};
use super::torques::{update_rotations_from_boundary_torques, TorqueError};
use super::workspace::TectonicWorkspace;
use crate::engine::BuildCancellationError;
use crate::generators::natural::random::LabeledSubstreams;
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{
    ResolvedFormationTimeline, ResolvedWorldFormation, SphericalTectonicForcingState, TectonicSpec,
    MIN_PLATE_COUNT,
};
use crate::world::spatial::SphericalSurfaceSnapshot;

#[derive(Debug)]
pub(super) struct EvolvedControlState {
    pub(super) current: TectonicState,
    pub(super) forcing: SphericalTectonicForcingState,
    pub(super) material_ledger: EvolutionMaterialLedger,
    pub(super) lineage_ledger: EvolutionLineageLedger,
}

pub(super) fn run_tectonic_evolution(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    formation: &ResolvedWorldFormation,
    streams: &LabeledSubstreams,
) -> Result<CanonicalTectonicState, RunnerError> {
    let current = evolve_current_state(surface, topology, spec, formation, streams)?;
    canonicalize_evolved_state(surface, current)
}

pub(super) fn evolve_current_state(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    formation: &ResolvedWorldFormation,
    streams: &LabeledSubstreams,
) -> Result<TectonicState, RunnerError> {
    let recipe = FormationTectonicRecipe::for_preset(formation.resolved());
    let timeline = formation.timeline();
    let delta_myr = timeline.step_duration_myr();
    let process_delta_myr = delta_myr as f32;
    let rift_delta_myr = rift_step_duration_myr(timeline)?;
    let initial = build_initial_state(surface, topology, spec, formation.resolved(), streams)?;
    let mut workspace = TectonicWorkspace::from_initial(initial);

    for step in 0..timeline.step_count() {
        let (current, next, coverage, events, actions) = workspace.step_parts();
        advance_samples(surface, topology, current, next, delta_myr)?;
        build_contacts(surface, topology, next, coverage, events)?;
        actions.begin_step(next.samples.len());
        apply_subduction(surface, events, current, next, actions, recipe, delta_myr)?;
        apply_collision(surface, events, current, next, actions, recipe)?;
        apply_divergent_extension(surface, events, next, actions, process_delta_myr)?;
        fill_spreading_gaps(surface, events, current, next, actions, recipe)?;
        maybe_rift_plates(
            step,
            rift_delta_myr,
            surface,
            current,
            next,
            actions,
            recipe,
            streams,
        )?;
        advance_solid_crust_ages(next, process_delta_myr)?;
        relax_legacy_compatibility_elevation(
            surface,
            events,
            next,
            actions,
            recipe,
            process_delta_myr,
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

/// Runs the separately versioned conservative V5 material semantics. The V4
/// loop above remains the frozen compatibility path.
pub(super) fn evolve_control_state_v5(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    formation: &ResolvedWorldFormation,
    streams: &LabeledSubstreams,
) -> Result<EvolvedControlState, RunnerError> {
    evolve_control_state_v5_with_resample_observer(
        surface,
        topology,
        spec,
        formation,
        streams,
        |_, _, _, _| Ok(()),
    )
}

pub(super) fn evolve_control_state_v5_with_resample_observer(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    spec: &TectonicSpec,
    formation: &ResolvedWorldFormation,
    streams: &LabeledSubstreams,
    mut on_resampled: impl FnMut(
        u16,
        &TectonicState,
        &EvolutionMaterialLedger,
        &EvolutionLineageLedger,
    ) -> Result<(), RunnerError>,
) -> Result<EvolvedControlState, RunnerError> {
    streams.check_cancelled()?;
    let recipe = FormationTectonicRecipe::for_preset(formation.resolved());
    let timeline = formation.timeline();
    let delta_myr = timeline.step_duration_myr();
    let process_delta_myr = delta_myr as f32;
    let rift_delta_myr = rift_step_duration_myr(timeline)?;
    let initial = build_initial_state_v5(surface, topology, spec, formation.resolved(), streams)?;
    trace_continental_inventory("initial", 0, &initial);
    streams.check_cancelled()?;
    let mut material_ledger = EvolutionMaterialLedger::capture_initial(&initial)?;
    let mut lineage_ledger = EvolutionLineageLedger::capture_initial(&initial)?;
    let mut workspace = TectonicWorkspace::from_initial(initial);
    apply_boundary_torques_to_current(surface, topology, &mut workspace)?;

    for step in 0..timeline.step_count() {
        streams.check_cancelled()?;
        let (current, next, coverage, events, actions) = workspace.step_parts();
        advance_samples(surface, topology, current, next, delta_myr)?;
        build_contacts(surface, topology, next, coverage, events)?;
        update_rotations_from_boundary_torques(surface, next, events)?;
        actions.begin_step(next.samples.len());
        apply_subduction_v5(surface, events, current, next, actions, recipe, delta_myr)?;
        let collision = apply_collision_v5(
            surface,
            events,
            current,
            next,
            actions,
            recipe,
            &mut material_ledger,
            process_delta_myr,
        )?;
        lineage_ledger.record_terrane_transfers(collision.terrane_transfer_events);
        apply_divergent_extension_v5(
            surface,
            events,
            next,
            actions,
            &mut material_ledger,
            process_delta_myr,
        )?;
        fill_spreading_gaps_v5(
            surface,
            events,
            current,
            next,
            actions,
            recipe,
            &mut material_ledger,
        )?;
        maybe_rift_plates(
            step,
            rift_delta_myr,
            surface,
            current,
            next,
            actions,
            recipe,
            streams,
        )?;
        advance_solid_crust_ages(next, process_delta_myr)?;
        actions.preserve_minimum_live_lineages(
            &next.samples,
            &current.plates,
            &next.plates,
            usize::from(MIN_PLATE_COUNT),
        )?;
        commit_process_actions_v5(next, actions, &mut material_ledger)?;
        workspace.swap_current_next();
        if resample_due(&workspace) {
            resample_current_state_v5(surface, topology, &mut workspace, &mut material_ledger)?;
            trace_continental_inventory("resampled", step, &workspace.current);
            mechanically_fragment_oversized_plates_v5(
                step,
                surface,
                topology,
                &mut workspace.current,
                recipe,
                streams,
                &mut lineage_ledger,
            )?;
            on_resampled(
                step + 1,
                &workspace.current,
                &material_ledger,
                &lineage_ledger,
            )?;
        }
        streams.check_cancelled()?;
    }
    if workspace.requires_resample() {
        resample_current_state_v5(surface, topology, &mut workspace, &mut material_ledger)?;
        mechanically_fragment_oversized_plates_v5(
            timeline.step_count(),
            surface,
            topology,
            &mut workspace.current,
            recipe,
            streams,
            &mut lineage_ledger,
        )?;
        on_resampled(
            timeline.step_count(),
            &workspace.current,
            &material_ledger,
            &lineage_ledger,
        )?;
    }
    apply_boundary_torques_to_current(surface, topology, &mut workspace)?;
    trace_continental_inventory("final", timeline.step_count(), &workspace.current);
    material_ledger.control_budget(&workspace.current)?;
    lineage_ledger.budget(&workspace.current)?;
    streams.check_cancelled()?;
    let forcing =
        evaluate_present_day_forcing(surface, topology, &workspace.current, recipe, delta_myr)?;
    Ok(EvolvedControlState {
        current: workspace.current,
        forcing,
        material_ledger,
        lineage_ledger,
    })
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

fn apply_boundary_torques_to_current(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    workspace: &mut TectonicWorkspace,
) -> Result<(), RunnerError> {
    build_contacts(
        surface,
        topology,
        &workspace.current,
        &mut workspace.coverage,
        &mut workspace.events,
    )?;
    update_rotations_from_boundary_torques(surface, &mut workspace.current, &workspace.events)?;
    Ok(())
}

fn rift_step_duration_myr(timeline: ResolvedFormationTimeline) -> Result<u16, RunnerError> {
    let step_duration_kyr = timeline.step_duration_kyr();
    if step_duration_kyr % 1_000 != 0 {
        return Err(RunnerError::NonIntegralRiftStepDuration { step_duration_kyr });
    }
    u16::try_from(step_duration_kyr / 1_000)
        .map_err(|_| RunnerError::RiftStepDurationOverflow { step_duration_kyr })
}

/// Prints the area-weighted continental thickness inventory of one state when
/// `SEKAI_V5_TRACE` is set (the P5 `SEKAI_P5_TRACE` precedent): the T0
/// calibration diagnostic that locates where the thickness spread is lost.
fn trace_continental_inventory(label: &str, step: u16, state: &TectonicState) {
    if std::env::var_os("SEKAI_V5_TRACE").is_none() {
        return;
    }
    let mut samples = state
        .samples
        .iter()
        .filter_map(|sample| {
            let area = sample.material.continental_reference_area_m2();
            sample
                .material
                .continental_thickness_km()
                .filter(|_| area > 0.0)
                .map(|thickness| (f64::from(thickness), area))
        })
        .collect::<Vec<_>>();
    samples.sort_by(|first, second| first.0.total_cmp(&second.0));
    let total_area = samples.iter().map(|sample| sample.1).sum::<f64>();
    if total_area <= 0.0 {
        eprintln!("[v5-trace] {label} step {step}: no continental material");
        return;
    }
    let mean = samples
        .iter()
        .map(|&(thickness, area)| thickness * area)
        .sum::<f64>()
        / total_area;
    let sd = (samples
        .iter()
        .map(|&(thickness, area)| (thickness - mean).powi(2) * area)
        .sum::<f64>()
        / total_area)
        .sqrt();
    let quantile = |q: f64| {
        let target = q * total_area;
        let mut cumulative = 0.0;
        samples
            .iter()
            .find(|&&(_, area)| {
                cumulative += area;
                cumulative >= target
            })
            .map_or(f64::NAN, |sample| sample.0)
    };
    eprintln!(
        "[v5-trace] {label} step {step}: continental_samples={} area_m2={total_area:.4e} mean_km={mean:.2} sd_km={sd:.2} p05={:.1} p50={:.1} p95={:.1} min={:.1} max={:.1}",
        samples.len(),
        quantile(0.05),
        quantile(0.50),
        quantile(0.95),
        samples.first().map_or(f64::NAN, |sample| sample.0),
        samples.last().map_or(f64::NAN, |sample| sample.0),
    );
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
    #[error("tectonic material failed: {0}")]
    Material(#[from] super::model::MaterialColumnError),
    #[error("tectonic lineage failed: {0}")]
    Lineage(#[from] super::model::LineageLedgerError),
    #[error("present-day tectonic forcing failed: {0}")]
    Forcing(#[from] ForcingError),
    #[error("boundary torque solve failed: {0}")]
    Torques(#[from] TorqueError),
    #[error("rift step duration {step_duration_kyr} kyr is not an integer number of Myr")]
    NonIntegralRiftStepDuration { step_duration_kyr: u32 },
    #[error("rift step duration {step_duration_kyr} kyr exceeds its integer-Myr process domain")]
    RiftStepDurationOverflow { step_duration_kyr: u32 },
    #[error("tectonic evolution was cancelled")]
    Cancelled(#[from] BuildCancellationError),
    #[cfg(test)]
    #[error("test-only resample observer failed: {message}")]
    TestObserver { message: String },
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};
    use std::f64::consts::PI;

    use super::{evolve_control_state_v5, run_tectonic_evolution};
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::foundation::tectonics::contacts::{
        build_contacts, ContactKind, CoverageScratch,
    };
    use crate::generators::natural::foundation::tectonics::initial_state::{
        build_initial_state, build_initial_state_v5,
    };
    use crate::generators::natural::foundation::tectonics::model::{LineageId, TectonicState};
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        ResolvedWorldFormation, ResolvedWorldFormationPreset, TectonicActivity, TectonicSpec,
        WorldFormationPreset, RESOLVED_WORLD_FORMATION_SCHEMA_V1,
    };
    use crate::world::spatial::{project_tangent, SphericalNaturalSurface, UnitVector3};
    use crate::world::{EdgeId, Meters, RootSeed, SphericalSpaceSpec, SurfaceVertexId};

    fn lineage_pair(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        state: &TectonicState,
        edge: EdgeId,
    ) -> Option<[LineageId; 2]> {
        let cells = surface.edge(edge).unwrap().cells;
        let mut pair = cells.map(|cell| state.samples[cell.raw() as usize].owner);
        if pair[0] == pair[1] {
            None
        } else {
            if pair[1] < pair[0] {
                pair.swap(0, 1);
            }
            Some(pair)
        }
    }

    fn trace_lineage_branch(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        state: &TectonicState,
        incident: &[Vec<EdgeId>],
        start: SurfaceVertexId,
        first_edge: EdgeId,
        target_length_m: f64,
    ) -> SurfaceVertexId {
        let pair = lineage_pair(surface, state, first_edge).unwrap();
        let mut previous_vertex = start;
        let mut edge = first_edge;
        let mut length = 0.0;
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert(edge) {
                return previous_vertex;
            }
            let record = surface.edge(edge).unwrap();
            let next_vertex = if record.vertices[0] == previous_vertex {
                record.vertices[1]
            } else {
                record.vertices[0]
            };
            length += record.length.get();
            if length >= target_length_m {
                return next_vertex;
            }
            let candidates = incident[next_vertex.raw() as usize]
                .iter()
                .copied()
                .filter(|&candidate| candidate != edge)
                .filter(|&candidate| lineage_pair(surface, state, candidate) == Some(pair))
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                return next_vertex;
            }
            previous_vertex = next_vertex;
            edge = candidates[0];
        }
    }

    fn macro_triple_angles(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        state: &TectonicState,
    ) -> Vec<f64> {
        let mut incident = vec![Vec::new(); surface.vertices().len()];
        for edge in surface.edges() {
            if lineage_pair(surface, state, edge.id).is_some() {
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
                .flat_map(|&edge| lineage_pair(surface, state, edge).unwrap())
                .collect::<BTreeSet<_>>();
            if owners.len() != 3 || edges.len() != 3 {
                continue;
            }
            let radial = vertex.position;
            let (east, north) = tangent_basis(radial);
            let mut azimuths = edges
                .iter()
                .filter_map(|&edge| {
                    let endpoint =
                        trace_lineage_branch(surface, state, &incident, vertex.id, edge, 750_000.0);
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

    fn normalize(vector: [f64; 3]) -> [f64; 3] {
        let length = dot(vector, vector).sqrt();
        vector.map(|value| value / length)
    }

    fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
        [
            first[1] * second[2] - first[2] * second[1],
            first[2] * second[0] - first[0] * second[2],
            first[0] * second[1] - first[1] * second[0],
        ]
    }

    fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
        first.into_iter().zip(second).map(|(a, b)| a * b).sum()
    }

    fn resolved_formation(preset: ResolvedWorldFormationPreset) -> ResolvedWorldFormation {
        ResolvedWorldFormation::new(
            RESOLVED_WORLD_FORMATION_SCHEMA_V1,
            WorldFormationPreset::Continents,
            preset,
        )
        .unwrap()
    }

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
        let formation = resolved_formation(ResolvedWorldFormationPreset::Continents);
        let initial =
            build_initial_state(&surface, &topology, &spec, formation.resolved(), &streams)
                .unwrap()
                .initial_owners()
                .into_iter()
                .map(|lineage| lineage.raw())
                .collect::<Vec<_>>();
        let final_state =
            run_tectonic_evolution(&surface, &topology, &spec, &formation, &streams).unwrap();

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
            &resolved_formation(ResolvedWorldFormationPreset::Supercontinent),
            &streams,
        )
        .expect("the minimum supported plate configuration must remain publishable");

        assert!((2..=64).contains(&final_state.plates.len()));
    }

    #[test]
    fn evolved_control_path_closes_its_material_ledger_deterministically() {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 162,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(0xE701_ED55),
            StageIdentity::new("runner-evolved-material-test", 1, "sekai.test"),
        ));
        let streams = LabeledSubstreams::capture(&mut rng);
        let spec = TectonicSpec {
            plate_count: 8,
            continental_crust_fraction: 0.38,
            ..TectonicSpec::default()
        };
        let formation = resolved_formation(ResolvedWorldFormationPreset::Continents);
        let first =
            evolve_control_state_v5(&surface, &topology, &spec, &formation, &streams).unwrap();
        first
            .material_ledger
            .control_budget(&first.current)
            .unwrap();
        first.lineage_ledger.budget(&first.current).unwrap();

        let second =
            evolve_control_state_v5(&surface, &topology, &spec, &formation, &streams).unwrap();
        assert_eq!(
            first.current.material_totals().unwrap(),
            second.current.material_totals().unwrap()
        );
        assert_eq!(
            first.current.initial_owners(),
            second.current.initial_owners()
        );
        assert_eq!(first.current.plates, second.current.plates);
        assert_eq!(first.forcing, second.forcing);
        assert_eq!(
            first.lineage_ledger.budget(&first.current).unwrap(),
            second.lineage_ledger.budget(&second.current).unwrap()
        );
        assert_eq!(
            first
                .current
                .samples
                .iter()
                .map(|sample| sample.material.bits())
                .collect::<Vec<_>>(),
            second
                .current
                .samples
                .iter()
                .map(|sample| sample.material.bits())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn evolved_locked_corpus_keeps_connected_bounded_plates_and_continental_material() {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: 4_842,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let spec = TectonicSpec {
            plate_count: 12,
            continental_crust_fraction: 0.38,
            ..TectonicSpec::default()
        };
        let mut triple_angles = Vec::new();
        let mut initial_triple_angles = Vec::new();
        let mut subduction_total = 0_usize;
        let mut subduction_passed = 0_usize;
        let mut collision_total = 0_usize;
        let mut collision_passed = 0_usize;
        let mut transform_uplift = Vec::new();
        let mut convergent_uplift = Vec::new();
        let formation = resolved_formation(ResolvedWorldFormationPreset::Continents);
        for seed in [
            42, 3, 7, 11, 19, 23, 29, 31, 43, 47, 59, 61, 71, 73, 83, 89, 97,
        ] {
            let mut rng = StageRng::from_seed(derive_stage_seed(
                RootSeed::new(seed),
                StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
            ));
            let streams = LabeledSubstreams::capture(&mut rng);
            let initial = build_initial_state_v5(
                &surface,
                &topology,
                &spec,
                ResolvedWorldFormationPreset::Continents,
                &streams,
            )
            .unwrap();
            initial_triple_angles.extend(macro_triple_angles(&surface, &initial));
            let evolved =
                evolve_control_state_v5(&surface, &topology, &spec, &formation, &streams).unwrap();
            let initial_continental = evolved
                .material_ledger
                .initial_control()
                .continental()
                .reference_area_m2();
            let final_continental = evolved
                .current
                .material_totals()
                .unwrap()
                .continental()
                .reference_area_m2();
            let retention = final_continental / initial_continental;
            assert!(
                (0.75..=1.15).contains(&retention),
                "seed {seed}: {retention}"
            );

            let mut maximum_share = 0.0_f64;
            for plate in &evolved.current.plates {
                let indices = evolved
                    .current
                    .samples
                    .iter()
                    .enumerate()
                    .filter(|(_, sample)| sample.owner == plate.lineage)
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                let area = indices
                    .iter()
                    .map(|&index| surface.cells()[index].area.get())
                    .sum::<f64>();
                maximum_share = maximum_share.max(area / surface.total_cell_area().get());
                let mut reached = vec![false; surface.cells().len()];
                let mut queue = VecDeque::from([indices[0]]);
                reached[indices[0]] = true;
                while let Some(cell) = queue.pop_front() {
                    for arc in &topology.arcs()[cell] {
                        let neighbor = arc.neighbor.raw() as usize;
                        if !reached[neighbor]
                            && evolved.current.samples[neighbor].owner == plate.lineage
                        {
                            reached[neighbor] = true;
                            queue.push_back(neighbor);
                        }
                    }
                }
                assert_eq!(
                    reached.iter().filter(|&&value| value).count(),
                    indices.len(),
                    "seed {seed}: disconnected {:?}",
                    plate.lineage
                );
            }
            assert!(maximum_share <= 0.45, "seed {seed}: {maximum_share}");
            let lineage = evolved.lineage_ledger.budget(&evolved.current).unwrap();
            let mut coverage = CoverageScratch::with_cell_capacity(surface.cells().len());
            let mut events = Vec::new();
            build_contacts(
                &surface,
                &topology,
                &evolved.current,
                &mut coverage,
                &mut events,
            )
            .unwrap();
            let uplift = evolved.forcing.uplift_rate_mm_per_year();
            let subsidence = evolved.forcing.subsidence_rate_mm_per_year();
            let shortening = evolved.forcing.shortening_rate_mm_per_year();
            for event in &events {
                let [Some(first), Some(second)] = event.sample_indices else {
                    continue;
                };
                let first = first as usize;
                let second = second as usize;
                let first_cell = evolved.current.samples[first].anchor.raw() as usize;
                let second_cell = evolved.current.samples[second].anchor.raw() as usize;
                match event.kind {
                    ContactKind::OceanicSubduction { descending } => {
                        subduction_total += 1;
                        let (descending_cell, overriding_cell) =
                            if evolved.current.samples[first].owner == descending {
                                (first_cell, second_cell)
                            } else {
                                (second_cell, first_cell)
                            };
                        subduction_passed += usize::from(
                            subsidence[descending_cell] > 0.0 && uplift[overriding_cell] > 0.0,
                        );
                        convergent_uplift.push(uplift[overriding_cell]);
                    }
                    ContactKind::ContinentalCollision => {
                        collision_total += 1;
                        collision_passed += usize::from(
                            shortening[first_cell] > 0.0
                                && shortening[second_cell] > 0.0
                                && uplift[first_cell] > 0.0
                                && uplift[second_cell] > 0.0,
                        );
                        convergent_uplift.extend([uplift[first_cell], uplift[second_cell]]);
                    }
                    ContactKind::Transform => {
                        transform_uplift.extend([uplift[first_cell], uplift[second_cell]]);
                    }
                    ContactKind::Gap | ContactKind::Divergence => {}
                }
            }
            triple_angles.extend(macro_triple_angles(&surface, &evolved.current));
            eprintln!(
                "evolved seed={seed} retention={retention:.6} max_plate={maximum_share:.6} plates={} fragmentations={}",
                evolved.current.plates.len(),
                lineage.mechanical_fragmentation_count()
            );
        }
        assert!(!triple_angles.is_empty());
        let regular_fraction = triple_angles
            .iter()
            .filter(|&&angle| (angle - 120.0).abs() <= 10.0)
            .count() as f64
            / triple_angles.len() as f64;
        let initial_regular_fraction = initial_triple_angles
            .iter()
            .filter(|&&angle| (angle - 120.0).abs() <= 10.0)
            .count() as f64
            / initial_triple_angles.len() as f64;
        eprintln!(
            "evolved macro triple angles={} regular120={regular_fraction:.6}; initial angles={} regular120={initial_regular_fraction:.6}",
            triple_angles.len(),
            initial_triple_angles.len(),
        );
        let subduction_fraction = subduction_passed as f64 / subduction_total as f64;
        let collision_fraction = collision_passed as f64 / collision_total as f64;
        let transform_median = median(&mut transform_uplift);
        let convergent_median = median(&mut convergent_uplift);
        eprintln!(
            "forcing subduction={subduction_passed}/{subduction_total} ({subduction_fraction:.6}) collision={collision_passed}/{collision_total} ({collision_fraction:.6}) transform_uplift_median={transform_median:.6} convergent_uplift_median={convergent_median:.6}"
        );
        assert!(regular_fraction <= 0.35, "{regular_fraction}");
        assert!(subduction_total > 0);
        assert!(collision_total > 0);
        assert!(subduction_fraction >= 0.80, "{subduction_fraction}");
        assert!(collision_fraction >= 0.80, "{collision_fraction}");
        assert!(
            transform_median <= convergent_median * 0.5,
            "transform={transform_median} convergent={convergent_median}"
        );
    }

    fn median(values: &mut [f32]) -> f64 {
        assert!(!values.is_empty());
        values.sort_by(f32::total_cmp);
        let middle = values.len() / 2;
        if values.len() % 2 == 0 {
            (f64::from(values[middle - 1]) + f64::from(values[middle])) * 0.5
        } else {
            f64::from(values[middle])
        }
    }
}
