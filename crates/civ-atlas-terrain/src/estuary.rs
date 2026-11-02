// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas estuary.ts, excluding only plume/tint/biome writes with no height consumer.
use crate::{
    noise::value_noise,
    random::sub_seed,
    raster::{spline, HEIGHT, WIDTH},
    river_geometry::DenseRiver,
    shoreline::Raw,
    terrain::Terrain,
    util::{check_cancel, smoothstep},
    TerrainError,
};
use std::f64::consts::PI;
type Bounds = [f64; 4];
#[derive(Debug, Default)]
struct Line {
    x: Vec<f64>,
    y: Vec<f64>,
}
#[derive(Debug)]
struct Channel {
    line: Line,
    w0: f64,
    w1: f64,
}
#[derive(Debug)]
struct Delta {
    g: [f64; 2],
    o: [f64; 2],
    len: f64,
    half: f64,
    chans: Vec<Channel>,
    cbox: Bounds,
    bounds: Bounds,
}
#[derive(Debug)]
pub(crate) struct Mouth {
    axis: Line,
    s: Vec<f64>,
    al: Vec<f64>,
    s_end: f64,
    bridge: bool,
    vd: f64,
    vw: f64,
    end: [f64; 2],
    ed: [f64; 2],
    pub center: [f64; 2],
    d: [f64; 2],
    shore: [f64; 2],
    flux: f64,
    w0: f64,
    lc: f64,
    lup: f64,
    rwh: f64,
    delta: Option<Delta>,
    fbox: Bounds,
    seed: u32,
}
fn fbm(x: f64, y: f64, seed: u32) -> f64 {
    0.55 * value_noise(x / 2.6, y / 2.6, seed)
        + 0.3 * value_noise(x / 1.15, y / 1.15, seed + 17)
        + 0.15 * value_noise(x / 0.5, y / 0.5, seed + 31)
}
fn nearest(line: &Line, q: [f64; 2]) -> [f64; 2] {
    let (mut best, mut bi) = (f64::INFINITY, 0.);
    for j in 0..line.x.len() - 1 {
        let ax = line.x[j];
        let ay = line.y[j];
        let ex = line.x[j + 1] - ax;
        let ey = line.y[j + 1] - ay;
        let l2 = ex * ex + ey * ey;
        let t = if l2 > 0. {
            ((q[0] - ax) * ex + (q[1] - ay) * ey) / l2
        } else {
            0.
        }
        .clamp(0., 1.);
        let cx = ax + t * ex - q[0];
        let cy = ay + t * ey - q[1];
        let d2 = cx * cx + cy * cy;
        if d2 < best {
            best = d2;
            bi = j as f64 + t;
        }
    }
    [best.sqrt(), bi]
}
fn at(v: &[f64], i: f64) -> f64 {
    let j = (i.floor() as usize).min(v.len() - 2);
    v[j] + (v[j + 1] - v[j]) * (i - j as f64)
}
fn bounds(line: &Line, pad: f64) -> Bounds {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (&x, &y) in line.x.iter().zip(&line.y) {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    }
    [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]
}
fn union(a: Bounds, b: Bounds) -> Bounds {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}
fn inside(b: Bounds, q: [f64; 2]) -> bool {
    q[0] >= b[0] && q[1] >= b[1] && q[0] <= b[2] && q[1] <= b[3]
}
fn delta_field(d: &Delta, q: [f64; 2], nz: f64, seed: u32) -> f64 {
    let vx = q[0] - d.o[0];
    let vy = q[1] - d.o[1];
    let a = vx * d.g[0] + vy * d.g[1];
    let c = vy * d.g[0] - vx * d.g[1];
    let lobe = 0.84 + 0.32 * value_noise(c / (0.42 * d.len) + 11.3, 0.5, seed + 5);
    let ea = a / if a > 0. { d.len * lobe } else { 0.5 * d.len };
    let ec = c / d.half;
    let fine = value_noise(q[0] / 0.6, q[1] / 0.6, seed + 41) - 0.5;
    1.2 * (1. - (ea * ea + ec * ec).sqrt()) + 0.26 * (nz - 0.5) + 0.08 * fine
}
fn delta_box(d: &Delta) -> Bounds {
    let mut line = Line::default();
    let nx = -d.g[1];
    let ny = d.g[0];
    let r = 1. + (1.3 + 0.2) / 1.2;
    for a in [-0.5 * r * d.len, 1.16 * r * d.len] {
        for c in [-r * d.half, r * d.half] {
            line.x.push(d.o[0] + d.g[0] * a + nx * c);
            line.y.push(d.o[1] + d.g[1] * a + ny * c);
        }
    }
    bounds(&line, 0.5)
}
fn delta_of(m: &Mouth, rw: f64, sea: &dyn Fn([f64; 2]) -> bool) -> Delta {
    let [cx, cy] = m.center;
    let [dx, dy] = m.d;
    let f = m.flux;
    let rwh = m.rwh;
    let seed = m.seed;
    let mut gx = dx + m.shore[0];
    let mut gy = dy + m.shore[1];
    let gl = gx.hypot(gy);
    gx /= gl;
    gy /= gl;
    let len = 0.3 * f.sqrt();
    let ax = cx - dx * 1.2 * rw;
    let ay = cy - dy * 1.2 * rw;
    let l0 = 0.12 * len + 1.2 * rw;
    let p1 = [ax + dx * l0, ay + dy * l0];
    let mut d = Delta {
        g: [gx, gy],
        o: [p1[0] - gx * 0.2 * len, p1[1] - gy * 0.2 * len],
        len,
        half: 0.9 * len,
        chans: Vec::new(),
        cbox: [0.; 4],
        bounds: [0.; 4],
    };
    let rnd = |k: f64| value_noise(k * 3.1 + 0.37, 7.7, seed);
    let aim = |p: [f64; 2], dir: [f64; 2], angle: f64| {
        let exit = |v: [f64; 2]| {
            let mut t = 0.3 * len;
            while t < 2.2 * len
                && delta_field(
                    &d,
                    [p[0] + v[0] * t, p[1] + v[1] * t],
                    fbm(p[0] + v[0] * t, p[1] + v[1] * t, seed),
                    seed,
                ) > -0.14
            {
                t += 0.25;
            }
            t
        };
        for k in [1., 0.75, 0.5, 0.25, 0.] {
            let a = angle * k;
            let v = [
                dir[0] * a.cos() - dir[1] * a.sin(),
                dir[0] * a.sin() + dir[1] * a.cos(),
            ];
            let l = exit(v);
            if (sea([p[0] + v[0] * l, p[1] + v[1] * l])
                && sea([p[0] + v[0] * l * 0.6, p[1] + v[1] * l * 0.6]))
                || k == 0.
            {
                return [v[0], v[1], l];
            }
        }
        [dir[0], dir[1], 0.]
    };
    let chan = |a: [f64; 2], b: [f64; 2], width: [f64; 2], k: f64| {
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        let vx = -(b[1] - a[1]) / l;
        let vy = (b[0] - a[0]) / l;
        let b1 = (rnd(k) - 0.5) * 0.36 * l;
        let b2 = (rnd(k + 0.5) - 0.5) * 0.2 * l;
        let n = (l / 0.35).ceil().max(6.) as usize;
        let mut line = Line::default();
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let off = b1 * (PI * t).sin() + b2 * (2. * PI * t).sin();
            line.x.push(a[0] + (b[0] - a[0]) * t + vx * off);
            line.y.push(a[1] + (b[1] - a[1]) * t + vy * off);
        }
        Channel {
            line,
            w0: width[0],
            w1: width[1],
        }
    };
    let mut chans = vec![chan([ax, ay], p1, [rwh * 0.8, rwh * 0.75], 1.)];
    let one = if rnd(9.) < 0.5 { -1. } else { 1. };
    for side in [-1., 1.] {
        let [ux, uy, l] = aim(p1, [gx, gy], side * (0.55 + 0.24 * (rnd(side + 3.) - 0.5)));
        let w = rwh * if side == one { 0.72 } else { 0.58 } * (0.9 + 0.2 * rnd(side + 5.));
        if f > 500. || side == one {
            let q = 0.45 + 0.15 * rnd(side + 7.);
            let p2 = [p1[0] + ux * l * q, p1[1] + uy * l * q];
            chans.push(chan(p1, p2, [w, w * 0.95], side + 11.));
            for s2 in [-1., 1.] {
                let [vx, vy, l2] = aim(
                    p2,
                    [ux, uy],
                    s2 * (0.32 + 0.2 * (rnd(side * 4. + s2 + 13.) - 0.5)),
                );
                let w2 = w * if s2 == side { 0.8 } else { 0.65 };
                chans.push(chan(
                    p2,
                    [p2[0] + vx * l2, p2[1] + vy * l2],
                    [w2, w2 * 1.45],
                    side * 4. + s2 + 17.,
                ));
            }
        } else {
            chans.push(chan(
                p1,
                [p1[0] + ux * l, p1[1] + uy * l],
                [w, w * 1.45],
                side + 21.,
            ));
        }
    }
    d.chans = chans;
    let mut cb = bounds(&d.chans[0].line, 0.);
    for c in &d.chans {
        cb = union(cb, bounds(&c.line, 2.4 * c.w0.max(c.w1)));
    }
    d.cbox = cb;
    d.bounds = union(cb, delta_box(&d));
    d
}
struct Coast<'a> {
    whole: &'a [Raw],
    base: &'a [f32],
}
impl Coast<'_> {
    fn water(&self, p: [f64; 2]) -> u8 {
        let ix = (p[0].floor() as isize).rem_euclid(WIDTH as isize) as usize;
        let iy = (p[1].floor() as isize).clamp(0, HEIGHT as isize - 1) as usize;
        self.whole[iy * WIDTH + ix].water
    }
    fn height(&self, p: [f64; 2]) -> f64 {
        spline(
            self.base,
            [
                ((p[0] % WIDTH as f64 + WIDTH as f64) % WIDTH as f64) - 0.5,
                p[1] - 0.5,
            ],
        )
    }
}
fn bridge_of(
    end: [f64; 2],
    dir: [f64; 2],
    depth: f64,
    seed: u32,
    coast: &Coast<'_>,
) -> Option<(Line, [f64; 2])> {
    let [ex, ey] = end;
    let [dx, dy] = dir;
    let mut best = None;
    let mut straight = None;
    let mut best_cost = f64::INFINITY;
    let mut min_l = f64::INFINITY;
    for a in -6..=6 {
        let r = a as f64 * PI / 18.;
        let ux = dx * r.cos() - dy * r.sin();
        let uy = dx * r.sin() + dy * r.cos();
        let mut l = -1.;
        let mut top = 0_f64;
        for k in 1..=32 {
            let t = k as f64 * 0.25;
            let p = [ex + ux * t, ey + uy * t];
            if coast.water(p) == 1 {
                l = t;
                break;
            }
            top = top.max(coast.height(p));
        }
        if l < 0. {
            continue;
        }
        let ray = [l, ux, uy, top];
        if a == 0 {
            straight = Some(ray);
        }
        min_l = min_l.min(l);
        let cost = l * (1. + 0.6 * r.abs()) + (top - depth).max(0.) / 15.;
        if cost < best_cost {
            best_cost = cost;
            best = Some(ray);
        }
    }
    if let Some(s) = straight {
        if s[0] <= 6. && s[0] <= 1.6 * min_l && s[3] - depth <= 30. {
            best = Some(s);
        }
    }
    let [l, ux, uy, top] = best?;
    if top - depth > 30. {
        return None;
    }
    let tx = ex + ux * l;
    let ty = ey + uy * l;
    let cx = ex + dx * 0.5 * l;
    let cy = ey + dy * 0.5 * l;
    let b1 = (value_noise(0.37, 1.7, seed) - 0.5) * 0.3 * l;
    let b2 = (value_noise(3.1, 5.3, seed) - 0.5) * 0.16 * l;
    let n = (l / 0.5).ceil().max(2.) as usize;
    let mut line = Line::default();
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let a = (1. - t) * (1. - t);
        let b = 2. * (1. - t) * t;
        let c = t * t;
        let off = b1 * (PI * t).sin() + b2 * (2. * PI * t).sin();
        line.x.push(a * ex + b * cx + c * tx - uy * off);
        line.y.push(a * ey + b * cy + c * ty + ux * off);
    }
    let ol = (tx - cx).hypot(ty - cy);
    let ol = if ol == 0. { 1. } else { ol };
    Some((line, [(tx - cx) / ol, (ty - cy) / ol]))
}
pub(crate) fn mouths(
    world: &Terrain,
    lines: &[DenseRiver],
    whole: &[Raw],
    base: &[f32],
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Mouth>, TerrainError> {
    let coast = Coast { whole, base };
    let salt = sub_seed(world.params.seed, "estuary");
    let threshold = world.river_threshold;
    let mut out = Vec::new();
    for d in lines {
        check_cancel(cancel)?;
        let Some(i) = d.mouth else { continue };
        let n = d.x.len();
        let px = d.x[i];
        let py = d.y[i];
        let mut dx = d.x[n - 1] - px;
        let mut dy = d.y[n - 1] - py;
        let l = dx.hypot(dy);
        if l <= 0. {
            continue;
        }
        dx /= l;
        dy /= l;
        let mut kind = 0;
        for j in 0..=24 {
            if kind != 0 {
                break;
            }
            let t = j as f64 * 0.5;
            kind = coast.water([px + dx * t, py + dy * t]);
        }
        if kind != 1 {
            continue;
        }
        let f = d.f[i];
        if f < 0.3 * threshold {
            continue;
        }
        let w0 = 0.15 * f.sqrt();
        let lc = 1.1 * w0 + 0.5;
        let lup = 3. * lc + 2.;
        let seed = (salt
            ^ ((px * 7. + 0.5).floor() as i32 as u32).wrapping_mul(92821)
            ^ ((py * 7. + 0.5).floor() as i32 as u32).wrapping_mul(68917))
            & 0xffff;
        let [vd, vw] = crate::valleys::valley_profile(d.w[i]);
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let mut bridge = false;
        let ed = [dx, dy];
        let mut s = 0.;
        let mut j = i;
        let mut up = vec![[d.x[j], d.y[j]]];
        while j > 0 && s < lup + 6. {
            s += (d.x[j] - d.x[j - 1]).hypot(d.y[j] - d.y[j - 1]);
            j -= 1;
            up.push([d.x[j], d.y[j]]);
        }
        up.reverse();
        let mut k = 0;
        while k < up.len() {
            xs.push(up[k][0]);
            ys.push(up[k][1]);
            k += if up.len() - k > 2 { 2 } else { 1 };
        }
        for k in (i + 2..n).step_by(2) {
            xs.push(d.x[k]);
            ys.push(d.y[k]);
        }
        if xs.last() != Some(&d.x[n - 1]) {
            xs.push(d.x[n - 1]);
            ys.push(d.y[n - 1]);
        }
        let k_end = xs.len() - 1;
        if f >= threshold && coast.water([d.x[n - 1], d.y[n - 1]]) != 1 {
            if let Some((b, o)) = bridge_of([d.x[n - 1], d.y[n - 1]], [dx, dy], vd, seed, &coast) {
                bridge = true;
                xs.extend(b.x);
                ys.extend(b.y);
                dx = o[0];
                dy = o[1];
            }
        }
        xs.push(xs[xs.len() - 1] + dx * 8.);
        ys.push(ys[ys.len() - 1] + dy * 8.);
        let mut cum = vec![0.];
        for k in 1..xs.len() {
            cum.push(cum[k - 1] + (xs[k] - xs[k - 1]).hypot(ys[k] - ys[k - 1]));
        }
        let mut c_at = -1.;
        for k in 0..xs.len() - 1 {
            if c_at >= 0. {
                break;
            }
            let seg = cum[k + 1] - cum[k];
            let mut t = 0.;
            while t < seg {
                let u = t / seg;
                if coast.water([
                    xs[k] + (xs[k + 1] - xs[k]) * u,
                    ys[k] + (ys[k + 1] - ys[k]) * u,
                ]) == 1
                {
                    c_at = cum[k] + t;
                    break;
                }
                t += 0.25;
            }
        }
        let end_at = cum[k_end];
        if c_at < 0. || c_at > end_at + 8. + if bridge { 2. } else { 0. } {
            continue;
        }
        let along = cum.iter().map(|&c| c_at - c).collect::<Vec<_>>();
        let mut k0 = 0;
        while k0 < xs.len() - 1 && along[k0 + 1] >= 0. {
            k0 += 1;
        }
        let u0 = if along[k0] - along[k0 + 1] > 0. {
            along[k0] / (along[k0] - along[k0 + 1])
        } else {
            0.
        };
        let cx = xs[k0] + (xs[k0 + 1] - xs[k0]) * u0;
        let cy = ys[k0] + (ys[k0 + 1] - ys[k0]) * u0;
        let mut gx = 0.;
        let mut gy = 0.;
        for a in 0..24 {
            let angle = a as f64 / 24. * 2. * PI;
            let ca = angle.cos();
            let sa = angle.sin();
            for rr in [2., 4., 6.] {
                if coast.water([cx + ca * rr, cy + sa * rr]) == 1 {
                    gx += ca;
                    gy += sa;
                }
            }
        }
        let gl = gx.hypot(gy);
        let sx = if gl > 0. { gx / gl } else { dx };
        let sy = if gl > 0. { gy / gl } else { dy };
        let mut al = Vec::with_capacity(xs.len());
        for k in 0..xs.len() {
            let a = k.saturating_sub(1);
            let b = (k + 1).min(xs.len() - 1);
            let ex = xs[b] - xs[a];
            let ey = ys[b] - ys[a];
            let el = ex.hypot(ey);
            al.push(smoothstep(
                0.1,
                0.5,
                (ex * sx + ey * sy) / if el == 0. { 1. } else { el },
            ));
        }
        let flat = coast.height([cx - 3. * sx, cy - 3. * sy]) < 40.
            && coast.height([cx - 7. * sx, cy - 7. * sy]) < 100.;
        let shallow = coast.height([cx + 5. * sx, cy + 5. * sy]) > -250.;
        let open = gl / (24. * 3.) > 0.25;
        let is_delta = f >= 250. && flat && shallow && open && sx * dx + sy * dy > 0.4;
        let rw = 0.13 + 1.25 * crate::river_geometry::width_fraction(f, threshold);
        let rwh = 0.5 * rw * 1.25;
        let axis = Line { x: xs, y: ys };
        let fbox = bounds(
            &axis,
            (2. * (rwh + 0.6 * w0) + 1.).max(rwh + 0.1 + vw + 0.5),
        );
        let mut m = Mouth {
            axis,
            s: along,
            al,
            s_end: c_at - end_at,
            bridge,
            vd,
            vw,
            end: [d.x[n - 1], d.y[n - 1]],
            ed,
            center: [cx, cy],
            d: [dx, dy],
            shore: [sx, sy],
            flux: f,
            w0,
            lc,
            lup,
            rwh,
            delta: None,
            fbox,
            seed,
        };
        if is_delta {
            m.delta = Some(delta_of(&m, rw, &|p| coast.water(p) == 1));
        }
        out.push(m);
    }
    Ok(out)
}
impl Mouth {
    pub(crate) fn sample(&self, xy: [f64; 2], scale: f64, threshold: f64, raw: &mut Raw) {
        let sh = ((xy[0] - self.center[0]) / WIDTH as f64 + 0.5).floor() * WIDTH as f64;
        let q = [xy[0] - sh, xy[1]];
        // Source pixelsOf includes its outward rounded boundary pixels.
        let pixel_inside = |b: Bounds| {
            let px = q[0] * scale - 0.5;
            let py = q[1] * scale - 0.5;
            px >= (b[0] * scale).floor()
                && px <= (b[2] * scale).ceil()
                && py >= (b[1] * scale).floor()
                && py <= (b[3] * scale).ceil()
        };
        if let Some(d) = &self.delta {
            if pixel_inside(d.bounds) {
                let nz = fbm(q[0], q[1], self.seed);
                let f = delta_field(d, q, nz, self.seed);
                if f >= -1.3 {
                    if raw.water == 1 {
                        if f > 0. {
                            raw.height = (2.5_f64.min(3. * f)) as f32;
                            raw.water = 0;
                            raw.ice = 0.;
                        } else {
                            let t = -f;
                            let sh0 = -45. * t.powf(1.7);
                            let e1 = sh0 + (raw.height as f64 - sh0) * smoothstep(0.5, 1.3, t);
                            if e1 > raw.height as f64 {
                                raw.height = e1 as f32;
                            }
                        }
                    } else if raw.water == 0 && f > -0.2 {
                        let wv = smoothstep(-0.2, 0.5, f)
                            * (1. - smoothstep(5., 10., raw.height as f64));
                        if wv > 0. {
                            let e0 = (3. * f).clamp(0.3, 2.5);
                            if raw.height as f64 > e0 {
                                raw.height =
                                    (raw.height as f64 + (e0 - raw.height as f64) * wv) as f32;
                            }
                        }
                    }
                    if f >= -0.35 && inside(d.cbox, q) {
                        let wn = 0.85 + 0.3 * value_noise(q[0] / 2.6, q[1] / 2.6, self.seed);
                        let land0 = raw.water == 0;
                        for c in &d.chans {
                            let [dd, pos] = nearest(&c.line, q);
                            let u = pos / (c.line.x.len() - 1) as f64;
                            let hw = (c.w0 + (c.w1 - c.w0) * smoothstep(0.6, 1., u)) * wn;
                            if dd > 2.2 * hw {
                                continue;
                            }
                            if land0 && dd < 1.5 * hw && raw.height < 20. {
                                let ec = 2.5 * ((dd / hw).powi(2) - 1.);
                                if ec < raw.height as f64 {
                                    raw.height = ec as f32;
                                    if ec < 0. && raw.water == 0 {
                                        raw.water = 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !pixel_inside(self.fbox) {
            return;
        }
        let [dist, pos] = nearest(&self.axis, q);
        let s = at(&self.s, pos);
        let vtop = if self.bridge {
            self.s_end + 4.
        } else {
            f64::NEG_INFINITY
        };
        if s >= self.lup.max(vtop) || s <= -4. {
            return;
        }
        let nz = fbm(q[0], q[1], self.seed);
        let flare = if self.delta.is_some() {
            0.
        } else {
            0.5 * self.w0 * at(&self.al, pos)
        };
        let hw = (self.rwh + flare * (-s.max(0.) / self.lc).exp()) * (0.8 + 0.4 * nz);
        let ends = if self.delta.is_some() {
            smoothstep(-0.8, 0., s)
        } else {
            smoothstep(-4., -3., s)
        };
        let gap = if self.bridge {
            (1. - smoothstep(self.s_end + 0.5, self.s_end + 1.1, s)) * ends
        } else {
            0.
        };
        let gv = if self.bridge {
            (1. - smoothstep(
                0.,
                3.,
                -((q[0] - self.end[0]) * self.ed[0] + (q[1] - self.end[1]) * self.ed[1]),
            )) * ends
        } else {
            0.
        };
        let core = self.rwh + 0.1;
        if dist > 1.6 * hw && dist > if gv > 0. { core + self.vw } else { 1.2 * core } {
            return;
        }
        if raw.water != 0 {
            return;
        }
        let mut e = raw.height as f64;
        let v = if gv > 0. {
            (1. - smoothstep(core, core + self.vw, dist)) * gv
        } else {
            0.
        };
        if v > 0. && e > 4. {
            e = (e - self.vd * v).max(4.);
        }
        let lim = 14.
            * if s > 0. {
                1. - 0.6 * (s / self.lup)
            } else {
                1.6
            };
        let a1 = (1. - smoothstep(0.8 * hw, 1.2 * hw, dist))
            * (1. - smoothstep(0.7 * lim, lim, e))
            * ends
            * (1. - smoothstep(0.85 * self.lup, self.lup, s));
        let s_lim = if self.bridge {
            (0.6 * self.lc).max(self.s_end + 0.8)
        } else {
            0.6 * self.lc
        };
        let a2 = (1. - smoothstep(0.8 * core, 1.2 * core, dist))
            * (1. - smoothstep(s_lim - 0.3, s_lim + 0.3, s))
            * gap.max(1. - smoothstep(2.5 * 14., 4. * 14., e))
            * ends;
        let a = a1.max(a2);
        if a > 0. {
            let dep = 0.6
                + if self.flux < threshold { 1.2 } else { 4.5 }
                    * (1. - (dist / hw).powi(2)).max(0.);
            e += (-dep - e) * a;
            if e < 0. {
                raw.water = 1;
            }
        }
        raw.height = e as f32;
    }
}
