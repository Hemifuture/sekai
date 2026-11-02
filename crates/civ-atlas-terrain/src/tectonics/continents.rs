// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas tectonics.ts pickContinents, shuffle and mergeFragments.
use crate::{
    geometry::SphereGeometry,
    mesh::Mesh,
    params::TerrainParams,
    random::{round24, Mulberry32},
    util::{check_cancel, smoothstep},
    TerrainError,
};

const WEIGHTS: [f64; 8] = [1.0, 0.62, 0.44, 0.33, 0.25, 0.2, 0.17, 0.15];
pub(super) struct Continents {
    pub continental: Vec<u8>,
    pub micro: Vec<u8>,
    pub archipelago: Vec<u8>,
}
pub(super) fn merge_fragments(
    mesh: &Mesh,
    plate: &mut [usize],
    count: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<(), TerrainError> {
    let n = mesh.n;
    let mut comp = vec![usize::MAX; n];
    for _ in 0..2 {
        check_cancel(cancel)?;
        comp.fill(usize::MAX);
        let mut sizes = Vec::new();
        let mut plates = Vec::new();
        let mut q = Vec::new();
        for i in 0..n {
            if comp[i] != usize::MAX {
                continue;
            }
            let id = sizes.len();
            let pk = plate[i];
            q.clear();
            q.push(i);
            comp[i] = id;
            let mut head = 0;
            while head < q.len() {
                let c = q[head];
                head += 1;
                for &j in &mesh.adj[mesh.adj_start[c]..mesh.adj_start[c + 1]] {
                    if comp[j] == usize::MAX && plate[j] == pk {
                        comp[j] = id;
                        q.push(j);
                    }
                }
            }
            sizes.push(q.len());
            plates.push(pk);
        }
        let mut main = vec![usize::MAX; count];
        for c in 0..sizes.len() {
            let pk = plates[c];
            if main[pk] == usize::MAX || sizes[c] > sizes[main[pk]] {
                main[pk] = c;
            }
        }
        if (0..sizes.len()).all(|c| main[plates[c]] == c) {
            return Ok(());
        }
        let mut cells = vec![Vec::new(); sizes.len()];
        for i in 0..n {
            if main[plates[comp[i]]] != comp[i] {
                cells[comp[i]].push(i);
            }
        }
        let mut target = vec![usize::MAX; sizes.len()];
        let mut votes = vec![0_usize; count];
        for c in 0..sizes.len() {
            if main[plates[c]] == c {
                continue;
            }
            votes.fill(0);
            for &i in &cells[c] {
                for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
                    if plate[j] != plates[c] {
                        votes[plate[j]] += 1;
                    }
                }
            }
            let mut best = usize::MAX;
            let mut bv = 0;
            for (pj, &v) in votes.iter().enumerate() {
                if v > bv {
                    bv = v;
                    best = pj;
                }
            }
            target[c] = best;
        }
        for i in 0..n {
            let t = target[comp[i]];
            if t != usize::MAX {
                plate[i] = t;
            }
        }
    }
    Ok(())
}

struct Selection<'a> {
    height: f64,
    count: usize,
    shared: &'a [f32],
    cx: &'a [f32],
    cy: &'a [f32],
    geo: &'a SphereGeometry<'a>,
    p: &'a TerrainParams,
}
impl Selection<'_> {
    fn lat(&self, k: usize) -> f64 {
        90.0 - (180.0 * f64::from(self.cy[k])) / self.height
    }
    fn polar(&self, k: usize) -> bool {
        self.lat(k).abs() > 58.0
    }
    fn hemi(&self, k: usize) -> f64 {
        if self.p.north == 0.0 {
            1.0
        } else {
            (1.0 + (self.p.north * self.lat(k)) / 30.0).max(0.02)
        }
    }
    fn near0(&self, k: usize, core: &[usize], width: f64) -> f64 {
        if self.p.gather == 0.0 || core.is_empty() {
            1.0
        } else {
            round24(
                ((-self.p.gather
                    * 10.0
                    * self.geo.point_dist(
                        [f64::from(self.cx[core[0]]), f64::from(self.cy[core[0]])],
                        [f64::from(self.cx[k]), f64::from(self.cy[k])],
                    ))
                    / width)
                    .exp(),
            )
        }
    }
    fn touches_other(&self, k: usize, c: i32, owner: &[i32]) -> bool {
        (0..self.count)
            .any(|b| self.shared[k * self.count + b] > 0.0 && owner[b] >= 0 && owner[b] != c)
    }
    fn near_cont(&self, k: usize, cont: &[u8]) -> bool {
        (0..self.count).any(|b| self.shared[k * self.count + b] > 0.0 && cont[b] != 0)
    }
}

pub(super) struct PickInput<'a> {
    pub params: &'a TerrainParams,
    pub plate: &'a [usize],
    pub count: usize,
    pub area: &'a [f32],
    pub centers: (&'a [f32], &'a [f32]),
    pub pole: i32,
}
pub(super) fn pick_continents(
    mesh: &Mesh,
    geo: &SphereGeometry<'_>,
    input: PickInput<'_>,
    rng: &mut Mulberry32,
    cancel: &dyn Fn() -> bool,
) -> Result<Continents, TerrainError> {
    let PickInput {
        params: p,
        plate,
        count,
        area,
        centers: (cx, cy),
        pole,
    } = input;
    let n = mesh.n;
    let width = mesh.width;
    let mut shared = vec![0.0_f32; count * count];
    for i in 0..n {
        let a = plate[i];
        for &j in &mesh.adj[mesh.adj_start[i]..mesh.adj_start[i + 1]] {
            let b = plate[j];
            if b != a {
                shared[a * count + b] += 1.0;
            }
        }
    }
    let s = Selection {
        height: mesh.height,
        count,
        shared: &shared,
        cx,
        cy,
        geo,
        p,
    };
    let avg = n as f64 / count as f64;
    let target = n as f64 * (p.land_fraction * 1.3).clamp(0.08, 0.92);
    let r = rng.next_f64();
    let mut c_count = if r < 0.12 {
        2
    } else if r < 0.45 {
        3
    } else if r < 0.82 {
        4
    } else {
        5
    };
    if p.continents > 0 {
        c_count = p.continents;
    }
    c_count = 1.max(
        c_count
            .min(count / 3)
            .min((target / (avg * 0.7)).floor() as usize),
    );
    let weight: Vec<_> = WEIGHTS[..c_count]
        .iter()
        .map(|&w| w * (0.8 + 0.4 * rng.next_f64()))
        .collect();
    let mut ax_x = Vec::new();
    let mut ax_y = Vec::new();
    for _ in 0..c_count {
        let a = rng.next_f64() * std::f64::consts::PI;
        ax_x.push(round24(a.cos()));
        ax_y.push(round24(a.sin()));
    }
    let mut core = Vec::new();
    let mut owner = vec![-1_i32; count];
    let mut cont_area = Vec::new();
    let polar_c = if pole != 0 && c_count >= 2 {
        c_count - 1
    } else {
        usize::MAX
    };
    let mut total = 0.0;
    let mut order: Vec<_> = (0..count).collect();
    for i in (1..count).rev() {
        let j = (rng.next_f64() * (i + 1) as f64).floor() as usize;
        order.swap(i, j);
    }
    for c in 0..c_count {
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        if c == polar_c {
            let at_pole =
                plate[geo.nearest([width / 2.0, if pole > 0 { 0.0 } else { mesh.height }], 0)];
            if owner[at_pole] < 0 {
                best = Some(at_pole);
            } else {
                for &k in &order {
                    if owner[k] >= 0 || f64::from(area[k]) < avg * 0.6 {
                        continue;
                    }
                    let score = f64::from(pole) * s.lat(k);
                    if score > best_score {
                        best_score = score;
                        best = Some(k);
                    }
                }
            }
            if let Some(best) = best {
                owner[best] = c as i32;
                core.push(best);
                cont_area.push(f64::from(area[best]));
                total += f64::from(area[best]);
                let mut cap_cnt = vec![0.0_f32; count];
                let mut cap_all = 0.0;
                for i in 0..n {
                    if f64::from(pole) * geo.latitude(i) <= 72.0 {
                        continue;
                    }
                    cap_cnt[plate[i]] += 1.0;
                    cap_all += 1.0;
                }
                for k in 0..count {
                    if owner[k] >= 0 || f64::from(cap_cnt[k]) < 0.15 * cap_all {
                        continue;
                    }
                    owner[k] = c as i32;
                    cont_area[c] += f64::from(area[k]);
                    total += f64::from(area[k]);
                }
            }
            continue;
        }
        for &k in &order {
            if owner[k] >= 0 || s.polar(k) || f64::from(area[k]) < avg * 0.6 {
                continue;
            }
            if c > 0 && s.touches_other(k, -2, &owner) {
                continue;
            }
            let mut dmin = width;
            for b in 0..count {
                if owner[b] >= 0 {
                    dmin = dmin.min(geo.point_dist(
                        [f64::from(cx[b]), f64::from(cy[b])],
                        [f64::from(cx[k]), f64::from(cy[k])],
                    ));
                }
            }
            let score = (if c == 0 {
                f64::from(area[k]) * (0.5 + rng.next_f64())
            } else {
                dmin * (0.7 + 0.6 * rng.next_f64()) * s.near0(k, &core, width)
            }) * s.hemi(k);
            if score > best_score {
                best_score = score;
                best = Some(k);
            }
        }
        let Some(best) = best else {
            break;
        };
        owner[best] = c as i32;
        core.push(best);
        cont_area.push(f64::from(area[best]));
        total += f64::from(area[best]);
    }
    if cont_area.is_empty() {
        owner[order[0]] = 0;
        core.push(order[0]);
        cont_area.push(f64::from(area[order[0]]));
        total += f64::from(area[order[0]]);
    }
    let wsum: f64 = weight[..cont_area.len()].iter().sum();
    let goal: Vec<_> = (0..cont_area.len())
        .map(|c| (target * weight[c]) / wsum)
        .collect();
    let mut closed = vec![false; cont_area.len()];
    let mut allow_merge = cont_area.len() == 1 || p.gather >= 0.7;
    while total < target {
        check_cancel(cancel)?;
        let mut c = None;
        let mut cs = f64::INFINITY;
        for q in 0..cont_area.len() {
            if closed[q] {
                continue;
            }
            let v = (cont_area[q] / goal[q]) * (0.85 + 0.3 * rng.next_f64());
            if v < cs {
                cs = v;
                c = Some(q);
            }
        }
        let Some(c) = c else {
            if allow_merge {
                break;
            }
            allow_merge = true;
            closed.fill(false);
            continue;
        };
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        for &k in &order {
            if owner[k] >= 0 {
                continue;
            }
            let mut sh = 0.0;
            for b in 0..count {
                if owner[b] == c as i32 {
                    sh += f64::from(shared[k * count + b]);
                }
            }
            if sh <= 0.0 || (!allow_merge && s.touches_other(k, c as i32, &owner)) {
                continue;
            }
            let mut score = sh.sqrt() * (0.5 + rng.next_f64());
            let al = geo.point_dot(
                [f64::from(cx[core[c]]), f64::from(cy[core[c]])],
                [f64::from(cx[k]), f64::from(cy[k])],
                [ax_x[c], ax_y[c]],
            );
            if c == polar_c {
                score *= 0.3 + 1.7 * smoothstep(35.0, 80.0, f64::from(pole) * s.lat(k));
            } else {
                score *= 0.3 + 1.4 * al * al;
                if s.polar(k) {
                    score *= 0.3;
                }
            }
            if cont_area[c] + f64::from(area[k]) > goal[c] * 1.5 {
                score *= 0.4;
            }
            score *= s.hemi(k) * s.near0(k, &core, width);
            if score > best_score {
                best_score = score;
                best = Some(k);
            }
        }
        let Some(best) = best else {
            closed[c] = true;
            continue;
        };
        owner[best] = c as i32;
        cont_area[c] += f64::from(area[best]);
        total += f64::from(area[best]);
    }
    let continental: Vec<u8> = owner.iter().map(|&o| u8::from(o >= 0)).collect();
    let mut micro = vec![0_u8; count];
    let n_micro = (((2.0 + (rng.next_f64() * 3.0).floor()) * p.rugged) + 0.5).floor() as usize;
    let mut got = 0;
    for pass in [0, 1] {
        for &k in &order {
            if got >= n_micro {
                break;
            }
            if continental[k] != 0
                || micro[k] != 0
                || s.polar(k)
                || f64::from(area[k]) > avg * 1.1
                || f64::from(area[k]) < avg * 0.12
            {
                continue;
            }
            if pass == 0
                && (s.near_cont(k, &continental)
                    || s.hemi(k) < 0.5
                    || s.near0(k, &core, width) < 0.3)
            {
                continue;
            }
            micro[k] = 1;
            got += 1;
        }
    }
    let mut archipelago = vec![0_u8; count];
    let n_arch = (((rng.next_f64() * 2.6).floor() * p.rugged) + 0.5).floor() as usize;
    got = 0;
    for &k in &order {
        if got >= n_arch {
            break;
        }
        if continental[k] != 0
            || micro[k] != 0
            || s.polar(k)
            || s.near_cont(k, &continental)
            || s.hemi(k) < 0.5
            || s.near0(k, &core, width) < 0.3
        {
            continue;
        }
        archipelago[k] = 1;
        got += 1;
    }
    Ok(Continents {
        continental,
        micro,
        archipelago,
    })
}
