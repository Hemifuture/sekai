//! Disposable, seam-safe two-dimensional geometry derived from a spherical surface.

use std::f64::consts::PI;

use thiserror::Error;

use super::{
    ProjectionBounds, ProjectionPoint, SphericalPresentationSource, SphericalProjection,
    SphericalProjectionError,
};
use crate::world::spatial::{SphericalSurfaceSnapshot, SurfaceRef, SurfaceRefError, UnitVector3};
use crate::world::{
    CellId, EdgeId, MAX_SPHERICAL_CELL_BOUNDARY_DEGREE, MAX_SPHERICAL_CELL_COUNT,
    MAX_SPHERICAL_EDGE_COUNT,
};

const MAX_CLIPPED_TRIANGLES_PER_FAN_TRIANGLE: usize = 4;
const TRIANGLE_VERTEX_COUNT: usize = 3;
const ARC_BISECTION_ITERATIONS: usize = 64;
const POLE_HORIZONTAL_EPSILON: f64 = 32.0 * f64::EPSILON;
const SPAN_EPSILON: f64 = 2.0e-12;

const DEFAULT_CELL_BUDGET: usize = MAX_SPHERICAL_CELL_COUNT as usize;
const DEFAULT_TRIANGLE_BUDGET: usize = DEFAULT_CELL_BUDGET
    * MAX_SPHERICAL_CELL_BOUNDARY_DEGREE
    * MAX_CLIPPED_TRIANGLES_PER_FAN_TRIANGLE;
const DEFAULT_VERTEX_BUDGET: usize = DEFAULT_TRIANGLE_BUDGET * TRIANGLE_VERTEX_COUNT;
const DEFAULT_INDEX_BUDGET: usize = DEFAULT_VERTEX_BUDGET;
const DEFAULT_EDGE_SEGMENT_BUDGET: usize = MAX_SPHERICAL_EDGE_COUNT as usize * 2;

/// One projected display vertex carrying its authoritative cell identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedMapVertex {
    position: ProjectionPoint,
    cell: CellId,
}

impl ProjectedMapVertex {
    /// Returns the projection-local position.
    pub const fn position(self) -> ProjectionPoint {
        self.position
    }

    /// Returns the authoritative cell represented by this display vertex.
    pub const fn cell(self) -> CellId {
        self.cell
    }
}

/// One seam-safe projected fragment of an authoritative spherical edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedEdgeSegment {
    start: ProjectionPoint,
    end: ProjectionPoint,
    edge: EdgeId,
}

impl ProjectedEdgeSegment {
    /// Returns the projected start point.
    pub const fn start(self) -> ProjectionPoint {
        self.start
    }

    /// Returns the projected end point.
    pub const fn end(self) -> ProjectionPoint {
        self.end
    }

    /// Returns the authoritative edge represented by this display fragment.
    pub const fn edge(self) -> EdgeId {
        self.edge
    }
}

/// Explicit allocation limits for one prepared spherical map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SphericalMeshBudgets {
    cells: usize,
    vertices: usize,
    indices: usize,
    edge_segments: usize,
}

impl SphericalMeshBudgets {
    /// Default limits covering every schema-V1 authoritative spherical surface.
    pub const DEFAULT: Self = Self::new(
        DEFAULT_CELL_BUDGET,
        DEFAULT_VERTEX_BUDGET,
        DEFAULT_INDEX_BUDGET,
        DEFAULT_EDGE_SEGMENT_BUDGET,
    );

    /// Creates an explicit set of presentation-only allocation limits.
    pub const fn new(cells: usize, vertices: usize, indices: usize, edge_segments: usize) -> Self {
        Self {
            cells,
            vertices,
            indices,
            edge_segments,
        }
    }

    /// Returns the cell limit.
    pub const fn cells(self) -> usize {
        self.cells
    }

    /// Returns the projected-vertex limit.
    pub const fn vertices(self) -> usize {
        self.vertices
    }

    /// Returns the projected-index limit.
    pub const fn indices(self) -> usize {
        self.indices
    }

    /// Returns the projected edge-fragment limit.
    pub const fn edge_segments(self) -> usize {
        self.edge_segments
    }

    /// Checks public output cardinalities against both budgets and `u32` storage.
    pub fn check_counts(
        self,
        cells: usize,
        vertices: usize,
        indices: usize,
        edge_segments: usize,
    ) -> Result<(), SphericalMeshError> {
        check_budget(cells, self.cells, CountKind::Cell)?;
        check_budget(vertices, self.vertices, CountKind::Vertex)?;
        check_budget(indices, self.indices, CountKind::Index)?;
        check_budget(edge_segments, self.edge_segments, CountKind::EdgeSegment)?;
        checked_u32(cells, "cell count")?;
        checked_u32(vertices, "projected vertex count")?;
        checked_u32(indices, "projected index count")?;
        checked_u32(edge_segments, "projected edge-segment count")?;
        Ok(())
    }
}

impl Default for SphericalMeshBudgets {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Failures while deriving disposable projected geometry from an authoritative surface.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalMeshError {
    /// The presentation source references a different authoritative surface.
    #[error("spherical presentation source references {source_ref:?}, not {surface:?}")]
    SourceSurfaceMismatch {
        /// The source-bound surface identity.
        source_ref: SurfaceRef,
        /// The supplied surface identity.
        surface: SurfaceRef,
    },
    /// The supplied surface cannot produce a valid authoritative identity.
    #[error("invalid spherical surface: {0}")]
    InvalidSurface(#[from] SurfaceRefError),
    /// Projection mathematics rejected an input or derived coordinate.
    #[error("spherical projection failed: {0}")]
    Projection(#[from] SphericalProjectionError),
    /// The authoritative cell cardinality exceeds the configured limit.
    #[error("spherical map cell count {actual} exceeds budget {max}")]
    CellBudgetExceeded { actual: usize, max: usize },
    /// The projected display-vertex cardinality exceeds the configured limit.
    #[error("spherical map vertex count {actual} exceeds budget {max}")]
    VertexBudgetExceeded { actual: usize, max: usize },
    /// The projected index cardinality exceeds the configured limit.
    #[error("spherical map index count {actual} exceeds budget {max}")]
    IndexBudgetExceeded { actual: usize, max: usize },
    /// The projected authoritative-edge fragment count exceeds the configured limit.
    #[error("spherical map edge-segment count {actual} exceeds budget {max}")]
    EdgeSegmentBudgetExceeded { actual: usize, max: usize },
    /// A checked count or index cannot be represented by its storage type.
    #[error("integer overflow while computing {context}")]
    IntegerOverflow { context: &'static str },
    /// A cell fan produced a non-finite, degenerate, or cross-map display triangle.
    #[error("cell {cell:?} produced invalid projected geometry")]
    InvalidCellGeometry { cell: CellId },
    /// An authoritative edge produced a non-finite, degenerate, or cross-map fragment.
    #[error("edge {edge:?} produced invalid projected geometry")]
    InvalidEdgeGeometry { edge: EdgeId },
    /// A cell was not represented by any usable display triangle.
    #[error("cell {cell:?} produced no projected triangles")]
    MissingCellGeometry { cell: CellId },
}

/// A bounded, source-bound projected map derived from one authoritative surface.
#[derive(Debug, Clone)]
pub struct PreparedProjectedMap {
    source: SphericalPresentationSource,
    projection: SphericalProjection,
    bounds: ProjectionBounds,
    cell_count: usize,
    vertices: Vec<ProjectedMapVertex>,
    indices: Vec<u32>,
    edge_segments: Vec<ProjectedEdgeSegment>,
}

impl PreparedProjectedMap {
    /// Builds cell and edge display fragments without changing spherical semantics.
    pub fn build(
        source: SphericalPresentationSource,
        surface: &SphericalSurfaceSnapshot,
        projection: SphericalProjection,
        budgets: SphericalMeshBudgets,
    ) -> Result<Self, SphericalMeshError> {
        let surface_ref = SurfaceRef::try_for_spherical(surface)?;
        if source.surface_ref() != surface_ref {
            return Err(SphericalMeshError::SourceSurfaceMismatch {
                source_ref: source.surface_ref(),
                surface: surface_ref,
            });
        }
        budgets.check_counts(surface.cells().len(), 0, 0, 0)?;

        let bounds = projection.bounds();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for cell in surface.cells() {
            let triangle_count_before = indices.len() / TRIANGLE_VERTEX_COUNT;
            for side in 0..cell.boundary_vertices.len() {
                let first = surface
                    .vertex(cell.boundary_vertices[side])
                    .expect("validated cell boundary vertex must exist")
                    .position;
                let second = surface
                    .vertex(cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()])
                    .expect("validated cell boundary vertex must exist")
                    .position;
                let fan_polygon = angular_fan_polygon(
                    [cell.centroid, first, second],
                    projection.central_meridian(),
                );
                for fragment in
                    split_polygon_at_seam(&fan_polygon, projection.central_meridian(), cell.id)?
                {
                    triangulate_fragment(
                        &fragment,
                        cell.id,
                        projection,
                        bounds,
                        budgets,
                        &mut vertices,
                        &mut indices,
                    )?;
                }
            }
            if indices.len() / TRIANGLE_VERTEX_COUNT == triangle_count_before {
                return Err(SphericalMeshError::MissingCellGeometry { cell: cell.id });
            }
        }

        let mut edge_segments = Vec::new();
        for edge in surface.edges() {
            let endpoints = edge.vertices.map(|vertex| {
                surface
                    .vertex(vertex)
                    .expect("validated edge vertex must exist")
                    .position
            });
            append_edge_fragments(
                endpoints,
                edge.id,
                projection,
                bounds,
                budgets,
                &mut edge_segments,
            )?;
        }
        budgets.check_counts(
            surface.cells().len(),
            vertices.len(),
            indices.len(),
            edge_segments.len(),
        )?;

        Ok(Self {
            source,
            projection,
            bounds,
            cell_count: surface.cells().len(),
            vertices,
            indices,
            edge_segments,
        })
    }

    /// Returns the immutable authoritative build identity.
    pub const fn source(&self) -> &SphericalPresentationSource {
        &self.source
    }

    /// Returns the configured projection used to derive this map.
    pub const fn projection(&self) -> SphericalProjection {
        self.projection
    }

    /// Returns the natural extent for the active projection.
    pub const fn bounds(&self) -> ProjectionBounds {
        self.bounds
    }

    /// Returns the authoritative cell cardinality.
    pub const fn cell_count(&self) -> usize {
        self.cell_count
    }

    /// Returns projected vertices in stable cell/fan/fragment order.
    pub fn vertices(&self) -> &[ProjectedMapVertex] {
        &self.vertices
    }

    /// Returns triangle indices in stable cell/fan/fragment order.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Returns seam-safe projected fragments in stable authoritative edge order.
    pub fn edge_segments(&self) -> &[ProjectedEdgeSegment] {
        &self.edge_segments
    }
}

#[derive(Debug, Clone, Copy)]
struct AngularVertex {
    direction: UnitVector3,
    latitude: f64,
    longitude: f64,
}

#[derive(Debug, Clone, Copy)]
enum CountKind {
    Cell,
    Vertex,
    Index,
    EdgeSegment,
}

fn check_budget(actual: usize, max: usize, kind: CountKind) -> Result<(), SphericalMeshError> {
    if actual <= max {
        return Ok(());
    }
    Err(match kind {
        CountKind::Cell => SphericalMeshError::CellBudgetExceeded { actual, max },
        CountKind::Vertex => SphericalMeshError::VertexBudgetExceeded { actual, max },
        CountKind::Index => SphericalMeshError::IndexBudgetExceeded { actual, max },
        CountKind::EdgeSegment => SphericalMeshError::EdgeSegmentBudgetExceeded { actual, max },
    })
}

fn checked_u32(value: usize, context: &'static str) -> Result<u32, SphericalMeshError> {
    u32::try_from(value).map_err(|_| SphericalMeshError::IntegerOverflow { context })
}

fn checked_add(
    left: usize,
    right: usize,
    context: &'static str,
) -> Result<usize, SphericalMeshError> {
    left.checked_add(right)
        .ok_or(SphericalMeshError::IntegerOverflow { context })
}

fn angular_fan_polygon(
    directions: [UnitVector3; TRIANGLE_VERTEX_COUNT],
    central_meridian: f64,
) -> Vec<AngularVertex> {
    // A pole is one spherical point but a whole horizontal projection boundary.
    // Duplicate only that disposable display vertex at the adjacent arc longitudes
    // so the polar fan fills to the outline without inventing a semantic vertex.
    let pole = directions.iter().position(|direction| is_pole(*direction));
    let reference_index = directions
        .iter()
        .position(|direction| !is_pole(*direction))
        .unwrap_or(0);
    let reference = relative_longitude(directions[reference_index], central_meridian);
    let vertices = directions.map(|direction| AngularVertex {
        direction,
        latitude: latitude(direction),
        longitude: unwrap_near(relative_longitude(direction, central_meridian), reference),
    });
    match pole {
        None => vertices.to_vec(),
        Some(0) => vec![
            pole_copy(vertices[0], vertices[1].longitude),
            vertices[1],
            vertices[2],
            pole_copy(vertices[0], vertices[2].longitude),
        ],
        Some(1) => vec![
            vertices[0],
            pole_copy(vertices[1], vertices[0].longitude),
            pole_copy(vertices[1], vertices[2].longitude),
            vertices[2],
        ],
        Some(2) => vec![
            vertices[0],
            vertices[1],
            pole_copy(vertices[2], vertices[1].longitude),
            pole_copy(vertices[2], vertices[0].longitude),
        ],
        Some(_) => unreachable!("a triangle has exactly three vertices"),
    }
}

fn is_pole(direction: UnitVector3) -> bool {
    let [x, y, _] = direction.components();
    x == 0.0 && y == 0.0
}

fn pole_copy(mut vertex: AngularVertex, longitude: f64) -> AngularVertex {
    vertex.longitude = longitude;
    vertex
}

fn angular_edge(directions: [UnitVector3; 2], central_meridian: f64) -> [AngularVertex; 2] {
    let first_longitude = relative_longitude(directions[0], central_meridian);
    [
        AngularVertex {
            direction: directions[0],
            latitude: latitude(directions[0]),
            longitude: first_longitude,
        },
        AngularVertex {
            direction: directions[1],
            latitude: latitude(directions[1]),
            longitude: unwrap_near(
                relative_longitude(directions[1], central_meridian),
                first_longitude,
            ),
        },
    ]
}

fn latitude(direction: UnitVector3) -> f64 {
    direction.components()[2].asin()
}

fn relative_longitude(direction: UnitVector3, central_meridian: f64) -> f64 {
    let [x, y, _] = direction.components();
    wrap_radians(y.atan2(x) - central_meridian)
}

fn continuous_relative_longitude(
    direction: UnitVector3,
    central_meridian: f64,
    reference: f64,
) -> f64 {
    let [x, y, _] = direction.components();
    if x.hypot(y) <= POLE_HORIZONTAL_EPSILON {
        reference
    } else {
        unwrap_near(relative_longitude(direction, central_meridian), reference)
    }
}

fn wrap_radians(angle: f64) -> f64 {
    (angle + PI).rem_euclid(2.0 * PI) - PI
}

fn unwrap_near(longitude: f64, reference: f64) -> f64 {
    longitude + ((reference - longitude) / (2.0 * PI)).round() * (2.0 * PI)
}

fn split_polygon_at_seam(
    polygon: &[AngularVertex],
    central_meridian: f64,
    cell: CellId,
) -> Result<Vec<Vec<AngularVertex>>, SphericalMeshError> {
    let min = polygon
        .iter()
        .map(|vertex| vertex.longitude)
        .fold(f64::INFINITY, f64::min);
    let max = polygon
        .iter()
        .map(|vertex| vertex.longitude)
        .fold(f64::NEG_INFINITY, f64::max);
    if min >= -PI && max <= PI {
        return Ok(vec![polygon.to_vec()]);
    }

    let (seam, first_keeps_less, second_shift) = if max > PI {
        (PI, true, -2.0 * PI)
    } else if min < -PI {
        (-PI, false, 2.0 * PI)
    } else {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
    };
    let first = clip_polygon(polygon, seam, first_keeps_less, central_meridian, cell)?;
    let mut second = clip_polygon(polygon, seam, !first_keeps_less, central_meridian, cell)?;
    for vertex in &mut second {
        vertex.longitude += second_shift;
    }

    Ok([first, second]
        .into_iter()
        .filter(|fragment| fragment.len() >= TRIANGLE_VERTEX_COUNT)
        .collect())
}

fn clip_polygon(
    polygon: &[AngularVertex],
    seam: f64,
    keep_less: bool,
    central_meridian: f64,
    cell: CellId,
) -> Result<Vec<AngularVertex>, SphericalMeshError> {
    let is_inside = |vertex: AngularVertex| {
        if keep_less {
            vertex.longitude <= seam
        } else {
            vertex.longitude >= seam
        }
    };
    let mut output = Vec::with_capacity(polygon.len() + 1);
    let mut previous = *polygon
        .last()
        .ok_or(SphericalMeshError::InvalidCellGeometry { cell })?;
    let mut previous_inside = is_inside(previous);
    for &current in polygon {
        let current_inside = is_inside(current);
        if previous_inside != current_inside {
            output.push(
                arc_intersection(previous, current, seam, central_meridian)
                    .ok_or(SphericalMeshError::InvalidCellGeometry { cell })?,
            );
        }
        if current_inside {
            output.push(current);
        }
        previous = current;
        previous_inside = current_inside;
    }
    deduplicate_polygon(&mut output);
    Ok(output)
}

fn deduplicate_polygon(polygon: &mut Vec<AngularVertex>) {
    polygon.dedup_by(|left, right| same_angular_vertex(*left, *right));
    if polygon.len() > 1 && same_angular_vertex(polygon[0], *polygon.last().unwrap()) {
        polygon.pop();
    }
}

fn same_angular_vertex(left: AngularVertex, right: AngularVertex) -> bool {
    left.direction.dot(right.direction) >= 1.0 - 8.0 * f64::EPSILON
        && (left.longitude - right.longitude).abs() <= 8.0 * f64::EPSILON
}

fn arc_intersection(
    start: AngularVertex,
    end: AngularVertex,
    seam: f64,
    central_meridian: f64,
) -> Option<AngularVertex> {
    if start.longitude == seam {
        return Some(AngularVertex {
            longitude: seam,
            ..start
        });
    }
    if end.longitude == seam {
        return Some(AngularVertex {
            longitude: seam,
            ..end
        });
    }
    if (start.longitude - seam).is_sign_positive() == (end.longitude - seam).is_sign_positive() {
        return None;
    }

    let mut low_t = 0.0;
    let mut high_t = 1.0;
    let start_is_low = start.longitude < seam;
    for _ in 0..ARC_BISECTION_ITERATIONS {
        let mid_t = (low_t + high_t) * 0.5;
        let direction = minor_arc_point(start.direction, end.direction, mid_t)?;
        // Keep longitude on the same continuous copy throughout bisection. Raw
        // wrapped longitude alone would jump by 2*pi at the very seam we seek.
        let expected = start.longitude + (end.longitude - start.longitude) * mid_t;
        let longitude = continuous_relative_longitude(direction, central_meridian, expected);
        if (longitude < seam) == start_is_low {
            low_t = mid_t;
        } else {
            high_t = mid_t;
        }
    }
    let direction = minor_arc_point(start.direction, end.direction, (low_t + high_t) * 0.5)?;
    Some(AngularVertex {
        direction,
        latitude: latitude(direction),
        longitude: seam,
    })
}

fn minor_arc_point(start: UnitVector3, end: UnitVector3, amount: f64) -> Option<UnitVector3> {
    let start = start.components();
    let end = end.components();
    UnitVector3::new(
        start[0] * (1.0 - amount) + end[0] * amount,
        start[1] * (1.0 - amount) + end[1] * amount,
        start[2] * (1.0 - amount) + end[2] * amount,
    )
    .ok()
}

fn triangulate_fragment(
    fragment: &[AngularVertex],
    cell: CellId,
    projection: SphericalProjection,
    bounds: ProjectionBounds,
    budgets: SphericalMeshBudgets,
    vertices: &mut Vec<ProjectedMapVertex>,
    indices: &mut Vec<u32>,
) -> Result<(), SphericalMeshError> {
    for pair in fragment[1..].windows(2) {
        append_triangle(
            [fragment[0], pair[0], pair[1]],
            cell,
            projection,
            bounds,
            budgets,
            vertices,
            indices,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_triangle(
    triangle: [AngularVertex; TRIANGLE_VERTEX_COUNT],
    cell: CellId,
    projection: SphericalProjection,
    bounds: ProjectionBounds,
    budgets: SphericalMeshBudgets,
    vertices: &mut Vec<ProjectedMapVertex>,
    indices: &mut Vec<u32>,
) -> Result<(), SphericalMeshError> {
    let points = triangle.map(|vertex| project_angular(vertex, projection));
    if points.iter().any(Result::is_err) {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
    }
    let mut points = points.map(Result::unwrap);
    if points
        .iter()
        .any(|point| !point.x().is_finite() || !point.y().is_finite())
    {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
    }
    let signed_area = signed_area(points);
    if !signed_area.is_finite() || signed_area == 0.0 {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
    }
    if signed_area < 0.0 {
        points.swap(1, 2);
    }
    let half_width = (bounds.max_x() - bounds.min_x()) * 0.5;
    if triangle_x_span(points) > half_width + SPAN_EPSILON {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
    }

    let next_vertices = checked_add(
        vertices.len(),
        TRIANGLE_VERTEX_COUNT,
        "projected vertex count",
    )?;
    let next_indices = checked_add(
        indices.len(),
        TRIANGLE_VERTEX_COUNT,
        "projected index count",
    )?;
    budgets.check_counts(0, next_vertices, next_indices, 0)?;
    let base = checked_u32(vertices.len(), "projected vertex index")?;
    vertices.extend(points.map(|position| ProjectedMapVertex { position, cell }));
    indices.extend([
        base,
        base.checked_add(1)
            .ok_or(SphericalMeshError::IntegerOverflow {
                context: "projected triangle index",
            })?,
        base.checked_add(2)
            .ok_or(SphericalMeshError::IntegerOverflow {
                context: "projected triangle index",
            })?,
    ]);
    Ok(())
}

fn signed_area(points: [ProjectionPoint; TRIANGLE_VERTEX_COUNT]) -> f64 {
    (points[1].x() - points[0].x()) * (points[2].y() - points[0].y())
        - (points[1].y() - points[0].y()) * (points[2].x() - points[0].x())
}

fn triangle_x_span(points: [ProjectionPoint; TRIANGLE_VERTEX_COUNT]) -> f64 {
    let min = points
        .iter()
        .map(|point| point.x())
        .fold(f64::INFINITY, f64::min);
    let max = points
        .iter()
        .map(|point| point.x())
        .fold(f64::NEG_INFINITY, f64::max);
    max - min
}

fn project_angular(
    vertex: AngularVertex,
    projection: SphericalProjection,
) -> Result<ProjectionPoint, SphericalProjectionError> {
    projection.forward_latitude_relative_longitude(vertex.latitude, vertex.longitude)
}

fn append_edge_fragments(
    endpoints: [UnitVector3; 2],
    edge: EdgeId,
    projection: SphericalProjection,
    bounds: ProjectionBounds,
    budgets: SphericalMeshBudgets,
    output: &mut Vec<ProjectedEdgeSegment>,
) -> Result<(), SphericalMeshError> {
    let segment_count_before = output.len();
    let [start, end] = angular_edge(endpoints, projection.central_meridian());
    if end.longitude > PI {
        let intersection = arc_intersection(start, end, PI, projection.central_meridian())
            .ok_or(SphericalMeshError::InvalidEdgeGeometry { edge })?;
        append_edge_segment(
            start,
            intersection,
            edge,
            projection,
            bounds,
            budgets,
            output,
        )?;
        append_edge_segment(
            shifted_longitude(intersection, -2.0 * PI),
            shifted_longitude(end, -2.0 * PI),
            edge,
            projection,
            bounds,
            budgets,
            output,
        )?;
    } else if end.longitude < -PI {
        let intersection = arc_intersection(start, end, -PI, projection.central_meridian())
            .ok_or(SphericalMeshError::InvalidEdgeGeometry { edge })?;
        append_edge_segment(
            start,
            intersection,
            edge,
            projection,
            bounds,
            budgets,
            output,
        )?;
        append_edge_segment(
            shifted_longitude(intersection, 2.0 * PI),
            shifted_longitude(end, 2.0 * PI),
            edge,
            projection,
            bounds,
            budgets,
            output,
        )?;
    } else {
        append_edge_segment(start, end, edge, projection, bounds, budgets, output)?;
    }
    if output.len() == segment_count_before {
        return Err(SphericalMeshError::InvalidEdgeGeometry { edge });
    }
    Ok(())
}

fn shifted_longitude(mut vertex: AngularVertex, shift: f64) -> AngularVertex {
    vertex.longitude += shift;
    vertex
}

fn append_edge_segment(
    start: AngularVertex,
    end: AngularVertex,
    edge: EdgeId,
    projection: SphericalProjection,
    bounds: ProjectionBounds,
    budgets: SphericalMeshBudgets,
    output: &mut Vec<ProjectedEdgeSegment>,
) -> Result<(), SphericalMeshError> {
    if start.direction == end.direction && start.longitude == end.longitude {
        return Ok(());
    }
    let start = project_angular(start, projection)
        .map_err(|_| SphericalMeshError::InvalidEdgeGeometry { edge })?;
    let end = project_angular(end, projection)
        .map_err(|_| SphericalMeshError::InvalidEdgeGeometry { edge })?;
    let finite = [start.x(), start.y(), end.x(), end.y()]
        .into_iter()
        .all(f64::is_finite);
    let half_width = (bounds.max_x() - bounds.min_x()) * 0.5;
    if !finite
        || (start.x() == end.x() && start.y() == end.y())
        || (start.x() - end.x()).abs() > half_width + SPAN_EPSILON
    {
        return Err(SphericalMeshError::InvalidEdgeGeometry { edge });
    }
    let next = checked_add(output.len(), 1, "projected edge-segment count")?;
    budgets.check_counts(0, 0, 0, next)?;
    output.push(ProjectedEdgeSegment { start, end, edge });
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::f64::consts::{FRAC_PI_2, PI};

    use crate::engine::BuildResultHash;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::view::{
        SphericalEntityLocator, SphericalPresentationSource, SphericalProjection,
        SphericalProjectionKind,
    };
    use crate::world::spatial::{central_angle, SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};
    use crate::world::{CellId, Meters, RootSeed, SphericalSpaceSpec};

    use super::{
        angular_fan_polygon, PreparedProjectedMap, SphericalMeshBudgets, SphericalMeshError,
    };

    const RADIUS: f64 = 6_371_000.0;
    const GEOMETRY_EPSILON: f64 = 2.0e-11;

    fn surface(cell_count: u32) -> SphericalSurfaceSnapshot {
        GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
            radius: Meters::new(RADIUS).unwrap(),
            target_cell_count: cell_count,
        })
        .unwrap()
    }

    fn source(surface: &SphericalSurfaceSnapshot) -> SphericalPresentationSource {
        SphericalPresentationSource::new(
            RootSeed::new(5),
            SurfaceRef::for_spherical(surface),
            BuildResultHash::new([5; 32]),
            1,
        )
    }

    fn direction(longitude: f64, latitude: f64) -> UnitVector3 {
        UnitVector3::new(
            latitude.cos() * longitude.cos(),
            latitude.cos() * longitude.sin(),
            latitude.sin(),
        )
        .unwrap()
    }

    fn relative_longitude(direction: UnitVector3, central_meridian: f64) -> f64 {
        let [x, y, _] = direction.components();
        (y.atan2(x) - central_meridian + PI).rem_euclid(2.0 * PI) - PI
    }

    fn unwrap_near(longitude: f64, reference: f64) -> f64 {
        longitude + ((reference - longitude) / (2.0 * PI)).round() * (2.0 * PI)
    }

    fn authoritative_fan_counts(
        surface: &SphericalSurfaceSnapshot,
        central_meridian: f64,
    ) -> BTreeMap<CellId, (usize, usize, usize)> {
        surface
            .cells()
            .iter()
            .map(|cell| {
                let centroid = relative_longitude(cell.centroid, central_meridian);
                let (non_seam, seam, pole) = (0..cell.boundary_vertices.len()).fold(
                    (0_usize, 0_usize, 0_usize),
                    |(non_seam, seam, pole), side| {
                        let first = surface
                            .vertex(cell.boundary_vertices[side])
                            .unwrap()
                            .position;
                        let second = surface
                            .vertex(
                                cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()],
                            )
                            .unwrap()
                            .position;
                        let directions = [cell.centroid, first, second];
                        if directions.iter().any(|direction| {
                            let [x, y, _] = direction.components();
                            x == 0.0 && y == 0.0
                        }) {
                            return (non_seam, seam, pole + 1);
                        }
                        let longitudes = [
                            centroid,
                            unwrap_near(relative_longitude(first, central_meridian), centroid),
                            unwrap_near(relative_longitude(second, central_meridian), centroid),
                        ];
                        if longitudes.iter().any(|longitude| longitude.abs() > PI) {
                            (non_seam, seam + 1, pole)
                        } else {
                            (non_seam + 1, seam, pole)
                        }
                    },
                );
                (cell.id, (non_seam, seam, pole))
            })
            .collect()
    }

    #[test]
    fn only_exact_poles_expand_to_multi_longitude_display_polygons() {
        let exact_pole = UnitVector3::new(0.0, 0.0, 1.0).unwrap();
        let near_pole = UnitVector3::new(1.0e-16, 0.0, 1.0).unwrap();
        let first = direction(-0.4, 1.2);
        let second = direction(0.4, 1.2);

        assert_eq!(
            angular_fan_polygon([exact_pole, first, second], 0.0).len(),
            4
        );
        assert_eq!(
            angular_fan_polygon([near_pole, first, second], 0.0).len(),
            3
        );
    }

    #[test]
    fn generated_maps_are_seam_safe_finite_complete_and_preserve_semantic_ids() {
        for cell_count in [42, 162] {
            let surface = surface(cell_count);
            let authoritative_cells = surface
                .cells()
                .iter()
                .map(|cell| cell.id)
                .collect::<BTreeSet<_>>();
            let authoritative_edges = surface
                .edges()
                .iter()
                .map(|edge| edge.id)
                .collect::<BTreeSet<_>>();

            for kind in [
                SphericalProjectionKind::EqualEarth,
                SphericalProjectionKind::Equirectangular,
            ] {
                for central_meridian in [0.0, FRAC_PI_2, PI - 1.0e-9] {
                    let projection = SphericalProjection::new(kind, central_meridian).unwrap();
                    let fan_counts = authoritative_fan_counts(&surface, central_meridian);
                    let map = PreparedProjectedMap::build(
                        source(&surface),
                        &surface,
                        projection,
                        SphericalMeshBudgets::default(),
                    )
                    .unwrap_or_else(|error| {
                        panic!(
                            "{cell_count} cells, {kind:?}, meridian {central_meridian}: {error:?}"
                        )
                    });

                    assert_eq!(map.source(), &source(&surface));
                    assert_eq!(map.projection(), projection);
                    assert_eq!(map.bounds(), projection.bounds());
                    assert_eq!(map.cell_count(), surface.cells().len());
                    assert_eq!(map.indices().len() % 3, 0);
                    assert!(map
                        .indices()
                        .iter()
                        .all(|&index| (index as usize) < map.vertices().len()));
                    assert!(map.vertices().iter().all(|vertex| {
                        vertex.position().x().is_finite() && vertex.position().y().is_finite()
                    }));

                    let half_width = (map.bounds().max_x() - map.bounds().min_x()) * 0.5;
                    let mut triangle_cells = BTreeSet::new();
                    let mut triangles_per_cell = BTreeMap::<CellId, usize>::new();
                    for triangle in map.indices().chunks_exact(3) {
                        let vertices = [
                            &map.vertices()[triangle[0] as usize],
                            &map.vertices()[triangle[1] as usize],
                            &map.vertices()[triangle[2] as usize],
                        ];
                        assert_eq!(vertices[0].cell(), vertices[1].cell());
                        assert_eq!(vertices[1].cell(), vertices[2].cell());
                        let points = vertices.map(|vertex| vertex.position());
                        let signed_area = (points[1].x() - points[0].x())
                            * (points[2].y() - points[0].y())
                            - (points[1].y() - points[0].y()) * (points[2].x() - points[0].x());
                        assert!(signed_area.is_finite() && signed_area != 0.0);
                        for edge in [[0, 1], [1, 2], [2, 0]] {
                            assert!(
                                (points[edge[0]].x() - points[edge[1]].x()).abs()
                                    <= half_width + GEOMETRY_EPSILON,
                                "{cell_count} cells, {kind:?}, meridian {central_meridian}, points {points:?}"
                            );
                        }
                        triangle_cells.insert(vertices[0].cell());
                        *triangles_per_cell.entry(vertices[0].cell()).or_default() += 1;
                    }

                    let vertex_cells = map
                        .vertices()
                        .iter()
                        .map(|vertex| vertex.cell())
                        .collect::<BTreeSet<_>>();
                    assert_eq!(vertex_cells, authoritative_cells);
                    assert_eq!(triangle_cells, authoritative_cells);
                    for (&cell, &(non_seam, seam, pole)) in &fan_counts {
                        let actual = triangles_per_cell[&cell];
                        assert!(actual >= non_seam + seam + pole * 2);
                        assert!(actual <= non_seam + seam * 3 + pole * 4);
                        if seam == 0 && pole == 0 {
                            assert_eq!(actual, non_seam);
                        }
                    }

                    assert!(map.edge_segments().iter().all(|segment| {
                        authoritative_edges.contains(&segment.edge())
                            && segment.start().x().is_finite()
                            && segment.start().y().is_finite()
                            && segment.end().x().is_finite()
                            && segment.end().y().is_finite()
                    }));
                    assert_eq!(
                        map.edge_segments()
                            .iter()
                            .map(|segment| segment.edge())
                            .collect::<BTreeSet<_>>(),
                        authoritative_edges
                    );
                    assert!(map.edge_segments().len() >= authoritative_edges.len());
                    for edge in surface.edges() {
                        let fragments = map
                            .edge_segments()
                            .iter()
                            .filter(|segment| segment.edge() == edge.id)
                            .collect::<Vec<_>>();
                        assert!((1..=2).contains(&fragments.len()));
                        let endpoints = edge
                            .vertices
                            .map(|vertex| surface.vertex(vertex).unwrap().position);
                        let original_arc = central_angle(endpoints[0], endpoints[1]);
                        for point in fragments
                            .iter()
                            .flat_map(|segment| [segment.start(), segment.end()])
                        {
                            let on_arc = projection.inverse(point).unwrap();
                            let arc_residual = central_angle(endpoints[0], on_arc)
                                + central_angle(on_arc, endpoints[1])
                                - original_arc;
                            assert!(arc_residual.abs() <= 2.0e-10, "edge {:?}", edge.id);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn anti_meridian_neighbors_inverse_to_the_authoritative_locator_ids() {
        let surface = surface(162);
        for kind in [
            SphericalProjectionKind::EqualEarth,
            SphericalProjectionKind::Equirectangular,
        ] {
            for central_meridian in [0.0, FRAC_PI_2, PI - 1.0e-9] {
                let projection = SphericalProjection::new(kind, central_meridian).unwrap();
                let locator = SphericalEntityLocator::new(source(&surface), &surface).unwrap();
                for offset in [-1.0e-6, 1.0e-6] {
                    let original = direction(central_meridian + PI + offset, 0.31);
                    let restored = projection
                        .inverse(projection.forward(original).unwrap())
                        .unwrap();
                    assert_eq!(locator.locate_cell(restored), locator.locate_cell(original));
                }
            }
        }
    }

    #[test]
    fn build_distinguishes_all_mesh_budget_failures() {
        let surface = surface(42);
        let generous = SphericalMeshBudgets::default();
        let cases = [
            (
                SphericalMeshBudgets::new(
                    41,
                    generous.vertices(),
                    generous.indices(),
                    generous.edge_segments(),
                ),
                "cell",
            ),
            (
                SphericalMeshBudgets::new(42, 0, generous.indices(), generous.edge_segments()),
                "vertex",
            ),
            (
                SphericalMeshBudgets::new(42, generous.vertices(), 0, generous.edge_segments()),
                "index",
            ),
            (
                SphericalMeshBudgets::new(42, generous.vertices(), generous.indices(), 0),
                "edge",
            ),
        ];
        let projection =
            SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.0).unwrap();

        for (budgets, expected) in cases {
            let error =
                PreparedProjectedMap::build(source(&surface), &surface, projection, budgets)
                    .unwrap_err();
            assert!(
                matches!(
                    (expected, error),
                    ("cell", SphericalMeshError::CellBudgetExceeded { .. })
                        | ("vertex", SphericalMeshError::VertexBudgetExceeded { .. })
                        | ("index", SphericalMeshError::IndexBudgetExceeded { .. })
                        | ("edge", SphericalMeshError::EdgeSegmentBudgetExceeded { .. })
                ),
                "unexpected {expected} budget result"
            );
        }
    }
}
