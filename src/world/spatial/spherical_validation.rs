use thiserror::Error;

use super::sphere_geometry::{add, cross, scale, subtract};
use super::{
    central_angle, project_tangent, spherical_triangle_area_unit, SphericalSurfaceSnapshot,
    UnitVector3, SPHERICAL_SURFACE_SCHEMA_V1,
};
use crate::world::{CellId, EdgeId, SurfaceVertexId};

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
        self.validate_edge_references()?;
        self.validate_cyclic_sides()?;
        self.validate_incidence()?;
        self.validate_manifold_topology()?;
        self.validate_edge_metrics()?;
        self.validate_cell_metrics()?;
        self.validate_orientation()?;
        self.validate_global_topology_and_area()?;
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
        }
        Ok(())
    }

    fn validate_cell_references(&self) -> Result<(), SphericalSurfaceValidationError> {
        for cell in &self.cells {
            let mut seen_vertices = std::collections::BTreeSet::new();
            for &vertex in &cell.boundary_vertices {
                if vertex.raw() as usize >= self.vertices.len() {
                    return Err(SphericalSurfaceValidationError::InvalidCellVertex {
                        cell: cell.id,
                        vertex,
                    });
                }
                if !seen_vertices.insert(vertex) {
                    return Err(SphericalSurfaceValidationError::DuplicateCellVertex {
                        cell: cell.id,
                        vertex,
                    });
                }
            }
            let mut seen_edges = std::collections::BTreeSet::new();
            for &edge in &cell.boundary_edges {
                if edge.raw() as usize >= self.edges.len() {
                    return Err(SphericalSurfaceValidationError::InvalidCellEdge {
                        cell: cell.id,
                        edge,
                    });
                }
                if !seen_edges.insert(edge) {
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
        let mut canonical_edges = std::collections::BTreeMap::new();
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
            if let Some(previous_edge) = canonical_edges.insert(edge.vertices, edge.id) {
                return Err(SphericalSurfaceValidationError::DuplicateCanonicalEdge {
                    edge: edge.id,
                    previous_edge,
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
        let mut incident_edges =
            vec![std::collections::BTreeSet::<EdgeId>::new(); self.vertices.len()];
        for edge in &self.edges {
            for vertex in edge.vertices {
                incident_edges[vertex.raw() as usize].insert(edge.id);
            }
        }
        let mut links =
            vec![std::collections::BTreeMap::<EdgeId, Vec<EdgeId>>::new(); self.vertices.len()];
        for cell in &self.cells {
            for position in 0..cell.boundary_vertices.len() {
                let vertex = cell.boundary_vertices[position];
                let previous = cell.boundary_edges
                    [(position + cell.boundary_edges.len() - 1) % cell.boundary_edges.len()];
                let next = cell.boundary_edges[position];
                links[vertex.raw() as usize]
                    .entry(previous)
                    .or_default()
                    .push(next);
                links[vertex.raw() as usize]
                    .entry(next)
                    .or_default()
                    .push(previous);
            }
        }

        for (position, (incident_edges, link)) in incident_edges.iter().zip(&links).enumerate() {
            if incident_edges.is_empty() {
                return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                    vertex: SurfaceVertexId::from_raw(position as u32),
                });
            }
            if link.len() != incident_edges.len()
                || link.keys().any(|edge| !incident_edges.contains(edge))
                || link.values().any(|neighbors| neighbors.len() != 2)
            {
                return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                    vertex: SurfaceVertexId::from_raw(position as u32),
                });
            }

            let start = *incident_edges
                .first()
                .expect("nonempty incident edge set has a first member");
            let mut reached = std::collections::BTreeSet::new();
            let mut pending = vec![start];
            while let Some(edge) = pending.pop() {
                if !reached.insert(edge) {
                    continue;
                }
                pending.extend(link[&edge].iter().copied());
            }
            if &reached != incident_edges {
                return Err(SphericalSurfaceValidationError::VertexLinkNotSingleCycle {
                    vertex: SurfaceVertexId::from_raw(position as u32),
                });
            }
        }
        Ok(())
    }

    fn validate_cell_adjacency_connected(&self) -> Result<(), SphericalSurfaceValidationError> {
        if self.cells.is_empty() {
            return Ok(());
        }
        let mut reached = std::collections::BTreeSet::new();
        let mut pending = vec![CellId::from_raw(0)];
        while let Some(cell) = pending.pop() {
            if !reached.insert(cell) {
                continue;
            }
            for &edge_id in &self.cells[cell.raw() as usize].boundary_edges {
                let owners = self.edges[edge_id.raw() as usize].cells;
                pending.push(if owners[0] == cell {
                    owners[1]
                } else {
                    owners[0]
                });
            }
        }
        if reached.len() != self.cells.len() {
            return Err(SphericalSurfaceValidationError::DisconnectedCellAdjacency {
                reached: reached.len(),
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
            if edge.length.get() <= 0.0
                || calculated_length <= 0.0
                || !metric_close(edge.length.get(), calculated_length, radius)
            {
                return Err(SphericalSurfaceValidationError::EdgeLengthMismatch {
                    edge: edge.id,
                    stored: edge.length.get(),
                    calculated: calculated_length,
                });
            }

            let first_site = self.cells[edge.cells[0].raw() as usize].site;
            let second_site = self.cells[edge.cells[1].raw() as usize].site;
            let calculated_center_distance = radius * central_angle(first_site, second_site);
            if edge.center_distance.get() <= 0.0
                || calculated_center_distance <= 0.0
                || !metric_close(
                    edge.center_distance.get(),
                    calculated_center_distance,
                    radius,
                )
            {
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
                if stored <= 0.0 || calculated <= 0.0 || !metric_close(stored, calculated, radius) {
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
            let tangent = project_tangent(site_delta, midpoint);
            let normal = normalized(tangent)
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
                let edge_normal = normalized(cross(first, second)).ok_or(
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

    fn validate_global_topology_and_area(&self) -> Result<(), SphericalSurfaceValidationError> {
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

        let stored = compensated_sum(self.cells.iter().map(|cell| cell.area.get()));
        let calculated = 4.0 * std::f64::consts::PI * self.radius.get() * self.radius.get();
        if !stored.is_finite()
            || !calculated.is_finite()
            || !area_close(stored, calculated, self.radius.get() * self.radius.get())
        {
            return Err(SphericalSurfaceValidationError::TotalAreaMismatch { stored, calculated });
        }
        Ok(())
    }
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
    let mut vector_area = [CompensatedSum::default(); 3];
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let triangle_area = spherical_triangle_area_unit(site, first, second);
        if triangle_area <= 0.0 || !triangle_area.is_finite() {
            return None;
        }
        area.add(triangle_area);

        // Adjacent fan triangles' two site spokes cancel analytically. Accumulating only
        // the surviving boundary term avoids destructive cancellation for short arcs.
        let edge_cross = cross(first.components(), second.components());
        let sine = edge_cross[0].hypot(edge_cross[1]).hypot(edge_cross[2]);
        if sine == 0.0 || !sine.is_finite() {
            return None;
        }
        let contribution = scale(edge_cross, central_angle(first, second) / sine);
        for component in 0..3 {
            vector_area[component].add(contribution[component]);
        }
    }

    let area = area.total();
    if area <= 0.0 || !area.is_finite() {
        return None;
    }
    let centroid = normalized(vector_area.map(CompensatedSum::total))?;
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

fn area_close(stored: f64, calculated: f64, radius_squared: f64) -> bool {
    stored.is_finite()
        && calculated.is_finite()
        && (stored - calculated).abs()
            <= ABSOLUTE_SCALE_ULPS * f64::EPSILON * radius_squared
                + AREA_RELATIVE_TOLERANCE * stored.abs().max(calculated.abs())
}

fn compensated_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = CompensatedSum::default();
    for value in values {
        sum.add(value);
    }
    sum.total()
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
