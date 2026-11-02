// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas riverGeometry.ts; all coefficients are pinned author-shape priors.
use crate::terrain::River;
use std::collections::HashMap;

// The pinned valleys.ts style and estuary.ts drawnWidth share the source flux law.
pub(crate) const VALLEY_MIN_WIDTH: f64 = 0.5;
pub(crate) const VALLEY_MAX_WIDTH: f64 = 7.5;
pub(crate) fn width_fraction(flux: f64, threshold: f64) -> f64 {
    ((flux - threshold).max(0.) / 1400.).powf(0.65).min(1.)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DenseRiver {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub w: Vec<f64>,
    pub f: Vec<f64>,
    pub mouth: Option<usize>,
}
fn near_x(x: f64, other: f64, width: f64) -> f64 {
    x - width * ((x - other) / width + 0.5).floor()
}
fn smooth01(x: f64) -> f64 {
    if x <= 0. {
        0.
    } else if x >= 1. {
        1.
    } else {
        x * x * (3. - 2. * x)
    }
}
fn grad1(seed: u32, i: i32) -> f64 {
    let mut h = (seed ^ (i as u32).wrapping_mul(0x9e3779b1)).wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h as f64 / 2147483648. - 1.
}
fn noise1(seed: u32, t: f64) -> f64 {
    let i = t.floor() as i32;
    let f = t - i as f64;
    let a = grad1(seed, i) * f;
    let b = grad1(seed, i + 1) * (f - 1.);
    let u = f * f * f * (f * (f * 6. - 15.) + 10.);
    2. * (a + (b - a) * u)
}
fn hash_xy(p: [f64; 4]) -> u32 {
    let mut h = 0x811c9dc5_u32;
    for v in p {
        h = (h ^ ((v * 64. + 0.5).floor() as i32 as u32)).wrapping_mul(0x01000193);
        h ^= h >> 15;
    }
    h
}
fn meander(d: &mut DenseRiver, seed: u32, from_lake: bool, off: Option<[f64; 2]>) {
    let n = d.x.len();
    if n < 3 {
        return;
    }
    let mut s = vec![0.; n];
    for i in (0..n - 1).rev() {
        let ex = d.x[i + 1] - d.x[i];
        let ey = d.y[i + 1] - d.y[i];
        s[i] = s[i + 1] + (ex * ex + ey * ey).sqrt();
    }
    let total = s[0];
    let mut delta = vec![[0.; 2]; n];
    for i in 0..n {
        let a = i.saturating_sub(1);
        let b = (i + 1).min(n - 1);
        let mut tx = d.x[b] - d.x[a];
        let mut ty = d.y[b] - d.y[a];
        let tl = (tx * tx + ty * ty).sqrt();
        if tl == 0. {
            continue;
        }
        tx /= tl;
        ty /= tl;
        let mut v = 0.;
        for (o, (l, a)) in [(8., 0.2), (3.7, 0.17), (1.75, 0.13), (0.85, 0.09)]
            .into_iter()
            .enumerate()
        {
            v += a
                * l
                * noise1(
                    seed.wrapping_add((o as u32).wrapping_mul(0x632be5ab)),
                    s[i] / l,
                );
        }
        v *= smooth01(s[i] / 2.5);
        if from_lake {
            v *= smooth01((total - s[i]) / 2.5);
        }
        if i > 0 && i < n - 1 {
            let ux = d.x[i] - d.x[i - 1];
            let uy = d.y[i] - d.y[i - 1];
            let wx = d.x[i + 1] - d.x[i];
            let wy = d.y[i + 1] - d.y[i];
            let lu = (ux * ux + uy * uy).sqrt();
            let lw = (wx * wx + wy * wy).sqrt();
            let cross = (ux * wy - uy * wx).abs();
            if lu > 0. && lw > 0. && cross > 0. {
                let r = 0.5 * (lu + lw) * lu * lw / cross;
                v = v.clamp(-0.5 * r, 0.5 * r);
            }
        }
        delta[i] = [-ty * v, tx * v];
    }
    for i in 0..n {
        let [mut ox, mut oy] = delta[i];
        if let Some(off) = off {
            let b = 1. - smooth01(s[i] / 4.);
            ox += b * off[0];
            oy += b * off[1];
        }
        d.x[i] += ox;
        d.y[i] += oy;
    }
}
/// Reconnect main stems, smooth and densify the source river polylines.
pub(crate) fn river_geometry(rivers: &[River], threshold: f64, wrap: f64) -> Vec<DenseRiver> {
    let mut ids = HashMap::new();
    let mut vx = Vec::new();
    let mut vy = Vec::new();
    let mut vf = Vec::new();
    let mut next: Vec<Option<usize>> = Vec::new();
    for r in rivers {
        let mut prev = None;
        for p in r.pts.chunks_exact(3) {
            // JS string keys identify both signed zero representations as zero.
            let key = (
                if p[0] == 0. { 0 } else { p[0].to_bits() },
                if p[1] == 0. { 0 } else { p[1].to_bits() },
            );
            let v = *ids.entry(key).or_insert_with(|| {
                let v = vx.len();
                vx.push(p[0] as f64);
                vy.push(p[1] as f64);
                vf.push(p[2] as f64);
                next.push(None);
                v
            });
            if let Some(i) = prev {
                next[i] = Some(v);
            }
            prev = Some(v);
        }
    }
    let n = vx.len();
    let mut main: Vec<Option<usize>> = vec![None; n];
    for u in 0..n {
        if let Some(v) = next[u] {
            if main[v].is_none_or(|m| vf[u] > vf[m]) {
                main[v] = Some(u);
            }
        }
    }
    let mut chains = Vec::new();
    for (h, m) in main.iter().enumerate() {
        if m.is_some() {
            continue;
        }
        let mut c = vec![h];
        let mut cur = h;
        while let Some(v) = next[cur] {
            c.push(v);
            if main[v] != Some(cur) {
                break;
            }
            cur = v;
        }
        if c.len() >= 2 {
            chains.push(c);
        }
    }
    let peak = |c: &Vec<usize>| {
        vf[if next[c[c.len() - 1]].is_some() {
            c[c.len() - 2]
        } else {
            c[c.len() - 1]
        }]
    };
    chains.sort_by(|a, b| peak(b).partial_cmp(&peak(a)).expect("finite river flux"));
    let mut smoothed: HashMap<usize, [f64; 2]> = HashMap::new();
    let mut moved = HashMap::new();
    let mut out = Vec::with_capacity(chains.len());
    for c in chains {
        let m = c.len();
        let mut ox = c.iter().map(|&v| vx[v]).collect::<Vec<_>>();
        let oy = c.iter().map(|&v| vy[v]).collect::<Vec<_>>();
        let f = c.iter().map(|&v| vf[v]).collect::<Vec<_>>();
        if wrap > 0. {
            for i in 1..m {
                ox[i] = near_x(ox[i], ox[i - 1], wrap);
            }
        }
        let last = c[m - 1];
        let joins = next[last].is_some();
        let join_pos = joins.then(|| {
            let p = smoothed
                .get(&last)
                .copied()
                .unwrap_or([ox[m - 1], oy[m - 1]]);
            [
                if wrap > 0. {
                    near_x(p[0], ox[m - 1], wrap)
                } else {
                    p[0]
                },
                p[1],
            ]
        });
        let from_lake = m > 2 && f[0] == f[1];
        let mut xs = ox.clone();
        let mut ys = oy.clone();
        if let Some(p) = join_pos {
            xs[m - 1] = p[0];
            ys[m - 1] = p[1];
        }
        for _ in 0..2 {
            let mut sx = xs.clone();
            let mut sy = ys.clone();
            for i in 1..m - 1 {
                sx[i] = 0.25 * xs[i - 1] + 0.5 * xs[i] + 0.25 * xs[i + 1];
                sy[i] = 0.25 * ys[i - 1] + 0.5 * ys[i] + 0.25 * ys[i + 1];
            }
            xs = sx;
            ys = sy;
        }
        for i in 1..m - 1 {
            smoothed.insert(c[i], [xs[i], ys[i]]);
        }
        let mut fv = f.clone();
        if join_pos.is_some() {
            fv[m - 1] = fv[m - 2];
        }
        let first = usize::from(from_lake);
        if from_lake {
            xs.insert(0, 2. * ox[0] - ox[1]);
            ys.insert(0, 2. * oy[0] - oy[1]);
            fv.insert(0, fv[0]);
        }
        let mouth = !joins;
        if mouth {
            xs.push(2. * ox[m - 1] - ox[m - 2]);
            ys.push(2. * oy[m - 1] - oy[m - 2]);
            fv.push(fv[fv.len() - 1]);
        }
        let count = xs.len();
        let mut d = DenseRiver::default();
        let mut at = vec![0; count];
        for i in 0..count - 1 {
            let i0 = i.saturating_sub(1);
            let i3 = (i + 2).min(count - 1);
            let p0x = if i0 == i {
                2. * xs[i] - xs[i + 1]
            } else {
                xs[i0]
            };
            let p0y = if i0 == i {
                2. * ys[i] - ys[i + 1]
            } else {
                ys[i0]
            };
            let p3x = if i3 == i + 1 {
                2. * xs[i + 1] - xs[i]
            } else {
                xs[i3]
            };
            let p3y = if i3 == i + 1 {
                2. * ys[i + 1] - ys[i]
            } else {
                ys[i3]
            };
            let (p1x, p1y, p2x, p2y) = (xs[i], ys[i], xs[i + 1], ys[i + 1]);
            let sub = (((p2x - p1x).powi(2) + (p2y - p1y).powi(2)).sqrt() / 0.25)
                .ceil()
                .max(2.) as usize;
            for s in usize::from(i != 0)..=sub {
                let t = s as f64 / sub as f64;
                let t2 = t * t;
                let t3 = t2 * t;
                d.x.push(
                    0.5 * (2. * p1x
                        + (-p0x + p2x) * t
                        + (2. * p0x - 5. * p1x + 4. * p2x - p3x) * t2
                        + (-p0x + 3. * p1x - 3. * p2x + p3x) * t3),
                );
                d.y.push(
                    0.5 * (2. * p1y
                        + (-p0y + p2y) * t
                        + (2. * p0y - 5. * p1y + 4. * p2y - p3y) * t2
                        + (-p0y + 3. * p1y - 3. * p2y + p3y) * t3),
                );
                let u = if t < 0.5 { 0. } else { (t - 0.5) * 2. };
                let fl = fv[i] + (fv[i + 1] - fv[i]) * u * u * (3. - 2. * u);
                let mut mul = 1.;
                let pos = i as f64 - first as f64 + t;
                if !from_lake && pos < 1.5 {
                    mul *= 0.45 + 0.55 * (pos / 1.5);
                }
                if mouth {
                    let back = (count - 1) as f64 - (i as f64 + t);
                    if back < 2. {
                        mul *= 1. + 0.5 * (1. - back / 2.).powi(2);
                    }
                }
                d.w.push(
                    (VALLEY_MIN_WIDTH
                        + (VALLEY_MAX_WIDTH - VALLEY_MIN_WIDTH) * width_fraction(fl, threshold))
                        * mul,
                );
                d.f.push(fl);
            }
            at[i + 1] = d.x.len() - 1;
        }
        let off = if join_pos.is_some() {
            moved.get(&last).copied()
        } else {
            None
        };
        if mouth {
            d.mouth = Some(at[count - 2]);
        }
        meander(
            &mut d,
            hash_xy([vx[last], vy[last], vx[c[m - 2]], vy[c[m - 2]]]),
            from_lake,
            off,
        );
        for i in 1..m - 1 {
            let j = at[i + first];
            moved.insert(c[i], [d.x[j] - xs[i + first], d.y[j] - ys[i + first]]);
        }
        out.push(d);
    }
    out
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture() -> Vec<River> {
        [
            vec![
                2046., 20., 30., 2047., 22., 60., 1., 25., 110., 3., 27., 180.,
            ],
            vec![5., 20., 22., 3., 23., 31., 1., 25., 110.],
            vec![2042., 16., 30., 2043., 18., 30., 2046., 20., 30.],
        ]
        .into_iter()
        .map(|pts| River {
            pts,
            cells: Vec::new(),
        })
        .collect()
    }
    #[test]
    fn wrap_half_circle_uses_source_round_tie_direction() {
        for x in [-1024., 1024., 3072.] {
            assert_eq!(near_x(x, 0., 2048.), -1024.);
        }
    }
    #[test]
    fn source_main_stem_seam_and_lake_geometry() {
        let lines = river_geometry(&fixture(), 25., 2048.);
        assert!(lines
            .iter()
            .flat_map(|d| d.x.iter().chain(&d.y).chain(&d.w).chain(&d.f))
            .all(|v| v.is_finite()));
        if !crate::geometry_reference::trig_matches() {
            eprintln!("host trig differs: skip exact river geometry fixture");
            return;
        }
        assert_eq!(lines.len(), crate::fine_reference::DENSE.len());
        for (d, e) in lines.iter().zip(crate::fine_reference::DENSE) {
            for (a, b) in [(&d.x, e.0), (&d.y, e.1), (&d.w, e.2), (&d.f, e.3)] {
                assert_eq!(a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), b);
            }
            assert_eq!(d.mouth, e.4);
        }
    }
}
