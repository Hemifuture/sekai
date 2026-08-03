#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::world::spatial::{
    add, central_angle, cross, dot, oriented_arc_normal, scale, spherical_polygon_metrics,
    subtract, SphericalSurfaceCell, SphericalSurfaceEdge, SphericalSurfaceSnapshot,
    SphericalSurfaceValidationError, SphericalSurfaceVertex, UnitVector3,
    SPHERICAL_SURFACE_SCHEMA_V1,
};
use crate::world::{
    CellId, EdgeId, Meters, SphericalSpaceSpec, SphericalSpecError, SquareMeters, SurfaceVertexId,
    MAX_GEODESIC_FREQUENCY, MIN_GEODESIC_FREQUENCY,
};

const GOLDEN_RATIO: f64 = 1.618_033_988_749_895;

const BASE_VERTEX_COMPONENTS: [[f64; 3]; 12] = [
    [-1.0, GOLDEN_RATIO, 0.0],
    [1.0, GOLDEN_RATIO, 0.0],
    [-1.0, -GOLDEN_RATIO, 0.0],
    [1.0, -GOLDEN_RATIO, 0.0],
    [0.0, -1.0, GOLDEN_RATIO],
    [0.0, 1.0, GOLDEN_RATIO],
    [0.0, -1.0, -GOLDEN_RATIO],
    [0.0, 1.0, -GOLDEN_RATIO],
    [GOLDEN_RATIO, 0.0, -1.0],
    [GOLDEN_RATIO, 0.0, 1.0],
    [-GOLDEN_RATIO, 0.0, -1.0],
    [-GOLDEN_RATIO, 0.0, 1.0],
];

const BASE_FACE_VERTICES: [[u8; 3]; 20] = [
    [0, 11, 5],
    [0, 5, 1],
    [0, 1, 7],
    [0, 7, 10],
    [0, 10, 11],
    [1, 5, 9],
    [5, 11, 4],
    [11, 10, 2],
    [10, 7, 6],
    [7, 1, 8],
    [3, 9, 4],
    [3, 4, 2],
    [3, 2, 6],
    [3, 6, 8],
    [3, 8, 9],
    [4, 9, 5],
    [2, 4, 11],
    [6, 2, 10],
    [8, 6, 7],
    [9, 8, 1],
];

/// Stable failures returned while constructing an authoritative spherical surface.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SphericalSurfaceBuildError {
    /// The requested spherical space violates its numerical or allocation budget.
    #[error("invalid spherical space: {0}")]
    InvalidSpec(#[from] SphericalSpecError),
    /// The private integer-key Delaunay mesh could not be constructed.
    #[error("geodesic Delaunay mesh construction failed")]
    MeshConstruction,
    /// A triangle could not produce a finite spherical circumcenter.
    #[error("Delaunay triangle {triangle} has a degenerate spherical circumcenter")]
    DegenerateTriangle {
        /// The deterministic triangle position.
        triangle: usize,
    },
    /// A Voronoi edge could not produce finite positive spherical metrics.
    #[error("Voronoi edge {edge:?} has degenerate spherical geometry")]
    DegenerateEdge {
        /// The deterministic edge identifier.
        edge: EdgeId,
    },
    /// A site could not produce an outward finite Voronoi polygon.
    #[error("Voronoi cell {cell:?} has degenerate spherical geometry")]
    DegenerateCell {
        /// The deterministic generating site identifier.
        cell: CellId,
    },
    /// Consecutive cell vertices did not resolve to their canonical edge.
    #[error("Voronoi cell {cell:?} side has no canonical Delaunay-dual edge")]
    MissingBoundaryEdge {
        /// The deterministic generating site identifier.
        cell: CellId,
    },
    /// A record count exceeded the stable identifier representation.
    #[error("geodesic surface record count exceeds stable identifier capacity")]
    IdentifierOverflow,
    /// The completed records did not satisfy the authoritative snapshot contract.
    #[error("constructed spherical surface failed validation: {0}")]
    InvalidSnapshot(#[from] SphericalSurfaceValidationError),
}

/// Builds deterministic closed spherical Voronoi topology without randomness or projection seams.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeodesicVoronoiBuilder;

impl GeodesicVoronoiBuilder {
    /// Resolves the requested cell budget and constructs one fully validated snapshot.
    pub fn build(
        space: &SphericalSpaceSpec,
    ) -> Result<SphericalSurfaceSnapshot, SphericalSurfaceBuildError> {
        space.validate()?;
        let mesh = GeodesicMesh::build(space.resolved_frequency())
            .map_err(|_| SphericalSurfaceBuildError::MeshConstruction)?;
        build_surface(space.radius, &mesh)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SiteKey {
    BaseVertex(u8),
    BaseEdge {
        first: u8,
        second: u8,
        weight_on_second: u16,
    },
    FaceInterior {
        face: u8,
        weights: [u16; 3],
    },
}

#[derive(Debug, Clone, PartialEq)]
struct GeodesicSite {
    id: CellId,
    key: SiteKey,
    direction: UnitVector3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OrientedTriangle {
    sites: [CellId; 3],
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EdgeIncidence {
    sites: [CellId; 2],
    triangles: [u32; 2],
}

#[derive(Debug, Clone, PartialEq)]
struct GeodesicMesh {
    sites: Vec<GeodesicSite>,
    triangles: Vec<OrientedTriangle>,
    edge_incidence: Vec<EdgeIncidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeodesicMeshError {
    FrequencyOutOfRange,
    CountOverflow,
    InvalidBaseVertex,
    MissingSite,
    DegenerateTriangle,
    NonManifoldEdge,
    CountMismatch,
}

impl GeodesicMesh {
    fn build(frequency: u32) -> Result<Self, GeodesicMeshError> {
        let (expected_sites, expected_edges, expected_triangles) = expected_counts(frequency)?;
        let base_vertices = base_vertices()?;
        let faces = oriented_faces(&base_vertices);
        let mut site_keys = BTreeSet::new();
        let mut triangle_keys = Vec::with_capacity(expected_triangles);

        for (face_index, face) in faces.iter().copied().enumerate() {
            let face_index =
                u8::try_from(face_index).map_err(|_| GeodesicMeshError::CountOverflow)?;
            for first_weight in 0..=frequency {
                for second_weight in 0..=(frequency - first_weight) {
                    let third_weight = frequency - first_weight - second_weight;
                    site_keys.insert(site_key(
                        face_index,
                        face,
                        [first_weight, second_weight, third_weight],
                    )?);
                }
            }

            for second_weight in 0..frequency {
                for third_weight in 0..(frequency - second_weight) {
                    let first_weight = frequency - second_weight - third_weight;
                    triangle_keys.push([
                        site_key(
                            face_index,
                            face,
                            [first_weight, second_weight, third_weight],
                        )?,
                        site_key(
                            face_index,
                            face,
                            [first_weight - 1, second_weight + 1, third_weight],
                        )?,
                        site_key(
                            face_index,
                            face,
                            [first_weight - 1, second_weight, third_weight + 1],
                        )?,
                    ]);

                    if first_weight > 1 {
                        triangle_keys.push([
                            site_key(
                                face_index,
                                face,
                                [first_weight - 1, second_weight + 1, third_weight],
                            )?,
                            site_key(
                                face_index,
                                face,
                                [first_weight - 2, second_weight + 1, third_weight + 1],
                            )?,
                            site_key(
                                face_index,
                                face,
                                [first_weight - 1, second_weight, third_weight + 1],
                            )?,
                        ]);
                    }
                }
            }
        }

        if site_keys.len() != expected_sites || triangle_keys.len() != expected_triangles {
            return Err(GeodesicMeshError::CountMismatch);
        }

        let mut site_ids = BTreeMap::new();
        let mut sites = Vec::with_capacity(expected_sites);
        for (index, key) in site_keys.into_iter().enumerate() {
            let raw_id = u32::try_from(index).map_err(|_| GeodesicMeshError::CountOverflow)?;
            let id = CellId::from_raw(raw_id);
            let direction = direction_for_key(key, frequency, &base_vertices, &faces)?;
            site_ids.insert(key, id);
            sites.push(GeodesicSite { id, key, direction });
        }

        let mut triangles = Vec::with_capacity(expected_triangles);
        for keys in triangle_keys {
            let triangle_sites = keys.map(|key| site_ids.get(&key).copied());
            let [Some(a), Some(b), Some(c)] = triangle_sites else {
                return Err(GeodesicMeshError::MissingSite);
            };
            let mut ids = [a, b, c];
            if ids[0] == ids[1] || ids[1] == ids[2] || ids[2] == ids[0] {
                return Err(GeodesicMeshError::DegenerateTriangle);
            }
            let orientation = triangle_orientation(ids, &sites);
            if orientation == 0.0 || !orientation.is_finite() {
                return Err(GeodesicMeshError::DegenerateTriangle);
            }
            if orientation < 0.0 {
                ids.swap(1, 2);
            }
            triangles.push(OrientedTriangle { sites: ids });
        }

        let edge_incidence = build_edge_incidence(&triangles, expected_edges)?;
        if triangles.len() != expected_triangles || edge_incidence.len() != expected_edges {
            return Err(GeodesicMeshError::CountMismatch);
        }

        Ok(Self {
            sites,
            triangles,
            edge_incidence,
        })
    }
}

fn build_surface(
    radius: Meters,
    mesh: &GeodesicMesh,
) -> Result<SphericalSurfaceSnapshot, SphericalSurfaceBuildError> {
    let vertices = build_voronoi_vertices(mesh)?;
    let boundaries = order_cell_boundaries(mesh, &vertices)?;
    let edges = build_voronoi_edges(radius, mesh, &vertices)?;
    let cells = build_voronoi_cells(radius, mesh, &vertices, boundaries, &edges.ids)?;
    SphericalSurfaceSnapshot::new(
        SPHERICAL_SURFACE_SCHEMA_V1,
        radius,
        vertices,
        cells,
        edges.records,
    )
    .map_err(SphericalSurfaceBuildError::from)
}

fn build_voronoi_vertices(
    mesh: &GeodesicMesh,
) -> Result<Vec<SphericalSurfaceVertex>, SphericalSurfaceBuildError> {
    mesh.triangles
        .iter()
        .enumerate()
        .map(|(triangle_index, triangle)| {
            let [a, b, c] = triangle
                .sites
                .map(|id| mesh.sites[id.raw() as usize].direction.components());
            let mut normal = cross(subtract(b, a), subtract(c, a));
            if dot(normal, add(add(a, b), c)) < 0.0 {
                normal = scale(normal, -1.0);
            }
            let position = UnitVector3::new(normal[0], normal[1], normal[2]).map_err(|_| {
                SphericalSurfaceBuildError::DegenerateTriangle {
                    triangle: triangle_index,
                }
            })?;
            let raw_id = u32::try_from(triangle_index)
                .map_err(|_| SphericalSurfaceBuildError::IdentifierOverflow)?;
            Ok(SphericalSurfaceVertex {
                id: SurfaceVertexId::from_raw(raw_id),
                position,
            })
        })
        .collect()
}

fn order_cell_boundaries(
    mesh: &GeodesicMesh,
    vertices: &[SphericalSurfaceVertex],
) -> Result<Vec<Vec<SurfaceVertexId>>, SphericalSurfaceBuildError> {
    let mut incident_triangles = vec![Vec::<SurfaceVertexId>::new(); mesh.sites.len()];
    for (triangle_index, triangle) in mesh.triangles.iter().enumerate() {
        let raw_id = u32::try_from(triangle_index)
            .map_err(|_| SphericalSurfaceBuildError::IdentifierOverflow)?;
        let vertex_id = SurfaceVertexId::from_raw(raw_id);
        for site in triangle.sites {
            incident_triangles[site.raw() as usize].push(vertex_id);
        }
    }

    for (site, boundary) in mesh.sites.iter().zip(&mut incident_triangles) {
        let (basis_x, basis_y) = deterministic_tangent_basis(site.direction)
            .ok_or(SphericalSurfaceBuildError::DegenerateCell { cell: site.id })?;
        boundary.sort_by(|first_id, second_id| {
            let first = vertices[first_id.raw() as usize].position;
            let second = vertices[second_id.raw() as usize].position;
            tangent_angle(first, basis_x, basis_y)
                .total_cmp(&tangent_angle(second, basis_x, basis_y))
                .then_with(|| first_id.cmp(second_id))
        });

        if boundary.len() < 3 {
            return Err(SphericalSurfaceBuildError::DegenerateCell { cell: site.id });
        }
        let signed_orientation = boundary
            .iter()
            .copied()
            .zip(boundary.iter().copied().cycle().skip(1))
            .take(boundary.len())
            .map(|(first, second)| {
                dot(
                    cross(
                        vertices[first.raw() as usize].position.components(),
                        vertices[second.raw() as usize].position.components(),
                    ),
                    site.direction.components(),
                )
            })
            .sum::<f64>();
        if !signed_orientation.is_finite() || signed_orientation == 0.0 {
            return Err(SphericalSurfaceBuildError::DegenerateCell { cell: site.id });
        }
        if signed_orientation < 0.0 {
            boundary.reverse();
            let final_position = boundary.len() - 1;
            boundary.rotate_left(final_position);
        }
    }
    Ok(incident_triangles)
}

fn deterministic_tangent_basis(site: UnitVector3) -> Option<(UnitVector3, UnitVector3)> {
    let components = site.components();
    let axis_index = (0..3).min_by(|&first, &second| {
        components[first]
            .abs()
            .total_cmp(&components[second].abs())
            .then_with(|| first.cmp(&second))
    })?;
    let mut axis = [0.0; 3];
    axis[axis_index] = 1.0;
    let first = cross(axis, components);
    let basis_x = UnitVector3::new(first[0], first[1], first[2]).ok()?;
    let second = cross(components, basis_x.components());
    let basis_y = UnitVector3::new(second[0], second[1], second[2]).ok()?;
    Some((basis_x, basis_y))
}

fn tangent_angle(point: UnitVector3, basis_x: UnitVector3, basis_y: UnitVector3) -> f64 {
    point.dot(basis_y).atan2(point.dot(basis_x))
}

struct BuiltVoronoiEdges {
    records: Vec<SphericalSurfaceEdge>,
    ids: BTreeMap<[SurfaceVertexId; 2], EdgeId>,
}

fn build_voronoi_edges(
    radius: Meters,
    mesh: &GeodesicMesh,
    vertices: &[SphericalSurfaceVertex],
) -> Result<BuiltVoronoiEdges, SphericalSurfaceBuildError> {
    let mut edges = Vec::with_capacity(mesh.edge_incidence.len());
    let mut edge_ids = BTreeMap::new();
    for (edge_index, incidence) in mesh.edge_incidence.iter().enumerate() {
        let raw_id = u32::try_from(edge_index)
            .map_err(|_| SphericalSurfaceBuildError::IdentifierOverflow)?;
        let edge_id = EdgeId::from_raw(raw_id);
        let vertex_ids = incidence.triangles.map(SurfaceVertexId::from_raw);
        let first_vertex = vertices[vertex_ids[0].raw() as usize].position;
        let second_vertex = vertices[vertex_ids[1].raw() as usize].position;
        let midpoint_components = add(first_vertex.components(), second_vertex.components());
        let midpoint = UnitVector3::new(
            midpoint_components[0],
            midpoint_components[1],
            midpoint_components[2],
        )
        .map_err(|_| SphericalSurfaceBuildError::DegenerateEdge { edge: edge_id })?;

        let cells = incidence.sites;
        let first_site = mesh.sites[cells[0].raw() as usize].direction;
        let second_site = mesh.sites[cells[1].raw() as usize].direction;
        let normal_from_first =
            oriented_arc_normal(first_vertex, second_vertex, first_site, second_site)
                .ok_or(SphericalSurfaceBuildError::DegenerateEdge { edge: edge_id })?;

        let length = positive_edge_metric(
            radius.get() * central_angle(first_vertex, second_vertex),
            edge_id,
        )?;
        let center_distance = positive_edge_metric(
            radius.get() * central_angle(first_site, second_site),
            edge_id,
        )?;
        let center_distances_to_midpoint = [first_site, second_site].map(|site| {
            positive_edge_metric(radius.get() * central_angle(site, midpoint), edge_id)
        });
        let [first_midpoint_distance, second_midpoint_distance] = center_distances_to_midpoint;

        if edge_ids.insert(vertex_ids, edge_id).is_some() {
            return Err(SphericalSurfaceBuildError::DegenerateEdge { edge: edge_id });
        }
        edges.push(SphericalSurfaceEdge {
            id: edge_id,
            vertices: vertex_ids,
            cells,
            midpoint,
            length,
            center_distance,
            center_distances_to_midpoint: [first_midpoint_distance?, second_midpoint_distance?],
            normal_from_first,
        });
    }
    Ok(BuiltVoronoiEdges {
        records: edges,
        ids: edge_ids,
    })
}

fn positive_edge_metric(value: f64, edge: EdgeId) -> Result<Meters, SphericalSurfaceBuildError> {
    if value <= 0.0 || !value.is_finite() {
        return Err(SphericalSurfaceBuildError::DegenerateEdge { edge });
    }
    Meters::new(value).map_err(|_| SphericalSurfaceBuildError::DegenerateEdge { edge })
}

fn build_voronoi_cells(
    radius: Meters,
    mesh: &GeodesicMesh,
    vertices: &[SphericalSurfaceVertex],
    boundaries: Vec<Vec<SurfaceVertexId>>,
    edge_ids: &BTreeMap<[SurfaceVertexId; 2], EdgeId>,
) -> Result<Vec<SphericalSurfaceCell>, SphericalSurfaceBuildError> {
    mesh.sites
        .iter()
        .zip(boundaries)
        .map(|(site, boundary_vertices)| {
            let polygon = boundary_vertices
                .iter()
                .map(|id| vertices[id.raw() as usize].position)
                .collect::<Vec<_>>();
            let (unit_area, centroid) = spherical_polygon_metrics(site.direction, &polygon)
                .ok_or(SphericalSurfaceBuildError::DegenerateCell { cell: site.id })?;
            let area = SquareMeters::new(unit_area * radius.get() * radius.get())
                .map_err(|_| SphericalSurfaceBuildError::DegenerateCell { cell: site.id })?;
            if area.get() <= 0.0 {
                return Err(SphericalSurfaceBuildError::DegenerateCell { cell: site.id });
            }

            let mut boundary_edges = Vec::with_capacity(boundary_vertices.len());
            for side in 0..boundary_vertices.len() {
                let first = boundary_vertices[side];
                let second = boundary_vertices[(side + 1) % boundary_vertices.len()];
                let endpoints = if first < second {
                    [first, second]
                } else {
                    [second, first]
                };
                boundary_edges.push(
                    *edge_ids
                        .get(&endpoints)
                        .ok_or(SphericalSurfaceBuildError::MissingBoundaryEdge { cell: site.id })?,
                );
            }

            Ok(SphericalSurfaceCell {
                id: site.id,
                site: site.direction,
                centroid,
                area,
                boundary_vertices,
                boundary_edges,
            })
        })
        .collect()
}

fn expected_counts(frequency: u32) -> Result<(usize, usize, usize), GeodesicMeshError> {
    if !(MIN_GEODESIC_FREQUENCY..=MAX_GEODESIC_FREQUENCY).contains(&frequency) {
        return Err(GeodesicMeshError::FrequencyOutOfRange);
    }
    let square = u64::from(frequency)
        .checked_mul(u64::from(frequency))
        .ok_or(GeodesicMeshError::CountOverflow)?;
    let sites = square
        .checked_mul(10)
        .and_then(|value| value.checked_add(2))
        .ok_or(GeodesicMeshError::CountOverflow)?;
    let edges = square
        .checked_mul(30)
        .ok_or(GeodesicMeshError::CountOverflow)?;
    let triangles = square
        .checked_mul(20)
        .ok_or(GeodesicMeshError::CountOverflow)?;
    Ok((
        usize::try_from(sites).map_err(|_| GeodesicMeshError::CountOverflow)?,
        usize::try_from(edges).map_err(|_| GeodesicMeshError::CountOverflow)?,
        usize::try_from(triangles).map_err(|_| GeodesicMeshError::CountOverflow)?,
    ))
}

fn base_vertices() -> Result<[UnitVector3; 12], GeodesicMeshError> {
    BASE_VERTEX_COMPONENTS
        .map(|[x, y, z]| UnitVector3::new(x, y, z))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| GeodesicMeshError::InvalidBaseVertex)?
        .try_into()
        .map_err(|_| GeodesicMeshError::InvalidBaseVertex)
}

fn oriented_faces(vertices: &[UnitVector3; 12]) -> [[u8; 3]; 20] {
    BASE_FACE_VERTICES.map(|mut face| {
        let [a, b, c] = face.map(|vertex| vertices[usize::from(vertex)].components());
        let normal = cross(subtract(b, a), subtract(c, a));
        if dot(normal, add(add(a, b), c)) < 0.0 {
            face.swap(1, 2);
        }
        face
    })
}

fn site_key(
    face_index: u8,
    face: [u8; 3],
    weights: [u32; 3],
) -> Result<SiteKey, GeodesicMeshError> {
    let [first_weight, second_weight, third_weight] = weights;
    match (first_weight == 0, second_weight == 0, third_weight == 0) {
        (false, true, true) => Ok(SiteKey::BaseVertex(face[0])),
        (true, false, true) => Ok(SiteKey::BaseVertex(face[1])),
        (true, true, false) => Ok(SiteKey::BaseVertex(face[2])),
        (true, false, false) => edge_site_key(face[1], second_weight, face[2], third_weight),
        (false, true, false) => edge_site_key(face[0], first_weight, face[2], third_weight),
        (false, false, true) => edge_site_key(face[0], first_weight, face[1], second_weight),
        (false, false, false) => {
            let [Ok(first), Ok(second), Ok(third)] = weights.map(u16::try_from) else {
                return Err(GeodesicMeshError::CountOverflow);
            };
            Ok(SiteKey::FaceInterior {
                face: face_index,
                weights: [first, second, third],
            })
        }
        (true, true, true) => Err(GeodesicMeshError::DegenerateTriangle),
    }
}

fn edge_site_key(
    first_vertex: u8,
    first_weight: u32,
    second_vertex: u8,
    second_weight: u32,
) -> Result<SiteKey, GeodesicMeshError> {
    let (first, second, weight_on_second) = if first_vertex < second_vertex {
        (first_vertex, second_vertex, second_weight)
    } else {
        (second_vertex, first_vertex, first_weight)
    };
    Ok(SiteKey::BaseEdge {
        first,
        second,
        weight_on_second: u16::try_from(weight_on_second)
            .map_err(|_| GeodesicMeshError::CountOverflow)?,
    })
}

fn direction_for_key(
    key: SiteKey,
    frequency: u32,
    vertices: &[UnitVector3; 12],
    faces: &[[u8; 3]; 20],
) -> Result<UnitVector3, GeodesicMeshError> {
    match key {
        SiteKey::BaseVertex(vertex) => Ok(vertices[usize::from(vertex)]),
        SiteKey::BaseEdge {
            first,
            second,
            weight_on_second,
        } => {
            let second_weight = u32::from(weight_on_second);
            let first_weight = frequency
                .checked_sub(second_weight)
                .ok_or(GeodesicMeshError::CountOverflow)?;
            normalized_weighted_sum(
                [first, second, first],
                [first_weight, second_weight, 0],
                vertices,
            )
        }
        SiteKey::FaceInterior { face, weights } => {
            normalized_weighted_sum(faces[usize::from(face)], weights.map(u32::from), vertices)
        }
    }
}

fn normalized_weighted_sum(
    vertex_ids: [u8; 3],
    weights: [u32; 3],
    vertices: &[UnitVector3; 12],
) -> Result<UnitVector3, GeodesicMeshError> {
    let mut components = [0.0; 3];
    for (vertex_id, weight) in vertex_ids.into_iter().zip(weights) {
        let vertex = vertices[usize::from(vertex_id)].components();
        for axis in 0..3 {
            components[axis] += vertex[axis] * f64::from(weight);
        }
    }
    UnitVector3::new(components[0], components[1], components[2])
        .map_err(|_| GeodesicMeshError::InvalidBaseVertex)
}

fn triangle_orientation(ids: [CellId; 3], sites: &[GeodesicSite]) -> f64 {
    let [a, b, c] = ids.map(|id| sites[id.raw() as usize].direction.components());
    dot(cross(subtract(b, a), subtract(c, a)), add(add(a, b), c))
}

fn build_edge_incidence(
    triangles: &[OrientedTriangle],
    expected_edges: usize,
) -> Result<Vec<EdgeIncidence>, GeodesicMeshError> {
    let mut owners = BTreeMap::<[CellId; 2], [Option<u32>; 2]>::new();
    for (triangle_index, triangle) in triangles.iter().enumerate() {
        let triangle_id =
            u32::try_from(triangle_index).map_err(|_| GeodesicMeshError::CountOverflow)?;
        for [first, second] in [
            [triangle.sites[0], triangle.sites[1]],
            [triangle.sites[1], triangle.sites[2]],
            [triangle.sites[2], triangle.sites[0]],
        ] {
            let sites = if first < second {
                [first, second]
            } else {
                [second, first]
            };
            let edge_owners = owners.entry(sites).or_insert([None, None]);
            if edge_owners[0].is_none() {
                edge_owners[0] = Some(triangle_id);
            } else if edge_owners[1].is_none() && edge_owners[0] != Some(triangle_id) {
                edge_owners[1] = Some(triangle_id);
            } else {
                return Err(GeodesicMeshError::NonManifoldEdge);
            }
        }
    }

    let mut incidence = Vec::with_capacity(expected_edges);
    for (sites, triangle_owners) in owners {
        let [Some(first), Some(second)] = triangle_owners else {
            return Err(GeodesicMeshError::NonManifoldEdge);
        };
        let triangles = if first < second {
            [first, second]
        } else {
            [second, first]
        };
        incidence.push(EdgeIncidence { sites, triangles });
    }
    Ok(incidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{CellId, MAX_GEODESIC_FREQUENCY};

    #[test]
    fn geodesic_frequencies_have_exact_euler_counts() {
        for (frequency, expected_sites, expected_edges, expected_triangles) in
            [(2, 42, 120, 80), (3, 92, 270, 180), (4, 162, 480, 320)]
        {
            let mesh = GeodesicMesh::build(frequency).unwrap();
            assert_eq!(mesh.sites.len(), expected_sites);
            assert_eq!(mesh.edge_incidence.len(), expected_edges);
            assert_eq!(mesh.triangles.len(), expected_triangles);
        }
    }

    #[test]
    fn geodesic_frequencies_assign_deterministic_ordered_records() {
        let first = GeodesicMesh::build(4).unwrap();
        let second = GeodesicMesh::build(4).unwrap();

        assert_eq!(first.sites, second.sites);
        assert_eq!(first.triangles, second.triangles);
        assert_eq!(first.edge_incidence, second.edge_incidence);
        assert!(first
            .sites
            .iter()
            .enumerate()
            .all(|(index, site)| site.id == CellId::from_raw(index as u32)));
        assert!(first.sites.iter().zip(&second.sites).all(|(a, b)| {
            a.direction.components().map(f64::to_bits) == b.direction.components().map(f64::to_bits)
        }));
    }

    #[test]
    fn geodesic_frequencies_reject_values_outside_supported_bounds() {
        assert!(GeodesicMesh::build(1).is_err());
        assert!(GeodesicMesh::build(MAX_GEODESIC_FREQUENCY + 1).is_err());
    }

    #[test]
    fn geodesic_frequencies_emit_outward_non_degenerate_manifold_triangles() {
        let mesh = GeodesicMesh::build(4).unwrap();

        for triangle in &mesh.triangles {
            let [a, b, c] = triangle
                .sites
                .map(|id| mesh.sites[id.raw() as usize].direction.components());
            let ab = subtract(b, a);
            let ac = subtract(c, a);
            let normal = cross(ab, ac);
            let radial = add(add(a, b), c);
            assert!(dot(normal, radial) > 0.0);
        }
        assert!(mesh.edge_incidence.iter().all(|edge| {
            edge.sites[0] < edge.sites[1] && edge.triangles[0] < edge.triangles[1]
        }));
    }

    fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }

    fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
}
