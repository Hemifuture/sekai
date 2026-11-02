// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas src/gen/util.ts; see NOTICE.md.
use crate::TerrainError;

pub(crate) fn check_cancel(cancel: &dyn Fn() -> bool) -> Result<(), TerrainError> {
    if cancel() {
        Err(TerrainError::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn piecewise(table: &[f64], x: f64) -> f64 {
    if x <= table[0] {
        return table[1];
    }
    for i in (2..table.len()).step_by(2) {
        if x <= table[i] {
            return table[i - 1]
                + ((table[i + 1] - table[i - 1]) * (x - table[i - 2])) / (table[i] - table[i - 2]);
        }
    }
    table[table.len() - 1]
}

pub(crate) fn order_by_key(key: &[f32]) -> Vec<usize> {
    let mut order: Vec<_> = (0..key.len()).collect();
    // Stable numeric order: JavaScript treats signed zeros as equal.
    order.sort_by(|&a, &b| {
        key[a]
            .partial_cmp(&key[b])
            .expect("finite climate ordering key")
    });
    order
}

/// Keep the upstream heap's child choice and equal-key placement exactly.
pub(crate) struct MinHeap {
    entries: Vec<(usize, f64)>,
}
impl MinHeap {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
        }
    }
    pub(crate) fn push(&mut self, id: usize, p: f64) {
        let mut i = self.entries.len();
        self.entries.push((id, p));
        while i > 0 {
            let parent = (i - 1) >> 1;
            if self.entries[parent].1 <= p {
                break;
            }
            self.entries[i] = self.entries[parent];
            i = parent;
        }
        self.entries[i] = (id, p);
    }
    pub(crate) fn pop(&mut self) -> Option<(usize, f64)> {
        let top = *self.entries.first()?;
        let last = self.entries.pop().expect("heap was nonempty");
        let n = self.entries.len();
        if n > 0 {
            let mut i = 0;
            loop {
                let mut c = 2 * i + 1;
                if c >= n {
                    break;
                }
                if c + 1 < n && self.entries[c + 1].1 < self.entries[c].1 {
                    c += 1;
                }
                if self.entries[c].1 >= last.1 {
                    break;
                }
                self.entries[i] = self.entries[c];
                i = c;
            }
            self.entries[i] = last;
        }
        Some(top)
    }
}
