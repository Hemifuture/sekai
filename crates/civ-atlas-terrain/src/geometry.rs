//! Spherical geometry from pinned Civ Atlas geometry.ts (see NOTICE.md).
use crate::{
    mesh::Mesh,
    noise::{fbm3, ridged3, Noise3},
    random::round24,
};
use std::{
    cell::RefCell,
    f64::consts::{PI, TAU},
};
/// Return the upstream round24 sine of x in radians.
pub(super) fn sin(x: f64) -> f64 {
    round24(x.sin())
}
/// Return the upstream round24 cosine of x in radians.
pub(super) fn cos(x: f64) -> f64 {
    round24(x.cos())
}
/// Return the upstream round24 atan2 angle of y and x.
pub(super) fn atan(y: f64, x: f64) -> f64 {
    round24(y.atan2(x))
}
/// Return upstream round24 arcsine with its numerical-domain boundary handling.
pub(super) fn asin(x: f64) -> f64 {
    round24(x.clamp(-1., 1.).asin())
}
/// Return the squared length of a three-dimensional vector in upstream operation order.
pub(super) fn norm2(v: [f64; 3]) -> f64 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}
/// Convert equatorial circumference in world units to sphere radius.
pub(super) fn sphere_radius(width: f64) -> f64 {
    width / TAU
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[derive(Debug, Clone, Copy)]
struct PointVec {
    v: [f64; 3],
    sin_lon: f64,
    cos_lon: f64,
    sin_lat: f64,
    cos_lat: f64,
}
impl PointVec {
    fn new(point: [f64; 2], width: f64, height: f64) -> Self {
        let lon = (point[0] / width) * TAU - PI;
        let lat = PI / 2. - (point[1] / height) * PI;
        let sin_lon = sin(lon);
        let cos_lon = cos(lon);
        let sin_lat = sin(lat);
        let cos_lat = cos(lat);
        Self {
            v: [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat],
            sin_lon,
            cos_lon,
            sin_lat,
            cos_lat,
        }
    }
    fn east(&self, v: [f64; 3]) -> f64 {
        -self.sin_lon * v[0] + self.cos_lon * v[1]
    }
    fn south(&self, v: [f64; 3]) -> f64 {
        self.sin_lat * self.cos_lon * v[0] + self.sin_lat * self.sin_lon * v[1]
            - self.cos_lat * v[2]
    }
}
/// Cached local frames and spatial lookup for one immutable spherical mesh.
#[derive(Debug)]
pub(crate) struct SphereGeometry<'a> {
    mesh: &'a Mesh,
    r: f64,
    /// Total spherical surface area in world units squared.
    pub(crate) area: f64,
    ex: Vec<f64>,
    ey: Vec<f64>,
    sx: Vec<f64>,
    sy: Vec<f64>,
    sz: Vec<f64>,
    finder: SphereFinder,
}
impl<'a> SphereGeometry<'a> {
    /// Build geometry caches for the validated input mesh.
    pub(crate) fn new(mesh: &'a Mesh) -> Self {
        let r = sphere_radius(mesh.width);
        let mut geo = Self {
            mesh,
            r,
            area: 4. * PI * r * r,
            ex: vec![0.; mesh.n],
            ey: vec![0.; mesh.n],
            sx: vec![0.; mesh.n],
            sy: vec![0.; mesh.n],
            sz: vec![0.; mesh.n],
            finder: SphereFinder::new(mesh),
        };
        for i in 0..mesh.n {
            let [px, py, pz] = geo.p(i);
            let rho = (px * px + py * py).sqrt();
            if rho < 1e-12 {
                geo.ey[i] = 1.;
                geo.sx[i] = if pz > 0. { 1. } else { -1. };
                continue;
            }
            geo.ex[i] = -py / rho;
            geo.ey[i] = px / rho;
            geo.sx[i] = (pz * px) / rho;
            geo.sy[i] = (pz * py) / rho;
            geo.sz[i] = -rho;
        }
        geo
    }
    fn p(&self, i: usize) -> [f64; 3] {
        [
            f64::from(self.mesh.xyz[3 * i]),
            f64::from(self.mesh.xyz[3 * i + 1]),
            f64::from(self.mesh.xyz[3 * i + 2]),
        ]
    }
    fn point(&self, p: [f64; 2]) -> PointVec {
        PointVec::new(p, self.mesh.width, self.mesh.height)
    }
    fn vector_to_point(&self, p: [f64; 3]) -> [f64; 2] {
        let l = norm2(p).sqrt();
        let lon = atan(p[1], p[0]);
        let lat = if l > 0. { asin(p[2] / l) } else { 0. };
        let mut x = ((lon + PI) / TAU) * self.mesh.width;
        if x >= self.mesh.width {
            x -= self.mesh.width
        } else if x < 0. {
            x += self.mesh.width
        }
        [x, ((PI / 2. - lat) / PI) * self.mesh.height]
    }
    /// Return world-unit chord distance between mesh sites i and j.
    pub(crate) fn dist(&self, i: usize, j: usize) -> f64 {
        self.r * norm2(sub(self.p(i), self.p(j))).sqrt()
    }
    /// Return squared world-unit chord distance between mesh sites i and j.
    pub(crate) fn dist2(&self, i: usize, j: usize) -> f64 {
        self.r * self.r * norm2(sub(self.p(i), self.p(j)))
    }
    /// Return chord lengths in the same order as mesh.adj.
    pub(crate) fn edge_lengths(&self) -> Vec<f64> {
        let mut out = vec![0.; self.mesh.adj.len()];
        for i in 0..self.mesh.n {
            for (k, value) in out
                .iter_mut()
                .enumerate()
                .take(self.mesh.adj_start[i + 1])
                .skip(self.mesh.adj_start[i])
            {
                *value = self.dist(i, self.mesh.adj[k]);
            }
        }
        out
    }
    /// Return world-unit chord distance from site i to the projected point.
    pub(crate) fn dist_to(&self, i: usize, point: [f64; 2]) -> f64 {
        self.r * norm2(sub(self.p(i), self.point(point).v)).sqrt()
    }
    /// Return world-unit chord distance between two projected points.
    pub(crate) fn point_dist(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        self.r * norm2(sub(self.point(a).v, self.point(b).v)).sqrt()
    }
    /// Return squared distance when site i is within radius r of c, otherwise -1.
    pub(crate) fn near2(&self, i: usize, c: usize, r: f64) -> f64 {
        let d2 = self.dist2(i, c);
        if d2 <= r * r {
            d2
        } else {
            -1.
        }
    }
    /// Return distance when site i is within radius r of point p, otherwise -1.
    pub(crate) fn near_to(&self, i: usize, p: [f64; 2], r: f64) -> f64 {
        let d = self.dist_to(i, p);
        if d <= r {
            d
        } else {
            -1.
        }
    }
    /// Return site indices within radius r of c, in ascending index order.
    pub(crate) fn cells_near(&self, c: usize, r: f64) -> Vec<usize> {
        self.finder
            .within(self.mesh, self.p(c), r.abs() / self.r, |i| {
                self.near2(i, c, r) >= 0.
            })
    }
    /// Return site indices within radius r of a projected point, in ascending order.
    pub(crate) fn cells_near_point(&self, p: [f64; 2], r: f64) -> Vec<usize> {
        self.finder
            .within(self.mesh, self.point(p).v, r.abs() / self.r, |i| {
                self.near_to(i, p, r) >= 0.
            })
    }
    /// Project the direction from i to j onto local east/south velocity v.
    pub(crate) fn edge_dot(&self, i: usize, j: usize, v: [f64; 2]) -> f64 {
        let [dx, dy, dz] = sub(self.p(j), self.p(i));
        let t = (v[0] * self.ex[i] + v[1] * self.sx[i]) * dx
            + (v[0] * self.ey[i] + v[1] * self.sy[i]) * dy
            + v[1] * self.sz[i] * dz;
        let l = (dx * dx + dy * dy + dz * dz).sqrt();
        t / if l == 0. { 1. } else { l }
    }
    /// Project the direction from point a to b onto local east/south velocity v.
    pub(crate) fn point_dot(&self, a: [f64; 2], b: [f64; 2], v: [f64; 2]) -> f64 {
        let a = self.point(a);
        let d = sub(self.point(b).v, a.v);
        let t = v[0] * a.east(d) + v[1] * a.south(d);
        let l = norm2(d).sqrt();
        t / if l == 0. { 1. } else { l }
    }
    /// Return wind ordering at site i using optional band cuts and pre-shift y coordinate.
    pub(crate) fn downwind(
        &self,
        i: usize,
        v: [f64; 2],
        cuts: Option<&[f64]>,
        by: Option<f64>,
    ) -> f64 {
        let x = f64::from(self.mesh.x[i]);
        let y = f64::from(self.mesh.y[i]);
        let Some(cuts) = cuts else {
            return x * v[0] + y * v[1];
        };
        let b = ((by.unwrap_or(y) * 6. / self.mesh.height).floor() as isize).clamp(0, 5) as usize;
        let mut xr = x - cuts[b];
        if xr < 0. {
            xr += self.mesh.width
        }
        xr * v[0] + y * v[1]
    }
    /// Return each wind band longitude cut minimizing land in the upstream moving window.
    pub(crate) fn wind_cuts(&self, water: &[u8]) -> Vec<f64> {
        const BANDS: usize = 6;
        const BINS: usize = 256;
        const WINDOW: isize = 12;
        let mut counts = vec![0.; BANDS * BINS];
        for (i, &water) in water.iter().enumerate().take(self.mesh.n) {
            if water == 1 {
                continue;
            }
            let b = ((f64::from(self.mesh.y[i]) * BANDS as f64 / self.mesh.height).floor()
                as usize)
                .min(BANDS - 1);
            let c = ((f64::from(self.mesh.x[i]) * BINS as f64 / self.mesh.width).floor() as usize)
                .min(BINS - 1);
            counts[b * BINS + c] += 1.;
        }
        let mut cuts = vec![0.; BANDS];
        for (b, cut) in cuts.iter_mut().enumerate() {
            let mut best = 0;
            let mut best_value = f64::INFINITY;
            for c in 0..BINS {
                let mut v = 0.;
                for k in -WINDOW..=WINDOW {
                    v += counts[b * BINS + (c as isize + k + BINS as isize) as usize % BINS];
                }
                if v < best_value {
                    best_value = v;
                    best = c;
                }
            }
            *cut = ((best as f64 + 0.5) * self.mesh.width) / BINS as f64;
        }
        cuts
    }
    /// Move site i along a great circle by the ordered sum of east/south segments.
    pub(crate) fn move_cell(&self, i: usize, segments: [[f64; 2]; 3]) -> [f64; 2] {
        let e = segments[0][0] + segments[1][0] + segments[2][0];
        let s = segments[0][1] + segments[1][1] + segments[2][1];
        let l = (e * e + s * s).sqrt();
        if l == 0. {
            return [f64::from(self.mesh.x[i]), f64::from(self.mesh.y[i])];
        }
        let th = l / self.r;
        let ct = cos(th);
        let st = sin(th) / l;
        let p = self.p(i);
        self.vector_to_point([
            p[0] * ct + (e * self.ex[i] + s * self.sx[i]) * st,
            p[1] * ct + (e * self.ey[i] + s * self.sy[i]) * st,
            p[2] * ct + s * self.sz[i] * st,
        ])
    }
    /// Wrap point longitude and constrain projected latitude to the poles in place.
    pub(crate) fn clamp_point(&self, p: &mut [f64; 2]) {
        p[0] %= self.mesh.width;
        if p[0] < 0. {
            p[0] += self.mesh.width
        }
        p[1] = p[1].clamp(0., self.mesh.height);
    }
    /// Return the projected midpoint of mesh sites a and b.
    pub(crate) fn mid(&self, a: usize, b: usize) -> [f64; 2] {
        let a = self.p(a);
        let b = self.p(b);
        self.vector_to_point([a[0] + b[0], a[1] + b[1], a[2] + b[2]])
    }
    /// Construct angular velocities from plate centroid speeds and author spin values.
    pub(crate) fn plate_motion(
        &self,
        vx: &[f32],
        vy: &[f32],
        centers: (&[f32], &[f32]),
        spin: &[f64],
    ) -> PlateMotion {
        let mut omega = Vec::with_capacity(vx.len() * 3);
        for k in 0..vx.len() {
            let a = self.point([f64::from(centers.0[k]), f64::from(centers.1[k])]);
            let vx = f64::from(vx[k]);
            let vy = f64::from(vy[k]);
            let t = [
                -a.sin_lon * vx + a.sin_lat * a.cos_lon * vy,
                a.cos_lon * vx + a.sin_lat * a.sin_lon * vy,
                -a.cos_lat * vy,
            ];
            let sp = (vx * vx + vy * vy).sqrt() * spin[k];
            let w = cross(a.v, t);
            omega.extend([
                (w[0] + sp * a.v[0]) as f32,
                (w[1] + sp * a.v[1]) as f32,
                (w[2] + sp * a.v[2]) as f32,
            ]);
        }
        PlateMotion { omega }
    }
    /// Return f32 projected centroids for site groups; empty groups remain zero.
    pub(crate) fn group_centroids(
        &self,
        group: &[usize],
        count: usize,
        size: &[usize],
    ) -> (Vec<f32>, Vec<f32>) {
        let mut sums = vec![[0.; 3]; count];
        for (i, &group) in group.iter().enumerate().take(self.mesh.n) {
            let p = self.p(i);
            for (sum, value) in sums[group].iter_mut().zip(p) {
                *sum += value;
            }
        }
        let mut cx = vec![0.; count];
        let mut cy = vec![0.; count];
        for k in 0..count {
            if size[k] == 0 || norm2(sums[k]) == 0. {
                continue;
            }
            let p = self.vector_to_point(sums[k]);
            cx[k] = p[0] as f32;
            cy[k] = p[1] as f32;
        }
        (cx, cy)
    }
    /// Return latitude in degrees for site i, positive north.
    pub(crate) fn latitude(&self, i: usize) -> f64 {
        90. - (180. * f64::from(self.mesh.y[i])) / self.mesh.height
    }
    /// Return single-layer surface noise using the upstream seed directly.
    pub(crate) fn noise(&self, seed: u32) -> SurfaceNoise<'a> {
        SurfaceNoise::new(self.mesh, seed, NoiseKind::Single)
    }
    /// Return fBm surface noise with explicit octaves and persistence.
    pub(crate) fn fbm(&self, seed: u32, octaves: usize, persistence: f64) -> SurfaceNoise<'a> {
        SurfaceNoise::new(
            self.mesh,
            seed,
            NoiseKind::Fbm {
                octaves,
                persistence,
            },
        )
    }
    /// Return ridged surface noise with explicit octaves and persistence.
    pub(crate) fn ridged(&self, seed: u32, octaves: usize, persistence: f64) -> SurfaceNoise<'a> {
        SurfaceNoise::new(
            self.mesh,
            seed,
            NoiseKind::Ridged {
                octaves,
                persistence,
            },
        )
    }
    /// Return the nearest site to the projected point, starting from the given hint.
    pub(crate) fn nearest(&self, point: [f64; 2], hint: usize) -> usize {
        self.finder.find(self.mesh, self.point(point).v, hint)
    }
    /// Return west/east land distances along coarse latitude rows of world-unit size g.
    pub(crate) fn zonal_land(&self, land: &[u8], g: f64) -> (Vec<f32>, Vec<f32>) {
        let gw = (self.mesh.width / g).ceil() as usize;
        let gh = (self.mesh.height / g).ceil() as usize;
        let col = |x: f32| ((f64::from(x) / g).floor() as usize).min(gw - 1);
        let row = |y: f32| ((f64::from(y) / g).floor() as usize).min(gh - 1);
        let mut grid = vec![false; gw * gh];
        for (i, &land) in land.iter().enumerate().take(self.mesh.n) {
            if land != 0 {
                grid[row(self.mesh.y[i]) * gw + col(self.mesh.x[i])] = true;
            }
        }
        let mut dw = vec![0f32; gw * gh];
        let mut de = vec![0f32; gw * gh];
        for r in 0..gh {
            let k = cos(PI / 2. - (((r as f64 + 0.5) * g) / self.mesh.height) * PI) * g;
            let mut last = -1e9;
            for c in 0..2 * gw {
                let cc = c % gw;
                if grid[r * gw + cc] {
                    last = c as f64;
                }
                if c >= gw {
                    dw[r * gw + cc] = ((c as f64 - last) * k) as f32;
                }
            }
            last = 1e9;
            for c in (0..2 * gw).rev() {
                let cc = c % gw;
                if grid[r * gw + cc] {
                    last = c as f64;
                }
                if c < gw {
                    de[r * gw + cc] = ((last - c as f64) * k) as f32;
                }
            }
        }
        let mut west = Vec::with_capacity(self.mesh.n);
        let mut east = Vec::with_capacity(self.mesh.n);
        for i in 0..self.mesh.n {
            let index = row(self.mesh.y[i]) * gw + col(self.mesh.x[i]);
            west.push(dw[index]);
            east.push(de[index]);
        }
        (west, east)
    }
}
/// Rigid plate angular velocities, stored with the upstream f32 boundary.
#[derive(Debug)]
pub(crate) struct PlateMotion {
    /// Interleaved angular velocity vectors, one per plate.
    pub(crate) omega: Vec<f32>,
}
impl PlateMotion {
    /// Return east/south plate k velocity at mesh site i, sharing geo's cached local frames.
    pub(crate) fn at(&self, geo: &SphereGeometry<'_>, k: usize, i: usize) -> [f64; 2] {
        let w = [
            f64::from(self.omega[3 * k]),
            f64::from(self.omega[3 * k + 1]),
            f64::from(self.omega[3 * k + 2]),
        ];
        let v = cross(w, geo.p(i));
        [
            v[0] * geo.ex[i] + v[1] * geo.ey[i],
            v[0] * geo.sx[i] + v[1] * geo.sy[i] + v[2] * geo.sz[i],
        ]
    }
}
#[derive(Debug, Clone, Copy)]
enum NoiseKind {
    Single,
    Fbm { octaves: usize, persistence: f64 },
    Ridged { octaves: usize, persistence: f64 },
}
/// Seeded simplex or fractal noise sampled on the mesh surface.
pub(crate) struct SurfaceNoise<'a> {
    mesh: &'a Mesh,
    noise: Noise3,
    kind: NoiseKind,
    r: f64,
}
impl<'a> SurfaceNoise<'a> {
    fn new(mesh: &'a Mesh, seed: u32, kind: NoiseKind) -> Self {
        Self {
            mesh,
            noise: Noise3::new(seed),
            kind,
            r: sphere_radius(mesh.width),
        }
    }
    fn sample(&self, p: [f64; 3]) -> f64 {
        match self.kind {
            NoiseKind::Single => self.noise.sample(p[0], p[1], p[2]),
            NoiseKind::Fbm {
                octaves,
                persistence,
            } => fbm3(&self.noise, p, octaves, persistence, 2.),
            NoiseKind::Ridged {
                octaves,
                persistence,
            } => ridged3(&self.noise, p, octaves, persistence),
        }
    }
    /// Sample the requested site; arguments retain upstream multiplication and offset order.
    pub(crate) fn at(&self, i: usize, f: f64, g: f64, offset: [f64; 2]) -> f64 {
        let f = f * self.r * g;
        self.sample([
            f64::from(self.mesh.xyz[3 * i]) * f + offset[0],
            f64::from(self.mesh.xyz[3 * i + 1]) * f + offset[1],
            f64::from(self.mesh.xyz[3 * i + 2]) * f,
        ])
    }
    /// Sample site i at world-unit scale s and the specified planar noise offset.
    pub(crate) fn at_scale(&self, i: usize, s: f64, offset: [f64; 2]) -> f64 {
        let f = self.r / s;
        self.sample([
            f64::from(self.mesh.xyz[3 * i]) * f + offset[0],
            f64::from(self.mesh.xyz[3 * i + 1]) * f + offset[1],
            f64::from(self.mesh.xyz[3 * i + 2]) * f,
        ])
    }
    /// Sample site i with its axial noise coordinate multiplied by sy.
    pub(crate) fn stretched(&self, i: usize, f: f64, sy: f64) -> f64 {
        let f = f * self.r;
        self.sample([
            f64::from(self.mesh.xyz[3 * i]) * f,
            f64::from(self.mesh.xyz[3 * i + 1]) * f,
            f64::from(self.mesh.xyz[3 * i + 2]) * f * sy,
        ])
    }
}
/// Return the upstream wrapping spatial hash bucket for three grid coordinates.
pub(super) fn bucket(a: i32, b: i32, c: i32, mask: usize) -> usize {
    ((a.wrapping_mul(73856093) ^ b.wrapping_mul(19349663) ^ c.wrapping_mul(83492791)) as usize)
        & mask
}
#[derive(Debug)]
struct SphereFinder {
    h: f64,
    head: Vec<usize>,
    next: Vec<usize>,
    visits: RefCell<Visits>,
}
#[derive(Debug)]
struct Visits {
    seen: Vec<u32>,
    stamp: u32,
}
impl SphereFinder {
    fn cell(&self, v: f64) -> i32 {
        ((v + 1.) / self.h).floor() as i32
    }
    fn new(mesh: &Mesh) -> Self {
        let h = (2. * mesh.spacing) / sphere_radius(mesh.width);
        let bins = (mesh.n * 2).next_power_of_two();
        let mut finder = Self {
            h,
            head: vec![usize::MAX; bins],
            next: vec![0; mesh.n],
            visits: RefCell::new(Visits {
                seen: vec![0; mesh.n],
                stamp: 0,
            }),
        };
        for i in (0..mesh.n).rev() {
            let key = bucket(
                finder.cell(f64::from(mesh.xyz[3 * i])),
                finder.cell(f64::from(mesh.xyz[3 * i + 1])),
                finder.cell(f64::from(mesh.xyz[3 * i + 2])),
                bins - 1,
            );
            finder.next[i] = finder.head[key];
            finder.head[key] = i;
        }
        finder
    }
    fn find(&self, mesh: &Mesh, q: [f64; 3], hint: usize) -> usize {
        let d2 = |i: usize| {
            norm2([
                f64::from(mesh.xyz[3 * i]) - q[0],
                f64::from(mesh.xyz[3 * i + 1]) - q[1],
                f64::from(mesh.xyz[3 * i + 2]) - q[2],
            ])
        };
        let gx = self.cell(q[0]);
        let gy = self.cell(q[1]);
        let gz = self.cell(q[2]);
        let mut best = if hint < mesh.n { hint } else { 0 };
        let mut bd = d2(best);
        let scan = |a, b, c, best: &mut usize, bd: &mut f64| {
            let mut i = self.head[bucket(a, b, c, self.head.len() - 1)];
            while i != usize::MAX {
                let d = d2(i);
                if d < *bd {
                    *bd = d;
                    *best = i;
                }
                i = self.next[i];
            }
        };
        scan(gx, gy, gz, &mut best, &mut bd);
        if bd > self.h * self.h {
            for a in gx - 1..=gx + 1 {
                for b in gy - 1..=gy + 1 {
                    for c in gz - 1..=gz + 1 {
                        scan(a, b, c, &mut best, &mut bd);
                    }
                }
            }
        }
        let mut c = best;
        loop {
            let mut nx = None;
            for &j in &mesh.adj[mesh.adj_start[c]..mesh.adj_start[c + 1]] {
                let d = d2(j);
                if d < bd {
                    bd = d;
                    nx = Some(j);
                }
            }
            match nx {
                None => return c,
                Some(next) => c = next,
            }
        }
    }
    fn within(
        &self,
        mesh: &Mesh,
        q: [f64; 3],
        rad: f64,
        keep: impl Fn(usize) -> bool,
    ) -> Vec<usize> {
        let e = rad * 1.0001 + 1e-9;
        let a0 = self.cell(q[0] - e);
        let a1 = self.cell(q[0] + e);
        let b0 = self.cell(q[1] - e);
        let b1 = self.cell(q[1] + e);
        let c0 = self.cell(q[2] - e);
        let c1 = self.cell(q[2] + e);
        let bins = f64::from(a1 - a0 + 1) * f64::from(b1 - b0 + 1) * f64::from(c1 - c0 + 1);
        if bins >= mesh.n as f64 {
            return (0..mesh.n).filter(|&i| keep(i)).collect();
        }
        let mut visits = self.visits.borrow_mut();
        visits.stamp += 1;
        if visits.stamp == u32::MAX {
            visits.seen.fill(0);
            visits.stamp = 1;
        }
        let stamp = visits.stamp;
        let mut out = Vec::new();
        for a in a0..=a1 {
            for b in b0..=b1 {
                for c in c0..=c1 {
                    let mut i = self.head[bucket(a, b, c, self.head.len() - 1)];
                    while i != usize::MAX {
                        if visits.seen[i] != stamp {
                            visits.seen[i] = stamp;
                            if keep(i) {
                                out.push(i);
                            }
                        }
                        i = self.next[i];
                    }
                }
            }
        }
        out.sort_unstable();
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry_reference::*;
    use crate::{
        mesh::{build_mesh, sphere_spacing},
        random::{sub_seed, Mulberry32},
    };
    fn mesh() -> Mesh {
        build_mesh(
            2048.,
            1024.,
            sphere_spacing(2048., 256),
            &mut Mulberry32::new(sub_seed(42, "mesh")),
            &|| false,
        )
        .expect("valid fixture")
    }
    #[test]
    fn upstream_geometry_noise_motion_and_wind() {
        let m = mesh();
        let g = SphereGeometry::new(&m);
        assert!(g.area.is_finite() && g.area > 0.);
        if !crate::geometry_reference::trig_matches() {
            eprintln!("Skipping exact geometry fixture: host trigonometry differs from Node");
            return;
        }
        let groups = (0..m.n).map(|i| i % 3).collect::<Vec<_>>();
        let (cx, cy) = g.group_centroids(&groups, 3, &[79; 3]);
        assert_eq!(
            cx.iter()
                .chain(&cy)
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            CENTERS
        );
        let motion = g.plate_motion(
            &[1., -2., 0.3],
            &[0.5, 0.4, -1.],
            (&cx, &cy),
            &[0.1, -0.3, 0.5],
        );
        assert_eq!(
            motion.omega.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            OMEGA
        );
        let water = (0..m.n).map(|i| u8::from(i % 3 != 0)).collect::<Vec<_>>();
        let cuts = g.wind_cuts(&water);
        assert_eq!(cuts.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), CUTS);
        let (west, east) = g.zonal_land(&water, 32.);
        assert_eq!(
            west.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            ZONAL_W
        );
        assert_eq!(
            east.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            ZONAL_E
        );
        let mut values = Vec::new();
        for i in [0, 1, 50, 236] {
            values.extend(motion.at(&g, i % 3, i));
            values.extend(g.move_cell(i, [[20., -15.], [3., 7.], [-2., 4.]]));
            values.extend(g.mid(i, (i + 1) % m.n));
            values.extend([
                g.dist(i, (i + 1) % m.n),
                g.dist2(i, (i + 1) % m.n),
                g.edge_dot(i, (i + 1) % m.n, [0.3, -0.7]),
                g.latitude(i),
                g.downwind(i, [0.3, -0.7], Some(&cuts), None),
                g.noise(12).at(i, 0.01, 1.3, [7., 9.]),
                g.fbm(12, 4, 0.6).at_scale(i, 80., [3., 5.]),
                g.ridged(12, 4, 0.5).stretched(i, 0.01, 0.4),
            ]);
        }
        values.extend([
            g.point_dist([0., 200.], [2047., 800.]),
            g.point_dot([10., 200.], [1800., 900.], [0.4, -0.2]),
        ]);
        assert_eq!(
            values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            GEOMETRY
        );
    }

    #[test]
    fn upstream_spatial_queries_cover_poles_seam_and_large_radius() {
        let m = mesh();
        let g = SphereGeometry::new(&m);
        assert!(g.edge_lengths().iter().all(|v| v.is_finite() && *v > 0.));
        for i in 0..m.n {
            assert_eq!(g.nearest([f64::from(m.x[i]), f64::from(m.y[i])], i), i);
            assert_eq!(g.near2(i, i, 0.), 0.);
        }
        let mut point = [-1., -1.];
        g.clamp_point(&mut point);
        assert_eq!(point, [2047., 0.]);
        if !crate::geometry_reference::trig_matches() {
            eprintln!("Skipping exact query fixture: host mesh trigonometry differs from Node");
            return;
        }
        for &(cell, radius, expected) in CELL_QUERIES {
            assert_eq!(g.cells_near(cell, radius), expected);
        }
        for &(point, radius, expected) in POINT_QUERIES {
            assert_eq!(g.cells_near_point(point, radius), expected);
        }
        for &(point, hint, expected) in NEAREST {
            assert_eq!(g.nearest(point, hint), expected);
        }
        let lengths = g.edge_lengths();
        for &(k, bits) in EDGE_LENGTHS {
            assert_eq!(lengths[k].to_bits(), bits);
        }
    }
}
