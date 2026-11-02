// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas gully.ts: author-initialization erosion noise.
use std::{f64::consts::TAU, sync::LazyLock};
const LUT_N: usize = 1024;
const MAX_OCT: usize = 7;
static LUT: LazyLock<[[f32; 2]; LUT_N + 1]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let a = i as f64 / LUT_N as f64 * TAU;
        [a.cos() as f32, a.sin() as f32]
    })
});
fn hash3(seed: u32, p: [i32; 3]) -> u32 {
    let mut h = seed ^ (p[0] as u32).wrapping_mul(0x9e3779b1);
    h = (h ^ (h >> 16)).wrapping_mul(0x85ebca6b);
    h ^= (p[1] as u32).wrapping_mul(0x85ebca77);
    h = (h ^ (h >> 13)).wrapping_mul(0xc2b2ae35);
    h ^= (p[2] as u32).wrapping_mul(0xc2b2ae3d);
    h = (h ^ (h >> 16)).wrapping_mul(0x27d4eb2f);
    h ^ (h >> 15)
}
#[derive(Debug)]
struct Layer {
    seed: u32,
    cell: Option<[i32; 3]>,
    jitter: [[f64; 3]; 125],
}
impl Layer {
    fn at(&mut self, p: [f64; 3], d: [f64; 3]) -> [f64; 4] {
        let cell = p.map(|v| v.floor() as i32);
        if self.cell != Some(cell) {
            self.cell = Some(cell);
            let mut q = 0;
            for i in -2..=2 {
                for j in -2..=2 {
                    for k in -2..=2 {
                        let h = hash3(self.seed, [cell[0] + i, cell[1] + j, cell[2] + k]);
                        self.jitter[q] = [
                            i as f64 + (h & 1023) as f64 / 1024.,
                            j as f64 + ((h >> 10) & 1023) as f64 / 1024.,
                            k as f64 + ((h >> 20) & 1023) as f64 / 1024.,
                        ];
                        q += 1;
                    }
                }
            }
        }
        let f = std::array::from_fn::<_, 3, _>(|i| p[i] - cell[i] as f64);
        let gaps = f.map(|f| {
            [
                (f + 1.) * (f + 1.),
                f * f,
                0.,
                (1. - f) * (1. - f),
                (2. - f) * (2. - f),
            ]
        });
        let (mut wt, mut h, mut g) = (0., 0., 0.);
        for i in 0..5 {
            if gaps[0][i] >= 3. {
                continue;
            }
            for j in 0..5 {
                let axy = gaps[0][i] + gaps[1][j];
                if axy >= 3. {
                    continue;
                }
                for (k, &gap) in gaps[2].iter().enumerate() {
                    if axy + gap >= 3. {
                        continue;
                    }
                    let q = (i * 5 + j) * 5 + k;
                    let [vx, vy, vz] = std::array::from_fn(|l| f[l] - self.jitter[q][l]);
                    let d2 = vx * vx + vy * vy + vz * vz;
                    if d2 >= 3. {
                        continue;
                    }
                    let t = 1. - d2 / 3.;
                    let w = t * t * t;
                    wt += w;
                    let mut m = vx * d[0] + vy * d[1] + vz * d[2];
                    m -= m.floor();
                    let f = m * LUT_N as f64;
                    let i0 = f as usize;
                    let a = f - i0 as f64;
                    let c = LUT[i0].map(f64::from);
                    let n = LUT[i0 + 1].map(f64::from);
                    h += (c[0] + (n[0] - c[0]) * a) * w;
                    g -= (c[1] + (n[1] - c[1]) * a) * w;
                }
            }
        }
        let inv = if wt > 0. { 1. / wt } else { 0. };
        let s = g * inv * TAU;
        [h * inv, s * d[0], s * d[1], s * d[2]]
    }
}
/// Mutable locality cache belongs to the caller's sampling workspace, never terrain identity.
#[derive(Debug)]
pub(crate) struct Gullies {
    layers: [Layer; MAX_OCT],
}
impl Gullies {
    pub(crate) fn new(seed: u32) -> Self {
        Self {
            layers: std::array::from_fn(|i| Layer {
                seed: seed.wrapping_add((i as u32 + 1).wrapping_mul(0x632be5ab)),
                cell: None,
                jitter: [[0.; 3]; 125],
            }),
        }
    }
    pub(crate) fn height(&mut self, input: [f64; 11], scale: f64) -> f64 {
        let [x, y, z, ex, ey, nx, ny, nz, mut ge, mut gn, mut a] = input;
        let (mut out, mut cs) = (0., 5.);
        for layer in &mut self.layers {
            if cs * scale < 2. || a <= 1.5 {
                break;
            }
            let m = (ge * ge + gn * gn).sqrt();
            if m < 1e-6 {
                break;
            }
            let pe = -gn / m;
            let pn = ge / m;
            let b = layer.at(
                [x / cs, y / cs, z / cs],
                [pe * ex + pn * nx, pe * ey + pn * ny, pn * nz],
            );
            out += a * b[0];
            let sx = b[1] * a / cs;
            let sy = b[2] * a / cs;
            let sz = b[3] * a / cs;
            ge += sx * ex + sy * ey;
            gn += sx * nx + sy * ny + sz * nz;
            a *= 0.45;
            cs *= 0.5;
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn erosion_noise_matches_original_zoom_octaves() {
        let exact = crate::geometry_reference::trig_matches();
        for &(input, scale, expected) in crate::fine_reference::GULLY {
            let value = Gullies::new(42).height(input, scale);
            assert!(value.is_finite());
            if exact {
                assert_eq!(value.to_bits(), expected);
            }
        }
        if !exact {
            eprintln!("host trig differs: skip exact gully fixture");
        }
    }
}
