// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas seaice.ts computeSeaIce/seaIceNodes; only the dropSpecks shape dependency.
use crate::{
    geometry::SphereGeometry,
    mesh::blur_field,
    random::sub_seed,
    terrain::Terrain,
    util::{check_cancel, order_by_key, smoothstep},
    TerrainError,
};
pub(crate) fn nodes(world: &Terrain, cancel: &dyn Fn() -> bool) -> Result<Vec<f32>, TerrainError> {
    let mesh = &world.mesh;
    let n = mesh.n;
    let geo = SphereGeometry::new(mesh);
    let c = &world.initial_climate;
    let cuts = geo.wind_cuts(&world.water);
    let key = (0..n)
        .map(|i| {
            geo.downwind(
                i,
                [c.wind_x[i] as f64, c.wind_y[i] as f64],
                Some(&cuts),
                None,
            ) as f32
        })
        .collect::<Vec<_>>();
    let order = order_by_key(&key);
    let mut chill = vec![0_f32; n];
    for _ in 0..2 {
        for &i in &order {
            check_cancel(cancel)?;
            let mut sw = 0.;
            let mut sc = 0.;
            for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
                let dot = geo.edge_dot(i, j, [-(c.wind_x[i] as f64), -(c.wind_y[i] as f64)]);
                if dot <= 0.1 {
                    continue;
                }
                sw += dot;
                sc += dot * chill[j] as f64;
            }
            let up = if sw > 0. { sc / sw } else { 0. };
            chill[i] = if world.water[i] != 1 {
                (up + (-10. * smoothstep(14., -6., c.temperature[i] as f64) - up) * 0.2) as f32
            } else {
                (up * 0.95) as f32
            };
        }
    }
    let land = world
        .water
        .iter()
        .map(|&w| if w == 1 { 0. } else { 1. })
        .collect::<Vec<_>>();
    let near = blur_field(mesh, &land, 5);
    check_cancel(cancel)?;
    let far = blur_field(mesh, &near, 30);
    let current = geo.fbm(sub_seed(world.params.seed, "seaice-current"), 2, 0.5);
    let mut ice = vec![0_f32; n];
    for i in 0..n {
        if world.water[i] != 1 {
            continue;
        }
        let open = 1. - (far[i] as f64 * 2.2).clamp(0., 1.);
        let deep = smoothstep(-200., -2500., world.elevation[i] as f64);
        let teff = c.temperature[i] as f64 + chill[i] as f64
            - 3. * (near[i] as f64 * 1.6).clamp(0., 1.)
            + 1.5 * open
            + 0.8 * deep
            + (3.5 + 4.5 * open) * current.stretched(i, 1. / 300., 0.55);
        ice[i] = smoothstep(-3., -11., teff) as f32;
    }
    let mut node = vec![0_f32; n];
    for i in 0..n {
        if world.water[i] == 1 {
            node[i] = ice[i];
            continue;
        }
        let mut s = 0.;
        let mut count = 0;
        for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
            if world.water[j] == 1 {
                s += ice[j] as f64;
                count += 1;
            }
        }
        node[i] = if count > 0 {
            (s / count as f64) as f32
        } else {
            0.
        };
    }
    let inv_r2 = 1. / (mesh.spacing * mesh.spacing * 4.);
    let mut out = vec![0_f32; n];
    for (i, output) in out.iter_mut().enumerate() {
        let mut sw = 0.;
        let mut sv = 0.;
        for j in std::iter::once(i).chain(
            mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]]
                .iter()
                .copied(),
        ) {
            let t = (1. - geo.dist2(j, i) * inv_r2).max(0.);
            let w = t * t + 1e-6;
            sw += w;
            sv += w * node[j] as f64;
        }
        *output = (sv / sw) as f32;
    }
    Ok(out)
}
