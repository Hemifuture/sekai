// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas creeks.ts; reuse the identical final drainage instead of solving it again.
use crate::{
    geometry::SphereGeometry,
    terrain::{River, Terrain, ICE_SHEET_T},
    util::check_cancel,
    TerrainError,
};
fn pair_hash(a: usize, b: usize) -> f64 {
    let mut h = ((a as u32) ^ 0x9e3779b9).wrapping_mul(0x85ebca6b)
        ^ (b as u32).wrapping_add(0x632be5ab).wrapping_mul(0xc2b2ae35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b3c6d);
    h ^= h >> 12;
    h as f64 / 4294967296.
}
pub(crate) fn creeks(
    world: &Terrain,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<River>, TerrainError> {
    let mesh = &world.mesh;
    let n = mesh.n;
    let geo = SphereGeometry::new(mesh);
    let receiver = &world.initial_receiver;
    let mut on_river = vec![false; n];
    for r in &world.initial_rivers {
        for &c in &r.cells {
            on_river[c] = true;
        }
    }
    let ice = |i: usize| (world.initial_climate.temperature[i] as f64) < ICE_SHEET_T;
    let is_creek = (0..n)
        .map(|i| {
            world.water[i] == 0
                && !on_river[i]
                && world.initial_flux[i] as f64 >= world.river_threshold * 0.2
                && !ice(i)
        })
        .collect::<Vec<_>>();
    let mut has_donor = vec![false; n];
    for i in 0..n {
        if is_creek[i] && receiver[i] != usize::MAX {
            has_donor[receiver[i]] = true;
        }
    }
    let mut sources = (0..n)
        .filter(|&i| is_creek[i] && !has_donor[i])
        .collect::<Vec<_>>();
    sources.sort_by(|&a, &b| {
        world.initial_flux[b]
            .partial_cmp(&world.initial_flux[a])
            .expect("finite flux")
            .then(a.cmp(&b))
    });
    let mut visited = vec![false; n];
    let mut out = Vec::new();
    for s in sources {
        check_cancel(cancel)?;
        let mut pts = vec![[
            mesh.x[s] as f64,
            mesh.y[s] as f64,
            world.initial_flux[s] as f64,
        ]];
        let mut chain = vec![s];
        let mut i = s;
        visited[i] = true;
        loop {
            let r = receiver[i];
            if r == usize::MAX {
                break;
            }
            if world.water[r] != 0 {
                let p = geo.mid(i, r);
                pts.push([p[0], p[1], world.initial_flux[i] as f64]);
                chain.push(r);
                break;
            }
            if on_river[r] {
                pts.push([
                    mesh.x[r] as f64,
                    mesh.y[r] as f64,
                    world.initial_flux[i] as f64,
                ]);
                chain.push(r);
                break;
            }
            if ice(r) {
                break;
            }
            pts.push([
                mesh.x[r] as f64,
                mesh.y[r] as f64,
                world.initial_flux[r] as f64,
            ]);
            chain.push(r);
            if visited[r] {
                break;
            }
            visited[r] = true;
            i = r;
        }
        if pts.len() < 2 {
            continue;
        }
        let mut p = Vec::with_capacity(pts.len() * 6);
        let mut cells = Vec::with_capacity(pts.len() * 2);
        for i in 0..pts.len() {
            p.extend(pts[i].map(|v| v as f32));
            cells.push(chain[i]);
            if i + 1 == pts.len() {
                break;
            }
            let dx = pts[i + 1][0] - pts[i][0];
            let dy = pts[i + 1][1] - pts[i][1];
            if dx.abs() > mesh.width / 2. {
                continue;
            }
            let off = (pair_hash(chain[i], chain[i + 1]) - 0.5) * 2. * 0.22;
            p.extend([
                (pts[i][0] + 0.5 * dx - off * dy) as f32,
                (pts[i][1] + 0.5 * dy + off * dx) as f32,
                pts[i][2] as f32,
            ]);
            cells.push(chain[i]);
        }
        out.push(River { pts: p, cells });
    }
    Ok(out)
}
