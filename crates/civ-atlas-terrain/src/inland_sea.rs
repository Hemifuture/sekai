// SPDX-License-Identifier: AGPL-3.0-only
// Civ Atlas src/gen/inlandSea.ts; default generation without sketch protection.
use crate::{mesh::Mesh, util::check_cancel, TerrainError};

pub(crate) fn connect_inland_seas(
    mesh: &Mesh,
    land: &mut [u8],
    cancel: &dyn Fn() -> bool,
) -> Result<(), TerrainError> {
    let n = mesh.n;
    let mut comp = vec![usize::MAX; n];
    let mut comps: Vec<Vec<usize>> = Vec::new();
    for i in 0..n {
        if land[i] != 0 || comp[i] != usize::MAX {
            continue;
        }
        check_cancel(cancel)?;
        let mut cells = vec![i];
        comp[i] = comps.len();
        let mut q = 0;
        while q < cells.len() {
            let a = cells[q];
            q += 1;
            for &j in &mesh.adj[mesh.adj_start[a]..mesh.adj_start[a + 1]] {
                if land[j] == 0 && comp[j] == usize::MAX {
                    comp[j] = comps.len();
                    cells.push(j);
                }
            }
        }
        comps.push(cells);
    }
    if comps.len() < 2 {
        return Ok(());
    }
    let mut main = 0;
    for c in 1..comps.len() {
        if comps[c].len() > comps[main].len() {
            main = c;
        }
    }
    let main_size = comps[main].len() as f64;
    let mut target = vec![0_u8; n];
    let mut closed = Vec::new();
    for (c, cells) in comps.iter().enumerate() {
        if c == main || cells.len() as f64 > main_size * 0.025 {
            for &i in cells {
                target[i] = 1;
            }
        } else {
            closed.push(c);
        }
    }
    closed.sort_by(|&a, &b| comps[b].len().cmp(&comps[a].len()).then(a.cmp(&b)));
    let scale = n as f64 / 36000.0;
    let crumb = 3_usize.max((12.0 * scale + 0.5).floor() as usize);
    let strait = 1_i32.max((3.0 * scale.sqrt() + 0.5).floor() as i32);
    let mut depth = vec![-1_i32; n];
    let mut from = vec![usize::MAX; n];
    for c in closed {
        check_cancel(cancel)?;
        let cells = &comps[c];
        if cells.len() > crumb {
            depth.fill(-1);
            let mut queue = Vec::new();
            for &i in cells {
                depth[i] = 0;
                from[i] = usize::MAX;
                queue.push(i);
            }
            let mut hit = None;
            let mut q = 0;
            while q < queue.len() && hit.is_none() {
                let a = queue[q];
                q += 1;
                if depth[a] > strait {
                    break;
                }
                for &j in &mesh.adj[mesh.adj_start[a]..mesh.adj_start[a + 1]] {
                    if depth[j] >= 0 {
                        continue;
                    }
                    if target[j] != 0 {
                        if depth[a] > 0 && depth[a] <= strait {
                            hit = Some(a);
                            break;
                        }
                        continue;
                    }
                    if land[j] == 0 {
                        continue;
                    }
                    depth[j] = depth[a] + 1;
                    from[j] = a;
                    queue.push(j);
                }
            }
            if let Some(mut a) = hit {
                while a != usize::MAX && land[a] != 0 {
                    land[a] = 0;
                    target[a] = 1;
                    a = from[a];
                }
                for &i in cells {
                    target[i] = 1;
                }
                continue;
            }
        }
        for &i in cells {
            land[i] = 1;
        }
    }
    Ok(())
}
