//! Sparse red subdivision of the upstream spherical Delaunay faces.

use std::collections::{hash_map::Entry, TryReserveError};
use std::num::NonZeroU32;

use civ_atlas_terrain::Terrain;
use rustc_hash::FxHashMap;

use super::super::{check_cancelled, AtlasGenerationError};
use crate::engine::BuildCancellation;
use crate::world::natural::atlas_terrain::coast::CoastMap;

/// One positive-length atomic segment, shared by the two named dense leaf compartments.
#[derive(Debug)]
pub(super) struct Passage {
    pub(super) cells: [u32; 2],
    pub(super) vertices: [u32; 2],
}

/// Current assembly, borrowing the raw affine vertex directions of the working mesh.
pub(super) struct Support<'a> {
    pub(super) directions: &'a [[f64; 3]],
    pub(super) passages: Vec<Passage>,
    pub(super) leaf_nodes: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Node {
    vertices: [u32; 3],
    face: u32,
    first_child: Option<NonZeroU32>,
}

/// Geometry shared across successive local splits. The first vertices and first nodes retain
/// source site IDs and source face IDs respectively. Directions are never normalized here:
/// radial projection after dyadic affine interpolation is the original barycentric chart.
pub(super) struct Mesh {
    pub(super) directions: Vec<[f64; 3]>,
    nodes: Vec<Node>,
    midpoints: FxHashMap<[u32; 2], u32>,
    roots: u32,
}

type BuildResult<T> = Result<T, AtlasGenerationError>;

fn invalid(message: &str) -> AtlasGenerationError {
    AtlasGenerationError::Geometry(message.to_owned())
}

fn allocation(error: TryReserveError) -> AtlasGenerationError {
    AtlasGenerationError::Geometry(error.to_string())
}

pub(super) fn edge_key([a, b]: [u32; 2]) -> [u32; 2] {
    [a.min(b), a.max(b)]
}

impl Mesh {
    /// Create one working leaf per author triangle, preserving original face order and raw xyz.
    pub(super) fn new(terrain: &Terrain) -> BuildResult<Self> {
        Self::from_mesh(terrain.site_xyz(), terrain.site_triangles())
    }

    fn from_mesh(xyz: &[f32], triangles: &[usize]) -> BuildResult<Self> {
        let sites = xyz.len() / 3;
        let faces = triangles.len() / 3;
        if xyz.len() % 3 != 0 || triangles.len() % 3 != 0 || sites > u32::MAX as usize {
            return Err(invalid("invalid coastal source mesh dimensions"));
        }
        let roots = u32::try_from(faces).map_err(|_| invalid("too many coastal source faces"))?;
        let mut directions = Vec::new();
        directions.try_reserve_exact(sites).map_err(allocation)?;
        directions.extend(
            xyz.chunks_exact(3)
                .map(|p| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]),
        );
        let mut nodes = Vec::new();
        nodes.try_reserve_exact(faces).map_err(allocation)?;
        for (face, triangle) in triangles.chunks_exact(3).enumerate() {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
            if a.max(b).max(c) >= sites || a == b || b == c || c == a {
                return Err(invalid("invalid coastal source face vertices"));
            }
            nodes.push(Node {
                vertices: [a as u32, b as u32, c as u32],
                face: face as u32,
                first_child: None,
            });
        }
        Ok(Self {
            directions,
            nodes,
            midpoints: FxHashMap::default(),
            roots,
        })
    }

    /// Return the working node's three vertex IDs in its local chart order.
    pub(super) fn node_vertices(&self, node: u32) -> [u32; 3] {
        self.nodes[node as usize].vertices
    }

    /// Direction of the affine centroid, independent of later splits.
    pub(super) fn node_center(&self, node: u32) -> [f64; 3] {
        let [a, b, c] = self
            .node_vertices(node)
            .map(|v| self.directions[v as usize]);
        std::array::from_fn(|k| a[k] + b[k] + c[k])
    }

    /// Return the source face containing this working node.
    pub(super) fn source_face(&self, node: u32) -> usize {
        self.nodes[node as usize].face as usize
    }

    /// Raw affine half-edge point, also used by the actual split.
    pub(super) fn edge_center(&self, edge: [u32; 2]) -> [f64; 3] {
        let [a, b] = edge.map(|v| self.directions[v as usize]);
        std::array::from_fn(|k| (a[k] + b[k]) * 0.5)
    }

    /// Return a midpoint ID only if a real red split has already installed it.
    pub(super) fn existing_midpoint(&self, edge: [u32; 2]) -> Option<u32> {
        self.midpoints.get(&edge_key(edge)).copied()
    }

    /// Carry a previously measured point to its current atomic edge after local refinement.
    pub(super) fn segment_at(&self, mut edge: [u32; 2], point: [f64; 3]) -> [u32; 2] {
        let unit = |[x, y, z]: [f64; 3]| {
            crate::world::spatial::UnitVector3::new(x, y, z).expect("finite coastal edge direction")
        };
        let point = unit(point);
        while let Some(middle) = self.existing_midpoint(edge) {
            let start = unit(self.directions[edge[0] as usize]);
            let mid = unit(self.directions[middle as usize]);
            edge = if start.dot(point) >= start.dot(mid) {
                [edge[0], middle]
            } else {
                [middle, edge[1]]
            };
        }
        edge_key(edge)
    }

    /// Return the three corner children and the central (BC, CA, AB) child. An existing split
    /// returns its original IDs. All storage and midpoint checks precede changes to the topology.
    pub(super) fn split(&mut self, index: u32) -> BuildResult<[u32; 4]> {
        let parent = *self
            .nodes
            .get(index as usize)
            .ok_or_else(|| invalid("invalid coastal split node"))?;
        if let Some(first) = parent.first_child {
            return Ok(std::array::from_fn(|i| first.get() + i as u32));
        }
        let end = self
            .nodes
            .len()
            .checked_add(4)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| invalid("too many coastal subdivision nodes"))?;
        let first = end - 4;
        let [a, b, c] = parent.vertices;
        let edges = [[a, b], [b, c], [c, a]].map(edge_key);
        let mut missing = 0_usize;
        let mut staged = [None; 3];
        let mut mids = [0; 3];
        for (i, edge) in edges.iter().copied().enumerate() {
            if let Some(&mid) = self.midpoints.get(&edge) {
                mids[i] = mid;
            } else {
                let p = self.edge_center(edge);
                if p == self.directions[edge[0] as usize]
                    || p == self.directions[edge[1] as usize]
                    || p == [0.; 3]
                {
                    return Err(invalid(
                        "coastal edge cannot be subdivided at this precision",
                    ));
                }
                let next = self
                    .directions
                    .len()
                    .checked_add(missing)
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| invalid("too many coastal vertices"))?;
                staged[i] = Some(p);
                mids[i] = next;
                missing += 1;
            }
        }
        self.nodes.try_reserve(4).map_err(allocation)?;
        self.directions.try_reserve(missing).map_err(allocation)?;
        self.midpoints.try_reserve(missing).map_err(allocation)?;
        for i in 0..3 {
            if let Some(p) = staged[i] {
                self.midpoints.insert(edges[i], mids[i]);
                self.directions.push(p);
            }
        }
        let [ab, bc, ca] = mids;
        for vertices in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [bc, ca, ab]] {
            self.nodes.push(Node {
                vertices,
                face: parent.face,
                first_child: None,
            });
        }
        self.nodes[index as usize].first_child = NonZeroU32::new(first);
        Ok(std::array::from_fn(|i| first + i as u32))
    }

    /// Assemble dense leaf IDs, arbitrary-point lookup and exactly one passage per minimal
    /// shared subsegment. An unpaired edge or a cancelled assembly publishes no partial map.
    pub(super) fn assemble(
        &self,
        cancel: &BuildCancellation,
    ) -> BuildResult<(CoastMap, Support<'_>)> {
        const POLL: usize = 1024;
        check_cancelled(cancel)?;
        let leaf_count = self.nodes.len() - (self.nodes.len() - self.roots as usize) / 4;
        let mut tree = Vec::new();
        let mut leaf_nodes = Vec::new();
        tree.try_reserve_exact(self.nodes.len())
            .map_err(allocation)?;
        leaf_nodes
            .try_reserve_exact(leaf_count)
            .map_err(allocation)?;
        for (i, node) in self.nodes.iter().enumerate() {
            if i % POLL == 0 {
                check_cancelled(cancel)?;
            }
            if let Some(first) = node.first_child {
                tree.push(first.get());
            } else {
                tree.push(leaf_nodes.len() as u32);
                leaf_nodes.push(i as u32);
            }
        }
        let map = CoastMap::from_tree(self.roots, tree, leaf_nodes.len() as u32)
            .map_err(AtlasGenerationError::Geometry)?;
        // Splitting an edge adds one vertex and one atomic edge. Euler's relation holds for
        // these polygonal leaf boundaries at any imbalance and supplies an exact capacity.
        let count = self
            .directions
            .len()
            .checked_add(leaf_count)
            .and_then(|n| n.checked_sub(2))
            .ok_or_else(|| invalid("invalid coastal sphere edge count"))?;
        u32::try_from(count).map_err(|_| invalid("too many coastal passages"))?;
        let mut passages = Vec::new();
        passages.try_reserve_exact(count).map_err(allocation)?;
        let mut pending = FxHashMap::default();
        let mut segments = Vec::new();
        let mut until_poll = 0;
        for (cell, &node) in leaf_nodes.iter().enumerate() {
            let [a, b, c] = self.node_vertices(node);
            for edge in [[a, b], [b, c], [c, a]] {
                segments.push(edge_key(edge));
                while let Some(ends) = segments.pop() {
                    if until_poll == 0 {
                        check_cancelled(cancel)?;
                        until_poll = POLL;
                    }
                    until_poll -= 1;
                    if let Some(&mid) = self.midpoints.get(&ends) {
                        // Stack order visits the first canonical half before its sibling.
                        segments.push(edge_key([mid, ends[1]]));
                        segments.push(edge_key([ends[0], mid]));
                    } else {
                        match pending.entry(ends) {
                            Entry::Vacant(entry) => {
                                entry.insert(cell as u32);
                            }
                            Entry::Occupied(entry) => {
                                let other = entry.remove();
                                passages.push(Passage {
                                    cells: [other, cell as u32],
                                    vertices: ends,
                                });
                            }
                        }
                    }
                }
            }
        }
        if !pending.is_empty() || passages.len() != count {
            return Err(invalid(
                "coastal source surface has unmatched or repeated edge segments",
            ));
        }
        Ok((
            map,
            Support {
                directions: &self.directions,
                passages,
                leaf_nodes,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_orders_share_subsegments_and_agree_with_point_queries() {
        let xyz = [
            1.,
            0.,
            0.,
            0.,
            1.,
            0.,
            0.,
            0.,
            1.,
            -0.57735026,
            -0.57735026,
            -0.57735026,
        ];
        let triangles = [0, 1, 2, 1, 0, 3, 2, 1, 3, 0, 2, 3];
        let mut mesh = Mesh::from_mesh(&xyz, &triangles).unwrap();
        let first = mesh.split(0).unwrap();
        let near_a = mesh.split(first[0]).unwrap();
        mesh.split(near_a[0]).unwrap();
        mesh.split(first[3]).unwrap();
        let opposite = mesh.split(1).unwrap();
        assert_eq!(mesh.split(0).unwrap(), first);
        let ab = mesh.node_vertices(first[3])[2];
        assert_eq!(mesh.node_vertices(opposite[3])[2], ab);
        assert_eq!(mesh.directions[ab as usize], [0.5, 0.5, 0.]);
        let quarter = mesh.existing_midpoint([0, ab]).unwrap();
        let eighth = mesh.existing_midpoint([0, quarter]).unwrap();
        for original in [[0, 1], [1, 0], [0, ab]] {
            assert_eq!(
                mesh.segment_at(original, [0.94, 0.06, 0.0]),
                edge_key([0, eighth])
            );
        }

        let (map, support) = mesh.assemble(&BuildCancellation::new()).unwrap();
        assert_eq!(map.cell_count(), 19);
        // At the three positive unit axes, raw Cartesian components are the original
        // chart's barycentric weights. This checks both corner and central recursion.
        for (cell, &node) in support
            .leaf_nodes
            .iter()
            .enumerate()
            .filter(|(_, &n)| mesh.source_face(n) == 0)
        {
            let verts = mesh
                .node_vertices(node)
                .map(|v| support.directions[v as usize]);
            let weights = std::array::from_fn(|k| (verts[0][k] + verts[1][k] + verts[2][k]) / 3.);
            assert_eq!(map.cell(0, weights), cell);
        }

        let mut intervals = Vec::new();
        let mut unique = std::collections::HashSet::new();
        for p in &support.passages {
            assert!(unique.insert(edge_key(p.vertices)));
            let faces = p
                .cells
                .map(|c| mesh.source_face(support.leaf_nodes[c as usize]));
            if faces != [0, 1] && faces != [1, 0] {
                continue;
            }
            let ends = p.vertices.map(|v| support.directions[v as usize]);
            assert!(ends.iter().all(|d| d[2] == 0.));
            let mut eighths = ends.map(|d| (d[1] * 8.) as u32);
            eighths.sort_unstable();
            intervals.push(eighths);
            let t = (ends[0][1] + ends[1][1]) * 0.5;
            let (a, b) = (0.99 * (1. - t), 0.99 * t);
            let mut wet_sides = p.cells;
            wet_sides.sort_unstable();
            let mut expected = [
                map.cell(0, [a, b, 0.01]) as u32,
                map.cell(1, [b, a, 0.01]) as u32,
            ];
            expected.sort_unstable();
            assert_eq!(wet_sides, expected);
        }
        intervals.sort_unstable();
        assert_eq!(intervals, [[0, 1], [1, 2], [2, 4], [4, 8]]);
    }
}
