//! Evidence probe for spec 2026-10-26 §12.10 (why the exported orographic share is 0).
//!
//! Not part of the test suite. To rerun, copy into `tests/zz_orographic_probe.rs` and run
//! `cargo test --release -j 4 --test zz_orographic_probe -- --ignored --nocapture`.
//! Rebuilds the §12.9 UI world (Standard, seed 1950802277578115247, land 0.47),
//! prints the global distribution of P4 `monthly_orographic_precipitation_mm_day`,
//! the published values around the two acceptance anchors, and a climate-grid
//! reconstruction of P4's LCL-limited Smith source from monthly-mean inputs.

use sekai::app::build_spherical_formation_external_artifacts;
use sekai::engine::{BuildCancellation, BuildEngine, MemoryStageCache};
use sekai::generators::natural::circulation::{CirculationOperators, CubedSphereGrid};
use sekai::generators::natural::{causal_natural_formation_graph, NaturalFormationBundleArtifact};
use sekai::generators::spatial::{
    remap_intensive_f32, ConservativeSurfaceMapBuilder, ProfileSurfaceBuilder,
};
use sekai::world::natural::{
    lcl_adjusted_orographic_condensation_kg_m2_s, raw_orographic_condensation_kg_m2_s,
    saturation_specific_humidity_kg_kg, GeologicSpec, NaturalQualityProfile, ReliefSpec,
    SurfaceWaterKind, TectonicSpec, WorldFormationSpec,
};
use sekai::world::spatial::{
    canonical_east_north_basis, central_angle, latitude_longitude_radians,
};
use sekai::world::{Meters, RootSeed};

fn lcl_height_m(q: f64, t: f64) -> f64 {
    // Invert the production LCL factor: with horizontal speed 1 and area 1 the
    // uplift equals the upslope velocity U, so factor = 1 - LCL/U.
    let u = 1.0e5;
    let raw = raw_orographic_condensation_kg_m2_s(q.min(saturation_specific_humidity_kg_kg(t)), u);
    let adj = lcl_adjusted_orographic_condensation_kg_m2_s(q, t, u, 1.0, 1.0);
    u * (1.0 - adj / raw)
}

#[test]
#[ignore = "probe"]
fn orographic_probe() {
    let profile = NaturalQualityProfile::Standard;
    let seed = 1_950_802_277_578_115_247_u64;
    let surface = ProfileSurfaceBuilder::build(
        profile,
        Meters::new(6_371_000.0).unwrap(),
        &BuildCancellation::new(),
    )
    .unwrap()
    .authoritative_surface()
    .clone();
    let root_seed = RootSeed::new(seed);
    let relief = ReliefSpec {
        target_land_fraction: 0.47,
        ..ReliefSpec::default()
    };
    let started = std::time::Instant::now();
    let external = build_spherical_formation_external_artifacts(
        root_seed,
        profile,
        &surface,
        &WorldFormationSpec::default(),
        &TectonicSpec::default(),
        &relief,
        &GeologicSpec::default(),
    )
    .unwrap();
    let result = BuildEngine::new(causal_natural_formation_graph().unwrap())
        .build(root_seed, external, &mut MemoryStageCache::new())
        .unwrap();
    println!(
        "built in {:.0} s, cells {}, build hash {}",
        started.elapsed().as_secs_f64(),
        surface.cells().len(),
        blake3::Hash::from(
            *result
                .verified_provenance()
                .unwrap()
                .result_hash()
                .as_bytes()
        )
        .to_hex()
    );
    let artifact = result
        .artifacts
        .get::<NaturalFormationBundleArtifact>()
        .unwrap();
    let bundle = artifact.bundle();
    let formation = bundle.surface_formation();
    let water = formation.hydrology().surface_water();
    let elevation = formation.terrain_fields().current_elevation_m();
    let fields = bundle.climate().fields();
    let total = fields.monthly_precipitation_mm_day().values();
    let oro = fields.monthly_orographic_precipitation_mm_day().values();
    let q = fields.monthly_specific_humidity().values();
    let temp = fields.monthly_air_temperature_c().values();
    let upper = fields.upper_wind_m_s().unwrap().values();
    let near = fields.near_surface_wind_m_s().values();

    // Global distribution.
    let n = surface.cells().len();
    let (mut pairs, mut pairs_pos, mut land_pairs, mut land_pairs_pos) = (0, 0, 0, 0);
    let (mut cells_any, mut land_cells, mut land_cells_any) = (0, 0, 0);
    let (mut p_area, mut o_area, mut lp_area, mut lo_area) = (0.0, 0.0, 0.0, 0.0);
    let mut max_oro = 0.0_f32;
    let mut lcl_pos = Vec::new();
    let mut lcl_zero = Vec::new();
    let mut band = [[0_u32; 2]; 18];
    for i in 0..n {
        let cell = &surface.cells()[i];
        let area = cell.area.get();
        let land = water.get(i) == Some(SurfaceWaterKind::DryLand);
        let mut any = false;
        let (lat, _) = latitude_longitude_radians(cell.centroid);
        let b = (((lat.to_degrees() + 90.0) / 10.0) as usize).min(17);
        for m in 0..12 {
            pairs += 1;
            p_area += area * f64::from(total[i][m]);
            o_area += area * f64::from(oro[i][m]);
            if oro[i][m] > 0.0 {
                pairs_pos += 1;
                any = true;
                max_oro = max_oro.max(oro[i][m]);
            }
            if land {
                land_pairs += 1;
                lp_area += area * f64::from(total[i][m]);
                lo_area += area * f64::from(oro[i][m]);
                let lcl = lcl_height_m(f64::from(q[i][m]), f64::from(temp[i][m]));
                if oro[i][m] > 0.0 {
                    land_pairs_pos += 1;
                    lcl_pos.push(lcl);
                } else {
                    lcl_zero.push(lcl);
                }
            }
        }
        cells_any += usize::from(any);
        if land {
            land_cells += 1;
            land_cells_any += usize::from(any);
            band[b][0] += 1;
            band[b][1] += u32::from(any);
        }
    }
    println!(
        "all (cell,month) {pairs}: oro>0 {pairs_pos} ({:.2}%); cells with any oro>0 {cells_any}/{n}",
        100.0 * pairs_pos as f64 / pairs as f64
    );
    println!(
        "dry-land (cell,month) {land_pairs}: oro>0 {land_pairs_pos} ({:.2}%); dry-land cells any {land_cells_any}/{land_cells}",
        100.0 * land_pairs_pos as f64 / land_pairs as f64
    );
    println!(
        "area-weighted oro/total: global {:.4}, dry land {:.4}; max oro {max_oro:.3} mm/d",
        o_area / p_area,
        lo_area / lp_area
    );
    for (b, [count, any]) in band.iter().enumerate() {
        if *count > 0 {
            println!(
                "  lat {:+}..{:+}: dry-land cells {count}, any oro {any} ({:.1}%)",
                b as i32 * 10 - 90,
                b as i32 * 10 - 80,
                100.0 * f64::from(*any) / f64::from(*count)
            );
        }
    }
    let q_of = |v: &mut Vec<f64>, p: f64| {
        v.sort_by(f64::total_cmp);
        if v.is_empty() {
            f64::NAN
        } else {
            v[((v.len() - 1) as f64 * p) as usize]
        }
    };
    println!(
        "dry-land LCL (m) where oro>0: p10 {:.0} p50 {:.0} p90 {:.0}; where oro=0: p10 {:.0} p50 {:.0} p90 {:.0}",
        q_of(&mut lcl_pos, 0.1), q_of(&mut lcl_pos, 0.5), q_of(&mut lcl_pos, 0.9),
        q_of(&mut lcl_zero, 0.1), q_of(&mut lcl_zero, 0.5), q_of(&mut lcl_zero, 0.9)
    );

    // The two acceptance anchors: cells within half the P4 spacing (288 km).
    let radius = 6_371_000.0;
    for anchor in [6584_usize, 24157] {
        let centre = surface.cells()[anchor].centroid;
        let (lat, lon) = latitude_longitude_radians(centre);
        let (east, north) = canonical_east_north_basis(centre);
        println!(
            "anchor {anchor} at {:+.2}, {:+.2}",
            lat.to_degrees(),
            lon.to_degrees()
        );
        let mut near_cells = Vec::new();
        for i in 0..n {
            let d = central_angle(centre, surface.cells()[i].centroid) * radius;
            if d <= 300_000.0 {
                near_cells.push((i, d));
            }
        }
        let max_any = near_cells
            .iter()
            .map(|(i, _)| oro[*i].iter().copied().fold(0.0_f32, f32::max))
            .fold(0.0_f32, f32::max);
        let elev: Vec<f32> = near_cells.iter().map(|(i, _)| elevation[*i]).collect();
        println!(
            "  cells within 300 km: {}, max oro any month {max_any}, elev min {:.0} max {:.0}",
            near_cells.len(),
            elev.iter().copied().fold(f32::INFINITY, f32::min),
            elev.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        );
        // Coarse-grid uplift estimate: L = P4 spacing; h_s = mean authoritative
        // elevation within L/2; uplift = h_s(p + u L/2) - h_s(p - u L/2).
        let spacing = 288_129.0_f64;
        let pc = centre.components();
        let local: Vec<([f64; 3], f64)> = (0..n)
            .filter(|j| {
                central_angle(centre, surface.cells()[*j].centroid) * radius <= 2.0 * spacing
            })
            .map(|j| {
                (
                    surface.cells()[j].centroid.components(),
                    f64::from(elevation[j]),
                )
            })
            .collect();
        let smooth = |p: [f64; 3]| {
            let (mut sum, mut count) = (0.0, 0.0);
            for (c, h) in &local {
                let dot = (p[0] * c[0] + p[1] * c[1] + p[2] * c[2]).clamp(-1.0, 1.0);
                if dot.acos() * radius <= spacing / 2.0 {
                    sum += h.max(0.0);
                    count += 1.0;
                }
            }
            sum / count
        };
        let theta = spacing / 2.0 / radius;
        let shift = |dir_en: [f64; 2], sign: f64| {
            let norm = dir_en[0].hypot(dir_en[1]);
            let d: [f64; 3] =
                std::array::from_fn(|a| (dir_en[0] * east[a] + dir_en[1] * north[a]) / norm);
            let p: [f64; 3] =
                std::array::from_fn(|a| pc[a] * theta.cos() + sign * d[a] * theta.sin());
            p
        };
        let uplift = |dir_en: [f64; 2]| smooth(shift(dir_en, 1.0)) - smooth(shift(dir_en, -1.0));
        let best = (0..36)
            .map(|k| {
                let a = f64::from(k) * 10.0_f64.to_radians();
                (uplift([a.sin(), a.cos()]), k * 10)
            })
            .fold(
                (f64::NEG_INFINITY, 0),
                |acc, x| if x.0 > acc.0 { x } else { acc },
            );
        println!(
            "  smoothed h at anchor {:.0} m; max uplift over 36 bearings {:.0} m (toward bearing {} deg)",
            smooth(pc),
            best.0,
            best.1
        );
        let i = anchor;
        for m in 0..12 {
            let rh = f64::from(q[i][m]) / saturation_specific_humidity_kg_kg(f64::from(temp[i][m]));
            let project = |v: [f32; 3]| {
                let v = v.map(f64::from);
                [
                    v[0] * east[0] + v[1] * east[1] + v[2] * east[2],
                    v[0] * north[0] + v[1] * north[1] + v[2] * north[2],
                ]
            };
            let up = project(upper[i][m]);
            let lo = project(near[i][m]);
            let lift = uplift(up);
            println!("  m{:02} uplift along upper wind {:+5.0} m", m + 1, lift);
            println!(
                "  m{:02} T {:6.1} q {:.5} RH {:.2} LCL {:5.0} m  P {:.3} oro {:.4}  upper EN [{:+.1},{:+.1}] near EN [{:+.1},{:+.1}]",
                m + 1,
                temp[i][m],
                q[i][m],
                rh,
                lcl_height_m(f64::from(q[i][m]), f64::from(temp[i][m])),
                total[i][m],
                oro[i][m],
                up[0], up[1], lo[0], lo[1]
            );
        }
    }

    // Climate-grid reconstruction of the production orographic source from
    // monthly-mean published inputs (spec probe, not the solver's per-step state).
    let grid = CubedSphereGrid::new(profile.climate_face_resolution(), radius).unwrap();
    let gs = grid.to_surface_snapshot().unwrap();
    let map = ConservativeSurfaceMapBuilder::build(&surface, &gs).unwrap();
    let sea = formation.terrain_fields().sea_level_m();
    let rel: Vec<f32> = remap_intensive_f32(&map, elevation)
        .unwrap()
        .into_iter()
        .map(|h| h - sea)
        .collect();
    let grad = CirculationOperators::new(&grid).gradient(&rel).unwrap();
    let land_src: Vec<f32> = (0..n)
        .map(|i| f32::from(u8::from(water.get(i) != Some(SurfaceWaterKind::Ocean))))
        .collect();
    let land_g = remap_intensive_f32(&map, &land_src).unwrap();
    let gn = gs.cells().len();
    let mut confusion = [[0_u32; 2]; 2]; // [predicted>0][published>0]
    let mut near_rows = Vec::new();
    let anchors = [
        surface.cells()[6584].centroid,
        surface.cells()[24157].centroid,
    ];
    for m in 0..12 {
        let pick = |src: &[[f32; 12]]| {
            let v: Vec<f32> = src.iter().map(|x| x[m]).collect();
            remap_intensive_f32(&map, &v).unwrap()
        };
        let qg = pick(q);
        let tg = pick(temp);
        let og = pick(oro);
        let ug: Vec<Vec<f32>> = (0..3)
            .map(|a| {
                let v: Vec<f32> = upper.iter().map(|x| x[m][a]).collect();
                remap_intensive_f32(&map, &v).unwrap()
            })
            .collect();
        for k in 0..gn {
            let u = [ug[0][k], ug[1][k], ug[2][k]].map(f64::from);
            let speed = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
            let w = u[0] * f64::from(grad[k][0])
                + u[1] * f64::from(grad[k][1])
                + u[2] * f64::from(grad[k][2]);
            let area = gs.cells()[k].area.get();
            let rate = lcl_adjusted_orographic_condensation_kg_m2_s(
                f64::from(qg[k]),
                f64::from(tg[k]),
                w,
                speed,
                area,
            ) * f64::from(land_g[k])
                * 86_400.0;
            if land_g[k] > 0.5 {
                confusion[usize::from(rate > 0.0)][usize::from(og[k] > 0.0)] += 1;
            }
            for (ai, a) in anchors.iter().enumerate() {
                if central_angle(*a, gs.cells()[k].centroid) * radius <= 300_000.0 {
                    let lcl = lcl_height_m(f64::from(qg[k]), f64::from(tg[k]));
                    let uplift = if speed > 0.0 {
                        w / speed * area.sqrt()
                    } else {
                        0.0
                    };
                    near_rows.push(format!(
                        "  anchor#{ai} grid {k} m{:02}: h {:5.0} |grad| {:.5} U {:4.1} uplift {:+6.0} m LCL {:5.0} m -> predicted {:.4} published {:.4} mm/d",
                        m + 1, rel[k],
                        (grad[k][0].powi(2) + grad[k][1].powi(2) + grad[k][2].powi(2)).sqrt(),
                        speed, uplift, lcl, rate, og[k]
                    ));
                }
            }
        }
    }
    println!(
        "climate-grid land cell-months (land>0.5): pred>0&pub>0 {}, pred>0&pub=0 {}, pred=0&pub>0 {}, pred=0&pub=0 {}",
        confusion[1][1], confusion[1][0], confusion[0][1], confusion[0][0]
    );
    for row in near_rows {
        println!("{row}");
    }
}
