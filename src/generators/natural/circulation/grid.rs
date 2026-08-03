use std::collections::BTreeMap;

use thiserror::Error;

use crate::world::natural::{CIRCULATION_SCHEMA_V1, MAX_CUBED_SPHERE_FACE_RESOLUTION};

use super::math::{
    add, central_angle, cross, dot, normalize, project_tangent, scale,
    spherical_triangle_area_unit, sub,
};

type VertexKey = (i64, i64, i64);
type EdgeKey = (u32, u32);

const UNIT_QUANTIZATION: f64 = 1.0e13;

#[derive(Debug, Clone, PartialEq)]
pub struct SphericalCell {
    id: u32,
    face: u8,
    row: u16,
    column: u16,
    center_unit: [f64; 3],
    area_m2: f64,
    edges: [u32; 4],
    neighbors: [u32; 4],
}

impl SphericalCell {
    pub const fn id(&self) -> u32 {
        self.id
    }

    pub const fn face(&self) -> u8 {
        self.face
    }

    pub const fn row(&self) -> u16 {
        self.row
    }

    pub const fn column(&self) -> u16 {
        self.column
    }

    pub const fn center_unit(&self) -> [f64; 3] {
        self.center_unit
    }

    pub const fn area_m2(&self) -> f64 {
        self.area_m2
    }

    pub const fn edges(&self) -> &[u32; 4] {
        &self.edges
    }

    pub const fn neighbors(&self) -> &[u32; 4] {
        &self.neighbors
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SphericalEdge {
    id: u32,
    vertices: [u32; 2],
    cells: [u32; 2],
    midpoint_unit: [f64; 3],
    length_m: f64,
    center_distance_m: f64,
    center_distances_to_midpoint_m: [f64; 2],
    normal_from_first: [f64; 3],
}

impl SphericalEdge {
    pub const fn id(&self) -> u32 {
        self.id
    }

    pub const fn vertices(&self) -> &[u32; 2] {
        &self.vertices
    }

    pub const fn cells(&self) -> &[u32; 2] {
        &self.cells
    }

    pub const fn midpoint_unit(&self) -> [f64; 3] {
        self.midpoint_unit
    }

    pub const fn length_m(&self) -> f64 {
        self.length_m
    }

    pub const fn center_distance_m(&self) -> f64 {
        self.center_distance_m
    }

    pub const fn center_distances_to_midpoint_m(&self) -> &[f64; 2] {
        &self.center_distances_to_midpoint_m
    }

    pub const fn normal_from_first(&self) -> [f64; 3] {
        self.normal_from_first
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CubedSphereGrid {
    face_resolution: u16,
    radius_m: f64,
    vertex_count: usize,
    cells: Vec<SphericalCell>,
    edges: Vec<SphericalEdge>,
    minimum_center_distance_m: f64,
    fingerprint: [u8; 32],
}

impl CubedSphereGrid {
    pub fn new(face_resolution: u16, radius_m: f64) -> Result<Self, CubedSphereGridError> {
        if !(1..=MAX_CUBED_SPHERE_FACE_RESOLUTION).contains(&face_resolution) {
            return Err(CubedSphereGridError::FaceResolutionOutOfRange {
                found: face_resolution,
                min: 1,
                max: MAX_CUBED_SPHERE_FACE_RESOLUTION,
            });
        }
        if !radius_m.is_finite() || radius_m <= 0.0 || !(radius_m * radius_m).is_finite() {
            return Err(CubedSphereGridError::InvalidRadius { found: radius_m });
        }

        let n = usize::from(face_resolution);
        let cell_count = 6_usize
            .checked_mul(n)
            .and_then(|value| value.checked_mul(n))
            .ok_or(CubedSphereGridError::AllocationOverflow)?;
        let edge_count = 2_usize
            .checked_mul(cell_count)
            .ok_or(CubedSphereGridError::AllocationOverflow)?;

        let mut vertices = Vec::with_capacity(cell_count + 2);
        let mut vertex_ids = BTreeMap::<VertexKey, u32>::new();
        let mut pending_cells = Vec::with_capacity(cell_count);
        let mut edge_owners = BTreeMap::<EdgeKey, Vec<u32>>::new();

        for face in 0_u8..6 {
            for row in 0..face_resolution {
                for column in 0..face_resolution {
                    let id = u32::try_from(pending_cells.len())
                        .map_err(|_| CubedSphereGridError::AllocationOverflow)?;
                    let corners = cell_corners(face, row, column, face_resolution)?;
                    let mut corner_ids = [0_u32; 4];
                    for (target, corner) in corner_ids.iter_mut().zip(corners) {
                        *target = weld_vertex(corner, &mut vertices, &mut vertex_ids)?;
                    }
                    let edge_keys = [
                        edge_key(corner_ids[0], corner_ids[1]),
                        edge_key(corner_ids[1], corner_ids[2]),
                        edge_key(corner_ids[2], corner_ids[3]),
                        edge_key(corner_ids[3], corner_ids[0]),
                    ];
                    for key in edge_keys {
                        edge_owners.entry(key).or_default().push(id);
                    }

                    let center_unit = face_point(
                        face,
                        (f64::from(column) + 0.5) / f64::from(face_resolution),
                        (f64::from(row) + 0.5) / f64::from(face_resolution),
                    )?;
                    let area_unit =
                        spherical_triangle_area_unit(corners[0], corners[1], corners[2])
                            + spherical_triangle_area_unit(corners[0], corners[2], corners[3]);
                    let area_m2 = area_unit * radius_m * radius_m;
                    if !area_m2.is_finite() || area_m2 <= 0.0 {
                        return Err(CubedSphereGridError::DegenerateGeometry);
                    }
                    pending_cells.push(PendingCell {
                        id,
                        face,
                        row,
                        column,
                        center_unit,
                        area_m2,
                        edge_keys,
                    });
                }
            }
        }

        if edge_owners.len() != edge_count {
            return Err(CubedSphereGridError::UnexpectedEdgeCount {
                expected: edge_count,
                found: edge_owners.len(),
            });
        }

        let mut edges = Vec::with_capacity(edge_count);
        let mut edge_ids = BTreeMap::<EdgeKey, u32>::new();
        for (key, owners) in edge_owners {
            if owners.len() != 2 {
                return Err(CubedSphereGridError::NonManifoldEdge {
                    vertices: [key.0, key.1],
                    owners: owners.len(),
                });
            }
            let mut cells = [owners[0], owners[1]];
            cells.sort_unstable();
            let first_center = pending_cells[cells[0] as usize].center_unit;
            let second_center = pending_cells[cells[1] as usize].center_unit;
            let first_vertex = vertices[key.0 as usize];
            let second_vertex = vertices[key.1 as usize];
            let midpoint_unit = normalize(add(first_vertex, second_vertex))
                .ok_or(CubedSphereGridError::DegenerateGeometry)?;
            let raw_normal = project_tangent(cross(first_vertex, second_vertex), midpoint_unit);
            let mut normal_from_first =
                normalize(raw_normal).ok_or(CubedSphereGridError::DegenerateGeometry)?;
            if dot(normal_from_first, sub(second_center, first_center)) < 0.0 {
                normal_from_first = scale(normal_from_first, -1.0);
            }
            let length_m = central_angle(first_vertex, second_vertex) * radius_m;
            let center_distance_m = central_angle(first_center, second_center) * radius_m;
            let center_distances_to_midpoint_m = [
                central_angle(first_center, midpoint_unit) * radius_m,
                central_angle(second_center, midpoint_unit) * radius_m,
            ];
            if !length_m.is_finite()
                || length_m <= 0.0
                || !center_distance_m.is_finite()
                || center_distance_m <= 0.0
                || center_distances_to_midpoint_m
                    .iter()
                    .any(|distance| !distance.is_finite() || *distance <= 0.0)
            {
                return Err(CubedSphereGridError::DegenerateGeometry);
            }
            let id =
                u32::try_from(edges.len()).map_err(|_| CubedSphereGridError::AllocationOverflow)?;
            edge_ids.insert(key, id);
            edges.push(SphericalEdge {
                id,
                vertices: [key.0, key.1],
                cells,
                midpoint_unit,
                length_m,
                center_distance_m,
                center_distances_to_midpoint_m,
                normal_from_first,
            });
        }

        let minimum_center_distance_m = edges
            .iter()
            .map(SphericalEdge::center_distance_m)
            .fold(f64::INFINITY, f64::min);
        let mut cells = Vec::with_capacity(cell_count);
        for pending in pending_cells {
            let mut cell_edges = [0_u32; 4];
            let mut neighbors = [0_u32; 4];
            for local in 0..4 {
                let edge_id = edge_ids[&pending.edge_keys[local]];
                let owners = edges[edge_id as usize].cells;
                cell_edges[local] = edge_id;
                neighbors[local] = if owners[0] == pending.id {
                    owners[1]
                } else {
                    owners[0]
                };
            }
            cells.push(SphericalCell {
                id: pending.id,
                face: pending.face,
                row: pending.row,
                column: pending.column,
                center_unit: pending.center_unit,
                area_m2: pending.area_m2,
                edges: cell_edges,
                neighbors,
            });
        }

        let fingerprint =
            calculate_fingerprint(face_resolution, radius_m, &vertices, &cells, &edges);
        Ok(Self {
            face_resolution,
            radius_m,
            vertex_count: vertices.len(),
            cells,
            edges,
            minimum_center_distance_m,
            fingerprint,
        })
    }

    pub const fn face_resolution(&self) -> u16 {
        self.face_resolution
    }

    pub const fn radius_m(&self) -> f64 {
        self.radius_m
    }

    pub const fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn cells(&self) -> &[SphericalCell] {
        &self.cells
    }

    pub fn edges(&self) -> &[SphericalEdge] {
        &self.edges
    }

    pub const fn minimum_center_distance_m(&self) -> f64 {
        self.minimum_center_distance_m
    }

    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingCell {
    id: u32,
    face: u8,
    row: u16,
    column: u16,
    center_unit: [f64; 3],
    area_m2: f64,
    edge_keys: [EdgeKey; 4],
}

fn cell_corners(
    face: u8,
    row: u16,
    column: u16,
    resolution: u16,
) -> Result<[[f64; 3]; 4], CubedSphereGridError> {
    let n = f64::from(resolution);
    let left = f64::from(column) / n;
    let right = f64::from(column + 1) / n;
    let bottom = f64::from(row) / n;
    let top = f64::from(row + 1) / n;
    Ok([
        face_point(face, left, bottom)?,
        face_point(face, right, bottom)?,
        face_point(face, right, top)?,
        face_point(face, left, top)?,
    ])
}

fn face_point(face: u8, u: f64, v: f64) -> Result<[f64; 3], CubedSphereGridError> {
    let basis = face_basis(face)?;
    let alpha = -std::f64::consts::FRAC_PI_4 + u * std::f64::consts::FRAC_PI_2;
    let beta = -std::f64::consts::FRAC_PI_4 + v * std::f64::consts::FRAC_PI_2;
    normalize(add(
        basis.normal,
        add(
            scale(basis.u_axis, alpha.tan()),
            scale(basis.v_axis, beta.tan()),
        ),
    ))
    .ok_or(CubedSphereGridError::DegenerateGeometry)
}

#[derive(Debug, Clone, Copy)]
struct FaceBasis {
    normal: [f64; 3],
    u_axis: [f64; 3],
    v_axis: [f64; 3],
}

fn face_basis(face: u8) -> Result<FaceBasis, CubedSphereGridError> {
    let basis = match face {
        0 => FaceBasis {
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
        },
        1 => FaceBasis {
            normal: [-1.0, 0.0, 0.0],
            u_axis: [0.0, -1.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
        },
        2 => FaceBasis {
            normal: [0.0, 1.0, 0.0],
            u_axis: [-1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
        },
        3 => FaceBasis {
            normal: [0.0, -1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 0.0, 1.0],
        },
        4 => FaceBasis {
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
        },
        5 => FaceBasis {
            normal: [0.0, 0.0, -1.0],
            u_axis: [-1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
        },
        _ => return Err(CubedSphereGridError::InvalidFace { found: face }),
    };
    Ok(basis)
}

fn weld_vertex(
    point: [f64; 3],
    vertices: &mut Vec<[f64; 3]>,
    ids: &mut BTreeMap<VertexKey, u32>,
) -> Result<u32, CubedSphereGridError> {
    let key = (
        quantize_unit(point[0]),
        quantize_unit(point[1]),
        quantize_unit(point[2]),
    );
    if let Some(id) = ids.get(&key) {
        return Ok(*id);
    }
    let id = u32::try_from(vertices.len()).map_err(|_| CubedSphereGridError::AllocationOverflow)?;
    ids.insert(key, id);
    vertices.push(point);
    Ok(id)
}

fn edge_key(first: u32, second: u32) -> EdgeKey {
    if first < second {
        (first, second)
    } else {
        (second, first)
    }
}

fn quantize_unit(value: f64) -> i64 {
    (value * UNIT_QUANTIZATION).round() as i64
}

fn calculate_fingerprint(
    face_resolution: u16,
    radius_m: f64,
    vertices: &[[f64; 3]],
    cells: &[SphericalCell],
    edges: &[SphericalEdge],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"sekai.cubed-sphere-grid\0");
    hasher.update(&CIRCULATION_SCHEMA_V1.to_le_bytes());
    hasher.update(&face_resolution.to_le_bytes());
    hasher.update(&radius_m.to_bits().to_le_bytes());
    hasher.update(&(vertices.len() as u32).to_le_bytes());
    for vertex in vertices {
        for component in vertex {
            hasher.update(&quantize_unit(*component).to_le_bytes());
        }
    }
    hasher.update(&(cells.len() as u32).to_le_bytes());
    for cell in cells {
        hasher.update(&cell.id.to_le_bytes());
        hasher.update(&[cell.face]);
        hasher.update(&cell.row.to_le_bytes());
        hasher.update(&cell.column.to_le_bytes());
        for component in cell.center_unit {
            hasher.update(&quantize_unit(component).to_le_bytes());
        }
        let area_unit = cell.area_m2 / (radius_m * radius_m);
        hasher.update(&quantize_unit(area_unit).to_le_bytes());
        for edge in cell.edges {
            hasher.update(&edge.to_le_bytes());
        }
        for neighbor in cell.neighbors {
            hasher.update(&neighbor.to_le_bytes());
        }
    }
    hasher.update(&(edges.len() as u32).to_le_bytes());
    for edge in edges {
        hasher.update(&edge.id.to_le_bytes());
        for vertex in edge.vertices {
            hasher.update(&vertex.to_le_bytes());
        }
        for cell in edge.cells {
            hasher.update(&cell.to_le_bytes());
        }
        for component in edge.midpoint_unit {
            hasher.update(&quantize_unit(component).to_le_bytes());
        }
        for component in edge.normal_from_first {
            hasher.update(&quantize_unit(component).to_le_bytes());
        }
        hasher.update(&quantize_unit(edge.length_m / radius_m).to_le_bytes());
        hasher.update(&quantize_unit(edge.center_distance_m / radius_m).to_le_bytes());
        for distance in edge.center_distances_to_midpoint_m {
            hasher.update(&quantize_unit(distance / radius_m).to_le_bytes());
        }
    }
    *hasher.finalize().as_bytes()
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum CubedSphereGridError {
    #[error("cubed-sphere face resolution {found} is outside {min}..={max}")]
    FaceResolutionOutOfRange { found: u16, min: u16, max: u16 },
    #[error("sphere radius {found} must be positive, finite, and squareable")]
    InvalidRadius { found: f64 },
    #[error("cubed-sphere allocation arithmetic overflowed")]
    AllocationOverflow,
    #[error("cubed-sphere face ID {found} is invalid")]
    InvalidFace { found: u8 },
    #[error("cubed-sphere geometry contains a degenerate edge or cell")]
    DegenerateGeometry,
    #[error("cubed-sphere expected {expected} unique edges but built {found}")]
    UnexpectedEdgeCount { expected: usize, found: usize },
    #[error("edge {vertices:?} has {owners} owners instead of exactly two")]
    NonManifoldEdge { vertices: [u32; 2], owners: usize },
}
