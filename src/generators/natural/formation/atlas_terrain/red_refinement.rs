//! Red hierarchy with disposable, one-edge green closure.
//!
//! Cervenka §2.6.1.1: a green child that needs refinement promotes its red
//! parent. Green geometry is never fed back as a new refinement parent.
use super::{geometry::SupportTriangle, AtlasGenerationError};
use crate::engine::BuildCancellation;
use crate::generators::natural::{geometric_midpoint, subdivision_children_from_midpoints};
use crate::world::spatial::{central_angle, UnitVector3};
use rustc_hash::FxHashMap;

struct RedLeaf {
    triangle: SupportTriangle,
    resolve: bool,
    at_spacing: bool,
    green: Option<(u8, u32)>,
}

pub(super) struct RedRefinement {
    leaves: Vec<RedLeaf>,
    midpoints: FxHashMap<[usize; 2], usize>,
    spacing: f64,
}

pub(super) struct ConformingMesh {
    pub triangles: Vec<SupportTriangle>,
    pub parents: Vec<usize>,
}

fn edge(a: usize, b: usize) -> [usize; 2] {
    [a.min(b), a.max(b)]
}

fn at_spacing(nodes: [usize; 3], directions: &[UnitVector3], spacing: f64) -> bool {
    (0..3).all(|i| central_angle(directions[nodes[i]], directions[nodes[(i + 1) % 3]]) <= spacing)
}

fn visit_leaf(parent: usize, leaf: &RedLeaf, visitor: &mut impl FnMut(usize, SupportTriangle)) {
    let (nodes, owner) = leaf.triangle;
    if let Some((side, midpoint)) = leaf.green {
        let i = usize::from(side);
        let m = midpoint as usize;
        let a = nodes[i];
        let b = nodes[(i + 1) % 3];
        let c = nodes[(i + 2) % 3];
        visitor(parent, ([a, m, c], owner));
        visitor(parent, ([m, b, c], owner));
    } else {
        visitor(parent, leaf.triangle);
    }
}

impl RedRefinement {
    pub(super) fn new(
        triangles: Vec<SupportTriangle>,
        directions: &[UnitVector3],
        spacing: f64,
    ) -> Result<Self, AtlasGenerationError> {
        // In this supported local spherical domain, normalized red midpoints
        // contract chord diameters. No planar similarity theorem is assumed.
        if triangles.iter().any(|(nodes, _)| {
            (0..3).any(|i| directions[nodes[i]].dot(directions[nodes[(i + 1) % 3]]) <= 0.0)
        }) {
            return Err(AtlasGenerationError::Geometry(
                "red root edge is outside the supported local spherical domain".into(),
            ));
        }
        Ok(Self {
            leaves: triangles
                .into_iter()
                .map(|triangle| RedLeaf {
                    triangle,
                    resolve: false,
                    at_spacing: at_spacing(triangle.0, directions, spacing),
                    green: None,
                })
                .collect(),
            midpoints: FxHashMap::default(),
            spacing,
        })
    }

    pub(super) fn leaf_count(&self) -> usize {
        self.leaves.len()
    }

    pub(super) fn inherited_resolution(&self, parent: usize) -> bool {
        self.leaves[parent].resolve
    }

    pub(super) fn visit_unresolved(&self, mut visitor: impl FnMut(usize, SupportTriangle)) {
        for (parent, leaf) in self.leaves.iter().enumerate() {
            // A green child lies in this local spherical red parent. Preserve
            // the exact angle predicate, computed once for immutable corners.
            if !leaf.at_spacing {
                visit_leaf(parent, leaf, &mut visitor);
            }
        }
    }

    pub(super) fn refine(
        &mut self,
        directions: &mut Vec<UnitVector3>,
        requested: &[bool],
        cancellation: &BuildCancellation,
    ) -> Result<(), AtlasGenerationError> {
        self.split(directions, requested, true, cancellation)?;
        loop {
            let promote = self
                .leaves
                .iter_mut()
                .map(|leaf| {
                    let (nodes, _) = leaf.triangle;
                    let mut count = 0;
                    let mut nested = false;
                    let mut green = None;
                    for i in 0..3 {
                        let a = nodes[i];
                        let b = nodes[(i + 1) % 3];
                        if let Some(&m) = self.midpoints.get(&edge(a, b)) {
                            count += 1;
                            green =
                                Some((i as u8, u32::try_from(m).expect("bounded support node ID")));
                            nested |= self.midpoints.contains_key(&edge(a, m))
                                || self.midpoints.contains_key(&edge(m, b));
                        }
                    }
                    leaf.green = green;
                    count >= 2 || nested
                })
                .collect::<Vec<_>>();
            if !promote.iter().any(|&v| v) {
                break;
            }
            self.split(directions, &promote, false, cancellation)?;
        }
        Ok(())
    }

    fn split(
        &mut self,
        directions: &mut Vec<UnitVector3>,
        marked: &[bool],
        requested: bool,
        cancellation: &BuildCancellation,
    ) -> Result<(), AtlasGenerationError> {
        let count = self.leaves.len() + 3 * marked.iter().filter(|&&v| v).count();
        super::geometry::guard_support_budget(count)?;
        let mut leaves = Vec::with_capacity(count);
        for (i, leaf) in self.leaves.drain(..).enumerate() {
            if i % 256 == 0 {
                super::check_cancelled(cancellation)?;
            }
            if !marked[i] {
                leaves.push(leaf);
                continue;
            }
            let (nodes, owner) = leaf.triangle;
            let mids = std::array::from_fn(|j| {
                let a = nodes[j];
                let b = nodes[(j + 1) % 3];
                *self.midpoints.entry(edge(a, b)).or_insert_with(|| {
                    let id = directions.len();
                    directions.push(geometric_midpoint(directions[a], directions[b]));
                    id
                })
            });
            leaves.extend(
                subdivision_children_from_midpoints(nodes, mids).map(|nodes| RedLeaf {
                    triangle: (nodes, owner),
                    resolve: leaf.resolve || requested,
                    at_spacing: at_spacing(nodes, directions, self.spacing),
                    green: None,
                }),
            );
        }
        self.leaves = leaves;
        Ok(())
    }

    pub(super) fn conforming_mesh(&self) -> Result<ConformingMesh, AtlasGenerationError> {
        let count = self
            .leaves
            .iter()
            .map(|leaf| 1 + usize::from(leaf.green.is_some()))
            .sum();
        super::geometry::guard_support_budget(count)?;
        let mut triangles = Vec::with_capacity(count);
        let mut parents = Vec::with_capacity(count);
        for (parent, leaf) in self.leaves.iter().enumerate() {
            visit_leaf(parent, leaf, &mut |parent, triangle| {
                triangles.push(triangle);
                parents.push(parent);
            });
        }
        Ok(ConformingMesh { triangles, parents })
    }
}
