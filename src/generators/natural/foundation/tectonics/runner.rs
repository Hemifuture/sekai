//! The sole bounded current-state tectonic evolution loop.
//!
//! Every step consumes only `current`, writes only reusable `next`, commits
//! process actions once, and discards the overwritten state. No history or
//! alternate final-owner path exists here.

use thiserror::Error;

use super::contacts::{build_contacts, ContactError, ContactEvent, ContactKind, CoverageScratch};
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
    mechanically_fragment_oversized_plates_v5, rebin_interior_gaps_v5,
    relax_legacy_compatibility_elevation, ProcessError, RiftFill,
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
        remember_established_trenches(next, events);
        actions.begin_step(next.samples.len());
        apply_subduction(surface, events, current, next, actions, recipe, delta_myr)?;
        apply_collision(surface, events, current, next, actions, recipe)?;
        apply_divergent_extension(surface, events, next, actions, process_delta_myr)?;
        fill_spreading_gaps(
            surface,
            events,
            current,
            next,
            actions,
            recipe,
            RiftFill::LegacyImmediateOcean,
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
        solve_quasi_static_rotations(surface, topology, next, coverage, events)?;
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
        let rebin =
            rebin_interior_gaps_v5(surface, topology, events, current, next, coverage, actions)?;
        let fill = fill_spreading_gaps_v5(
            surface,
            events,
            current,
            next,
            actions,
            recipe,
            &mut material_ledger,
        )?;
        if std::env::var_os("SEKAI_V5_TRACE").is_some() {
            eprintln!(
                "[v5-trace] step {step}: rebinned={} split={} spawned={}",
                rebin.rebinned_samples, rebin.split_fills, fill.spawned_samples
            );
        }
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
        if std::env::var_os("SEKAI_V5_TRACE").is_some() {
            let totals = workspace.current.material_totals()?;
            let processes = material_ledger.processes()?;
            eprintln!(
                "[v5-trace] step {step} committed: cont_vol_ratio={:.6} cont_area_ratio={:.6}",
                totals.continental().volume_m3()
                    / (material_ledger.initial_control().continental().volume_m3()
                        - processes.continental_consumed().volume_m3()),
                totals.continental().reference_area_m2()
                    / (material_ledger
                        .initial_control()
                        .continental()
                        .reference_area_m2()
                        + processes.rift_extension_continental_area_gain_m2()
                        - processes.collision_shortening_continental_area_loss_m2()
                        - processes.continental_consumed().reference_area_m2()),
            );
        }
        if resample_due(&workspace) {
            resample_current_state_v5(surface, topology, &mut workspace, &mut material_ledger)?;
            trace_continental_inventory("resampled", step, &workspace.current);
            if std::env::var_os("SEKAI_V5_TRACE").is_some() {
                let totals = workspace.current.material_totals()?;
                let processes = material_ledger.processes()?;
                eprintln!(
                    "[v5-trace] step {step} resampled: cont_vol_ratio={:.6}",
                    totals.continental().volume_m3()
                        / (material_ledger.initial_control().continental().volume_m3()
                            - processes.continental_consumed().volume_m3()),
                );
            }
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
    solve_quasi_static_rotations(
        surface,
        topology,
        &mut workspace.current,
        &mut workspace.coverage,
        &mut workspace.events,
    )
}

/// Boundary classification depends on the velocities the torque balance
/// produces from that classification, so the quasi-static solve is a fixed
/// point: classify, solve, reclassify, solve, and classify once more so the
/// events the processes consume match the rotations the step advances with.
/// Measured on the draft corpus: the second sweep costs about 10% of P2 time
/// and leaves locked and collision residuals unchanged; the 256 Myr end state
/// is chaotic enough that dropping it reshuffles which blocks suture, so the
/// count the gate corpus was pinned with is kept.
const QUASI_STATIC_SWEEPS: usize = 2;

fn solve_quasi_static_rotations(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    state: &mut TectonicState,
    coverage: &mut CoverageScratch,
    events: &mut Vec<ContactEvent>,
) -> Result<(), RunnerError> {
    for _ in 0..QUASI_STATIC_SWEEPS {
        build_contacts(surface, topology, state, coverage, events)?;
        remember_established_trenches(state, events);
        update_rotations_from_boundary_torques(surface, state, events)?;
    }
    build_contacts(surface, topology, state, coverage, events)?;
    remember_established_trenches(state, events);
    Ok(())
}

fn remember_established_trenches(state: &mut TectonicState, events: &[ContactEvent]) {
    for event in events {
        let [Some(first), Some(second)] = event.lineages else {
            continue;
        };
        match event.kind {
            ContactKind::OceanicSubduction { .. } => state.initiation.record_trench(first, second),
            ContactKind::ContinentalCollision | ContactKind::LockedConvergence => {
                state.initiation.record_resisted(first, second);
            }
            ContactKind::Divergence => state.initiation.release_resisted(first, second),
            ContactKind::Gap | ContactKind::Transform => {}
        }
    }
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

    use super::{
        evolve_control_state_v5, evolve_control_state_v5_with_resample_observer,
        run_tectonic_evolution,
    };
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
                    ContactKind::Gap | ContactKind::Divergence | ContactKind::LockedConvergence => {
                    }
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

    #[test]
    #[ignore]
    fn probe_archipelago_opening_control_connectivity() {
        use crate::engine::BuildCancellation;
        use crate::generators::spatial::ProfileSurfaceBuilder;
        use crate::world::natural::{
            CrustKind, NaturalQualityProfile, EARTH_WATER_REFERENCE_RADIUS_M,
        };

        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let surface = bundle.tectonic_control_surface();
        let view = SphericalNaturalSurface::from_validated(surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        for preset in [
            ResolvedWorldFormationPreset::Archipelago,
            ResolvedWorldFormationPreset::Continents,
        ] {
            for seed in [42_u64, 3] {
                for plate_count in [12_u16, 22] {
                    let spec = TectonicSpec {
                        plate_count,
                        continental_crust_fraction: preset.recommended_continental_crust_fraction(),
                        ..TectonicSpec::default()
                    };
                    let mut rng = StageRng::from_seed(derive_stage_seed(
                        RootSeed::new(seed),
                        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
                    ));
                    let streams = LabeledSubstreams::capture(&mut rng);
                    let state = build_initial_state_v5(surface, &topology, &spec, preset, &streams)
                        .unwrap();
                    let mut continental = vec![false; surface.cells().len()];
                    let mut owner_at = vec![None; surface.cells().len()];
                    for sample in &state.samples {
                        let index = sample.anchor.raw() as usize;
                        owner_at[index] = Some(sample.owner);
                        continental[index] = sample.kind == CrustKind::Continental;
                    }
                    let mut seen = vec![false; surface.cells().len()];
                    let mut areas = Vec::new();
                    let mut largest_plates = 0_usize;
                    let mut best_area = 0.0;
                    for start in 0..surface.cells().len() {
                        if !continental[start] || seen[start] {
                            continue;
                        }
                        let mut stack = vec![start];
                        seen[start] = true;
                        let mut area = 0.0;
                        let mut plates = BTreeSet::new();
                        while let Some(cell) = stack.pop() {
                            area += surface.cells()[cell].area.get();
                            if let Some(owner) = owner_at[cell] {
                                plates.insert(owner);
                            }
                            for arc in &topology.arcs()[cell] {
                                let neighbor = arc.neighbor.raw() as usize;
                                if neighbor >= surface.cells().len()
                                    || !continental[neighbor]
                                    || seen[neighbor]
                                {
                                    continue;
                                }
                                seen[neighbor] = true;
                                stack.push(neighbor);
                            }
                        }
                        if area > best_area {
                            best_area = area;
                            largest_plates = plates.len();
                        }
                        areas.push(area);
                    }
                    areas.sort_by(|first, second| second.total_cmp(first));
                    let total: f64 = areas.iter().sum();
                    let share = |index: usize| {
                        if total > 0.0 {
                            areas.get(index).copied().unwrap_or(0.0) / total
                        } else {
                            0.0
                        }
                    };
                    println!(
                        "G1d-open {preset:?} seed={seed} plates={plate_count} crust_n={} max={:.3} second={:.3} largest_plates={}",
                        areas.len(),
                        share(0),
                        share(1),
                        largest_plates
                    );
                }
            }
        }
    }

    /// G1e §5 evidence probe: plate speeds, residual convergence on resisted
    /// boundaries, resample overlap displacement, and crust connectivity on
    /// the draft control surface.
    #[test]
    #[ignore]
    fn probe_g1e_convergence_closure() {
        use crate::engine::BuildCancellation;
        use crate::generators::spatial::ProfileSurfaceBuilder;
        use crate::world::natural::{
            CrustKind, NaturalQualityProfile, EARTH_WATER_REFERENCE_RADIUS_M,
        };

        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let surface = bundle.tectonic_control_surface();
        let view = SphericalNaturalSurface::from_validated(surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        for preset in [
            ResolvedWorldFormationPreset::Archipelago,
            ResolvedWorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Supercontinent,
        ] {
            for seed in [42_u64, 3] {
                for plate_count in [12_u16, 22] {
                    let spec = TectonicSpec {
                        plate_count,
                        continental_crust_fraction: preset.recommended_continental_crust_fraction(),
                        ..TectonicSpec::default()
                    };
                    let mut rng = StageRng::from_seed(derive_stage_seed(
                        RootSeed::new(seed),
                        StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
                    ));
                    let streams = LabeledSubstreams::capture(&mut rng);
                    let formation = resolved_formation(preset);
                    let opening = {
                        let initial =
                            build_initial_state_v5(surface, &topology, &spec, preset, &streams)
                                .unwrap();
                        let mut workspace =
                            super::super::workspace::TectonicWorkspace::from_initial(initial);
                        super::apply_boundary_torques_to_current(
                            surface,
                            &topology,
                            &mut workspace,
                        )
                        .unwrap();
                        let largest = workspace
                            .current
                            .plates
                            .iter()
                            .map(|plate| {
                                (
                                    workspace
                                        .current
                                        .samples
                                        .iter()
                                        .filter(|sample| sample.owner == plate.lineage)
                                        .count(),
                                    plate.lineage,
                                )
                            })
                            .max()
                            .map(|(_, lineage)| lineage)
                            .unwrap();
                        let mut girdle = 0_usize;
                        let mut outer_descends = 0_usize;
                        for event in &workspace.events {
                            if let ContactKind::OceanicSubduction { descending } = event.kind {
                                if event.lineages.contains(&Some(largest)) {
                                    girdle += 1;
                                    outer_descends += usize::from(descending != largest);
                                }
                            }
                        }
                        (workspace.current.plates.len(), girdle, outer_descends)
                    };
                    let mut trajectory = Vec::new();
                    let opening_roughness = continental_thickness_roughness(
                        surface,
                        &build_initial_state_v5(surface, &topology, &spec, preset, &streams)
                            .unwrap(),
                    );
                    let evolved = evolve_control_state_v5_with_resample_observer(
                        surface,
                        &topology,
                        &spec,
                        &formation,
                        &streams,
                        |step, state, ledger, _| {
                            let (components, max_share) =
                                continental_component_shares(surface, &topology, state);
                            let initial = ledger.initial_control();
                            let processes = ledger.processes().unwrap();
                            let found = state.material_totals().unwrap();
                            let cont_area = found.continental().reference_area_m2()
                                / (initial.continental().reference_area_m2()
                                    + processes.rift_extension_continental_area_gain_m2()
                                    - processes.collision_shortening_continental_area_loss_m2()
                                    - processes.continental_consumed().reference_area_m2());
                            let cont_vol = found.continental().volume_m3()
                                / (initial.continental().volume_m3()
                                    - processes.continental_consumed().volume_m3());
                            let oce_area = found.oceanic().reference_area_m2()
                                / (initial.oceanic().reference_area_m2()
                                    + processes.oceanic_spreading_created().reference_area_m2()
                                    + processes.oceanic_coverage_created().reference_area_m2()
                                    - processes.oceanic_subducted().reference_area_m2()
                                    - processes.oceanic_coverage_consumed().reference_area_m2());
                            let oce_vol = found.oceanic().volume_m3()
                                / (initial.oceanic().volume_m3()
                                    + processes.oceanic_spreading_created().volume_m3()
                                    + processes.oceanic_coverage_created().volume_m3()
                                    - processes.oceanic_subducted().volume_m3()
                                    - processes.oceanic_coverage_consumed().volume_m3());
                            let entry = format!(
                                "{step}:{}p/{components}c/{max_share:.2}[{cont_area:.3}/{cont_vol:.3}/{oce_area:.3}/{oce_vol:.3}]",
                                state.plates.len()
                            );
                            println!("G1e-closure {preset:?} seed={seed} plates={plate_count} {entry}");
                            trajectory.push(entry);
                            Ok(())
                        },
                    )
                    .unwrap();
                    println!(
                        "G1e-traj {preset:?} seed={seed} plates={plate_count} {}",
                        trajectory.join(" ")
                    );
                    println!(
                        "G1e-rough {preset:?} seed={seed} plates={plate_count} opening={opening_roughness:.2} final={:.2} km",
                        continental_thickness_roughness(surface, &evolved.current)
                    );
                    let state = &evolved.current;
                    let mut coverage = CoverageScratch::with_cell_capacity(surface.cells().len());
                    let mut events = Vec::new();
                    build_contacts(surface, &topology, state, &mut coverage, &mut events).unwrap();

                    let mut descending = BTreeSet::new();
                    let mut locked_residual = Vec::new();
                    let mut collision_residual = Vec::new();
                    let mut counts = [0_usize; 6];
                    for event in &events {
                        match event.kind {
                            ContactKind::OceanicSubduction { descending: plate } => {
                                descending.insert(plate);
                                counts[0] += 1;
                            }
                            ContactKind::LockedConvergence => {
                                if event.edge.is_some() {
                                    locked_residual.push(-event.signed_normal_speed_mm_per_year);
                                }
                                counts[1] += 1;
                            }
                            ContactKind::ContinentalCollision => {
                                if event.edge.is_some() {
                                    collision_residual.push(-event.signed_normal_speed_mm_per_year);
                                }
                                counts[2] += 1;
                            }
                            ContactKind::Divergence => counts[3] += 1,
                            ContactKind::Transform => counts[4] += 1,
                            ContactKind::Gap => counts[5] += 1,
                        }
                    }
                    let mut slab_speeds = Vec::new();
                    let mut free_speeds = Vec::new();
                    for plate in &state.plates {
                        let centroid =
                            surface.cells()[plate.representative.raw() as usize].centroid;
                        let velocity = plate
                            .rotation
                            .velocity_mm_per_year(surface.radius(), centroid)
                            .unwrap();
                        let speed = dot(velocity, velocity).sqrt() as f32;
                        if descending.contains(&plate.lineage) {
                            slab_speeds.push(speed);
                        } else {
                            free_speeds.push(speed);
                        }
                    }
                    let continental_area = state
                        .samples
                        .iter()
                        .filter(|sample| sample.kind == CrustKind::Continental)
                        .map(|sample| surface.cells()[sample.anchor.raw() as usize].area.get())
                        .sum::<f64>();
                    let (crust_components, crust_max_share) =
                        continental_component_shares(surface, &topology, state);
                    let stat = |values: &mut Vec<f32>| {
                        if values.is_empty() {
                            (f32::NAN, f32::NAN, f32::NAN)
                        } else {
                            values.sort_by(f32::total_cmp);
                            (
                                values[0],
                                values[values.len() / 2],
                                values[values.len() - 1],
                            )
                        }
                    };
                    let sphere = surface.total_cell_area().get();
                    let processes = evolved.material_ledger.processes().unwrap();
                    let initial_ocean = evolved
                        .material_ledger
                        .initial_control()
                        .oceanic()
                        .reference_area_m2();
                    let final_ocean = state
                        .material_totals()
                        .unwrap()
                        .oceanic()
                        .reference_area_m2();
                    let spreading = processes.oceanic_spreading_created().reference_area_m2();
                    let subducted = processes.oceanic_subducted().reference_area_m2();
                    let slab = stat(&mut slab_speeds);
                    let free = stat(&mut free_speeds);
                    let locked = stat(&mut locked_residual);
                    let collision = stat(&mut collision_residual);
                    println!(
                        "G1e {preset:?} seed={seed} plates={plate_count} opening[plates/girdle/outer_descends]={:?} final_plates={} events[subd/locked/coll/div/tf/gap]={counts:?} slab_speed(min/med/max)={:.1}/{:.1}/{:.1} free_speed={:.1}/{:.1}/{:.1} locked_residual={:.1}/{:.1}/{:.1} collision_residual={:.1}/{:.1}/{:.1} overlap_moved/cont_area={:.4} ocean[spread/subd/absorbed]/sphere={:.3}/{:.3}/{:.3} crust_n={crust_components} crust_max={crust_max_share:.3}",
                        opening,
                        state.plates.len(),
                        slab.0, slab.1, slab.2,
                        free.0, free.1, free.2,
                        locked.0, locked.1, locked.2,
                        collision.0, collision.1, collision.2,
                        evolved.material_ledger.resample_overlap_moved_area_m2() / continental_area,
                        spreading / sphere,
                        subducted / sphere,
                        (spreading - subducted - (final_ocean - initial_ocean)) / sphere,
                    );
                }
            }
        }
    }

    /// Dumps the control-surface crust mask at every resample as an
    /// equirectangular PNG under `target/natural-quality/g1e/` (research only).
    #[test]
    #[ignore]
    fn probe_g1e_render_crust_trajectory() {
        use crate::engine::BuildCancellation;
        use crate::generators::spatial::ProfileSurfaceBuilder;
        use crate::world::natural::{
            CrustKind, NaturalQualityProfile, EARTH_WATER_REFERENCE_RADIUS_M,
        };

        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let surface = bundle.tectonic_control_surface();
        let view = SphericalNaturalSurface::from_validated(surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("natural-quality")
            .join("g1e");
        std::fs::create_dir_all(&dir).unwrap();
        let (width, height) = (360_u32, 180_u32);
        let mut raster = vec![0_usize; (width * height) as usize];
        for y in 0..height {
            let lat = std::f64::consts::PI * (0.5 - (f64::from(y) + 0.5) / f64::from(height));
            for x in 0..width {
                let lon =
                    2.0 * std::f64::consts::PI * ((f64::from(x) + 0.5) / f64::from(width) - 0.5);
                let direction = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];
                let mut best = (f64::NEG_INFINITY, 0_usize);
                for (index, cell) in surface.cells().iter().enumerate() {
                    let score = dot(cell.centroid.components(), direction);
                    if score > best.0 {
                        best = (score, index);
                    }
                }
                raster[(y * width + x) as usize] = best.1;
            }
        }
        let render = |name: &str, state: &TectonicState| {
            let mut kind = vec![CrustKind::Oceanic; surface.cells().len()];
            let mut owner = vec![0_u32; surface.cells().len()];
            for sample in &state.samples {
                kind[sample.anchor.raw() as usize] = sample.kind;
                owner[sample.anchor.raw() as usize] = sample.owner.raw();
            }
            let mut img = image::RgbImage::new(width, height);
            for (pixel, &cell) in raster.iter().enumerate() {
                let hue = (owner[cell] * 47 % 255) as u8;
                let rgb = match kind[cell] {
                    CrustKind::Continental => [200, 60 + hue / 3, 40],
                    CrustKind::Oceanic => [30, 60, 120 + hue / 2],
                };
                img.put_pixel(
                    (pixel as u32) % width,
                    (pixel as u32) / width,
                    image::Rgb(rgb),
                );
            }
            img.save(dir.join(format!("{name}.png"))).unwrap();
        };
        for (preset, seed, plate_count) in [
            (ResolvedWorldFormationPreset::Continents, 42_u64, 12_u16),
            (ResolvedWorldFormationPreset::Archipelago, 42, 12),
        ] {
            let spec = TectonicSpec {
                plate_count,
                continental_crust_fraction: preset.recommended_continental_crust_fraction(),
                ..TectonicSpec::default()
            };
            let mut rng = StageRng::from_seed(derive_stage_seed(
                RootSeed::new(seed),
                StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
            ));
            let streams = LabeledSubstreams::capture(&mut rng);
            let formation = resolved_formation(preset);
            let initial =
                build_initial_state_v5(surface, &topology, &spec, preset, &streams).unwrap();
            render(&format!("{preset:?}-{seed}-{plate_count}-000"), &initial);
            evolve_control_state_v5_with_resample_observer(
                surface,
                &topology,
                &spec,
                &formation,
                &streams,
                |step, state, _, _| {
                    render(&format!("{preset:?}-{seed}-{plate_count}-{step:03}"), state);
                    Ok(())
                },
            )
            .unwrap();
        }
        println!("wrote {}", dir.display());
    }

    /// Diagnostic: every plate shares one rotation, so nothing but the
    /// advection and remap machinery acts. The crust mask must stay a rigid
    /// copy of the opening mask (constant component count).
    #[test]
    #[ignore]
    fn probe_g1e_rigid_rotation_keeps_mask() {
        use super::super::model::EvolutionMaterialLedger;
        use super::super::processes::{
            advance_solid_crust_ages, apply_collision_v5, apply_divergent_extension_v5,
            apply_subduction_v5, commit_process_actions_v5, fill_spreading_gaps_v5,
            rebin_interior_gaps_v5,
        };
        use super::super::resample::resample_current_state_v5;
        use super::super::workspace::TectonicWorkspace;
        use crate::engine::BuildCancellation;
        use crate::generators::spatial::ProfileSurfaceBuilder;
        use crate::world::natural::{
            NaturalQualityProfile, SphericalPlateRotation, EARTH_WATER_REFERENCE_RADIUS_M,
        };
        use crate::world::spatial::UnitVector3;

        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let surface = bundle.tectonic_control_surface();
        let view = SphericalNaturalSurface::from_validated(surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let preset = ResolvedWorldFormationPreset::Continents;
        let spec = TectonicSpec {
            continental_crust_fraction: preset.recommended_continental_crust_fraction(),
            ..TectonicSpec::default()
        };
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
        ));
        let streams = LabeledSubstreams::capture(&mut rng);
        let recipe = super::FormationTectonicRecipe::for_preset(preset);
        let mut initial =
            build_initial_state_v5(surface, &topology, &spec, preset, &streams).unwrap();
        let shared = SphericalPlateRotation::new(
            UnitVector3::new(0.3, 0.4, 0.866).unwrap(),
            (60.0e-3_f64 * 1.0e12 / surface.radius().get()) as u64,
        )
        .unwrap();
        for plate in &mut initial.plates {
            plate.rotation = shared;
        }
        let (opening_components, opening_max) =
            continental_component_shares(surface, &topology, &initial);
        let mut ledger = EvolutionMaterialLedger::capture_initial(&initial).unwrap();
        let mut workspace = TectonicWorkspace::from_initial(initial);
        let delta_myr = 2.0_f64;
        let mut log = Vec::new();
        for step in 0..128_u16 {
            let (current, next, coverage, events, actions) = workspace.step_parts();
            super::advance_samples(surface, &topology, current, next, delta_myr).unwrap();
            build_contacts(surface, &topology, next, coverage, events).unwrap();
            actions.begin_step(next.samples.len());
            apply_subduction_v5(surface, events, current, next, actions, recipe, delta_myr)
                .unwrap();
            apply_collision_v5(
                surface,
                events,
                current,
                next,
                actions,
                recipe,
                &mut ledger,
                2.0,
            )
            .unwrap();
            apply_divergent_extension_v5(surface, events, next, actions, &mut ledger, 2.0).unwrap();
            let gaps = events
                .iter()
                .filter(|event| event.kind == ContactKind::Gap)
                .count();
            let rebin = rebin_interior_gaps_v5(
                surface, &topology, events, current, next, coverage, actions,
            )
            .unwrap();
            let fill = fill_spreading_gaps_v5(
                surface,
                events,
                current,
                next,
                actions,
                recipe,
                &mut ledger,
            )
            .unwrap();
            if std::env::var_os("G1E_RIGID_NO_RESAMPLE").is_some() && step < 6 {
                println!(
                    "G1e-rigid step {} gaps={gaps} rebinned={} split={} spawned={} events={}",
                    step + 1,
                    rebin.rebinned_samples,
                    rebin.split_fills,
                    fill.spawned_samples,
                    events.len()
                );
            }
            advance_solid_crust_ages(next, 2.0).unwrap();
            commit_process_actions_v5(next, actions, &mut ledger).unwrap();
            workspace.swap_current_next();
            let skip_resample = std::env::var_os("G1E_RIGID_NO_RESAMPLE").is_some();
            if skip_resample && step % 10 == 9 {
                log.push(format!(
                    "pre{}:r{:.2}",
                    step + 1,
                    continental_thickness_roughness(surface, &workspace.current)
                ));
            }
            if skip_resample && step < 6 {
                use crate::world::natural::CrustKind;
                let state = &workspace.current;
                let mut anchored = vec![0_u8; surface.cells().len()];
                let mut kinds = vec![[0_u8; 2]; surface.cells().len()];
                for sample in &state.samples {
                    let cell = sample.anchor.raw() as usize;
                    anchored[cell] += 1;
                    kinds[cell][usize::from(sample.kind == CrustKind::Continental)] += 1;
                }
                let holes = anchored.iter().filter(|&&n| n == 0).count();
                let doubles = anchored.iter().filter(|&&n| n >= 2).count();
                let mixed = kinds.iter().filter(|k| k[0] > 0 && k[1] > 0).count();
                let continental_cells = kinds.iter().filter(|k| k[1] > 0).count();
                let (components, max_share) =
                    continental_component_shares(surface, &topology, state);
                println!(
                    "G1e-rigid step {}: samples={} holes={holes} doubles={doubles} mixed={mixed} cont_cells={continental_cells} components={components} max={max_share:.2}",
                    step + 1,
                    state.samples.len()
                );
            }
            if super::resample_due(&workspace) {
                if !skip_resample {
                    resample_current_state_v5(surface, &topology, &mut workspace, &mut ledger)
                        .unwrap();
                }
                let (components, max_share) =
                    continental_component_shares(surface, &topology, &workspace.current);
                log.push(format!(
                    "{}:{components}c/{max_share:.2}/r{:.2}",
                    step + 1,
                    continental_thickness_roughness(surface, &workspace.current)
                ));
                if !skip_resample {
                    let initial = ledger.initial_control();
                    let processes = ledger.processes().unwrap();
                    let found = workspace.current.material_totals().unwrap();
                    let expected_cont_area = initial.continental().reference_area_m2()
                        + processes.rift_extension_continental_area_gain_m2()
                        - processes.collision_shortening_continental_area_loss_m2()
                        - processes.continental_consumed().reference_area_m2();
                    let expected_cont_volume = initial.continental().volume_m3()
                        - processes.continental_consumed().volume_m3();
                    let expected_oce_area = initial.oceanic().reference_area_m2()
                        + processes.oceanic_spreading_created().reference_area_m2()
                        + processes.oceanic_coverage_created().reference_area_m2()
                        - processes.oceanic_subducted().reference_area_m2()
                        - processes.oceanic_coverage_consumed().reference_area_m2();
                    let expected_oce_volume = initial.oceanic().volume_m3()
                        + processes.oceanic_spreading_created().volume_m3()
                        + processes.oceanic_coverage_created().volume_m3()
                        - processes.oceanic_subducted().volume_m3()
                        - processes.oceanic_coverage_consumed().volume_m3();
                    println!(
                        "G1e-rigid closure step {}: cont_area {:.4} cont_vol {:.4} oce_area {:.4} oce_vol {:.4}",
                        step + 1,
                        found.continental().reference_area_m2() / expected_cont_area,
                        found.continental().volume_m3() / expected_cont_volume,
                        found.oceanic().reference_area_m2() / expected_oce_area,
                        found.oceanic().volume_m3() / expected_oce_volume,
                    );
                }
                if skip_resample && step > 40 {
                    break;
                }
            }
        }
        println!(
            "G1e-rigid opening={opening_components}c/{opening_max:.2} {}",
            log.join(" ")
        );
        println!(
            "G1e-rigid roughness final={:.2} km",
            continental_thickness_roughness(surface, &workspace.current)
        );
    }

    /// G1e R3 regression: every corpus must close its material budget; seeds
    /// 8 (Continents) and 1 (Supercontinent) used to drop clamped oceanic
    /// rebalance residual (relative error 3.8e-4 / 9.1e-4).
    #[test]
    #[ignore]
    fn probe_g1e_closure_sweep() {
        use crate::engine::BuildCancellation;
        use crate::generators::spatial::ProfileSurfaceBuilder;
        use crate::world::natural::{NaturalQualityProfile, EARTH_WATER_REFERENCE_RADIUS_M};

        let bundle = ProfileSurfaceBuilder::build(
            NaturalQualityProfile::Draft,
            Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
            &BuildCancellation::new(),
        )
        .unwrap();
        let surface = bundle.tectonic_control_surface();
        let view = SphericalNaturalSurface::from_validated(surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let presets = [
            ResolvedWorldFormationPreset::Archipelago,
            ResolvedWorldFormationPreset::Continents,
            ResolvedWorldFormationPreset::Supercontinent,
        ];
        for (preset, seed) in presets
            .into_iter()
            .flat_map(|preset| (1..=16_u64).map(move |seed| (preset, seed)))
        {
            let spec = TectonicSpec {
                plate_count: 12,
                continental_crust_fraction: preset.recommended_continental_crust_fraction(),
                ..TectonicSpec::default()
            };
            let mut rng = StageRng::from_seed(derive_stage_seed(
                RootSeed::new(seed),
                StageIdentity::new("natural.evolved-tectonics", 5, "sekai.core"),
            ));
            let streams = LabeledSubstreams::capture(&mut rng);
            let formation = resolved_formation(preset);
            let result = evolve_control_state_v5_with_resample_observer(
                surface,
                &topology,
                &spec,
                &formation,
                &streams,
                |step, state, ledger, _| {
                    let initial = ledger.initial_control();
                    let processes = ledger.processes().unwrap();
                    let found = state.material_totals().unwrap();
                    let cont_area = found.continental().reference_area_m2()
                        / (initial.continental().reference_area_m2()
                            + processes.rift_extension_continental_area_gain_m2()
                            - processes.collision_shortening_continental_area_loss_m2()
                            - processes.continental_consumed().reference_area_m2());
                    let cont_vol = found.continental().volume_m3()
                        / (initial.continental().volume_m3()
                            - processes.continental_consumed().volume_m3());
                    let oce_area = found.oceanic().reference_area_m2()
                        / (initial.oceanic().reference_area_m2()
                            + processes.oceanic_spreading_created().reference_area_m2()
                            + processes.oceanic_coverage_created().reference_area_m2()
                            - processes.oceanic_subducted().reference_area_m2()
                            - processes.oceanic_coverage_consumed().reference_area_m2());
                    let oce_vol = found.oceanic().volume_m3()
                        / (initial.oceanic().volume_m3()
                            + processes.oceanic_spreading_created().volume_m3()
                            + processes.oceanic_coverage_created().volume_m3()
                            - processes.oceanic_subducted().volume_m3()
                            - processes.oceanic_coverage_consumed().volume_m3());
                    for (name, ratio) in [
                        ("cont_area", cont_area),
                        ("cont_vol", cont_vol),
                        ("oce_area", oce_area),
                        ("oce_vol", oce_vol),
                    ] {
                        assert!(
                            (ratio - 1.0).abs() <= 1.0e-4,
                            "{preset:?} seed={seed} step={step} {name} ratio {ratio}"
                        );
                    }
                    Ok(())
                },
            );
            println!(
                "G1e-sweep {preset:?} seed={seed} outcome={:?}",
                result.as_ref().err()
            );
            result.unwrap();
        }
    }

    /// Mean |thickness difference| across continental-continental edges, km.
    fn continental_thickness_roughness(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        state: &TectonicState,
    ) -> f64 {
        use crate::world::natural::CrustKind;
        let mut thickness = vec![None; surface.cells().len()];
        for sample in &state.samples {
            if sample.kind == CrustKind::Continental {
                thickness[sample.anchor.raw() as usize] =
                    sample.material.continental_thickness_km();
            }
        }
        let mut sum = 0.0;
        let mut count = 0_usize;
        for edge in surface.edges() {
            let [a, b] = edge.cells;
            if let (Some(x), Some(y)) = (thickness[a.raw() as usize], thickness[b.raw() as usize]) {
                sum += f64::from((x - y).abs());
                count += 1;
            }
        }
        if count == 0 {
            0.0
        } else {
            sum / count as f64
        }
    }

    fn continental_component_shares(
        surface: &crate::world::spatial::SphericalSurfaceSnapshot,
        topology: &NaturalTopologyIndex,
        state: &TectonicState,
    ) -> (usize, f64) {
        use crate::world::natural::CrustKind;
        let cell_count = surface.cells().len();
        let mut continental = vec![false; cell_count];
        for sample in &state.samples {
            if sample.kind == CrustKind::Continental {
                continental[sample.anchor.raw() as usize] = true;
            }
        }
        let mut seen = vec![false; cell_count];
        let mut areas = Vec::new();
        for start in 0..cell_count {
            if !continental[start] || seen[start] {
                continue;
            }
            seen[start] = true;
            let mut stack = vec![start];
            let mut area = 0.0;
            while let Some(cell) = stack.pop() {
                area += surface.cells()[cell].area.get();
                for arc in &topology.arcs()[cell] {
                    let neighbor = arc.neighbor.raw() as usize;
                    if continental[neighbor] && !seen[neighbor] {
                        seen[neighbor] = true;
                        stack.push(neighbor);
                    }
                }
            }
            areas.push(area);
        }
        let total: f64 = areas.iter().sum();
        let max = areas.iter().copied().fold(0.0, f64::max);
        (areas.len(), if total > 0.0 { max / total } else { 0.0 })
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
