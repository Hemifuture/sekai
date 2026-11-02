use crate::{
    geometry::{asin, atan, bucket, cos, norm2, sin, sphere_radius},
    random::Mulberry32,
    TerrainError,
};
use std::f64::consts::{PI, TAU};

// Civ Atlas geometry.ts sphereSpacing/poissonSphere author fill prior.
const POISSON_FILL: f64 = 0.66;
/// Unit-sphere sites and ordered CSR topology from Civ Atlas geometry.ts.
#[derive(Debug)]
pub(crate) struct Mesh {
    /// Equatorial circumference in upstream world units.
    pub(crate) width: f64,
    /// North-to-south map extent in projected world units.
    pub(crate) height: f64,
    /// Minimum sampling chord length in world units.
    pub(crate) spacing: f64,
    /// Number of sites actually placed by the Poisson sampler.
    pub(crate) n: usize,
    /// Equirectangular longitude coordinates, wrapped into the map width.
    pub(crate) x: Vec<f32>,
    /// Equirectangular latitude coordinates, north to south.
    pub(crate) y: Vec<f32>,
    /// Interleaved unit-sphere positions, with north along positive z.
    pub(crate) xyz: Vec<f32>,
    /// Triangle vertex indices, with seam/pole triangles partitioned first.
    // Task 4's sphere raster cover consumes this triangulation.
    #[allow(dead_code)]
    pub(crate) triangles: Vec<usize>,
    /// CSR neighbor offsets, including a final sentinel offset.
    pub(crate) adj_start: Vec<usize>,
    /// Neighbors in the original d3 halfedge traversal order.
    pub(crate) adj: Vec<usize>,
}
/// Return the upstream sampling spacing for `cells` requested sites and map `width`.
/// The fill fraction is the Civ Atlas author prior, not a physical constant.
pub(crate) fn sphere_spacing(width: f64, cells: usize) -> f64 {
    let r = sphere_radius(width);
    (4. * PI * r * r * POISSON_FILL / cells as f64).sqrt()
}
fn point_bucket(p: [f64; 3], g: f64, mask: usize) -> usize {
    bucket(
        ((p[0] + 1.) / g).floor() as i32,
        ((p[1] + 1.) / g).floor() as i32,
        ((p[2] + 1.) / g).floor() as i32,
        mask,
    )
}
fn poisson(
    r: f64,
    rng: &mut Mulberry32,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<f32>, TerrainError> {
    let expected = 4. * PI * POISSON_FILL / (r * r);
    let bins = (expected.ceil() as usize).next_power_of_two();
    let mut heads = vec![usize::MAX; bins];
    let mut next = Vec::with_capacity(expected.ceil() as usize);
    let mut points = Vec::<[f64; 3]>::with_capacity(expected.ceil() as usize);
    let g = 3.02 * r;
    let put =
        |p: [f64; 3], points: &mut Vec<[f64; 3]>, next: &mut Vec<usize>, heads: &mut [usize]| {
            let id = points.len();
            let b = point_bucket(p, g, bins - 1);
            points.push(p);
            next.push(heads[b]);
            heads[b] = id;
            id
        };
    let z0 = 2. * rng.next_f64() - 1.;
    let a0 = TAU * rng.next_f64();
    let q0 = (1. - z0 * z0).max(0.).sqrt();
    let mut active = vec![put(
        [q0 * cos(a0), q0 * sin(a0), z0],
        &mut points,
        &mut next,
        &mut heads,
    )];
    let mut near = Vec::<[f64; 3]>::with_capacity(128);
    let reach2 = (3.01 * r).powi(2);
    while !active.is_empty() {
        if cancel() {
            return Err(TerrainError::Cancelled);
        }
        let ai = (rng.next_f64() * active.len() as f64).floor() as usize;
        let p = points[active[ai]];
        let mut u = if p[2].abs() > 0.9 {
            [0., -p[2], p[1]]
        } else {
            [-p[1], p[0], 0.]
        };
        let ul = norm2(u).sqrt();
        for value in &mut u {
            *value /= ul;
        }
        let v = [
            p[1] * u[2] - p[2] * u[1],
            p[2] * u[0] - p[0] * u[2],
            p[0] * u[1] - p[1] * u[0],
        ];
        near.clear();
        let [gx, gy, gz] = p.map(|v| ((v + 1.) / g).floor() as i32);
        for a in gx - 1..=gx + 1 {
            for b in gy - 1..=gy + 1 {
                for c in gz - 1..=gz + 1 {
                    let mut id = heads[bucket(a, b, c, bins - 1)];
                    while id != usize::MAX {
                        let q = points[id];
                        if norm2([q[0] - p[0], q[1] - p[1], q[2] - p[2]]) <= reach2 {
                            near.push(q)
                        }
                        id = next[id];
                    }
                }
            }
        }
        let mut placed = false;
        for _ in 0..24 {
            let a = rng.next_f64() * TAU;
            let d = r * (1. + rng.next_f64());
            let ca = cos(a);
            let sa = sin(a);
            let mut q = [
                p[0] + d * (ca * u[0] + sa * v[0]),
                p[1] + d * (ca * u[1] + sa * v[1]),
                p[2] + d * (ca * u[2] + sa * v[2]),
            ];
            let ul = norm2(q).sqrt();
            for value in &mut q {
                *value /= ul;
            }
            if near
                .iter()
                .any(|n| norm2([n[0] - q[0], n[1] - q[1], n[2] - q[2]]) < r * r)
            {
                continue;
            }
            active.push(put(q, &mut points, &mut next, &mut heads));
            near.push(q);
            placed = true;
        }
        if !placed {
            active.swap_remove(ai);
        }
    }
    Ok(points
        .into_iter()
        .flat_map(|p| p.map(|v| v as f32))
        .collect())
}
struct Topology {
    triangles: Vec<usize>,
    adj_start: Vec<usize>,
    adj: Vec<usize>,
}

fn triangulate(xyz: &[f32], cancel: &dyn Fn() -> bool) -> Result<Topology, TerrainError> {
    use crate::triangulation::{Point, EMPTY};
    let n = xyz.len() / 3;
    if n < 4 {
        return Err(TerrainError::InvalidMesh);
    }
    let p0 = (0..n)
        .min_by(|&a, &b| xyz[3 * a + 2].total_cmp(&xyz[3 * b + 2]))
        .expect("nonempty mesh");
    let vx = f64::from(xyz[3 * p0]);
    let vy = f64::from(xyz[3 * p0 + 1]);
    let vz = f64::from(xyz[3 * p0 + 2]);
    let mut kx = -vy;
    let mut ky = vx;
    let sn = (kx * kx + ky * ky).sqrt();
    let cs = -vz;
    if sn > 1e-12 {
        kx /= sn;
        ky /= sn;
    }
    let mut coords = Vec::with_capacity(n - 1);
    let mut back = Vec::with_capacity(n - 1);
    let mut local = vec![EMPTY; n];
    for (i, local_index) in local.iter_mut().enumerate() {
        if cancel() {
            return Err(TerrainError::Cancelled);
        }
        if i == p0 {
            continue;
        }
        let x = f64::from(xyz[3 * i]);
        let y = f64::from(xyz[3 * i + 1]);
        let z = f64::from(xyz[3 * i + 2]);
        let (mut qx, mut qy, mut qz) = (x, y, z);
        if sn > 1e-12 {
            let kdp = kx * x + ky * y;
            qx = x * cs + ky * z * sn + kx * kdp * (1. - cs);
            qy = y * cs - kx * z * sn + ky * kdp * (1. - cs);
            qz = z * cs + (kx * y - ky * x) * sn;
        } else if cs < 0. {
            qy = -y;
            qz = -z;
        }
        let den = 1. + qz;
        *local_index = back.len();
        back.push(i);
        coords.push(Point {
            x: qx / den,
            y: qy / den,
        });
    }
    let del = crate::triangulation::triangulate(&coords, cancel)?;
    if cancel() {
        return Err(TerrainError::Cancelled);
    }
    if del.hull.len() < 3 || del.triangles.len() / 3 + del.hull.len() != 2 * n - 4 {
        return Err(TerrainError::InvalidMesh);
    }
    let mut triangles = del.triangles.iter().map(|&j| back[j]).collect::<Vec<_>>();
    for h in 0..del.hull.len() {
        triangles.extend([
            back[del.hull[h]],
            back[del.hull[(h + 1) % del.hull.len()]],
            p0,
        ]);
    }
    let mut inedges = vec![EMPTY; n - 1];
    let mut hull_index = vec![EMPTY; n - 1];
    let next_edge = |e: usize| if e % 3 == 2 { e - 2 } else { e + 1 };
    for e in 0..del.halfedges.len() {
        let p = del.triangles[next_edge(e)];
        if del.halfedges[e] == EMPTY || inedges[p] == EMPTY {
            inedges[p] = e;
        }
    }
    for (h, &i) in del.hull.iter().enumerate() {
        hull_index[i] = h;
    }
    let mut adj_start = Vec::with_capacity(n + 1);
    let mut adj = Vec::with_capacity(6 * n - 12);
    for (i, &li) in local.iter().enumerate() {
        if cancel() {
            return Err(TerrainError::Cancelled);
        }
        adj_start.push(adj.len());
        if i == p0 {
            adj.extend(del.hull.iter().map(|&h| back[h]));
            continue;
        }
        let e0 = inedges[li];
        if e0 != EMPTY {
            let mut e = e0;
            loop {
                let q = del.triangles[e];
                adj.push(back[q]);
                e = next_edge(e);
                if del.triangles[e] != li {
                    break;
                }
                e = del.halfedges[e];
                if e == EMPTY {
                    let h = del.hull[(hull_index[li] + 1) % del.hull.len()];
                    if h != q {
                        adj.push(back[h]);
                    }
                    break;
                }
                if e == e0 {
                    break;
                }
            }
        }
        if hull_index[li] != EMPTY {
            adj.push(p0);
        }
    }
    adj_start.push(adj.len());
    Ok(Topology {
        triangles,
        adj_start,
        adj,
    })
}
/// Build a Poisson-sampled spherical mesh using `rng` and world-unit dimensions.
///
/// Returns ordered topology and f32 positions, retaining upstream rounding boundaries.
/// Returns `InvalidMesh` for nonfinite/nonpositive dimensions, invalid spacing,
/// a site budget outside the upstream signed-index domain, or degenerate topology.
/// Returns `Cancelled` when the caller's callback requests interruption during work.
pub(crate) fn build_mesh(
    width: f64,
    height: f64,
    spacing: f64,
    rng: &mut Mulberry32,
    cancel: &dyn Fn() -> bool,
) -> Result<Mesh, TerrainError> {
    let r = spacing / sphere_radius(width);
    let expected = 4. * PI * POISSON_FILL / (r * r);
    if !width.is_finite()
        || !height.is_finite()
        || !spacing.is_finite()
        || width <= 0.
        || height <= 0.
        || !(width as f32).is_finite()
        || !(height as f32).is_finite()
        || width as f32 == 0.
        || height as f32 == 0.
        || spacing <= 0.
        || !(4. ..=f64::from(i32::MAX) / 6.).contains(&expected)
    {
        return Err(TerrainError::InvalidMesh);
    }
    if cancel() {
        return Err(TerrainError::Cancelled);
    }
    let xyz = poisson(r, rng, cancel)?;
    let n = xyz.len() / 3;
    let Topology {
        triangles: tris,
        adj_start,
        adj,
    } = triangulate(&xyz, cancel)?;
    let mut x = Vec::with_capacity(n);
    let mut y = Vec::with_capacity(n);
    for p in xyz.chunks_exact(3) {
        let lon = atan(f64::from(p[1]), f64::from(p[0]));
        let lat = asin(f64::from(p[2]));
        let fx = (((lon + PI) / TAU) * width) as f32;
        x.push(if f64::from(fx) >= width { 0. } else { fx });
        y.push((((PI / 2. - lat) / PI) * height) as f32);
    }
    let mut triangles = Vec::with_capacity(tris.len());
    for wrapped in [true, false] {
        for t in tris.chunks_exact(3) {
            let span =
                x[t[0]].max(x[t[1]]).max(x[t[2]]) as f64 - x[t[0]].min(x[t[1]]).min(x[t[2]]) as f64;
            if (span > width / 2.) == wrapped {
                triangles.extend_from_slice(t);
            }
        }
    }
    Ok(Mesh {
        width,
        height,
        spacing,
        n,
        x,
        y,
        xyz,
        triangles,
        adj_start,
        adj,
    })
}
/// Return `passes` rounds of ordered neighbor averaging without modifying `f`.
/// `f` must contain exactly one value per mesh site. Accumulation is f64 and each
/// pass stores f32, matching JavaScript Float32Array writes.
pub(crate) fn blur_field(mesh: &Mesh, f: &[f32], passes: usize) -> Vec<f32> {
    let mut a = f.to_vec();
    if passes == 0 {
        return a;
    }
    // Civ Atlas blurFront: an initially uniform region cannot change before
    // the varying frontier reaches it. Keep the f32 bit comparison, including -0.
    let mut seen = vec![false; mesh.n];
    let mut cells = Vec::with_capacity(mesh.n);
    if passes >= 8 {
        for i in 0..mesh.n {
            let same = !a[i].is_nan()
                && mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]]
                    .iter()
                    .all(|&j| a[j].to_bits() == a[i].to_bits());
            if !same {
                seen[i] = true;
                cells.push(i);
            }
        }
    }
    let sparse = passes >= 8 && cells.len() <= mesh.n / 2;
    if !sparse {
        cells.clear();
        cells.extend(0..mesh.n);
    }
    let mut b = if sparse { a.clone() } else { vec![0.; mesh.n] };
    let mut from = 0;
    for pass in 0..passes {
        if sparse && pass > 0 {
            let to = cells.len();
            for q in from..to {
                let i = cells[q];
                for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
                    if !seen[j] {
                        seen[j] = true;
                        cells.push(j);
                    }
                }
            }
            from = to;
        }
        for &i in &cells {
            let start = mesh.adj_start[i];
            let end = mesh.adj_start[i + 1];
            let mut sum = f64::from(a[i]);
            for &j in &mesh.adj[start..end] {
                sum += f64::from(a[j]);
            }
            b[i] = (sum / (end - start + 1) as f64) as f32;
        }
        std::mem::swap(&mut a, &mut b);
    }
    a
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry_reference::*;
    use crate::random::sub_seed;
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
    fn upstream_mesh_coordinates_and_ordered_topology() {
        let m = mesh();
        assert_eq!(m.triangles.len(), 3 * (2 * m.n - 4));
        assert_eq!(m.adj.len(), 6 * m.n - 12);
        assert_eq!(m.n + m.triangles.len() / 3 - m.adj.len() / 2, 2);
        for i in 0..m.n {
            let neighbors = &m.adj[m.adj_start[i]..m.adj_start[i + 1]];
            assert!(!neighbors.is_empty());
            for (k, &j) in neighbors.iter().enumerate() {
                assert_ne!(i, j);
                assert!(!neighbors[..k].contains(&j));
                assert!(m.adj[m.adj_start[j]..m.adj_start[j + 1]].contains(&i));
            }
        }
        assert!(m
            .xyz
            .iter()
            .all(|v| v.is_finite() && (-1. ..=1.).contains(v)));
        if !crate::geometry_reference::trig_matches() {
            eprintln!("Skipping exact mesh fixture: host trigonometry differs from Node oracle");
            return;
        }
        assert_eq!(m.xyz.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), XYZ);
        assert_eq!(m.x.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), X);
        assert_eq!(m.y.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), Y);
        assert_eq!(m.triangles, TRIANGLES);
        assert_eq!(m.adj_start, ADJ_START);
        assert_eq!(m.adj, ADJ);
    }
    #[test]
    fn upstream_blur_rounding_and_passes() {
        let m = mesh();
        let field = (0..XYZ.len() / 3)
            .map(|i| if i % 7 == 0 { i as f32 / 13. } else { 0. })
            .collect::<Vec<_>>();
        if !crate::geometry_reference::trig_matches() {
            eprintln!("Skipping exact blur fixture: host mesh trigonometry differs from Node");
            return;
        }
        for (p, expected) in [(0, BLUR_0), (1, BLUR_1), (8, BLUR_8)] {
            assert_eq!(
                blur_field(&m, &field, p)
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                expected
            );
        }
        let mut sparse = vec![0.; m.n];
        sparse[0] = 1.;
        assert_eq!(
            blur_field(&m, &sparse, 8)
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            BLUR_SPARSE
        );
    }

    #[test]
    fn mesh_rejects_invalid_bounds_and_cancels_during_sampling() {
        for (width, height, spacing) in [
            (0., 1024., 1.),
            (2048., -1., 1.),
            (2048., 1024., 0.),
            (f64::NAN, 1024., 1.),
            (2048., f64::INFINITY, 1.),
            (f64::MAX, 1024., 1.),
            (2048., f64::MAX, 1.),
            (2048., 1024., f64::MIN_POSITIVE),
            (2048., 1024., 2048.),
        ] {
            assert!(matches!(
                build_mesh(width, height, spacing, &mut Mulberry32::new(42), &|| false),
                Err(TerrainError::InvalidMesh)
            ));
        }
        let checks = std::cell::Cell::new(0);
        let cancel = || {
            checks.set(checks.get() + 1);
            checks.get() == 10
        };
        assert!(matches!(
            build_mesh(
                2048.,
                1024.,
                sphere_spacing(2048., 256),
                &mut Mulberry32::new(42),
                &cancel
            ),
            Err(TerrainError::Cancelled)
        ));
        assert_eq!(checks.get(), 10);
    }
}
