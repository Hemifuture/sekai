use thiserror::Error;

use super::sphere_geometry::{add, cross, dot, norm, subtract};
use super::{
    central_angle, oriented_arc_normal, spherical_triangle_area_unit, SphericalSurfaceSnapshot,
    UnitVector3, SPHERICAL_SURFACE_SCHEMA_V1,
};
use crate::world::{
    CellId, EdgeId, SurfaceVertexId, UnitError, MAX_SPHERICAL_CELL_BOUNDARY_DEGREE,
    MAX_SPHERICAL_CELL_COUNT, MAX_SPHERICAL_EDGE_COUNT, MAX_SPHERICAL_VERTEX_COUNT,
};

const UNIT_TOLERANCE: f64 = 1.0e-12;
const VECTOR_ANGLE_TOLERANCE: f64 = 1.0e-10;
const METRIC_RELATIVE_TOLERANCE: f64 = 1.0e-10;
const AREA_RELATIVE_TOLERANCE: f64 = 1.0e-10;
const ABSOLUTE_SCALE_ULPS: f64 = 16.0;

/// Stable failures for malformed or scientifically inconsistent spherical snapshots.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalSurfaceValidationError {
    /// The snapshot uses a schema version that this engine does not support.
    #[error(
        "unsupported spherical surface schema version {found}; supported version is {supported}"
    )]
    UnsupportedSchema { found: u16, supported: u16 },
    /// The snapshot radius is not finite and strictly positive.
    #[error("spherical surface radius must be finite and positive, got {found}")]
    InvalidRadius { found: f64 },
    /// A top-level record vector exceeds the schema V1 allocation budget.
    #[error("spherical surface {record} count {found} exceeds schema V1 maximum {max}")]
    RecordCountOutOfRange {
        record: &'static str,
        found: usize,
        max: usize,
    },
    /// A vertex ID does not equal its canonical vector position.
    #[error("vertex at position {position} has non-contiguous ID {found:?}")]
    NonContiguousVertexId {
        position: usize,
        found: SurfaceVertexId,
    },
    /// A cell ID does not equal its canonical vector position.
    #[error("cell at position {position} has non-contiguous ID {found:?}")]
    NonContiguousCellId { position: usize, found: CellId },
    /// An edge ID does not equal its canonical vector position.
    #[error("edge at position {position} has non-contiguous ID {found:?}")]
    NonContiguousEdgeId { position: usize, found: EdgeId },
    /// A direction is not a finite unit vector.
    #[error("{record} {id} field {field} is not a finite unit vector")]
    InvalidUnitVector {
        record: &'static str,
        id: u32,
        field: &'static str,
    },
    /// A cell cannot form a spherical polygon.
    #[error("cell {cell:?} boundary has only {vertices} vertices")]
    CellBoundaryTooSmall { cell: CellId, vertices: usize },
    /// A cell boundary exceeds the schema V1 geodesic-dual degree budget.
    #[error("cell {cell:?} boundary degree {found} exceeds schema V1 maximum {max}")]
    CellBoundaryDegreeOutOfRange {
        cell: CellId,
        found: usize,
        max: usize,
    },
    /// A cell's cyclic vertex and edge lists have different lengths.
    #[error("cell {cell:?} has {vertices} boundary vertices but {edges} boundary edges")]
    CellBoundaryLengthMismatch {
        cell: CellId,
        vertices: usize,
        edges: usize,
    },
    /// A cell references a vertex that is not present.
    #[error("cell {cell:?} references invalid vertex {vertex:?}")]
    InvalidCellVertex {
        cell: CellId,
        vertex: SurfaceVertexId,
    },
    /// A cell repeats a vertex in its local boundary.
    #[error("cell {cell:?} repeats boundary vertex {vertex:?}")]
    DuplicateCellVertex {
        cell: CellId,
        vertex: SurfaceVertexId,
    },
    /// A cell references an edge that is not present.
    #[error("cell {cell:?} references invalid edge {edge:?}")]
    InvalidCellEdge { cell: CellId, edge: EdgeId },
    /// A cell repeats an edge in its local boundary.
    #[error("cell {cell:?} repeats boundary edge {edge:?}")]
    DuplicateCellEdge { cell: CellId, edge: EdgeId },
    /// A cell's cyclic boundary does not begin at its minimum vertex ID.
    #[error("cell {cell:?} does not use its canonical boundary start")]
    NonCanonicalCellBoundaryStart { cell: CellId },
    /// An edge references a vertex that is not present.
    #[error("edge {edge:?} references invalid vertex {vertex:?}")]
    InvalidEdgeVertex {
        edge: EdgeId,
        vertex: SurfaceVertexId,
    },
    /// An edge uses the same endpoint twice.
    #[error("edge {edge:?} has duplicate endpoint {vertex:?}")]
    DuplicateEdgeEndpoint {
        edge: EdgeId,
        vertex: SurfaceVertexId,
    },
    /// An edge's endpoint IDs are not in canonical ascending order.
    #[error("edge {edge:?} endpoint IDs are not sorted")]
    UnsortedEdgeVertices { edge: EdgeId },
    /// Two edge records claim the same canonical endpoint pair.
    #[error("edge {edge:?} duplicates canonical endpoints already owned by {previous_edge:?}")]
    DuplicateCanonicalEdge { edge: EdgeId, previous_edge: EdgeId },
    /// An edge references a cell that is not present.
    #[error("edge {edge:?} references invalid owner {owner:?}")]
    InvalidEdgeOwner { edge: EdgeId, owner: CellId },
    /// An edge has only one distinct owner.
    #[error("edge {edge:?} names duplicate owner {owner:?}")]
    DuplicateEdgeOwner { edge: EdgeId, owner: CellId },
    /// An edge's owner IDs are not in canonical ascending order.
    #[error("edge {edge:?} owner IDs are not sorted")]
    UnsortedEdgeOwners { edge: EdgeId },
    /// A cyclic cell side does not map to its referenced canonical edge.
    #[error("cell {cell:?} side {side} does not match edge {edge:?}")]
    CellSideEdgeMismatch {
        cell: CellId,
        side: usize,
        edge: EdgeId,
    },
    /// An edge is not listed exactly once by each owner and nowhere else.
    #[error(
        "edge {edge:?} incidence is first={first_owner_count}, second={second_owner_count}, other={other_count}"
    )]
    EdgeIncidenceMismatch {
        edge: EdgeId,
        first_owner_count: usize,
        second_owner_count: usize,
        other_count: usize,
    },
    /// Both owners traverse a shared edge in the same direction.
    #[error("edge {edge:?} is traversed in the same direction by both owners")]
    EdgeTraversalMismatch { edge: EdgeId },
    /// The incident cells and edges around a vertex do not form one cyclic link.
    #[error("vertex {vertex:?} link is not one cycle")]
    VertexLinkNotSingleCycle { vertex: SurfaceVertexId },
    /// The cell graph induced by shared edges is not connected.
    #[error("cell adjacency reaches {reached} of {total} cells")]
    DisconnectedCellAdjacency { reached: usize, total: usize },
    /// A stored edge midpoint differs from its endpoint-arc midpoint.
    #[error("edge {edge:?} stores an incorrect midpoint")]
    EdgeMidpointMismatch { edge: EdgeId },
    /// A stored edge length differs from its recomputed arc length.
    #[error("edge {edge:?} stores length {stored}, expected {calculated}")]
    EdgeLengthMismatch {
        edge: EdgeId,
        stored: f64,
        calculated: f64,
    },
    /// A stored site-to-site distance differs from recomputation.
    #[error("edge {edge:?} stores center distance {stored}, expected {calculated}")]
    EdgeCenterDistanceMismatch {
        edge: EdgeId,
        stored: f64,
        calculated: f64,
    },
    /// A stored site-to-midpoint distance differs from recomputation.
    #[error(
        "edge {edge:?} stores midpoint distance {stored}, expected {calculated} for owner {owner}"
    )]
    EdgeMidpointDistanceMismatch {
        edge: EdgeId,
        owner: usize,
        stored: f64,
        calculated: f64,
    },
    /// A stored tangent normal differs from its oriented recomputation.
    #[error("edge {edge:?} stores an incorrect oriented tangent normal")]
    EdgeNormalMismatch { edge: EdgeId },
    /// A stored cell area differs from spherical polygon recomputation.
    #[error("cell {cell:?} stores area {stored}, expected {calculated}")]
    CellAreaMismatch {
        cell: CellId,
        stored: f64,
        calculated: f64,
    },
    /// A stored centroid differs from spherical polygon recomputation.
    #[error("cell {cell:?} stores an incorrect spherical centroid")]
    CellCentroidMismatch { cell: CellId },
    /// A cell boundary is not outward counter-clockwise around its site.
    #[error("cell {cell:?} boundary is not outward counter-clockwise at side {side}")]
    CellOrientationMismatch { cell: CellId, side: usize },
    /// The record counts do not describe a genus-zero closed manifold.
    #[error("Euler characteristic mismatch for V={vertices}, E={edges}, F={cells}")]
    EulerCharacteristicMismatch {
        vertices: usize,
        edges: usize,
        cells: usize,
    },
    /// Stored cell areas do not cover the sphere exactly once within tolerance.
    #[error("cell areas total {stored}, expected sphere area {calculated}")]
    TotalAreaMismatch { stored: f64, calculated: f64 },
    /// The stored content fingerprint does not match the canonical semantic fields.
    #[error("spherical surface fingerprint does not match its semantic fields")]
    FingerprintMismatch,
}

impl SphericalSurfaceSnapshot {
    /// Validates a constructed or deserialized closed spherical surface in deterministic order.
    pub fn validate(&self) -> Result<(), SphericalSurfaceValidationError> {
        self.validate_header()?;
        self.validate_ids_and_vectors()?;
        self.validate_cell_shapes()?;
        self.validate_cell_references()?;
        self.validate_canonical_boundary_starts()?;
        self.validate_edge_references()?;
        self.validate_cyclic_sides()?;
        self.validate_incidence()?;
        self.validate_euler_characteristic()?;
        self.validate_manifold_topology()?;
        self.validate_edge_metrics()?;
        self.validate_cell_metrics()?;
        self.validate_orientation()?;
        self.validate_global_area()?;
        if self.fingerprint != self.canonical_fingerprint() {
            return Err(SphericalSurfaceValidationError::FingerprintMismatch);
        }
        Ok(())
    }

    fn validate_header(&self) -> Result<(), SphericalSurfaceValidationError> {
        if self.schema_version != SPHERICAL_SURFACE_SCHEMA_V1 {
            return Err(SphericalSurfaceValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: SPHERICAL_SURFACE_SCHEMA_V1,
            });
        }
        validate_record_count(
            "vertex",
            self.vertices.len(),
            MAX_SPHERICAL_VERTEX_COUNT as usize,
        )?;
        validate_record_count("cell", self.cells.len(), MAX_SPHERICAL_CELL_COUNT as usize)?;
        validate_record_count("edge", self.edges.len(), MAX_SPHERICAL_EDGE_COUNT as usize)?;
        let radius = self.radius.get();
        if !radius.is_finite() || radius <= 0.0 {
            return Err(SphericalSurfaceValidationError::InvalidRadius { found: radius });
        }
        Ok(())
    }

    fn validate_ids_and_vectors(&self) -> Result<(), SphericalSurfaceValidationError> {
        for (position, vertex) in self.vertices.iter().enumerate() {
            if vertex.id.raw() as usize != position {
                return Err(SphericalSurfaceValidationError::NonContiguousVertexId {
                    position,
                    found: vertex.id,
                });
            }
            validate_unit(vertex.position, "vertex", vertex.id.raw(), "position")?;
        }
        for (position, cell) in self.cells.iter().enumerate() {
            if cell.id.raw() as usize != position {
                return Err(SphericalSurfaceValidationError::NonContiguousCellId {
                    position,
                    found: cell.id,
                });
            }
            validate_unit(cell.site, "cell", cell.id.raw(), "site")?;
            validate_unit(cell.centroid, "cell", cell.id.raw(), "centroid")?;
        }
        for (position, edge) in self.edges.iter().enumerate() {
            if edge.id.raw() as usize != position {
                return Err(SphericalSurfaceValidationError::NonContiguousEdgeId {
                    position,
                    found: edge.id,
                });
            }
            validate_unit(edge.midpoint, "edge", edge.id.raw(), "midpoint")?;
            validate_unit(
                edge.normal_from_first,
                "edge",
                edge.id.raw(),
                "normal_from_first",
            )?;
        }
        Ok(())
    }

    fn validate_cell_shapes(&self) -> Result<(), SphericalSurfaceValidationError> {
        for cell in &self.cells {
            if cell.boundary_vertices.len() < 3 {
                return Err(SphericalSurfaceValidationError::CellBoundaryTooSmall {
                    cell: cell.id,
                    vertices: cell.boundary_vertices.len(),
                });
            }
            if cell.boundary_vertices.len() != cell.boundary_edges.len() {
                return Err(
                    SphericalSurfaceValidationError::CellBoundaryLengthMismatch {
                        cell: cell.id,
                        vertices: cell.boundary_vertices.len(),
                        edges: cell.boundary_edges.len(),
                    },
                );
            }
            if cell.boundary_vertices.len() > MAX_SPHERICAL_CELL_BOUNDARY_DEGREE {
                return Err(
                    SphericalSurfaceValidationError::CellBoundaryDegreeOutOfRange {
                        cell: cell.id,
                        found: cell.boundary_vertices.len(),
                        max: MAX_SPHERICAL_CELL_BOUNDARY_DEGREE,
                    },
                );
            }
        }
        Ok(())
    }

    fn validate_cell_references(&self) -> Result<(), SphericalSurfaceValidationError> {
        for cell in &self.cells {
            for (position, &vertex) in cell.boundary_vertices.iter().enumerate() {
                if vertex.raw() as usize >= self.vertices.len() {
                    return Err(SphericalSurfaceValidationError::InvalidCellVertex {
                        cell: cell.id,
                        vertex,
                    });
                }
                if cell.boundary_vertices[..position].contains(&vertex) {
                    return Err(SphericalSurfaceValidationError::DuplicateCellVertex {
                        cell: cell.id,
                        vertex,
                    });
                }
            }
            for (position, &edge) in cell.boundary_edges.iter().enumerate() {
                if edge.raw() as usize >= self.edges.len() {
                    return Err(SphericalSurfaceValidationError::InvalidCellEdge {
                        cell: cell.id,
                        edge,
                    });
                }
                if cell.boundary_edges[..position].contains(&edge) {
                    return Err(SphericalSurfaceValidationError::DuplicateCellEdge {
                        cell: cell.id,
                        edge,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_edge_references(&self) -> Result<(), SphericalSurfaceValidationError> {
        for edge in &self.edges {
            for &vertex in &edge.vertices {
                if vertex.raw() as usize >= self.vertices.len() {
                    return Err(SphericalSurfaceValidationError::InvalidEdgeVertex {
                        edge: edge.id,
                        vertex,
                    });
                }
            }
            if edge.vertices[0] == edge.vertices[1] {
                return Err(SphericalSurfaceValidationError::DuplicateEdgeEndpoint {
                    edge: edge.id,
                    vertex: edge.vertices[0],
                });
            }
            if edge.vertices[0] > edge.vertices[1] {
                return Err(SphericalSurfaceValidationError::UnsortedEdgeVertices {
                    edge: edge.id,
                });
            }
            for &owner in &edge.cells {
                if owner.raw() as usize >= self.cells.len() {
                    return Err(SphericalSurfaceValidationError::InvalidEdgeOwner {
                        edge: edge.id,
                        owner,
                    });
                }
            }
            if edge.cells[0] == edge.cells[1] {
                return Err(SphericalSurfaceValidationError::DuplicateEdgeOwner {
                    edge: edge.id,
                    owner: edge.cells[0],
                });
            }
            if edge.cells[0] > edge.cells[1] {
                return Err(SphericalSurfaceValidationError::UnsortedEdgeOwners { edge: edge.id });
            }
        }

        let mut canonical_edges = self
            .edges
            .iter()
            .map(|edge| (edge.vertices, edge.id))
            .collect::<Vec<_>>();
        canonical_edges.sort_unstable();
        for pair in canonical_edges.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(SphericalSurfaceValidationError::DuplicateCanonicalEdge {
                    edge: pair[1].1,
                    previous_edge: pair[0].1,
                });
            }
        }
        Ok(())
    }

    fn validate_canonical_boundary_starts(&self) -> Result<(), SphericalSurfaceValidationError> {
        for cell in &self.cells {
            let minimum = cell
                .boundary_vertices
                .iter()
                .min()
                .expect("cell shape validation rejected empty boundaries");
            if cell.boundary_vertices.first() != Some(minimum) {
                return Err(
                    SphericalSurfaceValidationError::NonCanonicalCellBoundaryStart {
                        cell: cell.id,
                    },
                );
            }
        }
        Ok(())
    }

    fn validate_cyclic_sides(&self) -> Result<(), SphericalSurfaceValidationError> {
        for cell in &self.cells {
            for side in 0..cell.boundary_vertices.len() {
                let first = cell.boundary_vertices[side];
                let second = cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()];
                let edge_id = cell.boundary_edges[side];
                let edge = &self.edges[edge_id.raw() as usize];
                let matches = (edge.vertices[0] == first && edge.vertices[1] == second)
                    || (edge.vertices[0] == second && edge.vertices[1] == first);
                if !matches {
                    return Err(SphericalSurfaceValidationError::CellSideEdgeMismatch {
                        cell: cell.id,
                        side,
                        edge: edge_id,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_incidence(&self) -> Result<(), SphericalSurfaceValidationError> {
        let mut incidence = vec![[0_usize; 3]; self.edges.len()];
        for cell in &self.cells {
            for &edge_id in &cell.boundary_edges {
                let edge = &self.edges[edge_id.raw() as usize];
                let slot = if cell.id == edge.cells[0] {
                    0
                } else if cell.id == edge.cells[1] {
                    1
                } else {
                    2
                };
                incidence[edge_id.raw() as usize][slot] += 1;
            }
        }
        for (edge, counts) in self.edges.iter().zip(incidence) {
            if counts != [1, 1, 0] {
                return Err(SphericalSurfaceValidationError::EdgeIncidenceMismatch {
                    edge: edge.id,
                    first_owner_count: counts[0],
                    second_owner_count: counts[1],
                    other_count: counts[2],
                });
            }
        }
        Ok(())
    }

    fn validate_manifold_topology(&self) -> Result<(), SphericalSurfaceValidationError> {
        self.validate_opposite_edge_traversal()?;
        self.validate_vertex_links()?;
        self.validate_cell_adjacency_connected()
    }

    fn validate_opposite_edge_traversal(&self) -> Result<(), SphericalSurfaceValidationError> {
        let mut directions = vec![[None; 2]; self.edges.len()];
        for cell in &self.cells {
            for side in 0..cell.boundary_vertices.len() {
                let edge_id = cell.boundary_edges[side];
                let edge = &self.edges[edge_id.raw() as usize];
                let owner = usize::from(cell.id == edge.cells[1]);
                directions[edge_id.raw() as usize][owner] =
                    Some(cell.boundary_vertices[side] == edge.vertices[0]);
            }
        }
        for (edge, owners) in self.edges.iter().zip(directions) {
            if owners[0] == owners[1] {
                return Err(SphericalSurfaceValidationError::EdgeTraversalMismatch {
                    edge: edge.id,
                });
            }
        }
        Ok(())
    }

    fn validate_vertex_links(&self) -> Result<(), SphericalSurfaceValidationError> {
        let mut degrees = vec![0_usize; self.vertices.len()];
        for edge in &self.edges {
            for vertex in edge.vertices {
                degrees[vertex.raw() as usize] += 1;
            }
        }

        let mut offsets = Vec::with_capacity(self.vertices.len() + 1);
        offsets.push(0_usize);
        for &degree in &degrees {
            offsets.push(offsets.last().copied().unwrap() + degree);
        }
        let mut cursors = offsets[..self.vertices.len()].to_vec();
        let mut edge_slots = vec![[0_u32; 2]; self.edges.len()];
        for edge in &self.edges {
            for (endpoint, vertex) in edge.vertices.into_iter().enumerate() {
                let vertex = vertex.raw() as usize;
                edge_slots[edge.id.raw() as usize][endpoint] = cursors[vertex] as u32;
                cursors[vertex] += 1;
            }
        }

        let incidence_count = offsets.last().copied().unwrap_or(0);
        let mut link_neighbors = vec![[0_u32; 2]; incidence_count];
        let mut link_counts = vec![0_u8; incidence_count];
        for cell in &self.cells {
            for position in 0..cell.boundary_vertices.len() {
                let vertex = cell.boundary_vertices[position];
                let previous = cell.boundary_edges
                    [(position + cell.boundary_edges.len() - 1) % cell.boundary_edges.len()];
                let next = cell.boundary_edges[position];
                let Some(previous_slot) = self.vertex_edge_slot(vertex, previous, &edge_slots)
                else {
                    return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                        vertex,
                    });
                };
                let Some(next_slot) = self.vertex_edge_slot(vertex, next, &edge_slots) else {
                    return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                        vertex,
                    });
                };
                for (slot, neighbor) in [(previous_slot, next_slot), (next_slot, previous_slot)] {
                    let count = link_counts[slot] as usize;
                    if count == 2 {
                        return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                            vertex,
                        });
                    }
                    link_neighbors[slot][count] = neighbor as u32;
                    link_counts[slot] += 1;
                }
            }
        }

        let mut reached = vec![false; incidence_count];
        let mut pending = Vec::new();
        for (vertex, &degree) in degrees.iter().enumerate() {
            if degree == 0 {
                return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                    vertex: SurfaceVertexId::from_raw(vertex as u32),
                });
            }
            let range = offsets[vertex]..offsets[vertex + 1];
            if range.clone().any(|slot| link_counts[slot] != 2) {
                return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                    vertex: SurfaceVertexId::from_raw(vertex as u32),
                });
            }

            pending.clear();
            pending.push(offsets[vertex]);
            let mut reached_count = 0_usize;
            while let Some(slot) = pending.pop() {
                if reached[slot] {
                    continue;
                }
                reached[slot] = true;
                reached_count += 1;
                pending.extend(link_neighbors[slot].map(|neighbor| neighbor as usize));
            }
            if reached_count != degree {
                return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                    vertex: SurfaceVertexId::from_raw(vertex as u32),
                });
            }
        }
        Ok(())
    }

    fn vertex_edge_slot(
        &self,
        vertex: SurfaceVertexId,
        edge: EdgeId,
        edge_slots: &[[u32; 2]],
    ) -> Option<usize> {
        let edge_position = edge.raw() as usize;
        let endpoints = self.edges[edge_position].vertices;
        let endpoint = if endpoints[0] == vertex {
            0
        } else if endpoints[1] == vertex {
            1
        } else {
            return None;
        };
        Some(edge_slots[edge_position][endpoint] as usize)
    }

    fn validate_cell_adjacency_connected(&self) -> Result<(), SphericalSurfaceValidationError> {
        if self.cells.is_empty() {
            return Ok(());
        }
        let mut reached = vec![false; self.cells.len()];
        reached[0] = true;
        let mut reached_count = 1_usize;
        let mut pending = vec![CellId::from_raw(0)];
        while let Some(cell) = pending.pop() {
            for &edge_id in &self.cells[cell.raw() as usize].boundary_edges {
                let owners = self.edges[edge_id.raw() as usize].cells;
                let neighbor = if owners[0] == cell {
                    owners[1]
                } else {
                    owners[0]
                };
                let neighbor_position = neighbor.raw() as usize;
                if !reached[neighbor_position] {
                    reached[neighbor_position] = true;
                    reached_count += 1;
                    pending.push(neighbor);
                }
            }
        }
        if reached_count != self.cells.len() {
            return Err(SphericalSurfaceValidationError::DisconnectedCellAdjacency {
                reached: reached_count,
                total: self.cells.len(),
            });
        }
        Ok(())
    }

    fn validate_edge_metrics(&self) -> Result<(), SphericalSurfaceValidationError> {
        let radius = self.radius.get();
        for edge in &self.edges {
            let first_vertex = self.vertices[edge.vertices[0].raw() as usize].position;
            let second_vertex = self.vertices[edge.vertices[1].raw() as usize].position;
            let midpoint = normalized(add(first_vertex.components(), second_vertex.components()))
                .ok_or(SphericalSurfaceValidationError::EdgeMidpointMismatch {
                edge: edge.id,
            })?;
            if central_angle(edge.midpoint, midpoint) > VECTOR_ANGLE_TOLERANCE {
                return Err(SphericalSurfaceValidationError::EdgeMidpointMismatch {
                    edge: edge.id,
                });
            }

            let calculated_length = radius * central_angle(first_vertex, second_vertex);
            if !positive_metric_close(edge.length.get(), calculated_length, radius) {
                return Err(SphericalSurfaceValidationError::EdgeLengthMismatch {
                    edge: edge.id,
                    stored: edge.length.get(),
                    calculated: calculated_length,
                });
            }

            let first_site = self.cells[edge.cells[0].raw() as usize].site;
            let second_site = self.cells[edge.cells[1].raw() as usize].site;
            let calculated_center_distance = radius * central_angle(first_site, second_site);
            if !positive_metric_close(
                edge.center_distance.get(),
                calculated_center_distance,
                radius,
            ) {
                return Err(
                    SphericalSurfaceValidationError::EdgeCenterDistanceMismatch {
                        edge: edge.id,
                        stored: edge.center_distance.get(),
                        calculated: calculated_center_distance,
                    },
                );
            }

            for (owner, site) in [first_site, second_site].into_iter().enumerate() {
                let calculated = radius * central_angle(site, midpoint);
                let stored = edge.center_distances_to_midpoint[owner].get();
                if !positive_metric_close(stored, calculated, radius) {
                    return Err(
                        SphericalSurfaceValidationError::EdgeMidpointDistanceMismatch {
                            edge: edge.id,
                            owner,
                            stored,
                            calculated,
                        },
                    );
                }
            }

            let site_delta = subtract(second_site.components(), first_site.components());
            let site_separation = norm(site_delta);
            for endpoint in [first_vertex, second_vertex] {
                let bisector_residual =
                    dot(endpoint.components(), site_delta).abs() / site_separation;
                if !bisector_residual.is_finite() || bisector_residual > VECTOR_ANGLE_TOLERANCE {
                    return Err(SphericalSurfaceValidationError::EdgeNormalMismatch {
                        edge: edge.id,
                    });
                }
            }

            let normal = oriented_arc_normal(first_vertex, second_vertex, first_site, second_site)
                .ok_or(SphericalSurfaceValidationError::EdgeNormalMismatch { edge: edge.id })?;
            if central_angle(edge.normal_from_first, normal) > VECTOR_ANGLE_TOLERANCE {
                return Err(SphericalSurfaceValidationError::EdgeNormalMismatch { edge: edge.id });
            }
        }
        Ok(())
    }

    fn validate_cell_metrics(&self) -> Result<(), SphericalSurfaceValidationError> {
        let radius = self.radius.get();
        for cell in &self.cells {
            let polygon = cell
                .boundary_vertices
                .iter()
                .map(|id| self.vertices[id.raw() as usize].position)
                .collect::<Vec<_>>();
            let (unit_area, centroid) = spherical_polygon_metrics(cell.site, &polygon)
                .ok_or(SphericalSurfaceValidationError::CellCentroidMismatch { cell: cell.id })?;
            let calculated_area = unit_area * radius * radius;
            if calculated_area <= 0.0
                || !area_close(cell.area.get(), calculated_area, radius * radius)
            {
                return Err(SphericalSurfaceValidationError::CellAreaMismatch {
                    cell: cell.id,
                    stored: cell.area.get(),
                    calculated: calculated_area,
                });
            }
            if central_angle(cell.centroid, centroid) > VECTOR_ANGLE_TOLERANCE {
                return Err(SphericalSurfaceValidationError::CellCentroidMismatch {
                    cell: cell.id,
                });
            }
        }
        Ok(())
    }

    fn validate_orientation(&self) -> Result<(), SphericalSurfaceValidationError> {
        for cell in &self.cells {
            for side in 0..cell.boundary_vertices.len() {
                let first = self.vertices[cell.boundary_vertices[side].raw() as usize]
                    .position
                    .components();
                let second = self.vertices[cell.boundary_vertices
                    [(side + 1) % cell.boundary_vertices.len()]
                .raw() as usize]
                    .position
                    .components();
                let edge_normal = normalized(cross(first, subtract(second, first))).ok_or(
                    SphericalSurfaceValidationError::CellOrientationMismatch {
                        cell: cell.id,
                        side,
                    },
                )?;
                if edge_normal.dot(cell.site) <= 0.0 {
                    return Err(SphericalSurfaceValidationError::CellOrientationMismatch {
                        cell: cell.id,
                        side,
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_global_area(&self) -> Result<(), SphericalSurfaceValidationError> {
        let stored = match self.try_total_cell_area() {
            Ok(area) => area.get(),
            Err(UnitError::NonFinite(value) | UnitError::NegativeArea(value)) => value,
            Err(UnitError::InvalidRectangle) => {
                unreachable!("area construction cannot fail this way")
            }
        };
        let calculated = 4.0 * std::f64::consts::PI * self.radius.get() * self.radius.get();
        if !stored.is_finite()
            || !calculated.is_finite()
            || !area_close(stored, calculated, self.radius.get() * self.radius.get())
        {
            return Err(SphericalSurfaceValidationError::TotalAreaMismatch { stored, calculated });
        }
        Ok(())
    }

    fn validate_euler_characteristic(&self) -> Result<(), SphericalSurfaceValidationError> {
        let characteristic =
            self.vertices.len() as i128 - self.edges.len() as i128 + self.cells.len() as i128;
        if characteristic != 2 {
            return Err(
                SphericalSurfaceValidationError::EulerCharacteristicMismatch {
                    vertices: self.vertices.len(),
                    edges: self.edges.len(),
                    cells: self.cells.len(),
                },
            );
        }
        Ok(())
    }
}

fn validate_record_count(
    record: &'static str,
    found: usize,
    max: usize,
) -> Result<(), SphericalSurfaceValidationError> {
    if found > max {
        return Err(SphericalSurfaceValidationError::RecordCountOutOfRange { record, found, max });
    }
    Ok(())
}

fn validate_unit(
    vector: UnitVector3,
    record: &'static str,
    id: u32,
    field: &'static str,
) -> Result<(), SphericalSurfaceValidationError> {
    let components = vector.components();
    if components.iter().any(|component| !component.is_finite())
        || (vector.norm() - 1.0).abs() > UNIT_TOLERANCE
    {
        return Err(SphericalSurfaceValidationError::InvalidUnitVector { record, id, field });
    }
    Ok(())
}

pub(crate) fn spherical_polygon_metrics(
    site: UnitVector3,
    polygon: &[UnitVector3],
) -> Option<(f64, UnitVector3)> {
    if polygon.len() < 3 {
        return None;
    }

    let mut area = CompensatedSum::default();
    let mut weighted_centroid = [CompensatedSum::default(); 3];
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let triangle_area = spherical_triangle_area_unit(site, first, second);
        if triangle_area <= 0.0 || !triangle_area.is_finite() {
            return None;
        }
        area.add(triangle_area);

        let triangle_centroid = normalized(add(
            add(site.components(), first.components()),
            second.components(),
        ))?;
        for (sum, component) in weighted_centroid
            .iter_mut()
            .zip(triangle_centroid.components())
        {
            sum.add(triangle_area * component);
        }
    }

    let area = area.total();
    if area <= 0.0 || !area.is_finite() {
        return None;
    }
    let centroid = normalized(weighted_centroid.map(CompensatedSum::total))?;
    Some((area, centroid))
}

fn normalized(vector: [f64; 3]) -> Option<UnitVector3> {
    UnitVector3::new(vector[0], vector[1], vector[2]).ok()
}

fn metric_close(stored: f64, calculated: f64, radius: f64) -> bool {
    stored.is_finite()
        && calculated.is_finite()
        && (stored - calculated).abs()
            <= ABSOLUTE_SCALE_ULPS * f64::EPSILON * radius
                + METRIC_RELATIVE_TOLERANCE * stored.abs().max(calculated.abs())
}

fn positive_metric_close(stored: f64, calculated: f64, radius: f64) -> bool {
    stored > 0.0 && calculated > 0.0 && metric_close(stored, calculated, radius)
}

fn area_close(stored: f64, calculated: f64, radius_squared: f64) -> bool {
    stored.is_finite()
        && calculated.is_finite()
        && (stored - calculated).abs()
            <= ABSOLUTE_SCALE_ULPS * f64::EPSILON * radius_squared
                + AREA_RELATIVE_TOLERANCE * stored.abs().max(calculated.abs())
}

#[derive(Debug, Clone, Copy, Default)]
struct CompensatedSum {
    sum: f64,
    compensation: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let adjusted = value - self.compensation;
        let next = self.sum + adjusted;
        self.compensation = (next - self.sum) - adjusted;
        self.sum = next;
    }

    fn total(self) -> f64 {
        self.sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_record_budgets_reject_max_plus_one_without_allocating() {
        for (record, max) in [
            ("vertex", MAX_SPHERICAL_VERTEX_COUNT as usize),
            ("cell", MAX_SPHERICAL_CELL_COUNT as usize),
            ("edge", MAX_SPHERICAL_EDGE_COUNT as usize),
        ] {
            assert!(validate_record_count(record, max, max).is_ok());
            assert!(matches!(
                validate_record_count(record, max + 1, max),
                Err(SphericalSurfaceValidationError::RecordCountOutOfRange {
                    record: found_record,
                    found,
                    max: found_max,
                }) if found_record == record && found == max + 1 && found_max == max
            ));
        }
    }

    #[test]
    fn positive_metric_matching_uses_the_absolute_floor_without_admitting_zero() {
        let metric_kinds = [
            ("edge length", 1.0),
            ("center distance", 2.0),
            ("first midpoint distance", 3.0),
            ("second midpoint distance", 4.0),
        ];

        for radius in [1.0, 6_371_000.0, 100_000_000.0] {
            let absolute_floor = ABSOLUTE_SCALE_ULPS * f64::EPSILON * radius;
            let perturbation = 4.0 * f64::EPSILON * radius;
            assert!(perturbation < absolute_floor);

            for (kind, calculated_ulps) in metric_kinds {
                let calculated = calculated_ulps * f64::EPSILON * radius;
                let stored = calculated + perturbation;
                let relative_allowance =
                    METRIC_RELATIVE_TOLERANCE * stored.abs().max(calculated.abs());
                assert!(
                    absolute_floor > relative_allowance,
                    "absolute floor must dominate for {kind}"
                );
                assert!(
                    perturbation > relative_allowance,
                    "perturbation must exceed relative tolerance for {kind}"
                );
                assert!(
                    positive_metric_close(stored, calculated, radius),
                    "absolute floor must accept positive roundoff for {kind}"
                );

                for non_positive in [0.0, -f64::EPSILON * radius] {
                    assert!(
                        metric_close(non_positive, calculated, radius),
                        "tolerance alone must admit the {kind} witness"
                    );
                    assert!(
                        !positive_metric_close(non_positive, calculated, radius),
                        "positivity must be checked before tolerance for {kind}"
                    );
                }
            }
        }
    }

    #[test]
    fn area_matching_uses_the_absolute_floor_for_positive_roundoff() {
        for radius in [1.0, 6_371_000.0, 100_000_000.0] {
            let radius_squared = radius * radius;
            let calculated = 4.0 * f64::EPSILON * radius_squared;
            let perturbation = 4.0 * f64::EPSILON * radius_squared;
            let stored = calculated + perturbation;
            let absolute_floor = ABSOLUTE_SCALE_ULPS * f64::EPSILON * radius_squared;
            let relative_allowance = AREA_RELATIVE_TOLERANCE * stored.abs().max(calculated.abs());

            assert!(absolute_floor > relative_allowance);
            assert!(perturbation > relative_allowance);
            assert!(perturbation < absolute_floor);
            assert!(area_close(stored, calculated, radius_squared));
        }
    }

    #[test]
    fn generic_rotation_near_coincident_polygon_metrics_remain_well_conditioned() {
        let site = UnitVector3::new(1.0, 1.0, 1.0).unwrap();
        let tangent_x = UnitVector3::new(1.0, -1.0, 0.0).unwrap();
        let tangent_y = UnitVector3::new(1.0, 1.0, -2.0).unwrap();

        for offset in [1.0e-6, 1.0e-7] {
            let polygon = [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)].map(|(x, y)| {
                let site = site.components();
                let tangent_x = tangent_x.components();
                let tangent_y = tangent_y.components();
                UnitVector3::new(
                    site[0] + offset * (x * tangent_x[0] + y * tangent_y[0]),
                    site[1] + offset * (x * tangent_x[1] + y * tangent_y[1]),
                    site[2] + offset * (x * tangent_x[2] + y * tangent_y[2]),
                )
                .unwrap()
            });

            let (area, centroid) = spherical_polygon_metrics(site, &polygon).unwrap();
            let expected_area = 4.0 * offset * offset;
            assert!((area - expected_area).abs() / expected_area <= 1.0e-9);
            assert!(central_angle(centroid, site) <= 1.0e-12);
        }
    }
}
