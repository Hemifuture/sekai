//! Terrain-structure probes backing the 2026-08-19 terrain-algorithm audit.
//!
//! These are diagnostic writers, not gates. Run explicitly:
//! `cargo test --release --test terrain_audit_probe -- --ignored --nocapture`
//!
//! Outputs land-composition statistics to stdout and evidence renders to
//! `target/natural-quality/audit/`.

#[allow(dead_code)]
#[path = "support/natural_quality.rs"]
mod natural_quality;
mod support;

use std::collections::VecDeque;
use std::f64::consts::PI;
use std::path::PathBuf;

use image::{Rgb, RgbImage};
use natural_quality::QUALITY_SEEDS;
use sekai::app::default_spherical_space_spec;
use sekai::engine::{BuildCancellation, BuildEngine, ExternalArtifacts, MemoryStageCache};
use sekai::generators::natural::{
    continental_airy_elevation_m, dynamic_tectonic_response_m, gdh1_ocean_depth_m,
    solve_physical_sea_level, spherical_natural_foundation_graph, water_volume_at_sea_level_m3,
    AuthorConstraintsArtifact, ClimateSpecArtifact, GeologicSpecArtifact, HydroErosionSpecArtifact,
    ReliefSpecArtifact, RulePackSetArtifact, SphericalReliefArtifact, SphericalTectonicArtifact,
    TectonicSpecArtifact, WorldFormationSpecArtifact,
};
use sekai::generators::spatial::{
    ProfileSurfaceBuilder, SphericalSpaceArtifact, SphericalSurfaceArtifact,
};
use sekai::rules::{default_rule_pack_set, AuthorConstraints};
use sekai::world::natural::{
    hypsometric_mean, hypsometric_quantile, hypsometric_share_below, hypsometric_total_area,
    scaled_earth_ocean_inventory_m3, sort_hypsometric_samples, CrustKind, GeologicSpec,
    LandOceanKind, NaturalQualityProfile, ReliefSpec, ResolvedWorldFormationPreset,
    SurfaceWaterKind, TectonicActivity, TectonicSpec, WorldFormationSpec,
    CONTINENTAL_CRUST_DENSITY_KG_M3, EARTH_OCEAN_VOLUME_M3, EARTH_WATER_REFERENCE_RADIUS_M,
};
use sekai::world::spatial::SphericalSurfaceSnapshot;
use sekai::world::{CellId, Meters, RootSeed};
use support::global_circulation::{build_primary_relief, build_primary_relief_for};
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
    let current_elevation = terrain.current_elevation_m();
    let primary = terrain.elevation_components().primary_relief_m();
    let sea = terrain.sea_level_m();
    let land = terrain.land_ocean().raw_values();

    println!("== p5 probe (draft fixture) ==");
    println!(
        "corr(current, primary)={:.6}",
        pearson(current_elevation, primary)
    );
    let mut deltas: Vec<f32> = current_elevation
        .iter()
        .zip(primary)
        .map(|(&f, &p)| (f - p).abs())
        .collect();
    deltas.sort_by(f32::total_cmp);
    println!(
        "abs(current-primary)_m p50={:.2} p95={:.2} p99={:.2} max={:.2} | share>1m={:.4} share>10m={:.4}",
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

    landform_stats("p5/current", surface, current_elevation, sea, land);

    let raster = equirect_raster(surface);
    let dir = audit_output_dir();
    render_hypsometric(
        &dir.join("p5-elevation.png"),
        &raster,
        current_elevation,
        sea,
        land,
    );
    render_crust(&dir.join("p5-crust.png"), &raster, &kinds, ages, land);
    println!("wrote {}", dir.display());
}

// ---------------------------------------------------------------------------
// T0 hypsometric attribution (2026-08-21 calibration plan, Task 1).
//
// Every number below is measured on the same Draft fixture (seed 42) that the
// P5 product suites use, with production operators only: the exact bath-tub
// sea-level solve, the P3 Airy column recipe, the Parsons-Sclater depth law,
// and the P5 implicit stream-power kernel.
// ---------------------------------------------------------------------------

const HYPSO_QUANTILES: [f64; 5] = [0.05, 0.25, 0.50, 0.75, 0.95];
/// Lowland ceilings above sea level reported as land-area shares.
const LOWLAND_CEILINGS_M: [f32; 3] = [100.0, 200.0, 500.0];
/// Shallow-water ceilings below sea level reported as ocean-area shares.
const SHALLOW_CEILINGS_M: [f32; 3] = [200.0, 1_000.0, 3_000.0];
/// Earth ocean share of the surface: 361.84e6 of 510.07e6 km2 (Eakins & Sharman 2010, ETOPO1).
const EARTH_OCEAN_AREA_FRACTION: f64 = 0.7094;
/// Continental-thickness histogram edges for the CRUST1.0 shape comparison, in km.
const THICKNESS_BIN_EDGES_KM: [f32; 11] = [
    20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 44.0, 48.0, 52.0, 56.0, 60.0,
];
/// Reference ages at which the locked ocean depth law is tabulated, in Myr.
const OCEAN_AGE_TABLE_MYR: [f32; 4] = [20.0, 60.0, 100.0, 150.0];
/// Uniform continental thickening trials for the freeboard-closure what-if, in km.
const CLOSURE_THICKENING_TRIALS_KM: [f32; 4] = [0.0, 2.0, 4.0, 6.0];
const CLOSURE_TOLERANCE_M: f32 = 0.01;
const CLOSURE_MAX_ITERATIONS: usize = 64;

/// Weighted samples of one field over a cell subset, sorted by value; every
/// statistic is the production hypsometry helper the P5 gate uses.
struct Weighted(Vec<(f32, f64)>);

impl Weighted {
    fn collect(values: &[f32], weights: &[f64], include: impl Fn(usize) -> bool) -> Self {
        let mut samples: Vec<(f32, f64)> = values
            .iter()
            .zip(weights)
            .enumerate()
            .filter(|&(index, _)| include(index))
            .map(|(_, (&value, &weight))| (value, weight))
            .collect();
        sort_hypsometric_samples(&mut samples);
        Self(samples)
    }

    fn total(&self) -> f64 {
        hypsometric_total_area(&self.0)
    }

    fn mean(&self) -> f64 {
        hypsometric_mean(&self.0)
    }

    fn std_dev(&self) -> f64 {
        let mean = self.mean();
        (self
            .0
            .iter()
            .map(|&(value, weight)| (f64::from(value) - mean).powi(2) * weight)
            .sum::<f64>()
            / self.total())
        .sqrt()
    }

    fn quantile(&self, q: f64) -> f32 {
        hypsometric_quantile(&self.0, q)
    }

    fn quantiles(&self) -> [f32; 5] {
        HYPSO_QUANTILES.map(|q| self.quantile(q))
    }

    fn share_below(&self, ceiling: f32) -> f64 {
        hypsometric_share_below(&self.0, ceiling)
    }

    fn share_between(&self, low: f32, high: f32) -> f64 {
        self.0
            .iter()
            .filter(|sample| sample.0 >= low && sample.0 < high)
            .map(|sample| sample.1)
            .sum::<f64>()
            / self.total()
    }
}

fn print_quantiles(label: &str, samples: &Weighted) {
    let [p05, p25, p50, p75, p95] = samples.quantiles();
    println!(
        "{label}: p05={p05:.1} p25={p25:.1} p50={p50:.1} p75={p75:.1} p95={p95:.1} mean={:.1} sd={:.1}",
        samples.mean(),
        samples.std_dev(),
    );
}

fn share_line(samples: &Weighted, ceilings: &[f32]) -> String {
    ceilings
        .iter()
        .map(|&ceiling| format!("<{ceiling:.0}m={:.4}", samples.share_below(ceiling)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sea-level-relative land/ocean hypsometry plus the bath-tub identity
/// `sea = mean wet floor + inventory / wet area`; returns the land quantiles.
fn hypsometry(
    label: &str,
    areas: &[f64],
    elevation: &[f32],
    sea: f32,
    land: &[u32],
    inventory_m3: f64,
) -> [f32; 5] {
    let total_area: f64 = areas.iter().sum();
    let relief: Vec<f32> = elevation.iter().map(|&e| e - sea).collect();
    let depth: Vec<f32> = relief.iter().map(|&r| -r).collect();
    let land_relief = Weighted::collect(&relief, areas, |i| land[i] == 1);
    let ocean_depth = Weighted::collect(&depth, areas, |i| land[i] == 0);
    let wet_floor = Weighted::collect(elevation, areas, |i| land[i] == 0);
    let wet_area = wet_floor.total();
    println!(
        "== hypsometry [{label}] == sea_level_m={sea:.1} land_area_fraction={:.4}",
        land_relief.total() / total_area
    );
    print_quantiles("  land_relief_above_sea_m", &land_relief);
    println!(
        "  land_area_share_below: {}",
        share_line(&land_relief, &LOWLAND_CEILINGS_M)
    );
    print_quantiles("  ocean_depth_below_sea_m", &ocean_depth);
    println!(
        "  ocean_area_share_shallower_than: {}",
        share_line(&ocean_depth, &SHALLOW_CEILINGS_M)
    );
    println!(
        "  bathtub: wet_area_fraction={:.4} required_mean_depth_m={:.1} mean_wet_floor_m={:.1} identity_sea_m={:.1}",
        wet_area / total_area,
        inventory_m3 / wet_area,
        wet_floor.mean(),
        wet_floor.mean() + inventory_m3 / wet_area,
    );
    land_relief.quantiles()
}

/// Iterates the L1 freeboard closure: continental columns are re-referenced
/// to the solved sea level (plus an optional uniform thickening lift) until
/// the bath-tub solve reproduces its own datum.
fn freeboard_closure(
    label: &str,
    surface: &SphericalSurfaceSnapshot,
    elevation: &[f32],
    continental_weight: &[f32],
    inventory_m3: f64,
    lift_m: f32,
    initial_sea: f32,
) -> (f32, [f32; 5], Vec<u32>) {
    let mut sea = initial_sea;
    let mut shifted = vec![0.0_f32; elevation.len()];
    let mut iterations = 0;
    loop {
        iterations += 1;
        for ((slot, &base), &weight) in shifted.iter_mut().zip(elevation).zip(continental_weight) {
            *slot = base + (sea + lift_m) * weight;
        }
        let next = solve_physical_sea_level(surface, &shifted, inventory_m3)
            .unwrap()
            .sea_level_m();
        let converged = (next - sea).abs() <= CLOSURE_TOLERANCE_M;
        sea = next;
        if converged || iterations >= CLOSURE_MAX_ITERATIONS {
            break;
        }
    }
    let water = solve_physical_sea_level(surface, &shifted, inventory_m3).unwrap();
    let land = water.geometry().land_ocean();
    let areas = surface
        .cells()
        .iter()
        .map(|cell| cell.area.get())
        .collect::<Vec<_>>();
    println!(
        "-- freeboard closure [{label}] lift_m={lift_m:.1} iterations={iterations} datum_shift_m={sea:.1}"
    );
    let quantiles = hypsometry(
        label,
        &areas,
        &shifted,
        sea,
        land.raw_values(),
        inventory_m3,
    );
    (sea, quantiles, land.raw_values().to_vec())
}

#[test]
#[ignore = "audit probe writer; run explicitly with --ignored --nocapture in release"]
fn probe_t0_hypsometric_attribution() {
    let fixture = surface_formation_fixture();
    let formation = published_formation();
    let upstream = fixture.upstream;
    let surface = upstream.bundle.authoritative_surface();
    let relief = &upstream.relief;
    let evolved = &upstream.evolved;
    let substrate = &upstream.substrate;
    let terrain = formation.terrain_fields();
    let n = surface.cells().len();
    let areas: Vec<f64> = surface.cells().iter().map(|c| c.area.get()).collect();
    let total_area: f64 = areas.iter().sum();
    let inventory = relief.water_inventory_m3();
    let land_p3 = relief.land_ocean().raw_values();
    let land_p5 = terrain.land_ocean().raw_values();
    let kinds: Vec<CrustKind> = (0..n).map(|i| substrate.crust_kind(i).unwrap()).collect();
    let material = evolved.material();
    let continental_area = material.continental_reference_area_m2();
    let continental_weight: Vec<f32> = (0..n)
        .map(|i| {
            (continental_area[i] / (continental_area[i] + material.oceanic_reference_area_m2()[i]))
                as f32
        })
        .collect();
    println!("== t0 hypsometric attribution (draft fixture, seed 42, {n} cells) ==");
    println!(
        "cell area spread: max/mean={:.3} (every statistic below is area-weighted)",
        areas.iter().copied().fold(0.0_f64, f64::max) / (total_area / n as f64)
    );

    // (1) P3 primary relief on its own bath-tub sea level, and its components.
    println!("\n#### (1) P3 primary relief hypsometry and column components");
    let q_p3 = hypsometry(
        "p3/primary",
        &areas,
        relief.elevation_m(),
        relief.sea_level_m(),
        land_p3,
        inventory,
    );
    let components: [(&str, &[f32]); 6] = [
        ("isostatic_base", relief.isostatic_base_m()),
        ("dynamic_tectonic", relief.dynamic_tectonic_offset_m()),
        ("volcanic", relief.volcanic_construction_m()),
        ("passive_margin", relief.passive_margin_offset_m()),
        ("regional_detail", relief.conditioned_regional_detail_m()),
        ("elevation", relief.elevation_m()),
    ];
    let subsets: [(&str, Vec<bool>); 4] = [
        ("land", land_p3.iter().map(|&k| k == 1).collect()),
        ("wet", land_p3.iter().map(|&k| k == 0).collect()),
        (
            "continental-crust",
            kinds.iter().map(|&k| k == CrustKind::Continental).collect(),
        ),
        (
            "oceanic-crust",
            kinds.iter().map(|&k| k == CrustKind::Oceanic).collect(),
        ),
    ];
    for (subset, mask) in &subsets {
        for (name, values) in &components {
            print_quantiles(
                &format!("p3 {name} over {subset} (reference frame, m)"),
                &Weighted::collect(values, &areas, |i| mask[i]),
            );
        }
    }

    // (2) V5 continental crust thickness CDF as consumed by the P3 Airy column.
    println!("\n#### (2) V5 continental crust thickness distribution");
    let thickness: Vec<f32> = (0..n)
        .map(|i| material.compatibility_thickness_km(i).unwrap_or(f32::NAN))
        .collect();
    let max_substrate_mismatch = thickness
        .iter()
        .zip(substrate.crust_thickness_km())
        .map(|(&a, &b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    let is_continental = |i: usize| material.compatibility_kind(i) == Some(CrustKind::Continental);
    let continental_thickness = Weighted::collect(&thickness, continental_area, is_continental);
    println!(
        "continental reference area fraction={:.4} (substrate crust_thickness max |delta|={max_substrate_mismatch:.3} km)",
        continental_thickness.total() / total_area
    );
    print_quantiles(
        "continental thickness km (all continental-dominant cells)",
        &continental_thickness,
    );
    print_quantiles(
        "continental thickness km (P3 land cells)",
        &Weighted::collect(&thickness, continental_area, |i| {
            is_continental(i) && land_p3[i] == 1
        }),
    );
    print_quantiles(
        "continental thickness km (P3 submerged cells)",
        &Weighted::collect(&thickness, continental_area, |i| {
            is_continental(i) && land_p3[i] == 0
        }),
    );
    let histogram = THICKNESS_BIN_EDGES_KM
        .windows(2)
        .map(|edge| {
            format!(
                "[{:.0},{:.0})={:.3}",
                edge[0],
                edge[1],
                continental_thickness.share_between(edge[0], edge[1])
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    println!(
        "continental thickness area histogram: <20={:.3} {histogram} >=60={:.3}",
        continental_thickness.share_below(THICKNESS_BIN_EDGES_KM[0]),
        1.0 - continental_thickness
            .share_below(THICKNESS_BIN_EDGES_KM[THICKNESS_BIN_EDGES_KM.len() - 1]),
    );
    let density = CONTINENTAL_CRUST_DENSITY_KG_M3;
    let airy_reference = continental_airy_elevation_m(35.0, density);
    let airy_slope_m_per_km = continental_airy_elevation_m(36.0, density) - airy_reference;
    let emergence_at_reference_zero_km = 35.0 + (0.0 - airy_reference) / airy_slope_m_per_km;
    let emergence_at_p3_sea_km =
        35.0 + (relief.sea_level_m() - airy_reference) / airy_slope_m_per_km;
    println!(
        "airy: reference_column(35 km, {density:.0} kg/m3)={airy_reference:.1} m slope={airy_slope_m_per_km:.1} m/km | pure-Airy emergence threshold: at reference zero={emergence_at_reference_zero_km:.2} km (area share above={:.4}), at P3 sea={emergence_at_p3_sea_km:.2} km (area share above={:.4})",
        1.0 - continental_thickness.share_below(emergence_at_reference_zero_km),
        1.0 - continental_thickness.share_below(emergence_at_p3_sea_km),
    );
    println!(
        "airy-implied land spread from thickness alone: sd={:.0} m, p95-p05={:.0} m",
        continental_thickness.std_dev() * f64::from(airy_slope_m_per_km),
        f64::from(continental_thickness.quantile(0.95) - continental_thickness.quantile(0.05))
            * f64::from(airy_slope_m_per_km),
    );

    // (3) Current P3 relief -> current P5 equilibrium adjustment and process rates.
    println!("\n#### (3) P3 relief -> P5 current-state adjustment and process rates");
    let q_p5 = hypsometry(
        "p5/current",
        &areas,
        terrain.current_elevation_m(),
        terrain.sea_level_m(),
        land_p5,
        inventory,
    );
    println!(
        "p3 relief -> p5 current land quantile adjustment m: p05={:+.1} p25={:+.1} p50={:+.1} p75={:+.1} p95={:+.1} | sea_level delta={:+.2} m | land/ocean class changed area fraction={:.5}",
        q_p5[0] - q_p3[0],
        q_p5[1] - q_p3[1],
        q_p5[2] - q_p3[2],
        q_p5[3] - q_p3[3],
        q_p5[4] - q_p3[4],
        terrain.sea_level_m() - relief.sea_level_m(),
        (0..n)
            .filter(|&i| land_p3[i] != land_p5[i])
            .map(|i| areas[i])
            .sum::<f64>()
            / total_area,
    );
    let elevation_components = terrain.elevation_components();
    print_quantiles(
        "p5 equilibrium adjustment over land (m)",
        &Weighted::collect(
            elevation_components.equilibrium_adjustment_m(),
            &areas,
            |i| land_p5[i] == 1,
        ),
    );
    let process_rates = formation.process_rates();
    let p5_rates: [(&str, &[f32]); 8] = [
        (
            "tectonic_displacement_rate",
            process_rates.tectonic_displacement_rate_m_per_year(),
        ),
        (
            "fluvial_erosion_rate",
            process_rates.fluvial_erosion_rate_m_per_year(),
        ),
        (
            "hillslope_erosion_rate",
            process_rates.hillslope_erosion_rate_m_per_year(),
        ),
        (
            "hillslope_deposition_rate",
            process_rates.hillslope_deposition_rate_m_per_year(),
        ),
        (
            "routed_sediment_deposition_rate",
            process_rates.routed_sediment_deposition_rate_m_per_year(),
        ),
        (
            "coastal_erosion_rate",
            process_rates.coastal_erosion_rate_m_per_year(),
        ),
        (
            "coastal_deposition_rate",
            process_rates.coastal_deposition_rate_m_per_year(),
        ),
        (
            "isostatic_response_rate",
            process_rates.isostatic_response_rate_m_per_year(),
        ),
    ];
    for (name, values) in &p5_rates {
        print_quantiles(
            &format!("p5 current {name} over land (m/year)"),
            &Weighted::collect(values, &areas, |i| land_p5[i] == 1),
        );
    }
    let forcing = evolved.forcing();
    let net_rate: Vec<f32> = forcing
        .uplift_rate_mm_per_year()
        .iter()
        .zip(forcing.subsidence_rate_mm_per_year())
        .map(|(&u, &s)| u - s)
        .collect();
    let land_rate = Weighted::collect(&net_rate, &areas, |i| land_p5[i] == 1);
    print_quantiles("v5 net uplift rate over land (mm/yr)", &land_rate);
    println!(
        "land area share: net uplift >0.1 mm/yr={:.4} >1 mm/yr={:.4} | net subsidence <-0.1 mm/yr={:.4}",
        1.0 - land_rate.share_below(0.1),
        1.0 - land_rate.share_below(1.0),
        land_rate.share_below(-0.1),
    );
    let hydrology = formation.hydrology();
    let land_area: f64 = (0..n).filter(|&i| land_p5[i] == 1).map(|i| areas[i]).sum();
    let fluvially_active = (0..n)
        .filter(|&i| {
            land_p5[i] == 1
                && hydrology.surface_water().get(i) == Some(SurfaceWaterKind::DryLand)
                && hydrology.flow_receiver()[i].is_some()
        })
        .map(|i| areas[i])
        .sum::<f64>()
        / land_area;
    println!(
        "land area share eligible for stream-power incision (dry land with a receiver)={fluvially_active:.4}"
    );
    // (4) Ocean basin and water inventory accounting.
    println!("\n#### (4) Ocean basin / water inventory accounting");
    let earth_area = 4.0 * PI * EARTH_WATER_REFERENCE_RADIUS_M * EARTH_WATER_REFERENCE_RADIUS_M;
    let earth_mean_depth = EARTH_OCEAN_VOLUME_M3 / (EARTH_OCEAN_AREA_FRACTION * earth_area);
    let wet_floor = Weighted::collect(relief.elevation_m(), &areas, |i| land_p3[i] == 0);
    let required_depth = inventory / wet_floor.total();
    println!(
        "earth reference: ocean_area_fraction={EARTH_OCEAN_AREA_FRACTION:.4} mean_depth_m={earth_mean_depth:.0} (inventory {inventory:.4e} m3 on {total_area:.4e} m2)"
    );
    println!(
        "p3 sea level decomposition: floor_term=(mean_wet_floor + earth_mean_depth)={:+.1} m, area_term=(required_mean_depth - earth_mean_depth)={:+.1} m, sum={:+.1} m vs solved {:+.1} m",
        wet_floor.mean() + earth_mean_depth,
        required_depth - earth_mean_depth,
        wet_floor.mean() + required_depth,
        relief.sea_level_m(),
    );
    let wet_continental = Weighted::collect(relief.elevation_m(), &areas, |i| {
        land_p3[i] == 0 && kinds[i] == CrustKind::Continental
    });
    let wet_oceanic = Weighted::collect(relief.elevation_m(), &areas, |i| {
        land_p3[i] == 0 && kinds[i] == CrustKind::Oceanic
    });
    println!(
        "wet area split: continental-kind share={:.4} mean_elevation={:.1} m | oceanic-kind share={:.4} mean_elevation={:.1} m",
        wet_continental.total() / wet_floor.total(),
        wet_continental.mean(),
        wet_oceanic.total() / wet_floor.total(),
        wet_oceanic.mean(),
    );
    let ocean_age = Weighted::collect(substrate.ocean_age_myr(), &areas, |i| {
        kinds[i] == CrustKind::Oceanic
    });
    print_quantiles("oceanic crust age Myr (oceanic-kind cells)", &ocean_age);
    println!(
        "gdh1_depth_m: at area-weighted mean age {:.1} Myr={:.0} | {}",
        ocean_age.mean(),
        gdh1_ocean_depth_m(ocean_age.mean() as f32),
        OCEAN_AGE_TABLE_MYR
            .iter()
            .map(|&age| format!("{age:.0} Myr={:.0}", gdh1_ocean_depth_m(age)))
            .collect::<Vec<_>>()
            .join(" "),
    );
    print_quantiles(
        "oceanic crust thickness km (oceanic-dominant cells)",
        &Weighted::collect(&thickness, material.oceanic_reference_area_m2(), |i| {
            material.compatibility_kind(i) == Some(CrustKind::Oceanic)
        }),
    );

    // L0 what-if: drop the inherited V5 compatibility elevation from the P3
    // dynamic term and keep only the rate response (the production recipe with
    // a zero accumulated response), first on oceanic crust, then everywhere,
    // each followed by the uniform continental thickening trials.
    println!("\n#### (L0 what-if) P3 dynamic term without the inherited compatibility elevation");
    let inherited = evolved.compatibility().tectonic_elevation_m();
    for (label, kind) in [
        ("oceanic", CrustKind::Oceanic),
        ("continental", CrustKind::Continental),
    ] {
        print_quantiles(
            &format!(
                "v5 compatibility tectonic_elevation_m inherited by P3 over {label} crust (m)"
            ),
            &Weighted::collect(inherited, &areas, |i| kinds[i] == kind),
        );
    }
    for (label, predicate) in [
        ("net uplift", (|rate: f32| rate > 0.0) as fn(f32) -> bool),
        ("net subsidence", |rate: f32| rate < 0.0),
        ("no normal forcing", |rate: f32| rate == 0.0),
    ] {
        let samples = Weighted::collect(relief.dynamic_tectonic_offset_m(), &areas, |i| {
            kinds[i] == CrustKind::Oceanic && predicate(net_rate[i])
        });
        println!(
            "p3 dynamic_tectonic over oceanic crust with {label}: area share={:.4} mean={:.1} m p50={:.1} m",
            samples.total() / total_area,
            samples.mean(),
            samples.quantile(0.5),
        );
    }
    let rate_only: Vec<f32> = (0..n)
        .map(|i| {
            dynamic_tectonic_response_m(
                0.0,
                forcing.uplift_rate_mm_per_year()[i],
                forcing.subsidence_rate_mm_per_year()[i],
            )
        })
        .collect();
    for (scope, applies) in [
        (
            "oceanic-kind cells",
            kinds
                .iter()
                .map(|&k| k == CrustKind::Oceanic)
                .collect::<Vec<bool>>(),
        ),
        ("every cell", vec![true; n]),
    ] {
        let stripped: Vec<f32> = (0..n)
            .map(|i| {
                if applies[i] {
                    relief.elevation_m()[i] - relief.dynamic_tectonic_offset_m()[i] + rate_only[i]
                } else {
                    relief.elevation_m()[i]
                }
            })
            .collect();
        for thickening_km in CLOSURE_THICKENING_TRIALS_KM {
            let lift_m = thickening_km * airy_slope_m_per_km;
            let elevation: Vec<f32> = stripped
                .iter()
                .zip(&continental_weight)
                .map(|(&base, &weight)| base + lift_m * weight)
                .collect();
            let water = solve_physical_sea_level(surface, &elevation, inventory).unwrap();
            hypsometry(
                &format!(
                    "L0 on p3/primary over {scope}, uniform continental thickening {thickening_km:.0} km"
                ),
                &areas,
                &elevation,
                water.sea_level_m(),
                water.geometry().land_ocean().raw_values(),
                inventory,
            );
        }
    }

    // L1 what-if: re-reference the continental freeboard datum to the solved sea.
    println!("\n#### (L1 what-if) freeboard closure on the P3 column and on the P5 product");
    for thickening_km in CLOSURE_THICKENING_TRIALS_KM {
        let (sea, _, land) = freeboard_closure(
            &format!("L1 on p3/primary, uniform continental thickening {thickening_km:.0} km"),
            surface,
            relief.elevation_m(),
            &continental_weight,
            inventory,
            thickening_km * airy_slope_m_per_km,
            relief.sea_level_m(),
        );
        let emergent_continental = (0..n)
            .filter(|&i| land[i] == 1)
            .map(|i| continental_area[i])
            .sum::<f64>()
            / continental_thickness.total();
        println!(
            "  equivalent reference-column change={:+.2} km | emergent share of continental reference area={:.4}",
            sea / airy_slope_m_per_km,
            emergent_continental,
        );
    }
    freeboard_closure(
        "L1 on p5/current (diagnostic only; P5 would re-solve)",
        surface,
        terrain.current_elevation_m(),
        &continental_weight,
        inventory,
        0.0,
        terrain.sea_level_m(),
    );
}

// ---------------------------------------------------------------------------
// T0 corpus hypsometry (calibration spec §8.6 "measure, then pin"): the P3
// land hypsometry and the V5 continental inventory of every quality seed.
// ---------------------------------------------------------------------------

/// Thickness ceilings whose area shares bound the inventory tails (spec §4 L2).
const INVENTORY_THIN_CEILING_KM: f32 = 28.0;
const INVENTORY_THICK_FLOOR_KM: f32 = 44.0;

fn corpus_summary(label: &str, values: &[f64]) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{label}: min={:.3} median={:.3} max={:.3}",
        sorted[0],
        sorted[sorted.len() / 2],
        sorted[sorted.len() - 1],
    );
}

#[test]
#[ignore = "audit probe writer; run explicitly with --ignored --nocapture in release"]
fn probe_t0_corpus_hypsometry() {
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let n = surface.cells().len();
    let areas: Vec<f64> = surface.cells().iter().map(|c| c.area.get()).collect();
    let total_area: f64 = areas.iter().sum();
    println!(
        "== t0 corpus hypsometry (draft, {} seeds, {n} cells, P3 product) ==",
        QUALITY_SEEDS.len()
    );
    println!(
        "seed | sea_m | land | cont_area | p05 p25 p50 p75 p95 | mean | <100m | ocean_p50 | inv_mean inv_sd p95-p05 >=44 <28"
    );
    let mut columns: Vec<Vec<f64>> = vec![Vec::new(); 15];
    for seed in QUALITY_SEEDS {
        let (evolved, substrate, relief) = build_primary_relief(&bundle, seed);
        let sea = relief.sea_level_m();
        let land = relief.land_ocean().raw_values();
        let relief_m: Vec<f32> = relief.elevation_m().iter().map(|&e| e - sea).collect();
        let depth_m: Vec<f32> = relief_m.iter().map(|&r| -r).collect();
        let land_relief = Weighted::collect(&relief_m, &areas, |i| land[i] == 1);
        let ocean_depth = Weighted::collect(&depth_m, &areas, |i| land[i] == 0);
        let material = evolved.material();
        let thickness: Vec<f32> = (0..n)
            .map(|i| material.compatibility_thickness_km(i).unwrap_or(f32::NAN))
            .collect();
        let inventory =
            Weighted::collect(&thickness, material.continental_reference_area_m2(), |i| {
                material.compatibility_kind(i) == Some(CrustKind::Continental)
            });
        let [p05, p25, p50, p75, p95] = land_relief.quantiles();
        let row = [
            f64::from(sea),
            f64::from(relief.physical_land_fraction()),
            inventory.total() / total_area,
            f64::from(p05),
            f64::from(p25),
            f64::from(p50),
            f64::from(p75),
            f64::from(p95),
            land_relief.mean(),
            land_relief.share_below(LOWLAND_CEILINGS_M[0]),
            f64::from(ocean_depth.quantile(0.5)),
            inventory.mean(),
            inventory.std_dev(),
            f64::from(inventory.quantile(0.95) - inventory.quantile(0.05)),
            1.0 - inventory.share_below(INVENTORY_THICK_FLOOR_KM),
        ];
        let thin_share = inventory.share_below(INVENTORY_THIN_CEILING_KM);
        println!(
            "{seed:>4} | {:+7.1} | {:.4} | {:.4} | {:.0} {:.0} {:.0} {:.0} {:.0} | {:.0} | {:.4} | {:.0} | {:.2} {:.2} {:.1} {:.3} {:.3}",
            row[0], row[1], row[2], row[3], row[4], row[5], row[6], row[7], row[8], row[9], row[10], row[11], row[12], row[13], row[14], thin_share,
        );
        for (column, value) in columns.iter_mut().zip(row) {
            column.push(value);
        }
        let _ = substrate;
    }
    for (label, column) in [
        "sea_level_m",
        "land_fraction",
        "continental_area_fraction",
        "land_p05_m",
        "land_p25_m",
        "land_p50_m",
        "land_p75_m",
        "land_p95_m",
        "land_mean_m",
        "land_share_below_100m",
        "ocean_depth_p50_m",
        "inventory_mean_km",
        "inventory_sd_km",
        "inventory_p95_minus_p05_km",
        "inventory_share_ge_44km",
    ]
    .iter()
    .zip(&columns)
    {
        corpus_summary(label, column);
    }
}

const T0B_WATER_RATIOS: [f64; 6] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0];
const T0B_TARGET_LAND: [f64; 4] = [0.29, 0.38, 0.50, 0.60];
const T0B_PRESETS: [ResolvedWorldFormationPreset; 5] = [
    ResolvedWorldFormationPreset::Continents,
    ResolvedWorldFormationPreset::Archipelago,
    ResolvedWorldFormationPreset::Supercontinent,
    ResolvedWorldFormationPreset::GreatIsland,
    ResolvedWorldFormationPreset::VolcanicIslands,
];

fn land_area_at(
    elevation_m: &[f32],
    areas: &[f64],
    sea_level_m: f32,
    include: impl Fn(usize) -> bool,
) -> f64 {
    elevation_m
        .iter()
        .zip(areas)
        .enumerate()
        .filter(|(index, (&elevation, _))| {
            include(*index)
                && LandOceanKind::classify(elevation, sea_level_m) == LandOceanKind::Land
        })
        .map(|(_, (_, &area))| area)
        .sum()
}

/// T0b Task 1: (A) where the V5 continental inventory mean goes between the
/// initial table and the P3-facing terminal state, by the material ledger;
/// (B) land fraction against water inventory per formation preset, the
/// exposure ratio, and the implied water ratio for candidate targets.
#[test]
#[ignore = "audit probe writer; run explicitly with --ignored --nocapture in release"]
fn probe_t0b_land_fraction_driver() {
    let bundle = ProfileSurfaceBuilder::build(
        NaturalQualityProfile::Draft,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap();
    let surface = bundle.authoritative_surface();
    let n = surface.cells().len();
    let areas: Vec<f64> = surface.cells().iter().map(|c| c.area.get()).collect();
    let total_area: f64 = areas.iter().sum();
    let inventory = scaled_earth_ocean_inventory_m3(total_area).unwrap();
    let h_km = inventory / total_area / 1000.0;
    println!(
        "== t0b part A: continental inventory attribution (Continents, {} seeds, {n} cells) ==",
        QUALITY_SEEDS.len()
    );
    println!("earth water over the sphere h = {h_km:.3} km");
    println!(
        "seed | A0 | V0/A0 | rift+ | short- | consumed A V | A1 | V1/A1 | A2 | V2/A2 | dilution | volume | remap (km)"
    );
    let mut a_columns: Vec<Vec<f64>> = vec![Vec::new(); 9];
    for seed in QUALITY_SEEDS {
        let (evolved, _substrate, _relief) = build_primary_relief(&bundle, seed);
        let budget = evolved.material_budget();
        let c0 = budget.initial_control().continental();
        let c1 = budget.final_control().continental();
        let c2 = budget.final_authoritative().continental();
        let processes = budget.processes();
        let (a0, v0) = (c0.reference_area_m2(), c0.volume_m3());
        let (a1, v1) = (c1.reference_area_m2(), c1.volume_m3());
        let (a2, v2) = (c2.reference_area_m2(), c2.volume_m3());
        let mean0 = v0 / a0 / 1000.0;
        let mean1 = v1 / a1 / 1000.0;
        let mean2 = v2 / a2 / 1000.0;
        let dilution = v0 / a1 / 1000.0 - mean0;
        let volume = (v1 - v0) / a1 / 1000.0;
        let remap = mean2 - mean1;
        let rift = processes.rift_extension_continental_area_gain_m2() / a0;
        let shortening = processes.collision_shortening_continental_area_loss_m2() / a0;
        let consumed = processes.continental_consumed();
        let consumed_area = consumed.reference_area_m2() / a0;
        let consumed_volume = consumed.volume_m3() / v0;
        println!(
            "{seed:>4} | {:.4} | {mean0:.2} | {rift:+.4} | {shortening:+.4} | {consumed_area:.4} {consumed_volume:.4} | {:.4} | {mean1:.2} | {:.4} | {mean2:.2} | {dilution:+.2} | {volume:+.2} | {remap:+.2}",
            a0 / total_area,
            a1 / total_area,
            a2 / total_area,
        );
        for (column, value) in a_columns.iter_mut().zip([
            mean0,
            mean1,
            mean2,
            dilution,
            volume,
            remap,
            rift,
            shortening,
            consumed_area,
        ]) {
            column.push(value);
        }
    }
    for (label, column) in [
        "initial_mean_km",
        "final_control_mean_km",
        "final_authoritative_mean_km",
        "area_dilution_term_km",
        "volume_loss_term_km",
        "remap_term_km",
        "rift_area_gain_fraction",
        "shortening_area_loss_fraction",
        "consumed_area_fraction",
    ]
    .iter()
    .zip(&a_columns)
    {
        corpus_summary(label, column);
    }

    println!("== t0b part B: land fraction against water inventory per preset ==");
    for preset in T0B_PRESETS {
        let crust = preset.recommended_continental_crust_fraction();
        let nominal = f64::from(preset.recommended_land_fraction());
        let spec = TectonicSpec {
            continental_crust_fraction: crust,
            ..TectonicSpec::default()
        };
        println!("-- {preset:?}: crust {crust:.2}, nominal land {nominal:.2} --");
        println!(
            "seed | sea | L | cont | expo | D km | L(r=.25 .5 .75 1 1.5 2) | r(T=nom .29 .38 .50 .60) | oceanic land share at T"
        );
        let mut columns: Vec<Vec<f64>> = vec![Vec::new(); 21];
        for seed in QUALITY_SEEDS {
            let (_evolved, substrate, relief) =
                build_primary_relief_for(&bundle, seed, preset, &spec);
            let elevation = relief.elevation_m();
            let sea = relief.sea_level_m();
            let land = relief.land_ocean().raw_values();
            let continental =
                |index: usize| substrate.crust_kind(index) == Some(CrustKind::Continental);
            let continental_area: f64 = (0..n).filter(|&i| continental(i)).map(|i| areas[i]).sum();
            let land_area: f64 = (0..n).filter(|&i| land[i] == 1).map(|i| areas[i]).sum();
            let land_on_continental: f64 = (0..n)
                .filter(|&i| land[i] == 1 && continental(i))
                .map(|i| areas[i])
                .sum();
            let l = land_area / total_area;
            let exposure = land_on_continental / continental_area;
            let depth_km = relief.water_inventory_m3() / (total_area - land_area) / 1000.0;
            let mut row = vec![
                f64::from(sea),
                l,
                continental_area / total_area,
                exposure,
                depth_km,
            ];
            for ratio in T0B_WATER_RATIOS {
                let solved =
                    solve_physical_sea_level(surface, elevation, ratio * inventory).unwrap();
                row.push(f64::from(
                    solved
                        .geometry()
                        .global_land_area_fraction(surface)
                        .unwrap(),
                ));
            }
            let mut samples: Vec<(f32, f64)> = elevation
                .iter()
                .copied()
                .zip(areas.iter().copied())
                .collect();
            sort_hypsometric_samples(&mut samples);
            let mut oceanic_shares = Vec::with_capacity(5);
            for target in std::iter::once(nominal).chain(T0B_TARGET_LAND) {
                let sea_t = hypsometric_quantile(&samples, 1.0 - target);
                let volume = water_volume_at_sea_level_m3(surface, elevation, sea_t).unwrap();
                row.push(volume / inventory);
                let land_t = land_area_at(elevation, &areas, sea_t, |_| true);
                let oceanic_t = land_area_at(elevation, &areas, sea_t, |i| !continental(i));
                oceanic_shares.push(if land_t > 0.0 {
                    oceanic_t / land_t
                } else {
                    0.0
                });
            }
            row.extend(oceanic_shares);
            let fmt = |values: &[f64]| {
                values
                    .iter()
                    .map(|v| format!("{v:.3}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!(
                "{seed:>4} | {:+6.0} | {:.3} | {:.3} | {:.3} | {:.2} | {} | {} | {}",
                row[0],
                row[1],
                row[2],
                row[3],
                row[4],
                fmt(&row[5..11]),
                fmt(&row[11..16]),
                fmt(&row[16..21]),
            );
            for (column, value) in columns.iter_mut().zip(row) {
                column.push(value);
            }
        }
        let labels = [
            "sea_level_m".to_owned(),
            "land_fraction_r1".to_owned(),
            "continental_area_fraction".to_owned(),
            "exposure_ratio".to_owned(),
            "wet_mean_depth_km".to_owned(),
        ]
        .into_iter()
        .chain(
            T0B_WATER_RATIOS
                .iter()
                .map(|r| format!("land_fraction_r{r}")),
        )
        .chain(
            std::iter::once(nominal)
                .chain(T0B_TARGET_LAND)
                .map(|t| format!("water_ratio_for_land_{t:.2}")),
        )
        .chain(
            std::iter::once(nominal)
                .chain(T0B_TARGET_LAND)
                .map(|t| format!("oceanic_land_share_at_{t:.2}")),
        )
        .collect::<Vec<_>>();
        for (label, column) in labels.iter().zip(&columns) {
            corpus_summary(label, column);
        }
    }
}
