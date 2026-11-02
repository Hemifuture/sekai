// SPDX-License-Identifier: AGPL-3.0-only
// Ported from Civ Atlas util.ts and civ/engine/rand.ts; see NOTICE.md.

#[derive(Debug, Clone)]
pub(crate) struct Mulberry32(u32);

impl Mulberry32 {
    pub(crate) fn new(seed: u32) -> Self {
        Self(seed)
    }

    pub(crate) fn next_f64(&mut self) -> f64 {
        // JS Math.imul and bitwise operators keep only the low 32 bits.
        self.0 = self.0.wrapping_add(0x6d2b79f5);
        let mut t = (self.0 ^ (self.0 >> 15)).wrapping_mul(1 | self.0);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4294967296.0
    }
}

pub(crate) fn sub_seed(seed: u32, salt: &str) -> u32 {
    let mut h = seed ^ 0x9e3779b9;
    // Rust chars are Unicode scalars; JS charCodeAt hashes UTF-16 code units.
    for unit in salt.encode_utf16() {
        h = (h ^ u32::from(unit)).wrapping_mul(0x85ebca6b);
        h ^= h >> 13;
    }
    h
}

fn fmix(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^ (h >> 16)
}

pub(crate) fn keyed(base: u32, a: i32, b: i32, c: i32) -> f64 {
    let h = fmix(base ^ 0x2545f491);
    let h = fmix(h ^ (a as u32).wrapping_mul(0x9e3779b1));
    let h = fmix(h ^ (b as u32).wrapping_mul(0x85ebca77));
    let h = fmix(h ^ (c as u32).wrapping_mul(0xc2b2ae3d));
    f64::from(h) / 4294967296.0
}

pub(crate) fn round24(value: f64) -> f64 {
    if value.abs() < 1e290 {
        // Preserve the upstream Veltkamp split, including subnormals and signed zero.
        let c = value * 536870913.0;
        c - (c - value)
    } else {
        value
    }
}
