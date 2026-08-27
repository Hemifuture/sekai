//! Cortial-style oceanic crust creation in divergent coverage gaps.
//!
//! Empty coverage cells receive young ridge-high oceanic samples blended to
//! the nearest moving plate. This is the paper's continuous seafloor-generation
//! process expressed against the fixed authoritative sphere; samples are queued
//! and appended only by the shared action commit.

use super::{
    bounded_elevation, constants, event_lineation, ProcessActions, ProcessError, ProcessStats,
};
use crate::generators::natural::foundation::crust_physics::continental_isostatic_elevation_m;
use std::collections::VecDeque;

use crate::generators::natural::foundation::tectonics::contacts::{
    ContactEvent, ContactKind, CoverageScratch,
};
use crate::generators::natural::foundation::tectonics::model::{
    CrustSample, EvolutionMaterialLedger, FormationTectonicRecipe, LineageId, MaterialColumn,
    TectonicState,
};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{
    CrustKind, SphericalOrogenyKind, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, NO_OROGENY_AGE_SENTINEL_MYR,
};
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::CellId;

// McKenzie-style homogeneous pure-shear extension expressed over one coarse
// rift zone. The bounded per-step beta prevents a single resolved step from
// exhausting continental crust while repeated divergence can still reach the
// public minimum-thickness contract.
const CONTINENTAL_RIFT_ZONE_WIDTH_M: f64 = 400_000.0;
/// Bound on the pure-shear factor of one step, shared by rift extension and its
/// inverse, collision shortening.
pub(super) const MAXIMUM_STEP_STRETCH_FACTOR: f64 = 1.2;

pub(in crate::generators::natural::foundation::tectonics) fn apply_divergent_extension(
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

/// V5 pure-shear extension updates the extensive continental footprint while
/// preserving continental volume. The legacy compatibility fields are always
/// re-derived from the resulting material column.
pub(in crate::generators::natural::foundation::tectonics) fn apply_divergent_extension_v5(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    ledger: &mut EvolutionMaterialLedger,
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
        let Some(old_thickness) = sample.material.continental_thickness_km() else {
            continue;
        };
        if *speed <= 0.0 {
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
        let requested_beta = (1.0 + extension_m / CONTINENTAL_RIFT_ZONE_WIDTH_M)
            .clamp(1.0, MAXIMUM_STEP_STRETCH_FACTOR);
        let remaining_area_gain = ledger.remaining_rift_extension_area_m2();
        if remaining_area_gain <= 0.0 {
            continue;
        }
        let budget_beta =
            1.0 + remaining_area_gain / sample.material.continental_reference_area_m2();
        let beta = requested_beta.min(budget_beta);
        let (extended, area_gain) = sample.material.extend_continental_pure_shear(beta)?;
        if area_gain <= 0.0 {
            continue;
        }
        let new_thickness = extended
            .continental_thickness_km()
            .expect("extended continental material remains present");
        let subsidence = continental_isostatic_elevation_m(new_thickness)
            - continental_isostatic_elevation_m(old_thickness);
        sample.material = extended;
        sample.synchronize_compatibility_from_material();
        sample.tectonic_elevation_m = bounded_elevation(sample.tectonic_elevation_m + subsidence);
        ledger.record_rift_extension_area_gain(area_gain);
        affected_samples += 1;
    }
    Ok(ProcessStats {
        affected_samples,
        ..ProcessStats::default()
    })
}

pub(in crate::generators::natural::foundation::tectonics) fn fill_spreading_gaps(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
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
        let keep_continental = divergence
            .and_then(|event| match event.lineages {
                [Some(first), Some(second)] => Some((first, second)),
                _ => None,
            })
            .is_some_and(|(first, second)| {
                recipe
                    .oceanization
                    .forbids_breakup_ocean(&next.initiation, first, second)
            });
        let sample = if keep_continental {
            match donate_continental_fill(next, divergence, owner, cell.centroid, cell.area.get())?
            {
                Some(material) => continental_rift_fill_sample(
                    cell.centroid,
                    gap.cell,
                    owner,
                    lineation,
                    material,
                ),
                None => oceanic_spreading_sample(
                    cell.centroid,
                    gap.cell,
                    owner,
                    lineation,
                    cell.area.get(),
                )?,
            }
        } else {
            oceanic_spreading_sample(cell.centroid, gap.cell, owner, lineation, cell.area.get())?
        };
        spawned.push(sample);
        stats.spawned_samples += 1;
    }
    Ok(stats)
}

/// Farthest a sampling hole looks for its own plate's duplicate sample. One
/// step moves samples by a fraction of a cell and a resample interval by at
/// most `TARGET_ANGULAR_DISPLACEMENT_RAD`, so holes and doubles of the same
/// rigid plate sit within a few cells of each other.
const REBIN_SEARCH_HOPS: u32 = 3;

/// Semi-Lagrangian rebinning of sampling holes (G1e §3.4).
///
/// A rigid plate keeps its sample count, so a hole that is not adjacent to a
/// divergent boundary is a discretization artifact paired with a same-owner
/// double covering nearby. Moving one duplicate into the hole is conservative;
/// spawning crust there would be spreading with no ridge, and the duplicate
/// would later be absorbed by resampling as consumption with no trench. Holes
/// that are rebinned are dropped from `events` so the ridge fill skips them.
pub(in crate::generators::natural::foundation::tectonics) fn rebin_interior_gaps_v5(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    events: &mut Vec<ContactEvent>,
    current: &TectonicState,
    next: &mut TectonicState,
    coverage: &CoverageScratch,
    actions: &mut ProcessActions,
) -> Result<ProcessStats, ProcessError> {
    actions.validate_for(next.samples.len())?;
    let cell_count = surface.cells().len();
    let mut free = (0..next.samples.len())
        .map(|sample| actions.is_untouched(sample))
        .collect::<Vec<_>>();
    let (divergence_by_cell, current_sample_by_cell, _) = actions.spreading_scratch(cell_count);
    index_incident_divergence(surface, events, divergence_by_cell)?;
    index_current_samples(surface, current, current_sample_by_cell)?;
    let mut spare = vec![0_u32; cell_count];
    for (cell, slot) in spare.iter_mut().enumerate() {
        let covering = coverage.sample_indices(CellId::from_raw(cell as u32));
        if covering.len() >= 2
            && covering.iter().all(|&raw| {
                next.samples[raw as usize].owner == next.samples[covering[0] as usize].owner
            })
        {
            *slot = covering.len() as u32 - 1;
        }
    }
    let mut rebinned = vec![false; cell_count];
    let mut visited = vec![u32::MAX; cell_count];
    let mut queue = VecDeque::new();
    let mut stats = ProcessStats::default();
    for (stamp, gap) in events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind == ContactKind::Gap)
    {
        let cell_index = gap.cell.raw() as usize;
        if divergence_by_cell[cell_index].is_some() {
            continue;
        }
        let Some(owner) = current_sample_by_cell[cell_index]
            .map(|sample| current.samples[sample].owner)
            .or_else(|| closest_state_owner(current, surface.cells()[cell_index].centroid))
        else {
            continue;
        };
        let stamp = stamp as u32;
        queue.clear();
        queue.push_back((cell_index, 0_u32));
        visited[cell_index] = stamp;
        let mut duplicate = None;
        while let Some((cell, depth)) = queue.pop_front() {
            if spare[cell] > 0 {
                let candidate = coverage
                    .sample_indices(CellId::from_raw(cell as u32))
                    .iter()
                    .map(|&raw| raw as usize)
                    .filter(|&sample| next.samples[sample].owner == owner && free[sample])
                    .max();
                if let Some(sample) = candidate {
                    spare[cell] -= 1;
                    free[sample] = false;
                    duplicate = Some(sample);
                    break;
                }
            }
            if depth >= REBIN_SEARCH_HOPS {
                continue;
            }
            for arc in &topology.arcs()[cell] {
                let neighbor = arc.neighbor.raw() as usize;
                if visited[neighbor] != stamp {
                    visited[neighbor] = stamp;
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
        if let Some(sample) = duplicate {
            next.samples[sample].position = surface.cells()[cell_index].centroid;
            next.samples[sample].anchor = gap.cell;
            rebinned[cell_index] = true;
            stats.rebinned_samples += 1;
        }
    }
    events.retain(|event| event.kind != ContactKind::Gap || !rebinned[event.cell.raw() as usize]);
    Ok(stats)
}

pub(in crate::generators::natural::foundation::tectonics) fn fill_spreading_gaps_v5(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
    ledger: &mut EvolutionMaterialLedger,
) -> Result<ProcessStats, ProcessError> {
    let first_spawn = actions.spawned_samples().len();
    let stats = fill_spreading_gaps(surface, events, current, next, actions, recipe)?;
    for sample in &actions.spawned_samples()[first_spawn..] {
        if sample.kind == CrustKind::Oceanic {
            ledger.record_oceanic_spreading(sample.material.oceanic_amount()?);
        }
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
) -> Option<crate::generators::natural::foundation::tectonics::model::LineageId> {
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
) -> Option<crate::generators::natural::foundation::tectonics::model::LineageId> {
    closest_owner(state.samples.iter().enumerate(), direction)
}

fn closest_owner<'a>(
    samples: impl Iterator<Item = (usize, &'a CrustSample)>,
    direction: crate::world::spatial::UnitVector3,
) -> Option<crate::generators::natural::foundation::tectonics::model::LineageId> {
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

fn oceanic_spreading_sample(
    position: crate::world::spatial::UnitVector3,
    anchor: crate::world::CellId,
    owner: LineageId,
    lineation: [f32; 2],
    area_m2: f64,
) -> Result<CrustSample, ProcessError> {
    Ok(CrustSample {
        position,
        anchor,
        owner,
        kind: CrustKind::Oceanic,
        thickness_km: 7.0,
        age_myr: 0.0,
        tectonic_elevation_m: constants::OCEANIC_RIDGE_ELEVATION_M,
        lineation,
        orogeny: SphericalOrogenyKind::None,
        orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
        material: MaterialColumn::pure(CrustKind::Oceanic, area_m2, 7.0)?,
    })
}

fn continental_rift_fill_sample(
    position: crate::world::spatial::UnitVector3,
    anchor: crate::world::CellId,
    owner: LineageId,
    lineation: [f32; 2],
    material: MaterialColumn,
) -> CrustSample {
    let thickness_km = material
        .continental_thickness_km()
        .expect("donated rift fill is continental");
    CrustSample {
        position,
        anchor,
        owner,
        kind: CrustKind::Continental,
        thickness_km,
        age_myr: CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
        tectonic_elevation_m: bounded_elevation(continental_isostatic_elevation_m(thickness_km)),
        lineation,
        orogeny: SphericalOrogenyKind::None,
        orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
        material,
    }
}

fn donate_continental_fill(
    next: &mut TectonicState,
    divergence: Option<&ContactEvent>,
    owner: LineageId,
    target: crate::world::spatial::UnitVector3,
    requested_area_m2: f64,
) -> Result<Option<MaterialColumn>, ProcessError> {
    let mut donors = Vec::new();
    match divergence.map(|event| event.lineages) {
        Some([Some(first), Some(second)]) => {
            donors.push(first);
            if second != first {
                donors.push(second);
            }
        }
        Some([Some(first), None] | [None, Some(first)]) => donors.push(first),
        _ => donors.push(owner),
    }
    let mut candidates: Vec<usize> = next
        .samples
        .iter()
        .enumerate()
        .filter(|(_, sample)| {
            donors.contains(&sample.owner) && sample.material.continental_reference_area_m2() > 1.0
        })
        .map(|(index, _)| index)
        .collect();
    candidates.sort_by(|&first, &second| {
        next.samples[second]
            .position
            .dot(target)
            .total_cmp(&next.samples[first].position.dot(target))
            .then_with(|| first.cmp(&second))
    });
    let mut remaining_need = requested_area_m2;
    let mut donated: Option<MaterialColumn> = None;
    for index in candidates {
        if remaining_need <= 0.0 {
            break;
        }
        let (left, taken) = next.samples[index]
            .material
            .extract_continental_area(remaining_need)?;
        next.samples[index].material = left;
        next.samples[index].synchronize_compatibility_from_material();
        let Some(taken) = taken else {
            continue;
        };
        remaining_need -= taken.continental_reference_area_m2();
        donated = Some(match donated {
            None => taken,
            Some(existing) => MaterialColumn::new(
                existing.continental_reference_area_m2() + taken.continental_reference_area_m2(),
                existing.continental_volume_m3() + taken.continental_volume_m3(),
                0.0,
                0.0,
            )?,
        });
    }
    Ok(donated)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_divergent_extension, apply_divergent_extension_v5, fill_spreading_gaps,
        fill_spreading_gaps_v5, rebin_interior_gaps_v5,
    };
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::foundation::tectonics::contacts::{
        ContactEvent, ContactKind, CoverageScratch,
    };
    use crate::generators::natural::foundation::tectonics::initial_state::build_initial_state;
    use crate::generators::natural::foundation::tectonics::model::{
        EvolutionMaterialLedger, FormationTectonicRecipe, TectonicState,
    };
    use crate::generators::natural::foundation::tectonics::processes::{
        commit_process_actions, commit_process_actions_v5, constants, ProcessActions,
    };
    use crate::generators::natural::random::LabeledSubstreams;
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, ResolvedFormationTimeline, ResolvedWorldFormationPreset, TectonicSpec,
    };
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

    fn step_duration_myr() -> f32 {
        ResolvedFormationTimeline::sekai_reference().step_duration_myr() as f32
    }

    #[test]
    fn gap_spawns_young_ridge_high_ocean_without_changing_event_indices() {
        let (surface, topology) = fixture();
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
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
            ResolvedWorldFormationPreset::Continents,
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
    fn interior_hole_is_rebinned_from_its_own_duplicate_not_spawned() {
        let (surface, topology) = fixture();
        let current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
            &streams(),
        )
        .unwrap();
        let mut next = copy_state(&current);
        let hole = CellId::from_raw(3);
        let neighbor = topology.arcs()[3]
            .iter()
            .map(|arc| arc.neighbor)
            .find(|cell| next.samples[cell.raw() as usize].owner == next.samples[3].owner)
            .expect("a same-plate neighbor exists");
        next.samples[3].position = surface.cell(neighbor).unwrap().centroid;
        next.samples[3].anchor = neighbor;
        let mut coverage = CoverageScratch::with_cell_capacity(surface.cells().len());
        coverage
            .rebuild(surface.cells().len(), &next.samples)
            .unwrap();
        assert_eq!(coverage.count(hole), 0);
        assert_eq!(coverage.count(neighbor), 2);
        let mut events = vec![ContactEvent {
            cell: hole,
            edge: None,
            sample_indices: [None, None],
            lineages: [None, None],
            kind: ContactKind::Gap,
            signed_normal_speed_mm_per_year: 0.0,
            tangent_speed_mm_per_year: 0.0,
            overlap_depth: 0,
        }];
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());

        let stats = rebin_interior_gaps_v5(
            &surface,
            &topology,
            &mut events,
            &current,
            &mut next,
            &coverage,
            &mut actions,
        )
        .unwrap();

        assert_eq!(stats.rebinned_samples, 1);
        assert!(
            events.is_empty(),
            "a rebinned hole is no longer a spreading gap"
        );
        let anchored_at = |cell: CellId| {
            next.samples
                .iter()
                .filter(|sample| sample.anchor == cell)
                .count()
        };
        assert_eq!(anchored_at(hole), 1);
        assert_eq!(anchored_at(neighbor), 1);
        assert_eq!(next.samples.len(), current.samples.len());
        assert!(actions.spawned_samples().is_empty());
    }

    #[test]
    fn continental_divergence_uses_bounded_pure_shear_thinning_and_isostatic_subsidence() {
        let (surface, topology) = fixture();
        let current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
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
            step_duration_myr(),
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

    #[test]
    fn evolved_extension_gains_area_and_preserves_continental_volume() {
        let (surface, topology) = fixture();
        let current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
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
        let before = continental
            .iter()
            .map(|&index| next.samples[index].material)
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
        let mut ledger = EvolutionMaterialLedger::capture_initial(&current).unwrap();
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());

        let stats = apply_divergent_extension_v5(
            &surface,
            &[event],
            &mut next,
            &mut actions,
            &mut ledger,
            step_duration_myr(),
        )
        .unwrap();

        assert_eq!(stats.affected_samples, 2);
        let mut expected_gain = 0.0;
        for (&index, old) in continental.iter().zip(before) {
            let new = next.samples[index].material;
            assert!(new.continental_reference_area_m2() > old.continental_reference_area_m2());
            assert_eq!(
                new.continental_volume_m3().to_bits(),
                old.continental_volume_m3().to_bits()
            );
            assert!(
                new.continental_thickness_km().unwrap() < old.continental_thickness_km().unwrap()
            );
            expected_gain +=
                new.continental_reference_area_m2() - old.continental_reference_area_m2();
        }
        assert_eq!(
            ledger
                .processes()
                .unwrap()
                .rift_extension_continental_area_gain_m2(),
            expected_gain
        );
        ledger.control_budget(&next).unwrap();
    }

    #[test]
    fn evolved_spreading_creates_ledgered_age_zero_oceanic_material() {
        let (surface, topology) = fixture();
        let recipe = FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents);
        let current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
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
        let mut ledger = EvolutionMaterialLedger::capture_initial(&current).unwrap();
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());

        fill_spreading_gaps_v5(
            &surface,
            &[gap],
            &current,
            &mut next,
            &mut actions,
            recipe,
            &mut ledger,
        )
        .unwrap();
        let created = *actions.spawned_samples().last().unwrap();
        commit_process_actions_v5(&mut next, &mut actions, &mut ledger).unwrap();

        assert_eq!(created.age_myr, 0.0);
        assert_eq!(created.kind, CrustKind::Oceanic);
        assert_eq!(
            ledger.processes().unwrap().oceanic_spreading_created(),
            created.material.oceanic_amount().unwrap()
        );
        ledger.control_budget(&next).unwrap();
    }

    #[test]
    fn suppressed_breakup_fills_with_continent_while_complete_phase_spreads_ocean() {
        let (surface, topology) = fixture();
        let current = build_initial_state(
            &surface,
            &topology,
            &TectonicSpec::default(),
            ResolvedWorldFormationPreset::Continents,
            &streams(),
        )
        .unwrap();
        let first = current
            .samples
            .iter()
            .position(|sample| sample.kind == CrustKind::Continental)
            .expect("opening continents include continental crust");
        let first_owner = current.samples[first].owner;
        let second = current
            .samples
            .iter()
            .position(|sample| sample.kind == CrustKind::Continental && sample.owner != first_owner)
            .expect("Continents opening grows crust on more than one plate");
        let second_owner = current.samples[second].owner;
        let gap_cell = CellId::from_raw(3);
        let edge = surface
            .edges()
            .iter()
            .find(|edge| edge.cells.contains(&gap_cell))
            .expect("every cell has a boundary edge");
        let divergence = ContactEvent {
            cell: gap_cell,
            edge: Some(edge.id),
            sample_indices: [Some(first as u32), Some(second as u32)],
            lineages: [Some(first_owner), Some(second_owner)],
            kind: ContactKind::Divergence,
            signed_normal_speed_mm_per_year: 20.0,
            tangent_speed_mm_per_year: 0.0,
            overlap_depth: 0,
        };
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
        let events = [divergence, gap];

        let mut next = copy_state(&current);
        next.initiation.record_rift_pair(first_owner, second_owner);
        next.initiation.mark_dominant(first_owner);
        next.initiation.mark_dominant(second_owner);
        let before = next.material_totals().unwrap();
        let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
        actions.begin_step(next.samples.len());
        fill_spreading_gaps(
            &surface,
            &events,
            &current,
            &mut next,
            &mut actions,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Supercontinent),
        )
        .unwrap();
        let created = *actions.spawned_samples().last().unwrap();
        commit_process_actions(&mut next, &mut actions).unwrap();
        assert_eq!(created.kind, CrustKind::Continental);
        assert_eq!(
            next.material_totals().unwrap().continental().volume_m3(),
            before.continental().volume_m3()
        );

        let mut oceanized = copy_state(&current);
        oceanized
            .initiation
            .record_rift_pair(first_owner, second_owner);
        let mut ocean_actions = ProcessActions::with_sample_capacity(oceanized.samples.len());
        ocean_actions.begin_step(oceanized.samples.len());
        fill_spreading_gaps(
            &surface,
            &events,
            &current,
            &mut oceanized,
            &mut ocean_actions,
            FormationTectonicRecipe::for_preset(ResolvedWorldFormationPreset::Continents),
        )
        .unwrap();
        assert_eq!(
            ocean_actions.spawned_samples().last().unwrap().kind,
            CrustKind::Oceanic
        );
    }
}
