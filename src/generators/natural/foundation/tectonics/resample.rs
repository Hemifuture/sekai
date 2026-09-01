//! Deterministic moving-crust resampling and final plate canonicalization.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};

use thiserror::Error;

use super::contacts::ContactError;
use super::model::{
    CrustSample, EvolutionMaterialLedger, LineageId, MaterialColumn, MaterialColumnError,
    TectonicState, NEW_OCEANIC_CRUST_THICKNESS_KM,
};
use super::passive_margin::relax_passive_margins;
use super::workspace::TectonicWorkspace;
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{
    CrustKind, PlateIdField, SphericalOrogenyKind, SphericalPlate,
    CONTINENTAL_CRUST_AGE_SENTINEL_MYR, CONTINENTAL_CRUST_MAX_THICKNESS_KM,
    CONTINENTAL_CRUST_MIN_THICKNESS_KM, MAX_PLATE_COUNT, OCEANIC_CRUST_MAX_THICKNESS_KM,
    OCEANIC_CRUST_MIN_THICKNESS_KM,
};
use crate::world::spatial::{
    canonical_east_north_basis, spherical_triangle_area_unit, SphericalSurfaceSnapshot, UnitVector3,
};
use crate::world::{CellId, PlateId};

const DEFAULT_DELTA_YEARS: f64 = 2_000_000.0;
const TARGET_ANGULAR_DISPLACEMENT_RAD: f64 = 0.25;
const MINIMUM_RESAMPLE_INTERVAL: u16 = 10;
const MAXIMUM_RESAMPLE_INTERVAL: u16 = 60;
const MAXIMUM_TRIANGLE_CANDIDATES: usize = 12;
const TRIANGLE_AREA_EPSILON: f64 = 1.0e-14;
const DOMAIN_EVIDENCE_PENALTY_SHORT_SIDE_FRACTION: f64 = 1.0;
// Volume-preserving MBO threshold dynamics: three bounded Jacobi heat steps remove
// grid-scale categorical aliasing before the occupied-area threshold is applied.
// The radius stays local (a few authoritative cells) and no material history is kept.
const MATERIAL_HEAT_STEPS: usize = 3;
const MATERIAL_HEAT_NEIGHBOR_SHARE: f64 = 0.5;

#[derive(Debug)]
pub(super) struct CanonicalTectonicState {
    pub(super) samples: Vec<CrustSample>,
    pub(super) plates: Vec<SphericalPlate>,
    pub(super) cell_plates: PlateIdField,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum ResampleError {
    #[error("surface has {surface_cells} cells but topology has {topology_cells}")]
    CardinalityMismatch {
        surface_cells: usize,
        topology_cells: usize,
    },
    #[error("coverage construction failed: {0}")]
    Coverage(#[from] ContactError),
    #[error("authoritative cell {cell:?} remains uncovered after spreading")]
    UnresolvedCoverageGap { cell: CellId },
    #[error("coverage references sample {sample}, but only {samples} samples exist")]
    InvalidCoverageSample { sample: usize, samples: usize },
    #[error("final state has {samples} samples for {surface_cells} authoritative cells")]
    StateCardinalityMismatch {
        samples: usize,
        surface_cells: usize,
    },
    #[error("final sample {sample} references invalid anchor {anchor:?}")]
    InvalidFinalAnchor { sample: usize, anchor: CellId },
    #[error("more than one final sample is bound to {cell:?}")]
    DuplicateFinalAnchor { cell: CellId },
    #[error("final state has no sample bound to {cell:?}")]
    MissingFinalAnchor { cell: CellId },
    #[error("final component references missing lineage {lineage:?}")]
    UnknownLineage { lineage: LineageId },
    #[error(
        "rebalance could not place {volume_m3} m3 of {kind:?} volume anywhere within thickness bounds"
    )]
    UnplacedRebalanceResidual { kind: CrustKind, volume_m3: f64 },
    #[error("final active plate count {found} is outside {min}..={max}")]
    FinalPlateCountOutOfRange {
        found: usize,
        min: usize,
        max: usize,
    },
    #[error("moving crust has no live lineage with material samples")]
    NoLiveLineages,
    #[error(
        "cannot place unique markers for {lineages} live lineages on {cells} authoritative cells"
    )]
    DomainMarkerCapacityExceeded { lineages: usize, cells: usize },
    #[error("evidence-guided domain reconstruction did not reach {cell:?}")]
    UnassignedDomainCell { cell: CellId },
    #[error("current moving crust has no {kind:?} material source for conservative remapping")]
    MissingMaterialSource { kind: CrustKind },
    #[error("material operation failed: {0}")]
    Material(#[from] MaterialColumnError),
}

pub(super) fn resampling_interval_steps(state: &TectonicState) -> u16 {
    let maximum_step_angle = state
        .plates
        .iter()
        .map(|plate| plate.rotation.angular_rate_rad_per_year() * DEFAULT_DELTA_YEARS)
        .fold(0.0_f64, f64::max);
    if maximum_step_angle <= f64::EPSILON {
        return MAXIMUM_RESAMPLE_INTERVAL;
    }
    let unconstrained = (TARGET_ANGULAR_DISPLACEMENT_RAD / maximum_step_angle).floor();
    (unconstrained as u16).clamp(MINIMUM_RESAMPLE_INTERVAL, MAXIMUM_RESAMPLE_INTERVAL)
}

pub(super) fn resample_current_state(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    workspace: &mut TectonicWorkspace,
) -> Result<(), ResampleError> {
    let cell_count = surface.cells().len();
    if topology.cell_count() != cell_count {
        return Err(ResampleError::CardinalityMismatch {
            surface_cells: cell_count,
            topology_cells: topology.cell_count(),
        });
    }

    workspace
        .coverage
        .rebuild(cell_count, &workspace.current.samples)?;
    workspace
        .next
        .copy_plate_table_into_reusable_next(&workspace.current);
    workspace.next.samples.clear();
    workspace.next.samples.reserve(cell_count);
    let mut local_candidates = Vec::with_capacity(32);

    for cell in surface.cells() {
        let sample = resample_cell(
            surface,
            topology,
            &workspace.current.samples,
            &workspace.coverage,
            cell.id,
            &mut local_candidates,
        )?;
        workspace.next.samples.push(sample);
    }

    reconstruct_connected_plate_domains(
        surface,
        topology,
        &workspace.current.samples,
        &workspace.coverage,
        &mut workspace.next,
        &mut local_candidates,
    )?;
    conservative_material_remap(
        surface,
        topology,
        &workspace.current.samples,
        &mut workspace.next.samples,
    )?;

    std::mem::swap(&mut workspace.current, &mut workspace.next);
    workspace.next.samples.clear();
    workspace.next.plates.clear();
    workspace.events.clear();
    workspace.mark_resampled();
    Ok(())
}

/// V5 geometry follows the retained moving-sample reconstruction, while its
/// material solve is extensive and ledgered instead of occupied-anchor MBO.
pub(super) fn resample_current_state_v5(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    workspace: &mut TectonicWorkspace,
    ledger: &mut EvolutionMaterialLedger,
) -> Result<(), ResampleError> {
    let cell_count = surface.cells().len();
    if topology.cell_count() != cell_count {
        return Err(ResampleError::CardinalityMismatch {
            surface_cells: cell_count,
            topology_cells: topology.cell_count(),
        });
    }

    workspace
        .coverage
        .rebuild(cell_count, &workspace.current.samples)?;
    workspace
        .next
        .copy_plate_table_into_reusable_next(&workspace.current);
    workspace.next.samples.clear();
    workspace.next.samples.reserve(cell_count);
    let mut local_candidates = Vec::with_capacity(32);
    for cell in surface.cells() {
        let sample = resample_cell(
            surface,
            topology,
            &workspace.current.samples,
            &workspace.coverage,
            cell.id,
            &mut local_candidates,
        )?;
        workspace.next.samples.push(sample);
    }
    reconstruct_connected_plate_domains_v5(
        surface,
        topology,
        &workspace.current.samples,
        &workspace.coverage,
        &mut workspace.next,
        &mut local_candidates,
    )?;
    conservative_material_resample_v5(
        surface,
        topology,
        &workspace.current,
        &workspace.coverage,
        &mut workspace.next.samples,
        ledger,
    )?;

    std::mem::swap(&mut workspace.current, &mut workspace.next);
    workspace.next.samples.clear();
    workspace.next.plates.clear();
    workspace.events.clear();
    workspace.mark_resampled();
    Ok(())
}

fn conservative_material_resample_v5(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    source: &TectonicState,
    coverage: &super::contacts::CoverageScratch,
    remapped: &mut [CrustSample],
    ledger: &mut EvolutionMaterialLedger,
) -> Result<(), ResampleError> {
    let cell_count = surface.cells().len();
    if remapped.len() != cell_count {
        return Err(ResampleError::StateCardinalityMismatch {
            samples: remapped.len(),
            surface_cells: cell_count,
        });
    }
    // The mask is each cell's semi-Lagrangian winner kind: the rigidly
    // advected mask, with no area threshold, diffusion, or interpolation that
    // could bridge a seaway or open a hole nothing moved into (G1e §3.4). A
    // cell nobody covers keeps the kind the per-cell resample resolved.
    let mut kinds = Vec::with_capacity(cell_count);
    let mut winner_thickness = Vec::with_capacity(cell_count);
    for (cell, fallback) in surface.cells().iter().zip(remapped.iter()) {
        let winner = if coverage.sample_indices(cell.id).is_empty() {
            *fallback
        } else {
            source.samples[coverage_winner(&source.samples, coverage, cell.id, cell.centroid)?]
        };
        kinds.push(winner.kind);
        winner_thickness.push(match winner.kind {
            CrustKind::Continental => winner.material.continental_thickness_km(),
            CrustKind::Oceanic => winner.material.oceanic_thickness_km(),
        });
    }
    // One control cell is the resolution floor: a continental cell with no
    // continental neighbor, or an oceanic cell with no oceanic neighbor, is
    // sub-cell coastline jitter frozen by the remap, not a resolved island or
    // basin. Its kind follows its neighborhood; its parcel travels to the
    // nearest cell of its own kind below and the group rebalance closes the
    // cell's area.
    let mut flipped = Vec::new();
    for cell in 0..cell_count {
        let kind = kinds[cell];
        let isolated = topology.arcs()[cell]
            .iter()
            .all(|arc| kinds[arc.neighbor.raw() as usize] != kind);
        let others_remain = kinds
            .iter()
            .enumerate()
            .any(|(other, &other_kind)| other != cell && other_kind == kind);
        if isolated && others_remain {
            kinds[cell] = match kind {
                CrustKind::Continental => CrustKind::Oceanic,
                CrustKind::Oceanic => CrustKind::Continental,
            };
            flipped.push(cell);
        }
    }
    let nearest_continental = nearest_cell_of_kind(topology, &kinds, CrustKind::Continental);
    let nearest_oceanic = nearest_cell_of_kind(topology, &kinds, CrustKind::Oceanic);

    // Donor-cell deposition: every source column is a parcel landing whole in
    // its anchor cell when that cell is of its kind, otherwise in the nearest
    // cell of its kind. Reference area rides with the parcel, so a cell may
    // hold more or less than its own area; nothing is rescaled or mixed.
    let mut continental = vec![(0.0_f64, 0.0_f64); cell_count];
    let mut oceanic = vec![(0.0_f64, 0.0_f64); cell_count];
    let mut moved_area = 0.0;
    for (sample_index, sample) in source.samples.iter().enumerate() {
        let anchor = sample.anchor.raw() as usize;
        if anchor >= cell_count {
            return Err(ResampleError::InvalidFinalAnchor {
                sample: sample_index,
                anchor: sample.anchor,
            });
        }
        let material = sample.material;
        let continental_area = material.continental_reference_area_m2();
        if continental_area > 0.0 {
            let target = if kinds[anchor] == CrustKind::Continental {
                anchor
            } else {
                moved_area += continental_area;
                nearest_continental[anchor].ok_or(ResampleError::MissingMaterialSource {
                    kind: CrustKind::Continental,
                })?
            };
            continental[target].0 += continental_area;
            continental[target].1 += material.continental_volume_m3();
        }
        let oceanic_area = material.oceanic_reference_area_m2();
        if oceanic_area > 0.0 {
            let target = if kinds[anchor] == CrustKind::Oceanic {
                anchor
            } else {
                nearest_oceanic[anchor].ok_or(ResampleError::MissingMaterialSource {
                    kind: CrustKind::Oceanic,
                })?
            };
            oceanic[target].0 += oceanic_area;
            oceanic[target].1 += material.oceanic_volume_m3();
        }
    }

    // A flipped cell has no parcel of its new kind: it borrows its cell's
    // worth from its neighbors' parcels of that kind, in proportion to what
    // they hold and at their thickness, so nothing is created.
    for &cell in &flipped {
        let kind = kinds[cell];
        let neighbors = topology.arcs()[cell]
            .iter()
            .map(|arc| arc.neighbor.raw() as usize)
            .collect::<Vec<_>>();
        let pool = |store: &[(f64, f64)]| neighbors.iter().map(|&n| store[n].0).sum::<f64>();
        let need = surface.cells()[cell].area.get();
        let store = match kind {
            CrustKind::Continental => &mut continental,
            CrustKind::Oceanic => &mut oceanic,
        };
        let available = pool(store);
        if available <= 0.0 {
            continue;
        }
        let fraction = (need / available).min(0.5);
        for &n in &neighbors {
            let (area, volume) = store[n];
            let taken_area = area * fraction;
            let taken_volume = volume * fraction;
            store[n] = (area - taken_area, volume - taken_volume);
            store[cell].0 += taken_area;
            store[cell].1 += taken_volume;
        }
    }

    for index in 0..cell_count {
        if continental[index].0 > 0.0 || oceanic[index].0 > 0.0 {
            remapped[index].material = MaterialColumn::new(
                continental[index].0,
                continental[index].1,
                oceanic[index].0,
                oceanic[index].1,
            )?;
        }
        let previous_kind = remapped[index].kind;
        remapped[index].synchronize_compatibility_from_material();
        if remapped[index].kind == CrustKind::Continental {
            remapped[index].age_myr = CONTINENTAL_CRUST_AGE_SENTINEL_MYR;
        } else if previous_kind == CrustKind::Continental {
            remapped[index].age_myr = 0.0;
        }
    }
    ledger.record_resample_overlap_moved_area(moved_area);
    rebalance_columns_to_cells(
        surface,
        topology,
        remapped,
        &mut kinds,
        &winner_thickness,
        ledger,
    )?;
    Ok(())
}

/// Brings every resampled column back to its cell's area without touching the
/// mask (G1e §3.4). Within each plate-and-kind group the parcels' volume is
/// spread over the group's cells at each cell's own thickness, scaled by one
/// common factor so the group's volume is conserved: this is pure shear over
/// the group, thickening where parcels stacked (convergence) and thinning
/// where they spread (extension), recorded as collision shortening or rift
/// extension of continental area. Oceanic groups change area at their mean
/// thickness and record it as coverage created or consumed. Thickness bounds
/// are honored by fixing clamped cells and rescaling the rest.
fn rebalance_columns_to_cells(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    remapped: &mut [CrustSample],
    kinds: &mut [CrustKind],
    winner_thickness: &[Option<f32>],
    ledger: &mut EvolutionMaterialLedger,
) -> Result<(), ResampleError> {
    let mut groups: BTreeMap<(u32, bool), Vec<usize>> = BTreeMap::new();
    for (index, sample) in remapped.iter().enumerate() {
        groups
            .entry((sample.owner.raw(), kinds[index] == CrustKind::Continental))
            .or_default()
            .push(index);
    }
    for ((_, continental), mut cells) in groups {
        let kind = if continental {
            CrustKind::Continental
        } else {
            CrustKind::Oceanic
        };
        let (minimum_m, maximum_m) = match kind {
            CrustKind::Continental => (
                f64::from(CONTINENTAL_CRUST_MIN_THICKNESS_KM) * 1_000.0,
                f64::from(CONTINENTAL_CRUST_MAX_THICKNESS_KM) * 1_000.0,
            ),
            CrustKind::Oceanic => (
                f64::from(OCEANIC_CRUST_MIN_THICKNESS_KM) * 1_000.0,
                f64::from(OCEANIC_CRUST_MAX_THICKNESS_KM) * 1_000.0,
            ),
        };
        let mut total_area = 0.0;
        let mut total_volume = 0.0;
        let mut total_cells = 0.0;
        let mut thickness = Vec::with_capacity(cells.len());
        let mut parcel_area = Vec::with_capacity(cells.len());
        for &index in &cells {
            let material = remapped[index].material;
            let (area, volume) = match kind {
                CrustKind::Continental => (
                    material.continental_reference_area_m2(),
                    material.continental_volume_m3(),
                ),
                CrustKind::Oceanic => (
                    material.oceanic_reference_area_m2(),
                    material.oceanic_volume_m3(),
                ),
            };
            total_area += area;
            total_volume += volume;
            total_cells += surface.cells()[index].area.get();
            // Thickness is intensive and advects with the nearest sample; the
            // parcels that happen to stack in a cell contribute their volume
            // to the group, not a doubled column here.
            thickness.push(match winner_thickness[index] {
                Some(km) if km > 0.0 => f64::from(km) * 1_000.0,
                _ if area > 0.0 => volume / area,
                _ => 0.0,
            });
            parcel_area.push(area);
        }
        if total_area <= 0.0 || total_volume <= 0.0 {
            continue;
        }
        for value in &mut thickness {
            if *value <= 0.0 {
                *value = total_volume / total_cells;
            }
        }
        if kind == CrustKind::Continental {
            // Continental crust cannot be thinned below the supported floor
            // to cover more cells than its area affords: the thinnest cells
            // rupture into new ocean (McKenzie 1978 thinning ends in
            // breakup) and their material concentrates in the rest.
            loop {
                let weight: f64 = cells
                    .iter()
                    .zip(&thickness)
                    .map(|(&index, &t)| surface.cells()[index].area.get() * t)
                    .sum();
                if weight <= 0.0 {
                    break;
                }
                let scale = total_volume / weight;
                // Thinning is also bounded by the rift-extension budget (Wise
                // 1974 constant-freeboard argument behind the V5 ledger): a
                // deficit the budget cannot cover ruptures the cell with the
                // least material instead of thinning the whole group.
                let over_budget =
                    total_cells - total_area > ledger.remaining_rift_extension_area_m2();
                // Rupture happens at a margin: prefer cells that already touch
                // ocean so an interior cell is never turned into a lake.
                let at_margin = |position: usize| {
                    topology.arcs()[cells[position]]
                        .iter()
                        .any(|arc| kinds[arc.neighbor.raw() as usize] == CrustKind::Oceanic)
                };
                let pick = |candidates: Vec<usize>| {
                    candidates
                        .iter()
                        .copied()
                        .filter(|&position| at_margin(position))
                        .min_by(|&a, &b| parcel_area[a].total_cmp(&parcel_area[b]))
                        .or_else(|| {
                            candidates
                                .into_iter()
                                .min_by(|&a, &b| parcel_area[a].total_cmp(&parcel_area[b]))
                        })
                };
                let below_floor = pick(
                    (0..cells.len())
                        .filter(|&position| thickness[position] * scale < minimum_m)
                        .collect(),
                );
                let least_material = pick((0..cells.len()).collect());
                let Some(position) =
                    below_floor.or(if over_budget { least_material } else { None })
                else {
                    break;
                };
                if cells.len() == 1 {
                    // Too little material for even one cell: the fragment is
                    // absorbed by the nearest continental cell of another plate
                    // (its area retires by thickening there) and this cell
                    // ruptures into ocean.
                    let index = cells[0];
                    let mut remaining = total_volume;
                    for receiver in
                        cells_of_kind_by_distance(topology, kinds, CrustKind::Continental, index)
                    {
                        let host = remapped[receiver].material;
                        let capacity = host.continental_reference_area_m2() * maximum_m
                            - host.continental_volume_m3();
                        if capacity <= 0.0 {
                            continue;
                        }
                        let taken = remaining.min(capacity);
                        remapped[receiver].material = MaterialColumn::new(
                            host.continental_reference_area_m2(),
                            host.continental_volume_m3() + taken,
                            host.oceanic_reference_area_m2(),
                            host.oceanic_volume_m3(),
                        )?;
                        remapped[receiver].synchronize_compatibility_from_material();
                        remaining -= taken;
                        if remaining <= 0.0 {
                            break;
                        }
                    }
                    if remaining > 0.0 {
                        break;
                    }
                    ledger.record_collision_shortening_area_loss(total_area);
                    let area = surface.cells()[index].area.get();
                    let ocean = MaterialColumn::pure(
                        CrustKind::Oceanic,
                        area,
                        NEW_OCEANIC_CRUST_THICKNESS_KM,
                    )?;
                    ledger.record_oceanic_spreading(ocean.oceanic_amount()?);
                    remapped[index].material = ocean;
                    remapped[index].synchronize_compatibility_from_material();
                    remapped[index].age_myr = 0.0;
                    kinds[index] = CrustKind::Oceanic;
                    cells.clear();
                    break;
                }
                let index = cells.swap_remove(position);
                thickness.swap_remove(position);
                parcel_area.swap_remove(position);
                let area = surface.cells()[index].area.get();
                total_cells -= area;
                let ocean =
                    MaterialColumn::pure(CrustKind::Oceanic, area, NEW_OCEANIC_CRUST_THICKNESS_KM)?;
                ledger.record_oceanic_spreading(ocean.oceanic_amount()?);
                remapped[index].material = ocean;
                remapped[index].synchronize_compatibility_from_material();
                remapped[index].age_myr = 0.0;
                kinds[index] = CrustKind::Oceanic;
            }
        }
        if cells.is_empty() {
            continue;
        }
        let delta_area = total_cells - total_area;
        match kind {
            CrustKind::Continental => {
                if delta_area > 0.0 {
                    ledger.record_rift_extension_area_gain(delta_area);
                } else if delta_area < 0.0 {
                    ledger.record_collision_shortening_area_loss(-delta_area);
                }
            }
            CrustKind::Oceanic => {
                let delta_volume = delta_area * (total_volume / total_area);
                ledger.record_coverage_change(delta_area, delta_volume)?;
                total_volume += delta_volume;
            }
        }
        let mut fixed = vec![false; cells.len()];
        let mut fixed_volume = 0.0;
        loop {
            let free_weight: f64 = cells
                .iter()
                .zip(&thickness)
                .zip(&fixed)
                .filter(|(_, &is_fixed)| !is_fixed)
                .map(|((&index, &t), _)| surface.cells()[index].area.get() * t)
                .sum();
            if free_weight <= 0.0 {
                break;
            }
            let scale = (total_volume - fixed_volume) / free_weight;
            let mut clamped = false;
            for ((&index, t), is_fixed) in cells.iter().zip(&mut thickness).zip(&mut fixed) {
                if *is_fixed {
                    continue;
                }
                let scaled = *t * scale;
                if scaled < minimum_m || scaled > maximum_m {
                    *t = scaled.clamp(minimum_m, maximum_m);
                    *is_fixed = true;
                    fixed_volume += surface.cells()[index].area.get() * *t;
                    clamped = true;
                }
            }
            if !clamped {
                for (t, is_fixed) in thickness.iter_mut().zip(&fixed) {
                    if !*is_fixed {
                        *t *= scale;
                    }
                }
                break;
            }
        }
        // Volume the bounds could not place inside the group moves to the
        // nearest cells of the same kind outside it that still have room, so
        // the group's volume closes exactly; a leftover no cell can hold is a
        // hard error, never a silent loss (G1e R3: oceanic groups whose cells
        // all clamp used to drop their residual here).
        let placed: f64 = cells
            .iter()
            .zip(&thickness)
            .map(|(&index, &t)| surface.cells()[index].area.get() * t)
            .sum();
        let mut residual = total_volume - placed;
        if residual.abs() > 1.0e-9 * total_volume {
            for receiver in cells_of_kind_by_distance(topology, kinds, kind, cells[0]) {
                if cells.contains(&receiver) {
                    continue;
                }
                let host = remapped[receiver].material;
                let (host_area, host_volume) = match kind {
                    CrustKind::Continental => (
                        host.continental_reference_area_m2(),
                        host.continental_volume_m3(),
                    ),
                    CrustKind::Oceanic => {
                        (host.oceanic_reference_area_m2(), host.oceanic_volume_m3())
                    }
                };
                let room = if residual > 0.0 {
                    host_area * maximum_m - host_volume
                } else {
                    host_volume - host_area * minimum_m
                };
                if room <= 0.0 {
                    continue;
                }
                let moved = residual.abs().min(room) * residual.signum();
                remapped[receiver].material = match kind {
                    CrustKind::Continental => MaterialColumn::new(
                        host_area,
                        host_volume + moved,
                        host.oceanic_reference_area_m2(),
                        host.oceanic_volume_m3(),
                    )?,
                    CrustKind::Oceanic => MaterialColumn::new(
                        host.continental_reference_area_m2(),
                        host.continental_volume_m3(),
                        host_area,
                        host_volume + moved,
                    )?,
                };
                remapped[receiver].synchronize_compatibility_from_material();
                residual -= moved;
                if residual.abs() <= 1.0e-9 * total_volume {
                    break;
                }
            }
            if residual.abs() > 1.0e-9 * total_volume {
                return Err(ResampleError::UnplacedRebalanceResidual {
                    kind,
                    volume_m3: residual,
                });
            }
        }
        for (&index, &t) in cells.iter().zip(&thickness) {
            let area = surface.cells()[index].area.get();
            remapped[index].material = match kind {
                CrustKind::Continental => MaterialColumn::new(area, area * t, 0.0, 0.0)?,
                CrustKind::Oceanic => MaterialColumn::new(0.0, 0.0, area, area * t)?,
            };
            remapped[index].synchronize_compatibility_from_material();
        }
    }
    Ok(())
}

/// Cells of `kind` other than `start` in breadth-first order from it.
fn cells_of_kind_by_distance(
    topology: &NaturalTopologyIndex,
    kinds: &[CrustKind],
    kind: CrustKind,
    start: usize,
) -> Vec<usize> {
    let mut visited = vec![false; kinds.len()];
    let mut queue = VecDeque::from([start]);
    let mut order = Vec::new();
    visited[start] = true;
    while let Some(cell) = queue.pop_front() {
        if cell != start && kinds[cell] == kind {
            order.push(cell);
        }
        for arc in &topology.arcs()[cell] {
            let neighbor = arc.neighbor.raw() as usize;
            if !visited[neighbor] {
                visited[neighbor] = true;
                queue.push_back(neighbor);
            }
        }
    }
    order
}

/// Nearest cell of `kind` for every cell, by breadth-first hops; `None` when
/// no cell of that kind exists.
fn nearest_cell_of_kind(
    topology: &NaturalTopologyIndex,
    kinds: &[CrustKind],
    kind: CrustKind,
) -> Vec<Option<usize>> {
    let mut nearest = vec![None; kinds.len()];
    let mut queue = VecDeque::new();
    for (index, &cell_kind) in kinds.iter().enumerate() {
        if cell_kind == kind {
            nearest[index] = Some(index);
            queue.push_back(index);
        }
    }
    while let Some(cell) = queue.pop_front() {
        let source = nearest[cell];
        for arc in &topology.arcs()[cell] {
            let neighbor = arc.neighbor.raw() as usize;
            if nearest[neighbor].is_none() {
                nearest[neighbor] = source;
                queue.push_back(neighbor);
            }
        }
    }
    nearest
}

/// The covering sample closest to the cell centroid; ties take the lower index.
fn coverage_winner(
    samples: &[CrustSample],
    coverage: &super::contacts::CoverageScratch,
    cell: CellId,
    target: UnitVector3,
) -> Result<usize, ResampleError> {
    let exact = coverage.sample_indices(cell);
    if exact.is_empty() {
        return Err(ResampleError::UnresolvedCoverageGap { cell });
    }
    let mut winner_index = exact[0] as usize;
    let mut winner_score = sample_score(samples, winner_index, target)?;
    for &raw_index in &exact[1..] {
        let index = raw_index as usize;
        let score = sample_score(samples, index, target)?;
        if score > winner_score || (score == winner_score && index < winner_index) {
            winner_index = index;
            winner_score = score;
        }
    }
    Ok(winner_index)
}

/// Applies a conservative categorical correction after semi-Lagrangian
/// resampling.
///
/// Nearest-sample resampling supplies the data term. A bounded graph-heat step
/// followed by an occupied-area threshold is the volume-preserving MBO method:
/// it removes cell-scale categorical aliasing while preserving continental
/// area to within one target cell. This avoids both pinning crust to its
/// original Voronoi cells and losing a minority material through repeated
/// semi-Lagrangian resampling.
fn conservative_material_remap(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    source: &[CrustSample],
    remapped: &mut [CrustSample],
) -> Result<(), ResampleError> {
    let cell_count = surface.cells().len();
    if remapped.len() != cell_count {
        return Err(ResampleError::StateCardinalityMismatch {
            samples: remapped.len(),
            surface_cells: cell_count,
        });
    }
    let target_continental_area = occupied_material_area(surface, source, CrustKind::Continental)?;
    let total_area = surface.total_cell_area().get();
    let mut selected = vec![false; cell_count];

    let (continental_nearest, oceanic_nearest) = if target_continental_area <= 0.0 {
        (
            None,
            Some(nearest_material_map(
                surface,
                topology,
                source,
                CrustKind::Oceanic,
            )?),
        )
    } else if target_continental_area >= total_area {
        (
            Some(nearest_material_map(
                surface,
                topology,
                source,
                CrustKind::Continental,
            )?),
            None,
        )
    } else {
        let continental = nearest_material_map(surface, topology, source, CrustKind::Continental)?;
        let oceanic = nearest_material_map(surface, topology, source, CrustKind::Oceanic)?;
        let material_phase = diffuse_material_phase(topology, remapped);
        let mut order = (0..cell_count).collect::<Vec<_>>();
        order.sort_by(|&first, &second| {
            material_phase[second]
                .total_cmp(&material_phase[first])
                .then_with(|| {
                    geometric_material_affinity(surface, source, &continental, &oceanic, second)
                        .total_cmp(&geometric_material_affinity(
                            surface,
                            source,
                            &continental,
                            &oceanic,
                            first,
                        ))
                })
                .then_with(|| {
                    material_affinity(&continental, &oceanic, second).cmp(&material_affinity(
                        &continental,
                        &oceanic,
                        first,
                    ))
                })
                .then_with(|| first.cmp(&second))
        });
        let mut area = 0.0;
        for index in order {
            let next_area = area + surface.cells()[index].area.get();
            if next_area <= target_continental_area
                || (next_area - target_continental_area).abs()
                    <= (area - target_continental_area).abs()
            {
                selected[index] = true;
                area = next_area;
            } else {
                break;
            }
        }
        (Some(continental), Some(oceanic))
    };
    if target_continental_area >= total_area {
        selected.fill(true);
    }

    for (index, sample) in remapped.iter_mut().enumerate() {
        let desired = if selected[index] {
            CrustKind::Continental
        } else {
            CrustKind::Oceanic
        };
        if sample.kind == desired {
            continue;
        }
        let nearest = match desired {
            CrustKind::Continental => continental_nearest
                .as_ref()
                .expect("positive continental area has a nearest-material map"),
            CrustKind::Oceanic => oceanic_nearest
                .as_ref()
                .expect("non-total continental area has an oceanic map"),
        };
        let source_sample = source[nearest.source[index]];
        let owner = sample.owner;
        let cell = &surface.cells()[index];
        *sample = source_sample;
        sample.owner = owner;
        sample.position = cell.centroid;
        sample.anchor = cell.id;
        sample.lineation = transport_lineation(&source_sample, cell.centroid);
    }
    Ok(())
}

fn diffuse_material_phase(topology: &NaturalTopologyIndex, remapped: &[CrustSample]) -> Vec<f64> {
    let current = remapped
        .iter()
        .map(|sample| match sample.kind {
            CrustKind::Continental => 1.0,
            CrustKind::Oceanic => -1.0,
        })
        .collect::<Vec<_>>();
    diffuse_phase_values(topology, current)
}

fn diffuse_phase_values(topology: &NaturalTopologyIndex, mut current: Vec<f64>) -> Vec<f64> {
    let mut next = vec![0.0; current.len()];
    for _ in 0..MATERIAL_HEAT_STEPS {
        for (index, arcs) in topology.arcs().iter().enumerate() {
            let mut weighted_sum = 0.0;
            let mut weight_sum = 0.0;
            for arc in arcs {
                let weight = 1.0 / arc.traversal_cost.max(1) as f64;
                weighted_sum += weight * current[arc.neighbor.raw() as usize];
                weight_sum += weight;
            }
            let neighbor_mean = if weight_sum > 0.0 {
                weighted_sum / weight_sum
            } else {
                current[index]
            };
            next[index] = current[index] * (1.0 - MATERIAL_HEAT_NEIGHBOR_SHARE)
                + neighbor_mean * MATERIAL_HEAT_NEIGHBOR_SHARE;
        }
        std::mem::swap(&mut current, &mut next);
    }
    current
}

fn occupied_material_area(
    surface: &SphericalSurfaceSnapshot,
    samples: &[CrustSample],
    kind: CrustKind,
) -> Result<f64, ResampleError> {
    let mut occupied = vec![false; surface.cells().len()];
    let mut area = 0.0;
    for (sample_index, sample) in samples.iter().enumerate() {
        if sample.kind != kind {
            continue;
        }
        let index = sample.anchor.raw() as usize;
        let cell = surface
            .cells()
            .get(index)
            .ok_or(ResampleError::InvalidFinalAnchor {
                sample: sample_index,
                anchor: sample.anchor,
            })?;
        if !occupied[index] {
            occupied[index] = true;
            area += cell.area.get();
        }
    }
    Ok(area)
}

struct NearestMaterialMap {
    distance: Vec<u64>,
    source: Vec<usize>,
}

fn nearest_material_map(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    samples: &[CrustSample],
    kind: CrustKind,
) -> Result<NearestMaterialMap, ResampleError> {
    let mut distance = vec![u64::MAX; surface.cells().len()];
    let mut nearest = vec![usize::MAX; surface.cells().len()];
    let mut pending = BinaryHeap::new();
    for (sample_index, sample) in samples.iter().enumerate() {
        if sample.kind != kind {
            continue;
        }
        let cell_index = sample.anchor.raw() as usize;
        if cell_index >= surface.cells().len() {
            return Err(ResampleError::InvalidFinalAnchor {
                sample: sample_index,
                anchor: sample.anchor,
            });
        }
        if (0, sample_index) < (distance[cell_index], nearest[cell_index]) {
            distance[cell_index] = 0;
            nearest[cell_index] = sample_index;
            pending.push(Reverse((0_u64, sample_index, sample.anchor.raw())));
        }
    }
    if pending.is_empty() {
        return Err(ResampleError::MissingMaterialSource { kind });
    }

    while let Some(Reverse((cost, source_index, raw_cell))) = pending.pop() {
        let cell_index = raw_cell as usize;
        if (distance[cell_index], nearest[cell_index]) != (cost, source_index) {
            continue;
        }
        for arc in &topology.arcs()[cell_index] {
            let neighbor = arc.neighbor.raw() as usize;
            let candidate = (cost.saturating_add(arc.traversal_cost), source_index);
            if candidate < (distance[neighbor], nearest[neighbor]) {
                distance[neighbor] = candidate.0;
                nearest[neighbor] = candidate.1;
                pending.push(Reverse((candidate.0, candidate.1, arc.neighbor.raw())));
            }
        }
    }

    Ok(NearestMaterialMap {
        distance,
        source: nearest,
    })
}

fn material_affinity(
    continental: &NearestMaterialMap,
    oceanic: &NearestMaterialMap,
    cell: usize,
) -> i128 {
    i128::from(oceanic.distance[cell]) - i128::from(continental.distance[cell])
}

fn geometric_material_affinity(
    surface: &SphericalSurfaceSnapshot,
    samples: &[CrustSample],
    continental: &NearestMaterialMap,
    oceanic: &NearestMaterialMap,
    cell: usize,
) -> f64 {
    let target = surface.cells()[cell].centroid;
    let continental_distance = target
        .dot(samples[continental.source[cell]].position)
        .clamp(-1.0, 1.0)
        .acos();
    let oceanic_distance = target
        .dot(samples[oceanic.source[cell]].position)
        .clamp(-1.0, 1.0)
        .acos();
    oceanic_distance - continental_distance
}

#[derive(Clone, Copy, Debug)]
struct DomainMarker {
    lineage: LineageId,
    cell: CellId,
}

/// Reconstructs one connected material domain per live lineage using a
/// marker-based watershed on the authoritative spherical adjacency graph.
///
/// The per-cell resampling above remains the data term: crossing into a cell
/// whose locally strongest moved sample belongs to another lineage pays one
/// short-cell penalty.  A single reliable marker per lineage supplies the
/// topological constraint.  This is the graph analogue of marker-controlled
/// watershed/Potts regularization, and avoids turning the result back into a
/// fresh geometric Voronoi partition.
fn reconstruct_connected_plate_domains(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    source_samples: &[CrustSample],
    coverage: &super::contacts::CoverageScratch,
    next: &mut TectonicState,
    local_candidates: &mut Vec<usize>,
) -> Result<(), ResampleError> {
    let provisional = next
        .samples
        .iter()
        .map(|sample| sample.owner)
        .collect::<Vec<_>>();
    let markers = select_domain_markers(surface, source_samples, &mut next.plates)?;
    if markers.is_empty() {
        return Err(ResampleError::NoLiveLineages);
    }

    let owners = evidence_guided_watershed(topology, &provisional, &markers)?;
    for (cell_index, owner) in owners.into_iter().enumerate() {
        if provisional[cell_index] == owner {
            continue;
        }
        let cell = CellId::from_raw(cell_index as u32);
        next.samples[cell_index] = resample_cell_for_owner(
            surface,
            topology,
            source_samples,
            coverage,
            cell,
            owner,
            local_candidates,
        )?;
    }
    Ok(())
}

/// V5 keeps the advected ownership evidence dominant and uses the watershed
/// only to repair connectivity. V4's one-cell mismatch penalty repeatedly
/// regrew near-geometric marker Voronoi domains and erased warped boundaries.
fn reconstruct_connected_plate_domains_v5(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    source_samples: &[CrustSample],
    coverage: &super::contacts::CoverageScratch,
    next: &mut TectonicState,
    local_candidates: &mut Vec<usize>,
) -> Result<(), ResampleError> {
    let provisional = next
        .samples
        .iter()
        .map(|sample| sample.owner)
        .collect::<Vec<_>>();
    let markers = select_domain_markers(surface, source_samples, &mut next.plates)?;
    if markers.is_empty() {
        return Err(ResampleError::NoLiveLineages);
    }
    let owners = evidence_guided_watershed_with_penalty(topology, &provisional, &markers, 12.0)?;
    for (cell_index, owner) in owners.into_iter().enumerate() {
        if provisional[cell_index] == owner {
            continue;
        }
        let cell = CellId::from_raw(cell_index as u32);
        next.samples[cell_index] = resample_cell_for_owner(
            surface,
            topology,
            source_samples,
            coverage,
            cell,
            owner,
            local_candidates,
        )?;
    }
    Ok(())
}

fn select_domain_markers(
    surface: &SphericalSurfaceSnapshot,
    samples: &[CrustSample],
    plates: &mut Vec<super::model::ActivePlate>,
) -> Result<Vec<DomainMarker>, ResampleError> {
    let live_lineages = plates
        .iter()
        .filter(|plate| samples.iter().any(|sample| sample.owner == plate.lineage))
        .count();
    if live_lineages > surface.cells().len() {
        return Err(ResampleError::DomainMarkerCapacityExceeded {
            lineages: live_lineages,
            cells: surface.cells().len(),
        });
    }
    let mut used_cells = vec![false; surface.cells().len()];
    let mut markers = Vec::with_capacity(plates.len());
    plates.retain_mut(|plate| {
        let mut candidates = samples
            .iter()
            .enumerate()
            .filter(|(_, sample)| sample.owner == plate.lineage)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return false;
        }
        candidates.sort_by(|(first_index, first), (second_index, second)| {
            let first_target = surface.cells()[first.anchor.raw() as usize].centroid;
            let second_target = surface.cells()[second.anchor.raw() as usize].centroid;
            second
                .position
                .dot(second_target)
                .total_cmp(&first.position.dot(first_target))
                .then_with(|| first.anchor.cmp(&second.anchor))
                .then_with(|| first_index.cmp(second_index))
        });

        let preferred = candidates
            .iter()
            .find(|(_, sample)| !used_cells[sample.anchor.raw() as usize])
            .map(|(_, sample)| sample.anchor);
        let cell = preferred.unwrap_or_else(|| {
            let direction = candidates[0].1.position;
            surface
                .cells()
                .iter()
                .filter(|cell| !used_cells[cell.id.raw() as usize])
                .max_by(|first, second| {
                    first
                        .centroid
                        .dot(direction)
                        .total_cmp(&second.centroid.dot(direction))
                        .then_with(|| second.id.cmp(&first.id))
                })
                .expect("live lineages cannot outnumber material cells")
                .id
        });
        used_cells[cell.raw() as usize] = true;
        plate.representative = cell;
        markers.push(DomainMarker {
            lineage: plate.lineage,
            cell,
        });
        true
    });
    Ok(markers)
}

fn evidence_guided_watershed(
    topology: &NaturalTopologyIndex,
    provisional: &[LineageId],
    markers: &[DomainMarker],
) -> Result<Vec<LineageId>, ResampleError> {
    evidence_guided_watershed_with_penalty(
        topology,
        provisional,
        markers,
        DOMAIN_EVIDENCE_PENALTY_SHORT_SIDE_FRACTION,
    )
}

fn evidence_guided_watershed_with_penalty(
    topology: &NaturalTopologyIndex,
    provisional: &[LineageId],
    markers: &[DomainMarker],
    penalty_short_side_fraction: f64,
) -> Result<Vec<LineageId>, ResampleError> {
    let cell_count = topology.cell_count();
    let mut costs = vec![u64::MAX; cell_count];
    let mut owners = vec![None; cell_count];
    let mut pending = BinaryHeap::new();
    for marker in markers {
        let index = marker.cell.raw() as usize;
        costs[index] = 0;
        owners[index] = Some(marker.lineage);
        pending.push(Reverse((0_u64, marker.lineage.raw(), marker.cell.raw())));
    }
    let mismatch_penalty = topology
        .quantized_short_side_fraction(penalty_short_side_fraction)
        .max(1);

    while let Some(Reverse((cost, raw_lineage, raw_cell))) = pending.pop() {
        let cell_index = raw_cell as usize;
        let lineage = LineageId::from_raw(raw_lineage);
        if costs[cell_index] != cost || owners[cell_index] != Some(lineage) {
            continue;
        }
        for arc in &topology.arcs()[cell_index] {
            let neighbor = arc.neighbor.raw() as usize;
            let data_cost = if provisional[neighbor] == lineage {
                0
            } else {
                mismatch_penalty
            };
            let candidate_cost = cost
                .saturating_add(arc.traversal_cost)
                .saturating_add(data_cost);
            let candidate_key = (candidate_cost, raw_lineage);
            let current_key = (
                costs[neighbor],
                owners[neighbor].map_or(u32::MAX, LineageId::raw),
            );
            if candidate_key < current_key {
                costs[neighbor] = candidate_cost;
                owners[neighbor] = Some(lineage);
                pending.push(Reverse((candidate_cost, raw_lineage, arc.neighbor.raw())));
            }
        }
    }

    owners
        .into_iter()
        .enumerate()
        .map(|(index, owner)| {
            owner.ok_or(ResampleError::UnassignedDomainCell {
                cell: CellId::from_raw(index as u32),
            })
        })
        .collect()
}

fn resample_cell(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    samples: &[CrustSample],
    coverage: &super::contacts::CoverageScratch,
    cell: CellId,
    local_candidates: &mut Vec<usize>,
) -> Result<CrustSample, ResampleError> {
    let target = surface
        .cell(cell)
        .expect("validated spherical cell IDs are contiguous")
        .centroid;
    let winner_index = coverage_winner(samples, coverage, cell, target)?;
    let winner = samples[winner_index];

    resample_cell_from_winner(
        surface,
        topology,
        samples,
        coverage,
        cell,
        winner_index,
        winner,
        local_candidates,
    )
}

fn resample_cell_for_owner(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    samples: &[CrustSample],
    coverage: &super::contacts::CoverageScratch,
    cell: CellId,
    owner: LineageId,
    local_candidates: &mut Vec<usize>,
) -> Result<CrustSample, ResampleError> {
    let target = surface
        .cell(cell)
        .expect("validated spherical cell IDs are contiguous")
        .centroid;
    local_candidates.clear();
    append_owner_candidates(coverage, samples, cell, owner, local_candidates)?;
    for arc in &topology.arcs()[cell.raw() as usize] {
        append_owner_candidates(coverage, samples, arc.neighbor, owner, local_candidates)?;
    }
    if local_candidates.is_empty() {
        for (index, sample) in samples.iter().enumerate() {
            if sample.owner == owner {
                local_candidates.push(index);
            }
        }
    }
    let winner_index = local_candidates
        .iter()
        .copied()
        .max_by(|&first, &second| {
            samples[first]
                .position
                .dot(target)
                .total_cmp(&samples[second].position.dot(target))
                .then_with(|| second.cmp(&first))
        })
        .expect("every watershed lineage has at least one source sample");
    let winner = samples[winner_index];
    resample_cell_from_winner(
        surface,
        topology,
        samples,
        coverage,
        cell,
        winner_index,
        winner,
        local_candidates,
    )
}

#[allow(clippy::too_many_arguments)]
fn resample_cell_from_winner(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    samples: &[CrustSample],
    coverage: &super::contacts::CoverageScratch,
    cell: CellId,
    winner_index: usize,
    winner: CrustSample,
    local_candidates: &mut Vec<usize>,
) -> Result<CrustSample, ResampleError> {
    let target = surface
        .cell(cell)
        .expect("validated spherical cell IDs are contiguous")
        .centroid;

    local_candidates.clear();
    local_candidates.push(winner_index);
    append_compatible_candidates(coverage, samples, cell, winner, local_candidates)?;
    for arc in &topology.arcs()[cell.raw() as usize] {
        append_compatible_candidates(coverage, samples, arc.neighbor, winner, local_candidates)?;
    }
    if local_candidates.len() < 3 {
        for arc in &topology.arcs()[cell.raw() as usize] {
            for second in &topology.arcs()[arc.neighbor.raw() as usize] {
                append_compatible_candidates(
                    coverage,
                    samples,
                    second.neighbor,
                    winner,
                    local_candidates,
                )?;
            }
        }
    }
    let mut result = interpolate_from_candidates(target, winner, samples, local_candidates);
    result.position = target;
    result.anchor = cell;
    Ok(result)
}

pub(super) fn interpolate_dense_control_material(
    target: UnitVector3,
    winner_cell: CellId,
    topology: &NaturalTopologyIndex,
    samples: &[CrustSample],
    local_candidates: &mut Vec<usize>,
) -> CrustSample {
    let winner_index = winner_cell.raw() as usize;
    let winner = samples[winner_index];
    local_candidates.clear();
    local_candidates.push(winner_index);
    for arc in &topology.arcs()[winner_index] {
        append_dense_compatible_candidate(samples, arc.neighbor, winner, local_candidates);
    }
    if local_candidates.len() < 3 {
        for arc in &topology.arcs()[winner_index] {
            for second in &topology.arcs()[arc.neighbor.raw() as usize] {
                append_dense_compatible_candidate(
                    samples,
                    second.neighbor,
                    winner,
                    local_candidates,
                );
            }
        }
    }
    interpolate_from_candidates(target, winner, samples, local_candidates)
}

fn append_dense_compatible_candidate(
    samples: &[CrustSample],
    cell: CellId,
    winner: CrustSample,
    output: &mut Vec<usize>,
) {
    let index = cell.raw() as usize;
    let sample = samples[index];
    if sample.owner == winner.owner
        && sample.kind == winner.kind
        && sample.orogeny == winner.orogeny
        && !output.contains(&index)
    {
        output.push(index);
    }
}

fn interpolate_from_candidates(
    target: UnitVector3,
    winner: CrustSample,
    samples: &[CrustSample],
    local_candidates: &mut Vec<usize>,
) -> CrustSample {
    local_candidates.sort_by(|&first, &second| {
        samples[second]
            .position
            .dot(target)
            .total_cmp(&samples[first].position.dot(target))
            .then_with(|| first.cmp(&second))
    });
    local_candidates.truncate(MAXIMUM_TRIANGLE_CANDIDATES);

    if let Some((indices, weights)) = containing_triangle(target, samples, local_candidates) {
        interpolate_material(target, winner, samples, indices, weights)
    } else {
        winner
    }
}

fn append_owner_candidates(
    coverage: &super::contacts::CoverageScratch,
    samples: &[CrustSample],
    cell: CellId,
    owner: LineageId,
    output: &mut Vec<usize>,
) -> Result<(), ResampleError> {
    for &raw_index in coverage.sample_indices(cell) {
        let index = raw_index as usize;
        let sample = samples
            .get(index)
            .ok_or(ResampleError::InvalidCoverageSample {
                sample: index,
                samples: samples.len(),
            })?;
        if sample.owner == owner && !output.contains(&index) {
            output.push(index);
        }
    }
    Ok(())
}

fn sample_score(
    samples: &[CrustSample],
    index: usize,
    target: UnitVector3,
) -> Result<f64, ResampleError> {
    samples
        .get(index)
        .map(|sample| sample.position.dot(target))
        .ok_or(ResampleError::InvalidCoverageSample {
            sample: index,
            samples: samples.len(),
        })
}

fn append_compatible_candidates(
    coverage: &super::contacts::CoverageScratch,
    samples: &[CrustSample],
    cell: CellId,
    winner: CrustSample,
    output: &mut Vec<usize>,
) -> Result<(), ResampleError> {
    for &raw_index in coverage.sample_indices(cell) {
        let index = raw_index as usize;
        let sample = samples
            .get(index)
            .ok_or(ResampleError::InvalidCoverageSample {
                sample: index,
                samples: samples.len(),
            })?;
        if sample.owner == winner.owner
            && sample.kind == winner.kind
            && sample.orogeny == winner.orogeny
            && !output.contains(&index)
        {
            output.push(index);
        }
    }
    Ok(())
}

fn containing_triangle(
    target: UnitVector3,
    samples: &[CrustSample],
    candidates: &[usize],
) -> Option<([usize; 3], [f64; 3])> {
    for first in 0..candidates.len() {
        for second in first + 1..candidates.len() {
            for third in second + 1..candidates.len() {
                let indices = [candidates[first], candidates[second], candidates[third]];
                let points = indices.map(|index| samples[index].position);
                let area = spherical_triangle_area_unit(points[0], points[1], points[2]);
                if !area.is_finite() || area <= TRIANGLE_AREA_EPSILON {
                    continue;
                }
                let mut weights = [
                    spherical_triangle_area_unit(target, points[1], points[2]) / area,
                    spherical_triangle_area_unit(points[0], target, points[2]) / area,
                    spherical_triangle_area_unit(points[0], points[1], target) / area,
                ];
                let sum = weights.iter().sum::<f64>();
                let tolerance = (area * 1.0e-6).max(1.0e-12);
                if weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight >= 0.0)
                    && (sum - 1.0).abs() <= tolerance / area
                {
                    for weight in &mut weights {
                        *weight /= sum;
                    }
                    return Some((indices, weights));
                }
            }
        }
    }
    None
}

fn interpolate_material(
    target: UnitVector3,
    winner: CrustSample,
    samples: &[CrustSample],
    indices: [usize; 3],
    weights: [f64; 3],
) -> CrustSample {
    let blend = |field: fn(&CrustSample) -> f32| -> f32 {
        indices
            .into_iter()
            .zip(weights)
            .map(|(index, weight)| f64::from(field(&samples[index])) * weight)
            .sum::<f64>() as f32
    };
    let lineation = interpolate_lineation(target, samples, indices, weights);
    CrustSample {
        position: target,
        anchor: winner.anchor,
        owner: winner.owner,
        kind: winner.kind,
        thickness_km: blend(|sample| sample.thickness_km),
        age_myr: match winner.kind {
            CrustKind::Continental => winner.age_myr,
            CrustKind::Oceanic => blend(|sample| sample.age_myr),
        },
        tectonic_elevation_m: blend(|sample| sample.tectonic_elevation_m),
        lineation,
        orogeny: winner.orogeny,
        orogeny_age_myr: match winner.orogeny {
            SphericalOrogenyKind::None => winner.orogeny_age_myr,
            SphericalOrogenyKind::Andean | SphericalOrogenyKind::Himalayan => {
                blend(|sample| sample.orogeny_age_myr)
            }
        },
        material: winner.material,
    }
}

fn interpolate_lineation(
    target: UnitVector3,
    samples: &[CrustSample],
    indices: [usize; 3],
    weights: [f64; 3],
) -> [f32; 2] {
    let mut global = [0.0; 3];
    for (index, weight) in indices.into_iter().zip(weights) {
        let direction = global_lineation(&samples[index]);
        for axis in 0..3 {
            global[axis] += weight * direction[axis];
        }
    }
    lineation_at(target, global)
}

pub(super) fn transport_lineation(sample: &CrustSample, target: UnitVector3) -> [f32; 2] {
    lineation_at(target, global_lineation(sample))
}

fn global_lineation(sample: &CrustSample) -> [f64; 3] {
    let (east, north) = canonical_east_north_basis(sample.position);
    std::array::from_fn(|axis| {
        east[axis] * f64::from(sample.lineation[0]) + north[axis] * f64::from(sample.lineation[1])
    })
}

fn lineation_at(target: UnitVector3, mut global: [f64; 3]) -> [f32; 2] {
    let radial = target.components();
    let radial_component = dot(global, radial);
    for axis in 0..3 {
        global[axis] -= radial_component * radial[axis];
    }
    let (east, north) = canonical_east_north_basis(target);
    let components = [dot(global, east), dot(global, north)];
    let length = components[0].hypot(components[1]);
    if length <= f64::EPSILON {
        [0.0; 2]
    } else {
        [
            (components[0] / length) as f32,
            (components[1] / length) as f32,
        ]
    }
}

pub(super) fn canonicalize_final_plates(
    surface: &SphericalSurfaceSnapshot,
    state: TectonicState,
) -> Result<CanonicalTectonicState, ResampleError> {
    let cell_count = surface.cells().len();
    if state.samples.len() != cell_count {
        return Err(ResampleError::StateCardinalityMismatch {
            samples: state.samples.len(),
            surface_cells: cell_count,
        });
    }
    let plate_table = state.plates;

    let mut dense = vec![None; cell_count];
    for (index, sample) in state.samples.into_iter().enumerate() {
        let cell = sample.anchor.raw() as usize;
        if cell >= cell_count {
            return Err(ResampleError::InvalidFinalAnchor {
                sample: index,
                anchor: sample.anchor,
            });
        }
        if dense[cell].replace(sample).is_some() {
            return Err(ResampleError::DuplicateFinalAnchor {
                cell: sample.anchor,
            });
        }
    }
    let mut samples = dense
        .into_iter()
        .enumerate()
        .map(|(index, sample)| {
            sample.ok_or(ResampleError::MissingFinalAnchor {
                cell: CellId::from_raw(index as u32),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    relax_passive_margins(surface, &mut samples);

    let mut adjacency = vec![Vec::new(); cell_count];
    for edge in surface.edges() {
        adjacency[edge.cells[0].raw() as usize].push(edge.cells[1]);
        adjacency[edge.cells[1].raw() as usize].push(edge.cells[0]);
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable();
    }

    let mut reached = vec![false; cell_count];
    let mut components = Vec::new();
    for start in 0..cell_count {
        if reached[start] {
            continue;
        }
        let lineage = samples[start].owner;
        let mut cells = Vec::new();
        let mut queue = VecDeque::from([CellId::from_raw(start as u32)]);
        reached[start] = true;
        while let Some(cell) = queue.pop_front() {
            cells.push(cell);
            for &neighbor in &adjacency[cell.raw() as usize] {
                let index = neighbor.raw() as usize;
                if !reached[index] && samples[index].owner == lineage {
                    reached[index] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        cells.sort_unstable();
        let representative = representative_cell(surface, &cells);
        let rotation = plate_table
            .binary_search_by_key(&lineage, |plate| plate.lineage)
            .ok()
            .map(|index| plate_table[index].rotation)
            .ok_or(ResampleError::UnknownLineage { lineage })?;
        components.push((lineage, representative, cells, rotation));
    }
    components.sort_by_key(|(lineage, representative, _, _)| (*lineage, *representative));

    let minimum = 2;
    let maximum = MAX_PLATE_COUNT as usize;
    if !(minimum..=maximum).contains(&components.len()) {
        return Err(ResampleError::FinalPlateCountOutOfRange {
            found: components.len(),
            min: minimum,
            max: maximum,
        });
    }

    let mut raw_owners = vec![0; cell_count];
    let mut plates = Vec::with_capacity(components.len());
    for (index, (_, representative, cells, rotation)) in components.into_iter().enumerate() {
        let plate = PlateId::from_raw(index as u32);
        let canonical_lineage = LineageId::from_raw(index as u32);
        for cell in cells {
            let cell_index = cell.raw() as usize;
            raw_owners[cell_index] = plate.raw();
            samples[cell_index].owner = canonical_lineage;
        }
        plates.push(SphericalPlate::new(plate, representative, rotation));
    }

    Ok(CanonicalTectonicState {
        samples,
        plates,
        cell_plates: PlateIdField::from_raw(raw_owners),
    })
}

fn representative_cell(surface: &SphericalSurfaceSnapshot, cells: &[CellId]) -> CellId {
    let mut weighted = [0.0; 3];
    let mut total_area = 0.0;
    for &cell in cells {
        let record = &surface.cells()[cell.raw() as usize];
        let area = record.area.get();
        total_area += area;
        for (slot, component) in weighted.iter_mut().zip(record.centroid.components()) {
            *slot += area * component;
        }
    }
    let length = dot(weighted, weighted).sqrt();
    let degenerate_tolerance = total_area * 64.0 * f64::EPSILON;
    if !length.is_finite() || length <= degenerate_tolerance {
        return *cells
            .iter()
            .min()
            .expect("a connected component always contains a cell");
    }
    let direction = weighted.map(|component| component / length);
    cells.iter().copied().fold(cells[0], |best, candidate| {
        let candidate_score = dot(
            surface.cells()[candidate.raw() as usize]
                .centroid
                .components(),
            direction,
        );
        let best_score = dot(
            surface.cells()[best.raw() as usize].centroid.components(),
            direction,
        );
        if candidate_score > best_score || (candidate_score == best_score && candidate < best) {
            candidate
        } else {
            best
        }
    })
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first.into_iter().zip(second).map(|(a, b)| a * b).sum()
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_final_plates, conservative_material_remap, conservative_material_resample_v5,
        occupied_material_area, rebalance_columns_to_cells, resample_current_state,
        resampling_interval_steps, ResampleError,
    };
    use crate::generators::natural::foundation::tectonics::contacts::CoverageScratch;
    use crate::generators::natural::foundation::tectonics::model::{
        ActivePlate, CrustSample, EvolutionMaterialLedger, LineageId, MaterialColumn, TectonicState,
    };
    use crate::generators::natural::foundation::tectonics::workspace::TectonicWorkspace;
    use crate::generators::natural::topology::{
        farthest_point_seeds, multi_source_ownership, NaturalTopologyIndex,
    };
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR,
        NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::{
        canonical_east_north_basis, SphericalNaturalSurface, SphericalSurfaceSnapshot, UnitVector3,
    };
    use crate::world::{CellId, Meters, PlateId, SphericalSpaceSpec};

    fn fixture(cells: u32) -> (SphericalSurfaceSnapshot, NaturalTopologyIndex) {
        let surface = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(6_371_000.0).unwrap(),
            target_cell_count: cells,
        })
        .unwrap();
        let view = SphericalNaturalSurface::from_validated(&surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        (surface, topology)
    }

    fn rotation(lineage: u32, rate: u64) -> SphericalPlateRotation {
        let axis = match lineage % 3 {
            0 => UnitVector3::new(1.0, 0.0, 0.0).unwrap(),
            1 => UnitVector3::new(0.0, 1.0, 0.0).unwrap(),
            _ => UnitVector3::new(0.0, 0.0, 1.0).unwrap(),
        };
        SphericalPlateRotation::new(axis, rate).unwrap()
    }

    fn owners_for(surface: &SphericalSurfaceSnapshot, count: usize) -> Vec<LineageId> {
        let view = SphericalNaturalSurface::from_validated(surface).unwrap();
        let topology = NaturalTopologyIndex::from_surface(&view);
        let seeds = farthest_point_seeds(&topology, count, 0);
        multi_source_ownership(&topology, &seeds)
            .owners
            .into_iter()
            .map(LineageId::from_raw)
            .collect()
    }

    fn sample(cell: CellId, position: UnitVector3, owner: LineageId, marker: u32) -> CrustSample {
        let kind = if marker % 3 == 0 {
            CrustKind::Continental
        } else {
            CrustKind::Oceanic
        };
        CrustSample {
            position,
            anchor: cell,
            owner,
            kind,
            thickness_km: match kind {
                CrustKind::Continental => 30.0 + (marker % 16) as f32 * 0.5,
                CrustKind::Oceanic => 5.0 + (marker % 5) as f32 * 0.5,
            },
            age_myr: match kind {
                CrustKind::Continental => CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                CrustKind::Oceanic => (marker % 200) as f32,
            },
            tectonic_elevation_m: -4_000.0 + marker as f32 * 7.0,
            lineation: if marker % 4 == 0 {
                [1.0, 0.0]
            } else {
                [0.0; 2]
            },
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(
                kind,
                1.0,
                match kind {
                    CrustKind::Continental => 30.0 + (marker % 16) as f32 * 0.5,
                    CrustKind::Oceanic => 5.0 + (marker % 5) as f32 * 0.5,
                },
            )
            .unwrap(),
        }
    }

    fn state_from_owners(
        surface: &SphericalSurfaceSnapshot,
        owners: &[LineageId],
        plate_lineages: &[LineageId],
    ) -> TectonicState {
        let samples = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| sample(cell.id, cell.centroid, owners[index], index as u32))
            .collect();
        let plates = plate_lineages
            .iter()
            .copied()
            .map(|lineage| {
                let representative = owners
                    .iter()
                    .position(|&owner| owner == lineage)
                    .map_or(CellId::from_raw(0), |index| CellId::from_raw(index as u32));
                ActivePlate::new(lineage, representative, rotation(lineage.raw(), 10_000))
            })
            .collect();
        let next = plate_lineages
            .iter()
            .map(|lineage| lineage.raw())
            .max()
            .unwrap_or(0)
            + 1;
        TectonicState::new(samples, plates, next).unwrap()
    }

    /// G1e §3.4: a world with no plate motion must resample to itself. Any
    /// change in the continental mask here is displacement without a process.
    #[test]
    fn static_world_resamples_to_the_same_continental_mask() {
        use crate::engine::{derive_stage_seed, StageIdentity, StageRng};
        use crate::generators::natural::foundation::tectonics::initial_state::build_initial_state_v5;
        use crate::generators::natural::random::LabeledSubstreams;
        use crate::world::natural::{ResolvedWorldFormationPreset, TectonicSpec};
        use crate::world::RootSeed;

        let (surface, topology) = fixture(642);
        let preset = ResolvedWorldFormationPreset::Archipelago;
        let spec = TectonicSpec {
            continental_crust_fraction: preset.recommended_continental_crust_fraction(),
            ..TectonicSpec::default()
        };
        let mut rng = StageRng::from_seed(derive_stage_seed(
            RootSeed::new(42),
            StageIdentity::new("resample-static", 1, "sekai.test"),
        ));
        let streams = LabeledSubstreams::capture(&mut rng);
        let mut state =
            build_initial_state_v5(&surface, &topology, &spec, preset, &streams).unwrap();
        let still =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 1).unwrap();
        for plate in &mut state.plates {
            plate.rotation = still;
        }
        let before = state
            .samples
            .iter()
            .map(|sample| sample.kind)
            .collect::<Vec<_>>();
        let mut ledger = EvolutionMaterialLedger::capture_initial(&state).unwrap();
        let mut workspace = TectonicWorkspace::from_initial(state);
        conservative_resample_for_test(&surface, &topology, &mut workspace, &mut ledger);
        let after = workspace
            .current
            .samples
            .iter()
            .map(|sample| sample.kind)
            .collect::<Vec<_>>();
        let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        assert_eq!(
            changed, 0,
            "static resample changed {changed} cells' crust kind"
        );
    }

    fn conservative_resample_for_test(
        surface: &SphericalSurfaceSnapshot,
        topology: &NaturalTopologyIndex,
        workspace: &mut TectonicWorkspace,
        ledger: &mut EvolutionMaterialLedger,
    ) {
        super::resample_current_state_v5(surface, topology, workspace, ledger).unwrap();
    }

    #[test]
    fn conservative_remap_cannot_erase_current_continental_material() {
        let (surface, topology) = fixture(42);
        let owner = LineageId::from_raw(0);
        let mut source = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| sample(cell.id, cell.centroid, owner, index as u32 + 1))
            .collect::<Vec<_>>();
        for (index, material) in source.iter_mut().enumerate() {
            if index < 12 {
                material.kind = CrustKind::Continental;
                material.thickness_km = 38.0;
                material.age_myr = CONTINENTAL_CRUST_AGE_SENTINEL_MYR;
            } else {
                material.kind = CrustKind::Oceanic;
                material.thickness_km = 7.0;
                material.age_myr = 48.0;
            }
        }
        let mut remapped = source.clone();
        for material in &mut remapped {
            material.kind = CrustKind::Oceanic;
            material.thickness_km = 7.0;
            material.age_myr = 48.0;
        }
        let owners_before = remapped
            .iter()
            .map(|sample| sample.owner)
            .collect::<Vec<_>>();

        conservative_material_remap(&surface, &topology, &source, &mut remapped).unwrap();

        let expected_area = surface.cells()[..12]
            .iter()
            .map(|cell| cell.area.get())
            .sum::<f64>();
        let actual_area = surface
            .cells()
            .iter()
            .zip(&remapped)
            .filter(|(_, sample)| sample.kind == CrustKind::Continental)
            .map(|(cell, _)| cell.area.get())
            .sum::<f64>();
        let maximum_cell_area = surface
            .cells()
            .iter()
            .map(|cell| cell.area.get())
            .fold(0.0, f64::max);
        assert!((actual_area - expected_area).abs() <= maximum_cell_area);
        assert_eq!(
            remapped
                .iter()
                .map(|sample| sample.owner)
                .collect::<Vec<_>>(),
            owners_before
        );
        for (cell, sample) in surface.cells().iter().zip(&remapped) {
            assert_eq!(sample.anchor, cell.id);
            assert_eq!(sample.position, cell.centroid);
        }
    }

    #[test]
    fn v4_occupied_anchors_lose_overlap_but_v5_closes_all_extensive_material() {
        let (surface, topology) = fixture(42);
        let owner = LineageId::from_raw(0);
        let second = surface
            .opposite_cell(
                CellId::from_raw(0),
                surface.cell_edges(CellId::from_raw(0)).unwrap()[0],
            )
            .unwrap()
            .raw() as usize;
        let mut source_samples = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let mut value = sample(cell.id, cell.centroid, owner, index as u32 + 1);
                let kind = if index == 0 || index == second {
                    CrustKind::Continental
                } else {
                    CrustKind::Oceanic
                };
                let thickness = if kind == CrustKind::Continental {
                    38.0
                } else {
                    7.0
                };
                value.kind = kind;
                value.thickness_km = thickness;
                value.age_myr = if kind == CrustKind::Continental {
                    CONTINENTAL_CRUST_AGE_SENTINEL_MYR
                } else {
                    40.0
                };
                value.material = MaterialColumn::pure(kind, cell.area.get(), thickness).unwrap();
                value
            })
            .collect::<Vec<_>>();
        let mut extra = source_samples[second];
        extra.anchor = source_samples[0].anchor;
        extra.position = source_samples[0].position;
        source_samples.push(extra);
        let plate = ActivePlate::new(owner, CellId::from_raw(0), rotation(0, 10_000));
        let source = TectonicState::new(source_samples, vec![plate], 1).unwrap();
        let expected = source.material_totals().unwrap();
        let legacy_area =
            occupied_material_area(&surface, &source.samples, CrustKind::Continental).unwrap();
        assert!(legacy_area < expected.continental().reference_area_m2());

        let mut remapped = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let mut value = sample(cell.id, cell.centroid, owner, index as u32 + 2);
                value.kind = CrustKind::Oceanic;
                value.thickness_km = 7.0;
                value.age_myr = 40.0;
                value.material =
                    MaterialColumn::pure(CrustKind::Oceanic, cell.area.get(), 7.0).unwrap();
                value
            })
            .collect::<Vec<_>>();
        let mut ledger = EvolutionMaterialLedger::capture_initial(&source).unwrap();
        let mut coverage = CoverageScratch::with_cell_capacity(surface.cells().len());
        coverage
            .rebuild(surface.cells().len(), &source.samples)
            .unwrap();
        conservative_material_resample_v5(
            &surface,
            &topology,
            &source,
            &coverage,
            &mut remapped,
            &mut ledger,
        )
        .unwrap();
        let remapped_state = TectonicState::new(remapped.clone(), vec![plate], 1).unwrap();
        let totals = remapped_state.material_totals().unwrap();
        assert!(
            (totals.continental().volume_m3() - expected.continental().volume_m3()).abs()
                <= 1.0e-12 * expected.continental().volume_m3(),
            "continental volume is conserved"
        );
        for (index, value) in remapped.iter().enumerate() {
            let area = value.material.continental_reference_area_m2()
                + value.material.oceanic_reference_area_m2();
            let cell_area = surface.cells()[index].area.get();
            assert!(
                (area - cell_area).abs() <= 1.0e-6 * cell_area,
                "cell {index}"
            );
        }
        ledger.control_budget(&remapped_state).unwrap();

        let mut repeated = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let mut value = sample(cell.id, cell.centroid, owner, index as u32 + 2);
                value.kind = CrustKind::Oceanic;
                value.thickness_km = 7.0;
                value.age_myr = 40.0;
                value.material =
                    MaterialColumn::pure(CrustKind::Oceanic, cell.area.get(), 7.0).unwrap();
                value
            })
            .collect::<Vec<_>>();
        let mut repeated_ledger = EvolutionMaterialLedger::capture_initial(&source).unwrap();
        conservative_material_resample_v5(
            &surface,
            &topology,
            &source,
            &coverage,
            &mut repeated,
            &mut repeated_ledger,
        )
        .unwrap();
        assert_eq!(
            remapped
                .iter()
                .map(|sample| sample.material.bits())
                .collect::<Vec<_>>(),
            repeated
                .iter()
                .map(|sample| sample.material.bits())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn v5_deposition_sums_the_parcels_anchored_in_a_cell_without_moving_them() {
        let (surface, topology) = fixture(42);
        let owner = LineageId::from_raw(0);
        let thick_cell = surface.cells()[0].id;
        let mut source_samples = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let mut value = sample(cell.id, cell.centroid, owner, index as u32 + 1);
                value.kind = CrustKind::Oceanic;
                value.thickness_km = 7.0;
                value.age_myr = 40.0;
                value.material =
                    MaterialColumn::pure(CrustKind::Oceanic, cell.area.get(), 7.0).unwrap();
                value
            })
            .collect::<Vec<_>>();
        // A 50 km craton column anchored at cell 0 and a 25 km rifted column
        // that converged onto the same cell: two parcels, one cell.
        let thin_cell = surface
            .opposite_cell(thick_cell, surface.cell_edges(thick_cell).unwrap()[0])
            .unwrap();
        let thick_area = surface.cells()[0].area.get();
        let thin_area = surface.cells()[thin_cell.raw() as usize].area.get();
        source_samples[0].kind = CrustKind::Continental;
        source_samples[0].thickness_km = 50.0;
        source_samples[0].age_myr = CONTINENTAL_CRUST_AGE_SENTINEL_MYR;
        source_samples[0].material =
            MaterialColumn::pure(CrustKind::Continental, thick_area, 50.0).unwrap();
        let mut thin = source_samples[0];
        thin.thickness_km = 25.0;
        thin.material = MaterialColumn::pure(CrustKind::Continental, thin_area, 25.0).unwrap();
        thin.anchor = thin_cell;
        thin.position = surface.cells()[thin_cell.raw() as usize].centroid;
        source_samples[thin_cell.raw() as usize] = thin;
        let mut extra = thin;
        extra.anchor = thick_cell;
        extra.position = surface.cells()[0].centroid;
        source_samples.push(extra);
        let plate = ActivePlate::new(owner, thick_cell, rotation(0, 10_000));
        let source = TectonicState::new(source_samples, vec![plate], 1).unwrap();
        let expected = source.material_totals().unwrap();

        let mut remapped = surface
            .cells()
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let mut value = sample(cell.id, cell.centroid, owner, index as u32 + 2);
                value.kind = CrustKind::Oceanic;
                value.thickness_km = 7.0;
                value.age_myr = 40.0;
                value.material =
                    MaterialColumn::pure(CrustKind::Oceanic, cell.area.get(), 7.0).unwrap();
                value
            })
            .collect::<Vec<_>>();
        remapped[0] = source.samples[0];
        let mut coverage = CoverageScratch::with_cell_capacity(surface.cells().len());
        coverage
            .rebuild(surface.cells().len(), &source.samples)
            .unwrap();
        let mut ledger = EvolutionMaterialLedger::capture_initial(&source).unwrap();
        conservative_material_resample_v5(
            &surface,
            &topology,
            &source,
            &coverage,
            &mut remapped,
            &mut ledger,
        )
        .unwrap();

        let remapped_state = TectonicState::new(remapped.clone(), vec![plate], 1).unwrap();
        let totals = remapped_state.material_totals().unwrap();
        assert!(
            (totals.continental().volume_m3() - expected.continental().volume_m3()).abs()
                <= 1.0e-6 * expected.continental().volume_m3(),
            "continental volume is conserved"
        );
        ledger.control_budget(&remapped_state).unwrap();
        let cell_area = surface.cells()[0].area.get();
        assert!(
            (remapped[0].material.continental_reference_area_m2() - cell_area).abs()
                <= 1.0e-6 * cell_area,
            "the stacked parcels thicken their own cell instead of moving"
        );
        assert!(
            remapped[0].material.continental_thickness_km().unwrap() > 50.0,
            "two parcels in one cell mean a thicker column"
        );
        let elsewhere = remapped
            .iter()
            .enumerate()
            .filter(|(index, value)| {
                *index != 0 && value.material.continental_reference_area_m2() > 0.0
            })
            .count();
        assert_eq!(
            elsewhere, 1,
            "only cell 1's own continental parcel exists elsewhere"
        );
    }

    fn material_bits(sample: &CrustSample) -> (u32, [u32; 6], SphericalOrogenyKind) {
        (
            sample.kind.raw(),
            [
                sample.thickness_km.to_bits(),
                sample.age_myr.to_bits(),
                sample.tectonic_elevation_m.to_bits(),
                sample.lineation[0].to_bits(),
                sample.lineation[1].to_bits(),
                sample.orogeny_age_myr.to_bits(),
            ],
            sample.orogeny,
        )
    }

    fn offset(radial: UnitVector3, azimuth: f64, distance: f64) -> UnitVector3 {
        let (east, north) = canonical_east_north_basis(radial);
        let tangent = [
            east[0] * azimuth.cos() + north[0] * azimuth.sin(),
            east[1] * azimuth.cos() + north[1] * azimuth.sin(),
            east[2] * azimuth.cos() + north[2] * azimuth.sin(),
        ];
        let source = radial.components();
        UnitVector3::new(
            source[0] * distance.cos() + tangent[0] * distance.sin(),
            source[1] * distance.cos() + tangent[1] * distance.sin(),
            source[2] * distance.cos() + tangent[2] * distance.sin(),
        )
        .unwrap()
    }

    #[test]
    fn interval_tracks_maximum_displacement_and_stays_inside_paper_bounds() {
        let (surface, _) = fixture(42);
        let owners = owners_for(&surface, 2);
        let lineages = [LineageId::from_raw(0), LineageId::from_raw(1)];
        let mut slow = state_from_owners(&surface, &owners, &lineages);
        for plate in &mut slow.plates {
            plate.rotation = rotation(plate.lineage.raw(), 1);
        }
        assert_eq!(resampling_interval_steps(&slow), 60);

        let mut fast = state_from_owners(&surface, &owners, &lineages);
        fast.plates[0].rotation = rotation(
            fast.plates[0].lineage.raw(),
            MAX_SPHERICAL_PLATE_ANGULAR_RATE_PRAD_PER_YEAR,
        );
        assert_eq!(resampling_interval_steps(&fast), 10);
    }

    #[test]
    fn resampling_handles_one_overlap_gap_ties_and_reuses_the_other_buffer() {
        let (surface, topology) = fixture(42);
        let owners = owners_for(&surface, 2);
        let lineages = [LineageId::from_raw(0), LineageId::from_raw(1)];

        let unique_owners = (0..surface.cells().len())
            .map(|index| LineageId::from_raw(index as u32))
            .collect::<Vec<_>>();
        let unique_lineages = unique_owners.clone();
        let mut unique = state_from_owners(&surface, &unique_owners, &unique_lineages);
        // Keep this buffer-reuse/identity fixture single-phase. Mixed checkerboard
        // material is intentionally regularized by the conservative MBO pass and
        // has its own area/provenance tests below.
        for sample in &mut unique.samples {
            sample.kind = CrustKind::Oceanic;
            sample.thickness_km = 7.0;
            sample.age_myr = 50.0;
        }
        let expected = unique.samples.clone();
        let mut workspace = TectonicWorkspace::from_initial(unique);
        let reused = workspace.next.samples.as_ptr();
        resample_current_state(&surface, &topology, &mut workspace).unwrap();
        assert_eq!(workspace.current.samples.len(), surface.cells().len());
        assert_eq!(workspace.current.samples.as_ptr(), reused);
        for (actual, expected) in workspace.current.samples.iter().zip(expected) {
            assert_eq!(actual.anchor, expected.anchor);
            assert_eq!(material_bits(actual), material_bits(&expected));
        }

        let mut overlap = state_from_owners(&surface, &owners, &lineages);
        let target = surface.cells()[0].centroid;
        overlap.samples[0].position = surface.cells()[1].centroid;
        overlap.samples[0].thickness_km = 6.0;
        let mut nearer = overlap.samples[0];
        nearer.position = target;
        nearer.thickness_km = 8.0;
        overlap.samples.push(nearer);
        let mut workspace = TectonicWorkspace::from_initial(overlap);
        resample_current_state(&surface, &topology, &mut workspace).unwrap();
        assert_eq!(
            workspace.current.samples[0].thickness_km.to_bits(),
            8.0_f32.to_bits()
        );

        let mut tied = state_from_owners(&surface, &owners, &lineages);
        tied.samples[0].position = target;
        tied.samples[0].thickness_km = 6.25;
        let mut later = tied.samples[0];
        later.owner = if tied.samples[0].owner == lineages[0] {
            lineages[1]
        } else {
            lineages[0]
        };
        later.thickness_km = 7.75;
        tied.samples.push(later);
        let mut workspace = TectonicWorkspace::from_initial(tied);
        resample_current_state(&surface, &topology, &mut workspace).unwrap();
        assert_eq!(
            workspace.current.samples[0].thickness_km.to_bits(),
            6.25_f32.to_bits()
        );

        let mut filled = state_from_owners(&surface, &owners, &lineages);
        filled.samples.remove(0);
        let mut ridge = filled.samples[0];
        ridge.anchor = CellId::from_raw(0);
        ridge.position = target;
        ridge.kind = CrustKind::Oceanic;
        ridge.thickness_km = 7.0;
        ridge.age_myr = 0.0;
        ridge.tectonic_elevation_m = -1_000.0;
        filled.samples.push(ridge);
        let mut workspace = TectonicWorkspace::from_initial(filled);
        resample_current_state(&surface, &topology, &mut workspace).unwrap();
        assert_eq!(
            workspace.current.samples[0].age_myr.to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(workspace.current.samples[0].tectonic_elevation_m, -1_000.0);
    }

    #[test]
    fn resampling_uses_spherical_barycentric_material_interpolation_and_rejects_gaps() {
        let (surface, topology) = fixture(42);
        let owners = owners_for(&surface, 2);
        let lineages = [LineageId::from_raw(0), LineageId::from_raw(1)];
        let mut state = state_from_owners(&surface, &owners, &lineages);
        let target = surface.cells()[0].centroid;
        let owner = state.samples[0].owner;
        state.samples.remove(0);
        for (index, value) in [6.0_f32, 7.0, 8.0].into_iter().enumerate() {
            let mut vertex = sample(
                CellId::from_raw(0),
                offset(target, index as f64 * std::f64::consts::TAU / 3.0, 0.03),
                owner,
                1,
            );
            vertex.kind = CrustKind::Oceanic;
            vertex.thickness_km = value;
            vertex.age_myr = value * 10.0;
            vertex.tectonic_elevation_m = value * 100.0;
            vertex.lineation = [1.0, 0.0];
            state.samples.push(vertex);
        }
        let mut workspace = TectonicWorkspace::from_initial(state);
        resample_current_state(&surface, &topology, &mut workspace).unwrap();
        let result = workspace.current.samples[0];
        assert!((result.thickness_km - 7.0).abs() <= 1.0e-5);
        assert!((result.age_myr - 70.0).abs() <= 1.0e-4);
        assert!((result.tectonic_elevation_m - 700.0).abs() <= 1.0e-3);
        assert!((result.lineation[0] - 1.0).abs() <= 1.0e-6);
        assert!(result.lineation[1].abs() <= 1.0e-6);

        let mut unresolved = state_from_owners(&surface, &owners, &lineages);
        unresolved.samples.remove(0);
        let mut workspace = TectonicWorkspace::from_initial(unresolved);
        assert!(matches!(
            resample_current_state(&surface, &topology, &mut workspace),
            Err(ResampleError::UnresolvedCoverageGap { cell }) if cell == CellId::from_raw(0)
        ));
        assert_eq!(workspace.current.samples.len(), surface.cells().len() - 1);

        let (_, wrong_topology) = fixture(162);
        assert!(matches!(
            resample_current_state(&surface, &wrong_topology, &mut workspace),
            Err(ResampleError::CardinalityMismatch { .. })
        ));
    }

    #[test]
    fn resampling_reconstructs_each_active_plate_as_one_evidence_guided_domain() {
        let (surface, topology) = fixture(42);
        let lineages = [LineageId::from_raw(0), LineageId::from_raw(1)];
        let mut owners = surface
            .cells()
            .iter()
            .map(|_| lineages[1])
            .collect::<Vec<_>>();
        owners[0] = lineages[0];
        let remote = surface
            .cells()
            .iter()
            .filter(|cell| {
                cell.id != CellId::from_raw(0)
                    && !topology.arcs()[0].iter().any(|arc| arc.neighbor == cell.id)
            })
            .min_by(|first, second| {
                first
                    .centroid
                    .dot(surface.cells()[0].centroid)
                    .total_cmp(&second.centroid.dot(surface.cells()[0].centroid))
            })
            .unwrap()
            .id;
        owners[remote.raw() as usize] = lineages[0];
        let state = state_from_owners(&surface, &owners, &lineages);
        let mut workspace = TectonicWorkspace::from_initial(state);

        resample_current_state(&surface, &topology, &mut workspace).unwrap();

        for lineage in lineages {
            let owned = workspace
                .current
                .samples
                .iter()
                .filter(|sample| sample.owner == lineage)
                .count();
            assert!(owned > 0);
            let start = workspace
                .current
                .samples
                .iter()
                .position(|sample| sample.owner == lineage)
                .unwrap();
            let mut reached = vec![false; surface.cells().len()];
            let mut pending = vec![start];
            reached[start] = true;
            let mut count = 0;
            while let Some(cell) = pending.pop() {
                count += 1;
                for arc in &topology.arcs()[cell] {
                    let neighbor = arc.neighbor.raw() as usize;
                    if !reached[neighbor] && workspace.current.samples[neighbor].owner == lineage {
                        reached[neighbor] = true;
                        pending.push(neighbor);
                    }
                }
            }
            assert_eq!(count, owned, "lineage {lineage:?} remained fragmented");
        }
    }

    #[test]
    fn resampling_rejects_more_live_lineages_than_authoritative_cells() {
        let (surface, topology) = fixture(42);
        let owners = (0..surface.cells().len() as u32)
            .map(LineageId::from_raw)
            .collect::<Vec<_>>();
        let lineages = (0..=surface.cells().len() as u32)
            .map(LineageId::from_raw)
            .collect::<Vec<_>>();
        let mut state = state_from_owners(&surface, &owners, &lineages);
        state.samples.push(sample(
            CellId::from_raw(0),
            surface.cells()[0].centroid,
            *lineages.last().unwrap(),
            99,
        ));
        let mut workspace = TectonicWorkspace::from_initial(state);

        assert!(matches!(
            resample_current_state(&surface, &topology, &mut workspace),
            Err(ResampleError::DomainMarkerCapacityExceeded {
                lineages: 43,
                cells: 42
            })
        ));
    }

    #[test]
    fn canonicalization_splits_domains_drops_empty_lineages_and_preserves_material_bits() {
        let (surface, topology) = fixture(42);
        let first = LineageId::from_raw(3);
        let second = LineageId::from_raw(9);
        let empty = LineageId::from_raw(17);
        let mut owners = vec![second; surface.cells().len()];
        let origin = CellId::from_raw(0);
        let remote = surface
            .cells()
            .iter()
            .filter(|cell| {
                cell.id != origin
                    && !topology.arcs()[origin.raw() as usize]
                        .iter()
                        .any(|arc| arc.neighbor == cell.id)
            })
            .min_by(|first_cell, second_cell| {
                first_cell
                    .centroid
                    .dot(surface.cells()[0].centroid)
                    .total_cmp(&second_cell.centroid.dot(surface.cells()[0].centroid))
                    .then_with(|| first_cell.id.cmp(&second_cell.id))
            })
            .unwrap()
            .id;
        owners[origin.raw() as usize] = first;
        owners[remote.raw() as usize] = first;
        let state = state_from_owners(&surface, &owners, &[first, second, empty]);
        let before = state.samples.iter().map(material_bits).collect::<Vec<_>>();
        let canonical = canonicalize_final_plates(&surface, state).unwrap();

        assert_eq!(canonical.plates.len(), 3);
        assert_eq!(canonical.cell_plates.len(), surface.cells().len());
        assert_ne!(
            canonical.cell_plates.get(origin.raw() as usize),
            canonical.cell_plates.get(remote.raw() as usize)
        );
        assert_eq!(
            canonical
                .samples
                .iter()
                .map(material_bits)
                .collect::<Vec<_>>(),
            before
        );
        for (index, plate) in canonical.plates.iter().enumerate() {
            assert_eq!(plate.id(), PlateId::from_raw(index as u32));
            assert_eq!(
                canonical.cell_plates.get(plate.seed_cell().raw() as usize),
                Some(plate.id())
            );
        }
        assert!(!canonical
            .plates
            .iter()
            .any(|plate| plate.rotation() == rotation(empty.raw(), 10_000)));
    }

    #[test]
    fn representative_rule_and_final_plate_bounds_are_exact() {
        let (surface, _) = fixture(162);
        for count in [2_usize, 64] {
            let owners = owners_for(&surface, count);
            let lineages = (0..count as u32)
                .map(LineageId::from_raw)
                .collect::<Vec<_>>();
            let state = state_from_owners(&surface, &owners, &lineages);
            let expected_material = state.samples.iter().map(material_bits).collect::<Vec<_>>();
            let canonical = canonicalize_final_plates(&surface, state).unwrap();
            assert_eq!(canonical.plates.len(), count);
            assert_eq!(
                canonical
                    .samples
                    .iter()
                    .map(material_bits)
                    .collect::<Vec<_>>(),
                expected_material
            );
            for plate in &canonical.plates {
                let members = surface
                    .cells()
                    .iter()
                    .filter(|cell| {
                        canonical.cell_plates.get(cell.id.raw() as usize) == Some(plate.id())
                    })
                    .collect::<Vec<_>>();
                let sum = members.iter().fold([0.0; 3], |mut sum, cell| {
                    let area = cell.area.get();
                    for (slot, value) in sum.iter_mut().zip(cell.centroid.components()) {
                        *slot += value * area;
                    }
                    sum
                });
                let norm = sum
                    .into_iter()
                    .map(|value| value * value)
                    .sum::<f64>()
                    .sqrt();
                let expected = if norm <= f64::EPSILON {
                    members.iter().map(|cell| cell.id).min().unwrap()
                } else {
                    let mean = sum.map(|value| value / norm);
                    members
                        .iter()
                        .max_by(|first, second| {
                            first
                                .centroid
                                .components()
                                .into_iter()
                                .zip(mean)
                                .map(|(a, b)| a * b)
                                .sum::<f64>()
                                .total_cmp(
                                    &second
                                        .centroid
                                        .components()
                                        .into_iter()
                                        .zip(mean)
                                        .map(|(a, b)| a * b)
                                        .sum::<f64>(),
                                )
                                .then_with(|| second.id.cmp(&first.id))
                        })
                        .unwrap()
                        .id
                };
                assert_eq!(plate.seed_cell(), expected);
            }
        }

        let one = vec![LineageId::from_raw(0); surface.cells().len()];
        let state = state_from_owners(&surface, &one, &[LineageId::from_raw(0)]);
        assert!(matches!(
            canonicalize_final_plates(&surface, state),
            Err(ResampleError::FinalPlateCountOutOfRange {
                found: 1,
                min: 2,
                max: 64
            })
        ));

        let owners = (0..surface.cells().len())
            .map(|index| LineageId::from_raw((index % 65) as u32))
            .collect::<Vec<_>>();
        let lineages = (0..65_u32).map(LineageId::from_raw).collect::<Vec<_>>();
        let state = state_from_owners(&surface, &owners, &lineages);
        assert!(matches!(
            canonicalize_final_plates(&surface, state),
            Err(ResampleError::FinalPlateCountOutOfRange { found, min: 2, max: 64 }) if found > 64
        ));
    }

    /// G1e R3: when the clamping loop fixes every cell (some at the maximum,
    /// some at the minimum), the stranded residual must move to same-kind
    /// cells outside the group instead of being dropped (oceanic groups used
    /// to leak it — the seed 8 / seed 1 closure failures).
    #[test]
    fn oceanic_rebalance_residual_moves_to_receivers_instead_of_leaking() {
        let (surface, topology) = fixture(42);
        let cell_count = surface.cells().len();
        assert!(cell_count >= 4);
        let plate_a = LineageId::from_raw(0);
        let plate_b = LineageId::from_raw(1);
        let build = |owners: &dyn Fn(usize) -> LineageId| {
            let mut winner = Vec::with_capacity(cell_count);
            let remapped = surface
                .cells()
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let mut value = sample(cell.id, cell.centroid, owners(index), 1);
                    value.kind = CrustKind::Oceanic;
                    let area = cell.area.get();
                    // Group cells carry a feasible 10 km mean, but the winner
                    // hints (advected thickness) push one cell above the
                    // 15 km bound and the next below the 3 km bound, so the
                    // scale loop fixes both and strands part of the volume.
                    let (thickness, hint) = if owners(index) == plate_a {
                        (10.0, if index % 2 == 0 { 29.0 } else { 2.0 })
                    } else {
                        (9.0, 9.0)
                    };
                    value.thickness_km = thickness;
                    value.age_myr = 40.0;
                    value.material =
                        MaterialColumn::pure(CrustKind::Oceanic, area, thickness).unwrap();
                    winner.push(Some(hint));
                    value
                })
                .collect::<Vec<_>>();
            (remapped, winner)
        };
        let volume = |samples: &[CrustSample]| {
            samples
                .iter()
                .map(|value| value.material.oceanic_volume_m3())
                .sum::<f64>()
        };

        // Plate A on two cells, everyone else has room: the residual must land
        // on the other plate's cells and the total must close exactly.
        let owners = |index: usize| if index < 2 { plate_a } else { plate_b };
        let (mut remapped, winner) = build(&owners);
        let kinds = vec![CrustKind::Oceanic; cell_count];
        let before = volume(&remapped);
        let receivers_before = volume(&remapped[2..]);
        let plates = vec![
            ActivePlate::new(plate_a, CellId::from_raw(0), rotation(0, 10_000)),
            ActivePlate::new(plate_b, CellId::from_raw(2), rotation(1, 10_000)),
        ];
        let state = TectonicState::new(remapped.clone(), plates.clone(), 2).unwrap();
        let mut ledger = EvolutionMaterialLedger::capture_initial(&state).unwrap();
        let mut kinds_first = kinds.clone();
        rebalance_columns_to_cells(
            &surface,
            &topology,
            &mut remapped,
            &mut kinds_first,
            &winner,
            &mut ledger,
        )
        .unwrap();
        let after = volume(&remapped);
        assert!(
            (after - before).abs() <= 1.0e-9 * before,
            "oceanic volume is conserved: before {before} after {after}"
        );
        assert!(
            volume(&remapped[2..]) > receivers_before,
            "the stranded residual reaches cells outside the group"
        );
        for value in &remapped {
            let area = value.material.oceanic_reference_area_m2();
            let thickness = value.material.oceanic_volume_m3() / area / 1_000.0;
            assert!((3.0..=15.0).contains(&thickness), "thickness {thickness}");
        }

        // Every cell in one group: nothing can absorb the residual, which
        // must surface as an error, never as silent loss.
        let owners = |_: usize| plate_a;
        let (mut remapped, winner) = build(&owners);
        let state = TectonicState::new(
            remapped.clone(),
            vec![ActivePlate::new(
                plate_a,
                CellId::from_raw(0),
                rotation(0, 10_000),
            )],
            1,
        )
        .unwrap();
        let mut ledger = EvolutionMaterialLedger::capture_initial(&state).unwrap();
        let mut kinds_second = kinds;
        let result = rebalance_columns_to_cells(
            &surface,
            &topology,
            &mut remapped,
            &mut kinds_second,
            &winner,
            &mut ledger,
        );
        assert!(matches!(
            result,
            Err(ResampleError::UnplacedRebalanceResidual {
                kind: CrustKind::Oceanic,
                ..
            })
        ));
    }
}
