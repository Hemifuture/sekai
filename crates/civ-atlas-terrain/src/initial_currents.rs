// SPDX-License-Identifier: AGPL-3.0-only
// Private initialization currents from Civ Atlas src/gen/currents.ts.
use crate::{
    geometry::SphereGeometry,
    mesh::{blur_field, Mesh},
    random::round24,
    util::{check_cancel, piecewise, smoothstep},
    TerrainError,
};
#[derive(Debug)]
pub(crate) struct InitialCurrents {
    pub sst: Vec<f32>,
    pub u: Vec<f32>,
    pub v: Vec<f32>,
}
pub(super) const KM: f64 = 40000.0 / crate::terrain::MAP_W;
const WEST: &[f64] = &[
    0.0, 1.5, 10.0, 2.5, 20.0, 4.0, 35.0, 4.0, 42.0, 0.0, 50.0, -5.0, 62.0, -5.0, 75.0, -2.0, 90.0,
    0.0,
];
const EAST: &[f64] = &[
    0.0, -3.0, 8.0, -3.5, 15.0, -6.0, 28.0, -6.0, 36.0, -3.0, 42.0, 0.0, 48.0, 5.0, 56.0, 8.0,
    66.0, 7.0, 76.0, 3.0, 90.0, 0.0,
];
fn reach(d: f64, r: f64) -> f64 {
    if d >= r {
        0.0
    } else {
        let t = 1.0 - d / r;
        t * t
    }
}
fn gyre(a: f64) -> f64 {
    if a < 46.0 {
        round24(((std::f64::consts::PI * (a - 0.0)) / (46.0 - 0.0)).sin())
    } else if (46.0..72.0).contains(&a) {
        -0.7 * round24(((std::f64::consts::PI * (a - 46.0)) / (72.0 - 46.0)).sin())
    } else {
        0.0
    }
}
pub(crate) fn compute(
    mesh: &Mesh,
    geo: &SphereGeometry<'_>,
    water: &[u8],
    cancel: &dyn Fn() -> bool,
) -> Result<InitialCurrents, TerrainError> {
    let n = mesh.n;
    let mut comp = vec![usize::MAX; n];
    let mut big = vec![0_u8; n];
    let min_cells = 0.002 * n as f64;
    let mut stack = Vec::new();
    for s in 0..n {
        if water[s] == 1 || comp[s] != usize::MAX {
            continue;
        }
        check_cancel(cancel)?;
        let mut cells = Vec::new();
        comp[s] = s;
        stack.push(s);
        while let Some(c) = stack.pop() {
            cells.push(c);
            for &j in &mesh.adj[mesh.adj_start[c]..mesh.adj_start[c + 1]] {
                if water[j] != 1 && comp[j] == usize::MAX {
                    comp[j] = s;
                    stack.push(j);
                }
            }
        }
        if cells.len() as f64 >= min_cells {
            for c in cells {
                big[c] = 1;
            }
        }
    }
    let (west, east) = geo.zonal_land(&big, 4.0);
    let mut sst = vec![0.0_f32; n];
    let mut psi = vec![0.0_f32; n];
    for i in 0..n {
        if i % 256 == 0 {
            check_cancel(cancel)?;
        }
        let lat = geo.latitude(i);
        let dw = f64::from(west[i]) * KM;
        let de = f64::from(east[i]) * KM;
        if big[i] == 0 {
            let b = dw + de;
            let shape = if b > 1e7 {
                1.0
            } else if b > 0.0 {
                (1.0 - round24((-dw / 300.0).exp())) * (de / b) * smoothstep(1000.0, 3500.0, b)
            } else {
                0.0
            };
            psi[i] = (if lat >= 0.0 { 1.0 } else { -1.0 } * gyre(lat.abs()) * shape) as f32;
        }
        if water[i] != 1 {
            continue;
        }
        let wide = smoothstep(1000.0, 3500.0, dw + de);
        if wide == 0.0 {
            continue;
        }
        let pw = piecewise(WEST, lat.abs());
        let pe = piecewise(EAST, lat.abs());
        let kw = reach(dw, if pw > 0.0 { 1500.0 } else { 1300.0 });
        let ke = reach(de, if pe > 0.0 { 2200.0 } else { 1500.0 });
        let tw = wide * pw * kw;
        let te = wide * pe * ke;
        sst[i] = (tw + te) as f32;
    }
    let sea_f: Vec<_> = water.iter().map(|&w| f32::from(w == 1)).collect();
    let num = blur_field(mesh, &sst, 2);
    let den = blur_field(mesh, &sea_f, 2);
    for i in 0..n {
        sst[i] = if water[i] == 1 && den[i] > 0.0 {
            (f64::from(num[i]) / f64::from(den[i])) as f32
        } else {
            0.0
        };
    }
    let ps = blur_field(mesh, &psi, 2);
    let mut u = vec![0.0_f32; n];
    let mut v = vec![0.0_f32; n];
    for i in 0..n {
        if water[i] != 1 {
            continue;
        }
        let c = round24(((geo.latitude(i) * std::f64::consts::PI) / 180.0).cos());
        let mut sxx = 0.0;
        let mut sxy = 0.0;
        let mut syy = 0.0;
        let mut sxp = 0.0;
        let mut syp = 0.0;
        for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
            let mut dx = f64::from(mesh.x[j]) - f64::from(mesh.x[i]);
            dx -= mesh.width * (dx / mesh.width + 0.5).floor();
            let de = dx * c;
            let dn = f64::from(mesh.y[i]) - f64::from(mesh.y[j]);
            let dp = f64::from(ps[j]) - f64::from(ps[i]);
            sxx += de * de;
            sxy += de * dn;
            syy += dn * dn;
            sxp += de * dp;
            syp += dn * dp;
        }
        let det = sxx * syy - sxy * sxy;
        if det.abs() < 1e-9 {
            continue;
        }
        let ge = (sxp * syy - syp * sxy) / det;
        let gn = (syp * sxx - sxp * sxy) / det;
        u[i] = (-gn / 0.02) as f32;
        v[i] = (-ge / 0.02) as f32;
    }
    Ok(InitialCurrents { sst, u, v })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mesh::{build_mesh, sphere_spacing},
        random::{sub_seed, Mulberry32},
        terrain_reference as r,
    };
    #[test]
    fn initialization_currents_match_upstream() {
        let mesh = build_mesh(
            2048.0,
            1024.0,
            sphere_spacing(2048.0, 256),
            &mut Mulberry32::new(sub_seed(42, "mesh")),
            &|| false,
        )
        .expect("mesh");
        let water: Vec<_> = r::WATER.iter().map(|&v| v as u8).collect();
        let c = compute(&mesh, &SphereGeometry::new(&mesh), &water, &|| false).expect("currents");
        assert!(c.sst.iter().all(|v| v.is_finite()));
        if crate::geometry_reference::trig_matches() {
            for (got, want) in [
                (&c.sst, r::CURRENT_SST),
                (&c.u, r::CURRENT_U),
                (&c.v, r::CURRENT_V),
            ] {
                assert_eq!(got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), want);
            }
        }
    }
}
