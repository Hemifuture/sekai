// SPDX-License-Identifier: AGPL-3.0-only
// Ported from Civ Atlas src/gen/tectonics.ts; see NOTICE.md.
use crate::{
    geometry::SphereGeometry,
    mesh::{blur_field, Mesh},
    params::TerrainParams,
    random::{keyed, round24, sub_seed, Mulberry32},
    util::{check_cancel, smoothstep, MinHeap},
    TerrainError,
};
mod continents;
use continents::{merge_fragments, pick_continents, PickInput};

const SIDE_ARC: u8 = 1;
const SIDE_SLAB: u8 = 2;
const SIDE_UPPER: u8 = 3;
const SIDE_LOWER: u8 = 4;

fn band(d: f64, w: f64) -> f64 {
    let t = d / w;
    if !(-4.0..=4.0).contains(&t) {
        0.0
    } else {
        round24((-t * t).exp())
    }
}

struct DistanceField {
    dist: Vec<f32>,
    strength: Vec<f32>,
    src: Vec<usize>,
}
fn distance_field(
    mesh: &Mesh,
    elen: &[f64],
    sources: &[usize],
    strength: impl Fn(usize) -> f64,
    cancel: &dyn Fn() -> bool,
) -> Result<DistanceField, TerrainError> {
    let mut dist = vec![f64::INFINITY; mesh.n];
    let mut str = vec![0.0_f32; mesh.n];
    let mut src = vec![usize::MAX; mesh.n];
    let mut heap = MinHeap::new(mesh.n);
    for &s in sources {
        dist[s] = 0.0;
        str[s] = strength(s) as f32;
        src[s] = s;
        heap.push(s, 0.0);
    }
    let mut pops = 0;
    while let Some((i, d)) = heap.pop() {
        if pops % 256 == 0 {
            check_cancel(cancel)?;
        }
        pops += 1;
        if d > dist[i] {
            continue;
        }
        for (k, &length) in elen
            .iter()
            .enumerate()
            .take(mesh.adj_start[i + 1])
            .skip(mesh.adj_start[i])
        {
            let j = mesh.adj[k];
            let nd = d + length;
            if nd < dist[j] {
                dist[j] = nd;
                str[j] = str[i];
                src[j] = src[i];
                heap.push(j, nd);
            }
        }
    }
    Ok(DistanceField {
        dist: dist.into_iter().map(|v| v as f32).collect(),
        strength: str,
        src,
    })
}

#[derive(Debug)]
pub(crate) struct Tectonics {
    pub(crate) plate: Vec<usize>,
    pub(crate) land: Vec<u8>,
    pub(crate) uplift: Vec<f32>,
    pub(crate) ocean_depth: Vec<f32>,
    pub(crate) plateau: Vec<f32>,
    pub(crate) plate_continental: Vec<u8>,
    pub(crate) dist_div: Vec<f32>,
    pub(crate) strength_div: Vec<f32>,
}

pub(crate) fn build_tectonics(
    mesh: &Mesh,
    geo: &SphereGeometry<'_>,
    p: &TerrainParams,
    cancel: &dyn Fn() -> bool,
) -> Result<Tectonics, TerrainError> {
    let n = mesh.n;
    let w = mesh.width;
    let spacing = mesh.spacing;
    let elen = geo.edge_lengths();
    let mut rng = Mulberry32::new(sub_seed(p.seed, "plates"));
    let k_count = p.plates.max(3);
    let thin = |w: f64| w.max(0.9 * spacing);
    let rg = p.rugged;
    let rg_a = (1.0 + rg) / 2.0;
    let soft = 1.0 / p.hardness;
    let soft_w = round24(soft.sqrt());
    let min_seed_dist = 0.45 * (geo.area / k_count as f64).sqrt();
    let mut seeds = Vec::new();
    for tries in 0..k_count * 400 {
        if seeds.len() >= k_count {
            break;
        }
        let i = (rng.next_f64() * n as f64).floor() as usize;
        if seeds.iter().all(|&s| {
            geo.dist(s, i) > min_seed_dist * if tries > k_count * 200 { 0.5 } else { 1.0 }
        }) {
            seeds.push(i);
        }
    }
    let count = seeds.len();
    let mut rate = vec![0.0_f32; count];
    for v in &mut rate {
        let gauss = (rng.next_f64() + rng.next_f64() + rng.next_f64() - 1.5) * 2.0;
        *v = round24((0.42 * gauss).exp()).clamp(0.5, 1.9) as f32;
    }
    let rough = geo.fbm(sub_seed(p.seed, "plate-rough"), 5, 0.5);
    let fr = 3.0 / w;
    let mut plate0 = vec![usize::MAX; n];
    let mut cost = vec![f64::INFINITY; n];
    let mut heap = MinHeap::new(n);
    for (k, &s) in seeds.iter().enumerate() {
        plate0[s] = k;
        cost[s] = 0.0;
        heap.push(s, 0.0);
    }
    let mut closed = vec![false; n];
    let mut grow_w = vec![0.0; n];
    for (j, v) in grow_w.iter_mut().enumerate() {
        if j % 256 == 0 {
            check_cancel(cancel)?;
        }
        let nv = 0.5 + 0.5 * rough.at(j, fr, 1.0, [0.0, 0.0]);
        *v = 0.12 + 2.4 * nv * nv;
    }
    while let Some((i, _)) = heap.pop() {
        if closed[i] {
            continue;
        }
        closed[i] = true;
        if i % 256 == 0 {
            check_cancel(cancel)?;
        }
        let pk = plate0[i];
        for (k, &length) in elen
            .iter()
            .enumerate()
            .take(mesh.adj_start[i + 1])
            .skip(mesh.adj_start[i])
        {
            let j = mesh.adj[k];
            if closed[j] {
                continue;
            }
            let c = cost[i] + (length * grow_w[j]) / f64::from(rate[pk]);
            if c < cost[j] {
                cost[j] = c;
                plate0[j] = pk;
                heap.push(j, c);
            }
        }
    }
    let warp_a = geo.fbm(sub_seed(p.seed, "warp-a"), 4, 0.5);
    let warp_b = geo.fbm(sub_seed(p.seed, "warp-b"), 4, 0.5);
    let warp_c = geo.fbm(sub_seed(p.seed, "warp-c"), 3, 0.5);
    let warp_d = geo.fbm(sub_seed(p.seed, "warp-d"), 3, 0.5);
    let warp_e = geo.fbm(sub_seed(p.seed, "warp-e"), 2, 0.5);
    let fw = 2.2 / w;
    let fw2 = 6.5 / w;
    let fw3 = 18.0 / w;
    let amp = 0.085 * w;
    let amp2 = 0.035 * w * rg_a;
    let amp3 = 0.012 * w * rg;
    let mut plate = vec![0; n];
    for (i, pk) in plate.iter_mut().enumerate() {
        if i % 256 == 0 {
            check_cancel(cancel)?;
        }
        let mut q = geo.move_cell(
            i,
            [
                [
                    amp * warp_a.at(i, fw, 1.0, [0.0, 0.0]),
                    amp * warp_b.at(i, fw, 1.0, [5.2, 1.3]),
                ],
                [
                    amp2 * warp_c.at(i, fw2, 1.0, [0.0, 0.0]),
                    amp2 * warp_d.at(i, fw2, 1.0, [0.0, 0.0]),
                ],
                [
                    amp3 * warp_e.at(i, fw3, 1.0, [0.0, 0.0]),
                    amp3 * warp_e.at(i, fw3, 1.0, [7.1, -2.6]),
                ],
            ],
        );
        geo.clamp_point(&mut q);
        *pk = plate0[geo.nearest(q, i)];
    }
    merge_fragments(mesh, &mut plate, count, cancel)?;
    let mut vx = vec![0.0_f32; count];
    let mut vy = vec![0.0_f32; count];
    let mut density = vec![0.0_f32; count];
    let mut area = vec![0_usize; count];
    for &pk in &plate {
        area[pk] += 1;
    }
    for k in 0..count {
        let a = rng.next_f64() * std::f64::consts::PI * 2.0;
        let s = 0.3 + 0.7 * rng.next_f64();
        vx[k] = (round24(a.cos()) * s) as f32;
        vy[k] = (round24(a.sin()) * s) as f32;
        density[k] = rng.next_f64() as f32;
    }
    let centers = geo.group_centroids(&plate, count, &area);
    let spin_base = sub_seed(p.seed, "plate-spin");
    let spin: Vec<_> = (0..count)
        .map(|k| f64::from((2.0 * keyed(spin_base, k as i32, 0, 0) - 1.0) as f32))
        .collect();
    let motion = geo.plate_motion(&vx, &vy, (&centers.0, &centers.1), &spin);
    let polar_seed = sub_seed(p.seed, "polar-continent");
    let mut pole = if keyed(polar_seed, 0, 0, 0) < 0.4 {
        if keyed(polar_seed, 1, 0, 0) < 0.5 {
            1
        } else {
            -1
        }
    } else {
        0
    };
    if pole != 0 && p.north.abs() >= 0.5 {
        pole = if p.north > 0.0 { 1 } else { -1 };
    }
    let area_f: Vec<_> = area.iter().map(|&a| a as f32).collect();
    let pick = pick_continents(
        mesh,
        geo,
        PickInput {
            params: p,
            plate: &plate,
            count,
            area: &area_f,
            centers: (&centers.0, &centers.1),
            pole,
        },
        &mut rng,
        cancel,
    )?;
    let plate_continental: Vec<_> = (0..count)
        .map(|k| u8::from(pick.continental[k] != 0 || pick.micro[k] != 0))
        .collect();
    let crust: Vec<_> = plate
        .iter()
        .map(|&k| {
            if pick.continental[k] != 0 {
                2
            } else if pick.micro[k] != 0 {
                1
            } else {
                0
            }
        })
        .collect();
    let mut convergence = vec![0.0_f32; n];
    let mut side = vec![0_u8; n];
    let mut conv_sources = Vec::new();
    let mut div_sources = Vec::new();
    for i in 0..n {
        let pi = plate[i];
        if i % 256 == 0 {
            check_cancel(cancel)?;
        }
        let mut sum = 0.0;
        let mut cnt = 0;
        let mut other = usize::MAX;
        let mut oc = usize::MAX;
        for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
            let pj = plate[j];
            if pj == pi {
                continue;
            }
            if other == usize::MAX {
                other = pj;
                oc = j;
            }
            let va = motion.at(geo, pi, i);
            let vb = motion.at(geo, pj, i);
            sum += geo.edge_dot(i, j, [va[0] - vb[0], va[1] - vb[1]]);
            cnt += 1;
        }
        if cnt == 0 {
            continue;
        }
        let c = sum / f64::from(cnt);
        convergence[i] = c as f32;
        if c > 0.25 {
            conv_sources.push(i);
            let ca = crust[i] > 0;
            let cb = crust[oc] > 0;
            side[i] = if ca && cb {
                if density[pi] < density[other] {
                    SIDE_UPPER
                } else {
                    SIDE_LOWER
                }
            } else if ca != cb {
                if ca {
                    SIDE_ARC
                } else {
                    SIDE_SLAB
                }
            } else if density[pi] < density[other] {
                SIDE_ARC
            } else {
                SIDE_SLAB
            };
        } else if c < -0.25 {
            div_sources.push(i);
        }
    }
    let conv = distance_field(
        mesh,
        &elen,
        &conv_sources,
        |i| (f64::from(convergence[i]) / 0.9).clamp(0.0, 1.0),
        cancel,
    )?;
    let div = distance_field(
        mesh,
        &elen,
        &div_sources,
        |i| (-f64::from(convergence[i]) / 0.9).clamp(0.0, 1.0),
        cancel,
    )?;
    let side_of = |i: usize| {
        let s = conv.src[i];
        if s != usize::MAX && plate[s] == plate[i] {
            side[s]
        } else {
            0
        }
    };
    let shore: Vec<_> = (0..n)
        .filter(|&i| {
            mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]]
                .iter()
                .any(|&j| crust[j] != crust[i])
        })
        .collect();
    let sh = distance_field(mesh, &elen, &shore, |i| f64::from(convergence[i]), cancel)?;
    let arch_at = pick.archipelago.iter().any(|&a| a != 0);
    let arch_base: Vec<f32> = plate
        .iter()
        .map(|&k| f32::from(pick.archipelago[k]))
        .collect();
    let arch_blur = if arch_at {
        blur_field(
            mesh,
            &arch_base,
            4.max(((30.0 / spacing).powi(2) + 0.5).floor() as usize),
        )
    } else {
        arch_base
    };
    let coast_n = geo.fbm(sub_seed(p.seed, "land"), 3, 0.6);
    let coast_f = geo.fbm(sub_seed(p.seed, "coast-fine"), 4, 0.6);
    let rug_n = geo.fbm(sub_seed(p.seed, "rugged"), 3, 0.5);
    let shelf_isle = geo.fbm(sub_seed(p.seed, "shelf-isles"), 3, 0.5);
    let inset_n = geo.fbm(sub_seed(p.seed, "inset"), 3, 0.5);
    let island_n = geo.fbm(sub_seed(p.seed, "islands"), 4, 0.5);
    let bead_n = geo.noise(sub_seed(p.seed, "arc-beads"));
    let basin_n = geo.fbm(sub_seed(p.seed, "basins"), 5, 0.5);
    let fs = 2.6 / w;
    let fc = 3.4 / w;
    let ramp = 0.05 * w;
    let mut b_field = vec![0.0_f32; n];
    let mut l_field = vec![0.0_f32; n];
    for i in 0..n {
        if i % 256 == 0 {
            check_cancel(cancel)?;
        }
        let cr = crust[i];
        let c_s = f64::from(sh.strength[i]);
        let inset = (4.0 + 10.0 * smoothstep(0.1, -0.2, c_s) + 34.0 * smoothstep(-0.1, -0.6, c_s))
            * (0.55 + 0.9 * (0.5 + 0.5 * inset_n.at(i, fc, 1.3, [0.0, 0.0])));
        let sd = if cr > 0 {
            f64::from(sh.dist[i]) - inset
        } else {
            -f64::from(sh.dist[i]) - inset
        };
        let mut b = round24((sd / ramp).tanh());
        if cr == 1 {
            b = 0.8 * b - 0.12;
        }
        b_field[i] = b as f32;
        let rug = 0.25 + 1.35 * smoothstep(-0.35, 0.45, rug_n.at(i, fc, 0.8, [2.1, -6.3]));
        let coast = 0.75 * rg_a * coast_n.at(i, fc, 1.0, [0.0, 0.0])
            + 0.4 * rg * rug * coast_f.at(i, fc, 7.0, [0.0, 0.0]);
        let s = side_of(i);
        let cs = f64::from(conv.strength[i]);
        let cd = f64::from(conv.dist[i]);
        let mtn = if s == SIDE_UPPER || s == SIDE_LOWER || (s == SIDE_ARC && b > 0.0) {
            cs * band(cd, 22.0)
        } else {
            0.0
        };
        let arc = if s == SIDE_ARC && b < 0.2 {
            let beads = smoothstep(
                -0.35,
                0.45,
                bead_n.at_scale(i, 38.0, [0.0, 0.0]) + 0.35 * island_n.at(i, fc, 6.0, [0.0, 0.0]),
            );
            cs * band(cd - 11.0, thin(5.0)) * beads
        } else {
            0.0
        };
        let isl = if arch_blur[i] > 0.0 {
            f64::from(arch_blur[i]) * smoothstep(0.05, 0.55, island_n.at(i, fc, 2.4, [7.7, -3.3]))
        } else {
            0.0
        };
        let rift = b.max(0.0)
            * f64::from(div.strength[i])
            * band(
                f64::from(div.dist[i]),
                14.0 + 23.0 * f64::from(div.strength[i]),
            );
        let basin = if b > 0.45 {
            (b - 0.45) * 2.0 * smoothstep(0.2, 0.5, basin_n.at(i, fs, 1.2, [0.4 * coast, 0.0]))
        } else {
            0.0
        };
        let shelf_z = smoothstep(-0.8, -0.4, b) * (1.0 - smoothstep(-0.15, 0.15, b));
        let s_isle = if shelf_z > 0.0 {
            shelf_z * smoothstep(0.2, 0.55, shelf_isle.at(i, fc, 2.6, [-1.7, 4.4]))
        } else {
            0.0
        };
        if p.north != 0.0 {
            l_field[i] = (p.north * 0.22 * (geo.latitude(i) / 45.0).clamp(-1.0, 1.0)) as f32;
        }
        l_field[i] = (f64::from(l_field[i])
            + (0.62 * b + 0.4 * coast + 0.16 * mtn + 1.1 * arc + 0.6 * isl + 0.55 * rg * s_isle
                - 1.5 * rg * rift
                - 0.9 * rg * basin)) as f32;
    }
    let n_hot = 2 + (rng.next_f64() * 4.0).floor() as usize;
    let mut hot_at = Vec::new();
    for _ in 0..n_hot {
        let mut best = None;
        for _ in 0..60 {
            let i = (rng.next_f64() * n as f64).floor() as usize;
            if b_field[i] > -0.45 || hot_at.iter().any(|&j| geo.dist(j, i) < 180.0) {
                continue;
            }
            if best.is_none_or(|j| sh.dist[i] > sh.dist[j]) {
                best = Some(i);
            }
        }
        if let Some(best) = best {
            hot_at.push(best);
        }
    }
    let mut hot = vec![0.0_f32; n];
    for h in hot_at {
        let va = motion.at(geo, plate[h], h);
        let mut vl = (va[0] * va[0] + va[1] * va[1]).sqrt();
        if vl == 0.0 {
            vl = 1.0;
        }
        let dx = va[0] / vl;
        let dy = va[1] / vl;
        let m = 5 + (rng.next_f64() * 5.0).floor() as usize;
        let step = 12.0 + rng.next_f64() * 7.0;
        let r0 = 8.0 + rng.next_f64() * 4.0;
        for k in 0..m {
            let lat = (rng.next_f64() - 0.5) * 7.0;
            let q = geo.move_cell(
                h,
                [
                    [dx * step * k as f64, dy * step * k as f64],
                    [-(dy * lat), dx * lat],
                    [0.0, 0.0],
                ],
            );
            let r = thin(r0 * (1.0 - (0.55 * k as f64) / m as f64));
            let a = 1.45 * (1.0 - k as f64 / (m as f64 * 0.75));
            let radius = 3.0 * r;
            for i in geo.cells_near_point(q, radius) {
                let d = geo.near_to(i, q, radius);
                let g = band(d, r);
                if a > 0.0 {
                    l_field[i] = (f64::from(l_field[i]) + a * g) as f32;
                }
                hot[i] = f64::from(hot[i]).max(g * (0.5 + 0.5 * a.clamp(0.0, 1.0))) as f32;
            }
        }
    }
    let mut sorted = l_field.clone();
    sorted.sort_by(f32::total_cmp);
    let threshold =
        sorted[((1.0 - p.land_fraction.clamp(0.02, 0.95)) * (n - 1) as f64).floor() as usize];
    let mut land: Vec<_> = l_field.iter().map(|&l| u8::from(l > threshold)).collect();
    crate::inland_sea::connect_inland_seas(mesh, &mut land, cancel)?;
    finish_tectonics(
        mesh,
        geo,
        p,
        FinishInput {
            plate,
            land,
            plate_continental,
            div,
            conv,
            side,
            div_sources,
            hot,
            b_field,
            convergence,
            spacing,
            soft,
            soft_w,
            elen: &elen,
        },
        cancel,
    )
}

struct FinishInput<'a> {
    plate: Vec<usize>,
    land: Vec<u8>,
    plate_continental: Vec<u8>,
    div: DistanceField,
    conv: DistanceField,
    side: Vec<u8>,
    div_sources: Vec<usize>,
    hot: Vec<f32>,
    b_field: Vec<f32>,
    convergence: Vec<f32>,
    spacing: f64,
    soft: f64,
    soft_w: f64,
    elen: &'a [f64],
}
fn finish_tectonics(
    mesh: &Mesh,
    geo: &SphereGeometry<'_>,
    p: &TerrainParams,
    input: FinishInput<'_>,
    cancel: &dyn Fn() -> bool,
) -> Result<Tectonics, TerrainError> {
    let FinishInput {
        plate,
        land,
        plate_continental,
        div,
        conv,
        side,
        div_sources,
        hot,
        b_field,
        convergence,
        spacing,
        soft,
        soft_w,
        elen,
    } = input;
    let n = mesh.n;
    let fs = 2.6 / mesh.width;
    let thin = |w: f64| w.max(0.9 * spacing);
    let side_of = |i: usize| {
        let s = conv.src[i];
        if s != usize::MAX && plate[s] == plate[i] {
            side[s]
        } else {
            0
        }
    };
    // Only default-world consumers are retained; editor-only noises are never constructed.
    let sea_noise = geo.fbm(sub_seed(p.seed, "sea"), 5, 0.5);
    let shelf_n = geo.fbm(sub_seed(p.seed, "shelf"), 3, 0.5);
    let base_u = geo.fbm(sub_seed(p.seed, "uplift"), 5, 0.5);
    let hills = geo.fbm(sub_seed(p.seed, "hills"), 4, 0.5);
    let old = geo.ridged(sub_seed(p.seed, "old-ranges"), 5, 0.5);
    let plat = geo.fbm(sub_seed(p.seed, "plateau"), 4, 0.5);
    let shield = geo.fbm(sub_seed(p.seed, "shield"), 4, 0.5);
    let coast_src: Vec<_> = (0..n).filter(|&i| land[i] != 0).collect();
    let sea_src: Vec<_> = (0..n).filter(|&i| land[i] == 0).collect();
    let d_coast = distance_field(mesh, elen, &coast_src, |_| 0.0, cancel)?.dist;
    let ridge_src: Vec<_> = div_sources
        .into_iter()
        .filter(|&i| land[i] == 0 && b_field[i] < 0.1)
        .collect();
    let ridge = distance_field(
        mesh,
        elen,
        &ridge_src,
        |i| (-f64::from(convergence[i]) / 0.9).clamp(0.0, 1.0),
        cancel,
    )?;
    let mut ocean_depth = vec![0.0_f32; n];
    for i in 0..n {
        if i % 256 == 0 {
            check_cancel(cancel)?;
        }
        if land[i] != 0 {
            continue;
        }
        let rs = f64::from(ridge.strength[i]);
        let age = (f64::from(ridge.dist[i]) / 160.0).clamp(0.0, 1.0);
        let mut d =
            -2700.0 - 2700.0 * age.sqrt() + 500.0 * rs * band(f64::from(ridge.dist[i]), 14.0);
        d += 300.0 * sea_noise.at(i, fs, 2.0, [0.0, 0.0])
            + 90.0 * sea_noise.at(i, fs, 9.0, [3.3, -1.7]);
        let active = f64::from(conv.strength[i]) * band(f64::from(conv.dist[i]), 30.0);
        let sw = (3.0
            + 26.0
                * (1.0 - active)
                * (0.35 + 0.65 * smoothstep(-0.4, 0.5, shelf_n.at(i, fs, 1.6, [0.0, 0.0]))))
            * if b_field[i] > -0.2 { 1.3 } else { 0.7 };
        let dc = f64::from(d_coast[i]);
        let shelf = if dc < sw {
            -(30.0 + 150.0 * (dc / sw))
        } else {
            -(180.0 + 6500.0 * smoothstep(0.0, 24.0, dc - sw))
        };
        d = d.max(shelf);
        if side_of(i) == SIDE_ARC {
            d = d.max(
                -1600.0
                    - 2200.0
                        * (1.0
                            - f64::from(conv.strength[i])
                                * band(f64::from(conv.dist[i]) - 11.0, 9.0)),
            );
        }
        d += 2600.0 * f64::from(hot[i]);
        ocean_depth[i] = d.min(-20.0) as f32;
    }
    let smooth_depth = blur_field(mesh, &ocean_depth, 5);
    let mut trench = vec![0.0_f32; n];
    for i in 0..n {
        if land[i] != 0 || side_of(i) != SIDE_SLAB {
            continue;
        }
        trench[i] = ((3200.0 + 3300.0 * f64::from(conv.strength[i]))
            * f64::from(conv.strength[i])
            * band(f64::from(conv.dist[i]), thin(9.0))
            * smoothstep(2.0, 14.0, f64::from(d_coast[i]))) as f32;
    }
    let trench_s = blur_field(mesh, &trench, 1);
    for i in 0..n {
        ocean_depth[i] = if land[i] != 0 {
            0.0
        } else {
            (f64::from(smooth_depth[i]) - f64::from(trench_s[i])).clamp(-10500.0, -20.0) as f32
        };
    }
    let d_sea = distance_field(mesh, elen, &sea_src, |_| 0.0, cancel)?.dist;
    let mut uplift = vec![0.0_f32; n];
    let mut plateau = vec![0.0_f32; n];
    let mf = p.mountains;
    for i in 0..n {
        if i % 256 == 0 {
            check_cancel(cancel)?;
        }
        if land[i] == 0 {
            continue;
        }
        let mut u = 0.016 + 0.018 * soft * (base_u.at(i, fs, 1.5, [0.0, 0.0]) + 1.0);
        u += 0.1 * soft * smoothstep(0.3, 0.75, hills.at(i, fs, 2.2, [11.0, -3.0]));
        let s = side_of(i);
        let cs = f64::from(conv.strength[i]);
        let cd = f64::from(conv.dist[i]);
        let cont_f = if b_field[i] > 0.0 { 1.0 } else { 0.7 };
        if s == SIDE_UPPER || s == SIDE_LOWER {
            u += mf * 1.65 * soft_w * cs * band(cd, (7.0 + 9.0 * cs) * soft_w) * cont_f;
        } else if s == SIDE_ARC {
            u += mf * 1.45 * soft_w * cs * band(cd - 9.0, (5.0 + 5.0 * cs) * soft_w) * cont_f;
        } else if s == SIDE_SLAB {
            u += mf * 0.35 * cs * band(cd, 6.0) * cont_f;
        }
        let r = old.at(i, fs, 1.9, [3.1, -7.4]);
        u += mf
            * 0.4
            * soft
            * smoothstep(0.78, 0.96, r)
            * smoothstep(0.1, 0.55, hills.at(i, fs, 0.7, [-5.0, 2.0]) + 0.3);
        u *= 1.0 - 0.75 * f64::from(div.strength[i]) * band(f64::from(div.dist[i]), 20.0) * cont_f;
        uplift[i] = u as f32;
        let mut pb = 0.0;
        if (s == SIDE_UPPER || s == SIDE_ARC) && cs > 0.25 {
            let reach = if s == SIDE_UPPER { 28.0 } else { 16.0 }
                + 34.0 * (0.5 + 0.5 * plat.at(i, fs, 1.1, [4.0, 0.0]));
            let in_band = smoothstep(4.0, 12.0, cd) * (1.0 - smoothstep(reach, reach + 16.0, cd));
            let along = smoothstep(-0.25, 0.3, plat.at(i, fs, 2.3, [-8.0, 1.0]));
            pb = if s == SIDE_UPPER { 3400.0 } else { 2600.0 }
                * mf
                * soft
                * smoothstep(0.25, 0.8, cs)
                * in_band
                * along;
        }
        pb += 1400.0
            * smoothstep(-0.2, 0.3, shield.at(i, fs, 1.2, [0.0, 0.0]))
            * smoothstep(0.1, 0.5, f64::from(b_field[i]));
        plateau[i] = (pb * smoothstep(3.0, 30.0, f64::from(d_sea[i]))) as f32;
    }
    Ok(Tectonics {
        plate,
        land,
        uplift,
        ocean_depth,
        plateau,
        plate_continental,
        dist_div: div.dist,
        strength_div: div.strength,
    })
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
    fn tectonic_fields_match_pinned_upstream() {
        let p = TerrainParams {
            seed: 42,
            shape: crate::TerrainShape {
                cells: 256,
                ..Default::default()
            },
        };
        let mesh = build_mesh(
            2048.0,
            1024.0,
            sphere_spacing(2048.0, p.cells),
            &mut Mulberry32::new(sub_seed(p.seed, "mesh")),
            &|| false,
        )
        .expect("mesh");
        let tect =
            build_tectonics(&mesh, &SphereGeometry::new(&mesh), &p, &|| false).expect("tectonics");
        assert!(tect.uplift.iter().all(|v| v.is_finite()));
        assert!(tect.ocean_depth.iter().all(|v| v.is_finite()));
        if !crate::geometry_reference::trig_matches() {
            eprintln!("host trig differs; skip exact upstream fixture");
            return;
        }
        assert_eq!(tect.plate, r::TECT_PLATE, "plate topology");
        assert_eq!(
            tect.land
                .iter()
                .map(|&v| usize::from(v))
                .collect::<Vec<_>>(),
            r::TECT_LAND,
            "land"
        );
        for (name, got, want) in [
            ("uplift", &tect.uplift, r::TECT_UPLIFT),
            ("ocean depth", &tect.ocean_depth, r::TECT_OCEAN_DEPTH),
            ("plateau", &tect.plateau, r::TECT_PLATEAU),
            ("div distance", &tect.dist_div, r::TECT_DIST_DIV),
            ("div strength", &tect.strength_div, r::TECT_STRENGTH_DIV),
        ] {
            assert_eq!(
                got.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                want,
                "{name}"
            );
        }
    }
}
