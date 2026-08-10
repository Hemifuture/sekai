use std::collections::BTreeMap;

use super::crust::CrustMorphology;
use crate::generators::natural::connectivity::{normalized_plate_pair, StableUnionFind};
use crate::generators::natural::topology::NaturalTopologyIndex;
use crate::world::natural::{
    classify_spherical_boundary_kinematics, BoundaryKind, BoundaryRecord, PlateIdField,
    SphericalBoundarySegment, SphericalPlate,
};
use crate::world::spatial::SphericalSurfaceSnapshot;
use crate::world::{BoundarySegmentId, EdgeId, PlateId, SurfaceVertexId};

#[derive(Debug, Clone)]
struct BoundaryEventDraft {
    edge: EdgeId,
    vertices: [SurfaceVertexId; 2],
    plates: [PlateId; 2],
    kind: BoundaryKind,
    strength: f32,
    subducting_plate: Option<PlateId>,
}

pub(super) fn classify_and_aggregate_boundaries(
    surface: &SphericalSurfaceSnapshot,
    topology: &NaturalTopologyIndex,
    plates: &[SphericalPlate],
    owners: &PlateIdField,
    crust: &CrustMorphology,
) -> (Vec<BoundaryRecord>, Vec<SphericalBoundarySegment>) {
    debug_assert_eq!(topology.cell_count(), surface.cells().len());
    debug_assert_eq!(topology.edge_count(), surface.edges().len());
    let mut events = Vec::new();
    for edge in surface.edges() {
        let owner_plates = edge.cells.map(|cell| {
            owners
                .get(cell.raw() as usize)
                .expect("plate field is aligned with the validated surface")
        });
        if owner_plates[0] == owner_plates[1] {
            continue;
        }
        let indices = edge.cells.map(|cell| cell.raw() as usize);
        let classification = classify_spherical_boundary_kinematics(
            owner_plates,
            owner_plates.map(|plate| plates[plate.raw() as usize].rotation()),
            surface.radius(),
            edge,
            indices.map(|index| {
                crust
                    .kinds
                    .get(index)
                    .expect("crust field is aligned with the validated surface")
            }),
            indices.map(|index| crust.thickness_km[index]),
        )
        .expect("generated plate rotations are valid for the authoritative sphere");
        events.push(BoundaryEventDraft {
            edge: edge.id,
            vertices: edge.vertices,
            plates: normalized_plate_pair(owner_plates[0], owner_plates[1]),
            kind: classification.kind,
            strength: classification.strength,
            subducting_plate: classification.subducting_plate,
        });
    }
    aggregate_boundary_events(surface.edges().len(), &events)
}

fn aggregate_boundary_events(
    edge_count: usize,
    events: &[BoundaryEventDraft],
) -> (Vec<BoundaryRecord>, Vec<SphericalBoundarySegment>) {
    let mut vertex_members = BTreeMap::<SurfaceVertexId, Vec<usize>>::new();
    for (index, event) in events.iter().enumerate() {
        vertex_members
            .entry(event.vertices[0])
            .or_default()
            .push(index);
        vertex_members
            .entry(event.vertices[1])
            .or_default()
            .push(index);
    }
    let mut union = StableUnionFind::new(events.len());
    for members in vertex_members.values() {
        for first_index in 0..members.len() {
            for second_index in (first_index + 1)..members.len() {
                let first = members[first_index];
                let second = members[second_index];
                if boundary_events_are_compatible(&events[first], &events[second]) {
                    union.union(first, second);
                }
            }
        }
    }

    let mut by_root = BTreeMap::<usize, Vec<usize>>::new();
    for index in 0..events.len() {
        let root = union.find(index);
        by_root.entry(root).or_default().push(index);
    }
    let mut components = by_root.into_values().collect::<Vec<_>>();
    for component in &mut components {
        component.sort_by_key(|&index| events[index].edge);
    }
    components.sort_by_key(|component| events[component[0]].edge);

    let mut boundaries = vec![BoundaryRecord::none(); edge_count];
    let mut segments = Vec::with_capacity(components.len());
    for (segment_index, component) in components.into_iter().enumerate() {
        let segment_id = BoundarySegmentId::from_raw(segment_index as u32);
        let first_event = &events[component[0]];
        let member_edges = component
            .iter()
            .map(|&index| events[index].edge)
            .collect::<Vec<_>>();
        let mean_strength = component
            .iter()
            .map(|&index| events[index].strength)
            .sum::<f32>()
            / component.len() as f32;
        for &index in &component {
            let event = &events[index];
            boundaries[event.edge.raw() as usize] = BoundaryRecord::new(
                event.kind,
                event.strength,
                Some(segment_id),
                event.subducting_plate,
            );
        }
        segments.push(SphericalBoundarySegment::new(
            segment_id,
            first_event.plates,
            first_event.kind,
            member_edges,
            mean_strength,
            first_event.subducting_plate,
        ));
    }
    (boundaries, segments)
}

fn boundary_events_are_compatible(first: &BoundaryEventDraft, second: &BoundaryEventDraft) -> bool {
    first.plates == second.plates
        && first.kind == second.kind
        && first.subducting_plate == second.subducting_plate
}
