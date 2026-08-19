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
const MAX_PROJECTED_FRAGMENT_VERTICES: usize = 5;
const ARC_BISECTION_ITERATIONS: usize = 64;
const POLE_HORIZONTAL_EPSILON: f64 = 32.0 * f64::EPSILON;
const SEAM_SNAP_EPSILON: f64 = 64.0 * f64::EPSILON * PI;
const SPAN_EPSILON: f64 = 2.0e-12;
const GLOBE_RADIUS_EPSILON: f32 = 2.0e-6;

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
    direction: [f32; 3],
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

    /// Returns the unit-sphere direction this projected vertex displays.
    pub const fn direction(self) -> [f32; 3] {
        self.direction
    }
}

/// One undeformed unit-globe vertex carrying its authoritative raw cell ID.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct GlobeVertex {
    position: [f32; 3],
    cell: u32,
}

impl GlobeVertex {
    /// Returns the finite display-unit-sphere position.
    pub const fn position(self) -> [f32; 3] {
        self.position
    }

    /// Returns the authoritative cell represented by this display vertex.
    pub const fn cell(self) -> CellId {
        CellId::from_raw(self.cell)
    }
}

/// One authoritative edge segment retained on the display unit sphere for annotations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlobeEdgeSegment {
    start: [f32; 3],
    end: [f32; 3],
    edge: EdgeId,
}

impl GlobeEdgeSegment {
    pub(crate) const fn start(self) -> [f32; 3] {
        self.start
    }

    pub(crate) const fn end(self) -> [f32; 3] {
        self.end
    }

    pub(crate) const fn edge(self) -> EdgeId {
        self.edge
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
    /// A cell fan could not produce finite outward unit-globe geometry.
    #[error("cell {cell:?} produced invalid unit-globe geometry")]
    InvalidGlobeGeometry { cell: CellId },
    /// An authoritative edge produced a non-finite, degenerate, or cross-map fragment.
    #[error("amplified mesh arrays are inconsistent")]
    InvalidAmplifiedMesh,
    #[error("edge {edge:?} produced invalid projected geometry")]
    InvalidEdgeGeometry { edge: EdgeId },
    /// A cell was not represented by any usable display triangle.
    #[error("cell {cell:?} produced no projected triangles")]
    MissingCellGeometry { cell: CellId },
}

/// A bounded source-bound triangle mesh on the undeformed display unit sphere.
#[derive(Debug, Clone)]
pub struct PreparedGlobeMesh {
    source: SphericalPresentationSource,
    cell_count: usize,
    cell_centroids: Vec<UnitVector3>,
    edge_segments: Vec<GlobeEdgeSegment>,
    vertices: Vec<GlobeVertex>,
    indices: Vec<u32>,
}

impl PreparedGlobeMesh {
    /// Builds static unit-sphere geometry from authoritative surface directions.
    ///
    /// Field payloads, ranges, palettes, elevation, animation, and camera state
    /// are deliberately absent from this boundary and cannot alter stored bytes.
    pub fn build(
        source: SphericalPresentationSource,
        surface: &SphericalSurfaceSnapshot,
        budgets: SphericalMeshBudgets,
    ) -> Result<Self, SphericalMeshError> {
        let surface_ref = SurfaceRef::try_for_spherical(surface)?;
        if source.surface_ref() != surface_ref {
            return Err(SphericalMeshError::SourceSurfaceMismatch {
                source_ref: source.surface_ref(),
                surface: surface_ref,
            });
        }
        budgets.check_counts(surface.cells().len(), 0, 0, surface.edges().len())?;

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for cell in surface.cells() {
            for side in 0..cell.boundary_vertices.len() {
                let first = surface
                    .vertex(cell.boundary_vertices[side])
                    .expect("validated cell boundary vertex must exist")
                    .position;
                let second = surface
                    .vertex(cell.boundary_vertices[(side + 1) % cell.boundary_vertices.len()])
                    .expect("validated cell boundary vertex must exist")
                    .position;
                let mut triangle = [
                    globe_vertex(cell.centroid, cell.id)?,
                    globe_vertex(first, cell.id)?,
                    globe_vertex(second, cell.id)?,
                ];
                let winding = globe_winding(triangle);
                if !winding.is_finite() || winding == 0.0 {
                    return Err(SphericalMeshError::InvalidGlobeGeometry { cell: cell.id });
                }
                if winding < 0.0 {
                    triangle.swap(1, 2);
                }

                let next_vertex_count =
                    checked_add(vertices.len(), TRIANGLE_VERTEX_COUNT, "globe vertex count")?;
                let next_index_count =
                    checked_add(indices.len(), TRIANGLE_VERTEX_COUNT, "globe index count")?;
                budgets.check_counts(0, next_vertex_count, next_index_count, 0)?;
                let first_index = checked_u32(vertices.len(), "globe vertex index")?;
                let second_index = checked_u32(
                    checked_add(vertices.len(), 1, "globe vertex index")?,
                    "globe vertex index",
                )?;
                let third_index = checked_u32(
                    checked_add(vertices.len(), 2, "globe vertex index")?,
                    "globe vertex index",
                )?;
                vertices.extend(triangle);
                indices.extend([first_index, second_index, third_index]);
            }
        }
        let cell_centroids = surface.cells().iter().map(|cell| cell.centroid).collect();
        let edge_segments = surface
            .edges()
            .iter()
            .map(|edge| {
                let endpoints = edge.vertices.map(|vertex| {
                    surface
                        .vertex(vertex)
                        .expect("validated edge vertex must exist")
                        .position
                        .components()
                        .map(|component| component as f32)
                });
                GlobeEdgeSegment {
                    start: endpoints[0],
                    end: endpoints[1],
                    edge: edge.id,
                }
            })
            .collect::<Vec<_>>();
        budgets.check_counts(
            surface.cells().len(),
            vertices.len(),
            indices.len(),
            edge_segments.len(),
        )?;

        Ok(Self {
            source,
            cell_count: surface.cells().len(),
            cell_centroids,
            edge_segments,
            vertices,
            indices,
        })
    }

    /// Returns the immutable authoritative build identity.
    pub const fn source(&self) -> &SphericalPresentationSource {
        &self.source
    }

    /// Returns the authoritative cell cardinality.
    pub const fn cell_count(&self) -> usize {
        self.cell_count
    }

    /// Returns unit-sphere vertices in stable cell/fan order.
    pub fn vertices(&self) -> &[GlobeVertex] {
        &self.vertices
    }

    /// Returns checked triangle indices in stable cell/fan order.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Returns owned heap bytes for undeformed geometry using collection capacities.
    ///
    /// Allocator bookkeeping and the inline `Self` value are intentionally excluded.
    pub fn resident_bytes(&self) -> Result<usize, super::ResidentBytesError> {
        let context = "prepared unit globe";
        let total = super::resident::capacity_bytes::<UnitVector3>(
            self.cell_centroids.capacity(),
            context,
        )?;
        let total = super::resident::add_capacity::<GlobeEdgeSegment>(
            total,
            self.edge_segments.capacity(),
            context,
        )?;
        let total =
            super::resident::add_capacity::<GlobeVertex>(total, self.vertices.capacity(), context)?;
        super::resident::add_capacity::<u32>(total, self.indices.capacity(), context)
    }

    pub(crate) fn cell_centroids(&self) -> &[UnitVector3] {
        &self.cell_centroids
    }

    #[cfg(test)]
    pub(crate) fn set_cell_centroid_for_test(&mut self, cell: CellId, centroid: UnitVector3) {
        self.cell_centroids[cell.raw() as usize] = centroid;
    }

    pub(crate) fn edge_segments(&self) -> &[GlobeEdgeSegment] {
        &self.edge_segments
    }
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

    /// Returns owned heap bytes for projected geometry using collection capacities.
    ///
    /// Allocator bookkeeping and the inline `Self` value are intentionally excluded.
    pub fn resident_bytes(&self) -> Result<usize, super::ResidentBytesError> {
        let context = "prepared projected map";
        let total = super::resident::capacity_bytes::<ProjectedMapVertex>(
            self.vertices.capacity(),
            context,
        )?;
        let total = super::resident::add_capacity::<u32>(total, self.indices.capacity(), context)?;
        super::resident::add_capacity::<ProjectedEdgeSegment>(
            total,
            self.edge_segments.capacity(),
            context,
        )
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

fn globe_vertex(direction: UnitVector3, cell: CellId) -> Result<GlobeVertex, SphericalMeshError> {
    let position = direction.components().map(|component| component as f32);
    let radius_squared = position[0].mul_add(
        position[0],
        position[1].mul_add(position[1], position[2] * position[2]),
    );
    let radius = radius_squared.sqrt();
    if position.into_iter().any(|component| !component.is_finite())
        || !radius.is_finite()
        || (radius - 1.0).abs() > GLOBE_RADIUS_EPSILON
    {
        return Err(SphericalMeshError::InvalidGlobeGeometry { cell });
    }
    Ok(GlobeVertex {
        position,
        cell: cell.raw(),
    })
}

fn globe_winding(triangle: [GlobeVertex; TRIANGLE_VERTEX_COUNT]) -> f32 {
    let [first, second, third] = triangle.map(GlobeVertex::position);
    let first_edge = [
        second[0] - first[0],
        second[1] - first[1],
        second[2] - first[2],
    ];
    let second_edge = [
        third[0] - first[0],
        third[1] - first[1],
        third[2] - first[2],
    ];
    let normal = [
        first_edge[1] * second_edge[2] - first_edge[2] * second_edge[1],
        first_edge[2] * second_edge[0] - first_edge[0] * second_edge[2],
        first_edge[0] * second_edge[1] - first_edge[1] * second_edge[0],
    ];
    let outward = [
        first[0] + second[0] + third[0],
        first[1] + second[1] + third[1],
        first[2] + second[2] + third[2],
    ];
    normal[0].mul_add(
        outward[0],
        normal[1].mul_add(outward[1], normal[2] * outward[2]),
    )
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

fn angular_edge(directions: [UnitVector3; 2], central_meridian: f64) -> Option<[AngularVertex; 2]> {
    let poles = directions.map(is_pole);
    if poles == [true, true] {
        // Two exact-pole endpoints do not define a supported unique authoritative edge.
        return None;
    }
    let first_longitude = if poles[0] {
        relative_longitude(directions[1], central_meridian)
    } else {
        relative_longitude(directions[0], central_meridian)
    };
    let second_longitude = if poles[1] {
        first_longitude
    } else {
        unwrap_near(
            relative_longitude(directions[1], central_meridian),
            first_longitude,
        )
    };
    Some([
        AngularVertex {
            direction: directions[0],
            latitude: latitude(directions[0]),
            longitude: first_longitude,
        },
        AngularVertex {
            direction: directions[1],
            latitude: latitude(directions[1]),
            longitude: second_longitude,
        },
    ])
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
    let mut snapped = polygon.to_vec();
    for vertex in &mut snapped {
        let seam_distance = (vertex.longitude.abs() - PI).abs();
        if seam_distance <= SEAM_SNAP_EPSILON {
            vertex.longitude = vertex.longitude.signum() * PI;
        }
    }
    let min = snapped
        .iter()
        .map(|vertex| vertex.longitude)
        .fold(f64::INFINITY, f64::min);
    let max = snapped
        .iter()
        .map(|vertex| vertex.longitude)
        .fold(f64::NEG_INFINITY, f64::max);
    if min >= -PI && max <= PI {
        return Ok(vec![snapped]);
    }

    let (seam, first_keeps_less, second_shift) = if max > PI {
        (PI, true, -2.0 * PI)
    } else if min < -PI {
        (-PI, false, 2.0 * PI)
    } else {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
    };
    let first = clip_polygon(&snapped, seam, first_keeps_less, central_meridian, cell)?;
    let mut second = clip_polygon(&snapped, seam, !first_keeps_less, central_meridian, cell)?;
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
    if !(TRIANGLE_VERTEX_COUNT..=MAX_PROJECTED_FRAGMENT_VERTICES).contains(&fragment.len()) {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
    }
    let mut points = fragment
        .iter()
        .map(|&vertex| project_angular(vertex, projection))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SphericalMeshError::InvalidCellGeometry { cell })?;
    // Every fragment vertex carries its authoritative unit direction (seam
    // splits interpolate it on the arc), so the display mesh forwards it
    // instead of numerically inverting the projection. Winding repairs must
    // keep both sequences aligned, hence the pre-reversal here.
    let mut directions = fragment
        .iter()
        .map(|vertex| vertex.direction)
        .collect::<Vec<_>>();
    let twice_area = projected_polygon_twice_area(&points);
    if twice_area.is_finite() && twice_area < 0.0 {
        points.reverse();
        directions.reverse();
    }
    let triangles = triangulate_projected_polygon(&mut points)
        .ok_or(SphericalMeshError::InvalidCellGeometry { cell })?;
    for triangle in triangles {
        append_projected_triangle(
            triangle.map(|index| points[index]),
            triangle.map(|index| directions[index]),
            cell,
            bounds,
            budgets,
            vertices,
            indices,
        )?;
    }
    Ok(())
}

fn append_projected_triangle(
    points: [ProjectionPoint; TRIANGLE_VERTEX_COUNT],
    directions: [UnitVector3; TRIANGLE_VERTEX_COUNT],
    cell: CellId,
    bounds: ProjectionBounds,
    budgets: SphericalMeshBudgets,
    vertices: &mut Vec<ProjectedMapVertex>,
    indices: &mut Vec<u32>,
) -> Result<(), SphericalMeshError> {
    if points
        .iter()
        .any(|point| !point.x().is_finite() || !point.y().is_finite())
    {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
    }
    let signed_area = signed_area(points);
    if !signed_area.is_finite() || signed_area <= projected_area_epsilon(&points) {
        return Err(SphericalMeshError::InvalidCellGeometry { cell });
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
    for (position, direction) in points.into_iter().zip(directions) {
        vertices.push(ProjectedMapVertex {
            position,
            cell,
            direction: direction.components().map(|component| component as f32),
        });
    }
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

fn triangulate_projected_polygon(
    points: &mut [ProjectionPoint],
) -> Option<Vec<[usize; TRIANGLE_VERTEX_COUNT]>> {
    if !(TRIANGLE_VERTEX_COUNT..=MAX_PROJECTED_FRAGMENT_VERTICES).contains(&points.len())
        || points
            .iter()
            .any(|point| !point.x().is_finite() || !point.y().is_finite())
    {
        return None;
    }
    let epsilon = projected_area_epsilon(points);
    if !projected_polygon_is_simple(points, epsilon) {
        return None;
    }
    let mut polygon_area = projected_polygon_twice_area(points);
    if !polygon_area.is_finite() || polygon_area.abs() <= epsilon {
        return None;
    }
    if polygon_area < 0.0 {
        points.reverse();
        polygon_area = -polygon_area;
    }

    let mut remaining = (0..points.len()).collect::<Vec<_>>();
    let mut triangles = Vec::with_capacity(points.len() - 2);
    for _ in 0..MAX_PROJECTED_FRAGMENT_VERTICES {
        if remaining.len() == TRIANGLE_VERTEX_COUNT {
            break;
        }
        let mut ear = None;
        for position in 0..remaining.len() {
            let previous = remaining[(position + remaining.len() - 1) % remaining.len()];
            let current = remaining[position];
            let next = remaining[(position + 1) % remaining.len()];
            let triangle = [points[previous], points[current], points[next]];
            if signed_area(triangle) <= epsilon {
                continue;
            }
            if remaining.iter().copied().any(|candidate| {
                candidate != previous
                    && candidate != current
                    && candidate != next
                    && point_in_triangle_inclusive(points[candidate], triangle, epsilon)
            }) {
                continue;
            }
            ear = Some((position, [previous, current, next]));
            break;
        }
        let (position, triangle) = ear?;
        triangles.push(triangle);
        remaining.remove(position);
    }
    if remaining.len() != TRIANGLE_VERTEX_COUNT {
        return None;
    }
    let final_triangle = [remaining[0], remaining[1], remaining[2]];
    if signed_area(final_triangle.map(|index| points[index])) <= epsilon {
        return None;
    }
    triangles.push(final_triangle);
    if triangles.len() != points.len() - 2 {
        return None;
    }

    let triangle_area = triangles
        .iter()
        .map(|triangle| signed_area(triangle.map(|index| points[index])))
        .sum::<f64>();
    if (triangle_area - polygon_area).abs() > epsilon * points.len() as f64 * 2.0 {
        return None;
    }
    Some(triangles)
}

fn projected_polygon_twice_area(points: &[ProjectionPoint]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(start, end)| start.x() * end.y() - end.x() * start.y())
        .sum()
}

fn projected_area_epsilon(points: &[ProjectionPoint]) -> f64 {
    let scale = points
        .iter()
        .flat_map(|point| [point.x().abs(), point.y().abs()])
        .fold(1.0_f64, f64::max);
    64.0 * f64::EPSILON * scale * scale
}

fn projected_polygon_is_simple(points: &[ProjectionPoint], epsilon: f64) -> bool {
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        if projected_distance_squared(points[index], points[next]) <= epsilon * epsilon {
            return false;
        }
        for other in index + 1..points.len() {
            let other_next = (other + 1) % points.len();
            if index == other_next || next == other {
                continue;
            }
            if projected_segments_intersect(
                points[index],
                points[next],
                points[other],
                points[other_next],
                epsilon,
            ) {
                return false;
            }
        }
    }
    true
}

fn projected_segments_intersect(
    first_start: ProjectionPoint,
    first_end: ProjectionPoint,
    second_start: ProjectionPoint,
    second_end: ProjectionPoint,
    epsilon: f64,
) -> bool {
    let orientations = [
        projected_cross(first_start, first_end, second_start),
        projected_cross(first_start, first_end, second_end),
        projected_cross(second_start, second_end, first_start),
        projected_cross(second_start, second_end, first_end),
    ];
    if orientations[0] * orientations[1] < 0.0 && orientations[2] * orientations[3] < 0.0 {
        return true;
    }
    (orientations[0].abs() <= epsilon
        && projected_point_on_segment(second_start, first_start, first_end, epsilon))
        || (orientations[1].abs() <= epsilon
            && projected_point_on_segment(second_end, first_start, first_end, epsilon))
        || (orientations[2].abs() <= epsilon
            && projected_point_on_segment(first_start, second_start, second_end, epsilon))
        || (orientations[3].abs() <= epsilon
            && projected_point_on_segment(first_end, second_start, second_end, epsilon))
}

fn projected_point_on_segment(
    point: ProjectionPoint,
    start: ProjectionPoint,
    end: ProjectionPoint,
    epsilon: f64,
) -> bool {
    point.x() >= start.x().min(end.x()) - epsilon
        && point.x() <= start.x().max(end.x()) + epsilon
        && point.y() >= start.y().min(end.y()) - epsilon
        && point.y() <= start.y().max(end.y()) + epsilon
}

fn point_in_triangle_inclusive(
    point: ProjectionPoint,
    triangle: [ProjectionPoint; TRIANGLE_VERTEX_COUNT],
    epsilon: f64,
) -> bool {
    projected_cross(triangle[0], triangle[1], point) >= -epsilon
        && projected_cross(triangle[1], triangle[2], point) >= -epsilon
        && projected_cross(triangle[2], triangle[0], point) >= -epsilon
}

fn projected_cross(start: ProjectionPoint, end: ProjectionPoint, point: ProjectionPoint) -> f64 {
    (end.x() - start.x()) * (point.y() - start.y())
        - (end.y() - start.y()) * (point.x() - start.x())
}

fn projected_distance_squared(first: ProjectionPoint, second: ProjectionPoint) -> f64 {
    (first.x() - second.x()).powi(2) + (first.y() - second.y()).powi(2)
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
    let [start, end] = angular_edge(endpoints, projection.central_meridian())
        .ok_or(SphericalMeshError::InvalidEdgeGeometry { edge })?;
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

/// One direction-domain amplified subdivision mesh shared by both presenters.
///
/// Directions are unit vectors and colors are pre-lit sRGB bytes; the mesh is
/// built once per published world and re-projected per map view.
#[derive(Debug, Clone)]
pub struct AmplifiedSurfaceMesh {
    directions: Vec<[f32; 3]>,
    colors: Vec<[u8; 4]>,
    indices: Vec<u32>,
}

impl AmplifiedSurfaceMesh {
    /// Validates cardinalities, finiteness, and index bounds once.
    pub fn new(
        directions: Vec<[f32; 3]>,
        colors: Vec<[u8; 4]>,
        indices: Vec<u32>,
    ) -> Result<Self, SphericalMeshError> {
        if directions.is_empty()
            || directions.len() != colors.len()
            || indices.is_empty()
            || indices.len() % TRIANGLE_VERTEX_COUNT != 0
        {
            return Err(SphericalMeshError::InvalidAmplifiedMesh);
        }
        if directions
            .iter()
            .any(|direction| direction.iter().any(|component| !component.is_finite()))
        {
            return Err(SphericalMeshError::InvalidAmplifiedMesh);
        }
        let vertex_count = directions.len();
        if indices.iter().any(|&index| index as usize >= vertex_count) {
            return Err(SphericalMeshError::InvalidAmplifiedMesh);
        }
        Ok(Self {
            directions,
            colors,
            indices,
        })
    }

    /// Returns the unit directions of all subdivision vertices.
    pub fn directions(&self) -> &[[f32; 3]] {
        &self.directions
    }

    /// Returns the pre-lit sRGB vertex colors.
    pub fn colors(&self) -> &[[u8; 4]] {
        &self.colors
    }

    /// Returns the triangle index list.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Returns the triangle count.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / TRIANGLE_VERTEX_COUNT
    }
}

/// One projected amplified map vertex ready for GPU packing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmplifiedMapVertex {
    /// The projected planar position.
    pub position: [f32; 2],
    /// The pre-lit sRGB color carried over from the direction-domain vertex.
    pub color: [u8; 4],
}

/// Projects the amplified mesh for one map view.
///
/// Almost every subdivision triangle projects directly through the shared
/// vertex table; a triangle whose projected x-span exceeds half the outline
/// width (or with a non-finite corner) is re-cut through the seam pipeline,
/// its cut-point colors re-interpolated barycentrically on the source
/// triangle. Triangles the seam pipeline cannot represent are dropped rather
/// than failing the whole view.
pub fn project_amplified_map(
    mesh: &AmplifiedSurfaceMesh,
    projection: SphericalProjection,
) -> (Vec<AmplifiedMapVertex>, Vec<u32>) {
    #[cfg(not(target_arch = "wasm32"))]
    use rayon::prelude::*;

    let bounds = projection.bounds();
    let half_width = ((bounds.max_x() - bounds.min_x()) * 0.5 + SPAN_EPSILON) as f32;
    let central_meridian = projection.central_meridian();
    #[cfg(not(target_arch = "wasm32"))]
    let vertex_iter = mesh.directions().par_iter().zip(mesh.colors().par_iter());
    #[cfg(target_arch = "wasm32")]
    let vertex_iter = mesh.directions().iter().zip(mesh.colors().iter());
    let mut vertices: Vec<AmplifiedMapVertex> = vertex_iter
        .map(|(direction, color)| {
            let [x, y, z] = *direction;
            let longitude = f64::from(y).atan2(f64::from(x));
            let latitude = f64::from(z).clamp(-1.0, 1.0).asin();
            let relative = wrap_radians(longitude - central_meridian);
            let position = projection
                .forward_latitude_relative_longitude(latitude, relative)
                .map_or([f32::NAN; 2], |point| [point.x() as f32, point.y() as f32]);
            AmplifiedMapVertex {
                position,
                color: *color,
            }
        })
        .collect();
    let mut indices = Vec::with_capacity(mesh.indices().len());
    for triangle in mesh.indices().chunks_exact(TRIANGLE_VERTEX_COUNT) {
        let corners = [triangle[0], triangle[1], triangle[2]];
        let positions = corners.map(|index| vertices[index as usize].position);
        let finite = positions
            .iter()
            .all(|position| position[0].is_finite() && position[1].is_finite());
        let span = positions
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            - positions.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        if finite && span <= half_width {
            indices.extend_from_slice(triangle);
        } else {
            append_seam_cut_triangle(
                mesh,
                corners,
                projection,
                central_meridian,
                &mut vertices,
                &mut indices,
            );
        }
    }
    (vertices, indices)
}

/// Re-cuts one seam- or pole-adjacent subdivision triangle for the map.
fn append_seam_cut_triangle(
    mesh: &AmplifiedSurfaceMesh,
    corners: [u32; 3],
    projection: SphericalProjection,
    central_meridian: f64,
    vertices: &mut Vec<AmplifiedMapVertex>,
    indices: &mut Vec<u32>,
) {
    let mut recovered = Vec::with_capacity(TRIANGLE_VERTEX_COUNT);
    for &corner in &corners {
        let [x, y, z] = mesh.directions()[corner as usize];
        let Ok(direction) = UnitVector3::new(f64::from(x), f64::from(y), f64::from(z)) else {
            return;
        };
        recovered.push(direction);
    }
    let Ok(directions) = <[UnitVector3; TRIANGLE_VERTEX_COUNT]>::try_from(recovered) else {
        return;
    };
    let colors = corners.map(|corner| mesh.colors()[corner as usize]);
    let polygon = angular_fan_polygon(directions, central_meridian);
    let Ok(fragments) = split_polygon_at_seam(&polygon, central_meridian, CellId::from_raw(0))
    else {
        return;
    };
    for fragment in fragments {
        if fragment.len() < TRIANGLE_VERTEX_COUNT {
            continue;
        }
        let mut projected = Vec::with_capacity(fragment.len());
        for vertex in &fragment {
            let Ok(point) = project_angular(*vertex, projection) else {
                projected.clear();
                break;
            };
            projected.push((point, vertex.direction));
        }
        if projected.len() < TRIANGLE_VERTEX_COUNT {
            continue;
        }
        let Ok(base) = u32::try_from(vertices.len()) else {
            return;
        };
        for (point, direction) in &projected {
            vertices.push(AmplifiedMapVertex {
                position: [point.x() as f32, point.y() as f32],
                color: barycentric_color(&directions, &colors, *direction),
            });
        }
        for corner in 1..projected.len() - 1 {
            let (Ok(second), Ok(third)) = (
                u32::try_from(corner).map(|c| base + c),
                u32::try_from(corner + 1).map(|c| base + c),
            ) else {
                return;
            };
            indices.extend_from_slice(&[base, second, third]);
        }
    }
}

/// Interpolates a cut-point color barycentrically on its source triangle.
fn barycentric_color(
    directions: &[UnitVector3; TRIANGLE_VERTEX_COUNT],
    colors: &[[u8; 4]; TRIANGLE_VERTEX_COUNT],
    point: UnitVector3,
) -> [u8; 4] {
    fn triple(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
        a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
    }
    let [a, b, c] = directions.map(UnitVector3::components);
    let p = point.components();
    let determinant = triple(a, b, c);
    if determinant.abs() <= f64::EPSILON {
        return colors[0];
    }
    let mut weights = [
        triple(p, b, c) / determinant,
        triple(a, p, c) / determinant,
        triple(a, b, p) / determinant,
    ];
    let mut total = 0.0;
    for weight in &mut weights {
        *weight = weight.max(0.0);
        total += *weight;
    }
    if total <= f64::EPSILON {
        return colors[0];
    }
    let mut blended = [0u8; 4];
    for channel in 0..4 {
        let value = weights
            .iter()
            .zip(colors.iter())
            .map(|(weight, color)| weight / total * f64::from(color[channel]))
            .sum::<f64>();
        blended[channel] = value.round().clamp(0.0, 255.0) as u8;
    }
    blended
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::f64::consts::{FRAC_PI_2, PI};

    use crate::engine::BuildResultHash;
    use crate::generators::spatial::GeodesicVoronoiBuilder;
    use crate::view::{
        GlobeCamera, SphericalEntityLocator, SphericalPresentationSource, SphericalProjection,
        SphericalProjectionKind,
    };
    use crate::world::spatial::{central_angle, SphericalSurfaceSnapshot, SurfaceRef, UnitVector3};
    use crate::world::{CellId, EdgeId, Meters, RootSeed, SphericalSpaceSpec};

    use super::{
        angular_edge, angular_fan_polygon, append_edge_fragments, project_angular,
        split_polygon_at_seam, triangulate_fragment, AngularVertex, PreparedGlobeMesh,
        PreparedProjectedMap, ProjectedMapVertex, SphericalMeshBudgets, SphericalMeshError,
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

    #[test]
    fn seam_roundoff_does_not_create_a_degenerate_duplicate_fragment() {
        let polygon = [
            AngularVertex {
                direction: UnitVector3::new(
                    -0.779_856_009_277_915_4,
                    -0.012_682_137_155_643_595,
                    0.625_830_462_817_438_5,
                )
                .unwrap(),
                latitude: 0.676_195_830_414_554_3,
                longitude: -3.125_331_934_662_663,
            },
            AngularVertex {
                direction: UnitVector3::new(
                    -0.776_630_775_524_748,
                    3.398_588_507_649_087_4e-15,
                    0.629_956_060_775_534_2,
                )
                .unwrap(),
                latitude: 0.681_496_633_541_563_4,
                longitude: -3.141_592_653_589_797_6,
            },
            AngularVertex {
                direction: UnitVector3::new(
                    -0.783_185_232_425_882_7,
                    3.427_271_251_882_377e-15,
                    0.621_788_462_187_918_3,
                )
                .unwrap(),
                latitude: 0.671_024_213_846_250_6,
                longitude: -3.141_592_653_589_797_6,
            },
        ];

        let fragments = split_polygon_at_seam(&polygon, 0.0, CellId::from_raw(7130)).unwrap();

        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].len(), 3);
        assert!(fragments[0]
            .iter()
            .all(|vertex| (-PI..=PI).contains(&vertex.longitude)));
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

    fn angular_vertex(projected: [f64; 2]) -> AngularVertex {
        let longitude = projected[0] * PI;
        let latitude = projected[1] * FRAC_PI_2;
        AngularVertex {
            direction: direction(longitude, latitude),
            latitude,
            longitude,
        }
    }

    fn polygon_area(points: &[[f64; 2]]) -> f64 {
        points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(start, end)| start[0] * end[1] - end[0] * start[1])
            .sum::<f64>()
            * 0.5
    }

    fn cross_2d(start: [f64; 2], end: [f64; 2], point: [f64; 2]) -> f64 {
        (end[0] - start[0]) * (point[1] - start[1]) - (end[1] - start[1]) * (point[0] - start[0])
    }

    fn line_intersection(
        start: [f64; 2],
        end: [f64; 2],
        clip_start: [f64; 2],
        clip_end: [f64; 2],
    ) -> [f64; 2] {
        let start_side = cross_2d(clip_start, clip_end, start);
        let end_side = cross_2d(clip_start, clip_end, end);
        let amount = start_side / (start_side - end_side);
        [
            start[0] + (end[0] - start[0]) * amount,
            start[1] + (end[1] - start[1]) * amount,
        ]
    }

    fn triangle_intersection_area(first: [[f64; 2]; 3], second: [[f64; 2]; 3]) -> f64 {
        let mut intersection = first.to_vec();
        for edge in 0..3 {
            let clip_start = second[edge];
            let clip_end = second[(edge + 1) % 3];
            let mut clipped = Vec::new();
            let mut previous = *intersection.last().unwrap_or(&first[0]);
            let mut previous_inside = cross_2d(clip_start, clip_end, previous) >= -1.0e-14;
            for &current in &intersection {
                let current_inside = cross_2d(clip_start, clip_end, current) >= -1.0e-14;
                if previous_inside != current_inside {
                    clipped.push(line_intersection(previous, current, clip_start, clip_end));
                }
                if current_inside {
                    clipped.push(current);
                }
                previous = current;
                previous_inside = current_inside;
            }
            intersection = clipped;
            if intersection.is_empty() {
                return 0.0;
            }
        }
        polygon_area(&intersection).abs()
    }

    fn assert_exact_area_partition(
        polygon: &[[f64; 2]],
        vertices: &[ProjectedMapVertex],
        indices: &[u32],
    ) {
        let triangles = indices
            .chunks_exact(3)
            .map(|indices| {
                [indices[0], indices[1], indices[2]].map(|index| {
                    let point = vertices[index as usize].position();
                    [point.x(), point.y()]
                })
            })
            .collect::<Vec<_>>();
        let expected_area = polygon_area(polygon).abs();
        let triangle_area = triangles
            .iter()
            .map(|triangle| polygon_area(triangle))
            .sum::<f64>();
        let tolerance = 2.0e-12 * expected_area.max(1.0);
        assert!(
            (triangle_area - expected_area).abs() <= tolerance,
            "triangle area {triangle_area} does not partition polygon area {expected_area}"
        );
        assert!(triangles
            .iter()
            .all(|triangle| polygon_area(triangle) > 0.0));
        for first in 0..triangles.len() {
            for second in first + 1..triangles.len() {
                assert!(
                    triangle_intersection_area(triangles[first], triangles[second]) <= tolerance,
                    "triangles {first} and {second} overlap in their interiors"
                );
            }
        }
    }

    fn assert_fragment_partition(fragment: &[AngularVertex], projection: SphericalProjection) {
        let polygon = fragment
            .iter()
            .map(|&vertex| {
                let point = project_angular(vertex, projection).unwrap();
                [point.x(), point.y()]
            })
            .collect::<Vec<_>>();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        triangulate_fragment(
            fragment,
            CellId::from_raw(9),
            projection,
            projection.bounds(),
            SphericalMeshBudgets::DEFAULT,
            &mut vertices,
            &mut indices,
        )
        .unwrap();
        assert_exact_area_partition(&polygon, &vertices, &indices);
    }

    fn globe_hash(globe: &PreparedGlobeMesh) -> blake3::Hash {
        let mut hasher = blake3::Hasher::new();
        for vertex in globe.vertices() {
            for component in vertex.position() {
                hasher.update(&component.to_bits().to_le_bytes());
            }
            hasher.update(&vertex.cell().raw().to_le_bytes());
        }
        for &index in globe.indices() {
            hasher.update(&index.to_le_bytes());
        }
        hasher.finalize()
    }

    fn cross_3d(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
        [
            left[1] * right[2] - left[2] * right[1],
            left[2] * right[0] - left[0] * right[2],
            left[0] * right[1] - left[1] * right[0],
        ]
    }

    fn subtract_3d(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
        [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
    }

    fn dot_3d(left: [f32; 3], right: [f32; 3]) -> f32 {
        left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
    }

    #[test]
    fn generated_globe_is_an_outward_unit_sphere_with_exact_semantic_cells() {
        let surface = surface(162);
        let source = source(&surface);
        let budgets = SphericalMeshBudgets::default();

        // This exact call is the API regression: no field, range, palette, relief,
        // animation, or camera value can enter static globe construction.
        let globe = PreparedGlobeMesh::build(source.clone(), &surface, budgets).unwrap();

        assert_eq!(globe.source(), &source);
        assert_eq!(globe.cell_count(), surface.cells().len());
        assert_eq!(globe.indices().len() % 3, 0);
        assert!(globe
            .indices()
            .iter()
            .all(|&index| (index as usize) < globe.vertices().len()));
        assert!(globe.vertices().iter().all(|vertex| {
            let position = vertex.position();
            let radius = dot_3d(position, position).sqrt();
            position.into_iter().all(f32::is_finite) && (radius - 1.0).abs() <= 2.0e-6
        }));

        for triangle in globe.indices().chunks_exact(3) {
            let positions = [triangle[0], triangle[1], triangle[2]]
                .map(|index| globe.vertices()[index as usize].position());
            let normal = cross_3d(
                subtract_3d(positions[1], positions[0]),
                subtract_3d(positions[2], positions[0]),
            );
            let outward = [
                positions[0][0] + positions[1][0] + positions[2][0],
                positions[0][1] + positions[1][1] + positions[2][1],
                positions[0][2] + positions[1][2] + positions[2][2],
            ];
            assert!(dot_3d(normal, outward) > 0.0);
        }

        assert_eq!(
            globe
                .vertices()
                .iter()
                .map(|vertex| vertex.cell())
                .collect::<BTreeSet<_>>(),
            surface
                .cells()
                .iter()
                .map(|cell| cell.id)
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn elevation_ranges_and_camera_mutations_cannot_change_globe_geometry() {
        let surface = surface(42);
        let source = source(&surface);
        let budgets = SphericalMeshBudgets::default();
        let first_elevation = (0..surface.cells().len())
            .map(|index| -12_000.0 + index as f32 * 500.0)
            .collect::<Vec<_>>();
        let first_range = [-12_000.0_f32, 9_000.0_f32];
        assert!(first_elevation.iter().all(|value| value.is_finite()));
        assert!(first_range[0] < first_range[1]);
        let first = PreparedGlobeMesh::build(source.clone(), &surface, budgets).unwrap();
        let original_hash = globe_hash(&first);
        let geometry_revision = first.source().surface_ref();

        let second_elevation = (0..surface.cells().len())
            .map(|index| 2_000_000.0 - index as f32 * 75_000.0)
            .collect::<Vec<_>>();
        let second_range = [-1_000_000.0_f32, 2_000_000.0_f32];
        assert_ne!(first_elevation, second_elevation);
        assert_ne!(first_range, second_range);
        let second = PreparedGlobeMesh::build(source, &surface, budgets).unwrap();

        assert_eq!(globe_hash(&second), original_hash);
        assert_eq!(second.vertices(), first.vertices());
        assert_eq!(second.indices(), first.indices());
        assert_eq!(second.source().surface_ref(), geometry_revision);

        let mut camera = GlobeCamera::default();
        assert!(camera.trackball_drag([50.0, 50.0], [90.0, 35.0], [100.0, 100.0]));
        assert!(camera.zoom_by(6.0));
        assert_eq!(globe_hash(&first), original_hash);
        assert_eq!(first.source().surface_ref(), geometry_revision);
    }

    #[test]
    fn concave_projected_fragment_is_partitioned_without_overlap() {
        let projection =
            SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.0).unwrap();
        let fragment = [
            [0.45, -0.6],
            [0.0, 0.0],
            [0.45, 0.6],
            [-0.45, 0.6],
            [-0.45, -0.6],
        ]
        .map(angular_vertex);

        assert_fragment_partition(&fragment, projection);
        let mut clockwise = fragment;
        clockwise.reverse();
        assert_fragment_partition(&clockwise, projection);
    }

    #[test]
    fn self_intersecting_projected_fragment_is_rejected() {
        let projection =
            SphericalProjection::new(SphericalProjectionKind::Equirectangular, 0.0).unwrap();
        let fragment = [[-0.4, -0.4], [0.4, 0.4], [-0.4, 0.4], [0.4, -0.4]].map(angular_vertex);
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        assert!(matches!(
            triangulate_fragment(
                &fragment,
                CellId::from_raw(13),
                projection,
                projection.bounds(),
                SphericalMeshBudgets::DEFAULT,
                &mut vertices,
                &mut indices,
            ),
            Err(SphericalMeshError::InvalidCellGeometry { cell })
                if cell == CellId::from_raw(13)
        ));
        assert!(vertices.is_empty());
        assert!(indices.is_empty());
    }

    #[test]
    fn generated_high_latitude_seam_fragments_are_exact_area_partitions() {
        let surface = surface(162);
        let mut checked = 0_usize;
        for kind in [
            SphericalProjectionKind::EqualEarth,
            SphericalProjectionKind::Equirectangular,
        ] {
            for central_meridian in [0.0, FRAC_PI_2, PI - 1.0e-9] {
                let projection = SphericalProjection::new(kind, central_meridian).unwrap();
                for cell in surface.cells() {
                    for side in 0..cell.boundary_vertices.len() {
                        let endpoints =
                            [side, (side + 1) % cell.boundary_vertices.len()].map(|side| {
                                surface
                                    .vertex(cell.boundary_vertices[side])
                                    .unwrap()
                                    .position
                            });
                        let fan = angular_fan_polygon(
                            [cell.centroid, endpoints[0], endpoints[1]],
                            central_meridian,
                        );
                        let fragments =
                            split_polygon_at_seam(&fan, central_meridian, cell.id).unwrap();
                        if fragments.len() > 1
                            && fan.iter().any(|vertex| vertex.latitude.abs() > 0.9)
                        {
                            for fragment in fragments {
                                assert_fragment_partition(&fragment, projection);
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(
            checked > 0,
            "fixture must exercise high-latitude seam fragments"
        );
    }

    #[test]
    fn exact_pole_edges_use_incident_arc_longitudes_and_near_poles_do_not() {
        for kind in [
            SphericalProjectionKind::EqualEarth,
            SphericalProjectionKind::Equirectangular,
        ] {
            let projection = SphericalProjection::new(kind, 0.3).unwrap();
            for latitude_sign in [-1.0, 1.0] {
                let pole = UnitVector3::new(0.0, 0.0, latitude_sign).unwrap();
                let incident = direction(1.1, latitude_sign * 1.2);
                let companion = direction(1.3, latitude_sign * 1.1);
                let fill_pole = project_angular(
                    angular_fan_polygon([pole, incident, companion], 0.3)[0],
                    projection,
                )
                .unwrap();

                for endpoints in [[pole, incident], [incident, pole]] {
                    let mut segments = Vec::new();
                    append_edge_fragments(
                        endpoints,
                        EdgeId::from_raw(7),
                        projection,
                        projection.bounds(),
                        SphericalMeshBudgets::DEFAULT,
                        &mut segments,
                    )
                    .unwrap();
                    assert_eq!(segments.len(), 1);
                    let pole_point = [segments[0].start(), segments[0].end()]
                        .into_iter()
                        .find(|point| (point.y() - fill_pole.y()).abs() <= 2.0e-12)
                        .unwrap();
                    assert!((pole_point.x() - fill_pole.x()).abs() <= 2.0e-12);
                }

                let near_pole = direction(-1.0, latitude_sign * (FRAC_PI_2 - 1.0e-8));
                let expected = projection.forward(near_pole).unwrap();
                for endpoints in [[near_pole, incident], [incident, near_pole]] {
                    let angular = angular_edge(endpoints, projection.central_meridian()).unwrap();
                    let near_pole_vertex = if endpoints[0] == near_pole {
                        angular[0]
                    } else {
                        angular[1]
                    };
                    let actual = project_angular(near_pole_vertex, projection).unwrap();
                    assert!((actual.x() - expected.x()).abs() <= 2.0e-12);
                    assert!((actual.y() - expected.y()).abs() <= 2.0e-12);
                }
            }

            let mut segments = Vec::new();
            assert!(matches!(
                append_edge_fragments(
                    [
                        UnitVector3::new(0.0, 0.0, 1.0).unwrap(),
                        UnitVector3::new(0.0, 0.0, -1.0).unwrap(),
                    ],
                    EdgeId::from_raw(11),
                    projection,
                    projection.bounds(),
                    SphericalMeshBudgets::DEFAULT,
                    &mut segments,
                ),
                Err(SphericalMeshError::InvalidEdgeGeometry {
                    edge
                }) if edge == EdgeId::from_raw(11)
            ));
            assert!(segments.is_empty());
        }
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
