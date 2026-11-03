//! Offline check: the upstream evaluator's own drawn class at the dry B < 0
//! points exported by Sekai's `anchor_rule_probe`. Ignored, Release only; reads
//! the export named by `CIV_ATLAS_DRY_NEG_POINTS` and prints numbers only.

use super::*;

const CLASS: [&str; 3] = ["land", "ocean", "lake"];
const GROUP: [&str; 4] = [
    "invalid_marine_cell",
    "valid_marine_cell_only",
    "no_marine_cell",
    "below_node_pocket",
];

struct Depression {
    area: f64,
    lat: f64,
    lon: f64,
    group: usize,
    ocean_rule: [bool; 2],
}

#[derive(Default, Clone, Copy)]
struct Tally {
    n: usize,
    zoom: [usize; 3],
    whole: [usize; 3],
    coarse: [usize; 3],
}
impl Tally {
    fn add(&mut self, zoom: u8, whole: u8, coarse: u8) {
        self.n += 1;
        self.zoom[usize::from(zoom)] += 1;
        self.whole[usize::from(whole)] += 1;
        self.coarse[usize::from(coarse)] += 1;
    }
    fn line(&self, label: &str, weight_km2: f64) -> String {
        let share = |v: &[usize; 3]| {
            (0..3)
                .map(|i| format!("{}={:.3}", CLASS[i], v[i] as f64 / self.n.max(1) as f64))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "{label} km2={:.0} zoom_s4[{}] whole_s1[{}] coarse_site[{}]",
            self.n as f64 * weight_km2,
            share(&self.zoom),
            share(&self.whole),
            share(&self.coarse)
        )
    }
}

type Export = (f64, Vec<Depression>, Vec<([f64; 3], i64)>);

fn read_export() -> Export {
    let path = std::env::var("CIV_ATLAS_DRY_NEG_POINTS").expect("export path");
    let text = std::fs::read_to_string(path).expect("export");
    let mut weight_km2 = 0.;
    let mut deps = Vec::new();
    let mut points = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| f[i].parse::<f64>().expect("number");
        match f[0] {
            "W" => weight_km2 = num(1),
            "D" => deps.push(Depression {
                area: num(2) + num(3),
                lat: num(4),
                lon: num(5),
                group: if num(7) > 0. {
                    0
                } else if num(6) > 0. {
                    1
                } else {
                    2
                },
                ocean_rule: [num(8) > 0., num(9) > 0.],
            }),
            "P" => points.push(([num(1), num(2), num(3)], f[4].parse::<i64>().expect("id"))),
            other => panic!("unknown record {other}"),
        }
    }
    (weight_km2, deps, points)
}

#[test]
#[ignore = "reads a Sekai export; one default-world fine source; run serialized Release"]
fn upstream_class_at_dry_depressions() {
    let (weight_km2, deps, points) = read_export();
    let p = crate::TerrainParams {
        seed: 42,
        ..Default::default()
    };
    let terrain = Arc::new(crate::generate_terrain(&p, &|| false).expect("terrain"));
    let fine = FineTerrain::new(terrain, FineScale::Four, &|| false).expect("fine");
    let source = &fine.source;
    // The original scale-1 whole map, built exactly as `FineSource::new` builds it.
    let main_lines = river_geometry::river_geometry(
        &source.terrain.initial_rivers,
        source.terrain.river_threshold,
        WIDTH as f64,
    );
    let main_valleys = Valleys::new(
        &main_lines,
        source.terrain.river_threshold,
        1.,
        [WIDTH as f64, HEIGHT as f64],
    );
    let whole = shoreline::whole(
        &SampleSource {
            base: &source.base,
            cover: &source.cover,
            valleys: &main_valleys,
            scale: 1.,
            zoom: false,
        },
        &source.detail,
        &|| false,
    )
    .expect("whole");
    let mut scratch = source.detail.workspace();
    let mut groups = [Tally::default(); 4];
    let mut rules = [[Tally::default(); 2]; 2];
    let mut per_dep = vec![Tally::default(); deps.len()];
    let mut bed_sign_mismatch = 0;
    for &(d, dep) in &points {
        let xy = coordinates(d).expect("direction");
        let raw = fine.raw(xy, &mut scratch).expect("raw");
        let bed = fine.restore_bed(xy, d, raw.height).expect("bed");
        bed_sign_mismatch += usize::from(bed >= 0.);
        let px = (xy[0].floor() as usize).min(WIDTH - 1);
        let py = (xy[1].floor() as usize).min(HEIGHT - 1);
        let whole_class = whole[py * WIDTH + px].water;
        let (sites, w) = fine.source_triangle(d).expect("triangle");
        let nearest = (0..3)
            .max_by(|a, b| w[*a].total_cmp(&w[*b]))
            .expect("three nodes");
        let coarse = source.terrain.water[sites[nearest]];
        let group = if dep < 0 { 3 } else { deps[dep as usize].group };
        groups[group].add(raw.water, whole_class, coarse);
        if dep >= 0 {
            let dep = dep as usize;
            per_dep[dep].add(raw.water, whole_class, coarse);
            for r in 0..2 {
                rules[r][usize::from(deps[dep].ocean_rule[r])].add(raw.water, whole_class, coarse);
            }
        }
    }
    let mut total = Tally::default();
    for g in &groups {
        total.n += g.n;
        for i in 0..3 {
            total.zoom[i] += g.zoom[i];
            total.whole[i] += g.whole[i];
            total.coarse[i] += g.coarse[i];
        }
    }
    eprintln!(
        "UPSTREAM points={} bed_sign_mismatch={bed_sign_mismatch}",
        points.len()
    );
    eprintln!("UPSTREAM {}", total.line("all_dry_neg", weight_km2));
    for (g, tally) in groups.iter().enumerate() {
        eprintln!("UPSTREAM {}", tally.line(GROUP[g], weight_km2));
    }
    for (r, name) in ["B", "C"].iter().enumerate() {
        for (flag, label) in ["stays_dry", "becomes_ocean"].iter().enumerate() {
            eprintln!(
                "UPSTREAM {}",
                rules[r][flag].line(&format!("rule{name}_{label}"), weight_km2)
            );
        }
    }
    let mut order: Vec<usize> = (0..deps.len()).collect();
    order.sort_by(|a, b| deps[*b].area.total_cmp(&deps[*a].area));
    for &d in order.iter().take(15) {
        let dep = &deps[d];
        eprintln!(
            "UPSTREAM dep={d} area_km2={:.0} lat={:.2} lon={:.2} group={} ruleB_ocean={} ruleC_ocean={} {}",
            dep.area,
            dep.lat,
            dep.lon,
            GROUP[dep.group],
            dep.ocean_rule[0],
            dep.ocean_rule[1],
            per_dep[d].line("sampled", weight_km2)
        );
    }
}

const NO_LABEL: u32 = u32::MAX;

/// Label 4- or 8-connected ocean pixels; x wraps, y does not.
fn label_ocean(class: &[u8], width: usize, height: usize, diagonal: bool) -> (Vec<u32>, Vec<u64>) {
    let mut label = vec![NO_LABEL; class.len()];
    let mut sizes = Vec::new();
    let mut stack = Vec::new();
    let steps: &[(i64, i64)] = if diagonal {
        &[
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ]
    } else {
        &[(1, 0), (-1, 0), (0, 1), (0, -1)]
    };
    for start in 0..class.len() {
        if class[start] != 1 || label[start] != NO_LABEL {
            continue;
        }
        let id = sizes.len() as u32;
        let mut size = 0_u64;
        label[start] = id;
        stack.push(start);
        while let Some(k) = stack.pop() {
            size += 1;
            let (x, y) = ((k % width) as i64, (k / width) as i64);
            for &(dx, dy) in steps {
                let ny = y + dy;
                if ny < 0 || ny >= height as i64 {
                    continue;
                }
                let nx = (x + dx).rem_euclid(width as i64);
                let q = ny as usize * width + nx as usize;
                if class[q] == 1 && label[q] == NO_LABEL {
                    label[q] = id;
                    stack.push(q);
                }
            }
        }
        sizes.push(size);
    }
    (label, sizes)
}

/// Whether Sekai's dry B < 0 points are enclosed on the upstream evaluator's own
/// canonical-scale pixel grid, or only disconnected on Sekai's support mesh.
/// Seeds follow A5b on that grid: the pixel of each upstream marine site.
#[test]
#[ignore = "reads a Sekai export; evaluates the full canonical pixel grid; run serialized Release"]
fn upstream_pixel_connectivity_at_dry_depressions() {
    let (weight_km2, deps, points) = read_export();
    let p = crate::TerrainParams {
        seed: 42,
        ..Default::default()
    };
    let terrain = Arc::new(crate::generate_terrain(&p, &|| false).expect("terrain"));
    let fine = FineTerrain::new(terrain, FineScale::Four, &|| false).expect("fine");
    let s = usize::from(fine.scale.pixels_per_unit());
    let (width, height) = (WIDTH * s, HEIGHT * s);
    let threads = std::env::var("CIV_ATLAS_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8_usize);
    let t = std::time::Instant::now();
    let mut class = vec![0_u8; width * height];
    let rows = height.div_ceil(threads);
    std::thread::scope(|scope| {
        for (band, chunk) in class.chunks_mut(rows * width).enumerate() {
            let fine = &fine;
            scope.spawn(move || {
                let mut scratch = fine.source.detail.workspace();
                for (i, c) in chunk.iter_mut().enumerate() {
                    let k = band * rows * width + i;
                    let xy = [
                        ((k % width) as f64 + 0.5) / s as f64,
                        ((k / width) as f64 + 0.5) / s as f64,
                    ];
                    *c = fine.raw(xy, &mut scratch).expect("pixel").water;
                }
            });
        }
    });
    eprintln!(
        "PIXELS {width}x{height} ocean={} ms={}",
        class.iter().filter(|&&c| c == 1).count(),
        t.elapsed().as_millis()
    );
    let pixel_of = |d: [f64; 3]| {
        let xy = coordinates(d).expect("direction");
        let px = ((xy[0] * s as f64).floor() as usize).min(width - 1);
        let py = ((xy[1] * s as f64).floor() as usize).min(height - 1);
        py * width + px
    };
    // One pixel of row y as a fraction of the sphere: dλ (sin φ_top − sin φ_bottom) / 4π.
    let row_fraction = |y: usize| {
        let lat = |v: f64| std::f64::consts::FRAC_PI_2 - v / height as f64 * std::f64::consts::PI;
        (lat(y as f64).sin() - lat(y as f64 + 1.).sin()) / (2. * width as f64)
    };
    let sites = fine.source.terrain.mesh.xyz.chunks_exact(3);
    let marine_pixels: Vec<usize> = sites
        .zip(&fine.source.terrain.water)
        .filter(|(_, &w)| w == 1)
        .map(|(p, _)| pixel_of([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]))
        .collect();
    for diagonal in [false, true] {
        let (label, sizes) = label_ocean(&class, width, height, diagonal);
        let main = (0..sizes.len())
            .max_by_key(|&i| sizes[i])
            .expect("an ocean component") as u32;
        let mut seeded = vec![false; sizes.len()];
        for &k in &marine_pixels {
            if label[k] != NO_LABEL {
                seeded[label[k] as usize] = true;
            }
        }
        let mut unseeded_fraction = 0.;
        for (k, &l) in label.iter().enumerate() {
            if l != NO_LABEL && !seeded[l as usize] {
                unseeded_fraction += row_fraction(k / width);
            }
        }
        let conn = if diagonal { "8" } else { "4" };
        eprintln!(
            "COMPONENTS conn={conn} count={} seeded={} unseeded_ocean_sphere_fraction={unseeded_fraction:.6e}",
            sizes.len(),
            seeded.iter().filter(|&&v| v).count()
        );
        // 0 main ocean, 1 other seeded sea, 2 unseeded pixel component, 3 not ocean pixel.
        let mut tally = [0_usize; 4];
        let mut per_dep = vec![[0_usize; 4]; deps.len()];
        for &(d, dep) in &points {
            let l = label[pixel_of(d)];
            let kind = if l == NO_LABEL {
                3
            } else if l == main {
                0
            } else if seeded[l as usize] {
                1
            } else {
                2
            };
            tally[kind] += 1;
            if dep >= 0 {
                per_dep[dep as usize][kind] += 1;
            }
        }
        let names = [
            "main_ocean",
            "other_seeded_sea",
            "unseeded_enclosed",
            "not_ocean_pixel",
        ];
        eprintln!(
            "PIXELCONN conn={conn} dry_neg_km2={:.0} {}",
            points.len() as f64 * weight_km2,
            (0..4)
                .map(|i| format!("{}={:.0}km2", names[i], tally[i] as f64 * weight_km2))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut order: Vec<usize> = (0..deps.len()).collect();
        order.sort_by(|a, b| deps[*b].area.total_cmp(&deps[*a].area));
        for &d in order.iter().take(15) {
            let c = per_dep[d];
            eprintln!(
                "PIXELCONN conn={conn} dep={d} area_km2={:.0} lat={:.2} lon={:.2} group={} {}",
                deps[d].area,
                deps[d].lat,
                deps[d].lon,
                GROUP[deps[d].group],
                (0..4)
                    .map(|i| format!("{}={}", names[i], c[i]))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
}
