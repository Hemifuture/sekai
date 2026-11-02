// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas valleys.ts. Segment reduction is fixed by canonical scale.
use crate::{
    river_geometry::{DenseRiver, VALLEY_MAX_WIDTH, VALLEY_MIN_WIDTH},
    util::smoothstep,
};
/// Source valley depth and exterior wall width, also used for the estuary bridge.
pub(crate) fn valley_profile(width: f64) -> [f64; 2] {
    let t = ((width - VALLEY_MIN_WIDTH) / (VALLEY_MAX_WIDTH - VALLEY_MIN_WIDTH)).clamp(0., 1.);
    [30. + 50. * t, 2.5 + 1.5 * t]
}
#[derive(Debug)]
struct Segment {
    a: [f64; 2],
    d: [f64; 2],
    length2: f64,
    depth: f64,
    flat: f64,
    wall: f64,
    reach: f64,
}
#[derive(Debug)]
pub(crate) struct Valleys {
    segments: Vec<Segment>,
    bins: Vec<Vec<usize>>,
    cols: usize,
    rows: usize,
    scale: f64,
    width: f64,
}
// Lookup acceleration only: this does not change the source support or sampling.
const BIN: f64 = 16.;
impl Valleys {
    pub(crate) fn new(lines: &[DenseRiver], threshold: f64, scale: f64, size: [f64; 2]) -> Self {
        let cols = (size[0] / BIN).ceil() as usize;
        let rows = (size[1] / BIN).ceil() as usize;
        let mut out = Self {
            segments: Vec::new(),
            bins: vec![Vec::new(); cols * rows],
            cols,
            rows,
            scale,
            width: size[0],
        };
        let min2 = (2. / scale).powi(2);
        for d in lines {
            let mut i = 0;
            while i + 1 < d.x.len() {
                let mut j = i + 1;
                while j < d.x.len() - 1
                    && (d.x[j] - d.x[i]).powi(2) + (d.y[j] - d.y[i]).powi(2) < min2
                {
                    j += 1;
                }
                let rw = 0.5 * (d.w[i] + d.w[j]);
                let [depth, wall_width] = valley_profile(rw);
                let fl = 0.5 * (d.f[i] + d.f[j]);
                let cf = if fl < threshold {
                    (fl.max(0.) / threshold).sqrt()
                } else {
                    1.
                };
                let flat = 0.5 * rw * scale;
                let wall = flat + wall_width * scale * if cf < 1. { 0.5 + 0.5 * cf } else { 1. };
                let reach = wall + 2. * scale;
                let a = [d.x[i] * scale, d.y[i] * scale];
                let b = [d.x[j] * scale, d.y[j] * scale];
                let delta = [b[0] - a[0], b[1] - a[1]];
                let segment = Segment {
                    a,
                    d: delta,
                    length2: delta[0] * delta[0] + delta[1] * delta[1],
                    depth: if cf < 1. { depth * cf } else { depth },
                    flat,
                    wall,
                    reach,
                };
                let idx = out.segments.len();
                out.segments.push(segment);
                let y0 =
                    (((a[1].min(b[1]) - reach) / scale / BIN).floor() as isize).max(0) as usize;
                let y1 = (((a[1].max(b[1]) + reach) / scale / BIN).floor() as isize)
                    .min(rows as isize - 1);
                for shift in [0., -size[0], size[0]] {
                    let x0 = (((a[0].min(b[0]) - reach) / scale + shift) / BIN).floor() as isize;
                    let x1 = (((a[0].max(b[0]) + reach) / scale + shift) / BIN).floor() as isize;
                    for y in y0 as isize..=y1 {
                        for x in x0.max(0)..=x1.min(cols as isize - 1) {
                            out.bins[y as usize * cols + x as usize].push(idx);
                        }
                    }
                }
                i = j;
            }
        }
        out
    }
    pub(crate) fn sample(&self, point: [f64; 2]) -> [f32; 2] {
        let x = point[0].rem_euclid(self.width);
        let y = point[1];
        let bx = (x / BIN).floor() as usize;
        let by = ((y / BIN).floor() as isize).clamp(0, self.rows as isize - 1) as usize;
        let mut carve = 0_f32;
        let mut calm = 0_f32;
        for &id in &self.bins[by * self.cols + bx] {
            let s = &self.segments[id];
            for shift in [0., -self.width, self.width] {
                let qx = x * self.scale - (s.a[0] + shift * self.scale);
                let qy = y * self.scale - s.a[1];
                let t = if s.length2 > 0. {
                    ((qx * s.d[0] + qy * s.d[1]) / s.length2).clamp(0., 1.)
                } else {
                    0.
                };
                let ex = qx - t * s.d[0];
                let ey = qy - t * s.d[1];
                let d2 = ex * ex + ey * ey;
                if d2 >= s.reach * s.reach {
                    continue;
                }
                let dist = d2.sqrt();
                let cv = s.depth * (1. - smoothstep(s.flat, s.wall, dist));
                if cv > carve as f64 {
                    carve = cv as f32;
                }
                let q = 1. - smoothstep(s.flat, s.reach, dist);
                if q > calm as f64 {
                    calm = q as f32;
                }
            }
        }
        [carve, calm]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_carve_window() {
        if !crate::geometry_reference::trig_matches() {
            eprintln!("host trig differs: skip exact river carve fixture");
            return;
        }
        let lines = crate::river_geometry::river_geometry(
            &crate::river_geometry::tests::fixture(),
            25.,
            2048.,
        );
        for (scale, c, q) in [
            (
                2.,
                crate::fine_reference::CARVE_2,
                crate::fine_reference::CALM_2,
            ),
            (
                4.,
                crate::fine_reference::CARVE_4,
                crate::fine_reference::CALM_4,
            ),
        ] {
            let v = Valleys::new(&lines, 25., scale, [2048., 1024.]);
            for y in 0..16 {
                for x in 0..16 {
                    let k = y * 16 + x;
                    let p = [
                        -4. + (x as f64 + 0.5) / scale,
                        20. + (y as f64 + 0.5) / scale,
                    ];
                    assert_eq!(
                        v.sample(p).map(f32::to_bits),
                        [c[k], q[k]],
                        "{scale}, {x}, {y}"
                    );
                }
            }
        }
    }
}
