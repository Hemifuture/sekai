//! Terrain-structure probes backing the 2026-08-19 terrain-algorithm audit.
//!
//! These are diagnostic writers, not gates. Run explicitly:
//! `cargo test --release --test terrain_audit_probe -- --ignored --nocapture`
//!
//! Outputs land-composition statistics to stdout and evidence renders to
//! `target/natural-quality/audit/`.

mod support;

use std::collections::VecDeque;
use std::f64::consts::PI;
use std::path::PathBuf;

use image::{Rgb, RgbImage};
use sekai::app::default_spherical_space_spec;
use sekai::engine::{BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    spherical_natural_foundation_graph, AuthorConstraintsArtifact, ClimateSpecArtifact,
    GeologicSpecArtifact, HydroErosionSpecArtifact, ReliefSpecArtifact, RulePackSetArtifact,
    SphericalReliefArtifact, SphericalTectonicArtifact, TectonicSpecArtifact,
    WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{SphericalSpaceArtifact, SphericalSurfaceArtifact};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    CrustKind, GeologicSpec, ReliefSpec, TectonicActivity, TectonicSpec, WorldFormationSpec,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{CellId, RootSeed};
use support::surface_formation::{published_formation, surface_formation_fixture};

/// The root seed of the world currently on the user's screen.
const APP_SEED: u64 = 15_957_999_680_335_491_072;
const IMG_W: u32 = 1200;
const IMG_H: u32 = 600;

fn audit_output_dir() -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("natural-quality")
        .join("audit");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        let grand = parent[parent[x as usize] as usize];
        parent[x as usize] = grand;
        x = grand;
    }
    x
}

fn percentile(sorted: &[f32], q: f64) -> f32 {
    if sorted.is_empty() {
        return f32::NAN;
    }
    let index = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[index]
}

fn pearson(a: &[f32], b: &[f32]) -> f64 {
    assert_eq!(a.len(), b.len());
    let n = a.len() as f64;
    let mean_a = a.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let mean_b = b.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for (&x, &y) in a.iter().zip(b) {
        let dx = f64::from(x) - mean_a;
        let dy = f64::from(y) - mean_b;
        cov += dx * dy;
        var_a += dx * dx;
        var_b += dy * dy;
    }
    cov / (var_a.sqrt() * var_b.sqrt())
}

fn std_dev(values: &[f32]) -> f64 {
    let n = values.len() as f64;
    let mean = values.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    (values
        .iter()
        .map(|&v| (f64::from(v) - mean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt()
}

fn range(values: &[f32]) -> (f32, f32) {
    values
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)))
}

/// Nearest-cell equirectangular raster via seeded BFS (same scheme as the P5 atlas writer).
fn equirect_raster(surface: &SphericalSurfaceSnapshot) -> Vec<usize> {
    let seeds = surface
        .cells()
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let [x, y, z] = cell.centroid.components();
            let longitude = y.atan2(x);
            let latitude = z.clamp(-1.0, 1.0).asin();
            let pixel_x = ((longitude / (2.0 * PI) + 0.5) * f64::from(IMG_W))
                .floor()
                .clamp(0.0, f64::from(IMG_W - 1)) as u32;
            let pixel_y = ((0.5 - latitude / PI) * f64::from(IMG_H))
                .floor()
                .clamp(0.0, f64::from(IMG_H - 1)) as u32;
            (pixel_x, pixel_y, index)
        })
        .collect::<Vec<_>>();

    let mut cells = vec![usize::MAX; (IMG_W * IMG_H) as usize];
    let mut distances = vec![u32::MAX; cells.len()];
    let mut queue = VecDeque::new();
    for &(x, y, cell) in &seeds {
        let pixel = y as usize * IMG_W as usize + x as usize;
        if distances[pixel] != 0 || cell < cells[pixel] {
            distances[pixel] = 0;
            cells[pixel] = cell;
            queue.push_back(pixel);
        }
    }
    while let Some(pixel) = queue.pop_front() {
        let x = (pixel % IMG_W as usize) as i32;
        let y = (pixel / IMG_W as usize) as i32;
        let distance = distances[pixel].saturating_add(1);
        let cell = cells[pixel];
        for [next_x, next_y] in [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]] {
            let wrapped_x = next_x.rem_euclid(IMG_W as i32);
            if next_y < 0 || next_y >= IMG_H as i32 {
                continue;
            }
            let next = next_y as usize * IMG_W as usize + wrapped_x as usize;
            if distance < distances[next] || (distance == distances[next] && cell < cells[next]) {
                distances[next] = distance;
                cells[next] = cell;
                queue.push_back(next);
            }
        }
    }
    cells
}

fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn hypsometric_color(relief_m: f32) -> [f32; 3] {
    if relief_m < 0.0 {
        let depth = -relief_m;
        if depth < 200.0 {
            lerp_rgb([168.0, 202.0, 218.0], [110.0, 160.0, 195.0], depth / 200.0)
        } else if depth < 2000.0 {
            lerp_rgb(
                [110.0, 160.0, 195.0],
                [38.0, 78.0, 130.0],
                (depth - 200.0) / 1800.0,
            )
        } else {
            lerp_rgb(
                [38.0, 78.0, 130.0],
                [8.0, 22.0, 55.0],
                ((depth - 2000.0) / 5000.0).min(1.0),
            )
        }
    } else if relief_m < 300.0 {
        lerp_rgb([96.0, 138.0, 76.0], [140.0, 162.0, 88.0], relief_m / 300.0)
    } else if relief_m < 1200.0 {
        lerp_rgb(
            [140.0, 162.0, 88.0],
            [176.0, 146.0, 92.0],
            (relief_m - 300.0) / 900.0,
        )
    } else if relief_m < 3000.0 {
        lerp_rgb(
            [176.0, 146.0, 92.0],
            [148.0, 120.0, 106.0],
            (relief_m - 1200.0) / 1800.0,
        )
    } else {
        lerp_rgb(
            [148.0, 120.0, 106.0],
            [245.0, 245.0, 245.0],
            ((relief_m - 3000.0) / 2500.0).min(1.0),
        )
    }
}

fn smooth_grid(grid: &mut [f32]) {
    let w = IMG_W as usize;
    let h = IMG_H as usize;
    let source = grid.to_vec();
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            let mut count = 0.0;
            for dy in -1i32..=1 {
                let ny = y as i32 + dy;
                if ny < 0 || ny >= h as i32 {
                    continue;
                }
                for dx in -1i32..=1 {
                    let nx = (x as i32 + dx).rem_euclid(w as i32);
                    sum += source[ny as usize * w + nx as usize];
                    count += 1.0;
                }
            }
            grid[y * w + x] = sum / count;
        }
    }
}

fn hillshade(grid: &[f32]) -> Vec<f32> {
    let w = IMG_W as usize;
    let h = IMG_H as usize;
    // ~33 km of ground per pixel at the equator; exaggerate so cell-scale slopes read.
    let k = 1.0 / 350.0;
    let light = {
        let v = [-0.55f32, -0.65, 0.75];
        let norm = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / norm, v[1] / norm, v[2] / norm]
    };
    let mut shade = vec![1.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let xm = (x + w - 1) % w;
            let xp = (x + 1) % w;
            let ym = y.saturating_sub(1);
            let yp = (y + 1).min(h - 1);
            let gx = (grid[y * w + xp] - grid[y * w + xm]) * 0.5 * k;
            let gy = (grid[yp * w + x] - grid[ym * w + x]) * 0.5 * k;
            let mut normal = [-gx, -gy, 1.0];
            let norm = (normal[0] * normal[0] + normal[1] * normal[1] + 1.0).sqrt();
            normal = [normal[0] / norm, normal[1] / norm, normal[2] / norm];
            let dot = (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]).max(0.0);
            shade[y * w + x] = 0.45 + 0.75 * dot;
        }
    }
    shade
}

fn coastline_mask(raster: &[usize], land: &[u32]) -> Vec<bool> {
    let w = IMG_W as usize;
    let h = IMG_H as usize;
    let mut coast = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let here = land[raster[y * w + x]];
            let xp = (x + 1) % w;
            let yp = (y + 1).min(h - 1);
            if land[raster[y * w + xp]] != here || land[raster[yp * w + x]] != here {
                coast[y * w + x] = true;
            }
        }
    }
    coast
}

fn render_hypsometric(
    path: &std::path::Path,
    raster: &[usize],
    elevation: &[f32],
    sea_level: f32,
    land: &[u32],
) {
    let w = IMG_W as usize;
    let h = IMG_H as usize;
    let mut grid: Vec<f32> = raster.iter().map(|&cell| elevation[cell]).collect();
    smooth_grid(&mut grid);
    smooth_grid(&mut grid);
    let shade = hillshade(&grid);
    let coast = coastline_mask(raster, land);
    let mut img = RgbImage::new(IMG_W, IMG_H);
    for y in 0..h {
        for x in 0..w {
            let pixel = y * w + x;
            let cell = raster[pixel];
            let relief = if land[cell] == 1 {
                (elevation[cell] - sea_level).max(0.0)
            } else {
                (elevation[cell] - sea_level).min(-1.0)
            };
            let base = hypsometric_color(relief);
            let s = if land[cell] == 1 {
                shade[pixel]
            } else {
                0.35 + 0.65 * shade[pixel]
            };
            let mut rgb = [base[0] * s, base[1] * s, base[2] * s];
            if coast[pixel] {
                rgb = [15.0, 15.0, 15.0];
            }
            img.put_pixel(
                x as u32,
                y as u32,
                Rgb([
                    rgb[0].clamp(0.0, 255.0) as u8,
                    rgb[1].clamp(0.0, 255.0) as u8,
                    rgb[2].clamp(0.0, 255.0) as u8,
                ]),
            );
        }
    }
    img.save(path).unwrap();
}

fn render_crust(
    path: &std::path::Path,
    raster: &[usize],
    kinds: &[CrustKind],
    ages: &[f32],
    land: &[u32],
) {
    let w = IMG_W as usize;
    let h = IMG_H as usize;
    let coast = coastline_mask(raster, land);
    let mut img = RgbImage::new(IMG_W, IMG_H);
    for y in 0..h {
        for x in 0..w {
            let pixel = y * w + x;
            let cell = raster[pixel];
            let mut rgb = match kinds[cell] {
                CrustKind::Continental => [178.0, 40.0, 34.0],
                CrustKind::Oceanic => {
                    let t = (ages[cell] / 140.0).clamp(0.0, 1.0);
                    lerp_rgb([250.0, 224.0, 120.0], [28.0, 48.0, 92.0], t)
                }
            };
            if land[cell] != 1 {
                rgb = [rgb[0] * 0.45, rgb[1] * 0.45, rgb[2] * 0.45];
            }
            if coast[pixel] {
                rgb = [255.0, 255.0, 255.0];
            }
            img.put_pixel(
                x as u32,
                y as u32,
                Rgb([rgb[0] as u8, rgb[1] as u8, rgb[2] as u8]),
            );
        }
    }
    img.save(path).unwrap();
}

/// Prints landmass-shape statistics shared by both pipeline probes.
fn landform_stats(
    label: &str,
    surface: &SphericalSurfaceSnapshot,
    elevation: &[f32],
    sea_level: f32,
    land: &[u32],
) {
    let n = elevation.len();
    let areas: Vec<f64> = surface.cells().iter().map(|c| c.area.get()).collect();
    let total_area: f64 = areas.iter().sum();
    let land_area: f64 = areas
        .iter()
        .zip(land)
        .filter_map(|(&a, &k)| (k == 1).then_some(a))
        .sum();
    let land_cells = land.iter().filter(|&&k| k == 1).count();

    let mut parent: Vec<u32> = (0..n as u32).collect();
    let mut coastal = vec![false; n];
    for edge in surface.edges() {
        let a = edge.cells[0].raw() as usize;
        let b = edge.cells[1].raw() as usize;
        match (land[a], land[b]) {
            (1, 1) => {
                let (ra, rb) = (find(&mut parent, a as u32), find(&mut parent, b as u32));
                if ra != rb {
                    parent[ra as usize] = rb;
                }
            }
            (1, 0) => coastal[a] = true,
            (0, 1) => coastal[b] = true,
            _ => {}
        }
    }
    let mut component_cells = std::collections::BTreeMap::<u32, (u64, f64)>::new();
    for index in 0..n {
        if land[index] == 1 {
            let root = find(&mut parent, index as u32);
            let entry = component_cells.entry(root).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += areas[index];
        }
    }
    let mut sizes: Vec<(u64, f64)> = component_cells.values().copied().collect();
    sizes.sort_by(|a, b| b.1.total_cmp(&a.1));
    let singletons = sizes.iter().filter(|(cells, _)| *cells == 1).count();
    let tiny = sizes.iter().filter(|(cells, _)| *cells <= 4).count();
    let coastal_land = (0..n).filter(|&i| land[i] == 1 && coastal[i]).count();

    let mut land_relief: Vec<f32> = (0..n)
        .filter(|&i| land[i] == 1)
        .map(|i| elevation[i] - sea_level)
        .collect();
    land_relief.sort_by(f32::total_cmp);
    let mut ocean_depth: Vec<f32> = (0..n)
        .filter(|&i| land[i] == 0)
        .map(|i| sea_level - elevation[i])
        .collect();
    ocean_depth.sort_by(f32::total_cmp);

    println!("== landform [{label}] ==");
    println!("cells={n} land_fraction={:.4}", land_area / total_area);
    println!("sea_level_m={sea_level:.1}");
    println!(
        "land_components={} largest_component_land_share={:.4} second_share={:.4} singleton_islands={} tiny_components_le4={}",
        sizes.len(),
        sizes.first().map_or(0.0, |s| s.1 / land_area),
        sizes.get(1).map_or(0.0, |s| s.1 / land_area),
        singletons,
        tiny,
    );
    println!(
        "coastal_land_cell_fraction={:.4} ({} of {})",
        coastal_land as f64 / land_cells.max(1) as f64,
        coastal_land,
        land_cells,
    );
    println!(
        "land_relief_above_sea_m p05={:.1} p25={:.1} p50={:.1} p75={:.1} p95={:.1} max={:.1}",
        percentile(&land_relief, 0.05),
        percentile(&land_relief, 0.25),
        percentile(&land_relief, 0.50),
        percentile(&land_relief, 0.75),
        percentile(&land_relief, 0.95),
        land_relief.last().copied().unwrap_or(f32::NAN),
    );
    println!(
        "ocean_depth_below_sea_m p05={:.1} p50={:.1} p95={:.1} max={:.1}",
        percentile(&ocean_depth, 0.05),
        percentile(&ocean_depth, 0.50),
        percentile(&ocean_depth, 0.95),
        ocean_depth.last().copied().unwrap_or(f32::NAN),
    );
}

fn crust_composition(
    label: &str,
    surface: &SphericalSurfaceSnapshot,
    kinds: &[CrustKind],
    ages: &[f32],
    land: &[u32],
) {
    let n = kinds.len();
    let areas: Vec<f64> = surface.cells().iter().map(|c| c.area.get()).collect();
    let total_area: f64 = areas.iter().sum();
    let continental_area: f64 = (0..n)
        .filter(|&i| kinds[i] == CrustKind::Continental)
        .map(|i| areas[i])
        .sum();
    let land_area: f64 = (0..n).filter(|&i| land[i] == 1).map(|i| areas[i]).sum();
    let land_on_oceanic: f64 = (0..n)
        .filter(|&i| land[i] == 1 && kinds[i] == CrustKind::Oceanic)
        .map(|i| areas[i])
        .sum();
    let submerged_continental: f64 = (0..n)
        .filter(|&i| land[i] == 0 && kinds[i] == CrustKind::Continental)
        .map(|i| areas[i])
        .sum();
    let mut oceanic_land_ages: Vec<f32> = (0..n)
        .filter(|&i| land[i] == 1 && kinds[i] == CrustKind::Oceanic)
        .map(|i| ages[i])
        .collect();
    oceanic_land_ages.sort_by(f32::total_cmp);
    let mut oceanic_ocean_ages: Vec<f32> = (0..n)
        .filter(|&i| land[i] == 0 && kinds[i] == CrustKind::Oceanic)
        .map(|i| ages[i])
        .collect();
    oceanic_ocean_ages.sort_by(f32::total_cmp);

    println!("== crust composition [{label}] ==");
    println!(
        "continental_crust_area_fraction={:.4}",
        continental_area / total_area,
    );
    println!(
        "land_area_fraction={:.4} | share_of_land_on_OCEANIC_crust={:.4} | share_of_continental_crust_SUBMERGED={:.4}",
        land_area / total_area,
        land_on_oceanic / land_area.max(f64::EPSILON),
        submerged_continental / continental_area.max(f64::EPSILON),
    );
    println!(
        "oceanic-LAND crust age myr: p05={:.1} p50={:.1} p95={:.1} | oceanic-OCEAN age p50={:.1}",
        percentile(&oceanic_land_ages, 0.05),
        percentile(&oceanic_land_ages, 0.50),
        percentile(&oceanic_land_ages, 0.95),
        percentile(&oceanic_ocean_ages, 0.50),
    );
}

#[test]
#[ignore = "audit probe writer; run explicitly with --ignored --nocapture in release"]
fn probe_foundation_terrain_structure() {
    let space = default_spherical_space_spec();
    let tectonic_spec = TectonicSpec {
        plate_count: 22,
        continental_crust_fraction: 0.30,
        activity: TectonicActivity::Active,
        ..Default::default()
    };
    let relief_spec = ReliefSpec {
        target_land_fraction: 0.38,
        ..Default::default()
    };

    let mut external = ExternalArtifacts::new();
    external.insert(SphericalSpaceArtifact::new(space)).unwrap();
    external
        .insert(TectonicSpecArtifact::new(tectonic_spec))
        .unwrap();
    external
        .insert(ReliefSpecArtifact::new(relief_spec))
        .unwrap();
    external
        .insert(GeologicSpecArtifact::new(GeologicSpec::default()))
        .unwrap();
    external
        .insert(ClimateSpecArtifact::new(Default::default()))
        .unwrap();
    external
        .insert(HydroErosionSpecArtifact::new(Default::default()))
        .unwrap();
    external
        .insert(WorldFormationSpecArtifact::new(
            WorldFormationSpec::default(),
        ))
        .unwrap();
    external
        .insert(RulePackSetArtifact::new(default_rule_pack_set().unwrap()))
        .unwrap();
    external
        .insert(AuthorConstraintsArtifact::new(AuthorConstraints::default()))
        .unwrap();

    let outcome = BuildEngine::new(spherical_natural_foundation_graph().unwrap())
        .build(
            RootSeed::new(APP_SEED),
            external,
            &mut MemoryStageCache::new(),
        )
        .unwrap();
    let surface = outcome.artifacts.get::<SphericalSurfaceArtifact>().unwrap();
    let tectonic = outcome
        .artifacts
        .get::<SphericalTectonicArtifact>()
        .unwrap();
    let relief = outcome.artifacts.get::<SphericalReliefArtifact>().unwrap();
    let surface = surface.snapshot();
    let tectonic = tectonic.snapshot();
    let relief = relief.snapshot();

    let n = surface.cells().len();
    let kinds: Vec<CrustKind> = (0..n)
        .map(|i| tectonic.crust_kind(CellId::from_raw(i as u32)).unwrap())
        .collect();
    let ages = tectonic.crust_age_myr();
    let elevation = relief.elevation_m().values();
    let sea = relief.sea_level_m();
    let land = relief.land_ocean().raw_values();

    println!("== foundation probe seed={APP_SEED} plates=22 continental=0.30 activity=Active target_land=0.38 ==");
    crust_composition("foundation", surface, &kinds, ages, land);

    let crust_base = relief.crust_base_elevation_m().values();
    let tect_off = relief.tectonic_offset_m().values();
    let volc = relief.volcanic_offset_m().values();
    let regional = relief.regional_offset_m().values();
    for (name, component) in [
        ("crust_base", crust_base),
        ("tectonic_offset", tect_off),
        ("volcanic_offset", volc),
        ("regional_offset", regional),
        ("final_elevation", elevation),
    ] {
        let (lo, hi) = range(component);
        println!(
            "component {name}: std={:.1} min={lo:.1} max={hi:.1}",
            std_dev(component)
        );
    }
    println!(
        "corr(final, crust_base)={:.4} corr(final, crust_base+tect)={:.4}",
        pearson(elevation, crust_base),
        pearson(
            elevation,
            &crust_base
                .iter()
                .zip(tect_off)
                .map(|(&a, &b)| a + b)
                .collect::<Vec<f32>>(),
        ),
    );

    landform_stats("foundation/spherical-relief", surface, elevation, sea, land);

    let raster = equirect_raster(surface);
    let dir = audit_output_dir();
    render_hypsometric(
        &dir.join("foundation-elevation.png"),
        &raster,
        elevation,
        sea,
        land,
    );
    render_crust(
        &dir.join("foundation-crust.png"),
        &raster,
        &kinds,
        ages,
        land,
    );
    println!("wrote {}", dir.display());
}

#[test]
#[ignore = "audit probe writer; run explicitly with --ignored --nocapture in release"]
fn probe_p5_terrain_structure() {
    let fixture = surface_formation_fixture();
    let formation = published_formation();
    let inputs = fixture.inputs();
    let surface = inputs.surface;
    let terrain = formation.terrain_fields();
    let final_elevation = terrain.final_elevation_m();
    let primary = terrain.elevation_components().primary_elevation_m();
    let sea = terrain.sea_level_m();
    let land = terrain.land_ocean().raw_values();

    println!("== p5 probe (draft fixture) ==");
    println!(
        "corr(final, primary)={:.6}",
        pearson(final_elevation, primary)
    );
    let mut deltas: Vec<f32> = final_elevation
        .iter()
        .zip(primary)
        .map(|(&f, &p)| (f - p).abs())
        .collect();
    deltas.sort_by(f32::total_cmp);
    println!(
        "abs(final-primary)_m p50={:.2} p95={:.2} p99={:.2} max={:.2} | share>1m={:.4} share>10m={:.4}",
        percentile(&deltas, 0.50),
        percentile(&deltas, 0.95),
        percentile(&deltas, 0.99),
        deltas.last().copied().unwrap_or(f32::NAN),
        deltas.iter().filter(|&&d| d > 1.0).count() as f64 / deltas.len() as f64,
        deltas.iter().filter(|&&d| d > 10.0).count() as f64 / deltas.len() as f64,
    );

    let compatibility = inputs.tectonics.compatibility();
    let n = surface.cells().len();
    let kinds: Vec<CrustKind> = (0..n)
        .map(|i| {
            compatibility
                .crust_kind(CellId::from_raw(i as u32))
                .unwrap()
        })
        .collect();
    let ages = compatibility.crust_age_myr();
    crust_composition("p5/evolved", surface, &kinds, ages, land);

    let budget = inputs.tectonics.material_budget();
    let total_area: f64 = surface.total_cell_area().get();
    println!(
        "evolved material budget: initial_continental_fraction={:.4} final_continental_fraction={:.4}",
        budget.initial_control().continental().reference_area_m2() / total_area,
        budget.final_authoritative().continental().reference_area_m2() / total_area,
    );
    println!(
        "processes: continental_consumed_area_fraction={:.4} rift_extension_gain_fraction={:.4} oceanic_subducted_fraction={:.4}",
        budget.processes().continental_consumed().reference_area_m2() / total_area,
        budget.processes().rift_extension_continental_area_gain_m2() / total_area,
        budget.processes().oceanic_subducted().reference_area_m2() / total_area,
    );

    landform_stats("p5/final", surface, final_elevation, sea, land);

    let raster = equirect_raster(surface);
    let dir = audit_output_dir();
    render_hypsometric(
        &dir.join("p5-elevation.png"),
        &raster,
        final_elevation,
        sea,
        land,
    );
    render_crust(&dir.join("p5-crust.png"), &raster, &kinds, ages, land);
    println!("wrote {}", dir.display());
}
