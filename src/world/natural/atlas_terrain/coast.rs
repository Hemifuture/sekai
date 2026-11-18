//! Marine connectivity on the source sphere's selectively subdivided barycentric charts.

use serde::Serialize;

/// Compact red-split forest. Children are appended after their parent; dense leaves are numbered
/// in node order, so a value at most its own node index is a leaf ID. Every entry fits one u32.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CoastMap {
    roots: u32,
    tree: Vec<u32>,
    cells: u32,
    marine: Vec<u64>,
}

impl CoastMap {
    /// Bind the assembly's source roots, forward child pointers and dense leaf IDs to dry bits.
    /// Capacity and malformed forest dimensions are reported before publication.
    pub(crate) fn from_tree(roots: u32, tree: Vec<u32>, cells: u32) -> Result<Self, String> {
        let descendants = tree
            .len()
            .checked_sub(roots as usize)
            .ok_or("invalid coastal roots")?;
        if descendants % 4 != 0 || tree.len() - descendants / 4 != cells as usize {
            return Err("invalid coastal leaf count".to_owned());
        }
        let words = (cells as usize).div_ceil(u64::BITS as usize);
        let mut marine = Vec::new();
        marine.try_reserve_exact(words).map_err(|e| e.to_string())?;
        marine.resize(words, 0);
        Ok(Self {
            roots,
            tree,
            cells,
            marine,
        })
    }

    #[cfg(test)]
    pub(crate) fn unrefined(roots: u32) -> Self {
        Self::from_tree(roots, (0..roots).collect(), roots)
            .expect("unrefined fixture has one leaf per root")
    }

    /// Number of source charts in the author's triangle order.
    pub(crate) fn face_count(&self) -> usize {
        self.roots as usize
    }

    /// Number of separately floodable leaves.
    pub(crate) fn cell_count(&self) -> usize {
        self.cells as usize
    }

    /// Locate a point by source-face barycentric weights. At a shared midpoint the earliest
    /// incident corner owns the tie. The central child orders its vertices BC, CA, AB.
    pub(crate) fn cell(&self, face: usize, mut weights: [f64; 3]) -> usize {
        debug_assert!(face < self.face_count());
        let mut node = face;
        loop {
            let entry = self.tree[node] as usize;
            if entry <= node {
                return entry;
            }
            if let Some(corner) = weights.iter().position(|&w| w >= 0.5) {
                weights = weights.map(|w| 2.0 * w);
                weights[corner] -= 1.0;
                node = entry + corner;
            } else {
                weights = weights.map(|w| 1.0 - 2.0 * w);
                node = entry + 3;
            }
        }
    }

    /// Whether the resolved wet passages of this point's leaf reach a valid ocean source.
    pub(crate) fn is_marine(&self, face: usize, weights: [f64; 3]) -> bool {
        self.cell_is_marine(self.cell(face, weights))
    }

    /// Read the marine flood result for a dense leaf ID.
    pub(crate) fn cell_is_marine(&self, cell: usize) -> bool {
        debug_assert!(cell < self.cell_count());
        self.marine[cell / u64::BITS as usize] & (1 << (cell % u64::BITS as usize)) != 0
    }

    /// Mark a leaf reached from an ocean source in the current assembly.
    pub(crate) fn set_marine(&mut self, cell: usize) {
        debug_assert!(cell < self.cell_count());
        self.marine[cell / u64::BITS as usize] |= 1 << (cell % u64::BITS as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_ties_and_the_central_child_share_one_marine_bit_with_queries() {
        // Source face zero is split. The other root is the first dense leaf.
        let mut map = CoastMap::from_tree(2, vec![2, 0, 1, 2, 3, 4], 5).unwrap();
        assert_eq!(map.cell(0, [0.5, 0.5, 0.]), 1);
        assert_eq!(map.cell(0, [0., 0.5, 0.5]), 2);
        assert_eq!(map.cell(0, [0., 0., 1.]), 3);
        map.set_marine(4);
        assert!(map.is_marine(0, [1. / 3.; 3]));
        assert!(!map.is_marine(1, [1. / 3.; 3]));
    }
}
