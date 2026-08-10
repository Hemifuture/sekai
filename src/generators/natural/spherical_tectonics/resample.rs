//! Deterministic moving-crust resampling and final plate canonicalization.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::VecDeque;

use thiserror::Error;

use super::contacts::ContactError;
use super::model::{CrustSample, LineageId, TectonicState, TectonicWorkspace};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{
    CrustKind, PlateIdField, SphericalOrogenyKind, SphericalPlate, MAX_PLATE_COUNT,
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
    #[error("final active plate count {found} is outside {min}..={max}")]
    FinalPlateCountOutOfRange {
        found: usize,
        min: usize,
        max: usize,
    },
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

    std::mem::swap(&mut workspace.current, &mut workspace.next);
    workspace.next.samples.clear();
    workspace.next.plates.clear();
    workspace.events.clear();
    Ok(())
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
    let winner = samples[winner_index];

    local_candidates.clear();
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
    local_candidates.sort_by(|&first, &second| {
        samples[second]
            .position
            .dot(target)
            .total_cmp(&samples[first].position.dot(target))
            .then_with(|| first.cmp(&second))
    });
    local_candidates.truncate(MAXIMUM_TRIANGLE_CANDIDATES);

    let mut result =
        if let Some((indices, weights)) = containing_triangle(target, samples, local_candidates) {
            interpolate_material(target, winner, samples, indices, weights)
        } else {
            winner
        };
    result.position = target;
    result.anchor = cell;
    Ok(result)
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
        let sample = &samples[index];
        let (east, north) = canonical_east_north_basis(sample.position);
        for axis in 0..3 {
            global[axis] += weight
                * (east[axis] * f64::from(sample.lineation[0])
                    + north[axis] * f64::from(sample.lineation[1]));
        }
    }
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
        canonicalize_final_plates, resample_current_state, resampling_interval_steps, ResampleError,
    };
    use crate::generators::natural::spherical_tectonics::model::{
        ActivePlate, CrustSample, LineageId, TectonicState, TectonicWorkspace,
    };
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
        let unique = state_from_owners(&surface, &unique_owners, &unique_lineages);
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
}
