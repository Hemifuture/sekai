// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas src/gen/erosion.ts: Barnes Priority-Flood and Braun-Willett implicit erosion.
use crate::{
    geometry::SphereGeometry,
    mesh::Mesh,
    util::{check_cancel, MinHeap},
    TerrainError,
};

pub(crate) fn erode(
    mesh: &Mesh,
    geo: &SphereGeometry<'_>,
    input: ErosionInput<'_>,
    cancel: &dyn Fn() -> bool,
) -> Result<(), TerrainError> {
    let ErosionInput {
        land,
        h,
        uplift,
        rain,
        steps,
    } = input;
    let router = DrainageRouter::new(mesh, geo, land);
    let mut d = Drainage::new(mesh.n);
    let mut area = vec![0.0_f32; mesh.n];
    let dt = 0.9;
    for _ in 0..steps {
        check_cancel(cancel)?;
        router.route(h, 1e-5, &mut d, cancel)?;
        accumulate(&d, land, rain, &mut area);
        for &i in &d.order {
            let r = d.receiver[i];
            if r == usize::MAX {
                continue;
            }
            let hr = if land[r] != 0 { f64::from(h[r]) } else { 0.0 };
            let dist = geo.dist(r, i) / mesh.spacing;
            let f = (1.0 * dt * f64::from(area[i]).powf(0.5)) / dist;
            h[i] = ((f64::from(h[i]) + dt * f64::from(uplift[i]) + f * hr) / (1.0 + f)) as f32;
        }
    }
    Ok(())
}
pub(crate) struct ErosionInput<'a> {
    pub land: &'a [u8],
    pub h: &'a mut [f32],
    pub uplift: &'a [f32],
    pub rain: &'a [f32],
    pub steps: usize,
}

pub(crate) struct Drainage {
    pub order: Vec<usize>,
    pub receiver: Vec<usize>,
    pub filled: Vec<f32>,
}
impl Drainage {
    pub fn new(n: usize) -> Self {
        Self {
            order: Vec::with_capacity(n),
            receiver: vec![usize::MAX; n],
            filled: vec![0.0; n],
        }
    }
}
pub(crate) struct DrainageRouter<'a> {
    mesh: &'a Mesh,
    land: &'a [u8],
    elen: Vec<f64>,
    shore: Vec<usize>,
}
impl<'a> DrainageRouter<'a> {
    pub(crate) fn new(mesh: &'a Mesh, geo: &SphereGeometry<'_>, land: &'a [u8]) -> Self {
        let mut seen = vec![false; mesh.n];
        let mut shore = Vec::new();
        for i in 0..mesh.n {
            if land[i] != 0 {
                continue;
            }
            for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
                if land[j] != 0 && !seen[j] {
                    seen[j] = true;
                    shore.push(j);
                }
            }
        }
        Self {
            mesh,
            land,
            elen: geo.edge_lengths(),
            shore,
        }
    }
    pub(crate) fn route(
        &self,
        h: &[f32],
        eps: f64,
        out: &mut Drainage,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(), TerrainError> {
        let mesh = self.mesh;
        let land = self.land;
        out.receiver.fill(usize::MAX);
        out.order.clear();
        let mut state = vec![0_u8; mesh.n];
        let mut heap = MinHeap::new(mesh.n);
        for i in 0..mesh.n {
            if land[i] == 0 {
                state[i] = 2;
                out.filled[i] = h[i].min(0.0);
            }
        }
        for &j in &self.shore {
            state[j] = 1;
            out.filled[j] = f64::from(h[j]).max(eps) as f32;
            heap.push(j, f64::from(out.filled[j]));
        }
        while let Some((i, _)) = heap.pop() {
            if out.order.len() % 256 == 0 {
                check_cancel(cancel)?;
            }
            state[i] = 2;
            out.order.push(i);
            let fi = f64::from(out.filled[i]);
            for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
                if state[j] != 0 {
                    continue;
                }
                state[j] = 1;
                out.filled[j] = f64::from(h[j]).max(fi + eps) as f32;
                heap.push(j, f64::from(out.filled[j]));
            }
        }
        for &i in &out.order {
            let fi = f64::from(out.filled[i]);
            let mut best = usize::MAX;
            let mut best_s = f64::NEG_INFINITY;
            for k in mesh.adj_start[i]..mesh.adj_start[i + 1] {
                let j = mesh.adj[k];
                let fj = if land[j] != 0 {
                    f64::from(out.filled[j])
                } else {
                    f64::from(out.filled[j]).min(0.0)
                };
                if fj >= fi && land[j] != 0 {
                    continue;
                }
                let s = (fi - fj) / self.elen[k];
                if s > best_s {
                    best_s = s;
                    best = j;
                }
            }
            out.receiver[i] = best;
        }
        Ok(())
    }
}
pub(crate) fn accumulate(d: &Drainage, land: &[u8], weight: &[f32], out: &mut [f32]) {
    out.copy_from_slice(weight);
    for &i in d.order.iter().rev() {
        let r = d.receiver[i];
        if r != usize::MAX && land[r] != 0 {
            out[r] = (f64::from(out[r]) + f64::from(out[i])) as f32;
        }
    }
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
    fn erosion_steps_match_upstream_flood_order() {
        let mesh = build_mesh(
            2048.0,
            1024.0,
            sphere_spacing(2048.0, 256),
            &mut Mulberry32::new(sub_seed(42, "mesh")),
            &|| false,
        )
        .expect("mesh");
        let geo = SphereGeometry::new(&mesh);
        let land: Vec<_> = r::TECT_LAND.iter().map(|&v| v as u8).collect();
        let uplift: Vec<_> = r::TECT_UPLIFT.iter().map(|&v| f32::from_bits(v)).collect();
        let mut h: Vec<_> = r::H_0.iter().map(|&v| f32::from_bits(v)).collect();
        for (rain, want) in [(r::RAIN_0, r::H_18), (r::RAIN_18, r::H_36)] {
            let rain: Vec<_> = rain.iter().map(|&v| f32::from_bits(v)).collect();
            erode(
                &mesh,
                &geo,
                ErosionInput {
                    land: &land,
                    h: &mut h,
                    uplift: &uplift,
                    rain: &rain,
                    steps: 18,
                },
                &|| false,
            )
            .expect("erosion");
            assert!(h.iter().all(|v| v.is_finite()));
            if crate::geometry_reference::trig_matches() {
                assert_eq!(h.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), want);
            }
        }
    }
}
