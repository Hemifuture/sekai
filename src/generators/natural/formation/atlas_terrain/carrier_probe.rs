//! Offline carrier comparison: Atlas source Voronoi fans versus Goldberg fans.
//!
//! Measures fan-triangle shape, seed-dependent geometry rebuild time, and how
//! closely each carrier's primitive mosaic reproduces the sealed Atlas surface.
//! Ignored, Release only; prints numbers and sets no gates.

use super::*;
use crate::engine::BuildResultHash;
use crate::generators::natural::{interior_margin, subdivision_children};
use crate::generators::spatial::GeodesicVoronoiBuilder;
use crate::view::{SphericalEntityLocator, SphericalPresentationSource};
use crate::world::natural::SurfaceWaterKind;
use crate::world::spatial::SurfaceRef;
use crate::world::{RootSeed, SphericalSpaceSpec};
use rayon::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

fn c(v: UnitVector3) -> [f64; 3] {
    v.components()
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn len(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
/// Spherical excess (steradians) of a unit-sphere triangle.
pub(super) fn sph_area(t: &[UnitVector3; 3]) -> f64 {
    let [a, b, cc] = t.map(c);
    let num = dot(a, cross(b, cc)).abs();
    let den = 1.0 + dot(a, b) + dot(b, cc) + dot(cc, a);
    2.0 * num.atan2(den)
}
/// Chord-triangle interior angles in degrees and longest/shortest edge ratio.
fn shape(t: &[UnitVector3; 3]) -> (f64, f64) {
    let p = t.map(c);
    let mut min_angle = f64::MAX;
    let mut edges = [0.0; 3];
    for i in 0..3 {
        let u = sub(p[(i + 1) % 3], p[i]);
        let v = sub(p[(i + 2) % 3], p[i]);
        let cos = (dot(u, v) / (len(u) * len(v))).clamp(-1.0, 1.0);
        min_angle = min_angle.min(cos.acos().to_degrees());
        edges[i] = len(u);
    }
    let max = edges.iter().cloned().fold(0.0, f64::max);
    let min = edges.iter().cloned().fold(f64::MAX, f64::min);
    (min_angle, max / min)
}
pub(super) fn center(t: &[UnitVector3; 3]) -> UnitVector3 {
    let [a, b, cc] = t.map(c);
    UnitVector3::new(
        a[0] + b[0] + cc[0],
        a[1] + b[1] + cc[1],
        a[2] + b[2] + cc[2],
    )
    .unwrap()
}
fn pct(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}
fn cv(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt() / mean
}

pub(super) struct Carrier<'a> {
    name: &'static str,
    surface: &'a SphericalSurfaceSnapshot,
    locator: SphericalEntityLocator,
    spacing: Option<f64>,
}
impl<'a> Carrier<'a> {
    pub(super) fn new(name: &'static str, surface: &'a SphericalSurfaceSnapshot) -> Self {
        let source = SphericalPresentationSource::new(
            RootSeed::new(42),
            SurfaceRef::for_spherical(surface),
            BuildResultHash::new([42; 32]),
            1,
        );
        let locator = SphericalEntityLocator::new(source, surface).unwrap();
        Self {
            name,
            surface,
            locator,
            spacing: None,
        }
    }
    pub(super) fn sectors(&self, cell: usize) -> usize {
        self.surface.cells()[cell].boundary_vertices.len()
    }
    pub(super) fn corners(&self, cell: usize, sector: usize) -> [UnitVector3; 3] {
        let cl = &self.surface.cells()[cell];
        let v = self.surface.vertices();
        let n = cl.boundary_vertices.len();
        [
            cl.centroid,
            v[cl.boundary_vertices[sector].raw() as usize].position,
            v[cl.boundary_vertices[(sector + 1) % n].raw() as usize].position,
        ]
    }
    /// Same locate rule as the installed Atlas display evaluator.
    pub(super) fn locate(&self, d: UnitVector3, level: u8) -> (u64, Option<[UnitVector3; 3]>) {
        let cell = self.locator.locate_cell(d).unwrap().raw() as usize;
        if level == 0 {
            return (cell as u64, None);
        }
        let sector = (0..self.sectors(cell))
            .max_by(|a, b| {
                interior_margin(&self.corners(cell, *a), d)
                    .total_cmp(&interior_margin(&self.corners(cell, *b), d))
            })
            .unwrap();
        let mut t = self.corners(cell, sector);
        let mut path = 0_u64;
        for _ in 1..level {
            let ch = subdivision_children(t);
            let k = (0..4)
                .max_by(|a, b| interior_margin(&ch[*a], d).total_cmp(&interior_margin(&ch[*b], d)))
                .unwrap();
            path = path * 4 + k as u64;
            t = ch[k];
        }
        // cell < 2^20, sector < 2^8, path < 4^16 at the probed levels.
        (
            ((cell as u64) << 44) | ((sector as u64) << 36) | path,
            Some(t),
        )
    }
    /// Area-weighted sub-points of a primitive: cell → each sector split once;
    /// triangle → two further subdivision levels.
    fn sub_points(&self, cell: usize, tri: Option<[UnitVector3; 3]>) -> Vec<(UnitVector3, f64)> {
        let mut out = Vec::new();
        let push = |t: [UnitVector3; 3], depth: u8, out: &mut Vec<(UnitVector3, f64)>| {
            fn rec(t: [UnitVector3; 3], depth: u8, out: &mut Vec<(UnitVector3, f64)>) {
                if depth == 0 {
                    out.push((center(&t), sph_area(&t)));
                } else {
                    for ch in subdivision_children(t) {
                        rec(ch, depth - 1, out);
                    }
                }
            }
            rec(t, depth, out);
        };
        match tri {
            None => {
                for s in 0..self.sectors(cell) {
                    push(self.corners(cell, s), 1, &mut out);
                }
            }
            Some(t) => push(t, 2, &mut out),
        }
        out
    }
    fn primitive_center(&self, cell: usize, tri: Option<[UnitVector3; 3]>) -> UnitVector3 {
        match tri {
            None => self.surface.cells()[cell].centroid,
            Some(t) => center(&t),
        }
    }
}

fn shape_report(carrier: &Carrier<'_>) {
    let s = carrier.surface;
    let cell_areas: Vec<f64> = s.cells().iter().map(|cl| cl.area.get()).collect();
    let mut hist: HashMap<usize, usize> = HashMap::new();
    for cl in s.cells() {
        *hist.entry(cl.boundary_vertices.len()).or_default() += 1;
    }
    let mut hist: Vec<_> = hist.into_iter().collect();
    hist.sort();
    eprintln!(
        "CARRIER {} cells={} cell_area_cv={:.4} mean_cell_spacing_km={:.2} sectors_hist={:?}",
        carrier.name,
        s.cells().len(),
        cv(&cell_areas),
        (s.total_cell_area().get() / s.cells().len() as f64).sqrt() / 1000.0,
        hist
    );
    let mut l1 = Vec::new();
    for cell in 0..s.cells().len() {
        for sector in 0..carrier.sectors(cell) {
            l1.push(carrier.corners(cell, sector));
        }
    }
    // Proxy for the canonical water support: red faces needed to bring every
    // L1 fan triangle's longest arc to the canonical spacing (no green closure).
    if let Some(spacing) = carrier.spacing {
        let faces: f64 = l1
            .iter()
            .map(|t| {
                let longest = (0..3)
                    .map(|i| crate::world::spatial::central_angle(t[i], t[(i + 1) % 3]))
                    .fold(0.0, f64::max);
                let depth = (longest / spacing).log2().ceil().max(0.0);
                4_f64.powf(depth)
            })
            .sum();
        eprintln!(
            "CARRIER {} full_red_faces_at_canonical_spacing={:.0} per_cell={:.1}",
            carrier.name,
            faces,
            faces / s.cells().len() as f64
        );
    }
    let mut level = l1;
    for depth in [1_u8, 4] {
        if depth == 4 {
            for _ in 0..3 {
                level = level
                    .iter()
                    .flat_map(|t| subdivision_children(*t))
                    .collect();
            }
        }
        let (mut angles, mut ratios): (Vec<f64>, Vec<f64>) = level.par_iter().map(shape).unzip();
        let areas: Vec<f64> = level.par_iter().map(sph_area).collect();
        angles.sort_by(f64::total_cmp);
        ratios.sort_by(f64::total_cmp);
        let below = |x: f64| angles.partition_point(|a| *a < x) as f64 / angles.len() as f64;
        eprintln!(
            "CARRIER {} L{} tris={} min_angle_deg p0.1={:.2} p1={:.2} p5={:.2} p50={:.2} frac<15={:.4} frac<25={:.4} edge_ratio p50={:.3} p99={:.3} max={:.3} area_cv={:.4}",
            carrier.name, depth, level.len(),
            pct(&angles, 0.001), pct(&angles, 0.01), pct(&angles, 0.05), pct(&angles, 0.5),
            below(15.0), below(25.0),
            pct(&ratios, 0.5), pct(&ratios, 0.99), ratios[ratios.len() - 1], cv(&areas)
        );
    }
}

#[test]
#[ignore = "offline carrier geometry/rebuild-cost probe; Release only"]
fn carrier_geometry_probe() {
    let cancel = BuildCancellation::new();
    let params = TerrainParams {
        seed: 42,
        ..Default::default()
    };
    let t = Instant::now();
    let terrain = Arc::new(generate_terrain(&params, &|| false).unwrap());
    let terrain_ms = t.elapsed().as_millis();
    let directions: Vec<_> = terrain
        .site_xyz()
        .chunks_exact(3)
        .map(|p| UnitVector3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])).unwrap())
        .collect();
    let t = Instant::now();
    let atlas = build_source_voronoi_surface(
        Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
        &directions,
        terrain.site_triangles(),
        &mut || false,
    )
    .unwrap();
    let voronoi_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let fine = FineTerrain::new(terrain, ATLAS_FINE_SCALE, &|| false).unwrap();
    let fine_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let mut atlas_carrier = Carrier::new("atlas", &atlas);
    let atlas_locator_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let _domain = super::super::ClimateWorkDomainBuilder::build(
        &atlas,
        NaturalQualityProfile::Draft,
        &cancel,
    )
    .unwrap();
    let atlas_domain_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let goldberg = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
        target_cell_count: atlas.cells().len() as u32,
    })
    .unwrap();
    let goldberg_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let mut goldberg_carrier = Carrier::new("goldberg", &goldberg);
    let goldberg_locator_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let _domain = super::super::ClimateWorkDomainBuilder::build(
        &goldberg,
        NaturalQualityProfile::Draft,
        &cancel,
    )
    .unwrap();
    let goldberg_domain_ms = t.elapsed().as_millis();
    eprintln!(
        "REBUILD atlas generate_terrain_ms={terrain_ms} voronoi_ms={voronoi_ms} fine_new_ms={fine_ms} locator_ms={atlas_locator_ms} climate_domain_ms={atlas_domain_ms}"
    );
    eprintln!(
        "REBUILD goldberg build_ms={goldberg_ms} locator_ms={goldberg_locator_ms} climate_domain_ms={goldberg_domain_ms}"
    );
    atlas_carrier.spacing = Some(fine.canonical_angular_step_radians());
    goldberg_carrier.spacing = atlas_carrier.spacing;
    shape_report(&atlas_carrier);
    shape_report(&goldberg_carrier);
}

#[derive(Default, Clone, Copy)]
struct Tally {
    n: usize,
    wet_mismatch: usize,
    class_mismatch: usize,
    abs_err: f64,
    sq_err: f64,
    mixed: usize,
}

#[derive(Clone, Copy)]
struct PrimitiveValue {
    center_bed: f64,
    center_wet: bool,
    center_class: SurfaceWaterKind,
    area_bed: f64,
    area_wet: bool,
    area_class: SurfaceWaterKind,
    mixed: bool,
}

fn evaluate(
    snapshot: &AtlasTerrainSnapshot,
    carrier: &Carrier<'_>,
    key_cell: usize,
    tri: Option<[UnitVector3; 3]>,
) -> PrimitiveValue {
    let mut dirs = vec![carrier.primitive_center(key_cell, tri).components()];
    let subs = carrier.sub_points(key_cell, tri);
    dirs.extend(subs.iter().map(|(d, _)| d.components()));
    let samples = snapshot.sample_surface_batch(&dirs, &|| false).unwrap();
    let total: f64 = subs.iter().map(|(_, w)| w).sum();
    let mut bed = 0.0;
    let mut class_w = [0.0_f64; 3];
    for (s, (_, w)) in samples[1..].iter().zip(&subs) {
        bed += s.bed_elevation_m * w;
        let idx = match s.water {
            SurfaceWaterKind::DryLand => 0,
            SurfaceWaterKind::Lake => 1,
            SurfaceWaterKind::Ocean => 2,
        };
        class_w[idx] += w;
    }
    let best = (0..3)
        .max_by(|a, b| class_w[*a].total_cmp(&class_w[*b]))
        .unwrap();
    let area_class = [
        SurfaceWaterKind::DryLand,
        SurfaceWaterKind::Lake,
        SurfaceWaterKind::Ocean,
    ][best];
    let wet_w = class_w[1] + class_w[2];
    PrimitiveValue {
        center_bed: samples[0].bed_elevation_m,
        center_wet: samples[0].water != SurfaceWaterKind::DryLand,
        center_class: samples[0].water,
        area_bed: bed / total,
        area_wet: wet_w > class_w[0],
        area_class,
        mixed: wet_w > 0.0 && class_w[0] > 0.0,
    }
}

#[test]
#[ignore = "one canonical Atlas publication plus carrier fidelity sampling; Release only"]
fn carrier_fidelity_probe() {
    let _ = env_logger::Builder::new()
        .filter_module(
            "sekai::generators::natural::formation::atlas_terrain",
            log::LevelFilter::Debug,
        )
        .is_test(true)
        .try_init();
    let probes: usize = std::env::var("SEKAI_CARRIER_PROBES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(65_536);
    let max_level: u8 = std::env::var("SEKAI_CARRIER_MAX_LEVEL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let threads: usize = std::env::var("SEKAI_CARRIER_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap();

    let cancellation = BuildCancellation::new();
    let request = AtlasTerrainRequest {
        author: TerrainParams {
            seed: 42,
            ..Default::default()
        },
        ..Default::default()
    };
    let t = Instant::now();
    let artifact = AtlasTerrainGenerator::generate(&request, &cancellation).unwrap();
    eprintln!("FIDELITY generate_ms={}", t.elapsed().as_millis());
    let snapshot = artifact.snapshot();
    let radius = snapshot.surface().radius().get();
    eprintln!(
        "FIDELITY canonical_step_m={:.1}",
        snapshot.fine_terrain().canonical_angular_step_radians() * radius
    );
    let goldberg = GeodesicVoronoiBuilder::build(&SphericalSpaceSpec {
        radius: Meters::new(EARTH_WATER_REFERENCE_RADIUS_M).unwrap(),
        target_cell_count: snapshot.surface().cells().len() as u32,
    })
    .unwrap();
    let carriers = [
        Carrier::new("atlas", snapshot.surface()),
        Carrier::new("goldberg", &goldberg),
    ];

    // Fibonacci lattice: equal-area probes, so counts are area fractions.
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    let dirs: Vec<UnitVector3> = (0..probes)
        .map(|i| {
            let z = 1.0 - (2.0 * i as f64 + 1.0) / probes as f64;
            let r = (1.0 - z * z).sqrt();
            let a = golden * i as f64;
            UnitVector3::new(r * a.cos(), r * a.sin(), z).unwrap()
        })
        .collect();
    let t = Instant::now();
    let reference = pool.install(|| {
        dirs.par_chunks(1024)
            .flat_map_iter(|chunk| {
                let raw: Vec<_> = chunk.iter().map(|d| d.components()).collect();
                snapshot.sample_surface_batch(&raw, &|| false).unwrap()
            })
            .collect::<Vec<_>>()
    });
    let ref_wet = reference
        .iter()
        .filter(|s| s.water != SurfaceWaterKind::DryLand)
        .count();
    eprintln!(
        "FIDELITY probes={probes} reference_ms={} wet_fraction={:.4}",
        t.elapsed().as_millis(),
        ref_wet as f64 / probes as f64
    );

    for level in 0..=max_level {
        for carrier in &carriers {
            let t = Instant::now();
            let located: Vec<(u64, Option<[UnitVector3; 3]>)> =
                pool.install(|| dirs.par_iter().map(|d| carrier.locate(*d, level)).collect());
            let mut unique: HashMap<u64, (usize, Option<[UnitVector3; 3]>)> = HashMap::new();
            for (i, (key, tri)) in located.iter().enumerate() {
                let cell = carrier.locator.locate_cell(dirs[i]).unwrap().raw() as usize;
                unique.entry(*key).or_insert((cell, *tri));
            }
            let entries: Vec<_> = unique.into_iter().collect();
            let values: HashMap<u64, PrimitiveValue> = pool.install(|| {
                entries
                    .par_iter()
                    .map(|(key, (cell, tri))| (*key, evaluate(snapshot, carrier, *cell, *tri)))
                    .collect()
            });
            let (mut center, mut area) = (Tally::default(), Tally::default());
            for (i, (key, _)) in located.iter().enumerate() {
                let v = values[key];
                let r = reference[i];
                let r_wet = r.water != SurfaceWaterKind::DryLand;
                for (tally, bed, wet, class) in [
                    (&mut center, v.center_bed, v.center_wet, v.center_class),
                    (&mut area, v.area_bed, v.area_wet, v.area_class),
                ] {
                    tally.n += 1;
                    tally.wet_mismatch += usize::from(wet != r_wet);
                    tally.class_mismatch += usize::from(class != r.water);
                    let e = bed - r.bed_elevation_m;
                    tally.abs_err += e.abs();
                    tally.sq_err += e * e;
                    tally.mixed += usize::from(v.mixed);
                }
            }
            let primitive_count: usize = if level == 0 {
                carrier.surface.cells().len()
            } else {
                (0..carrier.surface.cells().len())
                    .map(|cl| carrier.sectors(cl))
                    .sum::<usize>()
                    * 4_usize.pow(u32::from(level) - 1)
            };
            let size_km =
                (carrier.surface.total_cell_area().get() / primitive_count as f64).sqrt() / 1000.0;
            for (mode, tally) in [("center", center), ("area", area)] {
                eprintln!(
                    "FIDELITY {} L{} prims={} size_km={:.3} mode={} wet_mismatch={:.5} class_mismatch={:.5} mae_m={:.1} rmse_m={:.1} coastal_prim_frac={:.4} unique={} ms={}",
                    carrier.name, level, primitive_count, size_km, mode,
                    tally.wet_mismatch as f64 / tally.n as f64,
                    tally.class_mismatch as f64 / tally.n as f64,
                    tally.abs_err / tally.n as f64,
                    (tally.sq_err / tally.n as f64).sqrt(),
                    tally.mixed as f64 / tally.n as f64,
                    entries.len(),
                    t.elapsed().as_millis()
                );
            }
        }
    }
}
