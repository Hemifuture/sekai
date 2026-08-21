use crate::world::PlateId;

/// Deterministic disjoint-set storage shared by planar and spherical boundary aggregation.
pub(super) struct StableUnionFind {
    parent: Vec<usize>,
}

impl StableUnionFind {
    pub(super) fn new(count: usize) -> Self {
        Self {
            parent: (0..count).collect(),
        }
    }

    pub(super) fn find(&mut self, index: usize) -> usize {
        let parent = self.parent[index];
        if parent != index {
            self.parent[index] = self.find(parent);
        }
        self.parent[index]
    }

    pub(super) fn union(&mut self, first: usize, second: usize) {
        let first = self.find(first);
        let second = self.find(second);
        if first == second {
            return;
        }
        let (root, child) = if first < second {
            (first, second)
        } else {
            (second, first)
        };
        self.parent[child] = root;
    }
}

pub(super) fn normalized_plate_pair(first: PlateId, second: PlateId) -> [PlateId; 2] {
    if first < second {
        [first, second]
    } else {
        [second, first]
    }
}
