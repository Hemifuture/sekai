use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::{SpatialCell, SpatialEdge, SpatialSnapshot, SPATIAL_SCHEMA_V1};
use crate::world::{CellId, EdgeId, Meters, WorldPoint, WorldRect};

const SHAPE_RELATIVE_TOLERANCE: f64 = 1.0e-8;
const NORMALIZED_GEOMETRY_TOLERANCE: f64 = 1.0e-9;
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
    /// A polygon side is zero-length within the bounds-scaled tolerance.
    #[error("cell {cell:?} polygon side {side} is degenerate")]
    DegeneratePolygonSide {
        /// The cell containing the degenerate side.
        cell: CellId,
        /// The starting vertex position of the side.
        side: usize,
    },
    /// A polygon intersects or retraces itself.
    #[error("cell {cell:?} polygon sides {first_side} and {second_side} intersect")]
    SelfIntersectingPolygon {
        /// The cell containing the intersection.
        cell: CellId,
        /// One intersecting side.
        first_side: usize,
        /// The other intersecting side.
        second_side: usize,
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
    /// A supplied edge does not represent an owner polygon side exactly once.
    #[error("cell {cell:?} polygon side {side} is represented by {count} edges")]
    PolygonSideEdgeCount {
        /// The cell whose side has the wrong representation count.
        cell: CellId,
        /// The starting vertex position of the side.
        side: usize,
        /// The number of supplied edge representations.
        count: usize,
    },
    /// An internal edge lies on the outer world rectangle.
    #[error("internal edge {edge:?} lies on the world rectangle")]
    InternalEdgeOnBounds {
        /// The misclassified internal edge.
        edge: EdgeId,
    },
    /// The two polygon sides paired by an internal edge have the same orientation.
    #[error("internal edge {edge:?} owners {first:?} and {second:?} have matching orientation")]
    InternalEdgeOrientationMismatch {
        /// The invalid internal edge.
        edge: EdgeId,
        /// One owner of the edge.
        first: CellId,
        /// The other owner of the edge.
        second: CellId,
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
    /// Boundary intervals do not cover one rectangle side exactly once.
    #[error("boundary edges do not cover the rectangle's {side} side exactly once")]
    BoundaryCoverageMismatch {
        /// The rectangle side with a gap, overlap, omission, or duplicate.
        side: &'static str,
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

#[derive(Debug, Clone, Copy)]
struct GeometryScale {
    min_x: f64,
    min_y: f64,
    scale: f64,
    width: f64,
    height: f64,
}

impl GeometryScale {
    fn new(bounds: WorldRect) -> Self {
        let scale = bounds.width().get().max(bounds.height().get());
        Self {
            min_x: bounds.min().x().get(),
            min_y: bounds.min().y().get(),
            scale,
            width: bounds.width().get() / scale,
            height: bounds.height().get() / scale,
        }
    }

    fn normalize(self, point: WorldPoint) -> NormalizedPoint {
        NormalizedPoint {
            x: (point.x().get() - self.min_x) / self.scale,
            y: (point.y().get() - self.min_y) / self.scale,
        }
    }

    fn close(self, first: WorldPoint, second: WorldPoint) -> bool {
        ((first.x().get() - second.x().get()) / self.scale)
            .hypot((first.y().get() - second.y().get()) / self.scale)
            <= NORMALIZED_GEOMETRY_TOLERANCE
    }

    fn contains(self, point: WorldPoint) -> bool {
        let point = self.normalize(point);
        point.x >= -NORMALIZED_GEOMETRY_TOLERANCE
            && point.x <= self.width + NORMALIZED_GEOMETRY_TOLERANCE
            && point.y >= -NORMALIZED_GEOMETRY_TOLERANCE
            && point.y <= self.height + NORMALIZED_GEOMETRY_TOLERANCE
    }

    fn bin(self, point: WorldPoint) -> PointBin {
        let point = self.normalize(point);
        PointBin {
            x: bounded_bin_coordinate(point.x),
            y: bounded_bin_coordinate(point.y),
        }
    }
}

fn bounded_bin_coordinate(coordinate: f64) -> i64 {
    let bin = (coordinate / NORMALIZED_GEOMETRY_TOLERANCE).floor();
    if bin <= i64::MIN as f64 {
        i64::MIN + 1
    } else if bin >= i64::MAX as f64 {
        i64::MAX - 1
    } else {
        bin as i64
    }
}

#[derive(Debug, Clone, Copy)]
struct NormalizedPoint {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PointBin {
    x: i64,
    y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SegmentBinKey {
    owner: CellId,
    first: PointBin,
    second: PointBin,
}

impl SegmentBinKey {
    fn new(owner: CellId, first: PointBin, second: PointBin) -> Self {
        if first <= second {
            Self {
                owner,
                first,
                second,
            }
        } else {
            Self {
                owner,
                first: second,
                second: first,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RectangleSide {
    Bottom,
    Right,
    Top,
    Left,
}

impl RectangleSide {
    fn name(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Right => "right",
            Self::Top => "top",
            Self::Left => "left",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BoundaryInterval {
    start: f64,
    end: f64,
    edge: EdgeId,
}

#[derive(Debug, Clone, Copy)]
struct SideMatch {
    side: usize,
    forward: bool,
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
        let geometry = GeometryScale::new(self.bounds);
        for cell in &self.cells {
            validate_cell(cell, geometry, self.cells.len())?;
        }
        validate_neighbor_symmetry(&self.cells)?;
        validate_total_area(self)?;
        validate_planar_embedding(self, geometry)
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
    geometry: GeometryScale,
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

    let (area, centroid) = polygon_area_and_centroid(&cell.polygon, geometry)
        .ok_or(SpatialValidationError::NonFinitePolygonCalculation { cell: cell.id })?;
    if area <= 0.0 {
        return Err(SpatialValidationError::NonPositivePolygonArea {
            cell: cell.id,
            signed_area: area,
        });
    }
    validate_polygon_simplicity(cell, geometry)?;
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
    if !geometry.contains(cell.site) {
        return Err(SpatialValidationError::SiteOutOfBounds {
            cell: cell.id,
            site: cell.site,
        });
    }
    for (vertex, point) in cell.polygon.iter().copied().enumerate() {
        if !geometry.contains(point) {
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

fn validate_polygon_simplicity(
    cell: &SpatialCell,
    geometry: GeometryScale,
) -> Result<(), SpatialValidationError> {
    let points: Vec<NormalizedPoint> = cell
        .polygon
        .iter()
        .copied()
        .map(|point| geometry.normalize(point))
        .collect();
    let side_count = points.len();

    for side in 0..side_count {
        let start = points[side];
        let end = points[(side + 1) % side_count];
        if distance(start, end) <= NORMALIZED_GEOMETRY_TOLERANCE {
            return Err(SpatialValidationError::DegeneratePolygonSide {
                cell: cell.id,
                side,
            });
        }
    }

    for vertex in 0..side_count {
        let previous_side = (vertex + side_count - 1) % side_count;
        let next_side = vertex;
        let previous = points[previous_side];
        let shared = points[vertex];
        let next = points[(vertex + 1) % side_count];
        if adjacent_sides_overlap(previous, shared, next) {
            return Err(SpatialValidationError::SelfIntersectingPolygon {
                cell: cell.id,
                first_side: previous_side,
                second_side: next_side,
            });
        }
    }

    for first_side in 0..side_count {
        let first_start = points[first_side];
        let first_end = points[(first_side + 1) % side_count];
        for second_side in (first_side + 1)..side_count {
            if (first_side + 1) % side_count == second_side
                || (second_side + 1) % side_count == first_side
            {
                continue;
            }
            let second_start = points[second_side];
            let second_end = points[(second_side + 1) % side_count];
            if segments_intersect(first_start, first_end, second_start, second_end) {
                return Err(SpatialValidationError::SelfIntersectingPolygon {
                    cell: cell.id,
                    first_side,
                    second_side,
                });
            }
        }
    }
    Ok(())
}

fn adjacent_sides_overlap(
    previous: NormalizedPoint,
    shared: NormalizedPoint,
    next: NormalizedPoint,
) -> bool {
    let first_x = previous.x - shared.x;
    let first_y = previous.y - shared.y;
    let second_x = next.x - shared.x;
    let second_y = next.y - shared.y;
    let cross = first_x * second_y - first_y * second_x;
    let scale = first_x.hypot(first_y).max(second_x.hypot(second_y));
    cross.abs() <= NORMALIZED_GEOMETRY_TOLERANCE * scale
        && first_x * second_x + first_y * second_y
            > NORMALIZED_GEOMETRY_TOLERANCE * NORMALIZED_GEOMETRY_TOLERANCE
}

fn segments_intersect(
    first_start: NormalizedPoint,
    first_end: NormalizedPoint,
    second_start: NormalizedPoint,
    second_end: NormalizedPoint,
) -> bool {
    let first_length = distance(first_start, first_end);
    let second_length = distance(second_start, second_end);
    let first_tolerance = NORMALIZED_GEOMETRY_TOLERANCE * first_length;
    let second_tolerance = NORMALIZED_GEOMETRY_TOLERANCE * second_length;
    let first_orientation = orientation(first_start, first_end, second_start);
    let second_orientation = orientation(first_start, first_end, second_end);
    let third_orientation = orientation(second_start, second_end, first_start);
    let fourth_orientation = orientation(second_start, second_end, first_end);

    if ((first_orientation > first_tolerance && second_orientation < -first_tolerance)
        || (first_orientation < -first_tolerance && second_orientation > first_tolerance))
        && ((third_orientation > second_tolerance && fourth_orientation < -second_tolerance)
            || (third_orientation < -second_tolerance && fourth_orientation > second_tolerance))
    {
        return true;
    }

    (first_orientation.abs() <= first_tolerance
        && point_on_segment(second_start, first_start, first_end))
        || (second_orientation.abs() <= first_tolerance
            && point_on_segment(second_end, first_start, first_end))
        || (third_orientation.abs() <= second_tolerance
            && point_on_segment(first_start, second_start, second_end))
        || (fourth_orientation.abs() <= second_tolerance
            && point_on_segment(first_end, second_start, second_end))
}

fn orientation(start: NormalizedPoint, end: NormalizedPoint, point: NormalizedPoint) -> f64 {
    (end.x - start.x) * (point.y - start.y) - (end.y - start.y) * (point.x - start.x)
}

fn point_on_segment(point: NormalizedPoint, start: NormalizedPoint, end: NormalizedPoint) -> bool {
    point.x >= start.x.min(end.x) - NORMALIZED_GEOMETRY_TOLERANCE
        && point.x <= start.x.max(end.x) + NORMALIZED_GEOMETRY_TOLERANCE
        && point.y >= start.y.min(end.y) - NORMALIZED_GEOMETRY_TOLERANCE
        && point.y <= start.y.max(end.y) + NORMALIZED_GEOMETRY_TOLERANCE
}

fn distance(first: NormalizedPoint, second: NormalizedPoint) -> f64 {
    (first.x - second.x).hypot(first.y - second.y)
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

fn validate_planar_embedding(
    snapshot: &SpatialSnapshot,
    geometry: GeometryScale,
) -> Result<(), SpatialValidationError> {
    let declared_pairs = declared_neighbor_pairs(&snapshot.cells);
    let owner_pair_counts =
        validate_and_count_edge_owners(&snapshot.edges, snapshot.cells.len(), &declared_pairs)?;
    for &(first, second) in &declared_pairs {
        let count = owner_pair_counts
            .get(&(first, second))
            .copied()
            .unwrap_or(0);
        if count != 1 {
            return Err(SpatialValidationError::NeighborEdgeCount {
                first,
                second,
                count,
            });
        }
    }

    let side_index = build_side_index(&snapshot.cells, geometry);
    let mut side_counts: Vec<Vec<usize>> = snapshot
        .cells
        .iter()
        .map(|cell| vec![0; cell.polygon.len()])
        .collect();
    let mut boundary_intervals = BTreeMap::<RectangleSide, Vec<BoundaryInterval>>::new();

    for edge in &snapshot.edges {
        validate_edge_length(edge)?;
        match edge.cells {
            [Some(owner), None] | [None, Some(owner)] => {
                let side_match =
                    match_owner_side(edge, owner, &snapshot.cells, &side_index, geometry)?;
                side_counts[owner.raw() as usize][side_match.side] += 1;
                let (side, interval) = classify_boundary_edge(edge, geometry)
                    .ok_or(SpatialValidationError::BoundaryEdgeOffBounds { edge: edge.id })?;
                boundary_intervals.entry(side).or_default().push(interval);
            }
            [Some(first), Some(second)] => {
                let first_match =
                    match_owner_side(edge, first, &snapshot.cells, &side_index, geometry)?;
                let second_match =
                    match_owner_side(edge, second, &snapshot.cells, &side_index, geometry)?;
                side_counts[first.raw() as usize][first_match.side] += 1;
                side_counts[second.raw() as usize][second_match.side] += 1;

                if classify_boundary_edge(edge, geometry).is_some() {
                    return Err(SpatialValidationError::InternalEdgeOnBounds { edge: edge.id });
                }
                if first_match.forward == second_match.forward {
                    return Err(SpatialValidationError::InternalEdgeOrientationMismatch {
                        edge: edge.id,
                        first,
                        second,
                    });
                }
            }
            [None, None] => unreachable!("edge ownership was validated before geometry"),
        }
    }

    for (cell_index, counts) in side_counts.iter().enumerate() {
        for (side, &count) in counts.iter().enumerate() {
            if count != 1 {
                return Err(SpatialValidationError::PolygonSideEdgeCount {
                    cell: CellId::from_raw(cell_index as u32),
                    side,
                    count,
                });
            }
        }
    }

    validate_boundary_coverage(boundary_intervals, geometry)
}

fn validate_and_count_edge_owners(
    edges: &[SpatialEdge],
    cell_count: usize,
    declared_pairs: &BTreeSet<(CellId, CellId)>,
) -> Result<BTreeMap<(CellId, CellId), usize>, SpatialValidationError> {
    let mut owner_pair_counts = BTreeMap::new();
    for edge in edges {
        match edge.cells {
            [None, None] => {
                return Err(SpatialValidationError::InvalidEdgeOwnership {
                    edge: edge.id,
                    owner_count: 0,
                });
            }
            [Some(owner), None] | [None, Some(owner)] => {
                validate_owner(edge.id, owner, cell_count)?;
            }
            [Some(first), Some(second)] => {
                validate_owner(edge.id, first, cell_count)?;
                validate_owner(edge.id, second, cell_count)?;
                if first == second {
                    return Err(SpatialValidationError::DuplicateEdgeOwner {
                        edge: edge.id,
                        cell: first,
                    });
                }
                let pair = normalized_pair(first, second);
                if !declared_pairs.contains(&pair) {
                    return Err(SpatialValidationError::InternalEdgeWithoutNeighbors {
                        edge: edge.id,
                        first,
                        second,
                    });
                }
                *owner_pair_counts.entry(pair).or_insert(0) += 1;
            }
        }
    }
    Ok(owner_pair_counts)
}

fn declared_neighbor_pairs(cells: &[SpatialCell]) -> BTreeSet<(CellId, CellId)> {
    let mut pairs = BTreeSet::new();
    for cell in cells {
        for &neighbor in &cell.neighbors {
            pairs.insert(normalized_pair(cell.id, neighbor));
        }
    }
    pairs
}

fn normalized_pair(first: CellId, second: CellId) -> (CellId, CellId) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn validate_owner(
    edge: EdgeId,
    owner: CellId,
    cell_count: usize,
) -> Result<(), SpatialValidationError> {
    if owner.raw() as usize >= cell_count {
        return Err(SpatialValidationError::InvalidEdgeOwner { edge, cell: owner });
    }
    Ok(())
}

fn validate_edge_length(edge: &SpatialEdge) -> Result<(), SpatialValidationError> {
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
    Ok(())
}

fn build_side_index(
    cells: &[SpatialCell],
    geometry: GeometryScale,
) -> BTreeMap<SegmentBinKey, Vec<usize>> {
    let mut index = BTreeMap::<SegmentBinKey, Vec<usize>>::new();
    for cell in cells {
        for side in 0..cell.polygon.len() {
            let start = geometry.bin(cell.polygon[side]);
            let end = geometry.bin(cell.polygon[(side + 1) % cell.polygon.len()]);
            index
                .entry(SegmentBinKey::new(cell.id, start, end))
                .or_default()
                .push(side);
        }
    }
    index
}

fn match_owner_side(
    edge: &SpatialEdge,
    owner: CellId,
    cells: &[SpatialCell],
    side_index: &BTreeMap<SegmentBinKey, Vec<usize>>,
    geometry: GeometryScale,
) -> Result<SideMatch, SpatialValidationError> {
    let start_bin = geometry.bin(edge.start);
    let end_bin = geometry.bin(edge.end);
    let cell = &cells[owner.raw() as usize];
    let mut found: Option<SideMatch> = None;

    for start_x in (start_bin.x - 1)..=(start_bin.x + 1) {
        for start_y in (start_bin.y - 1)..=(start_bin.y + 1) {
            for end_x in (end_bin.x - 1)..=(end_bin.x + 1) {
                for end_y in (end_bin.y - 1)..=(end_bin.y + 1) {
                    let key = SegmentBinKey::new(
                        owner,
                        PointBin {
                            x: start_x,
                            y: start_y,
                        },
                        PointBin { x: end_x, y: end_y },
                    );
                    let Some(candidates) = side_index.get(&key) else {
                        continue;
                    };
                    for &side in candidates {
                        let first = cell.polygon[side];
                        let second = cell.polygon[(side + 1) % cell.polygon.len()];
                        let forward =
                            geometry.close(edge.start, first) && geometry.close(edge.end, second);
                        let reverse =
                            geometry.close(edge.start, second) && geometry.close(edge.end, first);
                        if !forward && !reverse {
                            continue;
                        }
                        if let Some(previous) = found {
                            if previous.side != side {
                                return Err(SpatialValidationError::EdgeNotOnCellPolygon {
                                    edge: edge.id,
                                    cell: owner,
                                });
                            }
                        } else {
                            found = Some(SideMatch { side, forward });
                        }
                    }
                }
            }
        }
    }

    found.ok_or(SpatialValidationError::EdgeNotOnCellPolygon {
        edge: edge.id,
        cell: owner,
    })
}

fn classify_boundary_edge(
    edge: &SpatialEdge,
    geometry: GeometryScale,
) -> Option<(RectangleSide, BoundaryInterval)> {
    let start = geometry.normalize(edge.start);
    let end = geometry.normalize(edge.end);
    let candidates = [
        (
            RectangleSide::Bottom,
            start.y.abs().max(end.y.abs()),
            start.x.min(end.x),
            start.x.max(end.x),
        ),
        (
            RectangleSide::Right,
            (start.x - geometry.width)
                .abs()
                .max((end.x - geometry.width).abs()),
            start.y.min(end.y),
            start.y.max(end.y),
        ),
        (
            RectangleSide::Top,
            (start.y - geometry.height)
                .abs()
                .max((end.y - geometry.height).abs()),
            start.x.min(end.x),
            start.x.max(end.x),
        ),
        (
            RectangleSide::Left,
            start.x.abs().max(end.x.abs()),
            start.y.min(end.y),
            start.y.max(end.y),
        ),
    ];
    let mut best: Option<(RectangleSide, f64, f64, f64)> = None;
    for candidate in candidates {
        if candidate.1 > NORMALIZED_GEOMETRY_TOLERANCE {
            continue;
        }
        if best.is_none_or(|current| candidate.1 < current.1) {
            best = Some(candidate);
        }
    }
    best.map(|(side, _, start, end)| {
        (
            side,
            BoundaryInterval {
                start,
                end,
                edge: edge.id,
            },
        )
    })
}

fn validate_boundary_coverage(
    mut intervals: BTreeMap<RectangleSide, Vec<BoundaryInterval>>,
    geometry: GeometryScale,
) -> Result<(), SpatialValidationError> {
    for (side, extent) in [
        (RectangleSide::Bottom, geometry.width),
        (RectangleSide::Right, geometry.height),
        (RectangleSide::Top, geometry.width),
        (RectangleSide::Left, geometry.height),
    ] {
        let Some(side_intervals) = intervals.get_mut(&side) else {
            return Err(SpatialValidationError::BoundaryCoverageMismatch { side: side.name() });
        };
        side_intervals.sort_by(|first, second| {
            first
                .start
                .total_cmp(&second.start)
                .then_with(|| first.end.total_cmp(&second.end))
                .then_with(|| first.edge.cmp(&second.edge))
        });
        let mut cursor = 0.0;
        for interval in side_intervals {
            if (interval.start - cursor).abs() > NORMALIZED_GEOMETRY_TOLERANCE
                || interval.end <= interval.start + NORMALIZED_GEOMETRY_TOLERANCE
            {
                return Err(SpatialValidationError::BoundaryCoverageMismatch { side: side.name() });
            }
            cursor = interval.end;
        }
        if (cursor - extent).abs() > NORMALIZED_GEOMETRY_TOLERANCE {
            return Err(SpatialValidationError::BoundaryCoverageMismatch { side: side.name() });
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

fn polygon_area_and_centroid(
    polygon: &[WorldPoint],
    geometry: GeometryScale,
) -> Option<(f64, WorldPoint)> {
    let origin = polygon[0];
    let mut cross_terms = Vec::with_capacity(polygon.len());
    let mut centroid_x_terms = Vec::with_capacity(polygon.len());
    let mut centroid_y_terms = Vec::with_capacity(polygon.len());

    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let first_x = (first.x().get() - origin.x().get()) / geometry.scale;
        let first_y = (first.y().get() - origin.y().get()) / geometry.scale;
        let second_x = (second.x().get() - origin.x().get()) / geometry.scale;
        let second_y = (second.y().get() - origin.y().get()) / geometry.scale;
        let cross = first_x * second_y - second_x * first_y;
        let centroid_x_term = (first_x + second_x) * cross;
        let centroid_y_term = (first_y + second_y) * cross;
        if !cross.is_finite() || !centroid_x_term.is_finite() || !centroid_y_term.is_finite() {
            return None;
        }
        cross_terms.push(cross);
        centroid_x_terms.push(centroid_x_term);
        centroid_y_terms.push(centroid_y_term);
    }

    let cross_sum = compensated_sum(cross_terms);
    if !cross_sum.is_finite() {
        return None;
    }
    let scaled_area = 0.5 * cross_sum;
    let area = scaled_area * geometry.scale * geometry.scale;
    if !area.is_finite() || area <= 0.0 {
        return if area.is_finite() {
            Some((area, polygon[0]))
        } else {
            None
        };
    }

    let centroid_denominator = 3.0 * cross_sum;
    let centroid_x = origin.x().get()
        + compensated_sum(centroid_x_terms) / centroid_denominator * geometry.scale;
    let centroid_y = origin.y().get()
        + compensated_sum(centroid_y_terms) / centroid_denominator * geometry.scale;
    let centroid = WorldPoint::new(Meters::new(centroid_x).ok()?, Meters::new(centroid_y).ok()?);
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

fn point_is_finite(point: WorldPoint) -> bool {
    point.x().get().is_finite() && point.y().get().is_finite()
}
