// SPDX-License-Identifier: AGPL-3.0-only
// Ported from Civ Atlas noise.ts and simplex-noise 4.0.3; see NOTICE.md.
// Simplex implementation copyright (c) 2024 Jonas Wagner, MIT licensed.

use crate::random::Mulberry32;

// simplex-noise 4.0.3: Gustavson's 3D simplex skew, gradients and output scale.
const F3: f64 = 1.0 / 3.0;
const G3: f64 = 1.0 / 6.0;
const GRAD3: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
];

#[derive(Debug, Clone)]
pub(crate) struct Noise3 {
    perm: [u8; 512],
    gradients: Box<[[f64; 3]; 512]>,
}

impl Noise3 {
    pub(crate) fn new(seed: u32) -> Self {
        let mut rng = Mulberry32::new(seed);
        let mut perm = [0; 512];
        for (i, value) in perm[..256].iter_mut().enumerate() {
            *value = i as u8;
        }
        for i in 0..255 {
            let r = i + (rng.next_f64() * (256 - i) as f64) as usize;
            perm.swap(i, r);
        }
        let (first, second) = perm.split_at_mut(256);
        second.copy_from_slice(first);
        let gradients = Box::new(std::array::from_fn(|i| GRAD3[usize::from(perm[i] % 12)]));
        Self { perm, gradients }
    }

    // Production callers use finite surface coordinates whose skewed lattice fits i32.
    pub(crate) fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        let s = (x + y + z) * F3;
        let i = (x + s).floor() as i32;
        let j = (y + s).floor() as i32;
        let k = (z + s).floor() as i32;
        let t = (f64::from(i) + f64::from(j) + f64::from(k)) * G3;
        let x0 = x - (f64::from(i) - t);
        let y0 = y - (f64::from(j) - t);
        let z0 = z - (f64::from(k) - t);
        let (a, b) = if x0 >= y0 {
            if y0 >= z0 {
                ([1, 0, 0], [1, 1, 0])
            } else if x0 >= z0 {
                ([1, 0, 0], [1, 0, 1])
            } else {
                ([0, 0, 1], [1, 0, 1])
            }
        } else if y0 < z0 {
            ([0, 0, 1], [0, 1, 1])
        } else if x0 < z0 {
            ([0, 1, 0], [0, 1, 1])
        } else {
            ([0, 1, 0], [1, 1, 0])
        };
        let p1 = [
            x0 - a[0] as f64 + G3,
            y0 - a[1] as f64 + G3,
            z0 - a[2] as f64 + G3,
        ];
        let p2 = [
            x0 - b[0] as f64 + 2.0 * G3,
            y0 - b[1] as f64 + 2.0 * G3,
            z0 - b[2] as f64 + 2.0 * G3,
        ];
        let p3 = [
            x0 - 1.0 + 3.0 * G3,
            y0 - 1.0 + 3.0 * G3,
            z0 - 1.0 + 3.0 * G3,
        ];
        let [ii, jj, kk] = [(i & 255) as usize, (j & 255) as usize, (k & 255) as usize];
        let n0 = self.contribution([x0, y0, z0], [ii, jj, kk]);
        let n1 = self.contribution(p1, [ii + a[0], jj + a[1], kk + a[2]]);
        let n2 = self.contribution(p2, [ii + b[0], jj + b[1], kk + b[2]]);
        let n3 = self.contribution(p3, [ii + 1, jj + 1, kk + 1]);
        32.0 * (n0 + n1 + n2 + n3)
    }

    fn contribution(&self, [x, y, z]: [f64; 3], [i, j, k]: [usize; 3]) -> f64 {
        let mut t = 0.6 - x * x - y * y - z * z;
        if t < 0.0 {
            return 0.0;
        }
        let index = i + usize::from(self.perm[j + usize::from(self.perm[k])]);
        let [gx, gy, gz] = self.gradients[index];
        t *= t;
        t * t * (gx * x + gy * y + gz * z)
    }
}

pub(crate) fn fbm3(
    n: &Noise3,
    [x, y, z]: [f64; 3],
    octaves: usize,
    persistence: f64,
    lacunarity: f64,
) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut norm = 0.0;
    for octave in 0..octaves {
        let o = octave as f64;
        value += amplitude
            * n.sample(
                x * frequency + o * 17.3,
                y * frequency - o * 9.1,
                z * frequency + o * 5.7,
            );
        norm += amplitude;
        amplitude *= persistence;
        frequency *= lacunarity;
    }
    value / norm
}

pub(crate) fn ridged3(n: &Noise3, [x, y, z]: [f64; 3], octaves: usize, persistence: f64) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut norm = 0.0;
    let mut weight = 1.0;
    for octave in 0..octaves {
        let o = octave as f64;
        let mut r = 1.0
            - n.sample(
                x * frequency + o * 31.7,
                y * frequency + o * 7.7,
                z * frequency - o * 3.1,
            )
            .abs();
        r *= r;
        r *= weight;
        weight = (r * 2.0).min(1.0);
        value += amplitude * r;
        norm += amplitude;
        amplitude *= persistence;
        frequency *= 2.0;
    }
    value / norm
}

// Civ Atlas raster.ts consumes this periodic texture for authoritative fine elevation.
// Preconditions match upstream: size is a power of two; cells and octaves are positive.
pub(crate) fn tileable_fbm(
    seed: u32,
    size: usize,
    cells: usize,
    octaves: usize,
    persistence: f64,
) -> Vec<f32> {
    let mut out = vec![0.0_f32; size * size];
    let mut rng = Mulberry32::new(seed);
    const DIRS: usize = 256;
    let ux: [f32; DIRS] = std::array::from_fn(|i| {
        ((i as f64 / DIRS as f64) * std::f64::consts::PI * 2.0).cos() as f32
    });
    let uy: [f32; DIRS] = std::array::from_fn(|i| {
        ((i as f64 / DIRS as f64) * std::f64::consts::PI * 2.0).sin() as f32
    });
    let mut amplitude = 1.0;
    for octave in 0..octaves {
        let c = cells * (1 << octave);
        let mut gx = Vec::with_capacity(c * c);
        let mut gy = Vec::with_capacity(c * c);
        for _ in 0..c * c {
            let direction = (rng.next_f64() * DIRS as f64) as usize;
            gx.push(ux[direction]);
            gy.push(uy[direction]);
        }
        let ox = rng.next_f64() * c as f64;
        let oy = rng.next_f64() * c as f64;
        let frequency = c as f64 / size as f64;
        for py in 0..size {
            let v = py as f64 * frequency + oy;
            let vi = v as usize;
            let ty = v - vi as f64;
            let sy = fade(ty);
            let y0 = if vi >= c { vi - c } else { vi };
            let r0 = y0 * c;
            let r1 = if y0 + 1 == c { 0 } else { y0 + 1 } * c;
            for px in 0..size {
                let u = px as f64 * frequency + ox;
                let ui = u as usize;
                let tx = u - ui as f64;
                let sx = fade(tx);
                let c0 = if ui >= c { ui - c } else { ui };
                let c1 = if c0 + 1 == c { 0 } else { c0 + 1 };
                let n00 = f64::from(gx[r0 + c0]) * tx + f64::from(gy[r0 + c0]) * ty;
                let n10 = f64::from(gx[r0 + c1]) * (tx - 1.0) + f64::from(gy[r0 + c1]) * ty;
                let n01 = f64::from(gx[r1 + c0]) * tx + f64::from(gy[r1 + c0]) * (ty - 1.0);
                let n11 = f64::from(gx[r1 + c1]) * (tx - 1.0) + f64::from(gy[r1 + c1]) * (ty - 1.0);
                let a0 = n00 + (n10 - n00) * sx;
                let a1 = n01 + (n11 - n01) * sx;
                let pixel = &mut out[py * size + px];
                // TypedArray += computes in f64 and rounds only on assignment.
                *pixel = (f64::from(*pixel) + amplitude * (a0 + (a1 - a0) * sy)) as f32;
            }
        }
        amplitude *= persistence;
    }
    let mut sum = 0.0;
    let mut square_sum = 0.0;
    for &value in &out {
        let value = f64::from(value);
        sum += value;
        square_sum += value * value;
    }
    let mean = sum / out.len() as f64;
    let inv = 1.0
        / (square_sum / out.len() as f64 - mean * mean)
            .max(1e-12)
            .sqrt();
    for value in &mut out {
        *value = ((f64::from(*value) - mean) * inv) as f32;
    }
    out
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
