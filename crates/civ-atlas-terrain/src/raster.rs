// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas raster.ts/rasterWindow.ts: spherical coverage, interpolation and blur.
use crate::{
    initial_sea_ice,
    mesh::{blur_field, Mesh},
    terrain::Terrain,
    util::{check_cancel, smoothstep},
    TerrainError,
};
use std::f64::consts::{PI, TAU};
pub(crate) const WIDTH: usize = crate::terrain::MAP_W as usize;
pub(crate) const HEIGHT: usize = crate::terrain::MAP_H as usize;
const BIN: usize = 16;
#[derive(Debug)]
pub(crate) struct Triangle {
    pub nodes: [usize; 3],
    pub normals: [[f64; 3]; 3],
}
#[derive(Debug)]
pub(crate) struct Coverage {
    pub triangles: Vec<Triangle>,
    bins: Vec<Vec<usize>>,
}
pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn arc_z(p: [f64; 3], q: [f64; 3], n: [f64; 3], ext: &mut [f64; 2]) {
    let nn = dot(n, n);
    if nn <= 0. {
        return;
    }
    let t2 = n[2] * n[2] / nn;
    if t2 >= 1. {
        return;
    }
    let m = [-n[2] * n[0], -n[2] * n[1], nn - n[2] * n[2]];
    let s1 = dot(cross(p, m), n);
    let s2 = dot(cross(m, q), n);
    let top = (1. - t2).sqrt();
    if s1 >= 0. && s2 >= 0. && top > ext[0] {
        ext[0] = top;
    }
    if s1 <= 0. && s2 <= 0. && -top < ext[1] {
        ext[1] = -top;
    }
}
impl Coverage {
    #[cfg(test)]
    pub(crate) fn support_tiles(&self, active: &[f64]) -> Vec<[usize; 2]> {
        self.bins
            .iter()
            .enumerate()
            .filter_map(|(k, bin)| {
                bin.iter()
                    .any(|&t| self.triangles[t].nodes.iter().any(|&i| active[i] != 0.))
                    .then_some([k % (WIDTH / BIN) * BIN, k / (WIDTH / BIN) * BIN])
            })
            .collect()
    }
    pub(crate) fn new(mesh: &Mesh, cancel: &dyn Fn() -> bool) -> Result<Self, TerrainError> {
        let mut out = Self {
            triangles: Vec::with_capacity(mesh.triangles.len() / 3),
            bins: vec![Vec::new(); WIDTH / BIN * (HEIGHT / BIN)],
        };
        for ids in mesh.triangles.chunks_exact(3) {
            check_cancel(cancel)?;
            let p: [[_; 3]; 3] =
                std::array::from_fn(|i| std::array::from_fn(|j| mesh.xyz[3 * ids[i] + j] as f64));
            let mut normals = [cross(p[0], p[1]), cross(p[1], p[2]), cross(p[2], p[0])];
            let vol = dot(p[0], normals[1]);
            let t = out.triangles.len();
            let mut ext = [
                p[0][2].max(p[1][2]).max(p[2][2]),
                p[0][2].min(p[1][2]).min(p[2][2]),
            ];
            for i in 0..3 {
                arc_z(p[i], p[(i + 1) % 3], normals[i], &mut ext);
            }
            if vol < 0. {
                for n in &mut normals {
                    for v in n {
                        *v = -*v;
                    }
                }
            }
            out.triangles.push(Triangle {
                nodes: [ids[0], ids[1], ids[2]],
                normals,
            });
            if vol == 0. {
                continue;
            }
            let north = normals.iter().all(|n| n[2] >= 0.);
            let south = normals.iter().all(|n| n[2] <= 0.);
            let to_y = |z: f64| (PI / 2. - z.clamp(-1., 1.).asin()) / PI * HEIGHT as f64;
            let y0 = if north {
                0
            } else {
                ((to_y(ext[0]) - 1.) / BIN as f64).floor().max(0.) as usize
            };
            let y1 = if south {
                HEIGHT / BIN - 1
            } else {
                ((to_y(ext[1]) + 1.) / BIN as f64)
                    .floor()
                    .min((HEIGHT / BIN - 1) as f64) as usize
            };
            let (mut x0, mut x1) = (0_isize, (WIDTH / BIN - 1) as isize);
            if !north && !south && p.iter().all(|p| p[2].abs() < 0.999) {
                let mut l = p.map(|p| p[1].atan2(p[0]));
                if l[0] > l[1] {
                    l.swap(0, 1)
                }
                if l[1] > l[2] {
                    l.swap(1, 2)
                }
                if l[0] > l[1] {
                    l.swap(0, 1)
                }
                let [g1, g2, g3] = [l[1] - l[0], l[2] - l[1], l[0] + TAU - l[2]];
                let gm = g1.max(g2).max(g3);
                if gm > PI * 1.1 {
                    let lo = if g3 == gm {
                        l[0]
                    } else if g1 == gm {
                        l[1]
                    } else {
                        l[2]
                    };
                    let hi = if g3 == gm {
                        l[2]
                    } else if g1 == gm {
                        l[0] + TAU
                    } else {
                        l[1] + TAU
                    };
                    x0 = (((lo + PI) / TAU * WIDTH as f64 - 1.) / BIN as f64).floor() as isize;
                    x1 = (((hi + PI) / TAU * WIDTH as f64 + 1.) / BIN as f64).floor() as isize;
                }
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    out.bins[y * (WIDTH / BIN) + x.rem_euclid((WIDTH / BIN) as isize) as usize]
                        .push(t);
                }
            }
        }
        Ok(out)
    }
    pub(crate) fn locate(&self, xy: [f64; 2], q: [f64; 3]) -> Option<(usize, [f64; 3])> {
        let x = (xy[0].rem_euclid(WIDTH as f64) / BIN as f64) as usize;
        let y = (xy[1] / BIN as f64)
            .floor()
            .clamp(0., (HEIGHT / BIN - 1) as f64) as usize;
        // Source scan overwrites shared-edge pixels in triangle order.
        self.bins[y * (WIDTH / BIN) + x]
            .iter()
            .rev()
            .find_map(|&t| {
                let n = &self.triangles[t].normals;
                let dc = dot(n[0], q);
                if dc < 0. {
                    return None;
                }
                let da = dot(n[1], q);
                if da < 0. {
                    return None;
                }
                let db = dot(n[2], q);
                if db < 0. {
                    return None;
                }
                let sum = da + db + dc;
                (sum > 0.).then(|| (t, [da / sum, db / sum, dc / sum]))
            })
    }
}
/// f32 trigonometric table boundaries of sphereGrid/rasterizeWindow.
pub(crate) fn frame(xy: [f64; 2]) -> [f64; 4] {
    let lon = xy[0] / WIDTH as f64 * TAU - PI;
    let lat = PI / 2. - xy[1] / HEIGHT as f64 * PI;
    [
        lon.cos() as f32 as f64,
        lon.sin() as f32 as f64,
        lat.cos() as f32 as f64,
        lat.sin() as f32 as f64,
    ]
}
pub(crate) fn direction(f: [f64; 4]) -> [f64; 3] {
    [f[2] * f[0], f[2] * f[1], f[3]]
}
#[derive(Debug)]
pub(crate) struct RasterBase {
    pub elevation: Vec<f32>,
    pub planes: Vec<[[f32; 3]; 5]>,
}
impl RasterBase {
    pub(crate) fn new(
        world: &Terrain,
        cover: &Coverage,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self, TerrainError> {
        let m = &world.mesh;
        let n = m.n;
        let mut signed = vec![0_f32; n];
        let mut ground = vec![0_f32; n];
        let mut lake = vec![0_f32; n];
        for i in 0..n {
            let e = world.elevation[i] as f64;
            let w = world.water[i];
            signed[i] = if w == 1 { e } else { e.max(8.) } as f32;
            if w == 2 {
                lake[i] = 1.;
                signed[i] = (world.water_level[i] as f64).max(8.) as f32;
            }
            ground[i] = if w == 1 {
                0.
            } else {
                if w == 2 {
                    world.water_level[i] as f64
                } else {
                    e
                }
                .max(0.)
            } as f32;
        }
        let mad = (0..n)
            .map(|i| {
                let ns = &m.adj[m.adj_start[i]..m.adj_start[i + 1]];
                let sum = ns
                    .iter()
                    .map(|&j| (ground[i] as f64 - ground[j] as f64).abs())
                    .sum::<f64>();
                if ns.is_empty() {
                    0.
                } else {
                    (sum / ns.len() as f64) as f32
                }
            })
            .collect::<Vec<_>>();
        let rel = blur_field(m, &mad, 2);
        let mut amp = vec![0_f32; n];
        let mut coast = vec![0_f32; n];
        let mut rug = vec![0_f32; n];
        for i in 0..n {
            let sea = world.water[i] == 1;
            let coastal = m.adj[m.adj_start[i]..m.adj_start[i + 1]]
                .iter()
                .any(|&j| (world.water[j] == 1) != sea);
            let r = rel[i] as f64;
            let la = (7. + r * (0.1 + 0.9 * smoothstep(40., 350., r))).min(520.);
            let sea_amp = 30. + 0.02 * -(world.elevation[i] as f64);
            amp[i] = if !sea {
                la
            } else if coastal {
                sea_amp.min(la.max(12.))
            } else {
                sea_amp
            } as f32;
            coast[i] = if coastal {
                1.
            } else if sea {
                1e-3
            } else {
                0.
            };
            rug[i] = smoothstep(60., 450., r) as f32;
        }
        let ice = initial_sea_ice::nodes(world, cancel)?;
        let fields = [&lake, &amp, &coast, &rug, &ice];
        let planes = cover
            .triangles
            .iter()
            .map(|t| {
                std::array::from_fn(|q| {
                    let f = fields[q];
                    let [a, b, c] = t.nodes;
                    [
                        (f[a] as f64 - f[c] as f64) as f32,
                        (f[b] as f64 - f[c] as f64) as f32,
                        f[c],
                    ]
                })
            })
            .collect();
        let mut elevation = vec![0_f32; WIDTH * HEIGHT];
        for y in 0..HEIGHT {
            check_cancel(cancel)?;
            for x in 0..WIDTH {
                let xy = [x as f64 + 0.5, y as f64 + 0.5];
                let (t, w) =
                    cover
                        .locate(xy, direction(frame(xy)))
                        .ok_or(TerrainError::InvalidOutput {
                            field: "sphere coverage",
                        })?;
                let a = w[0] as f32 as f64;
                let b = w[1] as f32 as f64;
                let c = 1. - a - b;
                let ids = cover.triangles[t].nodes;
                elevation[y * WIDTH + x] = (a * signed[ids[0]] as f64
                    + b * signed[ids[1]] as f64
                    + c * signed[ids[2]] as f64) as f32;
            }
        }
        let r = (m.spacing * 0.35 + 0.5).floor().max(1.) as usize;
        let mut scratch = vec![0_f32; elevation.len()];
        blur(&mut elevation, &mut scratch, r, cancel)?;
        blur(&mut elevation, &mut scratch, r, cancel)?;
        Ok(Self { elevation, planes })
    }
    pub(crate) fn fields(&self, t: usize, w: [f64; 3]) -> [f64; 5] {
        let a = w[0] as f32 as f64;
        let b = w[1] as f32 as f64;
        self.planes[t].map(|p| p[0] as f64 * a + p[1] as f64 * b + p[2] as f64)
    }
}
fn blur(
    src: &mut [f32],
    tmp: &mut [f32],
    r: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<(), TerrainError> {
    for y in 0..HEIGHT {
        check_cancel(cancel)?;
        let row = y * WIDTH;
        let cl = frame([0.5, y as f64 + 0.5])[2];
        let rx = ((r as f64 / cl.max(1e-3) + 0.5).floor() as usize).min((WIDTH - 1) / 2);
        let inv = 1. / (2 * rx + 1) as f64;
        let mut acc = 0.;
        for x in -(rx as isize)..=rx as isize {
            acc += src[row + x.rem_euclid(WIDTH as isize) as usize] as f64;
        }
        for x in 0..WIDTH {
            tmp[row + x] = (acc * inv) as f32;
            acc +=
                src[row + (x + rx + 1) % WIDTH] as f64 - src[row + (x + WIDTH - rx) % WIDTH] as f64;
        }
    }
    let mut acc = vec![0_f64; WIDTH];
    for y in -(r as isize)..=r as isize {
        let row = y.clamp(0, HEIGHT as isize - 1) as usize * WIDTH;
        for x in 0..WIDTH {
            acc[x] += tmp[row + x] as f64;
        }
    }
    let inv = 1. / (2 * r + 1) as f64;
    for y in 0..HEIGHT {
        check_cancel(cancel)?;
        let row = y * WIDTH;
        let add = (y + r + 1).min(HEIGHT - 1) * WIDTH;
        let sub = y.saturating_sub(r) * WIDTH;
        for x in 0..WIDTH {
            src[row + x] = (acc[x] * inv) as f32;
            acc[x] += tmp[add + x] as f64 - tmp[sub + x] as f64;
        }
    }
    Ok(())
}
pub(crate) fn spline(base: &[f32], xy: [f64; 2]) -> f64 {
    let [sx, sy] = xy;
    let xf = sx.floor();
    let yf = sy.floor();
    let tx = sx - xf;
    let ty = sy - yf;
    let ux = 1. - tx;
    let uy = 1. - ty;
    let bx = [
        ux * ux * ux / 6.,
        (3. * tx * tx * tx - 6. * tx * tx + 4.) / 6.,
        (-3. * tx * tx * tx + 3. * tx * tx + 3. * tx + 1.) / 6.,
        tx * tx * tx / 6.,
    ];
    let x0 = (xf as isize - 1).rem_euclid(WIDTH as isize) as usize;
    let x1 = (x0 + 1) % WIDTH;
    let x2 = (x1 + 1) % WIDTH;
    let x3 = (x2 + 1) % WIDTH;
    let mut v = 0.;
    for j in 0..4 {
        let r = (yf as isize - 1 + j).clamp(0, HEIGHT as isize - 1) as usize * WIDTH;
        let by = match j {
            0 => uy * uy * uy / 6.,
            1 => (3. * ty * ty * ty - 6. * ty * ty + 4.) / 6.,
            2 => (-3. * ty * ty * ty + 3. * ty * ty + 3. * ty + 1.) / 6.,
            _ => (ty * ty * ty) / 6.,
        };
        v += by
            * (bx[0] * base[r + x0] as f64
                + bx[1] * base[r + x1] as f64
                + bx[2] * base[r + x2] as f64
                + bx[3] * base[r + x3] as f64);
    }
    v
}
pub(crate) fn bilinear(base: &[f32], xy: [f64; 2]) -> f64 {
    let [sx, sy] = xy;
    let sy = sy.clamp(0., (HEIGHT - 1) as f64);
    let xf = sx.floor();
    let tx = sx - xf;
    let x0 = (xf as isize).rem_euclid(WIDTH as isize) as usize;
    let x1 = (x0 + 1) % WIDTH;
    let y0 = sy.floor() as usize;
    let y1 = (y0 + 1).min(HEIGHT - 1);
    let ty = sy - y0 as f64;
    let a = base[y0 * WIDTH + x0] as f64;
    let b = base[y0 * WIDTH + x1] as f64;
    let c = base[y1 * WIDTH + x0] as f64;
    let d = base[y1 * WIDTH + x1] as f64;
    let top = a + (b - a) * tx;
    let bot = c + (d - c) * tx;
    top + (bot - top) * ty
}
