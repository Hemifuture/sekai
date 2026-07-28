use thiserror::Error;

use super::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
use crate::world::{CellId, EdgeId, WorldPoint, WorldRect};

const SHAPE_RELATIVE_TOLERANCE: f64 = 1.0e-8;
const BOUNDS_SCALE_TOLERANCE: f64 = 1.0e-9;
const TOTAL_AREA_RELATIVE_TOLERANCE: f64 = 1.0e-7;

/// Errors returned when spatial records do not form a valid rectangular partition.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SpatialValidationError {
    /// The snapshot uses a schema version that this engine does not support.
    #[error("unsupported spatial schema version {found}; supported version is {supported}")]
    UnsupportedSchema {
        /// The schema version found in the snapshot.
        found: u16,
        /// The schema version supported by this engine.
        supported: u16,
    },
    /// A cell ID does not equal its position in canonical cell order.
    #[error("cell at position {position} has non-contiguous ID {found:?}")]
    NonContiguousCellId {
        /// The expected canonical position.
        position: usize,
        /// The cell ID found at that position.
        found: CellId,
    },
    /// An edge ID does not equal its position in canonical edge order.
    #[error("edge at position {position} has non-contiguous ID {found:?}")]
    NonContiguousEdgeId {
        /// The expected canonical position.
        position: usize,
        /// The edge ID found at that position.
        found: EdgeId,
    },
    /// A polygon has fewer than three vertices.
    #[error("cell {cell:?} polygon has only {vertices} vertices")]
    PolygonTooSmall {
        /// The cell whose polygon is too small.
        cell: CellId,
        /// The number of polygon vertices found.
        vertices: usize,
    },
    /// A polygon vertex contains a non-finite coordinate.
    #[error("cell {cell:?} polygon vertex {vertex} is non-finite")]
    NonFinitePolygonVertex {
        /// The cell containing the invalid vertex.
        cell: CellId,
        /// The position of the invalid vertex.
        vertex: usize,
    },
    /// Stable polygon area or centroid calculation produced a non-finite result.
    #[error("cell {cell:?} polygon calculation is non-finite")]
    NonFinitePolygonCalculation {
        /// The cell whose polygon could not be calculated finitely.
        cell: CellId,
    },
    /// A polygon is clockwise or has zero signed area.
    #[error("cell {cell:?} polygon has non-positive signed area {signed_area}")]
    NonPositivePolygonArea {
        /// The cell containing the invalid polygon.
        cell: CellId,
        /// The calculated signed area.
        signed_area: f64,
    },
    /// A stored cell area differs from its calculated polygon area.
    #[error("cell {cell:?} stores area {stored}, but its polygon area is {calculated}")]
    AreaMismatch {
        /// The cell containing the mismatch.
        cell: CellId,
        /// The stored area in square meters.
        stored: f64,
        /// The calculated area in square meters.
        calculated: f64,
    },
    /// A stored centroid differs from its calculated polygon centroid.
    #[error(
        "cell {cell:?} stores centroid {stored:?}, but its polygon centroid is {calculated:?}"
    )]
    CentroidMismatch {
        /// The cell containing the mismatch.
        cell: CellId,
        /// The stored centroid.
        stored: WorldPoint,
        /// The calculated centroid.
        calculated: WorldPoint,
    },
    /// A cell site lies outside the rectangle's scale-aware tolerance.
    #[error("cell {cell:?} site {site:?} lies outside the world bounds")]
    SiteOutOfBounds {
        /// The cell containing the invalid site.
        cell: CellId,
        /// The site outside the bounds.
        site: WorldPoint,
    },
    /// A polygon vertex lies outside the rectangle's scale-aware tolerance.
    #[error("cell {cell:?} polygon vertex {vertex} at {point:?} lies outside the world bounds")]
    PolygonVertexOutOfBounds {
        /// The cell containing the invalid vertex.
        cell: CellId,
        /// The position of the invalid vertex.
        vertex: usize,
        /// The vertex outside the bounds.
        point: WorldPoint,
    },
    /// A cell lists the same neighbor more than once.
    #[error("cell {cell:?} lists duplicate neighbor {neighbor:?}")]
    DuplicateNeighbor {
        /// The cell containing the duplicate.
        cell: CellId,
        /// The duplicated neighbor.
        neighbor: CellId,
    },
    /// A cell's neighbors are not in ascending stable-ID order.
    #[error("cell {cell:?} neighbors are not sorted at {previous:?}, {next:?}")]
    UnsortedNeighbors {
        /// The cell containing the unordered pair.
        cell: CellId,
        /// The first neighbor in the unordered pair.
        previous: CellId,
        /// The second neighbor in the unordered pair.
        next: CellId,
    },
    /// A cell lists a neighbor that is not present in the snapshot.
    #[error("cell {cell:?} lists invalid neighbor {neighbor:?}")]
    InvalidNeighbor {
        /// The cell containing the invalid reference.
        cell: CellId,
        /// The neighbor not present in the snapshot.
        neighbor: CellId,
    },
    /// A cell lists itself as a neighbor.
    #[error("cell {cell:?} lists itself as a neighbor")]
    SelfNeighbor {
        /// The self-referencing cell.
        cell: CellId,
    },
    /// One side of a neighbor relation does not list the other side.
    #[error("cell {cell:?} lists {neighbor:?}, but the relation is not symmetric")]
    AsymmetricNeighbors {
        /// The cell containing the relation.
        cell: CellId,
        /// The cell that does not contain the reverse relation.
        neighbor: CellId,
    },
    /// An edge does not have one or two owning cells.
    #[error("edge {edge:?} has invalid owner count {owner_count}")]
    InvalidEdgeOwnership {
        /// The edge with invalid ownership.
        edge: EdgeId,
        /// The number of owners found.
        owner_count: usize,
    },
    /// An edge references a cell that is not present in the snapshot.
    #[error("edge {edge:?} references invalid owner {cell:?}")]
    InvalidEdgeOwner {
        /// The edge containing the invalid reference.
        edge: EdgeId,
        /// The owner not present in the snapshot.
        cell: CellId,
    },
    /// An internal edge names the same cell as both owners.
    #[error("edge {edge:?} names duplicate owner {cell:?}")]
    DuplicateEdgeOwner {
        /// The edge containing the duplicate.
        edge: EdgeId,
        /// The duplicated owner.
        cell: CellId,
    },
    /// An internal edge joins cells that are not neighbors.
    #[error("edge {edge:?} joins non-neighbors {first:?} and {second:?}")]
    InternalEdgeWithoutNeighbors {
        /// The invalid internal edge.
        edge: EdgeId,
        /// One owner of the edge.
        first: CellId,
        /// The other owner of the edge.
        second: CellId,
    },
    /// A neighbor pair does not own exactly one internal edge.
    #[error("neighbors {first:?} and {second:?} own {count} internal edges")]
    NeighborEdgeCount {
        /// One cell in the neighbor pair.
        first: CellId,
        /// The other cell in the neighbor pair.
        second: CellId,
        /// The number of matching internal edges.
        count: usize,
    },
    /// A stored edge length differs from the distance between its endpoints.
    #[error("edge {edge:?} stores length {stored}, but its endpoint distance is {calculated}")]
    EdgeLengthMismatch {
        /// The edge containing the mismatch.
        edge: EdgeId,
        /// The stored edge length in meters.
        stored: f64,
        /// The calculated endpoint distance in meters.
        calculated: f64,
    },
    /// An edge does not coincide with a complete segment of an owning polygon.
    #[error("edge {edge:?} does not coincide with owner {cell:?}'s polygon")]
    EdgeNotOnCellPolygon {
        /// The edge that does not match the polygon.
        edge: EdgeId,
        /// The owner whose polygon does not contain the edge.
        cell: CellId,
    },
    /// A one-owner boundary edge does not lie on the world rectangle.
    #[error("boundary edge {edge:?} does not lie on the world rectangle")]
    BoundaryEdgeOffBounds {
        /// The boundary edge away from the rectangle.
        edge: EdgeId,
    },
    /// The stored cell areas do not sum to the rectangle's area.
    #[error("cell areas sum to {stored}, but rectangle area is {rectangle}")]
    TotalAreaMismatch {
        /// The compensated sum of stored cell areas.
        stored: f64,
        /// The rectangle's calculated area.
        rectangle: f64,
    },
}

impl SpatialSnapshot {
    /// Rechecks every schema, geometry, ordering, ownership, and coverage invariant.
    pub fn validate(&self) -> Result<(), SpatialValidationError> {
        if self.schema_version != SPATIAL_SCHEMA_V1 {
            return Err(SpatialValidationError::UnsupportedSchema {
                found: self.schema_version,
                supported: SPATIAL_SCHEMA_V1,
            });
        }

        validate_ids(&self.cells, &self.edges)?;
        let tolerance = bounds_tolerance(self.bounds);

        for cell in &self.cells {
            validate_cell(cell, self.bounds, tolerance, self.cells.len())?;
        }
        validate_neighbor_symmetry(&self.cells)?;
        validate_edge_ownership(&self.cells, &self.edges)?;
        validate_neighbor_edge_counts(&self.cells, &self.edges)?;
        validate_total_area(self)?;
        validate_edge_geometry(self, tolerance)?;
        Ok(())
    }
}

fn validate_ids(
    cells: &[SpatialCell],
    edges: &[SpatialEdge],
) -> Result<(), SpatialValidationError> {
    for (position, cell) in cells.iter().enumerate() {
        if u64::from(cell.id.raw()) != position as u64 {
            return Err(SpatialValidationError::NonContiguousCellId {
                position,
                found: cell.id,
            });
        }
    }
    for (position, edge) in edges.iter().enumerate() {
        if u64::from(edge.id.raw()) != position as u64 {
            return Err(SpatialValidationError::NonContiguousEdgeId {
                position,
                found: edge.id,
            });
        }
    }
    Ok(())
}

fn validate_cell(
    cell: &SpatialCell,
    bounds: WorldRect,
    tolerance: f64,
    cell_count: usize,
) -> Result<(), SpatialValidationError> {
    if cell.polygon.len() < 3 {
        return Err(SpatialValidationError::PolygonTooSmall {
            cell: cell.id,
            vertices: cell.polygon.len(),
        });
    }
    for (vertex, point) in cell.polygon.iter().copied().enumerate() {
        if !point_is_finite(point) {
            return Err(SpatialValidationError::NonFinitePolygonVertex {
                cell: cell.id,
                vertex,
            });
        }
    }

    let (area, centroid) = polygon_area_and_centroid(&cell.polygon)
        .ok_or(SpatialValidationError::NonFinitePolygonCalculation { cell: cell.id })?;
    if area <= 0.0 {
        return Err(SpatialValidationError::NonPositivePolygonArea {
            cell: cell.id,
            signed_area: area,
        });
    }
    if !relative_eq(cell.area.get(), area, SHAPE_RELATIVE_TOLERANCE) {
        return Err(SpatialValidationError::AreaMismatch {
            cell: cell.id,
            stored: cell.area.get(),
            calculated: area,
        });
    }
    if !relative_eq(
        cell.centroid.x().get(),
        centroid.x().get(),
        SHAPE_RELATIVE_TOLERANCE,
    ) || !relative_eq(
        cell.centroid.y().get(),
        centroid.y().get(),
        SHAPE_RELATIVE_TOLERANCE,
    ) {
        return Err(SpatialValidationError::CentroidMismatch {
            cell: cell.id,
            stored: cell.centroid,
            calculated: centroid,
        });
    }
    if !point_in_bounds(cell.site, bounds, tolerance) {
        return Err(SpatialValidationError::SiteOutOfBounds {
            cell: cell.id,
            site: cell.site,
        });
    }
    for (vertex, point) in cell.polygon.iter().copied().enumerate() {
        if !point_in_bounds(point, bounds, tolerance) {
            return Err(SpatialValidationError::PolygonVertexOutOfBounds {
                cell: cell.id,
                vertex,
                point,
            });
        }
    }

    for pair in cell.neighbors.windows(2) {
        if pair[0] == pair[1] {
            return Err(SpatialValidationError::DuplicateNeighbor {
                cell: cell.id,
                neighbor: pair[0],
            });
        }
        if pair[0] > pair[1] {
            return Err(SpatialValidationError::UnsortedNeighbors {
                cell: cell.id,
                previous: pair[0],
                next: pair[1],
            });
        }
    }
    for &neighbor in &cell.neighbors {
        if neighbor == cell.id {
            return Err(SpatialValidationError::SelfNeighbor { cell: cell.id });
        }
        if neighbor.raw() as usize >= cell_count {
            return Err(SpatialValidationError::InvalidNeighbor {
                cell: cell.id,
                neighbor,
            });
        }
    }
    Ok(())
}

fn validate_neighbor_symmetry(cells: &[SpatialCell]) -> Result<(), SpatialValidationError> {
    for cell in cells {
        for &neighbor in &cell.neighbors {
            let reverse = &cells[neighbor.raw() as usize].neighbors;
            if reverse.binary_search(&cell.id).is_err() {
                return Err(SpatialValidationError::AsymmetricNeighbors {
                    cell: cell.id,
                    neighbor,
                });
            }
        }
    }
    Ok(())
}

fn validate_edge_ownership(
    cells: &[SpatialCell],
    edges: &[SpatialEdge],
) -> Result<(), SpatialValidationError> {
    for edge in edges {
        let owners: Vec<CellId> = edge.cells.iter().flatten().copied().collect();
        if !(1..=2).contains(&owners.len()) {
            return Err(SpatialValidationError::InvalidEdgeOwnership {
                edge: edge.id,
                owner_count: owners.len(),
            });
        }
        for &owner in &owners {
            if owner.raw() as usize >= cells.len() {
                return Err(SpatialValidationError::InvalidEdgeOwner {
                    edge: edge.id,
                    cell: owner,
                });
            }
        }
        if owners.len() == 2 {
            if owners[0] == owners[1] {
                return Err(SpatialValidationError::DuplicateEdgeOwner {
                    edge: edge.id,
                    cell: owners[0],
                });
            }
            if cells[owners[0].raw() as usize]
                .neighbors
                .binary_search(&owners[1])
                .is_err()
            {
                return Err(SpatialValidationError::InternalEdgeWithoutNeighbors {
                    edge: edge.id,
                    first: owners[0],
                    second: owners[1],
                });
            }
        }
    }
    Ok(())
}

fn validate_neighbor_edge_counts(
    cells: &[SpatialCell],
    edges: &[SpatialEdge],
) -> Result<(), SpatialValidationError> {
    for cell in cells {
        for &neighbor in &cell.neighbors {
            if cell.id >= neighbor {
                continue;
            }
            let count = edges
                .iter()
                .filter(|edge| edge_has_owners(edge, cell.id, neighbor))
                .count();
            if count != 1 {
                return Err(SpatialValidationError::NeighborEdgeCount {
                    first: cell.id,
                    second: neighbor,
                    count,
                });
            }
        }
    }
    Ok(())
}

fn validate_total_area(snapshot: &SpatialSnapshot) -> Result<(), SpatialValidationError> {
    let stored = compensated_sum(snapshot.cells.iter().map(|cell| cell.area.get()));
    let rectangle = snapshot.bounds.width().get() * snapshot.bounds.height().get();
    if !stored.is_finite()
        || !rectangle.is_finite()
        || !relative_eq(stored, rectangle, TOTAL_AREA_RELATIVE_TOLERANCE)
    {
        return Err(SpatialValidationError::TotalAreaMismatch { stored, rectangle });
    }
    Ok(())
}

fn validate_edge_geometry(
    snapshot: &SpatialSnapshot,
    tolerance: f64,
) -> Result<(), SpatialValidationError> {
    for edge in &snapshot.edges {
        let calculated = (edge.end.x().get() - edge.start.x().get())
            .hypot(edge.end.y().get() - edge.start.y().get());
        if !calculated.is_finite()
            || !relative_eq(edge.length.get(), calculated, SHAPE_RELATIVE_TOLERANCE)
        {
            return Err(SpatialValidationError::EdgeLengthMismatch {
                edge: edge.id,
                stored: edge.length.get(),
                calculated,
            });
        }

        let owners: Vec<CellId> = edge.cells.iter().flatten().copied().collect();
        for owner in owners.iter().copied() {
            if !edge_matches_polygon(edge, &snapshot.cells[owner.raw() as usize], tolerance) {
                return Err(SpatialValidationError::EdgeNotOnCellPolygon {
                    edge: edge.id,
                    cell: owner,
                });
            }
        }
        if owners.len() == 1 && !edge_on_bounds(edge, snapshot.bounds, tolerance) {
            return Err(SpatialValidationError::BoundaryEdgeOffBounds { edge: edge.id });
        }
    }
    Ok(())
}

fn polygon_area_and_centroid(polygon: &[WorldPoint]) -> Option<(f64, WorldPoint)> {
    let origin_x = polygon[0].x().get();
    let origin_y = polygon[0].y().get();
    let mut cross_terms = Vec::with_capacity(polygon.len());
    let mut centroid_x_terms = Vec::with_capacity(polygon.len());
    let mut centroid_y_terms = Vec::with_capacity(polygon.len());

    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let first_x = first.x().get() - origin_x;
        let first_y = first.y().get() - origin_y;
        let second_x = second.x().get() - origin_x;
        let second_y = second.y().get() - origin_y;
        let cross = first_x * second_y - second_x * first_y;
        cross_terms.push(cross);
        centroid_x_terms.push((first_x + second_x) * cross);
        centroid_y_terms.push((first_y + second_y) * cross);
    }

    let cross_sum = compensated_sum(cross_terms);
    let area = 0.5 * cross_sum;
    if !area.is_finite() || area <= 0.0 {
        return if area.is_finite() {
            Some((area, polygon[0]))
        } else {
            None
        };
    }
    let centroid_x = origin_x + compensated_sum(centroid_x_terms) / (3.0 * cross_sum);
    let centroid_y = origin_y + compensated_sum(centroid_y_terms) / (3.0 * cross_sum);
    let centroid = WorldPoint::new(
        crate::world::Meters::new(centroid_x).ok()?,
        crate::world::Meters::new(centroid_y).ok()?,
    );
    Some((area, centroid))
}

fn compensated_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for value in values {
        let adjusted = value - compensation;
        let next = sum + adjusted;
        compensation = (next - sum) - adjusted;
        sum = next;
    }
    sum
}

fn relative_eq(first: f64, second: f64, tolerance: f64) -> bool {
    first == second || (first - second).abs() <= tolerance * first.abs().max(second.abs())
}

fn bounds_tolerance(bounds: WorldRect) -> f64 {
    BOUNDS_SCALE_TOLERANCE * bounds.width().get().max(bounds.height().get())
}

fn point_is_finite(point: WorldPoint) -> bool {
    point.x().get().is_finite() && point.y().get().is_finite()
}

fn point_in_bounds(point: WorldPoint, bounds: WorldRect, tolerance: f64) -> bool {
    let min = bounds.min();
    let max = bounds.max();
    point.x().get() >= min.x().get() - tolerance
        && point.x().get() <= max.x().get() + tolerance
        && point.y().get() >= min.y().get() - tolerance
        && point.y().get() <= max.y().get() + tolerance
}

fn edge_has_owners(edge: &SpatialEdge, first: CellId, second: CellId) -> bool {
    let owners: Vec<CellId> = edge.cells.iter().flatten().copied().collect();
    owners.len() == 2
        && ((owners[0] == first && owners[1] == second)
            || (owners[0] == second && owners[1] == first))
}

fn edge_matches_polygon(edge: &SpatialEdge, cell: &SpatialCell, tolerance: f64) -> bool {
    (0..cell.polygon.len()).any(|index| {
        let first = cell.polygon[index];
        let second = cell.polygon[(index + 1) % cell.polygon.len()];
        (points_close(edge.start, first, tolerance) && points_close(edge.end, second, tolerance))
            || (points_close(edge.start, second, tolerance)
                && points_close(edge.end, first, tolerance))
    })
}

fn points_close(first: WorldPoint, second: WorldPoint, tolerance: f64) -> bool {
    (first.x().get() - second.x().get()).hypot(first.y().get() - second.y().get()) <= tolerance
}

fn edge_on_bounds(edge: &SpatialEdge, bounds: WorldRect, tolerance: f64) -> bool {
    let min = bounds.min();
    let max = bounds.max();
    let both_x_near = |coordinate: f64| {
        (edge.start.x().get() - coordinate).abs() <= tolerance
            && (edge.end.x().get() - coordinate).abs() <= tolerance
    };
    let both_y_near = |coordinate: f64| {
        (edge.start.y().get() - coordinate).abs() <= tolerance
            && (edge.end.y().get() - coordinate).abs() <= tolerance
    };
    both_x_near(min.x().get())
        || both_x_near(max.x().get())
        || both_y_near(min.y().get())
        || both_y_near(max.y().get())
}
