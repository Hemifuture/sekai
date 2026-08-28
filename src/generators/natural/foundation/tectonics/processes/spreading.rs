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
use crate::generators::natural::foundation::tectonics::kinematics::rigid_velocity;
use crate::generators::natural::foundation::tectonics::model::{
    CrustSample, EvolutionMaterialLedger, FormationTectonicRecipe, LineageId, MaterialColumn,
    TectonicState, NEW_OCEANIC_CRUST_THICKNESS_KM,
};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{
    CrustKind, SphericalOrogenyKind, CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
    CONTINENTAL_CRUST_MAX_THICKNESS_KM, CONTINENTAL_CRUST_MIN_THICKNESS_KM,
    NO_OROGENY_AGE_SENTINEL_MYR, OCEANIC_CRUST_MAX_THICKNESS_KM, OCEANIC_CRUST_MIN_THICKNESS_KM,
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

/// What a spreading hole is filled with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::generators::natural::foundation::tectonics) enum RiftFill {
    /// Frozen V4 path: every spreading hole is zero-age ocean unless the
    /// oceanization policy forbids breakup for the diverging pair.
    LegacyImmediateOcean,
    /// V5 (G1e §3.4): a hole between continental margins is filled with the
    /// area those margins gained by pure-shear stretching this step
    /// (McKenzie 1978; `apply_divergent_extension_v5` widened them), as long
    /// as they are still thicker than the supported floor. Ocean appears only
    /// when the margins have no stretched area to give: they sit at the floor,
    /// or divergence outpaces stretching, which is rupture (Brune et al.
    /// 2014). Transient divergence inside a continent therefore never plants
    /// ocean, and a rift must keep diverging to break up (Cortial et al. 2019).
    BreakupAfterThinning,
}

pub(in crate::generators::natural::foundation::tectonics) fn fill_spreading_gaps(
    surface: &SphericalSurfaceSnapshot,
    events: &[ContactEvent],
    current: &TectonicState,
    next: &mut TectonicState,
    actions: &mut ProcessActions,
    recipe: FormationTectonicRecipe,
    rift_fill: RiftFill,
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
        let forbidden = divergence
            .and_then(|event| match event.lineages {
                [Some(first), Some(second)] => Some((first, second)),
                _ => None,
            })
            .is_some_and(|(first, second)| {
                recipe
                    .oceanization
                    .forbids_breakup_ocean(&next.initiation, first, second)
            });
        let ocean = |next: &mut TectonicState| {
            let _ = next;
            oceanic_spreading_sample(cell.centroid, gap.cell, owner, lineation, cell.area.get())
        };
        let donated = if forbidden {
            let mut donors = Vec::new();
            legacy_donors(divergence, owner, &mut donors);
            donate_continental_fill(next, &donors, cell.centroid, cell.area.get())?
        } else if rift_fill == RiftFill::BreakupAfterThinning {
            stretched_margin_donation(surface, next, current_sample_by_cell, gap.cell)?
        } else {
            None
        };
        let sample = match donated {
            Some(material) => {
                continental_rift_fill_sample(cell.centroid, gap.cell, owner, lineation, material)
            }
            None => ocean(next)?,
        };
        spawned.push(sample);
        stats.spawned_samples += 1;
    }
    Ok(stats)
}

/// Material carried by a hole marker: enough to be a valid column, too
/// little to move any mass.
const MARKER_SAMPLE_AREA_M2: f64 = 1.0;

/// Semi-Lagrangian rebinning of sampling holes (G1e §3.4).
///
/// A rigid plate keeps its sample count, so a hole that no neighboring plate
/// is moving away from is a discretization artifact: the sample that left it
/// landed in a cell that already had one. The hole gets a marker sample that
/// carries the departing column's intensive state and one square metre of
/// material, so the mask and the thickness field advect with the plate, the
/// plate's parcels stay untouched, and the resample closes area and volume per
/// plate. A hole a neighboring plate is diverging from is real spreading and
/// is left to the ridge fill. Filled holes are dropped from `events`.
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
    let (_, current_sample_by_cell, _) = actions.spreading_scratch(cell_count);
    index_current_samples(surface, current, current_sample_by_cell)?;
    let mut worklist = VecDeque::new();
    for gap in events.iter().filter(|event| event.kind == ContactKind::Gap) {
        let cell_index = gap.cell.raw() as usize;
        let Some(occupant) = current_sample_by_cell[cell_index] else {
            continue;
        };
        let owner = current.samples[occupant].owner;
        if hole_is_spreading(surface, topology, next, coverage, cell_index, owner)? {
            continue;
        }
        worklist.push_back((cell_index, occupant));
    }

    let mut rebinned = vec![false; cell_count];
    let mut stats = ProcessStats::default();
    for (hole, occupant) in worklist {
        // The hole keeps the state of the column that just left it: thickness,
        // age, elevation and orogeny are intensive and advect with the plate.
        // The marker carries no material of its own (one square metre); the
        // plate's parcels stay where they landed and the resample spreads the
        // group's volume over its cells, so nothing is created or shuffled.
        let mut marker = current.samples[occupant];
        marker.position = surface.cells()[hole].centroid;
        marker.anchor = CellId::from_raw(hole as u32);
        marker.material = MaterialColumn::pure(
            marker.kind,
            MARKER_SAMPLE_AREA_M2,
            marker.thickness_km.clamp(
                match marker.kind {
                    CrustKind::Continental => CONTINENTAL_CRUST_MIN_THICKNESS_KM,
                    CrustKind::Oceanic => OCEANIC_CRUST_MIN_THICKNESS_KM,
                },
                match marker.kind {
                    CrustKind::Continental => CONTINENTAL_CRUST_MAX_THICKNESS_KM,
                    CrustKind::Oceanic => OCEANIC_CRUST_MAX_THICKNESS_KM,
                },
            ),
        )?;
        actions.push_spawned(marker);
        rebinned[hole] = true;
        stats.rebinned_samples += 1;
    }
    events.retain(|event| event.kind != ContactKind::Gap || !rebinned[event.cell.raw() as usize]);
    Ok(stats)
}

/// A hole is spreading when a plate other than its previous occupant covers a
/// neighboring cell and moves away from the hole relative to the occupant.
fn hole_is_spreading(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    next: &TectonicState,
    coverage: &CoverageScratch,
    cell_index: usize,
    occupant: LineageId,
) -> Result<bool, ProcessError> {
    let Some(occupant_plate) = next.plate(occupant) else {
        return Ok(false);
    };
    let radius = surface.radius();
    let center = surface.cells()[cell_index].centroid;
    let occupant_velocity = rigid_velocity(occupant_plate.rotation, radius, center)?;
    for arc in &topology.arcs()[cell_index] {
        let neighbor = arc.neighbor;
        let outward = {
            let target = surface.cells()[neighbor.raw() as usize]
                .centroid
                .components();
            let origin = center.components();
            [
                target[0] - origin[0],
                target[1] - origin[1],
                target[2] - origin[2],
            ]
        };
        for &raw in coverage.sample_indices(neighbor) {
            let other = next.samples[raw as usize].owner;
            if other == occupant {
                continue;
            }
            let Some(other_plate) = next.plate(other) else {
                continue;
            };
            let other_velocity = rigid_velocity(other_plate.rotation, radius, center)?;
            let relative = [
                other_velocity[0] - occupant_velocity[0],
                other_velocity[1] - occupant_velocity[1],
                other_velocity[2] - occupant_velocity[2],
            ];
            if super::dot(relative, outward) > 0.0 {
                return Ok(true);
            }
        }
    }
    Ok(false)
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
    let stats = fill_spreading_gaps(
        surface,
        events,
        current,
        next,
        actions,
        recipe,
        RiftFill::BreakupAfterThinning,
    )?;
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
        thickness_km: NEW_OCEANIC_CRUST_THICKNESS_KM,
        age_myr: 0.0,
        tectonic_elevation_m: constants::OCEANIC_RIDGE_ELEVATION_M,
        lineation,
        orogeny: SphericalOrogenyKind::None,
        orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
        material: MaterialColumn::pure(
            CrustKind::Oceanic,
            area_m2,
            NEW_OCEANIC_CRUST_THICKNESS_KM,
        )?,
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

/// V4 donors: the diverging pair when known, otherwise the hole's owner.
fn legacy_donors(divergence: Option<&ContactEvent>, owner: LineageId, donors: &mut Vec<LineageId>) {
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
}

/// Area the neighboring continental margins gained by stretching beyond
/// their own cells, taken from them for the hole while they stay thicker
/// than [`CONTINENTAL_CRUST_MIN_THICKNESS_KM`]. `None` when nothing stretched
/// is available: the divergence has outrun ductile stretching and the rift
/// ruptures into ocean.
fn stretched_margin_donation(
    surface: &SphericalSurfaceSnapshot,
    next: &mut TectonicState,
    current_sample_by_cell: &[Option<usize>],
    cell: CellId,
) -> Result<Option<MaterialColumn>, ProcessError> {
    let Some(edges) = surface.cell_edges(cell) else {
        return Ok(None);
    };
    let need = surface.cells()[cell.raw() as usize].area.get();
    let floor = f64::from(CONTINENTAL_CRUST_MIN_THICKNESS_KM) * (1.0 + 1.0e-3);
    let mut remaining = need;
    let mut donated: Option<MaterialColumn> = None;
    for &edge in edges {
        if remaining <= 0.0 {
            break;
        }
        let Some(neighbor) = surface.opposite_cell(cell, edge) else {
            continue;
        };
        let Some(index) = current_sample_by_cell[neighbor.raw() as usize] else {
            continue;
        };
        let material = next.samples[index].material;
        let thick = material
            .continental_thickness_km()
            .is_some_and(|thickness| f64::from(thickness) > floor);
        let anchor_area = surface.cells()[next.samples[index].anchor.raw() as usize]
            .area
            .get();
        let excess = material.continental_reference_area_m2() - anchor_area;
        if !thick || excess <= 0.0 {
            continue;
        }
        let (left, taken) = material.extract_continental_area(remaining.min(excess))?;
        let Some(taken) = taken else {
            continue;
        };
        next.samples[index].material = left;
        next.samples[index].synchronize_compatibility_from_material();
        remaining -= taken.continental_reference_area_m2();
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

fn donate_continental_fill(
    next: &mut TectonicState,
    donors: &[LineageId],
    target: crate::world::spatial::UnitVector3,
    requested_area_m2: f64,
) -> Result<Option<MaterialColumn>, ProcessError> {
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
        fill_spreading_gaps_v5, rebin_interior_gaps_v5, RiftFill,
    };
    use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
    use crate::generators::natural::foundation::tectonics::contacts::{
        ContactEvent, ContactKind, CoverageScratch,
    };
    use crate::generators::natural::foundation::tectonics::initial_state::build_initial_state;
    use crate::generators::natural::foundation::tectonics::model::{
        EvolutionMaterialLedger, FormationTectonicRecipe, MaterialColumn, TectonicState,
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
            RiftFill::LegacyImmediateOcean,
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
            RiftFill::LegacyImmediateOcean,
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

        fill_spreading_gaps(
            &surface,
            &[gap],
            &current,
            &mut next,
            &mut actions,
            recipe,
            RiftFill::LegacyImmediateOcean,
        )
        .unwrap();
        commit_process_actions(&mut next, &mut actions).unwrap();

        assert_eq!(next.samples.last().unwrap().owner, expected);
    }

    #[test]
    fn v5_rift_fills_from_stretched_margins_and_ruptures_without_them() {
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
        let owner = current.samples[3].owner;
        let mut ring = Vec::new();
        for &edge in surface.cell_edges(gap_cell).unwrap() {
            ring.push(surface.opposite_cell(gap_cell, edge).unwrap().raw() as usize);
        }
        let set_ring = |current: &mut TectonicState, stretch: f64| {
            for &index in &ring {
                let area = surface.cells()[index].area.get() * stretch;
                current.samples[index].owner = owner;
                current.samples[index].kind = CrustKind::Continental;
                current.samples[index].thickness_km = 38.0;
                current.samples[index].material =
                    MaterialColumn::pure(CrustKind::Continental, area, 38.0).unwrap();
            }
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
        let run = |current: &TectonicState| {
            let mut next = copy_state(current);
            let mut actions = ProcessActions::with_sample_capacity(next.samples.len());
            actions.begin_step(next.samples.len());
            fill_spreading_gaps(
                &surface,
                std::slice::from_ref(&gap),
                current,
                &mut next,
                &mut actions,
                recipe,
                RiftFill::BreakupAfterThinning,
            )
            .unwrap();
            actions.spawned_samples()[0].kind
        };
        // Margins stretched beyond their cells donate that excess: continental.
        set_ring(&mut current, 1.5);
        assert_eq!(run(&current), CrustKind::Continental);
        // Margins with nothing stretched to give: the rift ruptures into ocean.
        set_ring(&mut current, 1.0);
        assert_eq!(run(&current), CrustKind::Oceanic);
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
        // One rigid rotation for every plate: no boundary diverges, so the
        // hole can only be a sampling artifact.
        let shared = next.plates[0].rotation;
        for plate in &mut next.plates {
            plate.rotation = shared;
        }
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
        let marker = &actions.spawned_samples()[0];
        assert_eq!(marker.anchor, hole);
        assert_eq!(marker.kind, current.samples[3].kind);
        assert_eq!(marker.thickness_km, current.samples[3].thickness_km);
        assert!(marker.material.continental_reference_area_m2() <= 1.0);
        assert_eq!(
            next.samples[3].anchor, neighbor,
            "the double is not snapped back"
        );
        assert_eq!(next.samples[3].material, current.samples[3].material);
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
            RiftFill::LegacyImmediateOcean,
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
            RiftFill::LegacyImmediateOcean,
        )
        .unwrap();
        assert_eq!(
            ocean_actions.spawned_samples().last().unwrap().kind,
            CrustKind::Oceanic
        );
    }
}
