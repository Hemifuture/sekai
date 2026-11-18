//! Extend verified water-component labels across higher ground using the marker-rooted
//! minimum spanning forest of Cousty et al. (2009, doi:10.1109/TPAMI.2008.173).

#[cfg(not(target_arch = "wasm32"))]
use rayon::prelude::*;

use super::super::{check_cancelled, AtlasGenerationError};
use super::support::Passage;
use crate::engine::BuildCancellation;

/// Maximum graph entries between cooperative cancellation polls.
const POLL_INTERVAL: usize = 1024;

/// Accepted Kruskal edges, in canonical order, for the initially fixed markers.
pub(super) struct MarkerForest {
    edges: Vec<[u32; 2]>,
    initially_fixed: Vec<bool>,
}

impl MarkerForest {
    /// Build the minimum spanning forest for the disjoint `marine` and `closed` markers.
    /// The fixed bed supplies one sampled pass height per shared passage.
    /// Inconsistent marker arrays and cancellation return an error.
    pub(super) fn new(
        passages: &[Passage],
        bed: &[f64],
        marine: &[bool],
        closed: &[bool],
        cancel: &BuildCancellation,
    ) -> Result<Self, AtlasGenerationError> {
        let mut sets = RootedSets::new(marine, closed, None, cancel)?;
        let mut accepted = Vec::with_capacity(sets.unmarked_trees);
        if sets.unmarked_trees != 0 {
            let mut candidates = Vec::new();
            for batch in passages.chunks(POLL_INTERVAL) {
                check_cancelled(cancel)?;
                for edge in batch {
                    let [a, b] = edge.cells;
                    if sets.fixed[a as usize] && sets.fixed[b as usize] {
                        continue;
                    }
                    let [first, second] = edge.vertices.map(|v| bed[v as usize]);
                    let lower = first.min(second);
                    // The lower endpoint is the sampled pass above sea level.
                    // Nonnegative f64 bits sort by height, with both signed zeros at datum.
                    let spill = if lower > 0.0 { lower.to_bits() } else { 0 };
                    candidates.push((spill, a.min(b), a.max(b)));
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            candidates.par_sort_unstable();
            #[cfg(target_arch = "wasm32")]
            candidates.sort_unstable();
            for (i, (_, a, b)) in candidates.into_iter().enumerate() {
                if i % POLL_INTERVAL == 0 {
                    check_cancelled(cancel)?;
                }
                if sets.join([a, b]) {
                    accepted.push([a, b]);
                    if sets.unmarked_trees == 0 {
                        break;
                    }
                }
            }
        }
        Ok(Self {
            edges: accepted,
            initially_fixed: sets.fixed,
        })
    }

    /// Return the current marine catchment per compartment. Cells fixed at construction
    /// may change color and must stay marked. Components without a marker stay dry.
    /// The caller intersects the catchment with the original bed below sea level to
    /// obtain seawater. Inconsistent markers and cancellation return an error.
    pub(super) fn owners(
        &self,
        marine: &[bool],
        closed: &[bool],
        cancel: &BuildCancellation,
    ) -> Result<Vec<bool>, AtlasGenerationError> {
        let mut sets = RootedSets::new(marine, closed, Some(&self.initially_fixed), cancel)?;
        let mut ownership = marine.to_vec();
        if sets.unmarked_trees == 0 || !marine.contains(&true) {
            return Ok(ownership);
        }
        // Joining all markers to a virtual root before Kruskal makes every edge
        // skipped in the original pass a cycle. Adding root edges preserves those
        // cycles in every sorted prefix, so only the originally accepted edges matter.
        for batch in self.edges.chunks(POLL_INTERVAL) {
            check_cancelled(cancel)?;
            for &cells in batch {
                sets.join(cells);
            }
            if sets.unmarked_trees == 0 {
                break;
            }
        }
        for (i, owner) in ownership.iter_mut().enumerate() {
            if i % POLL_INTERVAL == 0 {
                check_cancelled(cancel)?;
            }
            if !sets.fixed[i] {
                *owner = marine[root(&mut sets.parent, i as u32) as usize];
            }
        }
        Ok(ownership)
    }
}

/// A marker remains the root of its tree. Ranks balance the unmarked trees.
struct RootedSets {
    parent: Vec<u32>,
    rank: Vec<u8>,
    fixed: Vec<bool>,
    unmarked_trees: usize,
}

impl RootedSets {
    fn new(
        marine: &[bool],
        closed: &[bool],
        initially_fixed: Option<&[bool]>,
        cancel: &BuildCancellation,
    ) -> Result<Self, AtlasGenerationError> {
        check_cancelled(cancel)?;
        if marine.len() != closed.len() || initially_fixed.is_some_and(|m| m.len() != marine.len())
        {
            return Err(AtlasGenerationError::Geometry(
                "coastal basin marker counts differ".into(),
            ));
        }
        let cells = u32::try_from(marine.len())
            .map_err(|_| AtlasGenerationError::Geometry("too many coastal basin cells".into()))?;
        let mut fixed = Vec::with_capacity(marine.len());
        let mut unmarked_trees = 0;
        for (i, (&ocean, &inland)) in marine.iter().zip(closed).enumerate() {
            if i % POLL_INTERVAL == 0 {
                check_cancelled(cancel)?;
            }
            if ocean && inland {
                return Err(AtlasGenerationError::Geometry(
                    "conflicting coastal basin markers".into(),
                ));
            }
            let marked = ocean || inland;
            if !marked && initially_fixed.is_some_and(|m| m[i]) {
                return Err(AtlasGenerationError::Geometry(
                    "initial coastal basin marker was removed".into(),
                ));
            }
            fixed.push(marked);
            unmarked_trees += usize::from(!marked);
        }
        Ok(Self {
            parent: (0..cells).collect(),
            rank: vec![0; marine.len()],
            fixed,
            unmarked_trees,
        })
    }

    fn join(&mut self, [a, b]: [u32; 2]) -> bool {
        let a = root(&mut self.parent, a);
        let b = root(&mut self.parent, b);
        if a == b || (self.fixed[a as usize] && self.fixed[b as usize]) {
            return false;
        }
        let priority = |i: u32| (self.fixed[i as usize], self.rank[i as usize]);
        let (pa, pb) = (priority(a), priority(b));
        let (keep, child) = if pa >= pb { (a, b) } else { (b, a) };
        self.parent[child as usize] = keep;
        if pa == pb {
            self.rank[keep as usize] += 1;
        }
        self.unmarked_trees -= 1;
        true
    }
}

/// Iterative path halving keeps large support graphs off the call stack.
fn root(parent: &mut [u32], mut cell: u32) -> u32 {
    while parent[cell as usize] != cell {
        let next = parent[cell as usize];
        parent[cell as usize] = parent[next as usize];
        cell = parent[cell as usize];
    }
    cell
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_owners(
        passages: &[Passage],
        bed: &[f64],
        marine: &[bool],
        closed: &[bool],
    ) -> Vec<bool> {
        let cancel = BuildCancellation::new();
        MarkerForest::new(passages, bed, marine, closed, &cancel)
            .unwrap()
            .owners(marine, closed, &cancel)
            .unwrap()
    }

    #[test]
    fn positive_rim_separates_a_closed_low_from_the_ocean_catchment() {
        // Seven incident triangles meet at the high central vertex. Each radial
        // passage has one lower endpoint. The ocean is in cell 0 and the low in 5.
        let bed = [18., 2., 3., 4., 6., 1., 2., 9.];
        let passages: Vec<_> = (0..7)
            .map(|i| Passage {
                cells: [i, (i + 1) % 7],
                vertices: [0, i + 1],
            })
            .collect();
        let mut marine = [false; 7];
        marine[0] = true;
        let mut closed = [false; 7];
        closed[5] = true;

        // Cell 3 reaches the ocean over passes of 2, 3 and 4. Its shortest
        // route to the closed low crosses 6. Only the highest pass counts.
        let map = fresh_owners(&passages, &bed, &marine, &closed);
        assert_eq!(map, [true, true, true, true, false, false, false]);
    }

    #[test]
    fn equal_spills_use_canonical_cells_and_a_markerless_component_stays_dry() {
        let bed = [3., 1.];
        let marine = [true, false, false, false, false];
        let closed = [false, false, true, false, false];
        let mut passages = [[2, 1], [3, 4], [0, 1]].map(|cells| Passage {
            cells,
            vertices: [0, 1],
        });
        for _ in 0..2 {
            let map = fresh_owners(&passages, &bed, &marine, &closed);
            assert_eq!(map, [true, true, false, false, false]);
            passages.reverse();
            for edge in &mut passages {
                edge.cells.reverse();
            }
        }
    }

    #[test]
    fn adding_and_recoloring_markers_matches_a_fresh_forest_with_tied_alternate_routes() {
        let bed = [8., 1., 2., 3., 4.];
        let passages = [
            ([0, 1], 2),
            ([1, 2], 3),
            ([2, 3], 1),
            ([3, 4], 3),
            ([4, 5], 2),
            ([5, 0], 4),
            ([1, 4], 3),
            ([2, 5], 3),
        ]
        .map(|(cells, level)| Passage {
            cells,
            vertices: [0, level],
        });
        let mut marine = [false; 6];
        let mut closed = [false; 6];
        closed[5] = true;
        let cancel = BuildCancellation::new();
        let forest = MarkerForest::new(&passages, &bed, &marine, &closed, &cancel).unwrap();
        let verify = |marine: &[bool], closed: &[bool], expected: [bool; 6]| {
            let reused = forest.owners(marine, closed, &cancel).unwrap();
            let fresh = fresh_owners(&passages, &bed, marine, closed);
            assert_eq!(reused, expected);
            assert_eq!(reused, fresh);
        };
        verify(&marine, &closed, [false; 6]);
        marine[0] = true;
        verify(&marine, &closed, [true, true, true, true, false, false]);
        closed[3] = true;
        closed[5] = false;
        marine[5] = true;
        verify(&marine, &closed, [true, true, false, false, true, true]);
        marine[2] = true;
        verify(&marine, &closed, [true, true, true, false, true, true]);
    }
}
