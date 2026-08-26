//! Read-only coverage bucketing and spherical contact classification.
//!
//! Following the moving-sample coverage pass in Cortial et al., authoritative
//! cells are compressed into stable buckets: empty buckets are spreading gaps,
//! multiply covered buckets are overlap candidates, and cross-lineage edge
//! neighbors are classified from local relative rigid velocity. The engineering
//! adaptation keeps the low-resolution control mesh fixed and uses its exact
//! tangent frames. This module never changes crust material, ownership or height.

#![cfg_attr(not(test), allow(dead_code))]

use thiserror::Error;

use super::kinematics::{rigid_velocity, KinematicsError};
use super::model::{CrustSample, LineageId, TectonicState};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::CrustKind;
use crate::world::spatial::{SphericalSurfaceSnapshot, UnitVector3};
use crate::world::{CellId, EdgeId, Meters};

const MINIMUM_ACTIVE_RELATIVE_SPEED_MM_PER_YEAR: f64 = 8.0;
const STRONG_NORMAL_FRACTION: f64 = 0.4;

#[derive(Debug, Default)]
pub(super) struct CoverageScratch {
    counts: Vec<u32>,
    offsets: Vec<u32>,
    sample_indices: Vec<u32>,
}

impl CoverageScratch {
    pub(super) fn with_cell_capacity(cell_count: usize) -> Self {
        Self {
            counts: Vec::with_capacity(cell_count),
            offsets: Vec::with_capacity(cell_count.saturating_add(1)),
            sample_indices: Vec::with_capacity(cell_count),
        }
    }

    pub(super) fn rebuild(
        &mut self,
        cell_count: usize,
        samples: &[CrustSample],
    ) -> Result<(), ContactError> {
        self.counts.clear();
        self.counts.resize(cell_count, 0);
        for (sample_index, sample) in samples.iter().enumerate() {
            let anchor = sample.anchor.raw() as usize;
            if anchor >= cell_count {
                return Err(ContactError::InvalidAnchor {
                    sample: sample_index,
                    anchor: sample.anchor,
                    cell_count,
                });
            }
            self.counts[anchor] =
                self.counts[anchor]
                    .checked_add(1)
                    .ok_or(ContactError::CoverageCountOverflow {
                        cell: sample.anchor,
                    })?;
        }

        self.offsets.clear();
        self.offsets.reserve(cell_count.saturating_add(1));
        self.offsets.push(0);
        for &count in &self.counts {
            let next = self
                .offsets
                .last()
                .copied()
                .expect("coverage offsets always contain the zero prefix")
                .checked_add(count)
                .ok_or(ContactError::CoverageOffsetOverflow)?;
            self.offsets.push(next);
        }

        self.sample_indices.clear();
        self.sample_indices.resize(samples.len(), 0);
        self.counts.fill(0);
        for (sample_index, sample) in samples.iter().enumerate() {
            let anchor = sample.anchor.raw() as usize;
            let slot = self.offsets[anchor] + self.counts[anchor];
            self.sample_indices[slot as usize] =
                u32::try_from(sample_index).map_err(|_| ContactError::SampleIndexOverflow {
                    sample: sample_index,
                })?;
            self.counts[anchor] += 1;
        }
        Ok(())
    }

    pub(super) fn count(&self, cell: CellId) -> u32 {
        self.counts.get(cell.raw() as usize).copied().unwrap_or(0)
    }

    pub(super) fn sample_indices(&self, cell: CellId) -> &[u32] {
        let index = cell.raw() as usize;
        let Some(&start) = self.offsets.get(index) else {
            return &[];
        };
        let Some(&end) = self.offsets.get(index + 1) else {
            return &[];
        };
        &self.sample_indices[start as usize..end as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContactKind {
    Gap,
    Transform,
    Divergence,
    OceanicSubduction { descending: LineageId },
    ContinentalCollision,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ContactEvent {
    pub(super) cell: CellId,
    pub(super) edge: Option<EdgeId>,
    pub(super) sample_indices: [Option<u32>; 2],
    pub(super) lineages: [Option<LineageId>; 2],
    pub(super) kind: ContactKind,
    pub(super) signed_normal_speed_mm_per_year: f32,
    pub(super) tangent_speed_mm_per_year: f32,
    pub(super) overlap_depth: u32,
}

type ContactStableKey = (
    CellId,
    Option<EdgeId>,
    Option<LineageId>,
    Option<LineageId>,
    Option<u32>,
    Option<u32>,
);

impl ContactEvent {
    pub(super) fn stable_key(&self) -> ContactStableKey {
        let owners = normalized_optional_pair(self.lineages);
        (
            self.cell,
            self.edge,
            owners[0],
            owners[1],
            self.sample_indices[0],
            self.sample_indices[1],
        )
    }

    fn boundary_identity(&self) -> Option<(CellId, EdgeId, [Option<LineageId>; 2])> {
        self.edge
            .map(|edge| (self.cell, edge, normalized_optional_pair(self.lineages)))
    }
}

pub(super) fn build_contacts(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    moved: &TectonicState,
    coverage: &mut CoverageScratch,
    events: &mut Vec<ContactEvent>,
) -> Result<(), ContactError> {
    if topology.cell_count() != surface.cells().len() {
        return Err(ContactError::CardinalityMismatch {
            surface_cells: surface.cells().len(),
            topology_cells: topology.cell_count(),
        });
    }
    coverage.rebuild(surface.cells().len(), &moved.samples)?;
    events.clear();

    for cell in surface.cells() {
        let count = coverage.count(cell.id);
        if count == 0 {
            events.push(ContactEvent {
                cell: cell.id,
                edge: None,
                sample_indices: [None, None],
                lineages: [None, None],
                kind: ContactKind::Gap,
                signed_normal_speed_mm_per_year: 0.0,
                tangent_speed_mm_per_year: 0.0,
                overlap_depth: 0,
            });
            continue;
        }
        if count > 1 {
            append_overlap_contacts(surface, moved, coverage, cell.id, events)?;
        }
    }

    let mut first_representatives = Vec::with_capacity(moved.plates.len());
    let mut second_representatives = Vec::with_capacity(moved.plates.len());
    for edge in surface.edges() {
        unique_owner_representatives(
            coverage.sample_indices(edge.cells[0]),
            moved,
            &mut first_representatives,
        );
        unique_owner_representatives(
            coverage.sample_indices(edge.cells[1]),
            moved,
            &mut second_representatives,
        );
        for &first_index in &first_representatives {
            for &second_index in &second_representatives {
                let first = &moved.samples[first_index as usize];
                let second = &moved.samples[second_index as usize];
                if first.owner == second.owner {
                    continue;
                }
                let relative =
                    relative_velocity(moved, first, second, surface.radius(), edge.midpoint)?;
                let normal = edge.normal_from_first.components();
                let signed_normal_speed = dot(relative, normal);
                let speed = norm(relative);
                let tangent_speed = (speed * speed - signed_normal_speed * signed_normal_speed)
                    .max(0.0)
                    .sqrt();
                if let Some(kind) = classify_pair(first, second, signed_normal_speed, tangent_speed)
                {
                    events.push(ContactEvent {
                        cell: edge.cells[0],
                        edge: Some(edge.id),
                        sample_indices: [Some(first_index), Some(second_index)],
                        lineages: [Some(first.owner), Some(second.owner)],
                        kind,
                        signed_normal_speed_mm_per_year: signed_normal_speed as f32,
                        tangent_speed_mm_per_year: tangent_speed as f32,
                        overlap_depth: 0,
                    });
                }
            }
        }
    }

    events.sort_by_key(ContactEvent::stable_key);
    events.dedup_by(|later, earlier| {
        earlier.boundary_identity().is_some()
            && earlier.boundary_identity() == later.boundary_identity()
    });
    Ok(())
}

fn append_overlap_contacts(
    surface: &SphericalSurfaceSnapshot,
    moved: &TectonicState,
    coverage: &CoverageScratch,
    cell: CellId,
    events: &mut Vec<ContactEvent>,
) -> Result<(), ContactError> {
    let mut representatives = Vec::with_capacity(moved.plates.len());
    unique_owner_representatives(coverage.sample_indices(cell), moved, &mut representatives);
    let radial = surface
        .cell(cell)
        .ok_or(ContactError::UnknownCell { cell })?
        .centroid;
    for first_position in 0..representatives.len() {
        for second_position in first_position + 1..representatives.len() {
            let first_index = representatives[first_position];
            let second_index = representatives[second_position];
            let first = &moved.samples[first_index as usize];
            let second = &moved.samples[second_index as usize];
            let relative = relative_velocity(moved, first, second, surface.radius(), radial)?;
            let speed = norm(relative);
            if let Some(kind) = classify_pair(first, second, -speed, 0.0) {
                events.push(ContactEvent {
                    cell,
                    edge: None,
                    sample_indices: [Some(first_index), Some(second_index)],
                    lineages: [Some(first.owner), Some(second.owner)],
                    kind,
                    signed_normal_speed_mm_per_year: -(speed as f32),
                    tangent_speed_mm_per_year: 0.0,
                    overlap_depth: coverage.count(cell) - 1,
                });
            }
        }
    }
    Ok(())
}

fn unique_owner_representatives(
    sample_indices: &[u32],
    moved: &TectonicState,
    representatives: &mut Vec<u32>,
) {
    representatives.clear();
    for &sample_index in sample_indices {
        let owner = moved.samples[sample_index as usize].owner;
        if representatives
            .iter()
            .all(|&existing| moved.samples[existing as usize].owner != owner)
        {
            representatives.push(sample_index);
        }
    }
}

pub(super) fn classify_pair(
    first: &CrustSample,
    second: &CrustSample,
    signed_normal_speed_mm_per_year: f64,
    tangent_speed_mm_per_year: f64,
) -> Option<ContactKind> {
    if first.owner == second.owner {
        return None;
    }
    let normal_speed = signed_normal_speed_mm_per_year.abs();
    let speed = normal_speed.hypot(tangent_speed_mm_per_year);
    if speed < MINIMUM_ACTIVE_RELATIVE_SPEED_MM_PER_YEAR {
        return None;
    }
    if normal_speed < speed * STRONG_NORMAL_FRACTION {
        return Some(ContactKind::Transform);
    }
    if signed_normal_speed_mm_per_year > 0.0 {
        return Some(ContactKind::Divergence);
    }

    match [first.kind, second.kind] {
        [CrustKind::Continental, CrustKind::Continental] => Some(ContactKind::ContinentalCollision),
        [CrustKind::Oceanic, CrustKind::Continental] => Some(ContactKind::OceanicSubduction {
            descending: first.owner,
        }),
        [CrustKind::Continental, CrustKind::Oceanic] => Some(ContactKind::OceanicSubduction {
            descending: second.owner,
        }),
        [CrustKind::Oceanic, CrustKind::Oceanic] => Some(ContactKind::OceanicSubduction {
            descending: older_oceanic_side(first, second),
        }),
    }
}

fn older_oceanic_side(first: &CrustSample, second: &CrustSample) -> LineageId {
    if first.age_myr > second.age_myr {
        first.owner
    } else if second.age_myr > first.age_myr {
        second.owner
    } else if first.thickness_km < second.thickness_km {
        first.owner
    } else if second.thickness_km < first.thickness_km {
        second.owner
    } else {
        first.owner.min(second.owner)
    }
}

fn relative_velocity(
    state: &TectonicState,
    first: &CrustSample,
    second: &CrustSample,
    radius: Meters,
    radial: UnitVector3,
) -> Result<[f64; 3], ContactError> {
    let first_rotation = state
        .plate(first.owner)
        .ok_or(ContactError::UnknownLineage {
            lineage: first.owner,
        })?
        .rotation;
    let second_rotation = state
        .plate(second.owner)
        .ok_or(ContactError::UnknownLineage {
            lineage: second.owner,
        })?
        .rotation;
    let first_velocity = rigid_velocity(first_rotation, radius, radial)?;
    let second_velocity = rigid_velocity(second_rotation, radius, radial)?;
    Ok([
        second_velocity[0] - first_velocity[0],
        second_velocity[1] - first_velocity[1],
        second_velocity[2] - first_velocity[2],
    ])
}

fn normalized_optional_pair(mut pair: [Option<LineageId>; 2]) -> [Option<LineageId>; 2] {
    if pair[1] < pair[0] {
        pair.swap(0, 1);
    }
    pair
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first.into_iter().zip(second).map(|(a, b)| a * b).sum()
}

fn norm(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

#[derive(Debug, Clone, PartialEq, Error)]
pub(super) enum ContactError {
    #[error("surface has {surface_cells} cells but topology has {topology_cells}")]
    CardinalityMismatch {
        surface_cells: usize,
        topology_cells: usize,
    },
    #[error("sample {sample} anchor {anchor:?} is outside {cell_count} cells")]
    InvalidAnchor {
        sample: usize,
        anchor: CellId,
        cell_count: usize,
    },
    #[error("coverage count overflowed for {cell:?}")]
    CoverageCountOverflow { cell: CellId },
    #[error("coverage prefix sum overflowed")]
    CoverageOffsetOverflow,
    #[error("sample index {sample} does not fit the compressed coverage layout")]
    SampleIndexOverflow { sample: usize },
    #[error("cell {cell:?} is outside the authoritative surface")]
    UnknownCell { cell: CellId },
    #[error("contact references missing lineage {lineage:?}")]
    UnknownLineage { lineage: LineageId },
    #[error("contact kinematics failed: {0}")]
    Kinematics(#[from] KinematicsError),
}

#[cfg(test)]
mod tests {
    use super::{build_contacts, classify_pair, ContactKind, CoverageScratch};
    use crate::generators::natural::foundation::tectonics::model::{
        ActivePlate, CrustSample, LineageId, MaterialColumn, TectonicState,
    };
    use crate::generators::natural::topology::NaturalTopologyIndex;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::world::natural::{
        CrustKind, SphericalOrogenyKind, SphericalPlateRotation,
        CONTINENTAL_CRUST_AGE_SENTINEL_MYR, NO_OROGENY_AGE_SENTINEL_MYR,
    };
    use crate::world::spatial::{SphericalNaturalSurface, SphericalSurfaceSnapshot, UnitVector3};
    use crate::world::{CellId, Meters, SphericalSpaceSpec};

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

    fn sample(owner: LineageId, kind: CrustKind, age_myr: f32) -> CrustSample {
        CrustSample {
            position: UnitVector3::new(1.0, 0.0, 0.0).unwrap(),
            anchor: CellId::from_raw(0),
            owner,
            kind,
            thickness_km: match kind {
                CrustKind::Continental => 38.0,
                CrustKind::Oceanic => 7.0,
            },
            age_myr: match kind {
                CrustKind::Continental => CONTINENTAL_CRUST_AGE_SENTINEL_MYR,
                CrustKind::Oceanic => age_myr,
            },
            tectonic_elevation_m: 0.0,
            lineation: [0.0; 2],
            orogeny: SphericalOrogenyKind::None,
            orogeny_age_myr: NO_OROGENY_AGE_SENTINEL_MYR,
            material: MaterialColumn::pure(
                kind,
                1.0,
                match kind {
                    CrustKind::Continental => 38.0,
                    CrustKind::Oceanic => 7.0,
                },
            )
            .unwrap(),
        }
    }

    fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
        [
            first[1] * second[2] - first[2] * second[1],
            first[2] * second[0] - first[0] * second[2],
            first[0] * second[1] - first[1] * second[0],
        ]
    }

    fn pole_for_velocity(radial: UnitVector3, velocity: [f64; 3]) -> UnitVector3 {
        let pole = cross(radial.components(), velocity);
        UnitVector3::new(pole[0], pole[1], pole[2]).unwrap()
    }

    fn two_lineage_state(
        surface: &SphericalSurfaceSnapshot,
        first_kind: CrustKind,
        first_age: f32,
        second_kind: CrustKind,
        second_age: f32,
        motion: Motion,
    ) -> (TectonicState, crate::world::EdgeId) {
        let edge = &surface.edges()[0];
        let normal = edge.normal_from_first.components();
        let tangent = cross(edge.midpoint.components(), normal);
        let (first_velocity, second_velocity) = match motion {
            Motion::Converging => (normal, normal.map(|value| -value)),
            Motion::Diverging => (normal.map(|value| -value), normal),
            Motion::Transform => (tangent.map(|value| -value), tangent),
        };
        let first = LineageId::from_raw(0);
        let second = LineageId::from_raw(1);
        let rotations = [
            SphericalPlateRotation::new(pole_for_velocity(edge.midpoint, first_velocity), 10_000)
                .unwrap(),
            SphericalPlateRotation::new(pole_for_velocity(edge.midpoint, second_velocity), 10_000)
                .unwrap(),
        ];
        let samples = surface
            .cells()
            .iter()
            .map(|cell| {
                let (owner, kind, age) = if cell.id == edge.cells[1] {
                    (second, second_kind, second_age)
                } else {
                    (first, first_kind, first_age)
                };
                let mut value = sample(owner, kind, age);
                value.position = cell.site;
                value.anchor = cell.id;
                value
            })
            .collect();
        let plates = vec![
            ActivePlate::new(first, edge.cells[0], rotations[0]),
            ActivePlate::new(second, edge.cells[1], rotations[1]),
        ];
        (TectonicState::new(samples, plates, 2).unwrap(), edge.id)
    }

    #[derive(Clone, Copy)]
    enum Motion {
        Converging,
        Diverging,
        Transform,
    }

    #[test]
    fn material_and_motion_classification_selects_the_physical_side() {
        let first = LineageId::from_raw(4);
        let second = LineageId::from_raw(9);
        let young_ocean = sample(first, CrustKind::Oceanic, 20.0);
        let old_ocean = sample(second, CrustKind::Oceanic, 140.0);
        let continent = sample(second, CrustKind::Continental, 0.0);
        let other_continent = sample(first, CrustKind::Continental, 0.0);

        assert_eq!(
            classify_pair(&young_ocean, &continent, -48.0, 2.0),
            Some(ContactKind::OceanicSubduction { descending: first })
        );
        assert_eq!(
            classify_pair(&young_ocean, &old_ocean, -48.0, 2.0),
            Some(ContactKind::OceanicSubduction { descending: second })
        );
        assert_eq!(
            classify_pair(&other_continent, &continent, -48.0, 2.0),
            Some(ContactKind::ContinentalCollision)
        );
        assert_eq!(
            classify_pair(&young_ocean, &old_ocean, 48.0, 2.0),
            Some(ContactKind::Divergence)
        );
        assert_eq!(
            classify_pair(&young_ocean, &old_ocean, 2.0, 48.0),
            Some(ContactKind::Transform)
        );
        let same_owner = sample(first, CrustKind::Continental, 0.0);
        assert_eq!(classify_pair(&young_ocean, &same_owner, -48.0, 2.0), None);
    }

    #[test]
    fn compressed_coverage_reports_gaps_and_ignores_same_owner_overlap() {
        let (surface, topology) = fixture();
        let first = LineageId::from_raw(0);
        let rotation =
            SphericalPlateRotation::new(UnitVector3::new(0.0, 0.0, 1.0).unwrap(), 10_000).unwrap();
        let samples = surface
            .cells()
            .iter()
            .map(|cell| {
                let mut value = sample(first, CrustKind::Continental, 0.0);
                value.position = cell.site;
                value.anchor = cell.id;
                value
            })
            .collect();
        let mut state = TectonicState::new(
            samples,
            vec![ActivePlate::new(first, CellId::from_raw(0), rotation)],
            1,
        )
        .unwrap();
        state.samples[0].anchor = CellId::from_raw(1);
        let before = state.samples.clone();
        let mut coverage = CoverageScratch::default();
        let mut events = Vec::new();
        build_contacts(&surface, &topology, &state, &mut coverage, &mut events).unwrap();

        assert_eq!(coverage.count(CellId::from_raw(0)), 0);
        assert_eq!(coverage.count(CellId::from_raw(1)), 2);
        assert_eq!(coverage.sample_indices(CellId::from_raw(1)), &[0, 1]);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == ContactKind::Gap)
                .count(),
            1
        );
        assert_eq!(events[0].cell, CellId::from_raw(0));
        assert!(events.iter().all(|event| event.kind == ContactKind::Gap));
        assert_eq!(state.samples, before);
    }

    #[test]
    fn real_edge_frames_drive_signed_events_and_order_is_stable() {
        let (surface, topology) = fixture();
        for (motion, expected_kind, expects_negative) in [
            (
                Motion::Converging,
                ContactKind::OceanicSubduction {
                    descending: LineageId::from_raw(0),
                },
                true,
            ),
            (Motion::Diverging, ContactKind::Divergence, false),
            (Motion::Transform, ContactKind::Transform, false),
        ] {
            let (state, target_edge) = two_lineage_state(
                &surface,
                CrustKind::Oceanic,
                80.0,
                CrustKind::Continental,
                0.0,
                motion,
            );
            let mut coverage = CoverageScratch::default();
            let mut events = Vec::new();
            build_contacts(&surface, &topology, &state, &mut coverage, &mut events).unwrap();
            let event = events
                .iter()
                .find(|event| event.edge == Some(target_edge))
                .unwrap();
            assert_eq!(event.kind, expected_kind);
            if expects_negative {
                assert!(event.signed_normal_speed_mm_per_year < 0.0);
            } else if motion as u8 == Motion::Diverging as u8 {
                assert!(event.signed_normal_speed_mm_per_year > 0.0);
            } else {
                assert!(
                    event.tangent_speed_mm_per_year > event.signed_normal_speed_mm_per_year.abs()
                );
            }
            assert!(events
                .windows(2)
                .all(|pair| pair[0].stable_key() <= pair[1].stable_key()));

            let first = events.clone();
            build_contacts(&surface, &topology, &state, &mut coverage, &mut events).unwrap();
            assert_eq!(events, first);
        }
    }

    #[test]
    fn different_owner_overlap_is_a_contact_candidate_with_membership() {
        let (surface, topology) = fixture();
        let (mut state, target_edge) = two_lineage_state(
            &surface,
            CrustKind::Oceanic,
            30.0,
            CrustKind::Continental,
            0.0,
            Motion::Converging,
        );
        let edge = surface.edge(target_edge).unwrap();
        let destination = edge.cells[0];
        state.samples[edge.cells[1].raw() as usize].anchor = destination;
        let mut coverage = CoverageScratch::default();
        let mut events = Vec::new();
        build_contacts(&surface, &topology, &state, &mut coverage, &mut events).unwrap();

        assert_eq!(coverage.count(destination), 2);
        let overlap = events
            .iter()
            .find(|event| {
                event.cell == destination && event.edge.is_none() && event.kind != ContactKind::Gap
            })
            .unwrap();
        assert_eq!(
            overlap.kind,
            ContactKind::OceanicSubduction {
                descending: LineageId::from_raw(0)
            }
        );
        assert_eq!(overlap.overlap_depth, 1);
        assert_eq!(overlap.sample_indices.iter().flatten().count(), 2);
        assert!(events
            .iter()
            .any(|event| { event.cell == edge.cells[1] && event.kind == ContactKind::Gap }));
    }

    #[test]
    fn contact_module_is_read_only_and_has_no_noise_dependency() {
        let source = include_str!("contacts.rs");
        let forbidden = [
            ["sample.", "kind ="].concat(),
            ["sample.", "owner ="].concat(),
            ["sample.", "thickness_km ="].concat(),
            ["sample.", "tectonic_elevation_m ="].concat(),
            ["morphology::", "noise"].concat(),
            ["Spherical", "Noise"].concat(),
        ];
        for forbidden in forbidden {
            assert!(
                !source.contains(&forbidden),
                "found forbidden source fragment {forbidden}"
            );
        }
    }
}
